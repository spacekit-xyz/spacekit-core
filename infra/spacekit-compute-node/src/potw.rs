//! Proof of Tangible Works (PoTW) host-side award accumulator.
//!
//! PoTW is how **the system, not the treasury, awards ASTRA** for work that a
//! quorum of reviewers attests actually happened. It is the emission-side
//! counterpart to the `Treasury` contract:
//!
//!   * `Treasury` (SKCL WASM contract) only *moves* a finite, pre-funded pool
//!     under M-of-N governance. It has **no mint authority**.
//!   * `PoTW` (this module) verifies an M-of-N *reviewer* quorum over a work
//!     award and, on success, tells the host to **credit** the recipient via
//!     `AstraRewards`, the same admin-only, host-orchestrated `OP_CREDIT` mint
//!     path `SraHost` uses. The mint is cap-enforced by `AstraRewards` (2B
//!     ASTRA), so PoTW can never exceed the protocol supply schedule.
//!
//! This module is deliberately the *verifier and bookkeeper only*. It answers
//! one question, "is this award authorized, in-budget, and not a replay?", 
//! and returns an [`AwardInstruction`] the host acts on. It never mints; wiring
//! the returned instruction to `AstraRewards`/`SraHost` is the host integration
//! point (see `verify_and_award` docs).
//!
//! # Reviewer receipt
//!
//! A [`PoTWReceipt`] carries the award parameters plus a set of reviewer
//! approvals. Each approval is a **SLH-DSA-SHA2-128s** (FIPS-205) signature over
//! the *award digest* — the same post-quantum scheme the browser wallet and the
//! settlement layer use, verified here with
//! [`verify_slh_dsa_signature`](spacekit_primitives::v1::crypto::quantum::verify_slh_dsa_signature).
//! Signing the digest (not the raw fields) means a reviewer's signature commits
//! to *exactly* `(work_id, recipient, amount, epoch)` and cannot be lifted onto
//! a different award.
//!
//! # Award digest
//!
//! ```text
//! digest = SHA-256(
//!     "SPACEKIT-POTW-AWARD-v1\n"
//!  || work_id            (32 bytes)
//!  || recipient          (20 bytes, the PQ address)
//!  || amount   (u128 LE, 16 bytes, uASTRA)
//!  || epoch    (u64  LE,  8 bytes)
//! )
//! ```
//!
//! The domain separator makes a PoTW award digest un-confusable with a rollup
//! bundle hash or any other SpaceKit structure.
//!
//! # Guardrails (all enforced before an award is emitted)
//!
//!   * **Quorum** — at least `threshold` *distinct* allow-listed reviewers must
//!     sign the digest. A reviewer is counted at most once.
//!   * **Per-work cap** — a single award may not exceed `per_work_cap`.
//!   * **Per-epoch budget** — the sum of awards in an epoch may not exceed
//!     `epoch_budget`.
//!   * **Replay** — each `work_id` may be awarded at most once, ever.
//!
//! State (per-epoch spend + the set of already-awarded work ids) is held in
//! memory and, when a path is configured, persisted to a JSON file with an
//! atomic write so it survives restarts — mirroring `rollup_registry`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[cfg(feature = "quantum")]
use spacekit_primitives::v1::crypto::quantum::verify_slh_dsa_signature;

/// Domain separator for the PoTW award digest. Bumping the version invalidates
/// every previously-signed receipt.
pub const POTW_AWARD_DOMAIN: &[u8] = b"SPACEKIT-POTW-AWARD-v1\n";

/// Env var pointing at the JSON state file. When unset, the accumulator keeps
/// its state only in memory.
pub const POTW_STATE_PATH_ENV: &str = "SPACEKIT_POTW_STATE_PATH";

