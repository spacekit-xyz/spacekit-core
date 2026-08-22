//! Paymaster contract — ERC-4337–inspired sponsored execution for SpaceKit.
//!
//! Complements x402 (HTTP 402 USDC payment) by enabling **gasless on-chain
//! operations** for end-users.  A **sponsor** deposits vault credit and defines
//! a policy controlling who can be sponsored, for which operations, and up to
//! what limits.  When an agent operates on behalf of a user whose API access
//! was already settled via x402, the paymaster covers the on-chain vault charge.
//!
//! ## Wire format (little-endian `u16` length-prefixed strings)
//!
//! | Op | Opcode | Payload |
//! |----|--------|---------|
//! | DEPOSIT       | `0x01` | (empty — uses `msg_value` for deposit amount) |
//! | WITHDRAW      | `0x02` | `[amount:u64le]` |
//! | SET_POLICY    | `0x03` | `[policy_json:str]` |
//! | SPONSOR_CHARGE| `0x04` | `[beneficiary:str][amount:u64le][operation:str]` |
//! | GET_BUDGET    | `0x05` | `[sponsor_did:str]` |
//! | GET_POLICY    | `0x06` | `[sponsor_did:str]` |
//!
//! ## Storage layout
//!
//! - `bal:<sponsor_did>` → u64le balance
//! - `pol:<sponsor_did>` → policy JSON bytes
//! - `spent:<sponsor_did>:<day>` → u64le daily spend

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
    fn msg_value() -> i64;
    fn get_timestamp() -> i64;
}

// ═══════════════════════════════════════════════════════════════════════════════
// Opcodes
// ═══════════════════════════════════════════════════════════════════════════════

const OP_DEPOSIT: u8        = 0x01;
const OP_WITHDRAW: u8       = 0x02;
const OP_SET_POLICY: u8     = 0x03;
const OP_SPONSOR_CHARGE: u8 = 0x04;
const OP_GET_BUDGET: u8     = 0x05;
const OP_GET_POLICY: u8     = 0x06;

const MAX_DID_LEN: usize    = 128;
const MAX_STRING_LEN: usize = 512;
const POLICY_MAX: usize     = 4096;
const SECONDS_PER_DAY: u64  = 86_400;

// ═══════════════════════════════════════════════════════════════════════════════
// Contract
// ═══════════════════════════════════════════════════════════════════════════════

struct PaymasterContract;

impl SpacekitContract for PaymasterContract {
    type Error = ContractError;
    fn init() -> Self { PaymasterContract }
    fn handle(&mut self, input: &[u8]) -> Result<Vec<u8>, ContractError> {
        if input.is_empty() { return Err(ContractError::InvalidInput); }
        match input[0] {
            OP_DEPOSIT        => handle_deposit(),
            OP_WITHDRAW       => handle_withdraw(&input[1..]),
            OP_SET_POLICY     => handle_set_policy(&input[1..]),
            OP_SPONSOR_CHARGE => handle_sponsor_charge(&input[1..]),
            OP_GET_BUDGET     => handle_get_budget(&input[1..]),
            OP_GET_POLICY     => handle_get_policy(&input[1..]),
            _ => Err(ContractError::InvalidInput),
        }
    }
}

spacekit_contract!(PaymasterContract);

// ═══════════════════════════════════════════════════════════════════════════════
// Policy (lightweight JSON subset — parsed with minimal no_std helpers)
// ═══════════════════════════════════════════════════════════════════════════════

/// Minimal policy validated by `check_policy`.  Full JSON is stored verbatim;
/// we only parse the fields we need for enforcement.
struct Policy {
    per_call_max: u64,
    daily_max: u64,
    expires_at: u64,
    allowed_ops_raw: String,
    allowed_dids_raw: String,
}

fn parse_u64_field(json: &str, field: &str) -> u64 {
    if let Some(idx) = json.find(field) {
        let rest = &json[idx + field.len()..];
        if let Some(colon) = rest.find(':') {
            let after_colon = rest[colon + 1..].trim_start();
            let after_colon = after_colon.trim_start_matches('"');
            let end = after_colon.find(|c: char| !c.is_ascii_digit()).unwrap_or(after_colon.len());
            if end > 0 {
                if let Ok(v) = u64::from_str_radix(&after_colon[..end], 10) {
                    return v;
                }
            }
        }
    }
    0
}

