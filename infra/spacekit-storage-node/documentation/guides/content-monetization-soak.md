# Content monetization E2E soak

Gate before paid content goes live. Validates Sprint 2 + Sprint 3 wiring end-to-end.

## Layers

| Layer | Command | When |
|-------|---------|------|
| **Automated (CI)** | `cargo test --test content_e2e_soak -p spacekit-storage-node` | Every PR; no live nodes |
| **Automated (unit)** | `cargo test --test content_sprint2 -p spacekit-storage-node` | Payment/settlement primitives |
| **CLI dev soak** | `./scripts/content-monetization-soak.sh dev` | Local stack; receipt + inbox path |
| **CLI live soak** | `./scripts/content-monetization-soak.sh live` | Compute + entitlement contract deployed |

## Prerequisites

```bash
# Terminal 1
spacekit network up

# Terminal 2 — dev soak (no on-chain OP_PURCHASE required)
export SPACEKIT_COMPUTE_URL=http://127.0.0.1:8545   # optional for verify HTTP

# Live soak: full contract deploy + env — see content-monetization-live-deploy.md
export SPACEKIT_ENTITLEMENT_CONTRACT_ID=<hex-contract-id>
export SPACEKIT_LICENSE_CONTRACT_ID=<hex>          # optional
export SPACEKIT_ESCROW_CONTRACT_ID=<hex>           # optional
```

Publisher and buyer need distinct DIDs (`spacekit init` in separate data dirs or profiles).

## Scenario matrix

### Minimum (v1 soak script + automated tests)

| # | Scenario | Automated test | CLI dev | CLI live |
|---|----------|----------------|---------|----------|
| H1 | Free content view | `soak_free_content_view_allowed` | ✓ | ✓ |
| H2 | PPV publish → pay → access → view | `soak_ppv_pay_settle_view_chain` | ✓ | ✓ (settle + OP_PURCHASE) |
| H3 | Publisher views own paid content | `soak_publisher_views_own_paid_content_without_grant` | ✓ | ✓ |
| H4 | Channel subscribe → view channel content | `soak_channel_subscribe_then_view` | ✓ | partial |
| H5 | Renew before expiration | `soak_renew_before_and_after_expiration` | manual | manual |
| H6 | Renew after expiration | same | manual | manual |
| H7 | Tier change on renewal | `soak_tier_change_on_renewal` | manual | manual |
| E1 | Payment not found | `soak_payment_not_found_and_wrong_recipient` | ✓ | ✓ |
| E2 | Wrong amount / recipient at settle | `soak_settlement_wrong_amount_and_recipient` | ✓ | ✓ |
| E3 | Duplicate payment reference | `soak_duplicate_payment_reference_rejected` | ✓ | ✓ |
| E4 | Idempotent second complete | `soak_idempotent_complete_pending` | ✓ | ✓ |
| E5 | Double pay (different tx) — one grant | `soak_double_pay_different_refs_single_grant_path` | manual | manual |

### Incremental (not in v1 script)

- Payment fails (insufficient funds / wrong network) — SpaceKit Pay router
- Settlement timeout + retry — pending remains `awaiting_payment`
- Publisher access without pay — automated H3
- Concurrent purchases — load test
- Storage restart mid-purchase — pending JSON recoverable under `content_payments/pending_purchases.json`
- Compute down during settle — CLI should error clearly (live soak)
- Content updated after purchase — policy TBD (grant covers fact_id, not mutable revision)

## Dev soak flow (H2)

```bash
# Publisher
spacekit content publish --channel did:spacekit:channel:soak:pub \
  --file ./fixture.txt --title "Soak PPV" --pricing pay_per_view --price 10
# note CONTENT_ID from output

# Buyer
spacekit content view --content-id <CONTENT_ID>
spacekit content pay --content-id <CONTENT_ID>
# record-payment (simulates SpaceKit Pay → inbox)
spacekit content record-payment \
  --reference tx-soak-1 --recipient <PUBLISHER_DID> \
  --scope content:<CONTENT_ID> --amount 10
spacekit content pay --content-id <CONTENT_ID> --await-settlement
spacekit content view --content-id <CONTENT_ID> --output /tmp/soak-out.txt
```

**Pass:** view returns bytes; `list-access` shows grant; no duplicate grant on second `--await-settlement`.

**Listener (automated settle):** run in a second terminal while testing:

```bash
spacekit content listen-settlements          # poll every 5s
# or one shot after record-payment:
spacekit content listen-settlements --once
```

`content pay --await-settlement` polls the same inbox (500ms default, 120s timeout).

**Production path:** compute `POST /v1/payments/verify` with `scope` (`content:{hex}` or `channel:{did}`)
forwards to storage `POST /api/content/settlements` when `SPACEKIT_STORAGE_NODE_URL` is set on compute.
Run `content listen-settlements` (or `--await-settlement`) to complete pending purchases.

## Live soak flow (H2)

```bash
spacekit content pay --content-id <CONTENT_ID> --tx-hash <real-tx> --amount 10
# or: pay → pay externally → settle --pending-id ... --tx-hash ... --amount 10
spacekit content view --content-id <CONTENT_ID> --output /tmp/soak-out.txt
```

**Pass:** settle prints entitlement hex; view succeeds; optional OP_VERIFY with entitlement id.

## Error checks (manual)

```bash
# Wrong amount (expect settle/complete to fail)
spacekit content settle --pending-id <id> --tx-hash tx-bad --amount 1

# Wrong recipient on record-payment (expect access --payment-ref to fail)
spacekit content record-payment --reference tx-w \
  --recipient did:spacekit:wrong --scope content:<id> --amount 10
spacekit content access --content-id <id> --payment-ref tx-w

# Duplicate reference
spacekit content access --content-id <id> --payment-ref tx-soak-1  # second time → error
```

## Pass criteria (soak sign-off)

- All `content_e2e_soak` and `content_sprint2` tests green in CI
- Dev script exits 0 on `dev` mode against `network up`
- Live script exits 0 on `live` when `SPACEKIT_ENTITLEMENT_CONTRACT_ID` set and contract responds
- No manual `record-payment` required for production path after settlement listener ships

## Related

- [scripts/README.md](../../scripts/README.md) — soak script reference (setup, interpretation, CI, roadmap)
- [content-monetization-live-deploy.md](./content-monetization-live-deploy.md) — live contract deploy checklist
- [CONTENT-SYSTEM-SPEC.md](../../CONTENT-SYSTEM-SPEC.md)
- [COMMANDS.md](../../../spacekit-cli/COMMANDS.md) — `content pay/settle/purchase`
- [federation-testing.md](./federation-testing.md) — operator / migration soaks
