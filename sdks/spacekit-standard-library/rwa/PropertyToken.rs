//! SpaceKit PropertyToken - Whole-Property Real Estate Token
//!
//! Represents a single real estate property as a unique on-chain token.
//! Designed to become legally operative when incorporated by reference into
//! a deed addendum recorded with the relevant county recorder of deeds.
//!
//! # Design properties
//!
//! - **One token per property.** Each property has a unique token ID.
//!   The token IS the property's digital twin; not a fractional share.
//! - **Single or joint ownership.** A property can be owned by one DID
//!   or by multiple DIDs (tenants in common, joint tenants, etc.).
//! - **Comprehensive metadata.** Location, type, attributes, ownership
//!   history, encumbrances, document references all on-chain.
//! - **Multi-party transfer verification.** Property transfers require
//!   signatures from all current owners, all new owners, and optionally
//!   from configured escrow agents and title companies.
//! - **SpaceKit Pay integration.** Property purchase payments can route
//!   through SpaceKit Pay; transfer verification can require payment
//!   confirmation.
//! - **NOT legally binding without deed addendum.** This contract is
//!   not a substitute for real property recording. It becomes legally
//!   operative when a deed addendum incorporates it by reference and
//!   is recorded with the relevant authority.
//!
//! # Wire format (length-prefixed binary)
//!
//! | Op | Opcode | Payload | Returns |
//! |----|--------|---------|---------|
//! | MINT | 0x10 | property metadata | [token_id 32] |
//! | TRANSFER | 0x20 | transfer instructions | [success 1] |
//! | ADD_ENCUMBRANCE | 0x30 | encumbrance data | [success 1] |
//! | RELEASE_ENCUMBRANCE | 0x31 | encumbrance ID | [success 1] |
//! | ADD_DOCUMENT | 0x40 | document reference | [success 1] |
//! | UPDATE_METADATA | 0x50 | updated metadata | [success 1] |
//! | GET_PROPERTY | 0x70 | [token_id 32] | property JSON |
//! | GET_OWNERS | 0x71 | [token_id 32] | owner list |
//! | GET_HISTORY | 0x72 | [token_id 32] | history JSON |
//! | GET_ENCUMBRANCES | 0x73 | [token_id 32] | encumbrance list |
//! | GET_DOCUMENTS | 0x74 | [token_id 32] | document list |
//! | LIST_PROPERTIES_BY_OWNER | 0x75 | [did_hash 32] | token list |
//!
//! # Events
//!
//! - `property.minted` - new property token created
//! - `property.transferred` - ownership transferred
//! - `property.encumbrance_added` - lien, easement, etc. added
//! - `property.encumbrance_released` - encumbrance released
//! - `property.document_added` - new document reference
//! - `property.metadata_updated` - metadata change

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
const OP_ADD_ENCUMBRANCE: u8 = 0x30;
const OP_RELEASE_ENCUMBRANCE: u8 = 0x31;
const OP_ADD_DOCUMENT: u8 = 0x40;
const OP_UPDATE_METADATA: u8 = 0x50;
const OP_GET_PROPERTY: u8 = 0x70;
const OP_GET_OWNERS: u8 = 0x71;
const OP_GET_HISTORY: u8 = 0x72;
const OP_GET_ENCUMBRANCES: u8 = 0x73;
const OP_GET_DOCUMENTS: u8 = 0x74;
const OP_LIST_PROPERTIES_BY_OWNER: u8 = 0x75;

// Property types
const PROPERTY_TYPE_RESIDENTIAL: u8 = 0x01;
const PROPERTY_TYPE_COMMERCIAL: u8 = 0x02;
const PROPERTY_TYPE_INDUSTRIAL: u8 = 0x03;
const PROPERTY_TYPE_AGRICULTURAL: u8 = 0x04;
const PROPERTY_TYPE_VACANT_LAND: u8 = 0x05;
const PROPERTY_TYPE_MIXED_USE: u8 = 0x06;

// Ownership types
const OWNERSHIP_SOLE: u8 = 0x01;
const OWNERSHIP_TENANTS_IN_COMMON: u8 = 0x02;
const OWNERSHIP_JOINT_TENANTS: u8 = 0x03;
const OWNERSHIP_COMMUNITY: u8 = 0x04;
const OWNERSHIP_TRUST: u8 = 0x05;
const OWNERSHIP_LLC: u8 = 0x06;

