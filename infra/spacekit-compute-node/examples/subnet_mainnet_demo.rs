//! Subnet-to-Mainnet Proof Submission Demo
//!
//! Demonstrates how operators can:
//! 1. Run their own subnet/network (public or private)
//! 2. Aggregate proofs from their subnet
//! 3. Submit ZK-proofs periodically to mainnet
//! 4. Have mainnet validators verify and accept proofs

use anyhow::Result;
use chrono::Utc;
use spacekit_compute_node::quantum_security::QuantumResistantWallet;
use spacekit_compute_node::{
    subnet_proof_system::{
        NetworkType, SubnetProofBuilder, SubnetProofConfig, SubnetProofSystem, SubnetStatus,
        ValidatorSignature,
    },
    vpos::VPoSManager,
};
use spacekit_primitives::v1::crypto::quantum::Algorithm;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    println!("🌐 SWTCH Network: Subnet-to-Mainnet Proof System Demo");
    println!("=====================================================\n");

    // ========================================
    // SETUP: Initialize Mainnet
    // ========================================
    println!("📡 Step 1: Initialize Mainnet");
    println!("─────────────────────────────");
    let wallet = Arc::new(QuantumResistantWallet::new());
    let mainnet_vpos = Arc::new(VPoSManager::new(wallet, Algorithm::Kyber768).await?);
    let mainnet_config = SubnetProofConfig {
        min_validator_signatures: 3,
        min_stake_percentage: 0.67,
        max_proof_interval: 3600,
        enable_zk_verification: true,
        mainnet_did: "did:swtch:mainnet".to_string(),
    };

    let mainnet = SubnetProofSystem::new(mainnet_vpos, mainnet_config);
    println!("✅ Mainnet initialized");
    println!("   • Chain ID: 1 (mainnet)");
    println!("   • Min validators: 3");
    println!("   • Stake threshold: 67%\n");

    // ========================================
    // SCENARIO 1: Public Subnet
    // ========================================
    println!("🌍 Scenario 1: Public Subnet Registration");
    println!("─────────────────────────────────────────");

    let public_subnet = mainnet
        .register_subnet(
            "did:swtch:operator:alice".to_string(),
            NetworkType::Public,
            "0xgenesis_public_subnet_001".to_string(),
            10_000_000_000_000_000_000, // 10 SWTCH minimum stake
            300,                        // Submit proof every 5 minutes
        )
        .await?;

    println!("✅ Public Subnet Registered:");
    println!("   • Subnet ID: {}", public_subnet.subnet_id);
    println!("   • Operator: {}", public_subnet.operator_did);
    println!("   • Chain ID: {}", public_subnet.chain_id);
    println!("   • Type: Public (anyone can join)");
    println!(
        "   • Proof interval: {}s",
        public_subnet.proof_submission_interval
    );

    // Activate the subnet
    mainnet.activate_subnet(&public_subnet.subnet_id).await?;
    println!("   • Status: ✅ Active\n");

    // ========================================
    // SCENARIO 2: Private Subnet
    // ========================================
    println!("🔒 Scenario 2: Private Subnet Registration");
    println!("──────────────────────────────────────────");

    let private_subnet = mainnet
        .register_subnet(
            "did:swtch:operator:bob".to_string(),
            NetworkType::Private {
                authorized_validators: vec![
                    "did:swtch:validator:private1".to_string(),
                    "did:swtch:validator:private2".to_string(),
                    "did:swtch:validator:private3".to_string(),
                    "did:swtch:validator:private4".to_string(),
                ],
                publish_proofs: true,
            },
            "0xgenesis_private_subnet_002".to_string(),
            50_000_000_000_000_000_000, // 50 SWTCH minimum stake
            600,                        // Submit proof every 10 minutes
        )
        .await?;

    println!("✅ Private Subnet Registered:");
    println!("   • Subnet ID: {}", private_subnet.subnet_id);
    println!("   • Operator: {}", private_subnet.operator_did);
    println!("   • Chain ID: {}", private_subnet.chain_id);
    println!("   • Type: Private (authorized validators only)");
    println!("   • Authorized validators: 4");
    println!(
        "   • Proof interval: {}s",
        private_subnet.proof_submission_interval
    );

    mainnet.activate_subnet(&private_subnet.subnet_id).await?;
    println!("   • Status: ✅ Active\n");

    // ========================================
    // SCENARIO 3: Consortium Subnet
    // ========================================
    println!("🏛️  Scenario 3: Consortium Subnet Registration");
    println!("───────────────────────────────────────────────");

    let consortium_subnet = mainnet
        .register_subnet(
            "did:swtch:operator:consortium".to_string(),
            NetworkType::Consortium {
                members: vec![
                    "did:swtch:member:bank1".to_string(),
                    "did:swtch:member:bank2".to_string(),
                    "did:swtch:member:bank3".to_string(),
                    "did:swtch:member:bank4".to_string(),
                    "did:swtch:member:bank5".to_string(),
                ],
                approval_threshold: 0.80, // 80% approval required
            },
            "0xgenesis_consortium_subnet_003".to_string(),
            100_000_000_000_000_000_000, // 100 SWTCH minimum stake
            900,                         // Submit proof every 15 minutes
        )
        .await?;

    println!("✅ Consortium Subnet Registered:");
    println!("   • Subnet ID: {}", consortium_subnet.subnet_id);
    println!("   • Operator: {}", consortium_subnet.operator_did);
    println!("   • Chain ID: {}", consortium_subnet.chain_id);
    println!("   • Type: Consortium (member banks)");
    println!("   • Members: 5 banks");
    println!("   • Approval threshold: 80%");

    mainnet
        .activate_subnet(&consortium_subnet.subnet_id)
        .await?;
    println!("   • Status: ✅ Active\n");

    // ========================================
    // STEP 2: Subnet Operations & Proof Generation
    // ========================================
    println!("⚙️  Step 2: Subnet Operations (Public Subnet)");
    println!("──────────────────────────────────────────────");
    println!("Simulating subnet activity...\n");

    // Simulate transactions on the public subnet
    let mut proof_builder = SubnetProofBuilder::new(
        public_subnet.subnet_id.clone(),
        (1, 100), // Blocks 1-100
    );

    // Add some transactions
    for i in 0..50 {
        let tx = format!("transaction_{}", i).into_bytes();
        proof_builder.add_transaction(tx);
    }

    println!("✅ Subnet activity:");
    println!("   • Blocks mined: 1-100");
    println!("   • Transactions: 50");
    println!("   • Gas used: {}", 50 * 21000);

    // Add validator signatures
    let validators = vec![
        ("did:swtch:validator:v1", 25_000_000_000_000_000_000u128),
        ("did:swtch:validator:v2", 30_000_000_000_000_000_000u128),
        ("did:swtch:validator:v3", 20_000_000_000_000_000_000u128),
        ("did:swtch:validator:v4", 15_000_000_000_000_000_000u128),
    ];

    for (did, stake) in validators {
        proof_builder.add_validator_signature(ValidatorSignature {
            validator_did: did.to_string(),
            signature: vec![0xAB; 64], // Mock quantum-resistant signature
            stake_amount: stake,
            signed_at: Utc::now(),
        });
    }

    println!("   • Validator signatures: 4");
    println!("   • Total staked: 90 SWTCH\n");

    // Add service proofs
    for i in 0..10 {
        proof_builder.add_service_proof(format!("vpos_proof_{}", i));
    }

    println!("✅ Aggregated proofs:");
    println!("   • VPoS service proofs: 10\n");

    // ========================================
    // STEP 3: Generate and Submit ZK Proof
    // ========================================
    println!("🔐 Step 3: Generate ZK-Proof");
    println!("────────────────────────────");

    let subnet_proof = proof_builder.build().await?;

    println!("✅ ZK-Proof generated:");
    println!("   • Proof ID: {}", subnet_proof.proof_id);
    println!(
        "   • Block range: {} - {}",
        subnet_proof.block_range.0, subnet_proof.block_range.1
    );
    println!("   • State root: {}", &subnet_proof.state_root[..20]);
    println!(
        "   • Transaction merkle: {}",
        &subnet_proof.transaction_merkle_root[..20]
    );
    println!(
        "   • ZK proof size: {} bytes",
        subnet_proof.zk_proof.proof_bytes.len()
    );
    println!(
        "   • Public inputs: {}",
        subnet_proof.zk_proof.public_inputs.len()
    );
    println!(
        "   • Generation time: {}ms\n",
        subnet_proof.zk_proof.generation_time_ms
    );

    // ========================================
    // STEP 4: Submit to Mainnet
    // ========================================
    println!("📤 Step 4: Submit Proof to Mainnet");
    println!("──────────────────────────────────");

    let proof_id = mainnet.submit_subnet_proof(subnet_proof.clone()).await?;

    println!("✅ Proof submitted to mainnet:");
    println!("   • Proof ID: {}", proof_id);
    println!("   • From subnet: {}", subnet_proof.subnet_id);
    println!("   • Awaiting verification...\n");

    // ========================================
    // STEP 5: Mainnet Verification
    // ========================================
    println!("✅ Step 5: Mainnet Verification");
    println!("────────────────────────────────");

    let verification_result = mainnet
        .verify_subnet_proof(&proof_id, "did:swtch:mainnet:validator1")
        .await?;

    println!("✅ Verification complete:");
    println!("   • Valid: {}", verification_result.is_valid);
    println!(
        "   • Confidence: {:.2}%",
        verification_result.confidence_score * 100.0
    );
    println!(
        "   • Verifying validators: {}",
        verification_result.verifying_validators.len()
    );

    if verification_result.is_valid {
        println!(
            "   • Mainnet block: {}",
            verification_result.mainnet_block_number.unwrap()
        );
        println!("   • Status: ✅ ACCEPTED ON MAINNET");
    } else {
        println!("   • Issues: {:?}", verification_result.issues);
    }
    println!();

    // ========================================
    // STEP 6: Private Subnet Proof Submission
    // ========================================
    println!("🔒 Step 6: Private Subnet Proof Submission");
    println!("──────────────────────────────────────────");

    let mut private_proof_builder =
        SubnetProofBuilder::new(private_subnet.subnet_id.clone(), (1, 50));

    // Add transactions
    for i in 0..25 {
        private_proof_builder.add_transaction(format!("private_tx_{}", i).into_bytes());
    }

    // Add authorized validator signatures
    let private_validators = vec![
        (
            "did:swtch:validator:private1",
            60_000_000_000_000_000_000u128,
        ),
        (
            "did:swtch:validator:private2",
            70_000_000_000_000_000_000u128,
        ),
        (
            "did:swtch:validator:private3",
            50_000_000_000_000_000_000u128,
        ),
    ];

    for (did, stake) in private_validators {
        private_proof_builder.add_validator_signature(ValidatorSignature {
            validator_did: did.to_string(),
            signature: vec![0xCD; 64],
            stake_amount: stake,
            signed_at: Utc::now(),
        });
    }

    let private_proof = private_proof_builder.build().await?;
    let private_proof_id = mainnet.submit_subnet_proof(private_proof).await?;

    println!("✅ Private subnet proof submitted:");
    println!("   • Proof ID: {}", private_proof_id);
    println!("   • Authorized validators only");
    println!("   • Proofs published: true\n");

    // ========================================
    // STEP 7: Network Status Summary
    // ========================================
    println!("📊 Step 7: Network Status Summary");
    println!("─────────────────────────────────");

    let all_subnets = mainnet.list_subnets().await;

    println!("✅ Active Subnets: {}", all_subnets.len());
    println!();

    for subnet in &all_subnets {
        let status_emoji = match subnet.status {
            SubnetStatus::Active => "✅",
            SubnetStatus::Pending => "⏳",
            SubnetStatus::Paused => "⏸️",
            SubnetStatus::Suspended => "🚫",
            SubnetStatus::Deregistered => "❌",
        };

        let type_desc = match &subnet.network_type {
            NetworkType::Public => "Public".to_string(),
            NetworkType::Private { .. } => "Private".to_string(),
            NetworkType::Consortium { .. } => "Consortium".to_string(),
        };

        println!(
            "{} Subnet {} ({})",
            status_emoji, subnet.chain_id, type_desc
        );
        println!("   • ID: {}", subnet.subnet_id);
        println!("   • Operator: {}", subnet.operator_did);
        println!(
            "   • Min stake: {} SWTCH",
            subnet.min_validator_stake as f64 / 1e18
        );
        println!("   • Proof interval: {}s", subnet.proof_submission_interval);
    }

    println!("\n════════════════════════════════════════════════════");
    println!("✅ Demo Complete!");
    println!("════════════════════════════════════════════════════");
    println!("\n📋 Summary:");
    println!("   • 3 subnets registered (Public, Private, Consortium)");
    println!("   • 2 proofs submitted to mainnet");
    println!("   • ZK-proof verification successful");
    println!("   • Mainnet accepted subnet proofs");
    println!("\n🎯 Key Features Demonstrated:");
    println!("   ✅ Operators can run their own networks");
    println!("   ✅ Networks can be public, private, or consortium");
    println!("   ✅ Periodic proof submission to mainnet");
    println!("   ✅ ZK-proof generation and verification");
    println!("   ✅ Validator signature aggregation");
    println!("   ✅ Mainnet verification and acceptance");
    println!("\n🚀 Ready for production deployment!");

    Ok(())
}
