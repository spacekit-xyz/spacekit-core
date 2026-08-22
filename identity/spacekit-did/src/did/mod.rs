pub mod credential_status;
pub mod did_registry_client;
pub mod did_spacekit;
pub mod did_wallet;
pub mod quantum;
pub mod trust_policy;
pub mod vc_issuer;
pub mod vc_verifier;

use quantum::{DecentralizedIdentifier, QuantumResistantWallet};

pub fn create_quantum_wallet() -> QuantumResistantWallet {
    QuantumResistantWallet::new()
}

pub fn create_did() -> DecentralizedIdentifier {
    QuantumResistantWallet::new().identity_doc.did
}
