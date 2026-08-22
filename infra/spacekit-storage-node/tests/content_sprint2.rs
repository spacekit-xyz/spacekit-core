//! Sprint 2: payment verification, grants, renewal.

use spacekit_storage_node::content_entitlement::{
    build_purchase_payload, build_verify_payload, decode_entitlement_record, parse_purchase_result,
};
use spacekit_storage_node::content_grants::{ContentGrantStore, GrantKind};
use spacekit_storage_node::content_payment::{payment_scope_channel, payment_scope_content};
use spacekit_storage_node::content_payment::{
    PaymentReceiptStore, PaymentVerifyError, VerifiedPayment,
};

#[test]
fn payment_verify_success_and_duplicate() {
    let dir = tempfile::tempdir().unwrap();
    let store = PaymentReceiptStore::new(dir.path());
    let scope = payment_scope_content(&"aa".repeat(32));
    store
        .record_payment(VerifiedPayment {
            reference: "tx-ok".into(),
            payer_did: "did:spacekit:buyer".into(),
            recipient_did: "did:spacekit:publisher".into(),
            amount_astra: 10.0,
            scope: scope.clone(),
            consumed: false,
            recorded_at: 1,
        })
        .unwrap();
    let ok = store
        .verify_receipt(
            "tx-ok",
            "did:spacekit:buyer",
            "did:spacekit:publisher",
            &scope,
            10.0,
        )
        .unwrap();
    assert_eq!(ok.amount_astra, 10.0);
    store.mark_consumed("tx-ok").unwrap();
    assert!(matches!(
        store.verify_receipt(
            "tx-ok",
            "did:spacekit:buyer",
            "did:spacekit:publisher",
            &scope,
            10.0
        ),
        Err(PaymentVerifyError::DuplicateReference)
    ));
}

#[test]
fn payment_not_found_and_amount_too_small() {
    let dir = tempfile::tempdir().unwrap();
    let store = PaymentReceiptStore::new(dir.path());
    let scope = payment_scope_content("deadbeef");
    assert!(matches!(
        store.verify_receipt("missing", "did:b", "did:p", &scope, 1.0),
        Err(PaymentVerifyError::NotFound)
    ));
    store
        .record_payment(VerifiedPayment {
            reference: "tx-small".into(),
            payer_did: "did:b".into(),
            recipient_did: "did:p".into(),
            amount_astra: 5.0,
            scope,
            consumed: false,
            recorded_at: 1,
        })
        .unwrap();
    assert!(matches!(
        store.verify_receipt("tx-small", "did:b", "did:p", "content:deadbeef", 10.0),
        Err(PaymentVerifyError::AmountTooSmall { .. })
    ));
}

#[test]
fn payment_wrong_recipient() {
    let dir = tempfile::tempdir().unwrap();
    let store = PaymentReceiptStore::new(dir.path());
    let scope = payment_scope_channel("did:spacekit:channel:1");
    store
        .record_payment(VerifiedPayment {
            reference: "tx-wrong".into(),
            payer_did: "did:b".into(),
            recipient_did: "did:publisher".into(),
            amount_astra: 1.0,
            scope: scope.clone(),
            consumed: false,
            recorded_at: 1,
        })
        .unwrap();
    assert!(matches!(
        store.verify_receipt("tx-wrong", "did:b", "did:other", &scope, 1.0),
        Err(PaymentVerifyError::WrongRecipient { .. })
    ));
}

#[test]
fn refund_on_grant_failure_unmarks_consumed() {
    let dir = tempfile::tempdir().unwrap();
    let store = PaymentReceiptStore::new(dir.path());
    store
        .record_payment(VerifiedPayment {
            reference: "tx-refund".into(),
            payer_did: "did:b".into(),
            recipient_did: "did:p".into(),
            amount_astra: 1.0,
            scope: "content:abc".into(),
            consumed: false,
            recorded_at: 1,
        })
        .unwrap();
    store.mark_consumed("tx-refund").unwrap();
    store
        .refund_on_grant_failure("tx-refund", "grant write failed")
        .unwrap();
    assert!(!store.is_reference_consumed("tx-refund"));
}

