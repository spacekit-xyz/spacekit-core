#!/usr/bin/env bash
# Growformer library-in-CLI entitlement soak (GROWFORMER_SPEC Phase 4).
#
# Usage:
#   ./scripts/growformer-access-soak.sh
#
# Prerequisites: spacekit on PATH (or SPACEKIT=), `spacekit network up`, spacekit init.

set -euo pipefail

SPACEKIT="${SPACEKIT:-spacekit}"
STORAGE_URL="${SPACEKIT_STORAGE_URL:-http://127.0.0.1:3030}"
CHANNEL="${GROWFORMER_SOAK_CHANNEL:-did:spacekit:channel:soak-growformer:$(date +%s)}"
PASS=0
FAIL=0

log() { echo "==> $*"; }
pass() { PASS=$((PASS + 1)); echo "PASS: $*"; }
fail() { FAIL=$((FAIL + 1)); echo "FAIL: $*" >&2; }

check_health() {
  if ! curl -sf "${STORAGE_URL}/api/agentic/health" >/dev/null 2>&1; then
    fail "storage not reachable at ${STORAGE_URL} (run: spacekit network up)"
    exit 1
  fi
  pass "storage health OK"
}

run_soak() {
  log "Publish growformer licensed feature"
  local pub_out
  pub_out=$("$SPACEKIT" content publish-feature \
    --channel "$CHANNEL" \
    --feature growformer \
    --title "Growformer Soak" \
    --description "Library-embedded growformer entitlement test" 2>&1) || {
    fail "publish-feature"
    echo "$pub_out"
    return
  }

  local cid
  cid=$(echo "$pub_out" | sed -n 's/.*Content ID: \([0-9a-f]*\).*/\1/p' | head -1)
  if [[ -z "$cid" ]]; then
    fail "could not parse Content ID from publish-feature"
    echo "$pub_out"
    return
  fi
  pass "published feature content_id=${cid:0:16}..."
  export GROWFORMER_CONTENT_ID="$cid"

  log "Grant access via --feature growformer"
  local access_out
  access_out=$("$SPACEKIT" content access --feature growformer 2>&1) || {
    fail "content access --feature growformer"
    echo "$access_out"
    return
  }
  if echo "$access_out" | grep -qi "entitlement granted\|Tier:"; then
    pass "feature access grant"
  else
    fail "expected entitlement grant message"
    echo "$access_out"
  fi

  log "Embedded growformer exec --help (entitlement enforced)"
  local exec_out
  if exec_out=$("$SPACEKIT" agent exec -- --help 2>&1); then
    if echo "$exec_out" | grep -q "Usage: growformer"; then
      pass "agent exec shows growformer help"
    else
      fail "growformer help not in exec output"
      echo "$exec_out" | head -20
    fi
  else
    fail "agent exec -- --help"
    echo "$exec_out" | head -20
  fi

  log "Install record present"
  local installs_out
  installs_out=$("$SPACEKIT" content installs 2>&1) || {
    fail "content installs"
    echo "$installs_out"
    return
  }
  if echo "$installs_out" | grep -q "${cid:0:16}"; then
    pass "content_installs lists feature"
  else
    fail "install not found for content id"
    echo "$installs_out"
  fi
}

check_health
run_soak

echo ""
echo "Growformer soak: ${PASS} passed, ${FAIL} failed"
if [[ "$FAIL" -gt 0 ]]; then
  exit 1
fi
