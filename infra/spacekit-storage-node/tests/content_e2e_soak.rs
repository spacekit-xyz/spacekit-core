//! E2E soak: content monetization happy paths + critical error paths (in-process).
//!
//! Run: `cargo test --test content_e2e_soak`
//!
//! Live CLI soak: see `documentation/guides/content-monetization-soak.md` and
//! `scripts/content-monetization-soak.sh`.

use spacekit_primitives::v1::crypto::quantum::SPHINCSSignature;
use spacekit_primitives::v1::fact::{
    AccessCondition, AccessPolicy, CollectionMethod, ConditionType, DataSource, FactCategory,
    FactContent, FactMetadata, FactPackage, KnowledgeDomain, LicenseType, ProofType,
    VerificationLevel, VerificationProof,
};
use spacekit_primitives::v1::identity::QuantumDID;
use spacekit_storage_node::content_access::{
    channel_did_from_fact, evaluate_content_access, ContentAccessDecision,
};
use spacekit_storage_node::content_grants::ContentGrantStore;
use spacekit_storage_node::content_payment::{
    payment_scope_channel, payment_scope_content, PaymentReceiptStore, PaymentVerifyError,
    VerifiedPayment,
};
use spacekit_storage_node::content_settlement::{
    validate_settlement_for_pending, ContentSettlementStore, PurchaseKind, SettlementReceipt,
};
use std::collections::HashMap;

fn ppv_fact(
    author: &QuantumDID,
    fact_id: [u8; 32],
    price: &str,
    channel_tag: Option<&str>,
) -> FactPackage {
    let mut params = HashMap::new();
    params.insert("price".into(), price.into());
    params.insert("currency".into(), "ASTRA".into());
    params.insert("content_id".into(), hex::encode(fact_id));
    let mut tags = vec!["content".into()];
    if let Some(ch) = channel_tag {
        tags.push(format!("channel:{ch}"));
    }
    FactPackage {
        fact_id,
        version: 1,
        created_at: 1,
        expires_at: None,
        content: FactContent::Binary {
            data: b"soak-payload".to_vec(),
            mime_type: "text/plain".into(),
            hash: [0u8; 32],
        },
        metadata: FactMetadata {
            category: FactCategory::UserGenerated,
            tags,
            domain: KnowledgeDomain::Custom("soak".into()),
            source: DataSource::UserInput {
                application: author.clone(),
                user: author.clone(),
            },
            collection_method: CollectionMethod::Manual,
            verification_level: VerificationLevel::SelfClaimed,
            license: LicenseType::Proprietary,
            size_bytes: 12,
            checksum: [0u8; 32],
        },
        author: author.clone(),
        signature: SPHINCSSignature::new(vec![0u8; 8], "sphincs-128s".into(), vec![0u8; 8]),
        verification_proof: VerificationProof {
            proof_type: ProofType::QuantumSignature,
            proof_data: vec![],
            verification_timestamp: 1,
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
    }
}

fn free_fact(author: &QuantumDID, fact_id: [u8; 32]) -> FactPackage {
    let mut f = ppv_fact(author, fact_id, "0", None);
    f.access_policy = AccessPolicy::Public;
    f
}

// --- Happy path ---

#[test]
fn soak_free_content_view_allowed() {
    let dir = tempfile::tempdir().unwrap();
    let grants = ContentGrantStore::new(dir.path());
    let author = QuantumDID::parse("did:spacekit:publisher").unwrap();
    let fact = free_fact(&author, [1u8; 32]);
    assert!(matches!(
        evaluate_content_access(&fact, "did:spacekit:viewer", &grants).unwrap(),
        ContentAccessDecision::Allowed
    ));
}

#[test]
fn soak_publisher_views_own_paid_content_without_grant() {
    let dir = tempfile::tempdir().unwrap();
    let grants = ContentGrantStore::new(dir.path());
    let author = QuantumDID::parse("did:spacekit:publisher").unwrap();
    let fact = ppv_fact(&author, [2u8; 32], "10", None);
    assert!(matches!(
        evaluate_content_access(&fact, author.as_str(), &grants).unwrap(),
        ContentAccessDecision::Allowed
    ));
}

#[test]
fn soak_ppv_pay_settle_view_chain() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path();
    let grants = ContentGrantStore::new(data);
    let settlements = ContentSettlementStore::new(data);
    let payments = PaymentReceiptStore::new(data);

    let publisher = "did:spacekit:publisher";
    let buyer = "did:spacekit:buyer";
    let cid = "aa".repeat(32);
    let scope = payment_scope_content(&cid);

    let author = QuantumDID::parse(publisher).unwrap();
    let fact = ppv_fact(
        &author,
        hex::decode(&cid).unwrap().try_into().unwrap(),
        "10",
        None,
    );
    assert!(matches!(
        evaluate_content_access(&fact, buyer, &grants).unwrap(),
        ContentAccessDecision::PaymentRequired { .. }
    ));

    let pending = settlements
        .create_pending(
            PurchaseKind::ContentPpv,
            buyer,
            publisher,
            Some(&cid),
            None,
            10.0,
            None,
        )
        .unwrap();

    let receipt = SettlementReceipt {
        tx_hash: "tx-soak-ppv".into(),
        amount: "10".to_string(),
        asset: "ASTRA".into(),
        payer_did: buyer.into(),
        beneficiary_did: publisher.into(),
        scope: scope.clone(),
        settled_at: 100,
    };
    validate_settlement_for_pending(&pending, &receipt).unwrap();
    settlements
        .apply_settlement_to_pending(&pending.id, &receipt)
        .unwrap();

    let ent = "bb".repeat(64);
    settlements
        .complete_pending_with_entitlement(&pending.id, &receipt.tx_hash, &ent, None, None)
        .unwrap();

    assert!(grants.has_content_grant(buyer, &cid));
    assert!(payments.is_reference_consumed(&receipt.tx_hash));
    assert!(matches!(
        evaluate_content_access(&fact, buyer, &grants).unwrap(),
        ContentAccessDecision::Allowed
    ));
}

