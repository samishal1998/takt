//! Deterministic admission and lifecycle decisions. All clocks and observations are inputs.
use crate::{
    graph::{self, Selection},
    model::*,
};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};

#[derive(Clone, Debug, Serialize)]
pub struct Budget {
    pub jobs: usize,
    pub cpu: u64,
    pub memory: Option<u64>,
    pub reserve_memory: u64,
    pub queue_timeout: Option<Duration>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct TaskRequest {
    pub cpu: u64,
    pub memory: u64,
    pub cpu_inferred: bool,
    pub memory_inferred: bool,
}
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub available_memory: Option<u64>,
    pub usage: BTreeMap<String, u64>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueReason {
    JobsLimit,
    CpuBudget,
    MemoryBudget,
    LiveMemoryPressure,
    DependencyWait,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum State {
    Pending,
    Queued {
        since: Duration,
        reason: QueueReason,
    },
    Starting,
    Running,
    Ready,
    Stopping,
    Terminal(Outcome),
}
impl State {
    pub fn is_active(&self) -> bool {
        matches!(
            self,
            Self::Starting | Self::Running | Self::Ready | Self::Stopping
        )
    }
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Terminal(_))
    }
    pub fn outcome(&self) -> Option<&Outcome> {
        if let Self::Terminal(o) = self {
            Some(o)
        } else {
            None
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Launch(String),
    Settled(String, Outcome),
    Stop(String, Reason),
}
pub struct Scheduler {
    pub tasks: BTreeMap<String, Task>,
    pub states: BTreeMap<String, State>,
    pub requests: BTreeMap<String, TaskRequest>,
    pub selection: Selection,
    pub budget: Budget,
    failure: FailurePolicy,
    service_failure: ServiceFailurePolicy,
    service_ancestors: BTreeMap<String, BTreeSet<String>>,
    failed_services: BTreeSet<String>,
    observed_failures: BTreeSet<String>,
}
impl Scheduler {
    pub fn new(config: &Config, selection: Selection, budget: Budget) -> Result<Self, String> {
        if budget.jobs == 0 || budget.cpu == 0 || budget.memory == Some(0) {
            return Err("effective jobs, CPU, and memory budgets must be positive".into());
        }
        let mut tasks = BTreeMap::new();
        let mut requests = BTreeMap::new();
        for id in &selection.tasks {
            let task = config
                .tasks
                .get(id)
                .ok_or_else(|| format!("unknown selected task `{id}`"))?
                .clone();
            let request = TaskRequest {
                cpu: task.resources.cpu.map_or(1000.min(budget.cpu), |v| v.0),
                memory: task
                    .resources
                    .memory
                    .map_or(budget.memory.map_or(1 << 30, |b| b.min(1 << 30)), |v| v.0),
                cpu_inferred: task.resources.cpu.is_none(),
                memory_inferred: task.resources.memory.is_none(),
            };
            if request.cpu > budget.cpu {
                return Err(format!(
                    "task `{id}` explicitly requests {} millicores, exceeding effective CPU budget {}",
                    request.cpu, budget.cpu
                ));
            }
            if budget.memory.is_some_and(|b| request.memory > b) {
                return Err(format!(
                    "task `{id}` explicitly requests {} memory bytes, exceeding effective memory budget {}",
                    request.memory,
                    budget.memory.unwrap_or(0)
                ));
            }
            requests.insert(id.clone(), request);
            tasks.insert(id.clone(), task);
        }
        let service_ancestors = graph::service_ancestry(config, &selection.tasks)?;
        // An ancestor service cannot release its reservation before its consumer executes.
        for (id, ancestors) in &service_ancestors {
            let request = requests[id];
            let mut cpu = request.cpu as u128;
            let mut memory = request.memory as u128;
            for ancestor in ancestors {
                cpu += requests[ancestor].cpu as u128;
                memory += requests[ancestor].memory as u128;
            }
            if ancestors.len() + 1 > budget.jobs
                || cpu > budget.cpu as u128
                || budget.memory.is_some_and(|b| memory > b as u128)
            {
                return Err(format!(
                    "service-reservation deadlock: task `{id}` and required services [{}] cannot fit jobs={}, CPU={} millicores, memory={:?} bytes",
                    ancestors.iter().cloned().collect::<Vec<_>>().join(", "),
                    budget.jobs,
                    budget.cpu,
                    budget.memory
                ));
            }
        }
        let states = selection
            .tasks
            .iter()
            .map(|id| (id.clone(), State::Pending))
            .collect();
        Ok(Self {
            tasks,
            states,
            requests,
            selection,
            budget,
            failure: config.failure,
            service_failure: config.service_failure,
            service_ancestors,
            failed_services: BTreeSet::new(),
            observed_failures: BTreeSet::new(),
        })
    }
    pub fn state(&self, id: &str) -> Option<&State> {
        self.states.get(id)
    }
    pub fn request(&self, id: &str) -> Option<&TaskRequest> {
        self.requests.get(id)
    }
    pub fn active(&self) -> Vec<&str> {
        self.states
            .iter()
            .filter(|(_, state)| state.is_active())
            .map(|(id, _)| id.as_str())
            .collect()
    }
    pub fn is_done(&self) -> bool {
        self.states.values().all(State::is_terminal)
    }
    pub fn any_failed(&self) -> bool {
        !self.observed_failures.is_empty()
            || self
                .states
                .values()
                .filter_map(State::outcome)
                .any(Outcome::is_failure)
    }
    pub fn started(&mut self, id: &str) -> Result<(), String> {
        match self.states.get_mut(id) {
            Some(state @ State::Starting) => {
                *state = State::Running;
                Ok(())
            }
            _ => Err(format!("task `{id}` was not starting")),
        }
    }
    pub fn ready(&mut self, id: &str) -> Result<(), String> {
        if !self
            .tasks
            .get(id)
            .is_some_and(|t| t.kind == TaskKind::Service && t.readiness.is_some())
        {
            return Err(format!("task `{id}` cannot become ready"));
        }
        match self.states.get_mut(id) {
            Some(state @ State::Running) => {
                *state = State::Ready;
                Ok(())
            }
            _ => Err(format!("service `{id}` was not running")),
        }
    }
    /// Record a known outcome immediately while retaining the cleanup reservation.
    /// Repeated stopping observations are idempotent; terminal/pending tasks reject them.
    pub fn stopping(&mut self, id: &str, outcome: &Outcome) -> Result<(), String> {
        let state = self
            .states
            .get_mut(id)
            .ok_or_else(|| format!("unknown task `{id}`"))?;
        if !state.is_active() {
            return Err(format!("task `{id}` is not active"));
        }
        *state = State::Stopping;
        self.observe_failure(id, outcome);
        Ok(())
    }

    fn observe_failure(&mut self, id: &str, outcome: &Outcome) {
        if outcome.is_failure()
            && self.observed_failures.insert(id.to_owned())
            && self.tasks[id].kind == TaskKind::Service
        {
            self.failed_services.insert(id.to_owned());
        }
    }
    /// Call only after the owned process tree is cleaned up and capture is drained.
    pub fn completed(&mut self, id: &str, outcome: Outcome) -> Result<(), String> {
        let state = self
            .states
            .get_mut(id)
            .ok_or_else(|| format!("unknown task `{id}`"))?;
        if !state.is_active() {
            return Err(format!(
                "task `{id}` cannot release reservations twice or before launch"
            ));
        }
        *state = State::Terminal(outcome.clone());
        self.observe_failure(id, &outcome);
        Ok(())
    }
    pub fn reservations(&self) -> (usize, u64, u64) {
        let (mut jobs, mut cpu, mut memory) = (0, 0_u64, 0_u64);
        for id in self.active() {
            jobs += 1;
            cpu = cpu.saturating_add(self.requests[id].cpu);
            memory = memory.saturating_add(self.requests[id].memory);
        }
        (jobs, cpu, memory)
    }
    pub fn cancel(&mut self, reason: Reason) -> Vec<Action> {
        let ids: Vec<_> = self.states.keys().cloned().collect();
        let mut actions = Vec::new();
        for id in ids {
            self.cancel_one(&id, reason, &mut actions);
        }
        actions
    }
    fn cancel_one(&mut self, id: &str, reason: Reason, actions: &mut Vec<Action>) {
        match self.states.get(id) {
            Some(State::Pending | State::Queued { .. }) => {
                self.settle(id, Outcome::cancelled(reason), actions)
            }
            Some(State::Starting | State::Running | State::Ready) => {
                self.states.insert(id.to_owned(), State::Stopping);
                actions.push(Action::Stop(id.to_owned(), reason));
            }
            _ => {}
        }
    }
    fn settle(&mut self, id: &str, outcome: Outcome, actions: &mut Vec<Action>) {
        self.states
            .insert(id.to_owned(), State::Terminal(outcome.clone()));
        actions.push(Action::Settled(id.to_owned(), outcome));
    }
    pub fn tick(&mut self, now: Duration, snapshot: &Snapshot) -> Vec<Action> {
        if self.failure == FailurePolicy::FailFast && self.any_failed() {
            return self.cancel(Reason::FailFast);
        }
        let mut actions = Vec::new();
        if self.service_failure == ServiceFailurePolicy::StopDependents {
            for failed in std::mem::take(&mut self.failed_services) {
                let mut dependent = BTreeSet::from([failed]);
                loop {
                    let before = dependent.len();
                    for (id, task) in &self.tasks {
                        if task.depends_on.iter().any(|d| dependent.contains(d.task())) {
                            dependent.insert(id.clone());
                        }
                    }
                    if dependent.len() == before {
                        break;
                    }
                }
                for id in dependent {
                    self.cancel_one(&id, Reason::ServiceFailed, &mut actions);
                }
            }
        } else {
            self.failed_services.clear();
        }
        // Settle cascading nonmatches before calculating eligibility timestamps.
        loop {
            let mut changed = false;
            let ids: Vec<_> = self.states.keys().cloned().collect();
            for id in ids {
                if !matches!(self.states[&id], State::Pending | State::Queued { .. }) {
                    continue;
                }
                match self.dependencies(&id) {
                    DependencyState::Blocked => {
                        self.settle(
                            &id,
                            Outcome {
                                kind: OutcomeKind::Blocked,
                                ..Outcome::failed(Reason::DependencyFailed)
                            },
                            &mut actions,
                        );
                        changed = true;
                    }
                    DependencyState::Skipped => {
                        self.settle(
                            &id,
                            Outcome {
                                kind: OutcomeKind::Skipped,
                                ..Outcome::failed(Reason::ConditionNotMatched)
                            },
                            &mut actions,
                        );
                        changed = true;
                    }
                    DependencyState::Matched => {
                        if matches!(self.states[&id], State::Pending) {
                            self.states.insert(
                                id,
                                State::Queued {
                                    since: now,
                                    reason: QueueReason::DependencyWait,
                                },
                            );
                        }
                    }
                    DependencyState::Waiting => {}
                }
            }
            if !changed {
                break;
            }
        }
        if self.failure == FailurePolicy::FailFast && self.any_failed() {
            actions.extend(self.cancel(Reason::FailFast));
            return actions;
        }
        self.stop_unused_services(&mut actions);
        let mut queued: Vec<_> = self
            .states
            .iter()
            .filter_map(|(id, s)| {
                if let State::Queued { since, .. } = s {
                    Some((*since, id.clone()))
                } else {
                    None
                }
            })
            .collect();
        queued.sort();
        let mut fairness_hold = false;
        for (since, id) in queued {
            if self
                .budget
                .queue_timeout
                .is_some_and(|timeout| now.saturating_sub(since) >= timeout)
            {
                self.settle(&id, Outcome::failed(Reason::QueueTimeout), &mut actions);
                if self.failure == FailurePolicy::FailFast {
                    actions.extend(self.cancel(Reason::FailFast));
                    return actions;
                }
                continue;
            }
            let reason = self.admission_reason(&id, snapshot);
            if let Some(reason) = reason {
                self.states.insert(id, State::Queued { since, reason });
                if now.saturating_sub(since) >= Duration::from_secs(5) {
                    fairness_hold = true;
                }
            } else if !fairness_hold {
                // Reserving and marking Starting is one atomic domain transition.
                self.states.insert(id.clone(), State::Starting);
                actions.push(Action::Launch(id));
            }
        }
        self.detect_live_service_deadlock(&mut actions);
        actions
    }
    fn dependencies(&self, id: &str) -> DependencyState {
        let (mut waiting, mut skipped, mut blocked) = (false, false, false);
        for dependency in &self.tasks[id].depends_on {
            let state = &self.states[dependency.task()];
            match (dependency, state) {
                (Dependency::Ready { .. }, State::Ready) => {}
                (Dependency::Exit { codes, .. }, State::Terminal(outcome))
                    if outcome.reason == Reason::NormalExit
                        && outcome
                            .exit_code
                            .and_then(|c| u8::try_from(c).ok())
                            .is_some_and(|c| codes.contains(&c)) => {}
                (_, State::Terminal(outcome)) => match outcome.kind {
                    OutcomeKind::Succeeded | OutcomeKind::Skipped => skipped = true,
                    _ => blocked = true,
                },
                (Dependency::Ready { .. }, State::Stopping) => blocked = true,
                _ => waiting = true,
            }
        }
        if blocked {
            DependencyState::Blocked
        } else if skipped {
            DependencyState::Skipped
        } else if waiting {
            DependencyState::Waiting
        } else {
            DependencyState::Matched
        }
    }
    fn admission_reason(&self, id: &str, snapshot: &Snapshot) -> Option<QueueReason> {
        let (jobs, cpu, memory) = self.reservations();
        let request = self.requests[id];
        if jobs >= self.budget.jobs {
            return Some(QueueReason::JobsLimit);
        }
        if (cpu as u128) + (request.cpu as u128) > self.budget.cpu as u128 {
            return Some(QueueReason::CpuBudget);
        }
        if self
            .budget
            .memory
            .is_some_and(|limit| (memory as u128) + (request.memory as u128) > limit as u128)
        {
            return Some(QueueReason::MemoryBudget);
        }
        if let Some(available) = snapshot.available_memory {
            // Available memory already excludes observed allocations. Only reserve the unconsumed remainder.
            // Unknown usage conservatively leaves the entire existing request outstanding.
            let outstanding: u128 = self
                .active()
                .iter()
                .map(|id| {
                    self.requests[*id]
                        .memory
                        .saturating_sub(snapshot.usage.get(*id).copied().unwrap_or(0))
                        as u128
                })
                .sum();
            if outstanding + request.memory as u128 + self.budget.reserve_memory as u128
                > available as u128
            {
                return Some(QueueReason::LiveMemoryPressure);
            }
        }
        None
    }
    fn stop_unused_services(&mut self, actions: &mut Vec<Action>) {
        let mut needed = BTreeSet::new();
        // A root can skip before an independent finite prerequisite settles.
        // Keep that prerequisite's service alive through its own completion.
        for (id, task) in &self.tasks {
            if task.kind == TaskKind::Task && !self.states[id].is_terminal() {
                needed.extend(self.service_ancestors[id].iter().cloned());
            }
        }
        for root in &self.selection.roots {
            if !self.states[root].is_terminal() {
                needed.extend(self.service_ancestors[root].iter().cloned());
                if self.tasks[root].kind == TaskKind::Service {
                    needed.insert(root.clone());
                }
            }
        }
        // An active consumer retains ancestors until its own cleanup completes.
        for id in self.active() {
            needed.extend(self.service_ancestors[id].iter().cloned());
        }
        let unused: Vec<_> = self
            .tasks
            .iter()
            .filter(|(id, t)| t.kind == TaskKind::Service && !needed.contains(*id))
            .map(|(id, _)| id.clone())
            .collect();
        for id in unused {
            match self.states[&id] {
                State::Starting | State::Running | State::Ready => {
                    self.states.insert(id.clone(), State::Stopping);
                    actions.push(Action::Stop(id, Reason::ServiceUnused));
                }
                State::Pending | State::Queued { .. } => self.settle(
                    &id,
                    Outcome {
                        kind: OutcomeKind::Skipped,
                        ..Outcome::failed(Reason::ConditionNotMatched)
                    },
                    actions,
                ),
                _ => {}
            }
        }
    }
    fn detect_live_service_deadlock(&mut self, actions: &mut Vec<Action>) {
        let active = self.active();
        if active.is_empty()
            || active.iter().any(|id| {
                self.tasks[*id].kind != TaskKind::Service
                    || matches!(self.states[*id], State::Starting | State::Stopping)
                    || (self.tasks[*id].readiness.is_some()
                        && !matches!(self.states[*id], State::Ready))
            })
        {
            return;
        }
        let queued: Vec<_> = self
            .states
            .iter()
            .filter_map(|(id, s)| match s {
                State::Queued {
                    reason:
                        QueueReason::JobsLimit | QueueReason::CpuBudget | QueueReason::MemoryBudget,
                    ..
                } => Some(id.clone()),
                _ => None,
            })
            .collect();
        if queued.is_empty() {
            return;
        }
        // If any task can start after observational pressure eases, services may still be released normally.
        if self.states.iter().any(|(_, s)| {
            matches!(
                s,
                State::Queued {
                    reason: QueueReason::LiveMemoryPressure | QueueReason::DependencyWait,
                    ..
                }
            )
        }) {
            return;
        }
        let service_names = active.join(", ");
        for id in queued {
            let mut outcome = Outcome::failed(Reason::ServiceDeadlock);
            outcome.detail = Some(format!(
                "live services [{service_names}] retain the slots or reservations required by `{id}`"
            ));
            self.settle(&id, outcome, actions);
        }
    }
}
enum DependencyState {
    Matched,
    Waiting,
    Skipped,
    Blocked,
}
