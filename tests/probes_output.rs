use proptest::prelude::*;
#[path = "support/events.rs"]
mod event_contract;
use serde_json::json;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use takt::model::Probe;
use takt::probes::{Matcher, Readiness};

proptest! {
    #[test]
    fn streaming_matcher_agrees_with_literal_byte_search(
        input in prop::collection::vec(any::<u8>(),0..512),
        pattern in prop::collection::vec(any::<u8>(),1..32),
        chunk in 1usize..64,
    ) {
        let expected=input.windows(pattern.len()).any(|window|window==pattern);
        let mut matcher=Matcher::new(&pattern);
        let mut actual=false;
        for bytes in input.chunks(chunk) {actual=matcher.feed(bytes);}
        prop_assert_eq!(actual,expected);
    }

    #[test]
    fn literal_match_survives_every_generated_chunk_boundary(
        prefix in prop::collection::vec(any::<u8>(),0..128),
        pattern in prop::collection::vec(any::<u8>(),1..32),
        chunk in 1usize..32,
    ) {
        let mut input=prefix;
        input.extend_from_slice(&pattern);
        let mut matcher=Matcher::new(&pattern);
        let mut found=false;
        for bytes in input.chunks(chunk) {found=matcher.feed(bytes);}
        prop_assert!(found);
    }
}

#[test]
fn either_stream_does_not_join_fragments_from_different_streams() {
    let spec: Probe =
        serde_json::from_value(json!({"type":"stdio","stream":"either","contains":"READY"}))
            .unwrap();
    let mut readiness = Readiness::new(spec);
    readiness.capture("stdout", b"REA");
    readiness.capture("stderr", b"DY");
    assert!(!readiness.tick().unwrap());
    readiness.capture("stdout", b"DY");
    assert!(readiness.tick().unwrap());
}

#[test]
fn stdio_bytes_observed_after_deadline_cannot_make_service_ready() {
    let spec: Probe = serde_json::from_value(
        json!({"type":"stdio","stream":"stdout","contains":"READY","timeout":"5ms"}),
    )
    .unwrap();
    let mut readiness = Readiness::new(spec);
    std::thread::sleep(Duration::from_millis(15));
    readiness.capture("stdout", b"READY");
    assert!(readiness.tick().is_err());
}

#[test]
fn unrepresentable_monotonic_deadline_returns_error_instead_of_panicking() {
    let mut readiness = Readiness::new(Probe::Tcp {
        host: "127.0.0.1".into(),
        port: 1,
        timeout: Some(takt::model::DurationValue(Duration::MAX)),
        interval: None,
        attempt_timeout: None,
    });
    assert!(readiness.tick().is_err());
}

fn bound_tcp_without_listening() -> (OwnedFd, u16) {
    // SAFETY: socket creates a new descriptor; errors are checked before ownership.
    let fd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0) };
    assert!(fd >= 0, "{}", std::io::Error::last_os_error());
    // SAFETY: the successful socket descriptor has one owner from this point.
    let socket = unsafe { OwnedFd::from_raw_fd(fd) };
    // SAFETY: FD_CLOEXEC applies only to this newly owned descriptor.
    let result = unsafe { libc::fcntl(socket.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) };
    assert_eq!(result, 0);
    // SAFETY: sockaddr_in is a C integer/byte record whose zeroed value is valid.
    let mut address: libc::sockaddr_in = unsafe { std::mem::zeroed() };
    address.sin_family = libc::AF_INET as libc::sa_family_t;
    address.sin_addr.s_addr = u32::from_ne_bytes([127, 0, 0, 1]);
    #[cfg(target_os = "macos")]
    {
        address.sin_len = std::mem::size_of_val(&address) as u8;
    }
    let mut length = std::mem::size_of_val(&address) as libc::socklen_t;
    // SAFETY: the pointer and length describe the initialized sockaddr_in above.
    let result = unsafe {
        libc::bind(
            socket.as_raw_fd(),
            (&address as *const libc::sockaddr_in).cast(),
            length,
        )
    };
    assert_eq!(result, 0);
    // SAFETY: getsockname writes no more than the provided live address buffer.
    let result = unsafe {
        libc::getsockname(
            socket.as_raw_fd(),
            (&mut address as *mut libc::sockaddr_in).cast(),
            &mut length,
        )
    };
    assert_eq!(result, 0);
    (socket, u16::from_be(address.sin_port))
}

