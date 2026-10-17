#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
target=${1:?usage: package.sh RUST_TARGET}
case "$target" in
  x86_64-unknown-linux-gnu|x86_64-unknown-linux-musl|aarch64-apple-darwin) ;;
  *) echo "unsupported release target: $target" >&2; exit 2 ;;
esac
binary="$PWD/target/$target/release/takt"
version=$("$binary" --version | awk '{print $2}')
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+([-+][a-zA-Z0-9.-]+)?$ ]]
name="takt-$version-$target"
stage=$(mktemp -d "${TMPDIR:-/tmp}/takt-package.XXXXXXXX")
trap 'rm -rf "$stage"' EXIT
mkdir -p "$stage/$name" dist
cp "$binary" LICENSE-MIT LICENSE-APACHE README.md "$stage/$name/"
cp -R docs examples schema "$stage/$name/"
mkdir -p "$stage/$name/brand/banners"
cp brand/banners/takt-docs-banner-dark.png "$stage/$name/brand/banners/"
tar -czf "dist/$name.tar.gz" -C "$stage" "$name"
if command -v sha256sum >/dev/null 2>&1; then
  (cd dist && sha256sum "$name.tar.gz" > "$name.tar.gz.sha256")
else
  (cd dist && shasum -a 256 "$name.tar.gz" > "$name.tar.gz.sha256")
fi
mkdir "$stage/extracted"
tar -xzf "dist/$name.tar.gz" -C "$stage/extracted"
extracted="$stage/extracted/$name/takt"
"$extracted" --version
"$extracted" doctor --output json > "dist/$target-doctor.ndjson"
export TAKT_TEST_BINARY="$extracted"
export TAKT_TEST_FIXTURE="$PWD/target/$target/debug/takt-test-fixture"
cargo test --locked --target "$target" --test integration --test process_lifecycle --test probes_output -- --test-threads=1
python3 scripts/measure-artifact.py "$target" "$extracted" "dist/$name.tar.gz"
cargo build --locked --release --target "$target" -p takt-test-fixture
python3 scripts/measure-performance.py "$extracted" \
  "$PWD/target/$target/release/takt-test-fixture" "dist/performance-$target"
cp "dist/performance-$target/measurement.json" "dist/$target-performance.json"
# Fixture travels only as CI evidence; it is never put in a release archive.
cp "$TAKT_TEST_FIXTURE" "dist/fixture-$target"
