//! Process identity and launcher checks independent of Takt's scheduler harness.
use serde_json::{Value, json};
use std::io::{self, Read};
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, ChildStdout, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

const DEADLINE: Duration = Duration::from_secs(12);

struct Harness {
    child: Child,
    stdout: ChildStdout,
    bytes: Vec<u8>,
    reaped: bool,
    task_groups: Vec<i32>,
    _directory: tempfile::TempDir,
}

impl Harness {
    fn start(mut task: Value, enforcement: &str) -> Self {
        task["kind"] = json!("task");
        let directory = tempfile::tempdir().unwrap();
        let config = json!({
            "version": 1,
            "enforcement": enforcement,
            "output": "json",
            "scheduling": {"jobs":1,"cpu":1,"memory":"128MiB","reserve_memory":"1MiB"},
            "defaults": {"resources":{"cpu":0.01,"memory":"1MiB"},"stop_grace":"150ms"},
            "tasks": {"work":task}
        });
        let path = directory.path().join("takt.json");
        std::fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
        let binary = std::env::var_os("TAKT_TEST_BINARY")
            .map(PathBuf::from)
            .unwrap_or_else(|| env!("CARGO_BIN_EXE_takt").into());
        let mut command = Command::new(binary);
        command
            .args(["run", "--file"])
            .arg(path)
            .arg("work")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .process_group(0);
        for key in [
            "TAKT_FILE",
            "TAKT_JOBS",
            "TAKT_CPU",
            "TAKT_MEMORY",
            "TAKT_ENFORCEMENT",
            "TAKT_OUTPUT",
        ] {
            command.env_remove(key);
        }
        let mut child = command.spawn().unwrap();
        let stdout = child.stdout.take().unwrap();
        takt::process::set_nonblocking(stdout.as_raw_fd()).unwrap();
        Self {
            child,
            stdout,
            bytes: Vec::new(),
            reaped: false,
            task_groups: Vec::new(),
            _directory: directory,
        }
    }

    fn read(&mut self) {
        let mut buffer = [0u8; 4096];
        loop {
            match self.stdout.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => {
                    assert!(
                        self.bytes.len() + count <= 1024 * 1024,
                        "test output exceeded bound"
                    );
                    self.bytes.extend_from_slice(&buffer[..count]);
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => panic!("read runner events: {error}"),
            }
        }
        for event in self.events() {
            if event["event"] == "task_started" {
                let pid = event["pid"].as_i64().unwrap() as i32;
                if !self.task_groups.contains(&pid) {
                    self.task_groups.push(pid);
                }
            }
        }
    }

    fn events(&self) -> Vec<Value> {
        self.bytes
            .split(|byte| *byte == b'\n')
            .filter_map(|line| serde_json::from_slice(line).ok())
            .collect()
    }

    fn logs(&self) -> String {
        self.events()
            .iter()
            .filter(|event| event["event"] == "log")
            .filter_map(|event| event["data"].as_str())
            .collect()
    }

