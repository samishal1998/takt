#!/usr/bin/env python3
"""Bounded, standard-library performance evidence; goals never hide test failures."""
import argparse
import ctypes
import hashlib
import json
import os
import pathlib
import platform
import shlex
import signal
import statistics
import struct
import subprocess
import tempfile
import time

MAC_SAMPLER = None


def summary(values):
    return {"samples": values, "median": statistics.median(values), "min": min(values), "max": max(values)}


def environment():
    env = os.environ.copy()
    for key in ["TAKT_FILE", "TAKT_JOBS", "TAKT_CPU", "TAKT_MEMORY", "TAKT_ENFORCEMENT", "TAKT_OUTPUT"]:
        env.pop(key, None)
    return env


def stop(process):
    if process.poll() is None:
        process.send_signal(signal.SIGTERM)
        try:
            process.wait(timeout=2)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=1)


def elapsed(command, env, timeout=15):
    def deadline_expired(_signal, _frame):
        raise TimeoutError("measurement command exceeded its independent deadline")

    start = time.perf_counter()
    process = subprocess.Popen(command, env=env, stdin=subprocess.DEVNULL,
                               stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    previous_handler = signal.signal(signal.SIGALRM, deadline_expired)
    signal.setitimer(signal.ITIMER_REAL, timeout)
    try:
        # Blocking waitpid records completion without Popen.wait(timeout)'s
        # polling delay, which materially distorts millisecond startup samples.
        code = process.wait()
        if code:
            raise RuntimeError(f"measurement command exited {code}: {command[:5]}")
        return time.perf_counter() - start
    finally:
        signal.setitimer(signal.ITIMER_REAL, 0)
        signal.signal(signal.SIGALRM, previous_handler)
        stop(process)


def repeated(command, env, count):
    elapsed(command, env)  # One explicitly unmeasured warm-up.
    return summary([elapsed(command, env) for _ in range(count)])


def snapshot(pid):
    global MAC_SAMPLER
    if platform.system() == "Linux":
        stat = pathlib.Path(f"/proc/{pid}/stat").read_text()
        fields = stat.rsplit(")", 1)[1].split()
        return {"cpu_seconds": (int(fields[11]) + int(fields[12])) / os.sysconf("SC_CLK_TCK"),
                "rss_bytes": int(fields[21]) * os.sysconf("SC_PAGE_SIZE")}
    if platform.system() == "Darwin":
        # libproc's public proc_taskinfo layout is six u64 values followed by
        # twelve i32 values. CPU totals use Mach absolute ticks, which require
        # timebase conversion on Apple Silicon; resident size uses bytes.
        if MAC_SAMPLER is None:
            library = ctypes.CDLL("/usr/lib/libproc.dylib", use_errno=True)
            library.proc_pidinfo.argtypes = [ctypes.c_int, ctypes.c_int, ctypes.c_uint64,
                                             ctypes.c_void_p, ctypes.c_int]
            library.proc_pidinfo.restype = ctypes.c_int
            system = ctypes.CDLL("/usr/lib/libSystem.B.dylib")
            timebase = (ctypes.c_uint32 * 2)()
            system.mach_timebase_info.argtypes = [ctypes.POINTER(ctypes.c_uint32)]
            system.mach_timebase_info.restype = ctypes.c_int
            if system.mach_timebase_info(timebase) or not timebase[1]:
                raise RuntimeError("Mach timebase unavailable")
            MAC_SAMPLER = (library, timebase[0] / timebase[1] / 1_000_000_000)
        library, seconds_per_tick = MAC_SAMPLER
        buffer = ctypes.create_string_buffer(96)
        count = library.proc_pidinfo(pid, 4, 0, buffer, len(buffer))  # PROC_PIDTASKINFO
        if count != len(buffer):
            raise OSError(ctypes.get_errno(), "proc_pidinfo failed")
        _, rss, user, system, _, _ = struct.unpack_from("=6Q", buffer)
        return {"cpu_seconds": (user + system) * seconds_per_tick, "rss_bytes": rss}
    raise RuntimeError("native CPU/RSS sampling supports Linux and macOS")


def config(tasks, jobs=4):
    return {"version": 1, "enforcement": "off", "output": "plain",
            "scheduling": {"jobs": jobs, "cpu": 1, "memory": "256MiB", "reserve_memory": "1MiB"},
            "defaults": {"resources": {"cpu": 0.01, "memory": "1MiB"}, "stop_grace": "100ms"},
            "tasks": tasks}


def task(fixture, *args):
    return {"kind": "task", "command": [str(fixture), *map(str, args)]}


def run_command(binary, path, selected):
    return [str(binary), "run", "--file", str(path), *selected]


def measure_idle(binary, fixture, directory, seconds, env):
    service = {"kind": "service", "command": [str(fixture), "idle", "75000"],
               "readiness": {"type": "stdio", "stream": "stdout", "contains": "PID ", "timeout": "3s"}}
    value = config({"idle": service}, jobs=1)
    value["output"] = "json"
    value["enforcement"] = "auto"
    path = directory / "idle.json"
    path.write_text(json.dumps(value))
    runner = subprocess.Popen(run_command(binary, path, ["idle"]), env=env,
                              stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    baseline = None
    captured = bytearray()
    os.set_blocking(runner.stdout.fileno(), False)

    def drain():
        while True:
            try:
                data = os.read(runner.stdout.fileno(), 8192)
            except BlockingIOError:
                break
            if not data:
                break
            captured.extend(data)
            if len(captured) > 1024 * 1024:
                raise RuntimeError("idle supervisor output exceeded 1 MiB")

    try:
        until = time.monotonic() + 5
        ready = False
        while not ready:
            drain()
            for line in captured.splitlines():
                try:
                    ready |= json.loads(line).get("event") == "task_ready"
                except json.JSONDecodeError:
                    pass  # Last record may span reads.
            if runner.poll() is not None or time.monotonic() > until:
                raise RuntimeError("idle service did not become ready within 5 seconds")
            time.sleep(0.01)
        baseline = subprocess.Popen([str(fixture), "idle", "75000"], env=env,
                                    stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        time.sleep(0.5)  # Exclude loader/startup CPU from the quiescent interval.
        started = time.monotonic()
        samples = []
        while True:
            drain()
            if runner.poll() is not None or baseline.poll() is not None:
                raise RuntimeError("idle process ended before observation completed")
            now = time.monotonic()
            samples.append({"elapsed_seconds": now - started, "supervisor": snapshot(runner.pid),
                            "sleeping_fixture_baseline": snapshot(baseline.pid)})
            if now - started >= seconds:
                break
            time.sleep(min(0.5, seconds - (now - started)))
        duration = samples[-1]["elapsed_seconds"] - samples[0]["elapsed_seconds"]
        events = [json.loads(line) for line in captured.splitlines() if line.endswith(b"}")]
        capabilities = next((event.get("capabilities") for event in events if event.get("event") == "capabilities"), None)
        result = {"duration_seconds": duration, "raw_samples": samples, "sampling_interval_seconds": 0.5,
                  "enforcement": "auto", "capabilities": capabilities}
        for key in ["supervisor", "sleeping_fixture_baseline"]:
            delta = samples[-1][key]["cpu_seconds"] - samples[0][key]["cpu_seconds"]
            result[key] = {"cpu_seconds": delta, "percent_one_core": 100 * delta / duration,
                           "rss_bytes": summary([sample[key]["rss_bytes"] for sample in samples])}
        return result
    finally:
        stop(runner)
        if baseline is not None:
            stop(baseline)
        runner.stdout.close()


def shell_batches(commands, slots):
    # Fixed identical commands use the same maximum number of active children.
    # The shell has no resource discovery, dependency graph, or readiness work.
    script = ["set -eu"]
    for offset in range(0, len(commands), slots):
        script.append('task_pids=""')
        for command in commands[offset:offset + slots]:
            script.append(" ".join(map(shlex.quote, command)) + ' & task_pids="$task_pids $!"')
        script.append('for task_pid in $task_pids; do wait "$task_pid"; done')
    return ["/bin/sh", "-c", "\n".join(script)]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=pathlib.Path)
    parser.add_argument("fixture", type=pathlib.Path)
    parser.add_argument("output", type=pathlib.Path)
    parser.add_argument("--idle-seconds", type=float, default=60)
    parser.add_argument("--samples", type=int, default=7)
    args = parser.parse_args()
    if not 1 <= args.idle_seconds <= 60 or not 1 <= args.samples <= 25:
        parser.error("idle duration must be 1..60 seconds and sample count 1..25")
    binary, fixture = args.binary.resolve(strict=True), args.fixture.resolve(strict=True)
    args.output.mkdir(parents=True, exist_ok=True)
    env = environment()
    digest = hashlib.sha256()
    with binary.open("rb") as stream:
        for block in iter(lambda: stream.read(65536), b""):
            digest.update(block)
    report = {"schema_version": 1, "measured_at_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
              "platform": platform.platform(), "machine": platform.machine(), "kernel": platform.release(),
              "libc": platform.libc_ver(), "logical_cpus": os.cpu_count(), "load_average": os.getloadavg(),
              "binary": str(binary), "sha256": digest.hexdigest(), "executable_bytes": binary.stat().st_size,
              "policy": {"finite_workload_enforcement": "off", "idle_enforcement": "auto", "aggregate_cpu_request": 1, "aggregate_memory_bytes": 256 * 1024 * 1024,
                         "task_cpu_request": 0.01, "task_memory_request_bytes": 1024 * 1024,
                         "max_fixture_allocation_bytes": 8 * 1024 * 1024, "maximum_parallel_tasks": 4,
                         "warmup_runs_per_case": 1, "samples_per_case": args.samples}}
    report["version_startup_seconds"] = repeated([str(binary), "--version"], env, args.samples)
    report["exec_exit_startup_seconds"] = repeated([str(binary), "exec", "--enforcement", "off", "--output", "plain", "--cpu", "1", "--memory", "256MiB", "--jobs", "1", "--", str(fixture), "exit", "0"], env, args.samples)
    with tempfile.TemporaryDirectory(prefix="takt-performance-") as scratch:
        directory = pathlib.Path(scratch)
        report["idle"] = measure_idle(binary, fixture, directory, args.idle_seconds, env)
        commands = [[str(fixture), "compute", "2000000"] for _ in range(8)]
        report["fixed_compute_comparison"] = []
        for slots in [1, 2, 4]:
            tasks = {f"work-{index}": {"kind": "task", "command": command} for index, command in enumerate(commands)}
            path = directory / f"fixed-{slots}.json"
            path.write_text(json.dumps(config(tasks, slots)))
            report["fixed_compute_comparison"].append({"slots": slots, "commands": commands,
                "takt_seconds": repeated(run_command(binary, path, sorted(tasks)), env, args.samples),
                "shell_seconds": repeated(shell_batches(commands, slots), env, args.samples)})
        scenarios = {
            "mixed": {"cpu": task(fixture, "compute", 10000000), "memory": task(fixture, "allocate", 8, 100), "wait": task(fixture, "sleep", 100)},
            "many_tiny": {f"tiny-{index:02}": task(fixture, "exit", 0) for index in range(16)},
            "heavy_output": {"output": task(fixture, "output", 1048576, "chunked")},
            "readiness": {"service": {"kind": "service", "command": [str(fixture), "startup", "5000"],
                "readiness": {"type": "stdio", "stream": "stdout", "contains": "READY", "timeout": "2s"}},
                "consumer": {**task(fixture, "exit", 0), "depends_on": [{"task": "service", "when": "ready"}]}}
        }
        report["representative_workloads"] = {}
        for name, tasks in scenarios.items():
            path = directory / f"{name}.json"
            value = config(tasks)
            path.write_text(json.dumps(value))
            selected = ["consumer"] if name == "readiness" else sorted(tasks)
            report["representative_workloads"][name] = {"config": value, "selected": selected,
                "wall_seconds": repeated(run_command(binary, path, selected), env, args.samples)}
    supervisor = report["idle"]["supervisor"]
    report["measurement_goals"] = {"executable_under_1000000_bytes": report["executable_bytes"] < 1000000,
        "idle_rss_under_5_mib": supervisor["rss_bytes"]["median"] < 5 * 1024 * 1024,
        "idle_cpu_under_0_1_percent_one_core": supervisor["percent_one_core"] < 0.1,
        "nonblocking_goals": True, "full_60_second_observation": args.idle_seconds == 60}
    destination = args.output / "measurement.json"
    destination.write_text(json.dumps(report, indent=2) + "\n")
    print(f"Performance evidence: {destination}")
    print(f"Supervisor only: {supervisor['percent_one_core']:.3f}% of one CPU; median RSS {supervisor['rss_bytes']['median'] / 1048576:.2f} MiB over {report['idle']['duration_seconds']:.2f}s")
    print(f"Executable: {report['executable_bytes']} bytes; version startup median {report['version_startup_seconds']['median'] * 1000:.2f} ms")


if __name__ == "__main__":
    main()
