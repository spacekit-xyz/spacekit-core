# SpaceKit / ASTRA — cryptographic primitives, per layer

This document states, precisely and per layer, which cryptographic primitive
each part of the SpaceKit stack uses **today**, which primitive *class* it
belongs to (signature / KEM / hash), whether it is post-quantum (PQ) or
classical, and — where it is classical — why, and the plan to migrate it.

It exists so that any claim we make about being "quantum-safe" can be checked
against the code, one layer at a time. "Quantum-safe" is only true of a *path*
when **all three** primitive classes on that path (signatures **and** key
encapsulation **and** hashing) are PQ. Where that is not yet the default, we say
so here rather than on a marketing page.

> Scope note: this is an inventory of the running implementation, not an
> aspiration. Each row cites the source of truth. If code and this doc disagree,
> the code wins and this doc is the bug.

---

## 1. Summary table

| Layer | Primitive class | Algorithm in use | PQ? | Default? |
|---|---|---|---|---|
| User identity / action signing (kit.space) | Signature | Ed25519 | ❌ classical | ✅ default |
| ↳ optional upgrade | Signature | SLH-DSA-SHA2-128s (FIPS-205) | ✅ PQ | ⬜ opt-in |
| Settlement bundle verification (compute node) | Signature | Ed25519 **or** SPHINCS+ (r3) **or** SLH-DSA (FIPS-205) | ✅/❌ depends on signer | — |
| Operator API auth (compute node) | Signature | SPHINCS+ / SLH-DSA | ✅ PQ | ✅ default |
| DID / consensus identity | Signature | Quantum-resistant DID (SPHINCS+/SLH-DSA) | ✅ PQ | ✅ default |
| EVM / L1 bridge interop | Signature | secp256k1 (ECDSA) | ❌ classical | interop-only |
| Transport / message & data encryption | KEM | Kyber-512/768/1024 (ML-KEM), NTRU-Prime, FrodoKEM (liboqs) | ✅ PQ | ✅ default |
| Account address derivation | Hash | SHA-256(pubkey)\[0..20] | ✅ PQ-resistant | ✅ default |
| Bundle / state commitments | Hash | SHA-256, verkle state commitment | ✅ PQ-resistant | ✅ default |

The honest one-line version: **KEM and hashing are PQ across the board;
signatures are PQ-*capable* everywhere but PQ-*by-default* only on
infrastructure (operator API, DID, consensus), not yet on end-user action
signing.**

---

## 2. Signatures — the layer that needs the most precision

Signatures are where "quantum-safe settlement" lives or dies, so they get the
most detail.

### 2.1 User identity & action signing — classical by default, PQ opt-in

A kit.space identity is minted as an **Ed25519** keypair
(`kit.space-website/src/auth/identity.ts` → `persistIdentity` writes
`algorithm = "ed25519"`). Every VM transaction / intent the user authorizes is
signed with that key via the wallet's active algorithm
(`src/protocol/useKitProtocol.ts` sets the tx signer from
`useDidWallet.sign` + `wallet.algorithm`).

A user can upgrade to a post-quantum identity — **SLH-DSA-SHA2-128s (FIPS-205)**
— via `useDidWallet.upgradeToQuantum()`, which mints a fresh key with the
RustCrypto `slh-dsa` crate compiled to WASM (`identity/wasm-did`). After upgrade,
the same signing path produces FIPS-205 signatures.

**This is the classical piece the reviewer is pointing at.** Because the default
identity is Ed25519, a default account's actions are signed classically. This is
the same constraint documented in the intent-protocol spec: user-facing signing
stayed on Ed25519/secp256k1 for wallet-key-type compatibility, with PQ applied
first to transport and infrastructure. It remains true here until the migration
in §5 lands.

### 2.2 Settlement bundle verification — verifier supports classical *and* PQ

