//! Cross-validator fingerprint attestation and mismatch slashing.
//!
//! Why this exists: the fingerprint EWMA update is fully deterministic given
//! (a) the prior fingerprint commitment from the previous block and (b) the
//! observed rotors in this block's vote leaves. Two honest validators with
//! the same inputs MUST produce byte-identical commitments. Disagreement is
//! evidence of a buggy or malicious node, no false positives possible.
//!
//! This is intentionally a separate post-finalization gossip path rather
//! than baking the fingerprint root into the envelope, which would force the
//! "combined state_root" change we deferred. The trade-off: detection lags
//! finalization by ~1 round, which is fine because fingerprint-divergence
//! evidence informs slashing decisions, not safety.
//!
//! ## Flow
//!
//! 1. Block N finalizes. Each validator independently runs
//!    `FingerprintVerkle::apply_batch` over the same vote leaves.
//! 2. Each validator computes `fingerprint_verkle_root` for block N.
//! 3. Each validator broadcasts a signed `FingerprintAttestation`.
//! 4. After collection (timeout or 2/3 attestations received), any node
//!    can detect mismatches via `FingerprintAttestationCollector::detect_mismatches`.
//! 5. Mismatches become `FingerprintAttestationMismatchEvidence`, fed into
//!    the slashing pipeline.
//!
//! ## What counts as a mismatch
//!
//! Two attestations from different validators for the same block height
//! that disagree on `fingerprint_verkle_root`. At least one is wrong,
//! either has a bug (rare, but the only false-positive class) or is
//! intentionally producing diverging state (the attack we care about).
//! Both are slashable; the network's slashing schedule may distinguish
//! "isolated minority" (likely bug) from "coordinated minority" (likely
//! attack) for severity tuning.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use alloy_primitives::B256;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AttestationError {
    DuplicateAttestation,
    InvalidSignatureFormat,
    HeightOutOfRange,
}

/// A validator's signed claim that, for block at `height`, the fingerprint
/// Verkle root after applying this block's updates is `fingerprint_root`.
///
/// `signature_digest` is the digest of the validator's Dilithium signature
/// over the attestation payload. Actual signature verification is done by
/// the consensus crate; this struct carries the binding.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FingerprintAttestation {
    pub height: u64,
    pub block_hash: B256,
    pub attester_did_hash: B256,
    pub fingerprint_root: B256,
    /// Hash of the previous block's fingerprint_root, for chain continuity.
    pub prev_fingerprint_root: B256,
    pub signature_digest: B256,
}

impl FingerprintAttestation {
    /// Domain-tagged signing payload. The Dilithium signature elsewhere
    /// covers exactly these bytes.
    pub fn signing_bytes(&self) -> [u8; 32 + 8 + 32 * 4] {
        const DOMAIN: &[u8] = b"spacekit-fingerprint-attestation-v1";
        let mut out = [0u8; 32 + 8 + 32 * 4];
        let mut o = 0;
        // We use a fixed 32-byte domain field (padded) for layout stability.
        let mut dom = [0u8; 32];
        let take = DOMAIN.len().min(32);
        dom[..take].copy_from_slice(&DOMAIN[..take]);
        out[o..o + 32].copy_from_slice(&dom);
        o += 32;
        out[o..o + 8].copy_from_slice(&self.height.to_be_bytes());
        o += 8;
        out[o..o + 32].copy_from_slice(self.block_hash.as_slice());
        o += 32;
        out[o..o + 32].copy_from_slice(self.attester_did_hash.as_slice());
        o += 32;
        out[o..o + 32].copy_from_slice(self.fingerprint_root.as_slice());
        o += 32;
        out[o..o + 32].copy_from_slice(self.prev_fingerprint_root.as_slice());
        out
    }
}

/// Evidence of two validators disagreeing on the fingerprint root for the
/// same block. Both attestations are included so the slasher can verify the
/// disagreement is genuine.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FingerprintAttestationMismatchEvidence {
    pub height: u64,
    pub block_hash: B256,
    pub attestation_a: FingerprintAttestation,
    pub attestation_b: FingerprintAttestation,
}

