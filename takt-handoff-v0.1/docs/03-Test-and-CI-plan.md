# Takt — Test, CI, and Release Acceptance Plan

**Version:** 0.1 · **Date:** 16 October 2026 · **Status:** test specification, not executed test results

## 1. Evidence hierarchy

Use four complementary layers: pure scheduler/graph tests, controlled process fixtures, real OS containment tests, and representative project dogfooding. A synthetic resource snapshot proves a scheduling decision, not kernel enforcement. A successful compile proves neither native execution nor cleanup correctness.

An independent harness must build and test the runner before Takt schedules any of its own checks. Preserve that independent path permanently.

## 2. Fixture executable

Build a development-only Rust fixture executable with bounded behaviors: sleep/exit, alternate stdout/stderr, emit partial/no-newline/non-UTF-8 output, fork a bounded tree, ignore a signal, spawn immediately, detach where supported, allocate/touch a bounded number of memory pages, burn CPU for a bounded duration, TCP listener, configurable HTTP responder, UDP echo/protocol responder, startup announcement, and delayed exit.

Every fixture accepts a maximum duration and bounded allocation/output/child count. The test harness retains an independent cleanup handle. Use loopback addresses, ephemeral ports allocated by the fixture, and explicit handshakes instead of races around a guessed “free port.” No internet access, production credentials, fork bombs, or unbounded allocators are required.

## 3. Required test inventory

### Configuration and graph

| ID | Scenario | Required assertion |
|---|---|---|
| C01 | Equivalent YAML and JSON | Same normalized model and decisions |
| C02 | CLI/env/file/auto precedence | Exact winning source reported per field |
| C03 | Invalid units, zero/negative/overflow values | Rejected with field-level error |
| C04 | Unknown fields, duplicate keys, unsupported version | Rejected before any task starts |
| C05 | Malformed/deep/alias-heavy/oversized YAML | Bounded parse failure; no runaway allocation |
| C06 | Multiple discovered config files | Ambiguity error, no silent precedence |
| C07 | Missing task, cycle, duplicate edge, name collision | Actionable graph error and cycle path |
| C08 | Direct argv quoting versus explicit shell | Exact arguments received; no hidden splitting |
| C09 | Config-relative working directories/environment removal | Reproducible paths; no secret dump |
| C10 | Shared prerequisite | Runs once per invocation |
| C11 | Invalid `ready`/`exit` kind combination | Validation rejects |

### Admission and dependency policies

| ID | Scenario | Required assertion |
|---|---|---|
| S01 | One versus multiple slots | Active tree count never exceeds budget |
| S02 | CPU-weighted and memory-weighted requests | Neither reservation sum exceeds capacity |
| S03 | Fractional CPU allowance | Fractional capacity preserved; one feasible task can run |
| S04 | Unspecified versus explicit oversized request | Only inferred default may shrink; explicit excess errors |
| S05 | Low/unknown memory or unreadable limit | Conservative documented behavior and provenance |
| S06 | Current memory plus outstanding reservations | No double subtraction or unsafe omission |
| S07 | Eligibility/FIFO ties and later smaller job | Stable decisions and bounded starvation |
| S08 | Service consumes only slot/remaining budget | No deadlock; unsatisfiable policy diagnosed |
| S09 | Dynamic pressure and pressure relief | Admission pauses/resumes without cancelling healthy tasks |
| S10 | Resource queue timeout versus dependency wait | Correct clock and termination reason |
| S11 | Success exit edge | Dependent cannot launch early |
| S12 | Expected nonzero with two conditional branches | Matching branch runs; other branch skips neutrally |
| S13 | Matching nonzero edge without success-code declaration | Upstream remains failed; policy outcome is explicit |
| S14 | Signal/timeout/OOM on prerequisite | Does not match a normal exit-code predicate |
| S15 | Continue versus fail-fast | Independent results collected or owned work cancelled as specified |
| S16 | Shared service and multiple finite roots | Lifetime lasts through all consumers, then cleanup |
| S17 | Nested runners and sibling processes | Observe real external ceilings; no global coordination claim |

Property tests generate small DAGs, budgets, outcomes, and event orderings. Invariants: no duplicate launch; no reservation underflow; no release before cleanup; all launched tasks had satisfied prerequisites; terminal states do not become running again; completed services do not remain advertised ready; and every finite feasible run eventually settles when fixtures terminate.