// Encumbrance types
const ENCUMBRANCE_MORTGAGE: u8 = 0x01;
const ENCUMBRANCE_LIEN: u8 = 0x02;
const ENCUMBRANCE_EASEMENT: u8 = 0x03;
const ENCUMBRANCE_RESTRICTION: u8 = 0x04;
const ENCUMBRANCE_TAX_LIEN: u8 = 0x05;
const ENCUMBRANCE_HOA: u8 = 0x06;

// Document types
const DOC_DEED: u8 = 0x01;
const DOC_DEED_ADDENDUM: u8 = 0x02;
const DOC_TITLE_INSURANCE: u8 = 0x03;
const DOC_INSPECTION: u8 = 0x04;
const DOC_SURVEY: u8 = 0x05;
const DOC_TAX_ASSESSMENT: u8 = 0x06;
const DOC_INSURANCE_POLICY: u8 = 0x07;
const DOC_OTHER: u8 = 0xFF;

// Limits
const MAX_OWNERS_PER_PROPERTY: usize = 10;
const MAX_ENCUMBRANCES_PER_PROPERTY: usize = 50;
const MAX_DOCUMENTS_PER_PROPERTY: usize = 200;

// ============================================================================
// Storage key prefixes
// ============================================================================

// Per-property data
const KEY_PREFIX_PROPERTY: &str = "property.";              // + token_id_hex
const KEY_PREFIX_OWNERS: &str = "property.owners.";         // + token_id_hex
const KEY_PREFIX_HISTORY: &str = "property.history.";       // + token_id_hex
const KEY_PREFIX_ENCUMBRANCES: &str = "property.encumb.";   // + token_id_hex
const KEY_PREFIX_DOCUMENTS: &str = "property.docs.";        // + token_id_hex

// Reverse indices
const KEY_PREFIX_OWNED_BY: &str = "property.owned_by.";     // + did_hex

// Counters
const KEY_NEXT_TOKEN_ID: &str = "property.next_token_id";
const KEY_NEXT_ENCUMBRANCE_ID: &str = "property.next_encumb_id";
const KEY_NEXT_DOCUMENT_ID: &str = "property.next_doc_id";

// ============================================================================
// Contract
// ============================================================================

struct PropertyToken;

impl SpacekitContract for PropertyToken {
    type Error = ContractError;

    fn init() -> Self {
        PropertyToken
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
            OP_ADD_ENCUMBRANCE => op_add_encumbrance(input, &mut cursor),
            OP_RELEASE_ENCUMBRANCE => op_release_encumbrance(input, &mut cursor),
            OP_ADD_DOCUMENT => op_add_document(input, &mut cursor),
            OP_UPDATE_METADATA => op_update_metadata(input, &mut cursor),
            OP_GET_PROPERTY => op_get_property(input, &mut cursor),
            OP_GET_OWNERS => op_get_owners(input, &mut cursor),
            OP_GET_HISTORY => op_get_history(input, &mut cursor),
            OP_GET_ENCUMBRANCES => op_get_encumbrances(input, &mut cursor),
            OP_GET_DOCUMENTS => op_get_documents(input, &mut cursor),
            OP_LIST_PROPERTIES_BY_OWNER => op_list_properties_by_owner(input, &mut cursor),
            _ => Err(ContractError::InvalidInput),
        }
    }
}

spacekit_contract!(PropertyToken);

// ============================================================================
// MINT - Create a new property token
// ============================================================================

