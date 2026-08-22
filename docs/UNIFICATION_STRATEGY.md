# SpaceKit Unification Strategy

**Document type:** Technical specification (identity, onboarding, deposits, client–node sync)  
**Status:** Draft for implementation alignment  
**Scope:** `spacekit.xyz-website`, `@spacekit/spacekit-js`, `spacekit-compute-node`, `spacekit-simulator`, future L1 vaults (Ethereum, BSC, Solana, Bitcoin)

This document merges **architectural unification** (identity, rollup sync, vault minting) with the **Progressive Trust Onboarding** state machine below. Where they overlap, the state machine governs **UX gating and sequencing**; the architecture sections govern **protocol semantics**.

---

## 1. Goals

1. **Minimum friction for Ethereum users:** Connect MetaMask / WalletConnect (already in `WalletConnectLanding.tsx` and Agent Hub via wagmi/RainbowKit) and obtain a **usable SpaceKit identity** without upfront seed-phrase friction.
2. **Progressive trust:** Users explore SpaceKit **before** heavy security steps; **PQ key backup is deferred** until the user attempts a first deposit **at or above $10** (see §3).
3. **Single logical user:** One **DID** is the canonical SpaceKit account; **external chain addresses** are **linked controllers**, not separate accounts.
4. **Spend and meter on SpaceKit:** High-frequency use (e.g. per-call API spend) is enforced on **SpaceKit compute / VM state**; **custody and deposits** use **external chain vaults** (USDC/USDT on Ethereum or BSC, later Solana programs, etc.).
5. **Path to multi-chain:** Same linking pattern for **Solana** and **Bitcoin** (address + signature proof); enabled from **State 5** onward in the progressive model.
6. **Client + node coherence:** **spacekit-js** is a **client sequencer / rollup participant**; **spacekit-compute-node** is **always canonical for balances**. The client **never** wins a conflict with the node.

---

## 2. Identity model: Ethereum (and friends) ↔ SpaceKit DID

### 2.1 Terminology

| Term | Meaning |
|------|--------|
| **Primary SpaceKit identity** | `did:spacekit:…` — canonical id for balances, storage ACLs, agent contracts, explorer. |
| **Linked wallet** | External address (e.g. `eip155:1:0xabc…`) cryptographically bound to that DID. |
| **Display handle** | Human-chosen name for UX (and optional resolver); not a substitute for DID uniqueness. |
| **DID preview** | Client-generated identifier **before** registrar confirmation (State 2 only); **not** canonical and must not be copy-promoted as an address. |

### 2.2 Quantum-resistant wallet and backup timing (progressive model)

**Do not** treat “derive SpaceKit keys from secp256k1 alone” as sufficient for long-term quantum resistance.

1. **PQ keypair (Kyber / Dilithium or project-standard PQ suite)**  
   - Generated in the **browser** (WASM / Web Crypto) when the user reaches the **backup-secured** milestone.  
   - **Mandatory PQ backup is deferred** until the user’s **first deposit attempt at or above $10** (State 4 → 5 transition). Until then, the user operates under **wallet-linked session** and **registry-backed DID** without full self-custody PQ backup (States 2–4).

2. **Binding to Ethereum (UX: “manage SpaceKit with MetaMask”)**  
   - User signs a **single EIP-712 batch** (State 2 → 3) containing link attestation **and** **session grant**: `{ did, chainId, address, nonce, expiry, sessionGranted }`.  
   - Registry stores **`link(eth_address, did, signature)`**.  
   - **One signature pop-up** for link + session; no second prompt for session alone.

3. **KEK from wallet (post–State 5)**  
   - After PQ key generation, a **KEK** may be derived from `personal_sign(challenge)` **only to encrypt** the PQ private key blob locally (MetaMask never holds Kyber secrets in clear).  
   - User completes **download encrypted keystore** OR **recovery phrase with confirmation** before the triggering **≥ $10** deposit completes.

**Future chains (State 5+)**

