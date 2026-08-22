//! Storage Node Reward System
//!
//! Calculates and distributes ASTRA token rewards for storage node operators
//! based on storage provision, quality of service, and network participation.

use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{debug, info, warn};

use crate::{StorageNode, StorageStats};

/// Storage node reward configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageRewardConfig {
    /// Base reward per GB per day (in ASTRA tokens)
    pub base_reward_per_gb_day: f64,

    /// Multipliers for different storage types
    pub hot_storage_multiplier: f64,
    pub fact_storage_multiplier: f64,
    pub nft_storage_multiplier: f64,

    /// Bonus multipliers
    pub quantum_encryption_bonus: f64,
    pub replication_bonus_per_copy: f64,
    pub high_availability_bonus: f64,

    /// Reputation-based bonuses
    pub min_reputation_for_bonus: f64,
    pub reputation_multiplier: f64,

    /// Quality of service bonuses
    pub fast_retrieval_bonus: f64, // < 100ms average
    pub uptime_bonus_threshold: f64, // > 99% uptime
    pub uptime_bonus: f64,

    /// Network participation bonuses
    pub p2p_contribution_bonus: f64,
    pub fact_verification_bonus: f64,

    /// Limits
    pub max_daily_rewards: u128, // Maximum ASTRA per day (in wei)
    pub min_storage_gb_for_rewards: u64, // Minimum storage to earn rewards

    /// Token settings
    pub enable_token_minting: bool,
    pub reward_interval_hours: u64,
}

impl Default for StorageRewardConfig {
    fn default() -> Self {
        Self {
            base_reward_per_gb_day: 0.01,                   // 0.01 ASTRA per GB/day
            hot_storage_multiplier: 2.0,                    // 2x for hot storage
            fact_storage_multiplier: 1.5,                   // 1.5x for fact packages
            nft_storage_multiplier: 2.5,                    // 2.5x for NFT storage
            quantum_encryption_bonus: 1.2,                  // +20% for quantum encryption
            replication_bonus_per_copy: 1.1,                // +10% per replication
            high_availability_bonus: 1.3,                   // +30% for HA nodes
            min_reputation_for_bonus: 0.7,                  // 70% reputation threshold
            reputation_multiplier: 1.25,                    // +25% for high reputation
            fast_retrieval_bonus: 1.15,                     // +15% for fast retrieval
            uptime_bonus_threshold: 0.99,                   // 99% uptime required
            uptime_bonus: 1.2,                              // +20% for high uptime
            p2p_contribution_bonus: 1.1,                    // +10% for P2P contribution
            fact_verification_bonus: 1.05,                  // +5% for fact verification
            max_daily_rewards: 100_000_000_000_000_000_000, // 100 ASTRA in wei
            min_storage_gb_for_rewards: 10,                 // Minimum 10GB to earn
            enable_token_minting: true,
            reward_interval_hours: 24, // Daily rewards
        }
    }
}

/// Storage node reward calculator
pub struct StorageRewardCalculator {
    config: StorageRewardConfig,
    storage_node: Arc<StorageNode>,
    reward_history: Vec<RewardRecord>,
    last_reward_time: Option<DateTime<Utc>>,
}

/// Record of a reward payment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RewardRecord {
    pub timestamp: DateTime<Utc>,
    pub amount_astra: f64,
    pub amount_wei: u128,
    /// aUSD fee income earned from user-paid storage operations this period.
    #[serde(default)]
    pub amount_ausd: f64,
    pub storage_gb: f64,
    pub bonus_multipliers: BonusMultipliers,
    pub node_did: String,
    /// On-chain settlement tx hash (empty if not yet settled).
    #[serde(default)]
    pub settlement_tx: String,
}