fn parse_string_field(json: &str, field: &str) -> String {
    if let Some(idx) = json.find(field) {
        let rest = &json[idx + field.len()..];
        if let Some(bracket) = rest.find('[') {
            let end = rest[bracket..].find(']').unwrap_or(rest.len() - bracket);
            return rest[bracket..bracket + end + 1].to_string();
        }
    }
    String::new()
}

fn parse_policy(json: &str) -> Policy {
    Policy {
        per_call_max: parse_u64_field(json, "per_call_max"),
        daily_max: parse_u64_field(json, "daily_max"),
        expires_at: parse_u64_field(json, "expires_at"),
        allowed_ops_raw: parse_string_field(json, "allowed_ops"),
        allowed_dids_raw: parse_string_field(json, "allowed_dids"),
    }
}

fn policy_allows_did(allowed_raw: &str, did: &str) -> bool {
    if allowed_raw.is_empty() || allowed_raw.contains("\"*\"") {
        return true;
    }
    allowed_raw.contains(did)
}

fn policy_allows_op(allowed_raw: &str, op: &str) -> bool {
    if allowed_raw.is_empty() || allowed_raw.contains("\"*\"") {
        return true;
    }
    allowed_raw.contains(op)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Handlers
// ═══════════════════════════════════════════════════════════════════════════════

/// DEPOSIT — sponsor adds vault credit.  Amount comes from `msg_value`.
fn handle_deposit() -> Result<Vec<u8>, ContractError> {
    let sponsor = get_caller()?;
    let deposit = unsafe { msg_value() } as u64;
    if deposit == 0 {
        return Err(ContractError::InvalidInput);
    }

    let current = load_balance(&sponsor);
    let new_balance = current.checked_add(deposit).ok_or(ContractError::Failed)?;
    save_balance(&sponsor, new_balance)?;

    let mut event = Vec::with_capacity(8 + sponsor.len());
    event.extend_from_slice(&new_balance.to_le_bytes());
    event.extend_from_slice(sponsor.as_bytes());
    spacekit_contract_sdk::emit_event_bytes("paymaster:deposit", &event);

    let mut out = Vec::with_capacity(9);
    out.push(1u8);
    out.extend_from_slice(&new_balance.to_le_bytes());
    Ok(out)
}

/// WITHDRAW — sponsor reclaims unused balance.
fn handle_withdraw(data: &[u8]) -> Result<Vec<u8>, ContractError> {
    let mut pos = 0usize;
    let amount = read_u64(data, &mut pos)?;

    let sponsor = get_caller()?;
    let current = load_balance(&sponsor);
    if amount > current {
        return Err(ContractError::InsufficientBalance);
    }

    save_balance(&sponsor, current - amount)?;

    let mut event = Vec::with_capacity(16);
    event.extend_from_slice(&amount.to_le_bytes());
    event.extend_from_slice(&(current - amount).to_le_bytes());
    spacekit_contract_sdk::emit_event_bytes("paymaster:withdraw", &event);

    Ok(vec![1u8])
}

/// SET_POLICY — sponsor defines who/what they will sponsor.
fn handle_set_policy(data: &[u8]) -> Result<Vec<u8>, ContractError> {
    let mut pos = 0usize;
    let policy_json = read_string(data, &mut pos)?;

    let sponsor = get_caller()?;
    let key = policy_key(&sponsor);
    host_storage_save(&key, policy_json.as_bytes())?;

    spacekit_contract_sdk::emit_event_bytes("paymaster:policy_set", sponsor.as_bytes());

    Ok(vec![1u8])
}

/// SPONSOR_CHARGE — charge the sponsor for an operation on behalf of a
/// beneficiary.  Validates policy, per-call limit, daily limit, expiry,
/// and allowed DIDs/operations.
fn handle_sponsor_charge(data: &[u8]) -> Result<Vec<u8>, ContractError> {
    let mut pos = 0usize;
    let sponsor_did = read_string(data, &mut pos)?;
    let amount      = read_u64(data, &mut pos)?;
    let operation   = read_string(data, &mut pos)?;

    let beneficiary = get_caller()?;
    let now = unsafe { get_timestamp() } as u64;

    let policy_bytes = host_storage_load_raw(&policy_key(&sponsor_did), POLICY_MAX)?;
    let policy_str = core::str::from_utf8(&policy_bytes)
        .map_err(|_| ContractError::InvalidInput)?;
    let policy = parse_policy(policy_str);

    if policy.expires_at > 0 && now > policy.expires_at {
        return Err(ContractError::Unauthorized);
    }
    if !policy_allows_did(&policy.allowed_dids_raw, &beneficiary) {
        return Err(ContractError::Unauthorized);
    }
    if !policy_allows_op(&policy.allowed_ops_raw, &operation) {
        return Err(ContractError::Unauthorized);
    }
    if policy.per_call_max > 0 && amount > policy.per_call_max {
        return Err(ContractError::InsufficientPayment);
    }

    let day = now / SECONDS_PER_DAY;
    let daily_key = daily_spend_key(&sponsor_did, day);
    let daily_spent = load_u64_or_zero(&daily_key);
    if policy.daily_max > 0 && daily_spent + amount > policy.daily_max {
        return Err(ContractError::InsufficientBalance);
    }

    let balance = load_balance(&sponsor_did);
    if amount > balance {
        return Err(ContractError::InsufficientBalance);
    }

    save_balance(&sponsor_did, balance - amount)?;
    save_u64(&daily_key, daily_spent + amount)?;

    let mut event = Vec::with_capacity(8 + sponsor_did.len() + beneficiary.len() + operation.len());
    event.extend_from_slice(&amount.to_le_bytes());
    event.extend_from_slice(sponsor_did.as_bytes());
    event.push(0);
    event.extend_from_slice(beneficiary.as_bytes());
    event.push(0);
    event.extend_from_slice(operation.as_bytes());
    spacekit_contract_sdk::emit_event_bytes("paymaster:sponsored", &event);

    Ok(vec![1u8])
}

/// GET_BUDGET — return remaining balance as u64le.
fn handle_get_budget(data: &[u8]) -> Result<Vec<u8>, ContractError> {
    let mut pos = 0usize;
    let sponsor_did = read_string(data, &mut pos)?;
    let balance = load_balance(&sponsor_did);
    let mut out = Vec::with_capacity(9);
    out.push(1u8);
    out.extend_from_slice(&balance.to_le_bytes());
    Ok(out)
}

/// GET_POLICY — return raw policy JSON for a sponsor.
fn handle_get_policy(data: &[u8]) -> Result<Vec<u8>, ContractError> {
    let mut pos = 0usize;
    let sponsor_did = read_string(data, &mut pos)?;
    let raw = host_storage_load_raw(&policy_key(&sponsor_did), POLICY_MAX)?;
    let mut out = Vec::with_capacity(1 + raw.len());
    out.push(1u8);
    out.extend_from_slice(&raw);
    Ok(out)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Storage helpers
// ═══════════════════════════════════════════════════════════════════════════════

fn balance_key(did: &str) -> String {
    let mut k = String::from("bal:");
    k.push_str(did);
    k
}

fn policy_key(did: &str) -> String {
    let mut k = String::from("pol:");
    k.push_str(did);
    k
}

fn daily_spend_key(did: &str, day: u64) -> String {
    format!("spent:{did}:{day}")
}

fn load_balance(did: &str) -> u64 {
    load_u64_or_zero(&balance_key(did))
}

fn save_balance(did: &str, amount: u64) -> Result<(), ContractError> {
    save_u64(&balance_key(did), amount)
}

fn load_u64_or_zero(key: &str) -> u64 {
    match host_storage_load_raw(key, 8) {
        Ok(raw) if raw.len() == 8 => {
            let mut b = [0u8; 8];
            b.copy_from_slice(&raw);
            u64::from_le_bytes(b)
        }
        _ => 0,
    }
}

fn save_u64(key: &str, value: u64) -> Result<(), ContractError> {
    host_storage_save(key, &value.to_le_bytes())
}

fn get_caller() -> Result<String, ContractError> {
    let mut buf = [0u8; MAX_DID_LEN];
    let len = unsafe { get_caller_did(buf.as_mut_ptr(), buf.len()) };
    if len <= 0 { return Err(ContractError::Unauthorized); }
    String::from_utf8(buf[..len as usize].to_vec())
        .map_err(|_| ContractError::InvalidInput)
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
