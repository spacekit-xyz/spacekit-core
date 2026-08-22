//! Sleeper-attack defenses for the spacetime consensus layer.
//!
//! Three components, each strictly stronger than the analogous defense
//! available without the rotor structure:
//!
//! 1. **Geometric median on Spin⁺(1,3)** (`geometric_median_rotor`). Replaces
//!    the Fréchet mean with the breakdown-point-1/2 estimator. With this,
//!    moving the consensus rotor requires *strictly more than 50%* of
//!    reputation-weighted validators to be malicious, not 1/3+.
//!
//! 2. **Behavioral fingerprint** (`RotorFingerprint`). Per-validator rolling
//!    statistics in rotor space. Detects "wake-up" events — sudden departures
//!    from a validator's historical rotor neighborhood — independent of
//!    whether the proof itself looks valid.
//!
//! 3. **Clique detection** (`detect_coordination_clique`). Identifies sets
//!    of validators whose rotors agree more tightly than their causal
//!    separation can explain. Output feeds into your existing slashing /
//!    reputation update path.
//!
//! These do not replace cryptoeconomic measures (bonded reputation,
//! decay, slashing). They sit alongside, raising the bar from
//! "1/3 weight defeats safety" to "1/2 weight defeats safety AND
//! coordinated wake-ups are detectable on attack day."

use crate::causal::CausalCoord;
use crate::fingerprint_verkle::FingerprintCommitment;
use crate::rotor::{Bivector, Rotor};
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use alloy_primitives::B256;

#[cfg(not(feature = "std"))]
use libm::sqrt;
#[cfg(feature = "std")]
fn sqrt(x: f64) -> f64 {
    x.sqrt()
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DefenseError {
    EmptyInput,
    NonPositiveTotalWeight,
    DegenerateInputs,
}

// =====================================================================
// 1. Geometric median on Spin⁺(1,3) (Weiszfeld iteration on the manifold)
// =====================================================================

#[derive(Debug, Clone, Copy)]
pub struct GeometricMedianConfig {
    pub max_iters: usize,
    pub tolerance: f64,
    /// Small constant to avoid division by zero when iterate equals an input.
    pub regularizer: f64,
}

impl Default for GeometricMedianConfig {
    fn default() -> Self {
        Self {
            max_iters: 32,
            tolerance: 1e-9,
            regularizer: 1e-8,
        }
    }
}

/// Weighted geometric median of rotors. Robust to up to 50% arbitrary
/// (Byzantine) inputs. Algorithm: Weiszfeld iteration adapted to the Spin
/// manifold — at each step the update is the *inverse-distance-weighted*
/// Fréchet mean of the inputs.
///
/// Convergence: superlinear near the optimum; in practice 5–10 iterations
/// for typical consensus loads.
///
/// **Use this instead of `aggregate_rotors` from `aggregation.rs` for any
/// commit where Byzantine resilience > 1/3 is required.** The mean version
/// remains useful for non-adversarial averaging (e.g., parameter tuning).
pub fn geometric_median_rotor(
    rotors_with_weights: &[(Rotor, f64)],
    config: &GeometricMedianConfig,
) -> Result<Rotor, DefenseError> {
    if rotors_with_weights.is_empty() {
        return Err(DefenseError::EmptyInput);
    }
    let total: f64 = rotors_with_weights.iter().map(|(_, w)| w).sum();
    if total <= 0.0 {
        return Err(DefenseError::NonPositiveTotalWeight);
    }

    // Initialize at the input with highest weight (median is well-defined
    // and the Weiszfeld iterates converge to it regardless of init for
    // generic inputs).
    let mut best_idx = 0;
    let mut best_w = f64::NEG_INFINITY;
    for (i, (_, w)) in rotors_with_weights.iter().enumerate() {
        if *w > best_w {
            best_w = *w;
            best_idx = i;
        }
    }
    let mut median = rotors_with_weights[best_idx].0;

    for _ in 0..config.max_iters {
        // For each input rotor R_i with weight w_i, compute
        //   tangent_i = log(median⁻¹ · R_i)
        //   distance_i = |tangent_i|
        //   reweight_i = w_i / max(distance_i, ε)
        // Then the Weiszfeld step is the reweighted Fréchet mean:
        //   step = (Σ reweight_i · tangent_i) / (Σ reweight_i)
        //   median ← median · exp(step)
        let median_inv = median.reverse();
        let mut sum = Bivector::ZERO;
        let mut reweight_total = 0.0;
        for (r, w) in rotors_with_weights {
            let rel = median_inv.compose(r);
            let tangent = match rel.log() {
                Ok(b) => b,
                Err(_) => continue,
            };
            let dist = sqrt(tangent.square_scalar().abs()).max(config.regularizer);
            let reweight = w / dist;
            reweight_total += reweight;
            sum = sum.add(&tangent.scale(reweight));
        }
        if reweight_total <= 0.0 {
            return Err(DefenseError::DegenerateInputs);
        }
        let step = sum.scale(1.0 / reweight_total);
        let step_norm = sqrt(step.square_scalar().abs());
        median = median.compose(&Rotor::exp(&step));
        if step_norm < config.tolerance {
            return Ok(median);
        }
    }
    Ok(median)
}

// =====================================================================
// 2. Behavioral fingerprint — per-validator rotor-neighborhood tracking
// =====================================================================

/// Rolling fingerprint of a validator's typical rotor behavior. Maintained
/// by every validator full-node observing the gossip layer; consulted before
/// accepting a vote that would carry the validator's full reputation weight.
///
/// The fingerprint is two-part:
///   - `centroid`: exponentially-weighted Fréchet mean of past rotors.
///   - `dispersion`: EWMA of geodesic distances from rotors to centroid.
///
/// A "wake-up" event is: incoming rotor at distance > k·dispersion from
/// centroid, with k typically 4–6 (the "Mahalanobis sigma" in this manifold
/// setting). This works regardless of whether the rotor *itself* is locally
/// valid — the question is whether it is *characteristic*.
#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RotorFingerprint {
    pub centroid: Rotor,
    /// Exponentially-weighted mean of geodesic distance from rotors to centroid.
    pub dispersion: f64,
    /// EWMA decay factor in (0, 1). Larger → longer memory.
    pub decay: f64,
    /// Number of rotors observed (for warm-up logic).
    pub samples: u32,
    /// Number of consecutive recent rotors at >sigma_threshold distance.
    pub consecutive_anomalies: u32,
}

