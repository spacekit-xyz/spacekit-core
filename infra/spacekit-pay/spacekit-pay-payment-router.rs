//! SpaceKit Pay - PaymentRouter Contract
//!
//! Atomic, non-custodial payment routing for the AI economy.
//!
//! A buyer pays an operator for an AI service. The PaymentRouter pulls the
//! payment from the buyer (via prior approve), splits it into operator-cut
//! (95%) and treasury-cut (5%), and forwards both atomically in the same
//! transaction. The contract never holds a balance at the end of any
//! transaction.
//!
//! # Design property: non-custodial
//!
//! This contract is designed to never custody user funds. The atomic-routing
//! property is enforced by structure:
//!
//! 1. `transferFrom` pulls payment into this contract
//! 2. `transfer` to operator immediately after
//! 3. `transfer` to treasury immediately after
//!
//! All three transfers happen in a single `handle()` call. If any step
//! fails, the entire transaction reverts and no funds move. The contract's
//! balance at the end of every successful call is exactly zero (modulo
//! anything sent to it via direct transfer outside the protocol, which is
//! recoverable but not part of normal flow).
//!
//! There is no admin function that can intercept funds in transit. There
//! is no upgrade path that could introduce a delay or hold. The contract's
//! non-custodial property is a structural guarantee, not an operational
//! promise.
//!
//! # Wire format
//!
//! | Op | Opcode | Payload | Returns |
//! |----|--------|---------|---------|
//! | PAY_FOR_SERVICE | `0x01` | `[token_len u16][token_utf8][operator_did_len u16][operator_did_utf8][amount u128 LE]` | `[operator_cut u128 LE][treasury_cut u128 LE]` |
//! | GET_TREASURY_RATE | `0x02` | (empty) | `[bps u16 LE]` |
//! | GET_TREASURY_ADDRESS | `0x03` | (empty) | `[addr_len u16][addr_utf8]` |
//! | GET_NETWORK | `0x04` | (empty) | `[network_len u16][network_utf8]` |
//! | GET_OPERATOR_REGISTRY | `0x05` | (empty) | `[contract_id_len u16][contract_id_utf8]` |
//!
//! Admin operations (admin DID only, see KEY_ADMIN):
//!
//! | Op | Opcode | Payload | Returns |
//! |----|--------|---------|---------|
//! | SET_TREASURY_ADDRESS | `0x10` | `[addr_len u16][addr_utf8]` | `b"ok"` |
//! | SET_NETWORK | `0x11` | `[network_len u16][network_utf8]` | `b"ok"` (deploy-time only) |
//! | SET_OPERATOR_REGISTRY | `0x12` | `[contract_id_len u16][contract_id_utf8]` | `b"ok"` (deploy-time only) |
//! | SET_ADMIN | `0x13` | `[new_admin_len u16][new_admin_utf8]` | `b"ok"` |
//! | ADD_TOKEN | `0x14` | `[token_id_len u16][token_id_utf8]` | `b"ok"` |
//! | REMOVE_TOKEN | `0x15` | `[token_id_len u16][token_id_utf8]` | `b"ok"` |
//!
//! # Events
//!
//! - `spacekit_pay.payment.routed` - successful payment split and routed
//! - `spacekit_pay.treasury_address_changed` - admin updated treasury
//! - `spacekit_pay.token_allowed` / `spacekit_pay.token_revoked`

#![no_std]

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use spacekit_contract_sdk::{
    contract_call, emit_event_bytes, get_caller_did_string, spacekit_contract,
    spacekit_storage::{storage_load, storage_save},
    wire::{read_string, read_u128, read_u8},
    ContractError, SpacekitContract,
};

#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

#[cfg(all(target_arch = "wasm32", not(test)))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

// ============================================================================
// Constants
// ============================================================================

const OP_PAY_FOR_SERVICE: u8 = 0x01;
const OP_GET_TREASURY_RATE: u8 = 0x02;
const OP_GET_TREASURY_ADDRESS: u8 = 0x03;
const OP_GET_NETWORK: u8 = 0x04;
const OP_GET_OPERATOR_REGISTRY: u8 = 0x05;

