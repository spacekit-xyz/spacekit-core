# Upload tokens (Stream A)

Browser and batch clients can upload blobs without signing every request with a
full DID key. An agent (or user session) mints a **short-lived upload token**
and passes it on CAS operations.

## Important: secret must reach the **server process**

`SPACEKIT_UPLOAD_TOKEN_SECRET` in the shell where you run `curl` does **not**
configure the node. The variable must be set **before starting** the storage
node (or written to the data directory file below), then restart:

```bash
export SPACEKIT_UPLOAD_TOKEN_SECRET="$(openssl rand -hex 32)"
spacekit network down && spacekit network up
```

Check configuration:

```bash
curl -s http://127.0.0.1:3030/api/agentic/health | jq .upload_tokens_configured
# true when configured
```

On start, if the env var is set, the node also copies it to
`{data_dir}/.upload_token_secret` so later restarts work without re-exporting env.

## Mint a token

Requires `Authorization: DID <did>` (owner of the upload).

```http
POST /api/upload-tokens
Authorization: DID did:spacekit:agent:demo
Content-Type: application/json

{
  "operation": "put_blob",
  "resource": "<blake3-hex-64-chars-or-*",
  "ttl_seconds": 900
}
```

Response:

```json
{
  "token": "skut1.<hex-payload>.<hex-mac>",
  "expires_at": 1710000900,
  "operation": "put_blob",
  "resource": "*"
}
```

### Signing secret

Configure one of:

- `SPACEKIT_UPLOAD_TOKEN_SECRET` — set **before** `spacekit network up` / `spacekit-storage-node start`
- `{data_dir}/.upload_token_secret` — one line (hex or passphrase); survives restarts
- `[runtime] upload_token_secret` in `~/.spacekit/network.toml` (supervisor writes the file)

64-character hex strings are decoded to 32 bytes; other strings are used as UTF-8 bytes.

Without a secret, mint returns `503` with a `hint` field; blob routes accept only `Authorization: DID`.

## Use a token

```http
PUT /blobs/{hash}
Authorization: UploadToken skut1.<payload>.<mac>
Content-Type: application/octet-stream

<raw bytes>
```

Supported operations:

| `operation` | HTTP use |
|-------------|----------|
| `put_blob` | `PUT /blobs/{hash}` (`resource` must match hash or `*`) |
| `get_blob` | `GET /blobs/{hash}` (strict read mode) |
| `put_fact` | `POST /facts` (hybrid/strict; token `sub` must match fact author) |

`Authorization: DID` continues to work for all routes.

## MCP

`upload_token_mint.v1` with `issuer_did`, `operation`, `resource`, `ttl_seconds`.
See [`mcp.md`](./mcp.md).

## Staging strict auth

[`blob-fact-auth-staging.md`](./blob-fact-auth-staging.md) — hybrid → strict rollout.

## Related

- Blob/fact policy modes: `SPACEKIT_BLOB_FACT_AUTH` (`permissive` | `strict` | `hybrid`)
- [`sandboxes.md`](./sandboxes.md) — agentic writes via sandbox + transactions
