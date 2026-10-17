#!/usr/bin/env python3
"""Select one exact test executable from Cargo's bounded compiler-artifact stream."""
import json
import pathlib
import sys

name = sys.argv[1]
found = []
for line in sys.stdin:
    event = json.loads(line)
    if event.get("reason") == "compiler-artifact" and event.get("target", {}).get("name") == name and event.get("executable"):
        found.append(event["executable"])
if len(found) != 1 or not pathlib.Path(found[0]).is_file():
    raise SystemExit(f"expected one {name} test executable; found {found}")
print(found[0])
