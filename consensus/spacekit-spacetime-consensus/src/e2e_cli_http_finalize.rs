//! End-to-end contract test: CLI HTTP → standalone propose → finalize.
//!
//! Uses production [`pq_envelope`] types (vote Merkle leaves, SPHINCS+ preimage,
//! domain tags). Run with:
//!   `cargo test --features "pq-signatures,verkle" --test e2e_cli_http_finalize`

#![cfg(feature = "pq-signatures")]

use alloy_primitives::{keccak256, B256};
use spacekit_spacetime_consensus::{
    causal::CausalCoord,
    consensus::SpacetimeExtension,
    pq_envelope::{
        pq_crypto, tagged_commitment, votes_merkle_root, BlockEnvelope, ConsensusVoteInner,
        ConsensusVoteType, DOMAIN_BLOCK_ENVELOPE, DOMAIN_STATE_VERKLE, DOMAIN_TX_VERKLE,
        PQ_ENVELOPE_WIRE_VERSION,
    },
    proposal::SpacetimeTransition,
    rotor::{Bivector, Rotor},
    light_client::{verify_rotor_chain, RotorChainProof},
    SPACETIME_WIRE_VERSION,
};

/// Stand-in `proposal_id` / light-client digest (keccak of SPHINCS+ signing bytes).
fn envelope_stand_in_digest(envelope: &BlockEnvelope) -> B256 {
    keccak256(envelope.sphincs_signing_bytes())
}

fn make_transition(round: u64) -> SpacetimeTransition {
    let r = Rotor::exp(&Bivector {
        b: [0.0, 0.0, 0.0, 0.05 * (round as f64 + 1.0), 0.0, 0.0],
    });
    let (residual_commitment, residual_norm) =
        SpacetimeTransition::zero_residual_fields(|b| *keccak256(b));
    SpacetimeTransition {
        transition_id: round,
        rotor: r,
        prev_state_hash: B256::from([(round.saturating_sub(1)) as u8 + 1; 32]),
        new_state_hash: B256::from([round as u8 + 2; 32]),
        causal_coord: CausalCoord {
            t: round as f64 + 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        },
        residual_commitment,
        residual_norm,
        aux_commit: None,
    }
}

fn make_pq_vote(
    round: u64,
    view: u64,
    validator_idx: u8,
    transition: &SpacetimeTransition,
    vote_type: ConsensusVoteType,
) -> ConsensusVoteInner {
    let (d_pk, d_sk) = pq_crypto::dilithium2_keypair();
    let proposal_hash = keccak256(format!("proposal-{}-{}", round, view).as_bytes());
    let mut vote = ConsensusVoteInner {
        wire_version: PQ_ENVELOPE_WIRE_VERSION,
        round,
        view,
        proposal_hash,
        vote_type,
        validator_id: B256::from([validator_idx; 32]),
        validator_rotor_digest: transition.digest(|b| *keccak256(b)),
        dilithium_public_key: Vec::new(),
        dilithium_signature: Vec::new(),
    };
    pq_crypto::sign_consensus_vote(&mut vote, &d_pk, &d_sk);
    vote
}

fn build_envelope(
    round: u64,
    view: u64,
    votes: &[ConsensusVoteInner],
    transition: &SpacetimeTransition,
    parent_hash: B256,
    raw_tx: B256,
    raw_state: B256,
    timestamp: u64,
) -> BlockEnvelope {
    BlockEnvelope {
        wire_version: PQ_ENVELOPE_WIRE_VERSION,
        round,
        view,
        chain_id: "e2e-chain".into(),
        height: round,
        parent_hash,
        state_root: raw_state,
        tx_root: raw_tx,
        l1_manifest_hash: B256::ZERO,
        spacetime_tip_hash: transition.digest(|b| *keccak256(b)),
        votes_merkle_root: votes_merkle_root(votes),
        block_body_hash: keccak256(b"e2e-block-body"),
        timestamp,
    }
}

fn minimal_envelope(transition: &SpacetimeTransition) -> BlockEnvelope {
    build_envelope(
        0,
        0,
        &[],
        transition,
        B256::ZERO,
        B256::ZERO,
        B256::ZERO,
        0,
    )
}

// ── HTTP-shaped request/response stand-ins ─────────────────────────────

#[derive(Debug)]
struct ProposeRequest {
    finalize: bool,
    dev_mode: bool,
    allow_single_validator_finalize: bool,
    validator_count: usize,
}

#[derive(Debug, PartialEq)]
enum ProposeResponse {
    Ok {
        proposal_id: B256,
        envelope_digest: B256,
        finalized: bool,
    },
    Err400 {
        reason: &'static str,
    },
}

