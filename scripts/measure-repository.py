#!/usr/bin/env python3
"""Compare this repository's four finite checks with a shell at two active slots."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shlex
import signal
import statistics
import subprocess
import time


COMMANDS = {
    "format": ["cargo", "fmt", "--all", "--check"],
    "lint": ["cargo", "clippy", "--workspace", "--all-targets", "--locked", "--", "-D", "warnings"],
    "fixtures": ["cargo", "build", "-p", "takt-test-fixture", "--locked"],
    "tests": ["cargo", "test", "--workspace", "--locked", "--", "--test-threads=1"],
}


def digest(path):
    value = hashlib.sha256()
    with path.open("rb") as source:
        while block := source.read(65536):
            value.update(block)
    return value.hexdigest()


def summary(values):
    return {"samples": values, "median": statistics.median(values),
            "min": min(values), "max": max(values)}


def measure(command, cwd, env):
    def expired(_signal, _frame):
        raise TimeoutError("repository measurement exceeded its 30-second deadline")

    started = time.perf_counter()
    process = subprocess.Popen(command, cwd=cwd, env=env, start_new_session=True,
                               stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                               stderr=subprocess.DEVNULL)
    previous = signal.signal(signal.SIGALRM, expired)
    signal.setitimer(signal.ITIMER_REAL, 30)
    try:
        code = process.wait()
        elapsed = time.perf_counter() - started
        if code:
            raise RuntimeError(f"repository check exited {code}; run the checks directly for diagnostics")
        return elapsed
    finally:
        signal.setitimer(signal.ITIMER_REAL, 0)
        signal.signal(signal.SIGALRM, previous)
        if process.poll() is None:
            # Takt handles this signal by stopping its task groups. The shell
            # and ordinary Cargo children share the independently owned group.
            os.killpg(process.pid, signal.SIGTERM)
            try:
                process.wait(timeout=2)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait(timeout=1)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--repository", type=Path, default=Path(__file__).resolve().parent.parent)
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    repository = args.repository.resolve(strict=True)
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    if not (repository / "Cargo.toml").is_file():
        parser.error("repository must contain Cargo.toml")
    env = os.environ.copy()
    for key in ("TAKT_FILE", "TAKT_JOBS", "TAKT_CPU", "TAKT_MEMORY", "TAKT_ENFORCEMENT", "TAKT_OUTPUT"):
        env.pop(key, None)
    overrides = {"CARGO_BUILD_JOBS": "2", "CARGO_TERM_COLOR": "never", "RUST_TEST_THREADS": "1"}
    env.update(overrides)
    tasks = {name: {"kind": "task", "command": command, "cwd": str(repository)}
             for name, command in COMMANDS.items()}
    tasks["tests"]["depends_on"] = [{"task": "fixtures", "when": "exit", "codes": [0]}]
    config = {"version": 1, "enforcement": "off", "failure": "continue", "output": "plain",
              "scheduling": {"jobs": 2, "cpu": 2, "memory": "4GiB", "reserve_memory": "1MiB"},
              "defaults": {"resources": {"cpu": 1, "memory": "1MiB"}, "stop_grace": "1s"},
              "tasks": tasks, "groups": {"checks": ["format", "lint", "tests"]}}
    config_path = output / "repository.json"
    config_path.write_text(json.dumps(config, indent=2) + "\n")

    # Format and the fixtures->tests chain occupy the first two slots. When
    # format finishes, lint occupies its slot. Tests require successful fixtures.
    quote = lambda name: " ".join(map(shlex.quote, COMMANDS[name]))
    shell = "\n".join([
        "set -u", 'result=0',
        f"{quote('format')} & format_pid=$!",
        f"( {quote('fixtures')} && {quote('tests')} ) & tests_pid=$!",
        'wait "$format_pid" || result=1',
        f"{quote('lint')} & lint_pid=$!",
        'wait "$tests_pid" || result=1',
        'wait "$lint_pid" || result=1',
        'exit "$result"',
    ])
    (output / "baseline.sh").write_text(shell + "\n")
    commands = {"takt": [str(binary), "run", "--file", str(config_path), "checks"],
                "shell": ["/bin/sh", "-c", shell]}
    report = {"schema_version": 1, "measured_at_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
              "platform": platform.platform(), "logical_cpus": os.cpu_count(),
              "load_average": os.getloadavg(), "binary": str(binary), "sha256": digest(binary),
              "repository": str(repository), "cargo_lock_sha256": digest(repository / "Cargo.lock"),
              "dogfood_file_sha256": digest(repository / "takt.yaml"),
              "policy": {"active_slots": 2, "cargo_build_jobs": 2, "enforcement": "off",
                         "environment_overrides": overrides, "identical_inherited_environment": True,
                         "stdin_stdout_stderr": "/dev/null", "deadline_seconds_per_run": 30,
                         "warmups_per_runner": 1, "samples_per_runner": 3,
                         "admission_requests_are_not_memory_limits": True},
              "commands": COMMANDS, "dependencies": {"tests": {"fixtures": [0]}},
              "config": config, "shell_script": shell, "warmups": {}, "sample_order": [],
              "observations": {"takt": [], "shell": []}}
    report_path = output / "measurement.json"
    try:
        for name in ("shell", "takt"):
            report["warmups"][name] = measure(commands[name], repository, env)
        for index in range(3):
            order = ("takt", "shell") if index % 2 == 0 else ("shell", "takt")
            for name in order:
                report["sample_order"].append(name)
                report["observations"][name].append(measure(commands[name], repository, env))
        report["wall_seconds"] = {name: summary(values) for name, values in report["observations"].items()}
        report["status"] = "passed"
    except (OSError, RuntimeError, TimeoutError) as error:
        report["status"] = "failed"
        report["error"] = str(error)
        raise
    finally:
        report_path.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({"report": str(report_path), "wall_seconds": report["wall_seconds"]}))


if __name__ == "__main__":
    main()
