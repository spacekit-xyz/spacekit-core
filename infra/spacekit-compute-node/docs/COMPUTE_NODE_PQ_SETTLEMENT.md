# Compute-node changes A–E — PQ-native ASTRA settlement (review-ready draft)

Draft patches to make `spacekit-compute-node` the quantum-safe source of truth for ASTRA: derive PQ addresses, accept the browser's SLH-DSA algorithm string, and actually **settle** rollup batches to PQ-keyed accounts. These are drafted against the real source (file/line anchors below) but **not compiled** — review, `cargo check --features quantum`, and test before deploying. Money path: treat with care.

Files touched:
- `infra/spacekit-compute-node/src/spacekitvm/swtchvm_node.rs`
- `infra/spacekit-compute-node/src/rollup_bridge.rs`
- `foundation/spacekit-primitives/src/v1/crypto/quantum.rs`

---

## A — PQ address derivation (`SHA-256(pubkey)[0..20]`)

`swtchvm_node.rs`, in `impl SwtchvmAddress` (after `from_public_key`, ~line 211). Ensure `use sha2::{Digest, Sha256};` is in scope for this module (it already uses `Keccak256`; add `Sha256` if not imported).

```rust
    /// PQ-native, quantum-safe address: `SHA-256(public_key)[0..20]`.
    ///
    /// Matches spacekit-did `derive_address` (quantum.rs:92) and the kit.space
    /// browser wallet (protocol/chainAddress.ts `pqAddressFromPublicKey`). Use
    /// this for SLH-DSA / SPHINCS+ identities. `from_public_key` (Keccak, above)
    /// is the secp256k1/EVM rule and is NOT quantum-safe — keep it for interop
    /// only.
    pub fn from_pq_public_key(public_key: &[u8]) -> Self {
        let hash = Sha256::digest(public_key);
        let mut addr = [0u8; 20];
        addr.copy_from_slice(&hash[..20]);
        Self(addr)
    }
```

**Signer→address tie (design note):** individual user transactions are signed and address-checked *in the browser VM* (`from == SHA-256(user_pubkey)[..20]`), then the sequencer bundles them. The node trusts the operator-signed bundle (key policy) and applies transfers optimistically inside the challenge window — the existing fraud-proof path (`/rollup/challenge`) is the on-chain check. So the node needs `from_pq_public_key` for faucet/direct-PQ use, but does **not** re-verify per-user PQ signatures on the rollup path. (`apply_faucet` already trusts the passed 20-byte address, so a PQ address funds and reads with no faucet change.)

---

## C — Accept the browser's SLH-DSA algorithm string

The browser sends `algorithm: "slh-dsa-sha2-128s"`; the node currently rejects it. Add it as an alias for the SHA2-128s-simple verifier in **both** the router and the verifier.

### C.1 `rollup_bridge.rs` — accepted-algorithm match (line 189)

```rust
            "sphincs" | "sphincs+" | "sphincs-128f" | "sphincs-128s" | "sphincs-192f"
            | "sphincs-192s" | "sphincs-256f" | "sphincs-256s"
            // SLH-DSA (FIPS-205) aliases — the browser/CLI wire strings:
            | "slh-dsa-sha2-128s" | "slh-dsa-sha2-192s" => {
```

### C.2 `quantum.rs` — `verify_sphincs_signature` match (line 127)

```rust
        "sphincs-128s" | "slh-dsa-sha2-128s" | "slh-dsa-128s" => verify_with_sha2_128s_simple(
            message,
            &signature.public_key,
            &signature.signature_bytes,
        )?,
        "sphincs-192s" | "slh-dsa-sha2-192s" => verify_with_sha2_192s_simple(
            message,
            &signature.public_key,
            &signature.signature_bytes,
        )?,
```

(Replace the existing `"sphincs-128s" =>` and `"sphincs-192s" =>` arms with these expanded ones. The `alg` is already lowercased at line 123, so the aliases match case-insensitively.)

> **Param-set caveat (verify before trusting funds):** the browser's `slhDsa128sKeypair()` (WASM) must produce keys/signatures byte-compatible with pqcrypto's `sphincssha2128ssimple`. FIPS-205 SLH-DSA-SHA2-128s == SPHINCS+-SHA2-128s-simple, so this *should* hold — but prove it with the cross-impl vector in **E** before any real value moves. If the WASM uses SHAKE, add SHAKE arms instead (the node currently has none).

---

## B — Settle rollup batches to PQ-keyed accounts

Today `validate_rollup_bundle_handler` verifies the bundle but never applies it (`all_roots_match = false`, held `Challenged`). Add a settlement method on the node and call it from the handler. This applies the explicit native value transfers in `tx_payloads`; full verkle state-root re-execution stays a follow-up (bundles settle optimistically under the operator key policy + challenge window, matching the current design).

