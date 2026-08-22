//! # SpaceKit Network - Quantum-Resistant Decentralized Identity
//!
//! A quantum-resistant decentralized identity (DID) system built on SPHINCS+ signatures.
//!
//! ## no_std support
//!
//! Disable the default `std` feature for `no_std` / WASM builds. The core
//! `SphincsPlus` primitives (keygen, sign, verify) are always available.
//! The full wallet, credential, and DID modules require `std`.

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

#[cfg(feature = "std")]
extern crate std;

#[cfg(feature = "std")]
pub mod did;

// Core quantum crypto — always available (no_std compatible)
pub mod sphincs;

pub use sphincs::{CryptoError, QuantumKeyPair, SphincsPlus};

// Re-export main types for convenient access (std only)
#[cfg(feature = "std")]
pub use did::quantum::{
    DecentralizedIdentifier, IdentityDocument, QuantumResistantWallet, VerifiableCredential,
};

#[cfg(feature = "std")]
pub use did::credential_status::{CredentialStatusChecker, RegistryStatusChecker};
#[cfg(feature = "std")]
pub use did::did_registry_client::{
    CredentialStatus, DidDocument, ServiceEndpoint, VerifiableDataRegistry, VerificationMethod,
};
#[cfg(feature = "std")]
pub use did::did_spacekit::{
    DidKeyExtractor, DidResolver, DidVerificationKey, SpacekitDidResolver, SpacekitKeyExtractor,
};
#[cfg(feature = "std")]
pub use did::did_wallet::{DidWallet, InMemoryDidWallet, LocalDid};
#[cfg(feature = "std")]
pub use did::trust_policy::{AccessPolicy, PolicyDecision, VpnPolicy};
#[cfg(feature = "std")]
pub use did::vc_issuer::{DidBasedVcIssuer, VcIssuer, VpnAccessCredential};
#[cfg(feature = "std")]
pub use did::vc_verifier::{SpacekitVcVerifier, VcVerifier, VpnAccessClaim};

/// Current version of the SpaceKit Network DID library
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Default DID method used by this library
pub const DEFAULT_DID_METHOD: &str = "spacekit:testnet";

#[cfg(feature = "std")]
/// Comprehensive demo of the quantum-resistant DID system
pub fn demo() {
    use std::collections::HashMap;

    println!("🔐 Quantum-Resistant Decentralized Identity System Demo\n");

    // Create a new quantum-resistant wallet
    let mut alice_wallet = QuantumResistantWallet::new();
    println!("Created Alice's wallet:");
    println!("{}\n", alice_wallet);

    // Sign some content
    let content = "This is a quantum-resistant signed message from Alice in Wonderland.";
    let signature = alice_wallet.sign_content(content).unwrap();
    println!("Signed content: \"{}\"", content);
    println!("Signature: {}\n", &signature[0..32]);

    // Verify the signature
    let is_valid = alice_wallet.verify_content(content, &signature).unwrap();
    println!(
        "Signature verification: {}\n",
        if is_valid { "✅ Valid" } else { "❌ Invalid" }
    );

    // Create another wallet to act as credential subject
    let bob_wallet = QuantumResistantWallet::new();
    println!("Created Bob's wallet:");
    println!("{}\n", bob_wallet);

    // Issue a credential from Alice to Bob
    let mut claims = HashMap::new();
    claims.insert("name".to_string(), "Bob Johnson".to_string());
    claims.insert("role".to_string(), "Developer".to_string());
    claims.insert("clearance".to_string(), "Level 3".to_string());

    let credential = alice_wallet
        .issue_credential(
            bob_wallet.identity_doc.did.as_ref(),
            "EmployeeCredential",
            claims,
            Some(730), // 2 years validity
        )
        .unwrap();

    println!("📜 Alice issued credential to Bob:");
    println!("  ID: {}", credential.id);
    println!("  Type: {}", credential.credential_type);
    println!(
        "  Issued: {}",
        credential.issued_at.format("%Y-%m-%d %H:%M:%S UTC")
    );
    println!("  Claims: {:?}\n", credential.claims);

    // Verify the credential
    let cred_valid = alice_wallet.verify_credential(&credential).unwrap();
    println!(
        "Credential verification: {}\n",
        if cred_valid {
            "✅ Valid"
        } else {
            "❌ Invalid"
        }
    );

    // Add credential to Alice's wallet (as issuer record)
    alice_wallet.add_credential(credential.clone());

    // Demonstrate key rotation
    println!("🔄 Rotating Alice's keys...");
    alice_wallet.rotate_keys().unwrap();
    println!("Alice now has {} key pairs\n", alice_wallet.key_pairs.len());

    // Export identity document
    println!("📄 Alice's Identity Document:");
    println!("{}\n", alice_wallet.export_identity_document().unwrap());

    // Demonstrate proof presentation
    let employee_credentials = alice_wallet.get_credentials_by_type("EmployeeCredential");
    println!(
        "👤 Employee credentials found: {}",
        employee_credentials.len()
    );

    let proof = alice_wallet.present_proof(&["EmployeeCredential"]);
    println!("🎫 Proof presentation contains {} credentials", proof.len());

    println!("\n✨ Demo completed! This system provides:");
    println!("  • Quantum-resistant signatures using SPHINCS+");
    println!("  • Decentralized identity management");
    println!("  • Verifiable credentials");
    println!("  • Key rotation capabilities");
    println!("  • Wallet address derivation from quantum keys");
}

#[cfg(all(test, feature = "std"))]
mod integration_tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_end_to_end_workflow() {
        // Create two wallets
        let issuer = QuantumResistantWallet::new();
        let mut holder = QuantumResistantWallet::new();

        // Issuer creates a credential for holder
        let mut claims = HashMap::new();
        claims.insert("name".to_string(), "Integration Test User".to_string());
        claims.insert("level".to_string(), "Advanced".to_string());

        let credential = issuer
            .issue_credential(
                holder.identity_doc.did.as_ref(),
                "TestCredential",
                claims,
                Some(30),
            )
            .unwrap();

        // Verify the credential
        assert!(issuer.verify_credential(&credential).unwrap());

        // Holder stores the credential
        holder.add_credential(credential.clone());

        // Present proof
        let proof = holder.present_proof(&["TestCredential"]);
        assert_eq!(proof.len(), 1);
        assert_eq!(proof[0].id, credential.id);
    }

    #[test]
    fn test_library_version() {
        assert!(!VERSION.is_empty());
    }
}
