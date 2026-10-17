# Configuration

Takt reads `takt.yaml`, `takt.yml`, or `takt.json` from the current directory.
Keep one discovery candidate per directory, or select a file with
`takt run checks --file path/to/takt.yaml`. Discovery does not search parent
directories.

Every file declares `version: 1` and at least one task. The
[JSON Schema](../schema/takt.schema.json) describes the fields; `takt validate`
also checks names, dependencies, units, and other semantic rules. Use
`takt plan TASK_OR_GROUP` to check resource feasibility before running commands.

## Tasks and groups

A task has a name, `kind: task` or `kind: service`, and exactly one execution form:

- `command: [PROGRAM, ARG, ...]` executes an argument array directly.
- `shell: 'COMMAND'` runs the string through `/bin/sh -c`.

Task and group names start with a letter and may contain letters, digits,
underscores, and hyphens. Names must be distinct across tasks and groups. Groups
contain task names, and selecting several tasks or groups runs each included task
once, together with its dependencies.

`cwd` is relative to the configuration file unless it is an absolute path; its
default is the configuration file's directory. Tasks inherit the invocation
environment. Use `env` for string-valued overrides and `unset_env` to remove
variables. Takt does not load `.env` files or expand variables in configuration
values. Shell commands can perform shell expansion explicitly.

## Requests and limits

`scheduling` sets the aggregate budget. A task's `resources` reserves part of that
budget for admission; its `limits` requests runtime enforcement. These are
independent settings. An inferred scheduling request never becomes an implicit
hard limit.

This complete example gives two small checks explicit requests and limits:

```yaml
version: 1
scheduling:
  jobs: 2
  cpu: auto
  memory: auto
  queue_timeout: 120s
enforcement: auto
failure: continue
defaults:
  resources:
    cpu: 0.25
    memory: 16MiB
  limits:
    cpu: 0.5
    memory: 128MiB
  timeout: 1m
  stop_grace: 5s
tasks:
  format:
    kind: task
    command: [printf, '%s\n', 'Format check']
  test:
    kind: task
    command: [printf, '%s\n', 'Test check']
groups:
  checks: [format, test]
```

`defaults` also accepts `env` and `unset_env`. Task settings override inherited
defaults; resource dimensions inherit independently. Requests declared in
`defaults` count as explicit requests.

| Value | Accepted form | Example |
| --- | --- | --- |
| Jobs | Positive integer, or `auto` in `scheduling` | `2` |
| CPU | Positive CPU equivalents, at most three decimal places | `0.5`, `2` |
| Memory | Positive integer followed by `B`, `KiB`, `MiB`, `GiB`, or `TiB` | `512MiB` |
| Duration | Positive integer followed by `ms`, `s`, `m`, or `h` | `250ms`, `5m` |

Aggregate CPU and memory also accept `auto`. Host ceilings bound the effective
budget even when you provide explicit values. Automatic memory reserve is the
larger of 256 MiB or 15% of initial available headroom; `scheduling.reserve_memory`
sets an explicit reserve. Inferred task requests begin at 1 CPU and 1 GiB and may
shrink to fit total capacity. Explicit requests that cannot fit are errors.

`scheduling.queue_timeout` defaults to `120s` and counts waiting for capacity,
excluding ordinary dependency waiting. Task `timeout` starts after execution is
confirmed and defaults to `off`. Both settings accept `off`. `stop_grace` defaults
to `5s` and controls the time allowed for graceful shutdown before forced cleanup.

### Enforcement modes

| `enforcement` | Behavior |
| --- | --- |
| `auto` | Apply available protection and report capability gaps and fallbacks. |
| `required` | Refuse to launch project commands if requested protection cannot be verified. |
| `off` | Keep admission scheduling; disable Takt-applied controllers and watchdog termination. |

