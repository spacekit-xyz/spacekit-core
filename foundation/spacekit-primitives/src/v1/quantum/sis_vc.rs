//! SIS-based VC API surface (Wee–Wu).
//! This re-exports the lattice VC primitives with SIS naming.

pub use crate::v1::quantum::lattice_vc::{
    Kyber1024Params as SisKyber1024Params, Kyber512Params as SisKyber512Params,
    Kyber768Params as SisKyber768Params, NistLatticeParameterSet as NistSisParameterSet,
    NistLatticeVc as NistSisVc, QuantumLatticeCommitment as SisCommitment,
    QuantumLatticeProof as SisProof, QuantumLatticeVc as WeeWuSisVc,
};
