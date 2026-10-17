# Implementation status and acceptance evidence

Checkpoint: 2026-10-17. This is an implementation and evidence ledger, not a release
announcement. The original handoff's numbered cases remain the acceptance inventory;
source code, simulated snapshots, cross-compilation, and configured workflows do
not substitute for native execution or real controller results.

The latest completed native checkpoint is [CI run
39050303823](https://github.com/samishal1998/takt/actions/runs/39050303823), commit
`5afd65a561bdfb7c89c8945fe1739cdd71b3f567`. All three native jobs and the Linux
compatibility guest passed. The overall run failed because both Linux safety
harnesses encountered `ENODEV` while sampling cgroups being removed. The sampler
fix, stronger safety assertions, and subsequent asynchronous launch changes still
need a complete remote rerun. The updated launch implementation passes the local
105-test checkpoint below. Earlier remote evidence does not validate those changes.

## Confirmed local checkpoints

The development host is Linux 6.8.0-79-generic x86-64 with glibc 2.39. The compiler
is `rustc 1.99.0 (b940084d7 2026-09-28)`.

| Check | Confirmed result |
|---|---|
| `cargo check --workspace --all-targets -j2` | Passed after adapting imports/options to parser 2.0 APIs |
| `cargo test --test config_scheduler -j2` | 38 passed, including generated-DAG/weighted-admission properties, explicit-auto provenance, alias/default/ancestry expansion budgets, and failures observed before cleanup |
| Independent library unit tests | 19 passed, including inherited parent cpuset discovery, bounded/nonblocking launch progress, absolute launch deadlines, handshake validation, and cancellation before the execution gate |
| `cargo test --test integration -j2 -- --test-threads=1` | 33 passed, including concurrent/queued launch fail-fast behavior and non-UTF policy-environment validation. Every parsed NDJSON event is checked against the published schema |
| Probe/output checks | Eight passed |
| Configuration schema | Seven maintained YAML/JSON examples validated structurally and against Rust parsing/graph rules |
| macOS cross-check | Library cross-check passed; subsequent native execution is recorded separately below |
| `cargo test --test events_schema -j2` | Three passed: event payload/envelope rejection, neutral-skip summary, and configuration-schema examples |
| `cargo fmt --all --check` | Passed after asynchronous launch integration |
| `cargo clippy --workspace --all-targets -j2 -- -D warnings` | Passed after the asynchronous launch change |
| `process_lifecycle` independent tests | Four default tests passed in 0.46 s; two capability-gated cases ignored outside their required environments |
| Real Linux protection | Nine bounded GNU debug controller cases and the required loader-constructor containment test passed inside a delegated user systemd unit |
| musl release execution | The GNU-hosted independent harness ran 31 integration and four lifecycle cases successfully with its executable override pointing to the musl release artifact |
| Measurements | Durable VPS performance and repository comparisons are published in [performance.md](performance.md); reviewed native size baselines and native observations are recorded below |

The complete local `cargo test --workspace -j2 -- --test-threads=1` checkpoint
contains 105 passing tests: 19 library, 38 configuration/scheduler, three schema,
33 integration, eight probe/output, and four process-lifecycle tests, with two
explicitly gated lifecycle proofs ignored in that default invocation. The local
log is `/tmp/takt-final-tests.log`.

`ManagedChild::start` creates the owned launcher; `poll_launch` advances its
handshake without blocking the supervision loop. Launch progress has a per-turn
byte bound and an absolute deadline, waits for the helper greeting before sending
task data, and requires execution EOF before reporting a started task. Tests
cover stalled progress, invalid handshakes, preserved execution errors, and
cancellation before the execution gate while retaining process identity through
cleanup. Concurrent and queued launch failure regressions also pass. Remote
native, controller, and compatibility results for this revision remain pending.
Update this table with later outcomes instead of treating earlier evidence as
validation of later edits.

## Native CI and artifact checkpoints

These results concern commit `5afd65a561bdfb7c89c8945fe1739cdd71b3f567`, before
the asynchronous launch change. Native jobs run independent checks, package and
extract archives, repeat integration/lifecycle checks against the extracted
executables, inspect dependencies, and retain 60-second performance observations.

| Lane | Confirmed result |
|---|---|
| Linux GNU | [Native job passed](https://github.com/samishal1998/takt/actions/runs/39050303823/job/117447356680) on Ubuntu 22.04 |
| Linux musl | [Native job passed](https://github.com/samishal1998/takt/actions/runs/39050303823/job/117447356701) on Ubuntu 22.04, including the no-dynamic-loader/shared-library check |
| macOS Apple Silicon | [Native job passed](https://github.com/samishal1998/takt/actions/runs/39050303823/job/117447356723), including portable lifecycle/probe/output checks and required-protection refusal; this is native execution, not a cross-check |
| Linux compatibility floor | [Guest job passed](https://github.com/samishal1998/takt/actions/runs/39050303823/job/117448268408). Guest serial output reports Linux `5.15.0-198-generic` and glibc `2.35`; both extracted GNU and musl archives pass checksum and smoke checks, including `localhost` resolution |
| GNU controller checkpoint | [Safety job failed](https://github.com/samishal1998/takt/actions/runs/39050303823/job/117447356533) in the sampling harness after successful task/aggregate OOM, CPU quota, immediate/detached tree, and exit-137 cases. The required loader-constructor test passed in 0.20 s |
| musl controller checkpoint | [Safety job failed](https://github.com/samishal1998/takt/actions/runs/39050303823/job/117447356665) in the sampling harness after successful task/aggregate OOM, CPU quota, immediate/detached tree, and exit-137 cases |

Downloaded artifacts for that run are under
`target/ci-evidence/39050303823/`. The compatibility artifact's
`guest-serial.log` records the actual guest platform and final `PASS`. Safety
artifacts preserve outer bounds, delegation, unprivileged workload identity,
controller counters, task outcomes, and GNU loader membership. The verified
0.1-CPU task consumed 159,576 µs over 1.53 seconds on GNU and 157,126 µs over
1.51 seconds on musl, with 15 throttled periods in each case.

The `ENODEV` fix treats only disappeared-controller errors as sampling races;
permission errors still fail. Stronger assertions require an already-running
survivor during task OOM, distinct aggregate-limit classification, ordinary
exit-137 classification, and repeated timeout/cancellation cleanup. These checks,
the complete cleanup sequence, privileged refusal/ancestor cases, and external
supervisor-death cleanup await a successful full safety rerun. Neither failed
safety job is recorded as a passing gate.

## Measured size and performance

The reviewed [size baseline](../scripts/size-baseline.json) comes from native
artifacts in [CI run 39049559556](https://github.com/samishal1998/takt/actions/runs/39049559556),
commit `16c91e9347e217793f6f3304f2933cea06a62609`. That run's successful native
jobs retained these full-feature release measurements; its failed safety and
compatibility jobs are not part of the size acceptance.

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
guarantee is inferred from them. The later run 39050303823 measured GNU at
2,169,824 bytes and 1.65% idle CPU, and musl at 2,294,624 bytes and 1.70% idle CPU;
both stayed within the size-regression threshold.

[Performance documentation](performance.md) retains the separate VPS fixture
measurements and their [raw report](evidence/linux-vps-performance.json), plus
the [repository comparison](evidence/linux-vps-repository.json). Identical
two-slot repository checks had medians of 9.825 seconds through Takt and 9.484
seconds through the shell over three measured runs each, with overlapping ranges.
Those results support no speed-superiority claim. Final launch changes require
new artifact hashes and measurements; these reports describe earlier checkpoints.

## Requirement traceability

| Handoff cases | Implementation/evidence | Remaining acceptance work |
|---|---|---|
| C01, C03–C07, C09–C11 | Strict parser, graph validation, defaults/env/cwd handling; parser and expansion budgets; examples and independent schema/model tests pass in the native matrix | Expanded release property cases and retained failing seeds if any; repeat final-source native checks |
| C02, C08 | CLI precedence/provenance, explicit-auto file metadata, and explicit argv/shell execution; exact-argument and precedence tests pass natively | Repeat final-source matrix after launch changes |
| S01–S07, S09–S10 | Injected clock/snapshots; weighted reservations, fractional CPU, explicit-versus-inferred requests, live-memory accounting, FIFO/aging, separate queue deadline; deterministic/property tests pass natively | Complete privileged ancestor/unknown-capability evidence and final-source rerun |
| S08, S16 | Static/live service-deadlock checks and shared-service lifetime tests pass; unfinished prerequisites retain required services after a root skips | Repeat service/cleanup regressions after asynchronous launch integration |
| S11–S15 | Exit predicates remain separate from success; neutral skips, fail-fast, and stop-dependents tests pass; known failures affect policy before cleanup releases reservations | Final launch/cancellation race regressions across all native targets |
| S17 | Platform discovery observes visible ancestor limits and affinity; no inter-runner reservation coordination is claimed | Nested-runner/sibling native evidence |
| R01–R09 | Four probes pass the native matrix, including malformed/oversized HTTP, wrong UDP peer/payload, immediate/split stdio, and deadline regressions; baseline guest checks hostname resolution | Final slow-DNS and asynchronous-launch deadline evidence; retain final artifact results |
| R10–R14 | Native tests pass for pre/post-readiness exit, default consumer continuation, transitive cancellation, and finite/persistent roots | Repeat service-exit/launch and interruption races after asynchronous launch changes |
| P01–P03, P10–P14 | Native pipe/output tests pass, including byte-safe NDJSON, blocked/broken consumers, grouped cleanup/fallback, and live schema validation | Expanded spool-disk failure/size-limit cases and final-source native rerun |
| P04–P08 | Earlier native GNU/musl/macOS lifecycle tests pass; GNU loader constructor demonstrates attachment before project loader code. Local asynchronous-launch, handshake cancellation, and concurrent failure regressions now pass | Repeat updated launch/cleanup checks across the remote native and controller matrix |
| P09 | Portable parent-death cleanup is not promised; release safety workflow retains external systemd cleanup | Execute and retain supervisor-death controller test |
| L01–L10 | Local GNU proof plus partial remote GNU/musl release-artifact evidence covers OOM, quota, immediate/detached trees, swap capability, and ordinary exit 137; GNU loader proof passes | Corrected sampler/stronger assertions; complete repeated cleanup, L06/L07, conditional absent-swap coverage, and both successful remote safety gates |
| Native macOS | Apple Silicon native suite and extracted-archive tests passed on macOS 15; native size/RSS/CPU observations retained | Repeat against final launch implementation; fallback remains best effort |
| Artifact compatibility | All three native archive checks passed; actual Linux 5.15/glibc 2.35 guest ran both extracted Linux artifacts successfully | Regenerate and validate final artifacts; verify release hashes and publication gates |
| Performance/size | Reviewed per-target size baseline; native 60-second observations and durable VPS fixture/repository comparisons published | Remeasure changed artifacts; retain missed size/idle-CPU goals and avoid unsupported speed claims |

## Milestone gates

| Milestone | Current interpretation |
|---|---|
| M0 | Accepted decisions and measured dependency/size baseline exist; native platform and GNU loader containment evidence passes at the recorded checkpoint; revised launch path passes locally and awaits remote validation |
| M1 | Independent finite/process/output suites passed on GNU, musl, and native macOS; final-source rerun remains |
| M2 | Configuration, graph, scheduler properties, provenance, and schema/example checks pass in the native matrix; preserve these checks through final changes |
| M3 | Local safety proof and partial GNU/musl release controller results pass, but both remote safety jobs remain failed; complete corrected safety runs are required |
| M4 | Four probes and service policies pass natively; revised launch/race/deadline evidence remains |
| M5 | Native output/schema checks and documentation exist; final failure-path coverage and updated artifacts remain |
| M6 | Native archives and actual compatibility guest passed; size/overhead and repository comparisons are measured. No final release acceptance until revised source passes native, safety, compatibility, and publication gates |

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
