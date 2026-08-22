//! Fingerprint-at-rest storage in the state Verkle.
//!
//! Behavioral fingerprints (see `defense::RotorFingerprint`) are long-lived
//! evidence: they accumulate across thousands of rounds, they're inputs to
//! slashing decisions, and stateless clients need to verify "validator X's
//! fingerprint at block N was Y" without holding history. This is exactly
//! what SIS-VC binding is for.
//!
//! ## Key layout
//!
//! In the state Verkle, fingerprints live under a reserved address namespace
//! distinct from user account state:
//!
//!   address = `FINGERPRINT_NAMESPACE` (20 bytes, fixed)
//!   key     = validator DID hash (32 bytes)
//!   value   = U256 derived from `FingerprintCommitment::digest()`
//!
//! The 20-byte address makes it trivially separable from EOA / contract
//! addresses; node implementations should reject any user transaction that
//! tries to write to this address.
//!
//! ## Update flow
//!
//! Each finalized block triggers fingerprint updates for every validator
//! that submitted a vote. The flow:
//!
//!   1. Coordinator collects (validator_did, rotor) pairs from the vote leaves.
//!   2. For each validator, load the existing FingerprintCommitment from the
//!      Verkle tree (or initialize if first observation).
//!   3. Apply `RotorFingerprint::update(rotor)` deterministically.
//!   4. Serialize the new fingerprint, hash to a digest, commit at
//!      (FINGERPRINT_NAMESPACE, did_hash).
//!   5. New state root reflects all updates; included in the block envelope.
//!
//! Because the update is deterministic (EWMA with fixed decay constant,
//! manifold-tangent step), every validator computes the same fingerprint
//! independently. Disagreement here is itself slashable evidence.
//!
//! ## Storage size
//!
//! Per-validator fingerprint serialization:
//!
//!   - centroid rotor (8 even-grade f64s): 64 bytes
//!   - dispersion (f64):                    8 bytes
//!   - decay (f64):                         8 bytes
//!   - samples (u32):                       4 bytes
//!   - consecutive_anomalies (u32):         4 bytes
//!   - reserved/version:                    4 bytes
//!   ──────────────────────────────────────────────
//!   total:                                92 bytes
//!
//! Hashed to 32 bytes for the Verkle value; full 92-byte payload stored
//! off-tree and proven via the digest.

use crate::algebra::Multivector;
use crate::defense::RotorFingerprint;
use crate::rotor::Rotor;
use alloy_primitives::{Address, B256, U256};

/// Reserved address namespace for fingerprint storage. Chosen to be
/// unmistakable (`0xFF...FE`) and outside any reasonable user range.
/// The last byte is `0xFE` to leave `0xFF...FF` available for other
/// system-level namespaces (clique evidence, slashing proofs, etc.).
pub const FINGERPRINT_NAMESPACE: Address = Address::new([
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    0xFF, 0xFF, 0xFF, 0xFE,
]);

/// Domain tag for fingerprint commitment digests.
const DOMAIN_FINGERPRINT: &[u8] = b"spacekit-fingerprint-v1";

/// Wire format version for the fingerprint payload.
pub const FINGERPRINT_WIRE_VERSION: u32 = 1;

/// Serialized form of a `RotorFingerprint` plus version stamp.
///
/// 92 bytes total. Stable across builds — never reorder fields without
/// bumping `FINGERPRINT_WIRE_VERSION` and writing a migration path.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FingerprintCommitment {
    pub version: u32,
    pub samples: u32,
    pub consecutive_anomalies: u32,
    pub dispersion: f64,
    pub decay: f64,
    /// The 8 even-grade coefficients of the centroid rotor in canonical order
    /// (scalar, γ₀γ₁, γ₀γ₂, γ₀γ₃, γ₁γ₂, γ₁γ₃, γ₂γ₃, pseudoscalar).
    pub centroid_coeffs: [f64; 8],
}

impl FingerprintCommitment {
    pub const SERIALIZED_SIZE: usize = 92;

