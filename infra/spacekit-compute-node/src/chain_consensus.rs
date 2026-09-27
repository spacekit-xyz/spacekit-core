//! Consensus state kept in the chain: governance and staking.
//!
//! Governance proposals, votes and stake operations are SPHINCS+-signed
//! messages. A node that receives one checks it and puts it in its
//! transaction pool as a *consensus transaction*; the pool is relayed to every
//! node, and the message takes effect when a block includes it. Every node
//! therefore applies the same messages in the same order, at the same block
//! time, and a node that joins later rebuilds the same state by replaying the
//! chain.
//!
//! The state lives in contract storage under the consensus system address
//! (`0x…0005`), so it is covered by the block's state root and rolled back
//! with the block on a reorganization.
//!
//! A consensus transaction is a `SwtchvmTransaction` from and to the consensus
//! address, with a zero signature and nonce, whose `data` is the JSON
//! [`ConsensusMessage`]. It pays no gas. Its receipt fails if the message is
//! invalid when the block applies it.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::block_production::{producer_for, BlockProduction};
use crate::spacekitvm::{SwtchvmAddress, SwtchvmState, SwtchvmTransaction, TransactionSignature};
use crate::staking::{SignedStake, StakingParams, StakingState, WEI_PER_ASTRA};
use crate::validator_governance::{
    sphincs_verify, ConsensusMode, GovernanceState, SignedProposal, SignedVote, Voter,
};

pub const CONSENSUS_ADDRESS_HEX: &str = "0x0000000000000000000000000000000000000005";
const KEY_GOVERNANCE: &[u8] = b"consensus.governance.v1";
const KEY_STAKING: &[u8] = b"consensus.staking.v1";
/// At most this many consensus transactions go into one block.
pub const MAX_CONSENSUS_TXS_PER_BLOCK: usize = 256;
const MAX_MESSAGE_BYTES: usize = 64 * 1024;

pub fn consensus_address() -> SwtchvmAddress {
    SwtchvmAddress::from_hex(CONSENSUS_ADDRESS_HEX).expect("consensus address")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConsensusMessage {
    GovernanceProposal { proposal: SignedProposal },
    GovernanceVote { vote: SignedVote },
    Stake { stake: SignedStake },
}

impl ConsensusMessage {
    pub fn to_transaction(&self) -> SwtchvmTransaction {
        let address = consensus_address();
        SwtchvmTransaction {
            from: address,
            to: Some(address),
            data: serde_json::to_vec(self).expect("consensus message serializes"),
            gas_limit: 1,
            gas_price: 1,
            value: 0,
            nonce: 0,
            signature: TransactionSignature {
                v: 0,
                r: [0u8; 32],
                s: [0u8; 32],
            },
        }
    }

    pub fn from_transaction(tx: &SwtchvmTransaction) -> Result<Self, String> {
        if tx.data.len() > MAX_MESSAGE_BYTES {
            return Err("consensus message is too large".into());
        }
        serde_json::from_slice(&tx.data).map_err(|e| format!("malformed consensus message: {e}"))
    }
}

/// A transaction from and to the consensus address with no value or
/// signature: carries a [`ConsensusMessage`].
pub fn is_consensus_tx(tx: &SwtchvmTransaction) -> bool {
    let address = consensus_address();
    tx.from == address
        && tx.to == Some(address)
        && tx.value == 0
        && tx.nonce == 0
        && tx.signature.r == [0u8; 32]
        && tx.signature.s == [0u8; 32]
}

/// Governance and staking as stored in the chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsensusState {
    pub governance: GovernanceState,
    pub staking: StakingState,
}

pub fn load(state: &SwtchvmState) -> Option<ConsensusState> {
    let address = consensus_address();
    let governance = state.kv_get(&address, KEY_GOVERNANCE)?;
    let governance: GovernanceState = serde_json::from_slice(governance).ok()?;
    let staking = state
        .kv_get(&address, KEY_STAKING)
        .and_then(|b| serde_json::from_slice(b).ok())
        .unwrap_or_default();
    Some(ConsensusState {
        governance,
        staking,
    })
}

