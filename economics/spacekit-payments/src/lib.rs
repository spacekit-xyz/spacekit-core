//! SpaceKit Payments
//!
//! SpaceKit has one currency, ASTRA, held as native balances on the SpaceKit
//! chain. This crate prices, verifies and routes payments in ASTRA only:
//!
//! - **Verification** (`verify`): a payment is a successful ASTRA transfer on
//!   the chain, checked by transaction hash.
//! - **Pay-per-request** (`middleware`, feature `warp-middleware`): `402`
//!   with an ASTRA price, then `X-PAYMENT: <tx hash>`.
//! - **Fee routing** (`fee_router`): payee share plus network fee, applied as
//!   chain transfers.
//! - **Intents** (`intent`): ASTRA transfers and contract value in intents.
//!
//! There are no USD-denominated balances, stablecoin rails or exchange rates.

pub mod fee_router;
pub mod intent;
pub mod types;
pub mod verify;

#[cfg(feature = "warp-middleware")]
pub mod middleware;

pub use fee_router::FeeRouter;
pub use intent::{IntentAction, IntentPaymentProcessor, SignedIntent};
pub use types::*;
pub use verify::{ChainLookup, ChainTransfer, PaymentVerifier};
