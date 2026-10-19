//! Black-box checks run by Cargo, independently of Takt's own scheduling.
#[path = "support/events.rs"]
mod event_contract;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read};
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

const CAPTURE_LIMIT: usize = 16 * 1024 * 1024;
static NEXT_DIR: AtomicUsize = AtomicUsize::new(0);

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "takt-integration-{}-{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).expect("unique scratch directory");
        Self(path)
    }
    fn config(&self, value: &Value) -> PathBuf {
        let path = self.0.join("takt.json");
        std::fs::write(&path, serde_json::to_vec(value).unwrap()).unwrap();
        path
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct OwnedChild(Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_some() {
            return;
        }
        // SAFETY: This group was created by this harness; its unreaped direct child
        // retains the group identifier, so an unrelated process cannot reuse it.
        #[cfg(unix)]
        unsafe {
            libc::kill(-(self.0.id() as i32), libc::SIGKILL);
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn spawn(command: &mut Command) -> OwnedChild {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    OwnedChild(command.spawn().expect("spawn test process"))
}

fn capture(mut stream: impl Read + Send + 'static) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut result = Vec::new();
        let mut block = [0; 8192];
        loop {
            match stream.read(&mut block) {
                Ok(0) => break,
                Ok(count) => {
                    let keep = count.min(CAPTURE_LIMIT.saturating_sub(result.len()));
                    result.extend_from_slice(&block[..keep]);
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => panic!("capture: {error}"),
            }
        }
        result
    })
}

struct Run {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}
impl Run {
    fn code(&self, expected: i32) {
        assert_eq!(
            self.status.code(),
            Some(expected),
            "stdout: {}\nstderr: {}",
            String::from_utf8_lossy(&self.stdout),
            String::from_utf8_lossy(&self.stderr)
        );
    }
    fn events(&self) -> Vec<Value> {
        let events = self
            .stdout
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| {
                serde_json::from_slice(line).unwrap_or_else(|error| {
                    panic!("invalid event {error}: {}", String::from_utf8_lossy(line))
                })
            })
            .collect::<Vec<Value>>();
        event_contract::assert_events(&events);
        events
    }
    fn logs(&self) -> String {
        self.events()
            .iter()
            .filter(|event| event["event"] == "log" && event["encoding"] == "utf8")
            .filter_map(|event| event["data"].as_str())
            .collect()
    }
    fn has_event(&self, event: &str, task: &str) -> bool {
        self.events()
            .iter()
            .any(|item| item["event"] == event && item["task_id"] == task)
    }
}

fn executable() -> PathBuf {
    std::env::var_os("TAKT_TEST_BINARY")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_takt")))
}

fn command() -> Command {
    let mut command = Command::new(executable());
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for name in [
        "TAKT_FILE",
        "TAKT_JOBS",
        "TAKT_CPU",
        "TAKT_MEMORY",
        "TAKT_ENFORCEMENT",
        "TAKT_OUTPUT",
    ] {
        command.env_remove(name);
    }
    command
}

fn run(command: &mut Command) -> Run {
    let mut child = spawn(command);
    let stdout = capture(child.0.stdout.take().unwrap());
    let stderr = capture(child.0.stderr.take().unwrap());
    let until = Instant::now() + Duration::from_secs(15);
    let status = loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            break status;
        }
        assert!(
            Instant::now() < until,
            "Takt exceeded independent 15s test deadline"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    Run {
        status,
        stdout: stdout.join().unwrap(),
        stderr: stderr.join().unwrap(),
    }
}

fn fixture(arguments: &[&str]) -> Value {
    let mut argv = vec![Value::String(
        takt_test_fixture::executable()
            .to_string_lossy()
            .into_owned(),
    )];
    argv.extend(
        arguments
            .iter()
            .map(|argument| Value::String((*argument).into())),
    );
    json!({"kind":"task","command":argv})
}

fn configuration(tasks: Value) -> Value {
    json!({
        "version":1, "enforcement":"off", "output":"json",
        "scheduling":{"jobs":4,"cpu":1,"memory":"256MiB","reserve_memory":"1MiB","queue_timeout":"2s"},
        "defaults":{"resources":{"cpu":0.01,"memory":"1MiB"},"stop_grace":"100ms"},
        "tasks":tasks
    })
}

fn configured(value: &Value, selections: &[&str]) -> Run {
    let scratch = Scratch::new();
    let path = scratch.config(value);
    run(command()
        .arg("run")
        .arg("--file")
        .arg(path)
        .args(selections))
}

#[test]
fn direct_argv_preserves_spaces_empty_arguments_and_metacharacters() {
    let result = run(command()
        .args(["exec", "--output", "json", "--enforcement", "off", "--"])
        .arg(takt_test_fixture::executable())
        .args(["args", "two words", "", "$(never-run)", "'quote'"]));
    result.code(0);
    let logs = result.logs();
    for value in ["9:two words", "0:", "12:$(never-run)", "7:'quote'"] {
        assert!(logs.contains(value), "missing {value}: {logs}");
    }
}

#[test]
fn child_failure_is_aggregated_and_spawn_failure_is_distinct() {
    let result = configured(
        &configuration(json!({"failure":fixture(&["exit","7"]),"ok":fixture(&["exit","0"])})),
        &["failure", "ok"],
    );
    result.code(1);
    assert!(result.has_event("task_started", "ok"));
    let result = configured(
        &configuration(
            json!({"missing":{"kind":"task","command":["/definitely-missing-takt-fixture"]}}),
        ),
        &["missing"],
    );
    result.code(1);
    assert!(String::from_utf8_lossy(&result.stdout).contains("spawn"));
}

#[test]
fn shared_prerequisite_runs_once_and_before_dependents() {
    let mut a = fixture(&["exit", "0"]);
    a["depends_on"] = json!([{"task":"prepare","when":"exit","codes":[0]}]);
    let result = configured(
        &configuration(json!({"prepare":fixture(&["sleep","30"]),"a":a,"b":a})),
        &["a", "b"],
    );
    result.code(0);
    let events = result.events();
    let started = |name: &str| {
        events
            .iter()
            .position(|event| event["event"] == "task_started" && event["task_id"] == name)
            .unwrap()
    };
    let finished = events
        .iter()
        .position(|event| event["event"] == "task_finished" && event["task_id"] == "prepare")
        .unwrap();
    assert!(finished < started("a") && finished < started("b"));
    assert_eq!(
        events
            .iter()
            .filter(|event| event["event"] == "task_started" && event["task_id"] == "prepare")
            .count(),
        1
    );
}

#[test]
fn conditional_exit_is_separate_from_success_and_skip_is_neutral() {
    let mut detector = fixture(&["exit", "2"]);
    detector["success_codes"] = json!([0, 2]);
    let mut selected = fixture(&["exit", "0"]);
    selected["depends_on"] = json!([{"task":"detect","when":"exit","codes":[2]}]);
    let mut skipped = fixture(&["exit", "99"]);
    skipped["depends_on"] = json!([{"task":"detect","when":"exit","codes":[0]}]);
    let mut config =
        configuration(json!({"detect":detector,"selected":selected,"skipped":skipped}));
    let result = configured(&config, &["selected", "skipped"]);
    result.code(0);
    assert!(result.has_event("task_started", "selected"));
    assert!(!result.has_event("task_started", "skipped"));
    config["tasks"]["detect"]["success_codes"] = json!([0]);
    let result = configured(&config, &["selected"]);
    result.code(1);
    assert!(result.has_event("task_started", "selected"));
}

#[test]
fn jobs_one_never_overlaps_supervised_trees() {
    let mut config = configuration(
        json!({"a":fixture(&["sleep","40"]),"b":fixture(&["sleep","40"]),"c":fixture(&["sleep","40"])}),
    );
    config["scheduling"]["jobs"] = json!(1);
    let result = configured(&config, &["a", "b", "c"]);
    result.code(0);
    let mut active = 0;
    for event in result.events() {
        if event["event"] == "task_started" {
            active += 1;
        }
        if event["event"] == "task_finished" {
            active -= 1;
        }
        assert!((0..=1).contains(&active), "overlapping jobs: {event}");
    }
    assert_eq!(active, 0);
}

#[test]
fn output_drains_both_pipes_and_encodes_invalid_utf8() {
    for mode in ["chunked", "partial", "invalid"] {
        let result = configured(
            &configuration(json!({"output":fixture(&["output","131072",mode])})),
            &["output"],
        );
        result.code(0);
        let events = result.events();
        for stream in ["stdout", "stderr"] {
            assert!(
                events
                    .iter()
                    .any(|event| event["event"] == "log" && event["stream"] == stream)
            );
        }
        if mode == "invalid" {
            assert!(
                events
                    .iter()
                    .any(|event| event["event"] == "log" && event["encoding"] == "base64")
            );
        }
    }
}

#[test]
fn ndjson_has_monotonic_sequences_and_a_final_summary() {
    let result = configured(&configuration(json!({"a":fixture(&["exit","0"])})), &["a"]);
    result.code(0);
    let events = result.events();
    assert!(!events.is_empty());
    let mut previous = None;
    for event in &events {
        assert_eq!(event["schema_version"], 1);
        let sequence = event["sequence"].as_u64().expect("sequence");
        assert!(previous.is_none_or(|before| sequence > before));
        previous = Some(sequence);
    }
    assert_eq!(events.last().unwrap()["event"], "summary");
}

fn service_config(mut probe: Value, service: Value) -> Value {
    if probe["type"] == "stdio" && probe.get("stream").is_none() {
        probe["stream"] = json!("stdout");
    }
    let mut service = service;
    service["kind"] = json!("service");
    service["readiness"] = probe;
    let mut consumer = fixture(&["exit", "0"]);
    consumer["depends_on"] = json!([{"task":"service","when":"ready"}]);
    configuration(json!({"service":service,"consumer":consumer}))
}

#[test]
fn stdio_readiness_handles_immediate_split_and_stderr_bytes() {
    for (args, stream) in [
        (vec!["startup", "1000"], "stdout"),
        (vec!["output", "0", "split-ready"], "stdout"),
        (vec!["startup", "1000", "stderr"], "stderr"),
    ] {
        let config = service_config(
            json!({"type":"stdio","contains":"READY","stream":stream,"timeout":"500ms"}),
            fixture(&args),
        );
        let result = configured(&config, &["consumer"]);
        result.code(0);
        assert!(result.has_event("task_ready", "service"));
        assert!(result.has_event("task_started", "consumer"));
    }
}

struct Server {
    _child: OwnedChild,
    address: std::net::SocketAddr,
}
impl Server {
    fn new(arguments: &[&str]) -> Self {
        let mut command = Command::new(takt_test_fixture::executable());
        command
            .args(arguments)
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        let mut child = spawn(&mut command);
        let mut line = String::new();
        BufReader::new(child.0.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        let address = line
            .trim()
            .strip_prefix("READY ")
            .expect("fixture startup handshake")
            .parse()
            .unwrap();
        Self {
            _child: child,
            address,
        }
    }
}

#[test]
fn tcp_http_and_udp_readiness_use_live_ephemeral_listeners() {
    for protocol in ["tcp", "http", "udp"] {
        let server = Server::new(&[protocol, "5000"]);
        let probe = match protocol {
            "http" => {
                json!({"type":"http","url":format!("http://{}/health",server.address),"body_contains":"READY","timeout":"1s"})
            }
            "udp" => {
                json!({"type":"udp","host":"127.0.0.1","port":server.address.port(),"payload":{"text":"PING"},"expect":{"equals":{"text":"PING"}},"timeout":"1s"})
            }
            _ => {
                json!({"type":"tcp","host":"127.0.0.1","port":server.address.port(),"timeout":"1s"})
            }
        };
        let result = configured(
            &service_config(probe, fixture(&["sleep", "3000"])),
            &["consumer"],
        );
        result.code(0);
        assert!(result.has_event("task_ready", "service"));
    }
}

#[test]
fn udp_requires_matching_payload_and_peer_not_just_send_success() {
    for mode in ["silent", "wrong", "wrong-peer"] {
        let server = Server::new(&["udp", "5000", mode]);
        let probe = json!({"type":"udp","host":"127.0.0.1","port":server.address.port(),"payload":{"text":"PING"},"expect":{"equals":{"text":"PING"}},"timeout":"250ms","attempt_timeout":"50ms","interval":"20ms"});
        let result = configured(
            &service_config(probe, fixture(&["sleep", "1000"])),
            &["consumer"],
        );
        result.code(1);
        assert!(!result.has_event("task_ready", "service"));
        assert!(!result.has_event("task_started", "consumer"));
    }
}

#[test]
fn http_retries_unhealthy_status_and_rejects_wrong_body() {
    let server = Server::new(&["http", "5000", "200", "READY", "1"]);
    let probe = json!({"type":"http","url":format!("http://{}/health",server.address),"body_contains":"READY","timeout":"1s","interval":"20ms"});
    configured(
        &service_config(probe, fixture(&["sleep", "3000"])),
        &["consumer"],
    )
    .code(0);
    let probe = json!({"type":"http","url":format!("http://{}/health",server.address),"body_contains":"WRONG","timeout":"150ms","interval":"20ms"});
    configured(
        &service_config(probe, fixture(&["sleep", "3000"])),
        &["consumer"],
    )
    .code(1);
}

#[test]
fn service_using_only_slot_is_rejected_without_deadlock() {
    let mut config = service_config(
        json!({"type":"stdio","contains":"READY"}),
        fixture(&["startup", "1000"]),
    );
    config["scheduling"]["jobs"] = json!(1);
    configured(&config, &["consumer"]).code(3);
}

#[test]
fn execution_timeout_is_not_a_matching_exit_edge() {
    let mut producer = fixture(&["ignore", "1000"]);
    producer["timeout"] = json!("50ms");
    let mut consumer = fixture(&["exit", "0"]);
    consumer["depends_on"] = json!([{"task":"producer","when":"exit","codes":[0,137,143]}]);
    let result = configured(
        &configuration(json!({"producer":producer,"consumer":consumer})),
        &["consumer"],
    );
    result.code(1);
    assert!(!result.has_event("task_started", "consumer"));
    assert!(String::from_utf8_lossy(&result.stdout).contains("timeout"));
}

#[test]
fn config_relative_cwd_and_environment_removal_are_respected() {
    let scratch = Scratch::new();
    std::fs::create_dir(scratch.0.join("child")).unwrap();
    let mut cwd = fixture(&["cwd"]);
    cwd["cwd"] = json!("child");
    let mut environment = fixture(&["environment", "TAKT_INTEGRATION_SECRET"]);
    environment["unset_env"] = json!(["TAKT_INTEGRATION_SECRET"]);
    let config = configuration(json!({"cwd":cwd,"environment":environment}));
    let result = run(command()
        .env("TAKT_INTEGRATION_SECRET", "private-value-never-log")
        .args(["run", "cwd", "environment", "--file"])
        .arg(scratch.config(&config)));
    result.code(0);
    assert!(
        result
            .logs()
            .contains(scratch.0.join("child").to_str().unwrap())
    );
    assert!(result.logs().contains("UNSET"));
    assert!(!String::from_utf8_lossy(&result.stdout).contains("private-value-never-log"));
}

#[test]
fn discovery_ambiguity_and_missing_explicit_selection_are_errors() {
    let scratch = Scratch::new();
    scratch.config(&configuration(json!({"a":fixture(&["exit","0"])})));
    std::fs::write(scratch.0.join("takt.yaml"), "version: 1\ntasks: {}\n").unwrap();
    run(command().current_dir(&scratch.0).args(["validate"])).code(2);
    run(command()
        .current_dir(&scratch.0)
        .args(["run", "--file", "takt.json"]))
    .code(2);
}

#[test]
fn duplicate_unknown_oversized_and_tagged_configuration_is_rejected() {
    let scratch = Scratch::new();
    for data in [
        "{\"version\":1,\"version\":1,\"tasks\":{}}".to_owned(),
        "{\"version\":1,\"unknown\":true,\"tasks\":{}}".to_owned(),
        "version: 1\ntasks: !arbitrary {}\n".to_owned(),
        " ".repeat(1024 * 1024 + 1),
    ] {
        let path = scratch.0.join("invalid.yaml");
        std::fs::write(&path, data).unwrap();
        run(command().args(["validate", "--file"]).arg(path)).code(2);
    }
}

#[test]
fn exit_137_without_controller_evidence_is_not_oom() {
    let result = configured(
        &configuration(json!({"exit":fixture(&["exit","137"])})),
        &["exit"],
    );
    result.code(1);
    let events = result.events();
    let outcome = events
        .iter()
        .find(|event| event["event"] == "task_finished" && event["task_id"] == "exit")
        .unwrap();
    assert!(!outcome["reason"].as_str().unwrap_or("").contains("oom"));
}

#[test]
fn fixture_bounds_are_enforced_before_large_work() {
    for args in [
        ["allocate", "129"],
        ["regroup-tree", "65"],
        ["regroup-child", "65"],
        ["tree", "9"],
        ["sleep", "10001"],
        ["output", "4194305"],
    ] {
        let status = Command::new(takt_test_fixture::executable())
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(2));
    }
}

#[cfg(target_os = "macos")]
#[test]
fn macos_required_aggregate_memory_refuses_before_launch() {
    let mut config = configuration(json!({"a":fixture(&["exit","0"])}));
    config["enforcement"] = json!("required");
    let result = configured(&config, &["a"]);
    result.code(3);
    assert!(!result.has_event("task_started", "a"));
}

// Used by release smoke tests as well as ordinary Cargo integration tests.
#[test]
fn help_and_doctor_are_available_without_a_configuration() {
    run(command().arg("--help")).code(0);
    let result = run(command().args(["doctor", "--output", "json"]));
    result.code(0);
    assert!(!result.stdout.is_empty());
    for line in result
        .stdout
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        let _: Value = serde_json::from_slice(line).unwrap();
    }
}

