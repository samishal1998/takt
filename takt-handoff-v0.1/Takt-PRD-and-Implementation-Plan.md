# Takt — PRD and Implementation Handoff

**Version 0.1 · 16 October 2026**

This standalone Markdown file combines the product requirements, implementation plan, test/CI plan, decision record, configuration examples, and verified technical sources. The companion complete ZIP also includes the draft schema and the full **02 Stack** documentation brand kit.

The application is not implemented by this document. All Takt CLI/configuration interfaces are proposed. Accepted product direction is separated from defaults requiring validation. No repository changes or application tests are claimed.

## Contents

[Product requirements](#product-requirements) · [Implementation plan](#implementation-plan) · [Test and CI plan](#test-and-ci-plan) · [Decision record](#decision-record) · [Configuration examples](#configuration-examples) · [Technical sources](#technical-sources)



---

<a id="product-requirements"></a>

## Takt — Product Requirements Document

**Version:** 0.1 · **Date:** 16 October 2026 · **Status:** implementation-ready proposal, not an implemented product

> Run independent work together when resources allow. Queue it when they do not.

### 1. Product definition

Takt is a small, standalone Rust command runner for finite development and CI workloads: linting, type checking, builds, tests, and other checks. It schedules a dependency graph, admits work against CPU and memory budgets, explains why work is waiting, and supervises each launched process tree. Long-running services are also supported, including services needed temporarily by a finite test run.

The primary distinction is **resource-aware admission and explicit lifecycle/dependency semantics**, not merely starting multiple commands. `concurrently` already documents a maximum-process setting and configurable success/termination behavior; Takt must not advertise queuing or concurrency limits as absent from that tool. No speed or memory superiority claim is approved without comparative measurements. [S01](#s01)

Takt's own runtime is independent of Node. That does **not** remove Node requirements from commands it launches, and inherited `NODE_OPTIONS` still affect Node children. Preserve child environment behavior unless the user explicitly changes it. [S02](#s02)

#### Agreed direction versus proposed defaults

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

### 2. Goals and boundaries

#### Goals

1. Make one checked-in workflow usable on a constrained laptop and a larger CI machine, without launching every command immediately.
2. Combine finite tasks, prerequisite commands, and temporary/persistent services in one understandable graph.
3. Respect explicit resource policy, expose capability gaps before execution, and avoid silently presenting monitoring as hard isolation.
4. Be easy to install as one executable, with no Takt daemon and no Node or Go runtime requirement.
5. Produce readable local output and structured CI evidence while keeping Takt's own overhead small.

#### Not in V1

Distributed execution, remote workers, caching, watch-mode restarts, cron scheduling, a plugin platform, a full-screen TUI, PTY multiplexing, automatic edits to package scripts, automatic tuning of every child tool's worker flags, privileged host setup, and Windows implementation. Custom-command readiness probes are deferred. Do not add a bespoke TLS implementation to meet the binary-size goal; HTTPS readiness is a scope decision, not something to fake by skipping certificate validation.

This is a supervisor for trusted project commands, **not a security sandbox for hostile code**. A process with sufficient privileges can invalidate containment assumptions. No cross-platform promise that a host can never become unresponsive is made.

### 3. Primary workflows

**Independent checks:** lint, type checking, and tests can overlap when admitted. On a smaller budget they queue. A failed lint must not hide the result of an independent type check under the default policy.

**Ordered build:** generate → build → package. Dependencies, rather than timing assumptions, determine the order.

**Service-backed test:** prepare data → start API → wait for HTTP readiness → run integration tests → stop the owned API. Takt exits after finite roots and their owned service cleanup finish.

**Persistent development:** start API and worker services and keep supervising them until cancellation. Dependencies can require service readiness, not service exit.

**Exit-code branching:** a detector returns an expected nonzero result; one branch runs and another is skipped. Declaring an edge that accepts that result does not silently redefine the detector's success policy.

**Protected Linux CI:** require enforceable aggregate memory and CPU bounds, apply explicit per-task caps, and fail before launching project commands when the required controllers or permissions are missing.

### 4. Core model

A **task** is a named command with a kind (`task` or `service`), execution specification, optional dependencies, resource requests, optional limits, environment, working directory, and lifecycle timeouts.

A **group** is a named list of tasks selected together. Group and task names must be disjoint. Selecting a task/group includes its dependency closure exactly once. An identical dependency shared by two roots runs once per invocation.

A **request** is a scheduling reservation, not an OS limit. A **limit** is a desired enforced ceiling whose actual implementation depends on the host capability report. A **job slot** is one supervised task tree; it is not one OS process or one child thread.

A **root** is explicitly selected by the user. A **dependency service** exists to support selected work. These are important for deciding when the invocation should finish.

#### States and terminal outcomes

```text
pending → eligible → queued → starting → running → succeeded / failed
                                      └ service: waiting_ready → ready
                                                          └ unexpected_exit
pending / eligible / queued → skipped / blocked / cancelled
starting / running / waiting_ready / ready → stopping → stopped / cancelled / failed
```

Each terminal outcome records a reason independently from its exit code: normal exit, signal, execution timeout, readiness timeout, verified memory-limit event, spawn failure, cancellation, or internal error. `ready` is not a terminal state. Stopping an owned service as planned is not a task failure.

Expose queue reasons including `jobs_limit`, `cpu_budget`, `memory_budget`, `live_memory_pressure`, and `dependency_wait`. Record dependency blocking and configuration/capability rejection separately; do not label every non-running task “queued.”

### 5. Scheduling and automatic budgets

#### 5.1 Precedence and discovery

Resolve invocation settings **CLI → `TAKT_*` environment → configuration → documented defaults/automatic detection**. Task-specific requests, limits, and environment override task defaults in the file. CLI scheduling options apply to the invocation, not silently to every task's hard limit.

Reject invalid values rather than treating them as automatic. Distinguish omitted, explicit `auto`, and explicit `off`. Do not use `0` to ambiguously mean both unlimited and disabled.

OS/container ceilings remain ceilings: a requested CPU budget of eight does not defeat a two-CPU container allowance. Show requested, discovered, and effective values plus their provenance. `CI=true` changes presentation defaults only, not resource capacity.

Use `std::thread::available_parallelism()` as an initial portable estimate, not a complete scheduler oracle. Its documentation explicitly describes limitations, including the lack of current-load information. [S03](#s03)

On Linux, investigate CPU affinity, enclosing cgroup constraints, and memory headroom at relevant visible ancestors. On macOS, validate a native available-memory/pressure adapter against actual supported runners; do not equate total installed RAM or just free pages with safely allocatable memory. Hidden/unreadable constraints must be reported as unknown, not silently invented.

#### 5.2 Proposed automatic policy

The first implementation should use deterministic reservations, not a feedback controller that constantly changes concurrency according to CPU percentage.

- CPU capacity is a positive decimal CPU-equivalent quantity, represented internally in integer millicores. Accept up to three fractional digits. Preserve fractional container capacity.
- Automatically chosen job slots are at least one and normally the floor of effective CPU capacity. Explicit `jobs` can be lower or higher, while resource admission still applies.
- Let `H0` be observed memory headroom at startup, bounded by the smallest known applicable constraint. Proposed reserve: `max(256 MiB, 15% of H0)`. Proposed aggregate memory budget: `max(0, H0 − reserve)`. A zero budget rejects execution rather than starting dangerously.
- An unspecified task request initially reserves one CPU-equivalent and 1 GiB memory. Only **inferred** requests may be reduced to fit a smaller positive total budget, so a single task can run on a sub-CPU or low-memory host. Explain the reduction. Explicit requests are never silently reduced.
- Unknown requirements remain estimates. Do not market this as predicting arbitrary commands' peak memory. Measure fixtures, improve documentation, and allow per-task overrides.

These numbers are initial proposals for benchmarking, not researched universal safe values. A machine with unknown memory headroom defaults to one job, emits an explicit warning, and cannot satisfy required aggregate memory protection without an explicit bound.

On Linux, `MemAvailable` is an estimate useful for admitting new applications, rather than just unused RAM. [S04](#s04)

#### 5.3 Admission rules

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

#### 5.4 Ordering, fairness, and impossibility

Default ready order is stable FIFO by eligibility, with lexical task ID as a deterministic tie-breaker. YAML/JSON object key order is not a contract. `jobs: 1` serializes launches but explicit dependency edges establish required order.

Permit a later task that fits to pass a temporarily resource-blocked task. After a proposed five-second wait, reserve newly released capacity for the older eligible task to prevent indefinite starvation. Services cannot be preempted just to satisfy fairness.

Reject a task whose explicit request exceeds the total capacity. Detect service-reservation deadlocks: a service occupying the only job slot cannot release it so its dependent can start. Diagnose the conflicting tasks and budgets; do not hang. Recheck feasibility as services become permanently live. Proposed resource-queue timeout is 120 seconds, configurable; readiness and execution timeouts are separate clocks. Waiting on a running finite prerequisite is not a resource-queue timeout.

### 6. Resource protection

#### 6.1 Supported modes

| Mode | Proposed behavior |
|---|---|
| `auto` | Apply supported hard controllers. Where unavailable, report exact fallback: CPU admission only; memory observation/watchdog when measurable. Never label fallback hard. |
| `required` | Every requested enforcement dimension, including resolved aggregate CPU/memory bounds, must be verified before project commands start. Otherwise exit with a capability error. |
| `off` | Retain scheduling reservations, but disable Takt-applied hard controllers and memory watchdog termination. External host limits still exist. |

User-facing capability output must separate CPU, aggregate memory, per-task memory, process-tree containment, and descendant accounting. A single `limits_supported: true` is insufficient.

#### 6.2 Linux backend

V1 hard protection targets appropriately delegated cgroup v2 environments. Use controller-based CPU bandwidth and memory ceilings, including per-task groups when a task has an explicit limit. Cgroup setup must not modify unrelated workloads or assume the caller owns the hierarchy. [S05](#s05) [S06](#s06)

**Proposed ownership layout:** an owned run subtree with a supervisor leaf and a separate workload subtree; task leaves live under the workload subtree. Keep the supervisor outside the child-workload aggregate memory cap, while respecting any external enclosing cap. Apply the aggregate CPU/memory policy to the workload subtree. Apply requested task caps to task leaves.

Never derive a task's hard memory limit merely from its guessed memory request. The aggregate cap bounds the combined workload; explicit task caps isolate individual budgets. An aggregate-only OOM can kill a task other than the one a user perceives as the culprit, so do not promise offender attribution.

Use workload-group OOM behavior and counter deltas as evidence where supported; an exit code such as 137 alone is not proof of a memory-limit kill. Preserve the original signal and event evidence. CPU bandwidth throttling is not a CPU-time deadline. [S05](#s05)

Configure a swap ceiling only when the controller and permissions support it; proposed protected-CI policy is no additional workload swap. Report this separately. Do not automatically raise privileges, alter global OOM settings, or change unrelated cgroups. A cgroup v1 or undelegated host is a detected reduced-capability environment, not a reason to pretend V1 hard isolation works.

#### 6.3 macOS and portable fallback

The V1 native macOS promise is **admission control plus explicitly best-effort memory monitoring/termination**, not Linux-equivalent aggregate memory containment. `required` rejects the run when the requested guarantee is unavailable. A user who needs hard protected execution can use a separately provisioned Linux environment; Takt does not install a VM automatically.

Do not substitute an address-space rlimit for aggregate resident memory or treat per-process limits as a process-tree controller. Apple's archived resource-limit documentation is not evidence of a current hard whole-tree guarantee; revalidate supported native behavior in the platform spike. [S07](#s07) [S08](#s08)

Proposed watchdog policy: sample bounded task-tree usage at 100–250 ms intervals; when an explicit task memory cap is observed breached, terminate that task tree. On an aggregate breach without reliable attribution, terminate the most recently started active workload and report `aggregate_memory_watchdog`, not “offending process identified.” This can act too late and may miss short-lived or detached descendants. Clearly surface this limitation. Do not implement fake CPU-percentage enforcement by repeatedly stopping and continuing processes in V1.

### 7. Dependency semantics and failure handling

#### Finite dependency

```yaml
depends_on:
  - task: generate
    when: exit
    codes: [0]
```

The edge matches only after normal finite-command exit with a listed code. Signal death, timeout, spawn failure, and OOM do not match exit-code predicates. All dependencies are ANDed. Duplicate edges, missing IDs, self-dependencies, cycles, `ready` edges to finite tasks, and `exit` edges to services are validation errors.

#### Expected nonzero results

`success_codes` defaults to `[0]`. A detector with expected result 2 must declare `success_codes: [0, 2]`; its dependent can declare `codes: [2]`. This keeps exit matching separate from run success. A matching edge alone does not erase an upstream failure. Under `continue`, such an edge may run after an actual nonzero failure, but the invocation still fails. Under `fail-fast`, failure policy wins and pending work is cancelled.

A condition that fails to match an otherwise successful prerequisite produces `skipped: condition_not_matched`, a neutral conditional branch. A failed prerequisite with no matching continuation produces `blocked: dependency_failed`. Propagate neutral branch skips without turning them into failures. Report an entirely skipped selection explicitly rather than claiming that checks ran.

#### Service readiness dependency

```yaml
depends_on:
  - task: api
    when: ready
```

The provider must have passed its probe and still be alive at the dependent's launch decision. Launch cannot guarantee it stays alive afterward. A provider failing before readiness blocks dependents. A provider exiting unexpectedly after readiness marks the run failed, blocks not-yet-started readiness dependents, but **does not kill already running dependents by default**. Proposed opt-in `service_failure: stop-dependents` cancels the transitive dependent set. Automatic service restart and readiness generations are deferred.

#### Invocation failure policy

`failure: continue` is the proposed default for checks: run independent eligible work, collect failures, then exit nonzero. `failure: fail-fast` cancels pending work and terminates active owned trees on the first unaccepted failure. Planned service teardown is not a failure trigger.

Proposed runner codes: `0` successful selected work/neutral skips; `1` workload failure; `2` invalid CLI/config/graph; `3` required capability or unsatisfiable resource policy; `70` internal runner error; `130` user SIGINT; `143` SIGTERM. Record raw child outcomes in the report rather than returning an arbitrary last child's code.

### 8. Readiness contracts

Readiness is opt-in for services and mandatory when another task depends on `when: ready`. A service with no readiness dependencies may omit a probe and is shown as `running`, not automatically `ready`. Capture starts before execution so a fast startup message is not lost.

| Probe | V1 matching rule | Important boundary |
|---|---|---|
| TCP | Successful bounded connection to configured host/port | Proves a listener accepted, not application health or ownership |
| HTTP | Bounded GET, accepted status list, optional bounded literal body match | No redirects by default; plain HTTP initially; HTTPS needs an approved TLS path |
| UDP | Send explicit payload and validate a reply from the configured peer, with exact bytes or a defined prefix | Sending or UDP `connect()` alone is not readiness |
| stdio | Literal byte substring on stdout, stderr, or either | Match across chunk boundaries; bound buffers; no regex engine required in V1 |

UDP is connectionless; use a request/response contract rather than inferring availability from a local socket operation. [S09](#s09)

Proposed defaults: overall readiness deadline 30 seconds; retry interval 250 ms; network attempt timeout 1 second. All deadlines use a monotonic clock. Bound DNS resolution as part of the deadline; do not let blocking hostname lookup defeat it. Probe responses are size-limited (proposed 64 KiB) and the HTTP body match is literal, not an unbounded parser or regular expression.

Fail validation for ambiguous/missing probe settings, invalid addresses, unsupported URL schemes, empty stdio patterns, or UDP without a payload/reply predicate. Readiness failures report the last relevant error and bounded diagnostics, without dumping secrets.

### 9. Process lifecycle, environment, and output

Prefer direct argument arrays. Shell execution must be explicit and uses `/bin/sh -c` on initial platforms. Never split a command string using whitespace and pretend shell quoting was preserved. An explicit Bash command is a user's task dependency, not a Takt runtime dependency.

Use a separate process group for each task. Rust exposes stable Unix process-group configuration; low-level pre-exec hooks have strict safety constraints and need a reviewed launch implementation. [S10](#s10)

On cancellation: stop admission, signal owned groups with SIGINT for user interruption or SIGTERM for normal teardown, wait a proposed five-second grace, then SIGKILL remaining owned workload. A second interrupt accelerates termination. Reap direct children and drain bounded remaining output. Service cleanup follows reverse dependency order where possible. Normal finite completion must clean up unexpected surviving descendants before releasing reservations.

Linux cgroup containment and macOS process groups have different strengths. Detached/session-changing descendants, PID reuse, parent death, and escaped descendants need explicit test cases and capability reporting. Never signal broad process-name matches. Takt itself being killed with SIGKILL cannot guarantee portable cleanup; document external runner/cgroup cleanup responsibilities.

Environment defaults to inherited variables, overlaid with per-task `env`, then per-task `unset_env`. Validate conflicting entries. Do not strip or rewrite `NODE_OPTIONS`, `GOMAXPROCS`, Rust worker counts, or tool-specific flags automatically. Optional informational `TAKT_TASK_CPU` and `TAKT_TASK_MEMORY_BYTES` describe allocations; they do not constrain child threads. Nested runners share real OS ceilings but cannot coordinate independent scheduling reservations without a future explicit protocol.

#### Output modes

`auto` selects readable terminal output when attached to a terminal and plain prefixed logs in CI. Other proposed choices are `plain`, `grouped`, and `json`. `NO_COLOR` disables color. No animation is essential to understand task state.

Preserve stdout/stderr identity per task. Ordering within a stream is preserved; combined inter-process ordering is not claimed. Every task's output must be drained concurrently. Large/non-UTF-8/newline-free output must not deadlock supervision or readiness matching.

Use bounded memory buffers. Grouped output spools to bounded temporary files; a proposed 64 MiB/task spill cap yields an explicit truncation marker while continuing to drain the pipe. Clean temporary files on ordinary termination. Separate log truncation from readiness inspection. A slow output consumer must not block child reaping, cancellation, or limit handling.

In JSON mode, stdout is NDJSON only; human diagnostics go to stderr. Emit versioned events and a final summary. Proposed common fields: `schema_version`, `run_id`, monotonically increasing `sequence`, `event`, `task_id` when applicable, monotonic elapsed time, reason, and relevant requested/effective budget. Omit environment values by default. JSON log events use a byte-safe representation when input is not valid UTF-8. Do not imply automatic redaction can recognize every secret printed by a child.

### 10. Proposed CLI and configuration

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

See the [example configurations](#configuration-examples) and [draft JSON Schema](#schema-in-bundle). Schema validation does not replace graph, platform, unit, or resource-feasibility validation.

### 11. Runtime architecture and delivery requirements

Keep the domain model and deterministic scheduler independent of process and OS APIs. Suggested boundaries: config, graph, scheduler, platform discovery, enforcement, process supervisor, probes, events, and renderer. Use library-plus-CLI structure without premature public SDK stability promises.

One downloaded executable per OS/architecture. Avoid runtime helper commands for probes. Linux system-linked and more self-contained builds can be separate artifacts after compatibility testing. macOS uses system facilities; do not describe it as a fully static executable. Shipping a smaller variant must not silently remove agreed V1 behavior.

Cargo documents stripping, optimization, LTO, and panic-profile controls, but none proves a particular Takt binary size. Compare equivalent feature sets and publish actual file sizes. [S11](#s11) Compatibility targets must be selected using the Rust target documentation and actual CI runs. [S12](#s12)

Proposed performance objectives, not measured claims: optimized CLI below 1,000,000 bytes where achievable; supervisor-only quiescent resident memory below 5 MiB; no busy polling; idle CPU under 0.1% of one core in a controlled 60-second observation. Report baseline noise and distinguish resident memory from virtual address space. Fail correctness before sacrificing safety to meet these targets.

### 12. Release acceptance

A V1 candidate must demonstrate finite parallel/serial execution, constrained admission, graph validation, exit-code branching, all four agreed readiness probes, service cleanup, deterministic failure aggregation, bounded logs, cancellation, and the required-mode preflight contract on Linux and macOS.

GitHub Actions is first-class validation, but real runner capacity varies. Do not assume “CI means 16 vCPUs,” and do not confuse a synthetic reported limit with an OS-enforced test. [S13](#s13)

Every advertised artifact needs native execution evidence, compatibility checks, exact size reporting, and checksum verification. Hard-memory claims require controlled real-controller tests; macOS must pass refusal/fallback tests without pretending to provide the same containment.

Implementation, CI, and release gates are defined in [02 — Implementation plan](#implementation-plan) and [03 — Test and CI plan](#test-and-ci-plan).

### 13. Decisions that must be closed during the first milestone

Choose repository and license; confirm initial OS/architecture floors; confirm the proposed defaults and conditional skip semantics; decide whether HTTPS is required in V1; approve the exact small-versus-standard artifact policy; select measured dependencies; and determine where delegated-controller integration tests will run.

The implementation agent must verify current library APIs and runner capabilities before coding against them. When a documented assumption fails, record the evidence, describe its effect, propose a small alternative, and ask the owner before changing accepted scope or guarantees. Do not replace a failing requirement with a silent fallback.

---

<a id="implementation-plan"></a>

## Takt — Implementation and Agent Handoff

**Version:** 0.1 · **Date:** 16 October 2026 · **Status:** proposed execution plan

Read the [PRD](#product-requirements) first. Its agreed requirements outrank convenience choices in this plan. No Takt repository has been identified in the supplied context, and no repository inspection, changes, builds, or CI execution are represented as completed by this handoff.

### 1. Engineering approach

Build the smallest reliable vertical slice, measure it, and then extend it. Keep Linux and macOS in the first runnable milestone. Finite task scheduling is the product's center; service orchestration must not consume the first several milestones before useful checks can run.

Use a Rust library plus a thin executable. An internal workspace may use these boundaries; separate crates are optional until dependency/compile-time boundaries justify them:

```text
takt/
  crates/
    takt-core/         # models, config lowering, graph, scheduling, policies
    takt-platform/     # discovery, launch, process accounting, controllers
    takt-cli/          # args, output, signal wiring, composition
  tests/
    fixtures/          # purpose-built bounded child-process executable
    integration/       # real process/lifecycle tests
    safety/            # Linux delegated-controller tests
  docs/
  examples/
  brand/
```

Keep proposed interfaces internal initially: `PlatformCapabilities`, `ResourceSnapshot`, `TaskRequest`, `TaskLimit`, `SchedulerInput`, `SchedulerDecision`, `TaskOutcome`, `ProbeOutcome`, and `RunEvent`. These names describe architecture; they are not verified third-party APIs. Inject a monotonic clock and resource observations into the scheduler so most tests do not sleep or execute commands.

### 2. Dependency and size spike

Start with a size ledger for a minimal executable and add capabilities one at a time: argument parsing, YAML/JSON, process handling, event serialization, logging, probes, and OS adapters. Record stripped executable bytes, linked libraries, feature flags, target, compiler, resident memory, and startup measurements.

| Area | Candidate or direction | Required decision evidence |
|---|---|---|
| Serialization | Serde plus JSON format support | Strict field handling, error size, executable impact |
| YAML | Evaluate maintained `yaml_serde` and `serde-saphyr` candidates | Current maintenance/security review, duplicate keys, bounded aliases/depth, actionable errors |
| CLI parsing | Small parser versus a full CLI parser with limited features | Help quality, quoting correctness, error handling, incremental size |
| I/O | Bounded worker design versus `mio`/`polling`-based event loop | Pipes, timers, signals, cancellation, macOS/Linux behavior, idle CPU |
| OS calls | Narrow reviewed native wrapper | Safety review and portability; do not scatter unchecked FFI |
| HTTP | Small maintained bounded client/parser suitable for readiness | No blocking supervision, deadline-bound DNS, size, status/body limits |
| TLS | Not a hand-written TLS implementation | Owner decides V1 HTTPS scope; verify secure backend and artifact impact |

The YAML and I/O candidates have current maintainer documentation, but this handoff does not pin package versions or claim they meet Takt's final size/safety requirements. Verify the exact version and interface at implementation time. [S14](#s14) [S15](#s15) [S16](#s16) [S17](#s17)

Avoid choosing a large async stack reflexively, but also avoid hand-writing a fragile runtime merely to save bytes. Blocking hostname resolution must be bounded and isolated. Reject architectures that can wedge process reaping when output blocks. Keep a bounded number of OS threads; unbounded threads-per-probe is not acceptable.

Try release stripping, `opt-level` size settings, LTO, and codegen-unit choices as measured variants. A panic-abort profile is not automatically accepted: it can bypass cleanup after a defect. Minimize panic paths, test parent-failure behavior, and document the external cleanup boundary. Cargo's controls are documented, but the winning configuration must come from the full-feature benchmark. [S11](#s11)

Do not include embedded logos, font assets, sample projects, or a browser UI in the CLI binary. The brand kit belongs in docs and release presentation.

### 3. Milestones and exit gates

#### M0 — Specification, compatibility, and risk reduction

**Deliver:** accepted decision log; repository inventory; supported targets/floors; dependency/size ledger; native discovery prototype; process-launch proof; Linux delegation proof; exact current API/source notes.

Prove a child cannot execute project code before its requested limits are attached. A practical candidate is a same-binary private launch mode: spawn a controlled helper, hold it behind a handshake, attach and verify the owned workload group, then allow exec. The helper must close inherited descriptors, validate its protocol, and never accept untrusted control messages. Compare this with audited low-level launch alternatives; do not improvise unsafe work in a multithreaded post-fork closure. [S10](#s10)

A successful spike must show that a child that forks immediately remains contained on the tested Linux path. Native macOS must show stable process-group cancellation and truthful capability rejection. Closing M0 requires owner decisions on HTTPS and protection/size trade-offs, not guessed approval.

#### M1 — Runnable finite-task slice

**Deliver:** direct argv execution; explicit shell strings; bounded stdout/stderr multiplexing; exit capture; working directory and environment; CLI serial/parallel controls; signal cleanup; basic Linux/macOS CI.

Use a bounded purpose-built test fixture rather than relying on `nc`, Python, Node, curl, or shell timing tricks for core correctness. Run two sleeping tasks in parallel and serial modes; verify the sequence and exit result. Kill a tree on cancellation. Reject malformed arguments and launch failures cleanly.

**Gate:** independent integration harness passes natively on Linux and macOS; no deadlock under large dual-stream output; first executable-size report published. At this point the program is useful without YAML, dependencies, or service mode.

#### M2 — Configuration, graph, and deterministic scheduling

**Deliver:** YAML/JSON parsing with source errors; field/version/duplicate validation; groups and dependency closure; cycle diagnostics; requests/budgets; fake-clock scheduler; FIFO/fairness policy; impossible-budget diagnostics; `validate`, `plan`, and `doctor`.

Implement finite exit edges before service edges. Test success classification separately from predicates. Add config/environment/CLI precedence, stable tie-breaking, resource queue timeouts, and a final run summary.

**Gate:** examples conform to schema and Rust model; property tests find no budget oversubscription, double launch, double release, or hidden ordering assumption. Discovery failure is explicit.

#### M3 — Protection and Linux controller integration

**Deliver:** owned cgroup-v2 layout; startup capability matrix; verified apply-before-exec; aggregate/per-task limits; OOM evidence; complete cleanup; required-mode rejection; native macOS/fallback memory watchdog.

Separate observation, admission, and enforcement in code. Treat controller errors as state transitions, not ignored log lines. Reserve supervisor headroom outside child-workload caps. Handle unavailable swap/process controllers independently. Never write to unowned parent controller files as an automatic “fix.” Delegation needs an explicit supported environment. [S06](#s06)

**Gate:** bounded real-controller tests demonstrate containment and runner survival; fallback tests prove no hard guarantee is falsely reported. Immediate-fork and nested-cgroup fixtures pass. Size regressions are explained.

#### M4 — Services and readiness

**Deliver:** service states; TCP, HTTP, UDP, and stdio probes; timeout and retry budgets; pre-readiness and post-readiness failure behavior; transient service lifetime for finite roots; persistent root-service lifetime.

Ensure probes inspect captured bytes before rendering. Test an immediate startup message, a pattern split across chunks, wrong-peer UDP responses, a pre-existing port listener, connection success with unhealthy HTTP status, and a service that dies just as its dependent is eligible.

**Gate:** no service-dependent job-slot deadlock; expected services stay alive while needed and stop on finite completion. UDP cannot become ready through send success alone. HTTPS is either implemented with the approved secure path or explicitly rejected and documented.

#### M5 — CI-facing output and product polish

**Deliver:** terminal/plain/grouped/JSON renderers; bounded spooling and cleanup; versioned NDJSON; structured errors; help/completions as size permits; explanation of selected budgets and queued work; documentation using the selected stack brand.

Add a concurrency-aware status summary without requiring a full-screen TUI. Test narrow terminals, no color, non-TTY output, invalid UTF-8, zero-newline output, slow consumers, and broken pipes. Human warnings must not corrupt JSON stdout.

**Gate:** stable event fixtures and black-box assertions cover every scheduling/termination reason. Resource summaries identify measured versus estimated values.

#### M6 — Release hardening and dogfooding

**Deliver:** release matrix; artifact checksum manifest; compatibility policy; installation instructions; performance/size ledger; real-project comparisons; CI dogfooding; release checklist and known limitations.

Use Takt to schedule some repository checks only after the independent harness is reliable. Keep bootstrap compilation and safety checks outside Takt so a scheduler defect cannot turn the entire test system into circular evidence.

**Gate:** every shipped artifact runs its smoke suite on the promised platform; required protection passes in an explicitly capable Linux environment; no advertised platform relies solely on cross-compilation. If a sub-1 MB artifact is infeasible with agreed features, provide measured evidence and request an explicit size-policy decision.

### 4. CI design

Use GitHub Actions from M1 onward. Proposed baseline labels to verify in the repository are `ubuntu-24.04`, `ubuntu-24.04-arm`, `macos-15`, and `macos-15-intel`. Runner availability, repository entitlement, and labels must be checked before committing the workflow. Do not hardcode capacity assumptions from those names. [S13](#s13)

The PR workflow performs format/lint checks, unit and scheduler-property tests, native integration tests, schema/example checks, binary-size collection, and basic packaged-artifact smoke tests. Release CI expands architecture/compatibility coverage. A dedicated Linux safety job establishes verified delegation and runs bounded controller tests. It must fail rather than silently skip when that job is explicitly designated the hard-limit gate.

Use minimum repository permissions, verified full commit SHAs for Actions, explicit job timeouts, and no production secrets in workload fixtures. Do not run attacker-controlled fork PR code in a privileged persistent self-hosted runner. GitHub's security guidance recommends SHA pinning. [S18](#s18)

This package intentionally includes a **workflow plan**, not a fake working `.github/workflows` file with invented action SHAs, nonexistent binaries, or assumed test names. Generate the actual workflow after repository discovery and M1 structure exist.

### 5. Installation and artifact policy

Start with versioned downloadable archives and checksums. A shell installer may be a transport convenience later, not the scheduler implementation. Never require `curl | sh`; document manual verification and extraction. A future package-manager or npm distribution wrapper may fetch/invoke the native binary, but Node is not part of the core execution path.

Candidate release targets are Linux x86-64/AArch64 and macOS x86-64/Apple Silicon. Decide whether Linux provides GNU/system-linked, musl/self-contained, or both after native testing. Inspect executable dependencies, minimum libc/macOS version, DNS behavior, and launch semantics. Rust target names and support tiers are documented; deployment compatibility is still an empirical release gate. [S12](#s12)

Define “small” using uncompressed executable bytes, not a ZIP size. Report each target/profile separately. Standard and size-optimized builds should run the same required feature suite; a reduced-capability binary must have an explicit different feature label and owner approval. Do not silently drop UDP, YAML, cancellation, or protection to meet a marketing number.

### 6. Agent instructions and change control

1. Read all three specification/test documents and inspect the actual repository before creating files or choosing paths. Never assume the unrelated Namefi repository is Takt's repository.
2. Recheck primary docs for the exact installed APIs. Create small runnable proofs for uncertain OS behavior. Pin dependencies after the spike, not from stale examples.
3. Implement one milestone in reviewable changes, including tests and updated documentation. Keep every acceptance claim tied to evidence.
4. On a mismatch, record the observed behavior, source/version, repro, affected requirement, and proposed options. Stop the affected promise and obtain approval before altering accepted scope or guarantees. Unaffected work can continue.
5. Update the requirement traceability table and size/capability ledger in every milestone review. Do not mark skipped tests as verified behavior.

#### Completion packet for each milestone

Provide changed paths, commands actually run, platform/runner details, passing/failing/skipped tests, exact executable size, new limitations, unresolved decisions, and the next milestone's smallest deliverable. Reports should distinguish real measurements from simulated observations.

### 7. First engineering backlog

| Ticket | Deliverable | Depends on | Done when |
|---|---|---|---|
| TAKT-001 | Repo and accepted decision record | Owner repo choice | Scope/floors/HTTPS/size policy captured |
| TAKT-002 | Build/size/dependency spike | 001 | Comparable artifact ledger exists |
| TAKT-003 | Safe child launch and group cleanup | 001 | Immediate-fork/cancel proofs pass on applicable platforms |
| TAKT-004 | Finite runner vertical slice | 002, 003 | M1 gate passes on Linux/macOS |
| TAKT-005 | Strict YAML/JSON and graph | 004 | Schema, precedence, branch tests pass |
| TAKT-006 | Weighted admission and diagnostics | 005 | Deterministic invariants/fairness tests pass |
| TAKT-007 | Linux protection/fallback contract | 003, 006 | Real safety job and refusal tests pass |
| TAKT-008 | Services and four readiness probes | 005, 006 | M4 lifecycle/probe gate passes |
| TAKT-009 | Structured output and UX | 004–008 | Event and bounded-output suite passes |
| TAKT-010 | Packaging, docs, native release tests | 007–009 | M6 gate passes; owner signs off remaining limits |

Ticket IDs are planning labels only; no GitHub issues or ClickUp tasks have been created.

---

<a id="test-and-ci-plan"></a>

## Takt — Test, CI, and Release Acceptance Plan

**Version:** 0.1 · **Date:** 16 October 2026 · **Status:** test specification, not executed test results

### 1. Evidence hierarchy

Use four complementary layers: pure scheduler/graph tests, controlled process fixtures, real OS containment tests, and representative project dogfooding. A synthetic resource snapshot proves a scheduling decision, not kernel enforcement. A successful compile proves neither native execution nor cleanup correctness.

An independent harness must build and test the runner before Takt schedules any of its own checks. Preserve that independent path permanently.

### 2. Fixture executable

Build a development-only Rust fixture executable with bounded behaviors: sleep/exit, alternate stdout/stderr, emit partial/no-newline/non-UTF-8 output, fork a bounded tree, ignore a signal, spawn immediately, detach where supported, allocate/touch a bounded number of memory pages, burn CPU for a bounded duration, TCP listener, configurable HTTP responder, UDP echo/protocol responder, startup announcement, and delayed exit.

Every fixture accepts a maximum duration and bounded allocation/output/child count. The test harness retains an independent cleanup handle. Use loopback addresses, ephemeral ports allocated by the fixture, and explicit handshakes instead of races around a guessed “free port.” No internet access, production credentials, fork bombs, or unbounded allocators are required.

### 3. Required test inventory

#### Configuration and graph

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

#### Admission and dependency policies

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

#### Services and readiness

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

#### Supervision, logs, and cancellation

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

### 4. Real Linux safety tests

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

### 5. Native macOS acceptance

Run all applicable portable tests natively. Test `required` refusal for unsupported aggregate protection, `auto` capability text, bounded watchdog termination, and group cleanup. The watchdog fixture stays well below host danger levels. Passing a sampled-memory test does not establish a hard cap. Record limitations for detached processes and shared-memory accounting.

Do not use Linux containers as evidence that a macOS-native binary works. Cross-compilation also does not replace a native runtime test.

### 6. CI matrix and cadence

| Lane | Cadence | Minimum evidence |
|---|---|---|
| Linux x86-64 portable | Every PR | Build, lint, unit/property, integration, size |
| macOS Apple Silicon portable | Every PR | Native process/probe/cleanup and refusal tests |
| Linux real-controller safety | Required for merge once M3 lands | L01–L10 with verified external boundaries |
| Linux AArch64 / macOS Intel | Nightly and release; PR for platform changes | Native artifact smoke and platform integration |
| Compatibility containers/VMs | Release | Promised libc/OS floor and runtime dependencies |
| Fuzz/property expansion | Scheduled and before release | Config, graph, streaming matcher, event-order exploration |
| Real project comparisons | Milestone and release | Equivalent finite workload versus shell/concurrently, no unsupported claims |

Verify exact labels, availability, and permissions in the repository. GitHub documents multiple hardware profiles; record observed resources for each execution rather than assuming a fixed 16-vCPU runner. [S13](#s13)

### 7. Performance and binary-size measurements

For each target/profile, record uncompressed executable bytes and archive bytes separately. Inspect linked dependencies, verify checksum, then run the downloaded/extracted artifact rather than only a build-tree binary.

Use a 60-second idle-service observation for supervisor CPU and resident memory, excluding child workload usage and distinguishing shared/virtual memory. Repeat measurements on a quiet controlled machine where practical. Proposed goals are below 1,000,000 executable bytes where feasible, below 5 MiB quiescent supervisor RSS, and below 0.1% of one core idle CPU. They are not current results. A CI VM's measurement noise must not produce unjustified precision.

Benchmark fixed independent tasks at one, two, and several admitted slots; mixed CPU/memory requests; many tiny tasks; readiness overhead; and heavy output. Compare identical commands, environment, logging, and concurrency budgets. Publish median plus spread, raw run data, platform, compiler, and features. Do not extrapolate runner language alone into whole-workflow savings.

Once a baseline is approved, block unexplained size growth beyond a proposed 10% or 100 KiB, whichever is larger. That regression policy is separate from the below-1 MB aspiration. A release above target needs an explicit decision, not misleading compressed-size reporting.

### 8. Requirement traceability

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

### 9. Release checklist

Release only after accepted decisions are closed, examples pass both structural and semantic validation, every advertised artifact has native smoke evidence, the designated safety gate passes, cleanup/output regressions are clear, binary/dependency measurements are published, installer checksums are verified, and docs accurately label platform differences.

Retain raw event logs, fixture outcomes, resource snapshots, controller capability evidence, artifact hashes, and size results as CI artifacts. Redact test secrets and never attach production environment dumps. Failures must be reproducible from bounded fixtures and recorded seeds.

This handoff has validated document/example/asset structure only. It does not report Takt application tests as executed; no application implementation exists in this package.

---

<a id="decision-record"></a>

## Takt — Decision Record

**Date:** 16 October 2026

### Accepted from the discussion

Takt is the chosen name. Rust is the implementation direction after considering Bash and Go. Linux/macOS are first; Windows comes later. Finite CI/check commands are primary, with both sequential and parallel execution. User-supplied settings take priority over automatic discovery. Long-running services remain supported. Dependencies include finite-command exit codes and service readiness. Initial probes are TCP, HTTP, UDP, and stdout/stderr; custom commands are later. Small native delivery and CI-based proof matter. The selected brand is **Concept 02 Stack**, not Concept 01 Waveform and not the later hybrid.

The earlier binary-size ranges and idle-memory numbers were estimates, not measurements. The PRD treats below 1 MB as a target and requires actual measurements. Likewise, native macOS protection is not represented as Linux-equivalent hard containment.

### Proposed defaults, not owner-approved implementation facts

| Decision | Proposed default | Validation/approval needed |
|---|---|---|
| Invocation precedence | CLI → environment → file → default/auto | Confirm with example-based tests |
| Failure aggregation | Continue independent checks | Confirm UX; fail-fast remains explicit |
| Edge mismatch | Neutral skip when prerequisite otherwise succeeded | Confirm conditional branch reporting |
| Service exit after ready | Fail run, do not kill active dependents by default | Confirm optional stop-dependents scope |
| Unknown task request | 1 CPU + 1 GiB, inferred values reduced on smaller hosts | Benchmark real lint/build/test workloads |
| Auto memory reserve | max(256 MiB, 15% of initial headroom) | Validate constrained host behavior |
| Resource queue timeout | 120 seconds | Confirm default versus unlimited opt-in |
| Readiness timeout / retry / attempt | 30 s / 250 ms / 1 s | Test slow startup and network failure |
| Stop grace | 5 seconds | Test conventional child tools |
| Memory watchdog | 100–250 ms observation; explicitly best effort | Validate overhead and platform accounting |
| HTTP scheme scope | Plain HTTP in baseline V1 | Owner must decide whether HTTPS is V1-required |
| Small binary | Same V1 behavior, below 1,000,000 bytes when feasible | Measure before public promise |

### Decisions to close before implementation commits

Repository and software license are unspecified. No repository or issues were created. Confirm macOS/Linux compatibility floors and which architectures must ship on day one. Confirm the source/availability of a delegated Linux controller test environment. Decide HTTPS scope, exact CLI/schema surface, release artifact variants, and what size overage is acceptable with full functionality.

Do not block useful design work on trademark or package-name availability, but do not claim the name is legally cleared or a registry namespace is available. No such search or reservation is represented as completed.

### Brand production note

The kit's SVGs are clean vector reconstructions of the selected raster concept. The three offset horizontal bars and cyan-to-green direction are retained. The wordmark is standardized with outlined glyphs so it does not depend on an installed font. These are not original editable files recovered from the image generator. Original exploration boards are preserved separately and explicitly marked non-production references.

### Change procedure

When a tested API or OS behavior contradicts this specification: record the exact version/platform and reproduction, identify the affected promise, present the smallest viable alternatives, and obtain approval before changing scope or guarantees. Correctness and honest protection claims take priority over an arbitrary size target.

---

<a id="configuration-examples"></a>

## Takt configuration examples

These are **proposed V1 configurations**, not commands tested against a released Takt binary. Project scripts and service executables are illustrative; replace them with real commands from the target repository. The files have been checked for YAML/JSON syntax and conformance to the accompanying draft schema. That is not runtime validation.

| File | Demonstrates |
|---|---|
| `01-checks.yaml` / `.json` | Equivalent automatic-budget finite check groups |
| `02-ordered.yaml` | Exit-code dependencies impose order regardless of available slots |
| `03-exit-code-branches.yaml` | Expected nonzero detector outcome and neutral branch skip |
| `04-service-backed-tests.yaml` | HTTP and stdio readiness; finite roots stop owned services; dev roots persist |
| `05-tcp-udp-probes.yaml` | TCP connection and UDP payload/reply contracts |
| `06-protected-linux-ci.yaml` | Explicit per-task limits plus required verified aggregate protection |

The examples intentionally include explicit task requests. A request larger than a machine's effective budget is a configuration/policy error; the runner must not silently shrink it. Reduce declared requests knowingly or use appropriate automatic defaults. For example, the two-CPU test in `01-checks.yaml` cannot run under a one-CPU total budget as written.

`03-exit-code-branches.yaml` is a deliberately simple future CLI smoke example: the detector returns 2 and declares that result successful; the `changed` branch should run and `unchanged` should skip. `jobs: 1` is not the source of branch correctness—the dependency predicates are.

The HTTP example expects a body containing the literal string `ready`; adapt it to your health endpoint. TCP readiness cannot prove a pre-existing listener is your service. UDP services must actually implement the stated `PING`/`PONG` exchange; this is not a generic readiness protocol that every UDP application supports.

### Draft data conventions

Task/group IDs begin with a letter and use letters, numbers, underscores, or hyphens. Memory values are positive integer quantities with `B`, `KiB`, `MiB`, `GiB`, or `TiB`. CPU is a positive number in CPU equivalents with up to three fractional digits. Durations use positive integer `ms`, `s`, `m`, or `h` values. Invocation jobs/CPU/memory can be `auto`; resource-queue timeout can be `off`. Task commands use argv arrays or an explicit shell string, never both.

Schema validation is structural only. The application still needs duplicate-key rejection during parsing, graph reference/cycle checks, success/edge semantics, numeric overflow checks, valid URL/address handling, live service feasibility, and platform protection checks.

### 01-checks.yaml

```yaml
# Proposed Takt v1 configuration; not an implemented CLI contract.
# Replace illustrative project commands with commands that exist in your repository.
version: 1
scheduling:
  jobs: auto
  cpu: auto
  memory: auto
  queue_timeout: 120s
enforcement: auto
failure: continue
output: auto
defaults:
  stop_grace: 5s
tasks:
  lint:
    kind: task
    command:
    - npm
    - run
    - lint
    resources:
      cpu: 1
      memory: 512MiB
    timeout: 5m
  typecheck:
    kind: task
    command:
    - npm
    - run
    - typecheck
    resources:
      cpu: 1
      memory: 1GiB
    timeout: 10m
  test:
    kind: task
    command:
    - npm
    - test
    resources:
      cpu: 2
      memory: 2GiB
    timeout: 15m
groups:
  checks:
  - lint
  - typecheck
  - test
```


### 02-ordered.yaml

```yaml
# Proposed Takt v1 configuration; not an implemented CLI contract.
# Replace illustrative project commands with commands that exist in your repository.
version: 1
scheduling:
  jobs: auto
failure: fail-fast
tasks:
  generate:
    kind: task
    command:
    - npm
    - run
    - generate
  build:
    kind: task
    command:
    - npm
    - run
    - build
    depends_on:
    - task: generate
      when: exit
      codes:
      - 0
  package:
    kind: task
    command:
    - npm
    - run
    - package
    depends_on:
    - task: build
      when: exit
      codes:
      - 0
groups:
  release-check:
  - package
```


### 03-exit-code-branches.yaml

```yaml
# Proposed Takt v1 configuration; not an implemented CLI contract.
# Replace illustrative project commands with commands that exist in your repository.
version: 1
scheduling:
  jobs: 1
failure: continue
tasks:
  detect:
    kind: task
    shell: exit 2
    success_codes:
    - 0
    - 2
  unchanged:
    kind: task
    shell: printf '%s\n' 'No changes detected'
    depends_on:
    - task: detect
      when: exit
      codes:
      - 0
  changed:
    kind: task
    shell: printf '%s\n' 'Changes detected; run the selected branch'
    depends_on:
    - task: detect
      when: exit
      codes:
      - 2
groups:
  branch-demo:
  - unchanged
  - changed
```


### 04-service-backed-tests.yaml

```yaml
# Proposed Takt v1 configuration; not an implemented CLI contract.
# Replace illustrative project commands with commands that exist in your repository.
version: 1
scheduling:
  jobs: 4
  cpu: 4
  memory: 4GiB
enforcement: auto
failure: continue
tasks:
  prepare:
    kind: task
    command:
    - npm
    - run
    - prepare:test-data
    resources:
      cpu: 1
      memory: 256MiB
  api:
    kind: service
    command:
    - npm
    - run
    - dev:api
    resources:
      cpu: 1
      memory: 1GiB
    depends_on:
    - task: prepare
      when: exit
      codes:
      - 0
    readiness:
      type: http
      url: http://127.0.0.1:3000/health
      status_codes:
      - 200
      body_contains: ready
      timeout: 30s
  worker:
    kind: service
    command:
    - npm
    - run
    - dev:worker
    resources:
      cpu: 1
      memory: 512MiB
    readiness:
      type: stdio
      stream: either
      contains: WORKER_READY
      timeout: 30s
  integration:
    kind: task
    command:
    - npm
    - run
    - test:integration
    resources:
      cpu: 2
      memory: 1GiB
    depends_on:
    - task: api
      when: ready
    - task: worker
      when: ready
    timeout: 10m
groups:
  checks:
  - integration
  dev:
  - api
  - worker
```


### 05-tcp-udp-probes.yaml

```yaml
# Proposed Takt v1 configuration; not an implemented CLI contract.
# Replace illustrative project commands with commands that exist in your repository.
version: 1
scheduling:
  jobs: 2
  cpu: 2
  memory: 2GiB
tasks:
  tcp-example:
    kind: service
    command:
    - ./your-tcp-service
    resources:
      cpu: 1
      memory: 512MiB
    readiness:
      type: tcp
      host: 127.0.0.1
      port: 8080
      timeout: 30s
      attempt_timeout: 1s
      interval: 250ms
  udp-example:
    kind: service
    command:
    - ./your-udp-service
    resources:
      cpu: 1
      memory: 512MiB
    readiness:
      type: udp
      host: 127.0.0.1
      port: 9090
      payload:
        text: PING
      expect:
        equals:
          text: PONG
      timeout: 30s
      attempt_timeout: 1s
      interval: 250ms
groups:
  probe-demo:
  - tcp-example
  - udp-example
```


### 06-protected-linux-ci.yaml

```yaml
# Proposed Takt v1 configuration; not an implemented CLI contract.
# Replace illustrative project commands with commands that exist in your repository.
version: 1
scheduling:
  jobs: 2
  cpu: 2
  memory: 3GiB
enforcement: required
failure: continue
tasks:
  lint:
    kind: task
    command:
    - npm
    - run
    - lint
    resources:
      cpu: 1
      memory: 1GiB
    limits:
      cpu: 1
      memory: 1GiB
    timeout: 5m
  test:
    kind: task
    command:
    - npm
    - test
    resources:
      cpu: 1
      memory: 1GiB
    limits:
      cpu: 1
      memory: 1GiB
    timeout: 10m
groups:
  checks:
  - lint
  - test
```


<a id="schema-in-bundle"></a>
### Schema in the companion bundle

The complete ZIP includes `schema/takt.schema.json` and an equivalent JSON version of the first example. The schema is a proposed structural contract; the implementation still needs semantic validation and actual runtime tests. See `VALIDATION.md` inside the ZIP for the package checks actually performed.


---

<a id="technical-sources"></a>

## Takt — Verified Technical Sources

**Checked:** 16 October 2026. These are primary platform/maintainer references used to constrain the proposal. They do not prove the proposed Takt interfaces are implemented. Moving documentation should be rechecked and exact dependency/toolchain versions recorded during M0. No lengthy quotations are reproduced.

<a id="s01"></a>
### S01 — concurrently capabilities

[Maintainer repository and API description](https://github.com/open-cli-tools/concurrently). Documents `maxProcesses`, success conditions, output handling, and configurable termination. This corrects an overly broad “only launches everything at once” comparison.

<a id="s02"></a>
### S02 — Node environment options

[Node.js command-line documentation](https://nodejs.org/api/cli.html#node_optionsoptions). `NODE_OPTIONS` configures Node invocations. A native supervisor does not eliminate its effect on inherited Node child environments.

<a id="s03"></a>
### S03 — Rust available parallelism

[std::thread::available_parallelism](https://doc.rust-lang.org/std/thread/fn.available_parallelism.html). Returns an estimate and documents host/container/affinity limitations. It does not expose current load.

<a id="s04"></a>
### S04 — Linux available-memory observation

[Linux man-pages: proc_meminfo(5)](https://man7.org/linux/man-pages/man5/proc_meminfo.5.html). Explains `MemAvailable` as an estimate for starting applications without swapping. A scheduling observation is not an allocation guarantee.

<a id="s05"></a>
### S05 — Linux cgroup v2 controllers

[Linux kernel cgroup-v2 documentation](https://docs.kernel.org/admin-guide/cgroup-v2.html). Defines CPU bandwidth, memory ceilings, group OOM behavior, event counters, swap controls, and hierarchy rules. The proposed owned-subtree layout and policy are Takt design choices, not copied interfaces.

<a id="s06"></a>
### S06 — Delegation and ownership

[systemd: Control Group APIs and Delegation](https://systemd.io/CGROUP_DELEGATION/). Describes delegated subtrees and ownership boundaries. Presence of cgroup files does not by itself establish permission to manage a subtree.

<a id="s07"></a>
### S07 — Apple resource limits, archival scope

[Apple archived getrlimit(2) reference](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/getrlimit.2.html). Describes process-specific resource limits. This archival reference is not a verified modern aggregate-memory isolation contract; native macOS behavior needs empirical validation.

<a id="s08"></a>
### S08 — rlimit distinctions

[Linux man-pages: getrlimit(2)](https://man7.org/linux/man-pages/man2/getrlimit.2.html). Distinguishes address-space, CPU-time, and other per-process limits. They are not interchangeable with aggregate workload memory and CPU-bandwidth policy.

<a id="s09"></a>
### S09 — UDP socket semantics

[Rust std::net::UdpSocket](https://doc.rust-lang.org/std/net/struct.UdpSocket.html). Defines UDP send/receive/peer behavior. Takt's validated response requirement is an application-level readiness design, not something implied by send success.

<a id="s10"></a>
### S10 — Unix child process setup

[Rust std::os::unix::process::CommandExt](https://doc.rust-lang.org/std/os/unix/process/trait.CommandExt.html). Stable `process_group` is available. `pre_exec` requires careful unsafe/post-fork handling. Do not assume every method shown on the page is stable; verify the exact selected API.

<a id="s11"></a>
### S11 — Rust release-profile controls

[Cargo profiles](https://doc.rust-lang.org/cargo/reference/profiles.html). Documents optimization, stripping, LTO, codegen, and panic settings. A particular binary size remains a measurement, not a consequence guaranteed by choosing Rust.

<a id="s12"></a>
### S12 — Rust compilation targets

[Rust platform support](https://doc.rust-lang.org/rustc/platform-support.html). Use to select candidate target triples and support expectations; artifact execution and OS/libc floor testing remain separate release requirements.

<a id="s13"></a>
### S13 — GitHub runner environments

[GitHub-hosted runners reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners). Describes labels, hardware variants, platform availability, and privilege context. Verify in the actual repository and record observed resources.

<a id="s14"></a>
### S14 — YAML parser candidate: yaml_serde

[yaml_serde maintainer API docs](https://docs.rs/yaml_serde/latest/yaml_serde/). Identifies a maintained YAML-organization fork and Serde integration. Candidate only; validate bounded parsing, diagnostics, features, and size.

<a id="s15"></a>
### S15 — YAML parser candidate: serde-saphyr

[serde-saphyr maintainer API docs](https://docs.rs/serde-saphyr/latest/serde_saphyr/). Alternative typed YAML implementation. Candidate only; package selection requires its own version/security/size review.

<a id="s16"></a>
### S16 — I/O candidate: Mio

[Mio Poll documentation](https://docs.rs/mio/latest/mio/struct.Poll.html). Documents readiness-based I/O and platform caveats. Takt must still supply correct process, timer, signal, and stream handling.

<a id="s17"></a>
### S17 — I/O candidate: polling

[polling documentation](https://docs.rs/polling/latest/polling/). Documents a portable OS event-polling interface. Candidate, not a guarantee of complete process-supervisor support.

<a id="s18"></a>
### S18 — GitHub Actions security

[GitHub secure-use reference](https://docs.github.com/en/actions/reference/security/secure-use). Supports least-privilege workflow design and full-length action commit pinning. Workflow SHAs must be resolved from the real action repository, not invented in a template.

### Brand/style references

[W3C WCAG 2.2 contrast minimum](https://www.w3.org/WAI/WCAG22/Understanding/contrast-minimum.html) informs documentation text contrast. Logo colors are not a blanket palette for readable body text. [Inter's official site](https://rsms.me/inter/) is the typography reference. Font binaries are not included in this package.
