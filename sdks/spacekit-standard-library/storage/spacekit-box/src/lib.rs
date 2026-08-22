//! spacekit-box (Dropbox-style) demo contract.
//! Stores blobs using spacekit_storage.

#![no_std]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::vec;

use spacekit_contract_sdk::{ContractError, ContractErrorCode, SpacekitContract};
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

struct SpacekitBox;

const OP_PUT: u8 = 1;
const OP_GET: u8 = 2;
const OP_DELETE: u8 = 3;

impl SpacekitContract for SpacekitBox {
    type Error = ContractError;

    fn init() -> Self {
        SpacekitBox
    }

    fn handle(&mut self, input: &[u8]) -> Result<Vec<u8>, ContractError> {
        handle(input)
    }
}

spacekit_contract!(SpacekitBox);

fn handle(input: &[u8]) -> Result<Vec<u8>, ContractError> {
    let mut cursor = 0usize;
    let op = read_u8(input, &mut cursor)?;
    match op {
        OP_PUT => {
            let name = read_string(input, &mut cursor)?;
            let blob = read_bytes(input, &mut cursor)?;
            if name.is_empty() || blob.is_empty() {
                return Err(ContractError::InvalidInput);
            }
            set_blob(&name, &blob)?;
            emit_event_bytes("spacekit.box.put", name.as_bytes());
            Ok(vec![1u8])
        }
        OP_GET => {
            let name = read_string(input, &mut cursor)?;
            let blob = get_blob(&name)?;
            Ok(blob)
        }
        OP_DELETE => {
            let name = read_string(input, &mut cursor)?;
            if name.is_empty() {
                return Err(ContractError::InvalidInput);
            }
            delete_blob(&name)?;
            emit_event_bytes("spacekit.box.delete", name.as_bytes());
            Ok(vec![1u8])
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

fn set_blob(name: &str, data: &[u8]) -> Result<(), ContractError> {
    let data_key = format_data_key(name);
    let len_key = format_len_key(name);
    storage_save_bytes(&data_key, data)?;
    storage_save_bytes(&len_key, &(data.len() as u64).to_le_bytes())?;
    Ok(())
}

fn get_blob(name: &str) -> Result<Vec<u8>, ContractError> {
    let len_key = format_len_key(name);
    let len_bytes = storage_load_bytes(&len_key, 8)?;
    if len_bytes.len() < 8 {
        return Err(ContractError::StorageError);
    }
    let len = u64::from_le_bytes([
        len_bytes[0], len_bytes[1], len_bytes[2], len_bytes[3],
        len_bytes[4], len_bytes[5], len_bytes[6], len_bytes[7],
    ]) as usize;
    if len == 0 {
        return Err(ContractError::StorageError);
    }
    let data_key = format_data_key(name);
    storage_load_bytes(&data_key, len)
}

fn delete_blob(name: &str) -> Result<(), ContractError> {
    let data_key = format_data_key(name);
    let len_key = format_len_key(name);
    storage_save_bytes(&data_key, &[])?;
    storage_save_bytes(&len_key, &0u64.to_le_bytes())?;
    Ok(())
}

fn format_data_key(name: &str) -> String {
    let mut key = String::from("spacekitbox:file:");
    key.push_str(name);
    key
}

fn format_len_key(name: &str) -> String {
    let mut key = String::from("spacekitbox:file_len:");
    key.push_str(name);
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

fn read_u16(input: &[u8], cursor: &mut usize) -> Result<u16, ContractError> {
    if *cursor + 2 > input.len() {
        return Err(ContractError::InvalidInput);
    }
    let bytes = [input[*cursor], input[*cursor + 1]];
    *cursor += 2;
    Ok(u16::from_le_bytes(bytes))
}

fn read_u32(input: &[u8], cursor: &mut usize) -> Result<u32, ContractError> {
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

fn read_bytes(input: &[u8], cursor: &mut usize) -> Result<Vec<u8>, ContractError> {
    let len = read_u32(input, cursor)? as usize;
    if *cursor + len > input.len() {
        return Err(ContractError::InvalidInput);
    }
    let slice = &input[*cursor..*cursor + len];
    *cursor += len;
    Ok(slice.to_vec())
}
