//! SpaceKit ASTRA Rewards Contract
//!
//! Tracks per-DID ASTRA balances on the SpaceKit network. Receives credit
//! instructions from the protocol's Service Reward Accumulator (SRA), enforces
//! the 2,000,000,000 ASTRA hard cap, and processes operator withdrawals
//! between DIDs.
//!
//! # Design properties
//!
//! - **Per-DID accounting.** Balances keyed by 32-byte DID hash.
//! - **Atomic operations.** Each operation succeeds entirely or reverts.
//! - **Hard cap enforcement.** Total ever-emitted cannot exceed 2B * 10^18 (wei-ASTRA).
//! - **Protocol-trusted credits.** Only admin DID (the SRA's authorized identity)
//!   can credit balances. No path to mint ASTRA outside the protocol's
//!   consensus-validated reward computation.
//! - **Read-open.** Anyone can query any DID's balance and the network state.
//!
//! # Wire format (length-prefixed binary)
//!
//! All operations dispatched by single-byte opcode followed by payload.
//!
//! | Op | Opcode | Payload                                           | Returns           |
//! |----|--------|---------------------------------------------------|-------------------|
//! | INIT                | 0x01 | [treasury_did_hash 32]                    | empty             |
//! | CREDIT              | 0x10 | [recipient_hash 32][amount 16][log_hash 32] | [new_balance 16] |
//! | WITHDRAW            | 0x20 | [recipient_hash 32][amount 16]              | [new_balance 16] |
//! | GET_BALANCE         | 0x30 | [did_hash 32]                              | [balance 16]      |
//! | GET_WITHDRAWN       | 0x31 | [did_hash 32]                              | [total 16]        |
//! | GET_TOTAL_EMITTED   | 0x32 | (empty)                                    | [total 16]        |
//! | GET_REMAINING_CAP   | 0x33 | (empty)                                    | [remaining 16]    |
//! | GET_WITHDRAWAL_COUNT| 0x34 | [did_hash 32]                              | [count 8]         |
//! | ROTATE_ADMIN        | 0xF0 | [new_admin_hash 32]                        | empty             |
//!
//! # Events
//!
//! - `astra_rewards.initialized`     - genesis allocation set
//! - `astra_rewards.credit`          - balance credited via SRA
//! - `astra_rewards.withdraw`        - balance transferred between DIDs
//! - `astra_rewards.cap_reached`     - credit attempt rejected due to cap
//! - `astra_rewards.admin_rotated`   - admin DID changed
//!
//! # References
//!
//! - ASTRA Emission Schedule (Document E)
//! - AstraRewards Contract Specification (Document F)
//! - Service Reward Accumulator Integration Spec (Document G)
//! - SpaceKit Tokenomics v2.0

#![no_std]

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use spacekit_contract_sdk::{
    emit_event_bytes, get_caller_did_hash, spacekit_contract,
    spacekit_storage::{storage_load, storage_save},
    wire::read_u8,
    ContractError, ContractErrorCode, SpacekitContract,
};

#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

// ============================================================================
// Constants
// ============================================================================

// Opcodes
const OP_INIT: u8 = 0x01;
const OP_CREDIT: u8 = 0x10;
const OP_WITHDRAW: u8 = 0x20;
const OP_GET_BALANCE: u8 = 0x30;
const OP_GET_WITHDRAWN: u8 = 0x31;
const OP_GET_TOTAL_EMITTED: u8 = 0x32;
const OP_GET_REMAINING_CAP: u8 = 0x33;
const OP_GET_WITHDRAWAL_COUNT: u8 = 0x34;
const OP_ROTATE_ADMIN: u8 = 0xF0;

// Hard cap: 2,000,000,000 ASTRA with 18 decimals = 2 * 10^27 wei-ASTRA
// 2_000_000_000 * 10^18 = 2_000_000_000_000_000_000_000_000_000
const HARD_CAP_WEI_ASTRA: u128 = 2_000_000_000_000_000_000_000_000_000;