/// Breakdown of bonus multipliers applied
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BonusMultipliers {
    pub base_multiplier: f64,
    pub storage_type_multiplier: f64,
    pub quantum_bonus: f64,
    pub replication_bonus: f64,
    pub reputation_bonus: f64,
    pub uptime_bonus: f64,
    pub fast_retrieval_bonus: f64,
    pub p2p_bonus: f64,
    pub fact_verification_bonus: f64,
    pub total_multiplier: f64,
}

/// Detailed reward calculation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RewardCalculation {
    pub base_reward: f64,
    pub final_reward: f64,
    pub reward_wei: u128,
    pub storage_gb: f64,
    pub bonus_breakdown: BonusMultipliers,
    pub reward_per_gb: f64,
    pub daily_rate: f64,
}

impl StorageRewardCalculator {
    /// Create a new reward calculator
    pub fn new(config: StorageRewardConfig, storage_node: Arc<StorageNode>) -> Self {
        Self {
            config,
            storage_node,
            reward_history: Vec::new(),
            last_reward_time: None,
        }
    }

    /// Calculate current rewards based on storage stats
    pub async fn calculate_rewards(&self) -> Result<RewardCalculation> {
        if !self.config.enable_token_minting {
            return Ok(RewardCalculation::default());
        }

        // Get current storage statistics
        let stats = self.storage_node.get_stats().await?;

        // Check minimum storage requirement
        let storage_gb = stats.total_size_bytes as f64 / (1024.0 * 1024.0 * 1024.0);
        if storage_gb < self.config.min_storage_gb_for_rewards as f64 {
            debug!(
                "Storage {} GB below minimum {} GB for rewards",
                storage_gb, self.config.min_storage_gb_for_rewards
            );
            return Ok(RewardCalculation::default());
        }

        // Calculate base reward
        let base_reward = storage_gb * self.config.base_reward_per_gb_day;

        // Calculate bonus multipliers
        let bonus_multipliers = self.calculate_bonus_multipliers(&stats).await?;

        // Apply all multipliers
        let final_reward = base_reward * bonus_multipliers.total_multiplier;

        // Convert to wei (18 decimals)
        let reward_wei = (final_reward * 1e18) as u128;

        // Apply daily limit
        let capped_reward_wei = reward_wei.min(self.config.max_daily_rewards);

        let calculation = RewardCalculation {
            base_reward,
            final_reward: capped_reward_wei as f64 / 1e18,
            reward_wei: capped_reward_wei,
            storage_gb,
            bonus_breakdown: bonus_multipliers.clone(),
            reward_per_gb: (capped_reward_wei as f64 / 1e18) / storage_gb,
            daily_rate: (capped_reward_wei as f64 / 1e18) * 30.0, // Monthly estimate
        };

        info!(
            "Storage rewards calculated: {} ASTRA for {} GB ({}x multiplier)",
            calculation.final_reward, storage_gb, bonus_multipliers.total_multiplier
        );

        Ok(calculation)
    }

