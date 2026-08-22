//! Production Metrics Module (Phase 5.3)
//!
//! Comprehensive production metrics collection and monitoring system for SWTCH Compute Node.
//! This module provides real-time metrics collection, Prometheus exports, network/storage
//! statistics, performance analytics, alerting, and cost analysis.
//!
//! Features:
//! - Central metrics aggregation and collection
//! - Prometheus metrics export
//! - Network and storage statistics aggregation
//! - Real-time performance analytics
//! - Threshold-based alerting system
//! - Cost analysis and tracking
//! - Grafana dashboard integration

use anyhow::Result;
use prometheus::{Counter, Encoder, Gauge, Histogram, Registry, TextEncoder};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{broadcast, RwLock};
use tokio::time::{interval, Instant};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

// Import our infrastructure
use crate::{
    cross_node_communication::CrossNodeCommunicationManager,
    p2p_service_discovery::P2PServiceDiscoveryManager,
    resource_monitor::{ResourceMetrics, ResourceMonitor},
    vpos::VPoSManager,
    ComputeNode, ComputeResult, ComputeTask, TaskStatus,
};

/// Production Metrics Manager - Central metrics collection and monitoring
pub struct ProductionMetricsManager {
    /// Metrics collection service
    metrics_collector: Arc<RwLock<MetricsCollector>>,

    /// Prometheus metrics exporter
    prometheus_exporter: Arc<PrometheusExporter>,

    /// Network and storage statistics aggregator
    network_stats_aggregator: Arc<RwLock<NetworkStatsAggregator>>,

    /// Performance analytics dashboard
    performance_analytics: Arc<RwLock<PerformanceAnalytics>>,

    /// Alerting system
    alerting_system: Arc<RwLock<AlertingSystem>>,

    /// Cost analysis module
    cost_analyzer: Arc<RwLock<CostAnalyzer>>,

    /// Configuration
    config: ProductionMetricsConfig,

    /// Event broadcaster
    event_broadcaster: broadcast::Sender<MetricsEvent>,
}

/// Configuration for production metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductionMetricsConfig {
    /// Enable metrics collection
    pub enabled: bool,

    /// Metrics collection interval
    pub collection_interval_seconds: u64,

    /// Prometheus metrics port
    pub prometheus_port: u16,

    /// Enable network statistics
    pub enable_network_stats: bool,

    /// Enable performance analytics
    pub enable_performance_analytics: bool,

    /// Enable alerting
    pub enable_alerting: bool,

    /// Enable cost analysis
    pub enable_cost_analysis: bool,

    /// Data retention period in days
    pub data_retention_days: u32,

    /// Alert thresholds
    pub alert_thresholds: AlertThresholds,
}

/// Alert thresholds configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertThresholds {
    /// CPU usage threshold (percentage)
    pub cpu_usage_threshold: f32,

    /// Memory usage threshold (percentage)
    pub memory_usage_threshold: f32,

    /// Network latency threshold (milliseconds)
    pub network_latency_threshold: f64,

    /// Error rate threshold (percentage)
    pub error_rate_threshold: f64,

    /// Cost threshold (dollars per hour)
    pub cost_threshold: f64,
}

/// Central metrics collection service
pub struct MetricsCollector {
    /// Resource monitor
    resource_monitor: ResourceMonitor,

    /// Collected metrics
    metrics: HashMap<String, MetricValue>,

    /// Metrics history
    metrics_history: Vec<MetricsSnapshot>,

    /// Last collection time
    last_collection: Instant,

    /// Collection statistics
    collection_stats: CollectionStats,
}

/// Prometheus metrics exporter
pub struct PrometheusExporter {
    /// Prometheus registry
    registry: Registry,

    /// Metrics definitions
    metrics: PrometheusMetrics,

    /// Export port
    port: u16,
}

/// Network and storage statistics aggregator
pub struct NetworkStatsAggregator {
    /// Network statistics
    network_stats: NetworkStatistics,

    /// Storage statistics
    storage_stats: StorageStatistics,

    /// Cross-node communication stats
    cross_node_stats: CrossNodeStatistics,

    /// P2P service discovery stats
    p2p_stats: P2PStatistics,

    /// Statistics history
    stats_history: Vec<NetworkStatsSnapshot>,
}

/// Performance analytics dashboard
pub struct PerformanceAnalytics {
    /// Performance metrics
    performance_metrics: PerformanceMetrics,

    /// Trend analysis
    trend_analysis: TrendAnalysis,

    /// Performance predictions
    performance_predictions: HashMap<String, PerformancePrediction>,

    /// Bottleneck detection
    bottleneck_detector: BottleneckDetector,

    /// Performance insights
    performance_insights: Vec<PerformanceInsight>,
}

/// Alerting system
pub struct AlertingSystem {
    /// Active alerts
    active_alerts: HashMap<String, Alert>,

    /// Alert history
    alert_history: Vec<AlertEvent>,

    /// Alert rules
    alert_rules: Vec<AlertRule>,

    /// Alert channels
    alert_channels: Vec<AlertChannel>,

    /// Alert statistics
    alert_stats: AlertStatistics,
}

/// Cost analysis module
pub struct CostAnalyzer {
    /// Cost metrics
    cost_metrics: CostMetrics,

    /// Cost breakdown
    cost_breakdown: CostBreakdown,

    /// Cost trends
    cost_trends: HashMap<String, Vec<CostDataPoint>>,

    /// Cost predictions
    cost_predictions: HashMap<String, CostPrediction>,

    /// Cost optimization recommendations
    optimization_recommendations: Vec<CostOptimizationRecommendation>,
}

/// Metrics event for broadcasting
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsEvent {
    pub event_type: String,
    pub timestamp: u64,
    pub data: serde_json::Value,
    pub severity: EventSeverity,
}

/// Event severity levels
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventSeverity {
    Info,
    Warning,
    Critical,
}

/// Metric value types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MetricValue {
    Counter(u64),
    Gauge(f64),
    Histogram(Vec<f64>),
    Summary {
        count: u64,
        sum: f64,
        quantiles: HashMap<String, f64>,
    },
}

/// Metrics snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsSnapshot {
    pub timestamp: u64,
    pub metrics: HashMap<String, MetricValue>,
}

