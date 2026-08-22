#![no_std]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::vec;

use spacekit_contract_sdk::{ContractError, ContractErrorCode, SpacekitContract};
use spacekit_contract_sdk::spacekit_contract;

#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

#[cfg(all(target_arch = "wasm32", not(test)))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { loop {} }

// ═══════════════════════════════════════════════════════════════════════════════
// Host imports
// ═══════════════════════════════════════════════════════════════════════════════

#[link(wasm_import_module = "spacekit_storage")]
extern "C" {
    fn storage_save(key_ptr: *const u8, key_len: usize, data_ptr: *const u8, data_len: usize) -> i32;
    fn storage_load(key_ptr: *const u8, key_len: usize, dest_ptr: *mut u8, max_len: usize) -> i32;
}

#[link(wasm_import_module = "env")]
extern "C" {
    fn get_caller_did(out_ptr: *mut u8, max_len: usize) -> i32;
}

#[link(wasm_import_module = "spacekit_crypto")]
extern "C" {
    fn sha256(data_ptr: *const u8, data_len: usize, out_ptr: *mut u8) -> i32;
}

// ═══════════════════════════════════════════════════════════════════════════════
// Opcodes
// ═══════════════════════════════════════════════════════════════════════════════

const OP_CREATE_VAULT: u8 = 1;
const OP_ADD_MEMBER: u8 = 2;
const OP_REMOVE_MEMBER: u8 = 3;
const OP_REQUEST_ACCESS: u8 = 4;
const OP_APPROVE_ACCESS: u8 = 5;
const OP_CHECK_ACCESS: u8 = 6;
const OP_RESOLVE_VAULT: u8 = 7;

const VAULT_VERSION: u8 = 1;
const MAX_MEMBERS: usize = 64;
const MAX_DID_LEN: usize = 128;

// ═══════════════════════════════════════════════════════════════════════════════
// Contract entry point
// ═══════════════════════════════════════════════════════════════════════════════

struct SharedVaultContract;

impl SpacekitContract for SharedVaultContract {
    type Error = ContractError;
    fn init() -> Self { SharedVaultContract }
    fn handle(&mut self, input: &[u8]) -> Result<Vec<u8>, ContractError> {
        dispatch(input)
    }
}

spacekit_contract!(SharedVaultContract);

