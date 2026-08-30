//! SpaceKit Treasury Contract
//!
//! Holds the project treasury — a **pre-funded** ASTRA pool — and disburses it
//! only under **M-of-N governance approval**. It has **no mint authority**: the
//! pool can never grow except by an explicit governance-recorded deposit, and
//! ASTRA is never created here. This is exactly what lets "the system, not the
//! treasury, awards" (see `docs/PROOF_OF_TANGIBLE_WORKS.md`): minting lives in
//! `AstraRewards`; the treasury only moves what it already holds.
//!
//! # Governance model (on-chain multisig)
//!
//! A disbursement is a two-phase multisig:
//!   1. a signer `PROPOSE`s `(spend_id, recipient, amount, memo)` — the proposer
//!      auto-approves;
//!   2. other signers `APPROVE(spend_id)` until approvals reach the threshold
//!      `M`, at which point the spend executes: the pool is debited and a
//!      `treasury.disbursed` event is emitted.
//!
//! The `treasury.disbursed` event is the authoritative disbursement instruction
//! that the host bridge / an `AstraRewards` transfer from the treasury DID acts
//! on to move the real ASTRA — the same host-orchestrated pattern SRA uses.
//!
//! # Wire format (single-byte opcode + payload; all integers little-endian)
//!
//! | Op | Opcode | Payload |
//! |----|--------|---------|
//! | INIT         | 0x01 | [balance 16][threshold 8][signer_count 8][signer 32]×count |
//! | PROPOSE      | 0x10 | [spend_id 32][recipient 32][amount 16][memo 32] |
//! | APPROVE      | 0x11 | [spend_id 32] |
//! | DEPOSIT      | 0x20 | [amount 16]  (signer-only; records a host-bridged inflow) |
//! | GET_BALANCE  | 0x30 | (empty) → [balance 16] |
//! | GET_PROPOSAL | 0x31 | [spend_id 32] → [recipient 32][amount 16][memo 32][approvals 8][executed 1] |
//! | GET_CONFIG   | 0x32 | (empty) → [threshold 8][signer_count 8] |

#![no_std]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use spacekit_contract_sdk::{
    emit_event_bytes, get_caller_did_hash,
    spacekit_contract,
    spacekit_storage::{storage_load, storage_save},
    wire::{read_u64, read_u8},
    ContractError, SpacekitContract,
};

#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

// ── Opcodes ────────────────────────────────────────────────────────────────
const OP_INIT: u8 = 0x01;
const OP_PROPOSE: u8 = 0x10;
const OP_APPROVE: u8 = 0x11;
const OP_DEPOSIT: u8 = 0x20;
const OP_GET_BALANCE: u8 = 0x30;
const OP_GET_PROPOSAL: u8 = 0x31;
const OP_GET_CONFIG: u8 = 0x32;

const ZERO_HASH: [u8; 32] = [0u8; 32];

