# Runtime contract

## Tasks, dependencies, and services

Task and group identifiers are disjoint. Selecting multiple roots includes their
dependency closure once. Every dependency must match. `when: exit` accepts only
a normal exit with a listed code; `success_codes` independently determines whether
that exit is successful. A matching failed prerequisite can continue a branch
under `continue`, but the invocation still fails. Unmatched successful branches
skip neutrally, including downstream branches.

`when: ready` requires a service probe and a provider still alive at launch.
Unexpected service exit fails the run and blocks future consumers. Active consumers
continue unless `service_failure: stop-dependents` is set. Services selected as roots
persist until cancellation. Dependency services stop after their consumers finish.
Readiness is not automatically inferred for a service without a probe.

## Scheduling defaults

CPU values use integer millicores with at most three decimal digits at input.
Memory uses positive integer B/KiB/MiB/GiB/TiB values. Duration values are positive
integer ms/s/m/h values. Resource queue and execution timeout can explicitly be `off`.
Execution timeout defaults to off and excludes waiting; stop grace is separate.

An inferred request starts at 1 CPU and 1 GiB and can shrink to fit total capacity.
File defaults count as explicit requests. CPU and memory limits are independent
from requests. Automatic memory reserve is the larger of 256 MiB or 15% of initial
headroom. Unknown memory headroom is reported and defaults to one job. OS ceilings
bound requested budgets. Read-only plans show effective policy and provenance.

Ready work uses eligibility FIFO with lexical identifier tie-breaking. Smaller
fitting tasks may pass blocked work until five-second aging reserves newly available
capacity for older work. Services count for their entire lifetime. Impossibly large
requests and service-reservation deadlocks are errors. Resource queue timeout is
120 seconds and excludes ordinary dependency waiting. Live pressure pauses admission
without terminating healthy tasks.

## Readiness

TCP checks a listener, not service identity. HTTP performs a plain GET, defaults to
status 200, does not redirect, and optionally matches a literal body substring.
HTTPS is rejected. UDP requires a reply from the configured peer matching exact
bytes or a prefix. Stdio matches literal UTF-8 pattern bytes across capture chunks,
with independent stdout/stderr state; invalid output bytes do not break matching.

Readiness defaults are 30 seconds overall, 250 ms retry interval, and one second
per network attempt including DNS. Responses are bounded to 64 KiB. At most two
private resolver subprocesses use the system resolver; numeric IPs bypass lookup.

## Lifecycle and enforcement

Project commands execute in separate process groups through a controlled same-binary
helper. The helper starts with a clean environment; task cwd/environment are applied
only after the parent verifies requested protection. Failed exec remains a spawn
failure rather than being confused with a legitimate exit 127.

Linux hard protection uses owned cgroup-v2 subtrees beneath an already delegated
hierarchy. Takt never raises privileges or edits unrelated parent settings. The
supervisor remains outside its workload cap. Partial controllers are reported by
dimension. Native macOS uses conservative available-memory estimates and process-group
RSS monitoring, with an explicit best-effort capability report.

Cancellation stops admission, signals owned groups, allows five seconds by default,
then forces termination. A second interrupt escalates. Reservations remain held until
cleanup. Process-group leaders remain unreaped during cleanup to prevent PID reuse.
Hard memory failures require controller evidence; exit 137 alone is not OOM proof.
Fallback watchdogs observe at 200 ms intervals. Aggregate fallback termination chooses
the most recently started workload and does not claim reliable offender attribution.

## Parsing and output bounds

Configuration is bounded to 1 MiB, depth 64, 100,000 parsed/expanded events, 1,024
anchors/aliases, and 4 MiB retained parser data. Duplicate fields and explicit YAML
tags are rejected. No includes, interpolation, or implicit dotenv loading occur.
Default/task environment merging is bounded separately to 100,000 retained
environment/unset entries and 8 MiB of names/values across defaults and normalized
tasks. Budget checks precede inherited string copies; exceeding a budget produces
configuration exit 2 with no environment values in the diagnostic.

The selected graph's service ancestry is computed in dependency order, with
limits of 100,000 retained entries, 8 MiB of copied ancestor names, and 1,000,000
traversal/union steps. These bounds prevent compact graphs from expanding into
unbounded planning memory or work. A graph exceeding them fails with policy exit
3 before launch; select fewer tasks or split that run. Finite-only chains retain
empty service-ancestor sets.

Logs use a 1 MiB renderer queue and grouped storage of at most 64 MiB per task.
Overflow is reported while pipes continue draining and readiness continues matching.
Lifecycle records are retained independently of disposable logs. Grouped spool files
are private temporary files. Final output drainage waits at most one second after
workload cleanup. Broken or undeliverable output fails the invocation.

Events use schema version 1, run ID, increasing sequence, monotonic elapsed milliseconds,
event name, optional task identifier, and event-specific data. Log records contain
`stream`, `encoding` (`utf8` or `base64`), and `data`. Summaries contain selected roots,
outcomes, counts, and exit code. JSON stdout contains NDJSON only.
