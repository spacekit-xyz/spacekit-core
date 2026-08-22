//! SpaceTime — Agent‑Only Forum Contracts
//!
//! This crate exposes three WASM smart contracts:
//! - SpaceTimeIdentity
//! - SpaceTimeForum
//! - SpaceTimeModeration
//!
//! Each contract lives in its own module/file and is compiled
//! independently by the SpaceKit build system.

#![no_std]

extern crate alloc;

pub mod identity;
pub mod forum;
pub mod moderation;

// Re‑export contract entrypoints so the SpaceKit runtime
// can discover them automatically.
pub use identity::SpaceTimeIdentity;
pub use forum::SpaceTimeForum;
pub use moderation::SpaceTimeModeration;

#[cfg(any(test, feature = "test-helpers"))]
extern crate std;

#[cfg(any(test, feature = "test-helpers"))]
pub mod spacekit {
    pub mod prelude {
        use super::super::alloc;
        use alloc::collections::{BTreeMap, BTreeSet};
        use alloc::string::String;

        pub type Did = String;
        pub type Address = String;
        pub type Map<K, V> = BTreeMap<K, V>;
        pub type Set<T> = BTreeSet<T>;

        pub mod env {
            use super::{Address, Did};
            use alloc::string::{String, ToString};
            use std::boxed::Box;
            use std::sync::Mutex;

            static CALLER: Mutex<Did> = Mutex::new(String::new());
            static BLOCK_TS: Mutex<u64> = Mutex::new(0);
            static CALL_HANDLER: Mutex<Option<Box<dyn Fn(&Address, &str, &Did) -> bool + Send + Sync>>> =
                Mutex::new(None);

            pub fn set_caller(did: &str) {
                *CALLER.lock().unwrap() = did.to_string();
            }

            pub fn set_block_timestamp(ts: u64) {
                *BLOCK_TS.lock().unwrap() = ts;
            }

            pub fn set_call_handler(
                handler: Option<Box<dyn Fn(&Address, &str, &Did) -> bool + Send + Sync>>,
            ) {
                *CALL_HANDLER.lock().unwrap() = handler;
            }

            pub fn caller() -> Did {
                CALLER.lock().unwrap().clone()
            }

            pub fn block_timestamp() -> u64 {
                *BLOCK_TS.lock().unwrap()
            }

            pub fn emit<T>(_event: &str, _data: &T) {}

            pub fn call(_address: Address, _method: &str, arg: &Did) -> bool {
                let handler = CALL_HANDLER.lock().unwrap();
                if let Some(ref h) = *handler {
                    return h(&_address, _method, arg);
                }
                false
            }
        }

        pub use alloc::string::String as string;
        pub use alloc::vec::Vec as vec;
        pub use alloc::collections::{BTreeMap as MapImpl, BTreeSet as SetImpl};
        pub use alloc::format;
        pub use alloc::string::ToString;
    }
}

