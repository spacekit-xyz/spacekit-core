//! SpaceKit Anchor — tamper-proof content-hash timestamps on chain.
//!
//! Stores `(caller_did, note_id) → content_hash + block_timestamp` in contract storage.
//! Note bodies never enter the contract — only a hex SHA-256 digest.
//!
//! Wire format (**little-endian** `u16` length prefixes):
//!
//! | Op | Opcode | Payload | Response |
//! |----|--------|---------|----------|
//! | HEALTH | `0x10` | (empty) | JSON status |
//! | ANCHOR | `0x01` | `[note_id blob][content_hash_hex blob]` | JSON anchor record |
//! | VERIFY | `0x02` | `[note_id blob]` | JSON anchor record or not-found |

#![no_std]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use spacekit_contract_sdk::{
    block_timestamp, emit_event_bytes, get_caller_did_string, payments::payment_vault_charge,
    spacekit_contract, storage_get, storage_set, ContractError, ContractErrorCode,
    SpacekitContract,
};

#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

#[cfg(all(target_arch = "wasm32", not(test)))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

struct SpaceKitAnchor;

const OP_ANCHOR: u8 = 0x01;
const OP_VERIFY: u8 = 0x02;
const OP_HEALTH: u8 = 0x10;

const COST_ANCHOR: &str = "50";
const STORAGE_MAX: usize = 512;
const HASH_HEX_LEN: usize = 64;

impl SpacekitContract for SpaceKitAnchor {
    type Error = ContractError;

    fn init() -> Self {
        SpaceKitAnchor
    }

    fn handle(&mut self, input: &[u8]) -> Result<Vec<u8>, ContractError> {
        if input.is_empty() {
            return Err(ContractError::InvalidInput);
        }
        match input[0] {
            OP_HEALTH => Ok(health_json()),
            OP_ANCHOR => handle_anchor(&input[1..]),
            OP_VERIFY => handle_verify(&input[1..]),
            _ => Err(ContractError::InvalidInput),
        }
    }
}

spacekit_contract!(SpaceKitAnchor);

fn health_json() -> Vec<u8> {
    br#"{"status":"ok","agent":"spacekit-anchor","version":1}"#.to_vec()
}

fn storage_key(caller: &str, note_id: &str) -> String {
    format!("anchor:{caller}:{note_id}")
}

fn validate_hash_hex(hash: &str) -> Result<(), ContractError> {
    if hash.len() != HASH_HEX_LEN {
        return Err(ContractError::InvalidInput);
    }
    if !hash.as_bytes().iter().all(|b| b.is_ascii_hexdigit()) {
        return Err(ContractError::InvalidInput);
    }
    Ok(())
}

fn handle_anchor(body: &[u8]) -> Result<Vec<u8>, ContractError> {
    let (note_id_bytes, rest) = read_blob_u16(body)?;
    let (hash_bytes, tail) = read_blob_u16(rest)?;
    if !tail.is_empty() || note_id_bytes.is_empty() || hash_bytes.is_empty() {
        return Err(ContractError::InvalidInput);
    }

    let note_id = core::str::from_utf8(&note_id_bytes).map_err(|_| ContractError::InvalidInput)?;
    let content_hash =
        core::str::from_utf8(&hash_bytes).map_err(|_| ContractError::InvalidInput)?;
    validate_hash_hex(content_hash)?;

    let caller = get_caller_did_string()?;
    payment_vault_charge(COST_ANCHOR, caller.as_str())?;

    let ts = block_timestamp();
    let record = format!(
        r#"{{"ok":true,"note_id":"{note_id}","content_hash":"{content_hash}","timestamp":{ts},"caller":"{caller}"}}"#
    );
    storage_set(
        storage_key(caller.as_str(), note_id).as_bytes(),
        record.as_bytes(),
    )?;

    emit_event_bytes(
        "spacekit.anchor.created",
        &(note_id.len() as u32).to_le_bytes(),
    );
    Ok(record.into_bytes())
}

fn handle_verify(body: &[u8]) -> Result<Vec<u8>, ContractError> {
    let (note_id_bytes, tail) = read_blob_u16(body)?;
    if !tail.is_empty() || note_id_bytes.is_empty() {
        return Err(ContractError::InvalidInput);
    }
    let note_id = core::str::from_utf8(&note_id_bytes).map_err(|_| ContractError::InvalidInput)?;
    let caller = get_caller_did_string()?;
    let key = storage_key(caller.as_str(), note_id);

    match storage_get(key.as_bytes(), STORAGE_MAX) {
        Some(bytes) if !bytes.is_empty() => Ok(bytes),
        _ => Ok(format!(
            r#"{{"ok":false,"note_id":"{note_id}","error":"not_found"}}"#
        )
        .into_bytes()),
    }
}

fn read_u16(cursor: &[u8]) -> Result<(usize, &[u8]), ContractError> {
    if cursor.len() < 2 {
        return Err(ContractError::InvalidInput);
    }
    Ok((
        usize::from(u16::from_le_bytes([cursor[0], cursor[1]])),
        &cursor[2..],
    ))
}

fn read_blob_u16(cursor: &[u8]) -> Result<(Vec<u8>, &[u8]), ContractError> {
    let (len, rest) = read_u16(cursor)?;
    if rest.len() < len {
        return Err(ContractError::InvalidInput);
    }
    Ok((rest[..len].to_vec(), &rest[len..]))
}
