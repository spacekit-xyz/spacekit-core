//! Per-DID idempotency cache + in-flight tracking + body fingerprinting (Phase 3).
//!
//! Mounted by [`crate::storage_facade::Facade`]. Wraps every write route so
//! that:
//!
//! - **Cache hit (key + matching fingerprint)**: return the cached
//!   `(status, body, headers)` verbatim.
//! - **In-flight hit**: block on the running request's `Notify` for up to
//!   `wait_timeout` (default 30s, max 120s) and return its result. Stripe/AWS
//!   pattern. Only return `409` if the wait exceeds the timeout.
//! - **Fingerprint mismatch (same key, different body)**: `422 Unprocessable
//!   Entity` with `{expected_fingerprint, got_fingerprint}` so the agent can
//!   surface the bug in its retry logic.
//!
//! TTLs are per-route (default 24h, max 7d). Long-running research agents that
//! resume the next day need >24h; short-lived control loops want shorter so
//! the cache stays small.

#![deny(clippy::all)]

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Notify, RwLock};
use tracing::debug;

/// Maximum idempotency-key TTL we will accept from the per-route registry.
pub const MAX_TTL_SECONDS: u64 = 7 * 24 * 60 * 60;
/// Hard cap on cached HTTP response bodies (large artifact routes must not pin multi-MB RAM).
pub const MAX_CACHED_BODY_BYTES: usize = 1024 * 1024;
/// Default idempotency-key TTL when a route has no override.
pub const DEFAULT_TTL_SECONDS: u64 = 24 * 60 * 60;
/// Default wait time when an idempotency key collides with an in-flight request.
pub const DEFAULT_INFLIGHT_WAIT_MS: u64 = 30_000;
/// Hard cap on the in-flight wait so a stuck request can't pin clients forever.
pub const MAX_INFLIGHT_WAIT_MS: u64 = 120_000;

/// Per-route TTL/wait override. Routes that take a long time (e.g. large repo
/// commits) configure a longer wait; cheap reads keep the default.
#[derive(Debug, Clone, Copy)]
pub struct IdempotencyConfig {
    pub ttl_seconds: u64,
    pub wait_timeout_ms: u64,
}

impl Default for IdempotencyConfig {
    fn default() -> Self {
        Self {
            ttl_seconds: DEFAULT_TTL_SECONDS,
            wait_timeout_ms: DEFAULT_INFLIGHT_WAIT_MS,
        }
    }
}

/// Cached HTTP response. Stored verbatim so the second request returns the
/// identical bytes the first request returned.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedResponse {
    pub status: u16,
    pub body: Vec<u8>,
    pub headers: Vec<(String, String)>,
    pub fingerprint: [u8; 32],
    pub stored_at: DateTime<Utc>,
    pub ttl_seconds: u64,
}

/// Decision returned by [`IdempotencyCache::check`].
pub enum Decision {
    /// No prior request with this `(did, route, key)` — caller should run the
    /// handler and then call [`IdempotencyCache::store`] with the response.
    Proceed,
    /// Identical (key + fingerprint) request previously settled — return the
    /// cached response verbatim.
    CachedHit(CachedResponse),
    /// Same key, different body. Caller should return `422 Unprocessable
    /// Entity` with `(expected, got)`.
    FingerprintMismatch { expected: [u8; 32], got: [u8; 32] },
    /// Identical request is in flight. Caller should wait for the
    /// `Notify`, then re-call `check` (which will then return
    /// `CachedHit`).
    InFlightWait {
        notify: Arc<Notify>,
        wait_timeout_ms: u64,
    },
}

#[derive(Default)]
struct InFlightSlot {
    notify: Arc<Notify>,
    fingerprint: [u8; 32],
}

