//! SpaceKit Payments
//!
//! Unified payment layer supporting:
//! - **x402**: HTTP-native paid APIs (USDC on Base via EIP-3009)
//! - **aUSD vault**: On-chain deposit vault → per-call signed charges
//! - **Native ASTRA**: In-VM fee/value transfers
//!
//! All payment methods resolve to a `Credit` that can be applied to VM balances.

pub mod ausd;
pub mod fee_router;
pub mod intent;
pub mod types;
pub mod x402;

#[cfg(feature = "warp-middleware")]
pub mod middleware;

pub use ausd::AusdVault;
pub use fee_router::FeeRouter;
pub use intent::{IntentAction, IntentPaymentProcessor, SignedIntent};
pub use types::*;
