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
        if mode == Mode::Grouped {
            use std::os::unix::fs::DirBuilderExt;
            if let Err(error) = std::fs::DirBuilder::new().mode(0o700).create(&spool_dir) {
                mode = Mode::Plain;
                spool_warning = Some(format!(
                    "grouped output unavailable: {error}; using plain output"
                ));
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
        std::thread::Builder::new()
            .name("takt-output".into())
            .spawn(move || {
                let result = render(worker_shared.clone(), mode, stdout, stderr, worker_dir);
                if result.is_err() {
                    worker_shared.failed.store(true, Ordering::Release);
                }
                let _ = done_tx.send(());
            })?;
        let mut output = Self {
            shared,
            done,
            started: Instant::now(),
            run_id,
            dropped: BTreeMap::new(),
            spool_dir,
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
        let _ = std::fs::remove_dir_all(&self.spool_dir);
        delivered
    }
}
impl Drop for Output {
    fn drop(&mut self) {
        if let Ok(mut queue) = self.shared.queue.lock() {
            queue.closed = true;
            self.shared.wake.notify_one();
        }
        let _ = std::fs::remove_dir_all(&self.spool_dir);
    }
}

struct Spool {
    file: File,
    bytes: u64,
    truncated: u64,
    path: PathBuf,
}
fn render(
    shared: Arc<Shared>,
    mode: Mode,
    mut stdout: File,
    mut stderr: File,
    dir: PathBuf,
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
                                    bytes: 0,
                                    truncated: 0,
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
                    .map(|(_, s)| s.bytes)
                    .sum();
                let spool = spools.get_mut(&key).expect("spool inserted");
                let keep = bytes
                    .len()
                    .min(SPOOL_LIMIT.saturating_sub(total_for_task) as usize);
                if let Err(e) = spool.file.write_all(&bytes[..keep]) {
                    writeln!(
                        stderr,
                        "[takt] grouped spool failed: {e}; remaining bytes truncated"
                    )?;
                    spool.truncated += bytes.len() as u64;
                } else {
                    spool.bytes += keep as u64;
                    spool.truncated += (bytes.len() - keep) as u64;
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
        } else if event == "task_finished" {
            if mode == Mode::Grouped {
                flush_task(task, &mut spools, &mut stdout, &mut stderr)?;
            }
            writeln!(
                stderr,
                "[{task}] {}{}",
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
                "[takt] finished (exit {})",
                message.event["exit_code"]
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
            if spool.truncated > 0 {
                writeln!(out, "\n[takt: {} bytes truncated]", spool.truncated)?;
            }
            let _ = std::fs::remove_file(spool.path);
        }
    }
    Ok(())
}