    fn await_log(&mut self, predicate: impl Fn(&str) -> bool) {
        let until = Instant::now() + DEADLINE;
        loop {
            self.read();
            if predicate(&self.logs()) {
                return;
            }
            assert!(
                Instant::now() < until,
                "announcement deadline: {}",
                String::from_utf8_lossy(&self.bytes)
            );
            if self.child.try_wait().unwrap().is_some() {
                self.reaped = true;
                panic!(
                    "runner ended before announcement: {}",
                    String::from_utf8_lossy(&self.bytes)
                );
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn signal(&self, signal: i32) {
        // SAFETY: the direct child remains unreaped while this harness signals it.
        assert_eq!(unsafe { libc::kill(self.child.id() as i32, signal) }, 0);
    }

    fn finish(&mut self) -> ExitStatus {
        let until = Instant::now() + DEADLINE;
        loop {
            self.read();
            if let Some(status) = self.child.try_wait().unwrap() {
                self.reaped = true;
                self.read();
                return status;
            }
            assert!(
                Instant::now() < until,
                "runner exceeded independent deadline"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        if !self.reaped {
            // The live supervisor retains task leaders until their groups empty.
            // Ask it to reap first; fixture lifetimes are independently <=10s.
            // SAFETY: our own direct child is unreaped and retains its PID.
            unsafe {
                libc::kill(self.child.id() as i32, libc::SIGTERM);
            }
            for _ in 0..50 {
                if self.child.try_wait().ok().flatten().is_some() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn fixture(args: &[&str]) -> Value {
    let mut command = vec![
        takt_test_fixture::executable()
            .to_string_lossy()
            .into_owned(),
    ];
    command.extend(args.iter().map(|s| (*s).to_owned()));
    json!({"command":command})
}

#[test]
fn missing_exec_and_cwd_are_launch_errors_without_started_events() {
    let mut missing = fixture(&["exit", "0"]);
    missing["cwd"] = json!("/definitely-missing-takt-working-directory");
    for task in [
        json!({"command":["/definitely-missing-takt-program"]}),
        missing,
    ] {
        let mut runner = Harness::start(task, "off");
        assert_eq!(runner.finish().code(), Some(1));
        let events = runner.events();
        assert!(!events.iter().any(|event| event["event"] == "task_started"));
        assert!(
            events.iter().any(
                |event| event["event"] == "task_finished" && event["reason"] == "spawn_failure"
            ),
            "{}",
            String::from_utf8_lossy(&runner.bytes)
        );
    }
}

#[test]
fn cancellation_kills_group_members_that_ignore_graceful_signals() {
    let mut runner = Harness::start(fixture(&["tree", "2", "10000", "ignore"]), "off");
    runner.await_log(|logs| logs.lines().filter(|line| line.starts_with("PID ")).count() == 2);
    let members: Vec<u32> = runner
        .logs()
        .lines()
        .filter_map(|line| line.strip_prefix("PID ")?.parse().ok())
        .collect();
    runner.signal(libc::SIGTERM);
    assert_eq!(runner.finish().code(), Some(143));
    assert!(
        runner
            .events()
            .iter()
            .any(|event| event["event"] == "task_finished" && event["reason"] == "cancelled")
    );
    #[cfg(target_os = "linux")]
    for pid in members {
        if let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) {
            let state = stat
                .rsplit_once(')')
                .unwrap()
                .1
                .split_whitespace()
                .next()
                .unwrap();
            assert!(
                state == "Z" || state == "X",
                "owned descendant {pid} still alive"
            );
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = members;
}

#[test]
fn successful_parent_exit_cleans_surviving_descendants_before_finishing() {
    let started = Instant::now();
    let mut runner = Harness::start(fixture(&["orphan-tree", "2", "10000", "ignore"]), "off");
    assert_eq!(runner.finish().code(), Some(0));
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "surviving descendants were not cleaned after their parent exited"
    );
    let members: Vec<u32> = runner
        .logs()
        .lines()
        .filter_map(|line| line.strip_prefix("CHILD ")?.parse().ok())
        .collect();
    assert_eq!(members.len(), 2);
    #[cfg(target_os = "linux")]
    for pid in members {
        if let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) {
            let state = stat
                .rsplit_once(')')
                .unwrap()
                .1
                .split_whitespace()
                .next()
                .unwrap();
            assert!(
                state == "Z" || state == "X",
                "owned descendant {pid} still alive at task completion"
            );
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = members;
}

fn regrouped_descendant(runner: &Harness) -> u32 {
    let logs = runner.logs();
    let identity = logs
        .lines()
        .find_map(|line| line.strip_prefix("GROUP "))
        .expect("changed-group child announced its identity");
    let identity: Vec<u32> = identity
        .split_whitespace()
        .map(|field| field.parse().unwrap())
        .collect();
    assert_eq!(identity.len(), 2, "{logs}");
    assert_eq!(
        identity[0], identity[1],
        "child did not create its own process group"
    );
    assert_ne!(identity[0] as i32, runner.task_groups[0]);
    identity[0]
}

#[test]
fn cancellation_kills_descendants_that_change_process_group() {
    let mut runner = Harness::start(fixture(&["regroup-tree", "4", "4000"]), "off");
    runner.await_log(|logs| logs.contains("ALLOCATED "));
    let member = regrouped_descendant(&runner);
    // Keep the parent alive through an observation interval before cancellation;
    // discovery is sampled even when resource enforcement is disabled.
    std::thread::sleep(Duration::from_millis(250));
    let started = Instant::now();
    runner.signal(libc::SIGTERM);
    assert_eq!(runner.finish().code(), Some(143));
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "changed-group child survived cancellation grace"
    );
    assert!(
        runner
            .events()
            .iter()
            .any(|event| event["event"] == "task_finished" && event["reason"] == "cancelled")
    );
    assert!(!runner.logs().contains("HOLD_COMPLETE"));
    takt_test_fixture::assert_process_stopped(member);
}

#[test]
fn successful_parent_exit_cleans_observed_descendants_in_other_groups() {
    let started = Instant::now();
    let mut runner = Harness::start(fixture(&["regroup-tree", "4", "4000", "orphan"]), "off");
    assert_eq!(runner.finish().code(), Some(0));
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "changed-group child survived parent exit"
    );
    let member = regrouped_descendant(&runner);
    takt_test_fixture::assert_process_stopped(member);
}

#[test]
fn second_interrupt_escalates_before_long_stop_grace() {
    let mut task = fixture(&["ignore", "10000"]);
    task["stop_grace"] = json!("5s");
    let mut runner = Harness::start(task, "off");
    runner.await_log(|logs| logs.contains("PID "));
    let started = Instant::now();
    runner.signal(libc::SIGINT);
    // Standard signals can coalesce; allow delivery of the first before the next.
    std::thread::sleep(Duration::from_millis(100));
    runner.signal(libc::SIGINT);
    assert_eq!(runner.finish().code(), Some(130));
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "second interrupt did not bypass the five-second grace"
    );
}

#[cfg(target_os = "linux")]
fn loader_constructor_proof(enforcement: &str) {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("loader.c");
    let library = directory.path().join("loader.so");
    let evidence = directory.path().join("evidence");
    std::fs::write(
        &source,
        r#"
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
__attribute__((constructor)) static void record(void) {
    const char *path = getenv("TAKT_LOADER_EVIDENCE");
    if (!path) _exit(90);
    int out = open(path, O_WRONLY | O_APPEND | O_CREAT, 0600);
    int in = open("/proc/self/cgroup", O_RDONLY);
    if (out < 0 || in < 0) _exit(91);
    char text[4096]; int count = read(in, text, sizeof(text));
    dprintf(out, "PID %d\n", getpid());
    if (count > 0) write(out, text, count);
    close(in); close(out);
}
"#,
    )
    .unwrap();
    let status = Command::new("cc")
        .args(["-shared", "-fPIC", "-O2", "-o"])
        .arg(&library)
        .arg(&source)
        .status()
        .expect("cc is required for explicit loader proof");
    assert!(status.success());
    let mut runner = Harness::start(
        json!({"command":["/bin/true"],"env":{"LD_PRELOAD":library,"TAKT_LOADER_EVIDENCE":evidence}}),
        enforcement,
    );
    assert_eq!(
        runner.finish().code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&runner.bytes)
    );
    let proof = std::fs::read_to_string(evidence).unwrap();
    assert_eq!(
        proof
            .lines()
            .filter(|line| line.starts_with("PID "))
            .count(),
        1,
        "task loader ran in the pre-attachment helper"
    );
    assert_eq!(
        proof.lines().next().unwrap(),
        format!("PID {}", runner.task_groups[0])
    );
    if enforcement == "required" {
        assert!(
            proof.contains("/takt-") && proof.contains("/workload/task-0"),
            "constructor ran outside task containment: {proof}"
        );
        eprintln!("loader constructor evidence:\n{proof}");
    }
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "explicit compiler-backed loader injection proof; run separately"]
fn task_loader_environment_is_applied_only_after_launcher_handshake() {
    loader_constructor_proof("off");
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires an independently bounded delegated Linux cgroup safety unit"]
fn task_loader_constructor_runs_inside_required_containment() {
    assert_eq!(std::env::var("TAKT_ISOLATED_SAFETY").as_deref(), Ok("1"));
    loader_constructor_proof("required");
}