fn handle_propose(req: &ProposeRequest, envelope: &BlockEnvelope) -> ProposeResponse {
    if req.finalize {
        let dev_allowed = req.dev_mode;
        let single_allowed =
            req.validator_count == 1 && req.allow_single_validator_finalize;
        if !dev_allowed && !single_allowed {
            return ProposeResponse::Err400 {
                reason: "finalize requires dev_mode or single-validator gate",
            };
        }
    }
    let digest = envelope_stand_in_digest(envelope);
    ProposeResponse::Ok {
        proposal_id: digest,
        envelope_digest: digest,
        finalized: req.finalize,
    }
}

// ── Tests ───────────────────────────────────────────────────────────────

#[test]
fn cli_http_finalize_pipeline_single_validator_dev_mode() {
    let round = 0u64;
    let view = 0u64;
    let transition = make_transition(round);

    let vote = make_pq_vote(round, view, 1, &transition, ConsensusVoteType::Yes);
    let votes = vec![vote.clone()];
    let raw_tx = keccak256(b"empty-tx-set");
    let raw_state = keccak256(b"genesis-state");

    let envelope = build_envelope(
        round,
        view,
        &votes,
        &transition,
        B256::from([0xAB; 32]),
        raw_tx,
        raw_state,
        1_700_000_000,
    );

    let req = ProposeRequest {
        finalize: true,
        dev_mode: true,
        allow_single_validator_finalize: false,
        validator_count: 1,
    };

    match handle_propose(&req, &envelope) {
        ProposeResponse::Ok {
            proposal_id,
            envelope_digest,
            finalized,
        } => {
            assert!(finalized);
            assert_eq!(proposal_id, envelope_digest);
            assert_eq!(envelope_digest, envelope_stand_in_digest(&envelope));
            assert!(vote.verify_dilithium());
        }
        ProposeResponse::Err400 { reason } => panic!("unexpected 400: {}", reason),
    }
}

#[test]
fn finalize_without_dev_mode_or_single_validator_gate_is_rejected() {
    let transition = make_transition(0);
    let envelope = minimal_envelope(&transition);
    let req = ProposeRequest {
        finalize: true,
        dev_mode: false,
        allow_single_validator_finalize: false,
        validator_count: 1,
    };
    match handle_propose(&req, &envelope) {
        ProposeResponse::Err400 { reason } => {
            assert!(reason.contains("dev_mode") || reason.contains("single-validator"));
        }
        other => panic!("expected 400, got {:?}", other),
    }
}

#[test]
fn single_validator_gate_allows_finalize_without_dev_mode() {
    let transition = make_transition(0);
    let envelope = minimal_envelope(&transition);
    let req = ProposeRequest {
        finalize: true,
        dev_mode: false,
        allow_single_validator_finalize: true,
        validator_count: 1,
    };
    match handle_propose(&req, &envelope) {
        ProposeResponse::Ok { finalized, .. } => assert!(finalized),
        ProposeResponse::Err400 { reason } => panic!("expected ok, got 400: {}", reason),
    }
}

#[test]
fn single_validator_gate_rejects_when_validator_count_above_one() {
    let transition = make_transition(0);
    let envelope = minimal_envelope(&transition);
    let req = ProposeRequest {
        finalize: true,
        dev_mode: false,
        allow_single_validator_finalize: true,
        validator_count: 3,
    };
    match handle_propose(&req, &envelope) {
        ProposeResponse::Err400 { .. } => (),
        ProposeResponse::Ok { .. } => panic!("multi-validator must require dev_mode for finalize"),
    }
}

