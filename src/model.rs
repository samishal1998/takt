//! Typed configuration and lifecycle vocabulary shared by the scheduler and runtime.
use serde::{Deserialize, Deserializer, Serialize, de};
use std::{collections::BTreeMap, fmt, path::PathBuf, str::FromStr, time::Duration};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Cpu(pub u64);

impl FromStr for Cpu {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (whole, fraction) = value.split_once('.').unwrap_or((value, ""));
        if whole.is_empty()
            || !whole.bytes().all(|b| b.is_ascii_digit())
            || !fraction.bytes().all(|b| b.is_ascii_digit())
            || fraction.len() > 3
            || (value.contains('.') && fraction.is_empty())
        {
            return Err(
                "CPU must be a positive decimal with at most three fractional digits".into(),
            );
        }
        let whole: u64 = whole.parse().map_err(|_| "CPU value overflows")?;
        let fraction: u64 = if fraction.is_empty() {
            0
        } else {
            fraction
                .parse::<u64>()
                .map_err(|_| "invalid CPU fraction")?
                * 10_u64.pow(3 - fraction.len() as u32)
        };
        let value = whole
            .checked_mul(1000)
            .and_then(|n| n.checked_add(fraction))
            .filter(|n| *n > 0)
            .ok_or("CPU must be positive and fit in integer millicores")?;
        Ok(Self(value))
    }
}
impl<'de> Deserialize<'de> for Cpu {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl de::Visitor<'_> for Visitor {
            type Value = Cpu;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a positive numeric CPU value with up to three fractional digits")
            }
            fn visit_u64<E: de::Error>(self, n: u64) -> Result<Cpu, E> {
                n.to_string().parse().map_err(E::custom)
            }
            fn visit_i64<E: de::Error>(self, n: i64) -> Result<Cpu, E> {
                n.to_string().parse().map_err(E::custom)
            }
            fn visit_f64<E: de::Error>(self, n: f64) -> Result<Cpu, E> {
                n.to_string().parse().map_err(E::custom)
            }
        }
        d.deserialize_any(Visitor)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Memory(pub u64);
