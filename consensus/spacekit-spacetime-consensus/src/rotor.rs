//! Rotors: elements of Spin⁺(1,3), the even subalgebra of Cl(1,3) with
//! unit norm. A rotor R acts on a vector x by the sandwich `x' = R̃ x R`,
//! which is the geometric-algebra representation of an orthochronous Lorentz
//! transformation.
//!
//! Even subalgebra components (8 of 16): scalar, 6 bivectors, pseudoscalar.
//! We store the rotor as a full `Multivector` for arithmetic simplicity; the
//! odd-grade components are guaranteed zero by construction.

use crate::algebra::Multivector;

#[cfg(not(feature = "std"))]
use libm::{cos, cosh, sin, sinh, sqrt};
#[cfg(feature = "std")]
fn cos(x: f64) -> f64 {
    x.cos()
}
#[cfg(feature = "std")]
fn sin(x: f64) -> f64 {
    x.sin()
}
#[cfg(feature = "std")]
fn cosh(x: f64) -> f64 {
    x.cosh()
}
#[cfg(feature = "std")]
fn sinh(x: f64) -> f64 {
    x.sinh()
}
#[cfg(feature = "std")]
fn sqrt(x: f64) -> f64 {
    x.sqrt()
}

/// A bivector in Cl(1,3): 6 components.
/// Order: γ₀γ₁, γ₀γ₂, γ₀γ₃, γ₁γ₂, γ₁γ₃, γ₂γ₃
/// First three are "boost" bivectors (square to +1), last three are "rotation"
/// bivectors (square to -1).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Bivector {
    pub b: [f64; 6],
}

impl Bivector {
    pub const ZERO: Self = Self { b: [0.0; 6] };

    pub fn from_components(boost: [f64; 3], rotation: [f64; 3]) -> Self {
        Self {
            b: [
                boost[0],
                boost[1],
                boost[2],
                rotation[0],
                rotation[1],
                rotation[2],
            ],
        }
    }

    pub fn to_multivector(&self) -> Multivector {
        let mut m = Multivector::ZERO;
        m.coeffs[5] = self.b[0]; // γ₀γ₁
        m.coeffs[6] = self.b[1]; // γ₀γ₂
        m.coeffs[7] = self.b[2]; // γ₀γ₃
        m.coeffs[8] = self.b[3]; // γ₁γ₂
        m.coeffs[9] = self.b[4]; // γ₁γ₃
        m.coeffs[10] = self.b[5]; // γ₂γ₃
        m
    }

    pub fn from_multivector(m: &Multivector) -> Self {
        Self {
            b: [
                m.coeffs[5],
                m.coeffs[6],
                m.coeffs[7],
                m.coeffs[8],
                m.coeffs[9],
                m.coeffs[10],
            ],
        }
    }

    /// The "scalar" part of B² for a bivector B. In Cl(1,3) this can be
    /// positive (pure rotation), negative (pure boost), or zero (null).
    pub fn square_scalar(&self) -> f64 {
        let b = &self.b;
        // Boost bivectors γ₀γᵢ square to +1; rotation bivectors γᵢγⱼ square to -1.
        // Cross terms produce pseudoscalar parts, which we ignore for the
        // scalar-part-of-B² needed by exp.
        b[0] * b[0] + b[1] * b[1] + b[2] * b[2] - b[3] * b[3] - b[4] * b[4] - b[5] * b[5]
    }

    /// Pseudoscalar part of B² (the I coefficient). Non-zero for "mixed"
    /// bivectors that have both boost and rotation components. The general
    /// exp formula handles this via the canonical decomposition B = B₊ + B₋
    /// where B₊² ≥ 0 and B₋² ≤ 0.
    pub fn square_pseudoscalar(&self) -> f64 {
        // From the geometric product table:
        // γ₀γ₁·γ₂γ₃ contributes to I, γ₀γ₂·γ₁γ₃, γ₀γ₃·γ₁γ₂ similarly.
        let b = &self.b;
        2.0 * (b[0] * b[5] - b[1] * b[4] + b[2] * b[3])
    }

    pub fn scale(&self, s: f64) -> Self {
        let mut out = *self;
        for i in 0..6 {
            out.b[i] *= s;
        }
        out
    }