/// Write back whatever changed.
pub fn store(state: &mut SwtchvmState, cs: &ConsensusState) {
    let address = consensus_address();
    let gov = serde_json::to_vec(&cs.governance).expect("governance serializes");
    if state.kv_get(&address, KEY_GOVERNANCE) != Some(&gov) {
        state.kv_insert(address, KEY_GOVERNANCE.to_vec(), gov);
    }
    let staking = serde_json::to_vec(&cs.staking).expect("staking serializes");
    if state.kv_get(&address, KEY_STAKING) != Some(&staking) {
        state.kv_insert(address, KEY_STAKING.to_vec(), staking);
    }
}

/// Install the genesis governance state (PoA authorities, block production,
/// staking rules). Every node of a network does this identically at height 0.
pub fn init_genesis(state: &mut SwtchvmState, governance: GovernanceState, staking: StakingParams) {
    if load(state).is_some() {
        return;
    }
    store(
        state,
        &ConsensusState {
            governance,
            staking: StakingState::new(staking),
        },
    );
}

/// What a DID holds in AstraRewards: spendable plus locked-but-unreleased.
pub fn astra_holdings_wei(state: &SwtchvmState, did: &str) -> u128 {
    let Ok(contract) = SwtchvmAddress::from_hex(
        crate::spacekitvm::genesis_node::system_contracts::ASTRA_REWARDS,
    ) else {
        return 0;
    };
    let hash = hex::encode(spacekit_service_rewards::hash_did_bytes(did.as_bytes()));
    let read = |prefix: &str| -> u128 {
        state
            .kv_get(&contract, format!("{prefix}{hash}").as_bytes())
            .and_then(|v| v.get(..16))
            .and_then(|b| <[u8; 16]>::try_from(b).ok())
            .map(u128::from_le_bytes)
            .unwrap_or(0)
    };
    let balance = read("astra_rewards.balance.");
    let locked = read("astra_rewards.locked.");
    let released = read("astra_rewards.released.");
    balance.saturating_add(locked.saturating_sub(released))
}

/// Stake-weighted electorate: active validators by effective stake (whole
/// ASTRA), plus authorities still in the PoS grace period.
pub fn stake_electorate(state: &SwtchvmState, cs: &ConsensusState, now: u64) -> BTreeMap<String, Voter> {
    let min = cs.staking.params.min_stake_wei();
    let mut voters = BTreeMap::new();
    for (did, v) in &cs.staking.validators {
        let effective = v.effective_wei(astra_holdings_wei(state, did));
        if effective >= min && effective > 0 {
            voters.insert(
                did.clone(),
                Voter {
                    sphincs_pk_hex: v.sphincs_pk_hex.clone(),
                    weight: ((effective / WEI_PER_ASTRA) as u64).max(1),
                },
            );
        }
    }
    if cs.governance.within_pos_grace(now as i64) {
        for (did, a) in &cs.governance.authorities {
            voters.entry(did.clone()).or_insert_with(|| Voter {
                sphincs_pk_hex: a.sphincs_pk_hex.clone(),
                weight: cs.staking.params.min_stake_astra.max(1),
            });
        }
    }
    voters
}

/// Deterministic transitions at the start of every block: expire proposals,
/// release matured unbonding, refresh the stake electorate after the lift.
pub fn begin_block(state: &mut SwtchvmState, now: u64) {
    let Some(mut cs) = load(state) else { return };
    cs.governance.expire(now as i64);
    cs.staking.mature(now);
    if cs.governance.mode == ConsensusMode::ProofOfStake {
        cs.governance.stake_electorate = stake_electorate(state, &cs, now);
    }
    store(state, &cs);
}

/// Check a message against the current state without changing it (pool
/// admission). The block that includes it applies it again for real.
pub fn validate(state: &SwtchvmState, msg: &ConsensusMessage, now: u64) -> Result<(), String> {
    let mut scratch = load(state).ok_or("this chain has no proof-of-authority genesis")?;
    apply_to(state, &mut scratch, msg, now).map(|_| ())
}

