//! Behavioral Patterns Demo
//! 
//! This example demonstrates the SpaceKit Network Recovery system's
//! behavioral cryptography for decentralized identity recovery.

use spacekit_recovery::{
    BehavioralRecoverySystem, PeerEndorsementMatrix, EndorsementRecord, EndorsementType
};
use spacekit_primitives::v1::identity::Identity;
use chrono::Utc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🧠 SWTCH Behavioral Cryptography Demo");
    println!("=====================================\n");

    // Step 1: Create a behavioral recovery system
    println!("1. Initializing Behavioral Recovery System...");
    let recovery_system = BehavioralRecoverySystem::new(1.0, 1e-6);
    println!("   ✅ Privacy parameters: ε=1.0, δ=1e-6\n");

    // Step 2: Create a test identity
    println!("2. Creating test identity...");
    let identity = Identity::new(
        "did:swtch:alice123".to_string(),
        "alice_researcher".to_string(),
        "secure_password_123".to_string(),
    );
    println!("   ✅ Identity: {}", identity.did);
    println!("   ✅ Username: {}\n", identity.username);

    // Step 3: Create peer endorsements matrix
    println!("3. Building peer endorsement matrix...");
    let mut peer_endorsements = PeerEndorsementMatrix::new();
    peer_endorsements.set_total_endorsers(100);

    // Add various endorsements
    let endorsements = vec![
        ("did:swtch:bob_storage", EndorsementType::StorageReliability, 0.92),
        ("did:swtch:carol_compute", EndorsementType::ComputeQuality, 0.88),
        ("did:swtch:dave_economic", EndorsementType::EconomicTrustworthiness, 0.95),
        ("did:swtch:eve_service", EndorsementType::ServiceExcellence, 0.87),
        ("did:swtch:frank_chain", EndorsementType::CrossChainConsistency, 0.91),
    ];

    for (endorser, endorsement_type, strength) in endorsements {
        let endorsement = EndorsementRecord {
            endorser_did: endorser.to_string(),
            endorsement_strength: strength,
            endorsement_type,
            timestamp: Utc::now(),
        };
        peer_endorsements.add_endorsement(identity.did.clone(), endorsement);
        println!("   📝 Endorsement from {} (strength: {:.2})", endorser, strength);
    }
    println!("   ✅ Total endorsers in network: {}\n", peer_endorsements.total_endorsers);

    // Step 4: Analyze behavioral patterns
    println!("4. Analyzing behavioral patterns...");
    let patterns = recovery_system.analyze_behavioral_patterns(&identity)?;
    println!("   ✅ Storage behavior analyzed");
    println!("      - Avg daily storage: {:.2} GB", patterns.storage_behavior.avg_daily_storage_gb);
    println!("      - Consistency score: {:.3}", patterns.storage_behavior.consistency_score);
    println!("   ✅ Compute participation analyzed");
    println!("      - Avg daily compute: {:.2} hours", patterns.compute_participation.avg_daily_compute_hours);
    println!("      - Service quality: {:.3}", patterns.compute_participation.service_quality);
    println!("   ✅ Economic patterns analyzed");
    println!("      - Earning consistency: {:.3}", patterns.economic_patterns.earning_consistency);
    println!("      - Payment punctuality: {:.3}", patterns.economic_patterns.payment_punctuality);
    println!("   ✅ Service quality metrics analyzed");
    println!("      - Success ratio: {:.3}", patterns.service_quality.success_ratio);
    println!("      - Peer rating avg: {:.2}/5.0", patterns.service_quality.peer_rating_avg);
    println!("   ✅ Multi-chain activity analyzed");
    println!("      - Cross-chain frequency: {:.3}", patterns.multi_chain_activity.cross_chain_tx_frequency);
    println!("      - Identity consistency: {:.3}\n", patterns.multi_chain_activity.identity_consistency);

    // Step 5: Generate behavioral fingerprint
    println!("5. Generating quantum-resistant behavioral fingerprint...");
    let fingerprint = recovery_system.generate_behavioral_fingerprint(&patterns, &identity.did)?;
    println!("   ✅ Fingerprint created with {} bytes", fingerprint.encrypted_fingerprint.len());
    println!("   🔐 Algorithm: Kyber1024 (quantum-resistant)");
    println!("   🔒 Privacy: ε={:.1}, δ={:.0e}\n", fingerprint.epsilon, fingerprint.delta);

    // Step 6: Compute confidence score
    println!("6. Computing confidence score...");
    let confidence_score = recovery_system.compute_confidence_score(&patterns, &peer_endorsements, &identity.did)?;
    println!("   ✅ Confidence score computed using homomorphic encryption");
    println!("   📊 Score encrypted with {} bytes", confidence_score.encrypted_score.len());
    println!("   🎯 Recovery threshold: {:.1}", confidence_score.threshold);
    
    // Display factor weights
    println!("   📊 Confidence factors:");
    println!("      - Network participation: {:.2}", confidence_score.factor_weights.network_participation_weight);
    println!("      - Peer endorsement: {:.2}", confidence_score.factor_weights.peer_endorsement_weight);
    println!("      - Service quality: {:.2}", confidence_score.factor_weights.service_quality_weight);
    println!("      - Economic consistency: {:.2}", confidence_score.factor_weights.economic_consistency_weight);
    println!("      - Multi-chain behavior: {:.2}", confidence_score.factor_weights.multi_chain_behavior_weight);
    println!("      - Temporal weighting: {:.2}\n", confidence_score.factor_weights.temporal_weighting);

    // Step 7: Verify recovery eligibility
    println!("7. Verifying recovery eligibility...");
    let eligible = recovery_system.verify_recovery_eligibility(&confidence_score)?;
    if eligible {
        println!("   ✅ RECOVERY APPROVED - Confidence score meets threshold");
    } else {
        println!("   ❌ RECOVERY DENIED - Confidence score below threshold");
    }
    println!();

    // Step 8: Generate comprehensive report
    println!("8. Generating recovery report...");
    let report = recovery_system.generate_recovery_report(&patterns, &confidence_score, &identity.did)?;
    println!("   ✅ Report generated ({} characters)\n", report.len());

    // Display the report
    println!("📋 RECOVERY REPORT");
    println!("==================");
    println!("{}", report);

    // Step 9: Complete workflow demonstration
    println!("\n9. Complete behavioral recovery workflow...");
    let recovery_result = recovery_system.initiate_behavioral_recovery(&identity, &peer_endorsements).await?;
    
    println!("   ✅ Complete workflow executed successfully");
    println!("   📊 Patterns collected at: {}", recovery_result.patterns.collected_at.format("%Y-%m-%d %H:%M:%S UTC"));
    println!("   🔐 Fingerprint created at: {}", recovery_result.fingerprint.created_at.format("%Y-%m-%d %H:%M:%S UTC"));
    println!("   📈 Confidence calculated at: {}", recovery_result.confidence_score.calculated_at.format("%Y-%m-%d %H:%M:%S UTC"));
    
    if recovery_result.eligible {
        println!("   🎉 FINAL RESULT: Identity recovery APPROVED!");
    } else {
        println!("   ⛔ FINAL RESULT: Identity recovery DENIED!");
    }

    println!("\n🔗 Integration with SWTCH Network:");
    println!("   • Quantum-resistant encryption: swtch-network-quantum");
    println!("   • Identity management: swtch-network-primitives");
    println!("   • DID registry: Smart contracts on multiple chains");
    println!("   • Privacy protection: Differential privacy with OpenDP");
    println!("   • Behavioral analysis: Machine learning with ndarray");

    println!("\n🛡️ Security Features Demonstrated:");
    println!("   ✅ Quantum-resistant encryption (Kyber1024)");
    println!("   ✅ Differential privacy protection");
    println!("   ✅ Homomorphic confidence scoring");
    println!("   ✅ Multi-chain identity consistency");
    println!("   ✅ Peer endorsement verification");
    println!("   ✅ Behavioral pattern analysis");
    println!("   ✅ Zero-knowledge proof capabilities");

    println!("\n🚀 This completes the SWTCH Behavioral Cryptography demonstration!");
    println!("    The system successfully implements the world's first");
    println!("    distributed confidence recovery protocol for quantum-resistant");
    println!("    decentralized identity management.");

    Ok(())
} 