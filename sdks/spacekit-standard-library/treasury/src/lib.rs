//! SpaceKit Treasury Contract
//!
//! Holds the project treasury and disburses it only under **M-of-N
//! governance approval**. The treasury's funds are the **native ASTRA balance
//! of this contract's address** (the system address `0x…0004`) — the one
//! ledger every account uses. There is no internal balance to keep in step
//! with anything, and the contract has no mint authority: the balance grows
//! only when someone sends ASTRA to it (the genesis allocation is minted to
//! this address by the rewards system call INIT).
//!
//! # Governance model (on-chain multisig)
//!
//! 1. a signer `PROPOSE`s `(spend_id, recipient, amount, memo)` — the proposer
//!    auto-approves;
//! 2. other signers `APPROVE(spend_id)` until approvals reach the threshold
//!    `M`; the spend then executes in the same transaction: the contract pays
//!    `amount` from its own balance to `recipient` and emits
//!    `treasury.disbursed`.
//!
//! The signer set and threshold are written by the chain at genesis (PoA
//! genesis `treasury` section); there is no INIT operation anyone could race.
//!
//! # Wire format (single-byte opcode + payload; all integers little-endian)
//!
//! | Op | Opcode | Payload |
//! |----|--------|---------|
//! | PROPOSE      | 0x10 | [spend_id 32][recipient 20][amount 16][memo 32] |
//! | APPROVE      | 0x11 | [spend_id 32] |
//! | DEPOSIT      | 0x20 | (empty; anyone; the attached value is the deposit) |
//! | GET_BALANCE  | 0x30 | (empty) → [balance 16] |
//! | GET_PROPOSAL | 0x31 | [spend_id 32] → [recipient 20][amount 16][memo 32][approvals 8][executed 1] |
//! | GET_CONFIG   | 0x32 | (empty) → [threshold 8][signer_count 8] |
//!
//! Value attached to any operation other than DEPOSIT is refused.

#![no_std]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use spacekit_contract_sdk::{
    emit_event_bytes, get_caller_did_hash,
    spacekit_contract,
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

#[link(wasm_import_module = "env")]
extern "C" {
    fn msg_value_u128(out_ptr: *mut u8) -> i32;
    fn get_balance_u128(address_ptr: *const u8, out_ptr: *mut u8) -> i32;
    fn transfer_u128(to_ptr: *const u8, amount_ptr: *const u8) -> i32;
}

/// This contract's own address (system contract `0x…0004`).
const SELF_ADDRESS: [u8; 20] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4];

// ── Opcodes ────────────────────────────────────────────────────────────────
const OP_PROPOSE: u8 = 0x10;
const OP_APPROVE: u8 = 0x11;
const OP_DEPOSIT: u8 = 0x20;
const OP_GET_BALANCE: u8 = 0x30;
const OP_GET_PROPOSAL: u8 = 0x31;
const OP_GET_CONFIG: u8 = 0x32;

const ZERO_HASH: [u8; 32] = [0u8; 32];
const ZERO_ADDR: [u8; 20] = [0u8; 20];

// ── Storage keys (written at genesis by the chain; see chain_consensus) ─────
const KEY_INIT: &str = "treasury.initialized";
const KEY_THRESHOLD: &str = "treasury.threshold";
const KEY_SIGNER_COUNT: &str = "treasury.signer_count";
const KEY_PREFIX_IS_SIGNER: &str = "treasury.is_signer."; // + hex(hash)
const KEY_PREFIX_PROPOSAL: &str = "treasury.proposal."; // + hex(spend_id)
const KEY_PREFIX_APPROVED: &str = "treasury.approved."; // + hex(spend_id).hex(signer)

// ── Contract ────────────────────────────────────────────────────────────────
struct Treasury;

impl SpacekitContract for Treasury {
    type Error = ContractError;

    fn init() -> Self {
        Treasury
    }

