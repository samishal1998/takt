# Implementation status and acceptance evidence

## v0.1.1 release checkpoint

[Takt v0.1.1](https://github.com/samishal1998/takt/releases/tag/v0.1.1) was published
on 2026-10-19 from commit `3fd71d79f2eadcc3b97fa58c789ffa3b13f25369` after all eight
jobs in [release run 39191247972](https://github.com/samishal1998/takt/actions/runs/39191247972)
passed. This patch improves best-effort descendant RSS accounting and cleanup
after observed descendants leave their original process group, and avoids
repeated SIGKILL broadcasts to zombie-only groups on macOS.

The [native Apple Silicon job](https://github.com/samishal1998/takt/actions/runs/39191247972/job/117864370335)
passed all four new descendant regressions for per-task memory, aggregate memory,
cancellation, and parent-exit cleanup in both the independent and extracted-archive
suites. The zombie-only process-group unit regression also passed. The
[runtime contract](runtime.md#lifecycle-and-enforcement) describes the remaining
sampling and process-identity limitations.

## v0.1.0 acceptance record

The detailed test counts, CI links, size/performance measurements, and milestone
tables below preserve the historical v0.1.0 acceptance record. They are not
measurements of v0.1.1.

Release checkpoint: 2026-10-17. [Takt v0.1.0](https://github.com/samishal1998/takt/releases/tag/v0.1.0)
was published from commit `321b13ecf8fd8ddf61b490783119c88fe1c1ed90` after
[release run 39052589648](https://github.com/samishal1998/takt/actions/runs/39052589648)
passed all three native jobs, both Linux controller safety jobs, the actual Linux
compatibility guest, and publication. The matching [main CI run
39052584350](https://github.com/samishal1998/takt/actions/runs/39052584350) also
passed. M0–M6 are complete for the approved GNU, musl, and Apple Silicon targets.
The nonblocking size and idle-CPU goals remain
unmet and are disclosed below.

The original handoff's numbered cases remain the acceptance inventory. Source
code, simulated snapshots, cross-compilation, and configured workflows do not
substitute for native execution or real controller results. The final release
execution includes P12 spool failures, R01/R04 readiness failures, asynchronous
launch/cancellation, and S17 nested runners with a live sibling.

## Confirmed tests

The development host is Linux 6.8.0-79-generic x86-64 with glibc 2.39. The compiler
is `rustc 1.99.0 (b940084d7 2026-09-28)`.

| Check | Confirmed result |
|---|---|
| `cargo check --workspace --all-targets -j2` | Passed after adapting imports/options to parser 2.0 APIs |
| `cargo test --test config_scheduler -j2` | 38 passed, including generated-DAG/weighted-admission properties, explicit-auto provenance, alias/default/ancestry expansion budgets, and failures observed before cleanup |
| Independent library unit tests | 23 passed on Linux and 20 on macOS, including asynchronous launch/cancellation, shared grouped-spool bounds, partial/zero-write failure, and stalled-resolver deadlines/cleanup |
| Independent integration tests | 33 passed on Linux and 34 on macOS, including concurrent/queued launch fail-fast behavior and non-UTF policy-environment validation. Every parsed NDJSON event is checked against the published schema |
| Probe/output checks | 11 passed on every target, including unavailable TCP/deadline, delayed-listener recovery, and bounded failed hostname lookup |
| Configuration schema | Seven maintained YAML/JSON examples validated structurally and against Rust parsing/graph rules |
| macOS cross-check | Library cross-check passed; subsequent native execution is recorded separately below |
| `cargo test --test events_schema -j2` | Three passed: event payload/envelope rejection, neutral-skip summary, and configuration-schema examples |
| `cargo fmt --all --check` | Passed in final native release checks |
| `cargo clippy --workspace --all-targets -j2 -- -D warnings` | Passed in final native release checks |
| `process_lifecycle` independent tests | Four default tests passed on every target; Linux capability/compiler-gated proofs ran separately in their required environments |
| Real Linux protection | 23 bounded case reports per GNU/musl target, separate supervisor-death cleanup, and GNU loader-constructor containment passed in final release safety jobs |
| Measurements | Durable VPS performance and repository comparisons are published in [performance.md](performance.md); reviewed native size baselines and native observations are recorded below |

The final native default suites contain 112 passing tests on Linux and 110 on
macOS; platform-specific tests account for the difference. The two explicitly
gated Linux lifecycle proofs are ignored by the default invocation and executed
separately where applicable. Release property tests use 2,048 cases. Each native
job also repeats integration, probe/output, and lifecycle checks against the
extracted release executable. The earlier complete local Linux checkpoint is
retained in `/tmp/takt-release-tests.log`.

`ManagedChild::start` creates the owned launcher; `poll_launch` advances its
handshake without blocking the supervision loop. Launch progress has a per-turn
byte bound and an absolute deadline, waits for the helper greeting before sending
task data, and requires execution EOF before reporting a started task. Tests
cover stalled progress, invalid handshakes, preserved execution errors, and
cancellation before the execution gate while retaining process identity through
cleanup. Concurrent and queued launch failure regressions pass locally and in
the final native release matrix.

The native suite additionally verifies a shared stdout/stderr spool cap,
exact retained/discarded-byte accounting after partial `ENOSPC` writes, and
zero-write failure. A failed spool stops accepting writes while capture continues.
TCP tests retain one bound socket through refusal or attempt timeout and delayed
listening; DNS tests cover a real `.invalid` lookup deadline and deterministic stalled helpers,
including the two-resolver cap, cancellation, and reaping. Packaged-artifact
checks include the probe suite. These cases pass on GNU, musl, and native macOS;
S17 passes in both Linux safety lanes.

## Native CI and artifact checkpoints

These results concern released commit `321b13ecf8fd8ddf61b490783119c88fe1c1ed90`.
Native jobs run independent checks, package and extract archives, repeat
integration/probe/lifecycle checks against the extracted executables, inspect
dependencies, and retain 60-second performance observations.

| Lane | Confirmed result |
|---|---|
| Linux GNU | [Native job passed](https://github.com/samishal1998/takt/actions/runs/39052589648/job/117454034960) on Ubuntu 22.04; highest required glibc symbol version is 2.34, below the promised 2.35 floor |
| Linux musl | [Native job passed](https://github.com/samishal1998/takt/actions/runs/39052589648/job/117454034920) on Ubuntu 22.04, including the no-dynamic-loader/shared-library check |
| macOS Apple Silicon | [Native job passed](https://github.com/samishal1998/takt/actions/runs/39052589648/job/117454034937) on macOS 15.7.9, including portable lifecycle/probe/output checks and required-protection refusal; Mach-O minimum OS is 15.0 |
| Linux compatibility floor | [Guest job passed](https://github.com/samishal1998/takt/actions/runs/39052589648/job/117454754038). Guest serial output reports Linux `5.15.0-198-generic` and glibc `2.35`; both extracted GNU and musl archives pass checksum and smoke checks, including `localhost` resolution |
| GNU controllers | [Safety job passed](https://github.com/samishal1998/takt/actions/runs/39052589648/job/117454034925): 23 independent case reports, external supervisor-death cleanup, and required loader-constructor containment proof |
| musl controllers | [Safety job passed](https://github.com/samishal1998/takt/actions/runs/39052589648/job/117454034981): 23 independent case reports and external supervisor-death cleanup |
| Publication | [Publish job passed](https://github.com/samishal1998/takt/actions/runs/39052589648/job/117455580110); the public release contains all three archives, individual checksums, `SHA256SUMS`, and six size/performance reports |

Downloaded artifacts for that run are under
`target/ci-evidence/39052589648/`. The compatibility artifact's
`guest-serial.log` records the actual guest platform and final
`TAKT_COMPATIBILITY_PASS`. Safety artifacts preserve outer bounds, delegation,
unprivileged workload identity,
controller counters, task outcomes, and GNU loader membership. The verified
0.1-CPU task consumed 159,216 µs over 1.53 seconds on GNU and 158,175 µs over
1.56 seconds on musl, with 15 throttled periods in each case.

An independent post-publication download verified all three archives against
the published `SHA256SUMS` and the exact accepted CI hashes. The downloaded GNU
executable passed `--version`, `doctor`, and direct `exec` smoke checks.
Those downloads are retained in `target/release-verification/v0.1.0/`.

The ordinary safety workloads ran as UID 1001. Every outer safety unit verified
512 MiB memory, 32 tasks, a 20-second deadline, zero swap, control-group cleanup,
and forced-kill availability. Task OOM left an already-running peer alive to
finish; aggregate OOM gave both tasks `aggregate_memory_limit`, and ordinary
exit 137 remained `normal_exit`. Repeated success, timeout, and cancellation cases
left no owned cgroups. Read-only, denied, and missing delegation checks passed;
`auto` reported fallback where applicable. Real 0.5-CPU/128-MiB ancestor tests
verified CPU ceilings and available memory headroom. Independent systemd cleanup
removed all owned groups after supervisor SIGKILL. Swap-present behavior was
exercised; the conditional
absent-swap branch was not exercised on these capable hosts.

S17 ran nested Takt invocations and a live sibling holding a 16-MiB payload under
a shared 0.5-CPU/96-MiB ceiling. Both outer and inner runners selected 500
millicores and accounted for shared memory headroom while the sibling remained
alive. Their reservations remain independent per runner.

| Target | Sibling charged bytes | Outer memory budget | Inner memory budget |
|---|---:|---:|---:|
| GNU | 17,653,760 | 81,342,464 | 79,470,592 |
| musl | 17,657,856 | 81,969,152 | 78,090,240 |

The per-target `nested-runner-sibling-ceilings.json` files preserve the real
ancestor limits, sibling charge, UID 1001, both discovered budgets, and their
provenance; both nested runners completed successfully.

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
guarantee is inferred from them.

The published artifacts from release run 39052589648 have these measurements:

| Native target | Executable bytes | Archive bytes | Median idle RSS bytes | Idle CPU, percent of one core over 60 seconds |
|---|---:|---:|---:|---:|
| Linux GNU | 2,171,544 | 1,077,724 | 3,670,016 | 0.900% |
| Linux musl | 2,294,624 | 1,144,233 | 1,835,008 | 1.450% |
| macOS Apple Silicon | 1,604,432 | 968,575 | 3,293,184 | 0.136% |

All pass the size-growth gate and the RSS goal; all still miss the absolute
size and idle-CPU goals. The release's per-target size/performance reports and
[checksum manifest](https://github.com/samishal1998/takt/releases/download/v0.1.0/SHA256SUMS)
identify the exact measured artifacts.

[Performance documentation](performance.md) retains the separate VPS fixture
measurements and their [raw report](evidence/linux-vps-performance.json), plus
the [repository comparison](evidence/linux-vps-repository.json). Identical
two-slot repository checks had medians of 9.825 seconds through Takt and 9.484
seconds through the shell over three measured runs each, with overlapping ranges.
Those results support no speed-superiority claim. The VPS comparisons describe
their recorded earlier checkpoints; the native release measurements above
describe the published artifacts.

## Requirement traceability

| Handoff cases | Implementation/evidence | Release acceptance result |
|---|---|---|
| C01, C03–C07, C09–C11 | Strict parser, graph validation, defaults/env/cwd handling; parser and expansion budgets; examples and independent schema/model tests | Passed on all native targets with 2,048 release property cases |
| C02, C08 | CLI precedence/provenance, explicit-auto file metadata, explicit argv/shell execution, exact arguments | Passed in final native and extracted-artifact checks |
| S01–S07, S09–S10 | Deterministic/property tests, real ancestor ceilings, unavailable capabilities | Passed in final native matrix and both Linux safety lanes |
| S08, S16 | Static/live service-deadlock checks, shared-service lifetime, retained prerequisites after neutral root skips | Passed in final native matrix |
| S11–S15 | Exit predicates separate from success; neutral skips, fail-fast, stop-dependents, asynchronous launch | Passed in final native matrix |
| S17 | Nested runners observe the shared 0.5-CPU/96-MiB ancestor and a live memory-consuming sibling | Passed for GNU and musl; numeric evidence above |
| R01–R09 | Four probes; unavailable/delayed TCP, real failed lookup, deterministic stalled-resolver deadlines, cancellation, and cleanup | Passed in native and extracted-artifact lanes; private resolver tests pass natively |
| R10–R14 | Pre/post-readiness exit, consumer policies, finite/persistent roots, asynchronous-launch interactions | Passed in final native matrix |
| P01–P03, P10–P14 | Output/events; P12 shared spool limits, partial disk-write failure, zero writes, truncation accounting, continued capture, cleanup | Passed in final native matrix and relevant extracted-artifact checks |
| P04–P08 | Asynchronous lifecycle; GNU loader constructor already attached before project loader code | Passed on all native targets and in GNU containment proof |
| P09 | External systemd cleanup after supervisor SIGKILL | Passed for GNU and musl; portable parent-death cleanup is not promised |
| L01–L10 | OOM, quotas, descendants, refusal/fallback, ancestors, swap-present reporting, ordinary exit 137, repeated cleanup, GNU loader proof | Both final safety lanes passed; absent-swap execution remains conditional and unclaimed on capable hosts |
| Native macOS | Apple Silicon native suite and extracted-archive checks on macOS 15, including latest spool/probe tests | Passed; fallback remains best effort |
| Artifact compatibility | All three native archives; actual Linux 5.15/glibc 2.35 guest with both Linux artifacts | Passed, published, and downloaded archives independently reverified |
| Performance/size | Reviewed size baseline; final native 60-second observations; durable VPS fixture/repository comparisons | Growth gate and RSS goal pass; nonblocking size/idle-CPU goals remain missed; no speed-superiority claim |

## Milestone gates

| Milestone | Completed gate |
|---|---|
| M0 | Complete: accepted decisions, measured baseline, native platforms, and GNU loader containment proof |
| M1 | Complete: independent finite/process/output suites and extracted artifacts pass on GNU, musl, and native macOS |
| M2 | Complete: configuration, graph, scheduler properties, provenance, and schema/example checks pass in the release matrix |
| M3 | Complete: both controller safety lanes pass, including S17 shared ancestors and external supervisor-death cleanup |
| M4 | Complete: all four probes and asynchronous service lifecycle pass; R01/R04 failure-path coverage passes natively |
| M5 | Complete: native output/event/schema checks and P12 spool failure coverage pass |
| M6 | Complete: all native archives, controller lanes, actual compatibility guest, checksum/publication gates, and independent published downloads pass; performance/size and repository comparisons are measured with limitations disclosed |

## Material boundaries

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
goal on the published artifacts. All required release evidence for the approved
scope is recorded above; conditional absent-swap execution remains unclaimed.