- **Solana:** `ed25519` signature over the same structured payload; store `solana:<pubkey>` in the linkage table.  
- **Bitcoin:** BIP-322 signmessage; store `bip322:<address>`.  
- **Policy:** Which link type is required for which operation (e.g. Ethereum-only for USDC deposits).

### 2.3 Ethereum RPC bridge (simulator / dev)

`spacekit-simulator` exposes an **Ethereum JSON-RPC surface** that maps **MetaMask-style calls** into **SpaceKit compute state** (e.g. SWTCHX). That is **compatibility for tooling**, not Ethereum mainnet custody. Production linking uses **real L1/L2 RPC + EIP-712** as above, plus registrar on SpaceKit or a dedicated contract.

---

## 3. Progressive trust onboarding (state machine)

### 3.1 Design principles

- Users **explore without upfront friction**; trust-gated capabilities unlock with **natural actions** (connect wallet, register DID, sync node, deposit).  
- **States 2 and 3 are distinct:** link attestation (user action) vs node RPC sync (automatic) — aids debugging and progressive UI disclosure.  
- **State 3 → 4** is **fully automatic** (no extra user action) once registrar and RPC are available — reduces re-engagement friction.  
- **Transitions are forward-only.** If the client is inconsistent (wagmi cleared, `localStorage` wiped), the client **re-syncs from last confirmed node state**; it does **not** regress the user through earlier “states” in a way that implies loss of on-chain/registry facts.  
- **Compute-node is always canonical** for balances; reconciliation: **node wins** (see §5.2 Phase C).

### 3.2 State overview

| State | Name | Summary |
|-------|------|--------|
| **1** | Unconnected | Browse only; no wallet, no DID, no VM. |
| **2** | Wallet linked | Demo VM + faucet + DID **preview**; EIP-712 session; no canonical DID / no real node balance / no deposits. |
| **3** | DID registered | Canonical DID in registry; Phase A read path; micro-payments; handle registration; no deposits **> $10**, no withdrawals, no full self-custody. |
| **4** | Balance synced | Confirmed node balance; small deposits **≤ $10**; Phase B write path + vault + proof_bridge commitments; still no large deposits / withdrawals until backup. |
| **5** | Backup secured | PQ backup done; large deposits; withdrawals; multi-chain linking; production execution policy as defined by product. |

### 3.3 State definitions

#### State 1: Unconnected

| Unlocked | Blocked |
|----------|---------|
| Browse public site and documentation | API access, balance visibility, faucet, deposits |

**Transition → State 2:** User connects wallet (wagmi session active).

**Implementation:** No wagmi session; no `spacekit:identityDid` in `localStorage`; `SpacekitVmContext` not initialised for privileged flows.

**UI:** Prominent “Connect wallet” CTA; **no** balance display; **no** demo VM access.

---

#### State 2: Wallet linked

| Unlocked | Blocked |
|----------|---------|
| Local / demo VM, testnet faucet, **DID preview** (non-canonical), Agent Hub exploration | Canonical DID, real node balance, deposits, PQ key operations |

**Transition → State 3:** User completes **EIP-712** link attestation + session grant; registrar persists DID + link; `spacekit:identityDid` becomes **canonical**.

**Implementation notes**

- `useAccount` active; `spacekit:identityDid` may hold **preview** value until registrar confirms — then same key holds canonical DID.  
- Local ASTRA seed in `SpacekitVmContext` — **always** labeled **Demo** in UI.  
- **DID preview:** render **non-copyable** (grayed / italic + “Preview” label). Users habitually copy address-like strings; do not present preview as a copy field.

**UI:** **Demo** badge on **all** balance figures; persistent nudge: “Complete identity setup to unlock real balances.”

---

#### State 3: DID registered

| Unlocked | Blocked |
|----------|---------|
| Canonical DID in registry, **micro-payments** (metered on compute-node), **node balance read-only**, handle registration | Deposits **> $10**, withdrawals, full self-custody |