#[test]
fn soak_channel_subscribe_then_view() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path();
    let grants = ContentGrantStore::new(data);
    let settlements = ContentSettlementStore::new(data);

    let publisher = "did:spacekit:pub";
    let buyer = "did:spacekit:buyer";
    let channel = "did:spacekit:channel:soak:pub";

    let author = QuantumDID::parse(publisher).unwrap();
    let fact_id = [9u8; 32];
    let fact = ppv_fact(&author, fact_id, "0", Some(channel));
    assert_eq!(channel_did_from_fact(&fact).as_deref(), Some(channel));

    let pending = settlements
        .create_pending(
            PurchaseKind::ChannelSubscription,
            buyer,
            publisher,
            None,
            Some(channel),
            25.0,
            None,
        )
        .unwrap();
    settlements
        .complete_pending_with_entitlement(
            &pending.id,
            "tx-ch",
            &"cc".repeat(64),
            Some(86400),
            None,
        )
        .unwrap();

    assert!(grants.has_channel_subscription(buyer, channel));
    assert!(matches!(
        evaluate_content_access(&fact, buyer, &grants).unwrap(),
        ContentAccessDecision::Allowed
    ));
}

#[test]
fn soak_renew_before_and_after_expiration() {
    let dir = tempfile::tempdir().unwrap();
    let grants = ContentGrantStore::new(dir.path());
    let user = "did:spacekit:viewer";
    let cid = "ee".repeat(32);
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
    let after = grants
        .renew_content_ppv(user, &cid, 50, None, None)
        .unwrap();
    assert!(after.expires_at.unwrap() > now);
}

