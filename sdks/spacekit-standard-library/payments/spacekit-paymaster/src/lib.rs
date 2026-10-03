//! Paymaster contract — ERC-4337–inspired sponsored execution for SpaceKit.
//!
//! A **sponsor** deposits native ASTRA into this contract and sets a policy
//! saying who may draw on it, for which operations, and up to what limits.
//! A permitted caller asks for a sponsored amount (`SPONSOR_CHARGE`); the
//! contract pays it to the caller's address from the sponsor's deposit, so
//! the caller can pay gas or a price without holding ASTRA of their own.
//!
//! Everything is native ASTRA in wei (`u128`): deposits arrive as the call's
//! value, and every payout is a real `transfer_u128` from this contract's own
//! balance. A sponsor's budget is exactly the ASTRA they deposited and have
//! not withdrawn or had drawn. There is no other unit.
//!
//! ## Wire format (little-endian; strings are `u16` length-prefixed UTF-8)
//!
//! | Op | Opcode | Payload | Value |
//! |----|--------|---------|-------|
//! | DEPOSIT        | `0x01` | (empty) | the deposit |
//! | WITHDRAW       | `0x02` | `[amount:u128]` | 0 |
//! | SET_POLICY     | `0x03` | `[policy_json:str]` | 0 |
//! | SPONSOR_CHARGE | `0x04` | `[sponsor_did:str][amount:u128][operation:str]` | 0 |
//! | GET_BUDGET     | `0x05` | `[sponsor_did:str]` → `[1][u128]` | 0 |
//! | GET_POLICY     | `0x06` | `[sponsor_did:str]` → `[1][json]` | 0 |
//!
//! ## Policy JSON
//!
//! ```json
//! { "allowed_dids": ["did:spacekit:…"], "allowed_ops": ["gas", "purchase"],
//!   "per_call_max": "1000000000000000000", "daily_max": "5000000000000000000",
//!   "expires_at": 1735689600 }
//! ```
//!
//! Amounts are wei. An empty or `"*"` list allows everyone / every operation;
//! a limit of 0 means none. Without a policy nothing can be drawn.
//!
//! ## Storage layout
//!
//! - `bal:<sponsor_did>` → u128le deposit balance (older 8-byte values read as u64)
//! - `pol:<sponsor_did>` → policy JSON bytes
//! - `spent:<sponsor_did>:<day>` → u128le spent that day

#![no_std]

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use spacekit_contract_sdk::spacekit_contract;
use spacekit_contract_sdk::{ContractError, ContractErrorCode, SpacekitContract};

#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

#[cfg(all(target_arch = "wasm32", not(test)))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

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
    /// Value attached to this call, u128 little-endian. 0 on success.
    fn msg_value_u128(out_ptr: *mut u8) -> i32;
    /// Pay from this contract's own native balance. 0 on success.
    fn transfer_u128(to_ptr: *const u8, amount_ptr: *const u8) -> i32;
    fn get_timestamp() -> i64;
}

// ═══════════════════════════════════════════════════════════════════════════════
// Opcodes
// ═══════════════════════════════════════════════════════════════════════════════

const OP_DEPOSIT: u8 = 0x01;
const OP_WITHDRAW: u8 = 0x02;
const OP_SET_POLICY: u8 = 0x03;
const OP_SPONSOR_CHARGE: u8 = 0x04;
const OP_GET_BUDGET: u8 = 0x05;
const OP_GET_POLICY: u8 = 0x06;

const MAX_DID_LEN: usize = 128;
const MAX_STRING_LEN: usize = 512;
const POLICY_MAX: usize = 4096;
const SECONDS_PER_DAY: u64 = 86_400;

// ═══════════════════════════════════════════════════════════════════════════════
// Contract
// ═══════════════════════════════════════════════════════════════════════════════

struct PaymasterContract;

impl SpacekitContract for PaymasterContract {
    type Error = ContractError;
    fn init() -> Self {
        PaymasterContract
    }
    fn handle(&mut self, input: &[u8]) -> Result<Vec<u8>, ContractError> {
        if input.is_empty() {
            return Err(ContractError::InvalidInput);
        }
        // Only a deposit takes value; anything sent with another call would
        // be stranded in this contract with no budget to show for it.
        if input[0] != OP_DEPOSIT && attached_value() != 0 {
            return Err(ContractError::InvalidInput);
        }
        match input[0] {
            OP_DEPOSIT => handle_deposit(),
            OP_WITHDRAW => handle_withdraw(&input[1..]),
            OP_SET_POLICY => handle_set_policy(&input[1..]),
            OP_SPONSOR_CHARGE => handle_sponsor_charge(&input[1..]),
            OP_GET_BUDGET => handle_get_budget(&input[1..]),
            OP_GET_POLICY => handle_get_policy(&input[1..]),
            _ => Err(ContractError::InvalidInput),
        }
    }
}

