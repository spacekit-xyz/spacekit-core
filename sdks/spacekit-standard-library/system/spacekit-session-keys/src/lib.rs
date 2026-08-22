//! Session-key management contract — ERC-4337–inspired delegated execution.
//!
//! Allows a DID owner to grant time-limited, scope-restricted authority to a
//! delegate DID (typically an agent).  Other contracts can call `OP_VALIDATE`
//! before executing privileged operations to verify the caller holds a valid
//! session.
//!
//! ## Wire format (little-endian `u16` length-prefixed strings)
//!
//! | Op | Opcode | Payload |
//! |----|--------|---------|
//! | CREATE  | `0x01` | `[delegate:str][scope:str][expires_at:u64le][spending_limit:u64le]` |
//! | VALIDATE| `0x02` | `[session_id:32][operation:str]` |
//! | REVOKE  | `0x03` | `[session_id:32]` |
//! | GET     | `0x04` | `[session_id:32]` |
//! | LIST    | `0x05` | `[owner_did:str]` |
//!
//! ## Storage layout
//!
//! - `sess:<hex(session_id)>` → session record bytes
//! - `idx:<owner_did>` → newline-separated hex session ids

#![no_std]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::vec;
use alloc::format;

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
    fn get_timestamp() -> i64;
}

#[link(wasm_import_module = "spacekit_crypto")]
extern "C" {
    fn sha256(data_ptr: *const u8, data_len: usize, out_ptr: *mut u8) -> i32;
}

// ═══════════════════════════════════════════════════════════════════════════════
// Opcodes
// ═══════════════════════════════════════════════════════════════════════════════

const OP_CREATE: u8   = 0x01;
const OP_VALIDATE: u8 = 0x02;
const OP_REVOKE: u8   = 0x03;
const OP_GET: u8      = 0x04;
const OP_LIST: u8     = 0x05;

const STATUS_ACTIVE: u8  = 1;
const STATUS_REVOKED: u8 = 0;

const MAX_DID_LEN: usize     = 128;
const MAX_STRING_LEN: usize  = 512;
const SESSION_RECORD_MAX: usize = 1024;
const INDEX_MAX: usize       = 8192;

// ═══════════════════════════════════════════════════════════════════════════════
// Contract
// ═══════════════════════════════════════════════════════════════════════════════

struct SessionKeysContract;

impl SpacekitContract for SessionKeysContract {
    type Error = ContractError;
    fn init() -> Self { SessionKeysContract }
    fn handle(&mut self, input: &[u8]) -> Result<Vec<u8>, ContractError> {
        if input.is_empty() { return Err(ContractError::InvalidInput); }
        match input[0] {
            OP_CREATE   => handle_create(&input[1..]),
            OP_VALIDATE => handle_validate(&input[1..]),
            OP_REVOKE   => handle_revoke(&input[1..]),
            OP_GET      => handle_get(&input[1..]),
            OP_LIST     => handle_list(&input[1..]),
            _ => Err(ContractError::InvalidInput),
        }
    }
}

spacekit_contract!(SessionKeysContract);

// ═══════════════════════════════════════════════════════════════════════════════
// Session record
// ═══════════════════════════════════════════════════════════════════════════════

struct Session {
    owner_did: String,
    delegate_did: String,
    scope: String,
    created_at: u64,
    expires_at: u64,
    spending_limit: u64,
    spent: u64,
    status: u8,
}

fn encode_session(s: &Session) -> Vec<u8> {
    let mut out = Vec::with_capacity(512);
    write_string(&mut out, &s.owner_did);
    write_string(&mut out, &s.delegate_did);
    write_string(&mut out, &s.scope);
    out.extend_from_slice(&s.created_at.to_le_bytes());
    out.extend_from_slice(&s.expires_at.to_le_bytes());
    out.extend_from_slice(&s.spending_limit.to_le_bytes());
    out.extend_from_slice(&s.spent.to_le_bytes());
    out.push(s.status);
    out
}

fn decode_session(data: &[u8]) -> Result<Session, ContractError> {
    let mut pos = 0usize;
    let owner_did    = read_string(data, &mut pos)?;
    let delegate_did = read_string(data, &mut pos)?;
    let scope        = read_string(data, &mut pos)?;
    let created_at   = read_u64(data, &mut pos)?;
    let expires_at   = read_u64(data, &mut pos)?;
    let spending_limit = read_u64(data, &mut pos)?;
    let spent        = read_u64(data, &mut pos)?;
    if pos >= data.len() { return Err(ContractError::InvalidInput); }
    let status = data[pos];
    Ok(Session { owner_did, delegate_did, scope, created_at, expires_at, spending_limit, spent, status })
}

