//! Bounded rendering kept off the supervision thread.
use base64::Engine;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::{FromRawFd, RawFd};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const LOG_QUEUE_BYTES: usize = 1024 * 1024;
const CONTROL_QUEUE_BYTES: usize = 8 * 1024 * 1024;
const SPOOL_LIMIT: u64 = 64 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Auto,
    Plain,
    Grouped,
    Json,
}
impl Mode {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "auto" => Ok(Self::Auto),
            "plain" => Ok(Self::Plain),
            "grouped" => Ok(Self::Grouped),
            "json" => Ok(Self::Json),
            _ => Err(format!("invalid output mode: {s}")),
        }
    }
}
struct Message {
    event: Value,
    cost: usize,
    control_cost: usize,
}
struct Queue {
    items: std::collections::VecDeque<Message>,
    log_bytes: usize,
    control_bytes: usize,
    closed: bool,
}
struct Shared {
    queue: Mutex<Queue>,
    wake: Condvar,
    failed: AtomicBool,
}

pub struct Output {
    shared: Arc<Shared>,
    done: mpsc::Receiver<()>,
    started: Instant,
    run_id: String,
    dropped: BTreeMap<(String, String), u64>,
    spool_dir: PathBuf,
    spool_owned: bool,
}

fn owned_fd(fd: RawFd) -> io::Result<File> {
    // SAFETY: duplicate the inherited descriptor, transferring only the new fd to File.
    let duplicate = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 3) };
    if duplicate < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: duplicate is a uniquely owned open descriptor.
    Ok(unsafe { File::from_raw_fd(duplicate) })
}

