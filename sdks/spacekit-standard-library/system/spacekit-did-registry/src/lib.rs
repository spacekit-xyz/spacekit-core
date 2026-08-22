#![no_std]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::vec;

use spacekit_contract_sdk::{ContractError, SpacekitContract, ContractErrorCode};
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

#[link(wasm_import_module = "spacekit_crypto")]
extern "C" {
    /// Verify a SPHINCS+-SHAKE-256-128s-simple detached signature.
    /// Returns 1 if valid, 0 if invalid, < 0 on error.
    fn sphincs_verify(
        pk_ptr: *const u8, pk_len: usize,
        msg_ptr: *const u8, msg_len: usize,
        sig_ptr: *const u8, sig_len: usize,
    ) -> i32;

    /// Compute SHA-256 hash. Writes 32 bytes to out_ptr.
    /// Returns 32 on success, < 0 on error.
    fn sha256(data_ptr: *const u8, data_len: usize, out_ptr: *mut u8) -> i32;
}

// ═══════════════════════════════════════════════════════════════════════════════
// Opcodes
// ═══════════════════════════════════════════════════════════════════════════════

const OP_REGISTER: u8 = 1;
const OP_RESOLVE: u8 = 2;
const OP_ROTATE: u8 = 3;
const OP_DEACTIVATE: u8 = 4;

/// DID document binary layout version
const DOC_VERSION: u8 = 1;

// ═══════════════════════════════════════════════════════════════════════════════
// ABI
// ═══════════════════════════════════════════════════════════════════════════════

/// ABI:
///
/// OP_REGISTER (1):
///   Input:  [op:u8][network:string][sphincs_pk:bytes][kyber_pk:bytes][sig:bytes]
///   Output: [1:u8][did:string]
///   Signature is over: [sphincs_pk ++ kyber_pk ++ network_bytes]
///
/// OP_RESOLVE (2):
///   Input:  [op:u8][did:string]
///   Output: [1:u8][doc_version:u8][did:string][sphincs_pk:bytes][kyber_pk:bytes]
///           [controller:string][nonce:u64le][active:u8][created:u64le][updated:u64le]
///
/// OP_ROTATE (3):
///   Input:  [op:u8][did:string][new_sphincs_pk:bytes][new_kyber_pk:bytes][sig:bytes]
///   Output: [1:u8]
///   Signature (by OLD sphincs key) over: [new_sphincs_pk ++ new_kyber_pk ++ nonce_le_bytes]
///
/// OP_DEACTIVATE (4):
///   Input:  [op:u8][did:string][sig:bytes]
///   Output: [1:u8]
///   Signature over: [did_bytes ++ nonce_le_bytes]
///
/// string encoding: [len:u16le][utf8_bytes]
/// bytes encoding:  [len:u16le][raw_bytes]
struct SpaceKitDidRegistry;

impl SpacekitContract for SpaceKitDidRegistry {
    type Error = ContractError;
    fn init() -> Self { SpaceKitDidRegistry }
    fn handle(&mut self, input: &[u8]) -> Result<Vec<u8>, ContractError> { handle(input) }
}

spacekit_contract!(SpaceKitDidRegistry);

// ═══════════════════════════════════════════════════════════════════════════════
// Dispatch
// ═══════════════════════════════════════════════════════════════════════════════

