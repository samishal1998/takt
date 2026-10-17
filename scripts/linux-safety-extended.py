#!/usr/bin/env python3
"""Disposable CI namespace setup and a bounded, unprivileged shared-ceiling proof."""
import json
import os
import pathlib
import runpy
import selectors
import subprocess
import tempfile
import time

assert os.environ.get("GITHUB_ACTIONS") == "true", "namespace cases require disposable CI"
assert os.geteuid() == 0, "only this namespace setup lane requires privilege"
workload_uid = int(os.environ["TAKT_WORKLOAD_UID"])
workload_gid = int(os.environ["TAKT_WORKLOAD_GID"])
assert workload_uid != 0
scope = runpy.run_path(str(pathlib.Path(__file__).with_name("linux-safety.py")))
execute, task, boundary, evidence = [scope[name] for name in ("execute", "task", "boundary", "evidence_directory")]


def assert_refused_before_launch(events):
    assert any(event.get("event") == "error" and event.get("reason") == "capability_unavailable" for event in events), events
    assert not any(event.get("event") == "task_started" for event in events), events


readonly = [
    "unshare", "--mount", "--propagation", "private", "/bin/bash", "-c",
    'set -e; mount --bind /sys/fs/cgroup /sys/fs/cgroup; mount -o remount,bind,ro /sys/fs/cgroup; exec "$@"',
    "readonly-cgroup",
]
events, _ = execute("readonly-required", {"a": task("exit", 0)}, expected=3, prefix=readonly)
assert_refused_before_launch(events)
events, _ = execute("readonly-auto", {"a": task("exit", 0)}, prefix=readonly, enforcement="auto")
capabilities = next(event["capabilities"] for event in events if event.get("event") == "capabilities")
assert "hard" not in capabilities["aggregate_memory"], capabilities

events, _ = execute("denied-required", {"a": task("exit", 0)}, expected=3, prefix=["setpriv", "--reuid=65534", "--regid=65534", "--clear-groups"])
assert_refused_before_launch(events)

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

# An outer Takt launches another Takt while an independent sibling holds 16 MiB.
# Both runners must observe the same real ancestor ceiling and its used memory;
# this checks discovery, not coordination between their independent reservations.
shared = boundary / "shared-runner-test"
shared.mkdir()
shared_limit = 96 * 1024 * 1024
(shared / "memory.max").write_text(str(shared_limit))
(shared / "cpu.max").write_text("50000 100000")
(shared / "cgroup.subtree_control").write_text("+cpu +memory +pids")
shared_supervisor = shared / "supervisor"
sibling_group = shared / "sibling"
shared_supervisor.mkdir()
sibling_group.mkdir()
(sibling_group / "memory.max").write_text(str(32 * 1024 * 1024))
for directory in (shared, shared_supervisor):
    for path in (directory, directory / "cgroup.procs", directory / "cgroup.threads", directory / "cgroup.subtree_control"):
        os.chown(path, workload_uid, workload_gid)


def enter_as_runner(leaf):
    return ["/bin/bash", "-c", 'set -e; printf "%s\\n" "$$" > "$1/cgroup.procs"; shift; exec "$@"', "shared", str(leaf), *drop_privilege]