impl FromStr for Memory {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        parse_quantity(
            value,
            &[
                ("TiB", 1 << 40),
                ("GiB", 1 << 30),
                ("MiB", 1 << 20),
                ("KiB", 1 << 10),
                ("B", 1),
            ],
        )
        .map(Self)
        .map_err(|e| format!("memory: {e}"))
    }
}
fn parse_quantity(value: &str, suffixes: &[(&str, u64)]) -> Result<u64, String> {
    for (suffix, scale) in suffixes {
        if let Some(n) = value.strip_suffix(suffix) {
            if n.is_empty() || n.starts_with('0') || !n.bytes().all(|b| b.is_ascii_digit()) {
                return Err("expected a positive integer and a supported unit".into());
            }
            return n
                .parse::<u64>()
                .ok()
                .and_then(|n| n.checked_mul(*scale))
                .filter(|n| *n > 0)
                .ok_or_else(|| "value overflows".into());
        }
    }
    Err("unsupported unit".into())
}
impl<'de> Deserialize<'de> for Memory {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        String::deserialize(d)?.parse().map_err(de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct DurationValue(pub Duration);
impl FromStr for DurationValue {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, String> {
        parse_quantity(
            value,
            &[("ms", 1), ("s", 1000), ("m", 60_000), ("h", 3_600_000)],
        )
        .map(|v| Self(Duration::from_millis(v)))
        .map_err(|e| format!("duration: {e}"))
    }
}
impl<'de> Deserialize<'de> for DurationValue {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        String::deserialize(d)?.parse().map_err(de::Error::custom)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Timeout {
    Off,
    After(Duration),
}
impl Timeout {
    pub fn duration(self) -> Option<Duration> {
        match self {
            Self::Off => None,
            Self::After(d) => Some(d),
        }
    }
}
impl FromStr for Timeout {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, String> {
        if value == "off" {
            Ok(Self::Off)
        } else {
            value.parse::<DurationValue>().map(|d| Self::After(d.0))
        }
    }
}
impl<'de> Deserialize<'de> for Timeout {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        String::deserialize(d)?.parse().map_err(de::Error::custom)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Auto<T> {
    Value(T),
    Auto(AutoWord),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AutoWord {
    #[serde(rename = "auto")]
    Auto,
}
impl<T> Auto<T> {
    pub fn explicit(&self) -> Option<&T> {
        match self {
            Self::Value(v) => Some(v),
            Self::Auto(_) => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Enforcement {
    #[default]
    Auto,
    Required,
    Off,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FailurePolicy {
    #[default]
    Continue,
    FailFast,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ServiceFailurePolicy {
    #[default]
    Continue,
    StopDependents,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OutputMode {
    #[default]
    Auto,
    Plain,
    Grouped,
    Json,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskKind {
    Task,
    Service,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scheduling {
    pub jobs: Option<Auto<usize>>,
    pub cpu: Option<Auto<Cpu>>,
    pub memory: Option<Auto<Memory>>,
    pub reserve_memory: Option<Memory>,
    pub queue_timeout: Option<Timeout>,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceSpec {
    pub cpu: Option<Cpu>,
    pub memory: Option<Memory>,
}
impl ResourceSpec {
    pub fn inherit(&mut self, defaults: Self) {
        self.cpu = self.cpu.or(defaults.cpu);
        self.memory = self.memory.or(defaults.memory);
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Defaults {
    #[serde(default)]
    pub resources: ResourceSpec,
    #[serde(default)]
    pub limits: ResourceSpec,
    pub timeout: Option<Timeout>,
    pub stop_grace: Option<DurationValue>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub unset_env: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub version: u32,
    #[serde(default)]
    pub scheduling: Scheduling,
    #[serde(default)]
    pub enforcement: Enforcement,
    #[serde(default)]
    pub failure: FailurePolicy,
    #[serde(default)]
    pub service_failure: ServiceFailurePolicy,
    #[serde(default)]
    pub output: OutputMode,
    #[serde(default)]
    pub defaults: Defaults,
    pub tasks: BTreeMap<String, Task>,
    #[serde(default)]
    pub groups: BTreeMap<String, Vec<String>>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Task {
    pub kind: TaskKind,
    pub command: Option<Vec<String>>,
    pub shell: Option<String>,
    pub cwd: Option<PathBuf>,
    pub success_codes: Option<Vec<u8>>,
    #[serde(default)]
    pub depends_on: Vec<Dependency>,
    pub readiness: Option<Probe>,
    #[serde(default)]
    pub resources: ResourceSpec,
    #[serde(default)]
    pub limits: ResourceSpec,
    pub timeout: Option<Timeout>,
    pub stop_grace: Option<DurationValue>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub unset_env: Vec<String>,
}
impl Task {
    pub fn command(command: Vec<String>) -> Self {
        Self {
            kind: TaskKind::Task,
            command: Some(command),
            shell: None,
            cwd: None,
            success_codes: None,
            depends_on: Vec::new(),
            readiness: None,
            resources: ResourceSpec::default(),
            limits: ResourceSpec::default(),
            timeout: None,
            stop_grace: None,
            env: BTreeMap::new(),
            unset_env: Vec::new(),
        }
    }
    pub fn accepts(&self, code: i32) -> bool {
        self.success_codes.as_ref().map_or(code == 0, |codes| {
            u8::try_from(code).is_ok_and(|c| codes.contains(&c))
        })
    }
    pub fn grace(&self) -> Duration {
        self.stop_grace.map_or(Duration::from_secs(5), |v| v.0)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "when", rename_all = "lowercase", deny_unknown_fields)]
pub enum Dependency {
    Exit { task: String, codes: Vec<u8> },
    Ready { task: String },
}
impl Dependency {
    pub fn task(&self) -> &str {
        match self {
            Self::Exit { task, .. } | Self::Ready { task } => task,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum Probe {
    Tcp {
        host: String,
        port: u16,
        timeout: Option<DurationValue>,
        interval: Option<DurationValue>,
        attempt_timeout: Option<DurationValue>,
    },
    Http {
        url: String,
        status_codes: Option<Vec<u16>>,
        body_contains: Option<String>,
        timeout: Option<DurationValue>,
        interval: Option<DurationValue>,
        attempt_timeout: Option<DurationValue>,
    },
    Udp {
        host: String,
        port: u16,
        payload: Bytes,
        expect: Reply,
        timeout: Option<DurationValue>,
        interval: Option<DurationValue>,
        attempt_timeout: Option<DurationValue>,
    },
    Stdio {
        stream: Stream,
        contains: String,
        timeout: Option<DurationValue>,
    },
}
impl Probe {
    pub fn timeout(&self) -> Duration {
        let timeout = match self {
            Self::Tcp { timeout, .. }
            | Self::Http { timeout, .. }
            | Self::Udp { timeout, .. }
            | Self::Stdio { timeout, .. } => timeout,
        };
        timeout.map_or(Duration::from_secs(30), |t| t.0)
    }
    pub fn interval(&self) -> Duration {
        let interval = match self {
            Self::Tcp { interval, .. }
            | Self::Http { interval, .. }
            | Self::Udp { interval, .. } => *interval,
            Self::Stdio { .. } => None,
        };
        interval.map_or(Duration::from_millis(250), |t| t.0)
    }
    pub fn attempt_timeout(&self) -> Duration {
        let timeout = match self {
            Self::Tcp {
                attempt_timeout, ..
            }
            | Self::Http {
                attempt_timeout, ..
            }
            | Self::Udp {
                attempt_timeout, ..
            } => *attempt_timeout,
            Self::Stdio { .. } => None,
        };
        timeout.map_or(Duration::from_secs(1), |t| t.0)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Stream {
    Stdout,
    Stderr,
    Either,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", deny_unknown_fields)]
pub enum Bytes {
    Text(String),
    Hex(String),
}
impl Bytes {
    pub fn decode(&self) -> Result<Vec<u8>, String> {
        match self {
            Self::Text(s) if !s.is_empty() => Ok(s.as_bytes().to_vec()),
            Self::Hex(s) if !s.is_empty() && s.len() % 2 == 0 => s
                .as_bytes()
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| {
                    let a = (pair[0] as char).to_digit(16).ok_or("invalid hex byte")?;
                    let b = (pair[1] as char).to_digit(16).ok_or("invalid hex byte")?;
                    Ok((a * 16 + b) as u8)
                })
                .collect::<Result<Vec<_>, &str>>()
                .map_err(str::to_owned),
            _ => {
                Err("bytes must contain nonempty text or an even-length hexadecimal string".into())
            }
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", deny_unknown_fields)]
pub enum Reply {
    Equals(Bytes),
    Prefix(Bytes),
}
impl Reply {
    pub fn bytes(&self) -> &Bytes {
        match self {
            Self::Equals(v) | Self::Prefix(v) => v,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeKind {
    Succeeded,
    Failed,
    Skipped,
    Blocked,
    Cancelled,
    Stopped,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    NormalExit,
    Signal,
    ExecutionTimeout,
    ReadinessTimeout,
    MemoryLimit,
    AggregateMemoryLimit,
    MemoryWatchdog,
    AggregateMemoryWatchdog,
    SpawnFailure,
    Cancelled,
    InternalError,
    ConditionNotMatched,
    DependencyFailed,
    QueueTimeout,
    ServiceDeadlock,
    ServiceUnused,
    ServiceFailed,
    FailFast,
    CleanupIncomplete,
    OutputError,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Outcome {
    pub kind: OutcomeKind,
    pub reason: Reason,
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
    pub detail: Option<String>,
}
impl Outcome {
    pub fn exit(code: i32, task: &Task) -> Self {
        Self {
            kind: if task.accepts(code) {
                OutcomeKind::Succeeded
            } else {
                OutcomeKind::Failed
            },
            reason: Reason::NormalExit,
            exit_code: Some(code),
            signal: None,
            detail: None,
        }
    }
    pub fn failed(reason: Reason) -> Self {
        Self {
            kind: OutcomeKind::Failed,
            reason,
            exit_code: None,
            signal: None,
            detail: None,
        }
    }
    pub fn cancelled(reason: Reason) -> Self {
        Self {
            kind: OutcomeKind::Cancelled,
            ..Self::failed(reason)
        }
    }
    pub fn stopped() -> Self {
        Self {
            kind: OutcomeKind::Stopped,
            ..Self::failed(Reason::ServiceUnused)
        }
    }
    pub fn is_failure(&self) -> bool {
        matches!(self.kind, OutcomeKind::Failed | OutcomeKind::Blocked)
    }
}