/// Collection statistics
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct CollectionStats {
    pub total_collections: u64,
    pub successful_collections: u64,
    pub failed_collections: u64,
    pub average_collection_time_ms: f64,
    pub last_collection_time: u64,
}

/// Prometheus metrics definitions
pub struct PrometheusMetrics {
    // System metrics
    pub cpu_usage: Gauge,
    pub memory_usage: Gauge,
    pub disk_usage: Gauge,
    pub network_io: Counter,

    // Compute metrics
    pub tasks_total: Counter,
    pub tasks_completed: Counter,
    pub tasks_failed: Counter,
    pub execution_time: Histogram,
    pub compute_cost: Counter,

    // Network metrics
    pub network_latency: Histogram,
    pub network_throughput: Gauge,
    pub active_connections: Gauge,
    pub p2p_peers: Gauge,

    // Storage metrics
    pub storage_operations: Counter,
    pub storage_latency: Histogram,
    pub storage_utilization: Gauge,
    pub storage_cost: Counter,

    // VPoS metrics
    pub vpos_proofs_generated: Counter,
    pub vpos_verification_time: Histogram,
    pub reputation_score: Gauge,
    // Note: Custom metrics would need a concrete enum type instead of dyn Metric
}

/// Network statistics
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct NetworkStatistics {
    pub total_connections: u64,
    pub active_connections: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub packets_sent: u64,
    pub packets_received: u64,
    pub average_latency_ms: f64,
    pub connection_errors: u64,
    pub last_updated: u64,
}

/// Storage statistics
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct StorageStatistics {
    pub total_operations: u64,
    pub read_operations: u64,
    pub write_operations: u64,
    pub bytes_read: u64,
    pub bytes_written: u64,
    pub storage_used_bytes: u64,
    pub storage_available_bytes: u64,
    pub average_read_latency_ms: f64,
    pub average_write_latency_ms: f64,
    pub storage_errors: u64,
    pub last_updated: u64,
}

/// Cross-node communication statistics
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct CrossNodeStatistics {
    pub total_messages_sent: u64,
    pub total_messages_received: u64,
    pub message_latency_ms: f64,
    pub failed_messages: u64,
    pub active_sessions: u64,
    pub load_balancing_decisions: u64,
    pub failover_events: u64,
    pub last_updated: u64,
}

/// P2P service discovery statistics
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct P2PStatistics {
    pub discovered_services: u64,
    pub service_announcements: u64,
    pub service_queries: u64,
    pub reputation_updates: u64,
    pub health_checks: u64,
    pub service_failures: u64,
    pub last_updated: u64,
}

/// Network statistics snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkStatsSnapshot {
    pub timestamp: u64,
    pub network_stats: NetworkStatistics,
    pub storage_stats: StorageStatistics,
    pub cross_node_stats: CrossNodeStatistics,
    pub p2p_stats: P2PStatistics,
}

/// Performance metrics
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    pub throughput_ops_per_sec: f64,
    pub latency_p50_ms: f64,
    pub latency_p95_ms: f64,
    pub latency_p99_ms: f64,
    pub error_rate_percent: f64,
    pub availability_percent: f64,
    pub resource_utilization_percent: f64,
    pub cost_per_operation: f64,
    pub last_updated: u64,
}

/// Trend analysis
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct TrendAnalysis {
    pub performance_trend: String, // "improving", "stable", "degrading"
    pub throughput_trend: f64,
    pub latency_trend: f64,
    pub error_rate_trend: f64,
    pub cost_trend: f64,
    pub confidence_score: f64,
    pub prediction_accuracy: f64,
    pub last_updated: u64,
}

/// Performance prediction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformancePrediction {
    pub metric: String,
    pub predicted_value: f64,
    pub confidence_interval: (f64, f64),
    pub time_horizon_hours: u64,
    pub prediction_method: String,
    pub created_at: u64,
}

/// Bottleneck detector
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct BottleneckDetector {
    pub detected_bottlenecks: Vec<Bottleneck>,
    pub bottleneck_score: f64,
    pub primary_bottleneck: Option<String>,
    pub bottleneck_impact: f64,
    pub last_analysis: u64,
}

/// Performance bottleneck
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bottleneck {
    pub component: String,
    pub bottleneck_type: String,
    pub severity: f64,
    pub impact_description: String,
    pub recommendation: String,
    pub detected_at: u64,
}

/// Performance insight
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceInsight {
    pub insight_type: String,
    pub title: String,
    pub description: String,
    pub impact: f64,
    pub recommendation: String,
    pub priority: String,
    pub created_at: u64,
}

/// Alert definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alert {
    pub id: String,
    pub rule_id: String,
    pub severity: AlertSeverity,
    pub title: String,
    pub description: String,
    pub metric_name: String,
    pub current_value: f64,
    pub threshold: f64,
    pub created_at: u64,
    pub last_triggered: u64,
    pub times_triggered: u64,
    pub status: AlertStatus,
}

/// Alert severity levels
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AlertSeverity {
    Info,
    Warning,
    Critical,
}

/// Alert status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AlertStatus {
    Active,
    Acknowledged,
    Resolved,
}

/// Alert event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertEvent {
    pub alert_id: String,
    pub event_type: String, // "triggered", "resolved", "acknowledged"
    pub timestamp: u64,
    pub message: String,
    pub value: f64,
}

/// Alert rule
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertRule {
    pub id: String,
    pub name: String,
    pub metric_name: String,
    pub condition: String, // "greater_than", "less_than", "equals"
    pub threshold: f64,
    pub duration_seconds: u64,
    pub severity: AlertSeverity,
    pub enabled: bool,
    pub channels: Vec<String>,
}

/// Alert channel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertChannel {
    pub id: String,
    pub name: String,
    pub channel_type: String, // "email", "webhook", "slack"
    pub config: serde_json::Value,
    pub enabled: bool,
}

/// Alert statistics
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct AlertStatistics {
    pub total_alerts: u64,
    pub active_alerts: u64,
    pub resolved_alerts: u64,
    pub alert_rate_per_hour: f64,
    pub mean_time_to_resolution: f64,
    pub false_positive_rate: f64,
    pub last_updated: u64,
}