#[test]
fn blocked_and_broken_output_do_not_block_process_cleanup() {
    for broken in [false, true] {
        let scratch = Scratch::new();
        let config = configuration(json!({"logs":fixture(&["output","4194304","partial"])}));
        let mut child = spawn(
            command()
                .args(["run", "logs", "--file"])
                .arg(scratch.config(&config)),
        );
        let mut output = child.0.stdout.take();
        if broken {
            drop(output.take());
        }
        let stderr = capture(child.0.stderr.take().unwrap());
        let started = Instant::now();
        let status = loop {
            if let Some(status) = child.0.try_wait().unwrap() {
                break status;
            }
            assert!(
                started.elapsed() < Duration::from_secs(3),
                "slow or broken sink wedged supervision"
            );
            std::thread::sleep(Duration::from_millis(10));
        };
        drop(output);
        let result = Run {
            status,
            stdout: Vec::new(),
            stderr: stderr.join().unwrap(),
        };
        result.code(1);
    }
}

#[test]
fn grouped_output_preserves_streams_and_cleans_owned_spools() {
    let scratch = Scratch::new();
    let mut config = configuration(json!({"logs":fixture(&["output","131072","chunked"])}));
    config["output"] = json!("grouped");
    let result = run(command()
        .env("TMPDIR", &scratch.0)
        .args(["run", "logs", "--file"])
        .arg(scratch.config(&config)));
    result.code(0);
    assert!(String::from_utf8_lossy(&result.stdout).contains("logs (stdout)"));
    assert!(String::from_utf8_lossy(&result.stderr).contains("logs (stderr)"));
    assert_eq!(
        std::fs::read_dir(&scratch.0).unwrap().count(),
        1,
        "owned spool directory leaked"
    );
}

