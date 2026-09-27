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
//! Proposals and votes travel over P2P gossip and are applied idempotently.
//! Governance is not yet ordered by the chain: two changes decided at the same
//! moment on different nodes could be applied in different orders. Authorities
//! should run one membership change at a time during bootstrap. Every node
//! publishes [`GovernanceState::state_hash`] so divergence is visible
//! (`GET /v1/chain/status`).

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use crate::consensus_coordinator::ConsensusCoordinator;
use crate::network::{NetworkService, P2PMessage};

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
const MAX_ORPHAN_VOTES: usize = 1_024;

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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tally {
    pub approve: usize,
    pub reject: usize,
    pub eligible: usize,
    pub needed: usize,
}

/// A change the validator set must reflect after a proposal executes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GovernanceEffect {
    AuthorityAdded { did: String, sphincs_public_key: Vec<u8> },
    AuthorityRemoved { did: String },
    ProofOfStakeActivated,
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
        }
    }

    pub fn from_genesis(genesis: &PoaGenesis, now: i64) -> Result<Self> {
        if genesis.authorities.is_empty() {
            bail!("PoA genesis must name at least one authority");
        }
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
        })
    }

    pub fn is_poa(&self) -> bool {
        self.mode == ConsensusMode::ProofOfAuthority
    }

    pub fn electorate_hash(&self) -> String {
        electorate_hash(self.authorities.keys().map(String::as_str))
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
        if !self.is_poa() {
            return Err(invalid(
                "the network has left proof of authority; validator-set governance is closed",
            ));
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

        let proposer_key = self
            .authority_key(&body.proposer_did)
            .ok_or_else(|| invalid(format!("{} is not an authority", body.proposer_did)))?;
        let signature = hex::decode(signed.signature_hex.trim())
            .map_err(|_| invalid("signature_hex is not hex"))?;
        if !verify(
            &proposer_key,
            &proposal_signing_payload(&signed.body_json),
            &signature,
        ) {
            return Err(invalid("proposal signature does not verify against the proposer's key"));
        }
        if body.electorate_hash != self.electorate_hash() {
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
        let key = self.authority_key(&vote.voter_did);
        let record = self
            .proposals
            .get(&vote.proposal_id)
            .ok_or(GovernanceError::UnknownProposal)?;
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
        let key = key.ok_or_else(|| invalid(format!("{} is not an authority", vote.voter_did)))?;
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
        let (approve, reject) = self.tally(record);
        let n = self.authorities.len();
        Tally {
            approve,
            reject,
            eligible: n,
            needed: approvals_needed(n),
        }
    }

    fn try_decide(&mut self, id: &str, now: i64) -> Vec<GovernanceEffect> {
        let Some(record) = self.proposals.get(id) else {
            return Vec::new();
        };
        if record.status != ProposalStatus::Pending {
            return Vec::new();
        }
        if record.body.electorate_hash != self.electorate_hash() || !self.is_poa() {
            self.set_status(id, ProposalStatus::Stale, now, None);
            return Vec::new();
        }
        let n = self.authorities.len();
        let needed = approvals_needed(n);
        let (approve, reject) = self.tally(record);
        let action = record.body.action.clone();
        let frozen = Tally {
            approve,
            reject,
            eligible: n,
            needed,
        };
        if approve >= needed || reject > n - needed {
            if let Some(p) = self.proposals.get_mut(id) {
                p.final_tally = Some(frozen);
            }
        }
        if approve >= needed {
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
        } else if reject > n - needed {
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
                self.authorities.remove(did);
                Ok(vec![GovernanceEffect::AuthorityRemoved { did: did.clone() }])
            }
            ProposalAction::LiftPoa => {
                self.mode = ConsensusMode::ProofOfStake;
                self.pos_activated_at = Some(now);
                Ok(vec![GovernanceEffect::ProofOfStakeActivated])
            }
        }
    }
}

// ── Node service ────────────────────────────────────────────────────────

/// Verify a SPHINCS+ signature (the scheme validator keys use).
pub fn sphincs_verify(public_key: &[u8], message: &[u8], signature: &[u8]) -> bool {
    spacekit_did::sphincs::SphincsPlus::verify(public_key, message, signature)
}

