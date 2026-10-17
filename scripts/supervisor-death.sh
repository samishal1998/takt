#!/usr/bin/env bash
# Dedicated external-boundary proof; never run outside disposable GitHub CI.
set -euo pipefail
[[ "${GITHUB_ACTIONS:-}" == true ]]
binary=${1:?Takt executable}
fixture=${2:?fixture executable}
unit="takt-death-${GITHUB_RUN_ID:?}-${GITHUB_RUN_ATTEMPT:?}-${3:?lane}"
work=$(mktemp -d "${RUNNER_TEMP:?}/takt-death.XXXXXXXX")
trap 'sudo systemctl stop "$unit" >/dev/null 2>&1 || true; rm -rf "$work"' EXIT
mkdir -p dist/safety
cat > "$work/takt.json" <<EOF
{"version":1,"output":"json","enforcement":"required","scheduling":{"cpu":1,"jobs":1,"memory":"128MiB","reserve_memory":"1MiB"},"tasks":{"tree":{"kind":"task","command":["$fixture","tree","2","10000","ignore"],"resources":{"cpu":0.01,"memory":"1MiB"},"limits":{"memory":"64MiB"}}}}
EOF
cat > "$work/start.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
relative=$(awk -F: '$1 == "0" {print $3}' /proc/self/cgroup)
boundary="/sys/fs/cgroup$relative"
[[ $(cat "$boundary/memory.max") == 536870912 ]]
[[ $(cat "$boundary/pids.max") == 32 ]]
[[ $(cat "$boundary/memory.swap.max") == 0 ]]
[[ $(systemctl show --value --property=RuntimeMaxUSec "$3") == 20s ]]
[[ $(systemctl show --value --property=KillMode "$3") == control-group ]]
[[ $(systemctl show --value --property=SendSIGKILL "$3") == yes ]]
systemctl show "$3" --property=ControlGroup --property=MemoryMax \
  --property=MemorySwapMax --property=TasksMax --property=RuntimeMaxUSec \
  --property=Delegate --property=KillMode --property=SendSIGKILL \
  --property=TimeoutStopUSec > "$4"
mkdir "$boundary/supervisor"
printf '%s\n' "$$" > "$boundary/supervisor/cgroup.procs"
printf '+cpu +memory +pids\n' > "$boundary/cgroup.subtree_control"
exec "$1" run tree --file "$2"
EOF
sudo systemd-run --unit="$unit" --wait --pipe --collect \
  --uid="$(id -u)" --gid="$(id -g)" \
  --property=Delegate=yes --property=MemoryMax=512M --property=MemorySwapMax=0 \
  --property=TasksMax=32 --property=RuntimeMaxSec=20s --property=TimeoutStopSec=1s \
  --property=KillMode=control-group /bin/bash "$work/start.sh" "$binary" "$work/takt.json" \
  "$unit" "$PWD/dist/safety/supervisor-death-boundary.txt" \
  > dist/safety/supervisor-death.ndjson 2> dist/safety/supervisor-death.stderr &
launcher=$!
ready=0
for attempt in {1..500}; do
  if grep -q 'CHILD ' dist/safety/supervisor-death.ndjson; then ready=1; break; fi
  sleep 0.01
done
[[ "$ready" == 1 ]]
group=$(sudo systemctl show --property=ControlGroup --value "$unit")
[[ -n "$group" && "$group" == *"$unit"* ]]
sudo systemctl kill --kill-who=main --signal=SIGKILL "$unit"
set +e
wait "$launcher"
status=$?
set -e
[[ "$status" != 0 ]]
for attempt in {1..200}; do
  if [[ ! -d "/sys/fs/cgroup$group" ]]; then
    printf 'External systemd boundary removed all workload cgroups after supervisor SIGKILL.\n' > dist/safety/supervisor-death.txt
    exit 0
  fi
  sleep 0.01
done
echo 'External boundary left an owned cgroup after supervisor death' >&2
exit 1
