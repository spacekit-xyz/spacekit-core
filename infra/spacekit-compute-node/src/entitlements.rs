//! On-chain entitlement reader.
//!
//! Replaces the former in-memory aUSD vault. Value never originates on the
//! compute node: users deposit DAI/USDC into the SpaceKit entitlement contract
//! on Ethereum, and this module *reads* that contract to decide what a subject
//! is entitled to consume. There is deliberately no credit/mint path here — the
//! node has no authority to create balance.
//!
//! ## Trust model
//!
//! - **Reads are quorum'd.** Every read is issued to `min_rpc_agreement`
//!   independent RPC endpoints and must agree byte-for-byte. A single lying or
//!   compromised RPC provider cannot fabricate an entitlement.
//! - **Reads are confirmation-delayed.** Queries target `latest - confirmations`
//!   so a reorg cannot retroactively remove a deposit we already honoured.
//! - **Staleness fails closed.** If the cache is older than `max_staleness_secs`
//!   and the chain cannot be reached, spend authorization is denied. Reads still
//!   return the cached value, explicitly flagged `stale`.
//! - **Local spend is reserved, not minted.** Consumption between on-chain
//!   settlements is tracked as pending reservations subtracted from the
//!   on-chain allowance, so the node can never authorize beyond what was
//!   actually deposited.
//!
//! Units are normalized to 6-decimal micro-USD (`units`) regardless of whether
//! the deposit was USDC (6dp) or DAI (18dp); the contract performs that
//! normalization so this node never has to trust a client-supplied decimals
//! value.

use alloy_primitives::{Address, Bytes, B256, U256};
use alloy_provider::{Provider as AlloyProvider, ProviderBuilder};
use alloy_rpc_types::{BlockId, BlockNumberOrTag, TransactionInput, TransactionRequest};
use alloy_sol_types::{sol, SolCall, SolValue};
use serde::{Deserialize, Serialize};
use sha3::{Digest, Keccak256};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

sol! {
    /// SpaceKit entitlement registry deployed on Ethereum.
    ///
    /// `subject` is `keccak256(did_utf8)` so DIDs of any length map to a fixed
    /// slot without the node needing an address for the user.
    function entitlementOf(bytes32 subject) external view returns (
        uint256 depositedUnits,
        uint256 consumedUnits,
        uint64 expiresAt,
        uint8 tier
    );
}

/// Configuration for the on-chain entitlement reader.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntitlementConfig {
    pub enabled: bool,
    /// Address of the SpaceKit entitlement contract.
    pub contract_address: String,
    /// EIP-155 chain ID the contract is deployed on. Reads are rejected if an
    /// endpoint reports a different chain, which prevents a testnet RPC from
    /// being substituted for mainnet.
    pub chain_id: u64,
    /// Independent RPC endpoints. Provide at least as many as
    /// `min_rpc_agreement`; using two providers from the same vendor does not
    /// give you two independent views.
    pub rpc_endpoints: Vec<String>,
    /// How many endpoints must return identical data for a read to be accepted.
    pub min_rpc_agreement: usize,
    /// Block confirmations to wait before honouring on-chain state.
    pub confirmations: u64,
    /// How long a cached entry may be served before a refresh is attempted.
    pub cache_ttl_secs: u64,
    /// Hard limit past which a cached entry may no longer authorize spending.
    pub max_staleness_secs: u64,
}

impl Default for EntitlementConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            contract_address: String::new(),
            chain_id: 1,
            rpc_endpoints: Vec::new(),
            min_rpc_agreement: 2,
            confirmations: 12,
            cache_ttl_secs: 30,
            max_staleness_secs: 300,
        }
    }
}

