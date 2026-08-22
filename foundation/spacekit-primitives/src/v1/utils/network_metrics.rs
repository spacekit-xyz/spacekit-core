use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

/// Network metrics collector for dynamic fee management
#[derive(Debug, Clone)]
pub struct NetworkMetricsCollector {
    /// Current network metrics
    metrics: Arc<RwLock<NetworkMetrics>>,
    /// Historical metrics for trend analysis
    history: Arc<RwLock<Vec<NetworkSnapshot>>>,
    /// Service-specific metrics
    service_metrics: Arc<RwLock<HashMap<String, ServiceMetrics>>>,
    /// Configuration for metrics collection
    config: MetricsConfig,
}

/// Current network utilization metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkMetrics {
    /// Storage metrics
    pub total_storage_capacity: u64,
    pub used_storage_capacity: u64,

    /// Compute metrics
    pub total_compute_capacity: u64,
    pub used_compute_capacity: u64,

    /// Bandwidth metrics
    pub total_bandwidth_capacity: u64,
    pub used_bandwidth_capacity: u64,

    /// Network health metrics
    pub active_nodes: u64,
    pub total_transactions_24h: u64,
    pub avg_response_time_ms: u64,
    pub network_reliability_score: f64,

    /// Timestamp of last update
    pub last_updated: DateTime<Utc>,
}

/// Historical snapshot of network metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkSnapshot {
    pub timestamp: DateTime<Utc>,
    pub metrics: NetworkMetrics,
    pub utilization_rates: UtilizationRates,
}

/// Calculated utilization rates
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UtilizationRates {
    pub storage_utilization: f64,   // 0.0 to 1.0
    pub compute_utilization: f64,   // 0.0 to 1.0
    pub bandwidth_utilization: f64, // 0.0 to 1.0
    pub overall_utilization: f64,   // 0.0 to 1.0
}

/// Service-specific metrics for fee calculation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceMetrics {
    pub service_type: String,
    pub request_count_24h: u64,
    pub avg_request_size: u64,
    pub success_rate: f64,
    pub avg_processing_time_ms: u64,
    pub peak_utilization_24h: f64,
    pub current_queue_size: u64,
}

/// Configuration for metrics collection
#[derive(Debug, Clone)]
pub struct MetricsConfig {
    pub collection_interval: Duration,
    pub history_retention_hours: u64,
    pub utilization_weights: UtilizationWeights,
    pub alert_thresholds: AlertThresholds,
}

/// Weights for calculating overall utilization
#[derive(Debug, Clone)]
pub struct UtilizationWeights {
    pub storage_weight: f64,
    pub compute_weight: f64,
    pub bandwidth_weight: f64,
}

/// Thresholds for utilization alerts
#[derive(Debug, Clone)]
pub struct AlertThresholds {
    pub warning_threshold: f64,   // 0.7 = 70%
    pub critical_threshold: f64,  // 0.9 = 90%
    pub emergency_threshold: f64, // 0.95 = 95%
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            collection_interval: Duration::from_secs(300), // 5 minutes
            history_retention_hours: 168,                  // 1 week
            utilization_weights: UtilizationWeights {
                storage_weight: 0.4,
                compute_weight: 0.35,
                bandwidth_weight: 0.25,
            },
            alert_thresholds: AlertThresholds {
                warning_threshold: 0.7,
                critical_threshold: 0.9,
                emergency_threshold: 0.95,
            },
        }
    }
}

impl NetworkMetricsCollector {
    /// Create a new network metrics collector
    pub fn new(config: Option<MetricsConfig>) -> Self {
        let config = config.unwrap_or_default();

        Self {
            metrics: Arc::new(RwLock::new(NetworkMetrics::default())),
            history: Arc::new(RwLock::new(Vec::new())),
            service_metrics: Arc::new(RwLock::new(HashMap::new())),
            config,
        }
    }

    /// Start the metrics collection loop
    pub async fn start_collection(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut interval = tokio::time::interval(self.config.collection_interval);

        loop {
            interval.tick().await;

            if let Err(e) = self.collect_metrics().await {
                log::error!("Failed to collect network metrics: {}", e);
            }

            if let Err(e) = self.cleanup_old_history().await {
                log::error!("Failed to cleanup old metrics: {}", e);
            }
        }
    }

    /// Collect current network metrics
    pub async fn collect_metrics(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Collect storage metrics
        let storage_metrics = self.collect_storage_metrics().await?;

        // Collect compute metrics
        let compute_metrics = self.collect_compute_metrics().await?;

        // Collect bandwidth metrics
        let bandwidth_metrics = self.collect_bandwidth_metrics().await?;

        // Collect network health metrics
        let network_health = self.collect_network_health().await?;

        // Update current metrics
        let new_metrics = NetworkMetrics {
            total_storage_capacity: storage_metrics.total_capacity,
            used_storage_capacity: storage_metrics.used_capacity,
            total_compute_capacity: compute_metrics.total_capacity,
            used_compute_capacity: compute_metrics.used_capacity,
            total_bandwidth_capacity: bandwidth_metrics.total_capacity,
            used_bandwidth_capacity: bandwidth_metrics.used_capacity,
            active_nodes: network_health.active_nodes,
            total_transactions_24h: network_health.transactions_24h,
            avg_response_time_ms: network_health.avg_response_time_ms,
            network_reliability_score: network_health.reliability_score,
            last_updated: Utc::now(),
        };

        // Calculate utilization rates
        let utilization_rates = self.calculate_utilization_rates(&new_metrics);

        // Update current metrics
        {
            let mut metrics = self.metrics.write().await;
            *metrics = new_metrics.clone();
        }

        // Store historical snapshot
        self.store_snapshot(new_metrics, utilization_rates.clone())
            .await?;

        // Check for alerts
        self.check_utilization_alerts(&utilization_rates).await;

        Ok(())
    }

