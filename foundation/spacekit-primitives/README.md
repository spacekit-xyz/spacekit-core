# SpaceKit Primitives

Shared cryptographic and platform primitives for the SpaceKit ecosystem: identity, wallets, crypto, and optional post-quantum (FIPS 203/204/205) support.

## Package format

The canonical interoperable `.spkg` container is specified in
[`docs/SPKG_V1.md`](docs/SPKG_V1.md). AppPackage types remain in
`v1::app`; the CLI and SDK implement archive production and consumption.

## Features

| Feature       | Default | Description |
|---------------|---------|-------------|
| **std**       | ✅      | Standard library and full crate (wallet, identity, crypto, SDK, storage, etc.). Disable for `no_std`. |
| **no_std**    | ❌      | Build without `std`; only `secrets_core` is compiled. Use with `secrets-core`. |
| **quantum**   | ✅      | Legacy post-quantum crypto (liboqs: Kyber, Dilithium, etc., and pqcrypto SPHINCS+). |
| **secrets-core** | ❌   | FIPS 203/204/205: ML-KEM-768, ML-DSA-65, SLH-DSA, keygen, derivation, hybrid encryption. |

- **Default:** `std` + `quantum` (full crate with legacy PQC).
- **Minimal no_std:** only `secrets_core` (NIST PQC + hybrid encryption), no BIP-39 mnemonic derivation.

## Building

From the `spacekit-primitives` directory:

```bash
# Full crate (std + quantum)
cargo build

# With FIPS 203/204/205 secrets-core
cargo build --features secrets-core

# no_std: only secrets_core (no mnemonic derivation)
cargo build --no-default-features --features no_std,secrets-core
```

## Usage

### Standard (with std)

```toml
[dependencies]
spacekit-primitives = { path = "..", features = ["secrets-core"] }
```

Default features include `std` and `quantum`. Add `secrets-core` for FIPS 203/204/205 APIs.

### no_std (secrets-core only)

For embedded or `no_std` targets, disable default features and enable `no_std` and `secrets-core`:

```toml
[dependencies]
spacekit-primitives = { path = "..", default-features = false, features = ["no_std", "secrets-core"] }
```

- Only the `secrets_core` module is built.
- `derive_from_mnemonic` and `MNEMONIC_DERIVE_SALT` are **not** available (they require `std` and `bip39`).
- Wallet-signature derivation, KEM, signing, and hybrid encryption are available.

## Secrets-core API (FIPS 203/204/205)

When the `secrets-core` feature is enabled, the crate re-exports the `secrets_core` API:

- **Types:** `SignerVariant`, `QuantumKeyMaterial`, `DerivedKeySet`, size constants (`KEM_PK_SIZE`, `KEM_CT_SIZE`, `MLDSA_PK_SIZE`, etc.), `DERIVE_MESSAGE`.
- **KEM (ML-KEM-768):** `kem_generate_keypair`, `encapsulate`, `decapsulate`.
- **Signatures (ML-DSA-65 / SLH-DSA):** `generate_signer_keypair`, `sign`, `verify`, `derive_pubkey_from_signing_key`.
- **Keygen:** `generate_keypair` (KEM + signer).
- **Derivation:** `derive_from_wallet_signature`; with `std` also `derive_from_mnemonic`, `MNEMONIC_DERIVE_SALT`.
- **Hybrid encryption:** `encrypt_for_recipient`, `decrypt_blob` (KEM + HKDF + AES-256-GCM).

### Example (with std)

```rust
use spacekit_primitives::secrets_core::{
    generate_keypair, SignerVariant, encrypt_for_recipient, decrypt_blob,
};

let kp = generate_keypair(SignerVariant::MlDsa65)?;
let blob = encrypt_for_recipient(b"secret", kp.kem_encap_bytes.as_slice(), b"context")?;
let plain = decrypt_blob(&blob, &kp.kem_decap_bytes, b"context")?;
```

### Secrets-core module layout

All of the following lives under `v1/secrets_core/` and is compiled only when `secrets-core` is enabled.

| File | Contents |
|------|----------|
| **mod.rs** | Feature gate and re-exports; root re-export in lib when feature is on. |
| **types.rs** | `SignerVariant` (MlDsa65, SlhDsaSha2128s, SlhDsaSha2192s); size constants (`KEM_PK_SIZE`, `KEM_CT_SIZE`, `KEM_DECAP_SIZE`, `MLDSA_*`, `SLH_*`); `QuantumKeyMaterial`; `DerivedKeySet`; `DERIVE_MESSAGE`. |
| **rng.rs** | OS RNG for rand_core 0.10 (getrandom-based); used by ML-DSA and SLH-DSA keygen. |
| **kem.rs** | ML-KEM-768: keypair, `encapsulate(encap_key) -> (ciphertext, shared_secret)`, `decapsulate(decap_seed, ciphertext) -> shared_secret`. |
| **signature.rs** | Sign/verify and keygen per variant (ML-DSA-65, SLH-DSA Sha2_128s/192s): `sign`, `verify`, `generate_signer_keypair`, `derive_pubkey_from_signing_key`. |
| **keygen.rs** | `generate_keypair(variant) -> QuantumKeyMaterial` (KEM + signer). |
| **derive.rs** | `derive_from_wallet_signature(sig, address) -> DerivedKeySet`; `derive_from_mnemonic(mnemonic, passphrase)` (when `std`). |
| **hybrid.rs** | `encrypt_for_recipient(plaintext, encap_bytes, context)` and `decrypt_blob(blob, decap_bytes, context)`. Blob: `[KEM_CT \|\| 12-byte IV \|\| AES-GCM ciphertext]`. |

### Secrets-core dependencies (when feature is on)

Optional deps enabled by `secrets-core`: `ml-kem` (0.3.0-rc.0), `ml-dsa` (0.1.0-rc.7), `slh-dsa` (0.2.0-rc.4), `hkdf`, `zeroize`, `rand_core` (0.10), `getrandom`, `aes-gcm`. The crate also uses `anyhow`, `serde`, `sha2` (always present). Mnemonic derivation uses `bip39` when `std` is enabled.

### RustCrypto API compatibility

Implementation targets the published APIs: **ml-kem** (64-byte decap seed, `FromSeed`, `KeyExport`), **ml-dsa** (32-byte signing seed, `KeyGen`, `Signer`/`Verifier`), **slh-dsa** (`SigningKey::<Sha2_128s>::new`, etc.). Exact versions and function signatures in code may be updated when newer crates are released; behavior (key sizes, blob layout, derivation domains) follows this design.

## License

See the repository root for license information.
