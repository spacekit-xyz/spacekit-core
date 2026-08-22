//! Cl(1,3) — the spacetime algebra of signature (+,-,-,-).
//!
//! Basis convention (Hestenes):
//!   γ_0² = +1, γ_1² = γ_2² = γ_3² = -1, γ_i γ_j = -γ_j γ_i (i ≠ j)
//!
//! 16-element multivector laid out by grade:
//!
//!   index | grade | basis blade
//!   ------+-------+------------
//!     0   |   0   | 1
//!     1   |   1   | γ_0
//!     2   |   1   | γ_1
//!     3   |   1   | γ_2
//!     4   |   1   | γ_3
//!     5   |   2   | γ_0 γ_1   (boost)
//!     6   |   2   | γ_0 γ_2   (boost)
//!     7   |   2   | γ_0 γ_3   (boost)
//!     8   |   2   | γ_1 γ_2   (rotation)
//!     9   |   2   | γ_1 γ_3   (rotation)
//!    10   |   2   | γ_2 γ_3   (rotation)
//!    11   |   3   | γ_0 γ_1 γ_2
//!    12   |   3   | γ_0 γ_1 γ_3
//!    13   |   3   | γ_0 γ_2 γ_3
//!    14   |   3   | γ_1 γ_2 γ_3
//!    15   |   4   | I = γ_0 γ_1 γ_2 γ_3   (pseudoscalar)
//!
//! The geometric product is computed by representing each blade as a bitmask
//! over {γ_0, γ_1, γ_2, γ_3}, XOR-ing the masks (since γ_i γ_i = ±1 removes
//! the pair), and tracking sign from anticommutation + metric.

use core::ops::{Add, Mul, Neg, Sub};

#[cfg(not(feature = "std"))]
use libm::sqrt;
#[cfg(feature = "std")]
fn sqrt(x: f64) -> f64 {
    x.sqrt()
}

pub const BASIS_DIM: usize = 16;

/// Bitmask → multivector index. Bit 0 = γ_0, bit 1 = γ_1, bit 2 = γ_2, bit 3 = γ_3.
const BLADE_INDEX: [usize; 16] = [
    0,  // 0000 -> 1
    1,  // 0001 -> γ_0
    2,  // 0010 -> γ_1
    5,  // 0011 -> γ_0 γ_1
    3,  // 0100 -> γ_2
    6,  // 0101 -> γ_0 γ_2
    8,  // 0110 -> γ_1 γ_2
    11, // 0111 -> γ_0 γ_1 γ_2
    4,  // 1000 -> γ_3
    7,  // 1001 -> γ_0 γ_3
    9,  // 1010 -> γ_1 γ_3
    12, // 1011 -> γ_0 γ_1 γ_3
    10, // 1100 -> γ_2 γ_3
    13, // 1101 -> γ_0 γ_2 γ_3
    14, // 1110 -> γ_1 γ_2 γ_3
    15, // 1111 -> I
];

/// Inverse mapping: multivector index → blade bitmask.
const INDEX_BLADE: [u8; 16] = [
    0b0000, // 1
    0b0001, // γ_0
    0b0010, // γ_1
    0b0100, // γ_2
    0b1000, // γ_3
    0b0011, // γ_0 γ_1
    0b0101, // γ_0 γ_2
    0b1001, // γ_0 γ_3
    0b0110, // γ_1 γ_2
    0b1010, // γ_1 γ_3
    0b1100, // γ_2 γ_3
    0b0111, // γ_0 γ_1 γ_2
    0b1011, // γ_0 γ_1 γ_3
    0b1101, // γ_0 γ_2 γ_3
    0b1110, // γ_1 γ_2 γ_3
    0b1111, // I
];

/// Metric: γ_0² = +1, γ_i² = -1 for i = 1,2,3.
#[inline]
fn metric(bit: u8) -> f64 {
    if bit == 0 {
        1.0
    } else {
        -1.0
    }
}

