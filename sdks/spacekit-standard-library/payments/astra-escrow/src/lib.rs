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
/// OP_CREATE (1) - define escrow terms
/// OP_RELEASE (2) - mark released
/// OP_REFUND (3) - mark refunded
/// OP_GET (4) - read full escrow record
struct SpaceKitEscrow;

// Opcodes
const OP_CREATE: u8   = 1;
const OP_RELEASE: u8  = 2;
const OP_REFUND: u8   = 3;
const OP_GET: u8      = 4;

// Status
const STATUS_OPEN: u8    = 1;
const STATUS_RELEASED: u8= 2;
const STATUS_REFUNDED: u8= 3;

impl SpacekitContract for SpaceKitEscrow {
    type Error = ContractError;
    fn init() -> Self { SpaceKitEscrow }
    fn handle(&mut self, input: &[u8]) -> Result<Vec<u8>, ContractError> { handle(input) }
}

spacekit_contract!(SpaceKitEscrow);

fn handle(input: &[u8]) -> Result<Vec<u8>, ContractError> {
    let mut cursor = 0usize;
    let op = read_u8(input, &mut cursor)?;

    match op {
        // [op][escrow_id:string][token_contract:string][payer:string][payee:string][amount:u64][arbiter:string]
        OP_CREATE => {
            let escrow_id      = read_string(input, &mut cursor)?;
            let token_contract = read_string(input, &mut cursor)?;
            let payer          = read_string(input, &mut cursor)?;
            let payee          = read_string(input, &mut cursor)?;
            let amount         = read_u64(input, &mut cursor)?;
            let arbiter        = read_string(input, &mut cursor)?;

            let mut buf = Vec::new();
            write_string(&mut buf, &token_contract)?;
            write_string(&mut buf, &payer)?;
            write_string(&mut buf, &payee)?;
            buf.extend_from_slice(&amount.to_le_bytes());
            write_string(&mut buf, &arbiter)?;
            buf.push(STATUS_OPEN);

            storage_save_bytes(&escrow_key(&escrow_id), &buf)?;
            Ok(vec![1u8])
        }

        // [op][escrow_id:string] -> mark RELEASED
        OP_RELEASE => {
            let escrow_id = read_string(input, &mut cursor)?;
            let mut data = storage_load_bytes(&escrow_key(&escrow_id), 1024)?;
            if data.is_empty() { return Err(ContractError::InvalidInput); }
            *data.last_mut().unwrap() = STATUS_RELEASED;
            storage_save_bytes(&escrow_key(&escrow_id), &data)?;
            Ok(vec![1u8])
        }

        // [op][escrow_id:string] -> mark REFUNDED
        OP_REFUND => {
            let escrow_id = read_string(input, &mut cursor)?;
            let mut data = storage_load_bytes(&escrow_key(&escrow_id), 1024)?;
            if data.is_empty() { return Err(ContractError::InvalidInput); }
            *data.last_mut().unwrap() = STATUS_REFUNDED;
            storage_save_bytes(&escrow_key(&escrow_id), &data)?;
            Ok(vec![1u8])
        }

        // [op][escrow_id:string] -> [1][token_contract][payer][payee][amount:u64][arbiter][status:u8]
        OP_GET => {
            let escrow_id = read_string(input, &mut cursor)?;
            let data = storage_load_bytes(&escrow_key(&escrow_id), 1024)?;
            let mut out = Vec::new();
            out.push(1u8);
            out.extend_from_slice(&data);
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

fn escrow_key(id: &str) -> String {
    let mut k = String::from("escrow:");
    k.push_str(id);
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

fn write_u16(out: &mut Vec<u8>, v: u16) { out.extend_from_slice(&v.to_le_bytes()); }

fn write_string(out: &mut Vec<u8>, s: &str) -> Result<(), ContractError> {
    let len = s.len();
    if len > u16::MAX as usize { return Err(ContractError::InvalidInput); }
    write_u16(out, len as u16);
    out.extend_from_slice(s.as_bytes());
    Ok(())
}
