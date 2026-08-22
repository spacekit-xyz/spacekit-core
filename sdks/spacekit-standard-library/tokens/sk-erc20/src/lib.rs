//! SpaceUSD ERC-20 Contract (no_std)
//!
//! Demo ERC-20 token for SpaceKit playground.
//! This is separate from Native ASTRA (the chain's native currency).

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

struct SpaceUsdErc20Contract;

const OP_MINT: u8 = 1;
const OP_TRANSFER: u8 = 2;
const OP_BALANCE: u8 = 3;
const OP_TOTAL_SUPPLY: u8 = 4;
const OP_METADATA: u8 = 5;

const TOKEN_NAME: &str = "SpaceUSD";
const TOKEN_SYMBOL: &str = "SUSD";
const TOKEN_DECIMALS: u8 = 18;

/// TODO: Add events, ownership, approvals, and allowances, burn option
impl SpacekitContract for SpaceUsdErc20Contract {
    type Error = ContractError;

    fn init() -> Self {
        SpaceUsdErc20Contract
    }

    fn handle(&mut self, input: &[u8]) -> Result<Vec<u8>, ContractError> {
        handle(input)
    }
}

spacekit_contract!(SpaceUsdErc20Contract);

fn handle(input: &[u8]) -> Result<Vec<u8>, ContractError> {
    let mut cursor = 0usize;
    let op = read_u8(input, &mut cursor)?;

    match op {
        OP_MINT => {
            let to_did = read_string(input, &mut cursor)?;
            let amount = read_u64(input, &mut cursor)?;
            if to_did.is_empty() || amount == 0 {
                return Err(ContractError::InvalidInput);
            }

            let balance = get_balance(&to_did);
            let new_balance = balance.checked_add(amount).ok_or(ContractError::InvalidInput)?;
            set_balance(&to_did, new_balance)?;

            let total_supply = get_total_supply();
            let new_supply = total_supply.checked_add(amount).ok_or(ContractError::InvalidInput)?;
            set_total_supply(new_supply)?;

            Ok(vec![1u8])
        }
        OP_TRANSFER => {
            let from_did = read_string(input, &mut cursor)?;
            let to_did = read_string(input, &mut cursor)?;
            let amount = read_u64(input, &mut cursor)?;
            if from_did.is_empty() || to_did.is_empty() || amount == 0 {
                return Err(ContractError::InvalidInput);
            }

            let from_balance = get_balance(&from_did);
            if from_balance < amount {
                return Err(ContractError::InvalidInput);
            }

            let to_balance = get_balance(&to_did);
            let new_to = to_balance.checked_add(amount).ok_or(ContractError::InvalidInput)?;
            let new_from = from_balance - amount;

            set_balance(&from_did, new_from)?;
            set_balance(&to_did, new_to)?;
            Ok(vec![1u8])
        }
        OP_BALANCE => {
            let did = read_string(input, &mut cursor)?;
            let balance = get_balance(&did);
            Ok(balance.to_le_bytes().to_vec())
        }
        OP_TOTAL_SUPPLY => {
            let supply = get_total_supply();
            Ok(supply.to_le_bytes().to_vec())
        }
        OP_METADATA => {
            let mut out = Vec::new();
            out.push(1u8);
            write_string(&mut out, TOKEN_NAME)?;
            write_string(&mut out, TOKEN_SYMBOL)?;
            out.push(TOKEN_DECIMALS);
            Ok(out)
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

fn balance_key(did: &str) -> String {
    let mut key = String::from("spaceusd:erc20:balance:");
    key.push_str(did);
    key
}

fn total_supply_key() -> String {
    "spaceusd:erc20:total_supply".to_string()
}

fn get_balance(did: &str) -> u64 {
    let key = balance_key(did);
    match storage_load_bytes(&key, 8) {
        Ok(data) if data.len() == 8 => u64::from_le_bytes([
            data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
        ]),
        _ => 0,
    }
}

fn set_balance(did: &str, amount: u64) -> Result<(), ContractError> {
    let key = balance_key(did);
    storage_save_bytes(&key, &amount.to_le_bytes())
}

fn get_total_supply() -> u64 {
    match storage_load_bytes(&total_supply_key(), 8) {
        Ok(data) if data.len() == 8 => u64::from_le_bytes([
            data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
        ]),
        _ => 0,
    }
}

fn set_total_supply(amount: u64) -> Result<(), ContractError> {
    storage_save_bytes(&total_supply_key(), &amount.to_le_bytes())
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

fn write_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn write_string(out: &mut Vec<u8>, value: &str) -> Result<(), ContractError> {
    let len = value.len();
    if len > u16::MAX as usize {
        return Err(ContractError::InvalidInput);
    }
    write_u16(out, len as u16);
    out.extend_from_slice(value.as_bytes());
    Ok(())
}