/// Cost metrics
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct CostMetrics {
    pub total_cost_usd: f64,
    pub compute_cost_usd: f64,
    pub storage_cost_usd: f64,
    pub network_cost_usd: f64,
    pub energy_cost_usd: f64,
    pub cost_per_hour: f64,
    pub cost_per_operation: f64,
    pub cost_efficiency_score: f64,
    pub last_updated: u64,
}

/// Cost breakdown
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct CostBreakdown {
    pub compute_percentage: f64,
    pub storage_percentage: f64,
    pub network_percentage: f64,
    pub energy_percentage: f64,
    pub overhead_percentage: f64,
    pub top_cost_drivers: Vec<CostDriver>,
    pub last_updated: u64,
}

/// Cost driver
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostDriver {
    pub component: String,
    pub cost_usd: f64,
    pub percentage: f64,
    pub trend: String,
    pub optimization_potential: f64,
}

/// Cost data point
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostDataPoint {
    pub timestamp: u64,
    pub cost_usd: f64,
    pub usage_units: f64,
    pub efficiency_score: f64,
}

/// Cost prediction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostPrediction {
    pub predicted_cost_usd: f64,
    pub confidence_interval: (f64, f64),
    pub time_horizon_hours: u64,
    pub factors: Vec<String>,
    pub prediction_method: String,
    pub created_at: u64,
}

/// Cost optimization recommendation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostOptimizationRecommendation {
    pub id: String,
    pub title: String,
    pub description: String,
    pub component: String,
    pub potential_savings_usd: f64,
    pub implementation_effort: String,
    pub priority: String,
    pub created_at: u64,
}

impl Default for ProductionMetricsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            collection_interval_seconds: 30,
            prometheus_port: 9190, // Changed from 9090 to avoid conflicts
            enable_network_stats: true,
            enable_performance_analytics: true,
            enable_alerting: true,
            enable_cost_analysis: true,
            data_retention_days: 30,
            alert_thresholds: AlertThresholds {
                cpu_usage_threshold: 80.0,
                memory_usage_threshold: 85.0,
                network_latency_threshold: 1000.0,
                error_rate_threshold: 5.0,
                cost_threshold: 10.0,
            },
        }
    }
}

impl ProductionMetricsManager {
    /// Create a new production metrics manager
    pub async fn new(config: ProductionMetricsConfig) -> Result<Self> {
        info!("🚀 Initializing Production Metrics Manager - Phase 5.3");

        // Create event broadcaster
        let (event_broadcaster, _) = broadcast::channel(1000);

        // Initialize metrics collector
        let metrics_collector = Arc::new(RwLock::new(MetricsCollector::new().await?));

        // Initialize Prometheus exporter
        let prometheus_exporter = Arc::new(PrometheusExporter::new(config.prometheus_port)?);

        // Initialize network statistics aggregator
        let network_stats_aggregator = Arc::new(RwLock::new(NetworkStatsAggregator::new()));

        // Initialize performance analytics
        let performance_analytics = Arc::new(RwLock::new(PerformanceAnalytics::new()));

        // Initialize alerting system
        let alerting_system = Arc::new(RwLock::new(AlertingSystem::new(&config.alert_thresholds)));

        // Initialize cost analyzer
        let cost_analyzer = Arc::new(RwLock::new(CostAnalyzer::new()));

        Ok(Self {
            metrics_collector,
            prometheus_exporter,
            network_stats_aggregator,
            performance_analytics,
            alerting_system,
            cost_analyzer,
            config,
            event_broadcaster,
        })
    }

    /// Start the production metrics system
    pub async fn start(&self) -> Result<()> {
        info!("🌟 Starting Production Metrics System - Phase 5.3");

        if !self.config.enabled {
            warn!("Production metrics disabled in configuration");
            return Ok(());
        }

        // Start metrics collection
        self.start_metrics_collection().await?;

        // Start Prometheus exporter
        if self.config.prometheus_port > 0 {
            self.start_prometheus_exporter().await?;
        }

        // Start network statistics aggregation
        if self.config.enable_network_stats {
            self.start_network_stats_aggregation().await?;
        }

        // Start performance analytics
        if self.config.enable_performance_analytics {
            self.start_performance_analytics().await?;
        }

        // Start alerting system
        if self.config.enable_alerting {
            self.start_alerting_system().await?;
        }

        // Start cost analysis
        if self.config.enable_cost_analysis {
            self.start_cost_analysis().await?;
        }

        info!("✅ Production Metrics System started successfully");
        Ok(())
    }

    /// Start metrics collection
    async fn start_metrics_collection(&self) -> Result<()> {
        info!("📊 Starting metrics collection service");

        let metrics_collector = Arc::clone(&self.metrics_collector);
        let interval_duration = Duration::from_secs(self.config.collection_interval_seconds);
        let event_broadcaster = self.event_broadcaster.clone();

        tokio::spawn(async move {
            let mut interval = interval(interval_duration);

            loop {
                interval.tick().await;

                let mut collector = metrics_collector.write().await;
                if let Err(e) = collector.collect_metrics().await {
                    error!("Failed to collect metrics: {}", e);

                    // Broadcast error event
                    let event = MetricsEvent {
                        event_type: "metrics_collection_error".to_string(),
                        timestamp: SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap()
                            .as_secs(),
                        data: serde_json::json!({ "error": e.to_string() }),
                        severity: EventSeverity::Warning,
                    };

                    let _ = event_broadcaster.send(event);
                }
            }
        });

        Ok(())
    }

    /// Start Prometheus exporter
    async fn start_prometheus_exporter(&self) -> Result<()> {
        info!(
            "🔧 Starting Prometheus metrics exporter on port {}",
            self.config.prometheus_port
        );

        let exporter = Arc::clone(&self.prometheus_exporter);
        let port = self.config.prometheus_port;

        tokio::spawn(async move {
            if let Err(e) = exporter.start_server(port).await {
                error!("Failed to start Prometheus exporter: {}", e);
            }
        });

        Ok(())
    }

    /// Start network statistics aggregation
    async fn start_network_stats_aggregation(&self) -> Result<()> {
        info!("🌐 Starting network statistics aggregation");

        let network_stats_aggregator = Arc::clone(&self.network_stats_aggregator);
        let interval_duration = Duration::from_secs(self.config.collection_interval_seconds);

        tokio::spawn(async move {
            let mut interval = interval(interval_duration);

            loop {
                interval.tick().await;

                let mut aggregator = network_stats_aggregator.write().await;
                if let Err(e) = aggregator.collect_network_stats().await {
                    error!("Failed to collect network statistics: {}", e);
                }
            }
        });

        Ok(())
    }

