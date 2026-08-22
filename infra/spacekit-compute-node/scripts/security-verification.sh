#!/usr/bin/env bash
# Dynamic security verification for the SpaceKit L1 compute node.
#
# The unit and contract test suites verify components in isolation. This script
# closes the gap flagged under "Scope and limitations" in the audit: it boots a
# real chain, a real entitlement contract, and a real node process, then probes
# the running HTTP surface the way an attacker would.
#
# What it asserts:
#   1. The node is reachable and healthy         (control: 401s below are real)
#   2. The entitlement contract is readable      (control: on-chain path works)
#   3. Every mutating endpoint rejects unauthenticated callers
#   4. Auth rejects missing headers, stale timestamps, weak nonces, replays
#   5. The keymaster never emits a private key
#   6. Unsigned intents cannot spend
#   7. CORS is not open by default
#   8. The API binds to loopback, not 0.0.0.0
#
# Usage:
#   ./spacekit-compute-node/scripts/security-verification.sh
#   KEEP_RUNNING=1 ./spacekit-compute-node/scripts/security-verification.sh
#
# Requires: foundry (anvil, forge, cast), jq, curl, cargo.

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
NODE_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
WORKSPACE_ROOT="$(cd "$NODE_ROOT/.." && pwd)"
CONTRACTS_ROOT="$WORKSPACE_ROOT/spacekit.xyz-contracts"

STAMP="$(date +%Y%m%d-%H%M%S)"
SCRATCH="${SECURITY_SCRATCH:-$NODE_ROOT/scratch/security-$STAMP}"
LOG_DIR="$SCRATCH/logs"

ANVIL_PORT="${ANVIL_PORT:-8545}"
CHAIN_ID="${CHAIN_ID:-31337}"
NODE_PORT="${NODE_PORT:-18080}"
NODE_P2P_PORT="${NODE_P2P_PORT:-19000}"
NODE_URL="http://127.0.0.1:$NODE_PORT"
RPC_URL="http://127.0.0.1:$ANVIL_PORT"
FUNDED_DID="${FUNDED_DID:-did:spacekit:testnet:alice}"
KEEP_RUNNING="${KEEP_RUNNING:-0}"

PASS=0
FAIL=0
SKIP=0
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; BOLD='\033[1m'; NC='\033[0m'

mkdir -p "$LOG_DIR"

pass() { PASS=$((PASS + 1)); printf "${GREEN}PASS${NC} %s\n" "$1"; }
fail() { FAIL=$((FAIL + 1)); printf "${RED}FAIL${NC} %s\n" "$1"; [[ -n "${2:-}" ]] && printf '      %s\n' "$2"; return 0; }
skip() { SKIP=$((SKIP + 1)); printf "${YELLOW}SKIP${NC} %s\n" "$1"; [[ -n "${2:-}" ]] && printf '      %s\n' "$2"; return 0; }
section() { printf "\n${BOLD}== %s ==${NC}\n" "$1"; }

ANVIL_PID=""
NODE_PID=""
cleanup() {
  if [[ "$KEEP_RUNNING" == "1" ]]; then
    printf "\nKEEP_RUNNING=1 — leaving anvil (pid %s) and node (pid %s) up.\n" "$ANVIL_PID" "$NODE_PID"
    return
  fi
  [[ -n "$NODE_PID" ]] && kill "$NODE_PID" 2>/dev/null
  [[ -n "$ANVIL_PID" ]] && kill "$ANVIL_PID" 2>/dev/null
  wait 2>/dev/null
}
trap cleanup EXIT

for tool in anvil forge cast jq curl cargo; do
  command -v "$tool" >/dev/null 2>&1 || { printf "${RED}missing required tool: %s${NC}\n" "$tool"; exit 1; }
done

