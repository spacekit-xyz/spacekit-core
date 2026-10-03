#![no_std]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::vec;

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
    /// Value attached to this call, native ASTRA wei, 16 bytes LE.
    fn msg_value_u128(out_ptr: *mut u8) -> i32;
    /// Pay from this contract's own balance. 0 = ok.
    fn transfer_u128(to_ptr: *const u8, amount_ptr: *const u8) -> i32;
    /// The block's timestamp (Unix seconds).
    fn get_timestamp() -> i64;
}

#[link(wasm_import_module = "spacekit_crypto")]
extern "C" {
    fn sha256(data_ptr: *const u8, data_len: usize, out_ptr: *mut u8) -> i32;
}

// ═══════════════════════════════════════════════════════════════════════════════
// Opcodes
// ═══════════════════════════════════════════════════════════════════════════════

const OP_CREATE_LISTING: u8    = 0x01;
const OP_PURCHASE: u8          = 0x02;
const OP_VERIFY: u8            = 0x03;
const OP_REVOKE: u8            = 0x04;
const OP_GET_LISTING: u8       = 0x05;
const OP_GET_ENTITLEMENT: u8   = 0x06;
/// Publisher-only grant (owner approve) — no payment required.
const OP_GRANT: u8             = 0x07;
/// VERIFY bound to an expected listing (use this, not OP_VERIFY).
const OP_VERIFY_LISTING: u8    = 0x08;
/// Extend a subscription entitlement by one period (pays the price).
const OP_RENEW: u8             = 0x09;

/// Pricing types matching `AppPricing` variants.
const PRICING_ONE_TIME: u8     = 1;
const PRICING_SUBSCRIPTION: u8 = 2;

/// Entitlement status values returned by OP_VERIFY.
const STATUS_VALID: u8         = 1;
const STATUS_EXPIRED: u8       = 0;
const STATUS_WRONG_BUYER: u8   = 2;
const STATUS_WRONG_FILE: u8    = 3;
const STATUS_REVOKED: u8       = 4;
const STATUS_WRONG_PK: u8      = 5;
const STATUS_WRONG_LISTING: u8 = 6;

/// Internal entitlement record status byte.
const ENT_ACTIVE: u8           = 1;
const ENT_REVOKED: u8          = 0;

const MAX_DID_LEN: usize       = 128;
const MAX_STRING_LEN: usize    = 512;
const LISTING_RECORD_MAX: usize = 2048;
const ENT_RECORD_MAX: usize    = 1024;

// ═══════════════════════════════════════════════════════════════════════════════
// Contract entry point
// ═══════════════════════════════════════════════════════════════════════════════

/// ABI:
///
/// Amounts are native ASTRA in wei (u128, 16 bytes LE). Payments go straight
/// to the publisher's address (the address in their `did:spacekit:<hex>`);
/// the contract never holds funds. Times are the block's timestamp.
///
/// OP_CREATE_LISTING (0x01):
///   Input:  [op][listing_id:string][file_id:string][price:u128le][token:string][pricing_type:u8][period:u64le]
///   Output: [1]
///   Only the caller DID becomes the publisher. A publisher may update price
///   and terms of their own listing, but not its file_id. No value accepted.
///
/// OP_PURCHASE (0x02):
///   Input:  [op][listing_id:string][buyer_pk_hash:32 bytes]
///   Output: [1][entitlement_id:32 bytes]
///   Requires value >= listing price; the whole value is paid to the
///   publisher. Emits "entitlement:granted".
///   `buyer_pk_hash` = SHA-256(buyer Kyber public key raw bytes); must be non-zero.
///
/// OP_VERIFY (0x03):
///   Input:  [op][entitlement_id:32 bytes][buyer_did:string][file_id:string][buyer_pk_hash:32 bytes]
///   Output: [1][status:u8]  (1=valid, 0=expired, 2=wrong_buyer, 3=wrong_file, 4=revoked, 5=wrong_pk)
///   Legacy: anyone can create a listing for any file_id, so a check by
///   file_id alone can be satisfied by a listing the content owner never
///   made. Use OP_VERIFY_LISTING.
///
/// OP_REVOKE (0x04):
///   Input:  [op][entitlement_id:32 bytes]
///   Output: [1]
///   Only the listing publisher can revoke.
///
/// OP_GET_LISTING (0x05):
///   Input:  [op][listing_id:string]
///   Output: [1][raw listing record bytes]
///
/// OP_GET_ENTITLEMENT (0x06):
///   Input:  [op][entitlement_id:32 bytes]
///   Output: [1][raw entitlement record bytes]
///
/// OP_GRANT (0x07):
///   Input:  [op][listing_id:string][recipient_did:string][buyer_pk_hash:32 bytes]
///   Output: [1][entitlement_id:32 bytes]
///   Only the listing publisher may grant. No payment. Emits "entitlement:granted".
///   Expiry follows the listing pricing type (one-time = never; subscription = now+period).
///
/// OP_VERIFY_LISTING (0x08):
///   Input:  [op][entitlement_id:32][buyer_did:string][listing_id:string][buyer_pk_hash:32]
///   Output: [1][status:u8]  (as OP_VERIFY, plus 6=wrong_listing)
///   The caller names the listing it trusts (e.g. the one a channel or file
///   was published with) and checks the publisher via OP_GET_LISTING. An
///   all-zero buyer_pk_hash skips the key check (for callers that
///   authenticate the buyer's DID and deliver nothing to a key).
///
/// OP_RENEW (0x09):
///   Input:  [op][entitlement_id:32]
///   Output: [1][expires_at:u64le]
///   Subscription listings only. Requires value >= price (paid to the
///   publisher). Extends from max(now, expires_at) by one period; the
///   entitlement id stays the same.
struct AstraEntitlementLedger;