#[test]
fn grouped_directory_failure_falls_back_without_losing_child_output() {
    let scratch = Scratch::new();
    let invalid_tmpdir = scratch.0.join("not-a-directory");
    std::fs::write(&invalid_tmpdir, b"owned fixture").unwrap();
    let mut config = configuration(json!({"logs":fixture(&["output","131072","chunked"])}));
    config["output"] = json!("grouped");
    let result = run(command()
        .env("TMPDIR", invalid_tmpdir)
        .args(["run", "logs", "--file"])
        .arg(scratch.config(&config)));
    result.code(0);
    assert!(result.stdout.len() >= 131072);
    assert!(result.stderr.len() >= 131072);
    assert!(String::from_utf8_lossy(&result.stderr).contains("grouped"));
}

#[test]
fn fail_fast_spawn_failure_cancels_queued_launches() {
    let mut config = configuration(json!({
        "a_missing":{"kind":"task","command":["/definitely-missing-takt-fixture"]},
        "b_independent":fixture(&["args","SHOULD_NOT_RUN"]),
        "c_independent":fixture(&["args","SHOULD_NOT_RUN"])
    }));
    config["failure"] = json!("fail-fast");
    config["scheduling"]["jobs"] = json!(1);
    let result = configured(&config, &["a_missing", "b_independent", "c_independent"]);
    result.code(1);
    assert!(!result.has_event("task_started", "b_independent"));
    assert!(!result.has_event("task_started", "c_independent"));
}

