//! Payable Demo Contract (no_std)
//!
//! Records attached value and allows withdrawals.

#![no_std]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::vec;

use spacekit_contract_sdk::{
    ContractError,
    ContractErrorCode,
    SpacekitContract,
    msg_value_u64,
    get_caller_did_string,
};
use spacekit_contract_sdk::spacekit_contract;
use spacekit_contract_sdk::emit_event_bytes;

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

struct PayableDemoContract;

const OP_DEPOSIT: u8 = 1;
const OP_TOTAL_RECEIVED: u8 = 2;
const OP_WITHDRAW: u8 = 3;
const OP_BALANCE: u8 = 4;

const TOTAL_KEY: &str = "payable:total_received";
const BALANCE_KEY: &str = "payable:balance";
const CONTRACT_DID: &str = "did:spacekit:contract:payable-demo";

impl SpacekitContract for PayableDemoContract {
    type Error = ContractError;

    fn init() -> Self {
        PayableDemoContract
    }

    fn handle(&mut self, input: &[u8]) -> Result<Vec<u8>, ContractError> {
        handle(input)
    }
}

spacekit_contract!(PayableDemoContract);

fn handle(input: &[u8]) -> Result<Vec<u8>, ContractError> {
    let mut cursor = 0usize;
    let op = read_u8(input, &mut cursor)?;

    match op {
        OP_DEPOSIT => {
            let amount = msg_value_u64();
            if amount == 0 {
                return Err(ContractError::InvalidInput);
            }
            let current = get_total_received();
            let next = current.checked_add(amount).ok_or(ContractError::InvalidInput)?;
            set_total_received(next)?;
            add_balance(amount)?;
            credit_contract_balance(amount)?;

            if let Ok(caller) = get_caller_did_string() {
                let event = format_deposit_event(&caller, amount);
                emit_event_bytes("payable.deposit", event.as_bytes());
            }
            Ok(vec![1u8])
        }
        OP_TOTAL_RECEIVED => {
            let total = get_total_received();
            Ok(total.to_le_bytes().to_vec())
        }
        OP_WITHDRAW => {
            let amount = read_u64(input, &mut cursor)?;
            if amount == 0 {
                return Err(ContractError::InvalidInput);
            }
            let current = get_balance();
            if current < amount {
                return Err(ContractError::InvalidInput);
            }
            set_balance(current - amount)?;
            debit_contract_balance(amount)?;
            if let Ok(caller) = get_caller_did_string() {
                credit_caller_balance(&caller, amount)?;
                let event = format_withdraw_event(&caller, amount);
                emit_event_bytes("payable.withdraw", event.as_bytes());
            }
            Ok(vec![1u8])
        }
        OP_BALANCE => {
            let balance = get_balance();
            Ok(balance.to_le_bytes().to_vec())
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

fn get_total_received() -> u64 {
    match storage_load_bytes(TOTAL_KEY, 8) {
        Ok(data) if data.len() == 8 => u64::from_le_bytes([
            data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
        ]),
        _ => 0,
    }
}

fn set_total_received(amount: u64) -> Result<(), ContractError> {
    storage_save_bytes(TOTAL_KEY, &amount.to_le_bytes())
}

fn get_balance() -> u64 {
    match storage_load_bytes(BALANCE_KEY, 8) {
        Ok(data) if data.len() == 8 => u64::from_le_bytes([
            data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
        ]),
        _ => 0,
    }
}

fn set_balance(amount: u64) -> Result<(), ContractError> {
    storage_save_bytes(BALANCE_KEY, &amount.to_le_bytes())
}

fn add_balance(amount: u64) -> Result<(), ContractError> {
    let current = get_balance();
    let next = current.checked_add(amount).ok_or(ContractError::InvalidInput)?;
    set_balance(next)
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

fn credit_contract_balance(amount: u64) -> Result<(), ContractError> {
    let key = format_balance_key(CONTRACT_DID);
    let current = get_balance_for_key(&key);
    let next = current.checked_add(amount).ok_or(ContractError::InvalidInput)?;
    storage_save_bytes(&key, &next.to_le_bytes())
}

fn debit_contract_balance(amount: u64) -> Result<(), ContractError> {
    let key = format_balance_key(CONTRACT_DID);
    let current = get_balance_for_key(&key);
    if current < amount {
        return Err(ContractError::InvalidInput);
    }
    storage_save_bytes(&key, &(current - amount).to_le_bytes())
}

fn credit_caller_balance(caller: &str, amount: u64) -> Result<(), ContractError> {
    let key = format_balance_key(caller);
    let current = get_balance_for_key(&key);
    let next = current.checked_add(amount).ok_or(ContractError::InvalidInput)?;
    storage_save_bytes(&key, &next.to_le_bytes())
}

fn get_balance_for_key(key: &str) -> u64 {
    match storage_load_bytes(key, 8) {
        Ok(data) if data.len() == 8 => u64::from_le_bytes([
            data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
        ]),
        _ => 0,
    }
}

fn format_balance_key(did: &str) -> String {
    let mut key = String::from("astra:erc20:balance:");
    key.push_str(did);
    key
}

fn read_u8(input: &[u8], cursor: &mut usize) -> Result<u8, ContractError> {
    if *cursor >= input.len() {
        return Err(ContractError::InvalidInput);
    }
    let value = input[*cursor];
    *cursor += 1;
    Ok(value)
}

fn format_deposit_event(caller: &str, amount: u64) -> String {
    let mut out = String::new();
    out.push_str(caller);
    out.push('|');
    out.push_str(&amount.to_string());
    out
}

fn format_withdraw_event(caller: &str, amount: u64) -> String {
    let mut out = String::from("withdraw|");
    out.push_str(caller);
    out.push('|');
    out.push_str(&amount.to_string());
    out
}
