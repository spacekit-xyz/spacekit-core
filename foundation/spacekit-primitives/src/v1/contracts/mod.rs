// WASM-only: `spacekit` declares `#[link(wasm_import_module = ...)]` host
// imports from the contract runtime. They have no native definition, and
// MSVC rejects the undefined externs, so this cannot compile for a native
// target.
#[cfg(target_arch = "wasm32")]
pub mod spacekit;