impl RotorFingerprint {
    /// When every historical observation is identical (zero EWMA variance), use
    /// this geodesic floor so a sudden departure still scores and can slash.
    const CONSTANT_HISTORY_SCALE: f64 = 0.05;

    pub fn new(decay: f64) -> Self {
        Self {
            centroid: Rotor::IDENTITY,
            dispersion: 0.0,
            decay,
            samples: 0,
            consecutive_anomalies: 0,
        }
    }

    fn effective_dispersion(&self) -> f64 {
        if self.dispersion > 1e-9 {
            self.dispersion
        } else {
            Self::CONSTANT_HISTORY_SCALE
        }
    }

    /// Update with a fresh observation. Returns the anomaly score (0 for typical,
    /// growing as the observation departs from the fingerprint).
    pub fn update(&mut self, observation: Rotor) -> f64 {
        if self.samples == 0 {
            self.centroid = observation;
            self.samples = 1;
            return 0.0;
        }

        // Geodesic distance from current centroid.
        let dist = self.centroid.distance(&observation);
        let anomaly_score = if self.samples < 2 {
            0.0
        } else {
            dist / self.effective_dispersion()
        };

        // EWMA update of dispersion.
        self.dispersion = self.decay * self.dispersion + (1.0 - self.decay) * dist;

        // Move centroid one step toward observation (manifold EWMA via tangent).
        if let Ok(tangent) = self.centroid.reverse().compose(&observation).log() {
            let step = tangent.scale(1.0 - self.decay);
            self.centroid = self.centroid.compose(&Rotor::exp(&step));
        }

        self.samples = self.samples.saturating_add(1);
        anomaly_score
    }

    /// Threshold check. Typical settings: σ = 4.0 for warning, σ = 6.0 for slash.
    pub fn is_anomalous(&self, observation: Rotor, sigma: f64) -> bool {
        if self.samples < 16 {
            return false;
        } // need warm-up
        let dist = self.centroid.distance(&observation);
        dist > sigma * self.effective_dispersion()
    }

    /// Canonical bytes for state Verkle (see [`FingerprintCommitment`]).
    pub fn to_bytes(&self) -> [u8; FingerprintCommitment::SERIALIZED_SIZE] {
        FingerprintCommitment::from_fingerprint(self).to_bytes()
    }

    pub fn from_bytes(buf: &[u8]) -> Option<Self> {
        FingerprintCommitment::from_bytes(buf)?.to_fingerprint()
    }

    /// Commitment digest for the state Verkle tree (`keccak256` in production).
    pub fn digest<F: Fn(&[u8]) -> [u8; 32]>(&self, hash_fn: F) -> B256 {
        FingerprintCommitment::from_fingerprint(self).digest(hash_fn)
    }
}