    fn handle(&mut self, input: &[u8]) -> Result<Vec<u8>, ContractError> {
        if input.is_empty() {
            return Err(ContractError::InvalidInput);
        }
        let mut cursor = 0usize;
        let opcode = read_u8(input, &mut cursor)?;
        if opcode != OP_DEPOSIT && attached_value() != 0 {
            return Err(ContractError::InvalidInput);
        }
        match opcode {
            OP_PROPOSE => op_propose(input, &mut cursor),
            OP_APPROVE => op_approve(input, &mut cursor),
            OP_DEPOSIT => op_deposit(input, &mut cursor),
            OP_GET_BALANCE => op_get_balance(),
            OP_GET_PROPOSAL => op_get_proposal(input, &mut cursor),
            OP_GET_CONFIG => op_get_config(),
            _ => Err(ContractError::InvalidInput),
        }
    }
}

#[cfg(not(test))]
spacekit_contract!(Treasury);

// ── PROPOSE ──────────────────────────────────────────────────────────────────
fn op_propose(input: &[u8], cursor: &mut usize) -> Result<Vec<u8>, ContractError> {
    require_initialized()?;
    let caller = get_caller_did_hash()?;
    require_signer(&caller)?;

    let spend_id = read_did_hash(input, cursor)?;
    let recipient = read_address20(input, cursor)?;
    let amount = read_u128(input, cursor)?;
    let memo = read_did_hash(input, cursor)?;

    if amount == 0 || recipient == ZERO_ADDR || spend_id == ZERO_HASH {
        return Err(ContractError::InvalidInput);
    }
    if storage_load(&proposal_key(&spend_id)).is_ok() {
        return Err(ContractError::InvalidInput); // spend_id already used
    }

    // Record proposal with the proposer's own approval already counted.
    let prop = Proposal {
        recipient,
        amount,
        memo,
        approvals: 1,
        executed: 0,
    };
    write_proposal(&spend_id, &prop)?;
    storage_save(&approved_key(&spend_id, &caller), &[1u8])?;

    let mut payload = Vec::with_capacity(112);
    payload.extend_from_slice(&spend_id);
    payload.extend_from_slice(&recipient);
    payload.extend_from_slice(&amount.to_le_bytes());
    payload.extend_from_slice(&caller);
    emit_event_bytes("treasury.proposed", &payload);

    // A threshold of 1 executes immediately.
    try_execute(&spend_id)?;
    Ok(Vec::new())
}

// ── APPROVE ──────────────────────────────────────────────────────────────────
fn op_approve(input: &[u8], cursor: &mut usize) -> Result<Vec<u8>, ContractError> {
    require_initialized()?;
    let caller = get_caller_did_hash()?;
    require_signer(&caller)?;

    let spend_id = read_did_hash(input, cursor)?;
    let mut prop = read_proposal(&spend_id)?;
    if prop.executed != 0 {
        return Err(ContractError::InvalidInput); // already disbursed
    }

    // Count each signer at most once.
    if storage_load(&approved_key(&spend_id, &caller)).is_err() {
        storage_save(&approved_key(&spend_id, &caller), &[1u8])?;
        prop.approvals = prop.approvals.checked_add(1).ok_or(ContractError::InvalidInput)?;
        write_proposal(&spend_id, &prop)?;
    }

    try_execute(&spend_id)?;
    Ok(prop.approvals.to_le_bytes().to_vec())
}