// ═══════════════════════════════════════════════════════════════════════════════
// Handlers
// ═══════════════════════════════════════════════════════════════════════════════

/// CREATE — owner grants delegate a scoped session.
fn handle_create(data: &[u8]) -> Result<Vec<u8>, ContractError> {
    let mut pos = 0usize;
    let delegate_did   = read_string(data, &mut pos)?;
    let scope          = read_string(data, &mut pos)?;
    let expires_at     = read_u64(data, &mut pos)?;
    let spending_limit = read_u64(data, &mut pos)?;

    let owner = get_caller()?;
    let now = unsafe { get_timestamp() } as u64;

    if expires_at <= now {
        return Err(ContractError::InvalidInput);
    }

    let session_id = derive_session_id(&owner, &delegate_did, now);

    let session = Session {
        owner_did: owner.clone(),
        delegate_did,
        scope,
        created_at: now,
        expires_at,
        spending_limit,
        spent: 0,
        status: STATUS_ACTIVE,
    };

    let key = session_storage_key(&session_id);
    host_storage_save(&key, &encode_session(&session))?;

    append_to_index(&owner, &session_id)?;

    let mut event_data = Vec::with_capacity(64);
    event_data.extend_from_slice(&session_id);
    event_data.extend_from_slice(owner.as_bytes());
    spacekit_contract_sdk::emit_event_bytes("session:created", &event_data);

    let mut out = Vec::with_capacity(33);
    out.push(1u8);
    out.extend_from_slice(&session_id);
    Ok(out)
}

/// VALIDATE — check if current caller holds a valid session for an operation
/// under `owner_did`.  Returns `[1][status:u8]` where 1=valid, 0=invalid.
fn handle_validate(data: &[u8]) -> Result<Vec<u8>, ContractError> {
    let mut pos = 0usize;
    let session_id = read_bytes32(data, &mut pos)?;
    let operation  = read_string(data, &mut pos)?;

    let session = load_session(&session_id)?;

    if session.status != STATUS_ACTIVE {
        return Ok(vec![1u8, 0]);
    }

    let caller = get_caller()?;
    if session.delegate_did != caller {
        return Ok(vec![1u8, 0]);
    }

    let now = unsafe { get_timestamp() } as u64;
    if now > session.expires_at {
        return Ok(vec![1u8, 0]);
    }

    if !scope_allows(&session.scope, &operation) {
        return Ok(vec![1u8, 0]);
    }

    Ok(vec![1u8, 1])
}

/// REVOKE — only the session owner can revoke.
fn handle_revoke(data: &[u8]) -> Result<Vec<u8>, ContractError> {
    let mut pos = 0usize;
    let session_id = read_bytes32(data, &mut pos)?;

    let mut session = load_session(&session_id)?;
    let caller = get_caller()?;

    if session.owner_did != caller {
        return Err(ContractError::Unauthorized);
    }

    session.status = STATUS_REVOKED;
    let key = session_storage_key(&session_id);
    host_storage_save(&key, &encode_session(&session))?;

    spacekit_contract_sdk::emit_event_bytes("session:revoked", &session_id);

    Ok(vec![1u8])
}

/// GET — return raw session record.
fn handle_get(data: &[u8]) -> Result<Vec<u8>, ContractError> {
    let mut pos = 0usize;
    let session_id = read_bytes32(data, &mut pos)?;
    let raw = host_storage_load_raw(&session_storage_key(&session_id), SESSION_RECORD_MAX)?;
    let mut out = Vec::with_capacity(1 + raw.len());
    out.push(1u8);
    out.extend_from_slice(&raw);
    Ok(out)
}

