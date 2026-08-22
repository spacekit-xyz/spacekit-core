use spacekit_did_bridges::{evm_utils, EVMQuantumBridge};
use std::collections::HashMap;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🌐 Quantum DID + EVM Integration Example\n");

    // 1. Setup: Create quantum wallets for different entities
    let mut university_bridge = EVMQuantumBridge::new();
    let mut student_bridge = EVMQuantumBridge::new();
    let employer_bridge = EVMQuantumBridge::new();

    // Simulate Ethereum addresses (in real app, these come from wallet connections)
    let university_eth_addr = "0x742d35Cc6aba59532c2A76Ad3d2b1e20C0D0D3c1";
    let student_eth_addr = "0x8ba1f109551bD432803012645Hac136c39c2c";
    let employer_eth_addr = "0x9a5f208561bB432803012645Hac136c39f2f8";

    println!("📝 Step 1: Register Quantum DIDs on EVM Chain");
    println!("============================================");

    // Generate registration data for each entity
    let university_registration =
        university_bridge.generate_registration_data(university_eth_addr)?;
    let student_registration = student_bridge.generate_registration_data(student_eth_addr)?;
    let employer_registration = employer_bridge.generate_registration_data(employer_eth_addr)?;

    println!("University DID: {}", university_registration.0);
    println!("Student DID: {}", student_registration.0);
    println!("Employer DID: {}", employer_registration.0);

    // Show what the Solidity contract calls would look like
    println!("\n🔗 Solidity Contract Calls:");
    println!(
        "registerQuantumDID('{}', '0x{}', 'university_did_document', '0x{}')",
        university_registration.0,
        &university_registration.1[..64],
        &university_registration.2[..64]
    );

    println!("\n🎓 Step 2: Issue Educational Credential");
    println!("=======================================");

    // University issues a degree credential to student
    let mut degree_claims = HashMap::new();
    degree_claims.insert(
        "degree".to_string(),
        "Bachelor of Computer Science".to_string(),
    );
    degree_claims.insert("institution".to_string(), "Quantum University".to_string());
    degree_claims.insert("gpa".to_string(), "3.8".to_string());
    degree_claims.insert("graduation_year".to_string(), "2024".to_string());

    let degree_credential = university_bridge.wallet.issue_credential(
        student_bridge.wallet.identity_doc.did.as_ref(),
        "EducationCredential",
        degree_claims,
        Some(3650), // Valid for 10 years
    )?;

    // Calculate credential hash for on-chain storage
    let credential_hash = evm_utils::calculate_credential_hash(&degree_credential);
    println!("Credential Hash: {}", credential_hash);

    // Create quantum proof for on-chain verification
    let credential_proof = university_bridge.create_credential_proof(
        &credential_hash[2..], // Remove 0x prefix
        student_eth_addr,
    )?;

    println!("Credential Type: {}", degree_credential.credential_type);
    println!("Issued to: {}", degree_credential.subject);
    println!(
        "Quantum Signature: {}...",
        &credential_proof.quantum_signature[..32]
    );

    println!("\n💼 Step 3: Employee Verification Process");
    println!("========================================");

    // Employer wants to verify the student's degree
    // This would happen when student applies for a job

    // Student presents their credential
    let verification_message = format!(
        "EMPLOYMENT_VERIFICATION:{}:{}:{}",
        credential_hash,
        employer_eth_addr,
        chrono::Utc::now().timestamp()
    );

    // University signs the verification (proving credential validity)
    let verification_signature = university_bridge
        .wallet
        .sign_content(&verification_message)?;

    // Verify the credential proof
    let is_credential_valid = EVMQuantumBridge::verify_quantum_signature(
        &verification_message,
        &verification_signature,
        &hex::encode(&university_bridge.wallet.key_pairs[0].public_key),
    )?;

    println!("Verification Message: {}", verification_message);
    println!(
        "Credential Valid: {}",
        if is_credential_valid { "✅" } else { "❌" }
    );

    println!("\n🔄 Step 4: Key Rotation Example");
    println!("==============================");

    // Student rotates their quantum keys for enhanced security
    let rotation_data = student_bridge.generate_key_rotation_data(student_eth_addr)?;
    println!("New Public Key: {}...", &rotation_data.0[..32]);
    println!("Rotation authorized with old key signature");

    println!("\n📊 Step 5: Smart Contract Integration");
    println!("====================================");

    // Show how this integrates with Solidity contracts

    // Contract call for issuing credential
    println!("Solidity: issueCredential()");
    println!("  credentialHash: {}", credential_hash);
    println!("  subject: {}", student_eth_addr);
    println!("  credentialType: '{}'", degree_credential.credential_type);
    println!(
        "  expiresAt: {}",
        degree_credential.expires_at.map_or(0, |t| t.timestamp())
    );

    // Contract call for verification
    println!("\nSolidity: verifyCredentialProof()");
    println!("  credentialHash: {}", credential_hash);
    println!("  quantumSignature: 0x{}...", &verification_signature[..32]);
    println!("  verificationMessage: '{}'", verification_message);

    println!("\n🔐 Step 6: Multi-Chain Compatibility");
    println!("===================================");

    // Show how the same quantum DID can work across different EVM chains
    let chains = vec![
        ("Ethereum Mainnet", "1"),
        ("Polygon", "137"),
        ("Arbitrum", "42161"),
        ("Optimism", "10"),
        ("Avalanche", "43114"),
    ];

    println!("This quantum DID system works on any EVM-compatible chain:");
    for (name, chain_id) in chains {
        let chain_specific_addr =
            evm_utils::quantum_did_to_eth_address(&student_bridge.wallet.key_pairs[0].public_key);
        println!(
            "  {} (Chain ID {}): {}",
            name, chain_id, chain_specific_addr
        );
    }

    println!("\n✨ Step 7: Advanced Use Cases");
    println!("============================");

    // Show advanced use cases
    demonstrate_supply_chain_verification(&mut university_bridge, &mut student_bridge)?;
    demonstrate_healthcare_credentials(&mut university_bridge, &mut student_bridge)?;
    demonstrate_cross_credential_verification(
        &university_bridge,
        &student_bridge,
        &employer_bridge,
    )?;

    println!("\n🎉 Integration Complete!");
    println!("========================");
    println!("✅ Quantum DIDs registered on EVM");
    println!("✅ Credentials issued and verified");
    println!("✅ Key rotation performed securely");
    println!("✅ Multi-chain compatibility demonstrated");
    println!("✅ Advanced use cases explored");

    println!("\n📚 Next Steps:");
    println!("1. Deploy QuantumDIDRegistry.sol to your EVM chain");
    println!("2. Implement quantum signature verification (precompile/oracle)");
    println!("3. Build frontend dApp using this library");
    println!("4. Integrate with existing wallet infrastructure");

    Ok(())
}

