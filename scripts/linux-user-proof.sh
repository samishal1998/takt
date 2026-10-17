#!/usr/bin/env bash
# Entry point inside an explicitly bounded temporary systemd user unit.
# The caller supplies absolute paths: takt, fixture, process test binary, evidence.
set -euo pipefail
[[ "${TAKT_ISOLATED_SAFETY:-}" == 1 ]]
[[ $# == 4 ]]
[[ $(uname -s) == Linux ]]
binary=$1
fixture=$2
process_tests=$3
evidence=$4
unit=${TAKT_SAFETY_UNIT:?}
relative=$(awk -F: '$1 == "0" {print $3}' /proc/self/cgroup)
boundary="/sys/fs/cgroup$relative"
[[ $(cat "$boundary/memory.max") == 536870912 ]]
[[ $(cat "$boundary/memory.swap.max") == 0 ]]
[[ $(cat "$boundary/pids.max") == 32 ]]
[[ $(systemctl --user show --value --property=RuntimeMaxUSec "$unit") == 20s ]]
[[ $(systemctl --user show --value --property=KillMode "$unit") == control-group ]]
[[ $(systemctl --user show --value --property=SendSIGKILL "$unit") == yes ]]
mkdir -p "$evidence"
systemctl --user show "$unit" --property=ControlGroup --property=MemoryMax \
    --property=MemorySwapMax --property=TasksMax --property=RuntimeMaxUSec \
    --property=Delegate --property=KillMode --property=SendSIGKILL \
    --property=TimeoutStopUSec > "$evidence/outer-boundary.txt"
# Every write below targets this transient unit, whose limits were read back.
mkdir "$boundary/supervisor"
printf '%s\n' "$$" > "$boundary/supervisor/cgroup.procs"
printf '+cpu +memory +pids\n' > "$boundary/cgroup.subtree_control"
export TAKT_SAFETY_BOUNDARY="$boundary"
export TAKT_TEST_BINARY="$binary"
export TAKT_TEST_FIXTURE="$fixture"
"$process_tests" --ignored --exact task_loader_constructor_runs_inside_required_containment \
    --nocapture > "$evidence/loader-constructor.txt" 2>&1
python3 "$(dirname "$0")/linux-safety.py" "$binary" "$fixture" "$evidence"
# The service manager removes its own supervisor directory after this shell exits.
for owned_run in "$boundary"/takt-*; do
    [[ ! -d "$owned_run" ]]
done