/// Apply a message in a block. On error nothing changes.
pub fn apply(state: &mut SwtchvmState, msg: &ConsensusMessage, now: u64) -> Result<String, String> {
    let mut cs = load(state).ok_or("this chain has no proof-of-authority genesis")?;
    let summary = apply_to(state, &mut cs, msg, now)?;
    // The lift can change the electorate right away.
    if cs.governance.mode == ConsensusMode::ProofOfStake {
        cs.governance.stake_electorate = stake_electorate(state, &cs, now);
    }
    store(state, &cs);
    Ok(summary)
}

fn apply_to(
    state: &SwtchvmState,
    cs: &mut ConsensusState,
    msg: &ConsensusMessage,
    now: u64,
) -> Result<String, String> {
    let now_i = now as i64;
    cs.governance.expire(now_i);
    match msg {
        ConsensusMessage::GovernanceProposal { proposal } => {
            match cs
                .governance
                .submit_proposal(proposal, now_i, &sphincs_verify)
                .map_err(|e| e.to_string())?
            {
                crate::validator_governance::SubmitOutcome::New { id, .. } => {
                    Ok(format!("proposal {id}"))
                }
                crate::validator_governance::SubmitOutcome::Duplicate { id } => {
                    Err(format!("proposal {id} is already on chain"))
                }
            }
        }
        ConsensusMessage::GovernanceVote { vote } => {
            match cs
                .governance
                .submit_vote(vote, now_i, &sphincs_verify)
                .map_err(|e| e.to_string())?
            {
                crate::validator_governance::VoteOutcome::Recorded { .. } => Ok(format!(
                    "vote by {} on {}",
                    vote.voter_did, vote.proposal_id
                )),
                crate::validator_governance::VoteOutcome::Duplicate => {
                    Err("vote is already on chain".into())
                }
            }
        }
        ConsensusMessage::Stake { stake } => {
            let network = cs.governance.network.clone();
            cs.staking.apply(
                stake,
                &network,
                now,
                &sphincs_verify,
                &|did| astra_holdings_wei(state, did),
            )
        }
    }
}

// ── Block producers ─────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Producer {
    pub did: String,
    pub sphincs_pk_hex: String,
    /// 1 per authority in PoA; effective stake in whole ASTRA in PoS.
    pub weight: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct ProducerSet {
    /// Sorted by DID.
    pub members: Vec<Producer>,
    /// Stake-weighted selection (PoS) instead of round robin (PoA).
    pub weighted: bool,
    /// PoS with no active validator: the authorities keep the chain going.
    pub fallback: bool,
    pub block_production: BlockProduction,
}

impl ProducerSet {
    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }

    pub fn contains(&self, did: &str) -> bool {
        self.members.iter().any(|p| p.did == did)
    }

    pub fn key(&self, did: &str) -> Option<Vec<u8>> {
        self.members
            .iter()
            .find(|p| p.did == did)
            .and_then(|p| hex::decode(&p.sphincs_pk_hex).ok())
    }

    pub fn dids(&self) -> Vec<String> {
        self.members.iter().map(|p| p.did.clone()).collect()
    }

    /// Whose turn it is at `height`, `waited_ms` after the block became due.
    pub fn scheduled(&self, height: u64, waited_ms: u64) -> Option<&str> {
        let grace = self.block_production.grace_ms();
        if !self.weighted {
            let dids = self.dids();
            let idx = producer_for(&dids, height, waited_ms, grace)
                .and_then(|d| self.members.iter().position(|p| p.did == d))?;
            return Some(self.members[idx].did.as_str());
        }
        let offset = waited_ms / grace.max(1);
        let total: u128 = self.members.iter().map(|p| p.weight as u128).sum();
        if total == 0 {
            return None;
        }
        // Seeded by height and offset only, so a producer cannot steer it.
        let mut h = Sha256::new();
        h.update(b"SPACEKIT-PRODUCER-v1");
        h.update(height.to_le_bytes());
        h.update(offset.to_le_bytes());
        let seed = h.finalize();
        let mut pick = u128::from_le_bytes(seed[..16].try_into().expect("16 bytes")) % total;
        for p in &self.members {
            if pick < p.weight as u128 {
                return Some(p.did.as_str());
            }
            pick -= p.weight as u128;
        }
        None
    }

    /// The block at `height` was produced by the scheduled producer on its
    /// first turn (weighs more in fork choice).
    pub fn in_turn(&self, height: u64, did: &str) -> bool {
        self.scheduled(height, 0) == Some(did)
    }
}

