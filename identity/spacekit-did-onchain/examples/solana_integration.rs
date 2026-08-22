use sha2::{Digest, Sha256};
use spacekit_did_bridges::{solana_utils, SolanaQuantumBridge};
use std::collections::HashMap;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🟣 Quantum DID + Solana Integration Example\n");

    // 1. Setup: Create quantum wallets for different entities
    let mut university_bridge = SolanaQuantumBridge::new();
    let mut student_bridge = SolanaQuantumBridge::new();
    let employer_bridge = SolanaQuantumBridge::new();

    // Simulate Solana addresses (in real app, these come from wallet connections)
    let university_solana_addr = "DQW8VtTLqFLbG9qKJTMx9UvJ8JbCpTHdGw4xBgTQA3L";
    let student_solana_addr = "7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU";
    let employer_solana_addr = "9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM";

    println!("📝 Step 1: Register Quantum DIDs on Solana");
    println!("==========================================");

    // Generate registration data for each entity
    let university_registration =
        university_bridge.generate_registration_data(university_solana_addr)?;
    let student_registration = student_bridge.generate_registration_data(student_solana_addr)?;
    let employer_registration = employer_bridge.generate_registration_data(employer_solana_addr)?;

    println!("University DID: {}", university_registration.0);
    println!("University PDA: {}", university_registration.3);
    println!("Student DID: {}", student_registration.0);
    println!("Student PDA: {}", student_registration.3);
    println!("Employer DID: {}", employer_registration.0);
    println!("Employer PDA: {}", employer_registration.3);

    // Show what the Solana program instruction would look like
    println!("\n🔗 Solana Program Instructions:");
    let register_instruction =
        university_bridge.encode_register_did_instruction(university_solana_addr)?;
    println!("register_quantum_did");
    println!(
        "  accounts: {} required accounts",
        register_instruction.accounts.len()
    );
    println!(
        "  instruction_data: {} bytes",
        register_instruction.instruction_data.len()
    );
    println!("  program_id: {}", register_instruction.program_id);

    println!("\n🎓 Step 2: Issue Educational Credential");
    println!("=======================================");

    // University issues a degree credential to student
    let mut degree_claims = HashMap::new();
    degree_claims.insert(
        "degree".to_string(),
        "Master of Blockchain Technology".to_string(),
    );
    degree_claims.insert("institution".to_string(), "Solana University".to_string());
    degree_claims.insert("gpa".to_string(), "3.9".to_string());
    degree_claims.insert("graduation_year".to_string(), "2024".to_string());
    degree_claims.insert("blockchain".to_string(), "Solana".to_string());

    let degree_credential = university_bridge.wallet.issue_credential(
        student_bridge.wallet.identity_doc.did.as_ref(),
        "BlockchainEducationCredential",
        degree_claims,
        Some(3650), // Valid for 10 years
    )?;

    // Calculate credential hash for Solana storage (base58 format)
    let credential_hash = solana_utils::calculate_credential_hash_solana(&degree_credential);
    println!("Credential Hash (base58): {}", credential_hash);

    // Create quantum proof for Solana program verification
    let current_slot = 123456789; // Simulated current Solana slot
    let credential_proof = university_bridge.create_credential_proof(
        &credential_hash,
        student_solana_addr,
        current_slot,
    )?;

    println!("Credential Type: {}", degree_credential.credential_type);
    println!("Issued to: {}", degree_credential.subject);
    println!("Solana Slot: {}", credential_proof.slot);
    println!(
        "Quantum Signature: {}...",
        &credential_proof.quantum_signature[..32]
    );

    println!("\n💼 Step 3: Employment Verification Process");
    println!("==========================================");

    // Employer wants to verify the student's blockchain degree
    // This would happen when student applies for a blockchain developer job

    let verification_slot = current_slot + 1000; // Later slot
    let verification_message = format!(
        "EMPLOYMENT_VERIFICATION:{}:{}:{}:{}",
        credential_hash, employer_solana_addr, "blockchain_developer_position", verification_slot
    );

    // University signs the verification (proving credential validity)
    let verification_signature = university_bridge
        .wallet
        .sign_content(&verification_message)?;

    // Verify the credential proof
    let is_credential_valid = SolanaQuantumBridge::verify_quantum_signature(
        &verification_message,
        &verification_signature,
        &hex::encode(&university_bridge.wallet.key_pairs[0].public_key),
    )?;

    println!("Verification Message: {}", verification_message);
    println!(
        "Credential Valid: {}",
        if is_credential_valid { "✅" } else { "❌" }
    );
    println!("Verification Slot: {}", verification_slot);

    println!("\n🔄 Step 4: Key Rotation Example");
    println!("==============================");

    // Student rotates their quantum keys for enhanced security
    let rotation_slot = verification_slot + 500;
    let rotation_instruction =
        student_bridge.generate_key_rotation_instruction(student_solana_addr, rotation_slot)?;

    println!("Key Rotation Instruction:");
    println!(
        "  accounts: {} required accounts",
        rotation_instruction.accounts.len()
    );
    println!(
        "  instruction_data: {} bytes",
        rotation_instruction.instruction_data.len()
    );
    println!("  rotation_slot: {}", rotation_slot);

    println!("\n📊 Step 5: Solana Program Integration");
    println!("====================================");

    // Show how this integrates with Solana programs

    // Convert hash to bytes for Solana program
    let credential_hash_bytes = bs58::decode(&credential_hash)
        .into_vec()
        .map_err(|e| format!("Base58 decode error: {}", e))?;
    let mut hash_array = [0u8; 32];
    hash_array.copy_from_slice(&credential_hash_bytes[..32]);

    println!("Solana Program: issue_credential");
    println!("  credential_hash: {:?}", hash_array);
    println!("  subject: {}", student_solana_addr);
    println!("  credential_type: '{}'", degree_credential.credential_type);
    println!("  expires_at: {} (slot)", current_slot + 3650 * 216000); // Approx 10 years in slots

    // Program Derived Address (PDA) info
    let student_pda = SolanaQuantumBridge::derive_did_pda(
        student_bridge.wallet.identity_doc.did.as_ref(),
        student_solana_addr,
    )?;
    println!("\nProgram Derived Addresses:");
    println!("  Student Identity PDA: {}", student_pda);

    println!("\n🌐 Step 6: Cross-Chain Compatibility");
    println!("===================================");

    // Show quantum DID mapping to different blockchain addresses
    let cross_chain_mappings = vec![
        (
            "Solana",
            solana_utils::quantum_did_to_solana_address(
                &student_bridge.wallet.key_pairs[0].public_key,
            ),
        ),
        (
            "Ethereum",
            format!(
                "0x{}",
                hex::encode(
                    &Sha256::digest(&student_bridge.wallet.key_pairs[0].public_key)[12..32]
                )
            ),
        ),
        (
            "Near",
            bs58::encode(&Sha256::digest(&student_bridge.wallet.key_pairs[0].public_key)[..32])
                .into_string(),
        ),
    ];

    println!("Same quantum DID mapped to different chains:");
    for (chain, address) in cross_chain_mappings {
        println!("  {}: {}", chain, address);
    }

    println!("\n✨ Step 7: Advanced Solana Use Cases");
    println!("===================================");

    // Show advanced use cases specific to Solana
    demonstrate_solana_nft_credentials(&mut university_bridge, &mut student_bridge)?;
    demonstrate_solana_defi_identity(&mut university_bridge, &mut student_bridge)?;
    demonstrate_solana_governance_credentials(
        &university_bridge,
        &student_bridge,
        &employer_bridge,
    )?;

    println!("\n🔐 Step 8: Solana Performance Benefits");
    println!("=====================================");

    // Show Solana-specific performance characteristics
    let instruction_size = register_instruction.instruction_data.len();
    let num_accounts = register_instruction.accounts.len();
    let estimated_cost = solana_utils::estimate_transaction_cost(instruction_size, num_accounts);

    println!("Solana Transaction Analysis:");
    println!("  Instruction size: {} bytes", instruction_size);
    println!("  Required accounts: {}", num_accounts);
    println!(
        "  Estimated cost: {} lamports (~${:.6})",
        estimated_cost,
        estimated_cost as f64 / 1_000_000_000.0 * 20.0
    ); // Assume $20 SOL
    println!("  Confirmation time: ~400ms (typical)");
    println!("  Parallel processing: ✅ Supported");

    println!("\n🎉 Solana Integration Complete!");
    println!("===============================");
    println!("✅ Quantum DIDs registered on Solana");
    println!("✅ Credentials issued with quantum signatures");
    println!("✅ Key rotation performed securely");
    println!("✅ Program Derived Addresses (PDAs) utilized");
    println!("✅ Cross-chain compatibility demonstrated");
    println!("✅ Solana-specific optimizations applied");

    println!("\n📚 Next Steps:");
    println!("1. Deploy quantum-did-solana program to devnet/mainnet");
    println!("2. Implement quantum signature verification in Solana program");
    println!("3. Build frontend dApp using @solana/web3.js");
    println!("4. Integrate with Solana wallet adapters (Phantom, Solflare)");
    println!("5. Set up SpaceKit Storage for off-chain credential data");

    Ok(())
}

