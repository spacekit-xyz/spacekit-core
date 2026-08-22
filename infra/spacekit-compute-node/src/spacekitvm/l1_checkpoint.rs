//! L1-oriented checkpoint metadata and SwtchVM snapshot manifests.
//!
//! Snapshot blob format (v1): `SNAPSHOT_MAGIC` + `SNAPSHOT_WIRE_VERSION` (LE u32) + bincode(`SwtchvmState`).
//! Legacy files with no header are still loaded (bincode only).
//!
//! A JSON sidecar (`*.manifest.json`) records integrity (`blob_sha256_hex`), `chain_id`, optional
//! [`SnapshotManifest::proposer_did`] (aligned with identity-native consensus narrative), and an
//! [`L1CheckpointHeader`] whose `tx_root_hex` is the **quantum Verkle** root
//! ([`spacekit_quantum_verkle::QuantumTree`] with [`NistSisScheme`]) over the batch of
//! SHA-256 transaction digests (empty batch → zero hash). This matches the same Verkle stack as
//! SwtchVM world state and `crate::state_anchor`, not a plain binary Merkle tree.
//!
//! Non-empty tx batches persist [`L1CheckpointHeader::verkle_witness_summary`] as JSON (digests +
//! [`QuantumMultiProof`] for the batch, scheme [`TX_ROOT_SCHEME_QUANTUM_VERKLE_V1`]). Legacy manifests
//! may use per-digest [`QuantumProof`]s in `inclusion_proofs_hex` instead of `multiproof_hex`.

use std::fs;
use std::path::{Path, PathBuf};

use alloy_primitives::{Address, B256, U256};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use spacekit_quantum_verkle::{NistSisScheme, QuantumMultiProof, QuantumProof, QuantumTree};

/// File magic for versioned SwtchVM snapshot files.
pub const SNAPSHOT_MAGIC: &[u8; 4] = b"SKVM";

/// Wire format version stored after magic.
pub const SNAPSHOT_WIRE_VERSION: u32 = 1;

/// JSON `manifest_version` for [`SnapshotManifest`].
pub const SNAPSHOT_MANIFEST_VERSION: u32 = 1;

/// `tx_root_hex` / witness encoding for L1 manifests (JSON `tx_root_scheme` default).
pub const TX_ROOT_SCHEME_QUANTUM_VERKLE_V1: &str = "quantum-verkle-v1";

fn default_tx_root_scheme() -> String {
    TX_ROOT_SCHEME_QUANTUM_VERKLE_V1.to_string()
}

/// JSON shape stored in [`L1CheckpointHeader::verkle_witness_summary`] for [`TX_ROOT_SCHEME_QUANTUM_VERKLE_V1`].
///
/// Prefer **`multiproof_hex`**: one [`QuantumMultiProof::to_bytes`] for the whole digest batch.
/// **`inclusion_proofs_hex`** is accepted for older manifests only.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TxBatchVerkleWitnessV1 {
    pub scheme: String,
    pub digests_hex: Vec<String>,
    /// Batch multiproof: `0x` + hex of [`QuantumMultiProof::to_bytes::<NistSisScheme>()`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub multiproof_hex: Option<String>,
    /// Legacy: per-digest proofs when `multiproof_hex` is absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inclusion_proofs_hex: Option<Vec<String>>,
}

/// Configuration threaded from the compute node (or env for tests / CLI).
#[derive(Debug, Clone)]
pub struct L1PersistenceConfig {
    pub chain_id: String,
    /// If true, missing manifest, hash mismatch, state_root mismatch, or chain_id mismatch is fatal on load.
    pub strict_manifest_verify: bool,
    /// Optional proposer / node DID stored on each manifest (unified-consensus “identity-native” hook).
    pub proposer_did: Option<String>,
}

impl Default for L1PersistenceConfig {
    fn default() -> Self {
        Self {
            chain_id: "spacekit-local".to_string(),
            strict_manifest_verify: false,
            proposer_did: None,
        }
    }
}

