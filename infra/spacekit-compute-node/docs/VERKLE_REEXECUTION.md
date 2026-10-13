# Verkle re-execution for rollup settlement (review-ready draft)

Goal: let `validate_rollup_bundle_handler` mark a bundle `Verified` (not optimistic `Pending`) by re-executing its transfers and proving the resulting state matches the roots the sequencer committed — and reject/slash on mismatch.

**The blocker this must design around:** the node's `SwtchvmState::state_root()` (`swtchvm_node.rs:519`) and its merkle fallback (`state_merkle_entries`, `:6519`) hash only **contract storage + `contract_kv`** — **not account balances, nonces, or code**. `execute_transaction` moves balances (`:1465-1486`) but that never changes `state_root()`. So a batch of pure ASTRA transfers leaves the state root **unchanged**, and comparing state roots would validate nothing about the money. Re-execution is meaningless until **accounts are committed to a root**.

Rather than change the existing consensus `state_root()` (a hard fork that rewrites every historical block's root), commit accounts to a **separate account root** carried in the bundle's existing `quantum_state_roots: Vec<String>` array (parallel per-block, `rollup_bridge.rs:60`). This is additive and non-consensus-breaking.

---

## 1. `SwtchvmState::account_root()` — deterministic account commitment

`swtchvm_node.rs`, `impl SwtchvmState` (near `state_root`, ~line 519). Deterministic: sort by address, hash `address ‖ balance_le ‖ nonce_le ‖ code_hash`, fold with a domain separator.

```rust
    /// Deterministic commitment to ALL account state (balance, nonce, code).
    /// Separate from `state_root()` (which covers only contract storage), so it
    /// can be added without changing the existing consensus root. The browser
    /// sequencer MUST compute this identically (same field order, LE u128/u64,
    /// SHA-256(code)) — see the determinism vector below.
    pub fn account_root(&self) -> String {
        let mut addrs: Vec<&SwtchvmAddress> = self.accounts.keys().collect();
        addrs.sort();
        let mut hasher = Sha256::new();
        hasher.update(b"SPACEKIT-ACCOUNT-ROOT-v1\n");
        for addr in addrs {
            let acct = &self.accounts[addr];
            hasher.update(addr.as_bytes());
            hasher.update(&acct.balance.to_le_bytes());          // u128 LE
            hasher.update(&acct.nonce.to_le_bytes());            // u64 LE
            let code_hash = match &acct.code {
                Some(code) => Sha256::digest(code).to_vec(),
                None => vec![0u8; 32],
            };
            hasher.update(&code_hash);
        }
        format!("0x{}", hex::encode(hasher.finalize()))
    }
```

---

## 2. Verify-then-settle on a cloned state

`SwtchvmState` is `Clone` (`:441`, deep copy) — the only fork primitive. Apply the batch to a clone, compute its `account_root()`, compare to the committed root, and only settle the live ledger on a match.

`impl SwtchvmNode`, replacing the optimistic `settle_rollup_bundle` call site. New method:

```rust
    /// Re-execute a bundle's native transfers against a CLONE and check the
    /// resulting account root against the sequencer's committed root.
    ///
    /// Returns:
    ///   Ok(Some(true))  — committed account root present and MATCHES → safe to settle as Verified
    ///   Ok(Some(false)) — committed root present but MISMATCHES  → do NOT trust (see rollout)
    ///   Ok(None)        — no committed account root → fall back to optimistic settlement
    pub async fn verify_rollup_bundle_roots(
        &self,
        bundle: &crate::rollup_bridge::RollupBundle,
    ) -> anyhow::Result<Option<bool>> {
        let committed = match bundle.quantum_state_roots.as_ref().and_then(|v| v.last()) {
            Some(r) => r.clone(),
            None => return Ok(None),
        };

        // Clone current state and apply the batch's transfers to the copy.
        let mut scratch = { self.runtime.get_state().read().await.clone() };
        if let Some(payloads) = &bundle.tx_payloads {
            for tx in payloads {
                let value: u128 = tx.value.parse().unwrap_or(0);
                if value == 0 { continue; }
                let from = SwtchvmAddress::from_hex(&tx.from)?;
                {
                    let a = scratch.get_account_mut(&from);
                    if a.balance < value { anyhow::bail!("reexec: insufficient balance for {}", tx.from); }
                    a.balance -= value;
                }
                if let Some(to_str) = &tx.to {
                    let to = SwtchvmAddress::from_hex(to_str)?;
                    scratch.get_account_mut(&to).balance =
                        scratch.get_account_mut(&to).balance.saturating_add(value);
                }
            }
        }

        let computed = scratch.account_root();
        Ok(Some(normalize_root(&computed) == normalize_root(&committed)))
    }
```

Helper (root strings are inconsistent across the node — `verkle:…`, `0x…`, bare hex; normalize before comparing):

```rust
fn normalize_root(s: &str) -> String {
    s.trim().trim_start_matches("verkle:").trim_start_matches("0x").to_ascii_lowercase()
}
```

---

## 3. Handler — gate settlement on the root check

In `validate_rollup_bundle_handler` (after `basic_ok`), replace the optimistic-only settle with verify-then-settle:

```rust
            let mut settled = false;
            let mut verified = false;
            let mut settle_error: Option<String> = None;
            let mut re_exec_results: Vec<ReExecutionResult> = Vec::new();

            if basic_ok {
                match node.verify_rollup_bundle_roots(&bundle).await {
                    // Committed root present and matches → verified settlement.
                    Ok(Some(true)) => {
                        verified = true;
                        match node.settle_rollup_bundle(&bundle).await {
                            Ok(_) => settled = true,
                            Err(e) => settle_error = Some(e.to_string()),
                        }
                        re_exec_results.push(ReExecutionResult {
                            block_index: bundle.to_height,
                            expected_state_root: bundle.quantum_state_roots
                                .as_ref().and_then(|v| v.last().cloned()).unwrap_or_default(),
                            computed_state_root: String::from("match"),
                            match_ok: true,
                        });
                    }
                    // ROLLOUT: while the browser↔node account_root determinism
                    // vector is unproven, treat a mismatch as a determinism bug,
                    // NOT fraud — fall back to optimistic settlement and warn.
                    // Once the vector passes, change this arm to REJECT + slash.
                    Ok(Some(false)) => {
                        tracing::warn!(bundle_id = %bundle.bundle_id,
                            "account_root mismatch — settling optimistically pending determinism proof");
                        if let Ok(_) = node.settle_rollup_bundle(&bundle).await { settled = true; }
                    }
                    // No committed root → optimistic settlement (current behavior).
                    Ok(None) => {
                        if let Ok(_) = node.settle_rollup_bundle(&bundle).await { settled = true; }
                    }
                    Err(e) => settle_error = Some(e.to_string()),
                }
            }

            let status = if !basic_ok {
                BundleStatus::Rejected
            } else if verified && settled {
                BundleStatus::Verified          // <- earned, not optimistic
            } else if settled {
                BundleStatus::Pending
            } else {
                BundleStatus::Challenged
            };
```

(Keep the ingest/archive gate on `basic_ok && settled`; update the JSON response to include `"verified": verified`.)

---

## 4. Who commits the root — a FULL-STATE sequencer, not the browser

Important: `account_root()` hashes **all** accounts in the node's state. Only a party holding the whole account set can reproduce it. A **browser VM cannot** — it knows only its own DID-keyed balances, so it can never emit a matching full-state root. Therefore:

- **Browser-pushed batches** carry transfers but no `quantum_state_roots` → `verify_rollup_bundle_roots` returns `Ok(None)` → they **settle as `Pending`** (re-executed and balance-checked by `settle_rollup_bundle`, held in the challenge window). This is the correct, honest outcome for browser-originated batches — the node's own re-execution is their validation.
- **`Verified`** is reserved for an **operator sequencer running a full node** (the rollup path is already operator-key-gated): it holds full state, computes `account_root` byte-identically (accounts sorted by 20-byte address, `SHA-256("SPACEKIT-ACCOUNT-ROOT-v1\n" ‖ Σ(address ‖ balance_le_u128 ‖ nonce_le_u64 ‖ code_hash))`, `0x`-hex), and commits it in `quantum_state_roots`. The node re-executes on a clone and confirms the match.

So the "client counterpart" lives in the **operator/sequencer node code** (Rust or a full spacekit-js node), not in the browser. The two-tier flow: browser VMs → operator sequencer (full state, commits `account_root`) → compute node (`Verified` on match).

---

## 5. Determinism vector (gate before flipping mismatch → reject)

The money-critical check, analogous to the SLH-DSA vector: build a fixed set of accounts + transfers, compute `account_root` in **both** the Rust node and the browser sequencer, and assert they're equal. Only after this passes should the handler's `Ok(Some(false))` arm be changed from "warn + optimistic settle" to "reject + slash" — otherwise a serialization difference would freeze or wrongly slash honest sequencers.

```rust
#[test]
fn account_root_is_deterministic() {
    // Seed a known state, apply a known transfer, assert account_root equals the
    // value the browser sequencer computes for the same inputs (paste the JS
    // output here once generated).
    // let s = SwtchvmState::with_accounts([...]);
    // assert_eq!(s.account_root(), "0x<value from the browser>");
}
```

---

## Why not change the global `state_root()` instead

Folding accounts into the existing verkle `state_root()` would be cleaner conceptually but is a **consensus hard fork**: every historical block's `state_root` would change, breaking stored roots, witnesses, and any anchored L1 checkpoints. The separate `account_root` in `quantum_state_roots` gets verifiable settlement now without rewriting chain history; folding accounts into the canonical root can follow as a planned network upgrade.

## Sequence to land this safely

1. Add `account_root()` + `verify_rollup_bundle_roots()` + handler gate (this draft) — additive; bundles without the root still settle `Pending`.
2. Add the client-side `account_root` to the spacekit-js sequencer.
3. Run the determinism vector (§5); fix serialization until Rust == JS.
4. Flip the mismatch arm to reject + slash. Now `Verified` means "re-executed and roots matched."
5. (Later, planned upgrade) fold accounts into the canonical `state_root()`.
