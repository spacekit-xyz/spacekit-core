//! Proof-of-authority bootstrap and validator-set governance.
//!
//! A new network starts in **proof of authority** (PoA): a fixed set of
//! authorities, named in a genesis file, validate without stake. The set only
//! changes through governance proposals that the current authorities sign and
//! vote on:
//!
//! - `add_authority`: admit an operator (DID + SPHINCS+ key) as a validator.
//! - `remove_authority`: drop an authority.
//! - `lift_poa`: end PoA and switch the network to proof of stake. Only
//!   accepted while the network has at least `min_validators_to_lift`
//!   authorities (default 10); nothing switches automatically.
//!
//! Each authority has one vote. A proposal passes with `ceil(2n/3)` approvals
//! and fails once enough authorities reject that approval is impossible.
//! Proposals expire after their voting period (default 7 days).
//!
//! ## What gets signed
//!
//! Proposals and votes are self-authenticating, so any node (or a relay such as
//! the website API) can accept and forward them. Signatures are SPHINCS+ by the
//! authority's key, over UTF-8 text:
//!
//! ```text
//! SPACEKIT-GOVERNANCE-PROPOSAL-v1\n{body_json}
//! SPACEKIT-GOVERNANCE-VOTE-v1\n{network}\n{proposal_id}\n{approve|reject}
//! ```
//!
//! `body_json` is signed exactly as submitted (no re-canonicalization), and the
//! proposal id is `hex(sha256(body_json))`. The body names the network, so a
//! testnet signature cannot be replayed on mainnet, and the hash of the
//! electorate (sorted authority DIDs) it was proposed under. Once the
//! authority set changes, every other pending proposal becomes `stale` and must
//! be proposed again under the new electorate.
//!
//! ## Replication
//!
//! Proposals and votes are consensus transactions (see `chain_consensus`):
//! a node checks them, queues them in its transaction pool (which is relayed
//! to every node) and they take effect when a block includes them, at that
//! block's time. The state lives in the chain, so every node applies the same
//! changes in the same order and a node that joins later replays them.
//!
//! ## After the lift
//!
//! In proof of stake the validator set comes from staking (`staking`), and
//! only protocol settings are governed (`set_block_production`). Votes are
//! weighted by effective stake in whole ASTRA, counted against the electorate
//! snapshot taken when the proposal was included, and a proposal passes with
//! two thirds of that weight.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::{info, warn};

use crate::consensus_coordinator::ConsensusCoordinator;

pub const PROPOSAL_DOMAIN: &str = "SPACEKIT-GOVERNANCE-PROPOSAL-v1";
pub const VOTE_DOMAIN: &str = "SPACEKIT-GOVERNANCE-VOTE-v1";

/// Authorities needed before a `lift_poa` proposal is accepted.
pub const DEFAULT_MIN_VALIDATORS_TO_LIFT: usize = 10;
pub const DEFAULT_VOTING_PERIOD_SECS: i64 = 7 * 86_400;
const MIN_VOTING_PERIOD_SECS: i64 = 3_600;
const MAX_VOTING_PERIOD_SECS: i64 = 30 * 86_400;
/// How far in the future a proposal's `created_at` may be.
const MAX_CLOCK_SKEW_SECS: i64 = 300;
/// After PoS activates, authorities keep validating unstaked for this long.
pub const DEFAULT_POS_GRACE_SECS: i64 = 30 * 86_400;
const MAX_TITLE_LEN: usize = 200;
const MAX_DESCRIPTION_LEN: usize = 4_000;

pub fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// Exact bytes an authority signs to submit a proposal.
pub fn proposal_signing_payload(body_json: &str) -> Vec<u8> {
    format!("{PROPOSAL_DOMAIN}\n{body_json}").into_bytes()
}

/// Exact bytes an authority signs to vote.
pub fn vote_signing_payload(network: &str, proposal_id: &str, choice: VoteChoice) -> Vec<u8> {
    format!("{VOTE_DOMAIN}\n{network}\n{proposal_id}\n{}", choice.as_str()).into_bytes()
}

/// `hex(sha256(body_json))`.
pub fn proposal_id(body_json: &str) -> String {
    sha256_hex(body_json.as_bytes())
}

/// Hash of the sorted authority DIDs, one per line.
pub fn electorate_hash<'a>(dids: impl IntoIterator<Item = &'a str>) -> String {
    let mut sorted: Vec<&str> = dids.into_iter().collect();
    sorted.sort_unstable();
    sha256_hex(sorted.join("\n").as_bytes())
}

/// A `did:spacekit:*` DID must end with `hex(sha256(pk)[..20])`, the same
/// derivation `/v1/did/register` and validator registration use.
pub fn did_matches_key(did: &str, public_key: &[u8]) -> bool {
    let address = hex::encode(&Sha256::digest(public_key)[..20]);
    did.starts_with("did:spacekit:") && did.ends_with(&address)
}

/// BFT fault tolerance of `n` equally weighted validators: `floor((n-1)/3)`.
pub fn fault_tolerance(n: usize) -> usize {
    n.saturating_sub(1) / 3
}

/// Approvals needed out of `n`: `ceil(2n/3)`.
pub fn approvals_needed(n: usize) -> usize {
    (2 * n).div_ceil(3)
}

/// Weight needed out of `total`: `ceil(2 * total / 3)`.
pub fn weight_needed(total: u64) -> u64 {
    ((2 * total as u128).div_ceil(3)) as u64
}

/// A proof-of-stake voter: a validator's key and its stake in whole ASTRA.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Voter {
    pub sphincs_pk_hex: String,
    pub weight: u64,
}

/// Hash of a stake-weighted electorate (`pos`, then `did weight` per line).
pub fn stake_electorate_hash(voters: &BTreeMap<String, Voter>) -> String {
    let mut text = String::from("pos\n");
    for (did, v) in voters {
        text.push_str(did);
        text.push(' ');
        text.push_str(&v.weight.to_string());
        text.push('\n');
    }
    sha256_hex(text.as_bytes())
}

