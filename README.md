# Takt

![Takt — Schedule. Balance. Flow.](brand/banners/takt-docs-banner-dark.png)

**Schedule. Balance. Flow.**

[Documentation](https://samishal1998.github.io/takt/) ·
[Quick start](docs/quick-start.md) ·
[Releases](https://github.com/samishal1998/takt/releases)

Takt runs project commands together when CPU and memory budgets allow, queues
them when capacity is occupied, and supervises their process trees. It supports
finite checks, ordered workflows, exit-code branches, and services needed by tests.

```sh
takt exec --command 'cargo fmt --check' --command 'cargo test' --jobs 2
takt run checks
takt plan checks --output json
takt doctor
```

## Build and install

Install the latest release into `~/.local/bin`:

```sh
curl -fsSL https://github.com/samishal1998/takt/releases/latest/download/install.sh | sh
```

Or use the standalone wrapper in a project:

```sh
curl -fsSL https://github.com/samishal1998/takt/releases/latest/download/taktw -o taktw
chmod +x taktw
./taktw --version
./taktw run checks
```

The wrapper reuses Takt from your `PATH` when available. Otherwise it downloads
the pinned release once, verifies its SHA-256, and reuses the cached executable
on subsequent runs, including offline. Both scripts work with `sh` or Bash and
select Linux x86-64 musl or macOS Apple Silicon automatically. See
[installation and wrapper options](docs/installation.md) for custom directories,
version pinning, and GNU builds.

Build from source with Rust 1.99.0:

```sh
cargo build --release --locked
./target/release/takt --help
cargo install --path . --locked
```

Versioned releases contain one executable, examples, schemas, and documentation.
Download the archive and its `.sha256` file from [GitHub Releases](https://github.com/samishal1998/takt/releases),
verify with `sha256sum -c ARCHIVE.tar.gz.sha256` (or `shasum -a 256 -c` on macOS),
extract it, and place `takt` in your PATH.

The supported release targets are Linux x86-64 GNU (kernel 5.15+, glibc 2.35+),
Linux x86-64 musl (kernel 5.15+, static executable), and macOS Apple Silicon
(macOS 15+). Native and compatibility gates must pass before a release is published.
Musl still needs host resolver configuration and can resolve names differently
from glibc. There is no Takt daemon or Node runtime dependency; launched commands
retain their own dependencies.

## Configure a workflow

Create `takt.yaml`, `takt.yml`, or `takt.json` in the working directory:

```yaml
version: 1
scheduling:
  jobs: auto
  cpu: auto
  memory: auto
tasks:
  format:
    kind: task
    command: [cargo, fmt, --check]
  test:
    kind: task
    command: [cargo, test]
groups:
  checks: [format, test]
```

Use argument arrays for direct execution, or `shell: 'command | command'` for
explicit `/bin/sh -c` execution. Direct CLI execution preserves arguments exactly:
`takt exec -- PROGRAM ARG...`. Multiple `--command` values each define a shell task.
Task working directories are relative to the configuration file. Environment
variables are inherited, then overridden by `env` and removed by `unset_env`.

Invocation policy resolves from CLI, then `TAKT_*` environment, configuration,
and automatic defaults. File discovery stays in the current directory and rejects
ambiguous candidates. `validate` and `plan` launch no project commands or cgroups.

See [configuration examples](examples/README.md), the [configuration schema](schema/takt.schema.json),
and the [runtime contract](docs/runtime.md).

## Resources and protection

Requests reserve scheduling capacity. Limits ask for enforcement. An estimated
request never silently becomes a per-task hard limit. Explicit requests that do
not fit the effective budget are rejected; only inferred defaults can shrink.

`--enforcement auto` applies available delegated Linux controllers and reports
fallbacks. `required` refuses before executing project commands when requested
protection cannot be verified. `off` retains admission scheduling while disabling
Takt-applied controllers and watchdog termination.

Native macOS and undelegated Linux provide admission and best-effort sampled memory
monitoring. Process groups cannot contain detached descendants; sampled accounting
can miss peaks and count shared pages more than once. Takt is for trusted project
commands. External supervision is responsible for cleanup if Takt itself is killed
with SIGKILL. See [testing and protection evidence](docs/testing.md).

## Output and exit status

Choose `--output plain`, `grouped`, or `json`; automatic output uses readable
prefixed logs. JSON output is versioned NDJSON with task events and a final summary.
Raw non-UTF-8 log chunks use base64. Environment values are omitted from automatic
diagnostics, but child output can contain secrets.

The default failure policy continues independent checks. `--failure fail-fast`
stops pending and active work after an unaccepted failure. Exit status is 0 for
success/neutral skips, 1 for workload or output failure, 2 for invalid input,
3 for impossible resource policy/required capability failure, 70 for an internal
runner error, 130 for SIGINT, or 143 for SIGTERM.

Output buffering and grouped storage are bounded. A permanently blocked output
consumer cannot receive a guaranteed final summary; Takt cleans up workloads and
returns failure instead of waiting indefinitely.

## Development

```sh
cargo build --workspace --locked
cargo test --workspace --locked -- --test-threads=1
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
```

The fixture executable is development-only and is excluded from release archives.
The independent Cargo harness remains the source of correctness evidence even
when Takt schedules repository checks. See [release gates](docs/release.md).

The Astro Starlight documentation site lives in [`website/`](website/README.md)
and deploys to GitHub Pages from this repository. It builds from the canonical
Markdown guides in `docs/` and `examples/README.md`.

The original planning package remains under `takt-handoff-v0.1/` for provenance.
Software is licensed under either [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE),
at your option.
