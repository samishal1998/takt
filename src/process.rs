//! Gated same-executable launch and identity-preserving Unix process supervision.
use crate::platform::TaskCgroup;
use serde::{Deserialize, Serialize};
use std::ffi::{OsStr, OsString};
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, RawFd};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, ChildStderr, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

mod observation;
pub(crate) use observation::ProcessTable;
use observation::ProcessTree;

const HELPER_FLAG: &str = "--__takt-launch";
const MAGIC: &[u8; 8] = b"TAKT\0\x01\r\n";
const MAX_LAUNCH_BYTES: usize = 1024 * 1024;
const LAUNCH_TIMEOUT: Duration = Duration::from_secs(5);
const LAUNCH_IO_BUDGET: usize = 64 * 1024;

enum LaunchPhase {
    Hello(usize),
    Payload(usize),
    Exec(usize),
    ExecResult,
}

struct LaunchHandshake {
    control: UnixStream,
    phase: LaunchPhase,
    payload: Vec<u8>,
    handshake: [u8; 8],
    response: Vec<u8>,
    deadline: Instant,
}

impl LaunchHandshake {
    fn new(control: UnixStream, encoded: Vec<u8>, started: Instant) -> Self {
        let mut payload = Vec::with_capacity(encoded.len() + 4);
        payload.extend_from_slice(&(encoded.len() as u32).to_be_bytes());
        payload.extend_from_slice(&encoded);
        Self {
            control,
            phase: LaunchPhase::Hello(0),
            payload,
            handshake: [0; 8],
            response: Vec::new(),
            deadline: started + LAUNCH_TIMEOUT,
        }
    }

    fn poll_at(&mut self, pid: u32, group: Option<&TaskCgroup>, now: Instant) -> io::Result<bool> {
        if now >= self.deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "launcher exceeded its five-second deadline",
            ));
        }
        let mut remaining = LAUNCH_IO_BUDGET;
        // Bound both bytes and syscalls: even a peer making tiny progress cannot
        // monopolize the supervisor's iteration or reset the absolute deadline.
        for _ in 0..16 {
            if remaining == 0 {
                break;
            }
            let operation = match self.phase {
                LaunchPhase::Hello(offset) => {
                    let end = (offset + remaining).min(MAGIC.len());
                    self.control.read(&mut self.handshake[offset..end])
                }
                LaunchPhase::Payload(offset) => {
                    let end = (offset + remaining).min(self.payload.len());
                    self.control.write(&self.payload[offset..end])
                }
                LaunchPhase::Exec(offset) => {
                    let end = (offset + remaining).min(4);
                    self.control.read(&mut self.handshake[offset..end])
                }
                LaunchPhase::ExecResult => {
                    let mut bytes = [0u8; 4097];
                    match self.control.read(&mut bytes[..remaining.min(4097)]) {
                        Ok(0) if self.response.is_empty() => return Ok(true),
                        Ok(0) => {
                            return Err(io::Error::other(format!(
                                "launch failed: {}",
                                String::from_utf8_lossy(&self.response)
                            )));
                        }
                        Ok(count) => {
                            if self.response.len() + count > 4096 {
                                return Err(io::Error::other(
                                    "launcher error response exceeds limit",
                                ));
                            }
                            self.response.extend_from_slice(&bytes[..count]);
                            Ok(count)
                        }
                        Err(error) => Err(error),
                    }
                }
            };
            let count = match operation {
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "launcher ended before completing its handshake",
                    ));
                }
                Ok(count) => count,
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) =>
                {
                    return Ok(false);
                }
                Err(error) => return Err(error),
            };
            remaining -= count;
            self.phase = match self.phase {
                LaunchPhase::Hello(offset) if offset + count == MAGIC.len() => {
                    if &self.handshake != MAGIC {
                        return Err(io::Error::other("invalid launcher handshake"));
                    }
                    if let Some(group) = group {
                        group.attach(pid)?;
                    }
                    LaunchPhase::Payload(0)
                }
                LaunchPhase::Hello(offset) => LaunchPhase::Hello(offset + count),
                LaunchPhase::Payload(offset) if offset + count == self.payload.len() => {
                    LaunchPhase::Exec(0)
                }
                LaunchPhase::Payload(offset) => LaunchPhase::Payload(offset + count),
                LaunchPhase::Exec(offset) if offset + count == 4 => {
                    if &self.handshake[..4] != b"EXEC" {
                        return Err(io::Error::other("launcher rejected execution payload"));
                    }
                    LaunchPhase::ExecResult
                }
                LaunchPhase::Exec(offset) => LaunchPhase::Exec(offset + count),
                LaunchPhase::ExecResult => LaunchPhase::ExecResult,
            };
        }
        Ok(false)
    }
}

