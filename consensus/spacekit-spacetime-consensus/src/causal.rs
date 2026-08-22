//! Causal-set ordering for the spacetime consensus layer.
//!
//! Every consensus event (proposal, vote, commit, transaction inclusion) is
//! tagged with a `CausalEvent` carrying a Minkowski-style coordinate. The
//! partial order is: `e₁ ≤ e₂` iff `e₂ - e₁` is causal (timelike or null with
//! positive time component) — i.e. `e₂` is in the forward light cone of `e₁`.
//!
//! Why this matters for your topology:
//!
//! - **Genesis full node (no VM)** anchors the origin (0, 0, 0, 0).
//! - **Validator full nodes** advance time monotonically; their events lie on
//!   timelike worldlines from genesis.
//! - **Browser VM nodes** receive events bounded by network propagation —
//!   their "light cone" is the gossip horizon. Events outside the cone are
//!   *concurrent* and conflict-resolved by deterministic rule (e.g. lower
//!   rotor norm wins), not by ordering.
//! - **Stateless light clients** check `≤` between received events without
//!   touching state; this is constant-time per pair.
//!
//! This replaces hand-rolled Lamport timestamps with a metric structure that
//! survives validator-frame changes (boosts in the algebra correspond to
//! changes in the gossip-rate-of-light reference).

use alloc::vec::Vec;
use alloy_primitives::B256;

