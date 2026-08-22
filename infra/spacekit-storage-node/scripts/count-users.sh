#!/usr/bin/env bash
# Count registered users / accounts across SpaceKit storage and website-api.
#
# IMPORTANT: Modern accounts (username + passkey) live in did_registry, NOT signups.
#   signups          = welcome-email flow only
#   did_registry     = POST /api/did/register (primary account count)
#   spacekit_users   = browser profile docs (per-user owner; admin query undercounts)
#   user_profiles    = legacy API profile bucket (often empty)
#
# Storage node:
#   STORAGE_NODE=http://127.0.0.1:3031 \
#   ADMIN_DID=did:spacekit:admin:website-api \
#   ./scripts/count-users.sh storage
#
# Lookup one username:
#   ./scripts/count-users.sh lookup astor
#
# Website-api:
#   WEBSITE_API=https://api.spacekit.xyz \
#   API_SECRET=your-x-api-secret \
#   ./scripts/count-users.sh website

set -euo pipefail

MODE="${1:-all}"
USERNAME="${2:-}"
STORAGE_NODE="${STORAGE_NODE:-http://127.0.0.1:3030}"
ADMIN_DID="${ADMIN_DID:-did:spacekit:admin:website-api}"
WEBSITE_API="${WEBSITE_API:-http://127.0.0.1:8080}"
API_SECRET="${API_SECRET:-}"
BASE="${STORAGE_NODE%/}"

DOC_QUERY='{"filters":[],"limit":50000,"offset":0,"sort_by":{"field":"updated_at","order":"Desc"}}'

storage_query_count() {
  local collection="$1"
  local out http_code
  out="$(mktemp)"
  http_code="$(curl -sS -o "${out}" -w '%{http_code}' -X POST \
    "${BASE}/query/documents/${collection}" \
    -H "Content-Type: application/json" \
    -H "Authorization: DID ${ADMIN_DID}" \
    -d "${DOC_QUERY}" 2>/dev/null)" || {
    echo "0 (query failed)"
    rm -f "${out}"
    return
  }
  if [[ "${http_code}" != "200" ]]; then
    echo "0 (HTTP ${http_code})"
    rm -f "${out}"
    return
  fi
  python3 - "${out}" <<'PY'
import json, sys
with open(sys.argv[1], encoding="utf-8") as f:
    data = json.load(f)
print(data.get("total_count", len(data.get("documents", []))))
PY
  rm -f "${out}"
}

lookup_username() {
  local name="${1:-}"
  name="$(echo "${name}" | tr '[:upper:]' '[:lower:]')"
  if [[ -z "${name}" ]]; then
    echo "Usage: $0 lookup <username>" >&2
    exit 1
  fi

  echo "Lookup: ${name} (did:spacekit:user:${name})"
  echo

  local out http_code
  out="$(mktemp)"
  http_code="$(curl -sS -o "${out}" -w '%{http_code}' \
    "${BASE}/api/documents/did_registry/${name}" \
    -H "Authorization: DID ${ADMIN_DID}" 2>/dev/null)" || http_code="000"

  if [[ "${http_code}" == "200" ]]; then
    echo "  did_registry:     ✅ found"
    python3 - "${out}" <<'PY'
import json, sys
with open(sys.argv[1], encoding="utf-8") as f:
    data = json.load(f)
doc = data.get("document") or data
body = doc.get("data") or doc
if isinstance(body, dict):
    for k in ("did", "username", "registered_at", "eth_address"):
        if k in body and body[k]:
            print(f"    {k}: {body[k]}")
PY
  else
    echo "  did_registry:     ❌ not found (HTTP ${http_code})"
  fi
  rm -f "${out}"

  out="$(mktemp)"
  http_code="$(curl -sS -o "${out}" -w '%{http_code}' \
    "${BASE}/api/documents/spacekit_users/did:spacekit:user:${name}" \
    -H "Authorization: DID did:spacekit:user:${name}" 2>/dev/null)" || http_code="000"
  if [[ "${http_code}" == "200" ]]; then
    echo "  spacekit_users:   ✅ profile document exists"
  else
    echo "  spacekit_users:   — no profile doc (HTTP ${http_code}; optional)"
  fi
  rm -f "${out}"

  if [[ -n "${WEBSITE_API}" ]]; then
    out="$(mktemp)"
    http_code="$(curl -sS -o "${out}" -w '%{http_code}' \
      "${WEBSITE_API%/}/api/did/resolve/${name}" 2>/dev/null)" || http_code="000"
    if [[ "${http_code}" == "200" ]]; then
      echo "  website-api:      ✅ resolvable"
      python3 - "${out}" <<'PY'
import json, sys
with open(sys.argv[1], encoding="utf-8") as f:
    data = json.load(f)
if data.get("found"):
    reg = data.get("registration") or data
    for k in ("did", "username", "registered_at"):
        if isinstance(reg, dict) and k in reg:
            print(f"    {k}: {reg[k]}")
PY
    else
      echo "  website-api:      ❌ not found (HTTP ${http_code})"
    fi
    rm -f "${out}"
  fi
}