/// Where gossip and HTTP submissions land.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Local,
    Gossip,
}

pub struct ValidatorGovernance {
    state: RwLock<GovernanceState>,
    state_path: Option<PathBuf>,
    coordinator: Arc<ConsensusCoordinator>,
    network: RwLock<Option<NetworkService>>,
    /// Votes that arrived before their proposal (gossip reordering).
    orphan_votes: RwLock<VecDeque<SignedVote>>,
    pos_grace_secs: i64,
}

impl ValidatorGovernance {
    /// Load persisted state, or start from the PoA genesis file, or fall back
    /// to plain proof of stake when neither exists.
    ///
    /// Environment:
    /// - `SPACEKIT_GOVERNANCE_STATE_PATH` (default
    ///   `temp_blockchain_storage/governance.json`)
    /// - `SPACEKIT_POA_GENESIS_FILE`: genesis authorities; read only when no
    ///   state file exists yet
    /// - `SPACEKIT_POS_GRACE_DAYS` (default 30)
    pub fn from_env(network: &str, coordinator: Arc<ConsensusCoordinator>) -> Result<Self> {
        let state_path = PathBuf::from(
            std::env::var("SPACEKIT_GOVERNANCE_STATE_PATH")
                .unwrap_or_else(|_| "temp_blockchain_storage/governance.json".to_string()),
        );
        let genesis_path = std::env::var("SPACEKIT_POA_GENESIS_FILE")
            .ok()
            .filter(|p| !p.trim().is_empty())
            .map(PathBuf::from);
        let grace_days: i64 = std::env::var("SPACEKIT_POS_GRACE_DAYS")
            .ok()
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(DEFAULT_POS_GRACE_SECS / 86_400);
        Self::load(
            network,
            Some(state_path),
            genesis_path.as_deref(),
            coordinator,
            grace_days.max(0) * 86_400,
        )
    }

    pub fn load(
        network: &str,
        state_path: Option<PathBuf>,
        genesis_path: Option<&Path>,
        coordinator: Arc<ConsensusCoordinator>,
        pos_grace_secs: i64,
    ) -> Result<Self> {
        let existing = match &state_path {
            Some(path) if path.exists() => {
                let raw = std::fs::read_to_string(path)
                    .map_err(|e| anyhow!("reading {}: {e}", path.display()))?;
                let state: GovernanceState = serde_json::from_str(&raw)
                    .map_err(|e| anyhow!("parsing {}: {e}", path.display()))?;
                if state.network != network {
                    bail!(
                        "{} belongs to network {:?}, this node runs {:?}",
                        path.display(),
                        state.network,
                        network
                    );
                }
                Some(state)
            }
            _ => None,
        };
        let state = match (existing, genesis_path) {
            (Some(state), _) => state,
            (None, Some(path)) => {
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
                let state = GovernanceState::from_genesis(&genesis, now_unix())?;
                info!(
                    authorities = state.authorities.len(),
                    "Starting in proof of authority from genesis {}",
                    path.display()
                );
                state
            }
            (None, None) => GovernanceState::proof_of_stake(network),
        };
        let gov = Self {
            state: RwLock::new(state),
            state_path,
            coordinator,
            network: RwLock::new(None),
            orphan_votes: RwLock::new(VecDeque::new()),
            pos_grace_secs,
        };
        Ok(gov)
    }

    pub async fn snapshot(&self) -> GovernanceState {
        let mut state = self.state.read().await.clone();
        state.expire(now_unix());
        state
    }

    pub async fn is_poa(&self) -> bool {
        self.state.read().await.is_poa()
    }

    fn persist(&self, state: &GovernanceState) {
        let Some(path) = &self.state_path else { return };
        let result = (|| -> Result<()> {
            if let Some(dir) = path.parent() {
                if !dir.as_os_str().is_empty() {
                    std::fs::create_dir_all(dir)?;
                }
            }
            let tmp = path.with_extension("json.tmp");
            std::fs::write(&tmp, serde_json::to_vec_pretty(state)?)?;
            std::fs::rename(&tmp, path)?;
            Ok(())
        })();
        if let Err(e) = result {
            warn!("could not persist governance state to {}: {e}", path.display());
        }
    }