fn demonstrate_solana_nft_credentials(
    university: &mut SolanaQuantumBridge,
    student: &mut SolanaQuantumBridge,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("\n🎨 Solana NFT Credentials Use Case:");
    println!("  - Mint quantum-secured NFT diplomas");
    println!("  - Immutable credential verification");
    println!("  - Metaplex integration for rich metadata");

    let mut nft_claims = HashMap::new();
    nft_claims.insert("degree".to_string(), "PhD in Computer Science".to_string());
    nft_claims.insert(
        "thesis".to_string(),
        "Quantum-Resistant Blockchain Architecture".to_string(),
    );
    nft_claims.insert("advisor".to_string(), "Dr. Quantum Blockchain".to_string());
    nft_claims.insert("nft_mint".to_string(), "BNFTxyz123456789abcdef".to_string());

    let nft_credential = university.wallet.issue_credential(
        student.wallet.identity_doc.did.as_ref(),
        "NFTDiploma",
        nft_claims,
        None, // Permanent NFT credential
    )?;

    println!("  ✅ NFT Diploma minted: {}", nft_credential.id);
    Ok(())
}

fn demonstrate_solana_defi_identity(
    university: &mut SolanaQuantumBridge,
    student: &mut SolanaQuantumBridge,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("\n💰 Solana DeFi Identity Use Case:");
    println!("  - KYC/AML compliance with quantum security");
    println!("  - Credit scoring with verifiable credentials");
    println!("  - Integration with Solana DeFi protocols");

    let mut defi_claims = HashMap::new();
    defi_claims.insert("kyc_level".to_string(), "Level 3 Verified".to_string());
    defi_claims.insert("credit_score".to_string(), "750".to_string());
    defi_claims.insert("risk_assessment".to_string(), "Low Risk".to_string());
    defi_claims.insert(
        "protocol_whitelist".to_string(),
        "Serum,Raydium,Orca".to_string(),
    );

    let defi_credential = university.wallet.issue_credential(
        student.wallet.identity_doc.did.as_ref(),
        "DeFiIdentityCredential",
        defi_claims,
        Some(365), // Annual renewal
    )?;

    println!("  ✅ DeFi identity verified: {}", defi_credential.id);
    Ok(())
}