    /// Start performance analytics
    async fn start_performance_analytics(&self) -> Result<()> {
        info!("⚡ Starting performance analytics");

        let performance_analytics = Arc::clone(&self.performance_analytics);
        let interval_duration = Duration::from_secs(self.config.collection_interval_seconds * 2);

        tokio::spawn(async move {
            let mut interval = interval(interval_duration);

            loop {
                interval.tick().await;

                let mut analytics = performance_analytics.write().await;
                if let Err(e) = analytics.analyze_performance().await {
                    error!("Failed to analyze performance: {}", e);
                }
            }
        });

        Ok(())
    }

    /// Start alerting system
    async fn start_alerting_system(&self) -> Result<()> {
        info!("🚨 Starting alerting system");

        let alerting_system = Arc::clone(&self.alerting_system);
        let interval_duration = Duration::from_secs(self.config.collection_interval_seconds);

        tokio::spawn(async move {
            let mut interval = interval(interval_duration);

            loop {
                interval.tick().await;

                let mut alerts = alerting_system.write().await;
                if let Err(e) = alerts.check_alerts().await {
                    error!("Failed to check alerts: {}", e);
                }
            }
        });

        Ok(())
    }

    /// Start cost analysis
    async fn start_cost_analysis(&self) -> Result<()> {
        info!("💰 Starting cost analysis");

        let cost_analyzer = Arc::clone(&self.cost_analyzer);
        let interval_duration = Duration::from_secs(self.config.collection_interval_seconds * 3);

        tokio::spawn(async move {
            let mut interval = interval(interval_duration);

            loop {
                interval.tick().await;

                let mut analyzer = cost_analyzer.write().await;
                if let Err(e) = analyzer.analyze_costs().await {
                    error!("Failed to analyze costs: {}", e);
                }
            }
        });

        Ok(())
    }

    /// Get production metrics summary
    pub async fn get_metrics_summary(&self) -> Result<ProductionMetricsSummary> {
        let metrics_collector = self.metrics_collector.read().await;
        let network_stats_aggregator = self.network_stats_aggregator.read().await;
        let performance_analytics = self.performance_analytics.read().await;
        let alerting_system = self.alerting_system.read().await;
        let cost_analyzer = self.cost_analyzer.read().await;

        Ok(ProductionMetricsSummary {
            collection_stats: metrics_collector.collection_stats.clone(),
            network_stats: network_stats_aggregator.network_stats.clone(),
            storage_stats: network_stats_aggregator.storage_stats.clone(),
            performance_metrics: performance_analytics.performance_metrics.clone(),
            active_alerts: alerting_system.active_alerts.len() as u64,
            cost_metrics: cost_analyzer.cost_metrics.clone(),
            last_updated: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        })
    }

    /// Subscribe to metrics events
    pub fn subscribe_to_events(&self) -> broadcast::Receiver<MetricsEvent> {
        self.event_broadcaster.subscribe()
    }
}

/// Production metrics summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductionMetricsSummary {
    pub collection_stats: CollectionStats,
    pub network_stats: NetworkStatistics,
    pub storage_stats: StorageStatistics,
    pub performance_metrics: PerformanceMetrics,
    pub active_alerts: u64,
    pub cost_metrics: CostMetrics,
    pub last_updated: u64,
}

impl MetricsCollector {
    /// Create a new metrics collector
    pub async fn new() -> Result<Self> {
        Ok(Self {
            resource_monitor: ResourceMonitor::new()?,
            metrics: HashMap::new(),
            metrics_history: Vec::new(),
            last_collection: Instant::now(),
            collection_stats: CollectionStats::default(),
        })
    }

    /// Collect all metrics
    pub async fn collect_metrics(&mut self) -> Result<()> {
        let start_time = Instant::now();

        // Collect system metrics
        let resource_metrics = self.resource_monitor.get_current_metrics().await?;

        // Update metrics
        self.metrics.insert(
            "cpu_usage_percent".to_string(),
            MetricValue::Gauge(resource_metrics.cpu_usage_percent as f64),
        );
        self.metrics.insert(
            "memory_usage_mb".to_string(),
            MetricValue::Gauge(resource_metrics.memory_usage_mb as f64),
        );
        self.metrics.insert(
            "memory_peak_mb".to_string(),
            MetricValue::Gauge(resource_metrics.memory_peak_mb as f64),
        );
        self.metrics.insert(
            "compute_units_used".to_string(),
            MetricValue::Counter(resource_metrics.compute_units_used),
        );
        self.metrics.insert(
            "energy_consumed_kwh".to_string(),
            MetricValue::Gauge(resource_metrics.energy_consumed_kwh),
        );

        // Create snapshot
        let snapshot = MetricsSnapshot {
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            metrics: self.metrics.clone(),
        };

        self.metrics_history.push(snapshot);

        // Limit history size
        if self.metrics_history.len() > 1000 {
            self.metrics_history.remove(0);
        }

        // Update collection stats
        let collection_time = start_time.elapsed();
        self.collection_stats.total_collections += 1;
        self.collection_stats.successful_collections += 1;
        self.collection_stats.average_collection_time_ms =
            (self.collection_stats.average_collection_time_ms + collection_time.as_millis() as f64)
                / 2.0;
        self.collection_stats.last_collection_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        self.last_collection = Instant::now();

        debug!("Collected metrics in {}ms", collection_time.as_millis());
        Ok(())
    }

    /// Get metric value
    pub fn get_metric(&self, name: &str) -> Option<&MetricValue> {
        self.metrics.get(name)
    }

    /// Get all metrics
    pub fn get_all_metrics(&self) -> &HashMap<String, MetricValue> {
        &self.metrics
    }

    /// Get metrics history
    pub fn get_metrics_history(&self) -> &Vec<MetricsSnapshot> {
        &self.metrics_history
    }
}

