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

struct SpaceKitInferenceMesh;

const OP_REGISTER_DONOR: u8 = 1;
const OP_GET_DONOR: u8      = 2;
const OP_CREATE_JOB: u8     = 3;
const OP_ASSIGN_JOB: u8     = 4;
const OP_SET_JOB_RESULT: u8 = 5;
const OP_GET_JOB: u8        = 6;

impl SpacekitContract for SpaceKitInferenceMesh {
    type Error = ContractError;

    fn init() -> Self { SpaceKitInferenceMesh }

    fn handle(&mut self, input: &[u8]) -> Result<Vec<u8>, ContractError> {
        handle(input)
    }
}

spacekit_contract!(SpaceKitInferenceMesh);

fn handle(input: &[u8]) -> Result<Vec<u8>, ContractError> {
    let mut cursor = 0usize;
    let op = read_u8(input, &mut cursor)?;

    match op {
        // REGISTER_DONOR
        OP_REGISTER_DONOR => {
            let did = read_string(input, &mut cursor)?;
            let gpu = read_u8(input, &mut cursor)?;
            let max_batch = read_u8(input, &mut cursor)?;
            let model_ids_csv = read_string(input, &mut cursor)?;

            let mut buf = Vec::new();
            buf.push(gpu);
            buf.push(max_batch);
            write_string(&mut buf, &model_ids_csv)?;

            storage_save_bytes(&donor_key(&did), &buf)?;
            Ok(vec![1u8])
        }

        // GET_DONOR
        OP_GET_DONOR => {
            let did = read_string(input, &mut cursor)?;
            let data = match storage_load_bytes(&donor_key(&did), 1024) {
                Ok(d) => d,
                Err(_) => return Ok(vec![0u8]), // not found
            };

            let mut out = Vec::new();
            out.push(1u8); // status
            // data layout: [gpu:u8][max_batch:u8][model_ids_csv:string]
            let mut dc = 0usize;
            let gpu = data[dc]; dc += 1;
            let max_batch = data[dc]; dc += 1;

            out.push(gpu);
            out.push(max_batch);
            out.extend_from_slice(&data[dc..]);
            Ok(out)
        }

        // CREATE_JOB
        OP_CREATE_JOB => {
            let job_id = read_string(input, &mut cursor)?;
            let borrower_did = read_string(input, &mut cursor)?;
            let model_id = read_string(input, &mut cursor)?;
            let input_uri = read_string(input, &mut cursor)?;
            let max_price = read_u64(input, &mut cursor)?;

            let mut buf = Vec::new();
            write_string(&mut buf, &borrower_did)?;
            write_string(&mut buf, &model_id)?;
            write_string(&mut buf, &input_uri)?;
            buf.extend_from_slice(&max_price.to_le_bytes());
            buf.push(1u8); // job_status = created
            write_string(&mut buf, ""); // donor_did empty
            write_string(&mut buf, ""); // result_uri empty

            storage_save_bytes(&job_key(&job_id), &buf)?;
            Ok(vec![1u8])
        }

        // ASSIGN_JOB
        OP_ASSIGN_JOB => {
            let job_id = read_string(input, &mut cursor)?;
            let donor_did = read_string(input, &mut cursor)?;

            let mut data = storage_load_bytes(&job_key(&job_id), 2048)?;
            let mut jc = 0usize;

            let _borrower = read_string_from(&data, &mut jc)?;
            let _model_id = read_string_from(&data, &mut jc)?;
            let _input_uri = read_string_from(&data, &mut jc)?;
            let _max_price = read_u64_from(&data, &mut jc)?;
            // overwrite job_status
            if jc >= data.len() { return Err(ContractError::InvalidInput); }
            data[jc] = 2u8; // assigned
            jc += 1;

            // overwrite donor_did + result_uri
            let _old_donor = read_string_from(&data, &mut jc)?;
            let _old_result = read_string_from(&data, &mut jc)?;

            let mut new_buf = Vec::new();
            let mut tmpc = 0usize;
            let borrower = read_string_from(&data, &mut tmpc)?;
            let model_id = read_string_from(&data, &mut tmpc)?;
            let input_uri = read_string_from(&data, &mut tmpc)?;
            let max_price = read_u64_from(&data, &mut tmpc)?;
            let job_status = data[tmpc]; tmpc += 1;

            write_string(&mut new_buf, &borrower)?;
            write_string(&mut new_buf, &model_id)?;
            write_string(&mut new_buf, &input_uri)?;
            new_buf.extend_from_slice(&max_price.to_le_bytes());
            new_buf.push(job_status);
            write_string(&mut new_buf, &donor_did)?;
            write_string(&mut new_buf, "")?;

            storage_save_bytes(&job_key(&job_id), &new_buf)?;
            Ok(vec![1u8])
        }

        // SET_JOB_RESULT
        OP_SET_JOB_RESULT => {
            let job_id = read_string(input, &mut cursor)?;
            let success = read_u8(input, &mut cursor)?;
            let result_uri = read_string(input, &mut cursor)?;

            let mut data = storage_load_bytes(&job_key(&job_id), 2048)?;
            let mut jc = 0usize;

            let borrower = read_string_from(&data, &mut jc)?;
            let model_id = read_string_from(&data, &mut jc)?;
            let input_uri = read_string_from(&data, &mut jc)?;
            let max_price = read_u64_from(&data, &mut jc)?;
            let _old_status = data[jc]; jc += 1;
            let donor_did = read_string_from(&data, &mut jc)?;
            let _old_result = read_string_from(&data, &mut jc)?;

            let new_status = if success == 1 { 3u8 } else { 4u8 };

            let mut new_buf = Vec::new();
            write_string(&mut new_buf, &borrower)?;
            write_string(&mut new_buf, &model_id)?;
            write_string(&mut new_buf, &input_uri)?;
            new_buf.extend_from_slice(&max_price.to_le_bytes());
            new_buf.push(new_status);
            write_string(&mut new_buf, &donor_did)?;
            write_string(&mut new_buf, &result_uri)?;

            storage_save_bytes(&job_key(&job_id), &new_buf)?;
            Ok(vec![1u8])
        }

        // GET_JOB
        OP_GET_JOB => {
            let job_id = read_string(input, &mut cursor)?;
            let data = match storage_load_bytes(&job_key(&job_id), 2048) {
                Ok(d) => d,
                Err(_) => return Ok(vec![0u8]),
            };

            let mut jc = 0usize;
            let borrower = read_string_from(&data, &mut jc)?;
            let model_id = read_string_from(&data, &mut jc)?;
            let input_uri = read_string_from(&data, &mut jc)?;
            let max_price = read_u64_from(&data, &mut jc)?;
            let job_status = data[jc]; jc += 1;
            let donor_did = read_string_from(&data, &mut jc)?;
            let result_uri = read_string_from(&data, &mut jc)?;

            let mut out = Vec::new();
            out.push(1u8); // status
            write_string(&mut out, &borrower)?;
            write_string(&mut out, &model_id)?;
            write_string(&mut out, &input_uri)?;
            out.extend_from_slice(&max_price.to_le_bytes());
            out.push(job_status);
            write_string(&mut out, &donor_did)?;
            write_string(&mut out, &result_uri)?;
            Ok(out)
        }

        _ => Err(ContractError::InvalidInput),
    }
}