    /// Calculate utilization rates from raw metrics
    pub fn calculate_utilization_rates(&self, metrics: &NetworkMetrics) -> UtilizationRates {
        let storage_util = if metrics.total_storage_capacity > 0 {
            metrics.used_storage_capacity as f64 / metrics.total_storage_capacity as f64
        } else {
            0.0
        };

        let compute_util = if metrics.total_compute_capacity > 0 {
            metrics.used_compute_capacity as f64 / metrics.total_compute_capacity as f64
        } else {
            0.0
        };

        let bandwidth_util = if metrics.total_bandwidth_capacity > 0 {
            metrics.used_bandwidth_capacity as f64 / metrics.total_bandwidth_capacity as f64
        } else {
            0.0
        };

        // Calculate weighted overall utilization
        let overall_util = storage_util * self.config.utilization_weights.storage_weight
            + compute_util * self.config.utilization_weights.compute_weight
            + bandwidth_util * self.config.utilization_weights.bandwidth_weight;

        UtilizationRates {
            storage_utilization: storage_util.min(1.0),
            compute_utilization: compute_util.min(1.0),
            bandwidth_utilization: bandwidth_util.min(1.0),
            overall_utilization: overall_util.min(1.0),
        }
    }

    /// Get current network metrics
    pub async fn get_current_metrics(&self) -> NetworkMetrics {
        self.metrics.read().await.clone()
    }

    /// Get current utilization rates
    pub async fn get_current_utilization(&self) -> UtilizationRates {
        let metrics = self.get_current_metrics().await;
        self.calculate_utilization_rates(&metrics)
    }

    /// Get historical metrics for trend analysis
    pub async fn get_historical_metrics(&self, hours: u64) -> Vec<NetworkSnapshot> {
        let history = self.history.read().await;
        let cutoff = Utc::now() - chrono::Duration::hours(hours as i64);

        history
            .iter()
            .filter(|snapshot| snapshot.timestamp > cutoff)
            .cloned()
            .collect()
    }

    /// Update service-specific metrics
    pub async fn update_service_metrics(&self, service_type: String, metrics: ServiceMetrics) {
        let mut service_metrics = self.service_metrics.write().await;
        service_metrics.insert(service_type, metrics);
    }

    /// Get service-specific metrics
    pub async fn get_service_metrics(&self, service_type: &str) -> Option<ServiceMetrics> {
        let service_metrics = self.service_metrics.read().await;
        service_metrics.get(service_type).cloned()
    }

    /// Get all service metrics
    pub async fn get_all_service_metrics(&self) -> HashMap<String, ServiceMetrics> {
        self.service_metrics.read().await.clone()
    }

    /// Store a historical snapshot
    async fn store_snapshot(
        &self,
        metrics: NetworkMetrics,
        utilization_rates: UtilizationRates,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let snapshot = NetworkSnapshot {
            timestamp: Utc::now(),
            metrics,
            utilization_rates,
        };

        let mut history = self.history.write().await;
        history.push(snapshot);

        Ok(())
    }

    /// Cleanup old historical data
    async fn cleanup_old_history(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let cutoff =
            Utc::now() - chrono::Duration::hours(self.config.history_retention_hours as i64);

        let mut history = self.history.write().await;
        history.retain(|snapshot| snapshot.timestamp > cutoff);

        Ok(())
    }

    /// Check for utilization alerts
    async fn check_utilization_alerts(&self, utilization: &UtilizationRates) {
        let thresholds = &self.config.alert_thresholds;

        // Check storage utilization
        if utilization.storage_utilization >= thresholds.emergency_threshold {
            log::error!(
                "EMERGENCY: Storage utilization at {:.1}%",
                utilization.storage_utilization * 100.0
            );
        } else if utilization.storage_utilization >= thresholds.critical_threshold {
            log::warn!(
                "CRITICAL: Storage utilization at {:.1}%",
                utilization.storage_utilization * 100.0
            );
        } else if utilization.storage_utilization >= thresholds.warning_threshold {
            log::info!(
                "WARNING: Storage utilization at {:.1}%",
                utilization.storage_utilization * 100.0
            );
        }

        // Check compute utilization
        if utilization.compute_utilization >= thresholds.emergency_threshold {
            log::error!(
                "EMERGENCY: Compute utilization at {:.1}%",
                utilization.compute_utilization * 100.0
            );
        } else if utilization.compute_utilization >= thresholds.critical_threshold {
            log::warn!(
                "CRITICAL: Compute utilization at {:.1}%",
                utilization.compute_utilization * 100.0
            );
        } else if utilization.compute_utilization >= thresholds.warning_threshold {
            log::info!(
                "WARNING: Compute utilization at {:.1}%",
                utilization.compute_utilization * 100.0
            );
        }

        // Check overall utilization
        if utilization.overall_utilization >= thresholds.emergency_threshold {
            log::error!(
                "EMERGENCY: Overall network utilization at {:.1}%",
                utilization.overall_utilization * 100.0
            );
        }
    }