/// In-memory idempotency cache. Disk-backed persistence is left as a follow-up
/// (the cache happily survives a single-process lifetime; multi-replica setups
/// should pair this with the `rate-limit-spacekit` distributed coordinator).
pub struct IdempotencyCache {
    entries: RwLock<HashMap<(String, String, String), CachedResponse>>,
    in_flight: RwLock<HashMap<(String, String, String), InFlightSlot>>,
    /// Per-route configuration. Routes not in the map use `IdempotencyConfig::default()`.
    route_config: RwLock<HashMap<String, IdempotencyConfig>>,
    capacity: usize,
    /// `CachedHit` returns (same key + fingerprint as a settled response).
    cached_hits_total: AtomicU64,
    /// New idempotency keys that proceeded to the handler (`Proceed` branch).
    fresh_proceeds_total: AtomicU64,
}

impl IdempotencyCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
            in_flight: RwLock::new(HashMap::new()),
            route_config: RwLock::new(HashMap::new()),
            capacity,
            cached_hits_total: AtomicU64::new(0),
            fresh_proceeds_total: AtomicU64::new(0),
        }
    }

    pub fn idempotency_totals(&self) -> (u64, u64) {
        (
            self.cached_hits_total.load(Ordering::Relaxed),
            self.fresh_proceeds_total.load(Ordering::Relaxed),
        )
    }

    /// Entry count, total cached body bytes, and largest single body.
    pub async fn memory_stats(&self) -> (usize, u64, u64) {
        let entries = self.entries.read().await;
        let count = entries.len();
        let total_bytes: u64 = entries.values().map(|e| e.body.len() as u64).sum();
        let largest = entries
            .values()
            .map(|e| e.body.len() as u64)
            .max()
            .unwrap_or(0);
        (count, total_bytes, largest)
    }

    pub async fn configure_route(&self, route: &str, cfg: IdempotencyConfig) {
        let cfg = IdempotencyConfig {
            ttl_seconds: cfg.ttl_seconds.min(MAX_TTL_SECONDS),
            wait_timeout_ms: cfg.wait_timeout_ms.min(MAX_INFLIGHT_WAIT_MS),
        };
        self.route_config
            .write()
            .await
            .insert(route.to_string(), cfg);
    }

    pub async fn route_config(&self, route: &str) -> IdempotencyConfig {
        self.route_config
            .read()
            .await
            .get(route)
            .copied()
            .unwrap_or_default()
    }

    /// BLAKE3 fingerprint of the canonical request body (typically
    /// `serde_json::to_vec(&serde_json::from_slice::<serde_json::Value>(body))`
    /// for JSON, or the raw bytes for binary uploads).
    pub fn fingerprint(body: &[u8]) -> [u8; 32] {
        *blake3::hash(body).as_bytes()
    }

    /// Inspect the cache and decide what the caller should do.
    pub async fn check(
        &self,
        did: &str,
        route: &str,
        key: &str,
        fingerprint: [u8; 32],
    ) -> Decision {
        let cache_key = (did.to_string(), route.to_string(), key.to_string());

        // Cached hit?
        {
            let entries = self.entries.read().await;
            if let Some(entry) = entries.get(&cache_key) {
                if !is_expired(entry) {
                    if entry.fingerprint == fingerprint {
                        self.cached_hits_total.fetch_add(1, Ordering::Relaxed);
                        return Decision::CachedHit(entry.clone());
                    }
                    return Decision::FingerprintMismatch {
                        expected: entry.fingerprint,
                        got: fingerprint,
                    };
                }
            }
        }

        // In-flight?
        {
            let in_flight = self.in_flight.read().await;
            if let Some(slot) = in_flight.get(&cache_key) {
                if slot.fingerprint == fingerprint {
                    let cfg = self.route_config(route).await;
                    return Decision::InFlightWait {
                        notify: slot.notify.clone(),
                        wait_timeout_ms: cfg.wait_timeout_ms,
                    };
                }
                return Decision::FingerprintMismatch {
                    expected: slot.fingerprint,
                    got: fingerprint,
                };
            }
        }

        // No prior request — claim the slot so concurrent retries become
        // InFlightWait rather than racing.
        let mut in_flight = self.in_flight.write().await;
        in_flight.insert(
            cache_key,
            InFlightSlot {
                notify: Arc::new(Notify::new()),
                fingerprint,
            },
        );
        self.fresh_proceeds_total.fetch_add(1, Ordering::Relaxed);
        Decision::Proceed
    }

    /// Store the handler's response and notify any waiters.
    pub async fn store(
        &self,
        did: &str,
        route: &str,
        key: &str,
        response: CachedResponse,
    ) -> Result<()> {
        if response.body.len() > MAX_CACHED_BODY_BYTES {
            debug!(
                did = %did,
                route = %route,
                bytes = response.body.len(),
                "idempotency: skip caching oversized response body"
            );
            let cache_key = (did.to_string(), route.to_string(), key.to_string());
            let mut in_flight = self.in_flight.write().await;
            if let Some(slot) = in_flight.remove(&cache_key) {
                slot.notify.notify_waiters();
            }
            return Ok(());
        }
        let cache_key = (did.to_string(), route.to_string(), key.to_string());

        let mut entries = self.entries.write().await;
        if entries.len() >= self.capacity {
            // Bounded LRU: evict the oldest entry. (Production code might pull
            // in `lru`; for now this is a hand-rolled MRU since we don't track
            // access time.)
            if let Some(victim) = entries
                .iter()
                .min_by_key(|(_, v)| v.stored_at)
                .map(|(k, _)| k.clone())
            {
                entries.remove(&victim);
            }
        }
        entries.insert(cache_key.clone(), response);
        drop(entries);

        let mut in_flight = self.in_flight.write().await;
        if let Some(slot) = in_flight.remove(&cache_key) {
            slot.notify.notify_waiters();
        }
        debug!(did = %did, route = %route, "idempotency: stored cache entry");
        Ok(())
    }

    /// Drop a slot without storing a response (called when the handler
    /// panicked or returned an error we don't want cached).
    pub async fn cancel(&self, did: &str, route: &str, key: &str) {
        let cache_key = (did.to_string(), route.to_string(), key.to_string());
        let mut in_flight = self.in_flight.write().await;
        if let Some(slot) = in_flight.remove(&cache_key) {
            slot.notify.notify_waiters();
        }
    }

    /// Wait on an in-flight slot. Returns `true` if the wait completed before
    /// the timeout; `false` if the caller should fall back to `409 Conflict`.
    pub async fn wait_for_inflight(notify: Arc<Notify>, timeout: Duration) -> bool {
        tokio::time::timeout(timeout, notify.notified())
            .await
            .is_ok()
    }
}