#[test]
fn soak_tier_change_on_renewal() {
    let dir = tempfile::tempdir().unwrap();
    let grants = ContentGrantStore::new(dir.path());
    let user = "did:v";
    let cid = "ff".repeat(32);
    grants
        .grant_content_ppv_full(
            user,
            &cid,
            None,
            None,
            None,
            Some("basic".into()),
            None,
            None,
        )
        .unwrap();
    let g = grants
        .renew_content_ppv(user, &cid, 3600, Some("premium".into()), None)
        .unwrap();
    assert_eq!(g.tier.as_deref(), Some("premium"));
}

// --- Critical error paths ---

#[test]
fn soak_payment_not_found_and_wrong_recipient() {
    let dir = tempfile::tempdir().unwrap();
    let store = PaymentReceiptStore::new(dir.path());
    let scope = payment_scope_content(&"11".repeat(32));
    assert!(matches!(
        store.verify_receipt("missing", "did:b", "did:p", &scope, 10.0),
        Err(PaymentVerifyError::NotFound)
    ));
    store
        .record_payment(VerifiedPayment {
            reference: "tx-w".into(),
            payer_did: "did:b".into(),
            recipient_did: "did:p".into(),
            amount_astra: 10.0,
            scope: scope.clone(),
            consumed: false,
            recorded_at: 1,
        })
        .unwrap();
    assert!(matches!(
        store.verify_receipt("tx-w", "did:b", "did:other", &scope, 10.0),
        Err(PaymentVerifyError::WrongRecipient { .. })
    ));
}

#[test]
fn soak_settlement_wrong_amount_and_recipient() {
    let dir = tempfile::tempdir().unwrap();
    let store = ContentSettlementStore::new(dir.path());
    let pending = store
        .create_pending(
            PurchaseKind::ContentPpv,
            "did:b",
            "did:pub",
            Some(&"22".repeat(32)),
            None,
            10.0,
            None,
        )
        .unwrap();
    let bad_amt = SettlementReceipt {
        tx_hash: "tx1".into(),
        amount: "5".into(),
        asset: "ASTRA".into(),
        payer_did: "did:b".into(),
        beneficiary_did: "did:pub".into(),
        scope: pending.scope.clone(),
        settled_at: 1,
    };
    assert!(validate_settlement_for_pending(&pending, &bad_amt).is_err());

    let bad_recv = SettlementReceipt {
        beneficiary_did: "did:other".into(),
        amount: "10".into(),
        ..bad_amt
    };
    assert!(validate_settlement_for_pending(&pending, &bad_recv).is_err());
}

#[test]
fn soak_duplicate_payment_reference_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let store = PaymentReceiptStore::new(dir.path());
    let scope = payment_scope_channel("did:spacekit:channel:x");
    store
        .record_payment(VerifiedPayment {
            reference: "tx-dup".into(),
            payer_did: "did:b".into(),
            recipient_did: "did:p".into(),
            amount_astra: 1.0,
            scope,
            consumed: false,
            recorded_at: 1,
        })
        .unwrap();
    store.mark_consumed("tx-dup").unwrap();
    let scope = payment_scope_channel("did:spacekit:channel:x");
    assert!(matches!(
        store.verify_receipt("tx-dup", "did:b", "did:p", &scope, 1.0),
        Err(PaymentVerifyError::DuplicateReference)
    ));
}

#[test]
fn soak_idempotent_complete_pending() {
    let dir = tempfile::tempdir().unwrap();
    let store = ContentSettlementStore::new(dir.path());
    let pending = store
        .create_pending(
            PurchaseKind::ContentPpv,
            "did:b",
            "did:p",
            Some(&"33".repeat(32)),
            None,
            1.0,
            None,
        )
        .unwrap();
    let ent = "dd".repeat(64);
    store
        .complete_pending_with_entitlement(&pending.id, "tx-idem", &ent, None, None)
        .unwrap();
    store
        .complete_pending_with_entitlement(&pending.id, "tx-idem", &ent, None, None)
        .unwrap();
    let p = store.get_pending(&pending.id).unwrap().unwrap();
    assert_eq!(p.status, "completed");
    assert_eq!(p.entitlement_id_hex.as_deref(), Some(ent.as_str()));
}