// Genesis treasury allocation: 350,000,000 ASTRA = 350 * 10^24 wei-ASTRA
const GENESIS_TREASURY_WEI: u128 = 350_000_000_000_000_000_000_000_000;

// Sentinel DID hashes (all-zero is treated as invalid)
const ZERO_HASH: [u8; 32] = [0u8; 32];

// Storage keys
const KEY_TOTAL_EMITTED: &str = "astra_rewards.total_emitted";
const KEY_IS_INITIALIZED: &str = "astra_rewards.is_initialized";
const KEY_ADMIN: &str = "astra_rewards.admin";

// Storage key prefixes (concatenated with hex-encoded DID hash for per-DID data)
const KEY_PREFIX_BALANCE: &str = "astra_rewards.balance.";
const KEY_PREFIX_WITHDRAWN: &str = "astra_rewards.withdrawn.";
const KEY_PREFIX_WITHDRAWAL_COUNT: &str = "astra_rewards.wcount.";

// ============================================================================
// Contract
// ============================================================================

struct AstraRewards;

impl SpacekitContract for AstraRewards {
    type Error = ContractError;

    fn init() -> Self {
        AstraRewards
    }

    fn handle(&mut self, input: &[u8]) -> Result<Vec<u8>, ContractError> {
        if input.is_empty() {
            return Err(ContractError::InvalidInput);
        }

        let mut cursor = 0usize;
        let opcode = read_u8(input, &mut cursor)?;

        match opcode {
            OP_INIT => op_init(input, &mut cursor),
            OP_CREDIT => op_credit(input, &mut cursor),
            OP_WITHDRAW => op_withdraw(input, &mut cursor),
            OP_GET_BALANCE => op_get_balance(input, &mut cursor),
            OP_GET_WITHDRAWN => op_get_withdrawn(input, &mut cursor),
            OP_GET_TOTAL_EMITTED => op_get_total_emitted(),
            OP_GET_REMAINING_CAP => op_get_remaining_cap(),
            OP_GET_WITHDRAWAL_COUNT => op_get_withdrawal_count(input, &mut cursor),
            OP_ROTATE_ADMIN => op_rotate_admin(input, &mut cursor),
            _ => Err(ContractError::InvalidInput),
        }
    }
}

#[cfg(not(test))]
spacekit_contract!(AstraRewards);

// ============================================================================
// Lifecycle: INIT
// ============================================================================

fn op_init(input: &[u8], cursor: &mut usize) -> Result<Vec<u8>, ContractError> {
    // Verify not yet initialized
    if storage_load(KEY_IS_INITIALIZED).is_ok() {
        return Err(ContractError::AlreadyInitialized);
    }

    // Read treasury DID hash
    let treasury_hash = read_did_hash(input, cursor)?;
    if treasury_hash == ZERO_HASH {
        return Err(ContractError::InvalidInput);
    }

    // The deployer becomes the initial admin
    let deployer_hash = get_caller_did_hash()?;

    // Credit treasury with the genesis allocation
    write_balance(&treasury_hash, GENESIS_TREASURY_WEI)?;

    // Set total_emitted to the treasury allocation
    write_u128(KEY_TOTAL_EMITTED, GENESIS_TREASURY_WEI)?;

    // Set admin to deployer
    storage_save(KEY_ADMIN, &deployer_hash)?;

    // Mark initialized
    storage_save(KEY_IS_INITIALIZED, &[1u8])?;

    // Emit initialization event
    let mut payload = Vec::with_capacity(48);
    payload.extend_from_slice(&treasury_hash);
    payload.extend_from_slice(&GENESIS_TREASURY_WEI.to_le_bytes());
    emit_event_bytes("astra_rewards.initialized", &payload);

    Ok(Vec::new())
}

// ============================================================================
// Credit (admin only): credits a DID's balance from SRA
// ============================================================================