fn is_expired(entry: &CachedResponse) -> bool {
    let elapsed = (Utc::now() - entry.stored_at).num_seconds() as u64;
    elapsed > entry.ttl_seconds
}

/// Per-DID token bucket rate limiter. Keyed off the `Authorization: DID <did>`
/// header.
pub struct DidRateLimiter {
    buckets: RwLock<HashMap<String, TokenBucket>>,
    /// Default token refill rate (tokens per second).
    refill_per_sec: f64,
    /// Default bucket capacity.
    capacity: f64,
    rejections_total: AtomicU64,
    /// Unix timestamp (seconds) of each throttle; trimmed to last ~4k for `/api/agentic/health`.
    recent_rejection_unix_secs: std::sync::Mutex<Vec<u64>>,
}

#[derive(Clone)]
struct TokenBucket {
    tokens: f64,
    last_refill: std::time::Instant,
}

impl DidRateLimiter {
    pub fn new(refill_per_sec: f64, capacity: f64) -> Self {
        Self {
            buckets: RwLock::new(HashMap::new()),
            refill_per_sec,
            capacity,
            rejections_total: AtomicU64::new(0),
            recent_rejection_unix_secs: std::sync::Mutex::new(Vec::new()),
        }
    }

    fn record_throttle(&self) {
        self.rejections_total.fetch_add(1, Ordering::Relaxed);
        let now = Utc::now().timestamp() as u64;
        let mut v = self.recent_rejection_unix_secs.lock().unwrap();
        v.push(now);
        const MAX: usize = 4096;
        while v.len() > MAX {
            v.remove(0);
        }
        let cutoff = now.saturating_sub(120);
        v.retain(|t| *t >= cutoff);
    }

    pub fn rate_limit_rejections_total(&self) -> u64 {
        self.rejections_total.load(Ordering::Relaxed)
    }