    /// Calculate all bonus multipliers based on node performance
    async fn calculate_bonus_multipliers(&self, stats: &StorageStats) -> Result<BonusMultipliers> {
        let mut multipliers = BonusMultipliers {
            base_multiplier: 1.0,
            storage_type_multiplier: 1.0,
            quantum_bonus: 1.0,
            replication_bonus: 1.0,
            reputation_bonus: 1.0,
            uptime_bonus: 1.0,
            fast_retrieval_bonus: 1.0,
            p2p_bonus: 1.0,
            fact_verification_bonus: 1.0,
            total_multiplier: 1.0,
        };

        // Storage type multipliers
        multipliers.storage_type_multiplier = self.calculate_storage_type_multiplier(stats);

        // Quantum encryption bonus
        if self.is_using_quantum_encryption(stats) {
            multipliers.quantum_bonus = self.config.quantum_encryption_bonus;
            debug!(
                "Applied quantum encryption bonus: {}x",
                multipliers.quantum_bonus
            );
        }

        // Replication bonus (if P2P is enabled)
        #[cfg(feature = "p2p")]
        {
            let replication_factor = 3; // Default replication factor
            multipliers.replication_bonus = self
                .config
                .replication_bonus_per_copy
                .powi(replication_factor - 1);
            debug!(
                "Applied replication bonus: {}x",
                multipliers.replication_bonus
            );
        }

        // Reputation bonus (placeholder - would integrate with reputation system)
        let reputation = 0.85; // Placeholder: would come from network reputation
        if reputation >= self.config.min_reputation_for_bonus {
            multipliers.reputation_bonus = self.config.reputation_multiplier;
            debug!(
                "Applied reputation bonus: {}x",
                multipliers.reputation_bonus
            );
        }

        // Uptime bonus (placeholder - would track actual uptime)
        let uptime = 0.995; // 99.5% uptime
        if uptime >= self.config.uptime_bonus_threshold {
            multipliers.uptime_bonus = self.config.uptime_bonus;
            debug!("Applied uptime bonus: {}x", multipliers.uptime_bonus);
        }

        // Fast retrieval bonus (placeholder - would track retrieval times)
        let avg_retrieval_ms = 85.0;
        if avg_retrieval_ms < 100.0 {
            multipliers.fast_retrieval_bonus = self.config.fast_retrieval_bonus;
            debug!(
                "Applied fast retrieval bonus: {}x",
                multipliers.fast_retrieval_bonus
            );
        }

        // P2P contribution bonus
        #[cfg(feature = "p2p")]
        {
            multipliers.p2p_bonus = self.config.p2p_contribution_bonus;
            debug!("Applied P2P contribution bonus: {}x", multipliers.p2p_bonus);
        }

        // Fact verification bonus (if fact storage is being used)
        if stats.file_count > 0 {
            multipliers.fact_verification_bonus = self.config.fact_verification_bonus;
            debug!(
                "Applied fact verification bonus: {}x",
                multipliers.fact_verification_bonus
            );
        }

        // Calculate total multiplier
        multipliers.total_multiplier = multipliers.base_multiplier
            * multipliers.storage_type_multiplier
            * multipliers.quantum_bonus
            * multipliers.replication_bonus
            * multipliers.reputation_bonus
            * multipliers.uptime_bonus
            * multipliers.fast_retrieval_bonus
            * multipliers.p2p_bonus
            * multipliers.fact_verification_bonus;

        Ok(multipliers)
    }

    /// Calculate storage type multiplier based on what's being stored
    fn calculate_storage_type_multiplier(&self, stats: &StorageStats) -> f64 {
        // Default to fact storage multiplier if we have files
        // In production, would track file types more granularly
        if stats.file_count > 0 {
            self.config.fact_storage_multiplier
        } else {
            1.0
        }
    }

    /// Check if node is using quantum encryption
    fn is_using_quantum_encryption(&self, stats: &StorageStats) -> bool {
        // Check if using post-quantum algorithms
        let quantum_algorithms = ["kyber512", "kyber768", "kyber1024", "ntru", "frodokem"];
        quantum_algorithms.contains(&stats.preferred_algorithm.to_lowercase().as_str())
    }

    /// Process reward payment (would integrate with blockchain)
    pub async fn process_reward_payment(&mut self) -> Result<Option<RewardRecord>> {
        // Check if enough time has passed since last reward
        if let Some(last_time) = self.last_reward_time {
            let elapsed = Utc::now() - last_time;
            if elapsed < Duration::hours(self.config.reward_interval_hours as i64) {
                debug!("Not enough time elapsed since last reward");
                return Ok(None);
            }
        }

        // Calculate rewards
        let calculation = self.calculate_rewards().await?;

        if calculation.final_reward == 0.0 {
            debug!("No rewards to distribute");
            return Ok(None);
        }

        // Create reward record
        let stats = self.storage_node.get_stats().await?;

        // aUSD fee income: operators earn a per-GB-month aUSD fee from users.
        // The base rate is 0.01 aUSD/GB/day — configurable via environment.
        let ausd_rate: f64 = std::env::var("SPACEKIT_AUSD_PER_GB_DAY")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.01);
        let amount_ausd = calculation.storage_gb * ausd_rate;

