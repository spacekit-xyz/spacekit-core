# SpaceKit identity architecture

`spacekit-did` is the **cryptographic and wallet library** in a larger SpaceKit stack. It is not a standalone decentralized identity network. Identity becomes **network-visible** when other components (CLI, compute node, registry contract) are running and wired together.

## System map

```
┌─────────────────────────────────────────────────────────────────────────┐
│  spacekit-cli                                                           │
│  `spacekit init`  → ~/.spacekit (config, KEM keys, placeholder DID)     │
│  `spacekit did create` → SPHINCS+ wallet + did:spacekit:testnet:…         │
│       └─ POST /v1/did/register (when compute node is up)                  │
└───────────────────────────────┬─────────────────────────────────────────┘
                                │ uses
                                ▼
┌─────────────────────────────────────────────────────────────────────────┐
│  spacekit-did (this crate)                                              │
│  SphincsPlus · QuantumResistantWallet · VC helpers · registry traits    │
└───────────────────────────────┬─────────────────────────────────────────┘
                                │ used by
                                ▼
┌─────────────────────────────────────────────────────────────────────────┐
│  spacekit-compute-node (testnet / standalone)                           │
│  Node identity · consensus `register_validator` · HTTP DID API            │
│  POST /v1/did/register · GET /v1/did/resolve (resolve stub today)       │
└───────────────────────────────┬─────────────────────────────────────────┘
                                │ target (in progress)
                                ▼
┌─────────────────────────────────────────────────────────────────────────┐
│  spacekit-standard-library / spacekit-did-registry (WASM contract)      │
│  REGISTER · RESOLVE · ROTATE · DEACTIVATE on SpaceKitVM storage           │
└─────────────────────────────────────────────────────────────────────────┘

Experimental (separate repo): spacekit-did-onchain — EVM/Solana reference only
```

## How DIDs are created (by component)

### 1. `spacekit init` (CLI — first-time setup)

**Command:** `spacekit init`  
**Location:** `spacekit-cli` → `handle_init`

What it does:

- Creates `~/.spacekit/` (`config.toml`, `keys/public_key.hex`, `keys/private_key.hex`).
- Assigns a **placeholder** DID: `did:spacekit:user:{uuid}` (stored in `config.toml`).
- Generates **KEM** key material (e.g. Kyber) via `spacekit-primitives`, **not** a full `QuantumResistantWallet` in this step.
- Does **not** call `POST /v1/did/register` and does **not** write to the on-chain / VM DID registry.

This is **local environment bootstrap**, not testnet registry enrollment.

### 2. Operational testnet DID (CLI wallet)

When commands need a `QuantumResistantWallet`, the CLI loads `spacekit-did` and **reconciles** the placeholder:

- If config has `did:spacekit:user:…`, the CLI derives  
  `did:spacekit:testnet:0x{14-char-prefix}` from the KEM public key (Keccak address convention).
- `apply_config_did` updates the in-memory wallet metadata; **SPHINCS+ keys** still come from `QuantumResistantWallet::new()` unless loaded from `did_wallet.json`.

See `load_or_create_did_wallet` in `spacekit-cli/src/full_client.rs`.

### 3. `spacekit did create` (registry-shaped DID + optional registration)

**Command:** `spacekit did create`  
**Uses:** `spacekit_did::QuantumResistantWallet` + `SphincsPlus`

What it does:

- Builds `did:spacekit:testnet:{sha256(sphincs_pk)[0:20]}` (hex address).
- Self-signs `sphincs_pk || kyber_pk || network` (matches compute-node and registry contract message layout).
- **Attempts** `POST {SPACEKIT_COMPUTE_URL}/v1/did/register` (profile default
  `http://127.0.0.1:9000`; `:8080` is the gateway).
- On success, compute node returns a synthesized document; on failure, DID still exists **locally**.

Kyber in this path may be a **placeholder** until the browser or another component rotates in real KEM keys.

### 4. Compute node (when testnet / standalone is running)

**Location:** `spacekit-compute-node` (`standalone.rs`, `quantum_security.rs`)