/// Execute a proposal once approvals reach the threshold: debit the pool and
/// emit the authoritative disbursement instruction. Idempotent — a no-op if the
/// threshold is not met or the proposal is already executed.
fn try_execute(spend_id: &[u8; 32]) -> Result<(), ContractError> {
    let mut prop = read_proposal(spend_id)?;
    if prop.executed != 0 {
        return Ok(());
    }
    let threshold = read_u64_or_zero(KEY_THRESHOLD)?;
    if (prop.approvals as u64) < threshold {
        return Ok(());
    }

    if own_balance() < prop.amount {
        return Err(ContractError::InsufficientBalance);
    }
    prop.executed = 1;
    write_proposal(spend_id, &prop)?;
    let amount = prop.amount.to_le_bytes();
    if unsafe { transfer_u128(prop.recipient.as_ptr(), amount.as_ptr()) } != 0 {
        return Err(ContractError::InsufficientBalance);
    }

    // Disbursed payload: spend_id(32)+recipient(20)+amount(16)+memo(32) = 100 bytes.
    let mut payload = Vec::with_capacity(100);
    payload.extend_from_slice(spend_id);
    payload.extend_from_slice(&prop.recipient);
    payload.extend_from_slice(&prop.amount.to_le_bytes());
    payload.extend_from_slice(&prop.memo);
    emit_event_bytes("treasury.disbursed", &payload);
    Ok(())
}

// ── DEPOSIT (anyone: the attached value is already in this contract) ──────────
fn op_deposit(_input: &[u8], _cursor: &mut usize) -> Result<Vec<u8>, ContractError> {
    require_initialized()?;
    let amount = attached_value();
    if amount == 0 {
        return Err(ContractError::InvalidInput);
    }
    let balance = own_balance();
    let mut payload = Vec::with_capacity(32);
    payload.extend_from_slice(&amount.to_le_bytes());
    payload.extend_from_slice(&balance.to_le_bytes());
    emit_event_bytes("treasury.deposited", &payload);
    Ok(balance.to_le_bytes().to_vec())
}

fn attached_value() -> u128 {
    let mut out = [0u8; 16];
    if unsafe { msg_value_u128(out.as_mut_ptr()) } != 0 {
        return 0;
    }
    u128::from_le_bytes(out)
}

fn own_balance() -> u128 {
    let mut out = [0u8; 16];
    if unsafe { get_balance_u128(SELF_ADDRESS.as_ptr(), out.as_mut_ptr()) } != 0 {
        return 0;
    }
    u128::from_le_bytes(out)
}

// ── Reads ────────────────────────────────────────────────────────────────────
fn op_get_balance() -> Result<Vec<u8>, ContractError> {
    Ok(own_balance().to_le_bytes().to_vec())
}

fn op_get_proposal(input: &[u8], cursor: &mut usize) -> Result<Vec<u8>, ContractError> {
    let spend_id = read_did_hash(input, cursor)?;
    let prop = read_proposal(&spend_id)?;
    let mut out = Vec::with_capacity(PROPOSAL_RECORD_LEN);
    out.extend_from_slice(&prop.recipient);
    out.extend_from_slice(&prop.amount.to_le_bytes());
    out.extend_from_slice(&prop.memo);
    out.extend_from_slice(&prop.approvals.to_le_bytes());
    out.push(prop.executed);
    Ok(out)
}

