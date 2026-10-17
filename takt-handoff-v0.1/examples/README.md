# Takt configuration examples

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

## Draft data conventions

Task/group IDs begin with a letter and use letters, numbers, underscores, or hyphens. Memory values are positive integer quantities with `B`, `KiB`, `MiB`, `GiB`, or `TiB`. CPU is a positive number in CPU equivalents with up to three fractional digits. Durations use positive integer `ms`, `s`, `m`, or `h` values. Invocation jobs/CPU/memory can be `auto`; resource-queue timeout can be `off`. Task commands use argv arrays or an explicit shell string, never both.

Schema validation is structural only. The application still needs duplicate-key rejection during parsing, graph reference/cycle checks, success/edge semantics, numeric overflow checks, valid URL/address handling, live service feasibility, and platform protection checks.