fn op_credit(input: &[u8], cursor: &mut usize) -> Result<Vec<u8>, ContractError> {
    require_initialized()?;
    require_admin()?;

    // Read payload
    let recipient_hash = read_did_hash(input, cursor)?;
    let amount = read_u128(input, cursor)?;
    let log_event_hash = read_did_hash(input, cursor)?; // 32-byte content hash

    if amount == 0 {
        return Err(ContractError::InvalidInput);
    }
    if recipient_hash == ZERO_HASH {
        return Err(ContractError::InvalidInput);
    }

    // Read current total_emitted
    let current_total = read_u128_or_zero(KEY_TOTAL_EMITTED)?;

    // Check cap (saturating_add to avoid overflow at exactly the boundary)
    let proposed_total = current_total.checked_add(amount).ok_or(ContractError::CapExceeded)?;

    if proposed_total > HARD_CAP_WEI_ASTRA {
        // Emit cap_reached event for audit purposes
        let remaining = HARD_CAP_WEI_ASTRA.saturating_sub(current_total);
        let mut payload = Vec::with_capacity(32);
        payload.extend_from_slice(&amount.to_le_bytes());
        payload.extend_from_slice(&remaining.to_le_bytes());
        emit_event_bytes("astra_rewards.cap_reached", &payload);
        return Err(ContractError::CapExceeded);
    }

    // Read recipient's current balance
    let current_balance = read_balance(&recipient_hash)?;
    let new_balance = current_balance.checked_add(amount).ok_or(ContractError::InvalidInput)?;

    // Update balance and total_emitted atomically
    write_balance(&recipient_hash, new_balance)?;
    write_u128(KEY_TOTAL_EMITTED, proposed_total)?;

    // Emit credit event with full audit payload:
    //   recipient_hash (32) + amount (16) + log_event_hash (32) + new_balance (16) + total_emitted (16) = 112 bytes
    let mut payload = Vec::with_capacity(112);
    payload.extend_from_slice(&recipient_hash);
    payload.extend_from_slice(&amount.to_le_bytes());
    payload.extend_from_slice(&log_event_hash);
    payload.extend_from_slice(&new_balance.to_le_bytes());
    payload.extend_from_slice(&proposed_total.to_le_bytes());
    emit_event_bytes("astra_rewards.credit", &payload);

    // Return new balance
    Ok(new_balance.to_le_bytes().to_vec())
}

// ============================================================================
// Withdraw: transfer balance to another DID
// ============================================================================

fn op_withdraw(input: &[u8], cursor: &mut usize) -> Result<Vec<u8>, ContractError> {
    require_initialized()?;

    let caller_hash = get_caller_did_hash()?;
    let recipient_hash = read_did_hash(input, cursor)?;
    let amount = read_u128(input, cursor)?;

    if amount == 0 {
        return Err(ContractError::InvalidInput);
    }
    if recipient_hash == ZERO_HASH {
        return Err(ContractError::InvalidInput);
    }

    // Read caller's balance
    let caller_balance = read_balance(&caller_hash)?;

    if caller_balance < amount {
        return Err(ContractError::InsufficientBalance);
    }

    // Compute new balances
    let new_caller_balance = caller_balance - amount;

    // Edge case: withdrawing to self produces a no-op balance change (audit trail still produced)
    let new_recipient_balance = if caller_hash == recipient_hash {
        // The balance after: subtract then re-add = same
        new_caller_balance.checked_add(amount).ok_or(ContractError::InvalidInput)?
    } else {
        // Read recipient's current balance and add
        let recipient_current = read_balance(&recipient_hash)?;
        recipient_current.checked_add(amount).ok_or(ContractError::InvalidInput)?
    };

    // Update caller's balance
    write_balance(&caller_hash, new_caller_balance)?;

    // Update recipient's balance (skip if self - already accounted)
    if caller_hash != recipient_hash {
        write_balance(&recipient_hash, new_recipient_balance)?;
    }

    // Update caller's lifetime withdrawn total
    let current_withdrawn = read_u128_or_zero(&withdrawn_key(&caller_hash))?;
    let new_withdrawn = current_withdrawn.checked_add(amount).ok_or(ContractError::InvalidInput)?;
    write_u128(&withdrawn_key(&caller_hash), new_withdrawn)?;

    // Update caller's withdrawal count
    let current_count = read_u64_or_zero(&wcount_key(&caller_hash))?;
    let new_count = current_count.checked_add(1).ok_or(ContractError::InvalidInput)?;
    write_u64(&wcount_key(&caller_hash), new_count)?;

    // Emit withdrawal event:
    //   from_hash (32) + to_hash (32) + amount (16) + new_from_balance (16) + withdrawal_count (8) = 104 bytes
    let mut payload = Vec::with_capacity(104);
    payload.extend_from_slice(&caller_hash);
    payload.extend_from_slice(&recipient_hash);
    payload.extend_from_slice(&amount.to_le_bytes());
    payload.extend_from_slice(&new_caller_balance.to_le_bytes());
    payload.extend_from_slice(&new_count.to_le_bytes());
    emit_event_bytes("astra_rewards.withdraw", &payload);

    // Return new caller balance
    Ok(new_caller_balance.to_le_bytes().to_vec())
}

