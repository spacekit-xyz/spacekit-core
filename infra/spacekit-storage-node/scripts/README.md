# SpaceKit storage-node scripts

Operational scripts for local validation, CI, and technical due diligence.

## Content monetization soak

**Script:** [`content-monetization-soak.sh`](content-monetization-soak.sh)

**Purpose:** End-to-end proof that paid content publishing and consumption works through the real CLI against a running local stack—not only in-process unit tests.

A passing **dev** soak (5 checks, 0 failures) means:

- Storage is reachable and healthy.
- Free content can be published and viewed without payment.
- Paid PPV content can be published, quoted, settled via the dev payment path, auto-completed from the settlement inbox, and viewed after grant.

This is the reference artifact for onboarding engineers, regression gates, and partner technical reviews.

---

## Prerequisites

| Requirement | Notes |
|-------------|--------|
| `spacekit` on `PATH` | Build from repo: `cargo build -p spacekit --release` |
| `spacekit init` | Once per machine: `spacekit init` (creates `~/.spacekit/`) |
| Local network | **Terminal 1:** `spacekit network up` (storage ~3030, compute ~8545) |
| `curl` | Used for storage health check |

Optional env:

| Variable | Default | Purpose |
|----------|---------|---------|
| `SPACEKIT` | `spacekit` | CLI binary name/path |
| `SPACEKIT_STORAGE_URL` | `http://127.0.0.1:3030` | Storage health endpoint |
| `SPACEKIT_COMPUTE_URL` | `http://127.0.0.1:8545` | Live mode only |
| `SOAK_DIR` | `/tmp/spacekit-content-soak-$$` | Temp fixture directory |

---

## Running

```bash
# Terminal 1
spacekit network up

# Terminal 2 (preferred for CLI devs)
spacekit content soak dev

# Or from repo root:
./spacekit-cli/scripts/content-monetization-soak.sh dev
./spacekit-storage-node/scripts/content-monetization-soak.sh dev
```

**Live mode** (real `OP_PURCHASE`, entitlement contract required):

```bash
# See documentation/guides/content-monetization-live-deploy.md
export SPACEKIT_ENTITLEMENT_CONTRACT_ID=<hex>
./spacekit-storage-node/scripts/content-monetization-soak.sh live
```

---

## What each check validates

### Dev mode summary line

`Soak summary: 5 passed, 0 failed`

| Check | What it proves |
|-------|----------------|
| **storage health OK** | `GET /api/agentic/health` on the storage node |
| **free view** (H1) | `content publish --pricing free` → `content view` returns payload |
| **published content_id=…** (H2) | PPV fact package stored; Content ID parseable |
| **inbox auto-complete** (H2) | Settlement inbox + listener + `pay --await-settlement` completes grant |
| **view after grant** (H2) | Buyer can retrieve bytes after settlement |

### H2 flow (paid PPV dev chain)

```text
publish (pay_per_view, 10 ASTRA)
  → content pay                    # creates pending purchase + quote
  → content record-payment         # simulates SpaceKit Pay → settlements_inbox.jsonl
  → content listen-settlements --once
  → content pay --pending-id <id> --await-settlement
  → content view --output <file>
```

**Important:** The second `content pay` must reuse the same pending id (`--pending-id`). Without it, a new pending is created and the inbox receipt no longer matches.

Dev settlement does **not** require on-chain `OP_PURCHASE` when `SPACEKIT_ENTITLEMENT_CONTRACT_ID` is unset; entitlement id is derived from the test tx hash.

State lives under `~/.spacekit/storage/content_payments/`:

- `pending_purchases.json`
- `settlements_inbox.jsonl`
- `processed_inbox_tx.json`
- `verified.json` (payment receipts)

---

## Interpreting results

### Success

```text
settlement listener: completed pending-… → entitlement <hex>
PASS: inbox auto-complete
Soak summary: 5 passed, 0 failed
```

Exit code `0`. Safe to treat as **dev monetization path green**.

### Common failures

| Symptom | Likely cause | Fix |
|---------|----------------|-----|
| `storage not reachable` | `network up` not running or wrong port | Start network; set `SPACEKIT_STORAGE_URL` |
| `content is free` on pay | Access policy lost on fact retrieve (old binary) | Rebuild `spacekit`; republish |
| `Timed out waiting for settlement` | New pending on second `pay` without `--pending-id` | Rebuild CLI; use soak script from repo |
| `listen-settlements` prints nothing | Inbox/recipient/scope mismatch | Check `record-payment --recipient` matches publisher DID from pay quote |
| `publish` / store errors | Encryption or key parse issues | Rebuild storage-node + CLI; see `fact_storage` / access policy docs |

### View passes without grant

If you use the **same DID** for publish and buy, the publisher can view own paid content without a buyer grant. For a strict buyer-only test, use two profiles or distinct DIDs.

---

## CI and automated tests

Two layers—run both before release:

### Layer 1 — In-process (every PR, no network)

```bash
cargo test --test content_sprint2 -p spacekit-storage-node
cargo test --test content_e2e_soak -p spacekit-storage-node
```

Covers payment verify, settlement matching, grants, error paths (wrong amount, duplicate ref, idempotent complete), and more. See [`documentation/guides/content-monetization-soak.md`](../documentation/guides/content-monetization-soak.md).

### Layer 2 — CLI dev soak (GitHub Actions)

Workflow: [`.github/workflows/content-monetization.yml`](../../.github/workflows/content-monetization.yml)

- **PR / push:** Layer 1 tests only (fast).
- **Nightly + manual:** Layer 1 + CLI `dev` soak with `spacekit network up`.

Live soak remains **manual** or **nightly against a dedicated env** with deployed WASM contracts.

---

## What to add next (soak maturity)

Add scenarios incrementally; each should pass in `content_e2e_soak` first, then in the shell script where practical.

| ID | Scenario | Priority |
|----|----------|----------|
| E6 | Wrong `record-payment` amount → settle rejected | High |
| E7 | Wrong recipient on `record-payment` | High |
| E8 | Duplicate payment reference | Medium (in e2e; extend script) |
| E9 | Settlement timeout (no inbox entry) | Medium |
| E10 | Second buyer, distinct DID (publisher ≠ buyer) | High for prod realism |
| E11 | Storage restart with pending JSON recovery | Medium |
| E12 | Channel subscribe + paid channel view (H4 live) | Medium |
| E13 | Refund path after forced grant failure | Medium (escrow env) |

---

## Related docs

- [content-monetization-soak.md](../documentation/guides/content-monetization-soak.md) — full scenario matrix
- [content-monetization-live-deploy.md](../documentation/guides/content-monetization-live-deploy.md) — WASM deploy + live soak
- [CONTENT_PUBLISHING.md](../CONTENT_PUBLISHING.md) — general content publish guide (channels, PPV, view)
- [GROWFORMER_SPEC.md](../GROWFORMER_SPEC.md) — growformer library-in-CLI (not binary publish)
- [scripts/growformer-access-soak.sh](growformer-access-soak.sh) — growformer entitlement E2E soak
- [CONTENT-SYSTEM-SPEC.md](../CONTENT-SYSTEM-SPEC.md) — system completion spec
- [COMMANDS.md](../../spacekit-cli/COMMANDS.md) — `content pay`, `listen-settlements`, `record-payment`

---

## Due diligence one-liner

> “Run `cargo test --test content_e2e_soak -p spacekit-storage-node`, then `spacekit network up` and `./spacekit-storage-node/scripts/content-monetization-soak.sh dev`. Five passes, zero failures, demonstrates publish → pay → settle → view for paid content.”
