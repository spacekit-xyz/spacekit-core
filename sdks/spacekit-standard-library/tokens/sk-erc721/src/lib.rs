//! ASTRA ERC-721 Contract (no_std)
//!
//! Basic ERC-721 style contract for ASTRA collectibles.

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

struct AstraErc721Contract;

const OP_MINT: u8 = 1;
const OP_TRANSFER: u8 = 2;
const OP_OWNER_OF: u8 = 3;
const OP_SET_TOKEN_URI: u8 = 4;
const OP_TOKEN_URI: u8 = 5;
const OP_TOTAL_SUPPLY: u8 = 6;

impl SpacekitContract for AstraErc721Contract {
    type Error = ContractError;

    fn init() -> Self {
        AstraErc721Contract
    }

    fn handle(&mut self, input: &[u8]) -> Result<Vec<u8>, ContractError> {
        handle(input)
    }
}

#[cfg(not(test))]
spacekit_contract!(AstraErc721Contract);

fn handle(input: &[u8]) -> Result<Vec<u8>, ContractError> {
    let mut cursor = 0usize;
    let op = read_u8(input, &mut cursor)?;

    match op {
        OP_MINT => {
            let token_id = read_u64(input, &mut cursor)?;
            let owner = read_string(input, &mut cursor)?;
            if owner.is_empty() {
                return Err(ContractError::InvalidInput);
            }
            if owner_exists(token_id) {
                return Err(ContractError::InvalidInput);
            }
            set_owner(token_id, &owner)?;
            increment_total_supply()?;
            Ok(vec![1u8])
        }
        OP_TRANSFER => {
            let token_id = read_u64(input, &mut cursor)?;
            let from = read_string(input, &mut cursor)?;
            let to = read_string(input, &mut cursor)?;
            if from.is_empty() || to.is_empty() {
                return Err(ContractError::InvalidInput);
            }
            let current_owner = get_owner(token_id)?;
            if current_owner != from {
                return Err(ContractError::InvalidInput);
            }
            set_owner(token_id, &to)?;
            Ok(vec![1u8])
        }
        OP_OWNER_OF => {
            let token_id = read_u64(input, &mut cursor)?;
            let owner = get_owner(token_id)?;
            Ok(owner.into_bytes())
        }
        OP_SET_TOKEN_URI => {
            let token_id = read_u64(input, &mut cursor)?;
            let uri = read_string(input, &mut cursor)?;
            if !owner_exists(token_id) {
                return Err(ContractError::InvalidInput);
            }
            set_token_uri(token_id, &uri)?;
            Ok(vec![1u8])
        }
        OP_TOKEN_URI => {
            let token_id = read_u64(input, &mut cursor)?;
            let uri = get_token_uri(token_id)?;
            Ok(uri.into_bytes())
        }
        OP_TOTAL_SUPPLY => {
            let supply = get_total_supply();
            Ok(supply.to_le_bytes().to_vec())
        }
        _ => Err(ContractError::InvalidInput),
    }
}

fn storage_save_bytes(key: &str, data: &[u8]) -> Result<(), ContractError> {
    let result = unsafe { storage_save(key.as_ptr(), key.len(), data.as_ptr(), data.len()) };
    if result >= 0 { Ok(()) } else { Err(ContractError::StorageError) }
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

fn owner_key(token_id: u64) -> String {
    let mut key = String::from("astra:erc721:owner:");
    key.push_str(&token_id.to_string());
    key
}

fn uri_key(token_id: u64) -> String {
    let mut key = String::from("astra:erc721:uri:");
    key.push_str(&token_id.to_string());
    key
}

fn total_supply_key() -> String {
    "astra:erc721:total_supply".to_string()
}

fn owner_exists(token_id: u64) -> bool {
    storage_load_bytes(&owner_key(token_id), 256).is_ok()
}

fn set_owner(token_id: u64, owner: &str) -> Result<(), ContractError> {
    storage_save_bytes(&owner_key(token_id), owner.as_bytes())
}

fn get_owner(token_id: u64) -> Result<String, ContractError> {
    let data = storage_load_bytes(&owner_key(token_id), 256)?;
    core::str::from_utf8(&data)
        .map(|s| s.to_string())
        .map_err(|_| ContractError::InvalidInput)
}

fn set_token_uri(token_id: u64, uri: &str) -> Result<(), ContractError> {
    storage_save_bytes(&uri_key(token_id), uri.as_bytes())
}

fn get_token_uri(token_id: u64) -> Result<String, ContractError> {
    let data = storage_load_bytes(&uri_key(token_id), 512)?;
    core::str::from_utf8(&data)
        .map(|s| s.to_string())
        .map_err(|_| ContractError::InvalidInput)
}

fn get_total_supply() -> u64 {
    match storage_load_bytes(&total_supply_key(), 8) {
        Ok(data) if data.len() == 8 => u64::from_le_bytes([
            data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
        ]),
        _ => 0,
    }
}

fn increment_total_supply() -> Result<(), ContractError> {
    let current = get_total_supply();
    let next = current.checked_add(1).ok_or(ContractError::InvalidInput)?;
    storage_save_bytes(&total_supply_key(), &next.to_le_bytes())
}

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
