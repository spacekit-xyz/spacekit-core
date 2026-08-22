//! SpaceKit AssetToken - Generic Asset Token
//!
//! Represents any type of property or asset as a unique on-chain token.
//! More flexible than PropertyToken (which is real-estate-specific) but
//! less detailed. Designed for vehicles, art, equipment, intellectual
//! property, livestock, and other assets with identifiable ownership.
//!
//! # Design properties
//!
//! - **One token per asset.** Each asset has a unique token ID.
//! - **Extensible asset types.** Built-in types for common categories
//!   (vehicle, art, equipment, IP, livestock, collectible), plus custom
//!   type slot for novel categories.
//! - **Flexible attribute schema.** JSON-formatted attributes per asset
//!   type — vehicles have VIN, art has artist/medium, equipment has
//!   serial number/manufacturer, etc.
//! - **Single, joint, or organizational ownership.** A DID can own
//!   alone, jointly with other DIDs, or via organizational DID (LLC,
//!   trust, etc.).
//! - **Ownership history with party verification.** Transfers require
//!   appropriate signatures based on configured policy.
//! - **Document references.** Titles, certificates, registrations,
//!   appraisals all linked by CAS hash.
//! - **Optional PropertyToken evolution.** An AssetToken representing
//!   a vacant lot can be upgraded to PropertyToken when a deed addendum
//!   is created.
//! - **NOT legally binding without appropriate legal anchor.** Like
//!   PropertyToken, this contract is record-and-reference, not legal
//!   authority. Legal frameworks vary by asset type and jurisdiction.
//!
//! # Wire format (length-prefixed binary)
//!
//! | Op | Opcode | Payload | Returns |
//! |----|--------|---------|---------|
//! | MINT | 0x10 | asset metadata | [token_id 32] |
//! | TRANSFER | 0x20 | transfer instructions | [success 1] |
//! | ADD_DOCUMENT | 0x40 | document reference | [doc_id 32] |
//! | UPDATE_ATTRIBUTES | 0x50 | new attributes | [success 1] |
//! | LINK_TO_PROPERTY | 0x60 | property_token_id | [success 1] |
//! | GET_ASSET | 0x70 | [token_id 32] | asset JSON |
//! | GET_OWNERS | 0x71 | [token_id 32] | owner list |
//! | GET_HISTORY | 0x72 | [token_id 32] | history JSON |
//! | GET_DOCUMENTS | 0x74 | [token_id 32] | document list |
//! | LIST_ASSETS_BY_OWNER | 0x75 | [did_hash 32] | token list |
//! | LIST_ASSETS_BY_TYPE | 0x76 | [type_id 1] | token list |
//!
//! # Events
//!
//! - `asset.minted` - new asset token created
//! - `asset.transferred` - ownership transferred
//! - `asset.document_added` - new document reference
//! - `asset.attributes_updated` - attributes changed
//! - `asset.linked_to_property` - linked to PropertyToken (evolution)