/// One reviewer's post-quantum attestation that a work award is legitimate.
///
/// Mirrors the settlement layer's `BundleSignature` encoding: the public key is
/// hex, the signature is base64. The signed message is the award digest.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewerApproval {
    /// SLH-DSA algorithm string, e.g. `"slh-dsa-sha2-128s"`.
    pub algorithm: String,
    /// Reviewer's SLH-DSA public key, hex-encoded (32 bytes for 128s).
    pub public_key_hex: String,
    /// SLH-DSA signature over the award digest, base64-encoded (7856 bytes for 128s).
    pub signature_base64: String,
}

/// A claim that a unit of tangible work merits an ASTRA award, together with the
/// reviewer quorum attesting to it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PoTWReceipt {
    /// Unique id for the work being rewarded. Also the replay key — awarded once.
    pub work_id_hex: String,
    /// Recipient PQ address (`0x` + 40 hex chars = 20 bytes) to be credited.
    pub recipient: String,
    /// Award amount in uASTRA (1 ASTRA = 1_000_000 uASTRA).
    #[serde(with = "amount_str")]
    pub amount: u128,
    /// Emission epoch this award is budgeted against.
    pub epoch: u64,
    /// Reviewer approvals (SLH-DSA signatures over the award digest).
    pub approvals: Vec<ReviewerApproval>,
}

/// Static governance parameters for the accumulator.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PoTWConfig {
    /// Allow-listed reviewer public keys, hex-encoded (lower-cased on load).
    pub reviewers: Vec<String>,
    /// Quorum size `M`: distinct valid reviewer signatures required.
    pub threshold: u64,
    /// Maximum total uASTRA that may be awarded within a single epoch.
    #[serde(with = "amount_str")]
    pub epoch_budget: u128,
    /// Maximum uASTRA for any one award.
    #[serde(with = "amount_str")]
    pub per_work_cap: u128,
}

/// The instruction returned once an award passes every guardrail. The host
/// performs the actual credit from this — PoTW itself never mints.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AwardInstruction {
    pub work_id_hex: String,
    pub recipient: String,
    #[serde(with = "amount_str")]
    pub amount: u128,
    pub epoch: u64,
    /// Hex of the digest the reviewers signed (audit trail).
    pub digest_hex: String,
    /// The distinct reviewer public keys whose signatures were accepted.
    pub approving_reviewers: Vec<String>,
    /// Epoch spend *after* this award is applied (audit trail).
    #[serde(with = "amount_str")]
    pub epoch_spent_after: u128,
}

/// Why an award was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PoTWError {
    NotInitialized,
    BadWorkId(String),
    BadRecipient(String),
    ZeroAmount,
    /// This `work_id` has already been awarded.
    DuplicateWork(String),
    /// Fewer than `threshold` distinct valid reviewer signatures.
    QuorumNotMet { got: u64, need: u64 },
    /// A signature was present from a key not on the reviewer allow-list.
    UnknownReviewer(String),
    /// A reviewer signature failed to verify against the award digest.
    InvalidSignature(String),
    /// The award exceeds the per-work cap.
    WorkCapExceeded { amount: u128, cap: u128 },
    /// The award would push the epoch over budget.
    EpochBudgetExceeded {
        epoch: u64,
        spent: u128,
        amount: u128,
        budget: u128,
    },
    /// Crypto verification is unavailable (built without the `quantum` feature).
    QuantumUnavailable,
    /// Decoding, I/O, or config error.
    Malformed(String),
}

