# Blob and fact auth — staging rollout

Use this checklist when moving a self-hosted node from **permissive** to **strict**
DID auth (`SPACEKIT_BLOB_FACT_AUTH=strict`) without breaking agents or browsers.

## 1. Preconditions

- Storage node has `data_dir` / CAS configured (blob refs register on fact ingest).
- Upload tokens configured for browser clients (see [`upload-tokens.md`](./upload-tokens.md)).
- Agents use `Authorization: DID <did>` on agentic routes and fact writes.

## 2. Configure signing material

**Upload tokens** (browser/blob path):

```bash
export SPACEKIT_UPLOAD_TOKEN_SECRET="$(openssl rand -hex 32)"
# Restart the node in the *same* shell (env is read at process start):
spacekit network down && spacekit network up
```

Or persist under the storage data directory (survives restarts without env):

```bash
echo -n "$SPACEKIT_UPLOAD_TOKEN_SECRET" > ~/.spacekit/network/storage/.upload_token_secret
chmod 600 ~/.spacekit/network/storage/.upload_token_secret
spacekit network down && spacekit network up
```

Confirm: `curl -s http://127.0.0.1:3030/api/agentic/health | jq .upload_tokens_configured` → `true`.

## 3. Hybrid mode soak (recommended)

**Environment:**

```bash
export SPACEKIT_BLOB_FACT_AUTH=hybrid   # facts strict, blob write requires DID/token
spacekit network down && spacekit network up
```

**Network profile** (persists across restarts):

```toml
[runtime]
upload_token_secret = "your-hex-or-passphrase"
blob_fact_auth = "hybrid"
```

The supervisor sets `SPACEKIT_BLOB_FACT_AUTH` and writes `.upload_token_secret` on start.

### Hybrid semantics

| Route | Hybrid behavior |
|-------|-----------------|
| `POST /facts` | Requires `Authorization: DID` (or `UploadToken` for `put_fact`); author must match |
| `PUT /blobs/{hash}` | Requires `DID` or scoped `UploadToken` |
| `GET /blobs/{hash}` | Open (no auth) |
| Agentic routes | Always require `Authorization: DID` |

### Automated soak

With the node running in hybrid mode and upload tokens configured:

```bash
cargo run -p spacekit-storage-node --example hybrid_auth_soak --features standalone -- \
  http://127.0.0.1:3030 did:spacekit:testnet:YOUR_DID
```

The soak verifies health/metrics labels, blob 401/201 paths, open GET, upload-token PUT, and fact 401/403/201.

### Metrics to watch (24–72h)

```bash
curl -s http://127.0.0.1:3030/api/agentic/health | jq '{
  blob_fact_auth_mode,
  upload_tokens_configured,
  did_rate_limit_rejections_total
}'
curl -s http://127.0.0.1:3030/api/agentic/metrics | grep -E 'blob_fact|upload_tokens|did_rate'
```

- `spacekit_blob_fact_auth_mode{mode="hybrid"}` should stay `1`.
- Spikes in `did_rate_limit_rejections_total` may indicate clients retrying without auth.
- Run `agentic_client_demo` and your production agents; fix any `401` on blob PUT or fact POST.

## 4. Strict mode

Only after **hybrid soak passes** (see §3).

```bash
export SPACEKIT_BLOB_FACT_AUTH=strict
spacekit network down && spacekit network up
```

### Strict semantics

| Route | Strict behavior |
|-------|-----------------|
| `PUT /blobs/{hash}` | `DID` or `UploadToken` (`put_blob`) |
| `GET /blobs/{hash}` | `DID` or `UploadToken` (`get_blob`); `blob_refs/` policy when registered |
| `POST /facts` | `DID` + author match + **valid SPHINCS+** signature |
| `GET /facts/{id}` | `AccessPolicy` on stored fact |

Use signature algorithm strings supported by primitives (e.g. `sphincs-128s`). Empty
signatures return `400`; invalid signatures return `403`.

### Automated soak

```bash
cargo run -p spacekit-storage-node --example strict_auth_soak --features standalone -- \
  http://127.0.0.1:3030 did:spacekit:testnet:YOUR_DID
```

The soak uses an ephemeral signed fact (`did:spacekit:strict:soak:signed`) so your
production DID key is not required for the signature check.

## 5. Rollback

```bash
unset SPACEKIT_BLOB_FACT_AUTH
# or: export SPACEKIT_BLOB_FACT_AUTH=permissive
spacekit network down && spacekit network up
```

## 6. Network profile (optional)

See section 3 for `[runtime] upload_token_secret` and `blob_fact_auth`. The supervisor
writes secrets into `{storage_data_dir}/` on start.

## Related

- [`hybrid_auth_soak`](../../examples/hybrid_auth_soak.rs) · [`strict_auth_soak`](../../examples/strict_auth_soak.rs)
- [`federation-roadmap.md`](./federation-roadmap.md)
- [`upload-tokens.md`](./upload-tokens.md)
- [`workspaces.md`](./workspaces.md)
- [`sandboxes.md`](./sandboxes.md)
