//! Deliberately bounded behaviors for black-box supervision tests.
use std::io::{self, Read, Write};
use std::net::{TcpListener, UdpSocket};
use std::process::{Command, ExitCode};
use std::time::{Duration, Instant};

const MAX_DURATION_MS: u64 = 10_000;
const MAX_BYTES: usize = 4 * 1024 * 1024;

fn number(args: &[String], index: usize, default: u64, max: u64) -> io::Result<u64> {
    let value = args.get(index).map_or(Ok(default), |value| {
        value
            .parse()
            .map_err(|_| io::Error::other("invalid integer"))
    })?;
    if value > max {
        return Err(io::Error::other(format!(
            "value {value} exceeds bound {max}"
        )));
    }
    Ok(value)
}

fn duration(args: &[String], index: usize) -> io::Result<Duration> {
    Ok(Duration::from_millis(number(
        args,
        index,
        200,
        MAX_DURATION_MS,
    )?))
}

fn announce(message: impl std::fmt::Display) -> io::Result<()> {
    writeln!(io::stdout(), "{message}")?;
    io::stdout().flush()
}

fn pause() {
    std::thread::sleep(Duration::from_millis(5));
}

fn run(args: &[String]) -> io::Result<u8> {
    match args.first().map(String::as_str).unwrap_or("help") {
        "exit" => Ok(number(args, 1, 0, 255)? as u8),
        "sleep" | "ignore" | "detach" | "idle" => {
            #[cfg(unix)]
            if args[0] == "ignore" {
                // SAFETY: signal installs only constant dispositions, never a Rust callback.
                unsafe {
                    libc::signal(libc::SIGTERM, libc::SIG_IGN);
                    libc::signal(libc::SIGINT, libc::SIG_IGN);
                }
            }
            #[cfg(unix)]
            if args[0] == "detach" && unsafe { libc::setsid() } < 0 {
                return Err(io::Error::last_os_error());
            }
            announce(format!("PID {}", std::process::id()))?;
            let wait = if args[0] == "idle" {
                Duration::from_millis(number(args, 1, 65_000, 75_000)?)
            } else {
                duration(args, 1)?
            };
            std::thread::sleep(wait);
            Ok(number(args, 2, 0, 255)? as u8)
        }
        "args" => {
            for argument in &args[1..] {
                announce(format!("{}:{argument}", argument.len()))?;
            }
            Ok(0)
        }
        "environment" => {
            let key = args.get(1).ok_or_else(|| io::Error::other("missing key"))?;
            announce(std::env::var(key).unwrap_or_else(|_| "UNSET".into()))?;
            Ok(0)
        }
        "cwd" => {
            announce(std::env::current_dir()?.display())?;
            Ok(0)
        }
        "output" => {
            let bytes = number(args, 1, 131_072, MAX_BYTES as u64)? as usize;
            let mode = args.get(2).map(String::as_str).unwrap_or("chunked");
            let mut stdout = io::stdout().lock();
            let mut stderr = io::stderr().lock();
            if mode == "split-ready" {
                stdout.write_all(b"REA")?;
                stdout.flush()?;
                std::thread::sleep(Duration::from_millis(30));
                stdout.write_all(b"DY")?;
                stdout.flush()?;
                std::thread::sleep(Duration::from_millis(1_000));
                return Ok(0);
            }
            let mut block = [b'x'; 4096];
            if mode == "invalid" {
                block[0] = 0xff;
                block[1] = 0xfe;
            }
            if mode == "chunked" {
                block[4095] = b'\n';
            }
            let mut remaining = bytes;
            while remaining > 0 {
                let amount = remaining.min(block.len());
                stdout.write_all(&block[..amount])?;
                stderr.write_all(&block[..amount])?;
                remaining -= amount;
            }
            stdout.flush()?;
            stderr.flush()?;
            Ok(0)
        }
        "startup" => {
            let wait = duration(args, 1)?;
            if args.get(2).is_some_and(|stream| stream == "stderr") {
                io::stderr().write_all(b"\xffREADY")?;
                io::stderr().flush()?;
            } else {
                io::stdout().write_all(b"READY")?;
                io::stdout().flush()?;
            }
            std::thread::sleep(wait);
            Ok(number(args, 3, 0, 255)? as u8)
        }
        "tree" | "orphan-tree" => {
            let count = number(args, 1, 2, 8)?;
            let wait = duration(args, 2)?;
            let mode = args.get(3).map(String::as_str).unwrap_or("sleep");
            if !matches!(mode, "sleep" | "ignore" | "detach") {
                return Err(io::Error::other(
                    "tree mode must be sleep, ignore, or detach",
                ));
            }
            let executable = std::env::current_exe()?;
            let mut children = Vec::new();
            for _ in 0..count {
                let child = Command::new(&executable)
                    .args([mode, &wait.as_millis().to_string()])
                    .spawn()?;
                announce(format!("CHILD {}", child.id()))?;
                children.push(child);
            }
            if args[0] == "tree" {
                for mut child in children {
                    child.wait()?;
                }
            }
            Ok(number(args, 4, 0, 255)? as u8)
        }
        "allocate" => {
            let mib = number(args, 1, 1, 128)? as usize;
            let mut memory = vec![0_u8; mib * 1024 * 1024];
            for page in memory.chunks_mut(4096) {
                page[0] = 1;
            }
            announce(format!("ALLOCATED {}", memory.len()))?;
            std::thread::sleep(duration(args, 2)?);
            std::hint::black_box(memory);
            Ok(0)
        }
        "burn" => {
            let until = Instant::now() + duration(args, 1)?;
            let mut state = 1_u64;
            while Instant::now() < until {
                for _ in 0..4096 {
                    state = std::hint::black_box(
                        state.wrapping_mul(6364136223846793005).wrapping_add(1),
                    );
                }
            }
            announce(format!("DONE {state}"))?;
            Ok(0)
        }
        "compute" => {
            let iterations = number(args, 1, 1_000_000, 10_000_000)?;
            let mut state = 1_u64;
            for _ in 0..iterations {
                state =
                    std::hint::black_box(state.wrapping_mul(6364136223846793005).wrapping_add(1));
            }
            announce(format!("DONE {state}"))?;
            Ok(0)
        }
        "tcp" | "http" => {
            let until = Instant::now() + duration(args, 1)?;
            let listener = TcpListener::bind("127.0.0.1:0")?;
            listener.set_nonblocking(true)?;
            announce(format!("READY {}", listener.local_addr()?))?;
            let status = number(args, 2, 200, 599)?;
            let body = args.get(3).map(String::as_str).unwrap_or("READY");
            if body.len() > 65_536 {
                return Err(io::Error::other("HTTP body exceeds 64 KiB"));
            }
            let mut unhealthy = number(args, 4, 0, 100)?;
            while Instant::now() < until {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        if args[0] == "http" {
                            stream.set_read_timeout(Some(Duration::from_millis(100)))?;
                            stream.set_write_timeout(Some(Duration::from_millis(100)))?;
                            let mut request = [0; 8192];
                            let _ = stream.read(&mut request);
                            let code = if unhealthy > 0 {
                                unhealthy -= 1;
                                503
                            } else {
                                status
                            };
                            let _ = write!(
                                stream,
                                "HTTP/1.1 {code} Fixture\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                                body.len()
                            );
                        }
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => pause(),
                    Err(error) => return Err(error),
                }
            }
            Ok(0)
        }
        "udp" => {
            let until = Instant::now() + duration(args, 1)?;
            let socket = UdpSocket::bind("127.0.0.1:0")?;
            socket.set_nonblocking(true)?;
            announce(format!("READY {}", socket.local_addr()?))?;
            let mode = args.get(2).map(String::as_str).unwrap_or("echo");
            let mut buffer = [0; 4096];
            while Instant::now() < until {
                match socket.recv_from(&mut buffer) {
                    Ok((size, peer)) => match mode {
                        "silent" => {}
                        "wrong" => {
                            socket.send_to(b"WRONG", peer)?;
                        }
                        "pong" => {
                            socket.send_to(b"PONG", peer)?;
                        }
                        "wrong-peer" => {
                            UdpSocket::bind("127.0.0.1:0")?.send_to(&buffer[..size], peer)?;
                        }
                        _ => {
                            socket.send_to(&buffer[..size], peer)?;
                        }
                    },
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => pause(),
                    Err(error) => return Err(error),
                }
            }
            Ok(0)
        }
        _ => Err(io::Error::other(
            "commands: exit, sleep, idle, ignore, detach, args, environment, cwd, output, startup, tree, orphan-tree, allocate, burn, compute, tcp, http, udp",
        )),
    }
}

fn main() -> ExitCode {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let deadline_ms = if args.first().is_some_and(|argument| argument == "idle") {
        76_000
    } else {
        MAX_DURATION_MS + 1_000
    };
    // This independent deadline also bounds descendants after the test runner dies.
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(deadline_ms));
        std::process::exit(124);
    });
    match run(&args) {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("fixture: {error}");
            ExitCode::from(2)
        }
    }
}