    pub fn from_fingerprint(fp: &RotorFingerprint) -> Self {
        let mv = fp.centroid.as_multivector();
        let centroid_coeffs = [
            mv.coeffs[0],  // scalar
            mv.coeffs[5],  // γ₀γ₁
            mv.coeffs[6],  // γ₀γ₂
            mv.coeffs[7],  // γ₀γ₃
            mv.coeffs[8],  // γ₁γ₂
            mv.coeffs[9],  // γ₁γ₃
            mv.coeffs[10], // γ₂γ₃
            mv.coeffs[15], // pseudoscalar
        ];
        Self {
            version: FINGERPRINT_WIRE_VERSION,
            samples: fp.samples,
            consecutive_anomalies: fp.consecutive_anomalies,
            dispersion: fp.dispersion,
            decay: fp.decay,
            centroid_coeffs,
        }
    }

    /// Reconstruct a usable `RotorFingerprint` from the committed bytes.
    /// Returns `None` if the centroid fails the rotor invariants — which
    /// indicates either a wire-format bug or a tampered commitment.
    pub fn to_fingerprint(&self) -> Option<RotorFingerprint> {
        if self.version != FINGERPRINT_WIRE_VERSION {
            return None;
        }
        let mut mv = Multivector::ZERO;
        mv.coeffs[0] = self.centroid_coeffs[0];
        mv.coeffs[5] = self.centroid_coeffs[1];
        mv.coeffs[6] = self.centroid_coeffs[2];
        mv.coeffs[7] = self.centroid_coeffs[3];
        mv.coeffs[8] = self.centroid_coeffs[4];
        mv.coeffs[9] = self.centroid_coeffs[5];
        mv.coeffs[10] = self.centroid_coeffs[6];
        mv.coeffs[15] = self.centroid_coeffs[7];
        let centroid = Rotor::renormalize(mv).ok()?;
        Some(RotorFingerprint {
            centroid,
            dispersion: self.dispersion,
            decay: self.decay,
            samples: self.samples,
            consecutive_anomalies: self.consecutive_anomalies,
        })
    }

    pub fn to_bytes(&self) -> [u8; Self::SERIALIZED_SIZE] {
        let mut out = [0u8; Self::SERIALIZED_SIZE];
        let mut o = 0;
        out[o..o + 4].copy_from_slice(&self.version.to_be_bytes());
        o += 4;
        out[o..o + 4].copy_from_slice(&self.samples.to_be_bytes());
        o += 4;
        out[o..o + 4].copy_from_slice(&self.consecutive_anomalies.to_be_bytes());
        o += 4;
        out[o..o + 8].copy_from_slice(&self.dispersion.to_le_bytes());
        o += 8;
        out[o..o + 8].copy_from_slice(&self.decay.to_le_bytes());
        o += 8;
        for c in &self.centroid_coeffs {
            out[o..o + 8].copy_from_slice(&c.to_le_bytes());
            o += 8;
        }
        debug_assert_eq!(o, Self::SERIALIZED_SIZE);
        out
    }

    pub fn from_bytes(b: &[u8]) -> Option<Self> {
        if b.len() < Self::SERIALIZED_SIZE {
            return None;
        }
        let mut o = 0;
        let version = u32::from_be_bytes(b[o..o + 4].try_into().ok()?);
        o += 4;
        let samples = u32::from_be_bytes(b[o..o + 4].try_into().ok()?);
        o += 4;
        let consecutive_anomalies = u32::from_be_bytes(b[o..o + 4].try_into().ok()?);
        o += 4;
        let dispersion = f64::from_le_bytes(b[o..o + 8].try_into().ok()?);
        o += 8;
        let decay = f64::from_le_bytes(b[o..o + 8].try_into().ok()?);
        o += 8;
        let mut centroid_coeffs = [0.0f64; 8];
        for i in 0..8 {
            centroid_coeffs[i] = f64::from_le_bytes(b[o..o + 8].try_into().ok()?);
            o += 8;
        }
        debug_assert_eq!(o, Self::SERIALIZED_SIZE);
        Some(Self {
            version,
            samples,
            consecutive_anomalies,
            dispersion,
            decay,
            centroid_coeffs,
        })
    }

    /// Domain-tagged digest of the serialized fingerprint. This is the
    /// value committed into the Verkle tree.
    pub fn digest<F: Fn(&[u8]) -> [u8; 32]>(&self, hash_fn: F) -> B256 {
        let bytes = self.to_bytes();
        let mut buf = Vec::with_capacity(DOMAIN_FINGERPRINT.len() + 1 + bytes.len());
        buf.extend_from_slice(DOMAIN_FINGERPRINT);
        buf.push(0x1f);
        buf.extend_from_slice(&bytes);
        B256::from(hash_fn(&buf))
    }
}