    /// Collect storage metrics from network nodes
    async fn collect_storage_metrics(
        &self,
    ) -> Result<StorageMetricsData, Box<dyn std::error::Error + Send + Sync>> {
        // In a real implementation, this would query storage nodes
        // For now, return mock data
        Ok(StorageMetricsData {
            total_capacity: 1000 * 1024 * 1024 * 1024 * 1024, // 1 PB
            used_capacity: 300 * 1024 * 1024 * 1024 * 1024,   // 300 TB
        })
    }

    /// Collect compute metrics from network nodes
    async fn collect_compute_metrics(
        &self,
    ) -> Result<ComputeMetricsData, Box<dyn std::error::Error + Send + Sync>> {
        // In a real implementation, this would query compute nodes
        Ok(ComputeMetricsData {
            total_capacity: 10000, // 10,000 CPU hours
            used_capacity: 3000,   // 3,000 CPU hours
        })
    }

    /// Collect bandwidth metrics from network nodes
    async fn collect_bandwidth_metrics(
        &self,
    ) -> Result<BandwidthMetricsData, Box<dyn std::error::Error + Send + Sync>> {
        // In a real implementation, this would query network infrastructure
        Ok(BandwidthMetricsData {
            total_capacity: 1000, // 1000 GB/s
            used_capacity: 400,   // 400 GB/s
        })
    }

    /// Collect network health metrics
    async fn collect_network_health(
        &self,
    ) -> Result<NetworkHealthData, Box<dyn std::error::Error + Send + Sync>> {
        // In a real implementation, this would query network status
        Ok(NetworkHealthData {
            active_nodes: 150,
            transactions_24h: 50000,
            avg_response_time_ms: 250,
            reliability_score: 0.98,
        })
    }
}

impl Default for NetworkMetrics {
    fn default() -> Self {
        Self {
            total_storage_capacity: 0,
            used_storage_capacity: 0,
            total_compute_capacity: 0,
            used_compute_capacity: 0,
            total_bandwidth_capacity: 0,
            used_bandwidth_capacity: 0,
            active_nodes: 0,
            total_transactions_24h: 0,
            avg_response_time_ms: 0,
            network_reliability_score: 0.0,
            last_updated: Utc::now(),
        }
    }
}

// Helper structs for metrics collection
#[derive(Debug)]
struct StorageMetricsData {
    total_capacity: u64,
    used_capacity: u64,
}

#[derive(Debug)]
struct ComputeMetricsData {
    total_capacity: u64,
    used_capacity: u64,
}

#[derive(Debug)]
struct BandwidthMetricsData {
    total_capacity: u64,
    used_capacity: u64,
}

#[derive(Debug)]
struct NetworkHealthData {
    active_nodes: u64,
    transactions_24h: u64,
    avg_response_time_ms: u64,
    reliability_score: f64,
}

/// Convert utilization rate to basis points (0-10000)
pub fn utilization_to_basis_points(utilization: f64) -> u64 {
    (utilization * 10000.0).round() as u64
}

/// Convert basis points to utilization rate (0.0-1.0)
pub fn basis_points_to_utilization(basis_points: u64) -> f64 {
    (basis_points as f64) / 10000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_utilization_conversion() {
        assert_eq!(utilization_to_basis_points(0.75), 7500);
        assert_eq!(basis_points_to_utilization(7500), 0.75);
    }

    #[tokio::test]
    async fn test_metrics_collector_creation() {
        let collector = NetworkMetricsCollector::new(None);
        let metrics = collector.get_current_metrics().await;
        assert_eq!(metrics.total_storage_capacity, 0);
    }

    #[test]
    fn test_utilization_calculation() {
        let collector = NetworkMetricsCollector::new(None);
        let metrics = NetworkMetrics {
            total_storage_capacity: 1000,
            used_storage_capacity: 750,
            total_compute_capacity: 100,
            used_compute_capacity: 50,
            total_bandwidth_capacity: 200,
            used_bandwidth_capacity: 100,
            ..Default::default()
        };

        let utilization = collector.calculate_utilization_rates(&metrics);
        assert_eq!(utilization.storage_utilization, 0.75);
        assert_eq!(utilization.compute_utilization, 0.5);
        assert_eq!(utilization.bandwidth_utilization, 0.5);

        // Overall: 0.75 * 0.4 + 0.5 * 0.35 + 0.5 * 0.25 = 0.6
        assert!((utilization.overall_utilization - 0.6).abs() < 0.001);
    }
}
