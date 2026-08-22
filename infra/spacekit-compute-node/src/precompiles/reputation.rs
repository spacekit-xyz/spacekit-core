//! Reputation Precompile for SpaceKitVM
//! 
//! Allows smart contracts to check user reputation scores, enforce thresholds,
//! and gate functionality based on social, service, or behavioral reputation.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use std::collections::HashMap;

/// Reputation types for different contexts
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ReputationType {
    /// Social behavior (messaging, community)
    Social,
    /// Service behavior (uptime, reliability)
    Service,
    /// Application behavior (quality, security)
    Application,
    /// Compute behavior (AI/ML accuracy, performance)
    Compute,
    /// Storage behavior (availability, integrity)
    Storage,
    /// Overall combined reputation
    Overall,
}

/// Reputation score with context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReputationContext {
    pub did: String,
    pub reputation_type: ReputationType,
    pub score: i64,
    pub confidence: f64, // 0.0 to 1.0
    pub total_interactions: u64,
    pub positive_interactions: u64,
    pub negative_interactions: u64,
    pub last_updated: u64, // Timestamp
}

/// Reputation precompile for smart contracts
pub struct ReputationPrecompile {
    /// Reputation scores by DID and type
    scores: Arc<RwLock<HashMap<String, HashMap<ReputationType, ReputationContext>>>>,
    /// Integration with messaging node reputation
    messaging_reputation: Option<Arc<RwLock<HashMap<String, i64>>>>,
    /// Behavioral cryptography scores
    behavioral_scores: Option<Arc<RwLock<HashMap<String, f64>>>>,
}

impl ReputationPrecompile {
    /// Create a new reputation precompile
    pub fn new() -> Self {
        Self {
            scores: Arc::new(RwLock::new(HashMap::new())),
            messaging_reputation: None,
            behavioral_scores: None,
        }
    }

    /// Link messaging node reputation
    pub fn link_messaging_reputation(&mut self, reputation: Arc<RwLock<HashMap<String, i64>>>) {
        self.messaging_reputation = Some(reputation);
    }

    /// Link behavioral cryptography scores
    pub fn link_behavioral_scores(&mut self, scores: Arc<RwLock<HashMap<String, f64>>>) {
        self.behavioral_scores = Some(scores);
    }

    /// Get reputation score for a DID
    pub async fn get_reputation(&self, did: &str, rep_type: ReputationType) -> Result<i64> {
        // Check if we have this score
        let scores = self.scores.read().await;
        
        if let Some(user_scores) = scores.get(did) {
            if let Some(context) = user_scores.get(&rep_type) {
                return Ok(context.score);
            }
        }

        // Fall back to linked sources
        match rep_type {
            ReputationType::Social => {
                if let Some(msg_rep) = &self.messaging_reputation {
                    let scores = msg_rep.read().await;
                    return Ok(*scores.get(did).unwrap_or(&0));
                }
            }
            ReputationType::Overall => {
                // Compute overall from all sources
                return self.compute_overall_reputation(did).await;
            }
            _ => {}
        }

        Ok(0) // Default neutral reputation
    }

    /// Check if DID meets reputation threshold
    pub async fn check_threshold(&self, did: &str, rep_type: ReputationType, threshold: i64) -> Result<bool> {
        let score = self.get_reputation(did, rep_type).await?;
        Ok(score >= threshold)
    }

    /// Update reputation score
    pub async fn update_reputation(
        &self,
        did: String,
        rep_type: ReputationType,
        delta: i64,
        reason: String,
    ) -> Result<()> {
        let mut scores = self.scores.write().await;
        
        let user_scores = scores.entry(did.clone()).or_insert_with(HashMap::new);
        
        let context = user_scores.entry(rep_type.clone()).or_insert_with(|| ReputationContext {
            did: did.clone(),
            reputation_type: rep_type.clone(),
            score: 0,
            confidence: 0.5,
            total_interactions: 0,
            positive_interactions: 0,
            negative_interactions: 0,
            last_updated: chrono::Utc::now().timestamp() as u64,
        });

        context.score += delta;
        context.total_interactions += 1;
        
        if delta > 0 {
            context.positive_interactions += 1;
        } else if delta < 0 {
            context.negative_interactions += 1;
        }

        context.last_updated = chrono::Utc::now().timestamp() as u64;
        
        // Update confidence based on interaction count
        context.confidence = (context.total_interactions as f64 / (context.total_interactions as f64 + 10.0)).min(1.0);

        println!("📊 Updated {} reputation for {}: {} ({:+} - {})",
                 match rep_type {
                     ReputationType::Social => "social",
                     ReputationType::Service => "service",
                     ReputationType::Application => "app",
                     ReputationType::Compute => "compute",
                     ReputationType::Storage => "storage",
                     ReputationType::Overall => "overall",
                 },
                 did, context.score, delta, reason);

        Ok(())
    }