impl PrometheusExporter {
    /// Create a new Prometheus exporter
    pub fn new(port: u16) -> Result<Self> {
        let registry = Registry::new();

        // Create Prometheus metrics
        let cpu_usage = Gauge::new("swtch_cpu_usage_percent", "CPU usage percentage")?;
        let memory_usage = Gauge::new("swtch_memory_usage_percent", "Memory usage percentage")?;
        let disk_usage = Gauge::new("swtch_disk_usage_percent", "Disk usage percentage")?;
        let network_io = Counter::new("swtch_network_io_bytes_total", "Network I/O bytes")?;

        let tasks_total = Counter::new("swtch_tasks_total", "Total tasks submitted")?;
        let tasks_completed = Counter::new("swtch_tasks_completed_total", "Total tasks completed")?;
        let tasks_failed = Counter::new("swtch_tasks_failed_total", "Total tasks failed")?;
        let execution_time = Histogram::with_opts(
            prometheus::HistogramOpts::new("swtch_execution_time_seconds", "Task execution time")
                .buckets(vec![0.1, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0, 60.0]),
        )?;
        let compute_cost = Counter::new("swtch_compute_cost_usd_total", "Total compute cost")?;

        let network_latency = Histogram::with_opts(
            prometheus::HistogramOpts::new("swtch_network_latency_seconds", "Network latency")
                .buckets(vec![0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0]),
        )?;
        let network_throughput = Gauge::new("swtch_network_throughput_bps", "Network throughput")?;
        let active_connections =
            Gauge::new("swtch_active_connections", "Active network connections")?;
        let p2p_peers = Gauge::new("swtch_p2p_peers", "P2P peers")?;

        let storage_operations =
            Counter::new("swtch_storage_operations_total", "Storage operations")?;
        let storage_latency = Histogram::with_opts(
            prometheus::HistogramOpts::new(
                "swtch_storage_latency_seconds",
                "Storage operation latency",
            )
            .buckets(vec![0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0]),
        )?;
        let storage_utilization =
            Gauge::new("swtch_storage_utilization_percent", "Storage utilization")?;
        let storage_cost = Counter::new("swtch_storage_cost_usd_total", "Total storage cost")?;

        let vpos_proofs_generated =
            Counter::new("swtch_vpos_proofs_generated_total", "VPoS proofs generated")?;
        let vpos_verification_time = Histogram::with_opts(
            prometheus::HistogramOpts::new(
                "swtch_vpos_verification_time_seconds",
                "VPoS verification time",
            )
            .buckets(vec![0.01, 0.05, 0.1, 0.25, 0.5, 1.0, 2.0, 5.0]),
        )?;
        let reputation_score = Gauge::new("swtch_reputation_score", "Node reputation score")?;

        // Register metrics
        registry.register(Box::new(cpu_usage.clone()))?;
        registry.register(Box::new(memory_usage.clone()))?;
        registry.register(Box::new(disk_usage.clone()))?;
        registry.register(Box::new(network_io.clone()))?;
        registry.register(Box::new(tasks_total.clone()))?;
        registry.register(Box::new(tasks_completed.clone()))?;
        registry.register(Box::new(tasks_failed.clone()))?;
        registry.register(Box::new(execution_time.clone()))?;
        registry.register(Box::new(compute_cost.clone()))?;
        registry.register(Box::new(network_latency.clone()))?;
        registry.register(Box::new(network_throughput.clone()))?;
        registry.register(Box::new(active_connections.clone()))?;
        registry.register(Box::new(p2p_peers.clone()))?;
        registry.register(Box::new(storage_operations.clone()))?;
        registry.register(Box::new(storage_latency.clone()))?;
        registry.register(Box::new(storage_utilization.clone()))?;
        registry.register(Box::new(storage_cost.clone()))?;
        registry.register(Box::new(vpos_proofs_generated.clone()))?;
        registry.register(Box::new(vpos_verification_time.clone()))?;
        registry.register(Box::new(reputation_score.clone()))?;

        let metrics = PrometheusMetrics {
            cpu_usage,
            memory_usage,
            disk_usage,
            network_io,
            tasks_total,
            tasks_completed,
            tasks_failed,
            execution_time,
            compute_cost,
            network_latency,
            network_throughput,
            active_connections,
            p2p_peers,
            storage_operations,
            storage_latency,
            storage_utilization,
            storage_cost,
            vpos_proofs_generated,
            vpos_verification_time,
            reputation_score,
            // custom_metrics removed due to object safety
        };

        Ok(Self {
            registry,
            metrics,
            port,
        })
    }

    /// Start Prometheus metrics server
    pub async fn start_server(&self, port: u16) -> Result<()> {
        use warp::Filter;

        let registry = self.registry.clone();
        let metrics_route = warp::path("metrics").and(warp::get()).map(move || {
            let encoder = TextEncoder::new();
            let metric_families = registry.gather();
            let mut buffer = Vec::new();
            encoder.encode(&metric_families, &mut buffer).unwrap();
            String::from_utf8(buffer).unwrap()
        });

        let health_route = warp::path("health").and(warp::get()).map(|| "OK");

        let routes = metrics_route.or(health_route);

        info!("Starting Prometheus metrics server on port {}", port);

        // Try to bind to the port, if it fails, try the next available port
        let mut actual_port = port;
        let mut attempts = 0;

        loop {
            match tokio::net::TcpListener::bind(("127.0.0.1", actual_port)).await {
                Ok(listener) => {
                    info!("✅ Prometheus server bound to port {}", actual_port);
                    drop(listener); // Release the listener so warp can bind
                    break;
                }
                Err(_) if attempts < 10 => {
                    actual_port += 1;
                    attempts += 1;
                    continue;
                }
                Err(e) => {
                    error!(
                        "Failed to bind Prometheus server after {} attempts: {}",
                        attempts, e
                    );
                    return Err(anyhow::anyhow!(
                        "Could not find available port for Prometheus server"
                    ));
                }
            }
        }

        warp::serve(routes)
            .run(([127, 0, 0, 1], actual_port)) // Changed from 0.0.0.0 to 127.0.0.1
            .await;

        Ok(())
    }

