//! aUSD Stablecoin — WASM ERC-20 Contract
//!
//! USD-pegged stablecoin for the SpaceKit marketplace. Minting and burning
//! are restricted to the authorized bridge address (set at deploy time).

#![no_std]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use spacekit_contract_sdk::{ContractError, ContractErrorCode, SpacekitContract};
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
    fn storage_load(key_ptr: *const u8, key_len: usize, dest_ptr: *mut u8, max_len: usize) -> i32;
}

// Opcodes (first byte of input buffer)
const OP_MINT: u8 = 0x01;
const OP_BURN: u8 = 0x02;
const OP_TRANSFER: u8 = 0x03;
const OP_BALANCE_OF: u8 = 0x04;
const OP_TOTAL_SUPPLY: u8 = 0x05;
const OP_APPROVE: u8 = 0x06;
const OP_ALLOWANCE: u8 = 0x07;
const OP_TRANSFER_FROM: u8 = 0x08;
const OP_SET_BRIDGE: u8 = 0x09;
const OP_METADATA: u8 = 0x0A;

const TOKEN_NAME: &str = "SpaceKit USD";
const TOKEN_SYMBOL: &str = "aUSD";
const TOKEN_DECIMALS: u8 = 2; // 2 decimals: 1 aUSD = 100 units

struct AusdContract;

impl SpacekitContract for AusdContract {
    type Error = ContractError;

    fn init() -> Self {
        AusdContract
    }

    fn handle(&mut self, input: &[u8]) -> Result<Vec<u8>, ContractError> {
        handle(input)
    }
}

spacekit_contract!(AusdContract);