#[test]
fn unavailable_tcp_endpoint_retries_until_bounded_readiness_deadline() {
    // Retain a bound, non-listening socket throughout: no guessed free port race.
    let (_socket, port) = bound_tcp_without_listening();
    let spec: Probe = serde_json::from_value(json!({
        "type":"tcp","host":"127.0.0.1","port":port,
        "timeout":"100ms","attempt_timeout":"25ms","interval":"10ms"
    }))
    .unwrap();
    let mut readiness = Readiness::new(spec);
    let started = Instant::now();
    let error = loop {
        match readiness.tick() {
            Ok(false) => std::thread::sleep(Duration::from_millis(1)),
            Ok(true) => panic!("non-listening endpoint became ready"),
            Err(error) => break error,
        }
        assert!(started.elapsed() < Duration::from_secs(2));
    };
    assert!(error.contains("readiness deadline exceeded"));
    // Linux refuses this endpoint; Darwin can leave the attempt pending instead.
    assert!(
        error.to_ascii_lowercase().contains("refused")
            || error.contains("network attempt timed out"),
        "{error}"
    );
    assert!(!readiness.ready);
}

#[test]
fn tcp_retry_becomes_ready_when_the_owned_endpoint_starts_listening() {
    let (socket, port) = bound_tcp_without_listening();
    let spec: Probe = serde_json::from_value(json!({
        "type":"tcp","host":"127.0.0.1","port":port,
        "timeout":"2s","attempt_timeout":"100ms","interval":"10ms"
    }))
    .unwrap();
    let mut readiness = Readiness::new(spec);
    let until = Instant::now() + Duration::from_secs(3);
    while readiness.last_error == "probe has not matched" {
        assert!(!readiness.tick().unwrap());
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(1));
    }
    // Both kernel responses prove the first attempt failed before listen starts.
    assert!(
        readiness
            .last_error
            .to_ascii_lowercase()
            .contains("refused")
            || readiness.last_error == "network attempt timed out",
        "{}",
        readiness.last_error
    );
    // SAFETY: start listening on the same bound socket retained across failure.
    let result = unsafe { libc::listen(socket.as_raw_fd(), 1) };
    assert_eq!(result, 0);
    while !readiness.tick().unwrap() {
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(readiness.ready);
}