The compute node's rollup verifier
(`infra/spacekit-compute-node/src/rollup_bridge.rs::validate_rollup_bundle`)
selects the verifier by the bundle signature's `algorithm` string:

- `ed25519` → `ed25519-dalek` (classical).
- `sphincs-*` → `pqcrypto-sphincsplus` — **SPHINCS+ round-3** (PQ).
- `slh-dsa-sha2-128s` / `slh-dsa-sha2-192s` → `verify_slh_dsa_signature`
  (`spacekit-primitives`), the RustCrypto **`slh-dsa` / FIPS-205** verifier.

The last arm was added deliberately: FIPS-205 SLH-DSA and SPHINCS+ round-3 share
key/signature **sizes** but are **not byte-compatible** (FIPS-205 changed the
message-hash domain separation), so they must be verified by their own
implementations. A determinism test
(`slh_dsa_browser_wasm_vector_verifies`) pins a real browser-generated
`(public key, signature)` and asserts it verifies under the node's FIPS-205
path — so a browser-signed, **PQ** settlement bundle is provably acceptable to
the node.

Net: settlement is **quantum-safe when the bundle is signed by an SLH-DSA
identity**, and classical when signed by an Ed25519 identity. The verifier does
not upgrade a classical signature; it only faithfully checks whichever was used.

**Enforceable PQ-only cutover (new).** Both settlement entry points
(`/rollup/submit` and `/rollup/validate`) now consult
`SPACEKIT_SETTLEMENT_PQ_ONLY`. When set, a bundle whose signature algorithm is
not post-quantum (`is_post_quantum_algorithm` — SLH-DSA / SPHINCS+ true, Ed25519
false) is **rejected**, so an operator can make "all settlement is quantum-safe"
a hard guarantee rather than a per-signer property. It is off by default so
legacy Ed25519 wallets keep settling until the operator commits to the cutover —
this switch *is* the migration mechanism, not a marketing change.

### 2.3 Operator API auth, DID, consensus — PQ by default

- **Operator/admin API** requests to the node are SPHINCS+/SLH-DSA signed and
  fail-closed (`infra/spacekit-compute-node/src/api_auth.rs`,
  `spacekit_did::sphincs`). No "auth disabled" path.
- **DID / consensus identity** uses the quantum-resistant DID
  (`quantum_security::QuantumResistantDID`, SPHINCS+/SLH-DSA).

These are PQ by default because they are infrastructure, not user wallets.

### 2.4 EVM bridge — secp256k1, classical, interop-only

`secp256k1` ECDSA is kept **only** for Ethereum/L1 bridge interop
(`kit.space-website/src/protocol/chainAddress.ts::evmAddressFromPublicKey`,
Keccak-256 address). It is explicitly **not** the native ASTRA account key and
the native settlement path never depends on it. It is classical by nature
(external chains require it) and is scoped so a break there cannot forge a
native ASTRA action.

---

## 3. Key encapsulation (KEM) — PQ

Transport, message, and data-at-rest encryption use lattice / code-based KEMs via
liboqs (`spacekit-primitives/src/v1/crypto/quantum.rs`): **Kyber-512/768/1024**
(the ML-KEM / FIPS-203 family), **NTRU-Prime sntrup761**, and **FrodoKEM-1344**.
`QuantumResistantEncryption` defaults to Kyber-512. This layer clears the PQ bar.

> Naming: we use the liboqs "Kyber" identifiers. Kyber is the basis of FIPS-203
> ML-KEM; where we say "ML-KEM family" we mean this lineage, not a claim of
> byte-identical FIPS-203 test-vector conformance for every parameter set.

---

## 4. Hashing — PQ-resistant

- Account addresses: `SHA-256(pubkey)[0..20]` (`chainAddress.ts`, matching
  `spacekit-did` and the CLI).
- Bundle commitments: `SHA-256` over a domain-separated canonical encoding
  (`rollup_bridge.rs::compute_bundle_hash`, domain `SPACEKIT-ROLLUP-BUNDLE-v2`).
