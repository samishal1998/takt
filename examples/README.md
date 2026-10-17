# Takt configuration examples

These V1 configurations are checked against Takt's parser and graph validator. Project scripts and service executables are illustrative; replace them with real commands from your repository. The branching example runs as supplied.

| File | Demonstrates |
|---|---|
| [01-checks.yaml](01-checks.yaml) / [01-checks.json](01-checks.json) | Equivalent automatic-budget finite check groups |
| [02-ordered.yaml](02-ordered.yaml) | Exit-code dependencies impose order regardless of available slots |
| [03-exit-code-branches.yaml](03-exit-code-branches.yaml) | Expected nonzero detector outcome and neutral branch skip |
| [04-service-backed-tests.yaml](04-service-backed-tests.yaml) | HTTP and stdio readiness; finite roots stop owned services; dev roots persist |
| [05-tcp-udp-probes.yaml](05-tcp-udp-probes.yaml) | TCP connection and UDP payload/reply contracts |
| [06-protected-linux-ci.yaml](06-protected-linux-ci.yaml) | Explicit per-task limits plus required verified aggregate protection |

The examples intentionally include explicit task requests. A request larger than a machine's effective budget is a configuration/policy error; the runner must not silently shrink it. Reduce declared requests knowingly or use appropriate automatic defaults. For example, the two-CPU test in `01-checks.yaml` cannot run under a one-CPU total budget as written.

Run `takt run --file examples/03-exit-code-branches.yaml branch-demo` to see exit-code branching: the detector returns 2 and declares that result successful; the `changed` branch runs and `unchanged` skips. Dependency predicates determine which branch runs.

The HTTP example expects a body containing the literal string `ready`; adapt it to your health endpoint. TCP readiness cannot prove a pre-existing listener is your service. UDP services must actually implement the stated `PING`/`PONG` exchange; this is not a generic readiness protocol that every UDP application supports.

## Data conventions

Task/group IDs begin with a letter and use letters, numbers, underscores, or hyphens. Memory values are positive integer quantities with `B`, `KiB`, `MiB`, `GiB`, or `TiB`. CPU is a positive number in CPU equivalents with up to three fractional digits. Durations use positive integer `ms`, `s`, `m`, or `h` values. Invocation jobs/CPU/memory can be `auto`; execution and resource-queue timeouts can be `off`. Task commands use argv arrays or an explicit shell string, never both.

Schema validation is structural. Takt additionally checks duplicate keys, graph references and cycles, success/edge semantics, numeric overflow, URL/address validity, service feasibility, and platform protection capabilities.