### Services and readiness

| ID | Scenario | Required assertion |
|---|---|---|
| R01 | TCP startup delay/refusal/timeout | Bounded attempts and correct readiness transition |
| R02 | TCP port already belongs to another listener | Document protocol/ownership limitation; no ownership claim |
| R03 | HTTP 503→200, body mismatch, oversized response | Match configured status/body within bounds |
| R04 | Redirect, unsupported scheme, bad hostname | No implicit insecure fallback; deadline remains bounded |
| R05 | UDP send succeeds but no reply | Never ready |
| R06 | UDP wrong peer/wrong payload/correct reply | Only configured peer and predicate match |
| R07 | Immediate stdio readiness before normal event loop work | Startup match retained |
| R08 | Pattern split across chunks or emitted without newline | Correct bounded streaming match |
| R09 | stderr-only readiness and invalid UTF-8 | Stream choice and byte handling remain correct |
| R10 | Service exits before readiness | Dependent blocked with original cause |
| R11 | Service exits after readiness | Run fails; active dependents continue by default |
| R12 | Service dies during dependent-launch decision | No stale-ready guarantee; outcome recorded coherently |
| R13 | Stop-dependents policy | Correct transitive subset cancelled, independent roots unaffected |
| R14 | Finite group finishes/persistent group remains | Correct automatic teardown/continued supervision |

### Supervision, logs, and cancellation

| ID | Scenario | Required assertion |
|---|---|---|
| P01 | Dual-stream output exceeds pipe capacity | No deadlock or lost process exit |
| P02 | Very long line and continuous log stream | Bounded RAM; explicit truncation where applicable |
| P03 | Slow output reader/broken pipe | Cancellation/reaping stay responsive; documented exit |
| P04 | SIGINT/SIGTERM and second interrupt | Grace/escalation and runner exit code are correct |
| P05 | Ignoring child and grandchild | Surviving owned workload is forcefully cleaned up |
| P06 | Shell exits while descendants remain | Reservations retained through owned cleanup |
| P07 | Detached descendant and PID reuse | Containment/limitations reported; no unrelated PID killed |
| P08 | Spawn failure and attach-before-exec failure | No project execution; partial resources cleaned |
| P09 | Supervisor killed unexpectedly | Linux external cleanup tested; portable guarantee not invented |
| P10 | JSON mode | Parseable NDJSON only on stdout, monotonic sequence |
| P11 | No-color, narrow/non-TTY terminal | Status understandable without color or animation |
| P12 | Grouped spool fills/fails/permissions denied | Clear bounded fallback; child pipes still drained |
| P13 | NODE_OPTIONS configured/invalid for Node | Runner unaffected; Node child's outcome preserved |
| P14 | Secret environment values | Not included in automatic diagnostics/events |

## 4. Real Linux safety tests

These tests run only inside an explicitly isolated, owned safety environment. **Do not attempt to exhaust a whole hosted runner.** Establish an independent outer boundary and watchdog before invoking Takt. A proposed fixture scale is a 512 MiB outer safety cap, a 64 MiB inner task cap, a hard fixture allocation ceiling of 128 MiB, a maximum 32-process outer limit where available, and a 20-second external timeout. Validate that these figures fit the actual runner before execution.

| ID | Scenario | Required evidence |
|---|---|---|
| L01 | Real per-task memory breach | Task outcome plus controller-counter delta; supervisor/other bounded task survive |
| L02 | Aggregate memory breach | Workload containment; honest non-offender-specific classification |
| L03 | Real CPU quota | Approximate CPU use over a sustained interval respects bandwidth, with tolerant timing |
| L04 | Fork immediately at startup | All fixture descendants start inside the intended limit domain |
| L05 | Descendant changes session/process group | Linux workload containment still accounts for it |
| L06 | Controller missing, read-only mount, delegation denied | `required` refuses before launch; `auto` reports fallback |
| L07 | Ancestor quota/memory tighter than local values | Effective discovery respects the known ancestor boundary |
| L08 | Swap controller absent/present | Exact capability reported; no false no-swap promise |
| L09 | Exit 137 without OOM evidence | Not mislabelled a verified OOM |
| L10 | Repeated runs/cancellation | No owned process/cgroup/temporary-file leak |