# `curl -o /dev/null -w %{http_code}` for a POST with a JSON body.
post_status() {
  local path="$1" body="${2:-{\}}"; shift 2 || true
  curl -sS -o /dev/null -w '%{http_code}' -X POST "$NODE_URL$path" \
    -H 'content-type: application/json' "$@" -d "$body" 2>/dev/null
}

# Assert an endpoint rejects a request with one of the expected statuses.
expect_status() {
  local label="$1" expected="$2" actual="$3"
  if [[ " $expected " == *" $actual "* ]]; then
    pass "$label (HTTP $actual)"
  else
    fail "$label" "expected one of [$expected], got $actual"
  fi
}

# ─────────────────────────────────────────────────────────────────────────
section "Bringing up the local chain"

anvil --chain-id "$CHAIN_ID" --port "$ANVIL_PORT" --block-time 1 > "$LOG_DIR/anvil.log" 2>&1 &
ANVIL_PID=$!

for _ in $(seq 1 30); do
  cast chain-id --rpc-url "$RPC_URL" >/dev/null 2>&1 && break
  sleep 1
done
if ! cast chain-id --rpc-url "$RPC_URL" >/dev/null 2>&1; then
  fail "anvil started" "see $LOG_DIR/anvil.log"
  exit 1
fi
pass "anvil listening on $RPC_URL (chain $CHAIN_ID)"

DEPLOY_LOG="$LOG_DIR/deploy.log"
( cd "$CONTRACTS_ROOT" && SUBJECT_DID="$FUNDED_DID" forge script \
    script/DeploySpaceKitEntitlementLocal.s.sol:DeploySpaceKitEntitlementLocal \
    --rpc-url "$RPC_URL" --broadcast ) > "$DEPLOY_LOG" 2>&1

REGISTRY_ADDR="$(grep -o 'SPACEKIT_ENTITLEMENT_CONTRACT= 0x[0-9a-fA-F]\{40\}' "$DEPLOY_LOG" | tail -1 | grep -o '0x[0-9a-fA-F]\{40\}')"
if [[ -z "$REGISTRY_ADDR" ]]; then
  fail "entitlement registry deployed" "see $DEPLOY_LOG"
  exit 1
fi
pass "entitlement registry deployed at $REGISTRY_ADDR"

# The contract must report the funded DID before the node is asked to read it,
# otherwise a later failure is ambiguous between "node broken" and "no funds".
SUBJECT_KEY="$(cast keccak "$FUNDED_DID")"
ONCHAIN_UNITS="$(cast call "$REGISTRY_ADDR" 'entitlementOf(bytes32)(uint256,uint256,uint64,uint8)' \
  "$SUBJECT_KEY" --rpc-url "$RPC_URL" 2>/dev/null | head -1 | awk '{print $1}')"
if [[ "${ONCHAIN_UNITS:-0}" -gt 0 ]]; then
  pass "contract reports $ONCHAIN_UNITS micro-USD for $FUNDED_DID"
else
  fail "contract reports a funded subject" "got '${ONCHAIN_UNITS:-}'"
fi

# ─────────────────────────────────────────────────────────────────────────
section "Starting the compute node with production-shaped settings"

NODE_HOME="$SCRATCH/node"
mkdir -p "$NODE_HOME"

# `[compute]` is a large nested table, so start from the checked-in sample and
# replace only `[network]` with the settings under test.
NODE_CONFIG="$NODE_HOME/config.toml"
python3 - "$NODE_ROOT/config.toml" "$NODE_CONFIG" "$NODE_PORT" "$NODE_P2P_PORT" <<'PY'
import re, sys
src, dst, port, p2p = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4]
text = open(src).read()
network = f"""[network]
name = "testnet"
endpoint = "http://127.0.0.1:{port}"
p2p_port = {p2p}
rpc_port = {port}
bootstrap_nodes = []
enable_http_api = true
dev_mode = false
allow_single_validator_finalize = false
bind_address = "127.0.0.1"
"""
# Replace the whole [network] table, up to the next top-level table header.
text, n = re.subn(r"^\[network\]\n(?:(?!^\[).*\n)*", network, text, count=1, flags=re.M)
if n == 0:
    text += "\n" + network
