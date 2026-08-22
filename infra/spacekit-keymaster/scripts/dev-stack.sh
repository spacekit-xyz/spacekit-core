#!/usr/bin/env bash
# Start/stop local SKKM stack without blocking the shell.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="${KEYMASTER_BIN_DIR:-$ROOT/target/release}"
COORD_PORT="${KEYMASTER_COORDINATOR_PORT:-8780}"
REG_PORT="${KEYMASTER_REGISTRY_PORT:-8770}"
GUARD_BASE="${KEYMASTER_GUARDIAN_BASE_PORT:-8781}"
STORAGE_URL="${KEYMASTER_STORAGE_URL:-http://127.0.0.1:3030}"
PID_DIR="${KEYMASTER_PID_DIR:-/tmp/spacekit-keymaster}"
LOG_DIR="${KEYMASTER_LOG_DIR:-/tmp/spacekit-keymaster}"

export KEYMASTER_DEV="${KEYMASTER_DEV:-1}"
export KEYMASTER_COORDINATOR_URL="http://127.0.0.1:${COORD_PORT}"
export KEYMASTER_REGISTRY_URL="http://127.0.0.1:${REG_PORT}"

require_bin() {
  local name="$1"
  if [[ ! -x "$BIN/$name" ]]; then
    echo "missing $BIN/$name — run: cargo build --release" >&2
    exit 1
  fi
}

stop_stack() {
  mkdir -p "$PID_DIR"
  for f in "$PID_DIR"/*.pid; do
    [[ -f "$f" ]] || continue
    pid="$(cat "$f")"
    kill "$pid" 2>/dev/null || true
    rm -f "$f"
  done
  pkill -f 'spacekit-keymaster-(coordinator|guardian|registry)' 2>/dev/null || true
}

wait_url() {
  local url="$1" tries="${2:-40}"
  for _ in $(seq 1 "$tries"); do
    if curl -sf "$url" >/dev/null 2>&1; then return 0; fi
    sleep 0.25
  done
  return 1
}

start_stack() {
  require_bin spacekit-keymaster-coordinator
  require_bin spacekit-keymaster-guardian
  require_bin spacekit-keymaster-registry

  stop_stack
  mkdir -p "$PID_DIR" "$LOG_DIR"

  nohup "$BIN/spacekit-keymaster-coordinator" \
    --port "$COORD_PORT" --storage-url "$STORAGE_URL" \
    >"$LOG_DIR/coordinator.log" 2>&1 &
  echo $! >"$PID_DIR/coordinator.pid"

  wait_url "$KEYMASTER_COORDINATOR_URL/v1/coordinator/info" || {
    echo "coordinator failed to start — see $LOG_DIR/coordinator.log" >&2
    exit 1
  }

  local ops=(meridian atlas vesper corona halcyon)
  for i in "${!ops[@]}"; do
    local port=$((GUARD_BASE + i))
    nohup "$BIN/spacekit-keymaster-guardian" \
      --port "$port" --operator "${ops[$i]}" \
      >"$LOG_DIR/guardian-${ops[$i]}.log" 2>&1 &
    echo $! >"$PID_DIR/guardian-${ops[$i]}.pid"
  done

  nohup "$BIN/spacekit-keymaster-registry" --port "$REG_PORT" \
    >"$LOG_DIR/registry.log" 2>&1 &
  echo $! >"$PID_DIR/registry.pid"

  wait_url "$KEYMASTER_REGISTRY_URL/v1/guardians" || {
    echo "registry failed to start — see $LOG_DIR/registry.log" >&2
    exit 1
  }

  local n
  n="$(curl -sf "$KEYMASTER_REGISTRY_URL/v1/guardians" | python3 -c 'import sys,json; print(len(json.load(sys.stdin)))')"
  echo "SKKM dev stack up (KEYMASTER_DEV=$KEYMASTER_DEV)"
  echo "  coordinator $KEYMASTER_COORDINATOR_URL"
  echo "  registry    $KEYMASTER_REGISTRY_URL"
  echo "  guardians   $n enrolled"
  echo "  logs        $LOG_DIR"
}

status_stack() {
  curl -sf "$KEYMASTER_COORDINATOR_URL/v1/coordinator/info" >/dev/null && echo "coordinator: ok" || echo "coordinator: down"
  curl -sf "$KEYMASTER_REGISTRY_URL/v1/guardians" >/dev/null && echo "registry: ok" || echo "registry: down"
}

cmd="${1:-start}"
case "$cmd" in
  start) start_stack ;;
  stop) stop_stack; echo "SKKM stack stopped" ;;
  restart) start_stack ;;
  status) status_stack ;;
  *)
    echo "usage: $0 {start|stop|restart|status}" >&2
    exit 1
    ;;
esac
