#!/usr/bin/env bash
set -euo pipefail
trap 'status=$?; printf "Compatibility smoke failed at line %s (exit %s)\n" "$LINENO" "$status" >&2' ERR
[[ $(uname -r) == 5.15.* ]]
[[ $(getconf GNU_LIBC_VERSION) == 'glibc 2.35' ]]
uname -a
getconf GNU_LIBC_VERSION
mkdir -p /tmp/takt-compatibility
cd /mnt/takt
sha256sum -c ./*.sha256
for target in x86_64-unknown-linux-gnu x86_64-unknown-linux-musl; do
  archive=(takt-*-$target.tar.gz)
  [[ ${#archive[@]} == 1 ]]
  tar -xzf "${archive[0]}" -C /tmp/takt-compatibility
  binary="/tmp/takt-compatibility/${archive[0]%.tar.gz}/takt"
  fixture="/mnt/takt/fixture-$target"
  "$binary" --version
  "$binary" doctor --output json
  "$binary" exec --output json --enforcement off -- "$fixture" args 'two words' '' '$(literal)'
  "$binary" exec --output json --enforcement off -- "$fixture" output 131072 invalid > /tmp/takt-compatibility/output.ndjson
  cat > /tmp/takt-compatibility/takt.json <<EOF
{"version":1,"enforcement":"off","output":"json","scheduling":{"jobs":2,"cpu":1,"memory":"128MiB","reserve_memory":"1MiB"},"defaults":{"resources":{"cpu":0.01,"memory":"1MiB"},"stop_grace":"50ms"},"tasks":{"service":{"kind":"service","command":["$fixture","startup","1000"],"readiness":{"type":"stdio","stream":"stdout","contains":"READY","timeout":"500ms"}},"consumer":{"kind":"task","command":["$fixture","exit","0"],"depends_on":[{"task":"service","when":"ready"}]}}}
EOF
  "$binary" validate --file /tmp/takt-compatibility/takt.json
  "$binary" run consumer --file /tmp/takt-compatibility/takt.json
  # Each libc variant resolves localhost through its own resolver path.
  server_output="/tmp/takt-compatibility/server-$target.txt"
  # Create an empty handshake file before launching; a previous target's data
  # must never satisfy this target's readiness check.
  : > "$server_output"
  "$fixture" tcp 5000 > "$server_output" &
  server_pid=$!
  trap 'kill "$server_pid" 2>/dev/null || true' EXIT
  for attempt in {1..100}; do
    if [[ -s "$server_output" ]]; then break; fi
    sleep 0.01
  done
  read -r ready address < "$server_output"
  [[ "$ready" == READY ]]
  port=${address##*:}
  cat > /tmp/takt-compatibility/probe.json <<EOF
{"version":1,"enforcement":"off","output":"json","scheduling":{"jobs":2,"cpu":1,"memory":"128MiB","reserve_memory":"1MiB"},"defaults":{"resources":{"cpu":0.01,"memory":"1MiB"},"stop_grace":"50ms"},"tasks":{"service":{"kind":"service","command":["$fixture","sleep","1000"],"readiness":{"type":"tcp","host":"localhost","port":$port,"timeout":"500ms"}},"consumer":{"kind":"task","command":["$fixture","exit","0"],"depends_on":[{"task":"service","when":"ready"}]}}}
EOF
  "$binary" run consumer --file /tmp/takt-compatibility/probe.json
  kill "$server_pid"
  wait "$server_pid" || true
  trap - EXIT
done
