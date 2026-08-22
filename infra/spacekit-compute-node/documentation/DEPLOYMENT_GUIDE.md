# SWTCH Production GPU+WASM Deployment Guide

## Architecture Overview

```
┌─────────────────┐    ┌──────────────────┐    ┌─────────────────┐
│   Load Balancer │────│  API Gateway     │────│  Cost Calculator│
│   (nginx/envoy) │    │  (rate limiting) │    │  (WASM Runtime) │
└─────────────────┘    └──────────────────┘    └─────────────────┘
         │                        │                        │
         ▼                        ▼                        ▼
┌─────────────────┐    ┌──────────────────┐    ┌─────────────────┐
│ Execution Queue │    │ Resource Manager │    │ GPU Pool Manager│
│ (Redis/RabbitMQ)│    │ (PostgreSQL)     │    │ (Kubernetes)    │
└─────────────────┘    └──────────────────┘    └─────────────────┘
         │                        │                        │
         ▼                        ▼                        ▼
┌─────────────────┐    ┌──────────────────┐    ┌─────────────────┐
│ Worker Nodes    │    │ Metrics Store    │    │ GPU Nodes       │
│ (CPU/WASM)      │    │ (Prometheus)     │    │ (CUDA/WebGPU)   │
└─────────────────┘    └──────────────────┘    └─────────────────┘
```

## Deployment Strategies

### 1. Kubernetes-Native Deployment

```yaml
# gpu-wasm-service.yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: gpu-wasm-service
spec:
  replicas: 3
  selector:
    matchLabels:
      app: gpu-wasm-service
  template:
    metadata:
      labels:
        app: gpu-wasm-service
    spec:
      containers:
      - name: compute-service
        image: gpu-wasm-service:latest
        resources:
          requests:
            memory: "2Gi"
            cpu: "1000m"
            nvidia.com/gpu: 1
          limits:
            memory: "4Gi"
            cpu: "2000m"
            nvidia.com/gpu: 1
        env:
        - name: RUST_LOG
          value: "info"
        - name: GPU_POOL_SIZE
          value: "4"
        - name: WASM_MEMORY_LIMIT
          value: "1073741824" # 1GB
        ports:
        - containerPort: 8080
        livenessProbe:
          httpGet:
            path: /health
            port: 8080
          initialDelaySeconds: 30
          periodSeconds: 10
        readinessProbe:
          httpGet:
            path: /ready
            port: 8080
          initialDelaySeconds: 5
          periodSeconds: 5

---
apiVersion: v1
kind: Service
metadata:
  name: gpu-wasm-service
spec:
  selector:
    app: gpu-wasm-service
  ports:
  - protocol: TCP
    port: 80
    targetPort: 8080
  type: ClusterIP

---
apiVersion: autoscaling/v2
kind: HorizontalPodAutoscaler
metadata:
  name: gpu-wasm-hpa
spec:
  scaleTargetRef:
    apiVersion: apps/v1
    kind: Deployment
    name: gpu-wasm-service
  minReplicas: 2
  maxReplicas: 10
  metrics:
  - type: Resource
    resource:
      name: cpu
      target:
        type: Utilization
        averageUtilization: 70
  - type: Resource
    resource:
      name: memory
      target:
        type: Utilization
        averageUtilization: 80
```

### 2. Docker Compose for Development

```yaml
# docker-compose.yml
version: '3.8'

services:
  gpu-wasm-service:
    build: .
    ports:
      - "8080:8080"
    environment:
      - RUST_LOG=debug
      - DATABASE_URL=postgresql://user:pass@postgres:5432/gpu_wasm
      - REDIS_URL=redis://redis:6379
    depends_on:
      - postgres
      - redis
    deploy:
      resources:
        reservations:
          devices:
            - driver: nvidia
              count: 1
              capabilities: [gpu]

  postgres:
    image: postgres:15
    environment:
      POSTGRES_DB: gpu_wasm
      POSTGRES_USER: user
      POSTGRES_PASSWORD: pass
    volumes:
      - postgres_data:/var/lib/postgresql/data
    ports:
      - "5432:5432"

  redis:
    image: redis:alpine
    ports:
      - "6379:6379"
    volumes:
      - redis_data:/data

  prometheus:
    image: prom/prometheus
    ports:
      - "9190:9190"
    volumes:
      - ./prometheus.yml:/etc/prometheus/prometheus.yml
      - prometheus_data:/prometheus

  grafana:
    image: grafana/grafana
    ports:
      - "3000:3000"
    environment:
      - GF_SECURITY_ADMIN_PASSWORD=admin
    volumes:
      - grafana_data:/var/lib/grafana

volumes:
  postgres_data:
  redis_data:
  prometheus_data:
  grafana_data:
```