    /// Register every authority with the coordinator (at startup).
    pub async fn sync_coordinator(&self) {
        let state = self.state.read().await.clone();
        if !state.is_poa() && !self.within_pos_grace(&state) {
            return;
        }
        for a in state.authorities.values() {
            let Ok(pk) = hex::decode(&a.sphincs_pk_hex) else { continue };
            if let Err(e) = self.coordinator.register_authority(a.did.clone(), pk).await {
                warn!("could not register authority {}: {e}", a.did);
            }
        }
    }

    fn within_pos_grace(&self, state: &GovernanceState) -> bool {
        state
            .pos_activated_at
            .is_some_and(|at| now_unix() < at + self.pos_grace_secs)
    }

    async fn apply_effects(&self, effects: &[GovernanceEffect]) {
        for effect in effects {
            match effect {
                GovernanceEffect::AuthorityAdded {
                    did,
                    sphincs_public_key,
                } => {
                    info!("Governance admitted authority {did}");
                    if let Err(e) = self
                        .coordinator
                        .register_authority(did.clone(), sphincs_public_key.clone())
                        .await
                    {
                        warn!("could not register authority {did}: {e}");
                    }
                }
                GovernanceEffect::AuthorityRemoved { did } => {
                    info!("Governance removed authority {did}");
                    self.coordinator.remove_validator(did).await;
                }
                GovernanceEffect::ProofOfStakeActivated => {
                    info!(
                        "Governance lifted proof of authority: the network is now proof of \
                         stake. Authorities must register stake within {} days.",
                        self.pos_grace_secs / 86_400
                    );
                }
            }
        }
    }

    async fn broadcast(&self, msg: P2PMessage) {
        if let Some(net) = self.network.read().await.as_ref() {
            if let Err(e) = net.broadcast(msg) {
                debug!("governance gossip failed: {e}");
            }
        }
    }

    pub async fn submit_proposal(
        &self,
        signed: SignedProposal,
        source: Source,
    ) -> Result<SubmitOutcome, GovernanceError> {
        let outcome = {
            let mut state = self.state.write().await;
            state.expire(now_unix());
            let outcome = state.submit_proposal(&signed, now_unix(), &sphincs_verify)?;
            if matches!(outcome, SubmitOutcome::New { .. }) {
                self.persist(&state);
            }
            outcome
        };
        if let SubmitOutcome::New { id, .. } = &outcome {
            info!(proposal = %id, ?source, "Accepted governance proposal");
            if let Ok(json) = serde_json::to_string(&signed) {
                self.broadcast(P2PMessage::GovernanceProposal { signed_json: json })
                    .await;
            }
            self.retry_orphans(id).await;
        }
        Ok(outcome)
    }

    pub async fn submit_vote(
        &self,
        vote: SignedVote,
        source: Source,
    ) -> Result<VoteOutcome, GovernanceError> {
        let result = {
            let mut state = self.state.write().await;
            let result = state.submit_vote(&vote, now_unix(), &sphincs_verify);
            if matches!(result, Ok(VoteOutcome::Recorded { .. })) {
                self.persist(&state);
            }
            result
        };
        match &result {
            Ok(VoteOutcome::Recorded { effects }) => {
                info!(proposal = %vote.proposal_id, voter = %vote.voter_did, ?source,
                    "Recorded governance vote");
                self.apply_effects(effects).await;
                if let Ok(json) = serde_json::to_string(&vote) {
                    self.broadcast(P2PMessage::GovernanceVote { signed_json: json })
                        .await;
                }
            }
            Err(GovernanceError::UnknownProposal) if source == Source::Gossip => {
                let mut orphans = self.orphan_votes.write().await;
                if orphans.len() >= MAX_ORPHAN_VOTES {
                    orphans.pop_front();
                }
                orphans.push_back(vote);
            }
            _ => {}
        }
        result
    }

    async fn retry_orphans(&self, proposal_id: &str) {
        let ready: Vec<SignedVote> = {
            let mut orphans = self.orphan_votes.write().await;
            let (ready, rest): (Vec<_>, Vec<_>) = orphans
                .drain(..)
                .partition(|v| v.proposal_id == proposal_id);
            orphans.extend(rest);
            ready
        };
        for vote in ready {
            if let Err(e) = self.submit_vote(vote, Source::Gossip).await {
                debug!("orphan governance vote rejected: {e}");
            }
        }
    }