// ── Types ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsensusMode {
    ProofOfAuthority,
    ProofOfStake,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Authority {
    pub did: String,
    pub sphincs_pk_hex: String,
    #[serde(default)]
    pub name: Option<String>,
    pub added_at: i64,
    /// Proposal that admitted this authority; `None` for genesis authorities.
    #[serde(default)]
    pub added_by: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProposalAction {
    AddAuthority {
        did: String,
        sphincs_pk_hex: String,
        #[serde(default)]
        name: Option<String>,
    },
    RemoveAuthority {
        did: String,
    },
    LiftPoa,
    /// Change when blocks are produced (see `block_production`).
    SetBlockProduction {
        config: crate::block_production::BlockProduction,
    },
}

/// The signed body of a proposal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposalBody {
    pub version: u32,
    pub network: String,
    pub action: ProposalAction,
    pub title: String,
    #[serde(default)]
    pub description: String,
    pub proposer_did: String,
    pub electorate_hash: String,
    pub created_at: i64,
    pub expires_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoteChoice {
    Approve,
    Reject,
}

impl VoteChoice {
    pub fn as_str(self) -> &'static str {
        match self {
            VoteChoice::Approve => "approve",
            VoteChoice::Reject => "reject",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedProposal {
    pub body_json: String,
    pub signature_hex: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedVote {
    pub proposal_id: String,
    pub voter_did: String,
    pub choice: VoteChoice,
    pub signature_hex: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalStatus {
    Pending,
    Executed,
    Rejected,
    Expired,
    /// The authority set or consensus mode changed before a decision.
    Stale,
    /// Passed, but the action was no longer valid when it executed.
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredVote {
    pub choice: VoteChoice,
    pub signature_hex: String,
    pub received_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposalRecord {
    pub id: String,
    pub body: ProposalBody,
    pub signed: SignedProposal,
    pub votes: BTreeMap<String, StoredVote>,
    pub status: ProposalStatus,
    pub submitted_at: i64,
    #[serde(default)]
    pub decided_at: Option<i64>,
    #[serde(default)]
    pub outcome: Option<String>,
    /// Tally frozen when the proposal was decided (the electorate may change
    /// afterwards, e.g. when this proposal added an authority).
    #[serde(default)]
    pub final_tally: Option<Tally>,
    /// Proof of stake: the validators and weights when the proposal was
    /// made. Votes are counted against this snapshot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub electorate: Option<BTreeMap<String, Voter>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tally {
    /// Approving votes (PoA) or approving stake in whole ASTRA (PoS).
    pub approve: u64,
    pub reject: u64,
    pub eligible: u64,
    pub needed: u64,
}

/// A change the validator set must reflect after a proposal executes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GovernanceEffect {
    AuthorityAdded { did: String, sphincs_public_key: Vec<u8> },
    AuthorityRemoved { did: String },
    ProofOfStakeActivated,
    BlockProductionChanged,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubmitOutcome {
    New { id: String, effects: Vec<GovernanceEffect> },
    Duplicate { id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VoteOutcome {
    Recorded { effects: Vec<GovernanceEffect> },
    Duplicate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GovernanceError {
    /// The proposal was made under a different authority set.
    StaleElectorate,
    UnknownProposal,
    Invalid(String),
}

impl std::fmt::Display for GovernanceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GovernanceError::StaleElectorate => write!(
                f,
                "proposal was made under a different authority set; re-propose it under the \
                 current electorate"
            ),
            GovernanceError::UnknownProposal => write!(f, "unknown proposal"),
            GovernanceError::Invalid(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for GovernanceError {}

fn invalid(msg: impl Into<String>) -> GovernanceError {
    GovernanceError::Invalid(msg.into())
}

/// Signature check, injected so the state machine stays pure and testable.
pub type VerifyFn<'a> = &'a dyn Fn(&[u8], &[u8], &[u8]) -> bool;

/// Genesis file (`SPACEKIT_POA_GENESIS_FILE`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoaGenesis {
    pub network: String,
    #[serde(default)]
    pub min_validators_to_lift: Option<usize>,
    pub authorities: Vec<GenesisAuthority>,
    /// When blocks are produced (default: on demand, see `block_production`).
    #[serde(default)]
    pub block_production: Option<crate::block_production::BlockProduction>,
    /// Staking rules after the lift (minimum stake, unbonding period).
    #[serde(default)]
    pub staking: Option<crate::staking::StakingParams>,
    /// Days authorities keep producing unstaked after the lift (default 30).
    #[serde(default)]
    pub pos_grace_days: Option<u64>,
    /// Treasury multisig (signers and threshold), written into the treasury
    /// contract's storage at genesis.
    #[serde(default)]
    pub treasury: Option<TreasuryGenesis>,
}

/// M-of-N signers of the treasury contract (`0x…0004`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreasuryGenesis {
    pub threshold: u64,
    pub signer_dids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisAuthority {
    pub did: String,
    pub sphincs_pk_hex: String,
    #[serde(default)]
    pub name: Option<String>,
}

// ── State machine ───────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GovernanceState {
    pub version: u32,
    pub network: String,
    pub mode: ConsensusMode,
    pub authorities: BTreeMap<String, Authority>,
    pub proposals: BTreeMap<String, ProposalRecord>,
    #[serde(default)]
    pub pos_activated_at: Option<i64>,
    pub min_validators_to_lift: usize,
    /// Authorities removed by governance. Kept so blocks they sealed while
    /// they were authorities still verify for nodes that catch up later.
    #[serde(default)]
    pub former_authorities: BTreeMap<String, Authority>,
    /// When blocks are produced. From the genesis file; changed by
    /// `set_block_production` proposals.
    #[serde(default)]
    pub block_production: crate::block_production::BlockProduction,
    /// After the lift: the staked validators and their weight (whole ASTRA),
    /// refreshed from the staking registry at the start of every block.
    #[serde(default)]
    pub stake_electorate: BTreeMap<String, Voter>,
    /// How long authorities keep producing unstaked after the lift.
    #[serde(default = "default_pos_grace_secs")]
    pub pos_grace_secs: i64,
}

fn default_pos_grace_secs() -> i64 {
    DEFAULT_POS_GRACE_SECS
}

impl GovernanceState {
    /// A network with no genesis authorities: plain proof of stake, and the
    /// governance endpoints refuse proposals.
    pub fn proof_of_stake(network: &str) -> Self {
        Self {
            version: 1,
            network: network.to_string(),
            mode: ConsensusMode::ProofOfStake,
            authorities: BTreeMap::new(),
            proposals: BTreeMap::new(),
            pos_activated_at: None,
            min_validators_to_lift: DEFAULT_MIN_VALIDATORS_TO_LIFT,
            former_authorities: BTreeMap::new(),
            block_production: Default::default(),
            stake_electorate: BTreeMap::new(),
            pos_grace_secs: DEFAULT_POS_GRACE_SECS,
        }
    }

    pub fn from_genesis(genesis: &PoaGenesis, now: i64) -> Result<Self> {
        if genesis.authorities.is_empty() {
            bail!("PoA genesis must name at least one authority");
        }
        let block_production = genesis.block_production.clone().unwrap_or_default();
        block_production
            .validate()
            .map_err(|e| anyhow!("genesis block_production: {e}"))?;
        let mut authorities = BTreeMap::new();
        for a in &genesis.authorities {
            let pk = hex::decode(a.sphincs_pk_hex.trim())
                .map_err(|_| anyhow!("genesis authority {}: sphincs_pk_hex is not hex", a.did))?;
            if !did_matches_key(&a.did, &pk) {
                bail!("genesis authority {} is not derived from its sphincs_pk_hex", a.did);
            }
            if authorities.contains_key(&a.did) {
                bail!("genesis authority {} is listed twice", a.did);
            }
            authorities.insert(
                a.did.clone(),
                Authority {
                    did: a.did.clone(),
                    sphincs_pk_hex: a.sphincs_pk_hex.trim().to_ascii_lowercase(),
                    name: a.name.clone(),
                    added_at: now,
                    added_by: None,
                },
            );
        }
        Ok(Self {
            version: 1,
            network: genesis.network.clone(),
            mode: ConsensusMode::ProofOfAuthority,
            authorities,
            proposals: BTreeMap::new(),
            pos_activated_at: None,
            min_validators_to_lift: genesis
                .min_validators_to_lift
                .unwrap_or(DEFAULT_MIN_VALIDATORS_TO_LIFT)
                .max(1),
            former_authorities: BTreeMap::new(),
            block_production,
            stake_electorate: BTreeMap::new(),
            pos_grace_secs: genesis
                .pos_grace_days
                .map(|d| (d as i64) * 86_400)
                .unwrap_or(DEFAULT_POS_GRACE_SECS),
        })
    }

    pub fn is_poa(&self) -> bool {
        self.mode == ConsensusMode::ProofOfAuthority
    }

    /// Sorted authority DIDs (block producer schedule order).
    pub fn authority_dids(&self) -> Vec<String> {
        self.authorities.keys().cloned().collect()
    }

    /// Public key of a current or former authority, for block seal checks.
    pub fn sealing_key(&self, did: &str) -> Option<Vec<u8>> {
        self.authorities
            .get(did)
            .or_else(|| self.former_authorities.get(did))
            .and_then(|a| hex::decode(&a.sphincs_pk_hex).ok())
    }

    /// PoA: hash of the authority DIDs. PoS: hash of the staked validators
    /// and their weights.
    pub fn electorate_hash(&self) -> String {
        if self.is_poa() {
            electorate_hash(self.authorities.keys().map(String::as_str))
        } else {
            stake_electorate_hash(&self.stake_electorate)
        }
    }

    /// Whether PoS authorities may still produce unstaked at `now`.
    pub fn within_pos_grace(&self, now: i64) -> bool {
        self.pos_activated_at
            .is_some_and(|at| now < at + self.pos_grace_secs)
    }

    /// Hash of what every node must agree on: mode and authority set.
    pub fn state_hash(&self) -> String {
        let mode = match self.mode {
            ConsensusMode::ProofOfAuthority => "poa",
            ConsensusMode::ProofOfStake => "pos",
        };
        let mut text = format!("{}\n{mode}\n", self.network);
        for a in self.authorities.values() {
            text.push_str(&a.did);
            text.push(' ');
            text.push_str(&a.sphincs_pk_hex);
            text.push('\n');
        }
        // Nodes that disagree on block production would take turns differently.
        text.push_str("production ");
        text.push_str(&self.block_production.canonical());
        text.push('\n');
        sha256_hex(text.as_bytes())
    }

    pub fn approvals_needed(&self) -> usize {
        approvals_needed(self.authorities.len())
    }

    fn authority_key(&self, did: &str) -> Option<Vec<u8>> {
        self.authorities
            .get(did)
            .and_then(|a| hex::decode(&a.sphincs_pk_hex).ok())
    }

    /// Mark pending proposals past their deadline as expired.
    pub fn expire(&mut self, now: i64) {
        for p in self.proposals.values_mut() {
            if p.status == ProposalStatus::Pending && now >= p.body.expires_at {
                p.status = ProposalStatus::Expired;
                p.decided_at = Some(p.body.expires_at);
            }
        }
    }

    /// Validate and store a signed proposal.
    pub fn submit_proposal(
        &mut self,
        signed: &SignedProposal,
        now: i64,
        verify: VerifyFn<'_>,
    ) -> Result<SubmitOutcome, GovernanceError> {
        let id = proposal_id(&signed.body_json);
        if self.proposals.contains_key(&id) {
            return Ok(SubmitOutcome::Duplicate { id });
        }
        let body: ProposalBody = serde_json::from_str(&signed.body_json)
            .map_err(|e| invalid(format!("body_json is not a valid proposal: {e}")))?;
        if body.version != 1 {
            return Err(invalid(format!("unsupported proposal version {}", body.version)));
        }
        if body.network != self.network {
            return Err(invalid(format!(
                "proposal is for network {:?}, this node is on {:?}",
                body.network, self.network
            )));
        }
        let title = body.title.trim();
        if title.is_empty() || title.len() > MAX_TITLE_LEN {
            return Err(invalid(format!("title must be 1-{MAX_TITLE_LEN} characters")));
        }
        if body.description.len() > MAX_DESCRIPTION_LEN {
            return Err(invalid(format!(
                "description must be at most {MAX_DESCRIPTION_LEN} characters"
            )));
        }
        if body.created_at > now + MAX_CLOCK_SKEW_SECS {
            return Err(invalid("created_at is in the future"));
        }
        let period = body.expires_at - body.created_at;
        if !(MIN_VOTING_PERIOD_SECS..=MAX_VOTING_PERIOD_SECS).contains(&period) {
            return Err(invalid(format!(
                "voting period must be between {MIN_VOTING_PERIOD_SECS}s and \
                 {MAX_VOTING_PERIOD_SECS}s"
            )));
        }
        if now >= body.expires_at {
            return Err(invalid("proposal has already expired"));
        }

        let proposer_key = if self.is_poa() {
            self.authority_key(&body.proposer_did)
                .ok_or_else(|| invalid(format!("{} is not an authority", body.proposer_did)))?
        } else {
            self.stake_electorate
                .get(&body.proposer_did)
                .and_then(|v| hex::decode(&v.sphincs_pk_hex).ok())
                .ok_or_else(|| {
                    invalid(format!("{} is not a staked validator", body.proposer_did))
                })?
        };
        let signature = hex::decode(signed.signature_hex.trim())
            .map_err(|_| invalid("signature_hex is not hex"))?;
        if !verify(
            &proposer_key,
            &proposal_signing_payload(&signed.body_json),
            &signature,
        ) {
            return Err(invalid("proposal signature does not verify against the proposer's key"));
        }
        // PoS proposals are counted against the stake snapshot taken when
        // they are included, so their electorate_hash is informational.
        if self.is_poa() && body.electorate_hash != self.electorate_hash() {
            return Err(GovernanceError::StaleElectorate);
        }
        self.check_action(&body.action)?;

        self.proposals.insert(
            id.clone(),
            ProposalRecord {
                id: id.clone(),
                body,
                signed: SignedProposal {
                    body_json: signed.body_json.clone(),
                    signature_hex: signed.signature_hex.trim().to_ascii_lowercase(),
                },
                votes: BTreeMap::new(),
                status: ProposalStatus::Pending,
                submitted_at: now,
                decided_at: None,
                outcome: None,
                final_tally: None,
                electorate: (!self.is_poa()).then(|| self.stake_electorate.clone()),
            },
        );
        // A single-authority network decides as soon as its one vote lands;
        // nothing to decide yet.
        Ok(SubmitOutcome::New {
            id,
            effects: Vec::new(),
        })
    }

    /// Whether `action` could execute against the current state.
    fn check_action(&self, action: &ProposalAction) -> Result<(), GovernanceError> {
        if !self.is_poa() && !matches!(action, ProposalAction::SetBlockProduction { .. }) {
            return Err(invalid(
                "after the lift, validators join and leave by staking; only protocol settings \
                 are governed (set_block_production)",
            ));
        }
        match action {
            ProposalAction::AddAuthority {
                did,
                sphincs_pk_hex,
                name,
            } => {
                let pk = hex::decode(sphincs_pk_hex.trim())
                    .map_err(|_| invalid("sphincs_pk_hex is not hex"))?;
                if pk.is_empty() {
                    return Err(invalid("sphincs_pk_hex is empty"));
                }
                if !did_matches_key(did, &pk) {
                    return Err(invalid(format!("{did} is not derived from sphincs_pk_hex")));
                }
                if self.authorities.contains_key(did) {
                    return Err(invalid(format!("{did} is already an authority")));
                }
                if name.as_ref().is_some_and(|n| n.len() > MAX_TITLE_LEN) {
                    return Err(invalid("name is too long"));
                }
                Ok(())
            }
            ProposalAction::RemoveAuthority { did } => {
                if !self.authorities.contains_key(did) {
                    return Err(invalid(format!("{did} is not an authority")));
                }
                if self.authorities.len() <= 1 {
                    return Err(invalid("cannot remove the last authority"));
                }
                Ok(())
            }
            ProposalAction::LiftPoa => {
                if self.authorities.len() < self.min_validators_to_lift {
                    return Err(invalid(format!(
                        "proof of authority can only be lifted with at least {} validators \
                         (currently {})",
                        self.min_validators_to_lift,
                        self.authorities.len()
                    )));
                }
                Ok(())
            }
            ProposalAction::SetBlockProduction { config } => {
                config.validate().map_err(invalid)?;
                if *config == self.block_production {
                    return Err(invalid("block production already has this setting"));
                }
                Ok(())
            }
        }
    }

    /// Validate and record a signed vote, then decide the proposal if the
    /// vote settles it.
    pub fn submit_vote(
        &mut self,
        vote: &SignedVote,
        now: i64,
        verify: VerifyFn<'_>,
    ) -> Result<VoteOutcome, GovernanceError> {
        self.expire(now);
        let network = self.network.clone();
        let record = self
            .proposals
            .get(&vote.proposal_id)
            .ok_or(GovernanceError::UnknownProposal)?;
        let key = match &record.electorate {
            Some(voters) => voters
                .get(&vote.voter_did)
                .and_then(|v| hex::decode(&v.sphincs_pk_hex).ok()),
            None => self.authority_key(&vote.voter_did),
        };
        let signature_hex = vote.signature_hex.trim().to_ascii_lowercase();

        if let Some(existing) = record.votes.get(&vote.voter_did) {
            if existing.choice == vote.choice && existing.signature_hex == signature_hex {
                return Ok(VoteOutcome::Duplicate);
            }
            return Err(invalid(format!(
                "{} has already voted {} on this proposal",
                vote.voter_did,
                existing.choice.as_str()
            )));
        }
        if record.status != ProposalStatus::Pending {
            return Err(invalid(format!(
                "proposal is {}, not pending",
                serde_json::to_string(&record.status).unwrap_or_default()
            )));
        }
        let key = key.ok_or_else(|| {
            invalid(format!("{} may not vote on this proposal", vote.voter_did))
        })?;
        let signature =
            hex::decode(&signature_hex).map_err(|_| invalid("signature_hex is not hex"))?;
        if !verify(
            &key,
            &vote_signing_payload(&network, &vote.proposal_id, vote.choice),
            &signature,
        ) {
            return Err(invalid("vote signature does not verify against the voter's key"));
        }

        let record = self
            .proposals
            .get_mut(&vote.proposal_id)
            .ok_or(GovernanceError::UnknownProposal)?;
        record.votes.insert(
            vote.voter_did.clone(),
            StoredVote {
                choice: vote.choice,
                signature_hex,
                received_at: now,
            },
        );
        let effects = self.try_decide(&vote.proposal_id, now);
        Ok(VoteOutcome::Recorded { effects })
    }

    /// `(approve, reject, eligible)`: PoA counts the current authorities'
    /// votes; PoS weighs votes by the stake snapshot taken at proposal time.
    pub fn tally_weights(&self, record: &ProposalRecord) -> (u64, u64, u64) {
        match &record.electorate {
            Some(voters) => {
                let total: u64 = voters.values().map(|v| v.weight).sum();
                let (a, r) = record.votes.iter().fold((0u64, 0u64), |(a, r), (did, v)| {
                    let w = voters.get(did).map(|x| x.weight).unwrap_or(0);
                    match v.choice {
                        VoteChoice::Approve => (a + w, r),
                        VoteChoice::Reject => (a, r + w),
                    }
                });
                (a, r, total)
            }
            None => {
                let (a, r) = self.tally(record);
                (a as u64, r as u64, self.authorities.len() as u64)
            }
        }
    }

    /// `(approve, reject)` counted over the current authorities only.
    pub fn tally(&self, record: &ProposalRecord) -> (usize, usize) {
        record
            .votes
            .iter()
            .filter(|(did, _)| self.authorities.contains_key(*did))
            .fold((0, 0), |(a, r), (_, v)| match v.choice {
                VoteChoice::Approve => (a + 1, r),
                VoteChoice::Reject => (a, r + 1),
            })
    }

    /// Live tally, or the frozen one once decided.
    pub fn tally_view(&self, record: &ProposalRecord) -> Tally {
        if let Some(t) = record.final_tally {
            return t;
        }
        let (approve, reject, eligible) = self.tally_weights(record);
        Tally {
            approve,
            reject,
            eligible,
            needed: weight_needed(eligible),
        }
    }

    fn try_decide(&mut self, id: &str, now: i64) -> Vec<GovernanceEffect> {
        let Some(record) = self.proposals.get(id) else {
            return Vec::new();
        };
        if record.status != ProposalStatus::Pending {
            return Vec::new();
        }
        // A PoA proposal must still match the authority set, and cannot
        // survive the lift. PoS proposals carry their own electorate.
        if record.electorate.is_none()
            && (record.body.electorate_hash != self.electorate_hash() || !self.is_poa())
        {
            self.set_status(id, ProposalStatus::Stale, now, None);
            return Vec::new();
        }
        let (approve, reject, eligible) = self.tally_weights(record);
        let needed = weight_needed(eligible);
        let action = record.body.action.clone();
        let frozen = Tally {
            approve,
            reject,
            eligible,
            needed,
        };
        let passed = eligible > 0 && approve >= needed;
        let failed = reject > eligible.saturating_sub(needed);
        if passed || failed {
            if let Some(p) = self.proposals.get_mut(id) {
                p.final_tally = Some(frozen);
            }
        }
        if passed {
            match self.execute(id, &action, now) {
                Ok(effects) => {
                    self.set_status(id, ProposalStatus::Executed, now, None);
                    self.stale_others(id, now);
                    effects
                }
                Err(e) => {
                    self.set_status(id, ProposalStatus::Failed, now, Some(e.to_string()));
                    Vec::new()
                }
            }
        } else if failed {
            self.set_status(id, ProposalStatus::Rejected, now, None);
            Vec::new()
        } else {
            Vec::new()
        }
    }

    fn set_status(&mut self, id: &str, status: ProposalStatus, now: i64, outcome: Option<String>) {
        if let Some(p) = self.proposals.get_mut(id) {
            p.status = status;
            p.decided_at = Some(now);
            p.outcome = outcome;
        }
    }

    /// After an execution, pending proposals made under the old electorate
    /// (or before PoS) can no longer be decided.
    fn stale_others(&mut self, executed: &str, now: i64) {
        let current = self.electorate_hash();
        let poa = self.is_poa();
        for (id, p) in self.proposals.iter_mut() {
            if id != executed
                && p.status == ProposalStatus::Pending
                && p.electorate.is_none()
                && (!poa || p.body.electorate_hash != current)
            {
                p.status = ProposalStatus::Stale;
                p.decided_at = Some(now);
            }
        }
    }

    fn execute(
        &mut self,
        id: &str,
        action: &ProposalAction,
        now: i64,
    ) -> Result<Vec<GovernanceEffect>, GovernanceError> {
        self.check_action(action)?;
        match action {
            ProposalAction::AddAuthority {
                did,
                sphincs_pk_hex,
                name,
            } => {
                let pk = hex::decode(sphincs_pk_hex.trim())
                    .map_err(|_| invalid("sphincs_pk_hex is not hex"))?;
                self.authorities.insert(
                    did.clone(),
                    Authority {
                        did: did.clone(),
                        sphincs_pk_hex: sphincs_pk_hex.trim().to_ascii_lowercase(),
                        name: name.clone(),
                        added_at: now,
                        added_by: Some(id.to_string()),
                    },
                );
                Ok(vec![GovernanceEffect::AuthorityAdded {
                    did: did.clone(),
                    sphincs_public_key: pk,
                }])
            }
            ProposalAction::RemoveAuthority { did } => {
                if let Some(former) = self.authorities.remove(did) {
                    self.former_authorities.insert(did.clone(), former);
                }
                Ok(vec![GovernanceEffect::AuthorityRemoved { did: did.clone() }])
            }
            ProposalAction::LiftPoa => {
                self.mode = ConsensusMode::ProofOfStake;
                self.pos_activated_at = Some(now);
                Ok(vec![GovernanceEffect::ProofOfStakeActivated])
            }
            ProposalAction::SetBlockProduction { config } => {
                self.block_production = config.clone();
                Ok(vec![GovernanceEffect::BlockProductionChanged])
            }
        }
    }
}

// ── Node service ────────────────────────────────────────────────────────

/// Verify a SPHINCS+ signature (the scheme validator keys use).
pub fn sphincs_verify(public_key: &[u8], message: &[u8], signature: &[u8]) -> bool {
    spacekit_did::sphincs::SphincsPlus::verify(public_key, message, signature)
}

/// Governance and staking, held in the chain (see `chain_consensus`).
///
/// Proposals, votes and stake messages submitted over HTTP are checked
/// against the head state and queued as consensus transactions. They take
/// effect when a block includes them, on every node alike; the transaction
/// pool relays them. Reads come from the head block's state.
pub struct ValidatorGovernance {
    network: String,
    /// Initial state from the PoA genesis file (installed at height 0).
    genesis: Option<GovernanceState>,
    staking_params: crate::staking::StakingParams,
    treasury_genesis: Option<TreasuryGenesis>,
    vm: std::sync::OnceLock<Arc<crate::spacekitvm::SwtchvmNode>>,
    coordinator: Arc<ConsensusCoordinator>,
    /// DIDs this node registered with the coordinator.
    registered: tokio::sync::Mutex<std::collections::BTreeSet<String>>,
}

impl ValidatorGovernance {
    /// `SPACEKIT_POA_GENESIS_FILE`: genesis authorities, block production and
    /// staking rules. Without it the node runs a plain chain with no
    /// governance.
    pub fn from_env(network: &str, coordinator: Arc<ConsensusCoordinator>) -> Result<Self> {
        let genesis_path = std::env::var("SPACEKIT_POA_GENESIS_FILE")
            .ok()
            .filter(|p| !p.trim().is_empty())
            .map(PathBuf::from);
        if std::env::var_os("SPACEKIT_POS_GRACE_DAYS").is_some() {
            warn!(
                "SPACEKIT_POS_GRACE_DAYS is ignored: the grace period is part of consensus; set \
                 pos_grace_days in the genesis file"
            );
        }
        Self::load(network, genesis_path.as_deref(), coordinator)
    }

    pub fn load(
        network: &str,
        genesis_path: Option<&Path>,
        coordinator: Arc<ConsensusCoordinator>,
    ) -> Result<Self> {
        let mut treasury_genesis = None;
        let (genesis, staking_params) = match genesis_path {
            Some(path) => {
                let raw = std::fs::read_to_string(path)
                    .map_err(|e| anyhow!("reading PoA genesis {}: {e}", path.display()))?;
                let genesis: PoaGenesis = serde_json::from_str(&raw)
                    .map_err(|e| anyhow!("parsing PoA genesis {}: {e}", path.display()))?;
                if genesis.network != network {
                    bail!(
                        "PoA genesis is for network {:?}, this node runs {:?}",
                        genesis.network,
                        network
                    );
                }
                let staking = genesis.staking.clone().unwrap_or_default();
                if let Some(t) = &genesis.treasury {
                    if t.threshold == 0 || t.threshold as usize > t.signer_dids.len() {
                        bail!("treasury threshold must be between 1 and the number of signers");
                    }
                    treasury_genesis = Some(t.clone());
                }
                // Genesis timestamps are fixed (0) so every node installs
                // byte-identical state.
                let state = GovernanceState::from_genesis(&genesis, 0)?;
                info!(
                    authorities = state.authorities.len(),
                    "Proof of authority genesis from {}",
                    path.display()
                );
                (Some(state), staking)
            }
            None => (None, crate::staking::StakingParams::default()),
        };
        Ok(Self {
            network: network.to_string(),
            genesis,
            staking_params,
            treasury_genesis,
            vm: std::sync::OnceLock::new(),
            coordinator,
            registered: tokio::sync::Mutex::new(Default::default()),
        })
    }

    /// Connect to the chain; installs the genesis state at height 0.
    pub async fn attach_vm(&self, vm: Arc<crate::spacekitvm::SwtchvmNode>) -> Result<()> {
        if let Some(genesis) = &self.genesis {
            vm.init_consensus_genesis(
                genesis.clone(),
                self.staking_params.clone(),
                self.treasury_genesis.clone(),
            )
            .await?;
        }
        let _ = self.vm.set(vm);
        Ok(())
    }

    fn vm(&self) -> Option<&Arc<crate::spacekitvm::SwtchvmNode>> {
        self.vm.get()
    }

    /// Governance as of the head block, with deadlines applied at `now` for
    /// display (the chain applies them when the next block begins).
    pub async fn snapshot(&self) -> GovernanceState {
        let mut state = match self.vm() {
            Some(vm) => match vm.consensus_state().await {
                Some(cs) => cs.governance,
                None => self
                    .genesis
                    .clone()
                    .unwrap_or_else(|| GovernanceState::proof_of_stake(&self.network)),
            },
            None => self
                .genesis
                .clone()
                .unwrap_or_else(|| GovernanceState::proof_of_stake(&self.network)),
        };
        state.expire(now_unix());
        state
    }

    pub async fn is_poa(&self) -> bool {
        self.snapshot().await.is_poa()
    }

    /// Whether this network has on-chain governance (a PoA genesis).
    pub fn has_genesis(&self) -> bool {
        self.genesis.is_some()
    }

    async fn queue(&self, msg: crate::chain_consensus::ConsensusMessage) -> Result<String, String> {
        let vm = self.vm().ok_or("the chain is not running yet")?;
        vm.submit_consensus_message(&msg)
            .await
            .map(|(hash, _)| format!("0x{}", hex::encode(hash)))
            .map_err(|e| e.to_string())
    }

    fn map_error(e: String) -> GovernanceError {
        if e.contains("different authority set") {
            GovernanceError::StaleElectorate
        } else if e == "unknown proposal" {
            GovernanceError::UnknownProposal
        } else {
            GovernanceError::Invalid(e)
        }
    }

    /// Check a proposal and queue it for the next block.
    pub async fn submit_proposal(
        &self,
        signed: SignedProposal,
    ) -> Result<(SubmitOutcome, Option<String>), GovernanceError> {
        let id = proposal_id(&signed.body_json);
        if self.snapshot().await.proposals.contains_key(&id) {
            return Ok((SubmitOutcome::Duplicate { id }, None));
        }
        let tx = self
            .queue(crate::chain_consensus::ConsensusMessage::GovernanceProposal { proposal: signed })
            .await
            .map_err(Self::map_error)?;
        info!(proposal = %id, "Queued governance proposal");
        Ok((
            SubmitOutcome::New {
                id,
                effects: Vec::new(),
            },
            Some(tx),
        ))
    }

    /// Check a vote and queue it for the next block.
    pub async fn submit_vote(&self, vote: SignedVote) -> Result<(VoteOutcome, Option<String>), GovernanceError> {
        let state = self.snapshot().await;
        if let Some(existing) = state
            .proposals
            .get(&vote.proposal_id)
            .and_then(|p| p.votes.get(&vote.voter_did))
        {
            if existing.choice == vote.choice
                && existing.signature_hex == vote.signature_hex.trim().to_ascii_lowercase()
            {
                return Ok((VoteOutcome::Duplicate, None));
            }
        }
        let tx = self
            .queue(crate::chain_consensus::ConsensusMessage::GovernanceVote { vote: vote.clone() })
            .await
            .map_err(Self::map_error)?;
        info!(proposal = %vote.proposal_id, voter = %vote.voter_did, "Queued governance vote");
        Ok((
            VoteOutcome::Recorded {
                effects: Vec::new(),
            },
            Some(tx),
        ))
    }

    /// Check a stake message and queue it for the next block.
    pub async fn submit_stake(&self, stake: crate::staking::SignedStake) -> Result<String, String> {
        self.queue(crate::chain_consensus::ConsensusMessage::Stake { stake })
            .await
    }

    /// Check a SPHINCS+-signed native transfer and queue it for the next block.
    pub async fn submit_transfer(
        &self,
        transfer: crate::chain_consensus::SignedTransfer,
    ) -> Result<String, String> {
        self.queue(crate::chain_consensus::ConsensusMessage::Transfer { transfer })
            .await
    }

    /// Staking rules and validators with their holdings and effective stake.
    pub async fn staking_view(&self) -> serde_json::Value {
        let Some(vm) = self.vm() else {
            return serde_json::json!({ "enabled": false });
        };
        let world = vm.runtime_state();
        let world = world.read().await;
        let Some(cs) = crate::chain_consensus::load(&world) else {
            return serde_json::json!({ "enabled": false });
        };
        let now = now_unix().max(0) as u64;
        let min = cs.staking.params.min_stake_wei();
        let validators: Vec<serde_json::Value> = cs
            .staking
            .validators
            .values()
            .map(|v| {
                let holdings = crate::chain_consensus::astra_holdings_wei(&world, &v.did);
                let effective = v.effective_wei(holdings);
                serde_json::json!({
                    "did": v.did,
                    "name": v.name,
                    "sphincs_pk_hex": v.sphincs_pk_hex,
                    "bonded_wei": v.bonded_wei.to_string(),
                    "unbonding": v.unbonding.iter().map(|u| serde_json::json!({
                        "amount_wei": u.amount_wei.to_string(),
                        "release_at": u.release_at,
                    })).collect::<Vec<_>>(),
                    "holdings_wei": holdings.to_string(),
                    "effective_wei": effective.to_string(),
                    "active": effective >= min && effective > 0,
                    "next_nonce": v.nonce,
                })
            })
            .collect();
        let producers = crate::chain_consensus::producer_set(&world, now);
        serde_json::json!({
            "enabled": true,
            "network": cs.governance.network,
            "mode": http::mode_str(cs.governance.mode),
            "params": cs.staking.params,
            "min_stake_wei": min.to_string(),
            "validators": validators,
            "producers": producers,
            "stake_domain": crate::staking::STAKE_DOMAIN,
        })
    }

    /// Keep the consensus coordinator's validator list in step with the
    /// chain's producer set.
    pub async fn sync_coordinator(&self) {
        let Some(vm) = self.vm() else { return };
        let set = vm.producer_set(now_unix().max(0) as u64).await;
        let wanted: std::collections::BTreeMap<String, Vec<u8>> = set
            .members
            .iter()
            .filter_map(|p| hex::decode(&p.sphincs_pk_hex).ok().map(|k| (p.did.clone(), k)))
            .collect();
        let mut registered = self.registered.lock().await;
        for (did, pk) in &wanted {
            if !registered.contains(did) {
                if let Err(e) = self.coordinator.register_authority(did.clone(), pk.clone()).await {
                    warn!("could not register validator {did}: {e}");
                    continue;
                }
                registered.insert(did.clone());
            }
        }
        let gone: Vec<String> = registered
            .iter()
            .filter(|d| !wanted.contains_key(*d))
            .cloned()
            .collect();
        for did in gone {
            self.coordinator.remove_validator(&did).await;
            registered.remove(&did);
        }
    }
}

// ── HTTP ────────────────────────────────────────────────────────────────

pub mod http {
    //! Public governance routes. Reads are open; writes carry their own
    //! signatures, so they need no request authentication.

    use super::*;
    use warp::filters::BoxedFilter;
    use warp::http::StatusCode;
    use warp::{Filter, Reply};

    const MAX_BODY_BYTES: u64 = 512 * 1024;

    fn json_status(value: serde_json::Value, status: StatusCode) -> Box<dyn Reply> {
        Box::new(warp::reply::with_status(warp::reply::json(&value), status))
    }

    pub fn mode_str(mode: ConsensusMode) -> &'static str {
        match mode {
            ConsensusMode::ProofOfAuthority => "proof_of_authority",
            ConsensusMode::ProofOfStake => "proof_of_stake",
        }
    }

    /// Summary of the validator set and governance rules.
    pub fn overview(state: &GovernanceState) -> serde_json::Value {
        let pos_grace_secs = state.pos_grace_secs;
        let n = state.authorities.len();
        let pending = state
            .proposals
            .values()
            .filter(|p| p.status == ProposalStatus::Pending)
            .count();
        serde_json::json!({
            "network": state.network,
            "mode": mode_str(state.mode),
            "state_hash": state.state_hash(),
            "electorate_hash": state.electorate_hash(),
            "authorities": state.authorities.values().collect::<Vec<_>>(),
            "authority_count": n,
            "approvals_needed": approvals_needed(n),
            "stake_electorate": state.stake_electorate,
            "stake_weight_total": state.stake_electorate.values().map(|v| v.weight).sum::<u64>(),
            "stake_weight_needed": weight_needed(state.stake_electorate.values().map(|v| v.weight).sum::<u64>()),
            "fault_tolerance": fault_tolerance(n),
            "min_validators_to_lift": state.min_validators_to_lift,
            "block_production": state.block_production,
            "can_lift_poa": state.is_poa() && n >= state.min_validators_to_lift,
            "pending_proposals": pending,
            "pos_activated_at": state.pos_activated_at,
            "pos_grace_ends_at": state.pos_activated_at.map(|t| t + pos_grace_secs),
            "rules": {
                "proposal_domain": PROPOSAL_DOMAIN,
                "vote_domain": VOTE_DOMAIN,
                "default_voting_period_secs": DEFAULT_VOTING_PERIOD_SECS,
                "min_voting_period_secs": MIN_VOTING_PERIOD_SECS,
                "max_voting_period_secs": MAX_VOTING_PERIOD_SECS,
            },
        })
    }

    pub fn proposal_view(state: &GovernanceState, p: &ProposalRecord, full: bool) -> serde_json::Value {
        let tally = state.tally_view(p);
        let mut v = serde_json::json!({
            "id": p.id,
            "status": p.status,
            "title": p.body.title,
            "description": p.body.description,
            "action": p.body.action,
            "proposer_did": p.body.proposer_did,
            "created_at": p.body.created_at,
            "expires_at": p.body.expires_at,
            "submitted_at": p.submitted_at,
            "decided_at": p.decided_at,
            "outcome": p.outcome,
            "electorate_hash": p.body.electorate_hash,
            "tally": tally,
            "votes": p.votes.iter().map(|(did, vote)| serde_json::json!({
                "voter_did": did,
                "choice": vote.choice,
                "received_at": vote.received_at,
            })).collect::<Vec<_>>(),
        });
        if full {
            v["body_json"] = serde_json::Value::String(p.signed.body_json.clone());
            v["signature_hex"] = serde_json::Value::String(p.signed.signature_hex.clone());
            v["vote_payloads"] = serde_json::json!({
                "approve": String::from_utf8_lossy(&vote_signing_payload(&state.network, &p.id, VoteChoice::Approve)),
                "reject": String::from_utf8_lossy(&vote_signing_payload(&state.network, &p.id, VoteChoice::Reject)),
            });
        }
        v
    }

    #[derive(Deserialize)]
    struct ListQuery {
        #[serde(default)]
        status: Option<String>,
    }

    fn error_reply(e: GovernanceError) -> Box<dyn Reply> {
        let status = match e {
            GovernanceError::StaleElectorate => StatusCode::CONFLICT,
            GovernanceError::UnknownProposal => StatusCode::NOT_FOUND,
            GovernanceError::Invalid(_) => StatusCode::BAD_REQUEST,
        };
        json_status(serde_json::json!({ "error": e.to_string() }), status)
    }

    pub fn routes(gov: Arc<ValidatorGovernance>) -> BoxedFilter<(Box<dyn Reply>,)> {
        let with_gov = {
            let gov = gov.clone();
            warp::any().map(move || gov.clone())
        };

        // GET /v1/governance
        let overview_route = warp::path!("v1" / "governance")
            .and(warp::get())
            .and(with_gov.clone())
            .and_then(|gov: Arc<ValidatorGovernance>| async move {
                let state = gov.snapshot().await;
                Ok::<_, warp::Rejection>(json_status(
                    overview(&state),
                    StatusCode::OK,
                ))
            });

        // GET /v1/governance/proposals?status=pending
        let list_route = warp::path!("v1" / "governance" / "proposals")
            .and(warp::get())
            .and(warp::query::<ListQuery>())
            .and(with_gov.clone())
            .and_then(|q: ListQuery, gov: Arc<ValidatorGovernance>| async move {
                let state = gov.snapshot().await;
                let wanted = q.status.as_deref().map(str::to_ascii_lowercase);
                let mut list: Vec<&ProposalRecord> = state
                    .proposals
                    .values()
                    .filter(|p| {
                        wanted.as_deref().is_none_or(|w| {
                            serde_json::to_value(p.status)
                                .ok()
                                .and_then(|s| s.as_str().map(|s| s == w))
                                .unwrap_or(false)
                        })
                    })
                    .collect();
                list.sort_by_key(|p| std::cmp::Reverse(p.submitted_at));
                let views: Vec<_> = list.iter().map(|p| proposal_view(&state, p, false)).collect();
                Ok::<_, warp::Rejection>(json_status(
                    serde_json::json!({ "network": state.network, "proposals": views }),
                    StatusCode::OK,
                ))
            });

        // GET /v1/governance/proposals/{id}
        let detail_route = warp::path!("v1" / "governance" / "proposals" / String)
            .and(warp::get())
            .and(with_gov.clone())
            .and_then(|id: String, gov: Arc<ValidatorGovernance>| async move {
                let state = gov.snapshot().await;
                let reply = match state.proposals.get(&id.to_ascii_lowercase()) {
                    Some(p) => json_status(proposal_view(&state, p, true), StatusCode::OK),
                    None => error_reply(GovernanceError::UnknownProposal),
                };
                Ok::<_, warp::Rejection>(reply)
            });

        // POST /v1/governance/proposals  { body_json, signature_hex }
        let submit_route = warp::path!("v1" / "governance" / "proposals")
            .and(warp::post())
            .and(warp::body::content_length_limit(MAX_BODY_BYTES))
            .and(warp::body::json::<SignedProposal>())
            .and(with_gov.clone())
            .and_then(|signed: SignedProposal, gov: Arc<ValidatorGovernance>| async move {
                // Queued for the next block; it takes effect when included.
                let reply = match gov.submit_proposal(signed).await {
                    Ok((SubmitOutcome::New { id, .. }, tx)) => json_status(
                        serde_json::json!({ "status": "queued", "id": id, "tx_hash": tx }),
                        StatusCode::ACCEPTED,
                    ),
                    Ok((SubmitOutcome::Duplicate { id }, _)) => json_status(
                        serde_json::json!({ "status": "duplicate", "id": id }),
                        StatusCode::OK,
                    ),
                    Err(e) => error_reply(e),
                };
                Ok::<_, warp::Rejection>(reply)
            });

        // POST /v1/governance/votes  { proposal_id, voter_did, choice, signature_hex }
        let vote_route = warp::path!("v1" / "governance" / "votes")
            .and(warp::post())
            .and(warp::body::content_length_limit(MAX_BODY_BYTES))
            .and(warp::body::json::<SignedVote>())
            .and(with_gov.clone())
            .and_then(|vote: SignedVote, gov: Arc<ValidatorGovernance>| async move {
                let id = vote.proposal_id.clone();
                let reply = match gov.submit_vote(vote).await {
                    Ok((outcome, tx)) => {
                        let state = gov.snapshot().await;
                        let proposal = state.proposals.get(&id).map(|p| proposal_view(&state, p, false));
                        json_status(
                            serde_json::json!({
                                "status": if matches!(outcome, VoteOutcome::Duplicate) { "duplicate" } else { "queued" },
                                "tx_hash": tx,
                                "proposal": proposal,
                            }),
                            if matches!(outcome, VoteOutcome::Duplicate) { StatusCode::OK } else { StatusCode::ACCEPTED },
                        )
                    }
                    Err(e) => error_reply(e),
                };
                Ok::<_, warp::Rejection>(reply)
            });

        // GET /v1/staking — rules, validators, stake, and the producer set.
        let staking_route = warp::path!("v1" / "staking")
            .and(warp::get())
            .and(with_gov.clone())
            .and_then(|gov: Arc<ValidatorGovernance>| async move {
                Ok::<_, warp::Rejection>(json_status(gov.staking_view().await, StatusCode::OK))
            });

        // POST /v1/staking  { body_json, signature_hex }
        let stake_route = warp::path!("v1" / "staking")
            .and(warp::post())
            .and(warp::body::content_length_limit(MAX_BODY_BYTES))
            .and(warp::body::json::<crate::staking::SignedStake>())
            .and(with_gov.clone())
            .and_then(|stake: crate::staking::SignedStake, gov: Arc<ValidatorGovernance>| async move {
                let reply = match gov.submit_stake(stake).await {
                    Ok(tx) => json_status(
                        serde_json::json!({ "status": "queued", "tx_hash": tx }),
                        StatusCode::ACCEPTED,
                    ),
                    Err(e) => json_status(serde_json::json!({ "error": e }), StatusCode::BAD_REQUEST),
                };
                Ok::<_, warp::Rejection>(reply)
            });

        // POST /v1/transfer  { body_json, signature_hex } — native ASTRA from
        // the address of a SPHINCS+ DID (see chain_consensus::TransferBody).
        let transfer_route = warp::path!("v1" / "transfer")
            .and(warp::post())
            .and(warp::body::content_length_limit(MAX_BODY_BYTES))
            .and(warp::body::json::<crate::chain_consensus::SignedTransfer>())
            .and(with_gov.clone())
            .and_then(
                |transfer: crate::chain_consensus::SignedTransfer, gov: Arc<ValidatorGovernance>| async move {
                    let reply = match gov.submit_transfer(transfer).await {
                        Ok(tx) => json_status(
                            serde_json::json!({ "status": "queued", "tx_hash": tx }),
                            StatusCode::ACCEPTED,
                        ),
                        Err(e) => json_status(serde_json::json!({ "error": e }), StatusCode::BAD_REQUEST),
                    };
                    Ok::<_, warp::Rejection>(reply)
                },
            );

        // GET /v1/governance/export — every signed proposal and vote, so a
        // relay or a lagging node can replay them.
        let export_route = warp::path!("v1" / "governance" / "export")
            .and(warp::get())
            .and(with_gov)
            .and_then(|gov: Arc<ValidatorGovernance>| async move {
                let state = gov.snapshot().await;
                let proposals: Vec<_> = state
                    .proposals
                    .values()
                    .map(|p| {
                        serde_json::json!({
                            "proposal": p.signed,
                            "submitted_at": p.submitted_at,
                            "votes": p.votes.iter().map(|(did, v)| serde_json::json!({
                                "vote": SignedVote {
                                    proposal_id: p.id.clone(),
                                    voter_did: did.clone(),
                                    choice: v.choice,
                                    signature_hex: v.signature_hex.clone(),
                                },
                                "received_at": v.received_at,
                            })).collect::<Vec<_>>(),
                        })
                    })
                    .collect();
                Ok::<_, warp::Rejection>(json_status(
                    serde_json::json!({ "network": state.network, "proposals": proposals }),
                    StatusCode::OK,
                ))
            });

        overview_route
            .or(list_route)
            .unify()
            .or(detail_route)
            .unify()
            .or(submit_route)
            .unify()
            .or(vote_route)
            .unify()
            .or(export_route)
            .unify()
            .or(staking_route)
            .unify()
            .or(stake_route)
            .unify()
            .or(transfer_route)
            .unify()
            .boxed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic fake: a "signature" is sha256(pk || msg).
    fn fake_sign(pk: &[u8], msg: &[u8]) -> Vec<u8> {
        let mut h = Sha256::new();
        h.update(pk);
        h.update(msg);
        h.finalize().to_vec()
    }
    fn fake_verify(pk: &[u8], msg: &[u8], sig: &[u8]) -> bool {
        fake_sign(pk, msg) == sig
    }

    struct Op {
        did: String,
        pk: Vec<u8>,
    }

    fn op(seed: u8) -> Op {
        let pk = vec![seed; 64];
        let address = hex::encode(&Sha256::digest(&pk)[..20]);
        Op {
            did: format!("did:spacekit:testnet:{address}"),
            pk,
        }
    }

    fn genesis(ops: &[Op], min_lift: usize) -> GovernanceState {
        GovernanceState::from_genesis(
            &PoaGenesis {
                network: "testnet".into(),
                min_validators_to_lift: Some(min_lift),
                block_production: None,
                staking: None,
                pos_grace_days: None,
                treasury: None,
                authorities: ops
                    .iter()
                    .map(|o| GenesisAuthority {
                        did: o.did.clone(),
                        sphincs_pk_hex: hex::encode(&o.pk),
                        name: None,
                    })
                    .collect(),
            },
            1_000,
        )
        .unwrap()
    }

    fn propose(state: &GovernanceState, by: &Op, action: ProposalAction, now: i64) -> SignedProposal {
        let body = ProposalBody {
            version: 1,
            network: "testnet".into(),
            action,
            title: "t".into(),
            description: String::new(),
            proposer_did: by.did.clone(),
            electorate_hash: state.electorate_hash(),
            created_at: now,
            expires_at: now + DEFAULT_VOTING_PERIOD_SECS,
        };
        let body_json = serde_json::to_string(&body).unwrap();
        let sig = fake_sign(&by.pk, &proposal_signing_payload(&body_json));
        SignedProposal {
            body_json,
            signature_hex: hex::encode(sig),
        }
    }

    fn vote(id: &str, by: &Op, choice: VoteChoice) -> SignedVote {
        SignedVote {
            proposal_id: id.to_string(),
            voter_did: by.did.clone(),
            choice,
            signature_hex: hex::encode(fake_sign(&by.pk, &vote_signing_payload("testnet", id, choice))),
        }
    }

    fn new_id(o: SubmitOutcome) -> String {
        match o {
            SubmitOutcome::New { id, .. } => id,
            other => panic!("expected new, got {other:?}"),
        }
    }

    #[test]
    fn thresholds() {
        assert_eq!(approvals_needed(3), 2);
        assert_eq!(approvals_needed(4), 3);
        assert_eq!(approvals_needed(10), 7);
        assert_eq!(fault_tolerance(3), 0);
        assert_eq!(fault_tolerance(4), 1);
        assert_eq!(fault_tolerance(10), 3);
    }

    #[test]
    fn add_authority_executes_at_two_thirds() {
        let ops: Vec<Op> = (1..=4).map(op).collect();
        let mut s = genesis(&ops, 10);
        let newcomer = op(9);
        let p = propose(
            &s,
            &ops[0],
            ProposalAction::AddAuthority {
                did: newcomer.did.clone(),
                sphincs_pk_hex: hex::encode(&newcomer.pk),
                name: Some("Operator 5".into()),
            },
            1_000,
        );
        let id = new_id(s.submit_proposal(&p, 1_000, &fake_verify).unwrap());
        assert_eq!(s.submit_proposal(&p, 1_000, &fake_verify).unwrap(), SubmitOutcome::Duplicate { id: id.clone() });

        for o in &ops[..2] {
            let out = s.submit_vote(&vote(&id, o, VoteChoice::Approve), 1_001, &fake_verify).unwrap();
            assert_eq!(out, VoteOutcome::Recorded { effects: vec![] });
        }
        let out = s.submit_vote(&vote(&id, &ops[2], VoteChoice::Approve), 1_002, &fake_verify).unwrap();
        assert_eq!(
            out,
            VoteOutcome::Recorded {
                effects: vec![GovernanceEffect::AuthorityAdded {
                    did: newcomer.did.clone(),
                    sphincs_public_key: newcomer.pk.clone()
                }]
            }
        );
        assert_eq!(s.proposals[&id].status, ProposalStatus::Executed);
        assert_eq!(s.authorities.len(), 5);
        assert!(s.submit_vote(&vote(&id, &ops[3], VoteChoice::Approve), 1_003, &fake_verify).is_err());
    }

    #[test]
    fn rejects_bad_signatures_non_authorities_and_wrong_network() {
        let ops: Vec<Op> = (1..=3).map(op).collect();
        let mut s = genesis(&ops, 10);
        let outsider = op(7);
        let p = propose(&s, &outsider, ProposalAction::RemoveAuthority { did: ops[0].did.clone() }, 1_000);
        assert!(s.submit_proposal(&p, 1_000, &fake_verify).is_err());

        let mut p = propose(&s, &ops[0], ProposalAction::RemoveAuthority { did: ops[1].did.clone() }, 1_000);
        p.signature_hex = hex::encode([0u8; 32]);
        assert!(s.submit_proposal(&p, 1_000, &fake_verify).is_err());

        let p = propose(&s, &ops[0], ProposalAction::RemoveAuthority { did: ops[1].did.clone() }, 1_000);
        let id = new_id(s.submit_proposal(&p, 1_000, &fake_verify).unwrap());
        assert!(s.submit_vote(&vote(&id, &outsider, VoteChoice::Approve), 1_001, &fake_verify).is_err());
        // A vote signed for another network does not verify here.
        let mut v = vote(&id, &ops[1], VoteChoice::Approve);
        v.signature_hex = hex::encode(fake_sign(&ops[1].pk, &vote_signing_payload("mainnet", &id, VoteChoice::Approve)));
        assert!(s.submit_vote(&v, 1_001, &fake_verify).is_err());
        // An approve signature cannot be presented as a reject.
        let mut v = vote(&id, &ops[1], VoteChoice::Approve);
        v.choice = VoteChoice::Reject;
        assert!(s.submit_vote(&v, 1_001, &fake_verify).is_err());
    }

    #[test]
    fn lift_poa_requires_ten_validators() {
        let ops: Vec<Op> = (1..=9).map(op).collect();
        let mut s = genesis(&ops, 10);
        let p = propose(&s, &ops[0], ProposalAction::LiftPoa, 1_000);
        let err = s.submit_proposal(&p, 1_000, &fake_verify).unwrap_err();
        assert!(err.to_string().contains("at least 10"), "{err}");

        let ops: Vec<Op> = (1..=10).map(op).collect();
        let mut s = genesis(&ops, 10);
        let p = propose(&s, &ops[0], ProposalAction::LiftPoa, 1_000);
        let id = new_id(s.submit_proposal(&p, 1_000, &fake_verify).unwrap());
        let mut last = None;
        for o in &ops[..7] {
            last = Some(s.submit_vote(&vote(&id, o, VoteChoice::Approve), 1_001, &fake_verify).unwrap());
        }
        assert_eq!(last.unwrap(), VoteOutcome::Recorded { effects: vec![GovernanceEffect::ProofOfStakeActivated] });
        assert_eq!(s.mode, ConsensusMode::ProofOfStake);
        assert_eq!(s.pos_activated_at, Some(1_001));
        // Governance is closed afterwards.
        let p = propose(&s, &ops[0], ProposalAction::RemoveAuthority { did: ops[1].did.clone() }, 1_002);
        assert!(s.submit_proposal(&p, 1_002, &fake_verify).is_err());
    }

    #[test]
    fn set_block_production_changes_the_state_hash() {
        use crate::block_production::{BlockProduction, ProductionMode};
        let ops: Vec<Op> = (1..=4).map(op).collect();
        let mut s = genesis(&ops, 10);
        assert_eq!(s.block_production.mode, ProductionMode::OnDemand);
        let before = s.state_hash();

        // Invalid and no-op settings are refused up front.
        let bad = ProposalAction::SetBlockProduction { config: BlockProduction::interval(10) };
        assert!(s.submit_proposal(&propose(&s, &ops[0], bad, 1_000), 1_000, &fake_verify).is_err());
        let same = ProposalAction::SetBlockProduction { config: BlockProduction::default() };
        assert!(s.submit_proposal(&propose(&s, &ops[0], same, 1_000), 1_000, &fake_verify).is_err());

        let config = BlockProduction::interval(1_000);
        let action = ProposalAction::SetBlockProduction { config: config.clone() };
        let id = new_id(s.submit_proposal(&propose(&s, &ops[0], action, 1_000), 1_000, &fake_verify).unwrap());
        let mut last = None;
        for o in &ops[..3] {
            last = Some(s.submit_vote(&vote(&id, o, VoteChoice::Approve), 1_001, &fake_verify).unwrap());
        }
        assert_eq!(last.unwrap(), VoteOutcome::Recorded { effects: vec![GovernanceEffect::BlockProductionChanged] });
        assert_eq!(s.block_production, config);
        assert_ne!(s.state_hash(), before);
        // The electorate did not change.
        assert_eq!(s.authorities.len(), 4);
    }

    #[test]
    fn rejection_and_expiry() {
        let ops: Vec<Op> = (1..=4).map(op).collect();
        let mut s = genesis(&ops, 10);
        let p = propose(&s, &ops[0], ProposalAction::RemoveAuthority { did: ops[3].did.clone() }, 1_000);
        let id = new_id(s.submit_proposal(&p, 1_000, &fake_verify).unwrap());
        // n=4 needs 3; two rejects make 3 approvals impossible.
        s.submit_vote(&vote(&id, &ops[1], VoteChoice::Reject), 1_001, &fake_verify).unwrap();
        assert_eq!(s.proposals[&id].status, ProposalStatus::Pending);
        s.submit_vote(&vote(&id, &ops[2], VoteChoice::Reject), 1_002, &fake_verify).unwrap();
        assert_eq!(s.proposals[&id].status, ProposalStatus::Rejected);

        let p = propose(&s, &ops[0], ProposalAction::RemoveAuthority { did: ops[2].did.clone() }, 1_010);
        let id = new_id(s.submit_proposal(&p, 1_010, &fake_verify).unwrap());
        let late = 1_010 + DEFAULT_VOTING_PERIOD_SECS;
        assert!(s.submit_vote(&vote(&id, &ops[1], VoteChoice::Approve), late, &fake_verify).is_err());
        assert_eq!(s.proposals[&id].status, ProposalStatus::Expired);
    }

    #[test]
    fn membership_change_makes_other_proposals_stale() {
        let ops: Vec<Op> = (1..=3).map(op).collect();
        let mut s = genesis(&ops, 10);
        let a = op(20);
        let b = op(21);
        let add = |s: &GovernanceState, o: &Op| {
            propose(s, &ops[0], ProposalAction::AddAuthority {
                did: o.did.clone(), sphincs_pk_hex: hex::encode(&o.pk), name: None,
            }, 1_000)
        };
        let pa = add(&s, &a);
        let pb = add(&s, &b);
        let ida = new_id(s.submit_proposal(&pa, 1_000, &fake_verify).unwrap());
        let idb = new_id(s.submit_proposal(&pb, 1_000, &fake_verify).unwrap());
        for o in &ops[..2] {
            s.submit_vote(&vote(&ida, o, VoteChoice::Approve), 1_001, &fake_verify).unwrap();
        }
        assert_eq!(s.proposals[&ida].status, ProposalStatus::Executed);
        assert_eq!(s.proposals[&idb].status, ProposalStatus::Stale);
        // Re-proposing under the old electorate is refused with a conflict.
        let old = pb.clone();
        let mut fresh = s.clone();
        fresh.proposals.remove(&idb);
        assert_eq!(fresh.submit_proposal(&old, 1_002, &fake_verify).unwrap_err(), GovernanceError::StaleElectorate);
    }

    #[test]
    fn genesis_rejects_mismatched_did() {
        let o = op(1);
        let g = PoaGenesis {
            network: "testnet".into(),
            min_validators_to_lift: None,
            block_production: None,
            staking: None,
            pos_grace_days: None,
            treasury: None,
            authorities: vec![GenesisAuthority {
                did: "did:spacekit:testnet:0000".into(),
                sphincs_pk_hex: hex::encode(&o.pk),
                name: None,
            }],
        };
        assert!(GovernanceState::from_genesis(&g, 0).is_err());
    }

    #[test]
    fn cannot_remove_last_authority() {
        let ops: Vec<Op> = (1..=1).map(op).collect();
        let mut s = genesis(&ops, 10);
        let p = propose(&s, &ops[0], ProposalAction::RemoveAuthority { did: ops[0].did.clone() }, 1_000);
        assert!(s.submit_proposal(&p, 1_000, &fake_verify).is_err());
    }
}