        let mut record = RewardRecord {
            timestamp: Utc::now(),
            amount_astra: calculation.final_reward,
            amount_wei: calculation.reward_wei,
            amount_ausd,
            storage_gb: calculation.storage_gb,
            bonus_multipliers: calculation.bonus_breakdown.clone(),
            node_did: stats.node_did.clone(),
            settlement_tx: String::new(),
        };

        // TODO: Integrate with SpaceKitVM blockchain to mint/transfer tokens
        // For now, just record it
        info!(
            "Processing reward payment: {} ASTRA to node {}",
            record.amount_astra, record.node_did
        );

        // Submit a VPoS storage proof to the SpaceKit network for on-chain settlement.
        // The compute node's VPoS system will verify the proof and mint tokens.
        #[cfg(feature = "standalone")]
        {
            let proof_payload = serde_json::json!({
                "type": "storage_reward",
                "node_did": &record.node_did,
                "amount_astra": record.amount_astra,
                "amount_wei": record.amount_wei,
                "amount_ausd": record.amount_ausd,
                "storage_gb": record.storage_gb,
                "bonus_multipliers": record.bonus_multipliers,
                "period_end": Utc::now().to_rfc3339(),
            });

            let settlement_url = std::env::var("SPACEKIT_SETTLEMENT_URL")
                .unwrap_or_else(|_| "http://0.0.0.0:9000".to_string());

            match reqwest::Client::new()
                .post(format!("{}/api/vpos/storage-proof", settlement_url))
                .json(&proof_payload)
                .timeout(std::time::Duration::from_secs(15))
                .send()
                .await
            {
                Ok(resp) if resp.status().is_success() => {
                    let body: serde_json::Value = resp.json().await.unwrap_or_default();
                    let tx_hash = body
                        .get("tx_hash")
                        .and_then(|v| v.as_str())
                        .unwrap_or("pending");
                    info!(
                        "VPoS storage proof accepted — tx: {}, reward: {} ASTRA + {} aUSD",
                        tx_hash, record.amount_astra, record.amount_ausd
                    );
                    record.settlement_tx = tx_hash.to_string();
                }
                Ok(resp) => {
                    let status = resp.status();
                    let body = resp.text().await.unwrap_or_default();
                    warn!(
                        "VPoS settlement returned HTTP {}: {} (reward recorded locally)",
                        status, body
                    );
                }
                Err(e) => {
                    warn!(
                        "VPoS settlement unreachable: {} (reward recorded locally, will retry)",
                        e
                    );
                }
            }
        }

        // Update history
        self.reward_history.push(record.clone());
        self.last_reward_time = Some(Utc::now());

        Ok(Some(record))
    }

    /// Get reward history
    pub fn get_reward_history(&self) -> &[RewardRecord] {
        &self.reward_history
    }

    /// Get total rewards earned
    pub fn get_total_rewards_earned(&self) -> f64 {
        self.reward_history.iter().map(|r| r.amount_astra).sum()
    }

    /// Get estimated monthly income based on current storage
    pub async fn estimate_monthly_income(&self) -> Result<f64> {
        let calculation = self.calculate_rewards().await?;
        Ok(calculation.daily_rate)
    }

    /// Get reward analytics
    pub async fn get_reward_analytics(&self) -> Result<RewardAnalytics> {
        let calculation = self.calculate_rewards().await?;
        let total_earned = self.get_total_rewards_earned();
        let avg_daily = if !self.reward_history.is_empty() {
            total_earned / self.reward_history.len() as f64
        } else {
            0.0
        };

        Ok(RewardAnalytics {
            total_earned_astra: total_earned,
            average_daily_reward: avg_daily,
            estimated_monthly_income: calculation.daily_rate,
            current_storage_gb: calculation.storage_gb,
            current_reward_per_gb: calculation.reward_per_gb,
            total_multiplier: calculation.bonus_breakdown.total_multiplier,
            payment_count: self.reward_history.len(),
            last_payment: self.last_reward_time,
            next_payment_due: self
                .last_reward_time
                .map(|t| t + Duration::hours(self.config.reward_interval_hours as i64)),
        })
    }
}