#[test]
fn fail_fast_spawn_failure_cancels_concurrent_launches() {
    let mut config = configuration(json!({
        "a_missing":{"kind":"task","command":["/definitely-missing-takt-fixture"]},
        "b_independent":fixture(&["sleep","2000"]),
        "c_independent":fixture(&["sleep","2000"])
    }));
    config["failure"] = json!("fail-fast");
    let result = configured(&config, &["a_missing", "b_independent", "c_independent"]);
    result.code(1);
    let events = result.events();
    let outcomes = &events.last().unwrap()["outcomes"];
    assert_eq!(outcomes["a_missing"]["reason"], "spawn_failure");
    for id in ["b_independent", "c_independent"] {
        assert_eq!(outcomes[id]["kind"], "cancelled");
        assert_eq!(outcomes[id]["reason"], "fail_fast");
    }
}

#[test]
fn successful_prerequisite_cleanup_does_not_block_its_continuation() {
    let mut consumer = fixture(&["exit", "0"]);
    consumer["depends_on"] = json!([{"task":"producer","when":"exit","codes":[0]}]);
    let result = configured(
        &configuration(
            json!({"producer":fixture(&["orphan-tree","2","3000","ignore"]),"consumer":consumer}),
        ),
        &["consumer"],
    );
    result.code(0);
    assert!(result.has_event("task_started", "consumer"));
}