impl core::fmt::Display for PoTWError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            PoTWError::NotInitialized => write!(f, "PoTW accumulator not initialized"),
            PoTWError::BadWorkId(s) => write!(f, "bad work_id: {s}"),
            PoTWError::BadRecipient(s) => write!(f, "bad recipient: {s}"),
            PoTWError::ZeroAmount => write!(f, "award amount must be non-zero"),
            PoTWError::DuplicateWork(id) => write!(f, "work {id} already awarded (replay)"),
            PoTWError::QuorumNotMet { got, need } => {
                write!(f, "reviewer quorum not met: {got}/{need}")
            }
            PoTWError::UnknownReviewer(k) => write!(f, "signature from non-reviewer key {k}"),
            PoTWError::InvalidSignature(k) => write!(f, "invalid reviewer signature from {k}"),
            PoTWError::WorkCapExceeded { amount, cap } => {
                write!(f, "award {amount} exceeds per-work cap {cap}")
            }
            PoTWError::EpochBudgetExceeded {
                epoch,
                spent,
                amount,
                budget,
            } => write!(
                f,
                "epoch {epoch} budget exceeded: spent {spent} + {amount} > {budget}"
            ),
            PoTWError::QuantumUnavailable => {
                write!(f, "quantum feature not enabled; cannot verify SLH-DSA")
            }
            PoTWError::Malformed(s) => write!(f, "malformed PoTW input: {s}"),
        }
    }
}

impl std::error::Error for PoTWError {}

/// Mutable bookkeeping: per-epoch spend and the set of awarded work ids.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PoTWState {
    /// epoch -> total uASTRA awarded so far (decimal string for u128 safety).
    epoch_spent: BTreeMap<u64, String>,
    /// hex(work_id) of every award already emitted.
    awarded_work: BTreeSet<String>,
}

/// Host-side PoTW accumulator: verifies reviewer quorums and tracks budget/replay.
#[derive(Debug, Clone)]
pub struct PoTWAccumulator {
    config: PoTWConfig,
    reviewers: BTreeSet<String>,
    state: PoTWState,
    path: Option<PathBuf>,
}

impl PoTWAccumulator {
    /// Build an in-memory accumulator (no persistence).
    pub fn new(config: PoTWConfig) -> Self {
        let reviewers = config
            .reviewers
            .iter()
            .map(|r| r.trim().to_lowercase())
            .collect();
        PoTWAccumulator {
            config,
            reviewers,
            state: PoTWState::default(),
            path: None,
        }
    }

    /// Build an accumulator persisted to `path`, loading prior state if the file
    /// exists. A relative path is resolved against the current working dir.
    pub fn load(config: PoTWConfig, path: PathBuf) -> Result<Self, PoTWError> {
        let mut acc = PoTWAccumulator::new(config);
        if path.exists() {
            let bytes = std::fs::read(&path)
                .map_err(|e| PoTWError::Malformed(format!("read state: {e}")))?;
            acc.state = serde_json::from_slice(&bytes)
                .map_err(|e| PoTWError::Malformed(format!("parse state: {e}")))?;
        }
        acc.path = Some(path);
        Ok(acc)
    }

    /// Resolve the state path from [`POTW_STATE_PATH_ENV`], if set.
    pub fn load_from_env(config: PoTWConfig) -> Result<Self, PoTWError> {
        match std::env::var(POTW_STATE_PATH_ENV) {
            Ok(p) if !p.trim().is_empty() => PoTWAccumulator::load(config, PathBuf::from(p)),
            _ => Ok(PoTWAccumulator::new(config)),
        }
    }

    pub fn config(&self) -> &PoTWConfig {
        &self.config
    }

    /// uASTRA already awarded in `epoch`.
    pub fn epoch_spent(&self, epoch: u64) -> u128 {
        self.state
            .epoch_spent
            .get(&epoch)
            .and_then(|s| s.parse::<u128>().ok())
            .unwrap_or(0)
    }

    /// Whether `work_id` has already been awarded.
    pub fn is_awarded(&self, work_id: &[u8; 32]) -> bool {
        self.state.awarded_work.contains(&hex::encode(work_id))
    }