#[test]
fn soak_double_pay_different_refs_single_grant_path() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path();
    let grants = ContentGrantStore::new(data);
    let settlements = ContentSettlementStore::new(data);
    let payments = PaymentReceiptStore::new(data);
    let cid = "44".repeat(32);
    let scope = payment_scope_content(&cid);
    let pending = settlements
        .create_pending(
            PurchaseKind::ContentPpv,
            "did:b",
            "did:p",
            Some(&cid),
            None,
            10.0,
            None,
        )
        .unwrap();
    for (i, tx) in ["tx-a", "tx-b"].iter().enumerate() {
        payments
            .record_payment(VerifiedPayment {
                reference: (*tx).into(),
                payer_did: "did:b".into(),
                recipient_did: "did:p".into(),
                amount_astra: 10.0,
                scope: scope.clone(),
                consumed: false,
                recorded_at: i as u64,
            })
            .unwrap();
    }
    settlements
        .complete_pending_with_entitlement(&pending.id, "tx-a", &"ee".repeat(64), None, None)
        .unwrap();
    assert!(grants.has_content_grant("did:b", &cid));
    assert!(payments.is_reference_consumed("tx-a"));
    assert!(!payments.is_reference_consumed("tx-b"));
}

#[test]
fn soak_listener_marks_inbox_processed() {
    let dir = tempfile::tempdir().unwrap();
    let store = ContentSettlementStore::new(dir.path());
    let cid = "66".repeat(32);
    let scope = payment_scope_content(&cid);
    let pending = store
        .create_pending(
            PurchaseKind::ContentPpv,
            "did:b",
            "did:p",
            Some(&cid),
            None,
            3.0,
            None,
        )
        .unwrap();
    let receipt = SettlementReceipt {
        tx_hash: "tx-listener".into(),
        amount: "3".into(),
        asset: "ASTRA".into(),
        payer_did: "did:b".into(),
        beneficiary_did: "did:p".into(),
        scope,
        settled_at: 1,
    };
    store.push_settlement_inbox(&receipt).unwrap();
    assert_eq!(store.list_inbox_unprocessed().unwrap().len(), 1);
    let matched = store.match_pending_for_receipt(&receipt).unwrap().unwrap();
    assert_eq!(matched.id, pending.id);
    store
        .complete_pending_with_entitlement(&pending.id, "tx-listener", &"ff".repeat(64), None, None)
        .unwrap();
    store.mark_inbox_processed("tx-listener").unwrap();
    assert!(store.list_inbox_unprocessed().unwrap().is_empty());
    assert!(store.is_inbox_processed("tx-listener"));
}

#[test]
fn soak_webhook_payload_shape_matches_inbox() {
    let dir = tempfile::tempdir().unwrap();
    let store = ContentSettlementStore::new(dir.path());
    let receipt = SettlementReceipt {
        tx_hash: "tx-hook".into(),
        amount: "9".into(),
        asset: "ASTRA".into(),
        payer_did: "did:buyer".into(),
        beneficiary_did: "did:pub".into(),
        scope: payment_scope_content(&"77".repeat(32)),
        settled_at: 42,
    };
    store.push_settlement_inbox(&receipt).unwrap();
    let listed = store.list_inbox_unprocessed().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].tx_hash, "tx-hook");
}