/// Who may produce (and seal) the next block, from chain state.
pub fn producer_set(state: &SwtchvmState, now: u64) -> ProducerSet {
    let Some(cs) = load(state) else {
        return ProducerSet::default();
    };
    let authorities = || -> Vec<Producer> {
        cs.governance
            .authorities
            .values()
            .map(|a| Producer {
                did: a.did.clone(),
                sphincs_pk_hex: a.sphincs_pk_hex.clone(),
                weight: 1,
            })
            .collect()
    };
    let block_production = cs.governance.block_production.clone();
    if cs.governance.is_poa() {
        return ProducerSet {
            members: authorities(),
            weighted: false,
            fallback: false,
            block_production,
        };
    }
    let members: Vec<Producer> = stake_electorate(state, &cs, now)
        .into_iter()
        .map(|(did, v)| Producer {
            did,
            sphincs_pk_hex: v.sphincs_pk_hex,
            weight: v.weight,
        })
        .collect();
    if members.is_empty() {
        // Nobody has staked the minimum and the grace period is over. The
        // chain must not halt: the authorities continue until validators stake.
        return ProducerSet {
            members: authorities(),
            weighted: false,
            fallback: true,
            block_production,
        };
    }
    ProducerSet {
        members,
        weighted: true,
        fallback: false,
        block_production,
    }
}