#[derive(Debug)]
struct LaunchCleanupIncomplete(String);

impl std::fmt::Display for LaunchCleanupIncomplete {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for LaunchCleanupIncomplete {}

pub fn launch_cleanup_incomplete(error: &io::Error) -> bool {
    error
        .get_ref()
        .is_some_and(|inner| inner.is::<LaunchCleanupIncomplete>())
}

#[derive(Clone, Debug)]
pub struct LaunchSpec {
    pub program: OsString,
    pub args: Vec<OsString>,
    pub env: Vec<(OsString, OsString)>,
    pub cwd: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ProcessExit {
    pub code: Option<i32>,
    pub signal: Option<i32>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireLaunch {
    version: u8,
    program: Vec<u8>,
    args: Vec<Vec<u8>>,
    env: Vec<(Vec<u8>, Vec<u8>)>,
    cwd: Vec<u8>,
}

impl From<&LaunchSpec> for WireLaunch {
    fn from(spec: &LaunchSpec) -> Self {
        Self {
            version: 1,
            program: spec.program.as_bytes().to_vec(),
            args: spec.args.iter().map(|s| s.as_bytes().to_vec()).collect(),
            env: spec
                .env
                .iter()
                .map(|(key, value)| (key.as_bytes().to_vec(), value.as_bytes().to_vec()))
                .collect(),
            cwd: spec.cwd.as_os_str().as_bytes().to_vec(),
        }
    }
}

impl WireLaunch {
    fn validate(&self) -> io::Result<()> {
        if self.version != 1
            || self.program.is_empty()
            || self.cwd.is_empty()
            || self.program.contains(&0)
            || self.cwd.contains(&0)
            || self.args.iter().any(|arg| arg.contains(&0))
            || self.env.iter().any(|(key, value)| {
                key.is_empty() || key.contains(&0) || key.contains(&b'=') || value.contains(&0)
            })
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid launch protocol",
            ));
        }
        Ok(())
    }
}

pub struct ManagedChild {
    child: Child,
    pub pid: u32,
    pub pgid: i32,
    pub stdout: Option<ChildStdout>,
    pub stderr: Option<ChildStderr>,
    group: Option<TaskCgroup>,
    exit: Option<ProcessExit>,
    reaped: bool,
    launch: Option<LaunchHandshake>,
    launch_confirmed: bool,
    tree: ProcessTree,
    fallback_signal: Option<i32>,
    observe_descendants: bool,
}

