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

/// ABI (short):
/// OP_GRANT_ROLE (1) – [op][role:string][account:string] -> [1]
/// OP_REVOKE_ROLE (2) – [op][role:string][account:string] -> [1]
/// OP_HAS_ROLE (3) – [op][role:string][account:string] -> [1][has:u8]
/// OP_SET_ADMIN (4) – [op][role:string][admin:string] -> [1]
/// OP_GET_ADMIN (5) – [op][role:string] -> [1][admin:string]
struct SpaceKitAccessControl;

// Opcodes
const OP_GRANT_ROLE: u8 = 1;
const OP_REVOKE_ROLE: u8 = 2;
const OP_HAS_ROLE: u8 = 3;
const OP_SET_ADMIN: u8 = 4;
const OP_GET_ADMIN: u8 = 5;

impl SpacekitContract for SpaceKitAccessControl {
    type Error = ContractError;
    fn init() -> Self { SpaceKitAccessControl }
    fn handle(&mut self, input: &[u8]) -> Result<Vec<u8>, ContractError> { handle(input) }
}

spacekit_contract!(SpaceKitAccessControl);

fn handle(input: &[u8]) -> Result<Vec<u8>, ContractError> {
    let mut cursor = 0usize;
    let op = read_u8(input, &mut cursor)?;

    match op {
        // [op][role:string][account:string]
        OP_GRANT_ROLE => {
            let role = read_string(input, &mut cursor)?;
            let account = read_string(input, &mut cursor)?;
            let key = role_member_key(&role, &account);
            storage_save_bytes(&key, &[1u8])?;
            Ok(vec![1u8])
        }

        // [op][role:string][account:string]
        OP_REVOKE_ROLE => {
            let role = read_string(input, &mut cursor)?;
            let account = read_string(input, &mut cursor)?;
            let key = role_member_key(&role, &account);
            // overwrite with 0; caller treats missing/0 as false
            storage_save_bytes(&key, &[0u8])?;
            Ok(vec![1u8])
        }

        // [op][role:string][account:string] -> [1][has:u8]
        OP_HAS_ROLE => {
            let role = read_string(input, &mut cursor)?;
            let account = read_string(input, &mut cursor)?;
            let key = role_member_key(&role, &account);
            let has = storage_load_bytes(&key, 1).map(|d| d.get(0).cloned().unwrap_or(0)).unwrap_or(0);
            Ok(vec![1u8, has])
        }

        // [op][role:string][admin:string]
        OP_SET_ADMIN => {
            let role = read_string(input, &mut cursor)?;
            let admin = read_string(input, &mut cursor)?;
            let key = role_admin_key(&role);
            storage_save_bytes(&key, admin.as_bytes())?;
            Ok(vec![1u8])
        }

        // [op][role:string] -> [1][admin:string]
        OP_GET_ADMIN => {
            let role = read_string(input, &mut cursor)?;
            let key = role_admin_key(&role);
            let data = storage_load_bytes(&key, 256)?;
            let admin = core::str::from_utf8(&data).map_err(|_| ContractError::InvalidInput)?;
            let mut out = Vec::new();
            out.push(1u8);
            out.extend_from_slice(admin.as_bytes());
            Ok(out)
        }

        _ => Err(ContractError::InvalidInput),
    }
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

fn role_member_key(role: &str, account: &str) -> String {
    let mut k = String::from("access:role:");
    k.push_str(role);
    k.push(':');
    k.push_str(account);
    k
}

fn role_admin_key(role: &str) -> String {
    let mut k = String::from("access:admin:");
    k.push_str(role);
    k
}

// IO helpers
fn read_u8(input: &[u8], cursor: &mut usize) -> Result<u8, ContractError> {
    if *cursor >= input.len() { return Err(ContractError::InvalidInput); }
    let v = input[*cursor]; *cursor += 1; Ok(v)
}

fn read_u16(input: &[u8], cursor: &mut usize) -> Result<u16, ContractError> {
    if *cursor + 2 > input.len() { return Err(ContractError::InvalidInput); }
    let b = [input[*cursor], input[*cursor + 1]]; *cursor += 2; Ok(u16::from_le_bytes(b))
}

fn read_string(input: &[u8], cursor: &mut usize) -> Result<String, ContractError> {
    let len = read_u16(input, cursor)? as usize;
    if *cursor + len > input.len() { return Err(ContractError::InvalidInput); }
    let slice = &input[*cursor..*cursor + len]; *cursor += len;
    core::str::from_utf8(slice).map(|s| s.to_string()).map_err(|_| ContractError::InvalidInput)
}
