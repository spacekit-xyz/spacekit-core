#!/usr/bin/env bash
set -euo pipefail

: "${ROUTEKIT_STORAGE_URL:?Set ROUTEKIT_STORAGE_URL}"
: "${ROUTEKIT_OPERATOR_DID:=did:spacekit:service:routekit}"
: "${ROUTEKIT_KEY_OWNER_DID:?Set ROUTEKIT_KEY_OWNER_DID for the client tenant}"
: "${ROUTEKIT_KEY_RATE_LIMIT_RPM:=60}"

if ! command -v jq >/dev/null 2>&1; then
  echo "jq is required" >&2
  exit 1
fi

key="${1:-sk-routekit-$(openssl rand -hex 24)}"
if [[ "${key}" != sk-routekit-* ]]; then
  echo "API keys must start with sk-routekit-" >&2
  exit 1
fi

key_hash="$(printf '%s' "${key}" | shasum -a 256 | awk '{print $1}')"
key_id="rk_${key_hash:0:16}"
payload="$(jq -n \
  --arg key_id "${key_id}" \
  --arg key_hash "${key_hash}" \
  --arg owner_did "${ROUTEKIT_KEY_OWNER_DID}" \
  --argjson rate_limit_rpm "${ROUTEKIT_KEY_RATE_LIMIT_RPM}" \
  '{key_id:$key_id,key_hash:$key_hash,owner_did:$owner_did,enabled:true,expires_at:null,rate_limit_rpm:$rate_limit_rpm}')"

curl --fail --silent --show-error \
  --request PUT \
  --header "Authorization: DID ${ROUTEKIT_OPERATOR_DID}" \
  --header "Content-Type: application/json" \
  --data "${payload}" \
  "${ROUTEKIT_STORAGE_URL%/}/api/documents/routekit-api-keys/${key_hash}" \
  >/dev/null

echo "Provisioned RouteKit API key ${key_id}."
echo "Store this value now; it will not be persisted in plaintext:"
echo "${key}"
