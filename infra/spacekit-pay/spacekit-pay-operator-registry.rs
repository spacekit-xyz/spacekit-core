//! SpaceKit Pay - OperatorRegistry Contract
//!
//! The OperatorRegistry is the source of truth for operator payout addresses
//! across networks. Operators self-register with a DID and one or more payout
//! addresses, keyed by network identifier.
//!
//! # Design properties
//!
//! - **Self-registration only.** Anyone can register as an operator. SWTCH
//!   Labs has no admin discretion over the operator set.
//! - **Self-managed.** Only the operator's own DID can update their
//!   registration. There is no admin override.
//! - **Read-open.** Anyone can look up an operator's payout address.
//!   PaymentRouter calls this on every payment.
//! - **Per-network addresses.** An operator can have different payout
//!   addresses on Ethereum, Base, Polygon, Solana, etc. The active address
//!   for a payment is determined by the network where the payment occurs.
//!
//! # Network identifiers
//!
//! Network identifiers are short canonical strings:
//! - "ethereum" - Ethereum mainnet
//! - "base"     - Base
//! - "polygon"  - Polygon PoS
//! - "arbitrum" - Arbitrum One
//! - "optimism" - Optimism
//! - "solana"   - Solana mainnet
//! - "spacekit" - SpaceKit mainnet
//!
//! The PaymentRouter on each network knows its own network identifier and
//! looks up the operator's address for that identifier.
//!
//! # Wire format
//!
//! | Op | Opcode | Payload | Returns |
//! |----|--------|---------|---------|
//! | LOOKUP | `0x01` | `[did_len u16][did_utf8][network_len u16][network_utf8]` | `[addr_len u16][addr_utf8]` or empty if not registered |
//! | REGISTER | `0x02` | `[network_len u16][network_utf8][addr_len u16][addr_utf8]` | `b"ok"` (caller's own DID only) |
//! | DEREGISTER | `0x03` | `[network_len u16][network_utf8]` | `b"ok"` (caller's own DID only) |
//! | LIST_NETWORKS | `0x04` | `[did_len u16][did_utf8]` | JSON array of registered networks |
//! | IS_REGISTERED | `0x05` | `[did_len u16][did_utf8][network_len u16][network_utf8]` | `b"1"` or `b"0"` |
//!
//! # Events
//!
//! - `spacekit_pay.operator.registered` - new registration
//! - `spacekit_pay.operator.updated` - address change for existing network
//! - `spacekit_pay.operator.deregistered` - network removed

