//! Opaque RouteKit API-key authentication backed by SpaceKit Storage Node.

use crate::storage_client::{ApiKeyRecord, StorageClient};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use subtle::ConstantTimeEq;

#[derive(Debug, Clone)]
pub struct AuthContext {
    pub key_id: String,
    pub owner_did: String,
}

#[derive(Clone)]
pub struct AuthService {
    storage: Option<StorageClient>,
    bootstrap: Arc<HashMap<String, ApiKeyRecord>>,
    cache: Arc<RwLock<HashMap<String, CachedKey>>>,
    rate_windows: Arc<Mutex<HashMap<String, RateWindow>>>,
    cache_ttl: Duration,
    default_rate_limit_rpm: u32,
}

#[derive(Clone)]
struct CachedKey {
    record: ApiKeyRecord,
    expires: Instant,
}

struct RateWindow {
    started: Instant,
    count: u32,
}

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("missing bearer API key")]
    Missing,
    #[error("invalid API key")]
    Invalid,
    #[error("API key is disabled or expired")]
    Disabled,
    #[error("authentication service unavailable")]
    Unavailable,
    #[error("rate limit exceeded")]
    RateLimited,
}

impl AuthService {
    pub fn new(
        storage: Option<StorageClient>,
        bootstrap_keys: &[String],
        cache_ttl: Duration,
        default_rate_limit_rpm: u32,
    ) -> Self {
        let bootstrap = bootstrap_keys
            .iter()
            .filter(|key| key.starts_with("sk-routekit-"))
            .map(|key| {
                let hash = hash_api_key(key);
                (
                    hash.clone(),
                    ApiKeyRecord {
                        key_id: format!("bootstrap-{}", &hash[..12]),
                        key_hash: hash,
                        owner_did: "did:spacekit:routekit:bootstrap".to_string(),
                        enabled: true,
                        expires_at: None,
                        rate_limit_rpm: default_rate_limit_rpm,
                    },
                )
            })
            .collect();

        Self {
            storage,
            bootstrap: Arc::new(bootstrap),
            cache: Arc::new(RwLock::new(HashMap::new())),
            rate_windows: Arc::new(Mutex::new(HashMap::new())),
            cache_ttl,
            default_rate_limit_rpm,
        }
    }

    pub fn is_configured(&self) -> bool {
        self.storage.is_some() || !self.bootstrap.is_empty()
    }

    pub async fn authenticate(
        &self,
        authorization: Option<&str>,
    ) -> Result<AuthContext, AuthError> {
        let key = authorization
            .and_then(|value| value.strip_prefix("Bearer "))
            .ok_or(AuthError::Missing)?;
        if !key.starts_with("sk-routekit-") || key.len() < 24 {
            return Err(AuthError::Invalid);
        }

        let hash = hash_api_key(key);
        let record = if let Some(record) = self.cached(&hash) {
            record
        } else if let Some(record) = self.bootstrap.get(&hash) {
            record.clone()
        } else if let Some(storage) = self.storage.as_ref() {
            match storage.get_api_key(&hash).await {
                Ok(Some(record)) => {
                    self.insert_cache(hash.clone(), record.clone());
                    record
                }
                Ok(None) => return Err(AuthError::Invalid),
                Err(error) => {
                    tracing::warn!(error = %error, "API-key lookup failed");
                    return Err(AuthError::Unavailable);
                }
            }
        } else {
            return Err(AuthError::Invalid);
        };

        let expected = hex::decode(&record.key_hash).map_err(|_| AuthError::Invalid)?;
        let actual = hex::decode(&hash).map_err(|_| AuthError::Invalid)?;
        if expected.len() != actual.len() || expected.ct_eq(&actual).unwrap_u8() != 1 {
            return Err(AuthError::Invalid);
        }
        if !record.enabled
            || record
                .expires_at
                .is_some_and(|expires| expires <= unix_now())
        {
            return Err(AuthError::Disabled);
        }

        self.check_rate_limit(&record.key_id, record.rate_limit_rpm.clamp(1, 10_000))?;
        Ok(AuthContext {
            key_id: record.key_id,
            owner_did: record.owner_did,
        })
    }

    fn cached(&self, hash: &str) -> Option<ApiKeyRecord> {
        let cache = self.cache.read().ok()?;
        let cached = cache.get(hash)?;
        (cached.expires > Instant::now()).then(|| cached.record.clone())
    }

    fn insert_cache(&self, hash: String, record: ApiKeyRecord) {
        if let Ok(mut cache) = self.cache.write() {
            cache.insert(
                hash,
                CachedKey {
                    record,
                    expires: Instant::now() + self.cache_ttl,
                },
            );
        }
    }

    fn check_rate_limit(&self, key_id: &str, configured_limit: u32) -> Result<(), AuthError> {
        let limit = if configured_limit == 0 {
            self.default_rate_limit_rpm
        } else {
            configured_limit
        };
        let mut windows = self
            .rate_windows
            .lock()
            .map_err(|_| AuthError::Unavailable)?;
        let window = windows.entry(key_id.to_string()).or_insert(RateWindow {
            started: Instant::now(),
            count: 0,
        });
        if window.started.elapsed() >= Duration::from_secs(60) {
            window.started = Instant::now();
            window.count = 0;
        }
        if window.count >= limit {
            return Err(AuthError::RateLimited);
        }
        window.count += 1;
        Ok(())
    }
}

pub fn hash_api_key(key: &str) -> String {
    hex::encode(Sha256::digest(key.as_bytes()))
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn accepts_bootstrap_key_and_rejects_unknown_key() {
        let key = "sk-routekit-test-01234567890123456789".to_string();
        let service =
            AuthService::new(None, std::slice::from_ref(&key), Duration::from_secs(60), 2);
        assert!(service
            .authenticate(Some(&format!("Bearer {key}")))
            .await
            .is_ok());
        assert!(matches!(
            service
                .authenticate(Some("Bearer sk-routekit-unknown-012345678901"))
                .await,
            Err(AuthError::Invalid)
        ));
    }

    #[tokio::test]
    async fn rate_limits_per_key() {
        let key = "sk-routekit-test-01234567890123456789".to_string();
        let service =
            AuthService::new(None, std::slice::from_ref(&key), Duration::from_secs(60), 1);
        assert!(service
            .authenticate(Some(&format!("Bearer {key}")))
            .await
            .is_ok());
        assert!(matches!(
            service.authenticate(Some(&format!("Bearer {key}"))).await,
            Err(AuthError::RateLimited)
        ));
    }
}
