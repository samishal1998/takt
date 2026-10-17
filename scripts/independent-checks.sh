#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
target=${1:?usage: independent-checks.sh RUST_TARGET}
export CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-2}
export RUST_TEST_THREADS=1
python3 -B scripts/test_artifact_abi.py
cargo fmt --all --check
cargo clippy --workspace --all-targets --target "$target" --locked -- -D warnings
cargo build --locked --target "$target" -p takt-test-fixture
export TAKT_TEST_FIXTURE="$PWD/target/$target/debug/takt-test-fixture"
cargo test --workspace --target "$target" --locked -- --test-threads=1
if [[ "$target" == x86_64-unknown-linux-gnu ]]; then
  cargo test --locked --target "$target" --test process_lifecycle -- \
    --ignored --exact task_loader_environment_is_applied_only_after_launcher_handshake
fi
cargo build --release --target "$target" --locked -p takt
for config in examples/*.yaml examples/*.json; do
  "target/$target/release/takt" validate --file "$config"
done