spacekit_contract!(PaymasterContract);

// ═══════════════════════════════════════════════════════════════════════════════
// Policy (lightweight JSON subset — parsed with minimal no_std helpers)
// ═══════════════════════════════════════════════════════════════════════════════

/// The fields `SPONSOR_CHARGE` enforces. The full JSON is stored verbatim.
struct Policy {
    per_call_max: u128,
    daily_max: u128,
    expires_at: u64,
    allowed_ops_raw: String,
    allowed_dids_raw: String,
}

/// A number field, bare or quoted (`"per_call_max": "1000"`).
fn parse_u128_field(json: &str, field: &str) -> u128 {
    let quoted = format!("\"{field}\"");
    if let Some(idx) = json.find(&quoted) {
        let rest = &json[idx + quoted.len()..];
        if let Some(colon) = rest.find(':') {
            let after = rest[colon + 1..].trim_start().trim_start_matches('"');
            let end = after.find(|c: char| !c.is_ascii_digit()).unwrap_or(after.len());
            if end > 0 {
                if let Ok(v) = after[..end].parse::<u128>() {
                    return v;
                }
            }
        }
    }
    0
}

fn parse_list_field(json: &str, field: &str) -> String {
    let quoted = format!("\"{field}\"");
    if let Some(idx) = json.find(&quoted) {
        let rest = &json[idx + quoted.len()..];
        if let Some(bracket) = rest.find('[') {
            if let Some(end) = rest[bracket..].find(']') {
                return rest[bracket..bracket + end + 1].to_string();
            }
        }
    }
    String::new()
}

fn parse_policy(json: &str) -> Policy {
    Policy {
        per_call_max: parse_u128_field(json, "per_call_max"),
        daily_max: parse_u128_field(json, "daily_max"),
        expires_at: parse_u128_field(json, "expires_at").min(u64::MAX as u128) as u64,
        allowed_ops_raw: parse_list_field(json, "allowed_ops"),
        allowed_dids_raw: parse_list_field(json, "allowed_dids"),
    }
}

/// Exact match of `"item"` in a JSON string list (`[]` or `["*"]` = any).
fn list_allows(list_raw: &str, item: &str) -> bool {
    if list_raw.is_empty() || list_raw.contains("\"*\"") {
        return true;
    }
    list_raw.contains(&format!("\"{item}\""))
}

// ═══════════════════════════════════════════════════════════════════════════════
// Handlers
// ═══════════════════════════════════════════════════════════════════════════════

/// DEPOSIT — the sponsor adds the call's value to their budget.
fn handle_deposit() -> Result<Vec<u8>, ContractError> {
    let sponsor = get_caller()?;
    let deposit = attached_value();
    if deposit == 0 {
        return Err(ContractError::InvalidInput);
    }
    let new_balance = load_balance(&sponsor)
        .checked_add(deposit)
        .ok_or(ContractError::Failed)?;
    save_balance(&sponsor, new_balance)?;

    let mut event = Vec::with_capacity(16 + sponsor.len());
    event.extend_from_slice(&new_balance.to_le_bytes());
    event.extend_from_slice(sponsor.as_bytes());
    spacekit_contract_sdk::emit_event_bytes("paymaster:deposit", &event);

    Ok(ok_u128(new_balance))
}

/// WITHDRAW — the sponsor takes unused ASTRA back to their address.
fn handle_withdraw(data: &[u8]) -> Result<Vec<u8>, ContractError> {
    let mut pos = 0usize;
    let amount = read_u128(data, &mut pos)?;
    if amount == 0 {
        return Err(ContractError::InvalidInput);
    }
    let sponsor = get_caller()?;
    let current = load_balance(&sponsor);
    if amount > current {
        return Err(ContractError::InsufficientBalance);
    }
    save_balance(&sponsor, current - amount)?;
    pay(&sponsor, amount)?;

    let mut event = Vec::with_capacity(32);
    event.extend_from_slice(&amount.to_le_bytes());
    event.extend_from_slice(&(current - amount).to_le_bytes());
    spacekit_contract_sdk::emit_event_bytes("paymaster:withdraw", &event);

    Ok(ok_u128(current - amount))
}

