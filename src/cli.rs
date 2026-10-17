use crate::{
    config, graph,
    model::*,
    output::{Mode, Output},
    platform, runtime,
    scheduler::{Budget, Scheduler},
};
use lexopt::Arg;
use serde_json::json;
use std::{collections::BTreeMap, ffi::OsString, path::PathBuf, time::Duration};

const HELP: &str = "Takt — resource-aware command runner\n\nUsage:\n  takt run TASK|GROUP... [OPTIONS]\n  takt exec -- PROGRAM [ARG...]\n  takt exec --command 'SHELL STRING' [--command 'SHELL STRING'] [OPTIONS]\n  takt validate [--file PATH] [--output json]\n  takt plan TASK|GROUP... [OPTIONS]\n  takt doctor [--output json]\n\nOptions:\n  --file PATH          YAML or JSON configuration (TAKT_FILE)\n  --jobs N|auto        Active task-tree slots (TAKT_JOBS)\n  --cpu N|auto         CPU equivalents, up to 3 decimals (TAKT_CPU)\n  --memory SIZE|auto   Aggregate memory budget, e.g. 2GiB (TAKT_MEMORY)\n  --enforcement MODE  auto|required|off (TAKT_ENFORCEMENT)\n  --output MODE       auto|plain|grouped|json (TAKT_OUTPUT)\n  --failure POLICY    continue|fail-fast\n  --queue-timeout TIME|off\n  -h, --help          Show help\n  -V, --version       Show version\n\nCLI settings override environment, file, and automatic defaults, in that order.\nConfiguration is discovered only in the current directory. Shell strings use /bin/sh -c.\nResource requests reserve capacity; they are not hard per-task limits.\n";

#[derive(Default)]
struct Args {
    command: Option<String>,
    names: Vec<String>,
    direct: Vec<OsString>,
    shell: Vec<String>,
    options: BTreeMap<String, String>,
    file: Option<PathBuf>,
    help: bool,
    version: bool,
}
fn parse() -> Result<Args, String> {
    let mut result = Args::default();
    let mut parser = lexopt::Parser::from_env();
    while let Some(arg) = parser.next().map_err(|e| e.to_string())? {
        match arg {
            Arg::Long("help") | Arg::Short('h') => result.help = true,
            Arg::Long("version") | Arg::Short('V') => result.version = true,
            Arg::Long("file") => {
                if result.file.is_some() {
                    return Err("--file supplied more than once".into());
                }
                result.file = Some(PathBuf::from(parser.value().map_err(|e| e.to_string())?));
            }
            Arg::Long("command") => result.shell.push(
                parser
                    .value()
                    .map_err(|e| e.to_string())?
                    .into_string()
                    .map_err(|_| "shell command must be UTF-8")?,
            ),
            Arg::Long(
                name @ ("jobs" | "cpu" | "memory" | "enforcement" | "output" | "failure"
                | "queue-timeout"),
            ) => {
                let name = name.to_owned();
                let value = parser
                    .value()
                    .map_err(|e| e.to_string())?
                    .into_string()
                    .map_err(|_| "option value must be UTF-8")?;
                if result.options.insert(name.clone(), value).is_some() {
                    return Err(format!("--{name} supplied more than once"));
                }
            }
            Arg::Value(value) => {
                if result.command.is_none() {
                    result.command = Some(value.into_string().map_err(|_| "invalid subcommand")?);
                } else if result.command.as_deref() == Some("exec") {
                    result.direct.push(value);
                } else {
                    result.names.push(
                        value
                            .into_string()
                            .map_err(|_| "task names must be UTF-8")?,
                    );
                }
            }
            other => return Err(other.unexpected().to_string()),
        }
    }
    if !result.direct.is_empty() && !result.shell.is_empty() {
        return Err("direct argv and --command cannot be combined".into());
    }
    if result.command.as_deref() != Some("exec") && !result.shell.is_empty() {
        return Err("--command requires takt exec".into());
    }
    Ok(result)
}

pub fn main_entry() -> i32 {
    if let Some(code) = crate::process::maybe_run_helper() {
        return code;
    }
    if let Some(code) = crate::probes::maybe_run_resolver() {
        return code;
    }
    let args = match parse() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("takt: {e}");
            return 2;
        }
    };
    if args.version {
        println!("takt {}", env!("CARGO_PKG_VERSION"));
        return 0;
    }
    if args.help || args.command.is_none() {
        print!("{HELP}");
        return 0;
    }
    let initial_output = args
        .options
        .get("output")
        .cloned()
        .or_else(|| std::env::var("TAKT_OUTPUT").ok())
        .unwrap_or_else(|| "auto".into());
    match execute(args) {
        Ok(code) => code,
        Err((code, error)) => {
            if initial_output == "json" {
                if let Ok(mut output) = Output::new(Mode::Json) {
                    output.emit("error", None, json!({"message":error,"exit_code":code}));
                    let _ = output.finish();
                }
            } else {
                eprintln!("takt: {error}");
            }
            code
        }
    }
}