impl ManagedChild {
    /// Start the neutral helper without waiting for its private launch protocol.
    /// Native self-exec and controller syscalls remain subject to OS I/O stalls.
    pub fn start(spec: &LaunchSpec, group: Option<TaskCgroup>) -> io::Result<Self> {
        let started = Instant::now();
        let wire = WireLaunch::from(spec);
        wire.validate()?;
        let encoded = serde_json::to_vec(&wire).map_err(io::Error::other)?;
        if encoded.len() > MAX_LAUNCH_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "launch specification exceeds 1 MiB",
            ));
        }
        let (parent_control, child_control) = UnixStream::pair()?;
        parent_control.set_nonblocking(true)?;
        let control_fd = child_control.as_raw_fd();
        let mut command = Command::new(std::env::current_exe()?);
        command
            .arg(HELPER_FLAG)
            .arg(control_fd.to_string())
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0);
        // SAFETY: the hook performs only the async-signal-safe fcntl syscall. No
        // allocation, locks, environment access, or project code executes here.
        unsafe {
            command.pre_exec(move || {
                if libc::fcntl(control_fd, libc::F_SETFD, 0) == -1 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut child = command.spawn()?;
        drop(child_control);
        let pid = child.id();
        // CPU-only cgroups still need sampled RSS, while their population/kill
        // interfaces remain authoritative for cleanup.
        let observe_descendants = group
            .as_ref()
            .is_none_or(|group| group.memory_usage().is_none());
        let mut managed = Self {
            pid,
            pgid: pid as i32,
            stdout: child.stdout.take(),
            stderr: child.stderr.take(),
            child,
            group,
            exit: None,
            reaped: false,
            launch: Some(LaunchHandshake::new(parent_control, encoded, started)),
            launch_confirmed: false,
            tree: ProcessTree::default(),
            fallback_signal: None,
            observe_descendants,
        };
        let capture: io::Result<()> = (|| {
            if let Some(stdout) = &managed.stdout {
                set_nonblocking(stdout.as_raw_fd())?;
            }
            if let Some(stderr) = &managed.stderr {
                set_nonblocking(stderr.as_raw_fd())?;
            }
            Ok(())
        })();
        if let Err(error) = capture {
            let _ = managed.signal(libc::SIGKILL);
            if managed.child.try_wait()?.is_some() {
                managed.reaped = true;
            }
            if !managed.reaped {
                return Err(io::Error::other(LaunchCleanupIncomplete(format!(
                    "{error}; launcher PID {} did not terminate after SIGKILL; cleanup incomplete",
                    managed.pid
                ))));
            }
            return Err(error);
        }
        Ok(managed)
    }

    pub fn launch_pending(&self) -> bool {
        self.launch.is_some()
    }

    pub fn launch_fd(&self) -> Option<RawFd> {
        self.launch
            .as_ref()
            .map(|launch| launch.control.as_raw_fd())
    }

    /// Advance bounded nonblocking launch I/O. Errors leave the owned process
    /// available for the runtime's ordinary signal, observation, and cleanup path.
    pub fn poll_launch(&mut self) -> io::Result<bool> {
        let Some(launch) = &mut self.launch else {
            return if self.launch_confirmed {
                Ok(true)
            } else {
                Err(io::Error::other("launcher already failed"))
            };
        };
        let result = launch.poll_at(self.pid, self.group.as_ref(), Instant::now());
        match result {
            Ok(true) => {
                self.launch_confirmed = true;
                self.launch = None;
            }
            Err(_) => self.launch = None,
            Ok(false) => {}
        }
        result
    }

    /// Observe but do not reap the leader, keeping its process-group ID reserved.
    pub fn poll_exit(&mut self) -> io::Result<Option<ProcessExit>> {
        if self.exit.is_some() {
            return Ok(self.exit);
        }
        // SAFETY: siginfo_t is an output buffer; all-zero is valid initialization.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        // SAFETY: valid child ID and writable siginfo_t; WNOWAIT retains identity.
        let result = unsafe {
            libc::waitid(
                libc::P_PID,
                self.pid as libc::id_t,
                &mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        if result == -1 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                return Ok(None);
            }
            return Err(error);
        }
        // SAFETY: waitid initialized the process-status fields of siginfo_t.
        let observed_pid = unsafe { info.si_pid() };
        if observed_pid == 0 {
            return Ok(None);
        }
        // SAFETY: a nonzero si_pid from WEXITED identifies the status union.
        let status = unsafe { info.si_status() };
        self.exit = Some(if info.si_code == libc::CLD_EXITED {
            ProcessExit {
                code: Some(status),
                signal: None,
            }
        } else {
            ProcessExit {
                code: None,
                signal: Some(status),
            }
        });
        Ok(self.exit)
    }

    pub fn signal(&mut self, signal: i32) -> io::Result<()> {
        if self.reaped {
            return Ok(());
        }
        // A cancellation action can arrive after a second interrupt already
        // forced termination. Never weaken cleanup of later-discovered children.
        let signal = if self.fallback_signal == Some(libc::SIGKILL) {
            libc::SIGKILL
        } else {
            signal
        };
        if self.group.is_none() {
            // Discover before signalling the parent, while PPID edges still exist.
            // Even a failed scan must not prevent signalling previously known IDs.
            let _ = self.tree.refresh(self.pid as i32, self.pgid);
            self.fallback_signal = Some(signal);
        }
        let mut error = if signal == libc::SIGKILL {
            self.group.as_ref().and_then(|group| group.kill().err())
        } else {
            None
        };
        // SAFETY: pgid belongs to the unreaped child; negative PID targets only
        // its process group. The unreaped leader prevents process-ID recycling.
        let group_signalled = unsafe { libc::kill(-self.pgid, signal) } == 0;
        if !group_signalled {
            let group_error = io::Error::last_os_error();
            if group_error.raw_os_error() != Some(libc::ESRCH) {
                error = Some(group_error);
            }
        }
        if self.group.is_none() {
            let descendants = self.tree.signal(self.pgid, signal, group_signalled);
            if error.is_none() {
                error = descendants.err();
            }
        }
        error.map_or(Ok(()), Err)
    }

    /// True only after leader exit and no live observed owned processes remain.
    /// The supervisor chooses the initial signal and grace; newly observed
    /// descendants receive that same signal while cleanup is in progress.
    pub fn cleanup_finished(&mut self) -> io::Result<bool> {
        if self.reaped {
            return Ok(true);
        }
        // The final tree observation must follow leader exit; otherwise a child
        // forked between the PID listing and leader exit could be missed at reap.
        let exit = self.poll_exit()?;
        let fallback_live = if self.group.is_none() {
            let live = self.tree.refresh(self.pid as i32, self.pgid);
            if let Some(signal) = self.fallback_signal {
                // A descendant can fork during grace or after the first kill.
                // Newly discovered identities receive the current stop signal.
                self.tree.signal(self.pgid, signal, false)?;
            }
            live?
        } else {
            false
        };
        if exit.is_none() {
            return Ok(false);
        }
        if let Some(group) = &self.group {
            if group.populated()? {
                return Ok(false);
            }
        } else if fallback_live {
            return Ok(false);
        }
        self.child.wait()?;
        self.reaped = true;
        Ok(true)
    }

    pub(crate) fn needs_process_observation(&self) -> bool {
        self.observe_descendants && !self.reaped
    }

    pub(crate) fn usage_memory(&mut self, processes: Option<&ProcessTable>) -> Option<u64> {
        if self.reaped {
            return Some(0);
        }
        if let Some(usage) = self.group.as_ref().and_then(TaskCgroup::memory_usage) {
            return Some(usage);
        }
        self.tree.memory(processes?, self.pid as i32, self.pgid)
    }

    pub fn oom_kills(&self) -> u64 {
        self.group.as_ref().map_or(0, TaskCgroup::oom_kills)
    }
}

impl Drop for ManagedChild {
    fn drop(&mut self) {
        if !self.reaped {
            // Last-resort owned cleanup on error paths. Normal teardown uses the
            // supervisor's grace period; Drop must never block on an unkillable task.
            let _ = self.signal(libc::SIGKILL);
            let _ = self.child.try_wait();
        }
    }
}

pub fn set_nonblocking(fd: RawFd) -> io::Result<()> {
    // SAFETY: fcntl accepts a descriptor integer and returns an error if invalid.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags == -1 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: preserves all flags and adds nonblocking I/O to this owned fd.
    if unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Invoke before CLI parsing or thread creation in the executable entry point.
pub fn maybe_run_helper() -> Option<i32> {
    let mut args = std::env::args_os();
    args.next();
    if args.next().as_deref() != Some(OsStr::new(HELPER_FLAG)) {
        return None;
    }
    let Some(fd) = args
        .next()
        .and_then(|value| value.to_str().and_then(|s| s.parse::<RawFd>().ok()))
    else {
        return Some(126);
    };
    if fd < 3 || args.next().is_some() {
        return Some(126);
    }
    Some(run_helper(fd).unwrap_or(126))
}

fn run_helper(fd: RawFd) -> io::Result<i32> {
    let mut kind: libc::c_int = 0;
    let mut size = std::mem::size_of_val(&kind) as libc::socklen_t;
    // SAFETY: kind and size are valid output buffers; fd is checked by getsockopt.
    if unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_TYPE,
            (&mut kind as *mut libc::c_int).cast(),
            &mut size,
        )
    } != 0
        || kind != libc::SOCK_STREAM
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "launcher requires a private stream socket",
        ));
    }
    // SAFETY: validated fd is inherited exclusively by this helper invocation.
    let mut control = unsafe { UnixStream::from_raw_fd(fd) };
    // SAFETY: owned live descriptor, setting close-on-exec is an atomic fd flag.
    if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } == -1 {
        return Err(io::Error::last_os_error());
    }
    close_inherited_descriptors(fd)?;
    control.set_read_timeout(Some(Duration::from_secs(5)))?;
    control.set_write_timeout(Some(Duration::from_secs(5)))?;
    control.write_all(MAGIC)?;
    let result = (|| {
        let mut length = [0u8; 4];
        control.read_exact(&mut length)?;
        let length = u32::from_be_bytes(length) as usize;
        if length == 0 || length > MAX_LAUNCH_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "launch frame exceeds limit",
            ));
        }
        let mut bytes = vec![0; length];
        control.read_exact(&mut bytes)?;
        let wire: WireLaunch = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
        wire.validate()?;
        let mut command = Command::new(OsString::from_vec(wire.program));
        command
            .args(wire.args.into_iter().map(OsString::from_vec))
            .env_clear()
            .envs(
                wire.env
                    .into_iter()
                    .map(|(key, value)| (OsString::from_vec(key), OsString::from_vec(value))),
            )
            .current_dir(PathBuf::from(OsString::from_vec(wire.cwd)));
        control.write_all(b"EXEC")?;
        Err::<(), _>(command.exec())
    })();
    if let Err(error) = result {
        let message = error.to_string();
        control.write_all(&message.as_bytes()[..message.len().min(4096)])?;
    }
    Ok(127)
}