#[test]
fn soak_inbox_auto_match_by_scope() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path();
    let settlements = ContentSettlementStore::new(data);
    let cid = "55".repeat(32);
    let scope = payment_scope_content(&cid);
    let pending = settlements
        .create_pending(
            PurchaseKind::ContentPpv,
            "did:b",
            "did:p",
            Some(&cid),
            None,
            7.0,
            None,
        )
        .unwrap();
    settlements
        .push_settlement_inbox(&SettlementReceipt {
            tx_hash: "tx-inbox".into(),
            amount: "7".into(),
            asset: "ASTRA".into(),
            payer_did: "did:b".into(),
            beneficiary_did: "did:p".into(),
            scope,
            settled_at: 1,
        })
        .unwrap();
    let open = settlements
        .find_open_pending_for_scope("did:b", &pending.scope)
        .unwrap()
        .unwrap();
    assert_eq!(open.id, pending.id);
    let inbox = settlements.list_inbox_unprocessed().unwrap();
    assert_eq!(inbox.len(), 1);
}

#[test]
fn soak_expired_content_grant_denied() {
    let dir = tempfile::tempdir().unwrap();
    let grants = ContentGrantStore::new(dir.path());
    let user = "did:spacekit:viewer";
    let cid = "ee".repeat(32);
    let now = chrono::Utc::now().timestamp() as u64;
    grants
        .grant_content_ppv(user, &cid, None, Some(now.saturating_sub(60)))
        .unwrap();
    let author = QuantumDID::parse("did:spacekit:publisher").unwrap();
    let fact = ppv_fact(
        &author,
        hex::decode(&cid).unwrap().try_into().unwrap(),
        "10",
        None,
    );
    assert!(matches!(
        evaluate_content_access(&fact, user, &grants).unwrap(),
        ContentAccessDecision::PaymentRequired { .. }
    ));
}

#[test]
fn soak_refund_unconsumes_payment_reference() {
    let dir = tempfile::tempdir().unwrap();
    let payments = PaymentReceiptStore::new(dir.path());
    let scope = payment_scope_content(&"88".repeat(32));
    payments
        .record_payment(VerifiedPayment {
            reference: "tx-refund".into(),
            payer_did: "did:b".into(),
            recipient_did: "did:p".into(),
            amount_astra: 5.0,
            scope,
            consumed: false,
            recorded_at: 1,
        })
        .unwrap();
    payments.mark_consumed("tx-refund").unwrap();
    assert!(payments.is_reference_consumed("tx-refund"));
    payments
        .refund_on_grant_failure("tx-refund", "grant failed in test")
        .unwrap();
    assert!(!payments.is_reference_consumed("tx-refund"));
}

#[test]
fn soak_licensed_feature_tier_grant_quota() {
    use spacekit_storage_node::licensed_feature::{
        default_growformer_feature, CAP_INFER, LICENSED_FEATURE_SCHEMA,
    };
    let dir = tempfile::tempdir().unwrap();
    let grants = ContentGrantStore::new(dir.path());
    let doc = default_growformer_feature("did:pub", "GF", "test");
    let cid = "gf".repeat(32);
    grants
        .grant_content_ppv_full(
            "did:buyer",
            &cid,
            None,
            Some(chrono::Utc::now().timestamp() as u64 + 3600),
            None,
            Some("personal".into()),
            None,
            None,
        )
        .unwrap();
    let caps = doc.capabilities_for_tier("personal");
    assert!(caps.contains(&CAP_INFER.to_string()));
    assert_eq!(doc.quota_for_tier("personal"), None);
    assert_eq!(doc.schema, LICENSED_FEATURE_SCHEMA);
}

#[test]
fn soak_settlement_timeout_leaves_pending_open() {
    let dir = tempfile::tempdir().unwrap();
    let store = ContentSettlementStore::new(dir.path());
    let pending = store
        .create_pending(
            PurchaseKind::ContentPpv,
            "did:b",
            "did:p",
            Some(&"99".repeat(32)),
            None,
            4.0,
            None,
        )
        .unwrap();
    assert_eq!(pending.status, "awaiting_payment");
    assert!(store
        .get_pending(&pending.id)
        .unwrap()
        .unwrap()
        .entitlement_id_hex
        .is_none());
}
