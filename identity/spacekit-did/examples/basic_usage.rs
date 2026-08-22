use spacekit_did::QuantumResistantWallet;
use std::collections::HashMap;

fn main() {
    println!("SpaceKit Network DID Library - Basic Usage Example\n");

    // Create a quantum-resistant wallet
    let wallet = QuantumResistantWallet::new();
    println!(
        "Created wallet with DID: {}",
        wallet.identity_doc.did.as_ref()
    );

    // Sign and verify content
    let message = "Hello from the quantum realm!";
    let signature = wallet
        .sign_content(message)
        .expect("Failed to sign content");
    let is_valid = wallet
        .verify_content(message, &signature)
        .expect("Failed to verify signature");

    println!("Signed message: '{}'", message);
    println!("Signature valid: {}", is_valid);

    // Create a credential
    let mut claims = HashMap::new();
    claims.insert("email".to_string(), "user@example.com".to_string());
    claims.insert("verified".to_string(), "true".to_string());

    let credential = wallet
        .issue_credential(
            "did:spacekit:testnet:target123",
            "EmailVerification",
            claims,
            Some(90), // 90 days validity
        )
        .expect("Failed to issue credential");

    println!("\nIssued credential:");
    println!("  ID: {}", credential.id);
    println!("  Type: {}", credential.credential_type);
    println!("  Claims: {:?}", credential.claims);

    // Verify the credential
    let cred_valid = wallet
        .verify_credential(&credential)
        .expect("Failed to verify credential");
    println!("  Valid: {}", cred_valid);

    println!("\nLibrary usage example completed successfully!");
}
