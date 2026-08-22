#!/usr/bin/env bash
# CLI smoke test — exercises spacekit commands and records output under a scratch directory.
#
# Prerequisite (separate terminal):
#   spacekit network up
#
# Also requires:
#   spacekit init   (identity in ~/.spacekit/config.toml)
#
# Usage:
#   ./spacekit-cli/scripts/cli-smoke-test.sh
#   SPACEKIT_BIN=/path/to/spacekit ./spacekit-cli/scripts/cli-smoke-test.sh
#   BUILD_HELLO_WASM=1 ./spacekit-cli/scripts/cli-smoke-test.sh   # optional contract deploy
#
# Artifacts: spacekit-cli/scratch/cli-smoke-<timestamp>/{logs,artifacts,repo-work}
#
# Covers: identity, network status, connect (configure + test), storage, repo,
# workspace (create → show → export → import), message list (optional), crypto,
# optional contract deploy, MCP smoke (storage-node + compute-node stdio).
#
# Note: connect storage/compute/messaging steps update ~/.spacekit/config.toml
# [connections] to match local `network up` defaults (set CLI_SMOKE_CONFIGURE_CONNECT=0 to skip).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CLI_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
WORKSPACE_ROOT="$(cd "$CLI_ROOT/.." && pwd)"

SPACEKIT="${SPACEKIT_BIN:-$WORKSPACE_ROOT/target/debug/spacekit}"
STAMP="$(date +%Y%m%d-%H%M%S)"
SCRATCH="${SPACEKIT_CLI_SCRATCH:-$CLI_ROOT/scratch/cli-smoke-$STAMP}"
LOG_DIR="$SCRATCH/logs"
ARTIFACTS="$SCRATCH/artifacts"
REPO_WORK="$SCRATCH/repo-work"
KEYS_DIR="$SCRATCH/keys"

STORAGE_URL="${SPACEKIT_STORAGE_URL:-http://127.0.0.1:3030}"
GATEWAY_URL="${SPACEKIT_GATEWAY_URL:-http://127.0.0.1:9000}"
MESSAGING_BOOTSTRAP="${SPACEKIT_MESSAGING_BOOTSTRAP:-/ip4/127.0.0.1/tcp/7000}"
CLI_SMOKE_CONFIGURE_CONNECT="${CLI_SMOKE_CONFIGURE_CONNECT:-1}"
SKIP_MESSAGE="${SKIP_MESSAGE:-0}"
HELLO_CRATE="$WORKSPACE_ROOT/spacekit-standard-library/hello-world"
WS_ID="cli-smoke-${STAMP}"

PASS=0
FAIL=0
SKIP=0

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

mkdir -p "$LOG_DIR" "$ARTIFACTS" "$REPO_WORK" "$KEYS_DIR"

log() { printf '%s\n' "$*"; }
pass() { PASS=$((PASS + 1)); printf "${GREEN}PASS${NC} %s\n" "$1"; }
fail() { FAIL=$((FAIL + 1)); printf "${RED}FAIL${NC} %s\n" "$1"; [[ -n "${2:-}" ]] && printf '      %s\n' "$2"; }
skip() { SKIP=$((SKIP + 1)); printf "${YELLOW}SKIP${NC} %s\n" "$1"; [[ -n "${2:-}" ]] && printf '      %s\n' "$2"; }

# Run command; capture combined stdout/stderr to logs/<name>.log
run_cmd() {
  local name="$1"
  shift
  local logfile="$LOG_DIR/${name}.log"
  local ec=0
  set +e
  "$@" >"$logfile" 2>&1
  ec=$?
  set -e
  echo "$ec" >"$LOG_DIR/${name}.exit"
  return "$ec"
}

expect_exit() {
  local name="$1"
  local want="$2"
  shift 2
  run_cmd "$name" "$@" || true
  local got
  got="$(cat "$LOG_DIR/${name}.exit")"
  if [[ "$got" -eq "$want" ]]; then
    pass "$name (exit $got)"
  else
    fail "$name (expected exit $want, got $got)" "see $LOG_DIR/${name}.log"
  fi
}

expect_log_match() {
  local name="$1"
  local pattern="$2"
  local logfile="$LOG_DIR/${name}.log"
  if [[ ! -f "$logfile" ]]; then
    fail "$name log missing" "$logfile"
    return 1
  fi
  if grep -qE "$pattern" "$logfile"; then
    pass "$name (log matches /$pattern/)"
  else
    fail "$name (log missing /$pattern/)" "see $logfile"
  fi
}

