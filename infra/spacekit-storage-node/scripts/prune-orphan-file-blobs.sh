#!/usr/bin/env bash
# Delete legacy /files UUID blobs that are no longer referenced by marketplace
# listings, deployment receipts, or content catalog documents.
#
# Prefers the server-side artifact ref index (Phase 1):
#   GET /api/admin/orphan-files
# Falls back to multi-query scan when the index is unavailable (older nodes).
#
# Usage:
#   STORAGE_NODE=http://127.0.0.1:3031 \
#   OWNER_DID=did:spacekit:user:you \
#   ./scripts/prune-orphan-file-blobs.sh
#
# Dry run (default): lists orphans only.
#   DRY_RUN=0 ./scripts/prune-orphan-file-blobs.sh   # actually DELETE

set -euo pipefail

STORAGE_NODE="${STORAGE_NODE:-http://127.0.0.1:3030}"
OWNER_DID="${OWNER_DID:-}"
DRY_RUN="${DRY_RUN:-1}"
BASE="${STORAGE_NODE%/}"

if [[ -z "$OWNER_DID" ]]; then
  echo "Set OWNER_DID (file owner whose orphans should be pruned)." >&2
  exit 1
fi

WORKDIR="$(mktemp -d "${TMPDIR:-/tmp}/spacekit-prune.XXXXXX")"
trap 'rm -rf "${WORKDIR}"' EXIT

orphans_json="${WORKDIR}/orphans.json"

echo "→ Querying orphan files via artifact ref index on ${BASE} ..."
http_code="$(curl -sS -o "${orphans_json}" -w '%{http_code}' \
  -H "Authorization: DID ${OWNER_DID}" \
  "${BASE}/api/admin/orphan-files?owner_did=${OWNER_DID}")"

if [[ "${http_code}" != "200" ]]; then
  echo "   Index API unavailable (HTTP ${http_code}) — using legacy multi-query scan." >&2
  exec "$(dirname "$0")/prune-orphan-file-blobs-legacy.sh"
fi

export ORPHANS_JSON="${orphans_json}"
export OWNER_DID BASE DRY_RUN

python3 - "${ORPHANS_JSON}" "${OWNER_DID}" "${BASE}" "${DRY_RUN}" <<'PY'
import json, sys, urllib.request, urllib.error

orphans_path, owner, base, dry_run_flag = sys.argv[1:5]
dry_run = dry_run_flag not in ("0", "false", "False", "no")

with open(orphans_path, encoding="utf-8") as f:
    payload = json.load(f)

files = payload.get("files", [])
if not files:
    print("\nNo orphan file blobs found.")
    raise SystemExit(0)

total_bytes = int(payload.get("total_bytes") or sum(f.get("size_bytes", 0) for f in files))
print(f"\nFound {len(files)} orphan file blob(s) (~{total_bytes / (1024*1024):.1f} MiB metadata sum):")
for row in files[:50]:
    print(f"   {row.get('file_id')}  ({int(row.get('size_bytes', 0)):,} bytes)")
if len(files) > 50:
    print(f"   … and {len(files) - 50} more")

if dry_run:
    print("\nDry run — set DRY_RUN=0 to DELETE these files.")
    raise SystemExit(0)

deleted = 0
for row in files:
    fid = row.get("file_id")
    if not fid:
        continue
    url = f"{base.rstrip('/')}/files/{fid}"
    req = urllib.request.Request(
        url,
        method="DELETE",
        headers={"requester-did": owner},
    )
    try:
        with urllib.request.urlopen(req) as resp:
            code = resp.status
    except urllib.error.HTTPError as e:
        code = e.code
        if code == 409:
            print(f"   ⚠️  blocked {fid} (still referenced — rebuild index or use ?force=true)")
            continue
    if code in (200, 204, 404):
        print(f"   ✅ deleted {fid} (HTTP {code})")
        deleted += 1
    else:
        print(f"   ⚠️  failed {fid} (HTTP {code})")

print(f"\nDone. Deleted {deleted} orphan file blob(s).")
PY