/// State-Verkle backed fingerprint registry.
///
/// In the standalone, this wraps the same `QuantumTree<NistSisScheme>` that
/// holds account state, just operating under `FINGERPRINT_NAMESPACE`. The
/// wrapper enforces:
///   1. Only the consensus layer can write to this namespace (callers must
///      hold the consensus capability — represented here as a typestate
///      witness).
///   2. Fingerprint updates are batched and committed atomically per block.
///   3. The off-tree fingerprint payload bytes are stored alongside the
///      digest for proof reconstruction.
#[cfg(feature = "verkle")]
pub mod store {
    use super::*;
    use crate::defense::FingerprintRegistry;
    use alloc::collections::BTreeMap;
    use spacekit_quantum_verkle::commitment::{
        NistSisScheme, QuantumProof, QuantumTree, SisOpening,
    };

    /// Witness type that proves the caller has consensus-layer authority to
    /// update fingerprints. Only constructible inside this crate.
    #[derive(Debug, Clone, Copy)]
    pub struct ConsensusWriteCap(());

    impl ConsensusWriteCap {
        pub(crate) fn new() -> Self {
            Self(())
        }
    }

    /// Point of entry for fingerprint EWMA updates after block finalization.
    /// External crates must use this instead of [`ConsensusWriteCap`].
    /// Each update is `(validator_did, rotor, residual_norm)` from the block's
    /// v2 [`crate::proposal::SpacetimeTransition`]. Residual norm is folded into
    /// a virtual rotor via [`FingerprintRegistry::project_joint`].
    pub fn apply_fingerprint_batch<F: Fn(&[u8]) -> [u8; 32] + Copy>(
        store: &mut FingerprintVerkle,
        updates: &[(B256, Rotor, f64)],
        default_decay: f64,
        hash_fn: F,
    ) -> Vec<B256> {
        store.apply_batch(updates, ConsensusWriteCap::new(), default_decay, hash_fn)
    }

    /// Rollback payload for reorg handling (challenge-window snapshots).
    #[derive(Debug, Clone)]
    pub struct FingerprintStoreSnapshot {
        pub payloads: BTreeMap<B256, FingerprintCommitment>,
    }

    pub struct FingerprintVerkle {
        pub tree: QuantumTree<NistSisScheme>,
        /// Off-tree storage of the full payload bytes by validator DID hash.
        /// The Verkle tree commits to the digest; this map holds the preimage
        /// that lets nodes reconstruct proofs.
        pub payloads: BTreeMap<B256, FingerprintCommitment>,
    }

    impl FingerprintVerkle {
        pub fn new() -> Self {
            Self {
                tree: QuantumTree::<NistSisScheme>::new(),
                payloads: BTreeMap::new(),
            }
        }

        /// Read a validator's fingerprint commitment. Returns `None` if the
        /// validator has no prior observations.
        pub fn get(&self, did_hash: &B256) -> Option<&FingerprintCommitment> {
            self.payloads.get(did_hash)
        }

        /// Domain-tagged Merkle root over all fingerprint commitments in this store.
        pub fn root_hash(&self) -> B256 {
            self.tree.root()
        }

        pub fn snapshot(&self) -> FingerprintStoreSnapshot {
            FingerprintStoreSnapshot {
                payloads: self.payloads.clone(),
            }
        }

        /// Restore from a height snapshot after reorg (rebuilds the Verkle tree).
        pub fn restore<F: Fn(&[u8]) -> [u8; 32] + Copy>(
            &mut self,
            snap: FingerprintStoreSnapshot,
            hash_fn: F,
        ) {
            self.payloads = snap.payloads;
            self.tree = QuantumTree::<NistSisScheme>::new();
            for (did_hash, commitment) in &self.payloads {
                let digest = commitment.digest(hash_fn);
                let value = U256::from_be_bytes::<32>(digest.0);
                self.tree.set(&FINGERPRINT_NAMESPACE, did_hash, value);
            }
        }