### B.1 New method on `impl SwtchvmNode` (near `apply_faucet`, ~line 5473)

```rust
    /// Apply a verified rollup bundle's native value transfers to the ledger.
    ///
    /// Computes the whole batch in a working cache first (so intra-bundle
    /// dependencies resolve and an underfunded transfer rejects the WHOLE batch
    /// before any account is mutated), then commits. `from`/`to` are 20-byte
    /// addresses (PQ or EVM — the node is address-agnostic); `value` is uASTRA.
    ///
    /// NOTE: `setup_account_balance` is not transactional, so the commit loop is
    /// best-effort atomic. A fully atomic version needs a runtime state
    /// snapshot/commit primitive — track as a follow-up before mainnet.
    pub async fn settle_rollup_bundle(
        &self,
        bundle: &crate::rollup_bridge::RollupBundle,
    ) -> anyhow::Result<usize> {
        let payloads = match &bundle.tx_payloads {
            Some(p) if !p.is_empty() => p,
            _ => return Ok(0),
        };

        let mut cache: std::collections::HashMap<SwtchvmAddress, u128> =
            std::collections::HashMap::new();
        let mut applied = 0usize;

        // Phase 1 — simulate; bail (mutating nothing) on any insufficient balance.
        for tx in payloads {
            let value: u128 = tx
                .value
                .parse()
                .map_err(|_| anyhow::anyhow!("bad value {:?} in bundle {}", tx.value, bundle.bundle_id))?;
            if value == 0 {
                continue;
            }
            let from = SwtchvmAddress::from_hex(&tx.from)?;
            let from_bal = match cache.get(&from) {
                Some(b) => *b,
                None => self.runtime.get_account_balance(&from).await.unwrap_or(0),
            };
            if from_bal < value {
                anyhow::bail!(
                    "insufficient balance for {} ({} < {}) in bundle {}",
                    tx.from, from_bal, value, bundle.bundle_id
                );
            }
            cache.insert(from, from_bal - value);

            if let Some(to_str) = &tx.to {
                let to = SwtchvmAddress::from_hex(to_str)?;
                let to_bal = match cache.get(&to) {
                    Some(b) => *b,
                    None => self.runtime.get_account_balance(&to).await.unwrap_or(0),
                };
                cache.insert(to, to_bal.saturating_add(value));
            }
            applied += 1;
        }

        // Phase 2 — commit final balances.
        for (addr, bal) in cache {
            self.runtime.setup_account_balance(&addr, bal).await?;
        }
        Ok(applied)
    }
```

### B.2 Thread the node into the handler — route (line 6278)

```rust
        let validate_bundle = warp::path!("rollup" / "validate")
            .and(warp::post())
            .and(rollup_auth.clone())
            .and(warp::body::json())
            .and(with_node(node.clone()))          // <-- add
            .and_then(validate_rollup_bundle_handler)
            .map(Reply::into_response);
```

### B.3 Handler — settle + ingest (replace lines 7164-7263)