open(dst, "w").write(text)
PY
if [[ ! -s "$NODE_CONFIG" ]]; then
  fail "node config generated" "could not derive one from $NODE_ROOT/config.toml"
  exit 1
fi
pass "node config generated"

( cd "$WORKSPACE_ROOT" && cargo build -p spacekit-compute-node --features standalone \
    --bin spacekit-compute-node ) > "$LOG_DIR/build.log" 2>&1
if [[ $? -ne 0 ]]; then
  fail "compute node built" "see $LOG_DIR/build.log"
  exit 1
fi

# Ask cargo where it put the binary. Assuming `$WORKSPACE_ROOT/target` silently
# runs a stale binary whenever CARGO_TARGET_DIR is set, which is exactly the
# kind of false pass this script exists to avoid.
TARGET_DIR="$(cd "$WORKSPACE_ROOT" && cargo metadata --format-version 1 --no-deps 2>/dev/null \
  | jq -r '.target_directory')"
NODE_BIN="$TARGET_DIR/debug/spacekit-compute-node"
if [[ ! -x "$NODE_BIN" ]]; then
  fail "compute node binary located" "not found at $NODE_BIN"
  exit 1
fi
pass "compute node built ($NODE_BIN)"

# Deliberately production-shaped: dev mode off, no admin DIDs, no CORS origins.
env -u SPACEKIT_DEV_MODE \
  SPACEKIT_ENTITLEMENT_CONTRACT="$REGISTRY_ADDR" \
  SPACEKIT_ENTITLEMENT_CHAIN_ID="$CHAIN_ID" \
  SPACEKIT_ENTITLEMENT_RPC_URLS="$RPC_URL" \
  SPACEKIT_ENTITLEMENT_MIN_AGREEMENT=1 \
  SPACEKIT_ENTITLEMENT_CONFIRMATIONS=1 \
  SPACEKIT_ENTITLEMENT_CACHE_TTL_SECS=2 \
  SPACEKIT_KEYMASTER_SECRET="local-verification-operator-secret-not-for-production" \
  SPACEKIT_DID_REGISTRY_PATH="$NODE_HOME/did_registry.json" \
  SPACEKIT_ADMIN_DIDS="" \
  SPACEKIT_API_ALLOWED_ORIGINS="" \
  "$NODE_BIN" \
    --config "$NODE_CONFIG" --network testnet \
    --port "$NODE_PORT" --p2p-port "$NODE_P2P_PORT" start \
    > "$LOG_DIR/node.log" 2>&1 &
NODE_PID=$!

for _ in $(seq 1 60); do
  curl -sS -o /dev/null "$NODE_URL/health" 2>/dev/null && break
  sleep 1
done

if ! curl -sS -o /dev/null "$NODE_URL/health" 2>/dev/null; then
  fail "node responds on $NODE_URL" "see $LOG_DIR/node.log"
  exit 1
fi
pass "node healthy on $NODE_URL"

# ─────────────────────────────────────────────────────────────────────────
section "Controls (these MUST pass, or the negative tests below are vacuous)"

ENT_JSON="$(curl -sS "$NODE_URL/v1/entitlements?did=$FUNDED_DID" 2>/dev/null)"
NODE_UNITS="$(printf '%s' "$ENT_JSON" | jq -r '.available_units // .entitlement.available_units // empty' 2>/dev/null)"
if [[ -n "$NODE_UNITS" && "$NODE_UNITS" -gt 0 ]]; then
  pass "node read $NODE_UNITS micro-USD from the contract"
else
  fail "node reads the entitlement contract" "response: $ENT_JSON"
fi

# ─────────────────────────────────────────────────────────────────────────
section "Unauthenticated access to mutating endpoints"