/// SET_POLICY — the sponsor defines who may draw, for what, and how much.
fn handle_set_policy(data: &[u8]) -> Result<Vec<u8>, ContractError> {
    let mut pos = 0usize;
    let policy_json = read_string(data, &mut pos)?;
    if policy_json.len() > POLICY_MAX {
        return Err(ContractError::InvalidInput);
    }
    let sponsor = get_caller()?;
    host_storage_save(&policy_key(&sponsor), policy_json.as_bytes())?;
    spacekit_contract_sdk::emit_event_bytes("paymaster:policy_set", sponsor.as_bytes());
    Ok(vec![1u8])
}

/// SPONSOR_CHARGE — pay `amount` from the sponsor's deposit to the caller,
/// within the sponsor's policy (expiry, allowed DIDs and operations,
/// per-call and daily limits).
fn handle_sponsor_charge(data: &[u8]) -> Result<Vec<u8>, ContractError> {
    let mut pos = 0usize;
    let sponsor_did = read_string(data, &mut pos)?;
    let amount = read_u128(data, &mut pos)?;
    let operation = read_string(data, &mut pos)?;
    if amount == 0 {
        return Err(ContractError::InvalidInput);
    }

    let beneficiary = get_caller()?;
    let now = block_time();

    // No policy, no sponsorship.
    let policy_bytes = host_storage_load_raw(&policy_key(&sponsor_did), POLICY_MAX)
        .map_err(|_| ContractError::Unauthorized)?;
    let policy_str = core::str::from_utf8(&policy_bytes).map_err(|_| ContractError::InvalidInput)?;
    let policy = parse_policy(policy_str);

    if policy.expires_at > 0 && now > policy.expires_at {
        return Err(ContractError::Unauthorized);
    }
    if !list_allows(&policy.allowed_dids_raw, &beneficiary) {
        return Err(ContractError::Unauthorized);
    }
    if !list_allows(&policy.allowed_ops_raw, &operation) {
        return Err(ContractError::Unauthorized);
    }
    if policy.per_call_max > 0 && amount > policy.per_call_max {
        return Err(ContractError::InsufficientPayment);
    }

    let daily_key = daily_spend_key(&sponsor_did, now / SECONDS_PER_DAY);
    let daily_spent = load_u128_or_zero(&daily_key);
    let new_daily = daily_spent.checked_add(amount).ok_or(ContractError::Failed)?;
    if policy.daily_max > 0 && new_daily > policy.daily_max {
        return Err(ContractError::InsufficientBalance);
    }

    let balance = load_balance(&sponsor_did);
    if amount > balance {
        return Err(ContractError::InsufficientBalance);
    }

    save_balance(&sponsor_did, balance - amount)?;
    save_u128(&daily_key, new_daily)?;
    pay(&beneficiary, amount)?;

    let mut event = Vec::with_capacity(18 + sponsor_did.len() + beneficiary.len() + operation.len());
    event.extend_from_slice(&amount.to_le_bytes());
    event.extend_from_slice(sponsor_did.as_bytes());
    event.push(0);
    event.extend_from_slice(beneficiary.as_bytes());
    event.push(0);
    event.extend_from_slice(operation.as_bytes());
    spacekit_contract_sdk::emit_event_bytes("paymaster:sponsored", &event);

    Ok(ok_u128(balance - amount))
}

/// GET_BUDGET — the sponsor's remaining deposit, `[1][u128le]`.
fn handle_get_budget(data: &[u8]) -> Result<Vec<u8>, ContractError> {
    let mut pos = 0usize;
    let sponsor_did = read_string(data, &mut pos)?;
    Ok(ok_u128(load_balance(&sponsor_did)))
}

/// GET_POLICY — the sponsor's raw policy JSON.
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
// Value helpers
// ═══════════════════════════════════════════════════════════════════════════════

fn attached_value() -> u128 {
    let mut out = [0u8; 16];
    if unsafe { msg_value_u128(out.as_mut_ptr()) } != 0 {
        return 0;
    }
    u128::from_le_bytes(out)
}

fn block_time() -> u64 {
    let t = unsafe { get_timestamp() };
    if t < 0 {
        0
    } else {
        t as u64
    }
}

/// Pay `amount` wei from this contract's balance to the address of `did`.
fn pay(did: &str, amount: u128) -> Result<(), ContractError> {
    let to = did_address(did)?;
    let amount_bytes = amount.to_le_bytes();
    if unsafe { transfer_u128(to.as_ptr(), amount_bytes.as_ptr()) } != 0 {
        return Err(ContractError::Failed);
    }
    Ok(())
}