sibling = None
try:
    before_sibling = int((shared / "memory.current").read_text())
    sibling = subprocess.Popen([*enter_as_runner(sibling_group), str(scope["fixture"]), "allocate", "16", "6000"], stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    handshake = bytearray()
    deadline = time.monotonic() + 1.5
    with selectors.DefaultSelector() as selector:
        selector.register(sibling.stdout, selectors.EVENT_READ)
        while b"\n" not in handshake:
            assert time.monotonic() < deadline, "shared-ceiling sibling handshake expired"
            for key, _ in selector.select(0.01):
                block = os.read(key.fd, 4096)
                assert block, f"sibling exited before allocation handshake: {handshake!r}"
                handshake.extend(block)
                assert len(handshake) <= 4096, "excess sibling handshake output"
    assert handshake.strip() == b"ALLOCATED 16777216", handshake
    assert sibling.poll() is None
    sibling_used = int((sibling_group / "memory.current").read_text())
    shared_used = int((shared / "memory.current").read_text())
    assert sibling_used >= 16 * 1024 * 1024
    assert shared_used >= before_sibling + 16 * 1024 * 1024
    assert str(sibling.pid) in (sibling_group / "cgroup.procs").read_text().splitlines()
    with tempfile.TemporaryDirectory(prefix="takt-nested-") as directory:
        os.chmod(directory, 0o755)
        inner_path = pathlib.Path(directory) / "inner.json"
        inner_path.write_text(json.dumps({
            "version": 1, "enforcement": "off", "output": "json",
            "scheduling": {"jobs": 1, "cpu": 1, "memory": "96MiB", "reserve_memory": "1MiB"},
            "tasks": {"short": task("sleep", 100)},
        }))
        nested_task = task("exit", 0, memory="96MiB")
        nested_task["command"] = [str(scope["binary"]), "run", "short", "--file", str(inner_path)]
        events, _ = execute("nested-runner-sibling", {"nested": nested_task}, prefix=enter_as_runner(shared_supervisor), search_root=shared)
    inner_output = "".join(event["data"] for event in events if event.get("event") == "log" and event.get("task_id") == "nested" and event.get("stream") == "stdout" and event.get("encoding") == "utf8")
    inner_events = [json.loads(line) for line in inner_output.splitlines()]
    inner_summary = next(event for event in inner_events if event.get("event") == "summary")
    assert inner_summary["exit_code"] == 0, inner_events
    budgets = {}
    for label, runner_events in (("outer", events), ("inner", inner_events)):
        budget = next(event["budget"] for event in runner_events if event.get("event") == "run_started")
        assert budget["cpu_millicores"] <= 500, budget
        assert budget["memory_bytes"] <= shared_limit - 16 * 1024 * 1024, budget
        source_prefix = f"{shared}/memory.max headroom="
        source = next(value for value in budget["discovered"]["provenance"] if value.startswith(source_prefix))
        observed_headroom = int(source.removeprefix(source_prefix).removesuffix(" bytes"))
        assert observed_headroom <= shared_limit - 16 * 1024 * 1024, budget
        budgets[label] = budget
    assert sibling.poll() is None, "independent sibling must remain active throughout both runners"
    assert (shared / "cpu.max").read_text().strip() == "50000 100000"
    assert int((shared / "memory.max").read_text()) == shared_limit
    (evidence / "nested-runner-sibling-inner.ndjson").write_text(inner_output)
    (evidence / "nested-runner-sibling-ceilings.json").write_text(json.dumps({
        "cpu_millicores": 500, "memory_max": shared_limit,
        "memory_before_sibling": before_sibling, "memory_with_sibling": shared_used,
        "sibling_memory_current": sibling_used, "sibling_pid": sibling.pid,
        "workload_uid": workload_uid, "budgets": budgets,
        "reservation_scope": "independent_per_runner",
    }, indent=2) + "\n")
finally:
    if sibling is not None:
        if sibling.poll() is None:
            sibling.terminate()
        try:
            sibling.wait(timeout=1)
        except subprocess.TimeoutExpired:
            sibling.kill()
            sibling.wait(timeout=1)
        sibling.stdout.close()
    sibling_group.rmdir()
    shared_supervisor.rmdir()
    shared.rmdir()

# A constrained current leaf must not be bypassed to reach the writable parent.
limited = boundary / "no-delegation-test"
limited.mkdir()
(limited / "memory.max").write_text(str(128 * 1024 * 1024))
prefix = ["/bin/bash", "-c", 'set -e; printf "%s\\n" "$$" > "$1/cgroup.procs"; shift; exec "$@"', "limited", str(limited), *drop_privilege]
try:
    events, _ = execute("missing-delegation-required", {"a": task("exit", 0)}, expected=3, prefix=prefix)
    assert_refused_before_launch(events)
    events, _ = execute("missing-delegation-auto", {"a": task("exit", 0)}, prefix=prefix, enforcement="auto")
    capabilities = next(event["capabilities"] for event in events if event.get("event") == "capabilities")
    assert "hard" not in capabilities["aggregate_memory"], capabilities
finally:
    limited.rmdir()

(evidence / "extended-capabilities.json").write_text(json.dumps(capabilities, indent=2) + "\n")
print("Read-only, denied, ancestor, nested-runner/sibling, and delegation checks passed.")
