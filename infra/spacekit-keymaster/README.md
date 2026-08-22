# spacekit-keymaster

SKKM-1 production services: guardian decrypt oracle, coordinator, and registry proxy.

## Binaries

| Binary | Default port | Role |
|--------|--------------|------|
| `spacekit-keymaster-coordinator` | 8780 | Manifest, placements, recovery tickets, storage gateway |
| `spacekit-keymaster-guardian` | 8781–8785 | Shard decrypt oracle (one per operator) |
| `spacekit-keymaster-registry` | 8770 | Guardian discovery (proxies coordinator) |
| `spacekit-keymaster-signer` | — | ML-DSA sign helper for Node CLI (`stdin` → sig bytes) |

## Build

```bash
cd spacekit-keymaster
cargo build --release
```

Binaries land in `target/release/`.

## Local network

With `spacekit network up --full`, the supervisor starts coordinator + five guardians + registry when `[services] keymaster = true`.

**Dev stack (non-blocking):**

```bash
./scripts/dev-stack.sh start   # KEYMASTER_DEV=1, relaxed rate limits
./scripts/dev-stack.sh status
./scripts/dev-stack.sh stop
```

Environment:

- `KEYMASTER_COORDINATOR_URL` — default `http://127.0.0.1:8780`
- `KEYMASTER_REGISTRY_URL` — default `http://127.0.0.1:8770`
- `KEYMASTER_STORAGE_URL` — storage-node HTTP base (default `http://127.0.0.1:3030`)
- `KEYMASTER_DEV=1` — guardian rate limit 50/min (for roundtrip tests); production omit this
- `KEYMASTER_RATE_LIMIT_MAX` / `KEYMASTER_RATE_LIMIT_WINDOW_S` — override guardian decrypt caps

## CLI ceremonies

```bash
# Mock (in-memory demo KEM)
spacekit keymaster roundtrip-test

# Production stack (coordinator + guardians must be running)
spacekit keymaster roundtrip-test --network prod
```

See `spacekit-projects/apps/keymaster/keymaster-ui/SKKM_SHARD_CUSTODY_SPEC.md` for the protocol.

## Contracts

`contracts/KeymasterPayments.sol` — minimal Shield subscription ledger (USDC).