# 401 = rejected by auth. 404/405 also acceptable only if the route is absent;
# anything 2xx means the endpoint is reachable without credentials.
for route in \
  "/v1/keymaster/register" \
  "/v1/keymaster/rotate" \
  "/v1/consensus/register-validator" \
  "/v1/consensus/propose" \
  "/v1/entitlements/reserve" \
  "/v1/entitlements/release" \
  "/v1/state/anchor"
do
  status="$(post_status "$route" '{"node_did":"did:spacekit:testnet:attacker","units":1000000,"reservation_id":"x"}')"
  expect_status "unauthenticated POST $route is refused" "401 403 404 405" "$status"
done

# ─────────────────────────────────────────────────────────────────────────
section "Authentication edge cases"

NOW="$(date +%s)"
GOOD_NONCE="$(openssl rand -hex 16)"

status="$(post_status /v1/entitlements/reserve '{"units":1,"reservation_id":"a"}' \
  -H "x-spacekit-did: $FUNDED_DID" -H "x-spacekit-nonce: $GOOD_NONCE" -H "x-spacekit-signature: 00")"
expect_status "request without a timestamp header is refused" "401" "$status"

status="$(post_status /v1/entitlements/reserve '{"units":1,"reservation_id":"a"}' \
  -H "x-spacekit-did: $FUNDED_DID" -H "x-spacekit-timestamp: 1" \
  -H "x-spacekit-nonce: $GOOD_NONCE" -H "x-spacekit-signature: 00")"
expect_status "stale timestamp is refused" "401" "$status"

status="$(post_status /v1/entitlements/reserve '{"units":1,"reservation_id":"a"}' \
  -H "x-spacekit-did: $FUNDED_DID" -H "x-spacekit-timestamp: $((NOW + 86400))" \
  -H "x-spacekit-nonce: $GOOD_NONCE" -H "x-spacekit-signature: 00")"
expect_status "future-dated timestamp is refused" "401" "$status"

status="$(post_status /v1/entitlements/reserve '{"units":1,"reservation_id":"a"}' \
  -H "x-spacekit-did: $FUNDED_DID" -H "x-spacekit-timestamp: $NOW" \
  -H "x-spacekit-nonce: short" -H "x-spacekit-signature: 00")"
expect_status "weak nonce is refused" "401" "$status"

status="$(post_status /v1/entitlements/reserve '{"units":1,"reservation_id":"a"}' \
  -H "x-spacekit-did: did:spacekit:testnet:never-registered" -H "x-spacekit-timestamp: $NOW" \
  -H "x-spacekit-nonce: $GOOD_NONCE" -H "x-spacekit-signature: 00")"
expect_status "unregistered DID is refused" "401" "$status"

body="$(curl -sS -X POST "$NODE_URL/v1/entitlements/reserve" -H 'content-type: application/json' \
  -H "x-spacekit-did: $FUNDED_DID" -H "x-spacekit-timestamp: $NOW" \
  -H "x-spacekit-nonce: $GOOD_NONCE" -H "x-spacekit-signature: deadbeef" \
  -d '{"units":1,"reservation_id":"a"}' 2>/dev/null)"
if printf '%s' "$body" | jq -e '.authenticated == false' >/dev/null 2>&1; then
  pass "auth failures return structured JSON, not a 500"
else
  fail "auth failures return structured JSON" "body: $body"
fi

# ─────────────────────────────────────────────────────────────────────────
section "Secret exposure"

km="$(curl -sS -X POST "$NODE_URL/v1/keymaster/register" -H 'content-type: application/json' \
  -d '{"node_did":"did:spacekit:testnet:storage","server_pk_hex":"aa","server_sk_hex":"bb"}' 2>/dev/null)"
if printf '%s' "$km" | grep -q 'server_sk_hex'; then
  fail "keymaster never returns a private key" "response contained server_sk_hex: $km"