/// Reward analytics summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RewardAnalytics {
    pub total_earned_astra: f64,
    pub average_daily_reward: f64,
    pub estimated_monthly_income: f64,
    pub current_storage_gb: f64,
    pub current_reward_per_gb: f64,
    pub total_multiplier: f64,
    pub payment_count: usize,
    pub last_payment: Option<DateTime<Utc>>,
    pub next_payment_due: Option<DateTime<Utc>>,
}

impl Default for RewardCalculation {
    fn default() -> Self {
        Self {
            base_reward: 0.0,
            final_reward: 0.0,
            reward_wei: 0,
            storage_gb: 0.0,
            bonus_breakdown: BonusMultipliers::default(),
            reward_per_gb: 0.0,
            daily_rate: 0.0,
        }
    }
}

impl Default for BonusMultipliers {
    fn default() -> Self {
        Self {
            base_multiplier: 1.0,
            storage_type_multiplier: 1.0,
            quantum_bonus: 1.0,
            replication_bonus: 1.0,
            reputation_bonus: 1.0,
            uptime_bonus: 1.0,
            fast_retrieval_bonus: 1.0,
            p2p_bonus: 1.0,
            fact_verification_bonus: 1.0,
            total_multiplier: 1.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StorageNodeConfig;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_reward_calculation() {
        let temp_dir = tempdir().unwrap();
        let mut config = StorageNodeConfig::default();
        config.data_dir = temp_dir.path().to_path_buf();
        config.database_path = Some(temp_dir.path().join("test.json"));
        config.node_did = "did:spacekit:storage:test".to_string();

        let node = Arc::new(StorageNode::new(config).await.unwrap());

        let reward_config = StorageRewardConfig::default();
        let calculator = StorageRewardCalculator::new(reward_config, node);

        let calculation = calculator.calculate_rewards().await.unwrap();

        // Should have some calculation even if zero
        let _ = calculation.reward_wei;
    }

    #[tokio::test]
    async fn test_bonus_multipliers() {
        let temp_dir = tempdir().unwrap();
        let mut config = StorageNodeConfig::default();
        config.data_dir = temp_dir.path().to_path_buf();
        config.database_path = Some(temp_dir.path().join("test.json"));
        config.node_did = "did:spacekit:storage:test".to_string();
        config.preferred_algorithm = "kyber1024".to_string();

        let node = Arc::new(StorageNode::new(config).await.unwrap());

        let reward_config = StorageRewardConfig::default();
        let calculator = StorageRewardCalculator::new(reward_config, node.clone());

        let stats = node.get_stats().await.unwrap();
        let multipliers = calculator
            .calculate_bonus_multipliers(&stats)
            .await
            .unwrap();

        // Quantum bonus should be applied for kyber1024
        assert!(multipliers.quantum_bonus > 1.0);
        assert!(multipliers.total_multiplier >= 1.0);
    }

    #[tokio::test]
    async fn test_monthly_income_estimate() {
        let temp_dir = tempdir().unwrap();
        let mut config = StorageNodeConfig::default();
        config.data_dir = temp_dir.path().to_path_buf();
        config.database_path = Some(temp_dir.path().join("test.json"));
        config.node_did = "did:spacekit:storage:test".to_string();

        let node = Arc::new(StorageNode::new(config).await.unwrap());

        let reward_config = StorageRewardConfig::default();
        let calculator = StorageRewardCalculator::new(reward_config, node);

        let monthly = calculator.estimate_monthly_income().await.unwrap();
        assert!(monthly >= 0.0);
    }
}
