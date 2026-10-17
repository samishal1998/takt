# Takt — Implementation and Agent Handoff

**Version:** 0.1 · **Date:** 16 October 2026 · **Status:** proposed execution plan

Read the [PRD](01-PRD.md) first. Its agreed requirements outrank convenience choices in this plan. No Takt repository has been identified in the supplied context, and no repository inspection, changes, builds, or CI execution are represented as completed by this handoff.

## 1. Engineering approach

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

## 2. Dependency and size spike

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

The YAML and I/O candidates have current maintainer documentation, but this handoff does not pin package versions or claim they meet Takt's final size/safety requirements. Verify the exact version and interface at implementation time. [S14](05-Technical-sources.md#s14) [S15](05-Technical-sources.md#s15) [S16](05-Technical-sources.md#s16) [S17](05-Technical-sources.md#s17)

Avoid choosing a large async stack reflexively, but also avoid hand-writing a fragile runtime merely to save bytes. Blocking hostname resolution must be bounded and isolated. Reject architectures that can wedge process reaping when output blocks. Keep a bounded number of OS threads; unbounded threads-per-probe is not acceptable.

Try release stripping, `opt-level` size settings, LTO, and codegen-unit choices as measured variants. A panic-abort profile is not automatically accepted: it can bypass cleanup after a defect. Minimize panic paths, test parent-failure behavior, and document the external cleanup boundary. Cargo's controls are documented, but the winning configuration must come from the full-feature benchmark. [S11](05-Technical-sources.md#s11)

Do not include embedded logos, font assets, sample projects, or a browser UI in the CLI binary. The brand kit belongs in docs and release presentation.

## 3. Milestones and exit gates

### M0 — Specification, compatibility, and risk reduction

**Deliver:** accepted decision log; repository inventory; supported targets/floors; dependency/size ledger; native discovery prototype; process-launch proof; Linux delegation proof; exact current API/source notes.

