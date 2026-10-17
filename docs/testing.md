# Testing Takt

The acceptance inventory is preserved in
[`takt-handoff-v0.1/docs/03-Test-and-CI-plan.md`](https://github.com/samishal1998/takt/blob/main/takt-handoff-v0.1/docs/03-Test-and-CI-plan.md).
Test source and workflow configuration are not evidence that a platform gate has passed.
Retain the associated CI run and artifact links when recording a release decision.

## Independent local checks

Run these directly through Cargo, before allowing Takt to schedule repository checks:

```sh
cargo build --locked -p takt-test-fixture
cargo test --workspace --locked -- --test-threads=1
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
```

`scripts/independent-checks.sh RUST_TARGET` additionally builds a release executable
and validates the maintained examples. The fixture is a development-only package;
it is absent from production dependency trees and release archives. Set
`TAKT_TEST_BINARY` to test an extracted Takt executable and `TAKT_TEST_FIXTURE` to
select the fixture built for that target. The ordinary integration tests never
invoke Takt to drive other tests.

## Bounded process fixtures

The fixture implements `exit`, `sleep`, `args`, `environment`, `cwd`, `output`,
`startup`, `tree`, `orphan-tree`, `ignore`, `detach`, `allocate`, `burn`, `compute`,
`idle`, `tcp`, `http`, and `udp`.
Arguments are documented by the command dispatch in its small source file.
Durations are at most 10 seconds; an independent watchdog exits at 11 seconds.
The dedicated performance-only `idle` mode permits at most 75 seconds and exits
at 76 seconds; `compute` performs at most ten million fixed arithmetic iterations.
Allocation is capped at 128 MiB, child count at eight, and dual-stream output at
4 MiB per stream. Allocator fixtures touch every page. Network fixtures bind
loopback port zero and flush `READY <address>` before the harness configures a
probe. No guessed free port, Node, Python, `nc`, or internet service is required
for Rust integration tests.

The harness retains child handles, creates its own process groups, drains both
streams concurrently with a 16 MiB capture cap per stream, and applies a 15-second
supervisor deadline. The network readiness cases explicitly test listener health;
they do not assert that a listener belongs to the service process.

Black-box coverage includes exact argv, spawn versus exit failures, shared
prerequisites, conditional nonzero branches, slot serialization, both output
streams, invalid UTF-8, event sequences, immediate and split stdio readiness,
TCP/HTTP/UDP, wrong UDP peers/payloads, unhealthy HTTP retries, impossible service
reservations, timeout edge classification, cwd/environment removal, strict parsing,
discovery ambiguity, and ordinary exit 137. Native macOS additionally checks
required aggregate-memory refusal. Pure model/platform tests complement these
process tests; none substitutes for real controller evidence.

## Real Linux controller lane

The dedicated `linux-safety` jobs run GNU and musl builds on disposable
`ubuntu-24.04` machines. They create a systemd delegated unit with an independently
verified 512 MiB memory ceiling, zero swap, 32-task ceiling, 20-second runtime limit,
and control-group cleanup. The setup moves only the unit's own supervisor into
a child leaf before enabling delegated controllers. It never modifies shared host
controller policy. Missing delegation or a mismatched external boundary fails
the job.

The ordinary workload cases run as the unprivileged delegated runner. The setup
retains parent and supervisor controller values in `delegation-base.json` and
`delegation-extended.json`, plus the independently read outer unit properties, before any
loader or workload test. `scripts/linux-safety.py` uses an independent subprocess harness with four-second
case deadlines and at most 2 MiB captured output per stream. It records controller
values/counters and raw NDJSON for per-task OOM, aggregate OOM, CPU bandwidth,
immediate and detached descendants, exit 137, and repeated cleanup. The actual
release evidence must include controller-counter deltas, successful unaffected
work where applicable, and absence of owned cgroup leaks. The outer systemd unit
remains the cleanup boundary if the supervisor or test harness fails.

The privileged extended script additionally uses private mount namespaces for
read-only controller refusal/fallback, drops privileges for denied-delegation
refusal, and checks real intermediate CPU/memory ceilings. The unprivileged lane
verifies reported swap capability against available controller files. A separate transient systemd unit
tests cleanup after the Takt supervisor receives SIGKILL. These cases run only in
the disposable hosted VM. An absent swap-controller branch is conditional on
actual kernel capability and must not be reported as exercised on a capable host.

Native process tests cover ignored signals, second-interrupt acceleration,
surviving descendants, blocked/broken output, grouped spool cleanup/fallback, and
loader environment isolation. Property tests run 256 cases routinely and 2,048
cases in the scheduled weekly and release lanes, including manual releases. Failed native jobs retain
generated regression seeds in separate `property-failures-*` artifacts. Link
the actual run evidence for these checks separately from source-level coverage.

The privileged namespace provisioning script belongs in its disposable CI
environment. The separate `linux-user-proof.sh` can run only within an explicitly
created, delegated systemd user unit whose outer bounds it verifies; it does not
perform privileged mount operations or modify shared controller policy.

## Platform and artifact lanes

The native matrix uses `ubuntu-22.04` for GNU and musl and `macos-15` for Apple
Silicon. It asserts the native Mac architecture. GitHub documents these runner
labels in its [hosted runner reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners).
All action dependencies are full commit SHAs verified against GitHub's repository
API during implementation; update them deliberately with the same verification.

The compatibility job boots the dated Ubuntu Jammy image
`release-20261004/ubuntu-22.04-server-cloudimg-amd64.img`, whose SHA-256 is
`012d81fade7e8ff4428fc35dfeee711d66d1f47d5ea9c5c8ced96a85b959835c`.
The [vendor checksum list](https://cloud-images.ubuntu.com/releases/jammy/release-20261004/SHA256SUMS)
and [package manifest](https://cloud-images.ubuntu.com/releases/jammy/release-20261004/ubuntu-22.04-server-cloudimg-amd64.manifest)
identify the image; the running guest must itself report Linux 5.15 and glibc 2.35.
Both extracted artifacts execute direct argv, dual-stream output, service readiness,
and `localhost` resolution in that guest. This job runs on main pushes and releases,
while PRs retain native artifact tests.

The scripts use Python 3's standard library only for CI evidence and bounded
controller orchestration. Python is not needed to install or run Takt.