#[test]
fn service_exit_before_readiness_preserves_cause_and_blocks_consumers() {
    let result = configured(
        &service_config(
            json!({"type":"stdio","contains":"READY","timeout":"1s"}),
            fixture(&["exit", "7"]),
        ),
        &["consumer"],
    );
    result.code(1);
    assert!(!result.has_event("task_started", "consumer"));
    let events = result.events();
    let outcomes = &events.last().unwrap()["outcomes"];
    assert_eq!(outcomes["service"]["exit_code"], 7);
    assert_eq!(outcomes["consumer"]["kind"], "blocked");
}

#[test]
fn service_failure_policy_controls_active_consumers_and_keeps_independent_work() {
    for policy in ["continue", "stop-dependents"] {
        let mut config = service_config(
            json!({"type":"stdio","contains":"READY","timeout":"1s"}),
            fixture(&["startup", "400", "stdout", "7"]),
        );
        config["service_failure"] = json!(policy);
        config["tasks"]["consumer"]["command"] = fixture(&["sleep", "900"])["command"].clone();
        let mut future = fixture(&["exit", "0"]);
        future["depends_on"] = json!([
            {"task":"consumer","when":"exit","codes":[0]},
            {"task":"service","when":"ready"}
        ]);
        config["tasks"]["future"] = future;
        config["tasks"]["independent"] = fixture(&["sleep", "900"]);
        let result = configured(&config, &["future", "independent"]);
        result.code(1);
        assert!(result.has_event("task_started", "consumer"));
        assert!(!result.has_event("task_started", "future"));
        let events = result.events();
        let outcomes = &events.last().unwrap()["outcomes"];
        assert_eq!(outcomes["service"]["exit_code"], 7);
        assert_eq!(outcomes["independent"]["kind"], "succeeded");
        assert_eq!(
            outcomes["consumer"]["kind"],
            if policy == "continue" {
                "succeeded"
            } else {
                "cancelled"
            }
        );
    }
}