    /// Verify a receipt **without** mutating state. Checks quorum, signatures,
    /// per-work cap, epoch budget, and replay against current state. Returns the
    /// instruction that *would* be emitted. Use this for a dry run; use
    /// [`verify_and_award`](Self::verify_and_award) to actually commit.
    pub fn verify_receipt(&self, receipt: &PoTWReceipt) -> Result<AwardInstruction, PoTWError> {
        let work_id = parse_work_id(&receipt.work_id_hex)?;
        let recipient20 = parse_recipient20(&receipt.recipient)?;
        if receipt.amount == 0 {
            return Err(PoTWError::ZeroAmount);
        }

        // Replay guard.
        if self.is_awarded(&work_id) {
            return Err(PoTWError::DuplicateWork(hex::encode(work_id)));
        }

        // Per-work cap.
        if receipt.amount > self.config.per_work_cap {
            return Err(PoTWError::WorkCapExceeded {
                amount: receipt.amount,
                cap: self.config.per_work_cap,
            });
        }

        // Epoch budget (checked against current spend).
        let spent = self.epoch_spent(receipt.epoch);
        let after = spent
            .checked_add(receipt.amount)
            .ok_or_else(|| PoTWError::Malformed("epoch spend overflow".into()))?;
        if after > self.config.epoch_budget {
            return Err(PoTWError::EpochBudgetExceeded {
                epoch: receipt.epoch,
                spent,
                amount: receipt.amount,
                budget: self.config.epoch_budget,
            });
        }

        // Quorum: verify each approval's signature over the digest and count
        // DISTINCT allow-listed reviewers.
        let digest = award_digest(&work_id, &recipient20, receipt.amount, receipt.epoch);
        let mut counted: BTreeSet<String> = BTreeSet::new();
        for approval in &receipt.approvals {
            let key = approval.public_key_hex.trim().to_lowercase();
            if !self.reviewers.contains(&key) {
                return Err(PoTWError::UnknownReviewer(key));
            }
            if counted.contains(&key) {
                continue; // a reviewer votes at most once
            }
            if verify_approval(&digest, approval)? {
                counted.insert(key);
            } else {
                return Err(PoTWError::InvalidSignature(key));
            }
        }

        let got = counted.len() as u64;
        if got < self.config.threshold {
            return Err(PoTWError::QuorumNotMet {
                got,
                need: self.config.threshold,
            });
        }

        Ok(AwardInstruction {
            work_id_hex: hex::encode(work_id),
            recipient: format!("0x{}", hex::encode(recipient20)),
            amount: receipt.amount,
            epoch: receipt.epoch,
            digest_hex: hex::encode(digest),
            approving_reviewers: counted.into_iter().collect(),
            epoch_spent_after: after,
        })
    }

    /// Verify a receipt and, on success, **commit** the award: record the epoch
    /// spend and the work id (replay guard), persist the state, and return the
    /// [`AwardInstruction`].
    ///
    /// # Host integration
    ///
    /// The returned instruction is what the host acts on to actually credit the
    /// recipient — it is *not* minted here. Mirroring `SraHost`, the host calls
    /// `AstraRewards` `OP_CREDIT` (admin-only, cap-enforced) via
    /// `SwtchvmRuntime::call_contract_public` for `(recipient, amount)`. Because
    /// this method commits budget/replay state *before* returning, the host must
    /// treat a returned instruction as spent: if the downstream credit call
    /// fails, reconcile rather than silently retrying a new receipt for the same
    /// `work_id` (which would now be rejected as a replay).
    pub fn verify_and_award(
        &mut self,
        receipt: &PoTWReceipt,
    ) -> Result<AwardInstruction, PoTWError> {
        let instruction = self.verify_receipt(receipt)?;
        let work_id = parse_work_id(&receipt.work_id_hex)?;

        // Commit epoch spend and replay guard.
        self.state
            .epoch_spent
            .insert(receipt.epoch, instruction.epoch_spent_after.to_string());
        self.state.awarded_work.insert(hex::encode(work_id));
        self.persist()?;
        Ok(instruction)
    }

    /// Atomically write state to the configured path (write temp + rename), if
    /// persistence is enabled. Mirrors the rollup registry's durable save.
    fn persist(&self) -> Result<(), PoTWError> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let json = serde_json::to_vec_pretty(&self.state)
            .map_err(|e| PoTWError::Malformed(format!("serialize state: {e}")))?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, &json)
            .map_err(|e| PoTWError::Malformed(format!("write temp state: {e}")))?;
        std::fs::rename(&tmp, path)
            .map_err(|e| PoTWError::Malformed(format!("rename state: {e}")))?;
        Ok(())
    }
}

