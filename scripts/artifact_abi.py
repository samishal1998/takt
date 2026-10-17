"""Interpret readelf evidence without conflating weak fallback imports with ABI floors."""
import re


def version_tuple(version):
    parts = tuple(map(int, version.split(".")))
    return parts + (0,) * max(0, 3 - len(parts))


def glibc_requirements(symbols, versions):
    required = set()
    weak_imports = []
    for line in symbols.splitlines():
        match = re.match(r"\s*\d+:\s+\S+\s+\S+\s+\S+\s+(\S+)\s+\S+\s+UND\s+(\S+)", line)
        if not match or "@GLIBC_" not in match[2]:
            continue
        symbol, version = match[2].split("@", 1)
        if match[1] == "WEAK":
            weak_imports.append({"symbol": symbol, "version": version})
        else:
            required.add(version)
    # The loader independently enforces nonweak VERNEED entries. Ignoring these
    # would incorrectly accept an executable whose undefined symbols are all weak.
    for name, flags in re.findall(r"Name:\s+(GLIBC_[\w.]+)\s+Flags:\s+(.*?)\s+Version:", versions):
        if "WEAK" not in flags.split():
            required.add(name)
    unsupported = []
    for version in sorted(required):
        numeric = re.fullmatch(r"GLIBC_([0-9.]+)", version)
        if numeric is None or version_tuple(numeric[1]) > (2, 35, 0):
            unsupported.append(version)
    return {"required_versions": sorted(required), "weak_imports": weak_imports, "unsupported_required_versions": unsupported}