impl Output {
    pub fn new(mut mode: Mode) -> io::Result<Self> {
        let stdout = owned_fd(1)?;
        let stderr = owned_fd(2)?;
        let run_id = format!(
            "{:x}-{:x}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );
        let spool_dir = std::env::temp_dir().join(format!("takt-{run_id}"));
        let mut spool_warning = None;
        let mut spool_owned = false;
        if mode == Mode::Grouped {
            use std::os::unix::fs::DirBuilderExt;
            if let Err(error) = std::fs::DirBuilder::new().mode(0o700).create(&spool_dir) {
                mode = Mode::Plain;
                spool_warning = Some(format!(
                    "grouped output unavailable: {error}; using plain output"
                ));
            } else {
                spool_owned = true;
            }
        }
        let shared = Arc::new(Shared {
            queue: Mutex::new(Queue {
                items: Default::default(),
                log_bytes: 0,
                control_bytes: 0,
                closed: false,
            }),
            wake: Condvar::new(),
            failed: AtomicBool::new(false),
        });
        let worker_shared = shared.clone();
        let worker_dir = spool_dir.clone();
        let (done_tx, done) = mpsc::channel();
        let worker = std::thread::Builder::new()
            .name("takt-output".into())
            .spawn(move || {
                let result = render(worker_shared.clone(), mode, stdout, stderr, worker_dir);
                if result.is_err() {
                    worker_shared.failed.store(true, Ordering::Release);
                }
                let _ = done_tx.send(());
            });
        if let Err(error) = worker {
            if spool_owned {
                let _ = std::fs::remove_dir_all(&spool_dir);
            }
            return Err(error);
        }
        let mut output = Self {
            shared,
            done,
            started: Instant::now(),
            run_id,
            dropped: BTreeMap::new(),
            spool_dir,
            spool_owned,
        };
        if let Some(message) = spool_warning {
            output.emit("warning", None, json!({"message": message}));
        }
        Ok(output)
    }
    pub fn emit(&mut self, event: &str, task_id: Option<&str>, mut fields: Value) {
        if !fields.is_object() {
            fields = json!({"detail":fields});
        }
        let object = fields.as_object_mut().expect("object created above");
        object.insert("schema_version".into(), json!(1));
        object.insert("run_id".into(), json!(self.run_id));
        object.insert("event".into(), json!(event));
        object.insert(
            "elapsed_ms".into(),
            json!(self.started.elapsed().as_millis() as u64),
        );
        if let Some(id) = task_id {
            object.insert("task_id".into(), json!(id));
        }
        let mut queue = self.shared.queue.lock().unwrap_or_else(|p| p.into_inner());
        let control_cost = fields
            .to_string()
            .len()
            .saturating_mul(2)
            .saturating_add(256);
        if queue.control_bytes.saturating_add(control_cost) > CONTROL_QUEUE_BYTES {
            self.shared.failed.store(true, Ordering::Release);
            return;
        }
        if !queue.closed {
            queue.control_bytes += control_cost;
            queue.items.push_back(Message {
                event: fields,
                cost: 0,
                control_cost,
            });
            self.shared.wake.notify_one();
        }
    }
    pub fn log(&mut self, task: &str, stream: &str, data: &[u8]) {
        let cost = data.len().saturating_mul(2).saturating_add(512);
        {
            let mut queue = self.shared.queue.lock().unwrap_or_else(|p| p.into_inner());
            if queue.closed || queue.log_bytes.saturating_add(cost) > LOG_QUEUE_BYTES {
                *self
                    .dropped
                    .entry((task.into(), stream.into()))
                    .or_default() += data.len() as u64;
                return;
            }
            queue.log_bytes += cost;
        }
        let (encoding, payload) = match std::str::from_utf8(data) {
            Ok(s) => ("utf8", s.to_owned()),
            Err(_) => (
                "base64",
                base64::engine::general_purpose::STANDARD.encode(data),
            ),
        };
        let event = json!({"schema_version":1,"run_id":self.run_id,"event":"log","elapsed_ms":self.started.elapsed().as_millis() as u64,"task_id":task,"stream":stream,"encoding":encoding,"data":payload});
        let mut queue = self.shared.queue.lock().unwrap_or_else(|p| p.into_inner());
        queue.items.push_back(Message {
            event,
            cost,
            control_cost: 0,
        });
        self.shared.wake.notify_one();
    }
    pub fn failed(&self) -> bool {
        self.shared.failed.load(Ordering::Acquire)
    }
    pub fn flush_truncations(&mut self) {
        for ((task, stream), bytes) in std::mem::take(&mut self.dropped) {
            self.emit(
                "log_truncated",
                Some(&task),
                json!({"stream":stream,"bytes":bytes,"reason":"output_queue_full"}),
            );
        }
    }
    pub fn finish(mut self) -> bool {
        self.flush_truncations();
        {
            let mut q = self.shared.queue.lock().unwrap_or_else(|p| p.into_inner());
            q.closed = true;
            self.shared.wake.notify_one();
        }
        let delivered = self.done.recv_timeout(Duration::from_secs(1)).is_ok() && !self.failed();
        // Unlink owned files even when the writer is blocked. Never await its destructor.
        if self.spool_owned {
            let _ = std::fs::remove_dir_all(&self.spool_dir);
        }
        delivered
    }
}
impl Drop for Output {
    fn drop(&mut self) {
        if let Ok(mut queue) = self.shared.queue.lock() {
            queue.closed = true;
            self.shared.wake.notify_one();
        }
        if self.spool_owned {
            let _ = std::fs::remove_dir_all(&self.spool_dir);
        }
    }
}

struct Spool {
    file: File,
    state: SpoolState,
    path: PathBuf,
}

#[derive(Default)]
struct SpoolState {
    bytes: u64,
    truncated: u64,
    failed: bool,
}

impl SpoolState {
    /// Account for every successful write, including a prefix before an error.
    /// Returning an error once lets the caller warn without retrying a failed spool.
    fn append(&mut self, writer: &mut impl Write, data: &[u8], available: u64) -> io::Result<()> {
        if self.failed {
            self.truncated = self.truncated.saturating_add(data.len() as u64);
            return Ok(());
        }
        let keep = data
            .len()
            .min(usize::try_from(available).unwrap_or(usize::MAX));
        let mut written = 0;
        while written < keep {
            let error = match writer.write(&data[written..keep]) {
                Ok(0) => io::Error::new(
                    io::ErrorKind::WriteZero,
                    "grouped spool stopped accepting bytes",
                ),
                Ok(count) => {
                    written += count;
                    self.bytes += count as u64;
                    continue;
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => error,
            };
            self.failed = true;
            self.truncated = self.truncated.saturating_add((data.len() - written) as u64);
            return Err(error);
        }
        self.truncated = self.truncated.saturating_add((data.len() - written) as u64);
        Ok(())
    }
}
fn render(
    shared: Arc<Shared>,
    mode: Mode,
    stdout: File,
    stderr: File,
    dir: PathBuf,
) -> io::Result<()> {
    render_with_limit(shared, mode, stdout, stderr, dir, SPOOL_LIMIT)
}

fn render_with_limit(
    shared: Arc<Shared>,
    mode: Mode,
    mut stdout: File,
    mut stderr: File,
    dir: PathBuf,
    spool_limit: u64,
) -> io::Result<()> {
    let mut seq = 0u64;
    let mut spools: BTreeMap<(String, String), Spool> = BTreeMap::new();
    loop {
        let message = {
            let mut q = shared.queue.lock().unwrap_or_else(|p| p.into_inner());
            while q.items.is_empty() && !q.closed {
                q = shared.wake.wait(q).unwrap_or_else(|p| p.into_inner());
            }
            match q.items.pop_front() {
                Some(m) => {
                    q.log_bytes = q.log_bytes.saturating_sub(m.cost);
                    q.control_bytes = q.control_bytes.saturating_sub(m.control_cost);
                    Some(m)
                }
                None => None,
            }
        };
        let Some(mut message) = message else { break };
        seq += 1;
        message.event["sequence"] = json!(seq);
        if mode == Mode::Json {
            serde_json::to_writer(&mut stdout, &message.event)?;
            stdout.write_all(b"\n")?;
            continue;
        }
        let event = message.event["event"].as_str().unwrap_or("event");
        let task = message.event["task_id"].as_str().unwrap_or("takt");
        if event == "log" {
            let stream = message.event["stream"].as_str().unwrap_or("stdout");
            let encoded = message.event["data"].as_str().unwrap_or_default();
            let bytes = if message.event["encoding"] == "base64" {
                base64::engine::general_purpose::STANDARD
                    .decode(encoded)
                    .unwrap_or_default()
            } else {
                encoded.as_bytes().to_vec()
            };
            if mode == Mode::Grouped {
                let key = (task.to_owned(), stream.to_owned());
                if !spools.contains_key(&key) {
                    let path = dir.join(format!("{}-{}.log", task, stream));
                    use std::os::unix::fs::OpenOptionsExt;
                    match OpenOptions::new()
                        .create_new(true)
                        .read(true)
                        .write(true)
                        .mode(0o600)
                        .open(&path)
                    {
                        Ok(file) => {
                            spools.insert(
                                key.clone(),
                                Spool {
                                    file,
                                    state: SpoolState::default(),
                                    path,
                                },
                            );
                        }
                        Err(e) => {
                            writeln!(
                                stderr,
                                "[takt] grouped output unavailable: {e}; using plain output"
                            )?;
                            write_log(
                                if stream == "stderr" {
                                    &mut stderr
                                } else {
                                    &mut stdout
                                },
                                task,
                                &bytes,
                            )?;
                            continue;
                        }
                    }
                }
                let total_for_task: u64 = spools
                    .iter()
                    .filter(|((id, _), _)| id == task)
                    .map(|(_, s)| s.state.bytes)
                    .sum();
                let spool = spools.get_mut(&key).expect("spool inserted");
                if let Err(e) = spool.state.append(
                    &mut spool.file,
                    &bytes,
                    spool_limit.saturating_sub(total_for_task),
                ) {
                    writeln!(
                        stderr,
                        "[takt] grouped spool failed: {e}; remaining bytes truncated"
                    )?;
                }
            } else {
                write_log(
                    if stream == "stderr" {
                        &mut stderr
                    } else {
                        &mut stdout
                    },
                    task,
                    &bytes,
                )?;
            }
        } else if event == "run_started" {
            let budget = &message.event["budget"];
            let cpu = budget["cpu_millicores"].as_u64().unwrap_or(0) as f64 / 1000.0;
            let memory = budget["memory_bytes"]
                .as_u64()
                .map(|bytes| format!("{:.1} MiB", bytes as f64 / 1_048_576.0))
                .unwrap_or_else(|| "unknown".into());
            writeln!(
                stderr,
                "[takt] budget: {} jobs, {cpu:.3} CPU cores, {memory} memory",
                budget["jobs"]
            )?;
        } else if event == "capabilities" {
            let capabilities = &message.event["capabilities"];
            writeln!(
                stderr,
                "[takt] protection: CPU {}; memory {}; processes {}",
                capabilities["cpu"].as_str().unwrap_or("unknown"),
                capabilities["aggregate_memory"]
                    .as_str()
                    .unwrap_or("unknown"),
                capabilities["process_tree_containment"]
                    .as_str()
                    .unwrap_or("unknown")
            )?;
            if let Some(details) = capabilities["details"].as_array() {
                for detail in details.iter().filter_map(Value::as_str) {
                    writeln!(stderr, "[takt] {detail}")?;
                }
            }
        } else if event == "task_finished" {
            if mode == Mode::Grouped {
                flush_task(task, &mut spools, &mut stdout, &mut stderr)?;
            }
            writeln!(
                stderr,
                "[{task}] {}: {}{}",
                message.event["kind"].as_str().unwrap_or("finished"),
                message.event["reason"].as_str().unwrap_or("finished"),
                message
                    .event
                    .get("exit_code")
                    .filter(|v| !v.is_null())
                    .map(|v| format!(" (exit {v})"))
                    .unwrap_or_default()
            )?;
        } else if event == "summary" {
            let tasks: Vec<String> = spools.keys().map(|(id, _)| id.clone()).collect();
            for id in tasks {
                flush_task(&id, &mut spools, &mut stdout, &mut stderr)?;
            }
            writeln!(
                stderr,
                "[takt] finished (exit {}): {}",
                message.event["exit_code"],
                message.event["counts"]
                    .as_object()
                    .map(|counts| counts
                        .iter()
                        .map(|(kind, count)| format!("{kind}={count}"))
                        .collect::<Vec<_>>()
                        .join(", "))
                    .unwrap_or_default()
            )?;
        } else if matches!(
            event,
            "error" | "warning" | "log_truncated" | "queue" | "task_ready"
        ) {
            writeln!(
                stderr,
                "[{task}] {event}: {}",
                message
                    .event
                    .get("message")
                    .or_else(|| message.event.get("reason"))
                    .unwrap_or(&Value::Null)
            )?;
        }
    }
    stdout.flush()?;
    stderr.flush()?;
    Ok(())
}
fn write_log(out: &mut File, task: &str, bytes: &[u8]) -> io::Result<()> {
    // Prefix bounded chunks; preserving bytes and stream identity does not depend on newlines.
    write!(out, "[{task}] ")?;
    out.write_all(bytes)
}
fn flush_task(
    task: &str,
    spools: &mut BTreeMap<(String, String), Spool>,
    stdout: &mut File,
    stderr: &mut File,
) -> io::Result<()> {
    use std::io::{Seek, SeekFrom};
    for stream in ["stdout", "stderr"] {
        if let Some(mut spool) = spools.remove(&(task.to_owned(), stream.to_owned())) {
            let out = if stream == "stderr" {
                &mut *stderr
            } else {
                &mut *stdout
            };
            writeln!(out, "--- {task} ({stream}) ---")?;
            spool.file.seek(SeekFrom::Start(0))?;
            let mut buf = [0u8; 8192];
            loop {
                let n = spool.file.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                out.write_all(&buf[..n])?;
            }
            if spool.state.truncated > 0 {
                writeln!(out, "\n[takt: {} bytes truncated]", spool.state.truncated)?;
            }
            let _ = std::fs::remove_file(spool.path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Seek, SeekFrom};

    struct PartialDisk {
        retained: Vec<u8>,
        fail_after: Option<usize>,
        calls: usize,
    }

    impl Write for PartialDisk {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.calls += 1;
            let count = if let Some(limit) = self.fail_after {
                if self.retained.len() >= limit {
                    return Err(io::Error::from_raw_os_error(libc::ENOSPC));
                }
                bytes.len().min(limit - self.retained.len())
            } else {
                bytes.len()
            };
            self.retained.extend_from_slice(&bytes[..count]);
            Ok(count)
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn partial_spool_failure_counts_retained_bytes_and_never_retries() {
        let mut disk = PartialDisk {
            retained: Vec::new(),
            fail_after: Some(3),
            calls: 0,
        };
        let mut spool = SpoolState::default();
        let error = spool.append(&mut disk, b"abcdefgh", 6).unwrap_err();
        assert_eq!(error.raw_os_error(), Some(libc::ENOSPC));
        assert_eq!(spool.bytes, 3);
        assert_eq!(spool.truncated, 5);
        assert!(spool.failed);
        assert_eq!(disk.retained, b"abc");

        // A recovering disk must not receive retries, exceed the shared task
        // limit, or cause another warning after this spool has failed.
        disk.fail_after = None;
        let calls_at_failure = disk.calls;
        assert!(spool.append(&mut disk, b"ijkl", 3).is_ok());
        assert!(spool.append(&mut disk, b"mn", 3).is_ok());
        assert_eq!(disk.calls, calls_at_failure);
        assert_eq!(spool.bytes, 3);
        assert_eq!(spool.truncated, 11);
        assert_eq!(disk.retained, b"abc");

        let mut other_stream = SpoolState::default();
        let mut other_disk = Vec::new();
        other_stream
            .append(&mut other_disk, b"12345", 6 - spool.bytes)
            .unwrap();
        assert_eq!(spool.bytes + other_stream.bytes, 6);
        assert_eq!(other_disk, b"123");
        assert_eq!(other_stream.truncated, 2);
    }

    #[test]
    fn zero_length_write_disables_spool_and_accounts_for_all_discarded_bytes() {
        let mut writer = io::Cursor::new([0u8; 2]);
        let mut spool = SpoolState::default();
        let error = spool.append(&mut writer, b"abcdef", 6).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::WriteZero);
        assert_eq!(spool.bytes, 2);
        assert_eq!(spool.truncated, 4);
        assert!(spool.append(&mut writer, b"gh", 4).is_ok());
        assert_eq!(spool.truncated, 6);
        assert_eq!(writer.into_inner(), *b"ab");
    }

    #[test]
    fn grouped_render_shares_one_task_cap_across_both_streams() {
        let directory = tempfile::tempdir().unwrap();
        let mut stdout = tempfile::tempfile().unwrap();
        let mut stderr = tempfile::tempfile().unwrap();
        let log = |task: &str, stream: &str, data: &str| {
            json!({"event":"log", "task_id":task, "stream":stream,
                   "encoding":"utf8", "data":data})
        };
        let events = [
            log("alpha", "stdout", "abcde"),
            log("alpha", "stderr", "WXYZ"),
            log("alpha", "stdout", "FGHI"),
            log("alpha", "stderr", "JK"),
            log("beta", "stdout", "123456"),
            json!({"event":"task_finished", "task_id":"alpha"}),
            json!({"event":"task_finished", "task_id":"beta"}),
        ];
        let shared = Arc::new(Shared {
            queue: Mutex::new(Queue {
                items: events
                    .into_iter()
                    .map(|event| Message {
                        event,
                        cost: 0,
                        control_cost: 0,
                    })
                    .collect(),
                log_bytes: 0,
                control_bytes: 0,
                closed: true,
            }),
            wake: Condvar::new(),
            failed: AtomicBool::new(false),
        });
        render_with_limit(
            shared,
            Mode::Grouped,
            stdout.try_clone().unwrap(),
            stderr.try_clone().unwrap(),
            directory.path().into(),
            10,
        )
        .unwrap();
        stdout.seek(SeekFrom::Start(0)).unwrap();
        stderr.seek(SeekFrom::Start(0)).unwrap();
        let mut out = String::new();
        let mut err = String::new();
        stdout.read_to_string(&mut out).unwrap();
        stderr.read_to_string(&mut err).unwrap();
        assert!(out.contains("abcdeF\n[takt: 3 bytes truncated]"), "{out}");
        assert!(err.contains("WXYZ\n[takt: 2 bytes truncated]"), "{err}");
        assert!(out.contains("--- beta (stdout) ---\n123456"), "{out}");
        assert_eq!(out.matches("bytes truncated").count(), 1);
        assert_eq!(err.matches("bytes truncated").count(), 1);
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
    }
}
