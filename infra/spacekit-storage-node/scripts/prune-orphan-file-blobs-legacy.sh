#!/usr/bin/env bash
# Legacy orphan prune: scan catalog documents via query API (pre artifact-ref-index).
set -euo pipefail

STORAGE_NODE="${STORAGE_NODE:-http://127.0.0.1:3030}"
OWNER_DID="${OWNER_DID:-}"
CATALOG_DID="${CATALOG_DID:-did:spacekit:admin:website-api}"
DRY_RUN="${DRY_RUN:-1}"
BASE="${STORAGE_NODE%/}"

if [[ -z "$OWNER_DID" ]]; then
  echo "Set OWNER_DID." >&2
  exit 1
fi

files_query_payload='{"filters":[],"limit":1000,"offset":0,"sort_by":{"field":"size","order":"Desc"},"joins":[],"window_functions":[],"distinct":false}'
doc_query_payload='{"filters":[],"limit":50000,"offset":0,"sort_by":{"field":"updated_at","order":"Desc"}}'

WORKDIR="$(mktemp -d "${TMPDIR:-/tmp}/spacekit-prune-legacy.XXXXXX")"
trap 'rm -rf "${WORKDIR}"' EXIT

storage_post_file() {
  local url="$1" did="$2" payload="$3" out="$4" label="$5"
  local http_code
  http_code="$(curl -sS -o "${out}" -w '%{http_code}' -X POST "${url}" \
    -H "Content-Type: application/json" -H "Authorization: DID ${did}" -d "${payload}")"
  if [[ "${http_code}" != "200" ]]; then
    echo "❌ ${label} failed (HTTP ${http_code}):" >&2
    head -c 2000 "${out}" >&2 || true
    echo >&2
    exit 1
  fi
}

storage_post_optional_file() {
  local url="$1" did="$2" payload="$3" out="$4"
  local http_code
  http_code="$(curl -sS -o "${out}" -w '%{http_code}' -X POST "${url}" \
    -H "Content-Type: application/json" -H "Authorization: DID ${did}" -d "${payload}" 2>/dev/null)" || {
    echo '{"documents":[]}' > "${out}"
    return
  }
  if [[ "${http_code}" != "200" ]]; then
    echo '{"documents":[]}' > "${out}"
  fi
}

storage_post_file "${BASE}/query/files" "${OWNER_DID}" "${files_query_payload}" \
  "${WORKDIR}/files.json" "POST /query/files"
storage_post_optional_file "${BASE}/query/documents/app_listings" "${CATALOG_DID}" \
  "${doc_query_payload}" "${WORKDIR}/listings.json"
storage_post_optional_file "${BASE}/query/documents/deployments" "${CATALOG_DID}" \
  "${doc_query_payload}" "${WORKDIR}/deployments.json"
storage_post_optional_file "${BASE}/query/documents/content_listings" "${CATALOG_DID}" \
  "${doc_query_payload}" "${WORKDIR}/content.json"

python3 - "${WORKDIR}" "${OWNER_DID}" "${BASE}" "${DRY_RUN}" <<'PY'
import json, os, sys, urllib.request, urllib.error

workdir, owner, base, dry_run_flag = sys.argv[1:5]
dry_run = dry_run_flag not in ("0", "false", "False", "no")

def load(name):
    with open(os.path.join(workdir, name), encoding="utf-8") as f:
        return json.load(f)

files = load("files.json")
listings = load("listings.json")
deployments = load("deployments.json")
content = load("content.json")

def collect_file_ids_from_doc(doc):
    ids = set()
    body = doc.get("data") or doc.get("body") or doc
    if isinstance(body, str):
        try:
            body = json.loads(body)
        except json.JSONDecodeError:
            return ids
    if not isinstance(body, dict):
        return ids
    for key in ("artifacts", "files"):
        arr = body.get(key)
        if not isinstance(arr, list):
            continue
        for item in arr:
            if isinstance(item, dict):
                fid = item.get("file_id")
                if isinstance(fid, str) and fid:
                    ids.add(fid)
    return ids

referenced = set()
for payload in (listings, deployments, content):
    for doc in payload.get("documents", []):
        referenced |= collect_file_ids_from_doc(doc)

owned = []
for row in files.get("files", files.get("results", [])):
    if not isinstance(row, dict):
        continue
    fid = row.get("id") or row.get("file_id")
    if not fid:
        continue
    if row.get("owner_did") and row["owner_did"] != owner:
        continue
    owned.append((fid, int(row.get("size") or 0)))

orphans = [(fid, sz) for fid, sz in owned if fid not in referenced]
orphans.sort(key=lambda x: x[1], reverse=True)

if not orphans:
    print("\nNo orphan file blobs found.")
    raise SystemExit(0)

total_bytes = sum(sz for _, sz in orphans)
print(f"\nFound {len(orphans)} orphan file blob(s) (~{total_bytes / (1024*1024):.1f} MiB metadata sum):")
for fid, sz in orphans[:50]:
    print(f"   {fid}  ({sz:,} bytes)")
if len(orphans) > 50:
    print(f"   … and {len(orphans) - 50} more")

if dry_run:
    print("\nDry run — set DRY_RUN=0 to DELETE these files.")
    raise SystemExit(0)

deleted = 0
for fid, _ in orphans:
    url = f"{base.rstrip('/')}/files/{fid}"
    req = urllib.request.Request(url, method="DELETE", headers={"requester-did": owner})
    try:
        with urllib.request.urlopen(req) as resp:
            code = resp.status
    except urllib.error.HTTPError as e:
        code = e.code
    if code in (200, 204, 404):
        print(f"   ✅ deleted {fid} (HTTP {code})")
        deleted += 1
    else:
        print(f"   ⚠️  failed {fid} (HTTP {code})")

print(f"\nDone. Deleted {deleted} orphan file blob(s).")
PY
