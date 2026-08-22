#![cfg_attr(feature = "no_std", no_std)]

#[cfg(feature = "no_std")]
extern crate alloc;

pub mod v1;

#[cfg(feature = "secrets-core")]
pub use v1::secrets_core;
