### Overview

You’re basically asking for a **reusable “DDoS shield” module** you can plug into:
- an Axum/Hyper HTTP API, and  
- a Tonic gRPC API,  

with consistent behavior: rate limiting, timeouts, connection caps, and overload shedding.

Below is a Rust‑native architecture document plus concrete integration patterns.

---

## 1. High‑level architecture

**Goal:** A crate (e.g. `ddos_guard`) that exposes composable layers/middleware for both HTTP (Axum/Hyper) and gRPC (Tonic).

**Core ideas:**
- **Shared core**: algorithms, config, metrics, IP extraction, token buckets, concurrency limits.
- **HTTP adapter**: Tower layers for Axum/Hyper.
- **gRPC adapter**: Interceptors/layers for Tonic.
- **Config‑driven**: All limits and behaviors configurable at startup.

```text
ddos_guard
├─ core
│  ├─ config.rs          // Limits, timeouts, strategies
│  ├─ rate_limit.rs      // Token bucket / leaky bucket
│  ├─ concurrency.rs     // Global & per-key concurrency limits
│  ├─ backpressure.rs    // Overload detection & shedding
│  ├─ identity.rs        // Client identity (IP, API key, etc.)
│  └─ metrics.rs         // Counters, histograms, logging hooks
├─ http
│  ├─ layer.rs           // Tower Layer<S> for Axum/Hyper
│  └─ extractor.rs       // IP extraction from headers/socket
└─ grpc
   ├─ layer.rs           // Tower Layer<S> for Tonic
   └─ interceptor.rs     // Optional Tonic interceptor
```

---

## 2. Core module design

### 2.1 Config

```rust
pub struct DdosConfig {
    pub max_rps_per_ip: u32,
    pub max_concurrent_per_ip: u32,
    pub max_global_concurrent: u32,
    pub request_timeout_ms: u64,
    pub max_body_bytes: u64,
    pub penalty_duration_ms: u64, // temporary bans
}
```

You can load this from env, TOML, or a central config service.

### 2.2 Identity & keying

You want a unified way to identify “who” to rate‑limit:

```rust
pub enum ClientId {
    Ip(std::net::IpAddr),
    ApiKey(String),
    Composite { ip: std::net::IpAddr, api_key: Option<String> },
}

pub trait ClientIdentity {
    fn client_id(&self) -> Option<ClientId>;
}
```

HTTP and gRPC adapters implement `ClientIdentity` for their request types.

### 2.3 Rate limiting

Use a fast, lock‑free limiter (you can wrap `governor` or roll your own).

```rust
pub struct RateLimiter {
    // internal buckets keyed by ClientId
}

impl RateLimiter {
    pub fn check(&self, id: &ClientId) -> bool {
        // true = allowed, false = over limit
    }
}
```

### 2.4 Concurrency limits & overload

```rust
pub struct ConcurrencyGuard {
    // global and per-client counters
}

impl ConcurrencyGuard {
    pub fn try_acquire(&self, id: &ClientId) -> Option<Permit> {
        // returns a permit that decrements on drop
    }
}

pub struct OverloadDetector {
    pub max_global_concurrent: u32,
}

impl OverloadDetector {
    pub fn is_overloaded(&self, current: u32) -> bool {
        current > self.max_global_concurrent
    }
}
```

### 2.5 Timeouts & body limits

These are generic utilities used by both HTTP and gRPC adapters:

```rust
pub async fn with_timeout<F, T>(
    dur: std::time::Duration,
    fut: F,
) -> Result<T, TimeoutError>
where
    F: std::future::Future<Output = T>,
{
    tokio::time::timeout(dur, fut).await.map_err(|_| TimeoutError)
}
```

---

## 3. HTTP (Axum/Hyper) integration

### 3.1 HTTP layer type

