#![no_std]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::vec;

use spacekit::{ContractError, SpacekitContract, ContractErrorCode};
use spacekit::spacekit_contract;

#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

#[cfg(all(target_arch = "wasm32", not(test)))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { loop {} }

#[link(wasm_import_module = "spacekit_storage")]
unsafe extern "C" {
    fn storage_save(key_ptr: *const u8, key_len: usize, data_ptr: *const u8, data_len: usize) -> i32;
    fn storage_load(key_ptr: *const u8, key_len: usize, dest_ptr: *mut u8, max_len: usize) -> usize;
}

struct SpaceKitReputation;

// Opcodes
const OP_SUBMIT: u8 = 1;
const OP_GET: u8    = 2;

impl SpacekitContract for SpaceKitReputation {
    type Error = ContractError;
    fn init() -> Self { SpaceKitReputation }
    fn handle(&mut self, input: &[u8]) -> Result<Vec<u8>, ContractError> { handle(input) }
}

spacekit_contract!(SpaceKitReputation);

fn handle(input: &[u8]) -> Result<Vec<u8>, ContractError> {
    let mut cursor = 0usize;
    let op = read_u8(input, &mut cursor)?;

    match op {
        // [op][subject_did:string][score:u8]
        OP_SUBMIT => {
            let subject = read_string(input, &mut cursor)?;
            let score   = read_u8(input, &mut cursor)?;

            let key = rep_key(&subject);
            let current = storage_load_bytes(&key, 16).ok();
            let (mut sum, mut count) = if let Some(data) = current {
                if data.len() == 16 {
                    let s = u64::from_le_bytes(data[0..8].try_into().unwrap());
                    let c = u64::from_le_bytes(data[8..16].try_into().unwrap());
                    (s, c)
                } else { (0, 0) }
            } else { (0, 0) };

            sum = sum.saturating_add(score as u64);
            count = count.saturating_add(1);

            let mut buf = Vec::with_capacity(16);
            buf.extend_from_slice(&sum.to_le_bytes());
            buf.extend_from_slice(&count.to_le_bytes());
            storage_save_bytes(&key, &buf)?;
            Ok(vec![1u8])
        }

        // [op][subject_did:string] -> [1][avg:u64][count:u64]
        OP_GET => {
            let subject = read_string(input, &mut cursor)?;
            let key = rep_key(&subject);
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

fn rep_key(subject: &str) -> String {
    let mut k = String::from("rep:global:");
    k.push_str(subject);
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