Prove a child cannot execute project code before its requested limits are attached. A practical candidate is a same-binary private launch mode: spawn a controlled helper, hold it behind a handshake, attach and verify the owned workload group, then allow exec. The helper must close inherited descriptors, validate its protocol, and never accept untrusted control messages. Compare this with audited low-level launch alternatives; do not improvise unsafe work in a multithreaded post-fork closure. [S10](05-Technical-sources.md#s10)

A successful spike must show that a child that forks immediately remains contained on the tested Linux path. Native macOS must show stable process-group cancellation and truthful capability rejection. Closing M0 requires owner decisions on HTTPS and protection/size trade-offs, not guessed approval.

### M1 — Runnable finite-task slice

**Deliver:** direct argv execution; explicit shell strings; bounded stdout/stderr multiplexing; exit capture; working directory and environment; CLI serial/parallel controls; signal cleanup; basic Linux/macOS CI.

Use a bounded purpose-built test fixture rather than relying on `nc`, Python, Node, curl, or shell timing tricks for core correctness. Run two sleeping tasks in parallel and serial modes; verify the sequence and exit result. Kill a tree on cancellation. Reject malformed arguments and launch failures cleanly.

**Gate:** independent integration harness passes natively on Linux and macOS; no deadlock under large dual-stream output; first executable-size report published. At this point the program is useful without YAML, dependencies, or service mode.

### M2 — Configuration, graph, and deterministic scheduling

**Deliver:** YAML/JSON parsing with source errors; field/version/duplicate validation; groups and dependency closure; cycle diagnostics; requests/budgets; fake-clock scheduler; FIFO/fairness policy; impossible-budget diagnostics; `validate`, `plan`, and `doctor`.

Implement finite exit edges before service edges. Test success classification separately from predicates. Add config/environment/CLI precedence, stable tie-breaking, resource queue timeouts, and a final run summary.

**Gate:** examples conform to schema and Rust model; property tests find no budget oversubscription, double launch, double release, or hidden ordering assumption. Discovery failure is explicit.

### M3 — Protection and Linux controller integration

**Deliver:** owned cgroup-v2 layout; startup capability matrix; verified apply-before-exec; aggregate/per-task limits; OOM evidence; complete cleanup; required-mode rejection; native macOS/fallback memory watchdog.

Separate observation, admission, and enforcement in code. Treat controller errors as state transitions, not ignored log lines. Reserve supervisor headroom outside child-workload caps. Handle unavailable swap/process controllers independently. Never write to unowned parent controller files as an automatic “fix.” Delegation needs an explicit supported environment. [S06](05-Technical-sources.md#s06)

**Gate:** bounded real-controller tests demonstrate containment and runner survival; fallback tests prove no hard guarantee is falsely reported. Immediate-fork and nested-cgroup fixtures pass. Size regressions are explained.

### M4 — Services and readiness

**Deliver:** service states; TCP, HTTP, UDP, and stdio probes; timeout and retry budgets; pre-readiness and post-readiness failure behavior; transient service lifetime for finite roots; persistent root-service lifetime.

Ensure probes inspect captured bytes before rendering. Test an immediate startup message, a pattern split across chunks, wrong-peer UDP responses, a pre-existing port listener, connection success with unhealthy HTTP status, and a service that dies just as its dependent is eligible.

**Gate:** no service-dependent job-slot deadlock; expected services stay alive while needed and stop on finite completion. UDP cannot become ready through send success alone. HTTPS is either implemented with the approved secure path or explicitly rejected and documented.

### M5 — CI-facing output and product polish

**Deliver:** terminal/plain/grouped/JSON renderers; bounded spooling and cleanup; versioned NDJSON; structured errors; help/completions as size permits; explanation of selected budgets and queued work; documentation using the selected stack brand.

Add a concurrency-aware status summary without requiring a full-screen TUI. Test narrow terminals, no color, non-TTY output, invalid UTF-8, zero-newline output, slow consumers, and broken pipes. Human warnings must not corrupt JSON stdout.

**Gate:** stable event fixtures and black-box assertions cover every scheduling/termination reason. Resource summaries identify measured versus estimated values.

### M6 — Release hardening and dogfooding

**Deliver:** release matrix; artifact checksum manifest; compatibility policy; installation instructions; performance/size ledger; real-project comparisons; CI dogfooding; release checklist and known limitations.

Use Takt to schedule some repository checks only after the independent harness is reliable. Keep bootstrap compilation and safety checks outside Takt so a scheduler defect cannot turn the entire test system into circular evidence.

**Gate:** every shipped artifact runs its smoke suite on the promised platform; required protection passes in an explicitly capable Linux environment; no advertised platform relies solely on cross-compilation. If a sub-1 MB artifact is infeasible with agreed features, provide measured evidence and request an explicit size-policy decision.

## 4. CI design

Use GitHub Actions from M1 onward. Proposed baseline labels to verify in the repository are `ubuntu-24.04`, `ubuntu-24.04-arm`, `macos-15`, and `macos-15-intel`. Runner availability, repository entitlement, and labels must be checked before committing the workflow. Do not hardcode capacity assumptions from those names. [S13](05-Technical-sources.md#s13)

The PR workflow performs format/lint checks, unit and scheduler-property tests, native integration tests, schema/example checks, binary-size collection, and basic packaged-artifact smoke tests. Release CI expands architecture/compatibility coverage. A dedicated Linux safety job establishes verified delegation and runs bounded controller tests. It must fail rather than silently skip when that job is explicitly designated the hard-limit gate.

Use minimum repository permissions, verified full commit SHAs for Actions, explicit job timeouts, and no production secrets in workload fixtures. Do not run attacker-controlled fork PR code in a privileged persistent self-hosted runner. GitHub's security guidance recommends SHA pinning. [S18](05-Technical-sources.md#s18)

This package intentionally includes a **workflow plan**, not a fake working `.github/workflows` file with invented action SHAs, nonexistent binaries, or assumed test names. Generate the actual workflow after repository discovery and M1 structure exist.

## 5. Installation and artifact policy

Start with versioned downloadable archives and checksums. A shell installer may be a transport convenience later, not the scheduler implementation. Never require `curl | sh`; document manual verification and extraction. A future package-manager or npm distribution wrapper may fetch/invoke the native binary, but Node is not part of the core execution path.

Candidate release targets are Linux x86-64/AArch64 and macOS x86-64/Apple Silicon. Decide whether Linux provides GNU/system-linked, musl/self-contained, or both after native testing. Inspect executable dependencies, minimum libc/macOS version, DNS behavior, and launch semantics. Rust target names and support tiers are documented; deployment compatibility is still an empirical release gate. [S12](05-Technical-sources.md#s12)

Define “small” using uncompressed executable bytes, not a ZIP size. Report each target/profile separately. Standard and size-optimized builds should run the same required feature suite; a reduced-capability binary must have an explicit different feature label and owner approval. Do not silently drop UDP, YAML, cancellation, or protection to meet a marketing number.

## 6. Agent instructions and change control

1. Read all three specification/test documents and inspect the actual repository before creating files or choosing paths. Never assume the unrelated Namefi repository is Takt's repository.
2. Recheck primary docs for the exact installed APIs. Create small runnable proofs for uncertain OS behavior. Pin dependencies after the spike, not from stale examples.
3. Implement one milestone in reviewable changes, including tests and updated documentation. Keep every acceptance claim tied to evidence.
4. On a mismatch, record the observed behavior, source/version, repro, affected requirement, and proposed options. Stop the affected promise and obtain approval before altering accepted scope or guarantees. Unaffected work can continue.
5. Update the requirement traceability table and size/capability ledger in every milestone review. Do not mark skipped tests as verified behavior.

### Completion packet for each milestone

Provide changed paths, commands actually run, platform/runner details, passing/failing/skipped tests, exact executable size, new limitations, unresolved decisions, and the next milestone's smallest deliverable. Reports should distinguish real measurements from simulated observations.

## 7. First engineering backlog

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