    /// Update metrics from collected data
    pub fn update_metrics(&self, metrics: &HashMap<String, MetricValue>) {
        for (name, value) in metrics {
            match value {
                MetricValue::Gauge(val) => match name.as_str() {
                    "cpu_usage_percent" => self.metrics.cpu_usage.set(*val),
                    "memory_usage_percent" => self.metrics.memory_usage.set(*val),
                    "network_throughput_bps" => self.metrics.network_throughput.set(*val),
                    "active_connections" => self.metrics.active_connections.set(*val),
                    "p2p_peers" => self.metrics.p2p_peers.set(*val),
                    "storage_utilization_percent" => self.metrics.storage_utilization.set(*val),
                    "reputation_score" => self.metrics.reputation_score.set(*val),
                    _ => {}
                },
                MetricValue::Counter(val) => match name.as_str() {
                    "tasks_total" => self.metrics.tasks_total.inc_by(*val as f64),
                    "tasks_completed" => self.metrics.tasks_completed.inc_by(*val as f64),
                    "tasks_failed" => self.metrics.tasks_failed.inc_by(*val as f64),
                    "compute_cost_usd" => self.metrics.compute_cost.inc_by(*val as f64),
                    "storage_operations" => self.metrics.storage_operations.inc_by(*val as f64),
                    "storage_cost_usd" => self.metrics.storage_cost.inc_by(*val as f64),
                    "vpos_proofs_generated" => {
                        self.metrics.vpos_proofs_generated.inc_by(*val as f64)
                    }
                    _ => {}
                },
                MetricValue::Histogram(vals) => match name.as_str() {
                    "execution_time_seconds" => {
                        for &val in vals {
                            self.metrics.execution_time.observe(val);
                        }
                    }
                    "network_latency_seconds" => {
                        for &val in vals {
                            self.metrics.network_latency.observe(val);
                        }
                    }
                    "storage_latency_seconds" => {
                        for &val in vals {
                            self.metrics.storage_latency.observe(val);
                        }
                    }
                    "vpos_verification_time_seconds" => {
                        for &val in vals {
                            self.metrics.vpos_verification_time.observe(val);
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }
}

impl NetworkStatsAggregator {
    /// Create a new network statistics aggregator
    pub fn new() -> Self {
        Self {
            network_stats: NetworkStatistics::default(),
            storage_stats: StorageStatistics::default(),
            cross_node_stats: CrossNodeStatistics::default(),
            p2p_stats: P2PStatistics::default(),
            stats_history: Vec::new(),
        }
    }

    /// Collect network statistics
    pub async fn collect_network_stats(&mut self) -> Result<()> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Collect network statistics (simplified - would integrate with actual network layer)
        self.network_stats.total_connections += 1;
        self.network_stats.active_connections = 5; // Example value
        self.network_stats.average_latency_ms = 50.0; // Example value
        self.network_stats.last_updated = timestamp;

        // Collect storage statistics
        self.storage_stats.total_operations += 1;
        self.storage_stats.average_read_latency_ms = 5.0; // Example value
        self.storage_stats.average_write_latency_ms = 10.0; // Example value
        self.storage_stats.last_updated = timestamp;

        // Collect cross-node statistics
        self.cross_node_stats.total_messages_sent += 1;
        self.cross_node_stats.message_latency_ms = 25.0; // Example value
        self.cross_node_stats.last_updated = timestamp;

        // Collect P2P statistics
        self.p2p_stats.discovered_services += 1;
        self.p2p_stats.health_checks += 1;
        self.p2p_stats.last_updated = timestamp;

        // Create snapshot
        let snapshot = NetworkStatsSnapshot {
            timestamp,
            network_stats: self.network_stats.clone(),
            storage_stats: self.storage_stats.clone(),
            cross_node_stats: self.cross_node_stats.clone(),
            p2p_stats: self.p2p_stats.clone(),
        };

        self.stats_history.push(snapshot);

        // Limit history size
        if self.stats_history.len() > 1000 {
            self.stats_history.remove(0);
        }

        debug!("Collected network statistics");
        Ok(())
    }

    /// Get network statistics
    pub fn get_network_stats(&self) -> &NetworkStatistics {
        &self.network_stats
    }

    /// Get storage statistics
    pub fn get_storage_stats(&self) -> &StorageStatistics {
        &self.storage_stats
    }

    /// Get statistics history
    pub fn get_stats_history(&self) -> &Vec<NetworkStatsSnapshot> {
        &self.stats_history
    }
}

impl PerformanceAnalytics {
    /// Create a new performance analytics system
    pub fn new() -> Self {
        Self {
            performance_metrics: PerformanceMetrics::default(),
            trend_analysis: TrendAnalysis::default(),
            performance_predictions: HashMap::new(),
            bottleneck_detector: BottleneckDetector::default(),
            performance_insights: Vec::new(),
        }
    }

    /// Analyze performance
    pub async fn analyze_performance(&mut self) -> Result<()> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Update performance metrics (simplified - would use actual data)
        self.performance_metrics.throughput_ops_per_sec = 100.0; // Example value
        self.performance_metrics.latency_p50_ms = 50.0; // Example value
        self.performance_metrics.latency_p95_ms = 150.0; // Example value
        self.performance_metrics.latency_p99_ms = 300.0; // Example value
        self.performance_metrics.error_rate_percent = 1.0; // Example value
        self.performance_metrics.availability_percent = 99.5; // Example value
        self.performance_metrics.resource_utilization_percent = 65.0; // Example value
        self.performance_metrics.cost_per_operation = 0.001; // Example value
        self.performance_metrics.last_updated = timestamp;

        // Analyze trends
        self.trend_analysis.performance_trend = "stable".to_string();
        self.trend_analysis.throughput_trend = 0.1; // Slight improvement
        self.trend_analysis.latency_trend = -0.05; // Slight improvement
        self.trend_analysis.error_rate_trend = 0.02; // Slight increase
        self.trend_analysis.cost_trend = 0.0; // Stable
        self.trend_analysis.confidence_score = 0.85;
        self.trend_analysis.prediction_accuracy = 0.92;
        self.trend_analysis.last_updated = timestamp;

        // Detect bottlenecks
        self.detect_bottlenecks().await?;

        // Generate insights
        self.generate_insights().await?;

        debug!("Analyzed performance");
        Ok(())
    }

    /// Detect performance bottlenecks
    async fn detect_bottlenecks(&mut self) -> Result<()> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Example bottleneck detection
        if self.performance_metrics.latency_p95_ms > 100.0 {
            let bottleneck = Bottleneck {
                component: "network".to_string(),
                bottleneck_type: "latency".to_string(),
                severity: 0.7,
                impact_description: "High network latency affecting response times".to_string(),
                recommendation: "Consider optimizing network configuration or upgrading bandwidth"
                    .to_string(),
                detected_at: timestamp,
            };

            self.bottleneck_detector
                .detected_bottlenecks
                .push(bottleneck);
            if self.bottleneck_detector.detected_bottlenecks.len() > 100 {
                self.bottleneck_detector.detected_bottlenecks.remove(0);
            }
        }

        // Update bottleneck detector
        self.bottleneck_detector.bottleneck_score = 0.3;
        self.bottleneck_detector.primary_bottleneck = Some("network".to_string());
        self.bottleneck_detector.bottleneck_impact = 0.15;
        self.bottleneck_detector.last_analysis = timestamp;

        Ok(())
    }

    /// Generate performance insights
    async fn generate_insights(&mut self) -> Result<()> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Example insight generation
        if self.performance_metrics.throughput_ops_per_sec > 95.0 {
            let insight = PerformanceInsight {
                insight_type: "optimization".to_string(),
                title: "High Throughput Performance".to_string(),
                description: "System is performing well with high throughput".to_string(),
                impact: 0.8,
                recommendation: "Maintain current configuration and monitor for consistency"
                    .to_string(),
                priority: "low".to_string(),
                created_at: timestamp,
            };

            self.performance_insights.push(insight);
        }

        // Limit insights history
        if self.performance_insights.len() > 100 {
            self.performance_insights.remove(0);
        }

        Ok(())
    }
}

impl AlertingSystem {
    /// Create a new alerting system
    pub fn new(thresholds: &AlertThresholds) -> Self {
        let mut alert_rules = Vec::new();

        // Create default alert rules
        alert_rules.push(AlertRule {
            id: "cpu_usage_high".to_string(),
            name: "High CPU Usage".to_string(),
            metric_name: "cpu_usage_percent".to_string(),
            condition: "greater_than".to_string(),
            threshold: thresholds.cpu_usage_threshold as f64,
            duration_seconds: 300,
            severity: AlertSeverity::Warning,
            enabled: true,
            channels: vec!["default".to_string()],
        });

        alert_rules.push(AlertRule {
            id: "memory_usage_high".to_string(),
            name: "High Memory Usage".to_string(),
            metric_name: "memory_usage_percent".to_string(),
            condition: "greater_than".to_string(),
            threshold: thresholds.memory_usage_threshold as f64,
            duration_seconds: 300,
            severity: AlertSeverity::Warning,
            enabled: true,
            channels: vec!["default".to_string()],
        });

        alert_rules.push(AlertRule {
            id: "network_latency_high".to_string(),
            name: "High Network Latency".to_string(),
            metric_name: "network_latency_ms".to_string(),
            condition: "greater_than".to_string(),
            threshold: thresholds.network_latency_threshold,
            duration_seconds: 180,
            severity: AlertSeverity::Critical,
            enabled: true,
            channels: vec!["default".to_string()],
        });

        let mut alert_channels = Vec::new();
        alert_channels.push(AlertChannel {
            id: "default".to_string(),
            name: "Default Log Channel".to_string(),
            channel_type: "log".to_string(),
            config: serde_json::json!({}),
            enabled: true,
        });

        Self {
            active_alerts: HashMap::new(),
            alert_history: Vec::new(),
            alert_rules,
            alert_channels,
            alert_stats: AlertStatistics::default(),
        }
    }