impl L1PersistenceConfig {
    pub fn from_env() -> Self {
        let chain_id =
            std::env::var("SPACEKIT_CHAIN_ID").unwrap_or_else(|_| "spacekit-local".into());
        let strict_manifest_verify = std::env::var("SPACEKIT_SNAPSHOT_STRICT")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        let proposer_did = std::env::var("SPACEKIT_PROPOSER_DID")
            .ok()
            .filter(|s| !s.is_empty());
        Self {
            chain_id,
            strict_manifest_verify,
            proposer_did,
        }
    }
}

/// Minimal L1-style header carried beside each snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct L1CheckpointHeader {
    pub height: u64,
    pub parent_hash_hex: String,
    pub state_root_hex: String,
    pub tx_root_hex: String,
    /// How to interpret `tx_root_hex` and [`Self::verkle_witness_summary`].
    #[serde(default = "default_tx_root_scheme")]
    pub tx_root_scheme: String,
    /// JSON [`TxBatchVerkleWitnessV1`] when the committed tx batch is non-empty; omitted if batch empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verkle_witness_summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotManifest {
    pub manifest_version: u32,
    pub chain_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposer_did: Option<String>,
    pub checkpoint: L1CheckpointHeader,
    /// SHA-256 hex (lowercase, no 0x) of the **exact** on-disk snapshot file bytes.
    pub blob_sha256_hex: String,
    pub written_at_rfc3339: String,
}

pub fn manifest_path_for_snapshot(bin_path: &Path) -> PathBuf {
    let mut s = bin_path.to_string_lossy().into_owned();
    s.push_str(".manifest.json");
    PathBuf::from(s)
}

pub fn zero_hash_hex() -> String {
    format!("0x{}", hex::encode([0u8; 32]))
}

fn sha256_hex_bytes(data: &[u8]) -> String {
    let h = Sha256::digest(data);
    hex::encode(h)
}

/// Synthetic EVM-style “contract address” for the **L1 tx-batch** Verkle trie only (must not collide
/// with real SWTCHVM account addresses in the state trie). Distinct from `state_anchor` DID registry.
pub const TX_BATCH_VERKLE_ADDRESS: Address = Address::new({
    let mut a = [0u8; 20];
    a[19] = 0x03;
    a
});

/// `0x` + hex **quantum Verkle** root for `tx_digests` (insert order matters; each leaf uses digest as both key and value hash).
///
/// Empty slice → [`zero_hash_hex`] (no Verkle update for that snapshot batch).
pub fn tx_batch_verkle_root_hex(tx_digests: &[[u8; 32]]) -> String {
    if tx_digests.is_empty() {
        return zero_hash_hex();
    }
    let mut tree = QuantumTree::<NistSisScheme>::new();
    let addr = TX_BATCH_VERKLE_ADDRESS;
    for d in tx_digests {
        let key = B256::from_slice(d.as_slice());
        let value = U256::from_be_bytes::<32>((*d).into());
        tree.set(&addr, &key, value);
    }
    format!("0x{}", hex::encode(tree.root().0))
}

fn strip_0x(s: &str) -> &str {
    s.strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .unwrap_or(s)
}

fn parse_digest32_hex(s: &str) -> Result<[u8; 32]> {
    let raw = hex::decode(strip_0x(s)).context("tx digest hex decode")?;
    if raw.len() != 32 {
        anyhow::bail!("tx digest must be 32 bytes, got {}", raw.len());
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&raw);
    Ok(out)
}

/// Thin default for serde / tests; not a valid committed manifest.
impl Default for L1CheckpointHeader {
    fn default() -> Self {
        Self {
            height: 0,
            parent_hash_hex: zero_hash_hex(),
            state_root_hex: zero_hash_hex(),
            tx_root_hex: zero_hash_hex(),
            tx_root_scheme: TX_ROOT_SCHEME_QUANTUM_VERKLE_V1.to_string(),
            verkle_witness_summary: None,
        }
    }
}