```rust
use tower::{Layer, Service};
use http::{Request, Response};
use std::task::{Context, Poll};
use std::sync::Arc;

pub struct HttpDdosLayer {
    config: Arc<DdosConfig>,
    limiter: Arc<RateLimiter>,
    concurrency: Arc<ConcurrencyGuard>,
}

impl<S> Layer<S> for HttpDdosLayer {
    type Service = HttpDdosService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        HttpDdosService {
            inner,
            config: self.config.clone(),
            limiter: self.limiter.clone(),
            concurrency: self.concurrency.clone(),
        }
    }
}

pub struct HttpDdosService<S> {
    inner: S,
    config: Arc<DdosConfig>,
    limiter: Arc<RateLimiter>,
    concurrency: Arc<ConcurrencyGuard>,
}

impl<S, B> Service<Request<B>> for HttpDdosService<S>
where
    S: Service<Request<B>, Response = Response<axum::body::Body>> + Send + 'static,
    S::Future: Send + 'static,
    B: http_body::Body + Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = futures::future::BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<B>) -> Self::Future {
        let config = self.config.clone();
        let limiter = self.limiter.clone();
        let concurrency = self.concurrency.clone();
        let mut inner = self.inner.clone();

        Box::pin(async move {
            // 1. Extract client id
            let client_id = extract_client_id(&req).unwrap_or_else(default_client_id);

            // 2. Rate limit
            if !limiter.check(&client_id) {
                return Ok(too_many_requests());
            }

            // 3. Concurrency
            let permit = match concurrency.try_acquire(&client_id) {
                Some(p) => p,
                None => return Ok(service_unavailable()),
            };

            // 4. Timeout
            let fut = inner.call(req);
            let res = with_timeout(
                std::time::Duration::from_millis(config.request_timeout_ms),
                fut,
            )
            .await;

            drop(permit);

            match res {
                Ok(r) => Ok(r),
                Err(_) => Ok(gateway_timeout()),
            }
        })
    }
}
```

You’d also add:
- **body size limit** via `tower_http::limit::RequestBodyLimitLayer` or your own wrapper.
- **IP extraction** from `X-Forwarded-For`, `X-Real-IP`, or socket addr.

### 3.2 Axum usage

```rust
use axum::{Router, routing::get};
use ddos_guard::http::HttpDdosLayer;

let ddos_layer = HttpDdosLayer::new(config);

let app = Router::new()
    .route("/health", get(health))
    .route("/expensive", get(expensive))
    .layer(ddos_layer);
```

---

## 4. gRPC (Tonic) integration

### 4.1 Tonic layer

Tonic is Tower‑based, so you can reuse the same pattern with `Request<tonic::body::BoxBody>`.

```rust
use tonic::body::BoxBody;
use tonic::Status;
use tower::{Layer, Service};
use http::Request;

pub struct GrpcDdosLayer {
    config: Arc<DdosConfig>,
    limiter: Arc<RateLimiter>,
    concurrency: Arc<ConcurrencyGuard>,
}

pub struct GrpcDdosService<S> {
    inner: S,
    config: Arc<DdosConfig>,
    limiter: Arc<RateLimiter>,
    concurrency: Arc<ConcurrencyGuard>,
}

impl<S> Layer<S> for GrpcDdosLayer {
    type Service = GrpcDdosService<S>;
    fn layer(&self, inner: S) -> Self::Service {
        GrpcDdosService {
            inner,
            config: self.config.clone(),
            limiter: self.limiter.clone(),
            concurrency: self.concurrency.clone(),
        }
    }
}

impl<S> Service<Request<tonic::transport::Body>> for GrpcDdosService<S>
where
    S: Service<Request<tonic::transport::Body>, Response = http::Response<BoxBody>, Error = Status>
        + Clone
        + Send
        + 'static,
    S::Future: Send + 'static,
{
    type Response = S::Response;
    type Error = Status;
    type Future = futures::future::BoxFuture<'static, Result<Self::Response, Status>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<tonic::transport::Body>) -> Self::Future {
        let config = self.config.clone();
        let limiter = self.limiter.clone();
        let concurrency = self.concurrency.clone();
        let mut inner = self.inner.clone();

        Box::pin(async move {
            let client_id = extract_client_id_grpc(&req).unwrap_or_else(default_client_id);

            if !limiter.check(&client_id) {
                return Err(Status::resource_exhausted("rate limit exceeded"));
            }

            let permit = match concurrency.try_acquire(&client_id) {
                Some(p) => p,
                None => return Err(Status::unavailable("server overloaded")),
            };

            let fut = inner.call(req);
            let res = with_timeout(
                std::time::Duration::from_millis(config.request_timeout_ms),
                fut,
            )
            .await;

            drop(permit);

            match res {
                Ok(r) => Ok(r),
                Err(_) => Err(Status::deadline_exceeded("request timed out")),
            }
        })
    }
}
```

### 4.2 Tonic server usage