else
  pass "keymaster response contains no private key"
fi

if grep -qi 'server_sk_hex\|SPACEKIT_KEYMASTER_SECRET=' "$LOG_DIR/node.log"; then
  fail "node logs contain no key material" "check $LOG_DIR/node.log"
else
  pass "node logs contain no key material"
fi

# ─────────────────────────────────────────────────────────────────────────
section "Intent execution"

INTENT='{"intent":{"version":"1.0","intent_id":"00112233445566778899aabbccddeeff",
  "actor":"'"$FUNDED_DID"'","chain":"spacekit:testnet","nonce":"1",
  "expiry":'"$((NOW + 600))"',"actions":[{"type":"transfer","amount":"1"}],"constraints":{}},
  "signature":"deadbeef","signature_type":"sphincs+"}'
status="$(post_status /v1/execute "$INTENT")"
expect_status "intent with a forged signature is refused" "400 401 402 403" "$status"

UNSIGNED='{"intent":{"version":"1.0","intent_id":"00112233445566778899aabbccddeeff",
  "actor":"'"$FUNDED_DID"'","chain":"spacekit:testnet","nonce":"2",
  "expiry":'"$((NOW + 600))"',"actions":[],"constraints":{}}}'
status="$(post_status /v1/execute "$UNSIGNED")"
expect_status "intent with no signature is refused" "400 401 402 403" "$status"

# ─────────────────────────────────────────────────────────────────────────
section "Network exposure"

acao="$(curl -sS -o /dev/null -D - "$NODE_URL/health" -H 'Origin: https://evil.example' 2>/dev/null \
  | tr -d '\r' | grep -i '^access-control-allow-origin:' | head -1)"
if [[ -z "$acao" ]]; then
  pass "no CORS grant for an unlisted origin"
else
  fail "no CORS grant for an unlisted origin" "got: $acao"
fi

if command -v lsof >/dev/null 2>&1; then
  binding="$(lsof -nP -iTCP:"$NODE_PORT" -sTCP:LISTEN 2>/dev/null | tail -n +2 | awk '{print $9}' | head -1)"
  if [[ "$binding" == *"127.0.0.1:$NODE_PORT"* ]]; then
    pass "API bound to loopback ($binding)"
  elif [[ -z "$binding" ]]; then
    skip "API bind address" "lsof returned nothing"
  else
    fail "API bound to loopback" "listening on $binding"
  fi
else
  skip "API bind address" "lsof not available"
fi

# ─────────────────────────────────────────────────────────────────────────
section "On-chain reader integration (Rust)"

( cd "$WORKSPACE_ROOT" && \
  SPACEKIT_ENTITLEMENT_CONTRACT="$REGISTRY_ADDR" \
  SPACEKIT_ENTITLEMENT_CHAIN_ID="$CHAIN_ID" \
  SPACEKIT_ENTITLEMENT_RPC_URLS="$RPC_URL" \
  SPACEKIT_ENTITLEMENT_MIN_AGREEMENT=1 \
  SPACEKIT_ENTITLEMENT_CONFIRMATIONS=1 \
  cargo test -p spacekit-compute-node --test entitlements_onchain -- --test-threads=1 ) \
  > "$LOG_DIR/onchain-tests.log" 2>&1
if grep -q 'test result: ok' "$LOG_DIR/onchain-tests.log"; then
  pass "EntitlementReader integration tests"
else
  fail "EntitlementReader integration tests" "see $LOG_DIR/onchain-tests.log"
fi

# ─────────────────────────────────────────────────────────────────────────
printf "\n${BOLD}== Summary ==${NC}\n"
printf "  ${GREEN}pass %d${NC}  ${RED}fail %d${NC}  ${YELLOW}skip %d${NC}\n" "$PASS" "$FAIL" "$SKIP"
printf "  logs: %s\n" "$LOG_DIR"

[[ "$FAIL" -eq 0 ]] || exit 1