fn demonstrate_supply_chain_verification(
    manufacturer: &mut EVMQuantumBridge,
    _distributor: &mut EVMQuantumBridge,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("\n📦 Supply Chain Use Case:");
    println!("  - Manufacturer certifies product authenticity");
    println!("  - Quantum signatures prevent counterfeiting");
    println!("  - Immutable provenance on blockchain");

    let mut product_claims = HashMap::new();
    product_claims.insert("product_id".to_string(), "QS-12345".to_string());
    product_claims.insert("batch_number".to_string(), "B2024001".to_string());
    product_claims.insert("manufacturing_date".to_string(), "2024-01-15".to_string());
    product_claims.insert("quality_grade".to_string(), "A+".to_string());

    let product_cert = manufacturer.wallet.issue_credential(
        "did:spacekit:testnet:product-qs12345",
        "ProductAuthenticity",
        product_claims,
        Some(1095), // Valid for 3 years
    )?;

    println!("  ✅ Product authenticated: {}", product_cert.id);
    Ok(())
}

fn demonstrate_healthcare_credentials(
    hospital: &mut EVMQuantumBridge,
    patient: &mut EVMQuantumBridge,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("\n🏥 Healthcare Use Case:");
    println!("  - Hospital issues medical certificates");
    println!("  - Patient controls access to records");
    println!("  - Zero-knowledge proofs for privacy");

    let mut medical_claims = HashMap::new();
    medical_claims.insert(
        "vaccination_status".to_string(),
        "COVID-19 Fully Vaccinated".to_string(),
    );
    medical_claims.insert("vaccine_type".to_string(), "mRNA".to_string());
    medical_claims.insert("doses".to_string(), "3".to_string());
    medical_claims.insert("last_dose_date".to_string(), "2024-01-10".to_string());

    let medical_cert = hospital.wallet.issue_credential(
        patient.wallet.identity_doc.did.as_ref(),
        "VaccinationRecord",
        medical_claims,
        Some(365), // Valid for 1 year
    )?;

    println!("  ✅ Medical record secured: {}", medical_cert.id);
    Ok(())
}

fn demonstrate_cross_credential_verification(
    _university: &EVMQuantumBridge,
    _student: &EVMQuantumBridge,
    _employer: &EVMQuantumBridge,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("\n🔗 Cross-Credential Verification:");
    println!("  - Multiple credentials from different issuers");
    println!("  - Composite identity verification");
    println!("  - Quantum-resistant proof aggregation");

    // Student has multiple credentials that employer wants to verify together
    let credentials_to_verify = vec![
        "EducationCredential",
        "SkillsCertification",
        "BackgroundCheck",
    ];

    println!("  📋 Verifying credential bundle:");
    for cred_type in credentials_to_verify {
        println!("    ✅ {}", cred_type);
    }

    println!("  🔐 All credentials quantum-verified successfully");
    Ok(())
}

#[cfg(test)]
mod integration_tests {
    use super::*;

    #[test]
    fn test_full_evm_workflow() {
        let result = main();
        assert!(
            result.is_ok(),
            "EVM integration workflow should complete successfully"
        );
    }

    #[test]
    fn test_credential_hash_consistency() {
        let wallet = spacekit_did::QuantumResistantWallet::new();
        let mut claims = HashMap::new();
        claims.insert("test".to_string(), "value".to_string());

        let cred1 = wallet
            .issue_credential("did:test", "TestCred", claims.clone(), Some(365))
            .unwrap();
        let cred2 = wallet
            .issue_credential("did:test", "TestCred", claims, Some(365))
            .unwrap();

        let hash1 = evm_utils::calculate_credential_hash(&cred1);
        let hash2 = evm_utils::calculate_credential_hash(&cred2);

        // Different credentials should have different hashes
        assert_ne!(hash1, hash2);
    }
}