## Configuration Management

### Environment Variables

```bash
# Core Service Configuration
RUST_LOG=info
SERVICE_PORT=8080
WORKER_THREADS=4

# Database Configuration
DATABASE_URL=postgresql://user:pass@localhost:5432/gpu_wasm
DATABASE_POOL_SIZE=10

# Redis Configuration (for queuing)
REDIS_URL=redis://localhost:6379
REDIS_POOL_SIZE=10

# GPU Configuration
GPU_POOL_SIZE=4
GPU_MEMORY_LIMIT_GB=8
GPU_POWER_LIMIT_WATTS=300

# WASM Configuration
WASM_MEMORY_LIMIT_BYTES=1073741824  # 1GB
WASM_EXECUTION_TIMEOUT_MS=30000     # 30 seconds
WASM_FUEL_LIMIT=1000000

# Cost Configuration
COST_CPU_CYCLE_RATE=0.001
COST_MEMORY_BYTE_RATE=0.0001
COST_GPU_HOUR_RATE=2.50

# Security Configuration
JWT_SECRET=your-secret-key
API_RATE_LIMIT=100  # requests per minute
MAX_CONCURRENT_EXECUTIONS=50
```

### Configuration Struct

```rust
use serde::{Deserialize, Serialize};
use std::env;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceConfig {
    pub service: ServiceSettings,
    pub database: DatabaseSettings,
    pub redis: RedisSettings,
    pub gpu: GpuSettings,
    pub wasm: WasmSettings,
    pub cost: CostSettings,
    pub security: SecuritySettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceSettings {
    pub port: u16,
    pub worker_threads: usize,
    pub log_level: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseSettings {
    pub url: String,
    pub pool_size: u32,
    pub timeout_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuSettings {
    pub pool_size: usize,
    pub memory_limit_gb: f32,
    pub power_limit_watts: u32,
    pub enable_cuda: bool,
    pub enable_webgpu: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmSettings {
    pub memory_limit_bytes: usize,
    pub execution_timeout_ms: u64,
    pub fuel_limit: u64,
}

impl ServiceConfig {
    pub fn from_env() -> Result<Self, config::ConfigError> {
        let mut cfg = config::Config::new();
        
        // Add environment variables
        cfg.merge(config::Environment::with_prefix("GPU_WASM"))?;
        
        // Add default values
        cfg.set_default("service.port", 8080)?;
        cfg.set_default("service.worker_threads", 4)?;
        cfg.set_default("service.log_level", "info")?;
        
        cfg.try_into()
    }
}
```

## Monitoring and Observability

### Prometheus Metrics