#[test]
fn explicitly_selected_service_stays_running_until_termination() {
    let scratch = Scratch::new();
    let config = service_config(
        json!({"type":"stdio","contains":"READY","timeout":"1s"}),
        fixture(&["startup", "5000"]),
    );
    let mut child = spawn(
        command()
            .args(["run", "service", "--file"])
            .arg(scratch.config(&config)),
    );
    let stdout = capture(child.0.stdout.take().unwrap());
    let stderr = capture(child.0.stderr.take().unwrap());
    std::thread::sleep(Duration::from_millis(200));
    assert!(child.0.try_wait().unwrap().is_none());
    // SAFETY: the harness retains the unreaped child, preventing PID reuse.
    assert_eq!(unsafe { libc::kill(child.0.id() as i32, libc::SIGTERM) }, 0);
    let until = Instant::now() + Duration::from_secs(3);
    let status = loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            break status;
        }
        assert!(
            Instant::now() < until,
            "persistent service did not terminate"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    let result = Run {
        status,
        stdout: stdout.join().unwrap(),
        stderr: stderr.join().unwrap(),
    };
    result.code(143);
    assert!(result.has_event("task_ready", "service"));
}

#[test]
fn automatic_memory_protection_terminates_a_bounded_over_limit_task() {
    let mut allocate = fixture(&["allocate", "32", "3000"]);
    allocate["limits"] = json!({"memory":"16MiB"});
    let mut config =
        configuration(json!({"allocate":allocate,"survivor":fixture(&["sleep","500"])}));
    config["enforcement"] = json!("auto");
    let result = configured(&config, &["allocate", "survivor"]);
    result.code(1);
    let events = result.events();
    let outcomes = &events.last().unwrap()["outcomes"];
    assert_eq!(outcomes["survivor"]["kind"], "succeeded");
    let reason = outcomes["allocate"]["reason"].as_str().unwrap();
    assert!(
        matches!(reason, "memory_watchdog" | "memory_limit"),
        "{outcomes}"
    );
    #[cfg(target_os = "macos")]
    assert_eq!(reason, "memory_watchdog");
}