/// Compute the award digest reviewers sign over.
pub fn award_digest(
    work_id: &[u8; 32],
    recipient20: &[u8; 20],
    amount: u128,
    epoch: u64,
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(POTW_AWARD_DOMAIN);
    hasher.update(work_id);
    hasher.update(recipient20);
    hasher.update(amount.to_le_bytes()); // u128 LE, 16 bytes
    hasher.update(epoch.to_le_bytes()); // u64 LE, 8 bytes
    let out = hasher.finalize();
    let mut digest = [0u8; 32];
    digest.copy_from_slice(&out);
    digest
}

/// Verify a single reviewer approval (SLH-DSA over the digest).
#[cfg(feature = "quantum")]
fn verify_approval(digest: &[u8; 32], approval: &ReviewerApproval) -> Result<bool, PoTWError> {
    let alg = approval.algorithm.trim().to_lowercase();
    match alg.as_str() {
        "slh-dsa-sha2-128s" | "slh-dsa-128s" | "slh-dsa-sha2-192s" | "slh-dsa-192s" => {
            let pk = hex::decode(approval.public_key_hex.trim())
                .map_err(|e| PoTWError::Malformed(format!("reviewer pubkey hex: {e}")))?;
            let sig = base64_decode(approval.signature_base64.trim())?;
            verify_slh_dsa_signature(digest, &alg, &pk, &sig)
                .map_err(|e| PoTWError::InvalidSignature(e.to_string()))
        }
        other => Err(PoTWError::Malformed(format!(
            "unsupported reviewer algorithm: {other}"
        ))),
    }
}

#[cfg(not(feature = "quantum"))]
fn verify_approval(_digest: &[u8; 32], _approval: &ReviewerApproval) -> Result<bool, PoTWError> {
    Err(PoTWError::QuantumUnavailable)
}

/// Base64 decode via the explicit engine API (works on base64 0.21/0.22).
#[cfg(feature = "quantum")]
fn base64_decode(s: &str) -> Result<Vec<u8>, PoTWError> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD
        .decode(s)
        .map_err(|e| PoTWError::Malformed(format!("reviewer signature base64: {e}")))
}