impl SpacekitContract for AstraEntitlementLedger {
    type Error = ContractError;
    fn init() -> Self { AstraEntitlementLedger }
    fn handle(&mut self, input: &[u8]) -> Result<Vec<u8>, ContractError> {
        dispatch(input)
    }
}

spacekit_contract!(AstraEntitlementLedger);

fn dispatch(input: &[u8]) -> Result<Vec<u8>, ContractError> {
    if input.is_empty() { return Err(ContractError::InvalidInput); }
    // Only purchases and renewals take payment; value sent with anything
    // else would be stranded in the contract, so refuse it.
    if input[0] != OP_PURCHASE && input[0] != OP_RENEW && attached_value() != 0 {
        return Err(ContractError::InvalidInput);
    }
    match input[0] {
        OP_CREATE_LISTING  => handle_create_listing(&input[1..]),
        OP_PURCHASE        => handle_purchase(&input[1..]),
        OP_VERIFY          => handle_verify(&input[1..]),
        OP_REVOKE          => handle_revoke(&input[1..]),
        OP_GET_LISTING     => handle_get_listing(&input[1..]),
        OP_GET_ENTITLEMENT => handle_get_entitlement(&input[1..]),
        OP_GRANT           => handle_grant(&input[1..]),
        OP_VERIFY_LISTING  => handle_verify_listing(&input[1..]),
        OP_RENEW           => handle_renew(&input[1..]),
        _ => Err(ContractError::InvalidInput),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Listing record
// ═══════════════════════════════════════════════════════════════════════════════

struct Listing {
    publisher_did: String,
    file_id: String,
    price: u128,
    token: String,
    pricing_type: u8,
    period: u64,
    active: u8,
}

fn encode_listing(l: &Listing) -> Vec<u8> {
    let mut out = Vec::with_capacity(256);
    write_string(&mut out, &l.publisher_did);
    write_string(&mut out, &l.file_id);
    out.extend_from_slice(&l.price.to_le_bytes()); // 16 bytes
    write_string(&mut out, &l.token);
    out.push(l.pricing_type);
    out.extend_from_slice(&l.period.to_le_bytes());
    out.push(l.active);
    out
}

fn decode_listing(data: &[u8]) -> Result<Listing, ContractError> {
    let mut pos = 0usize;
    let publisher_did = read_string(data, &mut pos)?;
    let file_id = read_string(data, &mut pos)?;
    let price = read_u128(data, &mut pos)?;
    let token = read_string(data, &mut pos)?;
    if pos >= data.len() { return Err(ContractError::InvalidInput); }
    let pricing_type = data[pos]; pos += 1;
    let period = read_u64(data, &mut pos)?;
    if pos >= data.len() { return Err(ContractError::InvalidInput); }
    let active = data[pos];
    Ok(Listing { publisher_did, file_id, price, token, pricing_type, period, active })
}

// ═══════════════════════════════════════════════════════════════════════════════
// Entitlement record
// ═══════════════════════════════════════════════════════════════════════════════

struct Entitlement {
    buyer_did: String,
    listing_id: String,
    granted_at: u64,
    expires_at: u64,
    status: u8,
    /// SHA-256(buyer Kyber public key) bound at purchase; all-zero = legacy record.
    buyer_pk_hash: [u8; 32],
}

fn encode_entitlement(e: &Entitlement) -> Vec<u8> {
    let mut out = Vec::with_capacity(256);
    write_string(&mut out, &e.buyer_did);
    write_string(&mut out, &e.listing_id);
    out.extend_from_slice(&e.granted_at.to_le_bytes());
    out.extend_from_slice(&e.expires_at.to_le_bytes());
    out.push(e.status);
    out.extend_from_slice(&e.buyer_pk_hash);
    out
}

fn decode_entitlement(data: &[u8]) -> Result<Entitlement, ContractError> {
    let mut pos = 0usize;
    let buyer_did = read_string(data, &mut pos)?;
    let listing_id = read_string(data, &mut pos)?;
    let granted_at = read_u64(data, &mut pos)?;
    let expires_at = read_u64(data, &mut pos)?;
    if pos >= data.len() { return Err(ContractError::InvalidInput); }
    let status = data[pos]; pos += 1;
    let buyer_pk_hash = if pos + 32 <= data.len() {
        read_bytes32(data, &mut pos)?
    } else {
        [0u8; 32]
    };
    Ok(Entitlement { buyer_did, listing_id, granted_at, expires_at, status, buyer_pk_hash })
}

// ═══════════════════════════════════════════════════════════════════════════════
// Handlers
// ═══════════════════════════════════════════════════════════════════════════════

fn handle_create_listing(data: &[u8]) -> Result<Vec<u8>, ContractError> {
    let mut pos = 0usize;
    let listing_id  = read_string(data, &mut pos)?;
    let file_id     = read_string(data, &mut pos)?;
    let price       = read_u128(data, &mut pos)?;
    let token       = read_string(data, &mut pos)?;
    if pos >= data.len() { return Err(ContractError::InvalidInput); }
    let pricing_type = data[pos]; pos += 1;
    let period      = read_u64(data, &mut pos)?;
    if listing_id.is_empty() || file_id.is_empty() {
        return Err(ContractError::InvalidInput);
    }
    if pricing_type != PRICING_ONE_TIME && pricing_type != PRICING_SUBSCRIPTION {
        return Err(ContractError::InvalidInput);
    }
    if pricing_type == PRICING_SUBSCRIPTION && period == 0 {
        return Err(ContractError::InvalidInput);
    }

    let caller = get_caller()?;
    // Payments go to this address, so the publisher must have one.
    publisher_address(&caller)?;
    let key = listing_storage_key(&listing_id);

    // Only the publisher may update a listing, and never to another file:
    // entitlements already sold were sold for that file.
    if let Ok(existing) = load_listing(&listing_id) {
        if existing.publisher_did != caller {
            return Err(ContractError::Unauthorized);
        }
        if existing.file_id != file_id {
            return Err(ContractError::InvalidInput);
        }
    }

    let listing = Listing {
        publisher_did: caller,
        file_id,
        price,
        token,
        pricing_type,
        period,
        active: 1,
    };
    host_storage_save(&key, &encode_listing(&listing))?;
    Ok(vec![1u8])
}

fn handle_purchase(data: &[u8]) -> Result<Vec<u8>, ContractError> {
    let mut pos = 0usize;
    let listing_id = read_string(data, &mut pos)?;
    let buyer_pk_hash = read_bytes32(data, &mut pos)?;
    if buyer_pk_hash == [0u8; 32] {
        return Err(ContractError::InvalidInput);
    }

    let listing = load_listing(&listing_id)?;
    if listing.active == 0 {
        return Err(ContractError::Failed);
    }

    let paid = attached_value();
    if paid < listing.price {
        return Err(ContractError::InsufficientPayment);
    }
    pay_publisher(&listing, paid)?;

    let buyer_did = get_caller()?;
    let now = block_time();

    let expires_at = match listing.pricing_type {
        PRICING_SUBSCRIPTION if listing.period > 0 => now.saturating_add(listing.period),
        _ => u64::MAX, // one-time: never expires
    };

    let entitlement_id = derive_entitlement_id(&buyer_did, &listing_id, now)?;

    let ent = Entitlement {
        buyer_did: buyer_did.clone(),
        listing_id: listing_id.clone(),
        granted_at: now,
        expires_at,
        status: ENT_ACTIVE,
        buyer_pk_hash,
    };
    let ent_key = entitlement_storage_key(&entitlement_id);
    host_storage_save(&ent_key, &encode_entitlement(&ent))?;

    // Emit event so indexers and the storage node can observe grants
    let mut event_data = Vec::with_capacity(32 + buyer_did.len() + listing_id.len());
    event_data.extend_from_slice(&entitlement_id);
    event_data.extend_from_slice(buyer_did.as_bytes());
    event_data.push(0); // separator
    event_data.extend_from_slice(listing_id.as_bytes());
    spacekit_contract_sdk::emit_event_bytes("entitlement:granted", &event_data);

    let mut out = Vec::with_capacity(33);
    out.push(1u8);
    out.extend_from_slice(&entitlement_id);
    Ok(out)
}

fn handle_verify(data: &[u8]) -> Result<Vec<u8>, ContractError> {
    let mut pos = 0usize;
    let entitlement_id = read_bytes32(data, &mut pos)?;
    let buyer_did = read_string(data, &mut pos)?;
    let file_id = read_string(data, &mut pos)?;
    let buyer_pk_hash = if pos + 32 <= data.len() {
        read_bytes32(data, &mut pos)?
    } else {
        [0u8; 32]
    };
    let ent = load_entitlement(&entitlement_id)?;
    let listing = load_listing(&ent.listing_id)?;
    if listing.file_id != file_id {
        return Ok(check_status(&ent, &buyer_did, &buyer_pk_hash).map_or_else(
            |s| vec![1u8, s],
            |_| vec![1u8, STATUS_WRONG_FILE],
        ));
    }
    Ok(vec![1u8, check_status(&ent, &buyer_did, &buyer_pk_hash).unwrap_or_else(|s| s)])
}

fn handle_verify_listing(data: &[u8]) -> Result<Vec<u8>, ContractError> {
    let mut pos = 0usize;
    let entitlement_id = read_bytes32(data, &mut pos)?;
    let buyer_did = read_string(data, &mut pos)?;
    let listing_id = read_string(data, &mut pos)?;
    let buyer_pk_hash = read_bytes32(data, &mut pos)?;
    let ent = load_entitlement(&entitlement_id)?;
    if ent.listing_id != listing_id {
        return Ok(vec![1u8, STATUS_WRONG_LISTING]);
    }
    // An all-zero key hash skips the key check: callers that authenticate the
    // buyer's DID themselves (and deliver nothing encrypted to a key) may not
    // know the buyer's key.
    if buyer_pk_hash == [0u8; 32] {
        return Ok(vec![1u8, check_status(&ent, &buyer_did, &ent.buyer_pk_hash).unwrap_or_else(|s| s)]);
    }
    Ok(vec![1u8, check_status(&ent, &buyer_did, &buyer_pk_hash).unwrap_or_else(|s| s)])
}

/// Ok(STATUS_VALID) or Err(the failing status), in a fixed order: revoked,
/// wrong buyer, expired, wrong key.
fn check_status(ent: &Entitlement, buyer_did: &str, buyer_pk_hash: &[u8; 32]) -> Result<u8, u8> {
    if ent.status == ENT_REVOKED {
        return Err(STATUS_REVOKED);
    }
    if ent.buyer_did != buyer_did {
        return Err(STATUS_WRONG_BUYER);
    }
    let now = block_time();
    if ent.expires_at != u64::MAX && now > ent.expires_at {
        return Err(STATUS_EXPIRED);
    }
    if ent.buyer_pk_hash != [0u8; 32]
        && (*buyer_pk_hash == [0u8; 32] || *buyer_pk_hash != ent.buyer_pk_hash)
    {
        return Err(STATUS_WRONG_PK);
    }
    Ok(STATUS_VALID)
}

fn handle_renew(data: &[u8]) -> Result<Vec<u8>, ContractError> {
    let mut pos = 0usize;
    let entitlement_id = read_bytes32(data, &mut pos)?;
    let mut ent = load_entitlement(&entitlement_id)?;
    if ent.status == ENT_REVOKED {
        return Err(ContractError::Failed);
    }
    let listing = load_listing(&ent.listing_id)?;
    if listing.active == 0 || listing.pricing_type != PRICING_SUBSCRIPTION || listing.period == 0 {
        return Err(ContractError::Failed);
    }
    let paid = attached_value();
    if paid < listing.price {
        return Err(ContractError::InsufficientPayment);
    }
    pay_publisher(&listing, paid)?;
    let now = block_time();
    ent.expires_at = ent.expires_at.max(now).saturating_add(listing.period);
    host_storage_save(&entitlement_storage_key(&entitlement_id), &encode_entitlement(&ent))?;
    spacekit_contract_sdk::emit_event_bytes("entitlement:renewed", &entitlement_id);
    let mut out = Vec::with_capacity(9);
    out.push(1u8);
    out.extend_from_slice(&ent.expires_at.to_le_bytes());
    Ok(out)
}

fn handle_revoke(data: &[u8]) -> Result<Vec<u8>, ContractError> {
    let mut pos = 0usize;
    let entitlement_id = read_bytes32(data, &mut pos)?;

    let mut ent = load_entitlement(&entitlement_id)?;
    let listing = load_listing(&ent.listing_id)?;

    let caller = get_caller()?;
    if listing.publisher_did != caller {
        return Err(ContractError::Unauthorized);
    }

    ent.status = ENT_REVOKED;
    let key = entitlement_storage_key(&entitlement_id);
    host_storage_save(&key, &encode_entitlement(&ent))?;

    spacekit_contract_sdk::emit_event_bytes("entitlement:revoked", &entitlement_id);

    Ok(vec![1u8])
}

fn handle_get_listing(data: &[u8]) -> Result<Vec<u8>, ContractError> {
    let mut pos = 0usize;
    let listing_id = read_string(data, &mut pos)?;
    let raw = host_storage_load_raw(&listing_storage_key(&listing_id), LISTING_RECORD_MAX)?;
    let mut out = Vec::with_capacity(1 + raw.len());
    out.push(1u8);
    out.extend_from_slice(&raw);
    Ok(out)
}

fn handle_get_entitlement(data: &[u8]) -> Result<Vec<u8>, ContractError> {
    let mut pos = 0usize;
    let entitlement_id = read_bytes32(data, &mut pos)?;
    let raw = host_storage_load_raw(&entitlement_storage_key(&entitlement_id), ENT_RECORD_MAX)?;
    let mut out = Vec::with_capacity(1 + raw.len());
    out.push(1u8);
    out.extend_from_slice(&raw);
    Ok(out)
}

/// Publisher approves a specific recipient for a listing (no payment).
fn handle_grant(data: &[u8]) -> Result<Vec<u8>, ContractError> {
    let mut pos = 0usize;
    let listing_id = read_string(data, &mut pos)?;
    let recipient_did = read_string(data, &mut pos)?;
    let buyer_pk_hash = read_bytes32(data, &mut pos)?;
    if buyer_pk_hash == [0u8; 32] {
        return Err(ContractError::InvalidInput);
    }
    if recipient_did.is_empty() || recipient_did.len() > MAX_DID_LEN {
        return Err(ContractError::InvalidInput);
    }

    let listing = load_listing(&listing_id)?;
    if listing.active == 0 {
        return Err(ContractError::Failed);
    }

    let caller = get_caller()?;
    if listing.publisher_did != caller {
        return Err(ContractError::Unauthorized);
    }

    let now = block_time();
    let expires_at = match listing.pricing_type {
        PRICING_SUBSCRIPTION if listing.period > 0 => now.saturating_add(listing.period),
        _ => u64::MAX,
    };

    let entitlement_id = derive_entitlement_id(&recipient_did, &listing_id, now)?;

    let ent = Entitlement {
        buyer_did: recipient_did.clone(),
        listing_id: listing_id.clone(),
        granted_at: now,
        expires_at,
        status: ENT_ACTIVE,
        buyer_pk_hash,
    };
    let ent_key = entitlement_storage_key(&entitlement_id);
    host_storage_save(&ent_key, &encode_entitlement(&ent))?;

    let mut event_data = Vec::with_capacity(32 + recipient_did.len() + listing_id.len());
    event_data.extend_from_slice(&entitlement_id);
    event_data.extend_from_slice(recipient_did.as_bytes());
    event_data.push(0);
    event_data.extend_from_slice(listing_id.as_bytes());
    spacekit_contract_sdk::emit_event_bytes("entitlement:granted", &event_data);

    let mut out = Vec::with_capacity(33);
    out.push(1u8);
    out.extend_from_slice(&entitlement_id);
    Ok(out)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════════

fn get_caller() -> Result<String, ContractError> {
    let mut buf = [0u8; MAX_DID_LEN];
    let len = unsafe { get_caller_did(buf.as_mut_ptr(), buf.len()) };
    if len <= 0 { return Err(ContractError::Unauthorized); }
    String::from_utf8(buf[..len as usize].to_vec())
        .map_err(|_| ContractError::InvalidInput)
}

/// SHA256(domain ++ buyer ++ 0 ++ listing ++ 0 ++ time ++ seq). `seq` counts
/// this buyer's entitlements, so two in the same block get distinct ids.
fn derive_entitlement_id(buyer_did: &str, listing_id: &str, timestamp: u64) -> Result<[u8; 32], ContractError> {
    let mut seq_key = String::from("seq:");
    seq_key.push_str(buyer_did);
    let seq = host_storage_load_raw(&seq_key, 8)
        .ok()
        .and_then(|b| <[u8; 8]>::try_from(b.as_slice()).ok())
        .map(u64::from_le_bytes)
        .unwrap_or(0);
    host_storage_save(&seq_key, &(seq + 1).to_le_bytes())?;
    let mut input = Vec::with_capacity(32 + buyer_did.len() + listing_id.len() + 18);
    input.extend_from_slice(b"SPACEKIT-ENTITLEMENT-v2");
    input.extend_from_slice(buyer_did.as_bytes());
    input.push(0);
    input.extend_from_slice(listing_id.as_bytes());
    input.push(0);
    input.extend_from_slice(&timestamp.to_le_bytes());
    input.extend_from_slice(&seq.to_le_bytes());
    Ok(host_sha256(&input))
}

fn attached_value() -> u128 {
    let mut out = [0u8; 16];
    if unsafe { msg_value_u128(out.as_mut_ptr()) } != 0 {
        return 0;
    }
    u128::from_le_bytes(out)
}

fn block_time() -> u64 {
    let t = unsafe { get_timestamp() };
    if t < 0 { 0 } else { t as u64 }
}

/// The 20-byte address in `did:spacekit:<40 hex>`.
fn publisher_address(did: &str) -> Result<[u8; 20], ContractError> {
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

/// Forward the whole payment to the publisher. The value arrived in this
/// contract's balance with the call; nothing stays behind.
fn pay_publisher(listing: &Listing, amount: u128) -> Result<(), ContractError> {
    if amount == 0 {
        return Ok(());
    }
    let to = publisher_address(&listing.publisher_did)?;
    let amount_bytes = amount.to_le_bytes();
    if unsafe { transfer_u128(to.as_ptr(), amount_bytes.as_ptr()) } != 0 {
        return Err(ContractError::Failed);
    }
    Ok(())
}

fn listing_storage_key(listing_id: &str) -> String {
    let mut k = String::from("listing:");
    k.push_str(listing_id);
    k
}

fn entitlement_storage_key(ent_id: &[u8; 32]) -> String {
    let mut k = String::from("ent:");
    k.push_str(&hex_encode(ent_id));
    k
}

fn load_listing(listing_id: &str) -> Result<Listing, ContractError> {
    let key = listing_storage_key(listing_id);
    let raw = host_storage_load_raw(&key, LISTING_RECORD_MAX)?;
    decode_listing(&raw)
}

fn load_entitlement(ent_id: &[u8; 32]) -> Result<Entitlement, ContractError> {
    let key = entitlement_storage_key(ent_id);
    let raw = host_storage_load_raw(&key, ENT_RECORD_MAX)?;
    decode_entitlement(&raw)
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

fn host_sha256(data: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    unsafe { sha256(data.as_ptr(), data.len(), out.as_mut_ptr()); }
    out
}

// ═══════════════════════════════════════════════════════════════════════════════
// Wire format helpers
// ═══════════════════════════════════════════════════════════════════════════════

fn read_u8(input: &[u8], pos: &mut usize) -> Result<u8, ContractError> {
    if *pos >= input.len() { return Err(ContractError::InvalidInput); }
    let v = input[*pos]; *pos += 1; Ok(v)
}

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

fn read_u128(input: &[u8], pos: &mut usize) -> Result<u128, ContractError> {
    if *pos + 16 > input.len() { return Err(ContractError::InvalidInput); }
    let mut b = [0u8; 16];
    b.copy_from_slice(&input[*pos..*pos + 16]);
    *pos += 16;
    Ok(u128::from_le_bytes(b))
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

fn read_bytes32(input: &[u8], pos: &mut usize) -> Result<[u8; 32], ContractError> {
    if *pos + 32 > input.len() { return Err(ContractError::InvalidInput); }
    let mut out = [0u8; 32];
    out.copy_from_slice(&input[*pos..*pos + 32]);
    *pos += 32;
    Ok(out)
}

fn write_string(out: &mut Vec<u8>, s: &str) {
    let len = s.len() as u16;
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(s.as_bytes());
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0xf) as usize] as char);
    }
    s
}