#[test]
fn renew_content_before_and_after_expiration() {
    let dir = tempfile::tempdir().unwrap();
    let grants = ContentGrantStore::new(dir.path());
    let user = "did:spacekit:viewer";
    let cid = "cc".repeat(32);
    let now = chrono::Utc::now().timestamp() as u64;
    grants
        .grant_content_ppv(user, &cid, None, Some(now + 100))
        .unwrap();
    let renewed = grants
        .renew_content_ppv(user, &cid, 200, None, None)
        .unwrap();
    assert!(renewed.expires_at.unwrap() >= now + 100);
    grants
        .grant_content_ppv(user, &cid, None, Some(now.saturating_sub(10)))
        .unwrap();
    let after_exp = grants
        .renew_content_ppv(user, &cid, 50, None, None)
        .unwrap();
    assert!(after_exp.expires_at.unwrap() > now);
}

#[test]
fn renew_with_tier_change_replaces_grant() {
    let dir = tempfile::tempdir().unwrap();
    let grants = ContentGrantStore::new(dir.path());
    let user = "did:v";
    let cid = "dd".repeat(32);
    grants
        .grant_content_ppv_full(
            user,
            &cid,
            None,
            None,
            None,
            Some("basic".into()),
            Some(7),
            None,
        )
        .unwrap();
    let g = grants
        .renew_content_ppv(user, &cid, 3600, Some("premium".into()), None)
        .unwrap();
    assert_eq!(g.tier.as_deref(), Some("premium"));
}

#[test]
fn entitlement_wire_payload_length() {
    let id = [1u8; 32];
    let pk_hash = [2u8; 32];
    let payload = build_verify_payload(&id, "did:b", "file1", &pk_hash);
    assert_eq!(payload[0], 0x03);
    assert_eq!(&payload[1..33], &id);
    assert_eq!(&payload[payload.len() - 32..], &pk_hash);
}

#[test]
fn decode_entitlement_record_roundtrip() {
    let mut raw = Vec::new();
    let write_str = |buf: &mut Vec<u8>, s: &str| {
        let b = s.as_bytes();
        buf.extend_from_slice(&(b.len() as u16).to_le_bytes());
        buf.extend_from_slice(b);
    };
    write_str(&mut raw, "did:buyer");
    write_str(&mut raw, "content:abc");
    raw.extend_from_slice(&1u64.to_le_bytes());
    raw.extend_from_slice(&(u64::MAX).to_le_bytes());
    raw.push(1);
    raw.extend_from_slice(&[9u8; 32]);
    let ent = decode_entitlement_record(&raw).unwrap();
    assert_eq!(ent.buyer_did, "did:buyer");
    assert_eq!(ent.status, 1);
    assert_eq!(ent.buyer_pk_hash, [9u8; 32]);
}

#[test]
fn parse_purchase_result_extracts_entitlement_id() {
    let mut raw = vec![1u8];
    raw.extend_from_slice(&[42u8; 32]);
    let id = parse_purchase_result(&raw).unwrap();
    assert_eq!(id, [42u8; 32]);
    let pk_hash = [7u8; 32];
    assert_eq!(
        build_purchase_payload("content:abc", &pk_hash).first(),
        Some(&0x02)
    );
}

#[test]
fn settlement_pending_and_complete() {
    let dir = tempfile::tempdir().unwrap();
    let store = spacekit_storage_node::content_settlement::ContentSettlementStore::new(dir.path());
    let pending = store
        .create_pending(
            spacekit_storage_node::content_settlement::PurchaseKind::ContentPpv,
            "did:buyer",
            "did:pub",
            Some("aa".repeat(32).as_str()),
            None,
            10.0,
            None,
        )
        .unwrap();
    store
        .complete_pending_with_entitlement(&pending.id, "tx-1", &"bb".repeat(64), None, None)
        .unwrap();
    let grants = spacekit_storage_node::content_grants::ContentGrantStore::new(dir.path());
    assert!(grants.has_content_grant("did:buyer", &"aa".repeat(32)));
}

