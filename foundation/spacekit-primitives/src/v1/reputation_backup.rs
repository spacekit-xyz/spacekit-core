use alloy_primitives::{Address, U256};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReputationProfile {
    pub address: Address,
    pub participant_score: ParticipantScore,
    pub eth_escrow_balance: U256,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParticipantScore {
    pub as_consumer: ReputationScore,
    pub as_producer: ReputationScore,
    pub product_scores: Vec<(String, U256)>,  // (product_hash, score)
    pub actions: Vec<(String, ReputationAction)>,  // (action_type, action)
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReputationScore {
    pub score: U256,
    pub total_actions: U256,
    pub successful_actions: U256,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReputationAction {
    pub weight: U256,
    pub last_action_time: U256,
}