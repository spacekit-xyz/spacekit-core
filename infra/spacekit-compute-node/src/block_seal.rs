//! Authority seals for proof-of-authority blocks.
//!
//! While the network has authorities (see [`crate::validator_governance`]),
//! every block must carry a seal: the block producer's authority DID and a
//! SPHINCS+ signature over
//!
//! ```text
//! SPACEKIT-BLOCK-SEAL-v1\n{chain_id}\n{number}\n{hex(block_hash)}
//! ```
//!
//! The seal travels next to the block in the P2P envelope, so block hashes and
//! stored blocks are unchanged. Nodes keep the seals they accept in a JSON-lines
//! file so they can serve them to peers that catch up later.
//!
//! Producers take turns ([`scheduled_producer`]): the authority at
//! `height mod n` (authorities sorted by DID) produces each block. If it has
//! not produced within the grace period the next authority may, and so on.
//! Importers accept a seal from any current or former authority; the schedule
//! only keeps honest producers from colliding.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const SEAL_DOMAIN: &str = "SPACEKIT-BLOCK-SEAL-v1";

/// Exact bytes an authority signs to seal a block.
pub fn seal_payload(chain_id: &str, number: u64, block_hash: &[u8; 32]) -> Vec<u8> {
    format!("{SEAL_DOMAIN}\n{chain_id}\n{number}\n{}", hex::encode(block_hash)).into_bytes()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockSeal {
    pub proposer_did: String,
    pub signature_hex: String,
}

/// An authority's signing key, loaded from a DID wallet
/// (`spacekit did create --save` writes `~/.spacekit/did_wallet.json`).
pub struct AuthoritySigner {
    pub did: String,
    pub public_key: Vec<u8>,
    secret_key: Vec<u8>,
}

impl std::fmt::Debug for AuthoritySigner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthoritySigner").field("did", &self.did).finish_non_exhaustive()
    }
}

impl AuthoritySigner {
    /// Load `{ did, sphincs_pk_hex, sphincs_sk_hex }`. The DID must be derived
    /// from the public key, as governance requires.
    pub fn from_wallet_file(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("reading authority wallet {}", path.display()))?;
        let json: serde_json::Value = serde_json::from_str(&raw)
            .with_context(|| format!("parsing authority wallet {}", path.display()))?;
        let did = json["did"].as_str().ok_or_else(|| anyhow!("wallet has no did"))?.to_string();
        let public_key = hex::decode(
            json["sphincs_pk_hex"].as_str().ok_or_else(|| anyhow!("wallet has no sphincs_pk_hex"))?,
        )?;
        let secret_key = hex::decode(
            json["sphincs_sk_hex"].as_str().ok_or_else(|| anyhow!("wallet has no sphincs_sk_hex"))?,
        )?;
        if !crate::validator_governance::did_matches_key(&did, &public_key) {
            bail!("authority wallet DID {did} is not derived from its public key");
        }
        Ok(Self {
            did,
            public_key,
            secret_key,
        })
    }

    /// `SPACEKIT_AUTHORITY_WALLET`, when set.
    pub fn from_env() -> Result<Option<Self>> {
        match std::env::var("SPACEKIT_AUTHORITY_WALLET") {
            Ok(path) if !path.trim().is_empty() => {
                Self::from_wallet_file(Path::new(path.trim())).map(Some)
            }
            _ => Ok(None),
        }
    }

    pub fn sign(&self, message: &[u8]) -> Result<Vec<u8>> {
        spacekit_did::sphincs::SphincsPlus::sign(&self.secret_key, message)
            .map_err(|_| anyhow!("invalid SPHINCS+ secret key in authority wallet"))
    }

    pub fn seal(&self, chain_id: &str, number: u64, block_hash: &[u8; 32]) -> Result<BlockSeal> {
        let signature = self.sign(&seal_payload(chain_id, number, block_hash))?;
        Ok(BlockSeal {
            proposer_did: self.did.clone(),
            signature_hex: hex::encode(signature),
        })
    }
}