fn op_get_config() -> Result<Vec<u8>, ContractError> {
    let threshold = read_u64_or_zero(KEY_THRESHOLD)?;
    let count = read_u64_or_zero(KEY_SIGNER_COUNT)?;
    let mut out = Vec::with_capacity(16);
    out.extend_from_slice(&threshold.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    Ok(out)
}

// ── Authorization ────────────────────────────────────────────────────────────
fn require_initialized() -> Result<(), ContractError> {
    if storage_load(KEY_INIT).is_err() {
        return Err(ContractError::NotInitialized);
    }
    Ok(())
}

fn require_signer(caller: &[u8; 32]) -> Result<(), ContractError> {
    if storage_load(&is_signer_key(caller)).is_err() {
        return Err(ContractError::Unauthorized);
    }
    Ok(())
}

// ── Proposal record ──────────────────────────────────────────────────────────
struct Proposal {
    recipient: [u8; 20],
    amount: u128,
    memo: [u8; 32],
    approvals: u64,
    executed: u8,
}

// Proposal record: recipient(20)+amount(16)+memo(32)+approvals(8)+executed(1) = 77 bytes.
const PROPOSAL_RECORD_LEN: usize = 77;

fn write_proposal(spend_id: &[u8; 32], p: &Proposal) -> Result<(), ContractError> {
    let mut buf = Vec::with_capacity(PROPOSAL_RECORD_LEN);
    buf.extend_from_slice(&p.recipient);
    buf.extend_from_slice(&p.amount.to_le_bytes());
    buf.extend_from_slice(&p.memo);
    buf.extend_from_slice(&p.approvals.to_le_bytes());
    buf.push(p.executed);
    storage_save(&proposal_key(spend_id), &buf)
}

fn read_proposal(spend_id: &[u8; 32]) -> Result<Proposal, ContractError> {
    let bytes = storage_load(&proposal_key(spend_id)).map_err(|_| ContractError::InvalidInput)?;
    if bytes.len() < PROPOSAL_RECORD_LEN {
        return Err(ContractError::StorageError);
    }
    let mut recipient = [0u8; 20];
    recipient.copy_from_slice(&bytes[0..20]);
    let mut amt = [0u8; 16];
    amt.copy_from_slice(&bytes[20..36]);
    let mut memo = [0u8; 32];
    memo.copy_from_slice(&bytes[36..68]);
    let mut appr = [0u8; 8];
    appr.copy_from_slice(&bytes[68..76]);
    Ok(Proposal {
        recipient,
        amount: u128::from_le_bytes(amt),
        memo,
        approvals: u64::from_le_bytes(appr),
        executed: bytes[76],
    })
}

// ── Key builders ─────────────────────────────────────────────────────────────
fn is_signer_key(h: &[u8; 32]) -> String {
    format!("{}{}", KEY_PREFIX_IS_SIGNER, hex_encode(h))
}
fn proposal_key(id: &[u8; 32]) -> String {
    format!("{}{}", KEY_PREFIX_PROPOSAL, hex_encode(id))
}
fn approved_key(id: &[u8; 32], signer: &[u8; 32]) -> String {
    format!("{}{}.{}", KEY_PREFIX_APPROVED, hex_encode(id), hex_encode(signer))
}

// ── Integer + wire helpers (mirrors AstraRewards) ────────────────────────────
fn read_u64_or_zero(key: &str) -> Result<u64, ContractError> {
    match storage_load(key) {
        Ok(bytes) => {
            if bytes.len() < 8 {
                return Err(ContractError::StorageError);
            }
            let mut a = [0u8; 8];
            a.copy_from_slice(&bytes[..8]);
            Ok(u64::from_le_bytes(a))
        }
        Err(_) => Ok(0),
    }
}

fn read_did_hash(input: &[u8], cursor: &mut usize) -> Result<[u8; 32], ContractError> {
    if input.len() < *cursor + 32 {
        return Err(ContractError::InvalidInput);
    }
    let mut a = [0u8; 32];
    a.copy_from_slice(&input[*cursor..*cursor + 32]);
    *cursor += 32;
    Ok(a)
}

fn read_address20(input: &[u8], cursor: &mut usize) -> Result<[u8; 20], ContractError> {
    if input.len() < *cursor + 20 {
        return Err(ContractError::InvalidInput);
    }
    let mut a = [0u8; 20];
    a.copy_from_slice(&input[*cursor..*cursor + 20]);
    *cursor += 20;
    Ok(a)
}

fn read_u128(input: &[u8], cursor: &mut usize) -> Result<u128, ContractError> {
    if input.len() < *cursor + 16 {
        return Err(ContractError::InvalidInput);
    }
    let mut a = [0u8; 16];
    a.copy_from_slice(&input[*cursor..*cursor + 16]);
    *cursor += 16;
    Ok(u128::from_le_bytes(a))
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(hex_char(b >> 4));
        out.push(hex_char(b & 0xF));
    }
    out
}
fn hex_char(n: u8) -> char {
    match n {
        0..=9 => (b'0' + n) as char,
        10..=15 => (b'a' + (n - 10)) as char,
        _ => '?',
    }
}