// ── Storage keys ─────────────────────────────────────────────────────────────
const KEY_INIT: &str = "treasury.initialized";
const KEY_BALANCE: &str = "treasury.balance";
const KEY_THRESHOLD: &str = "treasury.threshold";
const KEY_SIGNER_COUNT: &str = "treasury.signer_count";
const KEY_PREFIX_SIGNER: &str = "treasury.signer."; // + index
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
        match opcode {
            OP_INIT => op_init(input, &mut cursor),
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

// ── INIT ─────────────────────────────────────────────────────────────────────
fn op_init(input: &[u8], cursor: &mut usize) -> Result<Vec<u8>, ContractError> {
    if storage_load(KEY_INIT).is_ok() {
        return Err(ContractError::AlreadyInitialized);
    }
    let balance = read_u128(input, cursor)?;
    let threshold = read_u64(input, cursor)?;
    let signer_count = read_u64(input, cursor)?;

    if signer_count == 0 || threshold == 0 || threshold > signer_count {
        return Err(ContractError::InvalidInput);
    }

    // Read and register the signer set.
    for i in 0..signer_count {
        let signer = read_did_hash(input, cursor)?;
        if signer == ZERO_HASH {
            return Err(ContractError::InvalidInput);
        }
        storage_save(&signer_index_key(i), &signer)?;
        storage_save(&is_signer_key(&signer), &[1u8])?;
    }

    write_u128(KEY_BALANCE, balance)?;
    write_u64(KEY_THRESHOLD, threshold)?;
    write_u64(KEY_SIGNER_COUNT, signer_count)?;
    storage_save(KEY_INIT, &[1u8])?;

    let mut payload = Vec::with_capacity(32);
    payload.extend_from_slice(&balance.to_le_bytes());
    payload.extend_from_slice(&threshold.to_le_bytes());
    payload.extend_from_slice(&signer_count.to_le_bytes());
    emit_event_bytes("treasury.initialized", &payload);
    Ok(Vec::new())
}

// ── PROPOSE ──────────────────────────────────────────────────────────────────
fn op_propose(input: &[u8], cursor: &mut usize) -> Result<Vec<u8>, ContractError> {
    require_initialized()?;
    let caller = get_caller_did_hash()?;
    require_signer(&caller)?;

    let spend_id = read_did_hash(input, cursor)?;
    let recipient = read_did_hash(input, cursor)?;
    let amount = read_u128(input, cursor)?;
    let memo = read_did_hash(input, cursor)?;

    if amount == 0 || recipient == ZERO_HASH || spend_id == ZERO_HASH {
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

    let balance = read_u128_or_zero(KEY_BALANCE)?;
    if balance < prop.amount {
        return Err(ContractError::InsufficientBalance);
    }
    write_u128(KEY_BALANCE, balance - prop.amount)?;
    prop.executed = 1;
    write_proposal(spend_id, &prop)?;

    let mut payload = Vec::with_capacity(96);
    payload.extend_from_slice(spend_id);
    payload.extend_from_slice(&prop.recipient);
    payload.extend_from_slice(&prop.amount.to_le_bytes());
    payload.extend_from_slice(&prop.memo);
    emit_event_bytes("treasury.disbursed", &payload);
    Ok(())
}

// ── DEPOSIT (signer-only: record a host-bridged inflow) ──────────────────────
fn op_deposit(input: &[u8], cursor: &mut usize) -> Result<Vec<u8>, ContractError> {
    require_initialized()?;
    let caller = get_caller_did_hash()?;
    require_signer(&caller)?;

    let amount = read_u128(input, cursor)?;
    if amount == 0 {
        return Err(ContractError::InvalidInput);
    }
    let balance = read_u128_or_zero(KEY_BALANCE)?;
    let new_balance = balance.checked_add(amount).ok_or(ContractError::InvalidInput)?;
    write_u128(KEY_BALANCE, new_balance)?;

    let mut payload = Vec::with_capacity(48);
    payload.extend_from_slice(&amount.to_le_bytes());
    payload.extend_from_slice(&new_balance.to_le_bytes());
    payload.extend_from_slice(&caller);
    emit_event_bytes("treasury.deposited", &payload);
    Ok(new_balance.to_le_bytes().to_vec())
}

// ── Reads ────────────────────────────────────────────────────────────────────
fn op_get_balance() -> Result<Vec<u8>, ContractError> {
    Ok(read_u128_or_zero(KEY_BALANCE)?.to_le_bytes().to_vec())
}

fn op_get_proposal(input: &[u8], cursor: &mut usize) -> Result<Vec<u8>, ContractError> {
    let spend_id = read_did_hash(input, cursor)?;
    let prop = read_proposal(&spend_id)?;
    let mut out = Vec::with_capacity(89);
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
    recipient: [u8; 32],
    amount: u128,
    memo: [u8; 32],
    approvals: u64,
    executed: u8,
}

fn write_proposal(spend_id: &[u8; 32], p: &Proposal) -> Result<(), ContractError> {
    let mut buf = Vec::with_capacity(89);
    buf.extend_from_slice(&p.recipient);
    buf.extend_from_slice(&p.amount.to_le_bytes());
    buf.extend_from_slice(&p.memo);
    buf.extend_from_slice(&p.approvals.to_le_bytes());
    buf.push(p.executed);
    storage_save(&proposal_key(spend_id), &buf)
}

fn read_proposal(spend_id: &[u8; 32]) -> Result<Proposal, ContractError> {
    let bytes = storage_load(&proposal_key(spend_id)).map_err(|_| ContractError::InvalidInput)?;
    if bytes.len() < 89 {
        return Err(ContractError::StorageError);
    }
    let mut recipient = [0u8; 32];
    recipient.copy_from_slice(&bytes[0..32]);
    let mut amt = [0u8; 16];
    amt.copy_from_slice(&bytes[32..48]);
    let mut memo = [0u8; 32];
    memo.copy_from_slice(&bytes[48..80]);
    let mut appr = [0u8; 8];
    appr.copy_from_slice(&bytes[80..88]);
    Ok(Proposal {
        recipient,
        amount: u128::from_le_bytes(amt),
        memo,
        approvals: u64::from_le_bytes(appr),
        executed: bytes[88],
    })
}

// ── Key builders ─────────────────────────────────────────────────────────────
fn signer_index_key(i: u64) -> String {
    format!("{}{}", KEY_PREFIX_SIGNER, i)
}
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
fn read_u128_or_zero(key: &str) -> Result<u128, ContractError> {
    match storage_load(key) {
        Ok(bytes) => {
            if bytes.len() < 16 {
                return Err(ContractError::StorageError);
            }
            let mut a = [0u8; 16];
            a.copy_from_slice(&bytes[..16]);
            Ok(u128::from_le_bytes(a))
        }
        Err(_) => Ok(0),
    }
}
fn write_u128(key: &str, v: u128) -> Result<(), ContractError> {
    storage_save(key, &v.to_le_bytes())
}
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
fn write_u64(key: &str, v: u64) -> Result<(), ContractError> {
    storage_save(key, &v.to_le_bytes())
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
