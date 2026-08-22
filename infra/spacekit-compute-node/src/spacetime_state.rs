//! Validator fingerprint storage in SwtchVM state Verkle (long-lived evidence).
//!
//! Uses [`FingerprintCommitment`] wire format and [`FINGERPRINT_NAMESPACE`] from
//! `fingerprint_verkle.rs`. Full payload in `contract_kv`; digest in Verkle storage.

#[cfg(feature = "spacetime-consensus")]
mod inner {
    use alloy_primitives::{keccak256, B256};
    use spacekit_spacetime_consensus::{
        FingerprintCommitment, Rotor, RotorFingerprint, FINGERPRINT_NAMESPACE,
    };

    use crate::spacekitvm::swtchvm_node::{SwtchvmAddress, SwtchvmState};

    /// System account matching [`FINGERPRINT_NAMESPACE`] (`0xFF…FE`).
    pub fn spacetime_fingerprint_account() -> SwtchvmAddress {
        let mut bytes = [0u8; 20];
        bytes.copy_from_slice(FINGERPRINT_NAMESPACE.as_ref());
        SwtchvmAddress::new(bytes)
    }

    fn kv_key(validator_id: B256) -> Vec<u8> {
        let mut buf = b"spacekit-fingerprint-kv-v1".to_vec();
        buf.extend_from_slice(validator_id.as_slice());
        buf
    }

    /// Verkle storage key: validator DID hash (32 bytes), per `FingerprintVerkle::apply_batch`.
    fn verkle_storage_key(validator_id: B256) -> [u8; 32] {
        validator_id.0
    }

    pub fn get_validator_fingerprint(
        state: &SwtchvmState,
        validator_id: B256,
    ) -> Option<RotorFingerprint> {
        let bytes = state
            .contract_kv
            .get(&(spacetime_fingerprint_account(), kv_key(validator_id)))?;
        FingerprintCommitment::from_bytes(bytes)?.to_fingerprint()
    }

    pub fn get_validator_commitment(
        state: &SwtchvmState,
        validator_id: B256,
    ) -> Option<FingerprintCommitment> {
        let bytes = state
            .contract_kv
            .get(&(spacetime_fingerprint_account(), kv_key(validator_id)))?;
        FingerprintCommitment::from_bytes(bytes)
    }

    pub fn set_validator_fingerprint(
        state: &mut SwtchvmState,
        validator_id: B256,
        fp: RotorFingerprint,
    ) {
        let commitment = FingerprintCommitment::from_fingerprint(&fp);
        let serialized = commitment.to_bytes();
        state.contract_kv.insert(
            (spacetime_fingerprint_account(), kv_key(validator_id)),
            serialized.to_vec(),
        );
        let digest = commitment.digest(|b| *keccak256(b));
        state.set_storage(
            &spacetime_fingerprint_account(),
            verkle_storage_key(validator_id),
            digest.0,
        );
    }

    pub fn observe_validator_rotor(
        state: &mut SwtchvmState,
        validator_id: B256,
        rotor: Rotor,
        decay: f64,
    ) -> f64 {
        let mut fp = get_validator_fingerprint(state, validator_id)
            .unwrap_or_else(|| RotorFingerprint::new(decay));
        let score = fp.update(rotor);
        set_validator_fingerprint(state, validator_id, fp);
        score
    }

    /// Batch observe rotors (mirrors `FingerprintVerkle::apply_batch` on unified state).
    pub fn observe_validator_rotors_batch(
        state: &mut SwtchvmState,
        updates: &[(B256, Rotor)],
        default_decay: f64,
    ) -> Vec<B256> {
        let mut touched = Vec::with_capacity(updates.len());
        for (validator_id, rotor) in updates {
            observe_validator_rotor(state, *validator_id, *rotor, default_decay);
            touched.push(*validator_id);
        }
        touched
    }
}

#[cfg(feature = "spacetime-consensus")]
pub use inner::*;
