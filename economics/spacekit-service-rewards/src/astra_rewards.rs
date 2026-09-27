//! AstraRewards contract wire encoding (host → WASM).
//!
//! Spec: `spacekit-tokenomics/AstraRewards_Contract_Spec.md`

/// Opcode: initialize treasury allocation (genesis-only).
pub const OP_INIT: u8 = 0x01;
/// Opcode: SRA credit (admin-only).
pub const OP_CREDIT: u8 = 0x10;
/// Opcode: SRA credit into the recipient's locked balance while in proof of
/// authority (admin-only); credits normally after END_POA.
pub const OP_CREDIT_LOCKED: u8 = 0x11;
/// Opcode: read total emitted.
pub const OP_GET_TOTAL_EMITTED: u8 = 0x32;
/// Opcode: read `[locked 16][released 16][releasable 16]` for a DID.
pub const OP_GET_LOCKED: u8 = 0x35;
/// Opcode: read `[phase 1][genesis 8][cliff 8][end 8][pos_activated 8]`.
pub const OP_GET_PHASE: u8 = 0x36;
/// Opcode: mark/unmark a DID whose proof-of-authority credits are locked (admin-only).
pub const OP_SET_LOCKED_RECIPIENT: u8 = 0x40;
/// Opcode: end proof of authority; later credits are unlocked (admin-only, irreversible).
pub const OP_END_POA: u8 = 0x41;
/// Opcode: release a DID's vested locked ASTRA into its spendable balance (anyone).
pub const OP_RELEASE: u8 = 0x42;

/// Well-known treasury DID for genesis INIT.
pub const TREASURY_DID: &str = "did:spacekit:network:treasury";

/// Encode INIT payload: `[treasury_did_hash 32]`.
pub fn encode_init(treasury_did_hash: [u8; 32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(1 + 32);
    out.push(OP_INIT);
    out.extend_from_slice(&treasury_did_hash);
    out
}

/// Encode CREDIT payload: `[recipient 32][amount 16 LE][log_event_hash 32]`.
pub fn encode_credit(
    recipient_did_hash: [u8; 32],
    amount_wei: u128,
    log_event_hash: [u8; 32],
) -> Vec<u8> {
    let mut out = Vec::with_capacity(1 + 32 + 16 + 32);
    out.push(OP_CREDIT);
    out.extend_from_slice(&recipient_did_hash);
    out.extend_from_slice(&amount_wei.to_le_bytes());
    out.extend_from_slice(&log_event_hash);
    out
}

/// Encode CREDIT_LOCKED: same payload as CREDIT.
pub fn encode_credit_locked(
    recipient_did_hash: [u8; 32],
    amount_wei: u128,
    log_event_hash: [u8; 32],
) -> Vec<u8> {
    let mut out = encode_credit(recipient_did_hash, amount_wei, log_event_hash);
    out[0] = OP_CREDIT_LOCKED;
    out
}

/// Encode GET_TOTAL_EMITTED (empty payload after opcode).
pub fn encode_get_total_emitted() -> Vec<u8> {
    vec![OP_GET_TOTAL_EMITTED]
}

/// Encode SET_LOCKED_RECIPIENT: `[did_hash 32][flag 1]`.
pub fn encode_set_locked_recipient(did_hash: [u8; 32], locked: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(1 + 32 + 1);
    out.push(OP_SET_LOCKED_RECIPIENT);
    out.extend_from_slice(&did_hash);
    out.push(u8::from(locked));
    out
}

/// Encode END_POA (empty payload after opcode).
pub fn encode_end_poa() -> Vec<u8> {
    vec![OP_END_POA]
}

/// Encode RELEASE: `[did_hash 32]`.
pub fn encode_release(did_hash: [u8; 32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(1 + 32);
    out.push(OP_RELEASE);
    out.extend_from_slice(&did_hash);
    out
}

/// Encode GET_LOCKED: `[did_hash 32]`.
pub fn encode_get_locked(did_hash: [u8; 32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(1 + 32);
    out.push(OP_GET_LOCKED);
    out.extend_from_slice(&did_hash);
    out
}

/// Every 32-byte recipient key SRA credits may use for `did`.
///
/// SRA credits are keyed either by the FNV DID hash (contract-side identity)
/// or, for the devnet bridge, by the operator's 20-byte SwtchVM address padded
/// to 32 bytes. A `did:spacekit:<network>:<40 hex>` DID's suffix is that
/// address, so both keys are returned and both must be locked.
pub fn lock_recipient_hashes(did: &str) -> Vec<[u8; 32]> {
    let did = did.trim();
    let mut out = vec![hash_did_bytes(did.as_bytes())];
    if let Some(suffix) = did.rsplit(':').next() {
        let hex = suffix.strip_prefix("0x").unwrap_or(suffix);
        if hex.len() == 40 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            let mut addr = [0u8; 20];
            for (i, chunk) in hex.as_bytes().chunks(2).enumerate() {
                let s = core::str::from_utf8(chunk).unwrap_or("00");
                addr[i] = u8::from_str_radix(s, 16).unwrap_or(0);
            }
            let mut padded = [0u8; 32];
            padded[12..32].copy_from_slice(&addr);
            if !out.contains(&padded) {
                out.push(padded);
            }
        }
    }
    out
}

/// UTF-8 topic label padded/truncated to 32 bytes (SwtchVM log topic0).
pub fn topic_label_bytes(label: &str) -> [u8; 32] {
    let mut topic = [0u8; 32];
    let bytes = label.as_bytes();
    let n = bytes.len().min(32);
    topic[..n].copy_from_slice(&bytes[..n]);
    topic
}

/// FNV-1a DID hash (matches `spacekit-contract-sdk::hash_did_bytes`).
pub fn hash_did_bytes(did: &[u8]) -> [u8; 32] {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &b in did {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    let mut out = [0u8; 32];
    out[..8].copy_from_slice(&hash.to_le_bytes());
    for (i, &b) in did.iter().enumerate() {
        out[8 + (i % 24)] ^= b;
    }
    out
}

pub fn treasury_did_hash() -> [u8; 32] {
    hash_did_bytes(TREASURY_DID.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_payloads() {
        let p = encode_set_locked_recipient([3u8; 32], true);
        assert_eq!(p.len(), 34);
        assert_eq!((p[0], p[33]), (OP_SET_LOCKED_RECIPIENT, 1));
        assert_eq!(encode_end_poa(), vec![OP_END_POA]);
        assert_eq!(encode_release([4u8; 32]).len(), 33);
    }

    #[test]
    fn lock_recipient_hashes_cover_did_and_address() {
        let did = "did:spacekit:testnet:00112233445566778899aabbccddeeff00112233";
        let hashes = lock_recipient_hashes(did);
        assert_eq!(hashes.len(), 2);
        assert_eq!(hashes[0], hash_did_bytes(did.as_bytes()));
        assert_eq!(hashes[1][..12], [0u8; 12]);
        assert_eq!(hashes[1][12], 0x00);
        assert_eq!(hashes[1][13], 0x11);
        assert_eq!(hashes[1][31], 0x33);
        assert_eq!(lock_recipient_hashes("did:spacekit:user:alice").len(), 1);
    }

    #[test]
    fn credit_payload_length() {
        let p = encode_credit([1u8; 32], 100, [2u8; 32]);
        assert_eq!(p.len(), 1 + 32 + 16 + 32);
        assert_eq!(p[0], OP_CREDIT);
    }
}
