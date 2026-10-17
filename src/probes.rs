//! Deadline-bound, nonblocking readiness. Matching always precedes rendering.
use crate::model::{Probe, Reply, Stream};
use mio::net::{TcpStream, UdpSocket};
use std::io::{self, Read, Write};
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::os::fd::AsRawFd;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;
use ureq_proto::client::{Call, RecvResponseResult, SendRequestResult};

static RESOLVERS: AtomicUsize = AtomicUsize::new(0);
static RETIRED_RESOLVERS: Mutex<Vec<Child>> = Mutex::new(Vec::new());
const RESPONSE_LIMIT: usize = 64 * 1024;

/// Reap killed helpers without blocking. Retired helpers retain their slot until
/// reaped, keeping the total of live and retired resolver children at most two.
pub fn reap_resolvers() {
    let mut retired = RETIRED_RESOLVERS
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    retired.retain_mut(|child| {
        if child.try_wait().is_ok_and(|status| status.is_some()) {
            RESOLVERS.fetch_sub(1, Ordering::AcqRel);
            false
        } else {
            true
        }
    });
}

#[derive(Debug)]
pub struct Matcher {
    pattern: Vec<u8>,
    prefix: Vec<usize>,
    matched: usize,
    found: bool,
}
impl Matcher {
    pub fn new(pattern: &[u8]) -> Self {
        let mut prefix = vec![0; pattern.len()];
        let mut k = 0;
        for i in 1..pattern.len() {
            while k > 0 && pattern[i] != pattern[k] {
                k = prefix[k - 1];
            }
            if pattern[i] == pattern[k] {
                k += 1;
            }
            prefix[i] = k;
        }
        Self {
            pattern: pattern.to_vec(),
            prefix,
            matched: 0,
            found: pattern.is_empty(),
        }
    }
    pub fn feed(&mut self, data: &[u8]) -> bool {
        if self.found {
            return true;
        }
        for &b in data {
            while self.matched > 0 && b != self.pattern[self.matched] {
                self.matched = self.prefix[self.matched - 1];
            }
            if b == self.pattern[self.matched] {
                self.matched += 1;
            }
            if self.matched == self.pattern.len() {
                self.found = true;
                break;
            }
        }
        self.found
    }
}

struct Resolver {
    child: Option<Child>,
    bytes: Vec<u8>,
    deadline: Instant,
    killed: bool,
}
impl Resolver {
    fn start(host: &str, port: u16, deadline: Instant) -> io::Result<Option<Self>> {
        reap_resolvers();
        if RESOLVERS
            .try_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < 2).then_some(n + 1)
            })
            .is_err()
        {
            return Ok(None);
        }
        let spawned = (|| {
            Command::new(std::env::current_exe()?)
                .env_clear()
                .arg("__takt_resolve")
                .arg(host)
                .arg(port.to_string())
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
        })();
        let child = match spawned {
            Ok(child) => child,
            Err(error) => {
                RESOLVERS.fetch_sub(1, Ordering::AcqRel);
                return Err(error);
            }
        };
        let fd = child.stdout.as_ref().expect("piped").as_raw_fd();
        // Establish the cleanup owner before any fallible descriptor operation.
        let resolver = Self {
            child: Some(child),
            bytes: Vec::new(),
            deadline,
            killed: false,
        };
        // SAFETY: F_GETFL only reads flags on the owned resolver pipe.
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: F_SETFL changes flags on this owned open descriptor.
        let result = unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) };
        if result < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Some(resolver))
    }
    fn poll(&mut self) -> Result<Option<Vec<SocketAddr>>, String> {
        let child = self.child.as_mut().expect("active resolver owns its child");
        if Instant::now() >= self.deadline && !self.killed {
            let _ = child.kill();
            self.killed = true;
            return Err("DNS resolution deadline exceeded".into());
        }
        let mut buf = [0u8; 4096];
        if let Some(pipe) = child.stdout.as_mut() {
            loop {
                match pipe.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if self.bytes.len() + n > 8192 {
                            let _ = child.kill();
                            self.killed = true;
                            return Err("DNS resolver reply exceeds 8 KiB".into());
                        }
                        self.bytes.extend_from_slice(&buf[..n]);
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                    Err(e) => return Err(e.to_string()),
                }
            }
        }
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            if self.killed {
                return Err("DNS resolution deadline exceeded".into());
            }
            if !status.success() {
                return Err("DNS resolution failed".into());
            }
            let addresses: Vec<SocketAddr> = serde_json::from_slice(&self.bytes)
                .map_err(|e| format!("invalid resolver reply: {e}"))?;
            if addresses.is_empty() {
                return Err("DNS returned no addresses".into());
            }
            return Ok(Some(addresses));
        }
        Ok(None)
    }
}
impl Drop for Resolver {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            if child.try_wait().is_ok_and(|status| status.is_some()) {
                RESOLVERS.fetch_sub(1, Ordering::AcqRel);
            } else {
                let _ = child.kill();
                RETIRED_RESOLVERS
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .push(child);
            }
        }
    }
}

