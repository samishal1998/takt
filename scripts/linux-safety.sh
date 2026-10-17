#!/usr/bin/env bash
# Run only inside the disposable systemd unit established by CI.
set -euo pipefail
[[ "${TAKT_ISOLATED_SAFETY:-}" == 1 ]] || { echo 'requires explicit isolated CI safety environment' >&2; exit 2; }
[[ $(uname -s) == Linux ]]
cgroup_relative=$(awk -F: '$1 == "0" {print $3}' /proc/self/cgroup)
boundary="/sys/fs/cgroup$cgroup_relative"
[[ $(cat "$boundary/memory.max") == 536870912 ]]
[[ $(cat "$boundary/pids.max") == 32 ]]
[[ $(cat "$boundary/memory.swap.max") == 0 ]]
[[ $(systemctl show --value --property=RuntimeMaxUSec "${TAKT_SAFETY_UNIT:?}") == 20s ]]
# Move only this unit's process; never change a shared host controller.
mkdir "$boundary/supervisor"
printf '%s\n' "$$" > "$boundary/supervisor/cgroup.procs"
printf '+cpu +memory +pids\n' > "$boundary/cgroup.subtree_control"
export TAKT_SAFETY_BOUNDARY="$boundary"
if [[ "${TAKT_EXTENDED_SAFETY:-}" == 1 ]]; then
  exec python3 "$(dirname "$0")/linux-safety-extended.py" "$@"
fi
if [[ $# == 4 ]]; then
  export TAKT_TEST_BINARY="$1"
  export TAKT_TEST_FIXTURE="$2"
  "$4" --ignored --exact task_loader_constructor_runs_inside_required_containment \
    --nocapture > "$3/loader-constructor.txt" 2>&1
  set -- "$1" "$2" "$3"
fi
exec python3 "$(dirname "$0")/linux-safety.py" "$@"
