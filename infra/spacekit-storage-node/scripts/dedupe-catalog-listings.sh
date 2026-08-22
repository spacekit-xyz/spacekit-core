#!/usr/bin/env bash
# Remove duplicate marketplace catalog documents from storage.
#
# Deploy writes each listing twice (intentional mirror):
#   - publisher DID (owner copy)
#   - did:spacekit:admin:website-api (website catalog index)
# Query dedupe hides both in reads; this script deletes the redundant owner copy
# when an identical website-api mirror exists.
#
# Usage:
#   STORAGE_NODE=https://api.spacekit.xyz/api/storage \
#   PUBLISHER_DID=did:spacekit:user:astor \
#   ./scripts/dedupe-catalog-listings.sh
#
# Optional: COLLECTION=content_listings for media catalog dupes.

set -euo pipefail

STORAGE_NODE="${STORAGE_NODE:-http://127.0.0.1:3030}"
COLLECTION="${COLLECTION:-app_listings}"
PUBLISHER_DID="${PUBLISHER_DID:-}"
CATALOG_OWNER_DID="${CATALOG_OWNER_DID:-did:spacekit:admin:website-api}"
BASE="${STORAGE_NODE%/}"

if [[ -z "$PUBLISHER_DID" ]]; then
  echo "Set PUBLISHER_DID (the deploy owner DID whose mirror copies should be pruned)." >&2
  exit 1
fi

query_payload='{"filters":[],"limit":50000,"offset":0,"sort_by":{"field":"updated_at","order":"Desc"}}'

echo "→ Querying ${COLLECTION} on ${BASE} ..."
json="$(curl -fsS -X POST "${BASE}/query/documents/${COLLECTION}" \
  -H "Content-Type: application/json" \
  -H "Authorization: DID ${CATALOG_OWNER_DID}" \
  -d "${query_payload}")"

python3 - "$json" "$PUBLISHER_DID" "$CATALOG_OWNER_DID" "$BASE" "$COLLECTION" <<'PY'
import json, sys, urllib.request

payload = json.loads(sys.argv[1])
publisher = sys.argv[2]
catalog_owner = sys.argv[3]
base = sys.argv[4].rstrip("/")
collection = sys.argv[5]

docs = payload.get("documents", [])
by_id = {}
for doc in docs:
    by_id.setdefault(doc["id"], []).append(doc)

deleted = 0
for doc_id, group in sorted(by_id.items()):
    if len(group) < 2:
        continue
    owners = {d.get("owner_did") for d in group}
    if publisher not in owners or catalog_owner not in owners:
        continue
    # Keep website-api mirror; delete publisher mirror.
    url = f"{base}/api/documents/{collection}/{doc_id}"
    req = urllib.request.Request(
        url,
        method="DELETE",
        headers={"Authorization": f"DID {publisher}"},
    )
    try:
        with urllib.request.urlopen(req) as resp:
            code = resp.status
    except urllib.error.HTTPError as e:
        code = e.code
    if code in (204, 404):
        print(f"   ✅ deleted owner mirror {collection}/{doc_id} (HTTP {code})")
        deleted += 1
    else:
        print(f"   ⚠️  failed {collection}/{doc_id} (HTTP {code})")

print(f"\nDone. Removed {deleted} duplicate owner mirror(s).")
PY