impl Default for SnapshotManifest {
    fn default() -> Self {
        Self {
            manifest_version: SNAPSHOT_MANIFEST_VERSION,
            chain_id: String::new(),
            proposer_did: None,
            checkpoint: L1CheckpointHeader::default(),
            blob_sha256_hex: "0".repeat(64),
            written_at_rfc3339: chrono::Utc::now().to_rfc3339(),
        }
    }
}

/// Minimal L1 manifest for tests or off-chain proposals when blob bytes are not yet known.
pub fn minimal_l1_manifest_for_proposal(
    chain_id: &str,
    state_root_hex: &str,
    height: u64,
    parent_state_root_hex: &str,
) -> SnapshotManifest {
    SnapshotManifest {
        manifest_version: SNAPSHOT_MANIFEST_VERSION,
        chain_id: chain_id.to_string(),
        proposer_did: None,
        checkpoint: L1CheckpointHeader {
            height,
            parent_hash_hex: parent_state_root_hex.to_string(),
            state_root_hex: state_root_hex.to_string(),
            tx_root_hex: zero_hash_hex(),
            tx_root_scheme: TX_ROOT_SCHEME_QUANTUM_VERKLE_V1.to_string(),
            verkle_witness_summary: None,
        },
        blob_sha256_hex: "0".repeat(64),
        written_at_rfc3339: chrono::Utc::now().to_rfc3339(),
    }
}

fn encode_tx_batch_witness_v1(
    digests: &[[u8; 32]],
    tree: &QuantumTree<NistSisScheme>,
) -> Result<String> {
    let addr = TX_BATCH_VERKLE_ADDRESS;
    let keys: Vec<(Address, B256)> = digests
        .iter()
        .map(|d| (addr, B256::from_slice(d.as_slice())))
        .collect();
    let proof = tree
        .create_multi_proof(keys)
        .map_err(|e| anyhow::anyhow!("tx-batch verkle multiproof: {e}"))?;
    let multiproof_hex = format!("0x{}", hex::encode(proof.to_bytes::<NistSisScheme>()));
    let w = TxBatchVerkleWitnessV1 {
        scheme: TX_ROOT_SCHEME_QUANTUM_VERKLE_V1.to_string(),
        digests_hex: digests
            .iter()
            .map(|d| format!("0x{}", hex::encode(d)))
            .collect(),
        multiproof_hex: Some(multiproof_hex),
        inclusion_proofs_hex: None,
    };
    serde_json::to_string(&w).context("serialize TxBatchVerkleWitnessV1")
}

/// Recompute [`L1CheckpointHeader::tx_root_hex`] and optional witness JSON for `tx_digests`.
pub fn tx_batch_verkle_checkpoint_fields(
    tx_digests: &[[u8; 32]],
) -> Result<(String, Option<String>)> {
    if tx_digests.is_empty() {
        return Ok((zero_hash_hex(), None));
    }
    let mut tree = QuantumTree::<NistSisScheme>::new();
    let addr = TX_BATCH_VERKLE_ADDRESS;
    for d in tx_digests {
        let key = B256::from_slice(d.as_slice());
        let value = U256::from_be_bytes::<32>((*d).into());
        tree.set(&addr, &key, value);
    }
    let root_hex = format!("0x{}", hex::encode(tree.root().0));
    let witness_json = encode_tx_batch_witness_v1(tx_digests, &tree)?;
    Ok((root_hex, Some(witness_json)))
}

