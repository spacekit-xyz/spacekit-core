# DID-signed migration — implementation guide

Canonical specification: **[`DID-MIGRATION.md`](../../DID-MIGRATION.md)**.

## Shipped (phases 1–6, partial 7)

Automated matrix: `migration_v1_export_imports_to_v2_capable_destination` in `tests/enhancements_gaps.rs`. Destination `sign_manifest_role` for `destination_operator` does **not** upgrade a v1 inbound manifest to v2 (only `source_operator` export attestation sets v2).

| Component | Location |
|-----------|----------|
| `MigrationManifest` v1/v2 + `DidMigrationSignature` | `src/migration.rs` |
| Canonical payload (length-prefixed fields §3.5) | `canonical_signed_payload` |
| Operator SPHINCS+ keypair | `{data_dir}/.operator_sphincs_keypair` (auto-created on node start) |
| Export attaches `migration_manifest` | `Facade::export_workspace` signs `source_operator` when v2 negotiated |
| Import verifies + destination counter-sign | `Facade::import_workspace` |
| Migration audit fact | `spacekit:migration_record:v1` via `persist_migration_record` |
| Operator manifest extensions | `supported_migration_versions`, `did_signature_capable`, `sphincs_public_key_hex` |
| Health | `migration_signing_configured` on `GET /api/agentic/health` |
| CLI | `spacekit migration verify`, `spacekit migration sign` |

Layer 1 (HMAC) remains `handoff_signature` on the export bundle — unchanged.

## Operator setup

```bash
# Node creates keypair on first start:
#   {storage_data_dir}/.operator_sphincs_keypair

spacekit operator publish --display-name "My Op" --sign \
  --blob-fact-auth hybrid \
  --feature workspaces --feature federation_export

export SPACEKIT_MIGRATION_DEST_URL=http://dest-operator:3030   # optional v1/v2 negotiation
spacekit workspace export team-alpha -o /tmp/handoff.json
spacekit migration verify /tmp/handoff.json
```

## Env flags

| Variable | Effect |
|----------|--------|
| `SPACEKIT_REQUIRE_MIGRATION_ATTESTATION=true` | Reject imports without valid v2 `did_signatures` |
| `SPACEKIT_PUBLIC_HTTP_URL` | `source_operator_url` / destination URL on import counter-sign |
| `SPACEKIT_MIGRATION_DEST_URL` | Fetch destination `/api/operators/self` at export for version negotiation |

## Workspace owner signing (dev / CI)

Wallet UI integration is future work. For testing bilateral migrations:

```bash
spacekit migration keygen --signer-did did:spacekit:your:owner
spacekit migration sign /tmp/handoff.json --role workspace_owner --signer-did did:spacekit:your:owner
```

Keys live under `{storage_data_dir}/.migration_signer_keys/<blake3(did)>.json`.
Import with `SPACEKIT_MIGRATION_SCENARIO=bilateral` requires `source_operator` +
`workspace_owner` on the bundle; destination counter-signs on import.

## Live soak

```bash
cargo run -p spacekit-storage-node --example migration_auth_soak --features standalone -- \
  http://127.0.0.1:3030 did:spacekit:testnet:YOUR_DID
```

## Not yet shipped (phases 7–8)

- Workspace owner counter-sign via wallet UI (production path)
- Full federation testing matrix in CI (v1↔v2 automated case exists; sustained / 30-day soak not done)

See [`federation-roadmap.md`](./federation-roadmap.md).

## Import result

Successful import may return `migration_record_fact_id` (hex) — the deterministic
`spacekit:migration_record:v1` fact written under `{cas}/facts/`.

## Related

- [`federation-workspace-handoff.md`](./federation-workspace-handoff.md)
- [`federation-testing.md`](./federation-testing.md)
- [`operator-discovery.md`](./operator-discovery.md)
- [`phase-2-readiness.md`](./phase-2-readiness.md)
