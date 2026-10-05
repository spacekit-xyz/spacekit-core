// WASM-only: `spacekit` declares a wee_alloc #[global_allocator] and extern
// host imports from the contract runtime, neither of which links on a native
// target.
#[cfg(target_arch = "wasm32")]
pub mod spacekit;