// ============================================================================
// Read operations
// ============================================================================

fn op_get_balance(input: &[u8], cursor: &mut usize) -> Result<Vec<u8>, ContractError> {
    let did_hash = read_did_hash(input, cursor)?;
    let balance = read_balance(&did_hash)?;
    Ok(balance.to_le_bytes().to_vec())
}

fn op_get_withdrawn(input: &[u8], cursor: &mut usize) -> Result<Vec<u8>, ContractError> {
    let did_hash = read_did_hash(input, cursor)?;
    let withdrawn = read_u128_or_zero(&withdrawn_key(&did_hash))?;
    Ok(withdrawn.to_le_bytes().to_vec())
}

fn op_get_total_emitted() -> Result<Vec<u8>, ContractError> {
    let total = read_u128_or_zero(KEY_TOTAL_EMITTED)?;
    Ok(total.to_le_bytes().to_vec())
}

fn op_get_remaining_cap() -> Result<Vec<u8>, ContractError> {
    let total = read_u128_or_zero(KEY_TOTAL_EMITTED)?;
    let remaining = HARD_CAP_WEI_ASTRA.saturating_sub(total);
    Ok(remaining.to_le_bytes().to_vec())
}

fn op_get_withdrawal_count(input: &[u8], cursor: &mut usize) -> Result<Vec<u8>, ContractError> {
    let did_hash = read_did_hash(input, cursor)?;
    let count = read_u64_or_zero(&wcount_key(&did_hash))?;
    Ok(count.to_le_bytes().to_vec())
}

// ============================================================================
// Admin: rotate admin DID
// ============================================================================

fn op_rotate_admin(input: &[u8], cursor: &mut usize) -> Result<Vec<u8>, ContractError> {
    require_initialized()?;
    require_admin()?;

    let new_admin_hash = read_did_hash(input, cursor)?;
    if new_admin_hash == ZERO_HASH {
        return Err(ContractError::InvalidInput);
    }

    let old_admin_hash = storage_load(KEY_ADMIN).map_err(|_| ContractError::StorageError)?;
    storage_save(KEY_ADMIN, &new_admin_hash)?;

    // Emit rotation event
    let mut payload = Vec::with_capacity(64);
    payload.extend_from_slice(&old_admin_hash);
    payload.extend_from_slice(&new_admin_hash);
    emit_event_bytes("astra_rewards.admin_rotated", &payload);

    Ok(Vec::new())
}

// ============================================================================
// Authorization helpers
// ============================================================================

