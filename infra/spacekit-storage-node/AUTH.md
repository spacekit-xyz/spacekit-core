# Storage node request authentication

Implemented in `src/request_auth.rs` and `src/api/auth_routes.rs`.

## Why

Before this change, `Authorization: DID <did>` (or `Bearer <did>`) *was* the identity: any client could name any DID. That included the website-api's `did:spacekit:admin:website-api`, which is public and owns `auth_sessions`, `auth_passkeys` and `auth_magic_links`. The node also ignored `X-Storage-Secret`.

## Credentials the node accepts

| Credential | How a client gets it | What it can do |
|---|---|---|
| `Authorization: Bearer sktok1.…` session token | Sign a challenge with the DID's key (Ed25519 or SLH-DSA, below), or get one from a backend (`POST /api/auth/service-token`, e.g. the website-api's `/api/auth/storage-token`) | Act as that DID on any route |
| `Authorization: Bearer sktok1.…` app token | `POST /api/auth/delegate` with a session token | Only `/api/documents/app_<appId>_*`. With `act_as`, only `get`/`put` into that namespace; `list`/`delete` are the owner's. |
| `Authorization: DID <did>` + `X-Storage-Secret` | Trusted backends that hold the secret (website-api) | Act as any DID |
| `Authorization: DID <did>` alone | Nothing to get | Only in `SPACEKIT_DID_AUTH=legacy` mode, and never for protected DIDs |

Protected DIDs (`did:spacekit:admin:*`, plus `SPACEKIT_PROTECTED_DIDS`) never authenticate by bare claim, in either mode. The legacy switch cannot reopen the session-theft hole.

Tokens are HMAC-signed by the node (`blake3` keyed hash). Session tokens last up to 24 h (default 12 h). App tokens last up to 1 h and never outlive the session that minted them.

## Signed login

```text
POST /api/auth/challenge  { "did": "did:key:z6Mk…" }
  → { "challenge": "skch1.…", "message": "SpaceKit storage login\nDID: …\nChallenge: …", "expires_at": … }

client signs `message` (UTF-8 bytes) with its key

POST /api/auth/session    { "did", "challenge", "public_key_hex", "signature_hex",
                            "algorithm": "ed25519" | "slh-dsa-sha2-128s" | "slh-dsa-sha2-192s" }
  → { "token": "sktok1.…", "did", "expires_at" }
```

- The DID must be the `did:key` of `public_key_hex`: the W3C multibase form, or kit.space's short form (`did:key:z6Mk` followed by the first 44 hex characters of the key).
- Challenges are stateless (HMAC-signed), expire after 120 s, and each works once per process.
- SLH-DSA (FIPS 205) logins are for kit.space quantum identities. Their DID is `did:key:zQ3s` followed by the first 44 hex characters of the public key; SHA2-128s keys are 32 bytes and SHA2-192s keys 48. The node uses the same `slh-dsa` crate as `identity/wasm-did`, and a signature from that WASM verifies here.
- `did:spacekit:user:*` accounts have no key the node can check. They sign in to the website-api (passkey or magic link), which mints them a node session through `POST /api/auth/storage-token`. In the SDK that is `websiteStorageSession`.
- Login bodies may be up to 64 KiB, because SLH-DSA-192s signatures are 16 KiB (32 KiB as hex).
- `@spacekit/sdk/embed` has a client for this flow: `createStorageAuthClient`.

## Other endpoints

- `POST /api/auth/delegate` (session token) `{ app_id, act_as?, ttl_seconds? }` → app token.
- `POST /api/auth/service-token` (`X-Storage-Secret`) `{ did, app_id?, act_as?, ttl_seconds? }` → a token for a DID the backend has authenticated.
- `GET /api/auth/whoami` (token) → `{ did, scope, expires_at, mode }`.

## Other changes

- Auth failures return **401** with a reason, instead of warp's unhandled 500.
- Blob, fact and package uploads take the same credentials. A backend's `DID` + secret is turned into a one-minute token for those handlers.
- `POST /api/did/register`:
  - With `X-Storage-Secret`: registers or updates any DID.
  - Without it, in legacy mode: first registration only, no overwrites.
  - Without it, in strict mode: refused.
- SPKG uploads verify any `signatures/*.json` publisher signature and reject invalid ones (`src/spkg_signature.rs`). `SPACEKIT_REQUIRE_SIGNED_PACKAGES=true` also rejects unsigned uploads.
- `AppStorageEngine::verify_app` no longer reports a non-empty SPHINCS+ signature as valid.
- **Reserved app collections.** Collections named `app_<appId>___*` hold records only trusted services or the namespace owner may write, such as verified subscriptions (`app_<appId>___subscriptions`).
  - Service assertions can write them, and so can the owner's own unscoped session.
  - App-scoped tokens can only read them.
  - Bare DID claims are refused, even in legacy mode.

## Configuration

| Variable | Default | Meaning |
|---|---|---|
| `SPACEKIT_DID_AUTH` | `legacy` | `strict` rejects all bare DID headers |
| `SPACEKIT_STORAGE_SECRET` or `STORAGE_NODE_SECRET` | unset | Service secret. Without it, protected DIDs cannot authenticate by header at all. |
| `SPACEKIT_AUTH_TOKEN_SECRET` | `{data_dir}/.auth_token_secret`, created on first start | Token signing key. Set it explicitly when several nodes must accept each other's tokens. |
| `SPACEKIT_PROTECTED_DIDS` | none | Extra comma-separated DIDs that never accept a bare claim |
| `SPACEKIT_REQUIRE_SIGNED_PACKAGES` | `false` | Reject SPKG uploads without a valid publisher signature |

## Rollout

1. **Now.** Keep the node reachable only from the website-api and trusted hosts, if you can. Set the same random value as `STORAGE_NODE_SECRET` on the node and on the website-api. Deploy the node and website-api changes together. Protected DIDs then work only with the secret, and the proxy never lends admin identity. After deploying, delete or expire every document in `auth_sessions`, `auth_magic_links` and `auth_passkeys` challenges, and change `ADMIN_DID` to a non-public value.
2. **Migrate clients.** kit.space (this change) logs in with its Ed25519 key and gives apps app tokens. Move spacekit.xyz, Desktop and the CLI to session or service tokens. Run the node with `RUST_LOG=spacekit_auth_legacy=debug` to list requests that still use bare DID headers.
3. **Flip strict.** Set `SPACEKIT_DID_AUTH=strict` on the node and `OWNER_DID_AUTH=strict` on the website-api.