fn execute(args: Args) -> Result<i32, (i32, String)> {
    let command = args.command.as_deref().unwrap_or("help");
    if !matches!(command, "run" | "exec" | "validate" | "plan" | "doctor") {
        return Err((2, format!("unknown command {command}; use --help")));
    }
    let mut provenance = BTreeMap::<String, String>::new();
    let mut selected = args.names.clone();
    let mut direct = BTreeMap::new();
    let cwd = std::env::current_dir().map_err(|e| (2, e.to_string()))?;
    let mut config = if matches!(command, "exec" | "doctor") {
        let mut tasks = BTreeMap::new();
        if command == "exec" {
            if args.direct.is_empty() && args.shell.is_empty() {
                return Err((2, "exec requires -- PROGRAM ARG... or --command".into()));
            }
            if !args.direct.is_empty() {
                let name = "command1".to_owned();
                let mut task = Task::command(
                    args.direct
                        .iter()
                        .map(|a| a.to_string_lossy().into_owned())
                        .collect(),
                );
                task.cwd = Some(cwd.clone());
                tasks.insert(name.clone(), task);
                direct.insert(name.clone(), args.direct);
                selected.push(name);
            } else {
                for (i, shell) in args.shell.into_iter().enumerate() {
                    let name = format!("command{}", i + 1);
                    let mut task = Task::command(Vec::new());
                    task.command = None;
                    task.shell = Some(shell);
                    task.cwd = Some(cwd.clone());
                    tasks.insert(name.clone(), task);
                    selected.push(name);
                }
            }
        }
        Config {
            version: 1,
            scheduling: Scheduling::default(),
            enforcement: Enforcement::Auto,
            failure: FailurePolicy::Continue,
            service_failure: ServiceFailurePolicy::Continue,
            output: OutputMode::Auto,
            defaults: Defaults::default(),
            tasks,
            groups: BTreeMap::new(),
        }
    } else {
        let file_source = if args.file.is_some() {
            "cli"
        } else if std::env::var_os("TAKT_FILE").is_some() {
            "environment"
        } else {
            "discovery"
        };
        let file = args
            .file
            .or_else(|| std::env::var_os("TAKT_FILE").map(PathBuf::from));
        let path = config::discover(&cwd, file.as_deref()).map_err(|e| (2, e))?;
        let (loaded, sources) = config::load_with_sources(&path).map_err(|e| (2, e))?;
        provenance.extend(sources);
        provenance.insert("file".into(), file_source.into());
        loaded
    };
    for (key, env) in [
        ("jobs", "TAKT_JOBS"),
        ("cpu", "TAKT_CPU"),
        ("memory", "TAKT_MEMORY"),
        ("enforcement", "TAKT_ENFORCEMENT"),
        ("output", "TAKT_OUTPUT"),
    ] {
        let (value, source) = if let Some(value) = args.options.get(key) {
            (Some(value.clone()), "cli")
        } else {
            let value = match std::env::var(env) {
                Ok(value) => Some(value),
                Err(std::env::VarError::NotPresent) => None,
                Err(std::env::VarError::NotUnicode(_)) => {
                    return Err((2, format!("{env} must be valid UTF-8")));
                }
            };
            (value, "environment")
        };
        if let Some(value) = value {
            apply_option(&mut config, key, &value).map_err(|e| (2, e))?;
            provenance.insert(key.into(), source.into());
        } else {
            provenance
                .entry(key.into())
                .or_insert_with(|| "default_or_auto".into());
        }
    }
    for key in ["failure", "queue-timeout"] {
        if let Some(value) = args.options.get(key) {
            apply_option(&mut config, key, value).map_err(|e| (2, e))?;
            provenance.insert(key.into(), "cli".into());
        }
    }
    for key in [
        "failure",
        "queue-timeout",
        "reserve-memory",
        "service-failure",
    ] {
        provenance
            .entry(key.into())
            .or_insert_with(|| "default_or_auto".into());
    }
    let mode = match config.output {
        OutputMode::Auto => Mode::Auto,
        OutputMode::Plain => Mode::Plain,
        OutputMode::Grouped => Mode::Grouped,
        OutputMode::Json => Mode::Json,
    };
    if command == "doctor" {
        if !selected.is_empty() {
            return Err((2, "doctor does not accept task selections".into()));
        }
        return report(
            mode,
            "doctor",
            json!({"resources":platform::discover(),"capabilities":platform::inspect_capabilities(),"platform":std::env::consts::OS,"architecture":std::env::consts::ARCH}),
        );
    }
    graph::validate(&config).map_err(|e| (2, e))?;
    if command == "validate" {
        if !selected.is_empty() {
            return Err((2, "validate does not accept task selections".into()));
        }
        return report(
            mode,
            "validation",
            json!({"valid":true,"tasks":config.tasks.len(),"groups":config.groups.len()}),
        );
    }
    let selection = graph::select(&config, &selected).map_err(|e| (2, e))?;
    let snapshot = platform::discover();
    let (budget, details) = resolve_budget(&config, &snapshot, &provenance).map_err(|e| (3, e))?;
    let scheduler =
        Scheduler::new(&config, selection.clone(), budget.clone()).map_err(|e| (3, e))?;
    if command == "plan" {
        return report(
            mode,
            "plan",
            json!({"selected_roots":selection.roots,"tasks":selection.tasks,"budget":details,"requests":scheduler.requests,"capabilities":platform::inspect_capabilities()}),
        );
    }
    let output = Output::new(mode).map_err(|e| (70, e.to_string()))?;
    runtime::run(
        config, selection, scheduler, budget, details, direct, output,
    )
    .map_err(|e| (70, e))
}