/// LIST — return newline-separated hex session ids for an owner DID.
fn handle_list(data: &[u8]) -> Result<Vec<u8>, ContractError> {
    let mut pos = 0usize;
    let owner_did = read_string(data, &mut pos)?;
    let key = index_key(&owner_did);
    match host_storage_load_raw(&key, INDEX_MAX) {
        Ok(raw) => {
            let mut out = Vec::with_capacity(1 + raw.len());
            out.push(1u8);
            out.extend_from_slice(&raw);
            Ok(out)
        }
        Err(_) => Ok(vec![1u8]),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Scope matching
// ═══════════════════════════════════════════════════════════════════════════════

/// Wildcard `*` matches everything.  Pipe-separated scopes are checked
/// individually.  E.g. scope `"vault_charge|transfer"` allows operations
/// named `"vault_charge"` or `"transfer"`.
fn scope_allows(scope: &str, operation: &str) -> bool {
    if scope == "*" {
        return true;
    }
    for part in scope.split('|') {
        if part.trim() == operation {
            return true;
        }
    }
    false
}

fn split(s: &str, sep: char) -> impl Iterator<Item = &str> {
    s.split(sep)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════════

fn get_caller() -> Result<String, ContractError> {
    let mut buf = [0u8; MAX_DID_LEN];
    let len = unsafe { get_caller_did(buf.as_mut_ptr(), buf.len()) };
    if len <= 0 { return Err(ContractError::Unauthorized); }
    String::from_utf8(buf[..len as usize].to_vec())
        .map_err(|_| ContractError::InvalidInput)
}

fn derive_session_id(owner: &str, delegate: &str, timestamp: u64) -> [u8; 32] {
    let ts_bytes = timestamp.to_le_bytes();
    let mut input = Vec::with_capacity(owner.len() + delegate.len() + 8);
    input.extend_from_slice(owner.as_bytes());
    input.extend_from_slice(delegate.as_bytes());
    input.extend_from_slice(&ts_bytes);
    host_sha256(&input)
}

fn session_storage_key(id: &[u8; 32]) -> String {
    let mut k = String::from("sess:");
    k.push_str(&hex_encode(id));
    k
}

fn index_key(owner_did: &str) -> String {
    let mut k = String::from("idx:");
    k.push_str(owner_did);
    k
}

fn append_to_index(owner_did: &str, session_id: &[u8; 32]) -> Result<(), ContractError> {
    let key = index_key(owner_did);
    let hex = hex_encode(session_id);
    let new_entry = format!("{hex}\n");

    let existing = host_storage_load_raw(&key, INDEX_MAX).unwrap_or_default();
    let mut combined = existing;
    combined.extend_from_slice(new_entry.as_bytes());
    host_storage_save(&key, &combined)
}

fn load_session(id: &[u8; 32]) -> Result<Session, ContractError> {
    let key = session_storage_key(id);
    let raw = host_storage_load_raw(&key, SESSION_RECORD_MAX)?;
    decode_session(&raw)
}

fn host_storage_save(key: &str, data: &[u8]) -> Result<(), ContractError> {
    let rc = unsafe { storage_save(key.as_ptr(), key.len(), data.as_ptr(), data.len()) };
    if rc < 0 { Err(ContractError::StorageError) } else { Ok(()) }
}

fn host_storage_load_raw(key: &str, max_len: usize) -> Result<Vec<u8>, ContractError> {
    let mut buf = vec![0u8; max_len];
    let n = unsafe { storage_load(key.as_ptr(), key.len(), buf.as_mut_ptr(), buf.len()) };
    if n <= 0 { return Err(ContractError::StorageError); }
    buf.truncate(n as usize);
    Ok(buf)
}

fn host_sha256(data: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    unsafe { sha256(data.as_ptr(), data.len(), out.as_mut_ptr()); }
    out
}

// ═══════════════════════════════════════════════════════════════════════════════
// Wire format helpers
// ═══════════════════════════════════════════════════════════════════════════════

fn read_u16(input: &[u8], pos: &mut usize) -> Result<u16, ContractError> {
    if *pos + 2 > input.len() { return Err(ContractError::InvalidInput); }
    let b = [input[*pos], input[*pos + 1]]; *pos += 2;
    Ok(u16::from_le_bytes(b))
}

fn read_u64(input: &[u8], pos: &mut usize) -> Result<u64, ContractError> {
    if *pos + 8 > input.len() { return Err(ContractError::InvalidInput); }
    let mut b = [0u8; 8];
    b.copy_from_slice(&input[*pos..*pos + 8]);
    *pos += 8;
    Ok(u64::from_le_bytes(b))
}

fn read_string(input: &[u8], pos: &mut usize) -> Result<String, ContractError> {
    let len = read_u16(input, pos)? as usize;
    if *pos + len > input.len() || len > MAX_STRING_LEN {
        return Err(ContractError::InvalidInput);
    }
    let s = core::str::from_utf8(&input[*pos..*pos + len])
        .map_err(|_| ContractError::InvalidInput)?;
    *pos += len;
    Ok(s.to_string())
}

fn read_bytes32(input: &[u8], pos: &mut usize) -> Result<[u8; 32], ContractError> {
    if *pos + 32 > input.len() { return Err(ContractError::InvalidInput); }
    let mut out = [0u8; 32];
    out.copy_from_slice(&input[*pos..*pos + 32]);
    *pos += 32;
    Ok(out)
}

fn write_string(out: &mut Vec<u8>, s: &str) {
    let len = s.len() as u16;
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(s.as_bytes());
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