pub fn maybe_run_resolver() -> Option<i32> {
    let mut args = std::env::args_os();
    args.next();
    if args.next().as_deref() != Some(std::ffi::OsStr::new("__takt_resolve")) {
        return None;
    }
    let result = (|| -> Result<(), String> {
        let host = args
            .next()
            .ok_or("missing hostname")?
            .into_string()
            .map_err(|_| "invalid hostname")?;
        let port = args
            .next()
            .ok_or("missing port")?
            .into_string()
            .map_err(|_| "invalid port")?
            .parse::<u16>()
            .map_err(|_| "invalid port")?;
        if args.next().is_some() || host.len() > 253 {
            return Err("invalid resolver request".into());
        }
        let addresses: Vec<_> = (host.as_str(), port)
            .to_socket_addrs()
            .map_err(|e| e.to_string())?
            .take(16)
            .collect();
        serde_json::to_writer(io::stdout(), &addresses).map_err(|e| e.to_string())?;
        Ok(())
    })();
    Some(if result.is_ok() { 0 } else { 1 })
}

enum Attempt {
    Idle,
    Resolving(Resolver),
    Tcp(TcpStream),
    Http {
        socket: TcpStream,
        request: Vec<u8>,
        sent: usize,
        response: Vec<u8>,
    },
    Udp {
        socket: UdpSocket,
        sent: bool,
    },
}
pub struct Readiness {
    spec: Probe,
    started: Instant,
    next: Instant,
    attempt_deadline: Instant,
    attempt: Attempt,
    stdout: Option<Matcher>,
    stderr: Option<Matcher>,
    pub ready: bool,
    pub last_error: String,
    addresses: Vec<SocketAddr>,
    address_index: usize,
}
impl Readiness {
    pub fn new(spec: Probe) -> Self {
        let now = Instant::now();
        let (stdout, stderr) = match &spec {
            Probe::Stdio {
                stream, contains, ..
            } => (
                matches!(stream, Stream::Stdout | Stream::Either)
                    .then(|| Matcher::new(contains.as_bytes())),
                matches!(stream, Stream::Stderr | Stream::Either)
                    .then(|| Matcher::new(contains.as_bytes())),
            ),
            _ => (None, None),
        };
        Self {
            spec,
            started: now,
            next: now,
            attempt_deadline: now,
            attempt: Attempt::Idle,
            stdout,
            stderr,
            ready: false,
            last_error: "probe has not matched".into(),
            addresses: Vec::new(),
            address_index: 0,
        }
    }
    pub fn capture(&mut self, stream: &str, data: &[u8]) {
        if self.started.elapsed() >= self.spec.timeout() {
            return;
        }
        let matcher = if stream == "stdout" {
            &mut self.stdout
        } else {
            &mut self.stderr
        };
        if let Some(m) = matcher
            && m.feed(data)
        {
            self.ready = true;
        }
    }
    pub fn tick(&mut self) -> Result<bool, String> {
        reap_resolvers();
        if self.ready {
            return Ok(true);
        }
        let now = Instant::now();
        let overall_deadline = self
            .started
            .checked_add(self.spec.timeout())
            .ok_or("readiness deadline exceeds the monotonic clock range")?;
        if now.duration_since(self.started) >= self.spec.timeout() {
            self.attempt = Attempt::Idle;
            return Err(format!("readiness deadline exceeded: {}", self.last_error));
        }
        if matches!(self.spec, Probe::Stdio { .. }) {
            return Ok(false);
        }
        if matches!(self.attempt, Attempt::Idle) && now >= self.next {
            self.attempt_deadline = now
                .checked_add(self.spec.attempt_timeout())
                .ok_or("probe attempt deadline exceeds the monotonic clock range")?
                .min(overall_deadline);
            if self.addresses.is_empty() {
                let (host, port) = self.endpoint()?;
                if let Ok(ip) = host.trim_matches(['[', ']']).parse::<IpAddr>() {
                    self.addresses = vec![SocketAddr::new(ip, port)];
                } else {
                    match Resolver::start(&host, port, self.attempt_deadline) {
                        Ok(Some(r)) => self.attempt = Attempt::Resolving(r),
                        Ok(None) => return Ok(false),
                        Err(e) => self.retry(e.to_string())?,
                    };
                }
            }
            if !self.addresses.is_empty()
                && let Err(e) = self.connect()
            {
                self.retry(e)?;
            }
        }
        if let Attempt::Resolving(resolver) = &mut self.attempt {
            match resolver.poll() {
                Ok(Some(addresses)) => {
                    self.addresses = addresses;
                    if let Err(e) = self.connect() {
                        self.retry(e)?;
                    }
                }
                Err(e) => self.retry(e)?,
                Ok(None) => return Ok(false),
            }
        }
        if !matches!(self.attempt, Attempt::Idle) && now >= self.attempt_deadline {
            self.retry("network attempt timed out".into())?;
            return Ok(false);
        }
        match self.poll_network() {
            Ok(true) => {
                self.ready = true;
                self.attempt = Attempt::Idle;
            }
            Ok(false) => {}
            Err(e) => self.retry(e)?,
        }
        Ok(self.ready)
    }
    fn endpoint(&self) -> Result<(String, u16), String> {
        match &self.spec {
            Probe::Tcp { host, port, .. } | Probe::Udp { host, port, .. } => {
                Ok((host.clone(), *port))
            }
            Probe::Http { url, .. } => {
                let uri: url_alias::Uri =
                    url.parse().map_err(|e| format!("invalid HTTP URL: {e}"))?;
                if uri.scheme_str() != Some("http") {
                    return Err("only plain HTTP readiness is supported".into());
                }
                Ok((
                    uri.host().ok_or("missing HTTP hostname")?.to_owned(),
                    uri.port_u16().unwrap_or(80),
                ))
            }
            _ => Err("not a network probe".into()),
        }
    }
    fn connect(&mut self) -> Result<(), String> {
        let address = self.addresses[self.address_index % self.addresses.len()];
        self.address_index = (self.address_index + 1) % self.addresses.len();
        self.attempt = match &self.spec {
            Probe::Tcp { .. } => {
                Attempt::Tcp(TcpStream::connect(address).map_err(|e| e.to_string())?)
            }
            Probe::Http { url, .. } => Attempt::Http {
                socket: TcpStream::connect(address).map_err(|e| e.to_string())?,
                request: http_request(url)?.0,
                sent: 0,
                response: Vec::new(),
            },
            Probe::Udp { .. } => {
                let socket = UdpSocket::bind(
                    if address.is_ipv4() {
                        "0.0.0.0:0"
                    } else {
                        "[::]:0"
                    }
                    .parse()
                    .expect("literal address"),
                )
                .map_err(|e| e.to_string())?;
                socket.connect(address).map_err(|e| e.to_string())?;
                Attempt::Udp {
                    socket,
                    sent: false,
                }
            }
            _ => return Err("not a network probe".into()),
        };
        Ok(())
    }
    fn retry(&mut self, error: String) -> Result<(), String> {
        self.last_error = error;
        self.attempt = Attempt::Idle;
        self.next = Instant::now()
            .checked_add(self.spec.interval())
            .ok_or("probe retry deadline exceeds the monotonic clock range")?;
        Ok(())
    }
    fn poll_network(&mut self) -> Result<bool, String> {
        match &mut self.attempt {
            Attempt::Tcp(socket) => {
                if let Some(e) = socket.take_error().map_err(|e| e.to_string())? {
                    return Err(e.to_string());
                }
                Ok(socket.peer_addr().is_ok())
            }
            Attempt::Udp { socket, sent } => {
                let Probe::Udp {
                    payload, expect, ..
                } = &self.spec
                else {
                    unreachable!()
                };
                if !*sent {
                    match socket.send(&payload.decode()?) {
                        Ok(_) => *sent = true,
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(false),
                        Err(e) => return Err(e.to_string()),
                    }
                }
                let mut data = [0u8; 65536];
                match socket.recv(&mut data) {
                    Ok(n) => {
                        let wanted = expect.bytes().decode()?;
                        Ok(match expect {
                            Reply::Equals(_) => data[..n] == wanted,
                            Reply::Prefix(_) => data[..n].starts_with(&wanted),
                        })
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => Ok(false),
                    Err(e) => Err(e.to_string()),
                }
            }
            Attempt::Http {
                socket,
                request,
                sent,
                response,
            } => {
                if let Some(e) = socket.take_error().map_err(|e| e.to_string())? {
                    return Err(e.to_string());
                }
                while *sent < request.len() {
                    match socket.write(&request[*sent..]) {
                        Ok(0) => return Err("HTTP socket closed while writing".into()),
                        Ok(n) => *sent += n,
                        Err(e)
                            if matches!(
                                e.kind(),
                                io::ErrorKind::WouldBlock | io::ErrorKind::NotConnected
                            ) =>
                        {
                            return Ok(false);
                        }
                        Err(e) => return Err(e.to_string()),
                    }
                }
                let mut eof = false;
                let mut buf = [0u8; 8192];
                for _ in 0..8 {
                    match socket.read(&mut buf) {
                        Ok(0) => {
                            eof = true;
                            break;
                        }
                        Ok(n) => {
                            if response.len() + n > RESPONSE_LIMIT {
                                return Err("HTTP response exceeds 64 KiB".into());
                            }
                            response.extend_from_slice(&buf[..n]);
                        }
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                        Err(e) => return Err(e.to_string()),
                    }
                }
                let Probe::Http {
                    url,
                    status_codes,
                    body_contains,
                    ..
                } = &self.spec
                else {
                    unreachable!()
                };
                evaluate_http(
                    url,
                    response,
                    eof,
                    status_codes.as_deref().unwrap_or(&[200]),
                    body_contains.as_deref(),
                )
            }
            _ => Ok(false),
        }
    }
}
mod url_alias {
    pub use ureq_proto::http::Uri;
}
fn http_request(
    url: &str,
) -> Result<(Vec<u8>, Call<ureq_proto::client::state::RecvResponse>), String> {
    let request = ureq_proto::http::Request::get(url)
        .header("connection", "close")
        .body(())
        .map_err(|e| e.to_string())?;
    let mut call = Call::new(request).map_err(|e| e.to_string())?.proceed();
    let mut bytes = vec![0u8; 8192];
    let used = call.write(&mut bytes).map_err(|e| e.to_string())?;
    if !call.can_proceed() {
        return Err("HTTP request headers exceed 8 KiB".into());
    }
    bytes.truncate(used);
    match call.proceed().map_err(|e| e.to_string())? {
        Some(SendRequestResult::RecvResponse(response)) => Ok((bytes, response)),
        _ => Err("unexpected HTTP GET state".into()),
    }
}
fn evaluate_http(
    url: &str,
    input: &[u8],
    eof: bool,
    statuses: &[u16],
    contains: Option<&str>,
) -> Result<bool, String> {
    let (_, mut parser) = http_request(url)?;
    let (used, response) = parser
        .try_response(input, false)
        .map_err(|e| e.to_string())?;
    let Some(response) = response else {
        return if eof {
            Err("incomplete HTTP headers".into())
        } else {
            Ok(false)
        };
    };
    if response
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .is_some_and(|n| n > RESPONSE_LIMIT as u64)
    {
        return Err("HTTP response declares more than 64 KiB".into());
    }
    if !statuses.contains(&response.status().as_u16()) {
        return Err(format!("HTTP status {}", response.status()));
    }
    let Some(pattern) = contains else {
        return Ok(true);
    };
    let Some(RecvResponseResult::RecvBody(mut body)) = parser.proceed() else {
        return Err("HTTP body is absent".into());
    };
    let mut offset = used;
    let mut matcher = Matcher::new(pattern.as_bytes());
    let mut decoded = [0u8; 8192];
    while offset < input.len() {
        let (consumed, n) = body
            .read(&input[offset..], &mut decoded)
            .map_err(|e| e.to_string())?;
        offset += consumed;
        if matcher.feed(&decoded[..n]) {
            return Ok(true);
        }
        if consumed == 0 {
            break;
        }
    }
    if eof {
        return Err("HTTP body did not match".into());
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn literal_crosses_chunks_and_handles_binary() {
        let mut m = Matcher::new(b"ready");
        assert!(!m.feed(b"\xffre"));
        assert!(m.feed(b"ady\n"));
    }
    #[test]
    fn overlapping_prefix() {
        let mut m = Matcher::new(b"ababac");
        assert!(!m.feed(b"ababab"));
        assert!(m.feed(b"ac"));
    }
    #[test]
    fn chunked_http_body() {
        assert!(evaluate_http("http://localhost/",b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\nre\r\n3\r\nady\r\n0\r\n\r\n",true,&[200],Some("ready")).unwrap());
    }
    #[test]
    fn http_failure_status() {
        assert!(
            evaluate_http(
                "http://localhost/",
                b"HTTP/1.1 503 No\r\nContent-Length: 0\r\n\r\n",
                true,
                &[200],
                None
            )
            .is_err()
        );
    }
}
