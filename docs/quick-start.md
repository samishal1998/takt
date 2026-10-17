# Quick start

Takt runs commands when their CPU and memory requests fit the available budget.
Use it for parallel checks, ordered workflows, and services that must be ready
before tests start.

## Get Takt

Follow the [installation guide](installation.md), then check your installation:

```sh
takt --version
takt doctor
```

If you downloaded the project wrapper, use `./taktw` wherever this guide says
`takt`. The wrapper reuses an installed Takt or downloads and caches its pinned
release.

## Run a command

Run an executable directly, preserving each argument:

```sh
takt exec -- printf '%s\n' 'Hello from Takt'
```

Use `--command` for shell syntax. Each occurrence creates a separate task:

```sh
takt exec --jobs 2 \
  --command "printf '%s\n' 'First check'" \
  --command "printf '%s\n' 'Second check'"
```

Takt starts both when their resource requests fit. `--jobs 2` sets a maximum of
two active task trees; it does not force two tasks to run at once.

## Save a workflow

Create `takt.yaml` in your working directory. This example only needs the standard
`printf` command:

```yaml
version: 1
scheduling:
  jobs: 2
tasks:
  prepare:
    kind: task
    command: [printf, '%s\n', 'Prepared']
  check:
    kind: task
    command: [printf, '%s\n', 'Checks passed']
    depends_on:
      - task: prepare
        when: exit
        codes: [0]
  report:
    kind: task
    command: [printf, '%s\n', 'Report complete']
groups:
  checks: [check, report]
```

Validate the file and inspect what will run:

```sh
takt validate
takt plan checks
```

Both commands leave project commands unexecuted. The plan includes `prepare`
because `check` depends on it, and shows the effective CPU and memory budget.

Run the group:

```sh
takt run checks
```

`prepare` finishes before `check` starts. `report` can run independently when
capacity is available. A successful run exits with status `0`.

## Adapt it to your project

Replace the `command` arrays with your formatter, compiler, or test runner.
Give expensive tasks explicit [resource requests](configuration.md#requests-and-limits),
add dependencies for ordering, or use a
[service readiness probe](configuration.md#services-and-readiness) for integration
tests. Takt runs your existing tools; those tools keep their own dependencies.

Use `takt run checks --output json` for structured events and
`takt run checks --failure fail-fast` to stop work after an unaccepted failure.
See the [CLI reference](cli.md) for all options and exit statuses, or start from
the [example workflows](../examples/README.md).
