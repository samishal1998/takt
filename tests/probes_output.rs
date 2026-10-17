use proptest::prelude::*;
use serde_json::json;
use std::io::{Read, Write};
use std::net::TcpListener;
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
