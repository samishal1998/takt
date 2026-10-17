# Takt — Product Requirements Document

**Version:** 0.1 · **Date:** 16 October 2026 · **Status:** implementation-ready proposal, not an implemented product

> Run independent work together when resources allow. Queue it when they do not.

## 1. Product definition

Takt is a small, standalone Rust command runner for finite development and CI workloads: linting, type checking, builds, tests, and other checks. It schedules a dependency graph, admits work against CPU and memory budgets, explains why work is waiting, and supervises each launched process tree. Long-running services are also supported, including services needed temporarily by a finite test run.

The primary distinction is **resource-aware admission and explicit lifecycle/dependency semantics**, not merely starting multiple commands. `concurrently` already documents a maximum-process setting and configurable success/termination behavior; Takt must not advertise queuing or concurrency limits as absent from that tool. No speed or memory superiority claim is approved without comparative measurements. [S01](05-Technical-sources.md#s01)

Takt's own runtime is independent of Node. That does **not** remove Node requirements from commands it launches, and inherited `NODE_OPTIONS` still affect Node children. Preserve child environment behavior unless the user explicitly changes it. [S02](05-Technical-sources.md#s02)

### Agreed direction versus proposed defaults

| Agreed direction | Proposed implementation detail requiring validation |
|---|---|
| Name: **Takt**; brand: **02 Stack** | Exact vector geometry and documentation tokens supplied in the brand kit |
| Rust implementation | Dependency choices, I/O backend, and minimum supported Rust version |
| Linux and macOS first; Windows later | Initial release targets and OS compatibility floors |
| Finite work is the primary use case | Default admission estimates, fairness, timeouts, and failure policy |
| User limits override automatic choices | CLI → environment → file → defaults/auto precedence |
| Sequential and parallel execution with queuing | Weighted CPU/memory admission plus a process-tree slot limit |
| Long-running services and dependencies | Finite-root cleanup and live readiness at dependent launch |
| Exit-code dependency conditions | Separate edge matching from task success classification |
| TCP, HTTP, UDP, and stdout/stderr readiness | Exact match semantics and bounds; plain HTTP in V1, HTTPS pending scope approval |
| Prefer task failure over host exhaustion | Capability-gated Linux hard limits; honest best-effort native macOS behavior |
| Very small binary; aim below 1 MB | A measured, uncompressed target of **less than 1,000,000 bytes**, not a promise |
| GitHub Actions is a central validation environment | Matrix and independent safety harness in the test plan |

Everything named below as a Takt option, configuration field, event, or module is a **proposed product interface**. It is not an assertion that a released CLI or SDK already implements it.

## 2. Goals and boundaries

### Goals

1. Make one checked-in workflow usable on a constrained laptop and a larger CI machine, without launching every command immediately.
2. Combine finite tasks, prerequisite commands, and temporary/persistent services in one understandable graph.
3. Respect explicit resource policy, expose capability gaps before execution, and avoid silently presenting monitoring as hard isolation.
4. Be easy to install as one executable, with no Takt daemon and no Node or Go runtime requirement.
5. Produce readable local output and structured CI evidence while keeping Takt's own overhead small.

### Not in V1

Distributed execution, remote workers, caching, watch-mode restarts, cron scheduling, a plugin platform, a full-screen TUI, PTY multiplexing, automatic edits to package scripts, automatic tuning of every child tool's worker flags, privileged host setup, and Windows implementation. Custom-command readiness probes are deferred. Do not add a bespoke TLS implementation to meet the binary-size goal; HTTPS readiness is a scope decision, not something to fake by skipping certificate validation.

This is a supervisor for trusted project commands, **not a security sandbox for hostile code**. A process with sufficient privileges can invalidate containment assumptions. No cross-platform promise that a host can never become unresponsive is made.

## 3. Primary workflows

**Independent checks:** lint, type checking, and tests can overlap when admitted. On a smaller budget they queue. A failed lint must not hide the result of an independent type check under the default policy.

**Ordered build:** generate → build → package. Dependencies, rather than timing assumptions, determine the order.

**Service-backed test:** prepare data → start API → wait for HTTP readiness → run integration tests → stop the owned API. Takt exits after finite roots and their owned service cleanup finish.

**Persistent development:** start API and worker services and keep supervising them until cancellation. Dependencies can require service readiness, not service exit.

**Exit-code branching:** a detector returns an expected nonzero result; one branch runs and another is skipped. Declaring an edge that accepts that result does not silently redefine the detector's success policy.

**Protected Linux CI:** require enforceable aggregate memory and CPU bounds, apply explicit per-task caps, and fail before launching project commands when the required controllers or permissions are missing.

## 4. Core model

A **task** is a named command with a kind (`task` or `service`), execution specification, optional dependencies, resource requests, optional limits, environment, working directory, and lifecycle timeouts.

A **group** is a named list of tasks selected together. Group and task names must be disjoint. Selecting a task/group includes its dependency closure exactly once. An identical dependency shared by two roots runs once per invocation.

A **request** is a scheduling reservation, not an OS limit. A **limit** is a desired enforced ceiling whose actual implementation depends on the host capability report. A **job slot** is one supervised task tree; it is not one OS process or one child thread.

A **root** is explicitly selected by the user. A **dependency service** exists to support selected work. These are important for deciding when the invocation should finish.

### States and terminal outcomes

```text
pending → eligible → queued → starting → running → succeeded / failed
                                      └ service: waiting_ready → ready
                                                          └ unexpected_exit
pending / eligible / queued → skipped / blocked / cancelled
starting / running / waiting_ready / ready → stopping → stopped / cancelled / failed
```

Each terminal outcome records a reason independently from its exit code: normal exit, signal, execution timeout, readiness timeout, verified memory-limit event, spawn failure, cancellation, or internal error. `ready` is not a terminal state. Stopping an owned service as planned is not a task failure.

Expose queue reasons including `jobs_limit`, `cpu_budget`, `memory_budget`, `live_memory_pressure`, and `dependency_wait`. Record dependency blocking and configuration/capability rejection separately; do not label every non-running task “queued.”

## 5. Scheduling and automatic budgets

### 5.1 Precedence and discovery

Resolve invocation settings **CLI → `TAKT_*` environment → configuration → documented defaults/automatic detection**. Task-specific requests, limits, and environment override task defaults in the file. CLI scheduling options apply to the invocation, not silently to every task's hard limit.

Reject invalid values rather than treating them as automatic. Distinguish omitted, explicit `auto`, and explicit `off`. Do not use `0` to ambiguously mean both unlimited and disabled.

OS/container ceilings remain ceilings: a requested CPU budget of eight does not defeat a two-CPU container allowance. Show requested, discovered, and effective values plus their provenance. `CI=true` changes presentation defaults only, not resource capacity.

Use `std::thread::available_parallelism()` as an initial portable estimate, not a complete scheduler oracle. Its documentation explicitly describes limitations, including the lack of current-load information. [S03](05-Technical-sources.md#s03)

On Linux, investigate CPU affinity, enclosing cgroup constraints, and memory headroom at relevant visible ancestors. On macOS, validate a native available-memory/pressure adapter against actual supported runners; do not equate total installed RAM or just free pages with safely allocatable memory. Hidden/unreadable constraints must be reported as unknown, not silently invented.

### 5.2 Proposed automatic policy

The first implementation should use deterministic reservations, not a feedback controller that constantly changes concurrency according to CPU percentage.

- CPU capacity is a positive decimal CPU-equivalent quantity, represented internally in integer millicores. Accept up to three fractional digits. Preserve fractional container capacity.
- Automatically chosen job slots are at least one and normally the floor of effective CPU capacity. Explicit `jobs` can be lower or higher, while resource admission still applies.
- Let `H0` be observed memory headroom at startup, bounded by the smallest known applicable constraint. Proposed reserve: `max(256 MiB, 15% of H0)`. Proposed aggregate memory budget: `max(0, H0 − reserve)`. A zero budget rejects execution rather than starting dangerously.
- An unspecified task request initially reserves one CPU-equivalent and 1 GiB memory. Only **inferred** requests may be reduced to fit a smaller positive total budget, so a single task can run on a sub-CPU or low-memory host. Explain the reduction. Explicit requests are never silently reduced.
- Unknown requirements remain estimates. Do not market this as predicting arbitrary commands' peak memory. Measure fixtures, improve documentation, and allow per-task overrides.

These numbers are initial proposals for benchmarking, not researched universal safe values. A machine with unknown memory headroom defaults to one job, emits an explicit warning, and cannot satisfy required aggregate memory protection without an explicit bound.

On Linux, `MemAvailable` is an estimate useful for admitting new applications, rather than just unused RAM. [S04](05-Technical-sources.md#s04)

### 5.3 Admission rules

A task may start only when its dependencies match and:

```text
active task trees + 1 <= jobs
sum(active CPU requests) + candidate CPU request <= effective CPU budget
sum(active memory requests) + candidate memory request <= memory budget
live additional memory commitments fit within observed remaining headroom
```

Count services for their full lifetime, including readiness waiting. Resource release happens after owned workload cleanup, not merely when a shell parent exits.

Avoid double-subtracting live memory: current available-memory observations already exclude memory in use. Compare them with the candidate request plus outstanding unconsumed reservations, approximately `sum(max(0, request − observed usage))`, and the configured reserve. Unknown or shared-memory accounting must use a documented conservative fallback. Keep capacity accounting and observational pressure gates separate in code and events.

Do not freeze, kill, or dynamically alter existing jobs merely because a slot budget becomes lower. Pause new admission. Hard-limit breaches and the explicitly advertised best-effort memory watchdog follow the enforcement policy instead.

### 5.4 Ordering, fairness, and impossibility

Default ready order is stable FIFO by eligibility, with lexical task ID as a deterministic tie-breaker. YAML/JSON object key order is not a contract. `jobs: 1` serializes launches but explicit dependency edges establish required order.

Permit a later task that fits to pass a temporarily resource-blocked task. After a proposed five-second wait, reserve newly released capacity for the older eligible task to prevent indefinite starvation. Services cannot be preempted just to satisfy fairness.

Reject a task whose explicit request exceeds the total capacity. Detect service-reservation deadlocks: a service occupying the only job slot cannot release it so its dependent can start. Diagnose the conflicting tasks and budgets; do not hang. Recheck feasibility as services become permanently live. Proposed resource-queue timeout is 120 seconds, configurable; readiness and execution timeouts are separate clocks. Waiting on a running finite prerequisite is not a resource-queue timeout.

## 6. Resource protection

### 6.1 Supported modes

| Mode | Proposed behavior |
|---|---|
| `auto` | Apply supported hard controllers. Where unavailable, report exact fallback: CPU admission only; memory observation/watchdog when measurable. Never label fallback hard. |
| `required` | Every requested enforcement dimension, including resolved aggregate CPU/memory bounds, must be verified before project commands start. Otherwise exit with a capability error. |
| `off` | Retain scheduling reservations, but disable Takt-applied hard controllers and memory watchdog termination. External host limits still exist. |

User-facing capability output must separate CPU, aggregate memory, per-task memory, process-tree containment, and descendant accounting. A single `limits_supported: true` is insufficient.

### 6.2 Linux backend

V1 hard protection targets appropriately delegated cgroup v2 environments. Use controller-based CPU bandwidth and memory ceilings, including per-task groups when a task has an explicit limit. Cgroup setup must not modify unrelated workloads or assume the caller owns the hierarchy. [S05](05-Technical-sources.md#s05) [S06](05-Technical-sources.md#s06)

**Proposed ownership layout:** an owned run subtree with a supervisor leaf and a separate workload subtree; task leaves live under the workload subtree. Keep the supervisor outside the child-workload aggregate memory cap, while respecting any external enclosing cap. Apply the aggregate CPU/memory policy to the workload subtree. Apply requested task caps to task leaves.

Never derive a task's hard memory limit merely from its guessed memory request. The aggregate cap bounds the combined workload; explicit task caps isolate individual budgets. An aggregate-only OOM can kill a task other than the one a user perceives as the culprit, so do not promise offender attribution.

Use workload-group OOM behavior and counter deltas as evidence where supported; an exit code such as 137 alone is not proof of a memory-limit kill. Preserve the original signal and event evidence. CPU bandwidth throttling is not a CPU-time deadline. [S05](05-Technical-sources.md#s05)

Configure a swap ceiling only when the controller and permissions support it; proposed protected-CI policy is no additional workload swap. Report this separately. Do not automatically raise privileges, alter global OOM settings, or change unrelated cgroups. A cgroup v1 or undelegated host is a detected reduced-capability environment, not a reason to pretend V1 hard isolation works.

### 6.3 macOS and portable fallback

The V1 native macOS promise is **admission control plus explicitly best-effort memory monitoring/termination**, not Linux-equivalent aggregate memory containment. `required` rejects the run when the requested guarantee is unavailable. A user who needs hard protected execution can use a separately provisioned Linux environment; Takt does not install a VM automatically.

Do not substitute an address-space rlimit for aggregate resident memory or treat per-process limits as a process-tree controller. Apple's archived resource-limit documentation is not evidence of a current hard whole-tree guarantee; revalidate supported native behavior in the platform spike. [S07](05-Technical-sources.md#s07) [S08](05-Technical-sources.md#s08)

Proposed watchdog policy: sample bounded task-tree usage at 100–250 ms intervals; when an explicit task memory cap is observed breached, terminate that task tree. On an aggregate breach without reliable attribution, terminate the most recently started active workload and report `aggregate_memory_watchdog`, not “offending process identified.” This can act too late and may miss short-lived or detached descendants. Clearly surface this limitation. Do not implement fake CPU-percentage enforcement by repeatedly stopping and continuing processes in V1.

## 7. Dependency semantics and failure handling

### Finite dependency

```yaml
depends_on:
  - task: generate
    when: exit
    codes: [0]
```

The edge matches only after normal finite-command exit with a listed code. Signal death, timeout, spawn failure, and OOM do not match exit-code predicates. All dependencies are ANDed. Duplicate edges, missing IDs, self-dependencies, cycles, `ready` edges to finite tasks, and `exit` edges to services are validation errors.

### Expected nonzero results

`success_codes` defaults to `[0]`. A detector with expected result 2 must declare `success_codes: [0, 2]`; its dependent can declare `codes: [2]`. This keeps exit matching separate from run success. A matching edge alone does not erase an upstream failure. Under `continue`, such an edge may run after an actual nonzero failure, but the invocation still fails. Under `fail-fast`, failure policy wins and pending work is cancelled.

A condition that fails to match an otherwise successful prerequisite produces `skipped: condition_not_matched`, a neutral conditional branch. A failed prerequisite with no matching continuation produces `blocked: dependency_failed`. Propagate neutral branch skips without turning them into failures. Report an entirely skipped selection explicitly rather than claiming that checks ran.

### Service readiness dependency

```yaml
depends_on:
  - task: api
    when: ready
```

The provider must have passed its probe and still be alive at the dependent's launch decision. Launch cannot guarantee it stays alive afterward. A provider failing before readiness blocks dependents. A provider exiting unexpectedly after readiness marks the run failed, blocks not-yet-started readiness dependents, but **does not kill already running dependents by default**. Proposed opt-in `service_failure: stop-dependents` cancels the transitive dependent set. Automatic service restart and readiness generations are deferred.

### Invocation failure policy

`failure: continue` is the proposed default for checks: run independent eligible work, collect failures, then exit nonzero. `failure: fail-fast` cancels pending work and terminates active owned trees on the first unaccepted failure. Planned service teardown is not a failure trigger.

Proposed runner codes: `0` successful selected work/neutral skips; `1` workload failure; `2` invalid CLI/config/graph; `3` required capability or unsatisfiable resource policy; `70` internal runner error; `130` user SIGINT; `143` SIGTERM. Record raw child outcomes in the report rather than returning an arbitrary last child's code.

## 8. Readiness contracts

Readiness is opt-in for services and mandatory when another task depends on `when: ready`. A service with no readiness dependencies may omit a probe and is shown as `running`, not automatically `ready`. Capture starts before execution so a fast startup message is not lost.

| Probe | V1 matching rule | Important boundary |
|---|---|---|
| TCP | Successful bounded connection to configured host/port | Proves a listener accepted, not application health or ownership |
| HTTP | Bounded GET, accepted status list, optional bounded literal body match | No redirects by default; plain HTTP initially; HTTPS needs an approved TLS path |
| UDP | Send explicit payload and validate a reply from the configured peer, with exact bytes or a defined prefix | Sending or UDP `connect()` alone is not readiness |
| stdio | Literal byte substring on stdout, stderr, or either | Match across chunk boundaries; bound buffers; no regex engine required in V1 |

UDP is connectionless; use a request/response contract rather than inferring availability from a local socket operation. [S09](05-Technical-sources.md#s09)

Proposed defaults: overall readiness deadline 30 seconds; retry interval 250 ms; network attempt timeout 1 second. All deadlines use a monotonic clock. Bound DNS resolution as part of the deadline; do not let blocking hostname lookup defeat it. Probe responses are size-limited (proposed 64 KiB) and the HTTP body match is literal, not an unbounded parser or regular expression.

Fail validation for ambiguous/missing probe settings, invalid addresses, unsupported URL schemes, empty stdio patterns, or UDP without a payload/reply predicate. Readiness failures report the last relevant error and bounded diagnostics, without dumping secrets.

## 9. Process lifecycle, environment, and output

Prefer direct argument arrays. Shell execution must be explicit and uses `/bin/sh -c` on initial platforms. Never split a command string using whitespace and pretend shell quoting was preserved. An explicit Bash command is a user's task dependency, not a Takt runtime dependency.

Use a separate process group for each task. Rust exposes stable Unix process-group configuration; low-level pre-exec hooks have strict safety constraints and need a reviewed launch implementation. [S10](05-Technical-sources.md#s10)

On cancellation: stop admission, signal owned groups with SIGINT for user interruption or SIGTERM for normal teardown, wait a proposed five-second grace, then SIGKILL remaining owned workload. A second interrupt accelerates termination. Reap direct children and drain bounded remaining output. Service cleanup follows reverse dependency order where possible. Normal finite completion must clean up unexpected surviving descendants before releasing reservations.

Linux cgroup containment and macOS process groups have different strengths. Detached/session-changing descendants, PID reuse, parent death, and escaped descendants need explicit test cases and capability reporting. Never signal broad process-name matches. Takt itself being killed with SIGKILL cannot guarantee portable cleanup; document external runner/cgroup cleanup responsibilities.

Environment defaults to inherited variables, overlaid with per-task `env`, then per-task `unset_env`. Validate conflicting entries. Do not strip or rewrite `NODE_OPTIONS`, `GOMAXPROCS`, Rust worker counts, or tool-specific flags automatically. Optional informational `TAKT_TASK_CPU` and `TAKT_TASK_MEMORY_BYTES` describe allocations; they do not constrain child threads. Nested runners share real OS ceilings but cannot coordinate independent scheduling reservations without a future explicit protocol.

### Output modes

`auto` selects readable terminal output when attached to a terminal and plain prefixed logs in CI. Other proposed choices are `plain`, `grouped`, and `json`. `NO_COLOR` disables color. No animation is essential to understand task state.

Preserve stdout/stderr identity per task. Ordering within a stream is preserved; combined inter-process ordering is not claimed. Every task's output must be drained concurrently. Large/non-UTF-8/newline-free output must not deadlock supervision or readiness matching.

Use bounded memory buffers. Grouped output spools to bounded temporary files; a proposed 64 MiB/task spill cap yields an explicit truncation marker while continuing to drain the pipe. Clean temporary files on ordinary termination. Separate log truncation from readiness inspection. A slow output consumer must not block child reaping, cancellation, or limit handling.

In JSON mode, stdout is NDJSON only; human diagnostics go to stderr. Emit versioned events and a final summary. Proposed common fields: `schema_version`, `run_id`, monotonically increasing `sequence`, `event`, `task_id` when applicable, monotonic elapsed time, reason, and relevant requested/effective budget. Omit environment values by default. JSON log events use a byte-safe representation when input is not valid UTF-8. Do not imply automatic redaction can recognize every secret printed by a child.

## 10. Proposed CLI and configuration

```sh
# Finite group, with automatic budgets.
takt run checks

# One task tree at a time; graph dependencies still govern order.
takt run checks --jobs 1

# Explicit resources; require verified protection before project execution.
takt run checks --jobs 2 --cpu 2 --memory 4GiB --enforcement required

# Service group.
takt run dev

# Inspect without launching commands or creating cgroups.
takt validate --file takt.yaml
takt plan checks --file takt.yaml
takt doctor --output json

# Config-free finite shell commands; each --command is explicitly a shell string.
takt exec --command 'npm run lint' --command 'npm test' --jobs 2
```

Takt configuration is **YAML or JSON**, never TOML. Rust's native Cargo build manifests are toolchain files, not the application configuration format.

Discovery: use an explicit `--file`, otherwise inspect the working directory for `takt.yaml`, `takt.yml`, or `takt.json`; error when more than one candidate exists. No parent-directory/global-file search in V1. Resolve relative task working directories against the configuration directory. CLI-only commands use the invocation directory. No implicit `.env` loading, environment interpolation, YAML tags, or include system in V1.

Expose `TAKT_JOBS`, `TAKT_CPU`, `TAKT_MEMORY`, `TAKT_ENFORCEMENT`, `TAKT_OUTPUT`, and `TAKT_FILE` for corresponding invocation settings. Require a top-level `version: 1`; reject unknown fields and duplicate keys. Put parse-size, nesting, and alias-expansion bounds in the parser contract.

See the [example configurations](../examples/README.md) and [draft JSON Schema](../schema/takt.schema.json). Schema validation does not replace graph, platform, unit, or resource-feasibility validation.

## 11. Runtime architecture and delivery requirements

Keep the domain model and deterministic scheduler independent of process and OS APIs. Suggested boundaries: config, graph, scheduler, platform discovery, enforcement, process supervisor, probes, events, and renderer. Use library-plus-CLI structure without premature public SDK stability promises.

One downloaded executable per OS/architecture. Avoid runtime helper commands for probes. Linux system-linked and more self-contained builds can be separate artifacts after compatibility testing. macOS uses system facilities; do not describe it as a fully static executable. Shipping a smaller variant must not silently remove agreed V1 behavior.

Cargo documents stripping, optimization, LTO, and panic-profile controls, but none proves a particular Takt binary size. Compare equivalent feature sets and publish actual file sizes. [S11](05-Technical-sources.md#s11) Compatibility targets must be selected using the Rust target documentation and actual CI runs. [S12](05-Technical-sources.md#s12)

Proposed performance objectives, not measured claims: optimized CLI below 1,000,000 bytes where achievable; supervisor-only quiescent resident memory below 5 MiB; no busy polling; idle CPU under 0.1% of one core in a controlled 60-second observation. Report baseline noise and distinguish resident memory from virtual address space. Fail correctness before sacrificing safety to meet these targets.

## 12. Release acceptance

A V1 candidate must demonstrate finite parallel/serial execution, constrained admission, graph validation, exit-code branching, all four agreed readiness probes, service cleanup, deterministic failure aggregation, bounded logs, cancellation, and the required-mode preflight contract on Linux and macOS.

GitHub Actions is first-class validation, but real runner capacity varies. Do not assume “CI means 16 vCPUs,” and do not confuse a synthetic reported limit with an OS-enforced test. [S13](05-Technical-sources.md#s13)

Every advertised artifact needs native execution evidence, compatibility checks, exact size reporting, and checksum verification. Hard-memory claims require controlled real-controller tests; macOS must pass refusal/fallback tests without pretending to provide the same containment.

Implementation, CI, and release gates are defined in [02 — Implementation plan](02-Implementation-plan.md) and [03 — Test and CI plan](03-Test-and-CI-plan.md).

## 13. Decisions that must be closed during the first milestone

Choose repository and license; confirm initial OS/architecture floors; confirm the proposed defaults and conditional skip semantics; decide whether HTTPS is required in V1; approve the exact small-versus-standard artifact policy; select measured dependencies; and determine where delegated-controller integration tests will run.

The implementation agent must verify current library APIs and runner capabilities before coding against them. When a documented assumption fails, record the evidence, describe its effect, propose a small alternative, and ask the owner before changing accepted scope or guarantees. Do not replace a failing requirement with a silent fallback.