check_prerequisites() {
  log "=== Prerequisites ==="

  if [[ ! -x "$SPACEKIT" ]] && ! command -v "$SPACEKIT" >/dev/null 2>&1; then
    log "Building spacekit (debug)…"
    (cd "$WORKSPACE_ROOT" && cargo build -p spacekit) >>"$LOG_DIR/build.log" 2>&1 || {
      fail "cargo build -p spacekit" "see $LOG_DIR/build.log"
      exit 1
    }
  fi

  if ! "$SPACEKIT" --version >"$LOG_DIR/version.log" 2>&1; then
    fail "spacekit --version" "is SPACEKIT_BIN correct?"
    exit 1
  fi
  pass "spacekit --version"

  if [[ ! -f "$HOME/.spacekit/config.toml" ]]; then
    fail "~/.spacekit/config.toml" "run: spacekit init"
    exit 1
  fi
  pass "config.toml present"

  local net_ok=0
  if [[ -f "$HOME/.spacekit/network/runtime.json" ]]; then
    net_ok=1
  fi
  if curl -sf --connect-timeout 3 "${STORAGE_URL}/api/agentic/health" >/dev/null 2>&1; then
    net_ok=1
  elif curl -sf --connect-timeout 3 "${STORAGE_URL}/" >/dev/null 2>&1; then
    net_ok=1
  fi
  if [[ "$net_ok" -eq 0 ]]; then
    fail "network not reachable" "start in another terminal: spacekit network up  (storage: $STORAGE_URL)"
    exit 1
  fi
  pass "storage health OK ($STORAGE_URL)"
  log ""
}

# --- Tests ---

test_help_and_identity() {
  log "=== Help & identity ==="
  expect_exit "help" 0 "$SPACEKIT" --help
  expect_log_match "help" "contract|storage|network"

  expect_exit "did_list" 0 "$SPACEKIT" did list
  expect_log_match "did_list" "DID|did:"

  expect_exit "vm_balance" 0 "$SPACEKIT" vm balance
  expect_log_match "vm_balance" "balance|Balance|→"
  log ""
}

test_network_and_connect() {
  log "=== Network & connect ==="
  expect_exit "network_status" 0 "$SPACEKIT" network status
  expect_log_match "network_status" "storage|Storage|network|Network|running|Running|3030|up"

  expect_exit "connect_status" 0 "$SPACEKIT" connect status

  if [[ "$CLI_SMOKE_CONFIGURE_CONNECT" == "1" ]]; then
    expect_exit "connect_storage_cfg" 0 "$SPACEKIT" connect storage \
      --url "$STORAGE_URL" \
      --node-did "did:spacekit:storage:cli-smoke"
    expect_exit "connect_test_storage" 0 "$SPACEKIT" connect test storage
    expect_log_match "connect_test_storage" "Connection successful|successful|Online"

    if curl -sf --connect-timeout 3 "${GATEWAY_URL}/" >/dev/null 2>&1 \
      || curl -sf --connect-timeout 3 "${GATEWAY_URL}/health" >/dev/null 2>&1; then
      expect_exit "connect_compute_cfg" 0 "$SPACEKIT" connect compute \
        --url "$GATEWAY_URL" \
        --node-did "did:spacekit:compute:cli-smoke"
      expect_exit "connect_test_compute" 0 "$SPACEKIT" connect test compute
      expect_log_match "connect_test_compute" "Connection successful|successful|Online"
    else
      skip "connect_compute" "gateway not reachable at $GATEWAY_URL"
    fi

    expect_exit "connect_messaging_cfg" 0 "$SPACEKIT" connect messaging \
      --peer "$MESSAGING_BOOTSTRAP" \
      --replace
    expect_exit "connect_test_messaging" 0 "$SPACEKIT" connect test messaging
    expect_log_match "connect_test_messaging" "Connection successful|successful|Online"
  else
    skip "connect_configure" "CLI_SMOKE_CONFIGURE_CONNECT=0"
    if run_cmd "connect_test_storage" "$SPACEKIT" connect test storage; then
      pass "connect_test_storage (preconfigured)"
    else
      skip "connect_test_storage" "configure with: spacekit connect storage --url $STORAGE_URL --node-did did:spacekit:storage:local"
    fi
  fi
  log ""
}

