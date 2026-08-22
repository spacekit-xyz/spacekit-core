#!/usr/bin/env bash
# Dependency audit for spacekit-compute-node / workspace.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"
if command -v cargo-audit >/dev/null 2>&1; then
  cargo audit
else
  echo "cargo-audit not installed; install with: cargo install cargo-audit"
  exit 1
fi