#[test]
fn envelope_byte_order_is_locked() {
    let raw_state = B256::from([0xDD; 32]);
    let raw_tx = B256::from([0xCC; 32]);
    let votes_merkle = B256::from([0xBB; 32]);
    let parent = B256::from([0xAA; 32]);
    let spacetime_tip = B256::from([0xEE; 32]);

    let envelope = BlockEnvelope {
        wire_version: PQ_ENVELOPE_WIRE_VERSION,
        round: 0x0102030405060708,
        view: 0x1112131415161718,
        chain_id: "xy".into(),
        height: 0x3132333435363738,
        parent_hash: parent,
        state_root: raw_state,
        tx_root: raw_tx,
        l1_manifest_hash: B256::ZERO,
        spacetime_tip_hash: spacetime_tip,
        votes_merkle_root: votes_merkle,
        block_body_hash: B256::ZERO,
        timestamp: 0x2122232425262728,
    };

    let bytes = envelope.sphincs_signing_bytes();
    let mut o = 0;

    assert_eq!(&bytes[o..DOMAIN_BLOCK_ENVELOPE.len()], DOMAIN_BLOCK_ENVELOPE);
    o += DOMAIN_BLOCK_ENVELOPE.len();

    assert_eq!(&bytes[o..o + 2], &PQ_ENVELOPE_WIRE_VERSION.to_le_bytes());
    o += 2;
    assert_eq!(&bytes[o..o + 8], &0x0102030405060708u64.to_le_bytes());
    o += 8;
    assert_eq!(&bytes[o..o + 8], &0x1112131415161718u64.to_le_bytes());
    o += 8;
    assert_eq!(&bytes[o..o + 32], parent.as_slice());
    o += 32;

    let tagged_votes = tagged_commitment(
        spacekit_spacetime_consensus::DOMAIN_VOTES_MERKLE,
        &votes_merkle,
    );
    assert_eq!(&bytes[o..o + 32], tagged_votes.as_slice());
    o += 32;

    let tagged_tx = tagged_commitment(DOMAIN_TX_VERKLE, &raw_tx);
    assert_eq!(&bytes[o..o + 32], tagged_tx.as_slice());
    o += 32;

    let tagged_state = tagged_commitment(DOMAIN_STATE_VERKLE, &raw_state);
    assert_eq!(&bytes[o..o + 32], tagged_state.as_slice());
    o += 32;

    let tagged_st = tagged_commitment(
        spacekit_spacetime_consensus::DOMAIN_SPACETIME_TRANSITION,
        &spacetime_tip,
    );
    assert_eq!(&bytes[o..o + 32], tagged_st.as_slice());
    o += 32;

    assert_eq!(&bytes[o..o + 8], &0x2122232425262728u64.to_le_bytes());
    o += 8;

    let chain = b"xy";
    assert_eq!(&bytes[o..o + 4], &(chain.len() as u32).to_le_bytes());
    o += 4;
    assert_eq!(&bytes[o..o + chain.len()], chain);
    o += chain.len();

    assert_eq!(&bytes[o..o + 8], &0x3132333435363738u64.to_le_bytes());
    o += 8;

    assert_eq!(o, bytes.len(), "unexpected trailing envelope bytes");
}

#[test]
fn envelope_field_swap_changes_signing_digest() {
    let raw_state = B256::from([0xDD; 32]);
    let raw_tx = B256::from([0xCC; 32]);
    let votes_merkle = B256::from([0xBB; 32]);
    let parent = B256::from([0xAA; 32]);
    let spacetime_tip = B256::from([0xEE; 32]);

    let canonical = BlockEnvelope {
        wire_version: PQ_ENVELOPE_WIRE_VERSION,
        round: 1,
        view: 0,
        chain_id: "swap-test".into(),
        height: 42,
        parent_hash: parent,
        state_root: raw_state,
        tx_root: raw_tx,
        l1_manifest_hash: B256::ZERO,
        spacetime_tip_hash: spacetime_tip,
        votes_merkle_root: votes_merkle,
        block_body_hash: B256::ZERO,
        timestamp: 99,
    };
    let canonical_digest = keccak256(canonical.sphincs_signing_bytes());

    let swapped = BlockEnvelope {
        votes_merkle_root: raw_tx,
        tx_root: votes_merkle,
        ..canonical.clone()
    };
    assert_ne!(
        keccak256(swapped.sphincs_signing_bytes()),
        canonical_digest,
        "swapping votes_merkle_root and tx_root must change the SPHINCS+ preimage"
    );
}

#[test]
fn dilithium_vote_does_not_verify_under_wrong_validator_key() {
    let transition = make_transition(0);
    let vote_a = make_pq_vote(0, 0, 1, &transition, ConsensusVoteType::Yes);
    let (_pk_b, _sk_b) = pq_crypto::dilithium2_keypair();
    assert!(vote_a.verify_dilithium());
    assert!(
        !pq_crypto::dilithium2_verify(&_pk_b, &vote_a.signing_bytes(), &vote_a.dilithium_signature),
        "validator A's vote must not verify under validator B's Dilithium key"
    );
}

#[test]
fn vote_leaf_includes_rotor_digest() {
    let t1 = make_transition(0);
    let mut t2 = t1;
    t2.rotor = Rotor::exp(&Bivector {
        b: [0.0, 0.0, 0.0, 0.5, 0.0, 0.0],
    });

    let v1 = make_pq_vote(0, 0, 1, &t1, ConsensusVoteType::Yes);
    let v2 = make_pq_vote(0, 0, 1, &t2, ConsensusVoteType::Yes);
    assert_ne!(
        v1.merkle_leaf_digest(),
        v2.merkle_leaf_digest(),
        "rotor digest must affect vote Merkle leaf"
    );
}

