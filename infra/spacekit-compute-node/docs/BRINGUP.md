# ASTRA bring-up — validate the quantum-safe path end to end

One pass: build the node with PQ verification, run the tests, start it on `:8747`, and claim ASTRA from the website wallet so the compute-node balance and the feed balance line up.

## 1. Compile + test the compute node (with quantum on)

```bash
cd spacekit-core
cargo check -p spacekit-compute-node --features "standalone quantum"
cargo test  -p spacekit-compute-node --features "standalone quantum" \
  -- pq_address_matches_spec_vector slh_dsa_128s_alias_verifies
```

`pq_address_matches_spec_vector` proves the address rule; `slh_dsa_128s_alias_verifies` proves the node verifies an SLH-DSA-128s signature under the browser's `slh-dsa-sha2-128s` wire string. Both must pass before trusting the PQ path.

> If `--features quantum` still errors, confirm the `quantum = ["spacekit-primitives/quantum"]` line is in `infra/spacekit-compute-node/Cargo.toml` (added in this change).

## 2. Run the node on the port the website expects

```bash
cargo run --release -p spacekit-compute-node --features "standalone quantum" -- --port 8747
```

Smoke-check the REST + RPC surface:

```bash
curl -s -XPOST localhost:8747/rpc -d '{"jsonrpc":"2.0","id":1,"method":"eth_chainId","params":[]}'
# → {"jsonrpc":"2.0","id":1,"result":"0x539"}   (chain 1337)
```

## 3. Point the website at the node

`spacekit-web/kit.space-website/.env.local`:

```
VITE_SPACEKIT_COMPUTE_RPC=http://localhost:8747/rpc
VITE_SPACEKIT_COMPUTE_WS=ws://localhost:8747/ws
VITE_SPACEKIT_STORAGE_URL=http://localhost:3030
```

```bash
cd spacekit-web/kit.space-website && npm run dev
```

## 4. Claim + verify

1. Open the site → **Sign in / Create account** (mints an identity). For the quantum-safe address, run `upgradeToQuantum()` (or the UI upgrade) so the identity is SLH-DSA; a classical key still funds/reads, but only a quantum identity can sign the PQ settlement path.
2. Go to **Wallet** (`/wallet`). It shows your PQ address (`SHA-256(pubkey)[..20]`) and a `0` balance.
3. Click **Claim ASTRA** → the faucet credits your address (1 ASTRA, 1h cooldown). Balance updates to `1`.
4. Confirm on the node directly — the address is shown on the wallet card:

   ```bash
   curl -s localhost:8747/account/<0xYourPqAddress>
   # → {"address":"0x…","balance":"1000000","nonce":0,...}   (1_000_000 uASTRA = 1 ASTRA)
   ```
5. Open the **feed** — its balance now reads the same on-chain value (via `syncBalance` → `getAccount`), so wallet and feed agree. This is the unified balance.

## What "works" at this stage

- **Faucet + balance** are live and real (address-agnostic on the node, so this works today).
- **Feed and wallet share one balance.**
- **Settlement**: browser batches settle as `Pending` (re-executed + balance-checked in the challenge window). `Verified` requires a full-state operator sequencer that commits an `account_root` — see `VERKLE_REEXECUTION.md`.

## Still gated before real value / slashing

1. The **SLH-DSA determinism vector**: prove a browser-WASM-generated `(pubkey, signature)` verifies under the node (paste it into a Rust test) — confirms the WASM param set matches pqcrypto SHA2-128s-simple.
2. The **`account_root` determinism vector** (operator sequencer ↔ node) before flipping the Verkle mismatch arm to reject + slash.
3. Deploy the node (and storage/messaging) publicly and set the `VITE_SPACEKIT_*` envs to those hosts for a non-local launch.