/// Sign from reordering blade `a` to standard order then merging with `b`.
/// Counts swaps when sliding bits of `b` past bits of `a` that lie above them.
#[inline]
fn reorder_sign(a: u8, b: u8) -> f64 {
    let mut sign = 1.0f64;
    let mut b_temp = b;
    while b_temp != 0 {
        // Count how many bits of `a` are higher than the lowest set bit of b_temp.
        let low_b = b_temp.trailing_zeros() as u8;
        let mask = !((1u8 << (low_b + 1)).wrapping_sub(1));
        let swaps = ((a & mask) as u32).count_ones();
        if swaps & 1 == 1 {
            sign = -sign;
        }
        b_temp &= b_temp - 1;
    }
    sign
}

/// Product of two basis blades (by bitmask). Returns (result_index, sign).
fn blade_product(a: u8, b: u8) -> (usize, f64) {
    let mut sign = reorder_sign(a, b);
    // For each γ_i appearing in both a and b, it squares — apply metric.
    let mut shared = a & b;
    while shared != 0 {
        let bit = shared.trailing_zeros() as u8;
        sign *= metric(bit);
        shared &= shared - 1;
    }
    let result_mask = a ^ b;
    (BLADE_INDEX[result_mask as usize], sign)
}

/// Element of Cl(1,3). 16 real coefficients in the basis order above.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Multivector {
    pub coeffs: [f64; BASIS_DIM],
}

impl Multivector {
    pub const ZERO: Self = Self {
        coeffs: [0.0; BASIS_DIM],
    };
    pub const ONE: Self = {
        let mut c = [0.0; BASIS_DIM];
        c[0] = 1.0;
        Self { coeffs: c }
    };

    #[inline]
    pub fn scalar(x: f64) -> Self {
        let mut c = [0.0; BASIS_DIM];
        c[0] = x;
        Self { coeffs: c }
    }

    /// Reverse: reverses the order of basis vectors in each blade.
    /// For grade k, the reverse multiplies by (-1)^(k(k-1)/2).
    pub fn reverse(&self) -> Self {
        let mut out = *self;
        // grades 0,1: +; grades 2,3: -; grade 4: +
        // indices 5..=10 are grade 2, 11..=14 are grade 3, 15 is grade 4
        for i in 5..=10 {
            out.coeffs[i] = -out.coeffs[i];
        }
        for i in 11..=14 {
            out.coeffs[i] = -out.coeffs[i];
        }
        out
    }

    /// Grade involution: negates odd-grade components.
    pub fn involute(&self) -> Self {
        let mut out = *self;
        for i in 1..=4 {
            out.coeffs[i] = -out.coeffs[i];
        }
        for i in 11..=14 {
            out.coeffs[i] = -out.coeffs[i];
        }
        out
    }

    /// Scalar part of A * Ã. For a rotor this equals 1 (rotor norm condition).
    pub fn norm_squared(&self) -> f64 {
        let prod = (*self) * self.reverse();
        prod.coeffs[0]
    }

    pub fn norm(&self) -> f64 {
        sqrt(self.norm_squared().abs())
    }

    /// Project onto a single grade.
    pub fn grade(&self, k: usize) -> Self {
        let mut out = Self::ZERO;
        let ranges: &[(usize, usize)] = match k {
            0 => &[(0, 0)],
            1 => &[(1, 4)],
            2 => &[(5, 10)],
            3 => &[(11, 14)],
            4 => &[(15, 15)],
            _ => return out,
        };
        for &(lo, hi) in ranges {
            for i in lo..=hi {
                out.coeffs[i] = self.coeffs[i];
            }
        }
        out
    }

    /// Deterministic byte serialization (little-endian f64 ×16). Used by the
    /// Verkle binding for stable commitments.
    pub fn to_bytes(&self) -> [u8; 128] {
        let mut out = [0u8; 128];
        for (i, c) in self.coeffs.iter().enumerate() {
            out[i * 8..(i + 1) * 8].copy_from_slice(&c.to_le_bytes());
        }
        out
    }

