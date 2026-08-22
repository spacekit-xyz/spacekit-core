//! User Archetype Definitions and Behaviors
//! 
//! Defines the different user archetypes in the SWTCH network and their
//! characteristic behavioral patterns, as described in the simulation requirements.

use crate::{UserArchetype, PersonalityProfile, ServiceType, InteractionStyle};
use rand::{Rng, SeedableRng};
use rand::rngs::StdRng;
use serde::{Deserialize, Serialize};

/// Detailed archetype profile with behavioral tendencies
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchetypeProfile {
    pub archetype: UserArchetype,
    pub description: String,
    pub typical_behaviors: Vec<String>,
    pub preferred_services: Vec<ServiceType>,
    pub interaction_patterns: Vec<InteractionStyle>,
    pub activity_patterns: ActivityPatterns,
    pub economic_behavior: EconomicBehavior,
    pub security_profile: SecurityProfile,
}

/// Activity patterns for different archetypes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityPatterns {
    pub peak_hours: Vec<u8>,
    pub activity_frequency: ActivityFrequency,
    pub consistency_level: ConsistencyLevel,
    pub multi_chain_usage: MultiChainUsage,
}

/// Economic behavior patterns
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EconomicBehavior {
    pub staking_preference: StakingPreference,
    pub transaction_volume: TransactionVolume,
    pub governance_participation: GovernanceLevel,
    pub risk_tolerance: RiskTolerance,
}

