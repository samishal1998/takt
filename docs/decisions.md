# Takt v0.1 accepted decisions

These decisions implement the approved v0.1 plan. The original proposal remains in
[`takt-handoff-v0.1/`](../takt-handoff-v0.1/). Implementation and test evidence are
tracked separately in [implementation-status.md](implementation-status.md).

## Product and distribution

| Decision | Accepted choice |
|---|---|
| Repository | Public `samishal1998/takt`; this directory is the project root |
| License | MIT OR Apache-2.0 |
| Application | Standalone Rust finite command runner, with dependency and service support |
| Package structure | One production library/CLI package and a development-only bounded fixture package |
| Rust toolchain | 1.99.0, with a committed Cargo lockfile |
| Linux GNU artifact | x86-64, Linux 5.15+, glibc 2.35+ |
| Linux musl artifact | x86-64, Linux 5.15+, no runtime dynamic-loader/shared-library dependencies |
| macOS artifact | Apple Silicon, macOS 15+, system libraries |
| Feature parity | Identical production feature set across all three artifacts |
| Deferred platforms/features | Windows, Linux ARM64, macOS Intel, HTTPS, daemon/watch/restart modes, remote execution, caching, plugins |
| Distribution | Versioned archives and SHA-256 checksums, gated on native and compatibility evidence |

The compatibility floors are release requirements. A local VPS build or cross-check
does not establish that those requirements have passed. Release publication waits
for the gates in [release.md](release.md).

## Invocation and configuration

The CLI has `run`, `exec`, `validate`, `plan`, and `doctor`. `run` and `plan` require
an explicit task/group selection. Direct execution uses `takt exec -- PROGRAM ARG...`;
repeated `--command` options use `/bin/sh -c` and are mutually exclusive with direct
argv. Arguments are never split on whitespace. Runner exits are 0 for success or
neutral skips, 1 for workload/output failure, 2 for invalid input, 3 for required
capability or impossible resource policy, 70 for internal errors, and 130/143 for
SIGINT/SIGTERM. Raw child exits remain in task outcomes.

Configuration is YAML/JSON `version: 1`. Discovery considers only `takt.yaml`,
`takt.yml`, and `takt.json` in the working directory, and rejects ambiguity.
Explicit `--file` and `TAKT_FILE` take precedence. Relative task directories resolve
against the configuration directory. Invocation precedence is CLI, `TAKT_*`, file,
then documented defaults/discovery; `CI=true` does not increase resource capacity.

Task resource/limit defaults merge by dimension. Environment defaults merge by key;
task `env` restores a default-unset key, task `unset_env` removes a default-set key,
and a key in both fields at the same level is rejected. The parent environment is
otherwise inherited. No tool worker flags or `NODE_OPTIONS` values are rewritten.
Environment values are not included in automatic diagnostics.

The parser rejects unknown/duplicate fields, unsupported versions, null values,
explicit YAML tags, and YAML merge keys. Optional fields must be omitted instead
of set to null. Aliases are accepted within bounds; includes, interpolation, and
implicit `.env` loading are absent. Limits are 1 MiB input, depth 64, 100,000 events
including expansion, 1,024 anchors/aliases, and 4 MiB retained YAML scalar/anchor
data. JSON duplicate checking streams values before typed deserialization instead
of retaining a second full value tree. CPU precision is checked before floating
point conversion and stored as integer millicores.

Environment normalization retains at most 100,000 environment/unset entries and
8 MiB of their names/values across defaults and merged tasks. These limits are
checked before copying inherited strings; overridden or removed defaults are not
copied. Exceeding them is a configuration error recommending smaller inherited
environment data or separate configuration files.

Planning has separate expansion budgets: at most 100,000 retained service-ancestor
entries, 8 MiB of copied ancestor names, and 1,000,000 ancestry traversal/union
steps per selected graph. Ancestry is computed in dependency order; finite-only
chains do not create quadratic ancestry storage. Exceeding a planning budget
returns an actionable policy error before any command starts. Select fewer tasks
or split a run when its service dependency expansion exceeds these bounds.

## Scheduling and lifecycle defaults

