//! Simple SpaceKit Integration Demo
//! 
//! A streamlined example showing how the behavioral simulation integrates
//! with the new SWTCH primitives and recovery system concepts.

use anyhow::Result;
use serde_json;
use std::collections::HashMap;
use tokio::time::{sleep, Duration};
use tracing::{info, warn};
use alloy_primitives::Address;

// Import SWTCH primitives
use spacekit_primitives::v1::behavioral_types::{
    UserArchetype, ServiceType, InteractionStyle, PersonalityTraits,
    BehavioralFingerprint, ArchetypeBehavioralExpectations
};
use spacekit_primitives::v1::reputation::{
    ReputationScore, ReputationProfile, BehavioralReputationScore, ServiceParticipation
};

// Import local simulation types
use spacekit_behavioral_simulation::{
    UserArchetype as SimArchetype, ServiceType as SimServiceType,
    PersonalityProfile, BehavioralPattern, ActivityData,
};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    info!("🚀 SpaceKit Simple Integration Demo");
    // info!("=", * 50);
    
    // Demo 1: Primitives Showcase
    demo_primitives_showcase().await?;
    
    // Demo 2: Archetype Mapping
    demo_archetype_mapping().await?;
    
    // Demo 3: Behavioral Pattern Integration
    demo_behavioral_pattern_integration().await?;
    
    // Demo 4: Reputation System Integration
    demo_reputation_integration().await?;
    
    // Demo 5: Recovery Simulation Preview
    demo_recovery_simulation_preview().await?;
    
    info!("✅ Simple Integration Demo Completed!");
    
    Ok(())
}

/// Demonstrate the SpaceKit primitives
async fn demo_primitives_showcase() -> Result<()> {
    info!("📋 Demo 1: SpaceKit Primitives Showcase");
    println!();
    
    // Show all archetypes with their characteristics
    let archetypes = vec![
        UserArchetype::BaseUser,
        UserArchetype::Validator,
        UserArchetype::Developer,
        UserArchetype::Researcher,
        UserArchetype::Investor,
        UserArchetype::Regulator,
        UserArchetype::Other,
    ];
    
    info!("👥 User Archetypes in SpaceKit Network:");
    for archetype in &archetypes {
        let expectations = archetype.default_expectations();
        info!("  {} ({}%)", 
            archetype.description(),
            (archetype.population_percentage() * 100.0) as u32
        );
        info!("    Activity Range: {:.1}-{:.1}", 
            expectations.expected_activity_range.0,
            expectations.expected_activity_range.1
        );
        info!("    Primary Services: {:?}", expectations.expected_services);
        info!("    Interaction Style: {:?}", expectations.expected_interaction_style);
    }
    
    println!();
    
    // Show service types
    let services = vec![
        ServiceType::Compute,
        ServiceType::Storage,
        ServiceType::Messaging,
        ServiceType::AI,
        ServiceType::Identity,
        ServiceType::CrossChain,
        ServiceType::Encryption,
    ];
    
    info!("🔧 Available Services:");
    for service in services {
        info!("  • {:?}", service);
    }
    
    println!();
    
    // Show interaction styles
    let styles = vec![
        InteractionStyle::Collaborative,
        InteractionStyle::Independent,
        InteractionStyle::Supportive,
        InteractionStyle::Competitive,
        InteractionStyle::Suspicious,
    ];
    
    info!("🤝 Interaction Styles:");
    for style in styles {
        info!("  • {:?}", style);
    }
    
    println!();
    Ok(())
}

/// Demonstrate archetype mapping between simulation and primitives
async fn demo_archetype_mapping() -> Result<()> {
    info!("🔄 Demo 2: Archetype Mapping");
    println!();
    
    // Create mapping between simulation and primitives archetypes
    let mapping_pairs = vec![
        (SimArchetype::BaseUser, UserArchetype::BaseUser),
        (SimArchetype::Validator, UserArchetype::Validator),
        (SimArchetype::Developer, UserArchetype::Developer),
        (SimArchetype::Researcher, UserArchetype::Researcher),
        (SimArchetype::Investor, UserArchetype::Investor),
        (SimArchetype::Regulator, UserArchetype::Regulator),
        (SimArchetype::Other, UserArchetype::Other),
    ];
    
    info!("🔗 Archetype Mappings:");
    for (sim_archetype, prim_archetype) in mapping_pairs {
        let expectations = prim_archetype.default_expectations();
        info!("  Simulation::{:?} ↔ Primitives::{:?}", sim_archetype, prim_archetype);
        info!("    Expected Consistency: {:.1}-{:.1}", 
            expectations.expected_consistency_range.0,
            expectations.expected_consistency_range.1
        );
    }
    
    println!();
    Ok(())
}