**Transition → State 4:** Automatic when compute-node RPC returns **confirmed** account state (Phase A complete for this session).

**Implementation notes**

- Registration API or on-chain registrar stores DID document fragment + linked address.  
- **Phase A (read path):** on each wallet connect, client queries compute-node RPC for **balance + nonce** (and DID mapping if applicable).  
- Balance label: **“Syncing…”** until node responds with confirmed state.  
- Metering: SWTCHVM host functions (`msg_value`, `get_balance`, `transfer`).

**UI:** Replace **Demo** with **Syncing…** until confirmed; then show **real** balance **without** Demo/Syncing badge. Enable handle registration. Nudge: “Secure your wallet to unlock deposits.”

---

#### State 4: Balance synced

| Unlocked | Blocked |
|----------|---------|
| Real-time compute-node balance, **small deposits (≤ $10 per transaction)**, per-call metering, **RollupBundle** submission (Phase B), vault events + `proof_bridge` state roots | Large deposits **(> $10)**, withdrawals until State 5 |

**Transition → State 5:** User completes **PQ backup flow** triggered by **first deposit attempt ≥ $10** (backup must complete **before** that deposit transaction completes).

**Implementation notes**

- **Phase B (write path):** `SpacekitSequencer` submits signed **RollupBundles** to SpaceKit **ingress API**; on mismatch, client **drops or replays** from last committed height.  
- Vault contracts accept USDC/USDT (and configured assets) up to **$10 per tx** at this tier.  
- `proof_bridge.ts` Ethereum adapter active for **state root** commitments.  
- LayerZero (`layerzero_bridge.rs`): validate config; **trusted relayer** acceptable for mint until production bridge is relied upon.

**UI:** No Demo or Syncing; deposit UI for **≤ $10** only. Nudge: “Secure your wallet to unlock larger deposits and withdrawals.”

---

#### State 5: Backup secured (terminal)

| Unlocked | Blocked |
|----------|---------|
| Full self-custody, **uncapped** USDC/USDT deposits (per product risk limits), **withdrawals**, Solana/Bitcoin linking, production execution as defined | — |

**Implementation notes**

- Bridge/indexer: `Deposit` events `(payer, amount, asset, destination_did_hash, nonce)`; mint ASTRA to DID address on compute-node; idempotency `(source_chain, tx_hash, log_index)`.  
- Relayer trust: **disclosed in-product** (operator, upgrade keys).  
- Phase B fully required for production bundle submission where policy demands.

**UI:** Remove secure-wallet nudges; full deposit and withdrawal UI; multi-chain linking visible; settings link to relayer trust disclosure.

---

### 3.4 Transition matrix

| From | To | Trigger |
|------|-----|---------|
| 1 | 2 | Wallet connect |
| 2 | 3 | EIP-712 link + session; registrar success |
| 3 | 4 | Node RPC confirms balance (automatic) |
| 4 | 5 | PQ backup completed (gate for ≥ $10 deposit) |

Valid transitions are **forward-only** in the product sense above; **client cache loss** triggers **re-derivation of effective state from node + registry**, not arbitrary backward UX.

### 3.5 UI state indicators (invariants)

Users must always know which state they are in.

- **State 2:** Every balance shows **Demo** badge; DID preview is **not** copy-as-address.  
- **State 3:** **Syncing…** until RPC confirms; then real balance, no badge.  
- **State 4–5:** No Demo/Syncing on authoritative balance.  
- **Never** show two balance numbers **without** explicit labels (“Demo local” vs “Node”) — preferably **only show node balance** once State 3+ confirmed, and **hide or clearly separate** demo balance.  
- **No** balance figure that could be mistaken for **confirmed real** balance **before** Phase A sync completes (State 3).

### 3.6 Handles and DID shape (unchanged policy)

