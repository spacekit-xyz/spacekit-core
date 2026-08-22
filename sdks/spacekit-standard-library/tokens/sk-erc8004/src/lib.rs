//! ASTRA ERC-8004-style Agent Registry (no_std)
//!
//! Composition with existing ASTRA ERC-721:
//! - agent_id == token_id from the ERC-721 contract
//! - This contract does NOT mint/transfer tokens.
//! - It only stores agent profile, reputation, and validation records.

#![no_std]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::vec;

use spacekit_contract_sdk::{ContractError, SpacekitContract, ContractErrorCode};
use spacekit_contract_sdk::spacekit_contract;

#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

#[cfg(all(target_arch = "wasm32", not(test)))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[link(wasm_import_module = "spacekit_storage")]
extern "C" {
    fn storage_save(key_ptr: *const u8, key_len: usize, data_ptr: *const u8, data_len: usize) -> i32;
    fn storage_load(key_ptr: *const u8, key_len: usize, dest_ptr: *mut u8, max_len: usize) -> usize;
}

struct AstraAgent8004Contract;

// --- Opcodes ---
// Agent profile
const OP_AGENT_PROFILE_SET: u8 = 1;
const OP_AGENT_PROFILE_GET: u8 = 2;

// Reputation
const OP_AGENT_FEEDBACK_SUBMIT: u8 = 3;
const OP_AGENT_FEEDBACK_GET: u8    = 4;

// Validation
const OP_AGENT_VALIDATION_SET: u8  = 5;
const OP_AGENT_VALIDATION_GET: u8  = 6;

impl SpacekitContract for AstraAgent8004Contract {
    type Error = ContractError;

    fn init() -> Self {
        AstraAgent8004Contract
    }

    fn handle(&mut self, input: &[u8]) -> Result<Vec<u8>, ContractError> {
        handle(input)
    }
}

spacekit_contract!(AstraAgent8004Contract);

fn handle(input: &[u8]) -> Result<Vec<u8>, ContractError> {
    let mut cursor = 0usize;
    let op = read_u8(input, &mut cursor)?;

    match op {
        // -------------------
        // Agent profile
        // -------------------
        //
        // agent_id == token_id from ERC-721
        //
        // Input:  [op][agent_id:u64][profile_uri:string]
        // Output: [1] on success
        OP_AGENT_PROFILE_SET => {
            let agent_id    = read_u64(input, &mut cursor)?;
            let profile_uri = read_string(input, &mut cursor)?;
            if profile_uri.is_empty() {
                return Err(ContractError::InvalidInput);
            }
            storage_save_bytes(&agent_profile_key(agent_id), profile_uri.as_bytes())?;
            Ok(vec![1u8])
        }

        // Input:  [op][agent_id:u64]
        // Output: [1][profile_uri_bytes...]
        OP_AGENT_PROFILE_GET => {
            let agent_id = read_u64(input, &mut cursor)?;
            let data = storage_load_bytes(&agent_profile_key(agent_id), 512)?;
            let uri = core::str::from_utf8(&data)
                .map_err(|_| ContractError::InvalidInput)?;
            let mut out = Vec::new();
            out.push(1u8);
            out.extend_from_slice(uri.as_bytes());
            Ok(out)
        }

        // -------------------
        // Reputation
        // -------------------
        //
        // Simple aggregate: (sum:u64, count:u64)
        //
        // Input:  [op][agent_id:u64][score:u8]
        // Output: [1] on success
        OP_AGENT_FEEDBACK_SUBMIT => {
            let agent_id = read_u64(input, &mut cursor)?;
            let score    = read_u8(input, &mut cursor)?;

            let key = agent_reputation_key(agent_id);
            let current = storage_load_bytes(&key, 16).ok();
            let (mut sum, mut count) = if let Some(data) = current {
                if data.len() == 16 {
                    let s = u64::from_le_bytes(data[0..8].try_into().unwrap());
                    let c = u64::from_le_bytes(data[8..16].try_into().unwrap());
                    (s, c)
                } else {
                    (0, 0)
                }
            } else {
                (0, 0)
            };

            sum = sum.saturating_add(score as u64);
            count = count.saturating_add(1);

            let mut buf = Vec::with_capacity(16);
            buf.extend_from_slice(&sum.to_le_bytes());
            buf.extend_from_slice(&count.to_le_bytes());
            storage_save_bytes(&key, &buf)?;
            Ok(vec![1u8])
        }

        // Input:  [op][agent_id:u64]
        // Output: [1][avg_score:u64][count:u64]
        OP_AGENT_FEEDBACK_GET => {
            let agent_id = read_u64(input, &mut cursor)?;
            let key = agent_reputation_key(agent_id);
            let data = storage_load_bytes(&key, 16).unwrap_or_else(|_| {
                let mut z = Vec::with_capacity(16);
                z.extend_from_slice(&0u64.to_le_bytes());
                z.extend_from_slice(&0u64.to_le_bytes());
                z
            });

            let sum   = u64::from_le_bytes(data[0..8].try_into().unwrap());
            let count = u64::from_le_bytes(data[8..16].try_into().unwrap());
            let avg = if count == 0 { 0 } else { sum / count };

            let mut out = Vec::new();
            out.push(1u8);
            out.extend_from_slice(&avg.to_le_bytes());
            out.extend_from_slice(&count.to_le_bytes());
            Ok(out)
        }

        // -------------------
        // Validation
        // -------------------
        //
        // Validation record: task_id -> (agent_id, status, proof_uri)
        //
        // Input:  [op][task_id:string][agent_id:u64][status:u8][proof_uri:string]
        // Output: [1] on success
        OP_AGENT_VALIDATION_SET => {
            let task_id   = read_string(input, &mut cursor)?;
            let agent_id  = read_u64(input, &mut cursor)?;
            let status    = read_u8(input, &mut cursor)?;
            let proof_uri = read_string(input, &mut cursor)?;

            let mut value = Vec::new();
            value.extend_from_slice(&agent_id.to_le_bytes());
            value.push(status);
            value.extend_from_slice(proof_uri.as_bytes());

            storage_save_bytes(&agent_validation_key(&task_id), &value)?;
            Ok(vec![1u8])
        }

        // Input:  [op][task_id:string]
        // Output: [1][agent_id:u64][status:u8][proof_uri_bytes...]
        OP_AGENT_VALIDATION_GET => {
            let task_id = read_string(input, &mut cursor)?;
            let data = storage_load_bytes(&agent_validation_key(&task_id), 8 + 1 + 512)?;
            if data.len() < 9 {
                return Err(ContractError::InvalidInput);
            }

            let agent_id = u64::from_le_bytes(data[0..8].try_into().unwrap());
            let status   = data[8];
            let proof_uri_bytes = &data[9..];
            let proof_uri = core::str::from_utf8(proof_uri_bytes)
                .map_err(|_| ContractError::InvalidInput)?;

            let mut out = Vec::new();
            out.push(1u8);
            out.extend_from_slice(&agent_id.to_le_bytes());
            out.push(status);
            out.extend_from_slice(proof_uri.as_bytes());
            Ok(out)
        }

        _ => Err(ContractError::InvalidInput),
    }
}