fn assert_regrouped_descendant_stopped(result: &Run) {
    let logs = result.logs();
    let identity = logs
        .lines()
        .find_map(|line| line.strip_prefix("GROUP "))
        .expect("changed-group identity was announced before allocation");
    let identity: Vec<u32> = identity
        .split_whitespace()
        .map(|field| field.parse().unwrap())
        .collect();
    assert_eq!(identity.len(), 2, "{logs}");
    assert_eq!(
        identity[0], identity[1],
        "child did not create its own process group"
    );
    let events = result.events();
    let leader = events
        .iter()
        .find(|event| event["event"] == "task_started" && event["task_id"] == "tree")
        .unwrap()["pid"]
        .as_u64()
        .unwrap();
    assert_ne!(
        u64::from(identity[0]),
        leader,
        "fixture child is the task leader"
    );
    assert!(
        !logs.contains("HOLD_COMPLETE"),
        "memory hold completed without intervention"
    );
    takt_test_fixture::assert_process_stopped(identity[0]);
}

#[test]
fn automatic_memory_protection_counts_descendants_in_other_process_groups() {
    let mut task = fixture(&["regroup-tree", "48", "4000"]);
    task["limits"] = json!({"memory":"16MiB"});
    let mut config = configuration(json!({"tree":task}));
    config["enforcement"] = json!("auto");
    let started = Instant::now();
    let result = configured(&config, &["tree"]);
    result.code(1);
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "memory limit did not interrupt the four-second hold"
    );
    let events = result.events();
    let outcome = &events.last().unwrap()["outcomes"]["tree"];
    let reason = outcome["reason"].as_str().unwrap();
    assert!(
        matches!(reason, "memory_watchdog" | "memory_limit"),
        "{outcome}"
    );
    #[cfg(target_os = "macos")]
    assert_eq!(reason, "memory_watchdog");
    assert_regrouped_descendant_stopped(&result);
}