- **DID:** stable opaque id, e.g. `did:spacekit:<env>:user:<uuid-or-hash>` (`mainnet` | `testnet` | `dev`).  
- **Handle:** separate registration at State 3+; resolver maps handle → DID.  
- URLs like `spacekit:testnet:user-x:…` should treat **user-x** as handle or opaque path, not the sole source of truth for keys.

---

## 4. Deposits: USDC / USDT / BSC → ASTRA credits

### 4.1 Economic split

| Layer | Role |
|-------|------|
| **Solidity (Ethereum / BSC)** | Vault custody; **Deposit** events with `(payer, amount, asset, destination_did_hash, nonce)`. **Per-tx cap ≤ $10** until State 5 policy unlocks uncapped (contract + UI must agree). |
| **Bridge / indexer / relayer** | Watches events, **anti-replay**, mint credits on SpaceKit. |
| **spacekit-compute-node / SWTCHVM** | **Canonical** ASTRA balance; execution + metering. |

### 4.2 Mint semantics

- Define **credit units** (e.g. 6-decimal USD peg or native ASTRA decimals).  
- **Idempotency:** `(source_chain, tx_hash, log_index)`.  
- **Destination:** DID’s `SwtchvmAddress` (see `WALLET_DID_SYSTEM.md`).

### 4.3 LayerZero / Alloy module

`spacekit-compute-node` — `layerzero_bridge.rs` as transport when production-ready; until then **trusted relayer** with identical idempotent mint rules.

---

## 5. spacekit-js ↔ spacekit-compute-node: “rollup-style” sync

### 5.1 Roles

- **spacekit-js:** Local VM + sequencer; **demo / optimistic** state in early states.  
- **spacekit-compute-node:** **Canonical** balances and execution.

### 5.2 Phases (mapped to states)

| Phase | Description | Typical state |
|-------|-------------|---------------|
| **A — Read** | Client queries node RPC for balance + nonce on connect | 3 → 4 |
| **B — Write** | Signed `RollupBundle` to ingress API; `proof_bridge` commitments | 4+ |
| **C — Reconciliation** | On mismatch, **node wins**; client replays or resets from last committed height | Always |

### 5.3 Proof bridge vs balance mint

- **Proof bridge:** commitments / dispute / interoperability — **not** a substitute for vault minting.  
- Use **both** where product requires L1 attestations and vault-sourced credits.

---

## 6. Payments for services (micro-metering)

- **Deposits / refunds:** External vaults.  
- **Per-call metering (e.g. $0.002):** SpaceKit compute / WASM host path.  
- **Storage receipts:** Register artifact manifest with compute node or registry so billing matches deployed bytes.

---

## 7. Current implementation touchpoints (audit checklist)

| Area | Location / behavior |
|------|---------------------|
| EVM connect | `spacekit.xyz-website` — wagmi, `WalletConnectLanding.tsx`, Agent Hub |
| SpaceKit DID | `localStorage` `spacekit:identityDid`, `SpacekitClient` / `useSpacekitClient` |
| Local native ASTRA | `SpacekitVmContext` — `ensureNativeBalance` / `native:astra:balance:${did}` → must align with **State 2 Demo** rules |
| VM payment primitives | `spacekit-compute-node` `swtchvm_node.rs` |
| Cross-chain plumbing | `layerzero_bridge.rs` |
| Client attestations | `spacekit-js` — `proof_bridge.ts`, `proof_bridge_service.ts` |
| Reference | `spacekit-simulator/WALLET_DID_SYSTEM.md` |

---

## 8. Security and product notes

- **Linked ETH ≠ custodied ETH on SpaceKit** — clear chain labeling in UI by state.  
- **Relayer trust** — disclosed in settings from State 5.  
- **Handle squatting** — reserved names, rate limits, optional stake.  
- **Strict path variants** (e.g. separate payment link) remain available for high-risk SKUs without breaking the progressive default.

---

## 9. Implementation checklist (production readiness)

### State 1 → 2: Connect wallet