fn require_initialized() -> Result<(), ContractError> {
    if storage_load(KEY_IS_INITIALIZED).is_err() {
        return Err(ContractError::NotInitialized);
    }
    Ok(())
}

fn require_admin() -> Result<(), ContractError> {
    let caller_hash = get_caller_did_hash()?;
    let admin_hash = storage_load(KEY_ADMIN).map_err(|_| ContractError::Unauthorized)?;
    if caller_hash[..] != admin_hash[..] {
        return Err(ContractError::Unauthorized);
    }
    Ok(())
}

// ============================================================================
// Balance accessors
// ============================================================================

fn read_balance(did_hash: &[u8; 32]) -> Result<u128, ContractError> {
    read_u128_or_zero(&balance_key(did_hash))
}

fn write_balance(did_hash: &[u8; 32], balance: u128) -> Result<(), ContractError> {
    write_u128(&balance_key(did_hash), balance)
}

fn balance_key(did_hash: &[u8; 32]) -> String {
    format!("{}{}", KEY_PREFIX_BALANCE, hex_encode(did_hash))
}

fn withdrawn_key(did_hash: &[u8; 32]) -> String {
    format!("{}{}", KEY_PREFIX_WITHDRAWN, hex_encode(did_hash))
}

fn wcount_key(did_hash: &[u8; 32]) -> String {
    format!("{}{}", KEY_PREFIX_WITHDRAWAL_COUNT, hex_encode(did_hash))
}

// ============================================================================
// Integer storage helpers
// ============================================================================

fn read_u128_or_zero(key: &str) -> Result<u128, ContractError> {
    match storage_load(key) {
        Ok(bytes) => {
            if bytes.len() < 16 {
                return Err(ContractError::StorageError);
            }
            let mut arr = [0u8; 16];
            arr.copy_from_slice(&bytes[..16]);
            Ok(u128::from_le_bytes(arr))
        }
        Err(_) => Ok(0),
    }
}

fn write_u128(key: &str, value: u128) -> Result<(), ContractError> {
    storage_save(key, &value.to_le_bytes())
}

fn read_u64_or_zero(key: &str) -> Result<u64, ContractError> {
    match storage_load(key) {
        Ok(bytes) => {
            if bytes.len() < 8 {
                return Err(ContractError::StorageError);
            }
            let mut arr = [0u8; 8];
            arr.copy_from_slice(&bytes[..8]);
            Ok(u64::from_le_bytes(arr))
        }
        Err(_) => Ok(0),
    }
}

fn write_u64(key: &str, value: u64) -> Result<(), ContractError> {
    storage_save(key, &value.to_le_bytes())
}

// ============================================================================
// Wire format helpers
// ============================================================================

fn read_did_hash(input: &[u8], cursor: &mut usize) -> Result<[u8; 32], ContractError> {
    if input.len() < *cursor + 32 {
        return Err(ContractError::InvalidInput);
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&input[*cursor..*cursor + 32]);
    *cursor += 32;
    Ok(arr)
}

fn read_u128(input: &[u8], cursor: &mut usize) -> Result<u128, ContractError> {
    if input.len() < *cursor + 16 {
        return Err(ContractError::InvalidInput);
    }
    let mut arr = [0u8; 16];
    arr.copy_from_slice(&input[*cursor..*cursor + 16]);
    *cursor += 16;
    Ok(u128::from_le_bytes(arr))
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(hex_char(byte >> 4));
        out.push(hex_char(byte & 0xF));
    }
    out
}

fn hex_char(nibble: u8) -> char {
    match nibble {
        0..=9 => (b'0' + nibble) as char,
        10..=15 => (b'a' + (nibble - 10)) as char,
        _ => '?', // unreachable for nibbles
    }
}

// Unit tests: see `spacekit-tokenomics/tokenomics_update/AstraRewards.rs` (host tests)
// or run `cargo check -p astra-rewards --target wasm32-unknown-unknown`.
