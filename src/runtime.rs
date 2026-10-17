use crate::{
    graph::Selection,
    model::*,
    output::Output,
    platform::{self, EnforcementMode, Protection},
    probes::Readiness,
    process::{LaunchSpec, ManagedChild},
    scheduler::{Action, Budget, QueueReason, Scheduler, Snapshot, State},
};
use mio::{Events, Interest, Poll, Token, unix::SourceFd};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    io::{self, Read},
    os::{fd::AsRawFd, unix::net::UnixStream},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

struct Running {
    child: ManagedChild,
    launching: bool,
    started: Instant,
    readiness: Option<Readiness>,
    outcome: Option<Outcome>,
    stopping: Option<Instant>,
    killed: bool,
    captured_to_end: bool,
    aggregate_oom_seen: bool,
}
struct Signals {
    reader: UnixStream,
    ids: Vec<signal_hook::SigId>,
    interrupt: Arc<AtomicUsize>,
    terminate: Arc<AtomicUsize>,
}
impl Signals {
    fn install(poll: &Poll) -> io::Result<Self> {
        let (reader, writer) = UnixStream::pair()?;
        reader.set_nonblocking(true)?;
        writer.set_nonblocking(true)?;
        let interrupt = Arc::new(AtomicUsize::new(0));
        let terminate = Arc::new(AtomicUsize::new(0));
        let mut ids = Vec::new();
        for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGCHLD] {
            ids.push(signal_hook::low_level::pipe::register(
                signal,
                writer.try_clone()?,
            )?);
        }
        for (signal, count) in [
            (libc::SIGINT, interrupt.clone()),
            (libc::SIGTERM, terminate.clone()),
        ] {
            // SAFETY: the handler performs only a lock-free atomic increment.
            ids.push(unsafe {
                signal_hook::low_level::register(signal, move || {
                    count.fetch_add(1, Ordering::Relaxed);
                })
            }?);
        }
        poll.registry().register(
            &mut SourceFd(&reader.as_raw_fd()),
            Token(0),
            Interest::READABLE,
        )?;
        Ok(Self {
            reader,
            ids,
            interrupt,
            terminate,
        })
    }
    fn drain(&mut self) {
        let mut bytes = [0u8; 64];
        while self.reader.read(&mut bytes).is_ok_and(|n| n > 0) {}
    }
}
impl Drop for Signals {
    fn drop(&mut self) {
        for id in &self.ids {
            signal_hook::low_level::unregister(*id);
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    config: Config,
    selection: Selection,
    mut scheduler: Scheduler,
    budget: Budget,
    budget_details: Value,
    direct: BTreeMap<String, Vec<OsString>>,
    mut output: Output,
) -> Result<i32, String> {
    let mut poll = Poll::new().map_err(|e| e.to_string())?;
    // Signals must be installed before any workload is launched.
    let mut signals = Signals::install(&poll).map_err(|e| e.to_string())?;
    let mode = match config.enforcement {
        Enforcement::Auto => EnforcementMode::Auto,
        Enforcement::Required => EnforcementMode::Required,
        Enforcement::Off => EnforcementMode::Off,
    };
    let limits: Vec<_> = selection
        .tasks
        .iter()
        .map(|id| {
            let t = &config.tasks[id];
            (
                id.clone(),
                t.limits.cpu.map(|v| v.0),
                t.limits.memory.map(|v| v.0),
            )
        })
        .collect();
    let mut protection = match Protection::prepare(mode, budget.cpu, budget.memory, &limits) {
        Ok(p) => p,
        Err(error) => {
            output.emit(
                "error",
                None,
                json!({"reason":"capability_unavailable","message":error,"exit_code":3}),
            );
            output.finish();
            return Ok(3);
        }
    };
    output.emit(
        "run_started",
        None,
        json!({"selected_roots":selection.roots,"budget":budget_details}),
    );
    output.emit(
        "capabilities",
        None,
        json!({"capabilities":protection.capabilities}),
    );
    for warning in platform::discover().warnings {
        output.emit("warning", None, json!({"message":warning}));
    }
    if budget.memory.is_none() {
        output.emit("warning",None,json!({"message":"memory headroom is unknown; automatic concurrency is one job and memory protection requires an explicit bound"}));
    }
    for (id, request) in &scheduler.requests {
        if (request.cpu_inferred && request.cpu < 1000)
            || (request.memory_inferred && request.memory < (1 << 30))
        {
            output.emit("warning",Some(id),json!({"message":"inferred resource request reduced to fit effective capacity","request":request}));
        }
    }
    let started = Instant::now();
    let mut events = Events::with_capacity(128);
    let mut running: BTreeMap<String, Running> = BTreeMap::new();
    let mut signal_code = None;
    let mut signal_count = 0;
    let mut output_error = false;
    let mut internal_error = false;
    let mut poll_failed = false;
    let mut queue_reported = BTreeMap::new();
    let mut next_token = 1usize;
    let mut observation = platform::discover();
    let mut next_observation = Instant::now();
    let mut aggregate_oom = protection.aggregate_oom_kills();
    let mut cached_usage = BTreeMap::new();
    while !scheduler.is_done() || !running.is_empty() {
        crate::probes::reap_resolvers();
        let now = Instant::now();
        signals.drain();
        let count =
            signals.interrupt.load(Ordering::Relaxed) + signals.terminate.load(Ordering::Relaxed);
        let mut actions = Vec::new();
        if count > signal_count {
            let signal = if signals.interrupt.load(Ordering::Relaxed) > 0 {
                libc::SIGINT
            } else {
                libc::SIGTERM
            };
            if signal_code.is_none() {
                signal_code = Some(if signal == libc::SIGINT { 130 } else { 143 });
                actions.extend(scheduler.cancel(Reason::Cancelled));
            }
            if count > 1 {
                for task in running.values_mut() {
                    let _ = task.child.signal(libc::SIGKILL);
                    task.killed = true;
                }
            }
            signal_count = count;
        }
        if output.failed() && !output_error {
            output_error = true;
            actions.extend(scheduler.cancel(Reason::OutputError));
        }
        // Apply cancellation before observing more task events, so a later timeout
        // or readiness observation cannot replace the requested stop reason.
        actions.retain(|action| {
            if let Action::Stop(id, reason) = action {
                if let Some(task) = running.get_mut(id) {
                    stop_for_policy(task, *reason, signal_code);
                }
                false
            } else {
                true
            }
        });
        let needs_admission = scheduler
            .states
            .values()
            .any(|state| matches!(state, State::Pending | State::Queued { .. }));
        let sample_usage =
            now >= next_observation && (needs_admission || protection.watchdog_enabled());
        if sample_usage {
            observation = platform::discover();
            next_observation = now + Duration::from_millis(200);
        }
        let mut usage = cached_usage.clone();
        let mut finished = Vec::new();
        let mut capture_pending = false;
        let aggregate_now = protection.aggregate_oom_kills();
        let new_aggregate_oom = aggregate_now > aggregate_oom;
        aggregate_oom = aggregate_now;
        for (id, task) in &mut running {
            task.aggregate_oom_seen |= new_aggregate_oom;
            let spec = &config.tasks[id];
            if task.launching && task.outcome.is_none() {
                if config.failure == FailurePolicy::FailFast && scheduler.any_failed() {
                    stop_for_policy(task, Reason::FailFast, signal_code);
                } else {
                    match task.child.poll_launch() {
                        Ok(true) => {
                            task.launching = false;
                            task.started = Instant::now();
                            task.readiness = spec.readiness.clone().map(Readiness::new);
                            scheduler.started(id)?;
                            output.emit("task_started", Some(id), json!({"pid":task.child.pid}));
                        }
                        Ok(false) => {}
                        Err(error) => {
                            let mut outcome = Outcome::failed(Reason::SpawnFailure);
                            outcome.detail = Some(error.to_string());
                            begin_stop(task, outcome, libc::SIGKILL);
                        }
                    }
                }
            }
            let mut close_stdout = false;
            let mut close_stderr = false;
            for stream in ["stdout", "stderr"] {
                // Exec closes the control socket before target output is possible.
                // Initialize readiness first so immediate stdio bytes are retained.
                if task.launching && task.outcome.is_none() {
                    break;
                }
                let mut buf = [0u8; 8192];
                let mut read_total = 0;
                loop {
                    let result = if stream == "stdout" {
                        task.child.stdout.as_mut().map(|p| p.read(&mut buf))
                    } else {
                        task.child.stderr.as_mut().map(|p| p.read(&mut buf))
                    };
                    let Some(result) = result else { break };
                    match result {
                        Ok(0) => {
                            if stream == "stdout" {
                                close_stdout = true
                            } else {
                                close_stderr = true
                            }
                            break;
                        }
                        Ok(n) => {
                            if let Some(probe) = &mut task.readiness {
                                probe.capture(stream, &buf[..n]);
                            }
                            output.log(id, stream, &buf[..n]);
                            read_total += n;
                            if read_total >= 65536 {
                                capture_pending = true;
                                break;
                            }
                        }
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                        Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                        Err(e) => {
                            internal_error = true;
                            let mut outcome = Outcome::failed(Reason::InternalError);
                            outcome.detail = Some(format!("capture {stream}: {e}"));
                            begin_stop(task, outcome, libc::SIGTERM);
                            break;
                        }
                    }
                }
            }
            if close_stdout && let Some(pipe) = task.child.stdout.take() {
                let _ = poll.registry().deregister(&mut SourceFd(&pipe.as_raw_fd()));
            }
            if close_stderr && let Some(pipe) = task.child.stderr.take() {
                let _ = poll.registry().deregister(&mut SourceFd(&pipe.as_raw_fd()));
            }
            task.captured_to_end = task.child.stdout.is_none() && task.child.stderr.is_none();
            if sample_usage {
                if let Some(memory) = task.child.usage_memory() {
                    usage.insert(id.clone(), memory);
                } else {
                    usage.remove(id);
                }
            }
            let exit = if task.launching && task.outcome.is_none() {
                Ok(None)
            } else {
                task.child.poll_exit()
            };
            match exit {
                Ok(Some(exit)) => {
                    if task.outcome.is_none() {
                        let mut outcome = if exit.signal == Some(libc::SIGKILL)
                            && task.child.oom_kills() > 0
                        {
                            Outcome::failed(Reason::MemoryLimit)
                        } else if exit.signal == Some(libc::SIGKILL) && task.aggregate_oom_seen {
                            Outcome::failed(Reason::AggregateMemoryLimit)
                        } else if spec.kind == TaskKind::Service {
                            Outcome::failed(Reason::ServiceFailed)
                        } else if let Some(code) = exit.code {
                            Outcome::exit(code, spec)
                        } else {
                            Outcome::failed(Reason::Signal)
                        };
                        outcome.exit_code = exit.code;
                        outcome.signal = exit.signal;
                        begin_stop(task, outcome, libc::SIGTERM);
                    } else if let Some(outcome) = &mut task.outcome {
                        outcome.exit_code = exit.code;
                        outcome.signal = exit.signal;
                    }
                }
                Err(e) => {
                    internal_error = true;
                    let mut outcome = Outcome::failed(Reason::InternalError);
                    outcome.detail = Some(e.to_string());
                    begin_stop(task, outcome, libc::SIGTERM);
                }
                Ok(None) => {}
            }
            if task.outcome.is_none() && !task.launching {
                if spec
                    .timeout
                    .and_then(Timeout::duration)
                    .is_some_and(|limit| now.duration_since(task.started) >= limit)
                {
                    begin_stop(
                        task,
                        Outcome::failed(Reason::ExecutionTimeout),
                        libc::SIGTERM,
                    );
                }
                if protection.watchdog_enabled()
                    && !protection.capabilities.per_task_memory.starts_with("hard")
                    && spec
                        .limits
                        .memory
                        .is_some_and(|limit| usage.get(id).is_some_and(|used| *used > limit.0))
                {
                    begin_stop(task, Outcome::failed(Reason::MemoryWatchdog), libc::SIGTERM);
                }
                if task.outcome.is_none()
                    && let Some(probe) = &mut task.readiness
                    && matches!(scheduler.state(id), Some(State::Running))
                {
                    match probe.tick() {
                        Ok(true) => {
                            scheduler.ready(id)?;
                            output.emit("task_ready", Some(id), json!({}));
                        }
                        Err(error) => {
                            let mut outcome = Outcome::failed(Reason::ReadinessTimeout);
                            outcome.detail = Some(error);
                            begin_stop(task, outcome, libc::SIGTERM);
                        }
                        Ok(false) => {}
                    }
                }
            }
            if let Some(outcome) = &task.outcome {
                scheduler.stopping(id, outcome)?;
            }
            if let Some(stopping) = task.stopping {
                if now.duration_since(stopping) >= spec.grace() && !task.killed {
                    let _ = task.child.signal(libc::SIGKILL);
                    task.killed = true;
                }
                match task.child.cleanup_finished() {
                    Ok(true) => {
                        // Escaped descendants can retain pipe descriptors on fallback platforms.
                        // Bound post-cleanup drainage rather than hanging forever on such a pipe.
                        if task.captured_to_end
                            || now.duration_since(stopping) >= spec.grace() + Duration::from_secs(1)
                        {
                            if !task.captured_to_end {
                                output.emit("warning",Some(id),json!({"message":"capture closed after cleanup deadline; detached descendants may hold pipes"}));
                            }
                            finished.push(id.clone());
                        }
                    }
                    Err(e) => {
                        internal_error = true;
                        output.emit(
                            "error",
                            Some(id),
                            json!({"reason":"cleanup_error","message":e.to_string()}),
                        );
                    }
                    Ok(false) => {}
                }
                if now.duration_since(stopping) >= spec.grace() + Duration::from_secs(2)
                    && !finished.contains(id)
                {
                    internal_error = true;
                    let mut outcome = Outcome::failed(Reason::CleanupIncomplete);
                    outcome.detail = Some(
                        "owned processes remained after forced cleanup; stopping further admission"
                            .into(),
                    );
                    task.outcome = Some(outcome);
                    finished.push(id.clone());
                }
            }
        }
        for id in finished {
            usage.remove(&id);
            let task = running.remove(&id).expect("finished task exists");
            let outcome = task
                .outcome
                .clone()
                .unwrap_or_else(|| Outcome::failed(Reason::InternalError));
            emit_finished(&mut output, &id, &outcome);
            scheduler.completed(&id, outcome)?;
        }
        if internal_error {
            actions.extend(scheduler.cancel(Reason::InternalError));
        }
        if protection.watchdog_enabled() && protection.aggregate_usage().is_none() {
            let total: u64 = usage.values().copied().fold(0, u64::saturating_add);
            if budget.memory.is_some_and(|limit| total > limit)
                && let Some((id, task)) = running
                    .iter_mut()
                    .filter(|(_, t)| t.outcome.is_none())
                    .max_by_key(|(_, t)| t.started)
            {
                begin_stop(
                    task,
                    Outcome::failed(Reason::AggregateMemoryWatchdog),
                    libc::SIGTERM,
                );
                scheduler.stopping(id, task.outcome.as_ref().expect("stop outcome recorded"))?;
            }
        }
        cached_usage = usage.clone();
        actions.extend(scheduler.tick(
            started.elapsed(),
            &Snapshot {
                available_memory: observation.memory_available,
                usage,
            },
        ));
        for action in actions {
            match action {
                Action::Launch(id) => {
                    let late_interrupt = signals.interrupt.load(Ordering::Relaxed) > 0;
                    let late_termination = signals.terminate.load(Ordering::Relaxed) > 0;
                    if signal_code.is_none() && (late_interrupt || late_termination) {
                        signal_code = Some(if late_interrupt { 130 } else { 143 });
                    }
                    let cancellation = if late_interrupt || late_termination {
                        Some(Reason::Cancelled)
                    } else if internal_error {
                        Some(Reason::InternalError)
                    } else if output.failed() {
                        output_error = true;
                        Some(Reason::OutputError)
                    } else if config.failure == FailurePolicy::FailFast && scheduler.any_failed() {
                        Some(Reason::FailFast)
                    } else {
                        None
                    };
                    if let Some(reason) = cancellation {
                        let outcome = Outcome::cancelled(reason);
                        emit_finished(&mut output, &id, &outcome);
                        scheduler.completed(&id, outcome)?;
                        continue;
                    }
                    // A ready service must still be alive at this launch decision.
                    let ready_alive = config.tasks[&id]
                        .depends_on
                        .iter()
                        .filter_map(|d| {
                            if let Dependency::Ready { task } = d {
                                Some(task)
                            } else {
                                None
                            }
                        })
                        .all(|provider| {
                            running.get_mut(provider).is_some_and(|r| {
                                r.outcome.is_none()
                                    && r.child.poll_exit().is_ok_and(|exit| exit.is_none())
                            })
                        });
                    if !ready_alive {
                        let mut outcome = Outcome::failed(Reason::DependencyFailed);
                        outcome.kind = OutcomeKind::Blocked;
                        emit_finished(&mut output, &id, &outcome);
                        scheduler.completed(&id, outcome)?;
                        continue;
                    }
                    output.emit(
                        "task_starting",
                        Some(&id),
                        json!({"request":scheduler.requests[&id]}),
                    );
                    let spec = launch_spec(&id, &config.tasks[&id], direct.get(&id), &scheduler)?;
                    match ManagedChild::start(&spec, protection.task(&id)) {
                        Ok(child) => {
                            let mut capture_error = None;
                            for fd in [
                                child.stdout.as_ref().map(AsRawFd::as_raw_fd),
                                child.stderr.as_ref().map(AsRawFd::as_raw_fd),
                                child.launch_fd(),
                            ]
                            .into_iter()
                            .flatten()
                            {
                                if let Err(error) = poll.registry().register(
                                    &mut SourceFd(&fd),
                                    Token(next_token),
                                    Interest::READABLE,
                                ) {
                                    capture_error = Some(error.to_string());
                                    break;
                                }
                                next_token += 1;
                            }
                            let mut task = Running {
                                child,
                                launching: true,
                                started: Instant::now(),
                                readiness: None,
                                outcome: None,
                                stopping: None,
                                killed: false,
                                captured_to_end: false,
                                aggregate_oom_seen: false,
                            };
                            if let Some(error) = capture_error {
                                internal_error = true;
                                let mut outcome = Outcome::failed(Reason::InternalError);
                                outcome.detail = Some(format!("register capture: {error}"));
                                begin_stop(&mut task, outcome, libc::SIGTERM);
                                scheduler.stopping(
                                    &id,
                                    task.outcome.as_ref().expect("stop outcome recorded"),
                                )?;
                            }
                            running.insert(id, task);
                        }
                        Err(error) => {
                            let incomplete = crate::process::launch_cleanup_incomplete(&error);
                            internal_error |= incomplete;
                            let mut outcome = Outcome::failed(if incomplete {
                                Reason::CleanupIncomplete
                            } else {
                                Reason::SpawnFailure
                            });
                            outcome.detail = Some(error.to_string());
                            emit_finished(&mut output, &id, &outcome);
                            scheduler.completed(&id, outcome)?;
                        }
                    }
                }
                Action::Settled(id, outcome) => emit_finished(&mut output, &id, &outcome),
                Action::Stop(id, reason) => {
                    if let Some(task) = running.get_mut(&id) {
                        stop_for_policy(task, reason, signal_code);
                    }
                }
            }
        }
        for (id, state) in &scheduler.states {
            let reason = match state {
                State::Pending => Some(QueueReason::DependencyWait),
                State::Queued { reason, .. } => Some(*reason),
                _ => None,
            };
            if let Some(reason) = reason
                && queue_reported.get(id) != Some(&reason)
            {
                output.emit("queue", Some(id), json!({"reason":reason}));
                queue_reported.insert(id.clone(), reason);
            }
        }
        if scheduler.is_done() && running.is_empty() {
            break;
        }
        let timeout = if capture_pending {
            Duration::ZERO
        } else if running.values().any(|t| {
            (t.launching && t.outcome.is_none()) || t.readiness.as_ref().is_some_and(|r| !r.ready)
        }) {
            Duration::from_millis(20)
        } else {
            Duration::from_millis(100)
        };
        if poll_failed {
            // A failed poller must not bypass owned cleanup. Pipes remain
            // nonblocking and signals still update their atomic counters.
            std::thread::sleep(timeout.min(Duration::from_millis(20)));
            continue;
        }
        match poll.poll(&mut events, Some(timeout)) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => {
                internal_error = true;
                poll_failed = true;
                output.emit("error", None, json!({"reason":"internal_error", "message":format!("event polling failed: {error}")}));
            }
        }
    }
    crate::probes::reap_resolvers();
    let impossible_policy = scheduler
        .states
        .values()
        .filter_map(State::outcome)
        .any(|outcome| outcome.reason == Reason::ServiceDeadlock);
    let mut code = signal_code.unwrap_or(if internal_error {
        70
    } else if impossible_policy {
        3
    } else if scheduler.any_failed() || output_error {
        1
    } else {
        0
    });
    if let Err(error) = protection.cleanup() {
        output.emit(
            "error",
            None,
            json!({"reason":"cleanup_incomplete","message":error.to_string()}),
        );
        if signal_code.is_none() {
            code = 70;
        }
    }
    output.flush_truncations();
    let outcomes: BTreeMap<_, _> = scheduler
        .states
        .iter()
        .filter_map(|(id, s)| s.outcome().map(|o| (id, o)))
        .collect();
    let mut counts = BTreeMap::<String, usize>::new();
    for outcome in outcomes.values() {
        let key = serde_json::to_value(outcome.kind)
            .unwrap_or(Value::Null)
            .as_str()
            .unwrap_or("unknown")
            .to_owned();
        *counts.entry(key).or_default() += 1;
    }
    output.emit("summary",None,json!({"selected_roots":selection.roots,"outcomes":outcomes,"counts":counts,"exit_code":code}));
    if !output.finish() && signal_code.is_none() && code == 0 {
        code = 1;
    }
    Ok(code)
}
fn stop_for_policy(task: &mut Running, reason: Reason, signal_code: Option<i32>) {
    let outcome = if reason == Reason::ServiceUnused {
        Outcome::stopped()
    } else {
        Outcome::cancelled(reason)
    };
    let signal = if signal_code == Some(130) {
        libc::SIGINT
    } else {
        libc::SIGTERM
    };
    begin_stop(task, outcome, signal);
}
fn begin_stop(task: &mut Running, outcome: Outcome, signal: i32) {
    if task.outcome.is_none() {
        task.outcome = Some(outcome);
        task.stopping = Some(Instant::now());
        let _ = task.child.signal(signal);
    }
}
fn emit_finished(output: &mut Output, id: &str, outcome: &Outcome) {
    output.emit(
        "task_finished",
        Some(id),
        serde_json::to_value(outcome).unwrap_or_else(|_| json!({"reason":"internal_error"})),
    );
}
fn launch_spec(
    id: &str,
    task: &Task,
    direct: Option<&Vec<OsString>>,
    scheduler: &Scheduler,
) -> Result<LaunchSpec, String> {
    let argv = if let Some(argv) = direct {
        argv.clone()
    } else if let Some(command) = &task.command {
        command.iter().map(OsString::from).collect()
    } else {
        vec![
            OsString::from("/bin/sh"),
            OsString::from("-c"),
            OsString::from(task.shell.as_ref().ok_or("task has no command")?),
        ]
    };
    let mut env: BTreeMap<OsString, OsString> = std::env::vars_os().collect();
    for (key, value) in &task.env {
        env.insert(key.into(), value.into());
    }
    for key in &task.unset_env {
        env.remove(&OsString::from(key));
    }
    env.entry("TAKT_TASK_CPU".into()).or_insert_with(|| {
        format!(
            "{}.{:03}",
            scheduler.requests[id].cpu / 1000,
            scheduler.requests[id].cpu % 1000
        )
        .into()
    });
    env.entry("TAKT_TASK_MEMORY_BYTES".into())
        .or_insert_with(|| scheduler.requests[id].memory.to_string().into());
    Ok(LaunchSpec {
        program: argv.first().ok_or("empty argv")?.clone(),
        args: argv[1..].to_vec(),
        env: env.into_iter().collect(),
        cwd: task
            .cwd
            .clone()
            .unwrap_or(std::env::current_dir().map_err(|e| e.to_string())?),
    })
}