/// Verify [`TxBatchVerkleWitnessV1`] against `tx_root_hex` for scheme [`TX_ROOT_SCHEME_QUANTUM_VERKLE_V1`].
pub fn verify_l1_tx_batch_witness_json(tx_root_hex: &str, witness_json: &str) -> Result<()> {
    let w: TxBatchVerkleWitnessV1 =
        serde_json::from_str(witness_json).context("parse verkle_witness_summary JSON")?;
    if w.scheme != TX_ROOT_SCHEME_QUANTUM_VERKLE_V1 {
        anyhow::bail!(
            "witness.scheme {:?} != {:?}",
            w.scheme,
            TX_ROOT_SCHEME_QUANTUM_VERKLE_V1
        );
    }
    let mut digests: Vec<[u8; 32]> = Vec::with_capacity(w.digests_hex.len());
    for h in &w.digests_hex {
        digests.push(parse_digest32_hex(h)?);
    }
    let digests_sl = digests.as_slice();
    let expect_root = tx_batch_verkle_root_hex(digests_sl);
    if expect_root != tx_root_hex {
        anyhow::bail!(
            "tx_root_hex {} does not match witness digests (expect {})",
            tx_root_hex,
            expect_root
        );
    }
    let use_multi = w
        .multiproof_hex
        .as_ref()
        .map(|s| {
            let t = s.trim();
            !t.is_empty() && t != "0x"
        })
        .unwrap_or(false);

    if use_multi {
        let h = w.multiproof_hex.as_ref().unwrap();
        let mp_raw = hex::decode(strip_0x(h)).context("decode multiproof_hex")?;
        let mp = QuantumMultiProof::from_bytes::<NistSisScheme>(&mp_raw)
            .map_err(|e| anyhow::anyhow!(e))?;
        let mut tree = QuantumTree::<NistSisScheme>::new();
        let addr = TX_BATCH_VERKLE_ADDRESS;
        for d in digests_sl {
            let key = B256::from_slice(d.as_slice());
            let value = U256::from_be_bytes::<32>((*d).into());
            tree.set(&addr, &key, value);
        }
        let keys: Vec<(Address, B256)> = digests_sl
            .iter()
            .map(|d| (addr, B256::from_slice(d.as_slice())))
            .collect();
        let values: Vec<U256> = digests_sl
            .iter()
            .map(|d| U256::from_be_bytes::<32>((*d).into()))
            .collect();
        if !tree.verify_multi_proof(&mp, keys, values) {
            anyhow::bail!("verify_multi_proof failed");
        }
        return Ok(());
    }

    let Some(inclusion) = w.inclusion_proofs_hex.as_ref() else {
        anyhow::bail!("witness must set multiproof_hex or inclusion_proofs_hex");
    };
    if inclusion.len() != w.digests_hex.len() {
        anyhow::bail!(
            "witness digests length {} != legacy inclusion_proofs length {}",
            w.digests_hex.len(),
            inclusion.len()
        );
    }
    let mut tree = QuantumTree::<NistSisScheme>::new();
    let addr = TX_BATCH_VERKLE_ADDRESS;
    for d in digests_sl {
        let key = B256::from_slice(d.as_slice());
        let value = U256::from_be_bytes::<32>((*d).into());
        tree.set(&addr, &key, value);
    }
    for (i, d) in digests_sl.iter().enumerate() {
        let key = B256::from_slice(d.as_slice());
        let value = U256::from_be_bytes::<32>((*d).into());
        let raw = hex::decode(strip_0x(&inclusion[i])).context("decode inclusion proof hex")?;
        let proof =
            QuantumProof::from_bytes::<NistSisScheme>(&raw).map_err(|e| anyhow::anyhow!(e))?;
        if !tree.verify_proof(&proof, &addr, &key, value) {
            anyhow::bail!("inclusion proof {i} failed verify_proof");
        }
    }
    Ok(())
}

/// Wrap bincode payload for new writes.
pub fn wrap_snapshot_payload(bincode_payload: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(8 + bincode_payload.len());
    v.extend_from_slice(SNAPSHOT_MAGIC);
    v.extend_from_slice(&SNAPSHOT_WIRE_VERSION.to_le_bytes());
    v.extend_from_slice(bincode_payload);
    v
}

/// Strip known header or pass through legacy whole-file bincode.
pub fn unwrap_snapshot_file_bytes(data: &[u8]) -> Result<&[u8]> {
    if data.len() >= 8 && data[..4] == SNAPSHOT_MAGIC[..] {
        let ver = u32::from_le_bytes(
            data[4..8]
                .try_into()
                .map_err(|_| anyhow::anyhow!("snapshot version bytes"))?,
        );
        if ver != SNAPSHOT_WIRE_VERSION {
            anyhow::bail!(
                "unsupported SwtchVM snapshot wire version {} (this binary supports {})",
                ver,
                SNAPSHOT_WIRE_VERSION
            );
        }
        Ok(&data[8..])
    } else {
        Ok(data)
    }
}