/// Registry mapping validator DID hash → fingerprint. Lives in
/// `SpacetimeExtension` (add it there as a `pub fingerprints: FingerprintRegistry`).
#[derive(Debug, Default, Clone)]
pub struct FingerprintRegistry {
    pub by_validator: BTreeMap<B256, RotorFingerprint>,
    pub default_decay: f64,
}

impl FingerprintRegistry {
    pub fn new(default_decay: f64) -> Self {
        Self {
            by_validator: BTreeMap::new(),
            default_decay,
        }
    }

    /// Observe a rotor. In v2, prefer [`Self::observe_joint`] for transitions.
    pub fn observe(&mut self, did_hash: B256, rotor: Rotor) -> f64 {
        let decay = self.default_decay;
        let fp = self
            .by_validator
            .entry(did_hash)
            .or_insert_with(|| RotorFingerprint::new(decay));
        fp.update(rotor)
    }

    /// Observe joint (rotor, residual_norm) from a v2 [`crate::proposal::SpacetimeTransition`].
    pub fn observe_joint(&mut self, did_hash: B256, rotor: Rotor, residual_norm: f64) -> f64 {
        let virtual_rotor = Self::project_joint(rotor, residual_norm);
        let decay = self.default_decay;
        let fp = self
            .by_validator
            .entry(did_hash)
            .or_insert_with(|| RotorFingerprint::new(decay));
        fp.update(virtual_rotor)
    }

    pub(crate) fn project_joint(rotor: Rotor, residual_norm: f64) -> Rotor {
        let saturated = if residual_norm > 10.0 {
            10.0
        } else {
            residual_norm
        };
        #[cfg(feature = "std")]
        let angle = saturated.atan();
        #[cfg(not(feature = "std"))]
        let angle = libm::atan(saturated);
        let residual_bivector = Bivector {
            b: [0.0, 0.0, 0.0, 0.0, 0.0, angle],
        };
        let residual_rotation = Rotor::exp(&residual_bivector);
        rotor.compose(&residual_rotation)
    }

    pub fn check(&self, did_hash: &B256, rotor: Rotor, sigma: f64) -> Option<bool> {
        self.by_validator
            .get(did_hash)
            .map(|fp| fp.is_anomalous(rotor, sigma))
    }

    pub fn check_joint(
        &self,
        did_hash: &B256,
        rotor: Rotor,
        residual_norm: f64,
        sigma: f64,
    ) -> Option<bool> {
        let virtual_rotor = Self::project_joint(rotor, residual_norm);
        self.by_validator
            .get(did_hash)
            .map(|fp| fp.is_anomalous(virtual_rotor, sigma))
    }
}

// =====================================================================
// 3. Clique detection — coordinated wake-up identification
// =====================================================================

/// A validator's submission in a single round: who they are, where they are
/// in causal coords, what rotor they proposed.
#[derive(Debug, Clone)]
pub struct RoundSubmission {
    pub did_hash: B256,
    pub coord: CausalCoord,
    pub rotor: Rotor,
}

/// A clique of validators showing coordination beyond what their causal
/// separation would justify.
#[derive(Debug, Clone)]
pub struct CoordinationClique {
    pub members: Vec<B256>,
    pub avg_rotor_distance: f64,
    pub avg_causal_separation_sq: f64,
    /// Higher = more suspicious. Ratio of expected-to-observed rotor distance
    /// given causal separation, under the null hypothesis that independent
    /// honest validators with different mempools shouldn't agree more
    /// tightly than their light-cone overlap suggests.
    pub coordination_score: f64,
}

