# Implementation status and acceptance evidence

Checkpoint: 2026-10-17. This is an implementation and evidence ledger, not a release
announcement. The original handoff's numbered cases remain the acceptance inventory;
source code, simulated snapshots, cross-compilation, and configured workflows do
not substitute for native execution or real controller results.

The latest completed [CI run
39051419408](https://github.com/samishal1998/takt/actions/runs/39051419408) passed
all three native jobs, both Linux controller safety jobs, and the actual Linux
compatibility guest at commit `e4bc49c51d7fd2b9958497d9ca54aa976102952a`.
This includes asynchronous launch and its cancellation/cleanup regressions.
The subsequent P12 spool and R01/R04 readiness additions pass the complete local
112-test suite. Those additions and the new S17 nested-runner/sibling case await
the final release CI; the earlier successful run does not validate later edits.

## Confirmed local checkpoints

The development host is Linux 6.8.0-79-generic x86-64 with glibc 2.39. The compiler
is `rustc 1.99.0 (b940084d7 2026-09-28)`.

| Check | Confirmed result |
|---|---|
| `cargo check --workspace --all-targets -j2` | Passed after adapting imports/options to parser 2.0 APIs |
| `cargo test --test config_scheduler -j2` | 38 passed, including generated-DAG/weighted-admission properties, explicit-auto provenance, alias/default/ancestry expansion budgets, and failures observed before cleanup |
| Independent library unit tests | 23 passed, including discovery, asynchronous launch/cancellation, shared grouped-spool bounds, partial/zero-write failure, and stalled-resolver deadlines/cleanup |
| `cargo test --test integration -j2 -- --test-threads=1` | 33 passed, including concurrent/queued launch fail-fast behavior and non-UTF policy-environment validation. Every parsed NDJSON event is checked against the published schema |
| Probe/output checks | 11 passed, including TCP refusal/deadline, delayed-listener recovery, and bounded failed hostname lookup |
| Configuration schema | Seven maintained YAML/JSON examples validated structurally and against Rust parsing/graph rules |
| macOS cross-check | Library cross-check passed; subsequent native execution is recorded separately below |
| `cargo test --test events_schema -j2` | Three passed: event payload/envelope rejection, neutral-skip summary, and configuration-schema examples |
| `cargo fmt --all --check` | Passed with the P12 and R01/R04 additions |
| `cargo clippy --workspace --all-targets -j2 -- -D warnings` | Passed with the P12 and R01/R04 additions |
| `process_lifecycle` independent tests | Four default tests passed in 0.45 s; two capability-gated cases ignored outside their required environments |
| Real Linux protection | Nine bounded GNU debug controller cases and the required loader-constructor containment test passed inside a delegated user systemd unit |
| Measurements | Durable VPS performance and repository comparisons are published in [performance.md](performance.md); reviewed native size baselines and native observations are recorded below |

The complete local `cargo test --workspace -j2 -- --test-threads=1` checkpoint
contains 112 passing tests: 23 library, 38 configuration/scheduler, three schema,
33 integration, 11 probe/output, and four process-lifecycle tests, with two
explicitly gated lifecycle proofs ignored in that default invocation. The local
log is `/tmp/takt-release-tests.log`.

`ManagedChild::start` creates the owned launcher; `poll_launch` advances its
handshake without blocking the supervision loop. Launch progress has a per-turn
byte bound and an absolute deadline, waits for the helper greeting before sending
task data, and requires execution EOF before reporting a started task. Tests
cover stalled progress, invalid handshakes, preserved execution errors, and
cancellation before the execution gate while retaining process identity through
cleanup. Concurrent and queued launch failure regressions pass locally and in
the successful remote checkpoint.

The latest local suite additionally verifies a shared stdout/stderr spool cap,
exact retained/discarded-byte accounting after partial `ENOSPC` writes, and
zero-write failure. A failed spool stops accepting writes while capture continues.
TCP tests retain one bound socket through refusal and delayed listening; DNS
tests cover a real `.invalid` lookup deadline and deterministic stalled helpers,
including the two-resolver cap, cancellation, and reaping. Packaged-artifact
checks now include the probe suite. These new cases still need their final native
release execution. The newly added S17 scenario has not yet been executed.

## Native CI and artifact checkpoints

These results concern commit `e4bc49c51d7fd2b9958497d9ca54aa976102952a`, including
the asynchronous launch change. Native jobs run independent checks, package and
extract archives, repeat integration/lifecycle checks against the extracted
executables, inspect dependencies, and retain 60-second performance observations.

| Lane | Confirmed result |
|---|---|
| Linux GNU | [Native job passed](https://github.com/samishal1998/takt/actions/runs/39051419408/job/117450612710) on Ubuntu 22.04 |
| Linux musl | [Native job passed](https://github.com/samishal1998/takt/actions/runs/39051419408/job/117450612813) on Ubuntu 22.04, including the no-dynamic-loader/shared-library check |
| macOS Apple Silicon | [Native job passed](https://github.com/samishal1998/takt/actions/runs/39051419408/job/117450612584), including portable lifecycle/probe/output checks and required-protection refusal |
| Linux compatibility floor | [Guest job passed](https://github.com/samishal1998/takt/actions/runs/39051419408/job/117451471596). Guest serial output reports Linux `5.15.0-198-generic` and glibc `2.35`; both extracted GNU and musl archives pass checksum and smoke checks, including `localhost` resolution |
| GNU controllers | [Safety job passed](https://github.com/samishal1998/takt/actions/runs/39051419408/job/117450612529): 22 independent case reports, external supervisor-death cleanup, and required loader-constructor containment proof |
| musl controllers | [Safety job passed](https://github.com/samishal1998/takt/actions/runs/39051419408/job/117450612668): 22 independent case reports and external supervisor-death cleanup |

Downloaded artifacts for that run are under
`target/ci-evidence/39051419408/`. The compatibility artifact's
`guest-serial.log` records the actual guest platform and final `PASS`. Safety
artifacts preserve outer bounds, delegation, unprivileged workload identity,
controller counters, task outcomes, and GNU loader membership. The verified
0.1-CPU task consumed 148,656 µs over 1.52 seconds on GNU and 149,535 µs over
1.51 seconds on musl, with 14 throttled periods in each case.

The ordinary safety workloads ran as UID 1001. Every outer safety unit verified
512 MiB memory, 32 tasks, a 20-second deadline, zero swap, control-group cleanup,
and forced-kill availability. Task OOM left an already-running peer alive to
finish; aggregate OOM gave both tasks `aggregate_memory_limit`, and ordinary
exit 137 remained `normal_exit`. Repeated success, timeout, and cancellation cases
left no owned cgroups. Read-only, denied, and missing delegation checks passed;
`auto` reported fallback where applicable. A real 0.5-CPU/128-MiB ancestor yielded
500-millicore budgets and memory budgets of 132,304,896 bytes on GNU and
132,579,328 bytes on musl. Independent systemd cleanup removed all owned groups
after supervisor SIGKILL. Swap-present behavior was exercised; the conditional
absent-swap branch was not exercised on these capable hosts.

The new S17 case adds nested Takt invocations and a live 16-MiB sibling under a
shared 0.5-CPU/96-MiB ceiling. Its source and assertions are present; execution
evidence remains pending in final release CI.

## Measured size and performance

The reviewed [size baseline](../scripts/size-baseline.json) comes from native
artifacts in [CI run 39049559556](https://github.com/samishal1998/takt/actions/runs/39049559556),
commit `16c91e9347e217793f6f3304f2933cea06a62609`. That run's successful native
jobs retained these full-feature release measurements. The baseline records
artifact size independently from other acceptance gates.

| Native target | Executable bytes | Median supervisor idle RSS | Supervisor CPU, percent of one core over 60 seconds |
|---|---:|---:|---:|
| Linux GNU | 2,159,032 | 3,538,944 bytes | 1.55% |
| Linux musl | 2,282,336 | 1,835,008 bytes | 1.70% |
| macOS Apple Silicon, macOS 15.7.9 | 1,604,352 | 3,244,032 bytes | 0.161% |

These observations meet the below-5-MiB RSS goal but miss the below-1,000,000-byte
size and below-0.1%-of-one-core idle CPU goals. Goals remain nonblocking under
the accepted policy. Packaging enforces the reviewed baseline's separate growth
threshold: the larger of 10% or 100 KiB. Default `auto` enforcement used the
reported fallback watchdog during these idle measurements; no hard controller
guarantee is inferred from them. The successful run 39051419408 measured GNU at
2,170,664 bytes and 1.53% idle CPU, musl at 2,294,624 bytes and 1.65%, and macOS
at 1,604,416 bytes and 0.148%. All remained within the size-regression threshold;
all still exceeded the size and idle-CPU goals.

[Performance documentation](performance.md) retains the separate VPS fixture
measurements and their [raw report](evidence/linux-vps-performance.json), plus
the [repository comparison](evidence/linux-vps-repository.json). Identical
two-slot repository checks had medians of 9.825 seconds through Takt and 9.484
seconds through the shell over three measured runs each, with overlapping ranges.
Those results support no speed-superiority claim. Final source changes require
new artifact hashes and measurements; these reports describe earlier checkpoints.

## Requirement traceability

| Handoff cases | Implementation/evidence | Remaining acceptance work |
|---|---|---|
| C01, C03–C07, C09–C11 | Strict parser, graph validation, defaults/env/cwd handling; parser and expansion budgets; examples and independent schema/model tests pass in the native matrix | Expanded release property cases and retained failing seeds if any; repeat final-source native checks |
| C02, C08 | CLI precedence/provenance, explicit-auto file metadata, and explicit argv/shell execution; exact-argument and precedence tests pass natively | Final release-source matrix |
| S01–S07, S09–S10 | Deterministic/property tests pass natively; real ancestor and unavailable-capability checks pass in both Linux safety lanes | Final release-source rerun |
| S08, S16 | Static/live service-deadlock checks and shared-service lifetime tests pass; unfinished prerequisites retain required services after a root skips | Final release-source rerun |
| S11–S15 | Exit predicates remain separate from success; neutral skips, fail-fast, and stop-dependents tests pass natively, including asynchronous launch | Final release-source rerun |
| S17 | Discovery observes visible ancestor limits and affinity; dedicated nested-runner/sibling case added | Execute the new case in both final Linux safety lanes |
| R01–R09 | Native four-probe coverage passes; new local R01/R04 tests add refused/delayed TCP, real failed lookup, and deterministic stalled-resolver deadline/cancellation cleanup | Execute added cases in native and extracted-artifact release lanes |
| R10–R14 | Native tests pass for pre/post-readiness exit, consumer policies, finite/persistent roots, and asynchronous-launch interactions | Final release-source rerun |
| P01–P03, P10–P14 | Native output/event checks pass; new local P12 tests verify shared spool limits, partial disk-write failure, zero writes, truncation accounting, continued capture, and cleanup | Execute added spool tests natively against final source |
| P04–P08 | Native GNU/musl/macOS asynchronous lifecycle tests pass; GNU loader constructor demonstrates attachment before project loader code | Final release-source rerun |
| P09 | External systemd cleanup after supervisor SIGKILL passed for GNU and musl; portable parent-death cleanup is not promised | Retain final release safety evidence |
| L01–L10 | Both GNU/musl safety lanes pass OOM, quotas, descendants, refusal/fallback, ancestors, swap-present reporting, ordinary exit 137, and repeated cleanup; GNU loader proof passes | Final release safety rerun; absent-swap execution remains conditional and unclaimed on capable hosts |
| Native macOS | Apple Silicon native suite and extracted-archive tests passed on macOS 15; native size/RSS/CPU observations retained | Execute latest spool/probe additions; fallback remains best effort |
| Artifact compatibility | All three native archive checks passed; actual Linux 5.15/glibc 2.35 guest ran both extracted Linux artifacts successfully | Regenerate and validate final artifacts; verify release hashes and publication gates |
| Performance/size | Reviewed per-target size baseline; native 60-second observations and durable VPS fixture/repository comparisons published | Remeasure changed artifacts; retain missed size/idle-CPU goals and avoid unsupported speed claims |

## Milestone gates

| Milestone | Current interpretation |
|---|---|
| M0 | Accepted decisions, measured baseline, native platform, and GNU loader containment evidence pass at the recorded checkpoint |
| M1 | Independent finite/process/output suites passed on GNU, musl, and native macOS; final-source rerun remains |
| M2 | Configuration, graph, scheduler properties, provenance, and schema/example checks pass in the native matrix; preserve these checks through final changes |
| M3 | Both remote controller safety lanes pass, including external cleanup; new S17 evidence remains for final release CI |
| M4 | Four probes and asynchronous service lifecycle pass natively; added R01/R04 failure-path coverage passes locally and awaits release CI |
| M5 | Native output/schema checks pass; added P12 spool failure coverage passes locally and awaits release CI |
| M6 | Native archives, both safety lanes, and actual compatibility guest passed; size/overhead and repository comparisons are measured. Final source and S17 must pass release CI and publication gates before release acceptance |

## Material boundaries and open evidence

The local protection run used `scripts/linux-user-proof.sh` inside
`takt-proof-20261017-platform.service`, with `Delegate=yes`, independently verified
`MemoryMax=536870912`, `MemorySwapMax=0`, `TasksMax=32`, `RuntimeMaxSec=20s`,
`TimeoutStopSec=1s`, and `KillMode=control-group`. The unit was collected after the
proof. Local raw evidence is retained in `target/safety-user-evidence/`.

`outer-boundary.txt` records the outer policy. `loader-constructor.txt` records a
task loader constructor already in the owned `workload/task-0` cgroup, proving
task loader environment was applied after attachment. Task OOM used a 64 MiB cap
and bounded 128 MiB allocator with an unaffected task surviving; aggregate OOM
used a 64 MiB workload cap with two bounded 48 MiB allocators. Controller counters
and task outcomes distinguish those cases. The 0.1 CPU test recorded 159,276 µs
CPU across 1.60 s wall time and 15 throttled periods. Immediate/detached trees
reached three members in their owned domain. Exit 137 remained `normal_exit`, and
three repeated cleanup cases left no owned groups. These results concern the
current Linux kernel and GNU debug executables; they are not musl, macOS, release
artifact, or baseline-guest results.

Process-group fallback cannot contain descendants that change session/group, and
sampled RSS may double-count shared pages or miss detached/short-lived processes.
macOS fallback therefore provides no hard aggregate protection. `required` must
refuse unsupported requested dimensions. On Linux, cgroup visibility and delegation
are verified at runtime; a detected cgroup mount alone is not proof of permission.

Readiness reports its protocol observation. TCP cannot establish listener identity;
HTTP does not follow redirects or support HTTPS; UDP requires a matching peer and
reply; stdio matches literal bytes. No ownership or application-health guarantee is
inferred beyond those contracts.

The native and baseline-guest results above establish those platform checkpoints
for their recorded commit. They do not validate later source changes. A skipped
safety test or a failed safety job is not a passing hard-limit gate. Measured size
and idle-CPU goals are unmet and disclosed; RSS measurements meet the initial
goal on the sampled artifacts. Remaining safety, failure-path, and final-artifact
evidence requirements do not reduce the approved release scope.