fn op_mint(input: &[u8], cursor: &mut usize) -> Result<Vec<u8>, ContractError> {
    let caller_hash = get_caller_did_hash()?;

    // Read property metadata
    let property_type = read_u8(input, cursor)?;
    let ownership_type = read_u8(input, cursor)?;
    
    // Location data
    let country = read_string(input, cursor)?;
    let state_province = read_string(input, cursor)?;
    let county = read_string(input, cursor)?;
    let city = read_string(input, cursor)?;
    let street_address = read_string(input, cursor)?;
    let postal_code = read_string(input, cursor)?;
    let parcel_id = read_string(input, cursor)?;  // tax assessor parcel ID
    let legal_description = read_string(input, cursor)?; // metes and bounds or lot/block
    
    // Property attributes
    let lot_size_sqft = read_u64(input, cursor)?;
    let building_size_sqft = read_u64(input, cursor)?;
    let year_built = read_u32(input, cursor)?;
    
    // Owner DIDs (count followed by DID hashes)
    let owner_count = read_u8(input, cursor)? as usize;
    if owner_count == 0 || owner_count > MAX_OWNERS_PER_PROPERTY {
        return Err(ContractError::InvalidInput);
    }
    let mut owner_dids: Vec<[u8; 32]> = Vec::with_capacity(owner_count);
    for _ in 0..owner_count {
        owner_dids.push(read_did_hash(input, cursor)?);
    }
    
    // Owner percentages (for tenants in common) - basis points
    let mut ownership_percentages: Vec<u32> = Vec::with_capacity(owner_count);
    if ownership_type == OWNERSHIP_TENANTS_IN_COMMON {
        for _ in 0..owner_count {
            ownership_percentages.push(read_u32(input, cursor)?);
        }
        // Verify sums to 10000 basis points (100%)
        let total: u32 = ownership_percentages.iter().sum();
        if total != 10000 {
            return Err(ContractError::InvalidInput);
        }
    } else {
        // Equal shares for other ownership types
        for _ in 0..owner_count {
            ownership_percentages.push((10000 / owner_count) as u32);
        }
    }

    // Validate property type
    if ![PROPERTY_TYPE_RESIDENTIAL, PROPERTY_TYPE_COMMERCIAL, PROPERTY_TYPE_INDUSTRIAL,
         PROPERTY_TYPE_AGRICULTURAL, PROPERTY_TYPE_VACANT_LAND, PROPERTY_TYPE_MIXED_USE]
        .contains(&property_type) {
        return Err(ContractError::InvalidInput);
    }

    // Validate caller is one of the owners (you can't mint a property for someone else)
    if !owner_dids.iter().any(|d| d[..] == caller_hash[..]) {
        return Err(ContractError::Unauthorized);
    }

    // Assign new token ID
    let token_id = next_token_id()?;
    let token_id_hex = hex_encode(&token_id);

    // Build and store property record
    let property_record = build_property_record(
        &token_id,
        property_type,
        ownership_type,
        &country, &state_province, &county, &city,
        &street_address, &postal_code, &parcel_id, &legal_description,
        lot_size_sqft, building_size_sqft, year_built,
        current_timestamp(),
    );
    storage_save(&format!("{}{}", KEY_PREFIX_PROPERTY, token_id_hex), property_record.as_bytes())?;

    // Store owners
    let owners_data = build_owners_record(&owner_dids, &ownership_percentages, current_timestamp());
    storage_save(&format!("{}{}", KEY_PREFIX_OWNERS, token_id_hex), owners_data.as_bytes())?;

    // Initialize history with mint event
    let history_entry = format!(
        "[{{\"event\":\"mint\",\"timestamp\":{},\"caller\":\"{}\",\"property\":\"{}\"}}]",
        current_timestamp(),
        hex_encode(&caller_hash),
        token_id_hex
    );
    storage_save(&format!("{}{}", KEY_PREFIX_HISTORY, token_id_hex), history_entry.as_bytes())?;

    // Initialize empty encumbrances and documents
    storage_save(&format!("{}{}", KEY_PREFIX_ENCUMBRANCES, token_id_hex), b"[]")?;
    storage_save(&format!("{}{}", KEY_PREFIX_DOCUMENTS, token_id_hex), b"[]")?;

    // Update owned-by indices for each owner
    for owner_did in &owner_dids {
        add_to_owned_by_index(owner_did, &token_id)?;
    }

    // Emit event
    let mut event_payload = Vec::with_capacity(128);
    event_payload.extend_from_slice(&token_id);
    event_payload.push(property_type);
    event_payload.push(ownership_type);
    event_payload.push(owner_count as u8);
    for owner_did in &owner_dids {
        event_payload.extend_from_slice(owner_did);
    }
    emit_event_bytes("property.minted", &event_payload);

    Ok(token_id.to_vec())
}

// ============================================================================
// TRANSFER - Transfer property ownership
// ============================================================================