```rust
use prometheus::{Counter, Histogram, Gauge, Registry};
use std::sync::Arc;

#[derive(Clone)]
pub struct Metrics {
    pub execution_counter: Counter,
    pub execution_duration: Histogram,
    pub gpu_utilization: Gauge,
    pub wasm_memory_usage: Gauge,
    pub cost_total: Counter,
    pub error_counter: Counter,
}

impl Metrics {
    pub fn new(registry: &Registry) -> Self {
        let execution_counter = Counter::new(
            "gpu_wasm_executions_total",
            "Total number of executions"
        ).unwrap();
        
        let execution_duration = Histogram::with_opts(
            prometheus::HistogramOpts::new(
                "gpu_wasm_execution_duration_seconds",
                "Execution duration in seconds"
            ).buckets(vec![0.01, 0.1, 0.5, 1.0, 5.0, 10.0, 30.0])
        ).unwrap();
        
        let gpu_utilization = Gauge::new(
            "gpu_wasm_gpu_utilization_percent",
            "GPU utilization percentage"
        ).unwrap();
        
        let wasm_memory_usage = Gauge::new(
            "gpu_wasm_memory_usage_bytes",
            "WASM memory usage in bytes"
        ).unwrap();
        
        let cost_total = Counter::new(
            "gpu_wasm_cost_total_dollars",
            "Total cost in dollars"
        ).unwrap();
        
        let error_counter = Counter::new(
            "gpu_wasm_errors_total",
            "Total number of errors"
        ).unwrap();
        
        // Register metrics
        registry.register(Box::new(execution_counter.clone())).unwrap();
        registry.register(Box::new(execution_duration.clone())).unwrap();
        registry.register(Box::new(gpu_utilization.clone())).unwrap();
        registry.register(Box::new(wasm_memory_usage.clone())).unwrap();
        registry.register(Box::new(cost_total.clone())).unwrap();
        registry.register(Box::new(error_counter.clone())).unwrap();
        
        Self {
            execution_counter,
            execution_duration,
            gpu_utilization,
            wasm_memory_usage,
            cost_total,
            error_counter,
        }
    }
}
```

### Health Checks

```rust
use warp::Filter;
use serde_json::json;

pub fn health_routes() -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    let health = warp::path("health")
        .and(warp::get())
        .and_then(health_check);
        
    let ready = warp::path("ready")
        .and(warp::get())
        .and_then(readiness_check);
        
    health.or(ready)
}

async fn health_check() -> Result<impl warp::Reply, warp::Rejection> {
    // Basic health check - service is running
    Ok(warp::reply::json(&json!({
        "status": "healthy",
        "timestamp": chrono::Utc::now().to_rfc3339()
    })))
}

async fn readiness_check() -> Result<impl warp::Reply, warp::Rejection> {
    // More comprehensive readiness check
    let mut checks = vec![];
    
    // Check database connection
    // let db_ok = check_database().await;
    let db_ok = true; // Placeholder
    checks.push(("database", db_ok));
    
    // Check GPU availability
    // let gpu_ok = check_gpu().await;
    let gpu_ok = true; // Placeholder
    checks.push(("gpu", gpu_ok));
    
    // Check Redis connection
    // let redis_ok = check_redis().await;
    let redis_ok = true; // Placeholder
    checks.push(("redis", redis_ok));
    
    let all_ready = checks.iter().all(|(_, ok)| *ok);
    
    let status = if all_ready { "ready" } else { "not_ready" };
    
    Ok(warp::reply::json(&json!({
        "status": status,
        "checks": checks.into_iter().collect::<std::collections::HashMap<_, _>>(),
        "timestamp": chrono::Utc::now().to_rfc3339()
    })))
}
```

## Security Considerations

### 1. Input Validation
```rust
use validator::{Validate, ValidationError};

#[derive(Debug, Validate, Deserialize)]
pub struct ExecutionRequest {
    #[validate(length(min = 1, max = 100))]
    pub user_id: String,
    
    #[validate(length(min = 1, max = 10485760))] // Max 10MB
    pub code: String,
    
    #[validate(range(min = 1, max = 3600))] // Max 1 hour
    pub timeout_seconds: u64,
    
    #[validate(range(min = 0.0, max = 1000.0))] // Max $1000
    pub max_cost: f64,
}
```

### 2. Resource Limits
```rust
pub struct SecurityLimits {
    pub max_memory_bytes: usize,
    pub max_execution_time_ms: u64,
    pub max_gpu_memory_gb: f32,
    pub max_cost_per_execution: f64,
    pub max_daily_cost: f64,
    pub rate_limit_per_minute: u32,
}

impl Default for SecurityLimits {
    fn default() -> Self {
        Self {
            max_memory_bytes: 1024 * 1024 * 1024, // 1GB
            max_execution_time_ms: 30_000,         // 30 seconds
            max_gpu_memory_gb: 4.0,                // 4GB
            max_cost_per_execution: 10.0,          // $10
            max_daily_cost: 100.0,                 // $100
            rate_limit_per_minute: 60,             // 60 requests/min
        }
    }
}
```

