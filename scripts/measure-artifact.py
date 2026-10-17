#!/usr/bin/env python3
"""CI-only artifact evidence; Python is not a Takt runtime dependency."""
import hashlib
import json
import os
import pathlib
import platform
import re
import subprocess
import sys
from artifact_abi import glibc_requirements, version_tuple

target, executable, archive = sys.argv[1:]
executable, archive = pathlib.Path(executable), pathlib.Path(archive)


def output(*args):
    return subprocess.check_output(args, text=True, timeout=20, env={**os.environ, "LC_ALL": "C"})


abi = None
if target.endswith("linux-musl"):
    program_headers = output("readelf", "-l", str(executable))
    dependencies = output("readelf", "-d", str(executable))
    if "INTERP" in program_headers or "NEEDED" in dependencies:
        raise SystemExit("musl artifact has a dynamic loader or shared-library dependency")
elif target.endswith("linux-gnu"):
    dependencies = output("readelf", "--version-info", str(executable))
    symbols = output("readelf", "--dyn-syms", "--wide", str(executable))
    abi = glibc_requirements(symbols, dependencies)
    if abi["unsupported_required_versions"]:
        raise SystemExit(f"artifact requires unsupported glibc ABI: {abi['unsupported_required_versions']}")
else:
    dependencies = output("otool", "-L", str(executable))
    load_commands = output("otool", "-l", str(executable))
    versions = [version_tuple(version) for version in re.findall(r"\bminos ([0-9.]+)", load_commands)]
    if not versions or max(versions) > (15, 0, 0):
        raise SystemExit(f"unexpected macOS deployment target: {versions}")

digest = hashlib.sha256()
with executable.open("rb") as stream:
    for block in iter(lambda: stream.read(65536), b""):
        digest.update(block)
record = {
    "target": target,
    "platform": platform.platform(),
    "rustc": output("rustc", "--version").strip(),
    "profile": "release (all production features)",
    "executable_bytes": executable.stat().st_size,
    "archive_bytes": archive.stat().st_size,
    "sha256": digest.hexdigest(),
    "dependencies": dependencies,
    "glibc_abi": abi,
    "below_one_million_byte_goal": executable.stat().st_size < 1_000_000,
}
baseline_path = pathlib.Path("scripts/size-baseline.json")
baseline = json.loads(baseline_path.read_text()).get(target)
if baseline is not None:
    allowed = baseline + max(baseline * 0.1, 100 * 1024)
    if record["executable_bytes"] > allowed:
        raise SystemExit(f"unexplained size growth: {record['executable_bytes']} > {int(allowed)}; review and update baseline with evidence")
pathlib.Path(f"dist/{target}-size.json").write_text(json.dumps(record, indent=2) + "\n")
print(json.dumps(record, indent=2))
