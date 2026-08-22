#!/usr/bin/env python3
"""Fail when a RustSec vulnerability is present in RouteKit's production graph."""

import json
import subprocess
import sys


def run(command: list[str], allow_failure: bool = False) -> subprocess.CompletedProcess[str]:
    result = subprocess.run(command, check=False, capture_output=True, text=True)
    if result.returncode and not allow_failure:
        sys.stderr.write(result.stderr)
        raise SystemExit(result.returncode)
    return result


tree = run(
    [
        "cargo",
        "tree",
        "-p",
        "routekit",
        "--edges",
        "normal",
        "--prefix",
        "none",
        "--format",
        "{p}",
    ]
)
production_packages = {line.strip() for line in tree.stdout.splitlines() if line.strip()}

audit = run(["cargo", "audit", "--json"], allow_failure=True)
try:
    report = json.loads(audit.stdout)
except json.JSONDecodeError as error:
    sys.stderr.write(audit.stderr)
    raise SystemExit(f"cargo audit did not return JSON: {error}")

affected: list[str] = []
for item in report.get("vulnerabilities", {}).get("list", []):
    package = item["package"]
    package_id = f'{package["name"]} v{package["version"]}'
    if package_id in production_packages:
        advisory = item["advisory"]
        affected.append(f'{advisory["id"]}: {package_id}: {advisory["title"]}')

if affected:
    print("RouteKit production dependency vulnerabilities:", file=sys.stderr)
    for finding in affected:
        print(f"- {finding}", file=sys.stderr)
    raise SystemExit(1)

print(f"RouteKit production graph clean ({len(production_packages)} packages checked).")