fn close_inherited_descriptors(keep: RawFd) -> io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        // Enumerate before closing: do not invalidate the directory iterator's fd.
        let mut descriptors = Vec::new();
        for entry in std::fs::read_dir("/proc/self/fd")? {
            let entry = entry?;
            if let Some(fd) = entry
                .file_name()
                .to_str()
                .and_then(|name| name.parse::<RawFd>().ok())
                && fd > 2
                && fd != keep
            {
                descriptors.push(fd);
            }
            if descriptors.len() > 65536 {
                return Err(io::Error::other("too many inherited descriptors"));
            }
        }
        for fd in descriptors {
            // SAFETY: single-threaded private helper, closing unwanted inherited
            // descriptors. An iterator fd already closed simply yields EBADF.
            unsafe {
                libc::close(fd);
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        // SAFETY: getdtablesize has no preconditions. The helper is single-threaded.
        let limit = unsafe { libc::getdtablesize() };
        for fd in 3..limit {
            if fd != keep {
                // SAFETY: closing unwanted inherited descriptors; EBADF is harmless.
                unsafe {
                    libc::close(fd);
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn handshake(encoded: Vec<u8>, started: Instant) -> (LaunchHandshake, UnixStream) {
        let (parent, peer) = UnixStream::pair().unwrap();
        parent.set_nonblocking(true).unwrap();
        peer.set_nonblocking(true).unwrap();
        (LaunchHandshake::new(parent, encoded, started), peer)
    }

    #[test]
    fn stalled_launcher_poll_is_nonblocking_and_deadline_is_absolute() {
        let started = Instant::now();
        let (mut launch, mut peer) = handshake(vec![1, 2, 3], started);
        for _ in 0..100 {
            assert!(!launch.poll_at(0, None, started).unwrap());
        }
        assert!(started.elapsed() < Duration::from_secs(1));
        peer.write_all(&MAGIC[..1]).unwrap();
        assert!(
            !launch
                .poll_at(0, None, started + Duration::from_secs(4))
                .unwrap()
        );
        assert_eq!(
            launch
                .poll_at(0, None, started + LAUNCH_TIMEOUT)
                .unwrap_err()
                .kind(),
            io::ErrorKind::TimedOut
        );
    }

    #[test]
    fn cancellation_before_launch_gate_retains_identity_until_cleanup() {
        let started = Instant::now();
        let (launch, mut peer) = handshake(vec![1, 2, 3], started);
        // A bounded fixture stands in for the neutral helper, keeping this test
        // independent of the executable's CLI helper dispatch.
        let child = Command::new(takt_test_fixture::executable())
            .args(["sleep", "1000"])
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .unwrap();
        let pid = child.id();
        let mut managed = ManagedChild {
            child,
            pid,
            pgid: pid as i32,
            stdout: None,
            stderr: None,
            group: None,
            exit: None,
            reaped: false,
            launch: Some(launch),
            launch_confirmed: false,
            tree: ProcessTree::default(),
            fallback_signal: None,
            observe_descendants: true,
        };
        peer.write_all(MAGIC).unwrap();
        managed.signal(libc::SIGKILL).unwrap();
        managed.signal(libc::SIGTERM).unwrap();
        assert_eq!(managed.fallback_signal, Some(libc::SIGKILL));
        assert!(!managed.reaped);
        assert!(managed.launch_pending());
        let mut byte = [0u8; 1];
        assert_eq!(
            peer.read(&mut byte).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        let deadline = Instant::now() + Duration::from_secs(2);
        while !managed.cleanup_finished().unwrap() {
            assert!(Instant::now() < deadline, "pending helper cleanup deadline");
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(managed.reaped);
        assert_eq!(
            managed.poll_exit().unwrap().unwrap().signal,
            Some(libc::SIGKILL)
        );
    }

    #[test]
    fn launch_sends_nothing_before_hello_and_requires_exec_eof() {
        let started = Instant::now();
        let (mut launch, mut peer) = handshake(vec![1, 2, 3], started);
        peer.write_all(&MAGIC[..4]).unwrap();
        assert!(!launch.poll_at(0, None, started).unwrap());
        let mut bytes = [0u8; 7];
        assert_eq!(
            peer.read(&mut bytes).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        peer.write_all(&MAGIC[4..]).unwrap();
        assert!(!launch.poll_at(0, None, started).unwrap());
        peer.read_exact(&mut bytes).unwrap();
        assert_eq!(bytes, [0, 0, 0, 3, 1, 2, 3]);
        peer.write_all(b"EXEC").unwrap();
        assert!(!launch.poll_at(0, None, started).unwrap());
        drop(peer);
        assert!(launch.poll_at(0, None, started).unwrap());
    }

    #[test]
    fn launch_progress_has_a_per_iteration_byte_bound() {
        let started = Instant::now();
        let (mut launch, mut peer) = handshake(vec![0; MAX_LAUNCH_BYTES], started);
        peer.write_all(MAGIC).unwrap();
        assert!(!launch.poll_at(0, None, started).unwrap());
        let mut bytes = vec![0u8; LAUNCH_IO_BUDGET + 1];
        let mut total = 0;
        loop {
            match peer.read(&mut bytes) {
                Ok(count) if count > 0 => total += count,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                other => panic!("unexpected payload read: {other:?}"),
            }
        }
        assert!(total > 0 && total <= LAUNCH_IO_BUDGET - MAGIC.len());
        assert!(matches!(launch.phase, LaunchPhase::Payload(_)));
    }

    #[test]
    fn launch_preserves_exec_error_and_rejects_invalid_handshake() {
        let started = Instant::now();
        let (mut launch, mut peer) = handshake(vec![1], started);
        peer.write_all(MAGIC).unwrap();
        assert!(!launch.poll_at(0, None, started).unwrap());
        peer.read_exact(&mut [0u8; 5]).unwrap();
        peer.write_all(b"EXECmissing executable").unwrap();
        drop(peer);
        let error = launch.poll_at(0, None, started).unwrap_err();
        assert!(error.to_string().contains("missing executable"));

        let (mut launch, mut peer) = handshake(vec![1], started);
        peer.write_all(b"BADMAGIC").unwrap();
        assert!(launch.poll_at(0, None, started).is_err());
        let mut byte = [0u8; 1];
        assert_eq!(
            peer.read(&mut byte).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
    }

    #[test]
    fn wire_roundtrips_raw_argument_bytes() {
        let spec = LaunchSpec {
            program: "echo".into(),
            args: vec![OsString::from_vec(vec![b'a', 255, b'b'])],
            env: vec![],
            cwd: "/tmp".into(),
        };
        let bytes = serde_json::to_vec(&WireLaunch::from(&spec)).unwrap();
        let decoded: WireLaunch = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(decoded.args, vec![vec![b'a', 255, b'b']]);
        decoded.validate().unwrap();
    }

    #[test]
    fn protocol_rejects_nul_and_environment_assignments() {
        let mut wire = WireLaunch::from(&LaunchSpec {
            program: "echo".into(),
            args: vec![],
            env: vec![("bad=key".into(), "value".into())],
            cwd: "/tmp".into(),
        });
        assert!(wire.validate().is_err());
        wire.env.clear();
        wire.args.push(vec![0]);
        assert!(wire.validate().is_err());
    }
}