/// The 20-byte address in `did:spacekit:<40 hex>`.
fn did_address(did: &str) -> Result<[u8; 20], ContractError> {
    let hex = did.strip_prefix("did:spacekit:").ok_or(ContractError::InvalidInput)?;
    let hex = hex.strip_prefix("0x").unwrap_or(hex);
    if hex.len() != 40 {
        return Err(ContractError::InvalidInput);
    }
    let mut out = [0u8; 20];
    for (i, chunk) in hex.as_bytes().chunks(2).enumerate() {
        let hi = hex_val(chunk[0]).ok_or(ContractError::InvalidInput)?;
        let lo = hex_val(chunk[1]).ok_or(ContractError::InvalidInput)?;
        out[i] = (hi << 4) | lo;
    }
    Ok(out)
}

fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

fn ok_u128(v: u128) -> Vec<u8> {
    let mut out = Vec::with_capacity(17);
    out.push(1u8);
    out.extend_from_slice(&v.to_le_bytes());
    out
}

// ═══════════════════════════════════════════════════════════════════════════════
// Storage helpers
// ═══════════════════════════════════════════════════════════════════════════════

fn balance_key(did: &str) -> String {
    format!("bal:{did}")
}

fn policy_key(did: &str) -> String {
    format!("pol:{did}")
}

fn daily_spend_key(did: &str, day: u64) -> String {
    format!("spent:{did}:{day}")
}

fn load_balance(did: &str) -> u128 {
    load_u128_or_zero(&balance_key(did))
}

fn save_balance(did: &str, amount: u128) -> Result<(), ContractError> {
    save_u128(&balance_key(did), amount)
}

/// A u128le value; an 8-byte value written by the earlier u64 version reads
/// as that u64.
fn load_u128_or_zero(key: &str) -> u128 {
    match host_storage_load_raw(key, 16) {
        Ok(raw) if raw.len() == 16 => {
            let mut b = [0u8; 16];
            b.copy_from_slice(&raw);
            u128::from_le_bytes(b)
        }
        Ok(raw) if raw.len() == 8 => {
            let mut b = [0u8; 8];
            b.copy_from_slice(&raw);
            u64::from_le_bytes(b) as u128
        }
        _ => 0,
    }
}

fn save_u128(key: &str, value: u128) -> Result<(), ContractError> {
    host_storage_save(key, &value.to_le_bytes())
}

fn get_caller() -> Result<String, ContractError> {
    let mut buf = [0u8; MAX_DID_LEN];
    let len = unsafe { get_caller_did(buf.as_mut_ptr(), buf.len()) };
    if len <= 0 {
        return Err(ContractError::Unauthorized);
    }
    String::from_utf8(buf[..len as usize].to_vec()).map_err(|_| ContractError::InvalidInput)
}

fn host_storage_save(key: &str, data: &[u8]) -> Result<(), ContractError> {
    let rc = unsafe { storage_save(key.as_ptr(), key.len(), data.as_ptr(), data.len()) };
    if rc < 0 {
        Err(ContractError::StorageError)
    } else {
        Ok(())
    }
}

fn host_storage_load_raw(key: &str, max_len: usize) -> Result<Vec<u8>, ContractError> {
    let mut buf = vec![0u8; max_len];
    let n = unsafe { storage_load(key.as_ptr(), key.len(), buf.as_mut_ptr(), buf.len()) };
    if n <= 0 {
        return Err(ContractError::StorageError);
    }
    buf.truncate(n as usize);
    Ok(buf)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Wire format helpers
// ═══════════════════════════════════════════════════════════════════════════════

fn read_u16(input: &[u8], pos: &mut usize) -> Result<u16, ContractError> {
    if *pos + 2 > input.len() {
        return Err(ContractError::InvalidInput);
    }
    let b = [input[*pos], input[*pos + 1]];
    *pos += 2;
    Ok(u16::from_le_bytes(b))
}

fn read_u128(input: &[u8], pos: &mut usize) -> Result<u128, ContractError> {
    if *pos + 16 > input.len() {
        return Err(ContractError::InvalidInput);
    }
    let mut b = [0u8; 16];
    b.copy_from_slice(&input[*pos..*pos + 16]);
    *pos += 16;
    Ok(u128::from_le_bytes(b))
}

fn read_string(input: &[u8], pos: &mut usize) -> Result<String, ContractError> {
    let len = read_u16(input, pos)? as usize;
    if *pos + len > input.len() || len > MAX_STRING_LEN.max(POLICY_MAX) {
        return Err(ContractError::InvalidInput);
    }
    let s = core::str::from_utf8(&input[*pos..*pos + len]).map_err(|_| ContractError::InvalidInput)?;
    *pos += len;
    Ok(s.to_string())
}