fn handle(input: &[u8]) -> Result<Vec<u8>, ContractError> {
    let mut cursor = 0usize;
    let op = read_u8(input, &mut cursor)?;

    match op {
        OP_REGISTER => handle_register(input, &mut cursor),
        OP_RESOLVE => handle_resolve(input, &mut cursor),
        OP_ROTATE => handle_rotate(input, &mut cursor),
        OP_DEACTIVATE => handle_deactivate(input, &mut cursor),
        _ => Err(ContractError::InvalidInput),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// REGISTER
// ═══════════════════════════════════════════════════════════════════════════════

fn handle_register(input: &[u8], cursor: &mut usize) -> Result<Vec<u8>, ContractError> {
    let network = read_string(input, cursor)?;
    let sphincs_pk = read_bytes(input, cursor)?;
    let kyber_pk = read_bytes(input, cursor)?;
    let signature = read_bytes(input, cursor)?;

    // Derive the DID address: SHA-256(sphincs_pk)[0..20] -> hex
    let address = derive_address(&sphincs_pk)?;
    let did = build_did(&network, &address);

    // Verify the DID doesn't already exist
    let doc_key = did_storage_key(&did);
    if storage_load_bytes(&doc_key, 1).is_ok() {
        return Err(ContractError::Failed);
    }

    // Verify self-signature: sign(sphincs_pk ++ kyber_pk ++ network_bytes)
    let mut msg = Vec::with_capacity(sphincs_pk.len() + kyber_pk.len() + network.len());
    msg.extend_from_slice(&sphincs_pk);
    msg.extend_from_slice(&kyber_pk);
    msg.extend_from_slice(network.as_bytes());

    if !host_sphincs_verify(&sphincs_pk, &msg, &signature) {
        return Err(ContractError::HostError);
    }

    // Store the DID document
    let doc = DidDocument {
        did: did.clone(),
        sphincs_pk,
        kyber_pk,
        controller: did.clone(),
        nonce: 0,
        active: true,
        created_at: 0,
        updated_at: 0,
    };
    let encoded = encode_did_document(&doc);
    storage_save_bytes(&doc_key, &encoded)?;

    // Emit registration event
    spacekit_contract_sdk::emit_event_bytes("did:registered", did.as_bytes());

    // Return success + DID string
    let mut out = Vec::new();
    out.push(1u8);
    write_string_to(&mut out, &did);
    Ok(out)
}

// ═══════════════════════════════════════════════════════════════════════════════
// RESOLVE
// ═══════════════════════════════════════════════════════════════════════════════

fn handle_resolve(input: &[u8], cursor: &mut usize) -> Result<Vec<u8>, ContractError> {
    let did = read_string(input, cursor)?;
    let doc_key = did_storage_key(&did);

    // 64 KB max for a DID document (Kyber PK = 1568, SPHINCS+ PK = 32, sig overhead, etc.)
    let raw = storage_load_bytes(&doc_key, 65536)?;
    let doc = decode_did_document(&raw)?;

    if !doc.active {
        return Err(ContractError::Failed);
    }

    let mut out = Vec::new();
    out.push(1u8);
    out.extend_from_slice(&encode_did_document(&doc));
    Ok(out)
}

// ═══════════════════════════════════════════════════════════════════════════════
// ROTATE
// ═══════════════════════════════════════════════════════════════════════════════

fn handle_rotate(input: &[u8], cursor: &mut usize) -> Result<Vec<u8>, ContractError> {
    let did = read_string(input, cursor)?;
    let new_sphincs_pk = read_bytes(input, cursor)?;
    let new_kyber_pk = read_bytes(input, cursor)?;
    let signature = read_bytes(input, cursor)?;

    let doc_key = did_storage_key(&did);
    let raw = storage_load_bytes(&doc_key, 65536)?;
    let mut doc = decode_did_document(&raw)?;

    if !doc.active {
        return Err(ContractError::Failed);
    }

    // Verify signature with the CURRENT (old) SPHINCS+ key
    // Message: new_sphincs_pk ++ new_kyber_pk ++ nonce_le_bytes
    let nonce_bytes = doc.nonce.to_le_bytes();
    let mut msg = Vec::with_capacity(new_sphincs_pk.len() + new_kyber_pk.len() + 8);
    msg.extend_from_slice(&new_sphincs_pk);
    msg.extend_from_slice(&new_kyber_pk);
    msg.extend_from_slice(&nonce_bytes);

    if !host_sphincs_verify(&doc.sphincs_pk, &msg, &signature) {
        return Err(ContractError::HostError);
    }

    doc.sphincs_pk = new_sphincs_pk;
    doc.kyber_pk = new_kyber_pk;
    doc.nonce += 1;
    doc.updated_at = 0; // host would supply real timestamp

    let encoded = encode_did_document(&doc);
    storage_save_bytes(&doc_key, &encoded)?;

    spacekit_contract_sdk::emit_event_bytes("did:rotated", did.as_bytes());

    Ok(vec![1u8])
}

// ═══════════════════════════════════════════════════════════════════════════════
// DEACTIVATE
// ═══════════════════════════════════════════════════════════════════════════════

fn handle_deactivate(input: &[u8], cursor: &mut usize) -> Result<Vec<u8>, ContractError> {
    let did = read_string(input, cursor)?;
    let signature = read_bytes(input, cursor)?;

    let doc_key = did_storage_key(&did);
    let raw = storage_load_bytes(&doc_key, 65536)?;
    let mut doc = decode_did_document(&raw)?;

    if !doc.active {
        return Err(ContractError::Failed);
    }

    // Verify signature: sign(did_bytes ++ nonce_le_bytes)
    let nonce_bytes = doc.nonce.to_le_bytes();
    let mut msg = Vec::with_capacity(did.len() + 8);
    msg.extend_from_slice(did.as_bytes());
    msg.extend_from_slice(&nonce_bytes);

    if !host_sphincs_verify(&doc.sphincs_pk, &msg, &signature) {
        return Err(ContractError::HostError);
    }

    doc.active = false;
    doc.nonce += 1;
    doc.updated_at = 0;

    let encoded = encode_did_document(&doc);
    storage_save_bytes(&doc_key, &encoded)?;

    spacekit_contract_sdk::emit_event_bytes("did:deactivated", did.as_bytes());

    Ok(vec![1u8])
}

// ═══════════════════════════════════════════════════════════════════════════════
// DID Document encoding
// ═══════════════════════════════════════════════════════════════════════════════

struct DidDocument {
    did: String,
    sphincs_pk: Vec<u8>,
    kyber_pk: Vec<u8>,
    controller: String,
    nonce: u64,
    active: bool,
    created_at: u64,
    updated_at: u64,
}

fn encode_did_document(doc: &DidDocument) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.push(DOC_VERSION);
    write_string_to(&mut buf, &doc.did);
    write_bytes_to(&mut buf, &doc.sphincs_pk);
    write_bytes_to(&mut buf, &doc.kyber_pk);
    write_string_to(&mut buf, &doc.controller);
    buf.extend_from_slice(&doc.nonce.to_le_bytes());
    buf.push(if doc.active { 1 } else { 0 });
    buf.extend_from_slice(&doc.created_at.to_le_bytes());
    buf.extend_from_slice(&doc.updated_at.to_le_bytes());
    buf
}

fn decode_did_document(data: &[u8]) -> Result<DidDocument, ContractError> {
    let mut cursor = 0usize;
    let version = read_u8(data, &mut cursor)?;
    if version != DOC_VERSION {
        return Err(ContractError::InvalidInput);
    }
    let did = read_string(data, &mut cursor)?;
    let sphincs_pk = read_bytes(data, &mut cursor)?;
    let kyber_pk = read_bytes(data, &mut cursor)?;
    let controller = read_string(data, &mut cursor)?;
    let nonce = read_u64(data, &mut cursor)?;
    let active = read_u8(data, &mut cursor)? != 0;
    let created_at = read_u64(data, &mut cursor)?;
    let updated_at = read_u64(data, &mut cursor)?;

    Ok(DidDocument {
        did,
        sphincs_pk,
        kyber_pk,
        controller,
        nonce,
        active,
        created_at,
        updated_at,
    })
}

// ═══════════════════════════════════════════════════════════════════════════════
// Host wrappers
// ═══════════════════════════════════════════════════════════════════════════════

fn host_sphincs_verify(pk: &[u8], msg: &[u8], sig: &[u8]) -> bool {
    let r = unsafe {
        sphincs_verify(
            pk.as_ptr(), pk.len(),
            msg.as_ptr(), msg.len(),
            sig.as_ptr(), sig.len(),
        )
    };
    r == 1
}

fn derive_address(sphincs_pk: &[u8]) -> Result<String, ContractError> {
    let mut hash = [0u8; 32];
    let r = unsafe { sha256(sphincs_pk.as_ptr(), sphincs_pk.len(), hash.as_mut_ptr()) };
    if r < 0 {
        return Err(ContractError::HostError);
    }
    // First 20 bytes -> hex (40 chars)
    Ok(bytes_to_hex(&hash[..20]))
}

fn build_did(network: &str, address: &str) -> String {
    let mut did = String::from("did:spacekit:");
    did.push_str(network);
    did.push(':');
    did.push_str(address);
    did
}

fn did_storage_key(did: &str) -> String {
    let mut key = String::from("did:document:");
    key.push_str(did);
    key
}

fn storage_save_bytes(key: &str, data: &[u8]) -> Result<(), ContractError> {
    let r = unsafe { storage_save(key.as_ptr(), key.len(), data.as_ptr(), data.len()) };
    if r >= 0 { Ok(()) } else { Err(ContractError::StorageError) }
}

fn storage_load_bytes(key: &str, max_len: usize) -> Result<Vec<u8>, ContractError> {
    let mut buf = vec![0u8; max_len];
    let n = unsafe { storage_load(key.as_ptr(), key.len(), buf.as_mut_ptr(), max_len) };
    if n <= 0 { return Err(ContractError::StorageError); }
    buf.truncate(n as usize);
    Ok(buf)
}

// ═══════════════════════════════════════════════════════════════════════════════
// IO helpers
// ═══════════════════════════════════════════════════════════════════════════════

fn read_u8(input: &[u8], cursor: &mut usize) -> Result<u8, ContractError> {
    if *cursor >= input.len() { return Err(ContractError::InvalidInput); }
    let v = input[*cursor]; *cursor += 1; Ok(v)
}

fn read_u16(input: &[u8], cursor: &mut usize) -> Result<u16, ContractError> {
    if *cursor + 2 > input.len() { return Err(ContractError::InvalidInput); }
    let b = [input[*cursor], input[*cursor + 1]]; *cursor += 2; Ok(u16::from_le_bytes(b))
}

fn read_u64(input: &[u8], cursor: &mut usize) -> Result<u64, ContractError> {
    if *cursor + 8 > input.len() { return Err(ContractError::InvalidInput); }
    let mut b = [0u8; 8];
    b.copy_from_slice(&input[*cursor..*cursor + 8]);
    *cursor += 8;
    Ok(u64::from_le_bytes(b))
}

fn read_string(input: &[u8], cursor: &mut usize) -> Result<String, ContractError> {
    let len = read_u16(input, cursor)? as usize;
    if *cursor + len > input.len() { return Err(ContractError::InvalidInput); }
    let slice = &input[*cursor..*cursor + len]; *cursor += len;
    core::str::from_utf8(slice).map(|s| s.to_string()).map_err(|_| ContractError::InvalidInput)
}

fn read_bytes(input: &[u8], cursor: &mut usize) -> Result<Vec<u8>, ContractError> {
    let len = read_u16(input, cursor)? as usize;
    if *cursor + len > input.len() { return Err(ContractError::InvalidInput); }
    let slice = &input[*cursor..*cursor + len]; *cursor += len;
    Ok(slice.to_vec())
}

fn write_string_to(buf: &mut Vec<u8>, s: &str) {
    let len = s.len() as u16;
    buf.extend_from_slice(&len.to_le_bytes());
    buf.extend_from_slice(s.as_bytes());
}

fn write_bytes_to(buf: &mut Vec<u8>, data: &[u8]) {
    let len = data.len() as u16;
    buf.extend_from_slice(&len.to_le_bytes());
    buf.extend_from_slice(data);
}

const HEX_CHARS: &[u8; 16] = b"0123456789abcdef";

fn bytes_to_hex(bytes: &[u8]) -> String {
    let mut hex = Vec::with_capacity(bytes.len() * 2);
    for &b in bytes {
        hex.push(HEX_CHARS[(b >> 4) as usize]);
        hex.push(HEX_CHARS[(b & 0x0f) as usize]);
    }
    unsafe { String::from_utf8_unchecked(hex) }
}