On **node start**:

- Loads node identity (config / generated wallet) and calls  
  `consensus_coordinator.register_validator(local_did)` (in-memory validator set for consensus).

**HTTP API** (development / integration):

| Endpoint | Behavior today |
|----------|----------------|
| `POST /v1/did/register` | Verifies SPHINCS+ self-signature; derives `did:spacekit:{network}:{address}`; returns JSON document. **Does not yet persist** to `spacekit-did-registry` / storage replication (noted in source comments). |
| `GET /v1/did/resolve` | Validates DID format; returns stub (`resolved: false`) until wired to SpaceKitVM registry. |

### 5. Canonical registry (target production store)

**Location:** `spacekit-standard-library/system/spacekit-did-registry`

WASM contract implementing REGISTER / RESOLVE / ROTATE / DEACTIVATE with host `sphincs_verify`. This is the intended **shared registry** once invoked through SpaceKitVM and storage nodes—not implemented inside the `spacekit-did` crate itself.

## What this crate provides vs the system

| Capability | In `spacekit-did` | Elsewhere in SpaceKit |
|------------|-------------------|------------------------|
| SPHINCS+ sign/verify | Yes | Compute node, registry WASM, CLI |
| Local wallet & JSON credentials | Yes | CLI, compute-node wrapper |
| VPN VC issue/verify (with resolver) | Yes | Your app + registry impl |
| `did:spacekit:user:` from `init` | No (CLI only) | `spacekit init` |
| Testnet registry enrollment | No | `did create` + compute HTTP + future VM |
| Persistent DID resolution | Traits only | Registry contract + storage (WIP) |
| Consensus validator binding | No | `register_validator` on compute node |
| W3C DID / VC conformance | No | Custom method & JSON shapes |

## Known gaps (audit-relevant)

Document these explicitly when reviewing “quantum-resistant decentralized identity”:

1. **`spacekit init` DID ≠ registry DID** — placeholder `did:spacekit:user:{uuid}` until derived or replaced via `did create`.
2. **`verify_credential` (wallet)** — verifies with the **local** wallet key, not the issuer’s key from a registry (use `SpacekitVcVerifier` or fix before production).
3. **No persistent registry in this crate** — `VerifiableDataRegistry` / `DidResolver` are traits; no default networked implementation shipped here.
4. **Compute-node register is not durable** — HTTP register succeeds in-process; VM/registry persistence is TODO.
5. **Resolve is stubbed** on compute node until SpaceKitVM connects.
6. **Private keys** — `QuantumKeyPair` is `Serialize`/`Deserialize`; risk of accidental persistence in JSON.
7. **Credential canonicalization** — `serde_json` signing, not JCS / W3C canonical form.
8. **Dual crypto stacks** — `init` uses KEM keys in `~/.spacekit/keys`; wallet uses SPHINCS+ from `QuantumResistantWallet::new()` unless `did_wallet.json` is used—integrators must understand both.
9. **On-chain bridges** — moved to `spacekit-did-onchain`; not part of production identity claims for this crate.

## Suggested audit scope

**In scope for `spacekit-did`:**

- `src/sphincs.rs`, `src/did/quantum.rs`, wallet, `vc_issuer` / `vc_verifier`
- Assumptions documented above (library layer only)

**System integration (separate or explicit addendum):**

- `spacekit init` / `did create` flows in `spacekit-cli`
- `POST /v1/did/register` in `spacekit-compute-node`
- `spacekit-did-registry` WASM contract
- End-to-end: init → network up → register → resolve → VC issue/verify

**Out of scope unless claimed:**

- `spacekit-did-onchain` EVM/Solana experimental artifacts

## Related documentation

- [README.md](README.md) — crate usage
- [SECURITY.md](SECURITY.md) — vulnerability reporting
- [spacekit-compute-node/README.md](spacekit-compute-node/README.md) — HTTP DID endpoints
- [spacekit-did-onchain/README.md](spacekit-did-onchain/README.md) — experimental chain code