#[test]
fn channel_subscription_pending_and_complete() {
    let dir = tempfile::tempdir().unwrap();
    let store = spacekit_storage_node::content_settlement::ContentSettlementStore::new(dir.path());
    let ch = "did:spacekit:channel:test:abc12345";
    let pending = store
        .create_pending(
            spacekit_storage_node::content_settlement::PurchaseKind::ChannelSubscription,
            "did:buyer",
            "did:pub",
            None,
            Some(ch),
            5.0,
            None,
        )
        .unwrap();
    store
        .complete_pending_with_entitlement(
            &pending.id,
            "tx-ch",
            &"cc".repeat(64),
            Some(86400),
            None,
        )
        .unwrap();
    let grants = spacekit_storage_node::content_grants::ContentGrantStore::new(dir.path());
    assert!(grants.has_channel_subscription("did:buyer", ch));
}

#[test]
fn find_open_pending_for_scope() {
    let dir = tempfile::tempdir().unwrap();
    let store = spacekit_storage_node::content_settlement::ContentSettlementStore::new(dir.path());
    let scope = format!("content:{}", "dd".repeat(32));
    let p = store
        .create_pending(
            spacekit_storage_node::content_settlement::PurchaseKind::ContentPpv,
            "did:buyer",
            "did:pub",
            Some(&"dd".repeat(32)),
            None,
            1.0,
            None,
        )
        .unwrap();
    let found = store
        .find_open_pending_for_scope("did:buyer", &scope)
        .unwrap()
        .unwrap();
    assert_eq!(found.id, p.id);
}