fn report(mode: Mode, event: &str, value: serde_json::Value) -> Result<i32, (i32, String)> {
    if mode == Mode::Json {
        let mut output = Output::new(mode).map_err(|e| (70, e.to_string()))?;
        output.emit(event, None, value);
        Ok(if output.finish() { 0 } else { 1 })
    } else {
        println!(
            "{}",
            serde_json::to_string_pretty(&value).map_err(|e| (70, e.to_string()))?
        );
        Ok(0)
    }
}
fn apply_option(config: &mut Config, key: &str, value: &str) -> Result<(), String> {
    match key {
        "jobs" => {
            config.scheduling.jobs = Some(if value == "auto" {
                Auto::Auto(AutoWord::Auto)
            } else {
                Auto::Value(
                    value
                        .parse::<usize>()
                        .ok()
                        .filter(|n| *n > 0)
                        .ok_or("jobs must be positive or auto")?,
                )
            })
        }
        "cpu" => {
            config.scheduling.cpu = Some(if value == "auto" {
                Auto::Auto(AutoWord::Auto)
            } else {
                Auto::Value(value.parse()?)
            })
        }
        "memory" => {
            config.scheduling.memory = Some(if value == "auto" {
                Auto::Auto(AutoWord::Auto)
            } else {
                Auto::Value(value.parse()?)
            })
        }
        "enforcement" => {
            config.enforcement = serde_json::from_value(json!(value))
                .map_err(|_| "enforcement must be auto, required, or off")?
        }
        "output" => {
            config.output = serde_json::from_value(json!(value))
                .map_err(|_| "output must be auto, plain, grouped, or json")?
        }
        "failure" => {
            config.failure = serde_json::from_value(json!(value))
                .map_err(|_| "failure must be continue or fail-fast")?
        }
        "queue-timeout" => config.scheduling.queue_timeout = Some(value.parse()?),
        _ => return Err("unknown option".into()),
    }
    Ok(())
}
fn resolve_budget(
    config: &Config,
    snapshot: &platform::ResourceSnapshot,
    provenance: &BTreeMap<String, String>,
) -> Result<(Budget, serde_json::Value), String> {
    let requested_cpu = config
        .scheduling
        .cpu
        .as_ref()
        .and_then(Auto::explicit)
        .map(|c| c.0);
    let cpu = match (requested_cpu, snapshot.cpu_millis) {
        (Some(a), Some(b)) => a.min(b),
        (Some(a), None) => a,
        (None, Some(b)) => b,
        (None, None) => 1000,
    };
    let reserve = config
        .scheduling
        .reserve_memory
        .map(|m| m.0)
        .unwrap_or_else(|| {
            snapshot
                .memory_available
                .map_or(256 * 1024 * 1024, |n| (n / 100 * 15).max(256 * 1024 * 1024))
        });
    let requested_memory = config
        .scheduling
        .memory
        .as_ref()
        .and_then(Auto::explicit)
        .map(|m| m.0);
    let available = snapshot.memory_available.map(|n| n.saturating_sub(reserve));
    let memory = match (requested_memory, available) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, None) => a,
        (None, b) => b,
    };
    if cpu == 0 || memory == Some(0) {
        return Err(
            "no usable resource capacity after applying host ceilings and memory reserve".into(),
        );
    }
    let jobs = config
        .scheduling
        .jobs
        .as_ref()
        .and_then(Auto::explicit)
        .copied()
        .unwrap_or_else(|| {
            if snapshot.memory_available.is_none() {
                1
            } else {
                (cpu / 1000).max(1) as usize
            }
        });
    let queue_timeout = config
        .scheduling
        .queue_timeout
        .unwrap_or(Timeout::After(Duration::from_secs(120)))
        .duration();
    let budget = Budget {
        jobs,
        cpu,
        memory,
        reserve_memory: reserve,
        queue_timeout,
    };
    let details = json!({"jobs":jobs,"requested_cpu_millicores":requested_cpu,"cpu_millicores":cpu,"requested_memory_bytes":requested_memory,"memory_bytes":memory,"reserve_memory_bytes":reserve,"provenance":provenance,"discovered":snapshot,"queue_timeout_ms":queue_timeout.map(|d|d.as_millis())});
    Ok((budget, details))
}