/// Check a seal against the authority keys the caller trusts.
pub fn verify_seal(
    seal: &BlockSeal,
    chain_id: &str,
    number: u64,
    block_hash: &[u8; 32],
    authority_key: impl Fn(&str) -> Option<Vec<u8>>,
) -> Result<()> {
    let key = authority_key(&seal.proposer_did).ok_or_else(|| {
        anyhow!(
            "block {number} sealed by {}, which may not produce blocks",
            seal.proposer_did
        )
    })?;
    let signature = hex::decode(&seal.signature_hex)
        .map_err(|_| anyhow!("block {number} seal signature is not hex"))?;
    if !spacekit_did::sphincs::SphincsPlus::verify(
        &key,
        &seal_payload(chain_id, number, block_hash),
        &signature,
    ) {
        bail!("block {number} seal does not verify against {}", seal.proposer_did);
    }
    Ok(())
}

/// Who should produce `height`, given the parent block's age.
///
/// `authorities` must be sorted. The primary producer is
/// `authorities[height % n]`; every further `grace_secs` without a block hands
/// the turn to the next authority.
pub fn scheduled_producer<'a>(
    authorities: &'a [String],
    height: u64,
    secs_since_parent: u64,
    grace_secs: u64,
) -> Option<&'a str> {
    if authorities.is_empty() {
        return None;
    }
    let n = authorities.len() as u64;
    let offset = if grace_secs == 0 {
        0
    } else {
        secs_since_parent / grace_secs
    };
    let index = (height.wrapping_add(offset)) % n;
    Some(authorities[index as usize].as_str())
}

#[derive(Debug, Serialize, Deserialize)]
struct SealLine {
    number: u64,
    block_hash: String,
    seal: BlockSeal,
}

/// Accepted seals by block number, persisted as JSON lines.
///
/// Seals are large (a SPHINCS+ signature is about 30 KB), so only the most
/// recent ones stay in memory; older ones are read back from the file through
/// an index of line offsets.
pub struct SealStore {
    path: Option<PathBuf>,
    inner: std::sync::RwLock<SealIndex>,
}

#[derive(Default)]
struct SealIndex {
    recent: BTreeMap<u64, (String, BlockSeal)>,
    /// Every persisted seal: block number → (block hash, byte offset).
    offsets: BTreeMap<u64, (String, u64)>,
    file_len: u64,
}

/// Seals kept in memory.
const RECENT_SEALS: usize = 2_048;

impl SealIndex {
    fn keep_recent(&mut self, number: u64, hash: String, seal: BlockSeal) {
        self.recent.insert(number, (hash, seal));
        while self.recent.len() > RECENT_SEALS {
            let first = *self.recent.keys().next().expect("non-empty");
            self.recent.remove(&first);
        }
    }
}