fn donor_key(did: &str) -> String {
    let mut k = String::from("mesh:donor:");
    k.push_str(did);
    k
}

fn job_key(job_id: &str) -> String {
    let mut k = String::from("mesh:job:");
    k.push_str(job_id);
    k
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

// helpers for parsing from an in-memory buffer
fn read_string_from(buf: &[u8], cursor: &mut usize) -> Result<String, ContractError> {
    if *cursor + 2 > buf.len() { return Err(ContractError::InvalidInput); }
    let len = u16::from_le_bytes([buf[*cursor], buf[*cursor + 1]]) as usize;
    *cursor += 2;
    if *cursor + len > buf.len() { return Err(ContractError::InvalidInput); }
    let slice = &buf[*cursor..*cursor + len];
    *cursor += len;
    core::str::from_utf8(slice).map(|s| s.to_string()).map_err(|_| ContractError::InvalidInput)
}

fn read_u64_from(buf: &[u8], cursor: &mut usize) -> Result<u64, ContractError> {
    if *cursor + 8 > buf.len() { return Err(ContractError::InvalidInput); }
    let b = [
        buf[*cursor], buf[*cursor + 1], buf[*cursor + 2], buf[*cursor + 3],
        buf[*cursor + 4], buf[*cursor + 5], buf[*cursor + 6], buf[*cursor + 7],
    ];
    *cursor += 8;
    Ok(u64::from_le_bytes(b))
}

fn write_u16(out: &mut Vec<u8>, v: u16) { out.extend_from_slice(&v.to_le_bytes()); }

fn write_string(out: &mut Vec<u8>, s: &str) -> Result<(), ContractError> {
    let len = s.len();
    if len > u16::MAX as usize { return Err(ContractError::InvalidInput); }
    write_u16(out, len as u16);
    out.extend_from_slice(s.as_bytes());
    Ok(())
}