struct OwnedRunner(Child);
impl Drop for OwnedRunner {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            // SAFETY: the unreaped direct child owns this private process group.
            unsafe {
                libc::kill(-(self.0.id() as i32), libc::SIGKILL);
            }
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

#[test]
fn invalid_hostname_lookup_cannot_outlive_service_readiness_deadline() {
    let scratch = tempfile::tempdir().unwrap();
    let config = json!({
        "version":1,"enforcement":"off","output":"json",
        "scheduling":{"jobs":2,"cpu":1,"memory":"64MiB","reserve_memory":"1MiB"},
        "defaults":{"resources":{"cpu":0.01,"memory":"1MiB"},"stop_grace":"20ms"},
        "tasks":{
            "service":{"kind":"service","command":[takt_test_fixture::executable(),"sleep","2000"],
                "readiness":{"type":"http","url":"http://takt-readiness.invalid/health",
                    "timeout":"150ms","attempt_timeout":"50ms","interval":"10ms"}},
            "consumer":{"kind":"task","command":[takt_test_fixture::executable(),"exit","0"],
                "depends_on":[{"task":"service","when":"ready"}]}
        }
    });
    let path = scratch.path().join("takt.json");
    std::fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
    let executable =
        std::env::var_os("TAKT_TEST_BINARY").unwrap_or_else(|| env!("CARGO_BIN_EXE_takt").into());
    let mut command = Command::new(executable);
    command
        .args(["run", "consumer", "--file"])
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
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
    let started = Instant::now();
    let mut runner = OwnedRunner(command.spawn().unwrap());
    let stdout = runner.0.stdout.take().unwrap();
    let stderr = runner.0.stderr.take().unwrap();
    let capture = |stream: Box<dyn Read + Send>| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            stream.take(64 * 1024).read_to_end(&mut bytes).unwrap();
            bytes
        })
    };
    let stdout = capture(Box::new(stdout));
    let stderr = capture(Box::new(stderr));
    let status = loop {
        if let Some(status) = runner.0.try_wait().unwrap() {
            break status;
        }
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "DNS lookup wedged supervision"
        );
        std::thread::sleep(Duration::from_millis(5));
    };
    let stdout = stdout.join().unwrap();
    let stderr = stderr.join().unwrap();
    assert_eq!(
        status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&stderr)
    );
    let events: Vec<serde_json::Value> = stdout
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice(line).unwrap())
        .collect();
    event_contract::assert_events(&events);
    assert!(!events.iter().any(|event| event["event"] == "task_ready"));
    let outcomes = &events.last().unwrap()["outcomes"];
    assert_eq!(outcomes["service"]["reason"], "readiness_timeout");
    assert_eq!(outcomes["consumer"]["kind"], "blocked");
}

fn http_response(response: Vec<u8>, body: Option<&str>) -> Result<bool, String> {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let worker = std::thread::spawn(move || {
        let until = Instant::now() + Duration::from_millis(500);
        while Instant::now() < until {
            match listener.accept() {
                Ok((mut connection, _)) => {
                    connection
                        .set_read_timeout(Some(Duration::from_millis(100)))
                        .unwrap();
                    connection
                        .set_write_timeout(Some(Duration::from_millis(100)))
                        .unwrap();
                    let mut request = [0; 8192];
                    let _ = connection.read(&mut request);
                    let _ = connection.write_all(&response);
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(1))
                }
                Err(error) => panic!("accept: {error}"),
            }
        }
    });
    let mut specification = json!({"type":"http","url":format!("http://{address}/health"),"timeout":"150ms","attempt_timeout":"100ms","interval":"20ms"});
    if let Some(body) = body {
        specification["body_contains"] = json!(body);
    }
    let mut probe = Readiness::new(serde_json::from_value(specification).unwrap());
    let result = loop {
        match probe.tick() {
            Ok(false) => std::thread::sleep(Duration::from_millis(1)),
            result => break result,
        }
    };
    worker.join().unwrap();
    result
}

#[test]
fn http_chunked_body_match_and_redirect_refusal() {
    assert!(http_response(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\nRE\r\n3\r\nADY\r\n0\r\n\r\n".to_vec(),Some("READY")).unwrap());
    assert!(
        http_response(
            b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1/\r\nContent-Length: 0\r\n\r\n"
                .to_vec(),
            None
        )
        .is_err()
    );
}

#[test]
fn oversized_declared_http_response_is_rejected_before_matching_prefix() {
    assert!(
        http_response(
            b"HTTP/1.1 200 OK\r\nContent-Length: 1048576\r\n\r\nREADY".to_vec(),
            Some("READY")
        )
        .is_err()
    );
}

#[test]
fn malformed_and_incomplete_http_response_cannot_match() {
    for response in [
        b"HTTP/1.1 200 OK\r\nContent-Length: nonsense\r\n\r\nREADY".as_slice(),
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nwrong\r\nREADY\r\n".as_slice(),
        b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nREA".as_slice(),
    ] {
        assert!(http_response(response.to_vec(), Some("READY")).is_err());
    }
}