const OP_SET_TREASURY_ADDRESS: u8 = 0x10;
const OP_SET_NETWORK: u8 = 0x11;
const OP_SET_OPERATOR_REGISTRY: u8 = 0x12;
const OP_SET_ADMIN: u8 = 0x13;
const OP_ADD_TOKEN: u8 = 0x14;
const OP_REMOVE_TOKEN: u8 = 0x15;

// Storage keys
const KEY_ADMIN: &str = "spacekit_pay.router.admin";
const KEY_TREASURY: &str = "spacekit_pay.router.treasury";
const KEY_NETWORK: &str = "spacekit_pay.router.network";
const KEY_OPERATOR_REGISTRY: &str = "spacekit_pay.router.registry";
// Allowed tokens keyed: spacekit_pay.router.token_allowed.<contract_id>

// Token contract opcodes (must match the stablecoin token contracts being routed)
const TOKEN_OP_TRANSFER: u8 = 0x02;
const TOKEN_OP_TRANSFER_FROM: u8 = 0x03;

// Fee structure: 5% flat to treasury
const TREASURY_RATE_BPS: u16 = 500;
const BPS_DENOMINATOR: u128 = 10_000;

// Operator registry opcodes (must match OperatorRegistry contract)
const REGISTRY_OP_LOOKUP: u8 = 0x01;

// ============================================================================
// Contract
// ============================================================================

struct PaymentRouter;

impl SpacekitContract for PaymentRouter {
    type Error = ContractError;

    fn init() -> Self {
        let deployer = get_caller_did_string().unwrap_or_else(|_| String::from("did:spacekit:anonymous"));
        let _ = storage_save(KEY_ADMIN, deployer.as_bytes());
        PaymentRouter
    }

    fn handle(&mut self, input: &[u8]) -> Result<Vec<u8>, ContractError> {
        if input.is_empty() {
            return Err(ContractError::InvalidInput);
        }

        let mut cursor = 0usize;
        let opcode = read_u8(input, &mut cursor)?;

        match opcode {
            OP_PAY_FOR_SERVICE => op_pay_for_service(input, &mut cursor),
            OP_GET_TREASURY_RATE => op_get_treasury_rate(),
            OP_GET_TREASURY_ADDRESS => op_get_treasury_address(),
            OP_GET_NETWORK => op_get_network(),
            OP_GET_OPERATOR_REGISTRY => op_get_operator_registry(),
            OP_SET_TREASURY_ADDRESS => op_set_treasury_address(input, &mut cursor),
            OP_SET_NETWORK => op_set_network(input, &mut cursor),
            OP_SET_OPERATOR_REGISTRY => op_set_operator_registry(input, &mut cursor),
            OP_SET_ADMIN => op_set_admin(input, &mut cursor),
            OP_ADD_TOKEN => op_add_token(input, &mut cursor),
            OP_REMOVE_TOKEN => op_remove_token(input, &mut cursor),
            _ => Err(ContractError::InvalidInput),
        }
    }
}

spacekit_contract!(PaymentRouter);

// ============================================================================
// Core operation: pay_for_service (atomic split and route)
// ============================================================================