Linux hard protection requires an already delegated cgroup-v2 hierarchy. Native
macOS and undelegated Linux provide admission and best-effort sampled memory
monitoring. Run `takt doctor` to inspect the current host, and see the
[runtime contract](runtime.md#lifecycle-and-enforcement) for protection boundaries.

## Dependencies and exit-code branches

An exit dependency waits for a finite task to exit normally with one of the listed
`codes`. Every dependency on a task must match before it can start.
`success_codes`, which defaults to `[0]`, separately determines whether the
prerequisite's exit counts as successful.

For example, treat exit code `2` as a successful detection result and choose the
matching branch:

```yaml
version: 1
tasks:
  detect:
    kind: task
    shell: exit 2
    success_codes: [0, 2]
  unchanged:
    kind: task
    command: [printf, '%s\n', 'No changes']
    depends_on:
      - task: detect
        when: exit
        codes: [0]
  changed:
    kind: task
    command: [printf, '%s\n', 'Run checks for changes']
    depends_on:
      - task: detect
        when: exit
        codes: [2]
groups:
  checks: [unchanged, changed]
```

Running `takt run checks` executes `changed` and skips `unchanged` neutrally.
Without `success_codes: [0, 2]`, the detection task would fail the invocation even
though the `changed` branch matches and can run under `failure: continue`.

The default `failure: continue` lets independent work finish after a failure.
`failure: fail-fast` stops pending and active work after an unaccepted failure.
See [exit-code branches](../examples/03-exit-code-branches.yaml) for a downloadable
example.

## Services and readiness

A service reserves capacity for its whole lifetime. Consumers use `when: ready`
to wait for its probe. This runnable demonstration emits a readiness message,
keeps the service alive, and starts a dependent check:

```yaml
version: 1
scheduling:
  jobs: 2
defaults:
  resources:
    cpu: 0.1
    memory: 8MiB
  stop_grace: 1s
tasks:
  worker:
    kind: service
    shell: "printf '%s\\n' 'WORKER_READY'; exec sleep 30"
    readiness:
      type: stdio
      stream: stdout
      contains: WORKER_READY
      timeout: 5s
  check:
    kind: task
    command: [printf, '%s\n', 'Worker is ready']
    depends_on:
      - task: worker
        when: ready
```

Run `takt run check`. Takt starts `worker`, waits for `WORKER_READY`, runs `check`,
and stops the dependency service when its consumers finish. Selecting `worker`
directly keeps it running until cancellation or service exit.

| Probe `type` | Required fields | What readiness means |
| --- | --- | --- |
| `stdio` | `stream`, `contains` | Literal text appeared on `stdout`, `stderr`, or `either`. |
| `tcp` | `host`, `port` | A TCP connection succeeded; this does not identify the service. |
| `http` | `url` | A plain HTTP GET returned an accepted status, optionally matching `body_contains`. |
| `udp` | `host`, `port`, `payload`, `expect` | A reply from the configured peer matched exact bytes or a prefix. |

HTTP defaults to status `200`; set `status_codes` to change it. HTTP probes do not
follow redirects or support HTTPS. UDP `payload` uses `{text: ...}` or `{hex: ...}`;
`expect` wraps that byte form under `equals` or `prefix`.

Readiness defaults to a `30s` overall `timeout`. Network probes additionally accept
`interval` (default `250ms`) and `attempt_timeout` (default `1s`, including DNS).
See [service-backed tests](../examples/04-service-backed-tests.yaml) and
[TCP/UDP probes](../examples/05-tcp-udp-probes.yaml) for fuller examples.

An unexpected service exit fails the run and blocks future consumers. Active
consumers continue by default; set `service_failure: stop-dependents` to stop them.
Reserve enough jobs, CPU, and memory for services and their consumers together.

## Validation and policy precedence

CLI settings override the corresponding `TAKT_*` environment variables, then
configuration values, then automatic defaults. See the [CLI reference](cli.md)
for supported environment variables.

Unknown fields, duplicate keys, explicit YAML tags, dependency cycles, and
ambiguous discovery files are rejected. Configuration input is limited to 1 MiB;
the [runtime contract](runtime.md#parsing-and-output-bounds) documents additional
parser, graph, and output bounds.
