# Operator discovery (Stream E preview)

Operators advertise capacity and policy via a **`spacekit:operator:v1`** fact stored on
their node (public `POST /facts` when policy allows).

## Manifest schema

Built by `src/operator_manifest.rs`:

```json
{
  "operator_did": "did:spacekit:operator:alpha",
  "display_name": "Alpha Storage",
  "storage_http_url": "http://127.0.0.1:3030",
  "blob_fact_auth": "hybrid",
  "content_policy_uri": "https://example.com/spacekit-policy.json",
  "supported_features": [
    "workspaces",
    "sandboxes",
    "mcp",
    "federation_export"
  ],
  "supported_migration_versions": ["v1", "v2"],
  "did_signature_capable": true,
  "sphincs_public_key_hex": "<hex>",
  "published_at": 1779310408
}
```

| Field | Meaning |
|-------|---------|
| `operator_did` | Operator identity (matches node DID or org DID) |
| `storage_http_url` | Public HTTP API base for clients |
| `blob_fact_auth` | Active auth mode (`permissive` / `hybrid` / `strict`) |
| `content_policy_uri` | Stream D policy document |
| `supported_features` | Capability flags for clients |
| `supported_migration_versions` | `v1` (HMAC handoff only), `v2` (DID-signed manifests) |
| `did_signature_capable` | Node can produce/verify SPHINCS+ migration signatures |
| `sphincs_public_key_hex` | Public key for verifying this operator's migration signatures |

Deterministic fact id: `SHA256("spacekit-operator-v1\0" || operator_did)`.

## Publishing (today)

```bash
spacekit operator publish --display-name "My Node" \
  --storage-url http://127.0.0.1:3030 \
  --policy-uri https://example.com/policy.json \
  --blob-fact-auth hybrid \
  --feature workspaces --feature federation_export \
  --sign   # required when node uses strict mode

spacekit operator fact-id   # deterministic fact id hex
```

Or: build with `build_operator_fact_package` (Rust) and `POST /facts` with
`Authorization: DID <operator_did>`. In **strict** mode use `--sign` or a valid
SPHINCS+ signature (`sphincs-128s`).

## Read path (canonical)

```http
GET /api/operators/self
GET /api/operators/self?public_url=http://public-host:3030
```

Returns `spacekit:operator:self:v1` with either:

- `manifest_source: "published_fact"` — body from `POST /facts` (`spacekit operator publish`)
- `manifest_source: "runtime"` — synthesized from node DID, auth mode, and health flags

Response includes `Cache-Control: public, max-age=300`. Configure
`SPACEKIT_PUBLIC_HTTP_URL` (set automatically by `spacekit network up`) or pass
`public_url` when behind a reverse proxy.

```bash
spacekit operator show
curl -s http://127.0.0.1:3030/api/operators/self | jq .
```

There is no network-wide index yet; discovery is **pull from known operator URLs** or
future gossip/index (Stream E item 1).

## Client usage

1. Fetch operator manifest fact by id or query.
2. Read `content_policy_uri` before `POST /api/workspaces`.
3. Use `storage_http_url` for API calls and federation handoff (`replicate_blobs_from`).

## Related

- [`did-signed-migration.md`](./did-signed-migration.md)
- [`operator-abuse-policy.md`](./operator-abuse-policy.md)
- [`federation-workspace-handoff.md`](./federation-workspace-handoff.md)
- [`federation-roadmap.md`](./federation-roadmap.md)