count_storage_legacy_users() {
  local out http_code
  out="$(mktemp)"
  http_code="$(curl -sS -o "${out}" -w '%{http_code}' \
    -H "Authorization: DID ${ADMIN_DID}" \
    "${BASE}/service/all_users" 2>/dev/null)" || {
    echo "n/a"
    rm -f "${out}"
    return
  }
  if [[ "${http_code}" == "404" ]]; then
    echo "n/a (debug off)"
    rm -f "${out}"
    return
  fi
  if [[ "${http_code}" != "200" ]]; then
    echo "n/a (HTTP ${http_code})"
    rm -f "${out}"
    return
  fi
  python3 - "${out}" <<'PY'
import json, sys
with open(sys.argv[1], encoding="utf-8") as f:
    data = json.load(f)
print(len(data) if isinstance(data, list) else 0)
PY
  rm -f "${out}"
}

count_website_signups() {
  if [[ -z "${API_SECRET}" ]]; then
    echo "n/a (set API_SECRET)"
    return
  fi
  local out http_code
  out="$(mktemp)"
  http_code="$(curl -sS -o "${out}" -w '%{http_code}' \
    -H "X-API-Secret: ${API_SECRET}" \
    "${WEBSITE_API%/}/api/admin/signups" 2>/dev/null)" || {
    echo "n/a"
    rm -f "${out}"
    return
  }
  if [[ "${http_code}" != "200" ]]; then
    echo "n/a (HTTP ${http_code})"
    rm -f "${out}"
    return
  fi
  python3 - "${out}" <<'PY'
import json, sys
with open(sys.argv[1], encoding="utf-8") as f:
    data = json.load(f)
print(data.get("total", len(data.get("signups", []))))
PY
  rm -f "${out}"
}

print_storage() {
  echo "Storage node (${BASE})"
  echo "  did_registry (accounts):  $(storage_query_count did_registry)  ← primary"
  echo "  signups (welcome email):  $(storage_query_count signups)"
  echo "  user_profiles:            $(storage_query_count user_profiles)"
  echo "  legacy /service/signup:   $(count_storage_legacy_users)"
  echo
  echo "Note: spacekit_users profiles are stored per-user DID; use 'lookup <name>' to check one."
}

print_website() {
  echo "Website API (${WEBSITE_API%/})"
  echo "  signups (admin API):      $(count_website_signups)"
  echo "  (use did_registry on storage for account count)"
}

case "${MODE}" in
  lookup) lookup_username "${USERNAME}" ;;
  storage) print_storage ;;
  website) print_website ;;
  all)
    print_storage
    echo
    print_website
    ;;
  *)
    echo "Usage: $0 [storage|website|all|lookup <username>]" >&2
    exit 1
    ;;
esac