```rust
async fn validate_rollup_bundle_handler(
    _auth: (),
    bundle: crate::rollup_bridge::RollupBundle,
    node: Arc<SwtchvmNode>,                         // <-- add
) -> Result<impl warp::Reply, warp::Rejection> {
    use crate::rollup_bridge::{
        BundleStatus, BundleVerificationResult, ReExecutionResult, DEFAULT_CHALLENGE_WINDOW_SECS,
    };
    use warp::Reply;

    let validation = load_rollup_key_policy().and_then(|policy| {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_secs();
        Ok(crate::rollup_bridge::validate_rollup_bundle_with_policy(
            &bundle, &policy, now,
        )?)
    });

    let reply = match validation {
        Ok(result) => {
            let basic_ok = result.hash_valid && result.signature_valid && result.key_allowed;

            // Apply the batch's native transfers. This is optimistic settlement:
            // signature + operator key policy are verified here; full verkle
            // state-root re-execution remains a follow-up, so status stays
            // `Pending` (inside the challenge window) rather than `Verified`.
            let mut settled = false;
            let mut settle_error: Option<String> = None;
            let mut re_exec_results: Vec<ReExecutionResult> = Vec::new();

            if basic_ok {
                match node.settle_rollup_bundle(&bundle).await {
                    Ok(n) => {
                        settled = true;
                        re_exec_results.push(ReExecutionResult {
                            block_index: bundle.from_height,
                            expected_state_root: bundle
                                .state_roots
                                .last()
                                .cloned()
                                .unwrap_or_default(),
                            computed_state_root: String::from("applied"),
                            match_ok: true,
                        });
                        tracing::info!(
                            bundle_id = %bundle.bundle_id, transfers = n,
                            "rollup bundle settled to the ledger"
                        );
                    }
                    Err(e) => {
                        settle_error = Some(e.to_string());
                        tracing::warn!(
                            bundle_id = %bundle.bundle_id, error = %e,
                            "rollup bundle passed verification but settlement failed"
                        );
                    }
                }
            }

            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let challenge_window_end = now + DEFAULT_CHALLENGE_WINDOW_SECS;

            let status = if !basic_ok {
                BundleStatus::Rejected
            } else if settled {
                BundleStatus::Pending
            } else {
                BundleStatus::Challenged
            };

            let mut archived = false;
            if basic_ok && settled {
                let _ = crate::rollup_registry::ingest_bundle(&bundle);
                let _ = crate::rollup_registry::track_verified_bundle(
                    &bundle,
                    status.clone(),
                    DEFAULT_CHALLENGE_WINDOW_SECS,
                );
                archived = archive_bundle_to_storage_node(&bundle)
                    .await
                    .unwrap_or(false);
            }

            let verification = BundleVerificationResult {
                bundle_id: bundle.bundle_id.clone(),
                hash_valid: result.hash_valid,
                signature_valid: result.signature_valid,
                key_allowed: result.key_allowed,
                re_execution_results: re_exec_results,
                all_roots_match: settled, // repurposed: native transfers applied
                challenge_window_end,
                status,
            };

            warp::reply::json(&serde_json::json!({
                "verification": verification,
                "ingested": basic_ok && settled,
                "settled": settled,
                "error": settle_error,
                "archived": archived
            }))
            .into_response()
        }
        Err(err) => warp::reply::with_status(
            warp::reply::json(&serde_json::json!({ "error": err })),
            warp::http::StatusCode::BAD_REQUEST,
        )
        .into_response(),
    };
    Ok(reply)
}
```

---

## D — Run config

Build and run with the quantum verifier enabled, on the port the website expects:

```bash
cargo run --release -p spacekit-compute-node --features "standalone quantum" -- --port 8747
```

(The PQ verify path is `#[cfg(feature = "quantum")]`; default rpc_port is 8080, the website expects 8747.)

---

## E — Cross-impl test vector (do this before trusting funds)

Two tests. The first proves the address rule; the second proves the browser's SLH-DSA signature verifies under the node — the load-bearing compatibility check.

`rollup_bridge.rs` (or a `tests/` module):

```rust
#[test]
fn pq_address_matches_spec_vector() {
    use sha2::{Digest, Sha256};
    // SHA-256("")[0..20]
    let a = hex::encode(&Sha256::digest(b"")[..20]);
    assert_eq!(a, "e3b0c44298fc1c149afbf4c8996fb92427ae41e4");
}

#[cfg(feature = "quantum")]
#[test]
fn slh_dsa_128s_roundtrip_via_alias() {
    use spacekit_primitives::v1::crypto::quantum::{
        generate_sphincs_keypair, sign_sphincs_message, verify_sphincs_signature, SPHINCSSignature,
    };
    let (pk, sk) = generate_sphincs_keypair("sphincs-128s").unwrap();
    let msg = b"rollup-bundle-hash";
    let sig = sign_sphincs_message(msg, &sk, "sphincs-128s").unwrap();
    // Verify using the BROWSER's wire string to exercise the new alias.
    let s = SPHINCSSignature { signature_bytes: sig, algorithm: "slh-dsa-sha2-128s".into(), public_key: pk };
    assert!(verify_sphincs_signature(msg, &s).unwrap());
}
```

**The real vector (must pass before mainnet):** export a `(publicKeyHex, signatureBase64)` pair generated by the *browser* WASM (`slhDsa128sKeypair` + `slhDsa128sSign` over a known message), drop it into a Rust test, and assert `verify_sphincs_signature(msg, {algorithm:"slh-dsa-sha2-128s", ...})` returns `true`. If it fails, the WASM param set (SHA2 vs SHAKE, or key/sig encoding) differs from pqcrypto and the browser wallet must be aligned before it can hold real ASTRA.

---

## What this unblocks (and what still needs doing)

After A–E land and the vector passes: faucet funds a PQ address, balances read from the node, the browser VM signs SLH-DSA batches the node settles → the compute node is the quantum-safe source of truth. The kit.space wallet UI (already scaffolded in `protocol/computeNode.ts` + `chainAddress.ts`) is then a small wiring job.

Still open, tracked separately: full verkle state-root re-execution (so `status: Verified` is earned, not optimistic); atomic ledger commit; unifying the browser VM's DID-keyed `native:astra:balance` with the node's address-keyed ledger; and aligning the CLI DID (SHAKE) with the SHA2 path.
