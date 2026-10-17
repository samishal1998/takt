#!/usr/bin/env python3
"""Privileged namespace setup, only on the disposable CI VM; no allocation tests."""
import json
import os
import pathlib
import runpy

assert os.environ.get("GITHUB_ACTIONS") == "true", "namespace cases require disposable CI"
assert os.geteuid() == 0, "only this namespace setup lane requires privilege"
workload_uid = int(os.environ["TAKT_WORKLOAD_UID"])
workload_gid = int(os.environ["TAKT_WORKLOAD_GID"])
assert workload_uid != 0
scope = runpy.run_path(str(pathlib.Path(__file__).with_name("linux-safety.py")))
execute, task, boundary, evidence = [scope[name] for name in ("execute", "task", "boundary", "evidence_directory")]

readonly = [
    "unshare", "--mount", "--propagation", "private", "/bin/bash", "-c",
    'set -e; mount --bind /sys/fs/cgroup /sys/fs/cgroup; mount -o remount,bind,ro /sys/fs/cgroup; exec "$@"',
    "readonly-cgroup",
]
events, _ = execute("readonly-required", {"a": task("exit", 0)}, expected=3, prefix=readonly)
assert not any(event.get("event") == "task_started" for event in events), events
events, _ = execute("readonly-auto", {"a": task("exit", 0)}, prefix=readonly, enforcement="auto")
capabilities = next(event["capabilities"] for event in events if event.get("event") == "capabilities")
assert "hard" not in capabilities["aggregate_memory"], capabilities

events, _ = execute("denied-required", {"a": task("exit", 0)}, expected=3, prefix=["setpriv", "--reuid=65534", "--regid=65534", "--clear-groups"])
assert not any(event.get("event") == "task_started" for event in events), events

# A real intermediate ceiling must constrain discovery even when the task asks for more.
nested = boundary / "ancestor-test"
nested.mkdir()
(nested / "memory.max").write_text(str(128 * 1024 * 1024))
(nested / "cpu.max").write_text("50000 100000")
(nested / "cgroup.subtree_control").write_text("+cpu +memory +pids")
supervisor = nested / "supervisor"
supervisor.mkdir()
for directory in (nested, supervisor):
    for path in (directory, directory / "cgroup.procs", directory / "cgroup.threads", directory / "cgroup.subtree_control"):
        os.chown(path, workload_uid, workload_gid)
drop_privilege = ["setpriv", f"--reuid={workload_uid}", f"--regid={workload_gid}", "--clear-groups"]
prefix = ["/bin/bash", "-c", 'set -e; printf "%s\\n" "$$" > "$1/cgroup.procs"; shift; exec "$@"', "nested", str(supervisor), *drop_privilege]
try:
    events, _ = execute("ancestor-discovery", {"a": task("sleep", 100)}, prefix=prefix, search_root=nested)
    budget = next(event["budget"] for event in events if event.get("event") == "run_started")
    assert budget["cpu_millicores"] <= 500, budget
    assert budget["memory_bytes"] <= 128 * 1024 * 1024, budget
finally:
    supervisor.rmdir()
    nested.rmdir()

# A constrained current leaf must not be bypassed to reach the writable parent.
limited = boundary / "no-delegation-test"
limited.mkdir()
(limited / "memory.max").write_text(str(128 * 1024 * 1024))
prefix = ["/bin/bash", "-c", 'set -e; printf "%s\\n" "$$" > "$1/cgroup.procs"; shift; exec "$@"', "limited", str(limited), *drop_privilege]
try:
    events, _ = execute("missing-delegation-required", {"a": task("exit", 0)}, expected=3, prefix=prefix)
    assert not any(event.get("event") == "task_started" for event in events), events
    events, _ = execute("missing-delegation-auto", {"a": task("exit", 0)}, prefix=prefix, enforcement="auto")
    capabilities = next(event["capabilities"] for event in events if event.get("event") == "capabilities")
    assert "hard" not in capabilities["aggregate_memory"], capabilities
finally:
    limited.rmdir()

(evidence / "extended-capabilities.json").write_text(json.dumps(capabilities, indent=2) + "\n")
print("Read-only, denied, ancestor, and delegation checks passed.")