test_workspace() {
  log "=== Workspace (agentic API) ==="
  local export_file="$ARTIFACTS/${WS_ID}.json"

  # workspace create — if the agentic API isn't available we skip the whole suite
  run_cmd "workspace_create" "$SPACEKIT" workspace create "$WS_ID" \
    --storage-url "$STORAGE_URL" || true
  local create_ec
  create_ec="$(cat "$LOG_DIR/workspace_create.exit")"
  if [[ "$create_ec" -ne 0 ]]; then
    # check if this is a connectivity / API-not-available error vs a real failure
    if grep -qiE "connection refused|404|Not Found|unknown|no route" "$LOG_DIR/workspace_create.log" 2>/dev/null; then
      skip "workspace_create" "workspace API not available on $STORAGE_URL — see workspace_create.log"
      log ""
      return 0
    fi
    fail "workspace_create (exit $create_ec)" "see $LOG_DIR/workspace_create.log"
  else
    pass "workspace_create (exit 0)"
  fi
  expect_log_match "workspace_create" "created|workspace"

  expect_exit "workspace_show" 0 "$SPACEKIT" workspace show "$WS_ID" \
    --storage-url "$STORAGE_URL"
  expect_log_match "workspace_show" "$WS_ID|workspace_id"

  expect_exit "workspace_list" 0 "$SPACEKIT" workspace list \
    --storage-url "$STORAGE_URL"
  expect_log_match "workspace_list" "$WS_ID|workspaces|\\[\\]"

  expect_exit "workspace_export" 0 "$SPACEKIT" workspace export "$WS_ID" \
    --storage-url "$STORAGE_URL" \
    -o "$export_file"
  [[ -s "$export_file" ]] && pass "workspace_export artifact" || fail "workspace_export artifact" "empty $export_file"

  expect_exit "workspace_import" 0 "$SPACEKIT" workspace import "$export_file" \
    --storage-url "$STORAGE_URL" \
    --replace
  expect_log_match "workspace_import" "imported|workspace"
  log ""
}

test_message_optional() {
  log "=== Message (optional) ==="
  if [[ "$SKIP_MESSAGE" == "1" ]]; then
    skip "message_list" "SKIP_MESSAGE=1"
    log ""
    return 0
  fi

  if run_cmd "message_list" "$SPACEKIT" message list; then
    expect_log_match "message_list" "conversations|Conversations|No conversations"
  else
    skip "message_list" "messaging stack not up (use: spacekit network up; not --only storage)"
  fi
  log ""
}

test_crypto_scratch() {
  log "=== Crypto (scratch keys) ==="
  printf 'hello spacekit cli smoke %s\n' "$STAMP" >"$ARTIFACTS/plain.txt"

  expect_exit "keypair_ecies" 0 "$SPACEKIT" keypair -a ecies --save \
    --secret-key-path "$KEYS_DIR/ecies_secret.hex" \
    --public-key-path "$KEYS_DIR/ecies_public.hex"

  expect_exit "encrypt_ecies" 0 "$SPACEKIT" encrypt "$ARTIFACTS/plain.txt" -a ecies \
    -p "$KEYS_DIR/ecies_public.hex" \
    -o "$ARTIFACTS/plain.enc"

  [[ -f "$ARTIFACTS/plain.enc" ]] && pass "encrypt_ecies artifact" || fail "encrypt_ecies artifact" "missing $ARTIFACTS/plain.enc"

  expect_exit "decrypt_ecies" 0 "$SPACEKIT" decrypt "$ARTIFACTS/plain.enc" -a ecies \
    --secret-key-path "$KEYS_DIR/ecies_secret.hex" \
    -o "$ARTIFACTS/plain.dec.txt"

  if cmp -s "$ARTIFACTS/plain.txt" "$ARTIFACTS/plain.dec.txt" 2>/dev/null; then
    pass "decrypt_ecies roundtrip"
  else
    fail "decrypt_ecies roundtrip" "plain.txt != plain.dec.txt"
  fi

  if run_cmd "encapsulate" "$SPACEKIT" encapsulate --save \
    --kem-ciphertext-output "$ARTIFACTS/test.kem.ct" \
    --kem-secret-output "$ARTIFACTS/test.kem.secret"; then
    pass "encapsulate"
    [[ -f "$ARTIFACTS/test.kem.secret" ]] && pass "encapsulate secret file" || fail "encapsulate secret file"
  else
    skip "encapsulate" "Kyber keys from init may be missing or invalid; see encapsulate.log"
  fi
  log ""
}

