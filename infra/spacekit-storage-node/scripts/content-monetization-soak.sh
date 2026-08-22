#!/usr/bin/env bash
# Content monetization E2E soak (CLI against local network).
#
# Usage:
#   ./scripts/content-monetization-soak.sh dev     # record-payment + inbox auto-complete
#   ./scripts/content-monetization-soak.sh router # SpaceKit Pay verify → storage inbox (no record-payment)
#   ./scripts/content-monetization-soak.sh live    # requires SPACEKIT_ENTITLEMENT_CONTRACT_ID
#
# Prerequisites: spacekit on PATH, `spacekit network up` in another terminal.

set -euo pipefail

MODE="${1:-dev}"
SPACEKIT="${SPACEKIT:-spacekit}"
STORAGE_URL="${SPACEKIT_STORAGE_URL:-http://127.0.0.1:3030}"
COMPUTE_URL="${SPACEKIT_COMPUTE_URL:-http://127.0.0.1:8545}"
SOAK_DIR="${SOAK_DIR:-/tmp/spacekit-content-soak-$$}"
FIXTURE="${SOAK_DIR}/fixture.txt"
VIEW_OK_PATTERN='Content access granted|Saved to|retrieved|downloaded|Size:'
PASS=0
FAIL=0

log() { echo "==> $*"; }
pass() { PASS=$((PASS + 1)); echo "PASS: $*"; }
fail() { FAIL=$((FAIL + 1)); echo "FAIL: $*" >&2; }

mkdir -p "$SOAK_DIR"
echo "soak-$(date +%s)" >"$FIXTURE"

check_health() {
  if ! curl -sf "${STORAGE_URL}/api/agentic/health" >/dev/null 2>&1; then
    fail "storage not reachable at ${STORAGE_URL} (run: spacekit network up)"
    exit 1
  fi
  pass "storage health OK"
}

run_dev_h2() {
  log "H2 PPV dev chain (publish → pay → record-payment → await → view)"
  local channel="did:spacekit:channel:soak:$(date +%s)"
  local pub_out
  pub_out=$("$SPACEKIT" content publish \
    --channel "$channel" \
    --file "$FIXTURE" \
    --title "Soak PPV" \
    --pricing pay_per_view \
    --price 10 2>&1) || { fail "publish"; echo "$pub_out"; return; }

  local cid
  cid=$(echo "$pub_out" | sed -n 's/.*Content ID: \([0-9a-f]*\).*/\1/p' | head -1)
  if [[ -z "$cid" ]]; then
    fail "could not parse Content ID from publish output"
    return
  fi
  pass "published content_id=${cid:0:16}..."

  local pay_out pending
  pay_out=$("$SPACEKIT" content pay --content-id "$cid" 2>&1) || { fail "content pay"; echo "$pay_out"; return; }
  pending=$(echo "$pay_out" | sed -n 's/.*Pending: \(pending-[^ ]*\).*/\1/p' | head -1)
  if [[ -z "$pending" ]]; then
    fail "could not parse pending id"
    return
  fi

  local publisher
  publisher=$(echo "$pay_out" | sed -n 's/.*ASTRA → \(did:[^ ]*\).*/\1/p' | head -1)
  if [[ -z "$publisher" ]]; then
    publisher="did:spacekit:publisher"
  fi

  "$SPACEKIT" content record-payment \
    --reference "tx-soak-${cid:0:8}" \
    --recipient "$publisher" \
    --scope "content:${cid}" \
    --amount 10 >/dev/null || { fail "record-payment"; return; }

  local complete_out
  "$SPACEKIT" content listen-settlements --once 2>&1 || true
  complete_out=$("$SPACEKIT" content pay --content-id "$cid" --pending-id "$pending" --await-settlement 2>&1) || {
    fail "pay --await-settlement"
    echo "$complete_out"
    return
  }
  if echo "$complete_out" | grep -q "Auto-completed"; then
    pass "inbox auto-complete"
  else
    fail "expected Auto-completed in output"
    echo "$complete_out"
  fi

  local view_out="${SOAK_DIR}/view-out.txt"
  if "$SPACEKIT" content view --content-id "$cid" --output "$view_out" 2>&1 | grep -qE "$VIEW_OK_PATTERN"; then
    [[ -s "$view_out" ]] && pass "view after grant" || fail "view output empty"
  else
    fail "view after grant"
  fi
}

