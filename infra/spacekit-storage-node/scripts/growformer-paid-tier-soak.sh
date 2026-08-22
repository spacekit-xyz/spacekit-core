#!/usr/bin/env bash
# Growformer paid-tier entitlement soak (personal tier via dev payment flow).
#
# Usage:
#   ./scripts/growformer-paid-tier-soak.sh
#
# Prerequisites: spacekit on PATH (or SPACEKIT=), `spacekit network up`, spacekit init.

set -euo pipefail

SPACEKIT="${SPACEKIT:-spacekit}"
STORAGE_URL="${SPACEKIT_STORAGE_URL:-http://127.0.0.1:3030}"
CHANNEL="${GROWFORMER_SOAK_CHANNEL:-did:spacekit:channel:soak-growformer-paid:$(date +%s)}"
TIER="${GROWFORMER_SOAK_TIER:-personal}"
PERSONAL_PRICE="${GROWFORMER_SOAK_PERSONAL_PRICE:-20}"
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
    --title "Growformer Paid Soak" \
    --description "Paid-tier growformer entitlement test" 2>&1) || {
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

  log "Initiate pay for tier=${TIER}"
  local pay_out pending publisher price
  pay_out=$("$SPACEKIT" content pay --content-id "$cid" --tier "$TIER" 2>&1) || {
    fail "content pay --tier ${TIER}"
    echo "$pay_out"
    return
  }
  pending=$(echo "$pay_out" | sed -n 's/.*Pending: \(pending-[^ ]*\).*/\1/p' | head -1)
  price=$(echo "$pay_out" | sed -n 's/.*Amount: \([0-9.]*\) ASTRA.*/\1/p' | head -1)
  publisher=$(echo "$pay_out" | sed -n 's/.*ASTRA → \(did:[^ ]*\).*/\1/p' | head -1)
  if [[ -z "$pending" ]]; then
    fail "could not parse pending id from pay output"
    echo "$pay_out"
    return
  fi
  if [[ -z "$price" ]]; then
    price="$PERSONAL_PRICE"
  fi
  if [[ -z "$publisher" ]]; then
    publisher="did:spacekit:publisher"
  fi
  pass "pay quote pending=${pending:0:24}... amount=${price} ASTRA"

  log "Settle (${GROWFORMER_SOAK_SETTLE:-dev} path)"
  local settle_out tx="tx-growformer-paid-${cid:0:8}-$(date +%s)"
  if [[ "${GROWFORMER_SOAK_SETTLE:-dev}" == "router" ]]; then
    settle_out=$("$SPACEKIT" content settle \
      --pending-id "$pending" \
      --tx-hash "$tx" \
      --amount "$price" 2>&1) || {
      fail "content settle (router)"
      echo "$settle_out"
      return
    }
  else
    "$SPACEKIT" content record-payment \
      --reference "$tx" \
      --recipient "$publisher" \
      --scope "content:${cid}" \
      --amount "$price" >/dev/null || {
      fail "record-payment"
      return
    }
    "$SPACEKIT" content listen-settlements --once 2>&1 || true
    settle_out=$("$SPACEKIT" content pay \
      --content-id "$cid" \
      --tier "$TIER" \
      --pending-id "$pending" \
      --await-settlement 2>&1) || {
      fail "pay --await-settlement"
      echo "$settle_out"
      return
    }
  fi
  if echo "$settle_out" | grep -qi "Auto-completed\|Settled\|entitlement"; then
    pass "paid tier settlement"
  else
    fail "expected settlement completion"
    echo "$settle_out"
    return
  fi

  log "Verify grant tier=${TIER} (unlimited quota)"
  local access_out
  access_out=$("$SPACEKIT" content list-access 2>&1) || {
    fail "content list-access"
    echo "$access_out"
    return
  }
  if echo "$access_out" | grep -q "tier=${TIER}"; then
    pass "grant records tier=${TIER}"
  else
    fail "tier=${TIER} not in list-access output"
    echo "$access_out"
  fi
  if echo "$access_out" | grep -q "quota=unlimited"; then
    pass "personal tier has unlimited quota"
  else
    fail "expected quota=unlimited for paid tier"
    echo "$access_out"
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

  log "Embedded growformer exec --help (entitlement enforced)"
  local exec_out
  if exec_out=$("$SPACEKIT" agent exec -- --help 2>&1); then
    if echo "$exec_out" | grep -q "Usage: growformer"; then
      pass "agent exec shows growformer help after paid grant"
    else
      fail "growformer help not in exec output"
      echo "$exec_out" | head -20
    fi
  else
    fail "agent exec -- --help after paid grant"
    echo "$exec_out" | head -20
  fi

  log "Paid tier retained when free access attempted"
  local free_out free_status=0
  free_out=$("$SPACEKIT" content access --feature growformer 2>&1) || free_status=$?
  if echo "$free_out" | grep -qi "Tier: ${TIER}\|tier=${TIER}"; then
    pass "paid tier retained (free access did not downgrade)"
  elif [[ "$free_status" -ne 0 ]] && echo "$free_out" | grep -qi "requires payment"; then
    pass "free-tier access blocked while on paid tier"
  else
    fail "unexpected free access after paid grant"
    echo "$free_out"
  fi
}

check_health
run_soak

echo ""
echo "Growformer paid-tier soak: ${PASS} passed, ${FAIL} failed"
if [[ "$FAIL" -gt 0 ]]; then
  exit 1
fi