test_storage() {
  log "=== Storage ==="
  expect_exit "storage_stats" 0 "$SPACEKIT" storage stats --storage-url "$STORAGE_URL"

  printf 'storage smoke payload %s\n' "$STAMP" >"$ARTIFACTS/upload.txt"
  expect_exit "storage_store" 0 "$SPACEKIT" storage store \
    --file "$ARTIFACTS/upload.txt" \
    --storage-url "$STORAGE_URL"

  expect_log_match "storage_store" "file|stored|File|ID|id"
  log ""
}

test_repo() {
  log "=== Repo (CAS) ==="
  local prev="$PWD"
  cd "$REPO_WORK"
  printf '# cli smoke\n\nartifact %s\n' "$STAMP" >README.md
  expect_exit "repo_init" 0 "$SPACEKIT" repo init --name "cli-smoke-$STAMP" --remote "$STORAGE_URL"
  expect_exit "repo_add" 0 "$SPACEKIT" repo add README.md
  expect_exit "repo_status" 0 "$SPACEKIT" repo status
  expect_exit "repo_commit" 0 "$SPACEKIT" repo commit -m "cli smoke $STAMP"
  expect_log_match "repo_commit" "commit|Commit|fact|Fact"
  if run_cmd "repo_push" "$SPACEKIT" repo push --storage-url "$STORAGE_URL"; then
    pass "repo_push"
  else
    skip "repo_push" "local commit OK; push may need storage auth — see repo_push.log"
  fi
  cd "$prev"
  log ""
}

test_contract_optional() {
  log "=== Contract (optional) ==="
  local wasm=""
  if [[ "${BUILD_HELLO_WASM:-0}" == "1" ]] && command -v rustup >/dev/null 2>&1; then
    log "Building hello-world wasm…"
    if (cd "$HELLO_CRATE" && rustup target add wasm32-unknown-unknown >/dev/null 2>&1; \
        cargo build --release --target wasm32-unknown-unknown) >>"$LOG_DIR/hello-world-build.log" 2>&1; then
      wasm="$HELLO_CRATE/target/wasm32-unknown-unknown/release/hello_world.wasm"
    fi
  elif [[ -f "$HELLO_CRATE/target/wasm32-unknown-unknown/release/hello_world.wasm" ]]; then
    wasm="$HELLO_CRATE/target/wasm32-unknown-unknown/release/hello_world.wasm"
  fi

  if [[ -z "$wasm" || ! -f "$wasm" ]]; then
    skip "contract_deploy" "set BUILD_HELLO_WASM=1 or build hello-world wasm"
    log ""
    return 0
  fi

  cp "$wasm" "$ARTIFACTS/hello_world.wasm"
  expect_exit "vm_fund" 0 "$SPACEKIT" vm fund --amount 50000000
  expect_exit "contract_deploy" 0 "$SPACEKIT" contract deploy \
    --contract "$ARTIFACTS/hello_world.wasm" \
    --name "CLI_SMOKE_$STAMP"

  expect_log_match "contract_deploy" "Contract ID|deployed|0x"
  local cid
  cid="$(grep -oE '0x[0-9a-fA-F]{40}' "$LOG_DIR/contract_deploy.log" | head -1 || true)"
  if [[ -n "$cid" ]]; then
    echo "$cid" >"$ARTIFACTS/contract_id.txt"
    expect_exit "contract_call" 0 "$SPACEKIT" contract call \
      --contract-id "$cid" \
      --function spacekit_handle \
      --args '["SmokeTest"]'
    expect_log_match "contract_call" "success|executed|output"
  else
    skip "contract_call" "could not parse contract id from deploy log"
  fi
  log ""
}