        /// Apply a batch of fingerprint updates. Must be called inside the
        /// consensus crate's block-finalization path; the `_cap` parameter
        /// is the gating witness.
        ///
        /// Returns the set of validator DID hashes whose fingerprints were
        /// updated, in the order processed. Useful for emitting events.
        pub fn apply_batch<F: Fn(&[u8]) -> [u8; 32] + Copy>(
            &mut self,
            updates: &[(B256, Rotor, f64)],
            _cap: ConsensusWriteCap,
            default_decay: f64,
            hash_fn: F,
        ) -> Vec<B256> {
            let mut touched = Vec::with_capacity(updates.len());
            for (did_hash, observed_rotor, residual_norm) in updates {
                // Load existing or initialize.
                let mut fp = match self.payloads.get(did_hash) {
                    Some(commit) => match commit.to_fingerprint() {
                        Some(f) => f,
                        None => {
                            // Corrupted state — start over. In production this
                            // would be a loud warning and a slashing trigger.
                            RotorFingerprint::new(default_decay)
                        }
                    },
                    None => RotorFingerprint::new(default_decay),
                };
                let virtual_rotor =
                    FingerprintRegistry::project_joint(*observed_rotor, *residual_norm);
                fp.update(virtual_rotor);

                // Commit.
                let commitment = FingerprintCommitment::from_fingerprint(&fp);
                let digest = commitment.digest(hash_fn);
                let value = U256::from_be_bytes::<32>(digest.0);
                self.tree.set(&FINGERPRINT_NAMESPACE, did_hash, value);
                self.payloads.insert(*did_hash, commitment);
                touched.push(*did_hash);
            }
            touched
        }

        /// Produce a Verkle proof that a validator's fingerprint was X at
        /// the current state root. Light clients verify this against the
        /// state root committed in the block envelope.
        pub fn prove_fingerprint(&self, did_hash: &B256) -> Option<QuantumProof<SisOpening>> {
            self.tree
                .create_proof(&FINGERPRINT_NAMESPACE, did_hash)
                .ok()
        }

        /// Verify a Verkle proof that a validator's fingerprint digest is the
        /// claimed value.
        pub fn verify_fingerprint_proof<F: Fn(&[u8]) -> [u8; 32]>(
            &self,
            did_hash: &B256,
            commitment: &FingerprintCommitment,
            proof: &QuantumProof<SisOpening>,
            hash_fn: F,
        ) -> bool {
            let digest = commitment.digest(hash_fn);
            let value = U256::from_be_bytes::<32>(digest.0);
            self.tree
                .verify_proof(proof, &FINGERPRINT_NAMESPACE, did_hash, value)
        }