impl SealStore {
    pub fn open(path: Option<PathBuf>) -> Self {
        use std::io::BufRead;
        let mut index = SealIndex::default();
        if let Some(p) = &path {
            if let Ok(file) = std::fs::File::open(p) {
                let mut reader = std::io::BufReader::new(file);
                let mut offset = 0u64;
                let mut line = String::new();
                loop {
                    line.clear();
                    match reader.read_line(&mut line) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if let Ok(entry) = serde_json::from_str::<SealLine>(line.trim()) {
                                index
                                    .offsets
                                    .insert(entry.number, (entry.block_hash.clone(), offset));
                                index.keep_recent(entry.number, entry.block_hash, entry.seal);
                            }
                            offset += n as u64;
                        }
                    }
                }
                index.file_len = offset;
            }
        }
        Self {
            path,
            inner: std::sync::RwLock::new(index),
        }
    }

    /// `SPACEKIT_SEAL_STORE_PATH`, default `temp_blockchain_storage/block_seals.jsonl`.
    pub fn from_env() -> Self {
        let path = std::env::var("SPACEKIT_SEAL_STORE_PATH")
            .unwrap_or_else(|_| "temp_blockchain_storage/block_seals.jsonl".to_string());
        Self::open(Some(PathBuf::from(path)))
    }

    pub fn get(&self, number: u64, block_hash: &[u8; 32]) -> Option<BlockSeal> {
        use std::io::{BufRead, Seek, SeekFrom};
        let hash = hex::encode(block_hash);
        let offset = {
            let index = self.inner.read().unwrap_or_else(|p| p.into_inner());
            if let Some((h, seal)) = index.recent.get(&number) {
                return (*h == hash).then(|| seal.clone());
            }
            match index.offsets.get(&number) {
                Some((h, offset)) if *h == hash => *offset,
                _ => return None,
            }
        };
        let file = std::fs::File::open(self.path.as_ref()?).ok()?;
        let mut reader = std::io::BufReader::new(file);
        reader.seek(SeekFrom::Start(offset)).ok()?;
        let mut line = String::new();
        reader.read_line(&mut line).ok()?;
        serde_json::from_str::<SealLine>(line.trim())
            .ok()
            .filter(|e| e.number == number && e.block_hash == hash)
            .map(|e| e.seal)
    }

    pub fn insert(&self, number: u64, block_hash: &[u8; 32], seal: BlockSeal) {
        let hash = hex::encode(block_hash);
        let mut index = self.inner.write().unwrap_or_else(|p| p.into_inner());
        if index
            .recent
            .get(&number)
            .is_some_and(|(h, s)| *h == hash && *s == seal)
        {
            return;
        }
        if let Some(p) = &self.path {
            let line = SealLine {
                number,
                block_hash: hash.clone(),
                seal: seal.clone(),
            };
            let result = (|| -> Result<u64> {
                if let Some(dir) = p.parent() {
                    if !dir.as_os_str().is_empty() {
                        std::fs::create_dir_all(dir)?;
                    }
                }
                let text = format!("{}\n", serde_json::to_string(&line)?);
                let mut f = std::fs::OpenOptions::new().create(true).append(true).open(p)?;
                f.write_all(text.as_bytes())?;
                Ok(text.len() as u64)
            })();
            match result {
                Ok(written) => {
                    let offset = index.file_len;
                    index.offsets.insert(number, (hash.clone(), offset));
                    index.file_len += written;
                }
                Err(e) => tracing::warn!("could not persist block seal to {}: {e}", p.display()),
            }
        }
        index.keep_recent(number, hash, seal);
    }

    pub fn len(&self) -> usize {
        let index = self.inner.read().unwrap_or_else(|p| p.into_inner());
        index.offsets.len().max(index.recent.len())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Sealing for one node: its own signer (if it is an authority or
/// validator) and the seal store. Who may seal comes from chain state
/// (`chain_consensus::producer_set`).
pub struct BlockSealing {
    pub signer: Option<std::sync::Arc<AuthoritySigner>>,
    pub store: SealStore,
}

impl BlockSealing {
    /// Seal a block this node produced. `None` when this node has no
    /// authority key (its blocks will be rejected while seals are required).
    pub async fn seal_local(&self, chain_id: &str, number: u64, block_hash: [u8; 32]) -> Option<BlockSeal> {
        let signer = self.signer.clone()?;
        let chain_id = chain_id.to_string();
        // SPHINCS+ signing takes a noticeable fraction of a second.
        let sealed = tokio::task::spawn_blocking(move || signer.seal(&chain_id, number, &block_hash))
            .await
            .map_err(|e| anyhow!("seal task: {e}"))
            .and_then(|r| r);
        match sealed {
            Ok(seal) => {
                self.store.insert(number, &block_hash, seal.clone());
                Some(seal)
            }
            Err(e) => {
                tracing::warn!("could not seal block {number}: {e}");
                None
            }
        }
    }

    /// Accept or reject a peer's block against a producer set: the set of
    /// the block's parent state when the block extends the head. A chain with
    /// no PoA genesis (empty set) accepts unsealed blocks, as before.
    pub fn verify(
        &self,
        producers: &crate::chain_consensus::ProducerSet,
        seal: Option<&BlockSeal>,
        chain_id: &str,
        block: &crate::spacekitvm::SwtchvmBlock,
    ) -> Result<()> {
        if producers.is_empty() {
            return Ok(());
        }
        let number = block.number;
        let seal = seal.ok_or_else(|| anyhow!("block {number} has no seal"))?;
        if block.proposer_did.as_deref() != Some(seal.proposer_did.as_str()) {
            bail!(
                "block {number} names proposer {:?} but is sealed by {}",
                block.proposer_did,
                seal.proposer_did
            );
        }
        verify_seal(seal, chain_id, number, &block.hash, |did| producers.key(did))
    }

    /// Remember a verified peer seal so it can be served to late joiners.
    pub fn remember(&self, number: u64, block_hash: &[u8; 32], seal: Option<BlockSeal>) {
        if let Some(seal) = seal {
            self.store.insert(number, block_hash, seal);
        }
    }
}

/// Short fingerprint of an authority set, for logs.
pub fn authority_set_fingerprint(authorities: &[String]) -> String {
    hex::encode(&Sha256::digest(authorities.join("\n").as_bytes())[..6])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn authorities() -> Vec<String> {
        vec!["a".into(), "b".into(), "c".into(), "d".into()]
    }

    #[test]
    fn schedule_rotates_by_height_and_falls_back() {
        let a = authorities();
        assert_eq!(scheduled_producer(&a, 1, 0, 30), Some("b"));
        assert_eq!(scheduled_producer(&a, 4, 0, 30), Some("a"));
        // Primary missed its turn for one grace period: next authority.
        assert_eq!(scheduled_producer(&a, 1, 30, 30), Some("c"));
        assert_eq!(scheduled_producer(&a, 1, 95, 30), Some("a"));
        assert_eq!(scheduled_producer(&[], 1, 0, 30), None);
    }

    #[test]
    fn seal_roundtrip_and_store() {
        let kp = spacekit_did::sphincs::SphincsPlus::generate_keypair();
        let addr = hex::encode(&Sha256::digest(&kp.public_key)[..20]);
        let did = format!("did:spacekit:testnet:{addr}");
        let dir = std::env::temp_dir().join(format!("seal-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let wallet = dir.join("w.json");
        std::fs::write(
            &wallet,
            serde_json::json!({
                "did": did,
                "sphincs_pk_hex": hex::encode(&kp.public_key),
                "sphincs_sk_hex": hex::encode(&kp.private_key),
            })
            .to_string(),
        )
        .unwrap();
        let signer = AuthoritySigner::from_wallet_file(&wallet).unwrap();
        let hash = [7u8; 32];
        let seal = signer.seal("7777", 5, &hash).unwrap();
        let pk = kp.public_key.clone();
        let lookup = |d: &str| (d == did).then(|| pk.clone());
        verify_seal(&seal, "7777", 5, &hash, lookup).unwrap();
        assert!(verify_seal(&seal, "7777", 6, &hash, lookup).is_err());
        assert!(verify_seal(&seal, "mainnet", 5, &hash, lookup).is_err());
        assert!(verify_seal(&seal, "7777", 5, &hash, |_| None).is_err());

        let path = dir.join("seals.jsonl");
        let _ = std::fs::remove_file(&path);
        let store = SealStore::open(Some(path.clone()));
        store.insert(5, &hash, seal.clone());
        store.insert(5, &hash, seal.clone());
        let reopened = SealStore::open(Some(path.clone()));
        assert_eq!(reopened.get(5, &hash), Some(seal.clone()));
        assert_eq!(reopened.get(5, &[8u8; 32]), None);

        // Older seals fall out of memory and are read back from the file.
        for n in 6..(6 + RECENT_SEALS as u64 + 10) {
            reopened.insert(n, &[n as u8; 32], seal.clone());
        }
        assert!(reopened.inner.read().unwrap().recent.get(&5).is_none());
        assert_eq!(reopened.get(5, &hash), Some(seal.clone()));
        assert_eq!(reopened.get(7, &[7u8; 32]), Some(seal));
        assert_eq!(reopened.len(), RECENT_SEALS + 11);
        let _ = std::fs::remove_file(&path);
    }
}