#![no_std]

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use spacekit_contract_sdk::{
    emit_event_bytes, get_caller_did_string, spacekit_contract,
    spacekit_storage::{storage_load, storage_save},
    wire::{read_string, read_u8},
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

const OP_LOOKUP: u8 = 0x01;
const OP_REGISTER: u8 = 0x02;
const OP_DEREGISTER: u8 = 0x03;
const OP_LIST_NETWORKS: u8 = 0x04;
const OP_IS_REGISTERED: u8 = 0x05;

// Maximum length checks (defensive)
const MAX_NETWORK_LEN: usize = 64;
const MAX_ADDRESS_LEN: usize = 256;
const MAX_DID_LEN: usize = 256;

// ============================================================================
// Contract
// ============================================================================

struct OperatorRegistry;

impl SpacekitContract for OperatorRegistry {
    type Error = ContractError;

    fn init() -> Self {
        OperatorRegistry
    }

    fn handle(&mut self, input: &[u8]) -> Result<Vec<u8>, ContractError> {
        if input.is_empty() {
            return Err(ContractError::InvalidInput);
        }

        let mut cursor = 0usize;
        let opcode = read_u8(input, &mut cursor)?;

        match opcode {
            OP_LOOKUP => op_lookup(input, &mut cursor),
            OP_REGISTER => op_register(input, &mut cursor),
            OP_DEREGISTER => op_deregister(input, &mut cursor),
            OP_LIST_NETWORKS => op_list_networks(input, &mut cursor),
            OP_IS_REGISTERED => op_is_registered(input, &mut cursor),
            _ => Err(ContractError::InvalidInput),
        }
    }
}

spacekit_contract!(OperatorRegistry);

// ============================================================================
// Read operations
// ============================================================================

fn op_lookup(input: &[u8], cursor: &mut usize) -> Result<Vec<u8>, ContractError> {
    let did = read_string(input, cursor)?;
    let network = read_string(input, cursor)?;
    validate_did(&did)?;
    validate_network(&network)?;

    let key = address_key(&did, &network);
    match storage_load(&key) {
        Ok(addr_bytes) => {
            // Return as length-prefixed string for consistent wire format
            let mut out = Vec::with_capacity(addr_bytes.len() + 2);
            push_string_bytes(&mut out, &addr_bytes);
            Ok(out)
        }
        Err(_) => Ok(Vec::new()),
    }
}

fn op_is_registered(input: &[u8], cursor: &mut usize) -> Result<Vec<u8>, ContractError> {
    let did = read_string(input, cursor)?;
    let network = read_string(input, cursor)?;
    validate_did(&did)?;
    validate_network(&network)?;

    let key = address_key(&did, &network);
    let registered = storage_load(&key).is_ok();
    Ok(if registered { b"1".to_vec() } else { b"0".to_vec() })
}

fn op_list_networks(input: &[u8], cursor: &mut usize) -> Result<Vec<u8>, ContractError> {
    let did = read_string(input, cursor)?;
    validate_did(&did)?;

    // Networks are tracked via a per-DID index list
    let index_key = networks_index_key(&did);
    let networks_bytes = storage_load(&index_key).unwrap_or_default();
    let networks_str =
        String::from_utf8(networks_bytes).map_err(|_| ContractError::StorageError)?;

    // Stored as comma-separated list; return as JSON array
    let networks: Vec<&str> = networks_str.split(',').filter(|s| !s.is_empty()).collect();
    let json = format!(
        "[{}]",
        networks
            .iter()
            .map(|n| format!("\"{}\"", n))
            .collect::<Vec<_>>()
            .join(",")
    );
    Ok(json.into_bytes())
}

// ============================================================================
// Mutation operations (caller's own DID only)
// ============================================================================

fn op_register(input: &[u8], cursor: &mut usize) -> Result<Vec<u8>, ContractError> {
    let caller = get_caller_did_string()?;
    let network = read_string(input, cursor)?;
    let address = read_string(input, cursor)?;

    validate_did(&caller)?;
    validate_network(&network)?;
    validate_address(&address)?;

    let key = address_key(&caller, &network);
    let was_registered = storage_load(&key).is_ok();

    // Save the new address
    storage_save(&key, address.as_bytes())?;

    // Update the networks index for this DID (so list_networks works)
    update_networks_index(&caller, &network, true)?;

    // Emit appropriate event
    let topic = if was_registered {
        "spacekit_pay.operator.updated"
    } else {
        "spacekit_pay.operator.registered"
    };
    let mut payload = Vec::with_capacity(caller.len() + network.len() + address.len() + 6);
    push_string(&mut payload, &caller);
    push_string(&mut payload, &network);
    push_string(&mut payload, &address);
    emit_event_bytes(topic, &payload);

    Ok(b"ok".to_vec())
}

fn op_deregister(input: &[u8], cursor: &mut usize) -> Result<Vec<u8>, ContractError> {
    let caller = get_caller_did_string()?;
    let network = read_string(input, cursor)?;

    validate_did(&caller)?;
    validate_network(&network)?;

    let key = address_key(&caller, &network);

    // Only emit event if there was actually a registration
    if storage_load(&key).is_ok() {
        // Clear the address entry (write empty bytes)
        storage_save(&key, b"")?;
        update_networks_index(&caller, &network, false)?;

        let mut payload = Vec::with_capacity(caller.len() + network.len() + 4);
        push_string(&mut payload, &caller);
        push_string(&mut payload, &network);
        emit_event_bytes("spacekit_pay.operator.deregistered", &payload);
    }

    Ok(b"ok".to_vec())
}

// ============================================================================
// Storage helpers
// ============================================================================

fn address_key(did: &str, network: &str) -> String {
    format!("spacekit_pay.operator.{}.addr.{}", did, network)
}

fn networks_index_key(did: &str) -> String {
    format!("spacekit_pay.operator.{}.networks", did)
}

fn update_networks_index(did: &str, network: &str, add: bool) -> Result<(), ContractError> {
    let index_key = networks_index_key(did);
    let current_bytes = storage_load(&index_key).unwrap_or_default();
    let current =
        String::from_utf8(current_bytes).map_err(|_| ContractError::StorageError)?;

    let mut networks: Vec<&str> = current.split(',').filter(|s| !s.is_empty()).collect();

    if add {
        if !networks.contains(&network) {
            networks.push(network);
        }
    } else {
        networks.retain(|&n| n != network);
    }

    let updated = networks.join(",");
    storage_save(&index_key, updated.as_bytes())?;
    Ok(())
}

// ============================================================================
// Validation
// ============================================================================

fn validate_did(did: &str) -> Result<(), ContractError> {
    if did.is_empty() || did.len() > MAX_DID_LEN {
        return Err(ContractError::InvalidInput);
    }
    if !did.starts_with("did:") {
        return Err(ContractError::InvalidInput);
    }
    Ok(())
}

fn validate_network(network: &str) -> Result<(), ContractError> {
    if network.is_empty() || network.len() > MAX_NETWORK_LEN {
        return Err(ContractError::InvalidInput);
    }
    // Network identifiers must be lowercase ASCII alphanumeric plus underscore
    for byte in network.bytes() {
        let ok = byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_';
        if !ok {
            return Err(ContractError::InvalidInput);
        }
    }
    Ok(())
}

fn validate_address(address: &str) -> Result<(), ContractError> {
    if address.is_empty() || address.len() > MAX_ADDRESS_LEN {
        return Err(ContractError::InvalidInput);
    }
    // Address format validation is network-specific. The OperatorRegistry
    // does not enforce network-specific syntax here; the PaymentRouter on
    // each network validates the address format before using it. This
    // separation keeps the registry chain-agnostic.
    Ok(())
}

// ============================================================================
// Wire format helpers
// ============================================================================

fn push_string(out: &mut Vec<u8>, s: &str) {
    push_string_bytes(out, s.as_bytes());
}

fn push_string_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
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
    fn address_key_format() {
        let key = address_key("did:spacekit:testnet:0xabc", "ethereum");
        assert_eq!(key, "spacekit_pay.operator.did:spacekit:testnet:0xabc.addr.ethereum");
    }

    #[test]
    fn networks_index_key_format() {
        let key = networks_index_key("did:spacekit:testnet:0xabc");
        assert_eq!(key, "spacekit_pay.operator.did:spacekit:testnet:0xabc.networks");
    }

    #[test]
    fn validate_did_accepts_valid() {
        assert!(validate_did("did:spacekit:testnet:0xabc").is_ok());
        assert!(validate_did("did:ethr:0x1234").is_ok());
    }

    #[test]
    fn validate_did_rejects_invalid() {
        assert!(validate_did("").is_err());
        assert!(validate_did("not-a-did").is_err());
        assert!(validate_did("ethereum:0x1234").is_err());
    }

    #[test]
    fn validate_network_accepts_canonical() {
        assert!(validate_network("ethereum").is_ok());
        assert!(validate_network("base").is_ok());
        assert!(validate_network("polygon").is_ok());
        assert!(validate_network("arbitrum").is_ok());
        assert!(validate_network("optimism").is_ok());
        assert!(validate_network("solana").is_ok());
        assert!(validate_network("spacekit").is_ok());
        assert!(validate_network("eth_sepolia").is_ok());
    }

    #[test]
    fn validate_network_rejects_invalid() {
        assert!(validate_network("").is_err());
        assert!(validate_network("Ethereum").is_err()); // uppercase
        assert!(validate_network("eth-mainnet").is_err()); // hyphen
        assert!(validate_network("eth mainnet").is_err()); // space
    }

    #[test]
    fn validate_address_accepts_eth_format() {
        assert!(validate_address("0x742d35Cc6634C0532925a3b844Bc454e4438f44e").is_ok());
    }

    #[test]
    fn validate_address_accepts_solana_format() {
        assert!(validate_address("DRiP2Pn2K6fuMLKQmt5rZWxa91TGn3D2DvEQ23vAhEPF").is_ok());
    }

    #[test]
    fn validate_address_rejects_empty() {
        assert!(validate_address("").is_err());
    }
}
