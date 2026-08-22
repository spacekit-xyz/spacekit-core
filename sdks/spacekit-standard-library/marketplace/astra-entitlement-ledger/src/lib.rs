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
    fn msg_value() -> i64;
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
/// OP_CREATE_LISTING (0x01):
///   Input:  [op][listing_id:string][file_id:string][price:u64le][token:string][pricing_type:u8][period:u64le]
///   Output: [1]
///   Only the caller DID becomes the publisher.
///
/// OP_PURCHASE (0x02):
///   Input:  [op][listing_id:string][buyer_pk_hash:32 bytes]
///   Output: [1][entitlement_id:32 bytes]
///   Requires msg_value() >= listing price. Emits "entitlement:granted".
///   `buyer_pk_hash` = SHA-256(buyer Kyber public key raw bytes); must be non-zero.
///
/// OP_VERIFY (0x03):
///   Input:  [op][entitlement_id:32 bytes][buyer_did:string][file_id:string][buyer_pk_hash:32 bytes]
///   Output: [1][status:u8]  (1=valid, 0=expired, 2=wrong_buyer, 3=wrong_file, 4=revoked, 5=wrong_pk)
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
    match input[0] {
        OP_CREATE_LISTING  => handle_create_listing(&input[1..]),
        OP_PURCHASE        => handle_purchase(&input[1..]),
        OP_VERIFY          => handle_verify(&input[1..]),
        OP_REVOKE          => handle_revoke(&input[1..]),
        OP_GET_LISTING     => handle_get_listing(&input[1..]),
        OP_GET_ENTITLEMENT => handle_get_entitlement(&input[1..]),
        OP_GRANT           => handle_grant(&input[1..]),
        _ => Err(ContractError::InvalidInput),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Listing record
// ═══════════════════════════════════════════════════════════════════════════════

struct Listing {
    publisher_did: String,
    file_id: String,
    price: u64,
    token: String,
    pricing_type: u8,
    period: u64,
    active: u8,
}

fn encode_listing(l: &Listing) -> Vec<u8> {
    let mut out = Vec::with_capacity(256);
    write_string(&mut out, &l.publisher_did);
    write_string(&mut out, &l.file_id);
    out.extend_from_slice(&l.price.to_le_bytes());
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
    let price = read_u64(data, &mut pos)?;
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
    let price       = read_u64(data, &mut pos)?;
    let token       = read_string(data, &mut pos)?;
    if pos >= data.len() { return Err(ContractError::InvalidInput); }
    let pricing_type = data[pos]; pos += 1;
    let period      = read_u64(data, &mut pos)?;

    let caller = get_caller()?;
    let key = listing_storage_key(&listing_id);

    // Prevent overwriting someone else's listing
    if let Ok(existing) = load_listing(&listing_id) {
        if existing.publisher_did != caller {
            return Err(ContractError::Unauthorized);
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

    let paid = unsafe { msg_value() } as u64;
    if paid < listing.price {
        return Err(ContractError::InsufficientPayment);
    }

    let buyer_did = get_caller()?;
    let now = unsafe { get_timestamp() } as u64;

    let expires_at = match listing.pricing_type {
        PRICING_SUBSCRIPTION if listing.period > 0 => now + listing.period,
        _ => u64::MAX, // one-time: never expires
    };

    // entitlement_id = SHA256(buyer_did ++ listing_id ++ timestamp_le)
    let entitlement_id = derive_entitlement_id(&buyer_did, &listing_id, now);

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

    if ent.status == ENT_REVOKED {
        return Ok(vec![1u8, STATUS_REVOKED]);
    }
    if ent.buyer_did != buyer_did {
        return Ok(vec![1u8, STATUS_WRONG_BUYER]);
    }

    // Resolve the listing to check file_id match
    let listing = load_listing(&ent.listing_id)?;
    if listing.file_id != file_id {
        return Ok(vec![1u8, STATUS_WRONG_FILE]);
    }

    let now = unsafe { get_timestamp() } as u64;
    if ent.expires_at != u64::MAX && now > ent.expires_at {
        return Ok(vec![1u8, STATUS_EXPIRED]);
    }

    if ent.buyer_pk_hash != [0u8; 32] {
        if buyer_pk_hash == [0u8; 32] || buyer_pk_hash != ent.buyer_pk_hash {
            return Ok(vec![1u8, STATUS_WRONG_PK]);
        }
    }

    Ok(vec![1u8, STATUS_VALID])
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

    let now = unsafe { get_timestamp() } as u64;
    let expires_at = match listing.pricing_type {
        PRICING_SUBSCRIPTION if listing.period > 0 => now + listing.period,
        _ => u64::MAX,
    };

    let entitlement_id = derive_entitlement_id(&recipient_did, &listing_id, now);

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

fn derive_entitlement_id(buyer_did: &str, listing_id: &str, timestamp: u64) -> [u8; 32] {
    let ts_bytes = timestamp.to_le_bytes();
    let mut input = Vec::with_capacity(buyer_did.len() + listing_id.len() + 8);
    input.extend_from_slice(buyer_did.as_bytes());
    input.extend_from_slice(listing_id.as_bytes());
    input.extend_from_slice(&ts_bytes);
    host_sha256(&input)
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
