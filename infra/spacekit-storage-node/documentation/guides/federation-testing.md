# Federation testing strategy (Stream E)

Tests for multi-operator scenarios beyond single-node auth soaks.

## Automated (CI / local)

| Suite | Command | Covers |
|-------|---------|--------|
| Auth contract | `cargo test --test hybrid_auth` | Hybrid mode flags, token scope |
| Gaps A/B/C | `cargo test --test enhancements_gaps` | Workspaces, handoff, export/import, DID migration v2, v1→v2 matrix, destination counter-sign, content grants |
| Content monetization | `cargo test --test content_sprint2 --test content_e2e_soak` | Payment verify, settlement, PPV/channel soak (in-process) |
| Hybrid HTTP soak | `cargo run --example hybrid_auth_soak --features standalone` | Live node: 401/403/201 paths |
| Strict HTTP soak | `cargo run --example strict_auth_soak --features standalone` | Blob GET auth, SPHINCS+ facts |
| Migration HTTP soak | `cargo run --example migration_auth_soak --features standalone` | v2 export manifest, operator self |

## Manual two-node handoff (required before federation GA)

**Setup:** Node A and B, same `SPACEKIT_HANDOFF_SECRET`, compatible `blob_fact_auth`
(both hybrid or both strict), upload token secret on both if replicating.

```bash
# A: export
spacekit workspace export team-alpha -o /tmp/handoff.json --storage-url http://A:3030

# B: import + replicate
spacekit workspace import /tmp/handoff.json \
  --owner-did did:spacekit:dest:owner \
  --source-url http://A:3030 \
  --source-auth "DID did:spacekit:source:owner" \
  --storage-url http://B:3030
```

**Pass criteria:**

- Import returns `created: true` (or `replaced` on re-run with `--replace`)
- `blob_replication.failed` empty or only pre-existing hashes
- `GET /api/workspaces/team-alpha` on B returns content matching A
- Tampered bundle (drop `handoff_signature`) rejected when `SPACEKIT_REQUIRE_HANDOFF_SIGNATURE=true`
- Export JSON includes `migration_manifest` when `operator_did` is configured
- `spacekit migration verify /tmp/handoff.json` succeeds on source bundle
- After import on B: `migration_record_fact_id` set; migration record fact contains `destination_operator`

## DID-signed migration (two-node)

**Setup:** Same as handoff, plus operator keypair on both nodes (auto-created on start or
`{data_dir}/.operator_sphincs_keypair`). Publish manifest on A with `sphincs_public_key_hex`
so B can verify source signatures (or copy the operator fact + keypair in dev).

```bash
# A: negotiate v2 if B supports it
export SPACEKIT_MIGRATION_DEST_URL=http://B:3030
export SPACEKIT_PUBLIC_HTTP_URL=http://A:3030
spacekit operator publish --display-name "Op A" --sign --storage-url http://A:3030
spacekit workspace export team-alpha -o /tmp/handoff.json --storage-url http://A:3030
spacekit migration verify /tmp/handoff.json

# B: import (counter-signs + persists spacekit:migration_record:v1)
export SPACEKIT_PUBLIC_HTTP_URL=http://B:3030
spacekit workspace import /tmp/handoff.json \
  --owner-did did:spacekit:dest:owner \
  --source-url http://A:3030 \
  --storage-url http://B:3030
```

**Pass criteria:**

- `migration_manifest.schema_version` is `spacekit:migration:v2` when both sides support v2
- `did_signatures` includes `source_operator`; after import, record includes `destination_operator`
- Tampered `migration_manifest.workspace_id` fails `migration verify`
- With `SPACEKIT_REQUIRE_MIGRATION_ATTESTATION=true`, v1-only bundle without signatures rejected

### Version matrix

| Source | Dest | Expected `schema_version` | DID signatures |
|--------|------|---------------------------|----------------|
| v2 + keypair | v2 + keypair | v2 | source + destination on import |
| v2 + keypair | v1-only manifest | v1 | none (negotiated down) |
| v1-only | v2 | v1 | none (destination counter-sign does not upgrade to v2) |

**Automated:** `migration_v1_export_imports_to_v2_capable_destination` in `tests/enhancements_gaps.rs`.

## Operator self-discovery

```bash
spacekit operator publish --display-name "Dev" \
  --storage-url http://127.0.0.1:3030 --sign

spacekit operator show
# expect manifest_source=published_fact after publish

curl -s http://127.0.0.1:3030/api/operators/self | jq '{manifest_source, operator_did, manifest: .manifest.display_name}'
```

## Operator manifest (publish)

```bash
spacekit operator publish --display-name "Dev" \
  --storage-url http://127.0.0.1:3030 \
  --policy-uri https://example.com/policy.json

# fact id = SHA256("spacekit-operator-v1\0" || operator_did)
spacekit fact get <fact_id_hex> --storage-url http://127.0.0.1:3030
```

## Metrics during soak

```bash
curl -s http://127.0.0.1:3030/api/agentic/health | jq .
curl -s http://127.0.0.1:3030/api/agentic/metrics | grep -E 'handoff|blob_fact|upload'
```

Watch for:

- `handoff_signing_configured` / `require_handoff_signature`
- `migration_signing_configured`
- `spacekit_blob_fact_auth_mode{mode="hybrid"}` (or `strict`)
- `did_rate_limit_rejections_total` spikes after client fixes

## Future federation tests (not implemented)

- Cross-operator sandbox: create on A, collaborator on B, commit visible on A
- Operator index: discover manifest via gossip
- Workspace owner counter-sign on bilateral migrations
- Settlement hook after migration (SpaceKit Pay)
- Automated CI matrix for all v1↔v2 combinations

## Related

- [`federation-design.md`](./federation-design.md)
- [`blob-fact-auth-staging.md`](./blob-fact-auth-staging.md)