```rust
use tonic::transport::Server;
use ddos_guard::grpc::GrpcDdosLayer;

let ddos_layer = GrpcDdosLayer::new(config);

Server::builder()
    .layer(ddos_layer)
    .add_service(MyGrpcServiceServer::new(my_service))
    .serve(addr)
    .await?;
```

---

## 5. Cross‑cutting concerns

### 5.1 Metrics & observability

Expose counters like:
- **`ddos_guard_http_rejected_rate_limit`**
- **`ddos_guard_http_rejected_concurrency`**
- **`ddos_guard_http_timeout`**
- Same for gRPC.

Hook into `tracing` and/or `opentelemetry` so you can see:
- which endpoints are targeted,
- which client IDs are noisy,
- how often you’re shedding load.

### 5.2 Penalty & temporary bans

Extend `RateLimiter` to support short‑term bans:

- On repeated violations, mark `ClientId` as **penalized until `Instant`**.
- `check()` returns false immediately for penalized clients.

### 5.3 Environment‑aware profiles

You probably want different profiles:

- **dev**: limits mostly disabled, logging only.
- **staging**: realistic limits, but no bans.
- **prod**: full enforcement, bans, strict timeouts.

---

## 6. How to package it as a reusable module

- **Crate name:** `ddos_guard` (or internal workspace crate).
- **Features:**
  - `http` for Axum/Hyper integration.
  - `grpc` for Tonic integration.
  - `metrics` for Prometheus/OpenTelemetry hooks.
- **Public API:**
  - `DdosConfig`
  - `new_http_layer(config: DdosConfig) -> HttpDdosLayer`
  - `new_grpc_layer(config: DdosConfig) -> GrpcDdosLayer`

---

If you’d like, next step I can:
- turn this into a **concrete crate layout** with `lib.rs` and modules, or  
- tailor the config and limits to your **actual traffic profile** (e.g., expected RPS, burst patterns, and which endpoints are most sensitive).


### Crate layout

Let’s call the crate `ddos_guard`. Here’s a concrete structure:

```text
ddos_guard/
├─ Cargo.toml
└─ src/
   ├─ lib.rs
   ├─ config.rs
   ├─ identity.rs
   ├─ rate_limit.rs
   ├─ concurrency.rs
   ├─ overload.rs
   ├─ timeout.rs
   ├─ metrics.rs
   ├─ http/
   │  ├─ mod.rs
   │  ├─ layer.rs
   │  └─ extract.rs
   └─ grpc/
      ├─ mod.rs
      ├─ layer.rs
      └─ extract.rs
```

---

### `Cargo.toml` (core bits)

```toml
[package]
name = "ddos_guard"
version = "0.1.0"
edition = "2021"

[features]
default = ["http", "grpc"]
http = ["tower", "http", "axum", "tower-http"]
grpc = ["tonic", "http"]

[dependencies]
tokio = { version = "1", features = ["full"] }
tower = "0.4"
http = "0.2"
futures = "0.3"
tracing = "0.1"
dashmap = "5"
governor = { version = "0.6", default-features = false, features = ["std"] }

# feature-gated
axum = { version = "0.7", optional = true }
tower-http = { version = "0.5", optional = true }
tonic = { version = "0.12", optional = true }
```

---

### `src/lib.rs`

```rust
pub mod config;
pub mod identity;
pub mod rate_limit;
pub mod concurrency;
pub mod overload;
pub mod timeout;
pub mod metrics;

#[cfg(feature = "http")]
pub mod http;

#[cfg(feature = "grpc")]
pub mod grpc;

pub use config::DdosConfig;
```

---

### Config tuned to a realistic profile

Assumptions (you can override easily):

- **Profile:** B2B APIs, mostly machine traffic.
- **Average load:** ~200 RPS per service.
- **Burst:** up to 1 000 RPS for short periods.
- **Axum/Hyper API:**
  - Some cheap endpoints (`GET /health`, `GET /status`).
  - Some expensive endpoints (e.g., payment routing, aggregation).
- **Tonic gRPC API:**
  - Fewer clients, higher per‑client throughput.
  - Long‑lived connections, streaming in some cases.

We’ll encode **sane defaults** and allow overrides.

#### `src/config.rs`