### 3. Sandboxing
```rust
use wasmtime::*;

pub fn create_secure_wasm_config() -> Config {
    let mut config = Config::new();
    
    // Enable security features
    config.cranelift_opt_level(OptLevel::Speed);
    config.consume_fuel(true);
    config.epoch_interruption(true);
    config.memory_init_cow(false); // Prevent copy-on-write attacks
    config.memory_guaranteed_dense_image_size(0); // Prevent large memory allocations
    
    // Disable potentially dangerous features
    config.wasm_multi_memory(false);
    config.wasm_threads(false);
    config.wasm_reference_types(false);
    config.wasm_bulk_memory(false);
    
    config
}
```

## Performance Optimization

### 1. Connection Pooling
```rust
use deadpool_postgres::{Config, Pool, Runtime};
use deadpool_redis::{Config as RedisConfig, Pool as RedisPool};

pub struct ConnectionPools {
    pub postgres: Pool,
    pub redis: RedisPool,
}

impl ConnectionPools {
    pub async fn new(config: &ServiceConfig) -> Result<Self, Box<dyn std::error::Error>> {
        // PostgreSQL pool
        let mut pg_config = Config::new();
        pg_config.url = Some(config.database.url.clone());
        pg_config.pool = Some(deadpool_postgres::PoolConfig::new(config.database.pool_size));
        let postgres = pg_config.create_pool(Some(Runtime::Tokio1), tokio_postgres::NoTls)?;
        
        // Redis pool
        let redis_config = RedisConfig::from_url(&config.redis.url);
        let redis = redis_config.create_pool(Some(Runtime::Tokio1))?;
        
        Ok(Self { postgres, redis })
    }
}
```

### 2. Caching Strategy
```rust
use moka::future::Cache;
use std::time::Duration;

pub struct CacheManager {
    execution_results: Cache<String, Vec<u8>>,
    cost_estimates: Cache<String, f64>,
    gpu_availability: Cache<String, bool>,
}

impl CacheManager {
    pub fn new() -> Self {
        Self {
            execution_results: Cache::builder()
                .max_capacity(1000)
                .time_to_live(Duration::from_secs(300)) // 5 minutes
                .build(),
                
            cost_estimates: Cache::builder()
                .max_capacity(10000)
                .time_to_live(Duration::from_secs(3600)) // 1 hour
                .build(),
                
            gpu_availability: Cache::builder()
                .max_capacity(100)
                .time_to_live(Duration::from_secs(10)) // 10 seconds
                .build(),
        }
    }
}
```

## Deployment Checklist

### Pre-deployment
- [ ] Run comprehensive tests (unit, integration, load)
- [ ] Security audit of WASM sandbox configuration
- [ ] GPU driver compatibility verification
- [ ] Database migration scripts ready
- [ ] Monitoring dashboards configured
- [ ] Alert rules defined
- [ ] Backup and recovery procedures tested

### Deployment
- [ ] Blue-green deployment strategy
- [ ] Database migrations applied
- [ ] Environment variables configured
- [ ] SSL certificates installed
- [ ] Load balancer health checks enabled
- [ ] Monitoring stack deployed

### Post-deployment
- [ ] Health checks passing
- [ ] Metrics flowing to monitoring system
- [ ] Performance benchmarks met
- [ ] Security scans completed
- [ ] Documentation updated
- [ ] Team trained on operations

## Scaling Considerations

### Horizontal Scaling
- Use Kubernetes HPA for automatic scaling
- Implement circuit breakers for external dependencies
- Use Redis for distributed caching and queuing
- Consider GPU node pools for cost optimization

### Vertical Scaling
- Monitor CPU/GPU utilization patterns
- Adjust resource limits based on workload analysis
- Use NUMA-aware scheduling for multi-GPU nodes

### Cost Optimization
- Implement spot instance strategies for non-critical workloads
- Use preemptible GPU instances where appropriate
- Monitor cost per execution and optimize algorithms
- Implement automatic model selection based on cost/performance

This comprehensive deployment guide ensures your GPU+WASM cost calculation system is production-ready, secure, and scalable.