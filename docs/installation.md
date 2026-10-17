# Install Takt or use the project wrapper

Install the current release into `~/.local/bin`:

```sh
curl -fsSL https://github.com/samishal1998/takt/releases/latest/download/install.sh | sh
```

To choose another directory, pass `--bin-dir`:

```sh
curl -fsSL https://github.com/samishal1998/takt/releases/latest/download/install.sh |
  sh -s -- --bin-dir "$HOME/bin"
```

`TAKT_INSTALL_DIR` also sets the destination. The installer prints PATH guidance;
it does not edit shell profiles or require sudo. It replaces an existing regular
`takt` file atomically, but refuses a directory or symlink at the destination.
This avoids replacing a symlink managed by a package manager. Run `takt --version`
and `takt doctor` after adding the destination to your PATH.

To inspect the installer before executing it, download `install.sh` with
`curl -fSL ... -o install.sh`, read it, and run `sh install.sh`. The release also
offers archives and checksums for [manual installation](release.md#install-a-published-archive).

## Project wrapper

Download the standalone wrapper and commit it with a project:

```sh
curl -fsSL https://github.com/samishal1998/takt/releases/latest/download/taktw -o taktw
chmod +x taktw
./taktw run checks
```

It works from any working directory, preserves arguments and stdin, and replaces
itself with Takt so exit status and signals reach the application normally. It
does not require a checkout of this repository, Rust, Python, or Bedouin.

The wrapper uses its pinned release by default, downloading and verifying it
once before reusing the cached executable. The checked-in wrapper pins `0.1.0`.
To opt into reusing an installed Takt from PATH:

```sh
TAKT_USE_SYSTEM=1 ./taktw run checks
```

With this opt-in, an executable that reports a Takt version takes precedence,
including a different release version. Copies and symlinks of the wrapper named
`takt` are skipped to avoid recursion. If no installation is available, the
wrapper falls back to its pinned release. Set `TAKT_USE_SYSTEM=0` to override an
environment that enables system reuse and restore the pinned default.

The binary cache is `${XDG_CACHE_HOME:-$HOME/.cache}/takt/VERSION/TARGET/takt`.
Set `TAKT_CACHE_DIR` to change the cache root. Both scripts use this cache; warm
runs require no network. Downloads are verified against the SHA-256 embedded in
the script before extraction. Interrupted or invalid downloads never replace a
working cache entry. Concurrent first runs can download independently and publish
only complete executables.

Fetch a new wrapper to update its bundled version. For reproducible projects,
commit a reviewed wrapper and keep the default pinned-release behavior. To
install a particular release, replace `releases/latest/download` in the URLs with
`releases/download/vVERSION`; installer assets are available starting with v0.1.0.

## Platforms and requirements

| Host | Default release |
| --- | --- |
| Linux x86-64, kernel 5.15+ | `x86_64-unknown-linux-musl` (static) |
| macOS Apple Silicon, macOS 15+ | `aarch64-apple-darwin` |

Linux downloads use musl by default on both glibc and musl distributions. For the
GNU build on a glibc 2.35+ host, set `TAKT_TARGET=x86_64-unknown-linux-gnu`. If your
environment enables system reuse, also set `TAKT_USE_SYSTEM=0` so the wrapper
uses the selected release target. When selecting a download, unsupported host
architectures and incompatible target overrides fail with a diagnostic. Intel
macOS, Linux ARM, and native Windows releases are not yet available.

Cold downloads require `curl`, `tar`, and either `sha256sum` or `shasum`, alongside
standard Unix utilities. The scripts use POSIX shell syntax and also run under
macOS's Bash 3.2. The musl executable still uses the host's resolver configuration.

## Maintaining the scripts

The initial download and checksum selection was bootstrapped with
`bedouin install --generate samishal1998/takt@v0.1.0`. The repository maintains the
shared shell template and release metadata; Bedouin is not a runtime dependency.

```sh
python3 scripts/generate-bootstrap.py
python3 scripts/generate-bootstrap.py --check
python3 -B scripts/test_bootstrap.py
```

The release workflow generates installer and wrapper assets from the new
release's validated archive checksums before publication. Update
`scripts/bootstrap-release.json` and regenerate to advance the repository's
checked-in wrapper pin. Tests exercise installation, PATH detection, offline
cache reuse, platform selection, checksum rejection, and concurrent startup on
Linux and native macOS.