#[test]
fn aggregate_memory_protection_counts_descendants_in_other_process_groups() {
    let mut config = configuration(json!({"tree":fixture(&["regroup-tree","48","4000"])}));
    config["enforcement"] = json!("auto");
    config["scheduling"]["memory"] = json!("24MiB");
    let started = Instant::now();
    let result = configured(&config, &["tree"]);
    result.code(1);
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "aggregate budget did not interrupt the four-second hold"
    );
    let events = result.events();
    let outcome = &events.last().unwrap()["outcomes"]["tree"];
    let reason = outcome["reason"].as_str().unwrap();
    assert!(
        matches!(
            reason,
            "aggregate_memory_watchdog" | "aggregate_memory_limit"
        ),
        "{outcome}"
    );
    #[cfg(target_os = "macos")]
    assert_eq!(reason, "aggregate_memory_watchdog");
    assert_regrouped_descendant_stopped(&result);
}

#[test]
fn node_options_are_inherited_without_affecting_runner_and_secrets_are_not_diagnosed() {
    let result = run(command()
        .env("NODE_OPTIONS", "--takt-deliberately-invalid-node-option")
        .env("TAKT_TEST_PRIVATE_VALUE", "do-not-print-this-fixture-value")
        .args(["exec", "--enforcement", "off", "--output", "json", "--"])
        .arg(takt_test_fixture::executable())
        .args(["environment", "NODE_OPTIONS"]));
    result.code(0);
    assert!(
        result
            .logs()
            .contains("--takt-deliberately-invalid-node-option")
    );
    assert!(!String::from_utf8_lossy(&result.stdout).contains("do-not-print-this-fixture-value"));
    assert!(!String::from_utf8_lossy(&result.stderr).contains("do-not-print-this-fixture-value"));
}

#[test]
fn policy_provenance_distinguishes_explicit_auto_and_cli_env_file_defaults() {
    let scratch = Scratch::new();
    let config = json!({"version":1,"enforcement":"auto","output":"json","failure":"continue",
        "scheduling":{"jobs":2,"cpu":"auto","queue_timeout":"off"},
        "tasks":{"check":fixture(&["exit","0"])}});
    let path = scratch.config(&config);
    let result = run(command()
        .env("TAKT_JOBS", "3")
        .env("TAKT_MEMORY", "128MiB")
        .args(["plan", "check", "--jobs", "4", "--file"])
        .arg(path));
    result.code(0);
    let events = result.events();
    let sources = &events[0]["budget"]["provenance"];
    assert_eq!(sources["jobs"], "cli");
    assert_eq!(sources["memory"], "environment");
    for key in ["cpu", "enforcement", "output", "failure", "queue-timeout"] {
        assert_eq!(sources[key], "configuration", "{key}: {sources}");
    }
    assert_eq!(sources["reserve-memory"], "default_or_auto");
    assert_eq!(sources["file"], "cli");
    assert_eq!(events[0]["budget"]["jobs"], 4);
}

#[test]
fn direct_argv_preserves_non_utf8_bytes() {
    use std::os::unix::ffi::OsStringExt;
    let result = run(command()
        .args([
            "exec",
            "--enforcement",
            "off",
            "--output",
            "json",
            "--",
            "/usr/bin/printf",
            "%s",
        ])
        .arg(std::ffi::OsString::from_vec(vec![0xff, 0xfe])));
    result.code(0);
    assert!(result.events().iter().any(|event| event["event"] == "log"
        && event["encoding"] == "base64"
        && event["data"] == "//4="));
}

#[test]
fn invalid_policy_environment_is_rejected_unless_overridden() {
    use std::os::unix::ffi::OsStringExt;
    for name in [
        "TAKT_JOBS",
        "TAKT_CPU",
        "TAKT_MEMORY",
        "TAKT_ENFORCEMENT",
        "TAKT_OUTPUT",
    ] {
        let mut invocation = command();
        invocation
            .env(name, std::ffi::OsString::from_vec(vec![0xff]))
            .args(["exec", "--", "/usr/bin/printf", "SHOULD_NOT_RUN"]);
        let result = run(&mut invocation);
        result.code(2);
        assert!(!String::from_utf8_lossy(&result.stdout).contains("SHOULD_NOT_RUN"));
    }
    let result = run(command()
        .env("TAKT_JOBS", std::ffi::OsString::from_vec(vec![0xff]))
        .args([
            "exec",
            "--jobs",
            "1",
            "--enforcement",
            "off",
            "--output",
            "json",
            "--",
            "/usr/bin/printf",
            "valid override",
        ]));
    result.code(0);
    assert!(result.logs().contains("valid override"));
}