// -------------------
// Storage helpers
// -------------------

fn storage_save_bytes(key: &str, data: &[u8]) -> Result<(), ContractError> {
    let result = unsafe { storage_save(key.as_ptr(), key.len(), data.as_ptr(), data.len()) };
    if result >= 0 {
        Ok(())
    } else {
        Err(ContractError::StorageError)
    }
}

fn storage_load_bytes(key: &str, max_len: usize) -> Result<Vec<u8>, ContractError> {
    let mut buffer = vec![0u8; max_len];
    let read_len = unsafe { storage_load(key.as_ptr(), key.len(), buffer.as_mut_ptr(), max_len) };
    if read_len == 0 {
        return Err(ContractError::StorageError);
    }
    buffer.truncate(read_len);
    Ok(buffer)
}

// -------------------
// Key builders
// -------------------

fn agent_profile_key(agent_id: u64) -> String {
    let mut key = String::from("astra:erc8004:profile:");
    key.push_str(&agent_id.to_string());
    key
}

fn agent_reputation_key(agent_id: u64) -> String {
    let mut key = String::from("astra:erc8004:reputation:");
    key.push_str(&agent_id.to_string());
    key
}

fn agent_validation_key(task_id: &str) -> String {
    let mut key = String::from("astra:erc8004:validation:");
    key.push_str(task_id);
    key
}

// -------------------
// IO helpers
// -------------------

fn read_u8(input: &[u8], cursor: &mut usize) -> Result<u8, ContractError> {
    if *cursor >= input.len() {
        return Err(ContractError::InvalidInput);
    }
    let value = input[*cursor];
    *cursor += 1;
    Ok(value)
}

fn read_u16(input: &[u8], cursor: &mut usize) -> Result<u16, ContractError> {
    if *cursor + 2 > input.len() {
        return Err(ContractError::InvalidInput);
    }
    let bytes = [input[*cursor], input[*cursor + 1]];
    *cursor += 2;
    Ok(u16::from_le_bytes(bytes))
}

fn read_u64(input: &[u8], cursor: &mut usize) -> Result<u64, ContractError> {
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

fn read_string(input: &[u8], cursor: &mut usize) -> Result<String, ContractError> {
    let len = read_u16(input, cursor)? as usize;
    if *cursor + len > input.len() {
        return Err(ContractError::InvalidInput);
    }
    let slice = &input[*cursor..*cursor + len];
    *cursor += len;
    core::str::from_utf8(slice)
        .map(|s| s.to_string())
        .map_err(|_| ContractError::InvalidInput)
}