fn dispatch(input: &[u8]) -> Result<Vec<u8>, ContractError> {
    if input.is_empty() {
        return Err(ContractError::InvalidInput);
    }
    match input[0] {
        OP_CREATE_VAULT => handle_create_vault(&input[1..]),
        OP_ADD_MEMBER => handle_add_member(&input[1..]),
        OP_REMOVE_MEMBER => handle_remove_member(&input[1..]),
        OP_REQUEST_ACCESS => handle_request_access(&input[1..]),
        OP_APPROVE_ACCESS => handle_approve_access(&input[1..]),
        OP_CHECK_ACCESS => handle_check_access(&input[1..]),
        OP_RESOLVE_VAULT => handle_resolve_vault(&input[1..]),
        _ => Err(ContractError::InvalidInput),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Data structures (binary-serialized for `no_std`)
// ═══════════════════════════════════════════════════════════════════════════════

/// Vault record stored under key `vault:{vault_id}`
struct Vault {
    owner_did: String,
    file_id: String,
    threshold: u8,
    members: Vec<String>,
    created_at: u64,
}

/// Access request stored under key `vault:{vault_id}:access:{requester_hash}`
struct AccessRequest {
    requester_did: String,
    approvals: Vec<String>,
    requested_at: u64,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Handlers
// ═══════════════════════════════════════════════════════════════════════════════

/// OP_CREATE_VAULT
/// Input: [file_id_len:u16le][file_id][threshold:u8][member_count:u8]
///        for each: [did_len:u16le][did bytes]
/// Output: [vault_id: 64 ascii hex bytes]
fn handle_create_vault(data: &[u8]) -> Result<Vec<u8>, ContractError> {
    let mut pos = 0usize;
    let file_id = read_string(data, &mut pos)?;
    if pos >= data.len() { return Err(ContractError::InvalidInput); }
    let threshold = data[pos]; pos += 1;
    if pos >= data.len() { return Err(ContractError::InvalidInput); }
    let member_count = data[pos] as usize; pos += 1;

    if member_count > MAX_MEMBERS { return Err(ContractError::InvalidInput); }
    if threshold == 0 || threshold as usize > member_count {
        return Err(ContractError::InvalidInput);
    }

    let mut members = Vec::with_capacity(member_count);
    for _ in 0..member_count {
        members.push(read_string(data, &mut pos)?);
    }

    let caller = get_caller()?;
    let vault_id = derive_vault_id(&caller, &file_id);

    let vault = Vault {
        owner_did: caller,
        file_id,
        threshold,
        members,
        created_at: 0,
    };

    let key = vault_storage_key(&vault_id);
    host_storage_save(&key, &encode_vault(&vault))?;

    Ok(vault_id.into_bytes())
}

/// OP_ADD_MEMBER
/// Input: [vault_id_len:u16le][vault_id][new_member_did_len:u16le][new_member_did]
fn handle_add_member(data: &[u8]) -> Result<Vec<u8>, ContractError> {
    let mut pos = 0usize;
    let vault_id = read_string(data, &mut pos)?;
    let new_member = read_string(data, &mut pos)?;

    let caller = get_caller()?;
    let mut vault = load_vault(&vault_id)?;

    if vault.owner_did != caller { return Err(ContractError::Failed); }
    if vault.members.len() >= MAX_MEMBERS { return Err(ContractError::InvalidInput); }
    if vault.members.iter().any(|m| m == &new_member) { return Err(ContractError::InvalidInput); }

    vault.members.push(new_member);
    host_storage_save(&vault_storage_key(&vault_id), &encode_vault(&vault))?;

    Ok(vec![1])
}

/// OP_REMOVE_MEMBER
/// Input: [vault_id_len:u16le][vault_id][member_did_len:u16le][member_did]
fn handle_remove_member(data: &[u8]) -> Result<Vec<u8>, ContractError> {
    let mut pos = 0usize;
    let vault_id = read_string(data, &mut pos)?;
    let member_did = read_string(data, &mut pos)?;

    let caller = get_caller()?;
    let mut vault = load_vault(&vault_id)?;

    if vault.owner_did != caller { return Err(ContractError::Failed); }
    if member_did == vault.owner_did { return Err(ContractError::InvalidInput); }

    vault.members.retain(|m| m != &member_did);
    host_storage_save(&vault_storage_key(&vault_id), &encode_vault(&vault))?;

    Ok(vec![1])
}

/// OP_REQUEST_ACCESS
/// Input: [vault_id_len:u16le][vault_id]
fn handle_request_access(data: &[u8]) -> Result<Vec<u8>, ContractError> {
    let mut pos = 0usize;
    let vault_id = read_string(data, &mut pos)?;

    let caller = get_caller()?;
    let vault = load_vault(&vault_id)?;

    if vault.members.iter().any(|m| m == &caller) || vault.owner_did == caller {
        return Ok(vec![2]); // already has access
    }

    let req_key = access_request_key(&vault_id, &caller);
    let request = AccessRequest {
        requester_did: caller,
        approvals: Vec::new(),
        requested_at: 0,
    };
    host_storage_save(&req_key, &encode_access_request(&request))?;

    Ok(vec![1])
}

/// OP_APPROVE_ACCESS
/// Input: [vault_id_len:u16le][vault_id][requester_did_len:u16le][requester_did]
fn handle_approve_access(data: &[u8]) -> Result<Vec<u8>, ContractError> {
    let mut pos = 0usize;
    let vault_id = read_string(data, &mut pos)?;
    let requester_did = read_string(data, &mut pos)?;

    let caller = get_caller()?;
    let vault = load_vault(&vault_id)?;

    if vault.owner_did != caller && !vault.members.iter().any(|m| m == &caller) {
        return Err(ContractError::Failed);
    }

    let req_key = access_request_key(&vault_id, &requester_did);
    let mut request = load_access_request(&req_key)?;

    if request.approvals.iter().any(|a| a == &caller) {
        return Ok(vec![0]); // already approved by this member
    }

    request.approvals.push(caller);
    host_storage_save(&req_key, &encode_access_request(&request))?;

    if request.approvals.len() >= vault.threshold as usize {
        Ok(vec![2]) // threshold met — access granted
    } else {
        Ok(vec![1]) // approval recorded, not yet at threshold
    }
}

/// OP_CHECK_ACCESS
/// Input: [vault_id_len:u16le][vault_id][did_len:u16le][did]
/// Output: [0 = no access, 1 = member/owner, 2 = approved via threshold]
fn handle_check_access(data: &[u8]) -> Result<Vec<u8>, ContractError> {
    let mut pos = 0usize;
    let vault_id = read_string(data, &mut pos)?;
    let did = read_string(data, &mut pos)?;

    let vault = load_vault(&vault_id)?;

    if vault.owner_did == did || vault.members.iter().any(|m| m == &did) {
        return Ok(vec![1]);
    }

    let req_key = access_request_key(&vault_id, &did);
    match load_access_request_optional(&req_key) {
        Some(req) if req.approvals.len() >= vault.threshold as usize => Ok(vec![2]),
        _ => Ok(vec![0]),
    }
}

/// OP_RESOLVE_VAULT
/// Input: [vault_id_len:u16le][vault_id]
/// Output: raw encoded vault bytes
fn handle_resolve_vault(data: &[u8]) -> Result<Vec<u8>, ContractError> {
    let mut pos = 0usize;
    let vault_id = read_string(data, &mut pos)?;
    let vault = load_vault(&vault_id)?;
    Ok(encode_vault(&vault))
}

// ═══════════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════════

fn get_caller() -> Result<String, ContractError> {
    let mut buf = [0u8; MAX_DID_LEN];
    let len = unsafe { get_caller_did(buf.as_mut_ptr(), buf.len()) };
    if len <= 0 { return Err(ContractError::Failed); }
    String::from_utf8(buf[..len as usize].to_vec())
        .map_err(|_| ContractError::InvalidInput)
}

fn derive_vault_id(owner_did: &str, file_id: &str) -> String {
    let mut input = Vec::with_capacity(owner_did.len() + file_id.len());
    input.extend_from_slice(owner_did.as_bytes());
    input.extend_from_slice(file_id.as_bytes());
    let hash = host_sha256(&input);
    hex_encode(&hash)
}

fn vault_storage_key(vault_id: &str) -> String {
    let mut k = String::from("vault:");
    k.push_str(vault_id);
    k
}

fn access_request_key(vault_id: &str, requester_did: &str) -> String {
    let did_hash = host_sha256(requester_did.as_bytes());
    let mut k = String::from("vault:");
    k.push_str(vault_id);
    k.push_str(":access:");
    k.push_str(&hex_encode(&did_hash[..8]));
    k
}

fn host_storage_save(key: &str, data: &[u8]) -> Result<(), ContractError> {
    let rc = unsafe { storage_save(key.as_ptr(), key.len(), data.as_ptr(), data.len()) };
    if rc < 0 { Err(ContractError::StorageError) } else { Ok(()) }
}

fn host_storage_load(key: &str, buf: &mut [u8]) -> i32 {
    unsafe { storage_load(key.as_ptr(), key.len(), buf.as_mut_ptr(), buf.len()) }
}

fn host_sha256(data: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    unsafe { sha256(data.as_ptr(), data.len(), out.as_mut_ptr()); }
    out
}

fn load_vault(vault_id: &str) -> Result<Vault, ContractError> {
    let key = vault_storage_key(vault_id);
    let mut buf = [0u8; 8192];
    let len = host_storage_load(&key, &mut buf);
    if len <= 0 { return Err(ContractError::HostError); }
    decode_vault(&buf[..len as usize])
}

fn load_access_request(key: &str) -> Result<AccessRequest, ContractError> {
    let mut buf = [0u8; 4096];
    let len = host_storage_load(key, &mut buf);
    if len <= 0 { return Err(ContractError::HostError); }
    decode_access_request(&buf[..len as usize])
}

fn load_access_request_optional(key: &str) -> Option<AccessRequest> {
    let mut buf = [0u8; 4096];
    let len = host_storage_load(key, &mut buf);
    if len <= 0 { return None; }
    decode_access_request(&buf[..len as usize]).ok()
}

// ═══════════════════════════════════════════════════════════════════════════════
// Binary encoding
// ═══════════════════════════════════════════════════════════════════════════════

fn encode_vault(v: &Vault) -> Vec<u8> {
    let mut out = Vec::with_capacity(256);
    out.push(VAULT_VERSION);
    write_string(&mut out, &v.owner_did);
    write_string(&mut out, &v.file_id);
    out.push(v.threshold);
    out.push(v.members.len() as u8);
    for m in &v.members {
        write_string(&mut out, m);
    }
    out.extend_from_slice(&v.created_at.to_le_bytes());
    out
}

fn decode_vault(data: &[u8]) -> Result<Vault, ContractError> {
    if data.is_empty() || data[0] != VAULT_VERSION {
        return Err(ContractError::InvalidInput);
    }
    let mut pos = 1usize;
    let owner_did = read_string(data, &mut pos)?;
    let file_id = read_string(data, &mut pos)?;
    if pos >= data.len() { return Err(ContractError::InvalidInput); }
    let threshold = data[pos]; pos += 1;
    if pos >= data.len() { return Err(ContractError::InvalidInput); }
    let member_count = data[pos] as usize; pos += 1;
    let mut members = Vec::with_capacity(member_count);
    for _ in 0..member_count {
        members.push(read_string(data, &mut pos)?);
    }
    let created_at = if pos + 8 <= data.len() {
        u64::from_le_bytes(data[pos..pos+8].try_into().unwrap_or([0u8; 8]))
    } else {
        0
    };
    Ok(Vault { owner_did, file_id, threshold, members, created_at })
}

fn encode_access_request(r: &AccessRequest) -> Vec<u8> {
    let mut out = Vec::with_capacity(128);
    write_string(&mut out, &r.requester_did);
    out.push(r.approvals.len() as u8);
    for a in &r.approvals {
        write_string(&mut out, a);
    }
    out.extend_from_slice(&r.requested_at.to_le_bytes());
    out
}

fn decode_access_request(data: &[u8]) -> Result<AccessRequest, ContractError> {
    let mut pos = 0usize;
    let requester_did = read_string(data, &mut pos)?;
    if pos >= data.len() { return Err(ContractError::InvalidInput); }
    let approval_count = data[pos] as usize; pos += 1;
    let mut approvals = Vec::with_capacity(approval_count);
    for _ in 0..approval_count {
        approvals.push(read_string(data, &mut pos)?);
    }
    let requested_at = if pos + 8 <= data.len() {
        u64::from_le_bytes(data[pos..pos+8].try_into().unwrap_or([0u8; 8]))
    } else {
        0
    };
    Ok(AccessRequest { requester_did, approvals, requested_at })
}

fn write_string(out: &mut Vec<u8>, s: &str) {
    let len = s.len() as u16;
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(s.as_bytes());
}

fn read_string(data: &[u8], pos: &mut usize) -> Result<String, ContractError> {
    if *pos + 2 > data.len() { return Err(ContractError::InvalidInput); }
    let len = u16::from_le_bytes([data[*pos], data[*pos + 1]]) as usize;
    *pos += 2;
    if *pos + len > data.len() || len > MAX_DID_LEN {
        return Err(ContractError::InvalidInput);
    }
    let s = core::str::from_utf8(&data[*pos..*pos + len])
        .map_err(|_| ContractError::InvalidInput)?;
    *pos += len;
    Ok(s.to_string())
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0xf) as usize] as char);
    }
    s
}