/// A point in (1+3)-D Minkowski coordinates. The first component is "time"
/// (in some chosen unit — block height × expected block time, or true wall
/// clock), the next three are "space" (here, deterministic hashes of network
/// position, e.g. validator DID hash mapped to a sphere).
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CausalCoord {
    pub t: f64,
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl CausalCoord {
    pub const ORIGIN: Self = Self {
        t: 0.0,
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };

    /// Minkowski inner product with signature (+,-,-,-).
    pub fn minkowski_dot(&self, other: &Self) -> f64 {
        self.t * other.t - self.x * other.x - self.y * other.y - self.z * other.z
    }

    /// Squared interval. > 0: timelike; = 0: null; < 0: spacelike.
    pub fn interval_sq_to(&self, other: &Self) -> f64 {
        let dt = other.t - self.t;
        let dx = other.x - self.x;
        let dy = other.y - self.y;
        let dz = other.z - self.z;
        dt * dt - dx * dx - dy * dy - dz * dz
    }

    /// Derive a deterministic spatial coordinate from a DID hash. Maps the
    /// 32-byte hash to a unit-sphere position via the first 24 bytes
    /// (split into 8-byte little-endian floats).
    pub fn spatial_from_hash(hash: &B256) -> (f64, f64, f64) {
        let bytes: &[u8] = hash.as_slice();
        let mut buf = [0u8; 8];
        buf.copy_from_slice(&bytes[0..8]);
        let x = f64::from_le_bytes(buf);
        buf.copy_from_slice(&bytes[8..16]);
        let y = f64::from_le_bytes(buf);
        buf.copy_from_slice(&bytes[16..24]);
        let z = f64::from_le_bytes(buf);
        // Project onto unit sphere to bound coordinates.
        let n = (x * x + y * y + z * z).sqrt();
        if n.is_finite() && n > 0.0 {
            (x / n, y / n, z / n)
        } else {
            (0.0, 0.0, 0.0)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CausalRelation {
    /// `a` is in the strict past light cone of `b`: timelike-separated, a.t < b.t.
    Precedes,
    /// `a` is on the past null cone of `b`: lightlike, a.t < b.t.
    NullPrecedes,
    /// Spacelike-separated: concurrent. Order is resolved by tie-break.
    Concurrent,
    /// `b` is in the past light cone of `a`.
    Succeeds,
    /// `b` is on the past null cone of `a`.
    NullSucceeds,
    /// Identical event.
    Identical,
}

/// A consensus event with a causal coordinate and a content hash.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CausalEvent {
    pub coord: CausalCoord,
    /// Deterministic content hash (block hash, vote hash, etc.).
    pub content: B256,
}

impl CausalEvent {
    pub fn relation_to(&self, other: &Self) -> CausalRelation {
        if self.coord == other.coord && self.content == other.content {
            return CausalRelation::Identical;
        }
        let dt = other.coord.t - self.coord.t;
        let dx = other.coord.x - self.coord.x;
        let dy = other.coord.y - self.coord.y;
        let dz = other.coord.z - self.coord.z;
        let interval = dt * dt - dx * dx - dy * dy - dz * dz;
        if interval > 1e-12 {
            // timelike
            if dt > 0.0 {
                CausalRelation::Precedes
            } else {
                CausalRelation::Succeeds
            }
        } else if interval.abs() <= 1e-12 {
            // null
            if dt > 0.0 {
                CausalRelation::NullPrecedes
            } else if dt < 0.0 {
                CausalRelation::NullSucceeds
            } else {
                CausalRelation::Concurrent
            }
        } else {
            CausalRelation::Concurrent
        }
    }

    pub fn is_in_past_of(&self, other: &Self) -> bool {
        matches!(
            self.relation_to(other),
            CausalRelation::Precedes | CausalRelation::NullPrecedes
        )
    }
}

/// A causal set: events plus their pairwise relations. Used for DAG ordering
/// in the validator pool. Kept simple — production implementations should
/// use a sparser representation (e.g., transitive reduction).
#[derive(Debug, Clone, Default)]
pub struct CausalSet {
    pub events: Vec<CausalEvent>,
}

impl CausalSet {
    pub fn new() -> Self {
        Self { events: Vec::new() }
    }

    pub fn push(&mut self, e: CausalEvent) {
        self.events.push(e);
    }

    /// Topologically sort the events into a linear extension of the partial
    /// order. Ties (concurrent events) are broken by content-hash ordering,
    /// which is deterministic across all honest validators.
    pub fn linearize(&self) -> Vec<&CausalEvent> {
        let mut sorted: Vec<&CausalEvent> = self.events.iter().collect();
        sorted.sort_by(|a, b| {
            // Primary: timelike coordinate
            match a
                .coord
                .t
                .partial_cmp(&b.coord.t)
                .unwrap_or(core::cmp::Ordering::Equal)
            {
                core::cmp::Ordering::Equal => {
                    // Concurrent or same time slice — use content hash
                    a.content.as_slice().cmp(b.content.as_slice())
                }
                ord => ord,
            }
        });
        sorted
    }

    /// Return the maximal antichain at a given time slice: the set of events
    /// pairwise spacelike-separated at coordinate-time `t`. Useful for batching
    /// parallel-executable transactions.
    pub fn antichain_at(&self, t: f64, eps: f64) -> Vec<&CausalEvent> {
        self.events
            .iter()
            .filter(|e| (e.coord.t - t).abs() < eps)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(t: f64, x: f64, y: f64, z: f64, byte: u8) -> CausalEvent {
        CausalEvent {
            coord: CausalCoord { t, x, y, z },
            content: B256::from([byte; 32]),
        }
    }

    #[test]
    fn timelike_precedes() {
        let a = ev(0.0, 0.0, 0.0, 0.0, 1);
        let b = ev(1.0, 0.1, 0.0, 0.0, 2);
        // (1)² - (0.1)² > 0, dt > 0
        assert_eq!(a.relation_to(&b), CausalRelation::Precedes);
        assert_eq!(b.relation_to(&a), CausalRelation::Succeeds);
    }

    #[test]
    fn spacelike_concurrent() {
        let a = ev(0.0, 0.0, 0.0, 0.0, 1);
        let b = ev(0.1, 1.0, 0.0, 0.0, 2);
        // (0.1)² - 1 < 0
        assert_eq!(a.relation_to(&b), CausalRelation::Concurrent);
    }

    #[test]
    fn linearize_is_deterministic() {
        let mut cs = CausalSet::new();
        cs.push(ev(2.0, 0.0, 0.0, 0.0, 3));
        cs.push(ev(0.0, 0.0, 0.0, 0.0, 1));
        cs.push(ev(1.0, 0.0, 0.0, 0.0, 2));
        let order: Vec<u8> = cs
            .linearize()
            .iter()
            .map(|e| e.content.as_slice()[0])
            .collect();
        assert_eq!(order, vec![1, 2, 3]);
    }
}