    /// Listen for governance gossip and re-broadcast anything new.
    pub fn start_p2p_listener(self: &Arc<Self>, network: NetworkService) -> tokio::task::JoinHandle<()> {
        let gov = self.clone();
        tokio::spawn(async move {
            *gov.network.write().await = Some(network.clone());
            let mut rx = network.subscribe();
            loop {
                match rx.recv().await {
                    Ok(P2PMessage::GovernanceProposal { signed_json }) => {
                        match serde_json::from_str::<SignedProposal>(&signed_json) {
                            Ok(signed) => {
                                if let Err(e) = gov.submit_proposal(signed, Source::Gossip).await {
                                    debug!("gossiped governance proposal rejected: {e}");
                                }
                            }
                            Err(e) => debug!("malformed governance proposal gossip: {e}"),
                        }
                    }
                    Ok(P2PMessage::GovernanceVote { signed_json }) => {
                        match serde_json::from_str::<SignedVote>(&signed_json) {
                            Ok(vote) => {
                                if let Err(e) = gov.submit_vote(vote, Source::Gossip).await {
                                    debug!("gossiped governance vote rejected: {e}");
                                }
                            }
                            Err(e) => debug!("malformed governance vote gossip: {e}"),
                        }
                    }
                    Ok(_) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        warn!("governance listener lagged {n} messages");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        })
    }

    /// Expire proposals and, once the PoS grace period ends, drop authorities
    /// that have not registered stake.
    pub fn start_ticker(self: &Arc<Self>) -> tokio::task::JoinHandle<()> {
        let gov = self.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
            loop {
                interval.tick().await;
                gov.tick().await;
            }
        })
    }

    pub async fn tick(&self) {
        let now = now_unix();
        let state = {
            let mut state = self.state.write().await;
            let before = state.clone();
            state.expire(now);
            if *state != before {
                self.persist(&state);
            }
            state.clone()
        };
        if state.is_poa() || self.within_pos_grace(&state) {
            return;
        }
        let Some(activated) = state.pos_activated_at else { return };
        if now < activated + self.pos_grace_secs {
            return;
        }
        let dropped = self
            .coordinator
            .remove_unstaked_authorities(&state.authorities.keys().cloned().collect::<Vec<_>>())
            .await;
        for did in dropped {
            info!("PoS grace period over: {did} has no registered stake and stops validating");
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

    fn mode_str(mode: ConsensusMode) -> &'static str {
        match mode {
            ConsensusMode::ProofOfAuthority => "proof_of_authority",
            ConsensusMode::ProofOfStake => "proof_of_stake",
        }
    }

    /// Summary of the validator set and governance rules.
    pub fn overview(state: &GovernanceState, pos_grace_secs: i64) -> serde_json::Value {
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
            "fault_tolerance": fault_tolerance(n),
            "min_validators_to_lift": state.min_validators_to_lift,
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
                    overview(&state, gov.pos_grace_secs),
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
                let reply = match gov.submit_proposal(signed, Source::Local).await {
                    Ok(SubmitOutcome::New { id, .. }) => json_status(
                        serde_json::json!({ "status": "accepted", "id": id }),
                        StatusCode::CREATED,
                    ),
                    Ok(SubmitOutcome::Duplicate { id }) => json_status(
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
                let reply = match gov.submit_vote(vote, Source::Local).await {
                    Ok(outcome) => {
                        let state = gov.snapshot().await;
                        let proposal = state.proposals.get(&id).map(|p| proposal_view(&state, p, false));
                        json_status(
                            serde_json::json!({
                                "status": if matches!(outcome, VoteOutcome::Duplicate) { "duplicate" } else { "recorded" },
                                "proposal": proposal,
                            }),
                            StatusCode::OK,
                        )
                    }
                    Err(e) => error_reply(e),
                };
                Ok::<_, warp::Rejection>(reply)
            });

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
                            "votes": p.votes.iter().map(|(did, v)| SignedVote {
                                proposal_id: p.id.clone(),
                                voter_did: did.clone(),
                                choice: v.choice,
                                signature_hex: v.signature_hex.clone(),
                            }).collect::<Vec<_>>(),
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