pub fn read_manifest_optional(bin_pr: &Path) -> Result<Option<SnapshotManifest>> {
    let mp = manifest_path_for_snapshot(bin_pr);
    if !mp.is_file() {
        return Ok(None);
    }
    let txt = fs::read_to_string(&mp).with_context(|| format!("read manifest {}", mp.display()))?;
    let m: SnapshotManifest =
        serde_json::from_str(&txt).with_context(|| format!("parse manifest {}", mp.display()))?;
    Ok(Some(m))
}

/// Verify manifest vs on-disk blob and config. `state_root_hex` is the recomputed root after load.
pub fn verify_manifest_against_loaded(
    file_bytes: &[u8],
    state_root_hex: &str,
    manifest: &SnapshotManifest,
    l1: &L1PersistenceConfig,
) -> Result<()> {
    let digest = sha256_hex_bytes(file_bytes);
    if digest != manifest.blob_sha256_hex {
        let msg = format!(
            "snapshot blob hash mismatch (manifest {} vs file {})",
            manifest.blob_sha256_hex, digest
        );
        if l1.strict_manifest_verify {
            anyhow::bail!("{msg}");
        }
        tracing::warn!(target: "swtchvm", "{msg}");
    }

    if state_root_hex != manifest.checkpoint.state_root_hex {
        let msg = format!(
            "state_root mismatch after load (manifest {} vs recomputed {})",
            manifest.checkpoint.state_root_hex, state_root_hex
        );
        if l1.strict_manifest_verify {
            anyhow::bail!("{msg}");
        }
        tracing::warn!(target: "swtchvm", "{msg}");
    }

    if manifest.chain_id != l1.chain_id {
        let msg = format!(
            "chain_id mismatch (manifest {} vs config {})",
            manifest.chain_id, l1.chain_id
        );
        if l1.strict_manifest_verify {
            anyhow::bail!("{msg}");
        }
        tracing::warn!(target: "swtchvm", "{msg}");
    }

    if manifest.manifest_version != SNAPSHOT_MANIFEST_VERSION {
        if l1.strict_manifest_verify {
            anyhow::bail!(
                "manifest_version {} not supported (want {})",
                manifest.manifest_version,
                SNAPSHOT_MANIFEST_VERSION
            );
        }
        tracing::warn!(
            target: "swtchvm",
            "manifest_version {} differs from supported {}",
            manifest.manifest_version,
            SNAPSHOT_MANIFEST_VERSION
        );
    }

    if let Some(ref wit) = manifest.checkpoint.verkle_witness_summary {
        if !wit.trim().is_empty()
            && manifest.checkpoint.tx_root_scheme == TX_ROOT_SCHEME_QUANTUM_VERKLE_V1
        {
            match verify_l1_tx_batch_witness_json(&manifest.checkpoint.tx_root_hex, wit) {
                Ok(()) => {}
                Err(e) => {
                    let msg = format!("L1 tx-batch Verkle witness invalid: {e}");
                    if l1.strict_manifest_verify {
                        anyhow::bail!("{msg}");
                    }
                    tracing::warn!(target: "swtchvm", "{msg}");
                }
            }
        }
    }

    Ok(())
}

fn write_file_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create_dir_all {}", parent.display()))?;
    }
    let mut tmp = path.to_path_buf();
    let name = path
        .file_name()
        .map(|s| {
            let mut o = s.to_os_string();
            o.push(".tmp");
            o
        })
        .unwrap_or_else(|| std::ffi::OsString::from(".skvm-snap.tmp"));
    tmp.set_file_name(name);
    fs::write(&tmp, bytes).with_context(|| format!("write {}", tmp.display()))?;
    fs::rename(&tmp, path)
        .with_context(|| format!("rename {} -> {}", tmp.display(), path.display()))?;
    Ok(())
}