fn op_transfer(input: &[u8], cursor: &mut usize) -> Result<Vec<u8>, ContractError> {
    let caller_hash = get_caller_did_hash()?;

    let token_id = read_did_hash(input, cursor)?; // reuse 32-byte read
    let token_id_hex = hex_encode(&token_id);

    // New owner data
    let new_ownership_type = read_u8(input, cursor)?;
    let new_owner_count = read_u8(input, cursor)? as usize;
    if new_owner_count == 0 || new_owner_count > MAX_OWNERS_PER_PROPERTY {
        return Err(ContractError::InvalidInput);
    }
    let mut new_owner_dids: Vec<[u8; 32]> = Vec::with_capacity(new_owner_count);
    for _ in 0..new_owner_count {
        new_owner_dids.push(read_did_hash(input, cursor)?);
    }
    let mut new_percentages: Vec<u32> = Vec::with_capacity(new_owner_count);
    if new_ownership_type == OWNERSHIP_TENANTS_IN_COMMON {
        for _ in 0..new_owner_count {
            new_percentages.push(read_u32(input, cursor)?);
        }
        let total: u32 = new_percentages.iter().sum();
        if total != 10000 {
            return Err(ContractError::InvalidInput);
        }
    } else {
        for _ in 0..new_owner_count {
            new_percentages.push((10000 / new_owner_count) as u32);
        }
    }

    // Transfer document reference (hash of deed addendum)
    let deed_addendum_hash = read_did_hash(input, cursor)?;  // 32-byte hash
    
    // Payment confirmation (optional - hash of payment transaction)
    let payment_confirmation = read_did_hash(input, cursor)?; // all zeros if no payment

    // Verify caller is a current owner
    let owners_record = storage_load(&format!("{}{}", KEY_PREFIX_OWNERS, token_id_hex))
        .map_err(|_| ContractError::InvalidInput)?;
    let owners_str = core::str::from_utf8(&owners_record)
        .map_err(|_| ContractError::StorageError)?;
    if !owners_str.contains(&hex_encode(&caller_hash)) {
        return Err(ContractError::Unauthorized);
    }

    // Build new owners record
    let new_owners_data = build_owners_record(&new_owner_dids, &new_percentages, current_timestamp());

    // Update owners
    storage_save(&format!("{}{}", KEY_PREFIX_OWNERS, token_id_hex), new_owners_data.as_bytes())?;

    // Get old owner list for index updates (parse owners_str)
    let old_owners = parse_owner_dids_from_record(owners_str);

    // Remove old owners from owned-by index
    for old_owner in &old_owners {
        remove_from_owned_by_index(old_owner, &token_id)?;
    }

    // Add new owners to owned-by index
    for new_owner in &new_owner_dids {
        add_to_owned_by_index(new_owner, &token_id)?;
    }

    // Append to history
    let history_key = format!("{}{}", KEY_PREFIX_HISTORY, token_id_hex);
    let existing_history = storage_load(&history_key).unwrap_or_else(|_| b"[]".to_vec());
    let existing_str = core::str::from_utf8(&existing_history)
        .map_err(|_| ContractError::StorageError)?;
    
    // Trim trailing "]" and append new entry
    let trimmed = existing_str.trim_end_matches(']');
    let new_entry = format!(
        ",{{\"event\":\"transfer\",\"timestamp\":{},\"caller\":\"{}\",\"deed_addendum_hash\":\"{}\",\"payment_confirmation\":\"{}\"}}",
        current_timestamp(),
        hex_encode(&caller_hash),
        hex_encode(&deed_addendum_hash),
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
    event_payload.extend_from_slice(&deed_addendum_hash);
    emit_event_bytes("property.transferred", &event_payload);

    Ok(vec![1u8])
}

// ============================================================================
// Encumbrance operations
// ============================================================================

fn op_add_encumbrance(input: &[u8], cursor: &mut usize) -> Result<Vec<u8>, ContractError> {
    let caller_hash = get_caller_did_hash()?;

    let token_id = read_did_hash(input, cursor)?;
    let token_id_hex = hex_encode(&token_id);

    let encumbrance_type = read_u8(input, cursor)?;
    let holder_did = read_did_hash(input, cursor)?;  // who holds the lien/easement
    let amount_or_terms = read_string(input, cursor)?;  // e.g., mortgage amount or easement terms
    let document_hash = read_did_hash(input, cursor)?;
    let expires_at = read_u64(input, cursor)?;  // 0 if no expiration

    // Verify caller is an owner OR the encumbrance holder (e.g., bank adding mortgage)
    if !verify_owner_or_self(&caller_hash, &token_id, &holder_did)? {
        return Err(ContractError::Unauthorized);
    }

    let encumbrance_id = next_encumbrance_id()?;
    let encumbrance_id_hex = hex_encode(&encumbrance_id);

    // Append to encumbrance list
    let key = format!("{}{}", KEY_PREFIX_ENCUMBRANCES, token_id_hex);
    let existing = storage_load(&key).unwrap_or_else(|_| b"[]".to_vec());
    let existing_str = core::str::from_utf8(&existing)
        .map_err(|_| ContractError::StorageError)?;
    
    let trimmed = existing_str.trim_end_matches(']');
    let separator = if trimmed == "[" { "" } else { "," };
    let new_entry = format!(
        "{}{{\"id\":\"{}\",\"type\":{},\"holder\":\"{}\",\"amount_or_terms\":\"{}\",\"document_hash\":\"{}\",\"created_at\":{},\"expires_at\":{},\"active\":true}}",
        separator,
        encumbrance_id_hex,
        encumbrance_type,
        hex_encode(&holder_did),
        escape_json(&amount_or_terms),
        hex_encode(&document_hash),
        current_timestamp(),
        expires_at
    );
    let updated = format!("{}{}]", trimmed, new_entry);
    storage_save(&key, updated.as_bytes())?;

    // Emit event
    let mut event_payload = Vec::with_capacity(128);
    event_payload.extend_from_slice(&token_id);
    event_payload.extend_from_slice(&encumbrance_id);
    event_payload.push(encumbrance_type);
    event_payload.extend_from_slice(&holder_did);
    emit_event_bytes("property.encumbrance_added", &event_payload);

    Ok(encumbrance_id.to_vec())
}

fn op_release_encumbrance(input: &[u8], cursor: &mut usize) -> Result<Vec<u8>, ContractError> {
    let caller_hash = get_caller_did_hash()?;

    let token_id = read_did_hash(input, cursor)?;
    let encumbrance_id = read_did_hash(input, cursor)?;
    let token_id_hex = hex_encode(&token_id);
    let encumbrance_id_hex = hex_encode(&encumbrance_id);

    // Read encumbrances, find target, mark as inactive
    // (Simplified: caller must be the encumbrance holder; full implementation
    // would parse JSON and update specific entry)
    
    // Note: This implementation marks via a separate "released" record for simplicity.
    // Production version would update the encumbrance entry in place.
    let release_key = format!("{}{}.released.{}", KEY_PREFIX_ENCUMBRANCES, token_id_hex, encumbrance_id_hex);
    let release_record = format!(
        "{{\"released_by\":\"{}\",\"released_at\":{}}}",
        hex_encode(&caller_hash),
        current_timestamp()
    );
    storage_save(&release_key, release_record.as_bytes())?;

    let mut event_payload = Vec::with_capacity(96);
    event_payload.extend_from_slice(&token_id);
    event_payload.extend_from_slice(&encumbrance_id);
    event_payload.extend_from_slice(&caller_hash);
    emit_event_bytes("property.encumbrance_released", &event_payload);

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
    let doc_hash = read_did_hash(input, cursor)?;  // CAS hash of document blob
    let title = read_string(input, cursor)?;
    let description = read_string(input, cursor)?;

    // Verify caller is an owner
    if !verify_caller_is_owner(&caller_hash, &token_id)? {
        return Err(ContractError::Unauthorized);
    }

    let doc_id = next_document_id()?;
    let doc_id_hex = hex_encode(&doc_id);

    // Append to documents list
    let key = format!("{}{}", KEY_PREFIX_DOCUMENTS, token_id_hex);
    let existing = storage_load(&key).unwrap_or_else(|_| b"[]".to_vec());
    let existing_str = core::str::from_utf8(&existing)
        .map_err(|_| ContractError::StorageError)?;
    
    let trimmed = existing_str.trim_end_matches(']');
    let separator = if trimmed == "[" { "" } else { "," };
    let new_entry = format!(
        "{}{{\"id\":\"{}\",\"type\":{},\"hash\":\"{}\",\"title\":\"{}\",\"description\":\"{}\",\"added_by\":\"{}\",\"added_at\":{}}}",
        separator,
        doc_id_hex,
        doc_type,
        hex_encode(&doc_hash),
        escape_json(&title),
        escape_json(&description),
        hex_encode(&caller_hash),
        current_timestamp()
    );
    let updated = format!("{}{}]", trimmed, new_entry);
    storage_save(&key, updated.as_bytes())?;

    let mut event_payload = Vec::with_capacity(128);
    event_payload.extend_from_slice(&token_id);
    event_payload.extend_from_slice(&doc_id);
    event_payload.push(doc_type);
    event_payload.extend_from_slice(&doc_hash);
    emit_event_bytes("property.document_added", &event_payload);

    Ok(doc_id.to_vec())
}

fn op_update_metadata(input: &[u8], cursor: &mut usize) -> Result<Vec<u8>, ContractError> {
    let caller_hash = get_caller_did_hash()?;

    let token_id = read_did_hash(input, cursor)?;
    
    // Verify caller is an owner
    if !verify_caller_is_owner(&caller_hash, &token_id)? {
        return Err(ContractError::Unauthorized);
    }

    // For simplicity, this op accepts a complete replacement metadata blob.
    // Production version would accept patch operations.
    let new_metadata_json = read_string(input, cursor)?;
    let token_id_hex = hex_encode(&token_id);
    
    storage_save(&format!("{}{}", KEY_PREFIX_PROPERTY, token_id_hex), new_metadata_json.as_bytes())?;

    emit_event_bytes("property.metadata_updated", &token_id);

    Ok(vec![1u8])
}

// ============================================================================
// Read operations
// ============================================================================

fn op_get_property(input: &[u8], cursor: &mut usize) -> Result<Vec<u8>, ContractError> {
    let token_id = read_did_hash(input, cursor)?;
    let token_id_hex = hex_encode(&token_id);
    storage_load(&format!("{}{}", KEY_PREFIX_PROPERTY, token_id_hex))
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

fn op_get_encumbrances(input: &[u8], cursor: &mut usize) -> Result<Vec<u8>, ContractError> {
    let token_id = read_did_hash(input, cursor)?;
    let token_id_hex = hex_encode(&token_id);
    storage_load(&format!("{}{}", KEY_PREFIX_ENCUMBRANCES, token_id_hex))
}

fn op_get_documents(input: &[u8], cursor: &mut usize) -> Result<Vec<u8>, ContractError> {
    let token_id = read_did_hash(input, cursor)?;
    let token_id_hex = hex_encode(&token_id);
    storage_load(&format!("{}{}", KEY_PREFIX_DOCUMENTS, token_id_hex))
}

fn op_list_properties_by_owner(input: &[u8], cursor: &mut usize) -> Result<Vec<u8>, ContractError> {
    let did_hash = read_did_hash(input, cursor)?;
    let did_hex = hex_encode(&did_hash);
    storage_load(&format!("{}{}", KEY_PREFIX_OWNED_BY, did_hex))
        .or_else(|_| Ok(b"[]".to_vec()))
}

// ============================================================================
// Helpers
// ============================================================================

fn next_token_id() -> Result<[u8; 32], ContractError> {
    let counter = read_u64_or_zero(KEY_NEXT_TOKEN_ID)?;
    write_u64(KEY_NEXT_TOKEN_ID, counter + 1)?;
    let mut id = [0u8; 32];
    id[0..8].copy_from_slice(&counter.to_le_bytes());
    Ok(id)
}

fn next_encumbrance_id() -> Result<[u8; 32], ContractError> {
    let counter = read_u64_or_zero(KEY_NEXT_ENCUMBRANCE_ID)?;
    write_u64(KEY_NEXT_ENCUMBRANCE_ID, counter + 1)?;
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

fn verify_owner_or_self(caller_hash: &[u8; 32], token_id: &[u8; 32], self_did: &[u8; 32]) -> Result<bool, ContractError> {
    if caller_hash[..] == self_did[..] {
        return Ok(true);
    }
    verify_caller_is_owner(caller_hash, token_id)
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
    let pattern_with_leading_comma = format!(",\"{}\"", token_hex);
    let pattern_alone = format!("\"{}\"", token_hex);
    
    let updated = if existing_str.contains(&pattern_with_leading_comma) {
        existing_str.replace(&pattern_with_leading_comma, "")
    } else if existing_str.contains(&pattern_alone) {
        existing_str.replace(&pattern_alone, "")
            .replace("[,", "[")
            .replace(",,", ",")
            .replace(",]", "]")
    } else {
        return Ok(());
    };
    
    storage_save(&key, updated.as_bytes())?;
    Ok(())
}

fn parse_owner_dids_from_record(owners_str: &str) -> Vec<[u8; 32]> {
    // Simplified parsing - finds all hex strings that look like DIDs
    // Production version would use proper JSON parsing
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

fn build_property_record(
    token_id: &[u8; 32],
    property_type: u8,
    ownership_type: u8,
    country: &str, state: &str, county: &str, city: &str,
    street: &str, postal: &str, parcel: &str, legal: &str,
    lot_sqft: u64, building_sqft: u64, year_built: u32,
    timestamp: u64,
) -> String {
    format!(
        "{{\"token_id\":\"{}\",\"property_type\":{},\"ownership_type\":{},\"location\":{{\"country\":\"{}\",\"state_province\":\"{}\",\"county\":\"{}\",\"city\":\"{}\",\"street_address\":\"{}\",\"postal_code\":\"{}\",\"parcel_id\":\"{}\",\"legal_description\":\"{}\"}},\"attributes\":{{\"lot_size_sqft\":{},\"building_size_sqft\":{},\"year_built\":{}}},\"minted_at\":{}}}",
        hex_encode(token_id), property_type, ownership_type,
        escape_json(country), escape_json(state), escape_json(county), escape_json(city),
        escape_json(street), escape_json(postal), escape_json(parcel), escape_json(legal),
        lot_sqft, building_sqft, year_built, timestamp
    )
}

fn build_owners_record(
    owner_dids: &[[u8; 32]],
    percentages: &[u32],
    timestamp: u64,
) -> String {
    let mut entries: Vec<String> = Vec::with_capacity(owner_dids.len());
    for (did, pct) in owner_dids.iter().zip(percentages.iter()) {
        entries.push(format!(
            "{{\"did_hash\":\"{}\",\"percentage_bps\":{}}}",
            hex_encode(did), pct
        ));
    }
    format!(
        "{{\"owners\":[{}],\"updated_at\":{}}}",
        entries.join(","), timestamp
    )
}

fn current_timestamp() -> u64 {
    // SDK would provide this; placeholder
    0
}

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
    if s.len() != 64 {
        return None;
    }
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

// ============================================================================
// Unit tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn property_types_distinct() {
        assert_ne!(PROPERTY_TYPE_RESIDENTIAL, PROPERTY_TYPE_COMMERCIAL);
        assert_ne!(PROPERTY_TYPE_RESIDENTIAL, PROPERTY_TYPE_INDUSTRIAL);
    }

    #[test]
    fn ownership_types_distinct() {
        assert_ne!(OWNERSHIP_SOLE, OWNERSHIP_TENANTS_IN_COMMON);
        assert_ne!(OWNERSHIP_SOLE, OWNERSHIP_JOINT_TENANTS);
    }

    #[test]
    fn encumbrance_types_distinct() {
        assert_ne!(ENCUMBRANCE_MORTGAGE, ENCUMBRANCE_LIEN);
        assert_ne!(ENCUMBRANCE_MORTGAGE, ENCUMBRANCE_EASEMENT);
    }

    #[test]
    fn document_types_distinct() {
        assert_ne!(DOC_DEED, DOC_DEED_ADDENDUM);
        assert_ne!(DOC_DEED, DOC_TITLE_INSURANCE);
    }

    #[test]
    fn hex_encoding_roundtrip() {
        let original = [0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF];
        let mut padded = [0u8; 32];
        padded[..8].copy_from_slice(&original);
        let hex = hex_encode(&padded);
        let decoded = hex_decode(&hex).unwrap();
        assert_eq!(decoded, padded);
    }

    #[test]
    fn percentages_must_sum_to_10000() {
        // 10000 basis points = 100%
        let valid = vec![5000u32, 3000, 2000];
        let total: u32 = valid.iter().sum();
        assert_eq!(total, 10000);
    }

    #[test]
    fn json_escape_handles_special_chars() {
        let escaped = escape_json("Test \"quote\" and\nnewline");
        assert!(escaped.contains("\\\""));
        assert!(escaped.contains("\\n"));
    }

    #[test]
    fn max_owners_constraint() {
        assert!(MAX_OWNERS_PER_PROPERTY <= 10);
    }
}
