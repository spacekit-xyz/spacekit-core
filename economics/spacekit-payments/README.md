# spacekit-payments

Unified payment layer for the SpaceKit platform. Normalizes three distinct payment rails — **x402 (USDC on Base)**, **aUSD vault credits**, and **native ASTRA** — into a single `Credit` type that can be applied to SpacekitVM balances.

TODO: Remove the Ausd & Ausd Vault modules and the associated code. We are atomic and should not need to track balances.

## Architecture

```
┌──────────────┐  ┌───────────────┐  ┌──────────────────┐
│  x402 (USDC) │  │  aUSD Vault   │  │  Native ASTRA    │
│  EIP-3009    │  │  EIP-191 sig  │  │  In-VM transfer  │
└──────┬───────┘  └───────┬───────┘  └────────┬─────────┘
       │                  │                   │
       ▼                  ▼                   ▼
  ┌────────────────────────────────────────────────┐
  │              PaymentReceipt                    │
  │   (tx_hash, amount, asset, network, timestamp) │
  └──────────────────────┬─────────────────────────┘
                         │
                         ▼
  ┌────────────────────────────────────────────────┐
  │              FeeRouter                         │
  │   verify → deduct network fee → convert to     │
  │   ASTRA → apply credit via CreditApplier       │
  └──────────────────────┬─────────────────────────┘
                         │
              ┌──────────┴──────────┐
              ▼                     ▼
     ┌──────────────┐     ┌──────────────┐
     │  Beneficiary │     │   Treasury   │
     │  VM Balance  │     │  Fee Collect │
     └──────────────┘     └──────────────┘
```

## Modules

| Module | Description |
|--------|-------------|
| `types` | Core types: `PaymentNetwork`, `PaymentAsset`, `PaymentRequirement`, `PaymentReceipt`, `Credit`, `PaymentConfig` |
| `fee_router` | `FeeRouter` — converts verified receipts into ASTRA VM credits with configurable network fee (basis points) and treasury collection |
| `x402` | HTTP 402 Payment Required protocol: `X402Response` builder, `X402PaymentProof` parsing, facilitator relay verification |
| `ausd` | `AusdVault` — in-memory aUSD balance tracking with nonce-based replay protection and charge-to-receipt conversion |
| `middleware` | Warp filter for x402 payment gating on HTTP routes (feature: `warp-middleware`) |

## Features

| Feature | Default | Description |
|---------|---------|-------------|
| `x402` | yes | Enables `reqwest`-based facilitator relay for x402 payment verification |
| `warp-middleware` | yes | Enables the `middleware` module with warp filters for payment-gated routes |

## Usage

Add to `Cargo.toml`:

```toml
[dependencies]
spacekit-payments = { path = "../spacekit-payments" }
```

### FeeRouter

```rust
use spacekit_payments::{FeeRouter, PaymentConfig, PaymentReceipt, PaymentAsset, PaymentNetwork};
use spacekit_payments::fee_router::CreditApplier;

struct MyApplier;
impl CreditApplier for MyApplier {
    fn apply_credit(&self, credit: &spacekit_payments::Credit) -> anyhow::Result<()> {
        println!("Credit {} ASTRA to {}", credit.amount_astra, credit.beneficiary_did);
        Ok(())
    }
}

let config = PaymentConfig {
    pay_to_address: "0x...".to_string(),
    testnet: true,
    network_fee_bps: 25, // 0.25%
    usdc_to_astra_rate: 1_000_000.0,
    ..Default::default()
};

let router = FeeRouter::new(config, Arc::new(MyApplier));
```

### aUSD Vault

```rust
use spacekit_payments::AusdVault;
use spacekit_payments::ausd::VaultChargeRequest;

let vault = AusdVault::new();
vault.credit("did:spacekit:alice", 10.0).await;

let req = VaultChargeRequest {
    user_did: "did:spacekit:alice".to_string(),
    amount_ausd: "3.50".to_string(),
    nonce: 1,
    signature: "0x...".to_string(),
    description: Some("Contract execution".to_string()),
};
let receipt = vault.process_charge(&req).await?;
```

### x402 Payment Gate (Warp)

```rust
use spacekit_payments::middleware::{require_payment, PaymentGate, handle_payment_rejection};

let gate = PaymentGate {
    price_usdc: "0.01".to_string(),
    description: "Contract execution fee".to_string(),
    beneficiary_did: "did:spacekit:contract:xyz".to_string(),
};

let route = warp::path("execute")
    .and(require_payment(gate, config, fee_router))
    .map(|credit| { /* credit verified, proceed */ });
```

## Compute Node Integration

The compute node exposes these payment endpoints:

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/v1/payments/config` | GET | Returns accepted payment methods and network configuration |
| `/v1/payments/verify` | POST | Verify an x402 receipt and record the credit |
| `/v1/payments/charge-ausd` | POST | Deduct aUSD from a user's vault balance (nonce-protected) |
| `/v1/payments/credit-ausd` | POST | Credit a user's aUSD balance (from website deposit bridge) |
| `/v1/payments/balance-ausd` | GET | Query a user's aUSD vault balance |

## Tests

```bash
cargo test
```

7 tests covering fee routing (USDC + ASTRA), aUSD vault charges, nonce replay rejection, insufficient balance, and x402 response serialization.

## License

Apache-2.0
