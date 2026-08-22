#!/usr/bin/env python3
"""Fail when network docs drift from the current CLI/profile implementation."""

from __future__ import annotations

import argparse
import os
from pathlib import Path
import re
import subprocess
import tempfile
import tomllib


PORT_ROW = re.compile(r"^\| `([a-z0-9_]+)` \| ([0-9]+) \|", re.MULTILINE)
REQUIRED_HELP = (
    ("network",),
    ("network", "init"),
    ("network", "up"),
    ("network", "doctor"),
    ("network", "test"),
    ("network", "reset"),
    ("network", "join"),
    ("network", "manifest", "keygen"),
    ("network", "manifest", "sign"),
    ("network", "manifest", "verify"),
    ("network", "config", "show"),
    ("network", "peers"),
)


def run(binary: Path, *args: str, env: dict[str, str] | None = None) -> str:
    completed = subprocess.run(
        [str(binary), *args],
        check=True,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env=env,
    )
    return completed.stdout


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--spacekit-bin", required=True, type=Path)
    parser.add_argument("--doc", required=True, type=Path)
    parser.add_argument("--fixture", type=Path)
    args = parser.parse_args()

    binary = args.spacekit_bin.resolve()
    document = args.doc.read_text()
    start = document.index("<!-- network-port-map:start -->")
    end = document.index("<!-- network-port-map:end -->", start)
    documented_ports = {
        name: int(port) for name, port in PORT_ROW.findall(document[start:end])
    }

    for command in REQUIRED_HELP:
        help_text = run(binary, *command, "--help")
        if "Usage:" not in help_text:
            raise SystemExit(f"missing Usage line for: spacekit {' '.join(command)}")

    with tempfile.TemporaryDirectory(prefix="spacekit-doc-check-") as temporary:
        home = Path(temporary)
        env = os.environ.copy()
        env["HOME"] = str(home)
        env["SPACEKIT_NETWORK_CONFIG"] = str(home / "network.toml")
        run(binary, "init", "--did", "did:spacekit:docs-check", env=env)
        run(binary, "network", "init", "--profile", "local", "--force", env=env)
        shown = run(binary, "network", "config", "show", env=env)
        profile_text = shown[shown.index("version =") :]
        generated_ports = tomllib.loads(profile_text)["ports"]

    if documented_ports != generated_ports:
        missing = sorted(set(generated_ports) - set(documented_ports))
        extra = sorted(set(documented_ports) - set(generated_ports))
        changed = sorted(
            key
            for key in set(documented_ports) & set(generated_ports)
            if documented_ports[key] != generated_ports[key]
        )
        raise SystemExit(
            "network port map drifted: "
            f"missing={missing}, extra={extra}, changed={changed}"
        )

    if args.fixture:
        run(binary, "network", "manifest", "verify", str(args.fixture.resolve()))

    print(
        f"network docs valid: {len(documented_ports)} ports and "
        f"{len(REQUIRED_HELP)} CLI help paths"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
