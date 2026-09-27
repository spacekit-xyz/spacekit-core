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
//! | GET_LOCKED          | 0x35 | [did_hash 32]                              | [locked 16][released 16][releasable 16] |
//! | GET_PHASE           | 0x36 | (empty)                                    | [phase 1][genesis 8][cliff 8][end 8][pos_at 8] |
//! | SET_LOCKED_RECIPIENT| 0x40 | [did_hash 32][flag 1]                      | empty             |
//! | END_POA             | 0x41 | (empty)                                    | empty             |
//! | RELEASE             | 0x42 | [did_hash 32]                              | [released 16]     |
//!
//! # Proof-of-authority reward lock (Tokenomics §1.11, ASTRA_EMISSION §8a)
//!
//! While the network is in proof of authority (`phase = 0`, set at INIT),
//! CREDITs to a DID marked with SET_LOCKED_RECIPIENT (authorities and
//! operators affiliated with SWTCH Labs or the Foundation) go to a locked
//! balance instead of the spendable one. Locked ASTRA counts toward
//! `total_emitted` like any credit.
//!
//! Locked ASTRA vests on a schedule fixed at INIT from the genesis block time:
//! nothing before the cliff (365 days), then linearly until 1,095 days
//! (36 months). RELEASE (callable by anyone for any DID) moves the vested,
//! not-yet-released amount into the spendable balance; WITHDRAW releases the
//! caller's vested amount first. END_POA (admin, irreversible) stops new
//! credits from being locked; ASTRA locked before it keeps vesting on the same
//! schedule.
//!
//! # Events
//!
//! - `astra_rewards.initialized`     - genesis allocation set
//! - `astra_rewards.credit`          - balance credited via SRA
//! - `astra_rewards.withdraw`        - balance transferred between DIDs
//! - `astra_rewards.cap_reached`     - credit attempt rejected due to cap
//! - `astra_rewards.admin_rotated`   - admin DID changed
//! - `astra_rewards.credit_locked`   - PoA-phase credit added to a locked balance
//! - `astra_rewards.released`        - vested locked ASTRA moved to the spendable balance
//! - `astra_rewards.lock_recipient`  - a DID was marked or unmarked for locking
//! - `astra_rewards.poa_ended`       - proof of authority ended; new credits unlocked
//!
//! # References
//!
//! - ASTRA Emission Schedule (Document E)
//! - AstraRewards Contract Specification (Document F)
//! - Service Reward Accumulator Integration Spec (Document G)
//! - SpaceKit Tokenomics v2.0

#![cfg_attr(not(test), no_std)]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use spacekit_contract_sdk::{
    block_timestamp, emit_event_bytes, get_caller_did_hash, spacekit_contract,
    spacekit_storage::{storage_load, storage_save},
    wire::read_u8,
    ContractError, ContractErrorCode, SpacekitContract,
};

#[cfg(not(test))]
#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

#[cfg(not(test))]
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
const OP_GET_LOCKED: u8 = 0x35;
const OP_GET_PHASE: u8 = 0x36;
const OP_SET_LOCKED_RECIPIENT: u8 = 0x40;
const OP_END_POA: u8 = 0x41;
const OP_RELEASE: u8 = 0x42;

// Proof-of-authority reward lock schedule (seconds after genesis).
const LOCK_CLIFF_SECS: u64 = 365 * 86_400;
const LOCK_END_SECS: u64 = 3 * 365 * 86_400;

const PHASE_POA: u8 = 0;
const PHASE_POS: u8 = 1;

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
const KEY_PHASE: &str = "astra_rewards.phase";
const KEY_GENESIS_TS: &str = "astra_rewards.genesis_ts";
const KEY_POS_ACTIVATED_TS: &str = "astra_rewards.pos_activated_ts";

