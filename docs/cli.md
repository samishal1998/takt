# CLI reference

Run `takt --help` for the installed version's help. Every command below also works
with `./taktw`; see [installation](installation.md) for wrapper-specific settings.

## Commands

| Command | Purpose |
| --- | --- |
| `takt run TASK_OR_GROUP...` | Run selected roots and their dependency closure. |
| `takt exec -- PROGRAM ARG...` | Run one executable with its arguments preserved. |
| `takt exec --command 'SHELL STRING'` | Run a shell task; repeat `--command` for multiple tasks. |
| `takt validate` | Validate configuration and dependency relationships. |
| `takt plan TASK_OR_GROUP...` | Show selected tasks, requests, effective budget, and capabilities. |
| `takt doctor` | Report host resources and available protection. |

`run` and `plan` require at least one task or group. `validate` and `doctor` do not
accept task selections. `validate`, `plan`, and `doctor` launch no project commands
or workload cgroups. `exec` and `doctor` do not require a configuration file.

### Run configured tasks

```sh
takt run checks
takt run lint test --jobs 2 --failure fail-fast
takt run checks --file ci/takt.yaml
takt plan checks --output json
```

Task and group names come from your configuration. Takt includes shared
dependencies once. It discovers configuration only in the working directory;
use `--file` or `TAKT_FILE` to select another path.

### Run commands directly

Put Takt options before `--`. Everything after `--` belongs to the executable:

```sh
takt exec --jobs 1 --output plain -- printf '%s\n' 'A single argument'
```

For multiple shell tasks, repeat `--command`:

```sh
takt exec --jobs 2 \
  --command "printf '%s\n' 'First task'" \
  --command "printf '%s\n' 'Second task'"
```

Shell strings execute with `/bin/sh -c` in the invocation directory. Direct
arguments and `--command` cannot be combined in one invocation. `exec` uses
invocation settings and automatic task requests; use a configuration file when
you need per-task requests, limits, dependencies, or readiness probes.

## Options and environment

Precedence is CLI option, corresponding environment variable, configuration, then
automatic default.

| Option | Environment | Values and meaning |
| --- | --- | --- |
| `--file PATH` | `TAKT_FILE` | YAML or JSON configuration path. |
| `--jobs N` | `TAKT_JOBS` | Positive number of active task-tree slots, or `auto`. |
| `--cpu N` | `TAKT_CPU` | Aggregate CPU equivalents, up to three decimal places, or `auto`. |
| `--memory SIZE` | `TAKT_MEMORY` | Aggregate memory budget, such as `2GiB`, or `auto`. |
| `--enforcement MODE` | `TAKT_ENFORCEMENT` | `auto`, `required`, or `off`. |
| `--output MODE` | `TAKT_OUTPUT` | `auto`, `plain`, `grouped`, or `json`. |
| `--failure POLICY` | — | `continue` or `fail-fast`. |
| `--queue-timeout TIME` | — | Capacity wait timeout, such as `120s`, or `off`. |
| `-h`, `--help` | — | Print usage. |
| `-V`, `--version` | — | Print the Takt version. |

For example:

```sh
TAKT_JOBS=2 TAKT_OUTPUT=plain takt run checks
takt run checks --memory 2GiB --queue-timeout 30s
```

The host's resource ceilings still constrain explicit budgets. `--enforcement
required` refuses to launch commands when requested protection cannot be
verified. See [configuration](configuration.md#requests-and-limits) for resource
units, defaults, and the distinction between requests and limits.

Per-task execution timeouts, `stop_grace`, `reserve_memory`, and `service_failure`
are configuration settings; they have no dedicated CLI flags in v0.1.

## Output

| Mode | Behavior |
| --- | --- |
| `auto` | Readable, prefixed logs for workload execution. |
| `plain` | Stream readable, prefixed task logs. |
| `grouped` | Buffer bounded task output and print it in task groups. |
| `json` | Write versioned newline-delimited JSON events to stdout. |

The inspection commands (`validate`, `plan`, `doctor`) print a readable JSON
document by default. With `--output json`, they emit the same NDJSON event format
used by workload commands.

```sh
takt run checks --output json > takt-events.ndjson
takt doctor --output json
```

Events include a schema version, run ID, increasing sequence number, elapsed time,
event name, optional task name, and event data. Workload runs finish with a summary
of roots, outcomes, counts, and exit status when output is deliverable. Non-UTF-8
log chunks use base64. The [event schema](../schema/events.schema.json) describes
the envelope, and the [runtime contract](runtime.md#parsing-and-output-bounds)
explains output bounds and failure behavior.

Automatic diagnostics omit environment values. Commands can still print secrets
in their own output, so treat captured logs accordingly.

## Exit status and cancellation

| Status | Meaning |
| --- | --- |
| `0` | Success, including neutral branch skips. |
| `1` | Workload or output failure. |
| `2` | Invalid input or configuration. |
| `3` | Impossible resource policy or required capability failure. |
| `70` | Internal runner error. |
| `130` | Cancelled by SIGINT, usually Ctrl-C. |
| `143` | Cancelled by SIGTERM. |

Takt reports the workflow outcome, so a child's exit code is not passed through
unchanged. Use task `success_codes` to accept additional normal exit codes and
dependency `codes` to select exit-code branches.

The default `continue` policy lets independent checks finish after a failure.
`--failure fail-fast` stops pending and active work after an unaccepted failure.
Cancellation stops admission, signals owned process groups, waits for the stop
grace period, and then forces cleanup. A second interrupt escalates immediately.