fn write_manifest_atomic(bin_path: &Path, manifest: &SnapshotManifest) -> Result<()> {
    let mp = manifest_path_for_snapshot(bin_path);
    let json = serde_json::to_string_pretty(manifest).context("serialize SnapshotManifest")?;
    let fname = mp
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("manifest path has no file name"))?;
    let mut tname = fname.to_os_string();
    tname.push(".tmp");
    let mut tmp = mp.clone();
    tmp.set_file_name(tname);
    if let Some(parent) = mp.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&tmp, json.as_bytes()).with_context(|| format!("write {}", tmp.display()))?;
    fs::rename(&tmp, &mp).with_context(|| format!("rename manifest {}", mp.display()))?;
    Ok(())
}

/// Write wrapped snapshot bytes and manifest. `state_root_hex` must match post-load root for [`SwtchvmState`].
pub fn persist_swvm_snapshot(
    bin_path: &Path,
    wrapped_bytes: &[u8],
    state_root_hex: &str,
    l1: &L1PersistenceConfig,
    tx_digests: &[[u8; 32]],
) -> Result<()> {
    let (height, parent_hash_hex) = match read_manifest_optional(bin_path)? {
        Some(prev) => (
            prev.checkpoint.height.saturating_add(1),
            prev.checkpoint.state_root_hex,
        ),
        None => (0u64, zero_hash_hex()),
    };

    let (tx_root_hex, verkle_witness_summary) = tx_batch_verkle_checkpoint_fields(tx_digests)?;

    let manifest = SnapshotManifest {
        manifest_version: SNAPSHOT_MANIFEST_VERSION,
        chain_id: l1.chain_id.clone(),
        proposer_did: l1.proposer_did.clone(),
        checkpoint: L1CheckpointHeader {
            height,
            parent_hash_hex,
            state_root_hex: state_root_hex.to_string(),
            tx_root_hex,
            tx_root_scheme: TX_ROOT_SCHEME_QUANTUM_VERKLE_V1.to_string(),
            verkle_witness_summary,
        },
        blob_sha256_hex: sha256_hex_bytes(wrapped_bytes),
        written_at_rfc3339: chrono::Utc::now().to_rfc3339(),
    };

    write_file_atomic(bin_path, wrapped_bytes)?;
    write_manifest_atomic(bin_path, &manifest)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn witness_roundtrip_verify_without_file() -> Result<()> {
        let digests = [[9u8; 32]];
        let (root, wit) = tx_batch_verkle_checkpoint_fields(&digests)?;
        verify_l1_tx_batch_witness_json(&root, &wit.expect("witness"))?;
        Ok(())
    }

    #[test]
    fn witness_roundtrip_two_digests() -> Result<()> {
        let digests = [[1u8; 32], [2u8; 32]];
        let (root, wit) = tx_batch_verkle_checkpoint_fields(&digests)?;
        verify_l1_tx_batch_witness_json(&root, &wit.expect("witness"))?;
        Ok(())
    }

    #[test]
    fn persist_includes_tx_verkle_root_and_proposer() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let bin = dir.path().join("s.bin");
        let l1 = L1PersistenceConfig {
            chain_id: "c".into(),
            strict_manifest_verify: false,
            proposer_did: Some("did:spacekit:test-proposer".into()),
        };
        let leaf = [9u8; 32];
        persist_swvm_snapshot(
            &bin,
            &wrap_snapshot_payload(b"payload"),
            &zero_hash_hex(),
            &l1,
            &[leaf],
        )?;
        let m: SnapshotManifest =
            serde_json::from_str(&fs::read_to_string(manifest_path_for_snapshot(&bin))?)?;
        assert_eq!(
            m.proposer_did.as_deref(),
            Some("did:spacekit:test-proposer")
        );
        assert_eq!(m.checkpoint.tx_root_hex, tx_batch_verkle_root_hex(&[leaf]));
        assert_eq!(
            m.checkpoint.tx_root_scheme,
            TX_ROOT_SCHEME_QUANTUM_VERKLE_V1
        );
        let wit = m
            .checkpoint
            .verkle_witness_summary
            .as_deref()
            .expect("non-empty batch must carry witness");
        let v: TxBatchVerkleWitnessV1 = serde_json::from_str(wit)?;
        assert!(v.multiproof_hex.is_some());
        assert!(v.inclusion_proofs_hex.is_none());
        verify_l1_tx_batch_witness_json(&m.checkpoint.tx_root_hex, wit)?;
        Ok(())
    }

    #[test]
    fn unwrap_rejects_unknown_wire_version() {
        let mut bad = Vec::new();
        bad.extend_from_slice(SNAPSHOT_MAGIC);
        bad.extend_from_slice(&999u32.to_le_bytes());
        bad.extend_from_slice(&[1, 2, 3]);
        let e = unwrap_snapshot_file_bytes(&bad).unwrap_err();
        let s = e.to_string();
        assert!(s.contains("unsupported"), "{}", s);
    }

    #[test]
    fn strict_fails_on_tampered_manifest() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let bin = dir.path().join("state.bin");
        let l1 = L1PersistenceConfig {
            chain_id: "test-chain".into(),
            strict_manifest_verify: true,
            proposer_did: None,
        };
        let payload = vec![1u8, 2, 3, 4];
        let wrapped = wrap_snapshot_payload(&payload);
        let root = zero_hash_hex();
        persist_swvm_snapshot(&bin, &wrapped, &root, &l1, &[])?;

        let mp = manifest_path_for_snapshot(&bin);
        let mut m: SnapshotManifest = serde_json::from_str(&fs::read_to_string(&mp)?)?;
        m.blob_sha256_hex = "0".repeat(64);
        fs::write(&mp, serde_json::to_string_pretty(&m)?)?;

        let file_bytes = fs::read(&bin)?;
        let err = verify_manifest_against_loaded(
            &file_bytes,
            &root,
            &serde_json::from_str(&fs::read_to_string(&mp)?)?,
            &l1,
        )
        .unwrap_err();
        assert!(err.to_string().contains("hash mismatch"), "{}", err);
        Ok(())
    }

    #[test]
    fn strict_fails_on_tampered_tx_verkle_witness() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let bin = dir.path().join("state.bin");
        let l1 = L1PersistenceConfig {
            chain_id: "chain".into(),
            strict_manifest_verify: true,
            proposer_did: None,
        };
        let leaf = [2u8; 32]; // avoid collision with other tests' pattern if manifests ever shared
        persist_swvm_snapshot(
            &bin,
            &wrap_snapshot_payload(b"blob"),
            &zero_hash_hex(),
            &l1,
            &[leaf],
        )?;

        let mp = manifest_path_for_snapshot(&bin);
        let mut m: SnapshotManifest = serde_json::from_str(&fs::read_to_string(&mp)?)?;
        let Some(ref wj) = m.checkpoint.verkle_witness_summary else {
            anyhow::bail!("expected witness");
        };
        let mut wit: TxBatchVerkleWitnessV1 = serde_json::from_str(wj)?;
        wit.multiproof_hex = Some("0x00".to_string());
        m.checkpoint.verkle_witness_summary = Some(serde_json::to_string(&wit)?);
        fs::write(&mp, serde_json::to_string_pretty(&m)?)?;

        let file_bytes = fs::read(&bin)?;
        let err = verify_manifest_against_loaded(
            &file_bytes,
            &zero_hash_hex(),
            &serde_json::from_str(&fs::read_to_string(&mp)?)?,
            &l1,
        )
        .unwrap_err();
        let s = err.to_string();
        assert!(
            s.contains("witness invalid")
                || s.contains("multiproof")
                || s.contains("Serialization"),
            "{s}"
        );
        Ok(())
    }

    #[test]
    fn verkle_tx_batch_single_digest_nonzero() {
        let d = [[7u8; 32]];
        let h = tx_batch_verkle_root_hex(&d);
        assert_ne!(h, zero_hash_hex());
        assert!(h.starts_with("0x"));
    }
}
