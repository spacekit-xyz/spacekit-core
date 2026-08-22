# SpaceKit compute node — operations runbook

This runbook covers deployment hygiene (secrets, edge, observability), supply-chain checks, failure drills, and incident response. It complements the checklist in the root `README.md`.

## 1. Identity and keys

- Keep `[identity]` aligned with the SpaceKit CLI: `~/.spacekit/keys/private_key.hex`, `public_key.hex`, and the same `did` string as in CLI config.
- Filesystem permissions: `chmod 600 ~/.spacekit/keys/private_key.hex` (and tighten directory permissions if multi-user).
- The node applies your configured **DID string** to the runtime wallet and loads **Kyber-family** CLI hex keys when present. **Signing** for legacy APIs still uses the embedded **SPHINCS+** keypair inside `QuantumResistantWallet`; plan registry and client flows accordingly until Kyber-native signing is unified.

## 2. Configuration safety

- Do not commit live RPC URLs, bridge addresses, or facilitator secrets. Use `config.local.toml` (gitignored) or environment-specific overlays.
- Prefer TLS termination at a reverse proxy (nginx, Envoy, cloud LB) in production; the standalone Warp server listens in cleartext by default.

## 3. Observability

- Enable **`[compute.production_metrics_config]`** in `config.toml` so the library-side `ProductionMetricsManager` can expose Prometheus-style metrics when that subsystem is started by `ComputeNode` (see `production_metrics.rs`).
- Use **`GET /health`**, **`GET /status`**, and **`GET /v1/node/identity`** for synthetic monitoring (DID, resource limits, CLI KEM load metadata — never secrets).
- Run with `RUST_LOG=info` (or `debug` during incidents) and ship logs to your aggregator.

## 4. Network edge

- Restrict **`0.0.0.0`** exposure: bind HTTP/P2P behind firewall rules; allow only bastion or mesh peers. Standalone: set **`[network].enable_http_api = false`** (or **`start --no-http`**) to skip the Warp listener entirely while keeping P2P.
- Apply **rate limiting** and **WAF** at the proxy until in-process limits are standardized.
- Rotate **bootstrap peer** lists when infra changes.

## 5. Supply chain

- Run `scripts/audit-compute-node.sh` (or `cargo audit`) before releases; triage advisories for Wasmtime, `ring`, OQS-backed crates, and JSON/network stacks.
- Pin crate versions for tagged releases via workspace `Cargo.lock`.

## 6. Load and failure drills

- **HTTP smoke**: `scripts/smoke-http.sh [host] [port]` after deploy.
- **Soak**: drive concurrent `/v1/execute` or internal task APIs from staging; watch memory and task queues.
- **Restart**: kill -TERM the process, verify recovery and peer reconnection (P2P / bootstrap).

## 7. Incidents

| Symptom | Check | Mitigation |
|--------|--------|------------|
| Node won't start | Logs at startup; `config.toml` parse errors | Fix TOML; verify key paths expand (`~/` on Unix) |
| `cli_kem_loaded: false` | Key paths; file permissions | Align paths with CLI; ensure both `.hex` files exist |
| Signature / DID mismatch | Mixed Kyber CLI vs SPHINCS signing | Align client expectations; track parity work in `documentation/VM_PARITY.md` |
| High error rate | `RUST_LOG`, proxy logs | Roll back config; isolate failing route |

## 8. Key rotation

- CLI Kyber rotation: regenerate keys with the SpaceKit CLI, update `[identity]` paths and `did` if your process changes the DID, restart the node.
- For KeyMaster-backed storage keys, follow `keymaster` API docs and storage node rotation procedures.

## 9. ASTRA test credits, balances, and transactions (SwtchVM RPC vs operator HTTP)

Operators and integrators often ask how end users get **ASTRA** (native balance units on the in-tree SwtchVM state), check balances, and submit transactions from **TypeScript** after a wallet receives funds.

### 9.1 Two different HTTP surfaces (do not confuse them)

| Surface | Typical URL | What it is |
|---------|-------------|------------|
| **Standalone compute node** (`spacekit-compute-node start`) | `http://<host>:<rpc_port>/…` when **`[network].enable_http_api`** is true (default) | Operator API + full in-process SwtchVM dev HTTP on the same port (`/health`, `/v1/*`, `/account/…`, `POST /rpc`, `/faucet`, rollup routes, etc.). Set **`enable_http_api = false`** or **`start --no-http`** to disable HTTP entirely (P2P unchanged). **Port 9000** is P2P — not HTTP. |
| **SwtchVM dev RPC** (`SwtchvmNode::start_rpc_server` only) | `http://0.0.0.0:<port>/…` when embedded in another binary | Same route set as the SwtchVM portion of standalone; use when you are **not** using the standalone binary but still want a dedicated listener. Standalone normally serves these on **`rpc_port`** with the operator API unless HTTP is disabled. |

### 9.2 How users get ASTRA (SwtchVM faucet)

