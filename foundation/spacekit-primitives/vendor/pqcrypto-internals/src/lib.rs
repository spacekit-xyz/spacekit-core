#![no_std]

use core::slice;

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
type LibcSizeT = usize;
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
type LibcCInt = i32;
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
use libc::{c_int as LibcCInt, size_t as LibcSizeT};

/// Get random bytes; exposed for PQClean implementations.
///
/// # Safety
/// Assumes inputs are valid and may panic over FFI boundary if rng failed.
///
/// # Example
/// ```rust
/// use pqcrypto_internals::*;
/// let mut buf = [0u8;10];
/// unsafe {
///   PQCRYPTO_RUST_randombytes(buf.as_mut_ptr(), buf.len());
/// }
/// ```
#[no_mangle]
pub unsafe extern "C" fn PQCRYPTO_RUST_randombytes(
    buf: *mut u8,
    len: LibcSizeT,
) -> LibcCInt {
    let buf = slice::from_raw_parts_mut(buf, len);
    getrandom::getrandom(buf).expect("RNG Failed");
    0
}
