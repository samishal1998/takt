# Implementation status and acceptance evidence

Checkpoint: 2026-10-17. This is an implementation and evidence ledger, not a release
announcement. The original handoff's numbered cases remain the acceptance inventory;
source code, simulated snapshots, cross-compilation, and configured workflows do
not substitute for native execution or real controller results.

## Confirmed local checkpoints

The development host is Linux 6.8.0-79-generic x86-64 with glibc 2.39. The compiler
is `rustc 1.99.0 (b940084d7 2026-09-28)`.

| Check | Confirmed result |
|---|---|
| `cargo check --workspace --all-targets -j2` | Passed after adapting imports/options to parser 2.0 APIs |
| `cargo test --test config_scheduler -j2` | 34 passed, including generated-DAG/weighted-admission properties, explicit-auto provenance, alias-expansion budgets, and failures observed before cleanup |
| Independent library unit tests | 13 passed |
| `cargo test --test integration -j2 -- --test-threads=1` | 31 passed, with every parsed NDJSON event checked against the published schema |
| Probe/output checks | Eight passed |
| Configuration schema | Seven maintained YAML/JSON examples validated structurally and against Rust parsing/graph rules |
| macOS cross-check | Library cross-check passed; no native macOS execution is implied |
| `cargo test --test events_schema -j2` | Three passed: event payload/envelope rejection, neutral-skip summary, and configuration-schema examples |
| `cargo clippy --workspace --all-targets -j2 -- -D warnings` | Passed after schema-validator integration; final pipeline rerunning after cancellation and poll-error cleanup fixes |
| `process_lifecycle` independent tests | Four default tests passed in 0.44 s; two capability-gated cases ignored outside their required environments |
| Real Linux protection | Nine bounded GNU debug controller cases and the required loader-constructor containment test passed inside a delegated user systemd unit |
| Release measurements | No reviewed executable-size/RSS/idle-CPU baseline recorded at this checkpoint |

Tests and counts will increase during integration. Update this table with the final
commands and outcomes instead of treating an earlier passing checkpoint as evidence
for later edits. Root-level workflow logs and retained artifacts are the authority
for remote gates.

## Requirement traceability

| Handoff cases | Implementation/evidence | Remaining acceptance work |
|---|---|---|
| C01, C03–C07, C09–C11 | Strict parser, graph validation, defaults/env/cwd handling; config tests reject duplicate keys, tags, deep/large/alias-heavy input, invalid units, missing references, cycles, and service-edge mismatches | Retain failing-seed/fuzz evidence before release; event/config schemas now join independent validation |
| C02, C08 | CLI precedence/provenance, explicit-auto file metadata, and explicit argv/shell execution; independent exact-argument and precedence tests pass | Final native matrix remains |
| S01–S07, S09–S10 | Injected clock/snapshots; weighted reservations, fractional CPU, explicit-versus-inferred requests, memory-pressure accounting, FIFO/aging, separate queue deadline; deterministic/property tests pass | Native measured discovery evidence on all targets, including unknown/hidden constraints |
| S08, S16 | Static ancestor-service and live-service deadlock checks; shared temporary-service lifetime tests; unfinished prerequisites retain required services after a root skips | Native service/cleanup races on both operating systems |
| S11–S15 | Exit-code predicates independent of success; neutral skip propagation; signal/timeout/OOM do not match exit predicates; fail-fast and stop-dependents tests; failures affect policy while reservations remain held through cleanup | Native lifecycle evidence on all release targets |
| S17 | Platform discovery observes visible ancestor limits and affinity; no inter-runner reservation coordination is claimed | Nested-runner/sibling native evidence |
| R01–R09 | TCP/HTTP/UDP and stdio matcher implemented; initial black-box probe tests pass; DNS workers and responses are bounded | Final slow-DNS, malformed/oversized HTTP, peer-validation, immediate/split-output and deadline cases across all artifacts |
| R10–R14 | Unexpected exit, future-dependent blocking, default active-consumer continuation, optional transitive cancellation, finite versus persistent roots | Native race and persistent-service interruption evidence |
| P01–P03, P10–P14 | Concurrent pipe draining, bounded renderer/spool queues, byte-safe NDJSON, output truncation/failure paths; blocked/broken consumer and grouped-directory failure tests pass; live schema validation checks sequences and summary counts | Expanded spool-disk failure/size-limit and cross-platform output evidence |
| P04–P08 | Separate process groups; controlled attach-before-exec helper; graceful/forced cancellation; cleanup before reservation release; spawn versus exit classification | Current descendant-cleanup and signal-race fixture expansion must pass independently |
| P09 | Portable parent-death cleanup is not promised; release safety workflow retains external systemd cleanup | Execute and retain supervisor-death controller test |
| L01–L10 | Local GNU debug proof covers per-task/aggregate OOM, 0.1 CPU quota, immediate/detached descendants, zero-swap capability, exit 137, repeated cleanup, and loader-constructor attachment | Release GNU/musl artifacts, denied/read-only delegation (L06), ancestor-ceiling variants (L07), absent-swap variant, and remote safety gate |
| Native macOS | Native adapters and refusal/fallback implementation; Apple Silicon native workflow configured | Execute applicable process/probe/cancellation/watchdog suite on macOS 15 |
| Artifact compatibility | GNU/musl/macOS packaging, dependency checks, extracted-binary tests, and checksum generation configured; Linux baseline guest lane defined | Native extracted artifacts on all three targets; actual Linux 5.15/glibc 2.35 guest execution |
| Performance/size | Measurement and regression scripts present; production has no fixture/schema-validation dependency | Publish measured size, linked dependencies, startup/RSS/idle CPU, representative workloads, and reviewed baseline |

## Milestone gates

| Milestone | Current interpretation |
|---|---|
| M0 | Decisions, package scaffold, dependencies, parser model, and launch/protection implementation exist; native containment evidence is still required |
| M1 | Finite runner has passed the initial independent GNU Linux suite; macOS and final process/output cases remain |
| M2 | Configuration/graph/scheduler, CLI explicit-auto provenance, and event-schema integration pass locally, including properties; final independent pipeline is rerunning after runtime cleanup changes |
| M3 | Local bounded GNU controller proof passed; full target matrix and L06/L07/absent-swap variants remain |
| M4 | All four probes and service policies are implemented; final cross-platform race/deadline evidence remains |
| M5 | Renderers, schemas, diagnostics, documentation, blocked-consumer and live event-contract checks pass locally; remaining artifact/platform gates still apply |
| M6 | Release automation exists; no release is approved until every required native, safety, compatibility, and artifact gate passes |

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

Local success on glibc 2.39 does not prove the promised glibc 2.35 floor. macOS
cross-checks do not prove runtime behavior. A safety test skipped for missing
delegation is not a passing hard-limit gate. Binary size and runtime-memory goals
remain unmeasured until their reports are retained. These are outstanding evidence
requirements, not reductions in the approved release scope.
