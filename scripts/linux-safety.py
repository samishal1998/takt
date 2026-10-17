#!/usr/bin/env python3
"""Bounded real-controller tests, called only by linux-safety.sh in ephemeral CI."""
import json
import os
import pathlib
import selectors
import signal
import subprocess
import sys
import tempfile
import time

binary, fixture, evidence_directory = map(pathlib.Path, sys.argv[1:])
boundary = pathlib.Path(os.environ["TAKT_SAFETY_BOUNDARY"])
evidence_directory.mkdir(parents=True, exist_ok=True)
assert (boundary / "memory.max").read_text().strip() == "536870912"
assert (boundary / "pids.max").read_text().strip() == "32"
assert (boundary / "memory.swap.max").read_text().strip() == "0"


def task(*args, memory="128MiB", cpu=1):
    return {"kind": "task", "command": [str(fixture), *map(str, args)], "resources": {"cpu": 0.01, "memory": "1MiB"}, "limits": {"cpu": cpu, "memory": memory}, "stop_grace": "50ms"}


def counters(path):
    try:
        return dict(line.split() for line in path.read_text().splitlines())
    except FileNotFoundError:
        return {}


def execute(name, tasks, memory="256MiB", expected=0, prefix=(), enforcement="required", search_root=None, cancel_on_log=False):
    search_root = search_root or boundary
    config = {"version": 1, "enforcement": enforcement, "output": "json", "scheduling": {"jobs": 4, "cpu": 1, "memory": memory, "reserve_memory": "1MiB"}, "tasks": tasks}
    with tempfile.TemporaryDirectory(prefix="takt-safety-") as directory:
        config_path = pathlib.Path(directory) / "takt.json"
        config_path.write_text(json.dumps(config))
        os.chmod(directory, 0o755)
        start = time.monotonic()
        process = subprocess.Popen([*prefix, str(binary), "run", "--output", "json", "--file", str(config_path), *tasks], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        selector = selectors.DefaultSelector()
        selector.register(process.stdout, selectors.EVENT_READ, "stdout")
        selector.register(process.stderr, selectors.EVENT_READ, "stderr")
        captured = {"stdout": bytearray(), "stderr": bytearray()}
        snapshots = {}
        max_members = 0
        interrupted = False
        try:
            while selector.get_map() or process.poll() is None:
                if time.monotonic() - start > 4:
                    raise AssertionError(f"{name} exceeded independent 4-second timeout")
                for key, _ in selector.select(0.005):
                    block = os.read(key.fd, 8192)
                    if block:
                        captured[key.data].extend(block)
                        assert len(captured[key.data]) <= 2 * 1024 * 1024, "excess safety output"
                    else:
                        selector.unregister(key.fileobj)
                if cancel_on_log and not interrupted and b"PID " in captured["stdout"]:
                    process.send_signal(signal.SIGTERM)
                    interrupted = True
                for run in search_root.glob("takt-*"):
                    for directory in [run, *run.rglob("*")]:
                        if not directory.is_dir():
                            continue
                        snapshot = snapshots.setdefault(str(directory.relative_to(boundary)), {})
                        for filename in ["memory.events", "memory.events.local", "cpu.stat"]:
                            for field, value in counters(directory / filename).items():
                                key = f"{filename}:{field}"
                                snapshot[key] = max(snapshot.get(key, 0), int(value))
                        for filename in ["cpu.max", "memory.max", "memory.swap.max"]:
                            try:
                                snapshot[filename] = (directory / filename).read_text().strip()
                            except FileNotFoundError:
                                pass
                        try:
                            count = len((directory / "cgroup.procs").read_text().splitlines())
                            max_members = max(max_members, count)
                        except FileNotFoundError:
                            pass
            code = process.wait(timeout=1)
        finally:
            if process.poll() is None:
                process.kill()
                process.wait(timeout=1)
            selector.close()
        events = [json.loads(line) for line in captured["stdout"].splitlines()]
        report = {"name": name, "exit_code": code, "elapsed_seconds": time.monotonic() - start, "max_task_members": max_members, "controllers": snapshots, "stderr": captured["stderr"].decode(errors="replace")}
        (evidence_directory / f"{name}.ndjson").write_bytes(captured["stdout"])
        (evidence_directory / f"{name}.json").write_text(json.dumps(report, indent=2) + "\n")
        assert code == expected, report
        assert not list(search_root.glob("takt-*")), f"owned cgroups leaked after {name}"
        return events, report


def base_suite():
    assert os.geteuid() != 0, "ordinary containment tests must run as an unprivileged delegated user"
    (evidence_directory / "workload-identity.json").write_text(json.dumps({"uid": os.getuid(), "gid": os.getgid()}) + "\n")
    events, report = execute("task-oom", {"allocate": task("allocate", 128, 500, memory="64MiB"), "survivor": task("exit", 0)}, expected=1)
    assert any("memory" in str(event.get("reason", "")) or "oom" in str(event.get("reason", "")) for event in events), events
    assert any(value.get("memory.events:oom_kill", 0) > 0 for value in report["controllers"].values()), report
    assert any(event.get("task_id") == "survivor" and event.get("event") == "task_finished" and event.get("exit_code") == 0 for event in events), events

    events, report = execute("aggregate-oom", {"a": task("allocate", 48, 700), "b": task("allocate", 48, 700)}, memory="64MiB", expected=1)
    assert any(value.get("memory.events:oom_kill", 0) > 0 for value in report["controllers"].values()), report

    events, report = execute("cpu-quota", {"burn": task("burn", 1500, cpu=0.1)})
    quota = [value for value in report["controllers"].values() if value.get("cpu.max", "").startswith("10000 ")]
    assert quota, report
    assert max(value.get("cpu.stat:usage_usec", 0) for value in quota) < 600_000, report

    for mode in ["sleep", "detach"]:
        events, report = execute(f"immediate-tree-{mode}", {"tree": task("tree", 2, 200, mode)})
        assert report["max_task_members"] >= 3, report

    events, _ = execute("exit-137", {"ordinary": task("exit", 137)}, expected=1)
    assert not any("oom" in str(event.get("reason", "")) for event in events), events
    for repetition in range(3):
        execute(f"cleanup-{repetition}", {"short": task("exit", 0)})
    execute("cancel-cleanup", {"tree": task("tree", 2, 1000, "ignore")}, expected=143, cancel_on_log=True)

    events, report = execute("swap-capability", {"a": task("sleep", 100)})
    capabilities = next(event["capabilities"] for event in events if event.get("event") == "capabilities")
    workload = [value for path, value in report["controllers"].items() if path.endswith("/workload")]
    assert workload, report
    if "memory.swap.max" in workload[0]:
        assert workload[0]["memory.swap.max"] == "0", report
        assert capabilities["swap"] == "no_additional_workload_swap", capabilities
    else:
        assert capabilities["swap"] == "external_policy_only", capabilities
    print("Unprivileged real-controller safety checks passed; raw evidence retained.")


if __name__ == "__main__":
    base_suite()
