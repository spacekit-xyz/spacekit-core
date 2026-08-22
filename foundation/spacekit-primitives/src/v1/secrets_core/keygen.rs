//! Full keypair generation: ML-KEM-768 + signer (ML-DSA-65 or SLH-DSA).

use anyhow::Result;
use zeroize::Zeroizing;

use crate::v1::secrets_core::kem;
use crate::v1::secrets_core::signature;
use crate::v1::secrets_core::types::{QuantumKeyMaterial, SignerVariant};

/// Generate a full quantum keypair: ML-KEM-768 + signer (ML-DSA-65 or SLH-DSA).
pub fn generate_keypair(variant: SignerVariant) -> Result<QuantumKeyMaterial> {
    let (decap_seed, encap_bytes) = kem::generate_keypair()?;
    let decap_bytes = Zeroizing::new(decap_seed.as_ref().to_vec());

    let (sk_bytes, pk_bytes) = signature::generate_signer_keypair(&variant)?;

    Ok(QuantumKeyMaterial {
        kem_decap_bytes: decap_bytes,
        signer_sk_bytes: sk_bytes,
        signer_variant: variant,
        kem_encap_bytes: encap_bytes,
        signer_pk_bytes: pk_bytes,
    })
}