    pub fn from_bytes(b: &[u8; 128]) -> Self {
        let mut c = [0.0; BASIS_DIM];
        for i in 0..BASIS_DIM {
            let mut buf = [0u8; 8];
            buf.copy_from_slice(&b[i * 8..(i + 1) * 8]);
            c[i] = f64::from_le_bytes(buf);
        }
        Self { coeffs: c }
    }
}

impl Add for Multivector {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        let mut out = self;
        for i in 0..BASIS_DIM {
            out.coeffs[i] += rhs.coeffs[i];
        }
        out
    }
}

impl Sub for Multivector {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        let mut out = self;
        for i in 0..BASIS_DIM {
            out.coeffs[i] -= rhs.coeffs[i];
        }
        out
    }
}

impl Neg for Multivector {
    type Output = Self;
    fn neg(self) -> Self {
        let mut out = self;
        for i in 0..BASIS_DIM {
            out.coeffs[i] = -out.coeffs[i];
        }
        out
    }
}

impl Mul<f64> for Multivector {
    type Output = Self;
    fn mul(self, rhs: f64) -> Self {
        let mut out = self;
        for i in 0..BASIS_DIM {
            out.coeffs[i] *= rhs;
        }
        out
    }
}

/// Geometric product. O(256) blade-pair multiplies, fully unrolled by the
/// compiler at -O2. This is the only operation that depends on the metric.
impl Mul for Multivector {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self {
        let mut out = [0.0f64; BASIS_DIM];
        for i in 0..BASIS_DIM {
            let a_coef = self.coeffs[i];
            if a_coef == 0.0 {
                continue;
            }
            let a_mask = INDEX_BLADE[i];
            for j in 0..BASIS_DIM {
                let b_coef = rhs.coeffs[j];
                if b_coef == 0.0 {
                    continue;
                }
                let b_mask = INDEX_BLADE[j];
                let (k, sign) = blade_product(a_mask, b_mask);
                out[k] += sign * a_coef * b_coef;
            }
        }
        Self { coeffs: out }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metric_squares() {
        // γ_0² = +1, γ_1² = γ_2² = γ_3² = -1
        let mut g0 = Multivector::ZERO;
        g0.coeffs[1] = 1.0; // γ_0
        let mut g1 = Multivector::ZERO;
        g1.coeffs[2] = 1.0; // γ_1
        assert_eq!((g0 * g0).coeffs[0], 1.0);
        assert_eq!((g1 * g1).coeffs[0], -1.0);
    }

    #[test]
    fn anticommutation() {
        let mut g0 = Multivector::ZERO;
        g0.coeffs[1] = 1.0;
        let mut g1 = Multivector::ZERO;
        g1.coeffs[2] = 1.0;
        let ab = g0 * g1;
        let ba = g1 * g0;
        for i in 0..BASIS_DIM {
            assert!((ab.coeffs[i] + ba.coeffs[i]).abs() < 1e-12);
        }
    }

    #[test]
    fn pseudoscalar_squares_to_minus_one() {
        // I² = -1 in Cl(1,3)
        let mut i_elt = Multivector::ZERO;
        i_elt.coeffs[15] = 1.0;
        let sq = i_elt * i_elt;
        assert!((sq.coeffs[0] + 1.0).abs() < 1e-12);
    }

    #[test]
    fn round_trip_bytes() {
        let mut m = Multivector::ZERO;
        for i in 0..BASIS_DIM {
            m.coeffs[i] = (i as f64) * 0.1 - 0.3;
        }
        let b = m.to_bytes();
        let m2 = Multivector::from_bytes(&b);
        for i in 0..BASIS_DIM {
            assert!((m.coeffs[i] - m2.coeffs[i]).abs() < 1e-15);
        }
    }
}