/// Security behavior patterns
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityProfile {
    pub security_consciousness: SecurityLevel,
    pub compliance_adherence: ComplianceLevel,
    pub innovation_adoption: InnovationAdoption,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ActivityFrequency {
    Low,       // 1-3 transactions per hour
    Moderate,  // 4-8 transactions per hour
    High,      // 9-15 transactions per hour
    VeryHigh,  // 16+ transactions per hour
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConsistencyLevel {
    Variable,   // Inconsistent patterns
    Moderate,   // Somewhat predictable
    High,       // Very consistent
    Extreme,    // Almost robotic consistency
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MultiChainUsage {
    SingleChain,    // Primarily uses one blockchain
    FewChains,      // Uses 2-3 blockchains
    MultiChain,     // Uses 4-5 blockchains
    Universal,      // Uses all available chains
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StakingPreference {
    None,           // Doesn't stake
    Conservative,   // Small amounts, short duration
    Moderate,       // Medium amounts, medium duration
    Aggressive,     // Large amounts, long duration
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TransactionVolume {
    Low,        // Few transactions
    Medium,     // Regular transactions
    High,       // Many transactions
    VeryHigh,   // Constant transactions
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GovernanceLevel {
    None,       // Doesn't participate
    Passive,    // Votes occasionally
    Active,     // Regular participation
    Leader,     // Proposes and leads initiatives
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RiskTolerance {
    Conservative,   // Avoids new features
    Moderate,       // Cautious adoption
    Aggressive,     // Early adopter
    Extreme,        // Uses experimental features
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SecurityLevel {
    Basic,      // Minimal security practices
    Standard,   // Normal security awareness
    High,       // Strong security practices
    Paranoid,   // Maximum security measures
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ComplianceLevel {
    Lax,        // Ignores compliance
    Aware,      // Knows about compliance
    Adherent,   // Follows compliance
    Strict,     // Exceeds compliance requirements
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InnovationAdoption {
    Laggard,    // Very late adopter
    Follower,   // Adopts after others
    EarlyMajority, // Adopts when proven
    EarlyAdopter,  // Among first to adopt
}

/// Get detailed archetype profile
pub fn get_archetype_profile(archetype: &UserArchetype) -> ArchetypeProfile {
    match archetype {
        UserArchetype::BaseUser => ArchetypeProfile {
            archetype: archetype.clone(),
            description: "Typical network user who uses basic services for personal or small business needs".to_string(),
            typical_behaviors: vec![
                "Uses compute and storage services regularly".to_string(),
                "Participates in network during business hours".to_string(),
                "Moderate economic engagement".to_string(),
                "Follows established patterns".to_string(),
            ],
            preferred_services: vec![ServiceType::Compute, ServiceType::Storage, ServiceType::Messaging],
            interaction_patterns: vec![InteractionStyle::Collaborative, InteractionStyle::Independent],
            activity_patterns: ActivityPatterns {
                peak_hours: vec![9, 10, 11, 14, 15, 16, 17],
                activity_frequency: ActivityFrequency::Moderate,
                consistency_level: ConsistencyLevel::Moderate,
                multi_chain_usage: MultiChainUsage::FewChains,
            },
            economic_behavior: EconomicBehavior {
                staking_preference: StakingPreference::Conservative,
                transaction_volume: TransactionVolume::Medium,
                governance_participation: GovernanceLevel::Passive,
                risk_tolerance: RiskTolerance::Moderate,
            },
            security_profile: SecurityProfile {
                security_consciousness: SecurityLevel::Standard,
                compliance_adherence: ComplianceLevel::Aware,
                innovation_adoption: InnovationAdoption::Follower,
            },
        },
        
        UserArchetype::Validator => ArchetypeProfile {
            archetype: archetype.clone(),
            description: "Network validator who maintains infrastructure and validates transactions 24/7".to_string(),
            typical_behaviors: vec![
                "Operates infrastructure continuously".to_string(),
                "High security compliance".to_string(),
                "Significant economic stake in network".to_string(),
                "Active governance participation".to_string(),
                "Cross-chain operations".to_string(),
            ],
            preferred_services: vec![ServiceType::Compute, ServiceType::Identity, ServiceType::CrossChain],
            interaction_patterns: vec![InteractionStyle::Independent, InteractionStyle::Supportive],
            activity_patterns: ActivityPatterns {
                peak_hours: (0..24).collect(), // 24/7 operation
                activity_frequency: ActivityFrequency::VeryHigh,
                consistency_level: ConsistencyLevel::Extreme,
                multi_chain_usage: MultiChainUsage::Universal,
            },
            economic_behavior: EconomicBehavior {
                staking_preference: StakingPreference::Aggressive,
                transaction_volume: TransactionVolume::VeryHigh,
                governance_participation: GovernanceLevel::Active,
                risk_tolerance: RiskTolerance::Conservative,
            },
            security_profile: SecurityProfile {
                security_consciousness: SecurityLevel::Paranoid,
                compliance_adherence: ComplianceLevel::Strict,
                innovation_adoption: InnovationAdoption::EarlyMajority,
            },
        },
        
        UserArchetype::Developer => ArchetypeProfile {
            archetype: archetype.clone(),
            description: "Software developer building applications and contributing to the network".to_string(),
            typical_behaviors: vec![
                "Builds applications and smart contracts".to_string(),
                "Tests new features extensively".to_string(),
                "Irregular but intense work patterns".to_string(),
                "High collaboration with other developers".to_string(),
                "Innovation-focused".to_string(),
            ],
            preferred_services: vec![ServiceType::Compute, ServiceType::Storage, ServiceType::AI],
            interaction_patterns: vec![InteractionStyle::Collaborative, InteractionStyle::Competitive],
            activity_patterns: ActivityPatterns {
                peak_hours: vec![10, 11, 14, 15, 16, 17, 21, 22, 23], // Flexible hours
                activity_frequency: ActivityFrequency::High,
                consistency_level: ConsistencyLevel::Variable,
                multi_chain_usage: MultiChainUsage::MultiChain,
            },
            economic_behavior: EconomicBehavior {
                staking_preference: StakingPreference::Moderate,
                transaction_volume: TransactionVolume::High,
                governance_participation: GovernanceLevel::Active,
                risk_tolerance: RiskTolerance::Aggressive,
            },
            security_profile: SecurityProfile {
                security_consciousness: SecurityLevel::High,
                compliance_adherence: ComplianceLevel::Adherent,
                innovation_adoption: InnovationAdoption::EarlyAdopter,
            },
        },
        
        UserArchetype::Researcher => ArchetypeProfile {
            archetype: archetype.clone(),
            description: "Academic or industry researcher using the network for data analysis and collaboration".to_string(),
            typical_behaviors: vec![
                "Large-scale data storage and analysis".to_string(),
                "Collaboration with other researchers".to_string(),
                "Regular business hours activity".to_string(),
                "Methodical and consistent patterns".to_string(),
                "Focus on data integrity and provenance".to_string(),
            ],
            preferred_services: vec![ServiceType::Storage, ServiceType::AI, ServiceType::Compute],
            interaction_patterns: vec![InteractionStyle::Collaborative, InteractionStyle::Supportive],
            activity_patterns: ActivityPatterns {
                peak_hours: vec![9, 10, 11, 12, 13, 14, 15, 16],
                activity_frequency: ActivityFrequency::Moderate,
                consistency_level: ConsistencyLevel::High,
                multi_chain_usage: MultiChainUsage::FewChains,
            },
            economic_behavior: EconomicBehavior {
                staking_preference: StakingPreference::Conservative,
                transaction_volume: TransactionVolume::Medium,
                governance_participation: GovernanceLevel::Passive,
                risk_tolerance: RiskTolerance::Moderate,
            },
            security_profile: SecurityProfile {
                security_consciousness: SecurityLevel::High,
                compliance_adherence: ComplianceLevel::Strict,
                innovation_adoption: InnovationAdoption::EarlyMajority,
            },
        },
        
        UserArchetype::Investor => ArchetypeProfile {
            archetype: archetype.clone(),
            description: "Financial investor focused on network economics and cross-chain opportunities".to_string(),
            typical_behaviors: vec![
                "High-frequency cross-chain transactions".to_string(),
                "Market timing-based activity".to_string(),
                "Significant economic engagement".to_string(),
                "Risk assessment and management".to_string(),
                "Competitive behavior for opportunities".to_string(),
            ],
            preferred_services: vec![ServiceType::CrossChain, ServiceType::Identity],
            interaction_patterns: vec![InteractionStyle::Competitive, InteractionStyle::Independent],
            activity_patterns: ActivityPatterns {
                peak_hours: vec![6, 7, 8, 9, 14, 15, 16, 20, 21], // Market hours + evening
                activity_frequency: ActivityFrequency::High,
                consistency_level: ConsistencyLevel::Variable, // Market-driven
                multi_chain_usage: MultiChainUsage::Universal,
            },
            economic_behavior: EconomicBehavior {
                staking_preference: StakingPreference::Aggressive,
                transaction_volume: TransactionVolume::VeryHigh,
                governance_participation: GovernanceLevel::Active,
                risk_tolerance: RiskTolerance::Extreme,
            },
            security_profile: SecurityProfile {
                security_consciousness: SecurityLevel::High,
                compliance_adherence: ComplianceLevel::Adherent,
                innovation_adoption: InnovationAdoption::EarlyAdopter,
            },
        },
        
        UserArchetype::Regulator => ArchetypeProfile {
            archetype: archetype.clone(),
            description: "Regulatory entity monitoring network compliance and security".to_string(),
            typical_behaviors: vec![
                "Monitoring and auditing network activity".to_string(),
                "Strict compliance enforcement".to_string(),
                "Regular business hours operation".to_string(),
                "High security requirements".to_string(),
                "Conservative approach to new features".to_string(),
            ],
            preferred_services: vec![ServiceType::Identity, ServiceType::Encryption],
            interaction_patterns: vec![InteractionStyle::Independent, InteractionStyle::Supportive],
            activity_patterns: ActivityPatterns {
                peak_hours: vec![9, 10, 11, 12, 13, 14, 15, 16],
                activity_frequency: ActivityFrequency::Low,
                consistency_level: ConsistencyLevel::Extreme,
                multi_chain_usage: MultiChainUsage::FewChains,
            },
            economic_behavior: EconomicBehavior {
                staking_preference: StakingPreference::Conservative,
                transaction_volume: TransactionVolume::Low,
                governance_participation: GovernanceLevel::Leader,
                risk_tolerance: RiskTolerance::Conservative,
            },
            security_profile: SecurityProfile {
                security_consciousness: SecurityLevel::Paranoid,
                compliance_adherence: ComplianceLevel::Strict,
                innovation_adoption: InnovationAdoption::Laggard,
            },
        },
        
        UserArchetype::Other => ArchetypeProfile {
            archetype: archetype.clone(),
            description: "Miscellaneous users with diverse and unpredictable behavior patterns".to_string(),
            typical_behaviors: vec![
                "Variable activity patterns".to_string(),
                "Experimental usage".to_string(),
                "Diverse service preferences".to_string(),
                "Unpredictable engagement".to_string(),
            ],
            preferred_services: vec![ServiceType::Messaging, ServiceType::Storage],
            interaction_patterns: vec![
                InteractionStyle::Independent, 
                InteractionStyle::Collaborative,
                InteractionStyle::Supportive,
            ],
            activity_patterns: ActivityPatterns {
                peak_hours: vec![12, 18], // Random hours
                activity_frequency: ActivityFrequency::Low,
                consistency_level: ConsistencyLevel::Variable,
                multi_chain_usage: MultiChainUsage::SingleChain,
            },
            economic_behavior: EconomicBehavior {
                staking_preference: StakingPreference::None,
                transaction_volume: TransactionVolume::Low,
                governance_participation: GovernanceLevel::None,
                risk_tolerance: RiskTolerance::Moderate,
            },
            security_profile: SecurityProfile {
                security_consciousness: SecurityLevel::Basic,
                compliance_adherence: ComplianceLevel::Lax,
                innovation_adoption: InnovationAdoption::Follower,
            },
        },
    }
}

/// Generate realistic activity frequency based on archetype profile
pub fn get_activity_frequency_range(frequency: &ActivityFrequency) -> (f64, f64) {
    match frequency {
        ActivityFrequency::Low => (1.0, 3.0),
        ActivityFrequency::Moderate => (4.0, 8.0),
        ActivityFrequency::High => (9.0, 15.0),
        ActivityFrequency::VeryHigh => (16.0, 30.0),
    }
}

/// Generate personality traits based on archetype profile
pub fn generate_archetype_personality(
    profile: &ArchetypeProfile,
    rng: &mut StdRng,
) -> PersonalityProfile {
    let activity_range = get_activity_frequency_range(&profile.activity_patterns.activity_frequency);
    let base_activity = rng.gen_range(activity_range.0 as u8..=activity_range.1 as u8).min(10);
    
    let consistency = match profile.activity_patterns.consistency_level {
        ConsistencyLevel::Variable => rng.gen_range(3..=6),
        ConsistencyLevel::Moderate => rng.gen_range(6..=8),
        ConsistencyLevel::High => rng.gen_range(8..=9),
        ConsistencyLevel::Extreme => 10,
    };
    
    let collaboration = match profile.interaction_patterns.first() {
        Some(InteractionStyle::Collaborative) => rng.gen_range(7..=10),
        Some(InteractionStyle::Supportive) => rng.gen_range(6..=9),
        Some(InteractionStyle::Competitive) => rng.gen_range(3..=6),
        Some(InteractionStyle::Independent) => rng.gen_range(2..=5),
        Some(InteractionStyle::Suspicious) => rng.gen_range(1..=3),
        None => rng.gen_range(4..=7),
    };
    
    let innovation = match profile.security_profile.innovation_adoption {
        InnovationAdoption::Laggard => rng.gen_range(1..=3),
        InnovationAdoption::Follower => rng.gen_range(3..=5),
        InnovationAdoption::EarlyMajority => rng.gen_range(5..=7),
        InnovationAdoption::EarlyAdopter => rng.gen_range(7..=10),
    };
    
    let security_consciousness = match profile.security_profile.security_consciousness {
        SecurityLevel::Basic => rng.gen_range(2..=4),
        SecurityLevel::Standard => rng.gen_range(5..=7),
        SecurityLevel::High => rng.gen_range(7..=9),
        SecurityLevel::Paranoid => 10,
    };
    
    let economic_engagement = match profile.economic_behavior.staking_preference {
        StakingPreference::None => rng.gen_range(1..=3),
        StakingPreference::Conservative => rng.gen_range(3..=5),
        StakingPreference::Moderate => rng.gen_range(5..=7),
        StakingPreference::Aggressive => rng.gen_range(7..=10),
    };
    
    let cross_chain_preference = match profile.activity_patterns.multi_chain_usage {
        MultiChainUsage::SingleChain => rng.gen_range(1..=3),
        MultiChainUsage::FewChains => rng.gen_range(3..=5),
        MultiChainUsage::MultiChain => rng.gen_range(5..=7),
        MultiChainUsage::Universal => rng.gen_range(7..=10),
    };
    
    let risk_tolerance = match profile.economic_behavior.risk_tolerance {
        RiskTolerance::Conservative => rng.gen_range(1..=3),
        RiskTolerance::Moderate => rng.gen_range(4..=6),
        RiskTolerance::Aggressive => rng.gen_range(7..=8),
        RiskTolerance::Extreme => rng.gen_range(9..=10),
    };
    
    PersonalityProfile {
        archetype: profile.archetype.clone(),
        activity_level: base_activity,
        consistency,
        collaboration,
        innovation,
        security_consciousness,
        economic_engagement,
        cross_chain_preference,
        peak_hours: profile.activity_patterns.peak_hours.clone(),
        service_preferences: profile.preferred_services.clone(),
        risk_tolerance,
    }
}

/// Get all archetype profiles for analysis
pub fn get_all_archetype_profiles() -> Vec<ArchetypeProfile> {
    vec![
        get_archetype_profile(&UserArchetype::BaseUser),
        get_archetype_profile(&UserArchetype::Validator),
        get_archetype_profile(&UserArchetype::Developer),
        get_archetype_profile(&UserArchetype::Researcher),
        get_archetype_profile(&UserArchetype::Investor),
        get_archetype_profile(&UserArchetype::Regulator),
        get_archetype_profile(&UserArchetype::Other),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_archetype_profiles() {
        let profiles = get_all_archetype_profiles();
        assert_eq!(profiles.len(), 7);
        
        // Check that each archetype has distinct characteristics
        let validator = get_archetype_profile(&UserArchetype::Validator);
        assert!(matches!(validator.activity_patterns.consistency_level, ConsistencyLevel::Extreme));
        assert!(matches!(validator.security_profile.security_consciousness, SecurityLevel::Paranoid));
        
        let developer = get_archetype_profile(&UserArchetype::Developer);
        assert!(matches!(developer.activity_patterns.consistency_level, ConsistencyLevel::Variable));
        assert!(matches!(developer.security_profile.innovation_adoption, InnovationAdoption::EarlyAdopter));
    }
    
    #[test]
    fn test_personality_generation() {
        let mut rng = StdRng::seed_from_u64(12345);
        let profile = get_archetype_profile(&UserArchetype::Validator);
        let personality = generate_archetype_personality(&profile, &mut rng);
        
        assert_eq!(personality.archetype, UserArchetype::Validator);
        assert!(personality.security_consciousness >= 7); // Should be high for validators
        assert!(personality.consistency >= 8); // Should be very consistent
    }
}