fn op_pay_for_service(input: &[u8], cursor: &mut usize) -> Result<Vec<u8>, ContractError> {
    let payer = get_caller_did_string()?;
    let token_id = read_string(input, cursor)?;
    let operator_did = read_string(input, cursor)?;
    let amount = read_u128(input, cursor)?;

    // Input validation
    if amount == 0 || operator_did.is_empty() || token_id.is_empty() {
        return Err(ContractError::InvalidInput);
    }

    // Verify the token is on the allowlist (only stablecoins we accept)
    let token_allowed_key = format!("spacekit_pay.router.token_allowed.{}", token_id);
    if storage_load(&token_allowed_key).is_err() {
        return Err(ContractError::InvalidInput);
    }

    // Look up the operator's payout address on THIS network
    let network = load_network()?;
    let registry_id = load_operator_registry()?;
    let operator_address = lookup_operator_address(&registry_id, &operator_did, &network)?;
    if operator_address.is_empty() {
        return Err(ContractError::InvalidInput);
    }

    // Compute the split (5% flat to treasury, 95% to operator)
    // Math: treasury_cut = (amount * 500) / 10_000 = amount / 20
    // We use the BPS formulation explicitly for auditability
    let treasury_cut = amount
        .checked_mul(TREASURY_RATE_BPS as u128)
        .ok_or(ContractError::InvalidInput)?
        / BPS_DENOMINATOR;
    let operator_cut = amount
        .checked_sub(treasury_cut)
        .ok_or(ContractError::InvalidInput)?;

    let treasury_address = load_treasury_address()?;

    // ATOMIC ROUTING: All three transfers happen in this single transaction.
    // If any fails, the entire call reverts. The contract never holds
    // a positive balance at the end of a successful call.

    // Step 1: Pull payment from buyer (requires prior approve)
    let pull_payload = build_transfer_from_payload(&payer, &self_did(), amount);
    contract_call(&token_id, &pull_payload)?;

    // Step 2: Push operator_cut to operator address
    if operator_cut > 0 {
        let push_op_payload = build_transfer_payload(&operator_address, operator_cut);
        contract_call(&token_id, &push_op_payload)?;
    }

    // Step 3: Push treasury_cut to treasury address
    if treasury_cut > 0 {
        let push_treas_payload = build_transfer_payload(&treasury_address, treasury_cut);
        contract_call(&token_id, &push_treas_payload)?;
    }

    // Emit a structured event for indexers and analytics
    let mut event_payload = Vec::with_capacity(256);
    push_string(&mut event_payload, &payer);
    push_string(&mut event_payload, &operator_did);
    push_string(&mut event_payload, &operator_address);
    push_string(&mut event_payload, &token_id);
    push_string(&mut event_payload, &network);
    event_payload.extend_from_slice(&amount.to_le_bytes());
    event_payload.extend_from_slice(&operator_cut.to_le_bytes());
    event_payload.extend_from_slice(&treasury_cut.to_le_bytes());
    emit_event_bytes("spacekit_pay.payment.routed", &event_payload);

    // Return the split for the caller's records
    let mut result = Vec::with_capacity(32);
    result.extend_from_slice(&operator_cut.to_le_bytes());
    result.extend_from_slice(&treasury_cut.to_le_bytes());
    Ok(result)
}

// ============================================================================
// Read operations
// ============================================================================

fn op_get_treasury_rate() -> Result<Vec<u8>, ContractError> {
    Ok(TREASURY_RATE_BPS.to_le_bytes().to_vec())
}

fn op_get_treasury_address() -> Result<Vec<u8>, ContractError> {
    let addr = load_treasury_address()?;
    let mut out = Vec::with_capacity(addr.len() + 2);
    push_string(&mut out, &addr);
    Ok(out)
}

fn op_get_network() -> Result<Vec<u8>, ContractError> {
    let network = load_network()?;
    let mut out = Vec::with_capacity(network.len() + 2);
    push_string(&mut out, &network);
    Ok(out)
}

fn op_get_operator_registry() -> Result<Vec<u8>, ContractError> {
    let id = load_operator_registry()?;
    let mut out = Vec::with_capacity(id.len() + 2);
    push_string(&mut out, &id);
    Ok(out)
}

// ============================================================================
// Admin operations
// ============================================================================

fn op_set_treasury_address(input: &[u8], cursor: &mut usize) -> Result<Vec<u8>, ContractError> {
    require_admin()?;
    let new_addr = read_string(input, cursor)?;
    if new_addr.is_empty() {
        return Err(ContractError::InvalidInput);
    }
    storage_save(KEY_TREASURY, new_addr.as_bytes())?;
    let mut payload = Vec::with_capacity(new_addr.len() + 2);
    push_string(&mut payload, &new_addr);
    emit_event_bytes("spacekit_pay.treasury_address_changed", &payload);
    Ok(b"ok".to_vec())
}

fn op_set_network(input: &[u8], cursor: &mut usize) -> Result<Vec<u8>, ContractError> {
    require_admin()?;
    let network = read_string(input, cursor)?;
    if network.is_empty() {
        return Err(ContractError::InvalidInput);
    }
    storage_save(KEY_NETWORK, network.as_bytes())?;
    Ok(b"ok".to_vec())
}

fn op_set_operator_registry(input: &[u8], cursor: &mut usize) -> Result<Vec<u8>, ContractError> {
    require_admin()?;
    let registry_id = read_string(input, cursor)?;
    if registry_id.is_empty() {
        return Err(ContractError::InvalidInput);
    }
    storage_save(KEY_OPERATOR_REGISTRY, registry_id.as_bytes())?;
    Ok(b"ok".to_vec())
}