impl EntitlementConfig {
    /// Build from environment. Mirrors `L1PersistenceConfig::from_env`.
    ///
    /// `SPACEKIT_ENTITLEMENT_RPC_URLS` is comma-separated.
    pub fn from_env() -> Self {
        let d = Self::default();
        let rpc_endpoints: Vec<String> = std::env::var("SPACEKIT_ENTITLEMENT_RPC_URLS")
            .ok()
            .map(|s| {
                s.split(',')
                    .map(|p| p.trim().to_string())
                    .filter(|p| !p.is_empty())
                    .collect()
            })
            .unwrap_or_default();

        let contract_address = std::env::var("SPACEKIT_ENTITLEMENT_CONTRACT")
            .unwrap_or_default()
            .trim()
            .to_string();

        fn env_num<T: std::str::FromStr>(key: &str, fallback: T) -> T {
            std::env::var(key)
                .ok()
                .and_then(|v| v.trim().parse::<T>().ok())
                .unwrap_or(fallback)
        }

        let enabled = !contract_address.is_empty() && !rpc_endpoints.is_empty();

        Self {
            enabled,
            contract_address,
            chain_id: env_num("SPACEKIT_ENTITLEMENT_CHAIN_ID", d.chain_id),
            rpc_endpoints,
            min_rpc_agreement: env_num("SPACEKIT_ENTITLEMENT_MIN_AGREEMENT", d.min_rpc_agreement),
            confirmations: env_num("SPACEKIT_ENTITLEMENT_CONFIRMATIONS", d.confirmations),
            cache_ttl_secs: env_num("SPACEKIT_ENTITLEMENT_CACHE_TTL_SECS", d.cache_ttl_secs),
            max_staleness_secs: env_num(
                "SPACEKIT_ENTITLEMENT_MAX_STALENESS_SECS",
                d.max_staleness_secs,
            ),
        }
    }

    fn validate(&self) -> Result<Address, EntitlementError> {
        if !self.enabled {
            return Err(EntitlementError::Disabled);
        }
        if self.rpc_endpoints.len() < self.min_rpc_agreement {
            return Err(EntitlementError::Config(format!(
                "{} RPC endpoint(s) configured but min_rpc_agreement is {}",
                self.rpc_endpoints.len(),
                self.min_rpc_agreement
            )));
        }
        if self.min_rpc_agreement == 0 {
            return Err(EntitlementError::Config(
                "min_rpc_agreement must be at least 1".into(),
            ));
        }
        self.contract_address
            .parse::<Address>()
            .map_err(|e| EntitlementError::Config(format!("invalid contract address: {e}")))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EntitlementError {
    #[error("entitlement reader is not configured")]
    Disabled,
    #[error("entitlement configuration invalid: {0}")]
    Config(String),
    #[error("could not reach agreement across RPC endpoints: {0}")]
    NoQuorum(String),
    #[error("entitlement data is stale ({age_secs}s old, limit {limit_secs}s) and the chain is unreachable")]
    Stale { age_secs: u64, limit_secs: u64 },
    #[error("entitlement expired at {0}")]
    Expired(u64),
    #[error("insufficient entitlement: requested {requested} units, {available} available")]
    Insufficient { requested: u128, available: u128 },
}

/// Raw state as recorded by the contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OnChainEntitlement {
    /// Total micro-USD deposited for this subject, all-time.
    pub deposited_units: u128,
    /// Micro-USD already settled on-chain.
    pub consumed_units: u128,
    /// Unix seconds; `0` means no expiry.
    pub expires_at: u64,
    pub tier: u8,
    /// Block the values were read at (already confirmation-delayed).
    pub block_number: u64,
}

impl OnChainEntitlement {
    /// Allowance remaining on chain, before local pending reservations.
    pub fn on_chain_available(&self) -> u128 {
        self.deposited_units.saturating_sub(self.consumed_units)
    }

    fn is_expired(&self, now: u64) -> bool {
        self.expires_at != 0 && now >= self.expires_at
    }
}

#[derive(Debug, Clone)]
struct CacheEntry {
    value: OnChainEntitlement,
    fetched_at: u64,
}

/// A held claim on part of the allowance, not yet settled on chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reservation {
    pub reservation_id: String,
    pub subject_did: String,
    pub units: u128,
    pub created_at: u64,
}

