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

// This router is "logical": it records splits and routes; actual token transfers
// are done by the caller using ERC-20 contracts, but you can extend later.
/// ABI:
/// OP_SET_SPLIT (1) – define a split for a route_id
/// OP_GET_SPLIT (2) – read split config; caller uses it to route ERC‑20 transfers
struct SpaceKitPaymentRouter;

const OP_SET_SPLIT: u8 = 1;
const OP_GET_SPLIT: u8 = 2;

impl SpacekitContract for SpaceKitPaymentRouter {
    type Error = ContractError;
    fn init() -> Self { SpaceKitPaymentRouter }
    fn handle(&mut self, input: &[u8]) -> Result<Vec<u8>, ContractError> { handle(input) }
}

spacekit_contract!(SpaceKitPaymentRouter);

fn handle(input: &[u8]) -> Result<Vec<u8>, ContractError> {
    let mut cursor = 0usize;
    let op = read_u8(input, &mut cursor)?;

    match op {
        // [op][route_id:string][n:u8][account_1:string][bps_1:u16]...[account_n][bps_n]
        OP_SET_SPLIT => {
            let route_id = read_string(input, &mut cursor)?;
            let n = read_u8(input, &mut cursor)? as usize;
            if n == 0 { return Err(ContractError::InvalidInput); }

            let mut buf = Vec::new();
            buf.push(n as u8);
            for _ in 0..n {
                let acc = read_string(input, &mut cursor)?;
                let bps = read_u16(input, &mut cursor)?;
                write_string(&mut buf, &acc)?;
                buf.extend_from_slice(&bps.to_le_bytes());
            }

            storage_save_bytes(&route_key(&route_id), &buf)?;
            Ok(vec![1u8])
        }

        // [op][route_id:string] -> [1][n:u8][account_1][bps_1]...
        OP_GET_SPLIT => {
            let route_id = read_string(input, &mut cursor)?;
            let data = storage_load_bytes(&route_key(&route_id), 1024)?;
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

fn route_key(route_id: &str) -> String {
    let mut k = String::from("pay:route:");
    k.push_str(route_id);
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

fn write_u16(out: &mut Vec<u8>, v: u16) { out.extend_from_slice(&v.to_le_bytes()); }

fn write_string(out: &mut Vec<u8>, s: &str) -> Result<(), ContractError> {
    let len = s.len();
    if len > u16::MAX as usize { return Err(ContractError::InvalidInput); }
    write_u16(out, len as u16);
    out.extend_from_slice(s.as_bytes());
    Ok(())
}