fn handle(input: &[u8]) -> Result<Vec<u8>, ContractError> {
    let mut cursor = 0usize;
    let op = read_u8(input, &mut cursor)?;

    match op {
        OP_MINT => {
            let caller = read_string(input, &mut cursor)?;
            let to = read_string(input, &mut cursor)?;
            let amount = read_u64(input, &mut cursor)?;

            require_bridge(&caller)?;

            if to.is_empty() || amount == 0 {
                return Err(ContractError::InvalidInput);
            }

            let balance = get_balance(&to);
            let new_balance = balance.checked_add(amount).ok_or(ContractError::InvalidInput)?;
            set_balance(&to, new_balance)?;

            let supply = get_total_supply();
            set_total_supply(supply.checked_add(amount).ok_or(ContractError::InvalidInput)?)?;

            Ok(vec![1u8])
        }

        OP_BURN => {
            let caller = read_string(input, &mut cursor)?;
            let from = read_string(input, &mut cursor)?;
            let amount = read_u64(input, &mut cursor)?;

            // Bridge can burn anyone's tokens; holders can burn their own
            let bridge = get_bridge_address();
            if caller != bridge && caller != from {
                return Err(ContractError::Failed);
            }

            if amount == 0 {
                return Err(ContractError::InvalidInput);
            }

            let balance = get_balance(&from);
            if balance < amount {
                return Err(ContractError::InvalidInput);
            }

            set_balance(&from, balance - amount)?;

            let supply = get_total_supply();
            set_total_supply(supply.saturating_sub(amount))?;

            Ok(vec![1u8])
        }

        OP_TRANSFER => {
            let from = read_string(input, &mut cursor)?;
            let to = read_string(input, &mut cursor)?;
            let amount = read_u64(input, &mut cursor)?;

            do_transfer(&from, &to, amount)?;
            Ok(vec![1u8])
        }

        OP_BALANCE_OF => {
            let account = read_string(input, &mut cursor)?;
            let balance = get_balance(&account);
            Ok(balance.to_le_bytes().to_vec())
        }

        OP_TOTAL_SUPPLY => {
            let supply = get_total_supply();
            Ok(supply.to_le_bytes().to_vec())
        }

        OP_APPROVE => {
            let owner = read_string(input, &mut cursor)?;
            let spender = read_string(input, &mut cursor)?;
            let amount = read_u64(input, &mut cursor)?;

            set_allowance(&owner, &spender, amount)?;
            Ok(vec![1u8])
        }

        OP_ALLOWANCE => {
            let owner = read_string(input, &mut cursor)?;
            let spender = read_string(input, &mut cursor)?;
            let allowance = get_allowance(&owner, &spender);
            Ok(allowance.to_le_bytes().to_vec())
        }

        OP_TRANSFER_FROM => {
            let spender = read_string(input, &mut cursor)?;
            let from = read_string(input, &mut cursor)?;
            let to = read_string(input, &mut cursor)?;
            let amount = read_u64(input, &mut cursor)?;

            let current_allowance = get_allowance(&from, &spender);
            if current_allowance < amount {
                return Err(ContractError::Failed);
            }

            do_transfer(&from, &to, amount)?;
            set_allowance(&from, &spender, current_allowance - amount)?;
            Ok(vec![1u8])
        }

        OP_SET_BRIDGE => {
            let caller = read_string(input, &mut cursor)?;
            let new_bridge = read_string(input, &mut cursor)?;

            // Only the current bridge (or if no bridge set yet) can change the bridge address
            let current = get_bridge_address();
            if !current.is_empty() && caller != current {
                return Err(ContractError::Failed);
            }

            set_bridge_address(&new_bridge)?;
            Ok(vec![1u8])
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

fn do_transfer(from: &str, to: &str, amount: u64) -> Result<(), ContractError> {
    if from.is_empty() || to.is_empty() || amount == 0 {
        return Err(ContractError::InvalidInput);
    }

    let from_balance = get_balance(from);
    if from_balance < amount {
        return Err(ContractError::InvalidInput);
    }

    let to_balance = get_balance(to);
    let new_to = to_balance.checked_add(amount).ok_or(ContractError::InvalidInput)?;

    set_balance(from, from_balance - amount)?;
    set_balance(to, new_to)?;
    Ok(())
}

fn require_bridge(caller: &str) -> Result<(), ContractError> {
    let bridge = get_bridge_address();
    if bridge.is_empty() {
        // No bridge set yet — first call initializes it
        set_bridge_address(caller)?;
        return Ok(());
    }
    if caller != bridge {
        return Err(ContractError::Failed);
    }
    Ok(())
}

// ── Storage helpers ──────────────────────────────────────────────────────────

fn storage_save_bytes(key: &str, data: &[u8]) -> Result<(), ContractError> {
    let result = unsafe { storage_save(key.as_ptr(), key.len(), data.as_ptr(), data.len()) };
    if result >= 0 { Ok(()) } else { Err(ContractError::StorageError) }
}

fn storage_load_bytes(key: &str, max_len: usize) -> Option<Vec<u8>> {
    let mut buffer = vec![0u8; max_len];
    let read_len = unsafe { storage_load(key.as_ptr(), key.len(), buffer.as_mut_ptr(), max_len) };
    if read_len <= 0 {
        return None;
    }
    buffer.truncate(read_len as usize);
    Some(buffer)
}

fn balance_key(did: &str) -> String {
    let mut key = String::from("ausd:bal:");
    key.push_str(did);
    key
}

fn allowance_key(owner: &str, spender: &str) -> String {
    let mut key = String::from("ausd:allow:");
    key.push_str(owner);
    key.push(':');
    key.push_str(spender);
    key
}

fn get_balance(did: &str) -> u64 {
    let key = balance_key(did);
    match storage_load_bytes(&key, 8) {
        Some(data) if data.len() == 8 => u64::from_le_bytes([
            data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
        ]),
        _ => 0,
    }
}

fn set_balance(did: &str, amount: u64) -> Result<(), ContractError> {
    storage_save_bytes(&balance_key(did), &amount.to_le_bytes())
}

fn get_total_supply() -> u64 {
    match storage_load_bytes("ausd:supply", 8) {
        Some(data) if data.len() == 8 => u64::from_le_bytes([
            data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
        ]),
        _ => 0,
    }
}

fn set_total_supply(amount: u64) -> Result<(), ContractError> {
    storage_save_bytes("ausd:supply", &amount.to_le_bytes())
}

fn get_allowance(owner: &str, spender: &str) -> u64 {
    let key = allowance_key(owner, spender);
    match storage_load_bytes(&key, 8) {
        Some(data) if data.len() == 8 => u64::from_le_bytes([
            data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
        ]),
        _ => 0,
    }
}

fn set_allowance(owner: &str, spender: &str, amount: u64) -> Result<(), ContractError> {
    storage_save_bytes(&allowance_key(owner, spender), &amount.to_le_bytes())
}

fn get_bridge_address() -> String {
    storage_load_bytes("ausd:bridge", 256)
        .and_then(|bytes| core::str::from_utf8(&bytes).ok().map(|s| s.to_string()))
        .unwrap_or_default()
}

fn set_bridge_address(addr: &str) -> Result<(), ContractError> {
    storage_save_bytes("ausd:bridge", addr.as_bytes())
}

// ── Encoding helpers ─────────────────────────────────────────────────────────

fn read_u8(input: &[u8], cursor: &mut usize) -> Result<u8, ContractError> {
    if *cursor >= input.len() { return Err(ContractError::InvalidInput); }
    let v = input[*cursor];
    *cursor += 1;
    Ok(v)
}

fn read_u16(input: &[u8], cursor: &mut usize) -> Result<u16, ContractError> {
    if *cursor + 2 > input.len() { return Err(ContractError::InvalidInput); }
    let bytes = [input[*cursor], input[*cursor + 1]];
    *cursor += 2;
    Ok(u16::from_le_bytes(bytes))
}

fn read_u64(input: &[u8], cursor: &mut usize) -> Result<u64, ContractError> {
    if *cursor + 8 > input.len() { return Err(ContractError::InvalidInput); }
    let bytes = [
        input[*cursor], input[*cursor + 1], input[*cursor + 2], input[*cursor + 3],
        input[*cursor + 4], input[*cursor + 5], input[*cursor + 6], input[*cursor + 7],
    ];
    *cursor += 8;
    Ok(u64::from_le_bytes(bytes))
}

fn read_string(input: &[u8], cursor: &mut usize) -> Result<String, ContractError> {
    let len = read_u16(input, cursor)? as usize;
    if *cursor + len > input.len() { return Err(ContractError::InvalidInput); }
    let slice = &input[*cursor..*cursor + len];
    *cursor += len;
    core::str::from_utf8(slice).map(|s| s.to_string()).map_err(|_| ContractError::InvalidInput)
}

fn write_string(out: &mut Vec<u8>, value: &str) -> Result<(), ContractError> {
    let len = value.len();
    if len > u16::MAX as usize { return Err(ContractError::InvalidInput); }
    out.extend_from_slice(&(len as u16).to_le_bytes());
    out.extend_from_slice(value.as_bytes());
    Ok(())
}