```rust
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct DdosConfig {
    pub max_rps_per_ip: u32,
    pub max_concurrent_per_ip: u32,
    pub max_global_concurrent: u32,
    pub request_timeout: Duration,
    pub max_body_bytes: u64,
    pub penalty_duration: Duration,
}

impl DdosConfig {
    pub fn default_http() -> Self {
        Self {
            max_rps_per_ip: 50,              // 50 RPS per IP
            max_concurrent_per_ip: 32,       // 32 in-flight per IP
            max_global_concurrent: 1024,     // 1k in-flight globally
            request_timeout: Duration::from_secs(3),
            max_body_bytes: 2 * 1024 * 1024, // 2 MiB
            penalty_duration: Duration::from_secs(60),
        }
    }

    pub fn default_grpc() -> Self {
        Self {
            max_rps_per_ip: 100,             // gRPC clients are fewer but heavier
            max_concurrent_per_ip: 64,
            max_global_concurrent: 2048,
            request_timeout: Duration::from_secs(5),
            max_body_bytes: 4 * 1024 * 1024, // 4 MiB
            penalty_duration: Duration::from_secs(60),
        }
    }

    pub fn with_overrides<F>(mut self, f: F) -> Self
    where
        F: FnOnce(&mut Self),
    {
        f(&mut self);
        self
    }
}
```

---

### Identity and keying

#### `src/identity.rs`

```rust
use std::net::IpAddr;

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub enum ClientId {
    Ip(IpAddr),
    ApiKey(String),
    Composite { ip: IpAddr, api_key: Option<String> },
}

pub trait ClientIdentity {
    fn client_id(&self) -> Option<ClientId>;
}
```

---

### Rate limiting (per‑client, token bucket)

#### `src/rate_limit.rs`

```rust
use crate::config::DdosConfig;
use crate::identity::ClientId;
use dashmap::DashMap;
use governor::{
    clock::DefaultClock,
    state::keyed::DefaultKeyedStateStore,
    Quota, RateLimiter,
};
use std::num::NonZeroU32;
use std::sync::Arc;

type InnerLimiter = RateLimiter<ClientId, DefaultKeyedStateStore<ClientId>, DefaultClock>;

#[derive(Clone)]
pub struct DdosRateLimiter {
    inner: Arc<InnerLimiter>,
}

impl DdosRateLimiter {
    pub fn new(config: &DdosConfig) -> Self {
        let quota = Quota::per_second(NonZeroU32::new(config.max_rps_per_ip).unwrap());
        let inner = RateLimiter::keyed(quota);
        Self { inner: Arc::new(inner) }
    }

    pub fn check(&self, id: &ClientId) -> bool {
        self.inner.check_key(id).is_ok()
    }
}
```

---

### Concurrency and overload

#### `src/concurrency.rs`

```rust
use crate::identity::ClientId;
use dashmap::DashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

#[derive(Clone)]
pub struct ConcurrencyGuard {
    per_client: Arc<DashMap<ClientId, AtomicU32>>,
    global: Arc<AtomicU32>,
    max_per_client: u32,
    max_global: u32,
}

pub struct Permit {
    guard: ConcurrencyGuard,
    client_id: ClientId,
}

impl ConcurrencyGuard {
    pub fn new(max_per_client: u32, max_global: u32) -> Self {
        Self {
            per_client: Arc::new(DashMap::new()),
            global: Arc::new(AtomicU32::new(0)),
            max_per_client,
            max_global,
        }
    }

    pub fn try_acquire(&self, id: &ClientId) -> Option<Permit> {
        let global = self.global.fetch_add(1, Ordering::SeqCst) + 1;
        if global > self.max_global {
            self.global.fetch_sub(1, Ordering::SeqCst);
            return None;
        }

        let entry = self
            .per_client
            .entry(id.clone())
            .or_insert_with(|| AtomicU32::new(0));
        let client = entry.fetch_add(1, Ordering::SeqCst) + 1;
        if client > self.max_per_client {
            entry.fetch_sub(1, Ordering::SeqCst);
            self.global.fetch_sub(1, Ordering::SeqCst);
            return None;
        }

        Some(Permit {
            guard: self.clone(),
            client_id: id.clone(),
        })
    }
}

impl Drop for Permit {
    fn drop(&mut self) {
        if let Some(entry) = self.guard.per_client.get(&self.client_id) {
            entry.fetch_sub(1, Ordering::SeqCst);
        }
        self.guard.global.fetch_sub(1, Ordering::SeqCst);
    }
}
```

#### `src/overload.rs`