// Storage key prefixes (concatenated with hex-encoded DID hash for per-DID data)
const KEY_PREFIX_BALANCE: &str = "astra_rewards.balance.";
const KEY_PREFIX_WITHDRAWN: &str = "astra_rewards.withdrawn.";
const KEY_PREFIX_WITHDRAWAL_COUNT: &str = "astra_rewards.wcount.";
const KEY_PREFIX_LOCKED: &str = "astra_rewards.locked.";
const KEY_PREFIX_RELEASED: &str = "astra_rewards.released.";
const KEY_PREFIX_LOCK_FLAG: &str = "astra_rewards.lockflag.";

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
            OP_GET_LOCKED => op_get_locked(input, &mut cursor),
            OP_GET_PHASE => op_get_phase(),
            OP_SET_LOCKED_RECIPIENT => op_set_locked_recipient(input, &mut cursor),
            OP_END_POA => op_end_poa(),
            OP_RELEASE => op_release(input, &mut cursor),
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

    // The network starts in proof of authority; the lock schedule is anchored
    // to the genesis block time.
    storage_save(KEY_PHASE, &[PHASE_POA])?;
    write_u64(KEY_GENESIS_TS, block_timestamp())?;

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

    // Proof-of-authority credits to marked recipients are locked.
    if read_phase()? == PHASE_POA && is_lock_recipient(&recipient_hash)? {
        let locked = read_u128_or_zero(&locked_key(&recipient_hash))?;
        let new_locked = locked.checked_add(amount).ok_or(ContractError::InvalidInput)?;
        write_u128(&locked_key(&recipient_hash), new_locked)?;
        write_u128(KEY_TOTAL_EMITTED, proposed_total)?;

        // recipient (32) + amount (16) + log_event_hash (32) + locked_total (16) + total_emitted (16)
        let mut payload = Vec::with_capacity(112);
        payload.extend_from_slice(&recipient_hash);
        payload.extend_from_slice(&amount.to_le_bytes());
        payload.extend_from_slice(&log_event_hash);
        payload.extend_from_slice(&new_locked.to_le_bytes());
        payload.extend_from_slice(&proposed_total.to_le_bytes());
        emit_event_bytes("astra_rewards.credit_locked", &payload);

        // The spendable balance is unchanged.
        return Ok(read_balance(&recipient_hash)?.to_le_bytes().to_vec());
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

    // Vested locked ASTRA becomes spendable before the balance check.
    release_vested(&caller_hash)?;

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
// Proof-of-authority reward lock
// ============================================================================

/// Vested part of `locked` at `now` for a schedule anchored at `genesis`:
/// 0 before the cliff, linear from the cliff to the end, everything after.
pub fn vested_amount(locked: u128, genesis: u64, now: u64) -> u128 {
    let elapsed = now.saturating_sub(genesis);
    if elapsed < LOCK_CLIFF_SECS {
        return 0;
    }
    if elapsed >= LOCK_END_SECS {
        return locked;
    }
    let since_cliff = (elapsed - LOCK_CLIFF_SECS) as u128;
    let span = (LOCK_END_SECS - LOCK_CLIFF_SECS) as u128;
    // locked <= 2e27 and since_cliff < 7e7, so the product fits in u128.
    locked.saturating_mul(since_cliff) / span
}

fn read_phase() -> Result<u8, ContractError> {
    match storage_load(KEY_PHASE) {
        Ok(bytes) if !bytes.is_empty() => Ok(bytes[0]),
        // Contracts initialized before the lock existed have no phase: they
        // behave as proof of stake (nothing is locked).
        _ => Ok(PHASE_POS),
    }
}

fn is_lock_recipient(did_hash: &[u8; 32]) -> Result<bool, ContractError> {
    match storage_load(&lock_flag_key(did_hash)) {
        Ok(bytes) => Ok(bytes.first() == Some(&1)),
        Err(_) => Ok(false),
    }
}

/// `(locked_total, released, releasable_now)` for a DID.
fn lock_position(did_hash: &[u8; 32]) -> Result<(u128, u128, u128), ContractError> {
    let locked = read_u128_or_zero(&locked_key(did_hash))?;
    let released = read_u128_or_zero(&released_key(did_hash))?;
    if locked == 0 {
        return Ok((0, released, 0));
    }
    let genesis = read_u64_or_zero(KEY_GENESIS_TS)?;
    let vested = vested_amount(locked, genesis, block_timestamp());
    Ok((locked, released, vested.saturating_sub(released)))
}

/// Move vested, unreleased ASTRA into the spendable balance. Returns the amount moved.
fn release_vested(did_hash: &[u8; 32]) -> Result<u128, ContractError> {
    let (locked, released, releasable) = lock_position(did_hash)?;
    if releasable == 0 {
        return Ok(0);
    }
    let new_released = released.checked_add(releasable).ok_or(ContractError::InvalidInput)?;
    let new_balance = read_balance(did_hash)?
        .checked_add(releasable)
        .ok_or(ContractError::InvalidInput)?;
    write_u128(&released_key(did_hash), new_released)?;
    write_balance(did_hash, new_balance)?;

    // did (32) + released_now (16) + released_total (16) + locked_total (16)
    let mut payload = Vec::with_capacity(80);
    payload.extend_from_slice(did_hash);
    payload.extend_from_slice(&releasable.to_le_bytes());
    payload.extend_from_slice(&new_released.to_le_bytes());
    payload.extend_from_slice(&locked.to_le_bytes());
    emit_event_bytes("astra_rewards.released", &payload);
    Ok(releasable)
}

fn op_get_locked(input: &[u8], cursor: &mut usize) -> Result<Vec<u8>, ContractError> {
    let did_hash = read_did_hash(input, cursor)?;
    let (locked, released, releasable) = lock_position(&did_hash)?;
    let mut out = Vec::with_capacity(48);
    out.extend_from_slice(&locked.to_le_bytes());
    out.extend_from_slice(&released.to_le_bytes());
    out.extend_from_slice(&releasable.to_le_bytes());
    Ok(out)
}

fn op_get_phase() -> Result<Vec<u8>, ContractError> {
    let genesis = read_u64_or_zero(KEY_GENESIS_TS)?;
    let mut out = Vec::with_capacity(33);
    out.push(read_phase()?);
    out.extend_from_slice(&genesis.to_le_bytes());
    out.extend_from_slice(&genesis.saturating_add(LOCK_CLIFF_SECS).to_le_bytes());
    out.extend_from_slice(&genesis.saturating_add(LOCK_END_SECS).to_le_bytes());
    out.extend_from_slice(&read_u64_or_zero(KEY_POS_ACTIVATED_TS)?.to_le_bytes());
    Ok(out)
}

/// Admin: mark (`flag = 1`) or unmark (`flag = 0`) a DID whose PoA-phase
/// credits are locked. Marking has no effect after END_POA.
fn op_set_locked_recipient(input: &[u8], cursor: &mut usize) -> Result<Vec<u8>, ContractError> {
    require_initialized()?;
    require_admin()?;
    let did_hash = read_did_hash(input, cursor)?;
    let flag = read_u8(input, cursor)?;
    if did_hash == ZERO_HASH || flag > 1 {
        return Err(ContractError::InvalidInput);
    }
    storage_save(&lock_flag_key(&did_hash), &[flag])?;
    let mut payload = Vec::with_capacity(33);
    payload.extend_from_slice(&did_hash);
    payload.push(flag);
    emit_event_bytes("astra_rewards.lock_recipient", &payload);
    Ok(Vec::new())
}

/// Admin: end proof of authority. Irreversible; later credits are unlocked.
fn op_end_poa() -> Result<Vec<u8>, ContractError> {
    require_initialized()?;
    require_admin()?;
    if read_phase()? == PHASE_POS {
        return Ok(Vec::new());
    }
    let now = block_timestamp();
    storage_save(KEY_PHASE, &[PHASE_POS])?;
    write_u64(KEY_POS_ACTIVATED_TS, now)?;
    emit_event_bytes("astra_rewards.poa_ended", &now.to_le_bytes());
    Ok(Vec::new())
}

/// Anyone: release a DID's vested locked ASTRA into its spendable balance.
fn op_release(input: &[u8], cursor: &mut usize) -> Result<Vec<u8>, ContractError> {
    require_initialized()?;
    let did_hash = read_did_hash(input, cursor)?;
    let released = release_vested(&did_hash)?;
    Ok(released.to_le_bytes().to_vec())
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

fn locked_key(did_hash: &[u8; 32]) -> String {
    format!("{}{}", KEY_PREFIX_LOCKED, hex_encode(did_hash))
}

fn released_key(did_hash: &[u8; 32]) -> String {
    format!("{}{}", KEY_PREFIX_RELEASED, hex_encode(did_hash))
}

fn lock_flag_key(did_hash: &[u8; 32]) -> String {
    format!("{}{}", KEY_PREFIX_LOCK_FLAG, hex_encode(did_hash))
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

// Host tests below mock the SDK imports: `cargo test --lib` from this directory.
// Build for deployment: `cargo build --release --target wasm32-unknown-unknown`.

// ============================================================================
// Host tests: the SDK's host imports are provided by an in-memory mock.
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    const YEAR: u64 = 365 * 86_400;
    const GENESIS: u64 = 1_800_000_000;
    const ASTRA: u128 = 1_000_000_000_000_000_000;

    struct Host {
        storage: HashMap<Vec<u8>, Vec<u8>>,
        caller: String,
        now: u64,
        events: Vec<String>,
    }

    thread_local! {
        static HOST: RefCell<Host> = RefCell::new(Host {
            storage: HashMap::new(),
            caller: String::new(),
            now: 0,
            events: Vec::new(),
        });
    }

    #[no_mangle]
    extern "C" fn storage_save(kp: *const u8, kl: usize, vp: *const u8, vl: usize) -> i32 {
        let (k, v) = unsafe { (std::slice::from_raw_parts(kp, kl).to_vec(), std::slice::from_raw_parts(vp, vl).to_vec()) };
        HOST.with(|h| h.borrow_mut().storage.insert(k, v));
        0
    }

    #[no_mangle]
    extern "C" fn storage_load(kp: *const u8, kl: usize, out: *mut u8, max: usize) -> i32 {
        let k = unsafe { std::slice::from_raw_parts(kp, kl) };
        HOST.with(|h| match h.borrow().storage.get(k) {
            Some(v) => {
                let n = v.len().min(max);
                unsafe { std::ptr::copy_nonoverlapping(v.as_ptr(), out, n) };
                n as i32
            }
            None => -1,
        })
    }

    #[no_mangle]
    extern "C" fn get_caller_did(out: *mut u8, max: usize) -> i32 {
        HOST.with(|h| {
            let c = h.borrow().caller.clone();
            let n = c.len().min(max);
            unsafe { std::ptr::copy_nonoverlapping(c.as_ptr(), out, n) };
            n as i32
        })
    }

    #[no_mangle]
    extern "C" fn get_timestamp() -> i64 {
        HOST.with(|h| h.borrow().now as i64)
    }

    #[no_mangle]
    extern "C" fn emit_event(tp: *const u8, tl: usize, _dp: *const u8, _dl: usize) {
        let t = unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts(tp, tl)) }.to_string();
        HOST.with(|h| h.borrow_mut().events.push(t));
    }

    fn as_caller(did: &str) {
        HOST.with(|h| h.borrow_mut().caller = did.to_string());
    }
    fn at(now: u64) {
        HOST.with(|h| h.borrow_mut().now = now);
    }
    fn events() -> Vec<String> {
        HOST.with(|h| h.borrow().events.clone())
    }

    fn call(input: Vec<u8>) -> Result<Vec<u8>, ContractError> {
        AstraRewards.handle(&input)
    }
    fn u128_at(bytes: &[u8], i: usize) -> u128 {
        u128::from_le_bytes(bytes[i * 16..i * 16 + 16].try_into().unwrap())
    }

    const ADMIN: &str = "did:spacekit:network:sra";
    fn h(did: &str) -> [u8; 32] {
        spacekit_contract_sdk::hash_did_bytes(did.as_bytes())
    }

    fn init() {
        as_caller(ADMIN);
        at(GENESIS);
        let mut i = vec![OP_INIT];
        i.extend_from_slice(&h("did:spacekit:network:treasury"));
        call(i).unwrap();
    }
    fn credit(to: &str, amount: u128) -> Result<Vec<u8>, ContractError> {
        as_caller(ADMIN);
        let mut i = vec![OP_CREDIT];
        i.extend_from_slice(&h(to));
        i.extend_from_slice(&amount.to_le_bytes());
        i.extend_from_slice(&[7u8; 32]);
        call(i)
    }
    fn mark(did: &str, flag: u8) -> Result<Vec<u8>, ContractError> {
        let mut i = vec![OP_SET_LOCKED_RECIPIENT];
        i.extend_from_slice(&h(did));
        i.push(flag);
        call(i)
    }
    fn balance(did: &str) -> u128 {
        let mut i = vec![OP_GET_BALANCE];
        i.extend_from_slice(&h(did));
        u128_at(&call(i).unwrap(), 0)
    }
    fn locked(did: &str) -> (u128, u128, u128) {
        let mut i = vec![OP_GET_LOCKED];
        i.extend_from_slice(&h(did));
        let r = call(i).unwrap();
        (u128_at(&r, 0), u128_at(&r, 1), u128_at(&r, 2))
    }
    fn withdraw(from: &str, to: &str, amount: u128) -> Result<Vec<u8>, ContractError> {
        as_caller(from);
        let mut i = vec![OP_WITHDRAW];
        i.extend_from_slice(&h(to));
        i.extend_from_slice(&amount.to_le_bytes());
        call(i)
    }
    fn total_emitted() -> u128 {
        u128_at(&call(vec![OP_GET_TOTAL_EMITTED]).unwrap(), 0)
    }

    #[test]
    fn vesting_curve() {
        let l = 1_200 * ASTRA;
        assert_eq!(vested_amount(l, GENESIS, GENESIS), 0);
        assert_eq!(vested_amount(l, GENESIS, GENESIS + YEAR - 1), 0);
        assert_eq!(vested_amount(l, GENESIS, GENESIS + YEAR), 0);
        assert_eq!(vested_amount(l, GENESIS, GENESIS + 2 * YEAR), l / 2);
        assert_eq!(vested_amount(l, GENESIS, GENESIS + 3 * YEAR), l);
        assert_eq!(vested_amount(l, GENESIS, GENESIS + 10 * YEAR), l);
        assert_eq!(vested_amount(2_000_000_000 * ASTRA, GENESIS, GENESIS + 3 * YEAR - 1) > 0, true);
    }

    #[test]
    fn poa_credits_to_authorities_are_locked_and_vest() {
        init();
        let authority = "did:spacekit:testnet:aaaa";
        let independent = "did:spacekit:testnet:bbbb";
        as_caller(ADMIN);
        mark(authority, 1).unwrap();

        credit(authority, 900 * ASTRA).unwrap();
        credit(independent, 100 * ASTRA).unwrap();
        assert_eq!(balance(authority), 0);
        assert_eq!(locked(authority), (900 * ASTRA, 0, 0));
        assert_eq!(balance(independent), 100 * ASTRA);
        // Locked credits still count against the cap.
        assert_eq!(total_emitted(), 350_000_000 * ASTRA + 1_000 * ASTRA);
        assert!(events().iter().any(|e| e == "astra_rewards.credit_locked"));

        // Nothing can move before the cliff.
        at(GENESIS + YEAR / 2);
        assert!(matches!(withdraw(authority, independent, 1).unwrap_err(), ContractError::InsufficientBalance));

        // Halfway through vesting: half is releasable, and WITHDRAW releases it.
        at(GENESIS + 2 * YEAR);
        assert_eq!(locked(authority), (900 * ASTRA, 0, 450 * ASTRA));
        withdraw(authority, independent, 400 * ASTRA).unwrap();
        assert_eq!(balance(authority), 50 * ASTRA);
        assert_eq!(locked(authority), (900 * ASTRA, 450 * ASTRA, 0));
        assert!(withdraw(authority, independent, 100 * ASTRA).is_err());

        // Anyone can RELEASE after full vesting.
        at(GENESIS + 3 * YEAR);
        as_caller(independent);
        let mut i = vec![OP_RELEASE];
        i.extend_from_slice(&h(authority));
        assert_eq!(u128_at(&call(i).unwrap(), 0), 450 * ASTRA);
        assert_eq!(balance(authority), 500 * ASTRA);
        assert_eq!(locked(authority), (900 * ASTRA, 900 * ASTRA, 0));
    }

    #[test]
    fn end_poa_stops_new_locks_but_keeps_schedule() {
        init();
        let authority = "did:spacekit:testnet:cccc";
        as_caller(ADMIN);
        mark(authority, 1).unwrap();
        credit(authority, 100 * ASTRA).unwrap();

        at(GENESIS + 30 * 86_400);
        as_caller(ADMIN);
        call(vec![OP_END_POA]).unwrap();
        let phase = call(vec![OP_GET_PHASE]).unwrap();
        assert_eq!(phase[0], PHASE_POS);
        assert_eq!(u64::from_le_bytes(phase[25..33].try_into().unwrap()), GENESIS + 30 * 86_400);

        credit(authority, 50 * ASTRA).unwrap();
        assert_eq!(balance(authority), 50 * ASTRA);
        assert_eq!(locked(authority).0, 100 * ASTRA);
        // Still locked until the cliff.
        at(GENESIS + YEAR - 1);
        assert_eq!(locked(authority).2, 0);
    }

    #[test]
    fn lock_admin_ops_require_admin() {
        init();
        as_caller("did:spacekit:testnet:mallory");
        assert!(matches!(mark("did:spacekit:testnet:x", 1).unwrap_err(), ContractError::Unauthorized));
        assert!(matches!(call(vec![OP_END_POA]).unwrap_err(), ContractError::Unauthorized));
        as_caller(ADMIN);
        assert!(matches!(mark("did:spacekit:testnet:x", 2).unwrap_err(), ContractError::InvalidInput));
    }

    #[test]
    fn unmarked_or_legacy_contracts_credit_normally() {
        init();
        // Unmarking restores normal credits during PoA.
        let d = "did:spacekit:testnet:dddd";
        as_caller(ADMIN);
        mark(d, 1).unwrap();
        mark(d, 0).unwrap();
        credit(d, 5 * ASTRA).unwrap();
        assert_eq!(balance(d), 5 * ASTRA);

        // A contract initialized before the lock existed has no phase key.
        HOST.with(|h| {
            h.borrow_mut().storage.remove(KEY_PHASE.as_bytes());
        });
        mark(d, 1).unwrap();
        credit(d, 5 * ASTRA).unwrap();
        assert_eq!(balance(d), 10 * ASTRA);
    }
}