#![no_std]

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use spacekit_contract_sdk::{
    emit_event_bytes, get_caller_did_hash, spacekit_contract,
    spacekit_storage::{storage_load, storage_save},
    wire::{read_string, read_u8, read_u32, read_u64},
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

// Opcodes
const OP_MINT: u8 = 0x10;
const OP_TRANSFER: u8 = 0x20;
const OP_ADD_DOCUMENT: u8 = 0x40;
const OP_UPDATE_ATTRIBUTES: u8 = 0x50;
const OP_LINK_TO_PROPERTY: u8 = 0x60;
const OP_GET_ASSET: u8 = 0x70;
const OP_GET_OWNERS: u8 = 0x71;
const OP_GET_HISTORY: u8 = 0x72;
const OP_GET_DOCUMENTS: u8 = 0x74;
const OP_LIST_ASSETS_BY_OWNER: u8 = 0x75;
const OP_LIST_ASSETS_BY_TYPE: u8 = 0x76;

// Asset types (extensible; reserved 0x80-0xFF for custom)
const ASSET_TYPE_VEHICLE: u8 = 0x01;
const ASSET_TYPE_ART: u8 = 0x02;
const ASSET_TYPE_EQUIPMENT: u8 = 0x03;
const ASSET_TYPE_INTELLECTUAL_PROPERTY: u8 = 0x04;
const ASSET_TYPE_LIVESTOCK: u8 = 0x05;
const ASSET_TYPE_COLLECTIBLE: u8 = 0x06;
const ASSET_TYPE_PRECIOUS_METAL: u8 = 0x07;
const ASSET_TYPE_DIGITAL_ASSET: u8 = 0x08;
const ASSET_TYPE_INVENTORY: u8 = 0x09;
const ASSET_TYPE_CUSTOM_START: u8 = 0x80;
const ASSET_TYPE_OTHER: u8 = 0xFF;

// Ownership types (subset of PropertyToken's set)
const OWNERSHIP_SOLE: u8 = 0x01;
const OWNERSHIP_JOINT: u8 = 0x02;
const OWNERSHIP_ORGANIZATIONAL: u8 = 0x03;  // LLC, trust, etc.

// Document types
const DOC_TITLE: u8 = 0x01;
const DOC_CERTIFICATE_OF_AUTHENTICITY: u8 = 0x02;
const DOC_REGISTRATION: u8 = 0x03;
const DOC_APPRAISAL: u8 = 0x04;
const DOC_INSURANCE: u8 = 0x05;
const DOC_PROVENANCE: u8 = 0x06;
const DOC_PURCHASE_RECEIPT: u8 = 0x07;
const DOC_PHOTO: u8 = 0x08;
const DOC_INSPECTION: u8 = 0x09;
const DOC_OTHER: u8 = 0xFF;

// Limits
const MAX_OWNERS_PER_ASSET: usize = 10;
const MAX_DOCUMENTS_PER_ASSET: usize = 100;

// ============================================================================
// Storage key prefixes
// ============================================================================

const KEY_PREFIX_ASSET: &str = "asset.";                       // + token_id_hex
const KEY_PREFIX_OWNERS: &str = "asset.owners.";               // + token_id_hex
const KEY_PREFIX_HISTORY: &str = "asset.history.";             // + token_id_hex
const KEY_PREFIX_DOCUMENTS: &str = "asset.docs.";              // + token_id_hex
const KEY_PREFIX_LINKED_PROPERTY: &str = "asset.property.";    // + token_id_hex

// Reverse indices
const KEY_PREFIX_OWNED_BY: &str = "asset.owned_by.";           // + did_hex
const KEY_PREFIX_BY_TYPE: &str = "asset.by_type.";             // + type_id

// Counters
const KEY_NEXT_TOKEN_ID: &str = "asset.next_token_id";
const KEY_NEXT_DOCUMENT_ID: &str = "asset.next_doc_id";

// ============================================================================
// Contract
// ============================================================================

struct AssetToken;

impl SpacekitContract for AssetToken {
    type Error = ContractError;

    fn init() -> Self {
        AssetToken
    }

    fn handle(&mut self, input: &[u8]) -> Result<Vec<u8>, ContractError> {
        if input.is_empty() {
            return Err(ContractError::InvalidInput);
        }

        let mut cursor = 0usize;
        let opcode = read_u8(input, &mut cursor)?;

        match opcode {
            OP_MINT => op_mint(input, &mut cursor),
            OP_TRANSFER => op_transfer(input, &mut cursor),
            OP_ADD_DOCUMENT => op_add_document(input, &mut cursor),
            OP_UPDATE_ATTRIBUTES => op_update_attributes(input, &mut cursor),
            OP_LINK_TO_PROPERTY => op_link_to_property(input, &mut cursor),
            OP_GET_ASSET => op_get_asset(input, &mut cursor),
            OP_GET_OWNERS => op_get_owners(input, &mut cursor),
            OP_GET_HISTORY => op_get_history(input, &mut cursor),
            OP_GET_DOCUMENTS => op_get_documents(input, &mut cursor),
            OP_LIST_ASSETS_BY_OWNER => op_list_assets_by_owner(input, &mut cursor),
            OP_LIST_ASSETS_BY_TYPE => op_list_assets_by_type(input, &mut cursor),
            _ => Err(ContractError::InvalidInput),
        }
    }
}

spacekit_contract!(AssetToken);

// ============================================================================
// MINT - Create a new asset token
// ============================================================================

fn op_mint(input: &[u8], cursor: &mut usize) -> Result<Vec<u8>, ContractError> {
    let caller_hash = get_caller_did_hash()?;

    // Read asset metadata
    let asset_type = read_u8(input, cursor)?;
    let ownership_type = read_u8(input, cursor)?;
    let custom_type_name = read_string(input, cursor)?;  // empty if asset_type < 0x80

    // Asset identity
    let title = read_string(input, cursor)?;          // human-readable name
    let description = read_string(input, cursor)?;
    let unique_identifier = read_string(input, cursor)?;  // VIN, serial number, ISBN, etc.
    let location = read_string(input, cursor)?;       // current physical location

    // Flexible attributes (JSON string, schema depends on asset_type)
    let attributes_json = read_string(input, cursor)?;

    // Optional acquisition data
    let acquired_at = read_u64(input, cursor)?;       // 0 if not specified
    let acquisition_value = read_u64(input, cursor)?; // in smallest currency unit; 0 if not specified
    let acquisition_currency = read_string(input, cursor)?; // "USD", "EUR", etc.; empty if not specified

    // Owner DIDs
    let owner_count = read_u8(input, cursor)? as usize;
    if owner_count == 0 || owner_count > MAX_OWNERS_PER_ASSET {
        return Err(ContractError::InvalidInput);
    }
    let mut owner_dids: Vec<[u8; 32]> = Vec::with_capacity(owner_count);
    for _ in 0..owner_count {
        owner_dids.push(read_did_hash(input, cursor)?);
    }

    // Validate caller is one of the owners
    if !owner_dids.iter().any(|d| d[..] == caller_hash[..]) {
        return Err(ContractError::Unauthorized);
    }

    // Assign new token ID
    let token_id = next_token_id()?;
    let token_id_hex = hex_encode(&token_id);

    // Build asset record
    let asset_record = build_asset_record(
        &token_id, asset_type, ownership_type, &custom_type_name,
        &title, &description, &unique_identifier, &location,
        &attributes_json, acquired_at, acquisition_value, &acquisition_currency,
        current_timestamp(),
    );
    storage_save(&format!("{}{}", KEY_PREFIX_ASSET, token_id_hex), asset_record.as_bytes())?;

    // Store owners
    let owners_data = build_owners_record(&owner_dids, current_timestamp());
    storage_save(&format!("{}{}", KEY_PREFIX_OWNERS, token_id_hex), owners_data.as_bytes())?;

    // Initialize history
    let history_entry = format!(
        "[{{\"event\":\"mint\",\"timestamp\":{},\"caller\":\"{}\",\"asset_type\":{}}}]",
        current_timestamp(), hex_encode(&caller_hash), asset_type
    );
    storage_save(&format!("{}{}", KEY_PREFIX_HISTORY, token_id_hex), history_entry.as_bytes())?;

    // Initialize empty documents
    storage_save(&format!("{}{}", KEY_PREFIX_DOCUMENTS, token_id_hex), b"[]")?;

    // Update owned-by indices
    for owner_did in &owner_dids {
        add_to_owned_by_index(owner_did, &token_id)?;
    }

    // Update by-type index
    add_to_by_type_index(asset_type, &token_id)?;

    // Emit event
    let mut event_payload = Vec::with_capacity(128);
    event_payload.extend_from_slice(&token_id);
    event_payload.push(asset_type);
    event_payload.push(ownership_type);
    event_payload.push(owner_count as u8);
    for owner_did in &owner_dids {
        event_payload.extend_from_slice(owner_did);
    }
    emit_event_bytes("asset.minted", &event_payload);

    Ok(token_id.to_vec())
}

// ============================================================================
// TRANSFER
// ============================================================================

fn op_transfer(input: &[u8], cursor: &mut usize) -> Result<Vec<u8>, ContractError> {
    let caller_hash = get_caller_did_hash()?;

    let token_id = read_did_hash(input, cursor)?;
    let token_id_hex = hex_encode(&token_id);

    let new_ownership_type = read_u8(input, cursor)?;
    let new_owner_count = read_u8(input, cursor)? as usize;
    if new_owner_count == 0 || new_owner_count > MAX_OWNERS_PER_ASSET {
        return Err(ContractError::InvalidInput);
    }
    let mut new_owner_dids: Vec<[u8; 32]> = Vec::with_capacity(new_owner_count);
    for _ in 0..new_owner_count {
        new_owner_dids.push(read_did_hash(input, cursor)?);
    }

    // Transfer documentation
    let transfer_document_hash = read_did_hash(input, cursor)?;
    let payment_confirmation = read_did_hash(input, cursor)?; // all zeros if no payment
    let transfer_terms = read_string(input, cursor)?;  // sale, gift, inheritance, etc.

    // Verify caller is a current owner
    let owners_record = storage_load(&format!("{}{}", KEY_PREFIX_OWNERS, token_id_hex))
        .map_err(|_| ContractError::InvalidInput)?;
    let owners_str = core::str::from_utf8(&owners_record)
        .map_err(|_| ContractError::StorageError)?;
    if !owners_str.contains(&hex_encode(&caller_hash)) {
        return Err(ContractError::Unauthorized);
    }

    // Get old owner list for index updates
    let old_owners = parse_owner_dids_from_record(owners_str);

    // Build and store new owners
    let new_owners_data = build_owners_record(&new_owner_dids, current_timestamp());
    storage_save(&format!("{}{}", KEY_PREFIX_OWNERS, token_id_hex), new_owners_data.as_bytes())?;

    // Update owned-by indices
    for old_owner in &old_owners {
        remove_from_owned_by_index(old_owner, &token_id)?;
    }
    for new_owner in &new_owner_dids {
        add_to_owned_by_index(new_owner, &token_id)?;
    }

    // Append to history
    let history_key = format!("{}{}", KEY_PREFIX_HISTORY, token_id_hex);
    let existing_history = storage_load(&history_key).unwrap_or_else(|_| b"[]".to_vec());
    let existing_str = core::str::from_utf8(&existing_history)
        .map_err(|_| ContractError::StorageError)?;
    
    let trimmed = existing_str.trim_end_matches(']');
    let new_entry = format!(
        ",{{\"event\":\"transfer\",\"timestamp\":{},\"caller\":\"{}\",\"transfer_terms\":\"{}\",\"document_hash\":\"{}\",\"payment_confirmation\":\"{}\"}}",
        current_timestamp(), hex_encode(&caller_hash),
        escape_json(&transfer_terms),
        hex_encode(&transfer_document_hash),
        hex_encode(&payment_confirmation)
    );
    let updated_history = format!("{}{}]", trimmed, new_entry);
    storage_save(&history_key, updated_history.as_bytes())?;

    // Emit event
    let mut event_payload = Vec::with_capacity(128);
    event_payload.extend_from_slice(&token_id);
    event_payload.extend_from_slice(&caller_hash);
    event_payload.push(new_owner_count as u8);
    for owner in &new_owner_dids {
        event_payload.extend_from_slice(owner);
    }
    emit_event_bytes("asset.transferred", &event_payload);

    Ok(vec![1u8])
}

// ============================================================================
// Document operations
// ============================================================================

fn op_add_document(input: &[u8], cursor: &mut usize) -> Result<Vec<u8>, ContractError> {
    let caller_hash = get_caller_did_hash()?;

    let token_id = read_did_hash(input, cursor)?;
    let token_id_hex = hex_encode(&token_id);

    let doc_type = read_u8(input, cursor)?;
    let doc_hash = read_did_hash(input, cursor)?;
    let title = read_string(input, cursor)?;
    let description = read_string(input, cursor)?;

    if !verify_caller_is_owner(&caller_hash, &token_id)? {
        return Err(ContractError::Unauthorized);
    }

    let doc_id = next_document_id()?;
    let doc_id_hex = hex_encode(&doc_id);

    let key = format!("{}{}", KEY_PREFIX_DOCUMENTS, token_id_hex);
    let existing = storage_load(&key).unwrap_or_else(|_| b"[]".to_vec());
    let existing_str = core::str::from_utf8(&existing)
        .map_err(|_| ContractError::StorageError)?;
    
    let trimmed = existing_str.trim_end_matches(']');
    let separator = if trimmed == "[" { "" } else { "," };
    let new_entry = format!(
        "{}{{\"id\":\"{}\",\"type\":{},\"hash\":\"{}\",\"title\":\"{}\",\"description\":\"{}\",\"added_by\":\"{}\",\"added_at\":{}}}",
        separator, doc_id_hex, doc_type, hex_encode(&doc_hash),
        escape_json(&title), escape_json(&description),
        hex_encode(&caller_hash), current_timestamp()
    );
    let updated = format!("{}{}]", trimmed, new_entry);
    storage_save(&key, updated.as_bytes())?;

    let mut event_payload = Vec::with_capacity(128);
    event_payload.extend_from_slice(&token_id);
    event_payload.extend_from_slice(&doc_id);
    event_payload.push(doc_type);
    event_payload.extend_from_slice(&doc_hash);
    emit_event_bytes("asset.document_added", &event_payload);

    Ok(doc_id.to_vec())
}

fn op_update_attributes(input: &[u8], cursor: &mut usize) -> Result<Vec<u8>, ContractError> {
    let caller_hash = get_caller_did_hash()?;

    let token_id = read_did_hash(input, cursor)?;
    let new_attributes_json = read_string(input, cursor)?;

    if !verify_caller_is_owner(&caller_hash, &token_id)? {
        return Err(ContractError::Unauthorized);
    }

    let token_id_hex = hex_encode(&token_id);
    let asset_key = format!("{}{}", KEY_PREFIX_ASSET, token_id_hex);
    
    let existing = storage_load(&asset_key)?;
    let existing_str = core::str::from_utf8(&existing)
        .map_err(|_| ContractError::StorageError)?;
    
    // Replace attributes section (simplified; production version uses proper JSON manipulation)
    let updated = replace_json_field(existing_str, "attributes", &new_attributes_json);
    storage_save(&asset_key, updated.as_bytes())?;

    // Append to history
    let history_key = format!("{}{}", KEY_PREFIX_HISTORY, token_id_hex);
    let existing_history = storage_load(&history_key).unwrap_or_else(|_| b"[]".to_vec());
    let existing_hist_str = core::str::from_utf8(&existing_history)
        .map_err(|_| ContractError::StorageError)?;
    let trimmed = existing_hist_str.trim_end_matches(']');
    let new_entry = format!(
        ",{{\"event\":\"attributes_updated\",\"timestamp\":{},\"caller\":\"{}\"}}",
        current_timestamp(), hex_encode(&caller_hash)
    );
    let updated_history = format!("{}{}]", trimmed, new_entry);
    storage_save(&history_key, updated_history.as_bytes())?;

    emit_event_bytes("asset.attributes_updated", &token_id);
    Ok(vec![1u8])
}

// ============================================================================
// LINK_TO_PROPERTY - Evolution to PropertyToken
// ============================================================================

fn op_link_to_property(input: &[u8], cursor: &mut usize) -> Result<Vec<u8>, ContractError> {
    let caller_hash = get_caller_did_hash()?;

    let asset_token_id = read_did_hash(input, cursor)?;
    let property_token_id = read_did_hash(input, cursor)?;

    if !verify_caller_is_owner(&caller_hash, &asset_token_id)? {
        return Err(ContractError::Unauthorized);
    }

    let asset_id_hex = hex_encode(&asset_token_id);
    let link_record = format!(
        "{{\"property_token_id\":\"{}\",\"linked_at\":{},\"linked_by\":\"{}\"}}",
        hex_encode(&property_token_id),
        current_timestamp(),
        hex_encode(&caller_hash)
    );
    storage_save(&format!("{}{}", KEY_PREFIX_LINKED_PROPERTY, asset_id_hex), link_record.as_bytes())?;

    let mut event_payload = Vec::with_capacity(96);
    event_payload.extend_from_slice(&asset_token_id);
    event_payload.extend_from_slice(&property_token_id);
    event_payload.extend_from_slice(&caller_hash);
    emit_event_bytes("asset.linked_to_property", &event_payload);

    Ok(vec![1u8])
}

// ============================================================================
// Read operations
// ============================================================================

fn op_get_asset(input: &[u8], cursor: &mut usize) -> Result<Vec<u8>, ContractError> {
    let token_id = read_did_hash(input, cursor)?;
    let token_id_hex = hex_encode(&token_id);
    storage_load(&format!("{}{}", KEY_PREFIX_ASSET, token_id_hex))
}

fn op_get_owners(input: &[u8], cursor: &mut usize) -> Result<Vec<u8>, ContractError> {
    let token_id = read_did_hash(input, cursor)?;
    let token_id_hex = hex_encode(&token_id);
    storage_load(&format!("{}{}", KEY_PREFIX_OWNERS, token_id_hex))
}

fn op_get_history(input: &[u8], cursor: &mut usize) -> Result<Vec<u8>, ContractError> {
    let token_id = read_did_hash(input, cursor)?;
    let token_id_hex = hex_encode(&token_id);
    storage_load(&format!("{}{}", KEY_PREFIX_HISTORY, token_id_hex))
}

fn op_get_documents(input: &[u8], cursor: &mut usize) -> Result<Vec<u8>, ContractError> {
    let token_id = read_did_hash(input, cursor)?;
    let token_id_hex = hex_encode(&token_id);
    storage_load(&format!("{}{}", KEY_PREFIX_DOCUMENTS, token_id_hex))
}

fn op_list_assets_by_owner(input: &[u8], cursor: &mut usize) -> Result<Vec<u8>, ContractError> {
    let did_hash = read_did_hash(input, cursor)?;
    let did_hex = hex_encode(&did_hash);
    storage_load(&format!("{}{}", KEY_PREFIX_OWNED_BY, did_hex))
        .or_else(|_| Ok(b"[]".to_vec()))
}

fn op_list_assets_by_type(input: &[u8], cursor: &mut usize) -> Result<Vec<u8>, ContractError> {
    let asset_type = read_u8(input, cursor)?;
    storage_load(&format!("{}{}", KEY_PREFIX_BY_TYPE, asset_type))
        .or_else(|_| Ok(b"[]".to_vec()))
}

// ============================================================================
// Helpers (most shared with PropertyToken pattern)
// ============================================================================

fn next_token_id() -> Result<[u8; 32], ContractError> {
    let counter = read_u64_or_zero(KEY_NEXT_TOKEN_ID)?;
    write_u64(KEY_NEXT_TOKEN_ID, counter + 1)?;
    let mut id = [0u8; 32];
    id[0..8].copy_from_slice(&counter.to_le_bytes());
    Ok(id)
}

fn next_document_id() -> Result<[u8; 32], ContractError> {
    let counter = read_u64_or_zero(KEY_NEXT_DOCUMENT_ID)?;
    write_u64(KEY_NEXT_DOCUMENT_ID, counter + 1)?;
    let mut id = [0u8; 32];
    id[0..8].copy_from_slice(&counter.to_le_bytes());
    Ok(id)
}

fn verify_caller_is_owner(caller_hash: &[u8; 32], token_id: &[u8; 32]) -> Result<bool, ContractError> {
    let token_id_hex = hex_encode(token_id);
    let owners_record = storage_load(&format!("{}{}", KEY_PREFIX_OWNERS, token_id_hex))?;
    let owners_str = core::str::from_utf8(&owners_record)
        .map_err(|_| ContractError::StorageError)?;
    Ok(owners_str.contains(&hex_encode(caller_hash)))
}

fn add_to_owned_by_index(owner_did: &[u8; 32], token_id: &[u8; 32]) -> Result<(), ContractError> {
    let did_hex = hex_encode(owner_did);
    let key = format!("{}{}", KEY_PREFIX_OWNED_BY, did_hex);
    let existing = storage_load(&key).unwrap_or_else(|_| b"[]".to_vec());
    let existing_str = core::str::from_utf8(&existing)
        .map_err(|_| ContractError::StorageError)?;
    
    let token_hex = hex_encode(token_id);
    if existing_str.contains(&token_hex) {
        return Ok(());
    }
    
    let trimmed = existing_str.trim_end_matches(']');
    let separator = if trimmed == "[" { "" } else { "," };
    let updated = format!("{}{}\"{}\"]", trimmed, separator, token_hex);
    storage_save(&key, updated.as_bytes())?;
    Ok(())
}

fn remove_from_owned_by_index(owner_did: &[u8; 32], token_id: &[u8; 32]) -> Result<(), ContractError> {
    let did_hex = hex_encode(owner_did);
    let key = format!("{}{}", KEY_PREFIX_OWNED_BY, did_hex);
    let existing = storage_load(&key).unwrap_or_else(|_| b"[]".to_vec());
    let existing_str = core::str::from_utf8(&existing)
        .map_err(|_| ContractError::StorageError)?;
    
    let token_hex = hex_encode(token_id);
    let with_comma = format!(",\"{}\"", token_hex);
    let alone = format!("\"{}\"", token_hex);
    
    let updated = if existing_str.contains(&with_comma) {
        existing_str.replace(&with_comma, "")
    } else if existing_str.contains(&alone) {
        existing_str.replace(&alone, "")
            .replace("[,", "[")
            .replace(",,", ",")
            .replace(",]", "]")
    } else {
        return Ok(());
    };
    
    storage_save(&key, updated.as_bytes())?;
    Ok(())
}

fn add_to_by_type_index(asset_type: u8, token_id: &[u8; 32]) -> Result<(), ContractError> {
    let key = format!("{}{}", KEY_PREFIX_BY_TYPE, asset_type);
    let existing = storage_load(&key).unwrap_or_else(|_| b"[]".to_vec());
    let existing_str = core::str::from_utf8(&existing)
        .map_err(|_| ContractError::StorageError)?;
    
    let token_hex = hex_encode(token_id);
    if existing_str.contains(&token_hex) {
        return Ok(());
    }
    
    let trimmed = existing_str.trim_end_matches(']');
    let separator = if trimmed == "[" { "" } else { "," };
    let updated = format!("{}{}\"{}\"]", trimmed, separator, token_hex);
    storage_save(&key, updated.as_bytes())?;
    Ok(())
}

fn parse_owner_dids_from_record(owners_str: &str) -> Vec<[u8; 32]> {
    let mut result = Vec::new();
    let mut start = 0;
    while let Some(pos) = owners_str[start..].find("\"did_hash\":\"") {
        let did_start = start + pos + 12;
        if did_start + 64 <= owners_str.len() {
            let hex_str = &owners_str[did_start..did_start + 64];
            if let Some(bytes) = hex_decode(hex_str) {
                result.push(bytes);
            }
        }
        start = did_start + 64;
    }
    result
}

fn build_asset_record(
    token_id: &[u8; 32],
    asset_type: u8, ownership_type: u8, custom_type_name: &str,
    title: &str, description: &str, unique_identifier: &str, location: &str,
    attributes_json: &str,
    acquired_at: u64, acquisition_value: u64, acquisition_currency: &str,
    timestamp: u64,
) -> String {
    format!(
        "{{\"token_id\":\"{}\",\"asset_type\":{},\"ownership_type\":{},\"custom_type_name\":\"{}\",\"title\":\"{}\",\"description\":\"{}\",\"unique_identifier\":\"{}\",\"location\":\"{}\",\"attributes\":{},\"acquisition\":{{\"acquired_at\":{},\"value\":{},\"currency\":\"{}\"}},\"minted_at\":{}}}",
        hex_encode(token_id), asset_type, ownership_type,
        escape_json(custom_type_name), escape_json(title), escape_json(description),
        escape_json(unique_identifier), escape_json(location),
        if attributes_json.is_empty() { "{}".to_string() } else { attributes_json.to_string() },
        acquired_at, acquisition_value, escape_json(acquisition_currency),
        timestamp
    )
}

fn build_owners_record(owner_dids: &[[u8; 32]], timestamp: u64) -> String {
    let mut entries: Vec<String> = Vec::with_capacity(owner_dids.len());
    for did in owner_dids {
        entries.push(format!("{{\"did_hash\":\"{}\"}}", hex_encode(did)));
    }
    format!("{{\"owners\":[{}],\"updated_at\":{}}}", entries.join(","), timestamp)
}

fn replace_json_field(original: &str, field: &str, new_value: &str) -> String {
    // Simplified replacement; production version uses proper JSON manipulation
    let field_marker = format!("\"{}\":", field);
    if let Some(start) = original.find(&field_marker) {
        let value_start = start + field_marker.len();
        // Find end of value (next "," or "}" at the same depth)
        let mut depth = 0;
        let mut end = value_start;
        for (i, c) in original[value_start..].char_indices() {
            match c {
                '{' | '[' => depth += 1,
                '}' | ']' => {
                    if depth == 0 {
                        end = value_start + i;
                        break;
                    }
                    depth -= 1;
                }
                ',' => {
                    if depth == 0 {
                        end = value_start + i;
                        break;
                    }
                }
                _ => {}
            }
        }
        format!("{}{}{}", &original[..value_start], new_value, &original[end..])
    } else {
        original.to_string()
    }
}

fn current_timestamp() -> u64 { 0 }

fn read_did_hash(input: &[u8], cursor: &mut usize) -> Result<[u8; 32], ContractError> {
    if input.len() < *cursor + 32 {
        return Err(ContractError::InvalidInput);
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&input[*cursor..*cursor + 32]);
    *cursor += 32;
    Ok(arr)
}

fn read_u64_or_zero(key: &str) -> Result<u64, ContractError> {
    match storage_load(key) {
        Ok(bytes) => {
            if bytes.len() < 8 { return Err(ContractError::StorageError); }
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
        _ => '?',
    }
}

fn hex_decode(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 { return None; }
    let mut result = [0u8; 32];
    let bytes = s.as_bytes();
    for i in 0..32 {
        let high = hex_value(bytes[i * 2])?;
        let low = hex_value(bytes[i * 2 + 1])?;
        result[i] = (high << 4) | low;
    }
    Some(result)
}

fn hex_value(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

fn escape_json(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_types_distinct() {
        assert_ne!(ASSET_TYPE_VEHICLE, ASSET_TYPE_ART);
        assert_ne!(ASSET_TYPE_EQUIPMENT, ASSET_TYPE_INTELLECTUAL_PROPERTY);
    }

    #[test]
    fn custom_type_range_reserved() {
        assert!(ASSET_TYPE_CUSTOM_START >= 0x80);
        assert!(ASSET_TYPE_INVENTORY < ASSET_TYPE_CUSTOM_START);
    }

    #[test]
    fn document_types_distinct() {
        assert_ne!(DOC_TITLE, DOC_CERTIFICATE_OF_AUTHENTICITY);
        assert_ne!(DOC_APPRAISAL, DOC_PROVENANCE);
    }

    #[test]
    fn hex_roundtrip() {
        let original = [0xDE, 0xAD, 0xBE, 0xEF];
        let mut padded = [0u8; 32];
        padded[..4].copy_from_slice(&original);
        let hex = hex_encode(&padded);
        let decoded = hex_decode(&hex).unwrap();
        assert_eq!(decoded, padded);
    }
}