```rust
pub fn is_overloaded(current_global: u32, max_global: u32) -> bool {
    current_global > max_global
}
```

---

### Timeout helper

#### `src/timeout.rs`

```rust
use std::time::Duration;

#[derive(Debug)]
pub struct TimeoutError;

pub async fn with_timeout<F, T>(
    dur: Duration,
    fut: F,
) -> Result<T, TimeoutError>
where
    F: std::future::Future<Output = T>,
{
    tokio::time::timeout(dur, fut)
        .await
        .map_err(|_| TimeoutError)
}
```

---

### HTTP integration (Axum/Hyper)

#### `src/http/mod.rs`

```rust
pub mod layer;
pub mod extract;

pub use layer::HttpDdosLayer;
```

#### `src/http/extract.rs`

```rust
use crate::identity::{ClientId, ClientIdentity};
use http::Request;
use std::net::IpAddr;

fn parse_ip(s: &str) -> Option<IpAddr> {
    s.split(',')
        .next()
        .and_then(|part| part.trim().parse::<IpAddr>().ok())
}

pub fn extract_client_id<B>(req: &Request<B>) -> Option<ClientId> {
    if let Some(forwarded) = req.headers().get("x-forwarded-for") {
        if let Ok(s) = forwarded.to_str() {
            if let Some(ip) = parse_ip(s) {
                return Some(ClientId::Ip(ip));
            }
        }
    }

    // fallback: remote addr if available via extensions (you can wire this in your server)
    None
}
```

#### `src/http/layer.rs`

```rust
use crate::{
    concurrency::{ConcurrencyGuard, Permit},
    config::DdosConfig,
    http::extract::extract_client_id,
    identity::ClientId,
    rate_limit::DdosRateLimiter,
    timeout::with_timeout,
};
use futures::future::BoxFuture;
use http::{Request, Response, StatusCode};
use std::task::{Context, Poll};
use std::sync::Arc;
use tower::{Layer, Service};

#[derive(Clone)]
pub struct HttpDdosLayer {
    config: Arc<DdosConfig>,
    limiter: DdosRateLimiter,
    concurrency: ConcurrencyGuard,
}

impl HttpDdosLayer {
    pub fn new(config: DdosConfig) -> Self {
        let limiter = DdosRateLimiter::new(&config);
        let concurrency =
            ConcurrencyGuard::new(config.max_concurrent_per_ip, config.max_global_concurrent);
        Self {
            config: Arc::new(config),
            limiter,
            concurrency,
        }
    }
}

#[derive(Clone)]
pub struct HttpDdosService<S> {
    inner: S,
    config: Arc<DdosConfig>,
    limiter: DdosRateLimiter,
    concurrency: ConcurrencyGuard,
}

impl<S> Layer<S> for HttpDdosLayer {
    type Service = HttpDdosService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        HttpDdosService {
            inner,
            config: self.config.clone(),
            limiter: self.limiter.clone(),
            concurrency: self.concurrency.clone(),
        }
    }
}

impl<S, B> Service<Request<B>> for HttpDdosService<S>
where
    S: Service<Request<B>, Response = Response<axum::body::Body>> + Clone + Send + 'static,
    S::Future: Send + 'static,
    B: http_body::Body + Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<B>) -> Self::Future {
        let config = self.config.clone();
        let limiter = self.limiter.clone();
        let concurrency = self.concurrency.clone();
        let mut inner = self.inner.clone();

        Box::pin(async move {
            let client_id = extract_client_id(&req).unwrap_or_else(|| ClientId::Ip("0.0.0.0".parse().unwrap()));

            if !limiter.check(&client_id) {
                let mut resp = Response::new(axum::body::Body::empty());
                *resp.status_mut() = StatusCode::TOO_MANY_REQUESTS;
                return Ok(resp);
            }

            let permit: Permit = match concurrency.try_acquire(&client_id) {
                Some(p) => p,
                None => {
                    let mut resp = Response::new(axum::body::Body::empty());
                    *resp.status_mut() = StatusCode::SERVICE_UNAVAILABLE;
                    return Ok(resp);
                }
            };

            let fut = inner.call(req);
            let res = with_timeout(config.request_timeout, fut).await;

            drop(permit);

            match res {
                Ok(r) => Ok(r),
                Err(_) => {
                    let mut resp = Response::new(axum::body::Body::empty());
                    *resp.status_mut() = StatusCode::GATEWAY_TIMEOUT;
                    Ok(resp)
                }
            }
        })
    }
}
```