    /// Check alert conditions
    pub async fn check_alerts(&mut self) -> Result<()> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Example alert checking (would use actual metrics)
        let cpu_usage = 75.0; // Example value
        let memory_usage = 80.0; // Example value
        let network_latency = 200.0; // Example value

        // Collect rules to check (to avoid borrowing conflicts)
        let cpu_rule = self
            .alert_rules
            .iter()
            .find(|r| r.id == "cpu_usage_high")
            .cloned();
        let memory_rule = self
            .alert_rules
            .iter()
            .find(|r| r.id == "memory_usage_high")
            .cloned();
        let network_rule = self
            .alert_rules
            .iter()
            .find(|r| r.id == "network_latency_high")
            .cloned();

        // Check CPU usage alert
        if let Some(rule) = cpu_rule {
            if cpu_usage > rule.threshold {
                self.trigger_alert(&rule, cpu_usage, timestamp).await?;
            }
        }

        // Check memory usage alert
        if let Some(rule) = memory_rule {
            if memory_usage > rule.threshold {
                self.trigger_alert(&rule, memory_usage, timestamp).await?;
            }
        }

        // Check network latency alert
        if let Some(rule) = network_rule {
            if network_latency > rule.threshold {
                self.trigger_alert(&rule, network_latency, timestamp)
                    .await?;
            }
        }

        // Update alert statistics
        self.alert_stats.total_alerts = self.alert_history.len() as u64;
        self.alert_stats.active_alerts = self.active_alerts.len() as u64;
        self.alert_stats.last_updated = timestamp;

        debug!("Checked alerts");
        Ok(())
    }

    /// Trigger an alert
    async fn trigger_alert(&mut self, rule: &AlertRule, value: f64, timestamp: u64) -> Result<()> {
        let alert_id = format!("{}_{}", rule.id, timestamp);

        // Check if alert already exists
        if self.active_alerts.contains_key(&alert_id) {
            return Ok(());
        }

        let alert = Alert {
            id: alert_id.clone(),
            rule_id: rule.id.clone(),
            severity: rule.severity.clone(),
            title: rule.name.clone(),
            description: format!(
                "{} exceeded threshold: {} > {}",
                rule.name, value, rule.threshold
            ),
            metric_name: rule.metric_name.clone(),
            current_value: value,
            threshold: rule.threshold,
            created_at: timestamp,
            last_triggered: timestamp,
            times_triggered: 1,
            status: AlertStatus::Active,
        };

        self.active_alerts.insert(alert_id.clone(), alert);

        // Create alert event
        let event = AlertEvent {
            alert_id: alert_id.clone(),
            event_type: "triggered".to_string(),
            timestamp,
            message: format!("Alert {} triggered", rule.name),
            value,
        };

        self.alert_history.push(event);
        if self.alert_history.len() > 500 {
            self.alert_history.remove(0);
        }

        // Send alert (simplified - would use actual channels)
        warn!(
            "🚨 Alert triggered: {} - {} > {}",
            rule.name, value, rule.threshold
        );

        Ok(())
    }