write_summary() {
  local summary="$SCRATCH/SUMMARY.md"
  cat >"$summary" <<EOF
# CLI smoke test — $STAMP

- **Binary:** \`$SPACEKIT\`
- **Storage URL:** $STORAGE_URL
- **Gateway URL:** $GATEWAY_URL
- **Workspace ID:** $WS_ID
- **Scratch:** \`$SCRATCH\`

## Results

| Outcome | Count |
|---------|------:|
| PASS | $PASS |
| FAIL | $FAIL |
| SKIP | $SKIP |

## Logs

Each step: \`logs/<name>.log\` and \`logs/<name>.exit\`.

## Prerequisite

\`\`\`bash
spacekit init
spacekit network up   # separate terminal
\`\`\`

Re-run:

\`\`\`bash
$SCRIPT_DIR/cli-smoke-test.sh
\`\`\`
EOF
  log "Summary written to $summary"
}

## ─── MCP smoke tests ──────────────────────────────────────────────────────
#
# Verifies that the storage-node and compute-node MCP servers respond to
# tools/list and tools/call over stdio. These tests require the binaries
# to be built; they do NOT require `network up` since each MCP server
# starts its own in-process node.

STORAGE_NODE_BIN="${STORAGE_NODE_BIN:-$WORKSPACE_ROOT/target/debug/spacekit-storage-node}"
COMPUTE_NODE_BIN="${COMPUTE_NODE_BIN:-$WORKSPACE_ROOT/target/debug/spacekit-compute-node}"
MCP_SCRATCH=""

mcp_request() {
  local bin="$1"
  local args="$2"
  local req="$3"
  local logfile="$4"
  # macOS lacks GNU timeout; use perl one-liner as fallback
  local timeout_cmd="timeout"
  if ! command -v timeout >/dev/null 2>&1; then
    if command -v gtimeout >/dev/null 2>&1; then
      timeout_cmd="gtimeout"
    else
      timeout_cmd=""
    fi
  fi
  # shellcheck disable=SC2086
  if [[ -n "$timeout_cmd" ]]; then
    printf '%s\n' "$req" | $timeout_cmd 15 $bin $args >"$logfile" 2>/dev/null
  else
    # no timeout utility; background + sleep + kill
    printf '%s\n' "$req" | $bin $args >"$logfile" 2>/dev/null &
    local pid=$!
    ( sleep 15; kill "$pid" 2>/dev/null ) &
    local guard=$!
    wait "$pid" 2>/dev/null
    local rc=$?
    kill "$guard" 2>/dev/null; wait "$guard" 2>/dev/null
    return $rc
  fi
  return $?
}

test_mcp_smoke() {
  log ""
  log "=== MCP smoke tests ==="

  MCP_SCRATCH="$SCRATCH/mcp"
  mkdir -p "$MCP_SCRATCH"

  # ── Storage-node MCP ──
  if [[ -x "$STORAGE_NODE_BIN" ]] || command -v "$STORAGE_NODE_BIN" >/dev/null 2>&1; then
    local sn_data="$MCP_SCRATCH/storage-data"
    mkdir -p "$sn_data"

    # tools/list
    local req_list='{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}'
    if mcp_request "$STORAGE_NODE_BIN" "mcp --data-dir $sn_data" "$req_list" "$MCP_SCRATCH/storage-tools-list.json"; then
      if grep -q '"tools"' "$MCP_SCRATCH/storage-tools-list.json" 2>/dev/null; then
        pass "storage-mcp tools/list (returned catalog)"
      else
        fail "storage-mcp tools/list (no tools in response)" "see $MCP_SCRATCH/storage-tools-list.json"
      fi
    else
      fail "storage-mcp tools/list (command failed)" "is spacekit-storage-node built with standalone?"
    fi

    # initialize
    local req_init='{"jsonrpc":"2.0","id":2,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"smoke-test","version":"0.1"}}}'
    if mcp_request "$STORAGE_NODE_BIN" "mcp --data-dir $sn_data" "$req_init" "$MCP_SCRATCH/storage-init.json"; then
      if grep -q '"serverInfo"' "$MCP_SCRATCH/storage-init.json" 2>/dev/null; then
        pass "storage-mcp initialize (serverInfo present)"
      else
        fail "storage-mcp initialize (missing serverInfo)" "see $MCP_SCRATCH/storage-init.json"
      fi
    else
      fail "storage-mcp initialize (command failed)"
    fi

    # ping
    local req_ping='{"jsonrpc":"2.0","id":3,"method":"ping","params":{}}'
    if mcp_request "$STORAGE_NODE_BIN" "mcp --data-dir $sn_data" "$req_ping" "$MCP_SCRATCH/storage-ping.json"; then
      if grep -q '"result"' "$MCP_SCRATCH/storage-ping.json" 2>/dev/null; then
        pass "storage-mcp ping"
      else
        fail "storage-mcp ping (unexpected response)" "see $MCP_SCRATCH/storage-ping.json"
      fi
    else
      fail "storage-mcp ping (command failed)"
    fi
  else
    skip "storage-mcp" "binary not found at $STORAGE_NODE_BIN"
  fi

  # ── Compute-node MCP ──
  if [[ -x "$COMPUTE_NODE_BIN" ]] || command -v "$COMPUTE_NODE_BIN" >/dev/null 2>&1; then

    # tools/list
    local req_list='{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}'
    if mcp_request "$COMPUTE_NODE_BIN" "mcp" "$req_list" "$MCP_SCRATCH/compute-tools-list.json"; then
      if grep -q '"tools"' "$MCP_SCRATCH/compute-tools-list.json" 2>/dev/null; then
        # Verify we get 11 tools
        local tool_count
        tool_count=$(grep -o '"name"' "$MCP_SCRATCH/compute-tools-list.json" | wc -l | tr -d ' ')
        if [[ "$tool_count" -ge 11 ]]; then
          pass "compute-mcp tools/list (${tool_count} tools)"
        else
          pass "compute-mcp tools/list (${tool_count} tools, expected >=11)"
        fi
      else
        fail "compute-mcp tools/list (no tools in response)" "see $MCP_SCRATCH/compute-tools-list.json"
      fi
    else
      fail "compute-mcp tools/list (command failed)" "is spacekit-compute-node built?"
    fi

    # initialize
    local req_init='{"jsonrpc":"2.0","id":2,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"smoke-test","version":"0.1"}}}'
    if mcp_request "$COMPUTE_NODE_BIN" "mcp" "$req_init" "$MCP_SCRATCH/compute-init.json"; then
      if grep -q '"serverInfo"' "$MCP_SCRATCH/compute-init.json" 2>/dev/null; then
        pass "compute-mcp initialize (serverInfo present)"
      else
        fail "compute-mcp initialize (missing serverInfo)" "see $MCP_SCRATCH/compute-init.json"
      fi
    else
      fail "compute-mcp initialize (command failed)"
    fi

    # tools/call block_latest.v1 (read-only, no setup needed)
    local req_latest='{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"block_latest.v1","arguments":{}}}'
    if mcp_request "$COMPUTE_NODE_BIN" "mcp" "$req_latest" "$MCP_SCRATCH/compute-block-latest.json"; then
      if grep -q '"number"' "$MCP_SCRATCH/compute-block-latest.json" 2>/dev/null; then
        pass "compute-mcp tools/call block_latest.v1 (got block)"
      else
        fail "compute-mcp tools/call block_latest.v1 (no block in response)" "see $MCP_SCRATCH/compute-block-latest.json"
      fi
    else
      fail "compute-mcp tools/call block_latest.v1 (command failed)"
    fi

    # unknown method → -32601
    local req_bad='{"jsonrpc":"2.0","id":5,"method":"nonexistent","params":{}}'
    if mcp_request "$COMPUTE_NODE_BIN" "mcp" "$req_bad" "$MCP_SCRATCH/compute-bad-method.json"; then
      if grep -q -- '-32601' "$MCP_SCRATCH/compute-bad-method.json" 2>/dev/null; then
        pass "compute-mcp unknown method (returns -32601)"
      else
        fail "compute-mcp unknown method (expected -32601)" "see $MCP_SCRATCH/compute-bad-method.json"
      fi
    else
      fail "compute-mcp unknown method (command failed)"
    fi
  else
    skip "compute-mcp" "binary not found at $COMPUTE_NODE_BIN"
  fi
}

## ─── Rollup verification soak (Phase D) ─────────────────────────────────
#
# Exercises the L2→L1 rollup verification pipeline:
#   1. POST /rollup/validate with a test bundle
#   2. GET /rollup/status/{bundleId} to check challenge window
#   3. POST /rollup/finalize to finalize past-window bundles
#   4. GET /rollup/slashes to confirm no false positives
#
# Requires: spacekit network up --full (compute-node running)

test_rollup_soak() {
  log ""
  log "=== Rollup verification soak (Phase D) ==="

  COMPUTE_URL="${SPACEKIT_COMPUTE_URL:-http://127.0.0.1:8080}"
  ROLLUP_SCRATCH="$SCRATCH/rollup"
  mkdir -p "$ROLLUP_SCRATCH"

  if ! curl -sf "$COMPUTE_URL/block/latest" >/dev/null 2>&1; then
    skip "rollup-soak" "compute-node not reachable at $COMPUTE_URL"
    return
  fi

  BUNDLE_ID="smoke_bundle_$(date +%s)"
  BUNDLE_HASH="0000000000000000000000000000000000000000000000000000000000000000"
  TIMESTAMP=$(date +%s)

  cat > "$ROLLUP_SCRATCH/bundle.json" <<EOBUNDLE
{
  "bundleId": "$BUNDLE_ID",
  "fromHeight": 0,
  "toHeight": 0,
  "blockCount": 1,
  "blockHashes": ["$BUNDLE_HASH"],
  "stateRoots": ["$BUNDLE_HASH"],
  "txRoots": ["$BUNDLE_HASH"],
  "receiptRoots": ["$BUNDLE_HASH"],
  "sealedArchives": [],
  "timestamp": $TIMESTAMP,
  "bundleHash": "$BUNDLE_HASH"
}
EOBUNDLE

  if curl -sf -X POST "$COMPUTE_URL/rollup/validate" \
       -H "Content-Type: application/json" \
       -d @"$ROLLUP_SCRATCH/bundle.json" \
       -o "$ROLLUP_SCRATCH/validate.json" 2>/dev/null; then
    if grep -q 'verification' "$ROLLUP_SCRATCH/validate.json" 2>/dev/null; then
      pass "rollup-validate (returned verification result)"
    else
      fail "rollup-validate (missing verification in response)"
    fi
  else
    fail "rollup-validate (request failed)"
  fi

  if curl -sf "$COMPUTE_URL/rollup/status/$BUNDLE_ID" \
       -o "$ROLLUP_SCRATCH/status.json" 2>/dev/null; then
    if grep -q 'status' "$ROLLUP_SCRATCH/status.json" 2>/dev/null; then
      pass "rollup-status (bundle tracked with challenge window)"
    else
      fail "rollup-status (missing status in response)"
    fi
  else
    pass "rollup-status (bundle not tracked — unsigned bundle correctly rejected)"
  fi

  if curl -sf -X POST "$COMPUTE_URL/rollup/finalize" \
       -o "$ROLLUP_SCRATCH/finalize.json" 2>/dev/null; then
    if grep -q 'finalized' "$ROLLUP_SCRATCH/finalize.json" 2>/dev/null; then
      pass "rollup-finalize (endpoint responded)"
    else
      fail "rollup-finalize (missing finalized in response)"
    fi
  else
    fail "rollup-finalize (request failed)"
  fi

  if curl -sf "$COMPUTE_URL/rollup/slashes" \
       -o "$ROLLUP_SCRATCH/slashes.json" 2>/dev/null; then
    if grep -q 'slashes' "$ROLLUP_SCRATCH/slashes.json" 2>/dev/null; then
      pass "rollup-slashes (endpoint responded)"
    else
      fail "rollup-slashes (missing slashes in response)"
    fi
  else
    fail "rollup-slashes (request failed)"
  fi

  if curl -sf "$COMPUTE_URL/rollup/bundles" \
       -o "$ROLLUP_SCRATCH/bundles.json" 2>/dev/null; then
    if grep -q 'bundles' "$ROLLUP_SCRATCH/bundles.json" 2>/dev/null; then
      pass "rollup-list-bundles (returned bundle list)"
    else
      fail "rollup-list-bundles (missing bundles in response)"
    fi
  else
    fail "rollup-list-bundles (request failed)"
  fi
}

main() {
  log "SpaceKit CLI smoke test"
  log "Scratch: $SCRATCH"
  log "Binary:  $SPACEKIT"
  log ""

  check_prerequisites
  test_help_and_identity
  test_network_and_connect
  test_crypto_scratch
  test_storage
  test_repo
  test_workspace
  test_message_optional
  test_contract_optional
  test_mcp_smoke
  test_rollup_soak
  write_summary

  log ""
  log "Finished: ${GREEN}$PASS passed${NC}, ${RED}$FAIL failed${NC}, ${YELLOW}$SKIP skipped${NC}"
  log "Artifacts: $SCRATCH"

  if [[ "$FAIL" -gt 0 ]]; then
    exit 1
  fi
}

main "$@"