/// Demonstrate behavioral pattern integration
async fn demo_behavioral_pattern_integration() -> Result<()> {
    info!("🧠 Demo 3: Behavioral Pattern Integration");
    println!();
    
    // Create a sample user with primitives types
    let user_archetype = UserArchetype::Developer;
    let expectations = user_archetype.default_expectations();
    
    info!("👤 Sample User: {}", user_archetype.description());
    
    // Create personality traits
    let personality = PersonalityTraits {
        archetype: user_archetype.clone(),
        activity_level: 8,
        consistency: 6,
        collaboration: 9,
        innovation: 10,
        security_consciousness: 8,
        economic_engagement: 7,
        cross_chain_preference: 6,
        peak_hours: vec![10, 11, 14, 15, 16, 17, 21, 22, 23],
        service_preferences: vec![ServiceType::Compute, ServiceType::AI, ServiceType::Storage],
        risk_tolerance: 7,
    };
    
    info!("🎯 Personality Profile:");
    info!("  Activity Level: {}/10", personality.activity_level);
    info!("  Innovation: {}/10", personality.innovation);
    info!("  Collaboration: {}/10", personality.collaboration);
    info!("  Security Consciousness: {}/10", personality.security_consciousness);
    
    // Create behavioral fingerprint
    let fingerprint = BehavioralFingerprint {
        fingerprint_id: uuid::Uuid::new_v4().to_string(),
        did: format!("did:spacekit:developer:{}", uuid::Uuid::new_v4().to_string()[..8].to_string()),
        activity_frequency: expectations.expected_activity_range.0 + 3.0,
        peak_activity_hours: personality.peak_hours.clone(),
        service_preferences: personality.service_preferences.clone(),
        interaction_style: InteractionStyle::Collaborative,
        anomaly_score: 0.02, // Low anomaly score for legitimate user
        pattern_stability: 0.75,
        economic_participation: 0.65,
        cross_chain_activity: 0.55,
        security_compliance: 0.90,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    
    info!("📊 Behavioral Fingerprint:");
    info!("  DID: {}", fingerprint.did);
    info!("  Activity Frequency: {:.1}", fingerprint.activity_frequency);
    info!("  Pattern Stability: {:.2}", fingerprint.pattern_stability);
    info!("  Security Compliance: {:.2}", fingerprint.security_compliance);
    info!("  Anomaly Score: {:.3} (lower is better)", fingerprint.anomaly_score);
    
    println!();
    Ok(())
}

/// Demonstrate reputation system integration
async fn demo_reputation_integration() -> Result<()> {
    info!("⭐ Demo 4: Reputation System Integration");
    println!();
    
    // Create sample address (simplified)
    let user_address: Address = "0x742d35Cc6479C4D1f0C5c5b6F7d4dE7b5D2a1234".parse()
        .map_err(|_| anyhow::anyhow!("Invalid address"))?;
    
    // Create service participation records
    let mut compute_participation = ServiceParticipation::new(ServiceType::Compute);
    compute_participation.record_action(true, 0.9);
    compute_participation.record_action(true, 0.85);
    compute_participation.record_action(false, 0.3);
    compute_participation.record_action(true, 0.95);
    
    let mut storage_participation = ServiceParticipation::new(ServiceType::Storage);
    storage_participation.record_action(true, 0.8);
    storage_participation.record_action(true, 0.88);
    
    info!("📈 Service Participation Analysis:");
    info!("  🖥️  Compute Service:");
    info!("    Total Actions: {}", compute_participation.total_actions);
    info!("    Success Rate: {:.1}%", compute_participation.success_rate * 100.0);
    info!("    Quality Score: {:.2}", compute_participation.quality_score);
    info!("    Frequency: {:.2} actions/day", compute_participation.frequency);
    
    info!("  💾 Storage Service:");
    info!("    Total Actions: {}", storage_participation.total_actions);
    info!("    Success Rate: {:.1}%", storage_participation.success_rate * 100.0);
    info!("    Quality Score: {:.2}", storage_participation.quality_score);
    
    // Simulate reputation score calculation
    let overall_reputation = calculate_reputation_score(&[
        &compute_participation,
        &storage_participation,
    ]);
    
    info!("🏆 Overall Reputation Score: {:.2}/1.0", overall_reputation);
    
    if overall_reputation > 0.8 {
        info!("✅ User qualifies for behavioral recovery (high reputation)");
    } else if overall_reputation > 0.6 {
        warn!("⚠️  User may qualify for behavioral recovery (moderate reputation)");
    } else {
        warn!("❌ User needs to build more reputation for behavioral recovery");
    }
    
    println!();
    Ok(())
}

/// Demonstrate recovery simulation preview
async fn demo_recovery_simulation_preview() -> Result<()> {
    info!("🔄 Demo 5: Recovery Simulation Preview");
    println!();
    
    // Simulate a recovery scenario
    let archetype = UserArchetype::Developer;
    let confidence_threshold = 0.7;
    
    info!("🎭 Recovery Scenario: {} attempting identity recovery", 
        archetype.description());
    
    // Step 1: Pattern Analysis
    info!("📊 Step 1: Analyzing behavioral patterns...");
    sleep(Duration::from_millis(100)).await;
    let pattern_score = 0.82;
    info!("  ✅ Pattern analysis complete: {:.2}", pattern_score);
    
    // Step 2: Confidence Calculation
    info!("🎯 Step 2: Calculating confidence score...");
    sleep(Duration::from_millis(150)).await;
    let confidence_score = 0.78;
    info!("  ✅ Confidence score: {:.2}", confidence_score);
    
    // Step 3: Challenge Generation
    info!("🎲 Step 3: Generating behavioral challenges...");
    sleep(Duration::from_millis(80)).await;
    let challenges = vec![
        "Peak activity timing verification",
        "Service preference validation",
        "Cross-chain activity pattern check",
        "Economic behavior consistency",
    ];
    info!("  ✅ Generated {} challenges", challenges.len());
    
    // Step 4: Recovery Decision
    info!("⚖️  Step 4: Making recovery decision...");
    sleep(Duration::from_millis(120)).await;
    
    let recovery_approved = confidence_score >= confidence_threshold;
    
    if recovery_approved {
        info!("  ✅ RECOVERY APPROVED");
        info!("    Confidence: {:.2} (threshold: {:.2})", confidence_score, confidence_threshold);
        info!("    Pattern Score: {:.2}", pattern_score);
        info!("    Challenges Passed: {}/{}", challenges.len(), challenges.len());
    } else {
        warn!("  ❌ RECOVERY DENIED");
        warn!("    Confidence: {:.2} (threshold: {:.2})", confidence_score, confidence_threshold);
        warn!("    Additional verification required");
    }
    
    // Step 5: Privacy-Preserving Verification (Future Enhancement)
    info!("🔐 Step 5: Privacy-preserving verification (preview)...");
    sleep(Duration::from_millis(200)).await;
    info!("  🛡️  Differential privacy: ε=1.0, δ=1e-6");
    info!("  🔑 Zero-knowledge proofs: Enabled");
    info!("  ⚡ Verification time: ~250ms");
    
    println!();
    
    // Summary
    info!("📋 Recovery Simulation Summary:");
    info!("  🎭 Archetype: {}", archetype.description());
    info!("  📊 Pattern Score: {:.2}", pattern_score);
    info!("  🎯 Confidence Score: {:.2}", confidence_score);
    info!("  ✅ Recovery Status: {}", if recovery_approved { "APPROVED" } else { "DENIED" });
    info!("  🔐 Privacy Level: Comprehensive");
    
    println!();
    Ok(())
}

/// Helper function to calculate reputation score
fn calculate_reputation_score(participations: &[&ServiceParticipation]) -> f64 {
    if participations.is_empty() {
        return 0.0;
    }
    
    let total_actions: u64 = participations.iter().map(|p| p.total_actions).sum();
    let weighted_success: f64 = participations.iter()
        .map(|p| p.success_rate * p.total_actions as f64)
        .sum();
    let weighted_quality: f64 = participations.iter()
        .map(|p| p.quality_score * p.total_actions as f64)
        .sum();
    
    if total_actions == 0 {
        return 0.0;
    }
    
    let avg_success = weighted_success / total_actions as f64;
    let avg_quality = weighted_quality / total_actions as f64;
    
    // Combined score weighted 60% success rate, 40% quality
    (avg_success * 0.6 + avg_quality * 0.4).min(1.0)
}