fn parse_work_id(hexstr: &str) -> Result<[u8; 32], PoTWError> {
    let bytes = hex::decode(hexstr.trim().trim_start_matches("0x"))
        .map_err(|e| PoTWError::BadWorkId(e.to_string()))?;
    if bytes.len() != 32 {
        return Err(PoTWError::BadWorkId(format!(
            "expected 32 bytes, got {}",
            bytes.len()
        )));
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(out)
}

fn parse_recipient20(addr: &str) -> Result<[u8; 20], PoTWError> {
    let bytes = hex::decode(addr.trim().trim_start_matches("0x"))
        .map_err(|e| PoTWError::BadRecipient(e.to_string()))?;
    if bytes.len() != 20 {
        return Err(PoTWError::BadRecipient(format!(
            "expected 20-byte address, got {}",
            bytes.len()
        )));
    }
    let mut out = [0u8; 20];
    out.copy_from_slice(&bytes);
    Ok(out)
}

/// serde helper: (de)serialize u128 as a decimal string so JSON never loses
/// precision regardless of the parser's integer width.
mod amount_str {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(v: &u128, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&v.to_string())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u128, D::Error> {
        let s = String::deserialize(d)?;
        s.parse::<u128>().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── PoTW award-digest browser determinism vector ────────────────────────
    //
    // Generated in kit.space-website by wasm-did's SLH-DSA-SHA2-128s bindings
    // (pure-Rust `slh-dsa` / FIPS-205), signing the award digest for a fixed
    // award. The digest MUST match what `award_digest` computes here, and the
    // signature MUST verify under this node's `verify_slh_dsa_signature` — proving
    // a reviewer signing in the browser and the node verifier share one parameter
    // set. Inputs: work_id = 0x11×32, recipient = 0x22×20, amount = 500_000,
    // epoch = 7.
    const VEC_WORK_ID: [u8; 32] = [0x11u8; 32];
    const VEC_RECIPIENT: [u8; 20] = [0x22u8; 20];
    const VEC_AMOUNT: u128 = 500_000;
    const VEC_EPOCH: u64 = 7;
    const VEC_DIGEST_HEX: &str =
        "52382a2c79fefe6be796133ffc4348a76967ce00b329d8e50de04cde199b6794";
    const VEC_PUB_HEX: &str =
        "8e100242a4cc82fbdb9a48673751c501041c40ddce8c0a533ec339b17dfc2d23";
    #[cfg(feature = "quantum")]
    const VEC_SIG_HEX: &str = include_str!("../tests/vectors/potw_award_128s_browser.sig.hex");

    #[test]
    fn award_digest_matches_browser_vector() {
        let digest = award_digest(&VEC_WORK_ID, &VEC_RECIPIENT, VEC_AMOUNT, VEC_EPOCH);
        assert_eq!(hex::encode(digest), VEC_DIGEST_HEX);
    }

    #[cfg(feature = "quantum")]
    #[test]
    fn reviewer_browser_signature_verifies() {
        let digest = award_digest(&VEC_WORK_ID, &VEC_RECIPIENT, VEC_AMOUNT, VEC_EPOCH);
        let pk = hex::decode(VEC_PUB_HEX).unwrap();
        let sig = hex::decode(VEC_SIG_HEX.trim()).unwrap();
        assert_eq!(pk.len(), 32, "SLH-DSA-SHA2-128s public key is 32 bytes");
        assert_eq!(sig.len(), 7856, "SLH-DSA-SHA2-128s signature is 7856 bytes");
        let ok = verify_slh_dsa_signature(&digest, "slh-dsa-sha2-128s", &pk, &sig).unwrap();
        assert!(ok, "browser reviewer signature must verify on the node");
    }

    /// Negative control: the reviewer signature must NOT verify against a digest
    /// for a *different* amount — proving the signature commits to the award.
    #[cfg(feature = "quantum")]
    #[test]
    fn reviewer_signature_rejects_tampered_amount() {
        let tampered = award_digest(&VEC_WORK_ID, &VEC_RECIPIENT, VEC_AMOUNT + 1, VEC_EPOCH);
        let pk = hex::decode(VEC_PUB_HEX).unwrap();
        let sig = hex::decode(VEC_SIG_HEX.trim()).unwrap();
        let ok = verify_slh_dsa_signature(&tampered, "slh-dsa-sha2-128s", &pk, &sig).unwrap();
        assert!(!ok, "signature must not verify for a different amount");
    }

    #[cfg(feature = "quantum")]
    fn base64_encode(bytes: &[u8]) -> String {
        use base64::Engine as _;
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    #[cfg(feature = "quantum")]
    fn vector_config(threshold: u64, per_work_cap: u128, epoch_budget: u128) -> PoTWConfig {
        PoTWConfig {
            reviewers: vec![VEC_PUB_HEX.to_string()],
            threshold,
            epoch_budget,
            per_work_cap,
        }
    }

    #[cfg(feature = "quantum")]
    fn vector_receipt() -> PoTWReceipt {
        let sig = hex::decode(VEC_SIG_HEX.trim()).unwrap();
        PoTWReceipt {
            work_id_hex: hex::encode(VEC_WORK_ID),
            recipient: format!("0x{}", hex::encode(VEC_RECIPIENT)),
            amount: VEC_AMOUNT,
            epoch: VEC_EPOCH,
            approvals: vec![ReviewerApproval {
                algorithm: "slh-dsa-sha2-128s".into(),
                public_key_hex: VEC_PUB_HEX.into(),
                signature_base64: base64_encode(&sig),
            }],
        }
    }

    #[cfg(feature = "quantum")]
    #[test]
    fn full_award_flow_commits_and_blocks_replay() {
        let mut acc = PoTWAccumulator::new(vector_config(1, 1_000_000, 1_000_000));
        // First award succeeds and credits 500_000.
        let instr = acc.verify_and_award(&vector_receipt()).unwrap();
        assert_eq!(instr.amount, VEC_AMOUNT);
        assert_eq!(instr.digest_hex, VEC_DIGEST_HEX);
        assert_eq!(instr.approving_reviewers, vec![VEC_PUB_HEX.to_lowercase()]);
        assert_eq!(acc.epoch_spent(VEC_EPOCH), VEC_AMOUNT);
        assert!(acc.is_awarded(&VEC_WORK_ID));

        // Same work id again → replay rejected, spend unchanged.
        let err = acc.verify_and_award(&vector_receipt()).unwrap_err();
        assert!(matches!(err, PoTWError::DuplicateWork(_)));
        assert_eq!(acc.epoch_spent(VEC_EPOCH), VEC_AMOUNT);
    }

    #[cfg(feature = "quantum")]
    #[test]
    fn per_work_cap_enforced() {
        let acc = PoTWAccumulator::new(vector_config(1, VEC_AMOUNT - 1, 1_000_000));
        let err = acc.verify_receipt(&vector_receipt()).unwrap_err();
        assert!(matches!(err, PoTWError::WorkCapExceeded { .. }));
    }

    #[cfg(feature = "quantum")]
    #[test]
    fn epoch_budget_enforced() {
        let acc = PoTWAccumulator::new(vector_config(1, 1_000_000, VEC_AMOUNT - 1));
        let err = acc.verify_receipt(&vector_receipt()).unwrap_err();
        assert!(matches!(err, PoTWError::EpochBudgetExceeded { .. }));
    }

    #[cfg(feature = "quantum")]
    #[test]
    fn quorum_not_met_when_threshold_exceeds_signers() {
        // Require 2 signatures but the receipt carries only 1.
        let acc = PoTWAccumulator::new(vector_config(2, 1_000_000, 1_000_000));
        let err = acc.verify_receipt(&vector_receipt()).unwrap_err();
        assert!(matches!(err, PoTWError::QuorumNotMet { got: 1, need: 2 }));
    }

    #[cfg(feature = "quantum")]
    #[test]
    fn signature_from_non_reviewer_rejected() {
        // Empty allow-list → the vector's key is not a reviewer.
        let acc = PoTWAccumulator::new(PoTWConfig {
            reviewers: vec![],
            threshold: 1,
            epoch_budget: 1_000_000,
            per_work_cap: 1_000_000,
        });
        let err = acc.verify_receipt(&vector_receipt()).unwrap_err();
        assert!(matches!(err, PoTWError::UnknownReviewer(_)));
    }

    #[cfg(feature = "quantum")]
    #[test]
    fn persisted_state_survives_reload() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("potw_state_test_{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);

        {
            let mut acc =
                PoTWAccumulator::load(vector_config(1, 1_000_000, 1_000_000), path.clone())
                    .unwrap();
            acc.verify_and_award(&vector_receipt()).unwrap();
        }
        // Reload: the replay guard and epoch spend must persist.
        let acc =
            PoTWAccumulator::load(vector_config(1, 1_000_000, 1_000_000), path.clone()).unwrap();
        assert!(acc.is_awarded(&VEC_WORK_ID));
        assert_eq!(acc.epoch_spent(VEC_EPOCH), VEC_AMOUNT);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn recipient_and_work_id_parsing() {
        assert!(parse_recipient20("0x2222222222222222222222222222222222222222").is_ok());
        assert!(parse_recipient20("0xdead").is_err());
        assert!(parse_work_id(&"11".repeat(32)).is_ok());
        assert!(parse_work_id("0x1234").is_err());
    }
}
