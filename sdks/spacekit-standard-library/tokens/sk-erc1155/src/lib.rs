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
fn panic(_info: &core::panic::PanicInfo) -> ! { loop {} }

#[link(wasm_import_module = "spacekit_storage")]
extern "C" {
    fn storage_save(key_ptr: *const u8, key_len: usize, data_ptr: *const u8, data_len: usize) -> i32;
    fn storage_load(key_ptr: *const u8, key_len: usize, dest_ptr: *mut u8, max_len: usize) -> usize;
}

struct SpaceKitErc1155;

const OP_BALANCE_OF: u8      = 1;
const OP_BALANCE_OF_BATCH: u8= 2;
const OP_MINT: u8            = 3;
const OP_BURN: u8            = 4;
const OP_URI: u8             = 5;

impl SpacekitContract for SpaceKitErc1155 {
    type Error = ContractError;
    fn init() -> Self { SpaceKitErc1155 }
    fn handle(&mut self, input: &[u8]) -> Result<Vec<u8>, ContractError> { handle(input) }
}

spacekit_contract!(SpaceKitErc1155);

fn handle(input: &[u8]) -> Result<Vec<u8>, ContractError> {
    let mut cursor = 0usize;
    let op = read_u8(input, &mut cursor)?;

    match op {
        // [op][account:string][id:u64]
        OP_BALANCE_OF => {
            let account = read_string(input, &mut cursor)?;
            let id = read_u64(input, &mut cursor)?;
            let bal = get_balance(&account, id);
            Ok(bal.to_le_bytes().to_vec())
        }

        // [op][n:u8][account_1:string][id_1:u64]...[account_n][id_n]
        OP_BALANCE_OF_BATCH => {
            let n = read_u8(input, &mut cursor)? as usize;
            let mut out = Vec::with_capacity(1 + n * 8);
            out.push(1u8);
            for _ in 0..n {
                let account = read_string(input, &mut cursor)?;
                let id = read_u64(input, &mut cursor)?;
                let bal = get_balance(&account, id);
                out.extend_from_slice(&bal.to_le_bytes());
            }
            Ok(out)
        }

        // [op][to:string][id:u64][amount:u64]
        OP_MINT => {
            let to = read_string(input, &mut cursor)?;
            let id = read_u64(input, &mut cursor)?;
            let amount = read_u64(input, &mut cursor)?;
            if to.is_empty() || amount == 0 { return Err(ContractError::InvalidInput); }

            let bal = get_balance(&to, id);
            let new_bal = bal.checked_add(amount).ok_or(ContractError::InvalidInput)?;
            set_balance(&to, id, new_bal)?;
            Ok(vec![1u8])
        }

        // [op][from:string][id:u64][amount:u64]
        OP_BURN => {
            let from = read_string(input, &mut cursor)?;
            let id = read_u64(input, &mut cursor)?;
            let amount = read_u64(input, &mut cursor)?;
            if from.is_empty() || amount == 0 { return Err(ContractError::InvalidInput); }

            let bal = get_balance(&from, id);
            if bal < amount { return Err(ContractError::InvalidInput); }
            let new_bal = bal - amount;
            set_balance(&from, id, new_bal)?;
            Ok(vec![1u8])
        }

        // [op][id:u64] -> [1][uri:string]
        OP_URI => {
            let id = read_u64(input, &mut cursor)?;
            let key = uri_key(id);
            let data = storage_load_bytes(&key, 512)?;
            let uri = core::str::from_utf8(&data).map_err(|_| ContractError::InvalidInput)?;
            let mut out = Vec::new();
            out.push(1u8);
            out.extend_from_slice(uri.as_bytes());
            Ok(out)
        }

        _ => Err(ContractError::InvalidInput),
    }
}

fn balance_key(account: &str, id: u64) -> String {
    let mut k = String::from("erc1155:bal:");
    k.push_str(account);
    k.push(':');
    k.push_str(&id.to_string());
    k
}

fn uri_key(id: u64) -> String {
    let mut k = String::from("erc1155:uri:");
    k.push_str(&id.to_string());
    k
}

fn get_balance(account: &str, id: u64) -> u64 {
    let key = balance_key(account, id);
    match storage_load_bytes(&key, 8) {
        Ok(d) if d.len() == 8 => u64::from_le_bytes(d.try_into().unwrap()),
        _ => 0,
    }
}

fn set_balance(account: &str, id: u64, amount: u64) -> Result<(), ContractError> {
    let key = balance_key(account, id);
    storage_save_bytes(&key, &amount.to_le_bytes())
}

fn storage_save_bytes(key: &str, data: &[u8]) -> Result<(), ContractError> {
    let r = unsafe { storage_save(key.as_ptr(), key.len(), data.as_ptr(), data.len()) };
    if r >= 0 { Ok(()) } else { Err(ContractError::StorageError) }
}

fn storage_load_bytes(key: &str, max_len: usize) -> Result<Vec<u8>, ContractError> {
    let mut buf = vec![0u8; max_len];
    let n = unsafe { storage_load(key.as_ptr(), key.len(), buf.as_mut_ptr(), max_len) };
    if n == 0 { return Err(ContractError::StorageError); }
    buf.truncate(n);
    Ok(buf)
}

fn read_u8(input: &[u8], cursor: &mut usize) -> Result<u8, ContractError> {
    if *cursor >= input.len() { return Err(ContractError::InvalidInput); }
    let v = input[*cursor]; *cursor += 1; Ok(v)
}

fn read_u16(input: &[u8], cursor: &mut usize) -> Result<u16, ContractError> {
    if *cursor + 2 > input.len() { return Err(ContractError::InvalidInput); }
    let b = [input[*cursor], input[*cursor + 1]]; *cursor += 2; Ok(u16::from_le_bytes(b))
}

fn read_u64(input: &[u8], cursor: &mut usize) -> Result<u64, ContractError> {
    if *cursor + 8 > input.len() { return Err(ContractError::InvalidInput); }
    let b = [
        input[*cursor], input[*cursor + 1], input[*cursor + 2], input[*cursor + 3],
        input[*cursor + 4], input[*cursor + 5], input[*cursor + 6], input[*cursor + 7],
    ];
    *cursor += 8;
    Ok(u64::from_le_bytes(b))
}

fn read_string(input: &[u8], cursor: &mut usize) -> Result<String, ContractError> {
    let len = read_u16(input, cursor)? as usize;
    if *cursor + len > input.len() { return Err(ContractError::InvalidInput); }
    let slice = &input[*cursor..*cursor + len]; *cursor += len;
    core::str::from_utf8(slice).map(|s| s.to_string()).map_err(|_| ContractError::InvalidInput)
}
