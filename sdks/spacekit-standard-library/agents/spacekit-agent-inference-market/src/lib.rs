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

struct SpaceKitInferenceMarket;

// Opcodes
const OP_SET_AGENT_PRICE: u8   = 1;
const OP_GET_AGENT_PRICE: u8   = 2;
const OP_CREATE_JOB: u8        = 3;
const OP_SUBMIT_RESULT: u8     = 4;
const OP_GET_JOB: u8           = 5;

// Job status
const JOB_OPEN: u8      = 1;
const JOB_COMPLETED: u8 = 2;

impl SpacekitContract for SpaceKitInferenceMarket {
    type Error = ContractError;
    fn init() -> Self { SpaceKitInferenceMarket }
    fn handle(&mut self, input: &[u8]) -> Result<Vec<u8>, ContractError> { handle(input) }
}

spacekit_contract!(SpaceKitInferenceMarket);

fn handle(input: &[u8]) -> Result<Vec<u8>, ContractError> {
    let mut cursor = 0usize;
    let op = read_u8(input, &mut cursor)?;

    match op {
        // [op][agent_id:u64][token_contract:string][price_per_unit:u64]
        OP_SET_AGENT_PRICE => {
            let agent_id       = read_u64(input, &mut cursor)?;
            let token_contract = read_string(input, &mut cursor)?;
            let price          = read_u64(input, &mut cursor)?;

            let mut buf = Vec::new();
            write_string(&mut buf, &token_contract)?;
            buf.extend_from_slice(&price.to_le_bytes());
            storage_save_bytes(&agent_price_key(agent_id), &buf)?;
            Ok(vec![1u8])
        }

        // [op][agent_id:u64] -> [1][token_contract][price:u64]
        OP_GET_AGENT_PRICE => {
            let agent_id = read_u64(input, &mut cursor)?;
            let data = storage_load_bytes(&agent_price_key(agent_id), 256)?;
            let mut out = Vec::new();
            out.push(1u8);
            out.extend_from_slice(&data);
            Ok(out)
        }

        // [op][job_id:string][agent_id:u64][requester:string][input_uri:string][max_price:u64]
        OP_CREATE_JOB => {
            let job_id    = read_string(input, &mut cursor)?;
            let agent_id  = read_u64(input, &mut cursor)?;
            let requester = read_string(input, &mut cursor)?;
            let input_uri = read_string(input, &mut cursor)?;
            let max_price = read_u64(input, &mut cursor)?;

            let mut buf = Vec::new();
            buf.extend_from_slice(&agent_id.to_le_bytes());
            write_string(&mut buf, &requester)?;
            write_string(&mut buf, &input_uri)?;
            buf.extend_from_slice(&max_price.to_le_bytes());
            buf.push(JOB_OPEN);
            // result_uri omitted until completion

            storage_save_bytes(&job_key(&job_id), &buf)?;
            Ok(vec![1u8])
        }

        // [op][job_id:string][result_uri:string]
        OP_SUBMIT_RESULT => {
            let job_id     = read_string(input, &mut cursor)?;
            let result_uri = read_string(input, &mut cursor)?;

            let mut data = storage_load_bytes(&job_key(&job_id), 1024)?;
            if data.len() < 8 + 2 + 2 + 8 + 1 {
                return Err(ContractError::InvalidInput);
            }

            // append result_uri and set status
            write_string(&mut data, &result_uri)?;
            *data.last_mut().unwrap() = JOB_COMPLETED;
            storage_save_bytes(&job_key(&job_id), &data)?;
            Ok(vec![1u8])
        }

        // [op][job_id:string] -> [1][raw_job_bytes...]
        OP_GET_JOB => {
            let job_id = read_string(input, &mut cursor)?;
            let data = storage_load_bytes(&job_key(&job_id), 1024)?;
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

fn agent_price_key(agent_id: u64) -> String {
    let mut k = String::from("inf:price:");
    k.push_str(&agent_id.to_string());
    k
}

fn job_key(job_id: &str) -> String {
    let mut k = String::from("inf:job:");
    k.push_str(job_id);
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