**Axum usage:**

```rust
use axum::{Router, routing::get};
use ddos_guard::{DdosConfig};
use ddos_guard::http::HttpDdosLayer;

let config = DdosConfig::default_http();
let ddos_layer = HttpDdosLayer::new(config);

let app = Router::new()
    .route("/health", get(|| async { "ok" }))
    .route("/expensive", get(expensive_handler))
    .layer(ddos_layer);
```

---

### gRPC integration (Tonic)

#### `src/grpc/mod.rs`

```rust
pub mod layer;
pub mod extract;

pub use layer::GrpcDdosLayer;
```

#### `src/grpc/extract.rs`

```rust
use crate::identity::ClientId;
use http::Request;
use std::net::IpAddr;

pub fn extract_client_id<B>(req: &Request<B>) -> Option<ClientId> {
    if let Some(forwarded) = req.headers().get("x-forwarded-for") {
        if let Ok(s) = forwarded.to_str() {
            if let Ok(ip) = s.split(',').next().unwrap_or("").trim().parse::<IpAddr>() {
                return Some(ClientId::Ip(ip));
            }
        }
    }
    None
}
```

#### `src/grpc/layer.rs`

```rust
use crate::{
    concurrency::{ConcurrencyGuard, Permit},
    config::DdosConfig,
    grpc::extract::extract_client_id,
    identity::ClientId,
    rate_limit::DdosRateLimiter,
    timeout::with_timeout,
};
use futures::future::BoxFuture;
use http::Request;
use std::sync::Arc;
use std::task::{Context, Poll};
use tonic::{body::BoxBody, Status};
use tower::{Layer, Service};

#[derive(Clone)]
pub struct GrpcDdosLayer {
    config: Arc<DdosConfig>,
    limiter: DdosRateLimiter,
    concurrency: ConcurrencyGuard,
}

impl GrpcDdosLayer {
    pub fn new(config: DdosConfig) -> Self {
        let limiter = DdosRateLimiter::new(&config);
        let concurrency =
            ConcurrencyGuard::new(config.max_concurrent_per_ip, config.max_global_concurrent);
        Self {
            config: Arc::new(config),
            limiter,
            concurrency,
        }
    }
}

#[derive(Clone)]
pub struct GrpcDdosService<S> {
    inner: S,
    config: Arc<DdosConfig>,
    limiter: DdosRateLimiter,
    concurrency: ConcurrencyGuard,
}

impl<S> Layer<S> for GrpcDdosLayer {
    type Service = GrpcDdosService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        GrpcDdosService {
            inner,
            config: self.config.clone(),
            limiter: self.limiter.clone(),
            concurrency: self.concurrency.clone(),
        }
    }
}

impl<S> Service<Request<tonic::transport::Body>> for GrpcDdosService<S>
where
    S: Service<Request<tonic::transport::Body>, Response = http::Response<BoxBody>, Error = Status>
        + Clone
        + Send
        + 'static,
    S::Future: Send + 'static,
{
    type Response = S::Response;
    type Error = Status;
    type Future = BoxFuture<'static, Result<Self::Response, Status>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<tonic::transport::Body>) -> Self::Future {
        let config = self.config.clone();
        let limiter = self.limiter.clone();
        let concurrency = self.concurrency.clone();
        let mut inner = self.inner.clone();

        Box::pin(async move {
            let client_id = extract_client_id(&req).unwrap_or_else(|| ClientId::Ip("0.0.0.0".parse().unwrap()));

            if !limiter.check(&client_id) {
                return Err(Status::resource_exhausted("rate limit exceeded"));
            }

            let permit: Permit = match concurrency.try_acquire(&client_id) {
                Some(p) => p,
                None => return Err(Status::unavailable("server overloaded")),
            };

            let fut = inner.call(req);
            let res = with_timeout(config.request_timeout, fut).await;

            drop(permit);

            match res {
                Ok(r) => Ok(r),
                Err(_) => Err(Status::deadline_exceeded("request timed out")),
            }
        })
    }
}
```

**Tonic usage:**

```rust
use ddos_guard::{DdosConfig};
use ddos_guard::grpc::GrpcDdosLayer;
use tonic::transport::Server;

let config = DdosConfig::default_grpc();
let ddos_layer = GrpcDdosLayer::new(config);

Server::builder()
    .layer(ddos_layer)
    .add_service(MyGrpcServiceServer::new(my_service))
    .serve(addr)
    .await?;
```