    /// Compute overall reputation from all sources
    async fn compute_overall_reputation(&self, did: &str) -> Result<i64> {
        let scores = self.scores.read().await;
        let mut total = 0i64;
        let mut count = 0;

        // Get all reputation types for this DID
        if let Some(user_scores) = scores.get(did) {
            for (_type, context) in user_scores {
                total += context.score;
                count += 1;
            }
        }

        // Include messaging reputation
        if let Some(msg_rep) = &self.messaging_reputation {
            let msg_scores = msg_rep.read().await;
            if let Some(score) = msg_scores.get(did) {
                total += score;
                count += 1;
            }
        }

        // Include behavioral score (convert 0.0-1.0 to -500 to +500)
        if let Some(behavioral) = &self.behavioral_scores {
            let scores = behavioral.read().await;
            if let Some(score) = scores.get(did) {
                let normalized = (score - 0.5) * 1000.0; // 0.5 = neutral
                total += normalized as i64;
                count += 1;
            }
        }

        if count > 0 {
            Ok(total / count)
        } else {
            Ok(0)
        }
    }

    /// Get detailed reputation breakdown
    pub async fn get_reputation_breakdown(&self, did: &str) -> Result<ReputationBreakdown> {
        let scores = self.scores.read().await;
        let mut breakdown = ReputationBreakdown {
            did: did.to_string(),
            social: 0,
            service: 0,
            application: 0,
            compute: 0,
            storage: 0,
            overall: 0,
        };

        if let Some(user_scores) = scores.get(did) {
            for (rep_type, context) in user_scores {
                match rep_type {
                    ReputationType::Social => breakdown.social = context.score,
                    ReputationType::Service => breakdown.service = context.score,
                    ReputationType::Application => breakdown.application = context.score,
                    ReputationType::Compute => breakdown.compute = context.score,
                    ReputationType::Storage => breakdown.storage = context.score,
                    ReputationType::Overall => breakdown.overall = context.score,
                }
            }
        }

        // Get overall score
        breakdown.overall = self.compute_overall_reputation(did).await?;

        Ok(breakdown)
    }

    /// Record a successful interaction
    pub async fn record_success(&self, did: String, rep_type: ReputationType, delta: i64) -> Result<()> {
        self.update_reputation(did, rep_type, delta, "Successful interaction".to_string()).await
    }

    /// Record a failed interaction  
    pub async fn record_failure(&self, did: String, rep_type: ReputationType, penalty: i64) -> Result<()> {
        self.update_reputation(did, rep_type, -penalty, "Failed interaction".to_string()).await
    }
}

/// Reputation breakdown for detailed view
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReputationBreakdown {
    pub did: String,
    pub social: i64,
    pub service: i64,
    pub application: i64,
    pub compute: i64,
    pub storage: i64,
    pub overall: i64,
}

/// Smart contract interface for reputation checks
pub mod solidity {
    /// Function signatures for Solidity contracts
    
    /// Check if user meets reputation threshold
    /// function requireReputation(address user, string memory repType, int256 threshold)
    pub const REQUIRE_REPUTATION: &str = "requireReputation(address,string,int256)";
    
    /// Get user's reputation score
    /// function getReputation(address user, string memory repType) returns (int256)
    pub const GET_REPUTATION: &str = "getReputation(address,string)";
    
    /// Get overall reputation
    /// function getOverallReputation(address user) returns (int256)
    pub const GET_OVERALL_REPUTATION: &str = "getOverallReputation(address)";
    
    /// Get reputation breakdown
    /// function getReputationBreakdown(address user) returns (int256[6])
    pub const GET_REPUTATION_BREAKDOWN: &str = "getReputationBreakdown(address)";
}

impl Default for ReputationPrecompile {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_reputation_check() {
        let precompile = ReputationPrecompile::new();
        
        // Update social reputation
        precompile.update_reputation(
            "did:spacekit:user:alice".to_string(),
            ReputationType::Social,
            150,
            "Good community member".to_string(),
        ).await.unwrap();

        // Check threshold
        let meets_threshold = precompile.check_threshold(
            "did:spacekit:user:alice",
            ReputationType::Social,
            100,
        ).await.unwrap();

        assert!(meets_threshold);
    }

    #[tokio::test]
    async fn test_reputation_breakdown() {
        let precompile = ReputationPrecompile::new();
        
        let did = "did:spacekit:user:test";
        
        // Update different reputation types
        precompile.update_reputation(did.to_string(), ReputationType::Social, 100, "".to_string()).await.unwrap();
        precompile.update_reputation(did.to_string(), ReputationType::Compute, 200, "".to_string()).await.unwrap();
        precompile.update_reputation(did.to_string(), ReputationType::Storage, 150, "".to_string()).await.unwrap();

        // Get breakdown
        let breakdown = precompile.get_reputation_breakdown(did).await.unwrap();
        
        assert_eq!(breakdown.social, 100);
        assert_eq!(breakdown.compute, 200);
        assert_eq!(breakdown.storage, 150);
        assert_eq!(breakdown.overall, (100 + 200 + 150) / 3); // Average
    }
}