        /// Sweep stale fingerprints — validators who have not participated
        /// in `max_idle_samples` consecutive observations. Returns the DIDs
        /// removed. Called periodically by the consensus crate to bound tree
        /// growth.
        pub fn sweep_idle(&mut self, _max_idle_samples: u32, _cap: ConsensusWriteCap) -> Vec<B256> {
            // Idle-detection requires a "last seen block" counter we haven't
            // added to FingerprintCommitment yet. Stub for now: returns empty.
            // When wired, this will scan payloads, identify entries whose
            // samples count hasn't advanced in N rounds, and delete from
            // both `tree` and `payloads`.
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rotor::Bivector;

    fn h(b: &[u8]) -> [u8; 32] {
        let mut out = [0u8; 32];
        for (i, byte) in b.iter().enumerate() {
            out[i % 32] = out[i % 32].wrapping_add(*byte).wrapping_mul(31);
        }
        out
    }

    #[test]
    fn commitment_round_trip() {
        let mut fp = RotorFingerprint::new(0.95);
        for i in 0..50 {
            fp.update(Rotor::exp(&Bivector {
                b: [0.0, 0.0, 0.0, 0.01 + 0.001 * (i as f64), 0.0, 0.0],
            }));
        }
        let commitment = FingerprintCommitment::from_fingerprint(&fp);
        let bytes = commitment.to_bytes();
        let parsed = FingerprintCommitment::from_bytes(&bytes).unwrap();
        assert_eq!(parsed, commitment);

        // Roundtrip through fingerprint reconstruction.
        let reconstructed = parsed.to_fingerprint().unwrap();
        assert_eq!(reconstructed.samples, fp.samples);
        assert_eq!(
            reconstructed.consecutive_anomalies,
            fp.consecutive_anomalies
        );
        assert!((reconstructed.dispersion - fp.dispersion).abs() < 1e-12);
        assert!(reconstructed.centroid.distance(&fp.centroid) < 1e-6);
    }

    #[test]
    fn digest_is_domain_separated() {
        // Two payloads that are byte-identical but with the wrong domain
        // tag must produce different digests. We check by computing both.
        let mut fp = RotorFingerprint::new(0.9);
        fp.update(Rotor::IDENTITY);
        let commitment = FingerprintCommitment::from_fingerprint(&fp);
        let domain_tagged = commitment.digest(h);

        // Bare hash without domain — must differ.
        let bare = B256::from(h(&commitment.to_bytes()));
        assert_ne!(domain_tagged, bare);
    }

    #[test]
    fn fingerprint_namespace_is_distinctive() {
        // Sanity: the namespace doesn't accidentally collide with common
        // address patterns (zero, all-ones, etc).
        assert_ne!(FINGERPRINT_NAMESPACE, Address::ZERO);
        assert_ne!(
            FINGERPRINT_NAMESPACE.as_slice()[19],
            0xFF,
            "leave 0xFF...FF available for other system namespaces"
        );
    }

    #[cfg(feature = "verkle")]
    #[test]
    fn verkle_apply_and_prove() {
        use super::store::{apply_fingerprint_batch, FingerprintVerkle};

        let mut store = FingerprintVerkle::new();
        let did_a = B256::from([0xA1; 32]);
        let did_b = B256::from([0xB2; 32]);

        // Train each validator with a few observations.
        let updates_round_1: Vec<(B256, Rotor, f64)> = vec![
            (
                did_a,
                Rotor::exp(&Bivector {
                    b: [0.0, 0.0, 0.0, 0.01, 0.0, 0.0],
                }),
                0.0,
            ),
            (
                did_b,
                Rotor::exp(&Bivector {
                    b: [0.0, 0.0, 0.0, 0.02, 0.0, 0.0],
                }),
                0.0,
            ),
        ];
        let touched = apply_fingerprint_batch(&mut store, &updates_round_1, 0.95, h);
        assert_eq!(touched.len(), 2);

        // Subsequent rounds.
        for round in 0..30 {
            let updates: Vec<(B256, Rotor, f64)> = vec![
                (
                    did_a,
                    Rotor::exp(&Bivector {
                        b: [0.0, 0.0, 0.0, 0.01 + 0.001 * round as f64, 0.0, 0.0],
                    }),
                    0.01,
                ),
                (
                    did_b,
                    Rotor::exp(&Bivector {
                        b: [0.0, 0.0, 0.0, 0.02 + 0.001 * round as f64, 0.0, 0.0],
                    }),
                    0.01,
                ),
            ];
            apply_fingerprint_batch(&mut store, &updates, 0.95, h);
        }

        // Read back validator A's fingerprint.
        let commit_a = store.get(&did_a).unwrap();
        assert!(commit_a.samples >= 30);

        // Produce + verify a Verkle proof for A.
        let proof = store.prove_fingerprint(&did_a).expect("proof");
        assert!(
            store.verify_fingerprint_proof(&did_a, commit_a, &proof, h),
            "valid fingerprint proof must verify"
        );

        // Tamper with the commitment: verification must fail.
        let mut tampered = *commit_a;
        tampered.dispersion = 99.99;
        assert!(
            !store.verify_fingerprint_proof(&did_a, &tampered, &proof, h),
            "tampered commitment must not verify"
        );
    }

    #[cfg(feature = "verkle")]
    #[test]
    fn snapshot_restore_reverts_ewma_state() {
        use super::store::{apply_fingerprint_batch, FingerprintVerkle};

        let did = B256::from([0xC3; 32]);
        let mut store = FingerprintVerkle::new();
        let r1 = Rotor::exp(&Bivector {
            b: [0.0, 0.0, 0.0, 0.01, 0.0, 0.0],
        });
        apply_fingerprint_batch(&mut store, &[(did, r1, 0.0)], 0.95, h);
        let snap = store.snapshot();
        let after_one = *store.get(&did).unwrap();

        let r2 = Rotor::exp(&Bivector {
            b: [0.0, 0.0, 0.0, 0.5, 0.0, 0.0],
        });
        apply_fingerprint_batch(&mut store, &[(did, r2, 0.0)], 0.95, h);
        assert_ne!(store.get(&did).unwrap().samples, after_one.samples);

        store.restore(snap, h);
        assert_eq!(store.get(&did).unwrap(), &after_one);
    }
}