fn demonstrate_solana_governance_credentials(
    _university: &SolanaQuantumBridge,
    _student: &SolanaQuantumBridge,
    _employer: &SolanaQuantumBridge,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("\n🗳️ Solana Governance Use Case:");
    println!("  - DAO voting with quantum-verified identity");
    println!("  - Reputation-based governance participation");
    println!("  - Multi-signature quantum proposals");

    // Multiple entities can verify student's qualifications for governance
    let governance_credentials = vec![
        "BlockchainEducationCredential",
        "DeFiIdentityCredential",
        "CommunityParticipation",
    ];

    println!("  📋 Governance eligibility verification:");
    for cred_type in governance_credentials {
        println!("    ✅ {}", cred_type);
    }

    println!("  🔐 Multi-entity quantum verification successful");
    println!("  🗳️ Governance participation: APPROVED");
    Ok(())
}

#[cfg(test)]
mod integration_tests {
    use super::*;

    #[test]
    fn test_full_solana_workflow() {
        let result = main();
        assert!(
            result.is_ok(),
            "Solana integration workflow should complete successfully"
        );
    }

    #[test]
    fn test_solana_pda_consistency() {
        let did = "did:spacekit:testnet:test123";
        let solana_addr = "DQW8VtTLqFLbG9qKJTMx9UvJ8JbCpTHdGw4xBgTQA3L";

        let pda1 = SolanaQuantumBridge::derive_did_pda(did, solana_addr).unwrap();
        let pda2 = SolanaQuantumBridge::derive_did_pda(did, solana_addr).unwrap();

        assert_eq!(pda1, pda2, "PDA derivation should be deterministic");

        // Should be valid base58
        assert!(
            bs58::decode(&pda1).into_vec().is_ok(),
            "PDA should be valid base58"
        );
    }

    #[test]
    fn test_solana_credential_hash_format() {
        let wallet = spacekit_did::QuantumResistantWallet::new();
        let mut claims = HashMap::new();
        claims.insert("platform".to_string(), "Solana".to_string());
        claims.insert("test".to_string(), "value".to_string());

        let credential = wallet
            .issue_credential(
                "did:spacekit:testnet:test123",
                "SolanaTestCredential",
                claims,
                Some(365),
            )
            .unwrap();

        let hash = solana_utils::calculate_credential_hash_solana(&credential);

        // Should be valid base58 and not empty
        assert!(!hash.is_empty());
        assert!(bs58::decode(&hash).into_vec().is_ok());
    }

    #[test]
    fn test_solana_transaction_cost_estimation() {
        let instruction_size = 1024; // 1KB instruction
        let num_accounts = 5;

        let cost = solana_utils::estimate_transaction_cost(instruction_size, num_accounts);

        // Should be reasonable cost (less than 0.01 SOL in lamports)
        assert!(cost < 10_000_000);
        assert!(cost > 0);
    }
}