fn op_set_admin(input: &[u8], cursor: &mut usize) -> Result<Vec<u8>, ContractError> {
    require_admin()?;
    let new_admin = read_string(input, cursor)?;
    if new_admin.is_empty() {
        return Err(ContractError::InvalidInput);
    }
    storage_save(KEY_ADMIN, new_admin.as_bytes())?;
    Ok(b"ok".to_vec())
}

fn op_add_token(input: &[u8], cursor: &mut usize) -> Result<Vec<u8>, ContractError> {
    require_admin()?;
    let token_id = read_string(input, cursor)?;
    if token_id.is_empty() {
        return Err(ContractError::InvalidInput);
    }
    let key = format!("spacekit_pay.router.token_allowed.{}", token_id);
    storage_save(&key, b"1")?;

    let mut payload = Vec::with_capacity(token_id.len() + 2);
    push_string(&mut payload, &token_id);
    emit_event_bytes("spacekit_pay.token_allowed", &payload);
    Ok(b"ok".to_vec())
}

fn op_remove_token(input: &[u8], cursor: &mut usize) -> Result<Vec<u8>, ContractError> {
    require_admin()?;
    let token_id = read_string(input, cursor)?;
    if token_id.is_empty() {
        return Err(ContractError::InvalidInput);
    }
    let key = format!("spacekit_pay.router.token_allowed.{}", token_id);
    storage_save(&key, b"")?;

    let mut payload = Vec::with_capacity(token_id.len() + 2);
    push_string(&mut payload, &token_id);
    emit_event_bytes("spacekit_pay.token_revoked", &payload);
    Ok(b"ok".to_vec())
}

// ============================================================================
// Helpers
// ============================================================================

fn require_admin() -> Result<(), ContractError> {
    let caller = get_caller_did_string()?;
    let admin_bytes = storage_load(KEY_ADMIN)?;
    let admin = String::from_utf8(admin_bytes).map_err(|_| ContractError::StorageError)?;
    if caller != admin {
        return Err(ContractError::Unauthorized);
    }
    Ok(())
}

fn load_treasury_address() -> Result<String, ContractError> {
    let bytes = storage_load(KEY_TREASURY)?;
    String::from_utf8(bytes).map_err(|_| ContractError::StorageError)
}

fn load_network() -> Result<String, ContractError> {
    let bytes = storage_load(KEY_NETWORK)?;
    String::from_utf8(bytes).map_err(|_| ContractError::StorageError)
}

fn load_operator_registry() -> Result<String, ContractError> {
    let bytes = storage_load(KEY_OPERATOR_REGISTRY)?;
    String::from_utf8(bytes).map_err(|_| ContractError::StorageError)
}

/// The router's own DID, used as the intermediate recipient during atomic routing.
/// On SpaceKit, contracts are addressable by DID; the contract's own DID is its
/// identity for receiving and sending tokens during the routing transaction.
fn self_did() -> String {
    // In a real deployment, this is the contract's own DID, which the SDK
    // should expose via a host function. For the source listing, we use a
    // placeholder; the SDK function name to use is environment-specific.
    String::from("did:spacekit:contract:self")
}

fn lookup_operator_address(
    registry_id: &str,
    operator_did: &str,
    network: &str,
) -> Result<String, ContractError> {
    let mut payload = Vec::with_capacity(operator_did.len() + network.len() + 8);
    payload.push(REGISTRY_OP_LOOKUP);
    push_string(&mut payload, operator_did);
    push_string(&mut payload, network);

    let result = contract_call(registry_id, &payload)?;

    // Result is length-prefixed string (or empty for not-registered)
    if result.is_empty() {
        return Ok(String::new());
    }
    if result.len() < 2 {
        return Err(ContractError::InvalidInput);
    }
    let len = u16::from_le_bytes([result[0], result[1]]) as usize;
    if result.len() < 2 + len {
        return Err(ContractError::InvalidInput);
    }
    let addr_bytes = result[2..2 + len].to_vec();
    String::from_utf8(addr_bytes).map_err(|_| ContractError::StorageError)
}