impl FingerprintAttestationMismatchEvidence {
    /// Confirm the evidence demonstrates a genuine mismatch.
    pub fn verify_mismatch(&self) -> bool {
        // Must be different attesters.
        if self.attestation_a.attester_did_hash == self.attestation_b.attester_did_hash {
            return false;
        }
        // Same block.
        if self.attestation_a.height != self.height
            || self.attestation_b.height != self.height
            || self.attestation_a.block_hash != self.block_hash
            || self.attestation_b.block_hash != self.block_hash
        {
            return false;
        }
        // Different fingerprint roots.
        self.attestation_a.fingerprint_root != self.attestation_b.fingerprint_root
    }

    /// Both attesters are candidates for slashing — at least one is wrong.
    /// Returns (did_a, did_b). The slashing layer applies its severity
    /// schedule; minority-disagreers are usually slashed harder than the
    /// majority, but that policy lives in `spacekit-consensus`.
    pub fn slash_candidates(&self) -> (B256, B256) {
        (
            self.attestation_a.attester_did_hash,
            self.attestation_b.attester_did_hash,
        )
    }
}

/// Collects per-height attestations and exposes mismatch detection.
/// Bounded by `height_window`: attestations older than that are dropped to
/// keep memory finite.
#[derive(Debug, Clone)]
pub struct FingerprintAttestationCollector {
    pub by_height: BTreeMap<u64, BTreeMap<B256, FingerprintAttestation>>,
    pub height_window: u64,
    pub lowest_kept_height: u64,
}

impl FingerprintAttestationCollector {
    pub fn new(height_window: u64) -> Self {
        Self {
            by_height: BTreeMap::new(),
            height_window,
            lowest_kept_height: 0,
        }
    }

    /// Ingest an attestation. Rejects duplicates from the same attester
    /// at the same height — an attester signing two different attestations
    /// for one height is itself slashable (use `find_self_contradictions`).
    pub fn ingest(&mut self, att: FingerprintAttestation) -> Result<(), AttestationError> {
        if att.height < self.lowest_kept_height {
            return Err(AttestationError::HeightOutOfRange);
        }
        let bucket = self.by_height.entry(att.height).or_default();
        if let Some(existing) = bucket.get(&att.attester_did_hash) {
            // Same attestation: idempotent OK. Different: a self-contradiction
            // — the caller should pick this up via `find_self_contradictions`.
            if *existing == att {
                return Ok(());
            }
            return Err(AttestationError::DuplicateAttestation);
        }
        bucket.insert(att.attester_did_hash, att);
        Ok(())
    }

    /// Sweep attestations below `current_height - height_window`. Called
    /// periodically by the coordinator.
    pub fn sweep(&mut self, current_height: u64) {
        let cutoff = current_height.saturating_sub(self.height_window);
        self.by_height.retain(|h, _| *h >= cutoff);
        self.lowest_kept_height = cutoff;
    }

    /// All pairs of attestations at `height` that disagree on the
    /// fingerprint root. Returns evidence ready for the slashing pipeline.
    /// O(N²) in the number of attestations at the height; typically N is
    /// small (one per active validator), so this is cheap.
    pub fn detect_mismatches(&self, height: u64) -> Vec<FingerprintAttestationMismatchEvidence> {
        let bucket = match self.by_height.get(&height) {
            Some(b) => b,
            None => return Vec::new(),
        };
        let atts: Vec<&FingerprintAttestation> = bucket.values().collect();
        let mut evidence = Vec::new();
        for i in 0..atts.len() {
            for j in (i + 1)..atts.len() {
                if atts[i].fingerprint_root != atts[j].fingerprint_root {
                    let block_hash = atts[i].block_hash;
                    evidence.push(FingerprintAttestationMismatchEvidence {
                        height,
                        block_hash,
                        attestation_a: *atts[i],
                        attestation_b: *atts[j],
                    });
                }
            }
        }
        evidence
    }

    /// Find attesters who submitted contradicting attestations at the same
    /// height. This is a separate (stronger) class of evidence from
    /// peer-mismatch — a single validator cannot rationalize signing two
    /// different fingerprint roots for the same block.
    ///
    /// Returns (validator_did_hash, attestation_first_seen, conflict_attestation).
    /// Caller must store BOTH attestations long enough to detect this; the
    /// collector itself rejects the second `ingest` call with
    /// `DuplicateAttestation`, so this method takes a candidate attestation
    /// as input rather than scanning state.
    pub fn check_self_contradiction(
        &self,
        candidate: &FingerprintAttestation,
    ) -> Option<FingerprintAttestation> {
        let bucket = self.by_height.get(&candidate.height)?;
        let existing = bucket.get(&candidate.attester_did_hash)?;
        if existing == candidate {
            return None;
        }
        Some(*existing)
    }