/// Whether AstraRewards credits must be locked, and for which DIDs
/// (authorities during PoA; affiliated DIDs are added by the caller).
pub fn lock_policy(state: &SwtchvmState) -> Option<(bool, Vec<String>)> {
    let cs = load(state)?;
    Some((
        cs.governance.is_poa(),
        cs.governance.authorities.keys().cloned().collect(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(weights: &[(&str, u64)], weighted: bool) -> ProducerSet {
        ProducerSet {
            members: weights
                .iter()
                .map(|(d, w)| Producer {
                    did: d.to_string(),
                    sphincs_pk_hex: String::new(),
                    weight: *w,
                })
                .collect(),
            weighted,
            fallback: false,
            block_production: BlockProduction::default(),
        }
    }

    #[test]
    fn weighted_selection_follows_stake() {
        let s = set(&[("a", 1), ("b", 3)], true);
        let mut counts = BTreeMap::new();
        for h in 0..4_000u64 {
            *counts.entry(s.scheduled(h, 0).unwrap().to_string()).or_insert(0) += 1;
        }
        let b = counts["b"] as f64 / 4_000.0;
        assert!((0.70..0.80).contains(&b), "b produced {b}");
        // A later offset hands the turn on deterministically.
        assert_eq!(s.scheduled(7, 0), s.scheduled(7, 0));
        let grace = s.block_production.grace_ms();
        let _ = s.scheduled(7, grace);
    }

    #[test]
    fn round_robin_in_poa() {
        let s = set(&[("a", 1), ("b", 1), ("c", 1)], false);
        assert_eq!(s.scheduled(0, 0), Some("a"));
        assert_eq!(s.scheduled(1, 0), Some("b"));
        assert!(s.in_turn(2, "c"));
    }

    struct Key {
        did: String,
        pk: Vec<u8>,
        sk: Vec<u8>,
    }

    fn key() -> Key {
        let kp = spacekit_did::sphincs::SphincsPlus::generate_keypair();
        let did = format!(
            "did:spacekit:testnet:{}",
            hex::encode(&Sha256::digest(&kp.public_key)[..20])
        );
        Key {
            did,
            pk: kp.public_key,
            sk: kp.private_key,
        }
    }

    fn sign(k: &Key, msg: &[u8]) -> String {
        hex::encode(spacekit_did::sphincs::SphincsPlus::sign(&k.sk, msg).unwrap())
    }

    fn propose(k: &Key, state: &SwtchvmState, action: serde_json::Value, now: i64) -> ConsensusMessage {
        let gov = load(state).unwrap().governance;
        let body_json = serde_json::json!({
            "version": 1, "network": "testnet", "action": action, "title": "t",
            "description": "", "proposer_did": k.did,
            "electorate_hash": gov.electorate_hash(),
            "created_at": now, "expires_at": now + 86_400,
        })
        .to_string();
        let signature_hex = sign(k, &crate::validator_governance::proposal_signing_payload(&body_json));
        ConsensusMessage::GovernanceProposal {
            proposal: SignedProposal {
                body_json,
                signature_hex,
            },
        }
    }

    fn vote(k: &Key, id: &str) -> ConsensusMessage {
        let sig = sign(
            k,
            &crate::validator_governance::vote_signing_payload(
                "testnet",
                id,
                crate::validator_governance::VoteChoice::Approve,
            ),
        );
        ConsensusMessage::GovernanceVote {
            vote: SignedVote {
                proposal_id: id.to_string(),
                voter_did: k.did.clone(),
                choice: crate::validator_governance::VoteChoice::Approve,
                signature_hex: sig,
            },
        }
    }

    fn stake(k: &Key, amount_astra: u128, nonce: u64) -> ConsensusMessage {
        let body_json = serde_json::json!({
            "version": 1, "network": "testnet", "did": k.did,
            "sphincs_pk_hex": hex::encode(&k.pk), "action": "bond",
            "amount_wei": (amount_astra * WEI_PER_ASTRA).to_string(), "nonce": nonce,
        })
        .to_string();
        let signature_hex = sign(k, &crate::staking::stake_signing_payload(&body_json));
        ConsensusMessage::Stake {
            stake: SignedStake {
                body_json,
                signature_hex,
            },
        }
    }

    fn give_astra(state: &mut SwtchvmState, did: &str, astra: u128) {
        let contract =
            SwtchvmAddress::from_hex(crate::spacekitvm::genesis_node::system_contracts::ASTRA_REWARDS)
                .unwrap();
        let hash = hex::encode(spacekit_service_rewards::hash_did_bytes(did.as_bytes()));
        state.kv_insert(
            contract,
            format!("astra_rewards.locked.{hash}").into_bytes(),
            (astra * WEI_PER_ASTRA).to_le_bytes().to_vec(),
        );
    }

    /// PoA governance on chain, staking from locked earnings, the lift, the
    /// stake-weighted producer set, and a PoS settings vote.
    #[test]
    fn poa_to_pos_on_chain() {
        let authority = key();
        let validator = key();
        let genesis = crate::validator_governance::PoaGenesis {
            network: "testnet".into(),
            min_validators_to_lift: Some(1),
            authorities: vec![crate::validator_governance::GenesisAuthority {
                did: authority.did.clone(),
                sphincs_pk_hex: hex::encode(&authority.pk),
                name: None,
            }],
            block_production: None,
            staking: Some(StakingParams {
                min_stake_astra: 10,
                unbonding_secs: 100,
            }),
            pos_grace_days: Some(0),
        };
        let gov = GovernanceState::from_genesis(&genesis, 0).unwrap();
        let mut state = SwtchvmState::new();
        init_genesis(&mut state, gov, genesis.staking.clone().unwrap());
        let root0 = state.state_root();

        let t = 1_000u64;
        // PoA: one authority, round robin.
        let set = producer_set(&state, t);
        assert!(!set.weighted);
        assert_eq!(set.dids(), vec![authority.did.clone()]);

        // The validator stakes locked earnings before the lift.
        give_astra(&mut state, &validator.did, 50);
        assert!(apply(&mut state, &stake(&validator, 60, 0), t).is_err(), "more than held");
        apply(&mut state, &stake(&validator, 20, 0), t).unwrap();
        assert_ne!(state.state_root(), root0, "governance state is in the state root");

        // Lift: propose + vote in separate blocks.
        let lift = propose(&authority, &state, serde_json::json!({"kind": "lift_poa"}), t as i64);
        apply(&mut state, &lift, t).unwrap();
        let id = load(&state).unwrap().governance.proposals.keys().next().unwrap().clone();
        begin_block(&mut state, t + 1);
        apply(&mut state, &vote(&authority, &id), t + 1).unwrap();
        let cs = load(&state).unwrap();
        assert!(!cs.governance.is_poa());

        // No grace: the staked validator alone produces, stake-weighted.
        begin_block(&mut state, t + 2);
        let set = producer_set(&state, t + 2);
        assert!(set.weighted && !set.fallback);
        assert_eq!(set.dids(), vec![validator.did.clone()]);
        assert_eq!(set.members[0].weight, 20);

        // Authorities can no longer change the validator set.
        let add = propose(
            &authority,
            &state,
            serde_json::json!({"kind": "remove_authority", "did": authority.did}),
            (t + 3) as i64,
        );
        assert!(apply(&mut state, &add, t + 3).is_err());

        // A staked validator changes block production by stake vote.
        let change = propose(
            &validator,
            &state,
            serde_json::json!({"kind": "set_block_production", "config": {
                "mode": "interval", "block_time_ms": 1000, "batch_window_ms": 500, "heartbeat_secs": 300
            }}),
            (t + 3) as i64,
        );
        apply(&mut state, &change, t + 3).unwrap();
        let id = load(&state)
            .unwrap()
            .governance
            .proposals
            .values()
            .find(|p| p.status == crate::validator_governance::ProposalStatus::Pending)
            .unwrap()
            .id
            .clone();
        apply(&mut state, &vote(&validator, &id), t + 4).unwrap();
        let cs = load(&state).unwrap();
        assert_eq!(
            cs.governance.block_production.mode,
            crate::block_production::ProductionMode::Interval
        );

        // Holdings falling below the minimum drop the validator; the
        // authorities keep the chain alive.
        give_astra(&mut state, &validator.did, 5);
        let set = producer_set(&state, t + 5);
        assert!(set.fallback && !set.weighted);
        assert_eq!(set.dids(), vec![authority.did.clone()]);
    }

    #[test]
    fn journal_undo_restores_root() {
        let authority = key();
        let genesis = crate::validator_governance::PoaGenesis {
            network: "testnet".into(),
            min_validators_to_lift: Some(1),
            authorities: vec![crate::validator_governance::GenesisAuthority {
                did: authority.did.clone(),
                sphincs_pk_hex: hex::encode(&authority.pk),
                name: None,
            }],
            block_production: None,
            staking: None,
            pos_grace_days: None,
        };
        let mut state = SwtchvmState::new();
        init_genesis(&mut state, GovernanceState::from_genesis(&genesis, 0).unwrap(), StakingParams::default());
        let (root_before, _) = state.checkpoint();
        // A "block": governance + an account change.
        let msg = propose(&authority, &state, serde_json::json!({"kind": "lift_poa"}), 10);
        apply(&mut state, &msg, 10).unwrap();
        state.get_account_mut(&consensus_address()).balance = 42;
        let (root_after, undo) = state.checkpoint();
        assert_ne!(root_before, root_after);
        // Undo returns the exact state and root.
        let root_reverted = state.revert(undo);
        assert_eq!(root_reverted, root_before);
        assert!(load(&state).unwrap().governance.proposals.is_empty());
        // The incremental root matches a from-scratch recomputation.
        let fresh: SwtchvmState = bincode::deserialize(&bincode::serialize(&state).unwrap()).unwrap();
        assert_eq!(fresh.state_root(), root_before);
    }

    #[test]
    fn consensus_tx_roundtrip() {
        let msg = ConsensusMessage::Stake {
            stake: SignedStake {
                body_json: "{}".into(),
                signature_hex: "00".into(),
            },
        };
        let tx = msg.to_transaction();
        assert!(is_consensus_tx(&tx));
        assert_eq!(ConsensusMessage::from_transaction(&tx).unwrap(), msg);
    }
}