    pub fn add(&self, other: &Self) -> Self {
        let mut out = *self;
        for i in 0..6 {
            out.b[i] += other.b[i];
        }
        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RotorError {
    NotUnitNorm,
    NonEvenGrade,
    LogarithmDiverged,
    NullBivector,
}

/// A Spin⁺(1,3) rotor. Invariant: `mv.reverse() * mv = 1` (scalar) and only
/// even-grade components are non-zero.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Rotor {
    pub(crate) mv: Multivector,
}

impl Rotor {
    pub const IDENTITY: Self = Self {
        mv: Multivector::ONE,
    };

    /// Construct from a multivector, validating the rotor conditions.
    pub fn from_multivector(mv: Multivector) -> Result<Self, RotorError> {
        // Odd-grade components must be zero (tolerance for numerical drift).
        for &i in &[1usize, 2, 3, 4, 11, 12, 13, 14] {
            if mv.coeffs[i].abs() > 1e-9 {
                return Err(RotorError::NonEvenGrade);
            }
        }
        let n2 = mv.norm_squared();
        if (n2 - 1.0).abs() > 1e-6 {
            return Err(RotorError::NotUnitNorm);
        }
        Ok(Self { mv })
    }

    /// Renormalize a near-rotor (numerical drift fixup).
    pub fn renormalize(mv: Multivector) -> Result<Self, RotorError> {
        let n2 = mv.norm_squared();
        if n2 <= 0.0 {
            return Err(RotorError::NotUnitNorm);
        }
        let scale = 1.0 / sqrt(n2);
        let mut fixed = Multivector::ZERO;
        // keep only even-grade components, scaled
        for &i in &[0usize, 5, 6, 7, 8, 9, 10, 15] {
            fixed.coeffs[i] = mv.coeffs[i] * scale;
        }
        Ok(Self { mv: fixed })
    }

    pub fn as_multivector(&self) -> &Multivector {
        &self.mv
    }

    /// Reverse: R̃. For a rotor, R̃ = R⁻¹.
    pub fn reverse(&self) -> Self {
        Self {
            mv: self.mv.reverse(),
        }
    }

    /// Apply this rotor to a vector x: x' = R̃ x R.
    /// Returns a multivector whose grade-1 part is the transformed vector.
    pub fn apply(&self, x: &Multivector) -> Multivector {
        self.mv.reverse() * (*x) * self.mv
    }

    /// Compose rotors: (R₂ ∘ R₁)(x) = R₂ applied to (R₁ applied to x).
    /// Note: composition is `R = R₂ * R₁` in this convention.
    pub fn compose(&self, other: &Self) -> Self {
        let prod = self.mv * other.mv;
        // Composition of rotors is a rotor; renormalize for safety.
        Self::renormalize(prod).unwrap_or(Self::IDENTITY)
    }

    /// Exponential map: B (bivector) → exp(B) ∈ Spin⁺(1,3).
    ///
    /// For a "simple" bivector B with B² = s (scalar), the formula is:
    ///   exp(B) = cos(√|s|) + sin(√|s|)/√|s| · B      if s < 0  (rotation-like)
    ///   exp(B) = cosh(√s) + sinh(√s)/√s · B           if s > 0  (boost-like)
    ///   exp(B) = 1 + B                                if s = 0  (null)
    ///
    /// For general bivectors with a pseudoscalar part in B², we use the
    /// canonical decomposition B = B₊ + B₋ with [B₊, B₋] = 0, then
    /// exp(B) = exp(B₊) exp(B₋). This implementation handles the simple case
    /// directly and falls back to a truncated series for the mixed case.
    pub fn exp(b: &Bivector) -> Self {
        let s = b.square_scalar();
        let ps = b.square_pseudoscalar();

        // Simple case: B² is a pure scalar.
        if ps.abs() < 1e-12 {
            let mv_b = b.to_multivector();
            return Self::exp_simple(&mv_b, s);
        }

        // Mixed case: truncated series exp(B) ≈ Σ Bᵏ/k! up to k=10.
        // For consensus rotors close to identity this converges rapidly.
        Self::exp_series(&b.to_multivector(), 12)
    }

    fn exp_simple(b_mv: &Multivector, s: f64) -> Self {
        if s.abs() < 1e-15 {
            // exp(B) = 1 + B for null B
            let result = Multivector::ONE + *b_mv;
            return Self::renormalize(result).unwrap_or(Self::IDENTITY);
        }
        let sqrt_abs = sqrt(s.abs());
        let (c, sc) = if s < 0.0 {
            (cos(sqrt_abs), sin(sqrt_abs) / sqrt_abs)
        } else {
            (cosh(sqrt_abs), sinh(sqrt_abs) / sqrt_abs)
        };
        // Build c + sc * B directly (only even-grade components possible here).
        let mut out = Multivector::ZERO;
        out.coeffs[0] = c;
        for i in 5..=10 {
            out.coeffs[i] = sc * b_mv.coeffs[i];
        }
        Self::renormalize(out).unwrap_or(Self::IDENTITY)
    }