Never treat cgroup-controller availability on all GitHub runners as assured. The designated safety gate must provision or select a known capable environment. A lack of capability there is a failed gate, not a passing skip. Other generic CI jobs may record an explicit skip, but that cannot substitute for L01–L10.

## 5. Native macOS acceptance

Run all applicable portable tests natively. Test `required` refusal for unsupported aggregate protection, `auto` capability text, bounded watchdog termination, and group cleanup. The watchdog fixture stays well below host danger levels. Passing a sampled-memory test does not establish a hard cap. Record limitations for detached processes and shared-memory accounting.

Do not use Linux containers as evidence that a macOS-native binary works. Cross-compilation also does not replace a native runtime test.

## 6. CI matrix and cadence

| Lane | Cadence | Minimum evidence |
|---|---|---|
| Linux x86-64 portable | Every PR | Build, lint, unit/property, integration, size |
| macOS Apple Silicon portable | Every PR | Native process/probe/cleanup and refusal tests |
| Linux real-controller safety | Required for merge once M3 lands | L01–L10 with verified external boundaries |
| Linux AArch64 / macOS Intel | Nightly and release; PR for platform changes | Native artifact smoke and platform integration |
| Compatibility containers/VMs | Release | Promised libc/OS floor and runtime dependencies |
| Fuzz/property expansion | Scheduled and before release | Config, graph, streaming matcher, event-order exploration |
| Real project comparisons | Milestone and release | Equivalent finite workload versus shell/concurrently, no unsupported claims |

Verify exact labels, availability, and permissions in the repository. GitHub documents multiple hardware profiles; record observed resources for each execution rather than assuming a fixed 16-vCPU runner. [S13](05-Technical-sources.md#s13)

## 7. Performance and binary-size measurements

For each target/profile, record uncompressed executable bytes and archive bytes separately. Inspect linked dependencies, verify checksum, then run the downloaded/extracted artifact rather than only a build-tree binary.

Use a 60-second idle-service observation for supervisor CPU and resident memory, excluding child workload usage and distinguishing shared/virtual memory. Repeat measurements on a quiet controlled machine where practical. Proposed goals are below 1,000,000 executable bytes where feasible, below 5 MiB quiescent supervisor RSS, and below 0.1% of one core idle CPU. They are not current results. A CI VM's measurement noise must not produce unjustified precision.

Benchmark fixed independent tasks at one, two, and several admitted slots; mixed CPU/memory requests; many tiny tasks; readiness overhead; and heavy output. Compare identical commands, environment, logging, and concurrency budgets. Publish median plus spread, raw run data, platform, compiler, and features. Do not extrapolate runner language alone into whole-workflow savings.

Once a baseline is approved, block unexplained size growth beyond a proposed 10% or 100 KiB, whichever is larger. That regression policy is separate from the below-1 MB aspiration. A release above target needs an explicit decision, not misleading compressed-size reporting.

## 8. Requirement traceability

| Requirement | Acceptance evidence |
|---|---|
| Finite-first adaptive execution | S01–S10, M1/M2 native lanes |
| Ordered/dependent tasks and exit codes | C07/C10/C11, S11–S15 |
| Persistent and temporary services | S08/S16, R10–R14 |
| Four readiness mechanisms | R01–R09 |
| User/env policy over automatic values | C02/C03, S03–S06 |
| Prefer workload failure over machine exhaustion | L01–L10, macOS refusal/limits disclosure |
| No dependence on Node runtime for Takt | Minimal-system smoke, P13 |
| Reliable process cleanup | P04–P09, L04/L05/L10 |
| CI-readable and structured output | P01–P03/P10–P14 |
| Small, portable artifacts | Native release smoke, size/compatibility ledger |

## 9. Release checklist

Release only after accepted decisions are closed, examples pass both structural and semantic validation, every advertised artifact has native smoke evidence, the designated safety gate passes, cleanup/output regressions are clear, binary/dependency measurements are published, installer checksums are verified, and docs accurately label platform differences.

Retain raw event logs, fixture outcomes, resource snapshots, controller capability evidence, artifact hashes, and size results as CI artifacts. Redact test secrets and never attach production environment dumps. Failures must be reproducible from bounded fixtures and recorded seeds.

This handoff has validated document/example/asset structure only. It does not report Takt application tests as executed; no application implementation exists in this package.