    /// Get active alerts
    pub fn get_active_alerts(&self) -> &HashMap<String, Alert> {
        &self.active_alerts
    }

    /// Get alert statistics
    pub fn get_alert_stats(&self) -> &AlertStatistics {
        &self.alert_stats
    }
}

impl CostAnalyzer {
    /// Create a new cost analyzer
    pub fn new() -> Self {
        Self {
            cost_metrics: CostMetrics::default(),
            cost_breakdown: CostBreakdown::default(),
            cost_trends: HashMap::new(),
            cost_predictions: HashMap::new(),
            optimization_recommendations: Vec::new(),
        }
    }

    /// Analyze costs
    pub async fn analyze_costs(&mut self) -> Result<()> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Calculate cost metrics (simplified - would use actual data)
        self.cost_metrics.compute_cost_usd = 5.50; // Example value
        self.cost_metrics.storage_cost_usd = 1.25; // Example value
        self.cost_metrics.network_cost_usd = 0.75; // Example value
        self.cost_metrics.energy_cost_usd = 2.00; // Example value
        self.cost_metrics.total_cost_usd = self.cost_metrics.compute_cost_usd
            + self.cost_metrics.storage_cost_usd
            + self.cost_metrics.network_cost_usd
            + self.cost_metrics.energy_cost_usd;

        self.cost_metrics.cost_per_hour = self.cost_metrics.total_cost_usd;
        self.cost_metrics.cost_per_operation = self.cost_metrics.total_cost_usd / 1000.0; // Example
        self.cost_metrics.cost_efficiency_score = 0.85; // Example
        self.cost_metrics.last_updated = timestamp;

        // Calculate cost breakdown
        let total = self.cost_metrics.total_cost_usd;
        self.cost_breakdown.compute_percentage =
            (self.cost_metrics.compute_cost_usd / total) * 100.0;
        self.cost_breakdown.storage_percentage =
            (self.cost_metrics.storage_cost_usd / total) * 100.0;
        self.cost_breakdown.network_percentage =
            (self.cost_metrics.network_cost_usd / total) * 100.0;
        self.cost_breakdown.energy_percentage = (self.cost_metrics.energy_cost_usd / total) * 100.0;
        self.cost_breakdown.overhead_percentage = 0.0; // No overhead in this example

        // Create cost drivers
        self.cost_breakdown.top_cost_drivers = vec![
            CostDriver {
                component: "compute".to_string(),
                cost_usd: self.cost_metrics.compute_cost_usd,
                percentage: self.cost_breakdown.compute_percentage,
                trend: "stable".to_string(),
                optimization_potential: 0.15,
            },
            CostDriver {
                component: "energy".to_string(),
                cost_usd: self.cost_metrics.energy_cost_usd,
                percentage: self.cost_breakdown.energy_percentage,
                trend: "increasing".to_string(),
                optimization_potential: 0.25,
            },
        ];

        self.cost_breakdown.last_updated = timestamp;

        // Generate cost optimization recommendations
        self.generate_cost_recommendations().await?;

        debug!("Analyzed costs");
        Ok(())
    }

    /// Generate cost optimization recommendations
    async fn generate_cost_recommendations(&mut self) -> Result<()> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Example cost optimization recommendation
        if self.cost_metrics.energy_cost_usd > 1.5 {
            let recommendation = CostOptimizationRecommendation {
                id: Uuid::new_v4().to_string(),
                title: "Optimize Energy Consumption".to_string(),
                description:
                    "Energy costs are high. Consider implementing power management strategies."
                        .to_string(),
                component: "energy".to_string(),
                potential_savings_usd: 0.50,
                implementation_effort: "medium".to_string(),
                priority: "high".to_string(),
                created_at: timestamp,
            };

            self.optimization_recommendations.push(recommendation);
        }

        // Limit recommendations history
        if self.optimization_recommendations.len() > 50 {
            self.optimization_recommendations.remove(0);
        }

        Ok(())
    }

    /// Get cost metrics
    pub fn get_cost_metrics(&self) -> &CostMetrics {
        &self.cost_metrics
    }

    /// Get cost breakdown
    pub fn get_cost_breakdown(&self) -> &CostBreakdown {
        &self.cost_breakdown
    }

    /// Get optimization recommendations
    pub fn get_optimization_recommendations(&self) -> &Vec<CostOptimizationRecommendation> {
        &self.optimization_recommendations
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{sleep, Duration};

    #[tokio::test]
    async fn test_production_metrics_manager() {
        let config = ProductionMetricsConfig::default();
        let manager = ProductionMetricsManager::new(config).await.unwrap();

        // Test getting metrics summary
        let summary = manager.get_metrics_summary().await.unwrap();
        assert!(summary.last_updated > 0);
    }

    #[tokio::test]
    async fn test_metrics_collector() {
        let mut collector = MetricsCollector::new().await.unwrap();

        // Test collecting metrics
        collector.collect_metrics().await.unwrap();

        // Test getting metrics
        let metrics = collector.get_all_metrics();
        assert!(!metrics.is_empty());

        // Test collection stats
        assert!(collector.collection_stats.total_collections > 0);
    }

    #[tokio::test]
    async fn test_alerting_system() {
        let thresholds = AlertThresholds {
            cpu_usage_threshold: 80.0,
            memory_usage_threshold: 85.0,
            network_latency_threshold: 1000.0,
            error_rate_threshold: 5.0,
            cost_threshold: 10.0,
        };

        let mut alerting = AlertingSystem::new(&thresholds);

        // Test checking alerts
        alerting.check_alerts().await.unwrap();

        // Test alert rules
        assert!(!alerting.alert_rules.is_empty());
    }

    #[tokio::test]
    async fn test_cost_analyzer() {
        let mut analyzer = CostAnalyzer::new();

        // Test cost analysis
        analyzer.analyze_costs().await.unwrap();

        // Test cost metrics
        let metrics = analyzer.get_cost_metrics();
        assert!(metrics.total_cost_usd > 0.0);

        // Test cost breakdown
        let breakdown = analyzer.get_cost_breakdown();
        assert!(breakdown.compute_percentage > 0.0);
    }
}