/// Identify cliques of validators whose pairwise rotor agreement is
/// tighter than chance, given their causal separation.
///
/// Heuristic: for each pair, compute (rotor_distance / causal_distance).
/// Pairs in the bottom quantile form candidate edges; connected components
/// are cliques. Filter by minimum clique size and ratio threshold.
///
/// This is intentionally a heuristic — the Growformer optimizer should
/// tune `ratio_threshold` and `min_clique_size` against historical
/// false-positive rates.
pub fn detect_coordination_clique(
    submissions: &[RoundSubmission],
    ratio_threshold: f64,
    min_clique_size: usize,
) -> Vec<CoordinationClique> {
    let n = submissions.len();
    if n < min_clique_size {
        return Vec::new();
    }

    // Pairwise ratios.
    let mut edges: Vec<(usize, usize, f64, f64, f64)> = Vec::new();
    for i in 0..n {
        for j in (i + 1)..n {
            let rotor_d = submissions[i].rotor.distance(&submissions[j].rotor);
            let cd = &submissions[i].coord;
            let cd2 = &submissions[j].coord;
            let dt = cd.t - cd2.t;
            let dx = cd.x - cd2.x;
            let dy = cd.y - cd2.y;
            let dz = cd.z - cd2.z;
            // Use spacelike distance for causal separation; timelike pairs are
            // expected to agree more (they could have communicated).
            let causal_sep_sq = (dx * dx + dy * dy + dz * dz - dt * dt).max(0.0);
            let causal_sep = sqrt(causal_sep_sq);
            // Ratio: tiny rotor distance + large causal separation = suspicious.
            // We use rotor_d / (causal_sep + ε) — smaller = more suspicious.
            let ratio = rotor_d / (causal_sep + 1e-6);
            if ratio < ratio_threshold {
                edges.push((i, j, rotor_d, causal_sep_sq, ratio));
            }
        }
    }

    // Connected components over edge set.
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(parent: &mut Vec<usize>, x: usize) -> usize {
        if parent[x] != x {
            let r = find(parent, parent[x]);
            parent[x] = r;
        }
        parent[x]
    }
    for (i, j, _, _, _) in &edges {
        let ri = find(&mut parent, *i);
        let rj = find(&mut parent, *j);
        if ri != rj {
            parent[ri] = rj;
        }
    }

    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for i in 0..n {
        let r = find(&mut parent, i);
        groups.entry(r).or_default().push(i);
    }

    let mut cliques = Vec::new();
    for (_, members) in groups.into_iter() {
        if members.len() < min_clique_size {
            continue;
        }
        let member_set: alloc::collections::BTreeSet<usize> = members.iter().copied().collect();
        let mut sum_rotor_d = 0.0;
        let mut sum_causal_sep_sq = 0.0;
        let mut pair_count = 0;
        for (i, j, rd, csq, _) in &edges {
            if member_set.contains(i) && member_set.contains(j) {
                sum_rotor_d += rd;
                sum_causal_sep_sq += csq;
                pair_count += 1;
            }
        }
        if pair_count == 0 {
            continue;
        }
        let avg_rotor_d = sum_rotor_d / pair_count as f64;
        let avg_causal_sq = sum_causal_sep_sq / pair_count as f64;
        let coordination_score = sqrt(avg_causal_sq) / (avg_rotor_d + 1e-6);
        cliques.push(CoordinationClique {
            members: members.iter().map(|i| submissions[*i].did_hash).collect(),
            avg_rotor_distance: avg_rotor_d,
            avg_causal_separation_sq: avg_causal_sq,
            coordination_score,
        });
    }
    cliques.sort_by(|a, b| {
        b.coordination_score
            .partial_cmp(&a.coordination_score)
            .unwrap_or(core::cmp::Ordering::Equal)
    });
    cliques
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometric_median_resists_outliers() {
        // 3 honest rotors clustered near identity, 2 malicious rotors far away.
        let honest_b = [
            Bivector {
                b: [0.0, 0.0, 0.0, 0.01, 0.0, 0.0],
            },
            Bivector {
                b: [0.0, 0.0, 0.0, 0.02, 0.0, 0.0],
            },
            Bivector {
                b: [0.0, 0.0, 0.0, 0.015, 0.0, 0.0],
            },
        ];
        let malicious_b = [
            Bivector {
                b: [0.0, 0.0, 0.0, 2.0, 0.0, 0.0],
            },
            Bivector {
                b: [0.0, 0.0, 0.0, -2.0, 0.0, 0.0],
            },
        ];
        let inputs: Vec<(Rotor, f64)> = honest_b
            .iter()
            .chain(malicious_b.iter())
            .map(|b| (Rotor::exp(b), 1.0))
            .collect();

        let median = geometric_median_rotor(&inputs, &GeometricMedianConfig::default()).unwrap();
        // Geometric median should be close to the honest cluster.
        let true_center = Rotor::exp(&Bivector {
            b: [0.0, 0.0, 0.0, 0.015, 0.0, 0.0],
        });
        let d = median.distance(&true_center);
        assert!(d < 0.05, "median strayed too far: distance {}", d);
    }

    #[test]
    fn fingerprint_bytes_round_trip() {
        let mut fp = RotorFingerprint::new(0.95);
        fp.update(Rotor::exp(&Bivector {
            b: [0.0, 0.0, 0.0, 0.01, 0.0, 0.0],
        }));
        let bytes = fp.to_bytes();
        let parsed = RotorFingerprint::from_bytes(&bytes).expect("parse");
        assert_eq!(parsed.samples, fp.samples);
        assert_eq!(
            parsed.digest(|b| *alloy_primitives::keccak256(b)),
            fp.digest(|b| *alloy_primitives::keccak256(b))
        );
    }

    #[test]
    fn fingerprint_detects_wake_up() {
        let mut fp = RotorFingerprint::new(0.95);
        // Train on 20 small rotations.
        for i in 0..20 {
            let b = Bivector {
                b: [0.0, 0.0, 0.0, 0.01 + 0.001 * (i as f64), 0.0, 0.0],
            };
            fp.update(Rotor::exp(&b));
        }
        // Submit a far-away rotor.
        let attack = Rotor::exp(&Bivector {
            b: [0.0, 0.0, 0.0, 2.0, 0.0, 0.0],
        });
        assert!(fp.is_anomalous(attack, 4.0));
    }

    #[test]
    fn joint_signature_catches_residual_only_attack() {
        let mut registry = FingerprintRegistry::new(0.95);
        let did = B256::from([0xA1; 32]);
        let normal_rotor = Rotor::exp(&Bivector {
            b: [0.0, 0.0, 0.0, 0.05, 0.0, 0.0],
        });
        for _ in 0..40 {
            registry.observe_joint(did, normal_rotor, 0.01);
        }
        let attack_anomaly = registry
            .check_joint(&did, normal_rotor, 5.0, 4.0)
            .expect("fingerprint warm");
        assert!(
            attack_anomaly,
            "joint signature must catch residual-only attack"
        );
        let rotor_only_anomaly = registry.check(&did, normal_rotor, 4.0).unwrap_or(false);
        assert!(!rotor_only_anomaly, "rotor-only check must not flag");
    }

    #[test]
    fn clique_detection_finds_coordinated_distant_validators() {
        // 3 validators at spacelike-separated coords producing nearly identical rotors.
        let suspicious_rotor = Rotor::exp(&Bivector {
            b: [0.0, 0.0, 0.0, 1.5, 0.0, 0.0],
        });
        let suspicious = vec![
            RoundSubmission {
                did_hash: B256::from([1u8; 32]),
                coord: CausalCoord {
                    t: 1.0,
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                rotor: suspicious_rotor,
            },
            RoundSubmission {
                did_hash: B256::from([2u8; 32]),
                coord: CausalCoord {
                    t: 1.0,
                    x: 5.0,
                    y: 0.0,
                    z: 0.0,
                },
                rotor: suspicious_rotor,
            },
            RoundSubmission {
                did_hash: B256::from([3u8; 32]),
                coord: CausalCoord {
                    t: 1.0,
                    x: 0.0,
                    y: 5.0,
                    z: 0.0,
                },
                rotor: suspicious_rotor,
            },
        ];
        // 3 honest validators producing varying rotors.
        let honest = vec![
            RoundSubmission {
                did_hash: B256::from([4u8; 32]),
                coord: CausalCoord {
                    t: 1.0,
                    x: 2.0,
                    y: 0.0,
                    z: 0.0,
                },
                rotor: Rotor::exp(&Bivector {
                    b: [0.0, 0.0, 0.0, 0.1, 0.0, 0.0],
                }),
            },
            RoundSubmission {
                did_hash: B256::from([5u8; 32]),
                coord: CausalCoord {
                    t: 1.0,
                    x: 0.0,
                    y: 2.0,
                    z: 0.0,
                },
                rotor: Rotor::exp(&Bivector {
                    b: [0.0, 0.0, 0.0, 0.3, 0.0, 0.0],
                }),
            },
            RoundSubmission {
                did_hash: B256::from([6u8; 32]),
                coord: CausalCoord {
                    t: 1.0,
                    x: 0.0,
                    y: 0.0,
                    z: 2.0,
                },
                rotor: Rotor::exp(&Bivector {
                    b: [0.0, 0.0, 0.0, 0.05, 0.0, 0.0],
                }),
            },
        ];
        let mut all = suspicious;
        all.extend(honest);

        let cliques = detect_coordination_clique(&all, 0.5, 3);
        assert!(!cliques.is_empty(), "should detect at least one clique");
        // The top clique should contain validators 1, 2, 3.
        let top = &cliques[0];
        let dids: alloc::collections::BTreeSet<u8> =
            top.members.iter().map(|h| h.as_slice()[0]).collect();
        assert!(dids.contains(&1));
        assert!(dids.contains(&2));
        assert!(dids.contains(&3));
    }
}