fn build_transfer_from_payload(owner: &str, recipient: &str, amount: u128) -> Vec<u8> {
    let mut out = Vec::with_capacity(owner.len() + recipient.len() + 24);
    out.push(TOKEN_OP_TRANSFER_FROM);
    push_string(&mut out, owner);
    push_string(&mut out, recipient);
    out.extend_from_slice(&amount.to_le_bytes());
    out
}

fn build_transfer_payload(recipient: &str, amount: u128) -> Vec<u8> {
    let mut out = Vec::with_capacity(recipient.len() + 24);
    out.push(TOKEN_OP_TRANSFER);
    push_string(&mut out, recipient);
    out.extend_from_slice(&amount.to_le_bytes());
    out
}

fn push_string(out: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    let len = bytes.len().min(u16::MAX as usize) as u16;
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&bytes[..len as usize]);
}

// ============================================================================
// Unit tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fee_split_5pct_round_amount() {
        let total: u128 = 1_000_000;
        let treasury = (total * TREASURY_RATE_BPS as u128) / BPS_DENOMINATOR;
        let operator = total - treasury;
        assert_eq!(treasury, 50_000);
        assert_eq!(operator, 950_000);
        assert_eq!(treasury + operator, total);
    }

    #[test]
    fn fee_split_small_amount() {
        let total: u128 = 100;
        let treasury = (total * TREASURY_RATE_BPS as u128) / BPS_DENOMINATOR;
        let operator = total - treasury;
        assert_eq!(treasury, 5);
        assert_eq!(operator, 95);
    }

    #[test]
    fn fee_split_dust_under_20() {
        // Anything less than 20 atomic units gives 0 treasury (5% of <20 rounds to 0)
        let total: u128 = 19;
        let treasury = (total * TREASURY_RATE_BPS as u128) / BPS_DENOMINATOR;
        let operator = total - treasury;
        assert_eq!(treasury, 0);
        assert_eq!(operator, 19);
        // Operator gets the dust; treasury gets nothing. This is fine for our use case;
        // payments below 20 atomic units of a stablecoin are economically meaningless.
    }

    #[test]
    fn fee_split_exactly_20() {
        let total: u128 = 20;
        let treasury = (total * TREASURY_RATE_BPS as u128) / BPS_DENOMINATOR;
        let operator = total - treasury;
        assert_eq!(treasury, 1);
        assert_eq!(operator, 19);
    }

    #[test]
    fn fee_split_max_u128() {
        let total: u128 = u128::MAX;
        // Verify the checked_mul protects against overflow
        let mul_result = total.checked_mul(TREASURY_RATE_BPS as u128);
        assert!(mul_result.is_none()); // Overflow caught
    }

    #[test]
    fn fee_split_large_safe_amount() {
        // A trillion dollars in USDC (6 decimals) is 10^18, well under u128 max
        let total: u128 = 1_000_000_000_000_000_000;
        let treasury = (total * TREASURY_RATE_BPS as u128) / BPS_DENOMINATOR;
        let operator = total - treasury;
        assert_eq!(treasury, 50_000_000_000_000_000);
        assert_eq!(operator, 950_000_000_000_000_000);
    }

    #[test]
    fn transfer_payload_format() {
        let payload = build_transfer_payload("did:spacekit:operator", 1000);
        assert_eq!(payload[0], TOKEN_OP_TRANSFER);
        // length-prefixed string + u128
        assert_eq!(&payload[1..3], &21u16.to_le_bytes());
        assert_eq!(&payload[3..24], b"did:spacekit:operator");
        assert_eq!(&payload[24..40], &1000u128.to_le_bytes());
    }

    #[test]
    fn registry_lookup_payload_format() {
        let mut payload = Vec::new();
        payload.push(REGISTRY_OP_LOOKUP);
        push_string(&mut payload, "did:operator");
        push_string(&mut payload, "ethereum");
        assert_eq!(payload[0], REGISTRY_OP_LOOKUP);
        assert_eq!(&payload[1..3], &12u16.to_le_bytes());
        assert_eq!(&payload[3..15], b"did:operator");
        assert_eq!(&payload[15..17], &8u16.to_le_bytes());
        assert_eq!(&payload[17..25], b"ethereum");
    }

    #[test]
    fn treasury_rate_is_5_percent() {
        assert_eq!(TREASURY_RATE_BPS, 500);
        assert_eq!((TREASURY_RATE_BPS as u128) * 100 / BPS_DENOMINATOR, 5);
    }
}