- State: verkle-based state commitment.

SHA-2 is considered PQ-resistant: Grover gives only a quadratic speedup, leaving
SHA-256 at ~128-bit security against a quantum search. This layer clears the bar.

---

## 5. What is still classical, and the migration plan

Exactly one class is not PQ-by-default: **end-user action/intent signatures**
(Ed25519 default; SLH-DSA available but opt-in). Status of the closing work:

1. **Make PQ identity the sign-up default.** ⬜ **Not done — the last remaining
   gap.** `auth/identity.ts::generateIdentity` still mints Ed25519 and
   `persistIdentity` defaults `algorithm = "ed25519"`, so a *new* account signs
   classically until it runs `upgradeToQuantum`. This is a deliberate product
   decision (SLH-DSA-SHA2-128s signatures are ~7.8 KB and keys are larger), so it
   is left for an explicit call rather than flipped silently.
2. **Close the PQ recovery-key gap.** ✅ **Done.** `normalizeRecoveryKey`
   accepts 64-hex (Ed25519) and 128/192-hex (SLH-DSA) seeds;
   `algorithmForRecoveryKey` selects the scheme; `signInWithRecoveryKey` branches
   accordingly. An upgraded PQ identity can now be recovered on a new device.
3. **Sign settlement bundles with the user's PQ identity.** ✅ **Done and proven
   end-to-end.** `protocol/settlement.ts` + `WalletPanel` sign transfer bundles
   with the active identity (`useDidWallet.sign`) and submit to `/rollup/submit`;
   a PQ identity produces FIPS-205 signatures that the node verifies. A full
   self-custody transfer (faucet → debit → credit, replay + self-custody guards)
   was demonstrated with an SLH-DSA identity.
4. **Operator-sequencer `account_root` → Verified.** ✅ **Done.** `account_root()`
   is pinned by a determinism vector; `/rollup/account-root` exposes it;
   `SPACEKIT_ROLLUP_STRICT_ACCOUNT_ROOT` flips a root mismatch to `Rejected`.
5. **Enforceable PQ-only settlement.** ✅ **Done (opt-in).**
   `SPACEKIT_SETTLEMENT_PQ_ONLY` rejects classical bundle signatures at both
   settlement routes (§2.2) — the server-side cutover switch.
6. **Keep secp256k1 boxed to the bridge.** ✅ Held — no native ASTRA action
   depends on it; the settlement verifier has no secp256k1 arm.

Only **item 1** remains. With PQ recovery, PQ bundle signing, Verified finality,
and a PQ-only enforcement switch all in place, closing item 1 (default new
accounts to SLH-DSA) plus turning on `SPACEKIT_SETTLEMENT_PQ_ONLY` makes *all*
settlement post-quantum by construction.

Until item 1 is the default, the accurate public claim is:

> "SpaceKit supports a fully post-quantum settlement path — **SLH-DSA-SHA2-128s
> (FIPS-205)** signatures, **Kyber / ML-KEM** key encapsulation, **SHA-2**
> hashing — available today and verified end to end. Post-quantum signing is
> **opt-in per account**; new accounts default to Ed25519 for wallet
> compatibility, with a migration path to PQ by default. External-chain bridging
> uses secp256k1 by necessity and is isolated from native settlement."

That sentence is defensible line-by-line against this document. "All settlement
is quantum-safe" is **not**, until the default flips.

---

## 6. How to challenge this doc

Every row above cites a file. To verify signatures end-to-end, run:

```bash
cargo test -p spacekit-compute-node --features "standalone quantum" -- slh_dsa sphincs
```

`slh_dsa_browser_wasm_vector_verifies` proves a browser FIPS-205 signature
verifies on the node; `sphincs_128s_roundtrip_verifies` covers the pqcrypto
path. If either fails, §2.2 is wrong and should be corrected here first.
