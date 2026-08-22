#!/usr/bin/env bash
# Minimal HTTP smoke after deploy (step 8 drills).
set -euo pipefail
HOST="${1:-127.0.0.1}"
PORT="${2:-8080}"
BASE="http://${HOST}:${PORT}"
for path in /health /status /v1/node/identity; do
  echo "GET ${BASE}${path}"
  curl -fsS "${BASE}${path}" | head -c 400
  echo ""
done
