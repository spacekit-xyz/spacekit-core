//! Cursor-based wire encoding and decoding for contract `handle` payloads.
//!
//! Conventions used across SpaceKit example contracts:
//! - Little-endian fixed-width integers (`u8` / `u16` / `u32` / `u64`).
//! - Length-prefixed UTF-8 strings: `u16` LE length, then bytes ([`read_string`],
//!   [`read_string_max`], [`write_string`]).
//! - Length-prefixed opaque bytes: `u16` or `u32` length ([`read_bytes_u16`],
//!   [`read_bytes_u32`] and the `write_bytes_*` pair).
//!
//! On failure, readers and fallible writers return [`crate::ContractError::InvalidInput`].

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::ContractError;

// ─── decode ─────────────────────────────────────────────────────────────────

/// Read one byte from `input` at `cursor`, then advance `cursor` by 1.
pub fn read_u8(input: &[u8], cursor: &mut usize) -> Result<u8, ContractError> {
    if *cursor >= input.len() {
        return Err(ContractError::InvalidInput);
    }
    let value = input[*cursor];
    *cursor += 1;
    Ok(value)
}

/// Read a `u16` in little-endian order, then advance `cursor` by 2.
pub fn read_u16(input: &[u8], cursor: &mut usize) -> Result<u16, ContractError> {
    if *cursor + 2 > input.len() {
        return Err(ContractError::InvalidInput);
    }
    let bytes = [input[*cursor], input[*cursor + 1]];
    *cursor += 2;
    Ok(u16::from_le_bytes(bytes))
}

/// Read a `u32` in little-endian order, then advance `cursor` by 4.
pub fn read_u32(input: &[u8], cursor: &mut usize) -> Result<u32, ContractError> {
    if *cursor + 4 > input.len() {
        return Err(ContractError::InvalidInput);
    }
    let bytes = [
        input[*cursor],
        input[*cursor + 1],
        input[*cursor + 2],
        input[*cursor + 3],
    ];
    *cursor += 4;
    Ok(u32::from_le_bytes(bytes))
}

/// Read a `u64` in little-endian order, then advance `cursor` by 8.
pub fn read_u64(input: &[u8], cursor: &mut usize) -> Result<u64, ContractError> {
    if *cursor + 8 > input.len() {
        return Err(ContractError::InvalidInput);
    }
    let bytes = [
        input[*cursor],
        input[*cursor + 1],
        input[*cursor + 2],
        input[*cursor + 3],
        input[*cursor + 4],
        input[*cursor + 5],
        input[*cursor + 6],
        input[*cursor + 7],
    ];
    *cursor += 8;
    Ok(u64::from_le_bytes(bytes))
}

/// Read exactly `N` bytes into a fixed array (no length prefix).
pub fn read_fixed<const N: usize>(input: &[u8], cursor: &mut usize) -> Result<[u8; N], ContractError> {
    if *cursor + N > input.len() {
        return Err(ContractError::InvalidInput);
    }
    let mut out = [0u8; N];
    out.copy_from_slice(&input[*cursor..*cursor + N]);
    *cursor += N;
    Ok(out)
}

/// Read a 32-byte blob (e.g. hash or id); equivalent to [`read_fixed::<32>`].
#[inline]
pub fn read_bytes32(input: &[u8], cursor: &mut usize) -> Result<[u8; 32], ContractError> {
    read_fixed::<32>(input, cursor)
}

/// `u16` length prefix, then that many bytes (owned copy).
pub fn read_bytes_u16(input: &[u8], cursor: &mut usize) -> Result<Vec<u8>, ContractError> {
    let len = read_u16(input, cursor)? as usize;
    if *cursor + len > input.len() {
        return Err(ContractError::InvalidInput);
    }
    let slice = &input[*cursor..*cursor + len];
    *cursor += len;
    Ok(slice.to_vec())
}

/// `u32` length prefix, then that many bytes (owned copy).
pub fn read_bytes_u32(input: &[u8], cursor: &mut usize) -> Result<Vec<u8>, ContractError> {
    let len = read_u32(input, cursor)? as usize;
    if *cursor + len > input.len() {
        return Err(ContractError::InvalidInput);
    }
    let slice = &input[*cursor..*cursor + len];
    *cursor += len;
    Ok(slice.to_vec())
}

/// Read a `u16` LE length followed by that many UTF-8 bytes as an owned string.
#[inline]
pub fn read_string(input: &[u8], cursor: &mut usize) -> Result<String, ContractError> {
    read_string_max(input, cursor, usize::MAX)
}

/// Like [`read_string`], but rejects payloads whose UTF-8 byte length exceeds `max_utf8_bytes`.
pub fn read_string_max(
    input: &[u8],
    cursor: &mut usize,
    max_utf8_bytes: usize,
) -> Result<String, ContractError> {
    let len = read_u16(input, cursor)? as usize;
    if len > max_utf8_bytes {
        return Err(ContractError::InvalidInput);
    }
    if *cursor + len > input.len() {
        return Err(ContractError::InvalidInput);
    }
    let slice = &input[*cursor..*cursor + len];
    *cursor += len;
    core::str::from_utf8(slice)
        .map(|s| s.to_string())
        .map_err(|_| ContractError::InvalidInput)
}

/// Ensures the entire `input` was consumed after parsing (`cursor == input.len()`).
#[inline]
pub fn expect_consumed(input: &[u8], cursor: usize) -> Result<(), ContractError> {
    if cursor != input.len() {
        return Err(ContractError::InvalidInput);
    }
    Ok(())
}

// ─── encode ─────────────────────────────────────────────────────────────────

/// Append one byte.
#[inline]
pub fn write_u8(out: &mut Vec<u8>, value: u8) {
    out.push(value);
}

/// Append `value` as `u16` little-endian.
#[inline]
pub fn write_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// Append `value` as `u32` little-endian.
#[inline]
pub fn write_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// Append `value` as `u64` little-endian.
#[inline]
pub fn write_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// Append `u16` LE length then UTF-8 bytes. Errors if `s` does not fit in `u16` bytes.
pub fn write_string(out: &mut Vec<u8>, s: &str) -> Result<(), ContractError> {
    let len = s.len();
    if len > u16::MAX as usize {
        return Err(ContractError::InvalidInput);
    }
    write_u16(out, len as u16);
    out.extend_from_slice(s.as_bytes());
    Ok(())
}

/// Append `u16` LE length then raw bytes. Errors if `data` does not fit in `u16` bytes.
pub fn write_bytes_u16(out: &mut Vec<u8>, data: &[u8]) -> Result<(), ContractError> {
    let len = data.len();
    if len > u16::MAX as usize {
        return Err(ContractError::InvalidInput);
    }
    write_u16(out, len as u16);
    out.extend_from_slice(data);
    Ok(())
}

/// Append `u32` LE length then raw bytes. Errors if `data` does not fit in `u32`.
pub fn write_bytes_u32(out: &mut Vec<u8>, data: &[u8]) -> Result<(), ContractError> {
    let len = data.len();
    if len > u32::MAX as usize {
        return Err(ContractError::InvalidInput);
    }
    write_u32(out, len as u32);
    out.extend_from_slice(data);
    Ok(())
}

/// Append a fixed-size blob with no length prefix (e.g. a 32-byte hash).
#[inline]
pub fn write_fixed<const N: usize>(out: &mut Vec<u8>, bytes: &[u8; N]) {
    out.extend_from_slice(bytes);
}