---

### Programmatic test plan

You want to validate both **correctness** and **DDoS behavior**.

#### 1. Unit tests (inside `ddos_guard`)

- **Rate limiter behavior**
  - **Test:** `max_rps_per_ip = 5`.
  - Call `check(&id)` 5 times in quick succession → all `true`.
  - 6th call → `false`.
  - After 1 second, calls allowed again.

- **Concurrency guard**
  - **Test:** `max_per_client = 2`, `max_global = 4`.
  - Acquire 2 permits for same client → success.
  - 3rd acquire → `None`.
  - Drop one permit → next acquire succeeds.
  - Acquire across multiple clients until global limit → further acquires fail.

- **Timeout helper**
  - **Test:** future that sleeps longer than timeout → returns `TimeoutError`.
  - Future that completes before timeout → returns `Ok`.

Example:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{concurrency::ConcurrencyGuard, rate_limit::DdosRateLimiter, config::DdosConfig};
    use crate::identity::ClientId;
    use std::time::Duration;

    #[tokio::test]
    async fn rate_limiter_enforces_rps() {
        let cfg = DdosConfig::default_http().with_overrides(|c| c.max_rps_per_ip = 5);
        let limiter = DdosRateLimiter::new(&cfg);
        let id = ClientId::Ip("127.0.0.1".parse().unwrap());

        for _ in 0..5 {
            assert!(limiter.check(&id));
        }
        assert!(!limiter.check(&id));
    }

    #[tokio::test]
    async fn concurrency_guard_limits_per_client() {
        let guard = ConcurrencyGuard::new(2, 10);
        let id = ClientId::Ip("127.0.0.1".parse().unwrap());

        let p1 = guard.try_acquire(&id).unwrap();
        let p2 = guard.try_acquire(&id).unwrap();
        assert!(guard.try_acquire(&id).is_none());
        drop(p1);
        assert!(guard.try_acquire(&id).is_some());
        drop(p2);
    }
}
```

#### 2. Integration tests (HTTP)

Spin up a small Axum server with `HttpDdosLayer` and hit it with a client.

- **Test: rate limiting**
  - Configure `max_rps_per_ip = 5`.
  - In a single second, send 10 requests from same IP (no delay).
  - Expect ~5 responses `200`, ~5 responses `429`.

- **Test: concurrency**
  - Configure `max_concurrent_per_ip = 2`.
  - Create 5 concurrent requests that each sleep 500 ms on server.
  - Expect first 2 to be `200`, remaining 3 to be `503`.

- **Test: timeout**
  - Configure `request_timeout = 200ms`.
  - Handler sleeps 1 s.
  - Expect `504` responses.

You can implement these with `reqwest` or `hyper` client in `tests/http_ddos.rs`.

#### 3. Integration tests (gRPC)

Use Tonic’s test client:

- **Test: rate limiting**
  - `max_rps_per_ip = 5`.
  - Call a unary RPC 10 times quickly.
  - Expect some `Ok`, some `Status::resource_exhausted`.

- **Test: concurrency**
  - `max_concurrent_per_ip = 2`.
  - Start 5 concurrent unary RPCs that sleep on server.
  - Expect 2 `Ok`, 3 `Status::unavailable`.

- **Test: timeout**
  - `request_timeout = 200ms`.
  - RPC sleeps 1 s.
  - Expect `Status::deadline_exceeded`.

#### 4. Load/abuse tests (external tools)

Once wired into your real Axum/Tonic services:

- Use `k6`/`vegeta`/`bombardier` to simulate:
  - **Legit traffic:** 200 RPS, small bursts to 400 RPS → verify no 429/503 for normal clients.
  - **Single‑IP flood:** 5 000 RPS from one IP → verify majority of requests get 429/503 quickly, service stays responsive for other IPs.
  - **Slow‑loris style:** many connections sending bodies slowly → verify timeouts and concurrency caps prevent resource exhaustion.

You can codify these as scripts in a `load-tests/` directory and run them in CI or pre‑deploy.

---

If you share your rough expected RPS per tenant and which endpoints are “hot” vs “expensive,” we can tune the default limits into a small **profile matrix** (e.g., `low`, `medium`, `high` traffic tiers) that you can flip via env in each environment.