run_router_h2() {
  log "H2 PPV router path (publish → pay → settle via /v1/payments/verify → inbox)"
  local channel="did:spacekit:channel:soak-router:$(date +%s)"
  local pub_out
  pub_out=$("$SPACEKIT" content publish \
    --channel "$channel" \
    --file "$FIXTURE" \
    --title "Soak Router PPV" \
    --pricing pay_per_view \
    --price 10 2>&1) || { fail "router publish"; echo "$pub_out"; return; }

  local cid
  cid=$(echo "$pub_out" | sed -n 's/.*Content ID: \([0-9a-f]*\).*/\1/p' | head -1)
  [[ -n "$cid" ]] || { fail "parse content id"; return; }
  pass "published content_id=${cid:0:16}..."

  local pay_out pending
  pay_out=$("$SPACEKIT" content pay --content-id "$cid" 2>&1) || { fail "content pay"; echo "$pay_out"; return; }
  pending=$(echo "$pay_out" | sed -n 's/.*Pending: \(pending-[^ ]*\).*/\1/p' | head -1)
  [[ -n "$pending" ]] || { fail "parse pending id"; return; }

  local tx="tx-router-${cid:0:8}-$(date +%s)"
  local settle_out
  settle_out=$("$SPACEKIT" content settle \
    --pending-id "$pending" \
    --tx-hash "$tx" \
    --amount 10 2>&1) || {
    fail "content settle (router path)"
    echo "$settle_out"
    return
  }
  if echo "$settle_out" | grep -qi "Settled\|entitlement\|Auto-completed"; then
    pass "router settle → grant"
  else
    fail "unexpected settle output"
    echo "$settle_out"
    return
  fi

  local view_out="${SOAK_DIR}/view-router-out.txt"
  if "$SPACEKIT" content view --content-id "$cid" --output "$view_out" 2>&1 | grep -qE "$VIEW_OK_PATTERN"; then
    [[ -s "$view_out" ]] && pass "view after router settle" || fail "view output empty"
  else
    fail "view after router settle"
  fi
}

run_live_h2() {
  if [[ -z "${SPACEKIT_ENTITLEMENT_CONTRACT_ID:-}" ]]; then
    fail "SPACEKIT_ENTITLEMENT_CONTRACT_ID required for live mode"
    return
  fi
  if ! curl -sf "${COMPUTE_URL}/health" >/dev/null 2>&1; then
    fail "compute not reachable at ${COMPUTE_URL}"
    return
  fi
  pass "compute health OK (live mode)"

  log "H2 PPV live chain (publish → pay → settle with tx)"
  local channel="did:spacekit:channel:soak-live:$(date +%s)"
  local pub_out
  pub_out=$("$SPACEKIT" content publish \
    --channel "$channel" \
    --file "$FIXTURE" \
    --title "Soak Live" \
    --pricing pay_per_view \
    --price 10 2>&1) || { fail "live publish"; return; }

  local cid
  cid=$(echo "$pub_out" | sed -n 's/.*Content ID: \([0-9a-f]*\).*/\1/p' | head -1)
  [[ -n "$cid" ]] || { fail "parse content id"; return; }

  local tx="${SOAK_TX_HASH:-tx-soak-live-$(date +%s)}"
  local settle_out
  settle_out=$("$SPACEKIT" content pay --content-id "$cid" --tx-hash "$tx" --amount 10 2>&1) || {
    fail "live pay+settle (check contract + buyer balance)"
    echo "$settle_out"
    return
  }
  if echo "$settle_out" | grep -qi "settled\|entitlement"; then
    pass "live settle"
  else
    fail "live settle output unexpected"
    echo "$settle_out"
  fi
}

run_h1_free() {
  log "H1 free content"
  local channel="did:spacekit:channel:free:$(date +%s)"
  local pub_out
  pub_out=$("$SPACEKIT" content publish \
    --channel "$channel" \
    --file "$FIXTURE" \
    --title "Soak Free" \
    --pricing free 2>&1) || { fail "free publish"; return; }
  local cid
  cid=$(echo "$pub_out" | sed -n 's/.*Content ID: \([0-9a-f]*\).*/\1/p' | head -1)
  [[ -n "$cid" ]] || { fail "parse free content id"; return; }
  if "$SPACEKIT" content view --content-id "$cid" 2>&1 | grep -qE "$VIEW_OK_PATTERN"; then
    pass "free view"
  else
    fail "free view blocked"
  fi
}

main() {
  log "content monetization soak mode=${MODE} dir=${SOAK_DIR}"
  check_health
  run_h1_free
  case "$MODE" in
    dev) run_dev_h2 ;;
    router) run_router_h2 ;;
    live) run_live_h2 ;;
    *)
      echo "usage: $0 dev|router|live" >&2
      exit 2
      ;;
  esac
  echo ""
  echo "Soak summary: ${PASS} passed, ${FAIL} failed"
  [[ "$FAIL" -eq 0 ]]
}

main "$@"