#[test]
#[test]
fn store_retrieve_roundtrips_ppv_access_policy() {
    use spacekit_primitives::v1::fact::{
        AccessCondition, AccessPolicy, CollectionMethod, ConditionType, DataSource, FactCategory,
        FactContent, FactMetadata, FactPackage, KnowledgeDomain, LicenseType, VerificationLevel,
    };
    use spacekit_primitives::v1::identity::QuantumDID;
    use spacekit_storage_node::fact_storage::{
        CompressionAlgorithm, FactStorageConfig, FactStorageEngine, StorageTierConfig,
    };
    use std::collections::HashMap;

    let dir = tempfile::tempdir().unwrap();
    let db = std::sync::Arc::new(
        spacekit_storage_node::database::Database::new(dir.path().join("db").to_str().unwrap())
            .unwrap(),
    );
    db.initialize().unwrap();
    let qc = std::sync::Arc::new(spacekit_storage_node::quantum::QuantumCrypto::default());
    let cfg = FactStorageConfig {
        storage_dir: dir.path().join("facts"),
        max_fact_size: 10_000_000,
        enable_compression: false,
        compression_algorithm: CompressionAlgorithm::None,
        enable_deduplication: false,
        verification_cache_size: 100,
        enable_auto_indexing: false,
        storage_tiers: StorageTierConfig {
            hot_storage_dir: dir.path().join("hot"),
            cold_storage_dir: dir.path().join("cold"),
            archive_threshold_days: 30,
            max_hot_storage_bytes: 10_000_000,
        },
    };
    let rt = tokio::runtime::Runtime::new().unwrap();
    let engine = rt.block_on(FactStorageEngine::new(db, qc, cfg)).unwrap();
    let author = QuantumDID::parse("did:spacekit:pub").unwrap();
    let mut params = HashMap::new();
    params.insert("price".into(), "10".into());
    params.insert("currency".into(), "ASTRA".into());
    let fact = FactPackage {
        fact_id: [9u8; 32],
        version: 1,
        created_at: 1,
        expires_at: None,
        content: FactContent::Binary {
            data: b"paid".to_vec(),
            mime_type: "text/plain".into(),
            hash: [2u8; 32],
        },
        metadata: FactMetadata {
            category: FactCategory::UserGenerated,
            tags: vec!["pricing:pay_per_view".into(), "price:10".into()],
            domain: KnowledgeDomain::Custom("test".into()),
            source: DataSource::UserInput {
                application: author.clone(),
                user: author.clone(),
            },
            collection_method: CollectionMethod::Manual,
            verification_level: VerificationLevel::SelfClaimed,
            license: LicenseType::Proprietary,
            size_bytes: 4,
            checksum: [2u8; 32],
        },
        author,
        signature: spacekit_primitives::v1::crypto::quantum::SPHINCSSignature::new(
            vec![],
            "sphincs".into(),
            vec![],
        ),
        verification_proof: spacekit_primitives::v1::fact::VerificationProof {
            proof_type: spacekit_primitives::v1::fact::ProofType::QuantumSignature,
            proof_data: vec![],
            verification_timestamp: 0,
            verifier: None,
        },
        dependencies: vec![],
        citations: vec![],
        confidence_score: 1.0,
        access_policy: AccessPolicy::Conditional(vec![AccessCondition {
            condition_type: ConditionType::PaymentRequired,
            parameters: params,
        }]),
        encryption: None,
    };
    let id = fact.fact_id;
    rt.block_on(engine.store_fact(fact)).unwrap();
    let loaded = rt.block_on(engine.retrieve_fact(id)).unwrap().unwrap();
    match &loaded.access_policy {
        spacekit_primitives::v1::fact::AccessPolicy::Conditional(conds) => {
            let pay = conds
                .iter()
                .find(|c| c.condition_type == ConditionType::PaymentRequired)
                .unwrap();
            assert_eq!(pay.parameters.get("price").map(|s| s.as_str()), Some("10"));
        }
        other => panic!("expected Conditional PPV policy, got {other:?}"),
    }
}

#[test]
fn escrow_opcode_payloads() {
    use spacekit_storage_node::content_escrow::{
        build_create_escrow_payload, build_refund_payload, build_release_payload,
        escrow_id_for_pending, OP_CREATE, OP_REFUND, OP_RELEASE,
    };
    let id = escrow_id_for_pending("pending-abc");
    assert_eq!(id, "content-pending:pending-abc");
    let create = build_create_escrow_payload(
        &id,
        "ASTRA",
        "did:buyer",
        "did:pub",
        10_000_000,
        "did:arbiter",
    );
    assert_eq!(create[0], OP_CREATE);
    let release = build_release_payload(&id);
    assert_eq!(release[0], OP_RELEASE);
    let refund = build_refund_payload(&id);
    assert_eq!(refund[0], OP_REFUND);
}

#[test]
fn license_opcode_payloads() {
    use spacekit_storage_node::content_license::{
        build_has_license_payload, build_mint_payload, OP_HAS_LICENSE, OP_MINT,
    };
    let mint = build_mint_payload("did:buyer", &"aa".repeat(32), 1_000_000);
    assert_eq!(mint[0], OP_MINT);
    let has = build_has_license_payload("did:buyer", &"aa".repeat(32));
    assert_eq!(has[0], OP_HAS_LICENSE);
}

#[test]
fn local_legacy_grant_still_works() {
    let dir = tempfile::tempdir().unwrap();
    let grants = ContentGrantStore::new(dir.path());
    let user = "did:legacy";
    let cid = "ee".repeat(32);
    grants.grant_content_ppv(user, &cid, None, None).unwrap();
    assert!(grants.has_content_grant(user, &cid));
    let listed = grants.list_for_requester(user).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].kind, GrantKind::ContentPpv);
}
