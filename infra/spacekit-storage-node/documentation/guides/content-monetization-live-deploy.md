# Content monetization — live deploy checklist

Use this before `./scripts/content-monetization-soak.sh live` or paid launch.

## 1. Stack

```bash
spacekit network up
```

Confirm compute (`8545`) and storage (`3000` or your port) respond.

## 2. Deploy WASM contracts (compute)

From repo root, build stdlib contracts if needed, then deploy each:

```bash
# Entitlement ledger (listings + OP_PURCHASE + OP_VERIFY)
spacekit contract deploy \
  --wasm spacekit-standard-library/marketplace/astra-entitlement-ledger/target/wasm32-unknown-unknown/release/astra_entitlement_ledger.wasm
# note contract id → SPACEKIT_ENTITLEMENT_CONTRACT_ID

# AppLicenseNFT (per-content license tokens)
# Build first: cd spacekit-standard-library && cargo build -p spacekit-app-license-nft --release --target wasm32-unknown-unknown
spacekit contract deploy \
  --wasm spacekit-standard-library/target/wasm32-unknown-unknown/release/spacekit_app_license_nft.wasm
# → SPACEKIT_LICENSE_CONTRACT_ID

# astra-escrow (optional hold/release/refund audit trail)
spacekit contract deploy \
  --wasm spacekit-standard-library/payments/astra-escrow/target/wasm32-unknown-unknown/release/astra_escrow.wasm
# → SPACEKIT_ESCROW_CONTRACT_ID
```

Exact `contract deploy` flags follow your local `spacekit contract deploy --help` (profile, compute URL).

## 3. Environment (publisher + buyer shells)

```bash
export SPACEKIT_COMPUTE_URL=http://127.0.0.1:8545
export SPACEKIT_STORAGE_NODE_URL=http://127.0.0.1:3000

export SPACEKIT_ENTITLEMENT_CONTRACT_ID=<hex>
export SPACEKIT_LICENSE_CONTRACT_ID=<hex>          # optional
export SPACEKIT_ESCROW_CONTRACT_ID=<hex>           # optional
export SPACEKIT_ESCROW_ARBITER_DID=did:spacekit:treasury
export SPACEKIT_ESCROW_TOKEN=ASTRA

# Compute → storage settlement webhook (both nodes)
export SPACEKIT_CONTENT_SETTLEMENT_SECRET=<shared-secret>   # optional but recommended
```

On **compute**, set the same `SPACEKIT_STORAGE_NODE_URL` and `SPACEKIT_CONTENT_SETTLEMENT_SECRET` so `POST /v1/payments/verify` forwards to `POST /api/content/settlements`.

## 4. Live soak

```bash
cd spacekit-storage-node
./scripts/content-monetization-soak.sh live
```

Or manual H2:

```bash
spacekit content publish --channel did:spacekit:channel:live:pub \
  --file ./fixture.txt --title "Live PPV" --pricing pay_per_view --price 10

spacekit content pay --content-id <CONTENT_ID>
# complete SpaceKit Pay; verify hits compute → storage inbox

spacekit content pay --content-id <CONTENT_ID> --await-settlement
# or: spacekit content listen-settlements --once

spacekit content view --content-id <CONTENT_ID> --output /tmp/live-out.txt
```

**Pass:** view succeeds; settle logs entitlement hex; with license contract, grant includes `license_token_id`; with escrow contract, escrow moves open → released (or refunded on forced failure).

## 5. Failure / refund drill

Simulate grant failure (e.g. stop compute before second `pay --await-settlement` after inbox entry, or use wrong entitlement contract). Expect:

- Local `refund_log.json` entry via `refund_on_grant_failure`
- Payment reference unmarked consumed (buyer can retry)
- If `SPACEKIT_ESCROW_CONTRACT_ID` set: `OP_REFUND` on `content-pending:<pending_id>`

## Related

- [content-monetization-soak.md](./content-monetization-soak.md)
- [CONTENT-SYSTEM-SPEC.md](../../CONTENT-SYSTEM-SPEC.md)
- [COMMANDS.md](../../../spacekit-cli/COMMANDS.md)
