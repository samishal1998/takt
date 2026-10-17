#!/usr/bin/env python3
"""Generate standalone installers from the pinned manifest or release checksums."""

import argparse
import json
import os
from pathlib import Path
import re
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
TARGETS = (
    "x86_64-unknown-linux-gnu",
    "x86_64-unknown-linux-musl",
    "aarch64-apple-darwin",
)
ENTRYPOINTS = {
    "install.sh": '{\n    takt_install "$@"\n}\n',
    "taktw": """takt_launch() {
    # A PATH version probe must never recurse into another copy of the launcher.
    [ "${TAKT_BOOTSTRAP_PROBE:-}" != 1 ] || exit 127
    case "${TAKT_USE_SYSTEM:-0}" in
        1) takt_use_installed "$@" ;;
        0) ;;
        *) takt_fail 'TAKT_USE_SYSTEM must be 0 or 1' ;;
    esac
    takt_cached_binary
    exec "$takt_binary" "$@"
}

{
    takt_launch "$@"
}
""",
}


def read_small(path):
    with path.open("r", encoding="utf-8") as source:
        text = source.read(65537)
    if len(text) > 65536:
        raise ValueError(f"input exceeds 64 KiB: {path}")
    return text


def release_hashes(version, checksums):
    expected = {f"takt-{version}-{target}.tar.gz": target for target in TARGETS}
    hashes = {}
    for number, line in enumerate(read_small(checksums).splitlines(), 1):
        if not line.strip():
            continue
        match = re.fullmatch(r"([0-9a-fA-F]{64}) [ *](.+)", line)
        if match is None:
            raise ValueError(f"invalid SHA256SUMS line {number}")
        digest, filename = match.groups()
        if filename not in expected:
            raise ValueError(f"unexpected release archive in SHA256SUMS: {filename}")
        target = expected[filename]
        if target in hashes:
            raise ValueError(f"duplicate checksum for {filename}")
        hashes[target] = digest.lower()
    return hashes


def write_atomic(path, contents):
    if path.is_symlink():
        raise ValueError(f"refusing symlink output: {path}")
    descriptor, temporary_name = tempfile.mkstemp(prefix=f".{path.name}.", dir=path.parent)
    temporary = Path(temporary_name)
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8", newline="\n") as output:
            output.write(contents)
            os.fchmod(output.fileno(), 0o755)
        os.replace(temporary, path)
    finally:
        temporary.unlink(missing_ok=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="check generated files without writing")
    parser.add_argument("--version", help="release version, without a v prefix; requires --checksums")
    parser.add_argument("--checksums", type=Path, help="SHA256SUMS from the verified release archives")
    parser.add_argument("--output-dir", type=Path, default=ROOT)
    args = parser.parse_args()
    if bool(args.version) != bool(args.checksums):
        parser.error("--version and --checksums must be supplied together")
    if args.version:
        version = args.version
        hashes = release_hashes(version, args.checksums)
    else:
        manifest = json.loads(read_small(ROOT / "scripts/bootstrap-release.json"))
        version = manifest["version"]
        hashes = manifest["sha256"]
    if not isinstance(version, str) or not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?", version):
        raise ValueError("version must be MAJOR.MINOR.PATCH with optional prerelease/build suffix, without a v prefix")
    if not isinstance(hashes, dict) or set(hashes) != set(TARGETS):
        raise ValueError("checksums must include exactly the three supported targets")
    for target, digest in hashes.items():
        if not isinstance(digest, str) or not re.fullmatch(r"[0-9a-f]{64}", digest):
            raise ValueError(f"invalid SHA256 for {target}")
    template = read_small(ROOT / "scripts/bootstrap.sh.in").replace("@VERSION@", version)
    for target, digest in hashes.items():
        template = template.replace("@SHA_" + target.replace("-", "_").upper() + "@", digest)
    if not args.check:
        args.output_dir.mkdir(parents=True, exist_ok=True)
    stale = []
    for name, entrypoint in ENTRYPOINTS.items():
        contents = template.replace("@ENTRYPOINT@", entrypoint.rstrip("\n"))
        if re.search(r"@[A-Z_]+@", contents):
            raise ValueError("unresolved template placeholder")
        destination = args.output_dir / name
        if args.check:
            if not destination.is_file() or destination.is_symlink() or read_small(destination) != contents or not os.access(destination, os.X_OK):
                stale.append(name)
        else:
            write_atomic(destination, contents)
    if stale:
        raise ValueError("generated bootstrap files are stale: " + ", ".join(stale) + "; run python3 scripts/generate-bootstrap.py")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError, TypeError) as error:
        print(f"generate-bootstrap: {error}", file=sys.stderr)
        sys.exit(1)