    /// Count of throttles in the last `window_secs` (based on sampled timestamps).
    pub fn rate_limit_rejections_in_window(&self, window_secs: u64) -> u64 {
        let now = Utc::now().timestamp() as u64;
        let cutoff = now.saturating_sub(window_secs);
        match self.recent_rejection_unix_secs.lock() {
            // `MutexGuard::iter` yields `&&u64` here; compare the inner value.
            Ok(v) => v.iter().filter(|t| **t >= cutoff).count() as u64,
            Err(_) => 0,
        }
    }

    /// Try to consume one token. Returns `Ok(())` on success or `Err(retry_after)` on throttle.
    pub async fn check(&self, did: &str) -> std::result::Result<(), Duration> {
        let mut buckets = self.buckets.write().await;
        let now = std::time::Instant::now();
        let bucket = buckets
            .entry(did.to_string())
            .or_insert_with(|| TokenBucket {
                tokens: self.capacity,
                last_refill: now,
            });
        let elapsed = (now - bucket.last_refill).as_secs_f64();
        bucket.tokens = (bucket.tokens + elapsed * self.refill_per_sec).min(self.capacity);
        bucket.last_refill = now;
        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            Ok(())
        } else {
            // How long until we have 1 token?
            let needed = 1.0 - bucket.tokens;
            let secs = needed / self.refill_per_sec;
            self.record_throttle();
            Err(Duration::from_secs_f64(secs))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration as StdDuration;

    #[tokio::test]
    async fn cached_hit_returns_verbatim() {
        let cache = IdempotencyCache::new(8);
        let body = b"{\"x\":1}";
        let fp = IdempotencyCache::fingerprint(body);
        let did = "did:spacekit:user:alice";
        let route = "POST /api/transactions";
        let key = "abc";

        // First check should be Proceed.
        match cache.check(did, route, key, fp).await {
            Decision::Proceed => {}
            _ => panic!("expected Proceed"),
        }

        // Store the response.
        cache
            .store(
                did,
                route,
                key,
                CachedResponse {
                    status: 200,
                    body: b"ok".to_vec(),
                    headers: vec![],
                    fingerprint: fp,
                    stored_at: Utc::now(),
                    ttl_seconds: 60,
                },
            )
            .await
            .unwrap();

        // Second identical request hits the cache.
        match cache.check(did, route, key, fp).await {
            Decision::CachedHit(c) => assert_eq!(c.body, b"ok"),
            _ => panic!("expected CachedHit"),
        }
    }

    #[tokio::test]
    async fn fingerprint_mismatch_returns_diff() {
        let cache = IdempotencyCache::new(8);
        let did = "did:spacekit:user:bob";
        let route = "POST /api/sandboxes";
        let key = "key1";
        let fp1 = IdempotencyCache::fingerprint(b"a");
        let fp2 = IdempotencyCache::fingerprint(b"b");
        match cache.check(did, route, key, fp1).await {
            Decision::Proceed => {}
            _ => panic!("expected Proceed"),
        }
        cache
            .store(
                did,
                route,
                key,
                CachedResponse {
                    status: 200,
                    body: b"ok".to_vec(),
                    headers: vec![],
                    fingerprint: fp1,
                    stored_at: Utc::now(),
                    ttl_seconds: 60,
                },
            )
            .await
            .unwrap();
        match cache.check(did, route, key, fp2).await {
            Decision::FingerprintMismatch { expected, got } => {
                assert_eq!(expected, fp1);
                assert_eq!(got, fp2);
            }
            _ => panic!("expected FingerprintMismatch"),
        }
    }

    #[tokio::test]
    async fn rate_limiter_throttles() {
        // 1 token/sec, capacity 1 → second consume fails.
        let rl = DidRateLimiter::new(1.0, 1.0);
        let did = "did:spacekit:test";
        rl.check(did).await.expect("first should pass");
        let r = rl.check(did).await;
        assert!(r.is_err());
        // Sleep enough to refill.
        tokio::time::sleep(StdDuration::from_millis(1100)).await;
        rl.check(did).await.expect("third should pass after refill");
    }
}
