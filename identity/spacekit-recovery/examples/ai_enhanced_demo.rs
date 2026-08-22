// SpaceKit Network Recovery: AI-Enhanced Behavioral Analysis Demo
// Demonstrating the integration of AI module with behavioral patterns

use spacekit_recovery::{
    BehavioralRecoverySystem, RecoveryResult,
    BehavioralPatterns, PeerEndorsementMatrix, EndorsementRecord, EndorsementType,
    ai::{BehavioralAI, AIAnalysisResult},
};
use spacekit_primitives::v1::identity::Identity;
use chrono::Utc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🤖 SWTCH AI-Enhanced Behavioral Recovery Demo");
    println!("================================================");

    // Step 1: Initialize the behavioral recovery system
    println!("\n1️⃣ Initializing Behavioral Recovery System...");
    let recovery_system = BehavioralRecoverySystem::new(1.0, 1e-6);
    
    // Step 2: Initialize the AI analysis system
    println!("2️⃣ Initializing AI Analysis System...");
    let mut ai_system = BehavioralAI::new();
    
    // Optional: Initialize with Cortex node (simulated)
    // let mut ai_system = BehavioralAI::with_cortex("https://cortex.swtch.network".to_string())?;
    
    // Step 3: Create a test identity
    println!("3️⃣ Creating test identity...");
    let identity = Identity::new(
        "did:swtch:ai_enhanced_test".to_string(),
        "ai_test_user".to_string(),
        "secure_password_123".to_string(),
    );
    
    // Step 4: Build peer endorsement matrix
    println!("4️⃣ Building peer endorsement matrix...");
    let mut peer_endorsements = PeerEndorsementMatrix::new();
    peer_endorsements.set_total_endorsers(75);
    
    // Add multiple endorsements across different categories - using correct enum variants
    let endorsements = vec![
        (EndorsementType::StorageReliability, 0.95),
        (EndorsementType::ComputeQuality, 0.88),
        (EndorsementType::ServiceExcellence, 0.92),
        (EndorsementType::EconomicTrustworthiness, 0.90),
        (EndorsementType::CrossChainConsistency, 0.87),
    ];
    
    for (endorsement_type, strength) in endorsements {
        let endorsement = EndorsementRecord {
            endorser_did: format!("did:swtch:endorser_{}", rand::random::<u32>()),
            endorsement_strength: strength,
            endorsement_type,
            timestamp: Utc::now(),
        };
        peer_endorsements.add_endorsement(identity.did.clone(), endorsement);
    }
    
    // Step 5: Perform behavioral recovery analysis
    println!("5️⃣ Performing behavioral recovery analysis...");
    let recovery_result: RecoveryResult = recovery_system
        .initiate_behavioral_recovery(&identity, &peer_endorsements)
        .await?;
    
    // Get the actual confidence score by decrypting it
    let confidence_scorer = recovery_system.get_confidence_scorer();
    let overall_confidence = confidence_scorer.decrypt_confidence_score(&recovery_result.confidence_score.encrypted_score)?;
    
    println!("   ✅ Recovery analysis complete");
    println!("   📊 Confidence Score: {:.3}", overall_confidence);
    println!("   ⚖️  Recovery Eligible: {}", recovery_result.eligible);
    
    // Step 6: AI-Enhanced Analysis
    println!("\n6️⃣ Performing AI-Enhanced Analysis...");
    
    // AI analysis of behavioral patterns
    let ai_analysis: AIAnalysisResult = ai_system
        .analyze_behavioral_patterns(
            &recovery_result.patterns,
            &recovery_result.fingerprint,
            &recovery_result.confidence_score,
            &identity.did,
        )
        .await?;
    
    println!("   🧠 AI Analysis Complete");
    println!("   🎯 AI Confidence: {:.3}", ai_analysis.ai_confidence);
    println!("   🔍 Anomaly Score: {:.3}", ai_analysis.anomaly_report.anomaly_score);
    println!("   ⚠️  Detected Anomalies: {}", ai_analysis.anomaly_report.detected_anomalies.len());
    println!("   🎨 Recognized Patterns: {}", ai_analysis.recognition_result.recognized_patterns.len());
    println!("   🛡️  Threat Level: {:?}", ai_analysis.threat_assessment.threat_level);
    
    // Step 7: Display AI Recommendations
    println!("\n7️⃣ AI Recommendations:");
    for (i, recommendation) in ai_analysis.recommendations.iter().enumerate() {
        println!("   {}. {:?} (Confidence: {:.2}, Priority: {:?})", 
                 i + 1,
                 recommendation.recommendation_type,
                 recommendation.confidence,
                 recommendation.priority);
        println!("      📝 {}", recommendation.description);
    }
    
    // Step 8: Real-time monitoring simulation
    println!("\n8️⃣ Simulating Real-time Behavioral Monitoring...");
    let current_patterns = &recovery_result.patterns;
    let baseline_patterns = &recovery_result.patterns; // Same for demo
    
    let monitoring_recommendations = ai_system
        .monitor_behavioral_changes(current_patterns, baseline_patterns, &identity.did)
        .await?;
    
    println!("   📡 Monitoring Active");
    println!("   📈 Behavioral Deviation: Minimal (demo)");
    println!("   🔔 Monitoring Alerts: {}", monitoring_recommendations.len());
    
    // Step 9: System Status Check
    println!("\n9️⃣ AI System Status:");
    let system_status = ai_system.get_system_status();
    println!("   🔧 Anomaly Detector Ready: {}", system_status.anomaly_detector_ready);
    println!("   🎯 Pattern Recognizer Ready: {}", system_status.pattern_recognizer_ready);
    println!("   🛡️  Attack Detector Ready: {}", system_status.attack_detector_ready);
    println!("   🌐 Cortex Connected: {}", system_status.cortex_connected);
    println!("   📚 Learning Enabled: {}", system_status.learning_enabled);
    
    // Step 10: Final Assessment
    println!("\n🔟 Final AI-Enhanced Assessment:");
    
    let combined_confidence = (overall_confidence + ai_analysis.ai_confidence) / 2.0;
    let security_score = 1.0 - ai_analysis.anomaly_report.anomaly_score;
    let final_recommendation = if combined_confidence > 0.7 && security_score > 0.8 {
        "APPROVE RECOVERY"
    } else if combined_confidence > 0.5 {
        "REQUIRE ADDITIONAL VERIFICATION"
    } else {
        "DENY RECOVERY"
    };
    
    println!("   🎯 Combined Confidence: {:.3}", combined_confidence);
    println!("   🔒 Security Score: {:.3}", security_score);
    println!("   ⚖️  Final Recommendation: {}", final_recommendation);
    
    println!("\n✨ AI-Enhanced Behavioral Recovery Demo Complete!");
    println!("   This demo showcased the world's first production-ready");
    println!("   behavioral cryptography system with AI enhancement for");
    println!("   quantum-resistant decentralized identity recovery.");
    
    Ok(())
} 