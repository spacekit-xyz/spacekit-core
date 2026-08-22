//! AstraRewards contract wire encoding (host → WASM).
//!
//! Spec: `spacekit-tokenomics/AstraRewards_Contract_Spec.md`

/// Opcode: initialize treasury allocation (genesis-only).
pub const OP_INIT: u8 = 0x01;
/// Opcode: SRA credit (admin-only).
pub const OP_CREDIT: u8 = 0x10;
/// Opcode: read total emitted.
pub const OP_GET_TOTAL_EMITTED: u8 = 0x32;

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

/// Encode GET_TOTAL_EMITTED (empty payload after opcode).
pub fn encode_get_total_emitted() -> Vec<u8> {
    vec![OP_GET_TOTAL_EMITTED]
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
    fn credit_payload_length() {
        let p = encode_credit([1u8; 32], 100, [2u8; 32]);
        assert_eq!(p.len(), 1 + 32 + 16 + 32);
        assert_eq!(p[0], OP_CREDIT);
    }
}