| Policy | Default |
|---|---|
| Failure | Continue independent work |
| Unexpected service failure | Block future consumers; allow active consumers to finish |
| Successful unmatched exit condition | Neutral skip, propagated to dependents |
| Task success codes | `[0]`, independent from dependency exit-code predicates |
| Inferred request | 1 CPU and 1 GiB; only inferred dimensions may shrink to a smaller positive total budget |
| Automatic job slots | At least one; normally floor of effective CPU capacity |
| Unknown memory headroom | Explicit warning and one automatic job slot |
| Memory reserve | `max(256 MiB, 15% of initial headroom)` |
| Eligibility ordering | FIFO by eligibility time, with lexical task ID ties |
| Fairness | Smaller fitting tasks may pass; after five seconds retain newly released capacity for the older task |
| Resource queue timeout | 120 seconds; dependency waiting does not consume this deadline |
| Execution timeout | `off` |
| Stop grace | Five seconds, then forced cleanup; second interruption escalates |
| Watchdog sampling | 200 ms |

Requests reserve capacity; limits request enforced ceilings. External OS ceilings
still bound invocation budgets. Live memory admission compares new requests and
unconsumed existing reservations against observed headroom plus the reserve,
without subtracting already-used memory twice. Unknown task usage conservatively
leaves its entire reservation outstanding.

Services consume slots and reservations until cleanup. Selected root services are
persistent; dependency services remain alive through all consumers and stop in
dependency order when unused. Feasibility checks include ancestor-service
reservations, and live service deadlocks are diagnosed. Normal finite exits become
available to exit predicates only after descendant cleanup completes.

## Protection, probes, and output

One Mio loop owns scheduling, capture, process state, signals, timers, and probes.
A controlled same-binary helper starts with a clean environment, waits for verified
containment, then applies the task environment/cwd and executes. Its handshake
distinguishes spawn failures from legitimate command exits. Process-group identity
is retained through cleanup. Linux uses owned delegated cgroup-v2 subtrees and
keeps the supervisor outside the aggregate workload memory cap. Requested bounds
are verified before project code in `required` mode. Unsupported dimensions are
reported separately; fallback monitoring never claims hard protection.

Readiness supports TCP connection, plain HTTP GET, validated UDP replies, and
streaming literal-byte stdio matches. HTTP accepts status 200 by default, follows
no redirects, and rejects HTTPS. Defaults are 30 seconds overall, 250 ms retry
interval, and one second per network attempt including DNS. Responses are bounded
to 64 KiB. At most two private resolver subprocesses keep system lookup cancellation
and deadlines under supervisor control.

Output supports auto/plain/grouped/NDJSON. A separate worker owns duplicated output
descriptors. Disposable logs have a 1 MiB queue; lifecycle records have an independent
8 MiB queue. Grouped spooling is bounded to 64 MiB per task. Readiness examines raw
captured bytes before rendering or truncation. Final output drainage is bounded to
one second; broken or incomplete delivery fails the invocation unless interruption
already determines exit 130/143. The event contract is
[`schema/events.schema.json`](../schema/events.schema.json).

## Dependency and measurement policy

Production dependencies are Serde/JSON, `serde-saphyr` 2.0.0 with deserialization
only, `granit-parser` 2.0.0 for tag/budget preflight, `lexopt`, Mio, `signal-hook`,
`ureq-proto` 0.6.4, `libc`, and Base64. Cargo.lock records exact resolutions.
The parser APIs were checked against installed source; HTTP uses the maintained
protocol parser with network I/O owned by the supervisor. Schema validation uses
development-only `jsonschema` 0.59.0 with external HTTP/file resolution disabled;
it is not linked into shipped executables. The API and disabled-resolution contract
are documented by the [crate maintainer](https://docs.rs/jsonschema/0.59.0/jsonschema/).

Executable size below 1,000,000 bytes, quiescent supervisor RSS below 5 MiB, and
idle CPU below 0.1% of a core are measurement goals, not release-size blockers.
After a reviewed baseline exists, unexplained growth above the larger of 10% or
100 KiB blocks packaging. Correctness, required protection, native execution, and
feature parity remain release gates regardless of size.