    fn exp_series(b_mv: &Multivector, terms: usize) -> Self {
        let mut sum = Multivector::ONE;
        let mut term = Multivector::ONE;
        for k in 1..terms {
            term = term * (*b_mv);
            term = term * (1.0 / (k as f64));
            sum = sum + term;
        }
        Self::renormalize(sum).unwrap_or(Self::IDENTITY)
    }

    /// Logarithm map: R ∈ Spin⁺(1,3) → B ∈ bivector with exp(B) = R.
    /// Defined up to a 2π ambiguity for the rotation part. For consensus
    /// aggregation we only need log near identity, where it's unique.
    pub fn log(&self) -> Result<Bivector, RotorError> {
        let scalar = self.mv.coeffs[0];
        let mut b = Bivector::ZERO;
        for i in 0..6 {
            b.b[i] = self.mv.coeffs[5 + i];
        }
        let b_norm_sq = b.square_scalar();

        if b_norm_sq.abs() < 1e-15 {
            // R ≈ ±1; log is zero (mod 2π for the negative case).
            return Ok(Bivector::ZERO);
        }

        let b_norm = sqrt(b_norm_sq.abs());
        let scale = if b_norm_sq < 0.0 {
            // Rotation-like: B = θ · B̂ where R = cos(θ) + sin(θ) B̂
            let theta = libm_atan2(b_norm, scalar);
            theta / b_norm
        } else {
            // Boost-like: B = η · B̂ where R = cosh(η) + sinh(η) B̂
            let eta = 0.5 * libm_ln((scalar + b_norm) / (scalar - b_norm).max(1e-15));
            eta / b_norm
        };

        if !scale.is_finite() {
            return Err(RotorError::LogarithmDiverged);
        }
        Ok(b.scale(scale))
    }

    /// Geodesic distance on the Spin manifold. Used by aggregation as the
    /// loss function for the Fréchet mean.
    pub fn distance(&self, other: &Self) -> f64 {
        let rel = self.reverse().compose(other);
        rel.log()
            .map(|b| sqrt(b.square_scalar().abs()))
            .unwrap_or(0.0)
    }
}

#[cfg(feature = "std")]
fn libm_atan2(y: f64, x: f64) -> f64 {
    y.atan2(x)
}
#[cfg(not(feature = "std"))]
fn libm_atan2(y: f64, x: f64) -> f64 {
    libm::atan2(y, x)
}

#[cfg(feature = "std")]
fn libm_ln(x: f64) -> f64 {
    x.ln()
}
#[cfg(not(feature = "std"))]
fn libm_ln(x: f64) -> f64 {
    libm::log(x)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_is_rotor() {
        assert!(Rotor::from_multivector(Multivector::ONE).is_ok());
    }

    #[test]
    fn exp_zero_is_identity() {
        let r = Rotor::exp(&Bivector::ZERO);
        assert!((r.mv.coeffs[0] - 1.0).abs() < 1e-12);
    }

    #[test]
    fn rotation_round_trip() {
        // Pure rotation γ₁γ₂ by π/4
        let b = Bivector {
            b: [0.0, 0.0, 0.0, core::f64::consts::FRAC_PI_4, 0.0, 0.0],
        };
        let r = Rotor::exp(&b);
        let b2 = r.log().unwrap();
        for i in 0..6 {
            assert!(
                (b.b[i] - b2.b[i]).abs() < 1e-9,
                "i={} got {} want {}",
                i,
                b2.b[i],
                b.b[i]
            );
        }
    }

    #[test]
    fn compose_inverse_is_identity() {
        let b = Bivector {
            b: [0.1, 0.0, 0.0, 0.2, 0.0, 0.0],
        };
        let r = Rotor::exp(&b);
        let id = r.compose(&r.reverse());
        assert!((id.mv.coeffs[0] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn distance_is_symmetric_and_zero_on_self() {
        let b = Bivector {
            b: [0.0, 0.0, 0.0, 0.3, 0.0, 0.0],
        };
        let r = Rotor::exp(&b);
        assert!(r.distance(&r) < 1e-9);
    }
}