When **`spacekit-compute-node start`** is running, use **`[network].rpc_port`** (e.g. **8080**) for both operator routes and the faucet:

```bash
export SWTCHVM_PORT=8080   # same as rpc_port / --port
```

**HTTP**

```bash
curl -sS -X POST "http://127.0.0.1:${SWTCHVM_PORT}/faucet" \
  -H "Content-Type: application/json" \
  -d '{"did":"did:spacekit:user:alice","address":"0x…20-byte hex…","amount":null}'
```

- **`did`** — rate-limit / cooldown key (per-DID policy in code).
- **`address`** — 20-byte account (`SwtchvmAddress` hex form; `0x` prefix per `from_hex` rules in code).
- **`amount`** — optional; omit to use the default drip.

**Policy (hardcoded today)** — default **1_000_000** units per request (comment in code: 1 ASTRA at 1e6 µASTRA), **3600 s** cooldown between requests per DID, **10** lifetime requests per DID. Response JSON: `success`, `amount`, `new_balance`, optional `error`, optional `cooldown_remaining`.

**JSON-RPC** (same node, `POST /rpc`):

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "spacekit_faucet",
  "params": ["did:spacekit:user:alice", "0xYourTwentyByteAddress", null]
}
```

Third param is optional numeric or string amount (see `parse_u128` in `swtchvm_node.rs`).

### 9.3 Checking balances (SwtchVM)

**JSON-RPC `eth_getBalance`** — params `[address, blockTag]`; implementation uses the address and returns hex balance from in-memory SwtchVM account state:

```bash
curl -sS -X POST "http://127.0.0.1:${SWTCHVM_PORT}/rpc" \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"eth_getBalance","params":["0x…","latest"]}'
```

**REST `GET /account/<40-hex-chars>`** — returns the full `SwtchvmAccount` JSON (balance, nonce, code, …) or 404 if unknown.

### 9.4 TypeScript snippets (SwtchVM RPC)

Use **`fetch`** against the host/port where **`start_rpc_server`** listens (again: **not** the default standalone `[network].rpc_port` unless you wire them).

Faucet:

```typescript
const base = "http://127.0.0.1:9545"; // example SWTCHVM_PORT
const faucet = await fetch(`${base}/faucet`, {
  method: "POST",
  headers: { "Content-Type": "application/json" },
  body: JSON.stringify({
    did: "did:spacekit:user:alice",
    address: "0x…", // 20-byte hex account
    amount: undefined, // or a string / number for custom drip
  }),
});
const body = await faucet.json(); // FaucetResponse
```

Balance (EIP-1193–style RPC):

```typescript
const rpc = await fetch(`${base}/rpc`, {
  method: "POST",
  headers: { "Content-Type": "application/json" },
  body: JSON.stringify({
    jsonrpc: "2.0",
    id: 1,
    method: "eth_getBalance",
    params: ["0x…", "latest"],
  }),
});
const { result } = await rpc.json(); // hex string, e.g. "0xf4240"
```

### 9.5 When users “submit to the blockchain” from TypeScript (wallet + flow)

**Mental model:** SwtchVM maintains **blocks, transactions, receipts, and account balances** inside `SwtchvmNode`. Faucet credits **`address`** balance. **Mining** includes pending txs into a block (see `mine_block` path and `SwtchvmNode::mine_block` logic in `swtchvm_node.rs`). A **wallet** in your dapp is whatever holds the user’s **DID + 20-byte address** (and eventually signs **`SwtchvmTransaction`**); the faucet only needs **`did` + `address`** for the drip.

**Current code caveat — `POST /transaction`:** The Warp handler for `POST /transaction` is still a **stub** (returns a fixed mock hash; it does **not** call `submit_transaction` on the node). So you **cannot** rely on that endpoint for real inclusion yet.

**Practical path for TypeScript today**

1. **Product / browser / Node VM:** Use **`spacekit-js`** — deploy contract WASM, **`submitTransaction`**, **`mineBlock`**, JSON-RPC `vm_*`, and optional EIP-1193 bridge. That is where “wallet got funds → call contract → mine” is documented and exercised (`spacekit-js/README.md`, `docs/SpaceKitJS-Technical-Whitepaper-v1.0.md`).
2. **This crate’s SwtchVM only:** Use **`SwtchvmCli`** in-process, or embed **`SwtchvmNode`** and call **`submit_transaction` / `mine_block`** from Rust until `POST /transaction` is implemented and signed tx format is documented for HTTP clients.

**CLI / other testnets:** For simulator-style faucet commands, see **`spacekit-cli`** (`simulator faucet` in that repo’s README)—different stack from the SwtchVM HTTP faucet above.

### 9.6 Operator action items

- **`GET /account/<hex>`** and full SwtchVM block/tx routes are still only on **`SwtchvmNode::start_rpc_server`** if you spawn it separately; extend standalone if you need those on **`rpc_port`** too.
- Document for users which **base URL** funds and balances use when you add reverse proxies (path prefixes, TLS).