    /// Quorum check: do we have attestations from at least `min_count`
    /// validators at this height, all agreeing? Returns the agreed root
    /// if so. Returns None if no quorum OR if any disagreement exists.
    pub fn agreed_root(&self, height: u64, min_count: usize) -> Option<B256> {
        let bucket = self.by_height.get(&height)?;
        if bucket.len() < min_count {
            return None;
        }
        let mut iter = bucket.values();
        let first = iter.next()?;
        for att in iter {
            if att.fingerprint_root != first.fingerprint_root {
                return None;
            }
        }
        Some(first.fingerprint_root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn att(height: u64, block: u8, attester: u8, root: u8, prev: u8) -> FingerprintAttestation {
        FingerprintAttestation {
            height,
            block_hash: B256::from([block; 32]),
            attester_did_hash: B256::from([attester; 32]),
            fingerprint_root: B256::from([root; 32]),
            prev_fingerprint_root: B256::from([prev; 32]),
            signature_digest: B256::from([0xAA; 32]),
        }
    }

    #[test]
    fn matching_attestations_no_evidence() {
        let mut c = FingerprintAttestationCollector::new(100);
        c.ingest(att(10, 1, 0x01, 0xFF, 0xEE)).unwrap();
        c.ingest(att(10, 1, 0x02, 0xFF, 0xEE)).unwrap();
        c.ingest(att(10, 1, 0x03, 0xFF, 0xEE)).unwrap();
        assert!(c.detect_mismatches(10).is_empty());
        assert_eq!(c.agreed_root(10, 3), Some(B256::from([0xFF; 32])));
    }

    #[test]
    fn diverging_attestation_produces_evidence() {
        let mut c = FingerprintAttestationCollector::new(100);
        c.ingest(att(10, 1, 0x01, 0xFF, 0xEE)).unwrap();
        c.ingest(att(10, 1, 0x02, 0xFF, 0xEE)).unwrap();
        c.ingest(att(10, 1, 0x03, 0x00, 0xEE)).unwrap(); // dissenter
        let evidence = c.detect_mismatches(10);
        // Honest pair (1,2) agree; dissenter (3) disagrees with both → 2 evidence items.
        assert_eq!(evidence.len(), 2);
        for e in &evidence {
            assert!(e.verify_mismatch());
        }
        // Quorum: any single disagreement breaks consensus on the root.
        assert!(c.agreed_root(10, 3).is_none());
    }

    #[test]
    fn self_contradiction_detected() {
        let mut c = FingerprintAttestationCollector::new(100);
        let a1 = att(10, 1, 0x05, 0xAA, 0xEE);
        let a2_conflicting = att(10, 1, 0x05, 0xBB, 0xEE);
        c.ingest(a1).unwrap();
        assert_eq!(c.check_self_contradiction(&a2_conflicting), Some(a1));
        // Second ingest is rejected as duplicate.
        assert_eq!(
            c.ingest(a2_conflicting),
            Err(AttestationError::DuplicateAttestation)
        );
    }

    #[test]
    fn idempotent_ingest() {
        let mut c = FingerprintAttestationCollector::new(100);
        let a = att(10, 1, 0x05, 0xAA, 0xEE);
        c.ingest(a).unwrap();
        // Same attestation re-ingested is OK.
        c.ingest(a).unwrap();
    }

    #[test]
    fn sweep_drops_old_heights() {
        let mut c = FingerprintAttestationCollector::new(10);
        c.ingest(att(5, 1, 0x01, 0xFF, 0xEE)).unwrap();
        c.ingest(att(15, 2, 0x01, 0xFF, 0xEE)).unwrap();
        c.sweep(20);
        // height 5 is below 20-10 = 10, should be dropped.
        assert!(c.by_height.get(&5).is_none());
        assert!(c.by_height.get(&15).is_some());
    }

    #[test]
    fn signing_bytes_stable() {
        let a = att(0x12345678, 0xAB, 0xCD, 0xEF, 0x00);
        let bytes_a = a.signing_bytes();
        // Re-compute, must be identical (deterministic serialization).
        let bytes_a2 = a.signing_bytes();
        assert_eq!(&bytes_a[..], &bytes_a2[..]);
        // A different attester produces different bytes.
        let mut b = a;
        b.attester_did_hash = B256::from([0xDE; 32]);
        assert_ne!(&bytes_a[..], &b.signing_bytes()[..]);
    }
}