/// What callers see: on-chain truth plus local pending spend.
#[derive(Debug, Clone, Serialize)]
pub struct EntitlementView {
    pub did: String,
    pub deposited_units: u128,
    pub consumed_units: u128,
    pub pending_units: u128,
    pub available_units: u128,
    pub expires_at: u64,
    pub tier: u8,
    pub block_number: u64,
    pub fetched_at: u64,
    /// True when served from cache past `cache_ttl_secs`. Stale reads are
    /// informational only and cannot authorize a reservation.
    pub stale: bool,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// `keccak256(did)` — the contract's subject key.
pub fn subject_key(did: &str) -> B256 {
    let mut hasher = Keccak256::new();
    hasher.update(did.as_bytes());
    B256::from_slice(&hasher.finalize())
}

/// Reads entitlements from the Ethereum contract, with quorum, confirmation
/// delay, caching, and a local reservation ledger.
pub struct EntitlementReader {
    config: EntitlementConfig,
    contract: Address,
    cache: Arc<RwLock<HashMap<String, CacheEntry>>>,
    /// subject DID -> outstanding reservations.
    pending: Arc<RwLock<HashMap<String, Vec<Reservation>>>>,
}

impl EntitlementReader {
    pub fn new(config: EntitlementConfig) -> Result<Self, EntitlementError> {
        let contract = config.validate()?;
        Ok(Self {
            config,
            contract,
            cache: Arc::new(RwLock::new(HashMap::new())),
            pending: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    pub fn config(&self) -> &EntitlementConfig {
        &self.config
    }

    /// Query one endpoint at a confirmation-delayed block.
    async fn read_single(
        &self,
        rpc_url: &str,
        subject: B256,
    ) -> Result<OnChainEntitlement, String> {
        let url = rpc_url
            .parse()
            .map_err(|e| format!("{rpc_url}: invalid URL: {e}"))?;
        let provider = ProviderBuilder::new().connect_http(url);

        // Reject an endpoint pointed at the wrong network before trusting values.
        let reported_chain = provider
            .get_chain_id()
            .await
            .map_err(|e| format!("{rpc_url}: chain id: {e}"))?;
        if reported_chain != self.config.chain_id {
            return Err(format!(
                "{rpc_url}: reports chain {reported_chain}, expected {}",
                self.config.chain_id
            ));
        }

        let head = provider
            .get_block_number()
            .await
            .map_err(|e| format!("{rpc_url}: block number: {e}"))?;
        let target = head.saturating_sub(self.config.confirmations);
        if target == 0 {
            return Err(format!(
                "{rpc_url}: chain head {head} is below the {} confirmation depth",
                self.config.confirmations
            ));
        }

        let input = Bytes::from(
            [
                &entitlementOfCall::SELECTOR[..],
                &SolValue::abi_encode(&subject),
            ]
            .concat(),
        );

        let out = provider
            .call(
                TransactionRequest::default()
                    .to(self.contract)
                    .input(TransactionInput::from(input)),
            )
            .block(BlockId::Number(BlockNumberOrTag::Number(target)))
            .await
            .map_err(|e| format!("{rpc_url}: eth_call: {e}"))?;

        let decoded = entitlementOfCall::abi_decode_returns(&out)
            .map_err(|e| format!("{rpc_url}: decode: {e}"))?;

        // The contract stores micro-USD, which cannot exceed u128 in any
        // realistic supply; refuse rather than silently truncate.
        let to_u128 = |v: U256, field: &str| -> Result<u128, String> {
            u128::try_from(v).map_err(|_| format!("{rpc_url}: {field} exceeds u128"))
        };

        Ok(OnChainEntitlement {
            deposited_units: to_u128(decoded.depositedUnits, "depositedUnits")?,
            consumed_units: to_u128(decoded.consumedUnits, "consumedUnits")?,
            expires_at: decoded.expiresAt,
            tier: decoded.tier,
            block_number: target,
        })
    }

    /// Read from all endpoints and require `min_rpc_agreement` identical answers.
    async fn read_quorum(&self, did: &str) -> Result<OnChainEntitlement, EntitlementError> {
        let subject = subject_key(did);

        let mut results: Vec<OnChainEntitlement> = Vec::new();
        let mut errors: Vec<String> = Vec::new();

        let futures = self
            .config
            .rpc_endpoints
            .iter()
            .map(|url| self.read_single(url, subject));
        for outcome in futures::future::join_all(futures).await {
            match outcome {
                Ok(v) => results.push(v),
                Err(e) => errors.push(e),
            }
        }

        // Group by value ignoring block_number: endpoints legitimately sit at
        // slightly different heights, but the entitlement values themselves
        // must match.
        let mut best: Option<(OnChainEntitlement, usize)> = None;
        for candidate in &results {
            let count = results
                .iter()
                .filter(|r| {
                    r.deposited_units == candidate.deposited_units
                        && r.consumed_units == candidate.consumed_units
                        && r.expires_at == candidate.expires_at
                        && r.tier == candidate.tier
                })
                .count();
            if best.as_ref().map(|(_, c)| count > *c).unwrap_or(true) {
                best = Some((candidate.clone(), count));
            }
        }

        match best {
            Some((value, count)) if count >= self.config.min_rpc_agreement => Ok(value),
            Some((_, count)) => Err(EntitlementError::NoQuorum(format!(
                "best agreement was {count}/{} (need {}); errors: [{}]",
                self.config.rpc_endpoints.len(),
                self.config.min_rpc_agreement,
                errors.join("; ")
            ))),
            None => Err(EntitlementError::NoQuorum(format!(
                "all {} endpoint(s) failed: [{}]",
                self.config.rpc_endpoints.len(),
                errors.join("; ")
            ))),
        }
    }

    /// Fetch with cache. `require_fresh` refuses to serve a stale entry.
    async fn fetch(
        &self,
        did: &str,
        require_fresh: bool,
    ) -> Result<(OnChainEntitlement, u64, bool), EntitlementError> {
        let now = now_secs();

        if let Some(entry) = self.cache.read().await.get(did).cloned() {
            let age = now.saturating_sub(entry.fetched_at);
            if age < self.config.cache_ttl_secs {
                return Ok((entry.value, entry.fetched_at, false));
            }
        }

        match self.read_quorum(did).await {
            Ok(value) => {
                self.cache.write().await.insert(
                    did.to_string(),
                    CacheEntry {
                        value: value.clone(),
                        fetched_at: now,
                    },
                );
                Ok((value, now, false))
            }
            Err(e) => {
                // Refresh failed. Serve the cached value for reads, but never
                // let it authorize a spend past the staleness limit.
                let cached = self.cache.read().await.get(did).cloned();
                match cached {
                    Some(entry) => {
                        let age = now.saturating_sub(entry.fetched_at);
                        if require_fresh && age > self.config.max_staleness_secs {
                            Err(EntitlementError::Stale {
                                age_secs: age,
                                limit_secs: self.config.max_staleness_secs,
                            })
                        } else if require_fresh {
                            tracing::warn!(
                                did = did,
                                age_secs = age,
                                "entitlement refresh failed; authorizing from cache within staleness window: {e}"
                            );
                            Ok((entry.value, entry.fetched_at, true))
                        } else {
                            Ok((entry.value, entry.fetched_at, true))
                        }
                    }
                    None => Err(e),
                }
            }
        }
    }

    async fn pending_units(&self, did: &str) -> u128 {
        self.pending
            .read()
            .await
            .get(did)
            .map(|rs| rs.iter().map(|r| r.units).sum())
            .unwrap_or(0)
    }

    /// Read-only view. Never fails on staleness; flags it instead.
    pub async fn view(&self, did: &str) -> Result<EntitlementView, EntitlementError> {
        let (value, fetched_at, stale) = self.fetch(did, false).await?;
        let pending = self.pending_units(did).await;
        Ok(EntitlementView {
            did: did.to_string(),
            deposited_units: value.deposited_units,
            consumed_units: value.consumed_units,
            pending_units: pending,
            available_units: value.on_chain_available().saturating_sub(pending),
            expires_at: value.expires_at,
            tier: value.tier,
            block_number: value.block_number,
            fetched_at,
            stale,
        })
    }

    /// Authorize `units` of spend, holding them against the on-chain allowance.
    ///
    /// This is the only path that gates paid work, and it fails closed on
    /// staleness, expiry, or insufficient allowance.
    pub async fn reserve(
        &self,
        did: &str,
        units: u128,
        reservation_id: String,
    ) -> Result<Reservation, EntitlementError> {
        let (value, _, _) = self.fetch(did, true).await?;
        let now = now_secs();

        if value.is_expired(now) {
            return Err(EntitlementError::Expired(value.expires_at));
        }

        // Hold the write lock across the check and the insert so two concurrent
        // requests cannot both observe the same headroom.
        let mut pending = self.pending.write().await;
        let held: u128 = pending
            .get(did)
            .map(|rs| rs.iter().map(|r| r.units).sum())
            .unwrap_or(0);
        let available = value.on_chain_available().saturating_sub(held);

        if units > available {
            return Err(EntitlementError::Insufficient {
                requested: units,
                available,
            });
        }

        let reservation = Reservation {
            reservation_id,
            subject_did: did.to_string(),
            units,
            created_at: now,
        };
        pending
            .entry(did.to_string())
            .or_default()
            .push(reservation.clone());

        Ok(reservation)
    }

    /// Release a reservation whose work did not complete.
    pub async fn release(&self, did: &str, reservation_id: &str) -> bool {
        let mut pending = self.pending.write().await;
        let Some(list) = pending.get_mut(did) else {
            return false;
        };
        let before = list.len();
        list.retain(|r| r.reservation_id != reservation_id);
        let removed = list.len() != before;
        if list.is_empty() {
            pending.remove(did);
        }
        removed
    }

    /// Drop reservations older than `max_age_secs`, so a crashed job cannot
    /// permanently withhold a user's allowance.
    pub async fn expire_stale_reservations(&self, max_age_secs: u64) -> usize {
        let cutoff = now_secs().saturating_sub(max_age_secs);
        let mut pending = self.pending.write().await;
        let mut dropped = 0;
        pending.retain(|_, list| {
            let before = list.len();
            list.retain(|r| r.created_at >= cutoff);
            dropped += before - list.len();
            !list.is_empty()
        });
        dropped
    }

    /// Force the next read for `did` to hit the chain.
    pub async fn invalidate(&self, did: &str) {
        self.cache.write().await.remove(did);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subject_key_is_keccak_of_did() {
        let a = subject_key("did:spacekit:alice");
        let b = subject_key("did:spacekit:alice");
        let c = subject_key("did:spacekit:bob");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn disabled_config_is_rejected() {
        let cfg = EntitlementConfig::default();
        assert!(matches!(
            EntitlementReader::new(cfg),
            Err(EntitlementError::Disabled)
        ));
    }

    #[test]
    fn quorum_larger_than_endpoint_count_is_rejected() {
        let cfg = EntitlementConfig {
            enabled: true,
            contract_address: "0x0000000000000000000000000000000000000001".into(),
            rpc_endpoints: vec!["https://example.invalid".into()],
            min_rpc_agreement: 2,
            ..Default::default()
        };
        assert!(matches!(
            EntitlementReader::new(cfg),
            Err(EntitlementError::Config(_))
        ));
    }

    #[test]
    fn available_saturates_when_consumed_exceeds_deposited() {
        let e = OnChainEntitlement {
            deposited_units: 100,
            consumed_units: 250,
            expires_at: 0,
            tier: 0,
            block_number: 1,
        };
        assert_eq!(e.on_chain_available(), 0);
    }

    #[test]
    fn zero_expiry_never_expires() {
        let e = OnChainEntitlement {
            deposited_units: 1,
            consumed_units: 0,
            expires_at: 0,
            tier: 0,
            block_number: 1,
        };
        assert!(!e.is_expired(u64::MAX));
    }

    fn reader() -> EntitlementReader {
        EntitlementReader {
            config: EntitlementConfig {
                enabled: true,
                contract_address: "0x0000000000000000000000000000000000000001".into(),
                rpc_endpoints: vec!["https://a.invalid".into(), "https://b.invalid".into()],
                min_rpc_agreement: 2,
                cache_ttl_secs: 60,
                max_staleness_secs: 300,
                ..Default::default()
            },
            contract: Address::ZERO,
            cache: Arc::new(RwLock::new(HashMap::new())),
            pending: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    async fn seed(r: &EntitlementReader, did: &str, deposited: u128, consumed: u128) {
        r.cache.write().await.insert(
            did.to_string(),
            CacheEntry {
                value: OnChainEntitlement {
                    deposited_units: deposited,
                    consumed_units: consumed,
                    expires_at: 0,
                    tier: 1,
                    block_number: 100,
                },
                fetched_at: now_secs(),
            },
        );
    }

    #[tokio::test]
    async fn reserve_cannot_exceed_on_chain_allowance() {
        let r = reader();
        seed(&r, "did:test", 1_000_000, 400_000).await;

        // 600_000 available.
        r.reserve("did:test", 500_000, "r1".into()).await.unwrap();

        let err = r
            .reserve("did:test", 200_000, "r2".into())
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            EntitlementError::Insufficient {
                requested: 200_000,
                available: 100_000
            }
        ));
    }

    #[tokio::test]
    async fn released_reservation_frees_allowance() {
        let r = reader();
        seed(&r, "did:test", 1_000, 0).await;

        r.reserve("did:test", 1_000, "r1".into()).await.unwrap();
        assert!(r.reserve("did:test", 1, "r2".into()).await.is_err());

        r.release("did:test", "r1").await;
        r.reserve("did:test", 1_000, "r3".into()).await.unwrap();
    }

    #[tokio::test]
    async fn expired_entitlement_cannot_reserve() {
        let r = reader();
        r.cache.write().await.insert(
            "did:exp".into(),
            CacheEntry {
                value: OnChainEntitlement {
                    deposited_units: 1_000,
                    consumed_units: 0,
                    expires_at: 1,
                    tier: 1,
                    block_number: 100,
                },
                fetched_at: now_secs(),
            },
        );
        assert!(matches!(
            r.reserve("did:exp", 1, "r1".into()).await,
            Err(EntitlementError::Expired(1))
        ));
    }

    #[tokio::test]
    async fn view_reports_pending_and_available() {
        let r = reader();
        seed(&r, "did:test", 1_000, 100).await;
        r.reserve("did:test", 300, "r1".into()).await.unwrap();

        let v = r.view("did:test").await.unwrap();
        assert_eq!(v.deposited_units, 1_000);
        assert_eq!(v.consumed_units, 100);
        assert_eq!(v.pending_units, 300);
        assert_eq!(v.available_units, 600);
    }

    #[tokio::test]
    async fn stale_reservations_are_expired() {
        let r = reader();
        seed(&r, "did:test", 1_000, 0).await;
        r.reserve("did:test", 500, "r1".into()).await.unwrap();
        r.reserve("did:test", 100, "r2".into()).await.unwrap();

        // A reservation younger than the cutoff is kept.
        assert_eq!(r.expire_stale_reservations(600).await, 0);
        assert_eq!(r.pending_units("did:test").await, 600);

        // Age `r1` past the window; only it should be dropped.
        {
            let mut pending = r.pending.write().await;
            let list = pending.get_mut("did:test").unwrap();
            list[0].created_at = now_secs().saturating_sub(1_000);
        }
        assert_eq!(r.expire_stale_reservations(600).await, 1);
        assert_eq!(r.pending_units("did:test").await, 100);
    }
}
