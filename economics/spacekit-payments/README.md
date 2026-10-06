# spacekit-payments

Payments for the SpaceKit platform, in **ASTRA only**.

ASTRA is SpaceKit's one currency. It exists only as native balances on the SpaceKit chain, in wei (18 decimals). This crate prices, verifies and routes payments in ASTRA. There are no USD-denominated balances, no stablecoin rails (x402/USDC, aUSD) and no exchange rates.

## How a payment works

```
payer ──ASTRA transfer (chain tx)──▶ payee address
                 │
                 ▼  tx hash
        PaymentVerifier ── looks the tx up on the chain (ChainLookup):
                           success? right payee? enough value? not used before?
                 │
                 ▼
          PaymentReceipt { tx_hash, from, to, amount_wei, block_number, settled_at }
```

A payment is proven by its transaction hash. Nothing is taken on the payer's word.

## Modules

| Module | Description |
|--------|-------------|
| `types` | `PaymentAsset` (ASTRA), `PaymentRequirement`, `PaymentReceipt`, `Credit`, `PaymentConfig`; exact decimal helpers `parse_astra`, `format_astra`, `parse_wei` |
| `verify` | `PaymentVerifier` checks a payment against the chain through the `ChainLookup` trait (the compute node implements it for its own chain) |
| `fee_router` | `FeeRouter` splits an ASTRA payment into the payee's share and the network fee (basis points) and hands both to a `CreditApplier`, which must move them as chain transfers (never mint) |
| `intent` | ASTRA actions in signed intents: contract value (`value_astra`), transfers, and the `max_value_wei` cap; other assets are refused |
| `middleware` | Warp filter for pay-per-request routes: `402` with an ASTRA price, then `X-PAYMENT: <tx hash>` (feature `warp-middleware`) |

## Features

| Feature | Default | Description |
|---------|---------|-------------|
| `warp-middleware` | yes | Enables the `middleware` module |

## Usage

```toml
[dependencies]
spacekit-payments = { path = "../spacekit-payments" }
```

### Verify a payment

```rust
use spacekit_payments::{parse_astra, PaymentAsset, PaymentRequirement, PaymentVerifier};

// `chain` implements `ChainLookup` (e.g. the compute node's `SwtchvmNode`).
let verifier = PaymentVerifier::new(chain).with_min_confirmations(2);
let requirement = PaymentRequirement {
    amount_wei: parse_astra("5")?.to_string(),
    asset: PaymentAsset::ASTRA,
    pay_to: "0x…payee".to_string(),
    chain_id: None,
    description: Some("Studio notes, 30 days".to_string()),
};
let receipt = verifier.verify(&tx_hash, &requirement)?; // fails if reused
```

The set of used transactions is in memory. A service that grants something lasting for a payment must also store the transaction hash with the grant, so a restart cannot accept the same payment twice.

### Pay-per-request route (Warp)

```rust
use spacekit_payments::middleware::{handle_payment_rejection, require_payment, PaymentGate};

let gate = PaymentGate {
    price_wei: parse_astra("0.01")?,
    pay_to: "0x…payee".to_string(),
    chain_id: Some("spacekit-mainnet".to_string()),
    description: "Contract execution fee".to_string(),
};

let route = warp::path("execute")
    .and(require_payment(gate, verifier.clone()))
    .map(|receipt| { /* paid by receipt.tx_hash */ })
    .recover(handle_payment_rejection);
```

## Compute node endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/v1/payments/config` | GET | Accepted asset (ASTRA, 18 decimals), chain id, network fee |
| `/v1/payments/verify` | POST | `{ tx_hash, pay_to, amount_wei, scope? }`: verify an ASTRA payment once; content and channel scopes are forwarded to the storage node's settlement inbox |
| `/v1/tx/{hash}` | GET | The transaction: `from`, `to`, `value_wei`, `success`, `confirmations` |
| `/v1/execute` | POST | Validate a signed intent (ASTRA only); lists the chain transactions it needs |

## Tests

```bash
cargo test -p spacekit-payments
```

Covers exact ASTRA decimal parsing, payment verification (payee, amount, failed transactions, confirmations, replay), fee routing and intent limits. The compute node's tests check verification against real mined blocks.

## License

Apache-2.0
