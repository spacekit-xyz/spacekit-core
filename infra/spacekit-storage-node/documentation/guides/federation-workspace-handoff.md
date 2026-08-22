# Federation — workspace handoff (Phase 3 preview)

Operators moving a workspace between nodes use a portable **export bundle**
and **import** on the destination node, with optional **CAS blob replication**.

## Export

```http
GET /api/workspaces/{workspace_id}/export
Authorization: DID did:spacekit:owner
```

```bash
spacekit workspace export team-alpha -o /tmp/team-alpha.json \
  --storage-url http://127.0.0.1:3030
```

Response includes `referenced_blob_hashes` collected from associated repo
`heads/main` commit trees:

```json
{
  "schema": "spacekit:workspace:v1",
  "fact_id": "<64-hex>",
  "owner_did": "did:spacekit:owner",
  "workspace_id": "team-alpha",
  "content": { "...": "WorkspaceContent" },
  "exported_at": 1779310408,
  "referenced_blob_hashes": ["abc123..."],
  "handoff_signature": "<blake3-keyed-hex-mac>"
}
```

When `SPACEKIT_HANDOFF_SECRET` is set (or `{data_dir}/.handoff_secret` exists), export
includes `handoff_signature` — an HMAC over the core export JSON **excluding**
`handoff_signature` and `migration_manifest`. Destination nodes verify on import.
Falls back to the upload-token secret if no handoff secret is configured.

```bash
export SPACEKIT_HANDOFF_SECRET="$(openssl rand -hex 32)"
# restart node; same secret on source and destination for verify-only handoff
export SPACEKIT_REQUIRE_HANDOFF_SIGNATURE=true   # reject unsigned imports
```

`GET /api/agentic/health` exposes `handoff_signing_configured` and
`require_handoff_signature`.

MCP: `workspace_export.v1`.

## Migration manifest (layer 2)

When `operator_did` is configured, export also attaches `migration_manifest`
(`spacekit:migration:v1` or `v2`). v2 includes SPHINCS+ `did_signatures` from the
source operator when both sides support v2 (negotiate via `SPACEKIT_MIGRATION_DEST_URL`
→ destination `GET /api/operators/self`).

```json
{
  "migration_manifest": {
    "schema_version": "spacekit:migration:v2",
    "migration_id": "...",
    "source_operator_url": "http://A:3030",
    "workspace_id": "team-alpha",
    "manifest_hash": "blake3:...",
    "did_signatures": [{ "signer_role": "source_operator", "...": "..." }]
  }
}
```

On import, the destination verifies signatures, appends `destination_operator` when
configured, and stores `spacekit:migration_record:v1` as a public fact. CLI:
`spacekit migration verify` / `migration sign`. Guide:
[`did-signed-migration.md`](./did-signed-migration.md).

## Import (destination node)

```http
POST /api/workspaces/import
Authorization: DID did:spacekit:destination-owner
Content-Type: application/json

{
  "bundle": { "...": "export JSON" },
  "on_conflict": "reject",
  "owner_did": "did:spacekit:destination-owner",
  "replicate_blobs_from": "http://SOURCE:3030",
  "replicate_source_authorization": "DID did:spacekit:source-owner"
}
```

Response includes optional `blob_replication` (`fetched`, `skipped_existing`, `failed`).

```bash
spacekit workspace import /tmp/team-alpha.json \
  --owner-did did:spacekit:dest:owner \
  --source-url http://SOURCE:3030 \
  --source-auth "DID did:spacekit:source-owner" \
  --storage-url http://DEST:3030
```

MCP: `workspace_import.v1` with optional `replicate_blobs_from`.

## Blob replication only

```http
POST /api/blobs/replicate
Content-Type: application/json

{
  "source_url": "http://SOURCE:3030",
  "hashes": ["<blake3-hex>", "..."],
  "source_authorization": "DID did:spacekit:reader"
}
```

MCP: `blobs_replicate.v1`.

## Hybrid auth staging

Before cross-node pulls in production, soak with `blob_fact_auth = "hybrid"` in
`network.toml`. See [`blob-fact-auth-staging.md`](./blob-fact-auth-staging.md).

## Operator metrics

```http
GET /api/agentic/metrics
GET /api/agentic/health
```

## Operator discovery

Publish a `spacekit:operator:v1` manifest fact (see [`operator-discovery.md`](./operator-discovery.md))
so clients know your HTTP URL, auth mode, and content policy URI before handoff.

## Roadmap

Full Stream E timeline: [`federation-roadmap.md`](./federation-roadmap.md).

## Not yet included

Workspace owner counter-sign (wallet UI), sandbox journal migration,
network-wide operator index, multi-hop routing, and settlement hooks.