- [ ] wagmi + MetaMask + WalletConnect configured.  
- [ ] DID **preview** generated client-side (**not** in registry).  
- [ ] `SpacekitVmContext` local ASTRA seed with **Demo** enforced in UI.  
- [ ] **Demo** badge on all balance components.  
- [ ] DID preview non-copyable (grayed / italic / “Preview”).

### State 2 → 3: Sign link attestation

- [ ] EIP-712 payload: `did`, `chainId`, `address`, `nonce`, `expiry`, `sessionGranted`.  
- [ ] **One** signature for link + session.  
- [ ] Registration API or on-chain registrar after verify.  
- [ ] `spacekit:identityDid` canonical in `localStorage` after success.  
- [ ] Balance UI: **Demo** → **Syncing…**

### State 3 → 4: Node RPC sync (automatic)

- [ ] Phase A: query compute-node RPC on wallet connect.  
- [ ] **Syncing…** until response; then real balance, no badge.  
- [ ] Handle registration UI enabled.  
- [ ] Nudge: secure wallet for deposits (non-blocking).

### State 4 → 5: PQ backup (deferred)

- [ ] Backup flow opens on **first deposit attempt ≥ $10** only.  
- [ ] PQ keygen in browser (WASM / Web Crypto).  
- [ ] KEK from `personal_sign`; encrypted keystore **or** phrase + confirmation.  
- [ ] **Backup must complete before** triggering ≥ $10 deposit succeeds.  
- [ ] Phase B: ingress + `RollupBundles`; vault uncapped (policy); withdrawals; proof_bridge; settings disclosure for relayer.

### Cross-cutting: balance invariants

- [ ] State 2: no balance without **Demo** badge.  
- [ ] State 3: no “real” balance display before sync confirmation.  
- [ ] Never two balances without explicit labels; **node authoritative** always.

---

## 10. Next concrete steps (execution roadmap)

Ordered for **dependency flow** and **visible user value**:

1. **Introduce explicit onboarding state in the website**  
   - Single source of truth (e.g. React context or small store): `unconnected | walletLinked | didRegistered | balanceSynced | backupSecured`, derived from `wagmi` + `localStorage` + **registrar response** + **RPC health**.  
   - Refactor `SpacekitVmContext` so **demo ASTRA** is only seeded when `state >= walletLinked` and **every** balance consumer reads the **badge** from this store.

2. **EIP-712 link + session batch + registrar stub**  
   - Implement typed data and **one** `signTypedData` call.  
   - Stand up a minimal **registrar** (server or test contract) that verifies signature and returns **canonical DID**; persist link server-side or on-chain.  
   - Until registrar exists, **do not** promote `spacekit:identityDid` as canonical (stay in State 2 semantics).

3. **Phase A: compute-node balance RPC**  
   - Expose or consume existing JSON-RPC / HTTP method for **balance + nonce** by SpaceKit address derived from DID.  
   - On connect + after State 3, poll or subscribe until success → transition UI to State 4.  
   - **Remove or hide** demo balance when node balance is shown (or dual-label only during explicit debug mode).

4. **Vault + policy for ≤ $10 (State 4)**  
   - Smart contract **per-tx cap** and UI validation aligned with progressive spec.  
   - Relayer mint with idempotency key; index **Deposit** events.

5. **PQ backup gate (State 4 → 5)**  
   - Deposit flow: if `amount >= $10` (in configured decimals), **interrupt** to backup wizard; on success, allow tx broadcast.  
   - Implement keystore download + phrase path; wire KEK from `personal_sign`.

6. **Phase B: RollupBundle ingress**  
   - Define ingress API on `spacekit-compute-node` (verify batch signature, ordering, DID/session).  
   - Wire `SpacekitSequencer` in `spacekit-js` to submit bundles when `state >= balanceSynced` and policy allows.

7. **proof_bridge + disclosure**  
   - Enable Ethereum adapter for state roots per environment.  
   - Settings page: relayer operator, contract addresses, upgrade key policy.

This document should be updated when registrar contracts, ingress APIs, and vault caps are finalized.
