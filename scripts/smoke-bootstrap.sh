#!/usr/bin/env bash
# Exercise the public release archives on each native runner, in isolated paths.
set -euo pipefail
cd "$(dirname "$0")/.."
target=${1:?usage: smoke-bootstrap.sh RUST_TARGET}
stage=$(mktemp -d "${TMPDIR:-/tmp}/takt-bootstrap-smoke.XXXXXXXX")
trap 'rm -rf "$stage"' EXIT
export TAKT_TARGET="$target" TAKT_CACHE_DIR="$stage/cache"
sh install.sh --bin-dir "$stage/installed bin"
"$stage/installed bin/takt" --version
"$stage/installed bin/takt" doctor

# Any attempt to download now fails. The warm cache and installed PATH must work.
mkdir "$stage/offline"
cat > "$stage/offline/curl" <<'SH'
#!/bin/sh
echo 'Unexpected network access after installation' >&2
exit 99
SH
chmod +x "$stage/offline/curl"
expected=$("$stage/installed bin/takt" --version)
actual=$(PATH="$stage/offline:$PATH" TAKT_USE_SYSTEM=0 sh taktw --version)
test "$actual" = "$expected"
actual=$(PATH="$stage/installed bin:$stage/offline:$PATH" TAKT_USE_SYSTEM=1 \
  TAKT_CACHE_DIR="$stage/unused-cache" sh taktw --version)
test "$actual" = "$expected"
test ! -e "$stage/unused-cache"
echo "Verified installation, offline cache, and PATH reuse for $target"