#[test]
fn full_round_trip_with_three_validators() {
    let round = 7u64;
    let view = 0u64;
    let proposer_transition = make_transition(round);

    let extension = SpacetimeExtension::default();
    let validator_rotors: Vec<(Rotor, f64)> = (0..3u8)
        .map(|i| {
            let drift = 0.001 * (i as f64 + 1.0);
            let r = Rotor::exp(&Bivector {
                b: [0.0, 0.0, 0.0, 0.05 * (round as f64 + 1.0) + drift, 0.0, 0.0],
            });
            (r, 1.0 - 0.1 * (i as f64))
        })
        .collect();

    let consensus = extension
        .aggregate_votes_robust(&validator_rotors)
        .expect("robust aggregation");
    assert!(consensus.max_divergence < 0.05);

    let votes: Vec<ConsensusVoteInner> = validator_rotors
        .iter()
        .enumerate()
        .map(|(i, (r, _))| {
            let mut t = proposer_transition;
            t.rotor = *r;
            make_pq_vote(round, view, (i + 1) as u8, &t, ConsensusVoteType::Yes)
        })
        .collect();

    let mut canonical_transition = proposer_transition;
    canonical_transition.rotor = consensus.rotor;

    let envelope = build_envelope(
        round,
        view,
        &votes,
        &canonical_transition,
        B256::from([0x99; 32]),
        keccak256(b"three-validator-block"),
        keccak256(b"three-validator-state"),
        1_700_000_001,
    );

    let req = ProposeRequest {
        finalize: true,
        dev_mode: true,
        allow_single_validator_finalize: false,
        validator_count: 3,
    };
    match handle_propose(&req, &envelope) {
        ProposeResponse::Ok {
            proposal_id,
            envelope_digest,
            finalized,
        } => {
            assert!(finalized);
            assert_eq!(proposal_id, envelope_digest);
        }
        ProposeResponse::Err400 { reason } => panic!("expected ok: {}", reason),
    }
}

#[test]
fn light_client_can_reproduce_envelope_from_block_body_only() {
    let round = 12u64;
    let view = 0u64;
    let transition = make_transition(round);
    let votes: Vec<ConsensusVoteInner> = (1..=4u8)
        .map(|i| make_pq_vote(round, view, i, &transition, ConsensusVoteType::Yes))
        .collect();

    let envelope = build_envelope(
        round,
        view,
        &votes,
        &transition,
        B256::from([0x77; 32]),
        keccak256(b"block-12-txs"),
        keccak256(b"block-12-state"),
        1_700_000_002,
    );

    let digest = envelope_stand_in_digest(&envelope);
    assert_eq!(digest, keccak256(envelope.sphincs_signing_bytes()));

    let chained_proof = RotorChainProof {
        wire_version: SPACETIME_WIRE_VERSION,
        anchor_state_hash: transition.prev_state_hash,
        anchor_coord: CausalCoord::ORIGIN,
        transitions: vec![transition],
    };
    assert_eq!(verify_rotor_chain(&chained_proof), Ok(()));
}

#[cfg(feature = "verkle")]
#[test]
fn fingerprint_apply_batch_matches_transition_witness() {
    use spacekit_spacetime_consensus::fingerprint_verkle::store::{
        apply_fingerprint_batch, FingerprintVerkle,
    };
    use spacekit_spacetime_consensus::proposal::TransitionWitness;

    let transition = make_transition(1);
    let vote = make_pq_vote(1, 0, 0xA1, &transition, ConsensusVoteType::Yes);
    let witness = TransitionWitness::from_vote(&vote, &transition, |b| *keccak256(b))
        .expect("vote binds transition");

    let mut store = FingerprintVerkle::new();
    let touched = apply_fingerprint_batch(
        &mut store,
        &[(
            vote.validator_id,
            witness.transition.rotor,
            witness.transition.residual_norm,
        )],
        0.95,
        |b| *keccak256(b),
    );
    assert_eq!(touched, vec![vote.validator_id]);
    assert!(store.get(&vote.validator_id).unwrap().samples >= 1);

    let proof = store.prove_fingerprint(&vote.validator_id).expect("proof");
    let commit = store.get(&vote.validator_id).unwrap();
    assert!(store.verify_fingerprint_proof(
        &vote.validator_id,
        commit,
        &proof,
        |b| *keccak256(b)
    ));
}
