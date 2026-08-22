//! Integration tests for ENHANCEMENTS.md gaps A/B/C.

use spacekit_primitives::v1::fact::AccessPolicy;
use spacekit_primitives::v1::fact::{AccessCondition, ConditionType};
use spacekit_storage_node::access_policy::{
    create_fact_verification_message, fact_allows_reader, fact_allows_reader_with_roles,
    fact_post_allowed, fact_requires_signature, BlobFactAuthMode,
};
use spacekit_storage_node::database::Database;
use spacekit_storage_node::repo_commit::{apply_repo_tree, commit_from_tree};
use spacekit_storage_node::sandbox::SandboxConfig;
use spacekit_storage_node::storage_facade::resolve_enable_real_transactions;
use spacekit_storage_node::storage_facade::{Facade, FacadeConfig};
use spacekit_storage_node::transaction::TransactionModification;
use spacekit_storage_node::upload_token::{
    authorize_blob_write, mint_upload_token, verify_upload_token, MintUploadTokenRequest, UploadOp,
};
use spacekit_storage_node::workspace::{
    build_workspace_fact_package, cap_sandbox_config, WorkspaceContent, WorkspaceImportConflict,
    WorkspaceQuotas, WorkspaceStatus, SCHEMA_WORKSPACE_V1,
};
use std::collections::BTreeMap;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

fn migration_scenario_test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

async fn temp_facade_with_handoff_secret() -> (tempfile::TempDir, Arc<Facade>) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(".handoff_secret"), b"test-handoff-hmac-key").unwrap();
    temp_facade_in_dir(dir).await
}

async fn temp_facade() -> (tempfile::TempDir, Arc<Facade>) {
    temp_facade_in_dir(tempfile::tempdir().unwrap()).await
}

async fn temp_facade_in_dir(dir: tempfile::TempDir) -> (tempfile::TempDir, Arc<Facade>) {
    temp_facade_in_dir_with_operator(dir, "did:spacekit:test:operator").await
}

async fn temp_facade_in_dir_with_operator(
    dir: tempfile::TempDir,
    operator_did: &str,
) -> (tempfile::TempDir, Arc<Facade>) {
    let db_path = dir.path().join("db");
    let db = Arc::new(Database::new(db_path.to_str().expect("utf8 path")).unwrap());
    db.initialize().unwrap();
    let cfg = FacadeConfig {
        enable_real_transactions: true,
        cas_data_dir: Some(dir.path().to_path_buf()),
        sandbox_persistence_root: Some(dir.path().join("sandboxes")),
        operator_did: Some(operator_did.to_string()),
        ..Default::default()
    };
    let facade = Arc::new(Facade::new(db, cfg).await.unwrap());
    (dir, facade)
}

#[tokio::test]
async fn signed_export_import_roundtrip() {
    let _lock = migration_scenario_test_lock();
    std::env::remove_var("SPACEKIT_MIGRATION_SCENARIO");
    let (_src_dir, src) = temp_facade_with_handoff_secret().await;
    let (_dst_dir, dst) = temp_facade_with_handoff_secret().await;
    let owner = "did:spacekit:handoff:owner";
    src.create_workspace(WorkspaceContent {
        workspace_id: "signed-ws".into(),
        owner_did: owner.into(),
        collaborators: vec![],
        associated_repos: vec![],
        quotas: Default::default(),
        default_access_policy: AccessPolicy::Public,
        status: WorkspaceStatus::Active,
        created_at: 1,
        updated_at: 1,
    })
    .await
    .unwrap();
    if let Some(src_cas) = src.cas_data_dir() {
        let _ = spacekit_storage_node::migration::load_or_create_operator_keypair(src_cas).unwrap();
    }
    let bundle = src
        .export_workspace(owner, "signed-ws")
        .await
        .unwrap()
        .unwrap();
    assert!(bundle.handoff_signature.is_some());
    if let (Some(src_cas), Some(dst_cas)) = (src.cas_data_dir(), dst.cas_data_dir()) {
        let kp =
            spacekit_storage_node::migration::load_or_create_operator_keypair(src_cas).unwrap();
        std::fs::write(
            dst_cas.join(".operator_sphincs_keypair"),
            serde_json::to_vec_pretty(&kp).unwrap(),
        )
        .unwrap();
        use spacekit_storage_node::operator_manifest::{
            build_operator_fact_package, operator_fact_storage_path, OperatorManifestContent,
        };
        {
            let op = "did:spacekit:test:operator";
            let content = OperatorManifestContent {
                operator_did: op.into(),
                display_name: "Test Op".into(),
                storage_http_url: "http://127.0.0.1:3030".into(),
                blob_fact_auth: "hybrid".into(),
                content_policy_uri: None,
                supported_features: vec!["workspaces".into()],
                published_at: 1,
                supported_migration_versions: vec!["v1".into(), "v2".into()],
                did_signature_capable: true,
                sphincs_public_key_hex: Some(hex::encode(&kp.public_key)),
            };
            let pkg = build_operator_fact_package(content).unwrap();
            let path = operator_fact_storage_path(dst_cas, op);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(&path, serde_json::to_vec(&pkg).unwrap()).unwrap();
        }
    }
    let mut tampered = bundle.clone();
    tampered.content.workspace_id = "tampered-id".into();
    assert!(dst
        .import_workspace(
            owner,
            tampered,
            WorkspaceImportConflict::Reject,
            None,
            None,
            None,
        )
        .await
        .is_err());
    let result = dst
        .import_workspace(
            owner,
            bundle,
            WorkspaceImportConflict::Reject,
            None,
            None,
            None,
        )
        .await
        .unwrap();
    assert!(result.created);
}

#[test]
fn access_policy_public_and_private() {
    let author = "did:spacekit:author";
    let reader = "did:spacekit:reader";
    assert!(fact_allows_reader(&AccessPolicy::Public, reader, author));
    let policy = AccessPolicy::Private(
        [spacekit_primitives::v1::identity::QuantumDID::parse(reader).unwrap()]
            .into_iter()
            .collect(),
    );
    assert!(fact_allows_reader(&policy, reader, author));
    assert!(!fact_allows_reader(&policy, "did:spacekit:other", author));
    assert!(fact_post_allowed(author, author));
    assert!(!fact_post_allowed(author, reader));
}

#[test]
fn strict_mode_requires_signatures() {
    assert!(fact_requires_signature(BlobFactAuthMode::Strict));
    assert!(!fact_requires_signature(BlobFactAuthMode::Hybrid));
    assert!(!fact_requires_signature(BlobFactAuthMode::Permissive));
}

#[tokio::test]
async fn operator_self_published_then_runtime() {
    use spacekit_storage_node::operator_manifest::{
        build_operator_fact_package, load_published_operator_manifest, operator_fact_storage_path,
        OperatorManifestContent,
    };
    let dir = tempfile::tempdir().unwrap();
    let op = "did:spacekit:op:self-test";
    let (dir, facade) = temp_facade_in_dir_with_operator(dir, op).await;
    let url = "http://127.0.0.1:3030".to_string();
    let runtime = facade.operator_self(url.clone()).await.unwrap();
    assert_eq!(runtime.manifest_source, "runtime");
    assert_eq!(runtime.operator_did, op);
    assert!(runtime.fact_id.is_none());

    let content = OperatorManifestContent {
        operator_did: op.into(),
        display_name: "Self Test Op".into(),
        storage_http_url: url.clone(),
        blob_fact_auth: "hybrid".into(),
        content_policy_uri: Some("https://example.com/policy".into()),
        supported_features: vec!["workspaces".into()],
        published_at: 42,
        supported_migration_versions: vec!["v1".into()],
        did_signature_capable: false,
        sphincs_public_key_hex: None,
    };
    let pkg = build_operator_fact_package(content.clone()).unwrap();
    let path = operator_fact_storage_path(dir.path(), op);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, serde_json::to_vec(&pkg).unwrap()).unwrap();
    assert!(load_published_operator_manifest(dir.path(), op)
        .await
        .unwrap()
        .is_some());
    let published = facade.operator_self(url).await.unwrap();
    assert_eq!(published.manifest_source, "published_fact");
    assert_eq!(published.manifest.display_name, "Self Test Op");
    assert!(published.fact_id.is_some());
}

#[tokio::test]
async fn export_includes_v2_migration_manifest_when_keypair_present() {
    let dir = tempfile::tempdir().unwrap();
    let op = "did:spacekit:op:migrate";
    let (_dir, facade) = temp_facade_in_dir_with_operator(dir, op).await;
    let kp = spacekit_storage_node::migration::load_or_create_operator_keypair(
        facade.cas_data_dir().unwrap(),
    )
    .unwrap();
    let _ = kp;
    facade
        .create_workspace(WorkspaceContent {
            workspace_id: "mig-ws".into(),
            owner_did: "did:spacekit:owner:mig".into(),
            collaborators: vec![],
            associated_repos: vec![],
            quotas: Default::default(),
            default_access_policy: AccessPolicy::Public,
            status: WorkspaceStatus::Active,
            created_at: 1,
            updated_at: 1,
        })
        .await
        .unwrap();
    std::env::set_var("SPACEKIT_PUBLIC_HTTP_URL", "http://127.0.0.1:3030");
    let bundle = facade
        .export_workspace("did:spacekit:owner:mig", "mig-ws")
        .await
        .unwrap()
        .unwrap();
    let mig = bundle.migration_manifest.as_ref().unwrap();
    assert_eq!(
        mig.schema_version,
        spacekit_storage_node::migration::SCHEMA_VERSION_V2
    );
    assert!(!mig.did_signatures.is_empty());
}

#[tokio::test]
async fn import_countersigns_destination_and_persists_migration_record() {
    let (_src_dir, src) = temp_facade_with_handoff_secret().await;
    let (_dst_dir, dst) = temp_facade_with_handoff_secret().await;
    let owner = "did:spacekit:owner:dest-sign";
    src.create_workspace(WorkspaceContent {
        workspace_id: "dest-sign-ws".into(),
        owner_did: owner.into(),
        collaborators: vec![],
        associated_repos: vec![],
        quotas: Default::default(),
        default_access_policy: AccessPolicy::Public,
        status: WorkspaceStatus::Active,
        created_at: 1,
        updated_at: 1,
    })
    .await
    .unwrap();
    if let (Some(src_cas), Some(dst_cas)) = (src.cas_data_dir(), dst.cas_data_dir()) {
        let kp =
            spacekit_storage_node::migration::load_or_create_operator_keypair(src_cas).unwrap();
        std::fs::write(
            dst_cas.join(".operator_sphincs_keypair"),
            serde_json::to_vec_pretty(&kp).unwrap(),
        )
        .unwrap();
        use spacekit_storage_node::operator_manifest::{
            build_operator_fact_package, operator_fact_storage_path, OperatorManifestContent,
        };
        let op = "did:spacekit:test:operator";
        let content = OperatorManifestContent {
            operator_did: op.into(),
            display_name: "Test Op".into(),
            storage_http_url: "http://127.0.0.1:3030".into(),
            blob_fact_auth: "hybrid".into(),
            content_policy_uri: None,
            supported_features: vec![],
            published_at: 1,
            supported_migration_versions: vec!["v1".into(), "v2".into()],
            did_signature_capable: true,
            sphincs_public_key_hex: Some(hex::encode(&kp.public_key)),
        };
        let pkg = build_operator_fact_package(content).unwrap();
        let path = operator_fact_storage_path(dst_cas, op);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, serde_json::to_vec(&pkg).unwrap()).unwrap();
    }
    std::env::set_var("SPACEKIT_PUBLIC_HTTP_URL", "http://127.0.0.1:3031");
    let bundle = src
        .export_workspace(owner, "dest-sign-ws")
        .await
        .unwrap()
        .unwrap();
    let migration_id = bundle
        .migration_manifest
        .as_ref()
        .unwrap()
        .migration_id
        .clone();
    let result = dst
        .import_workspace(
            owner,
            bundle,
            WorkspaceImportConflict::Reject,
            None,
            None,
            None,
        )
        .await
        .unwrap();
    let record_id = result
        .migration_record_fact_id
        .expect("migration record fact");
    assert_eq!(record_id.len(), 64);
    let dst_cas = dst.cas_data_dir().unwrap();
    let path =
        spacekit_storage_node::migration::migration_record_storage_path(dst_cas, &migration_id);
    assert!(path.exists(), "migration record at {}", path.display());
    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(raw.contains("destination_operator"));
}

#[tokio::test]
async fn import_accepts_v1_migration_manifest_without_did_signatures() {
    let (_src_dir, src) = temp_facade().await;
    let (_dst_dir, dst) = temp_facade().await;
    let src_owner = "did:spacekit:owner:v1mig-src";
    let dst_owner = "did:spacekit:owner:v1mig-dst";
    src.create_workspace(WorkspaceContent {
        workspace_id: "v1-mig-ws".into(),
        owner_did: src_owner.into(),
        collaborators: vec![],
        associated_repos: vec![],
        quotas: Default::default(),
        default_access_policy: AccessPolicy::Public,
        status: WorkspaceStatus::Active,
        created_at: 1,
        updated_at: 1,
    })
    .await
    .unwrap();
    let mut bundle = src
        .export_workspace(src_owner, "v1-mig-ws")
        .await
        .unwrap()
        .unwrap();
    let mut mig = bundle.migration_manifest.take().unwrap();
    mig.schema_version = spacekit_storage_node::migration::SCHEMA_VERSION_V1.to_string();
    mig.did_signatures.clear();
    bundle.migration_manifest = Some(mig);
    let result = dst
        .import_workspace(
            dst_owner,
            bundle,
            WorkspaceImportConflict::Reject,
            Some(dst_owner.into()),
            None,
            None,
        )
        .await
        .unwrap();
    assert!(result.created);
}

#[tokio::test]
async fn import_with_workspace_owner_signature_when_bilateral_scenario() {
    let _lock = migration_scenario_test_lock();
    std::env::remove_var("SPACEKIT_MIGRATION_SCENARIO");
    let (_src_dir, src) = temp_facade_with_handoff_secret().await;
    let (_dst_dir, dst) = temp_facade_with_handoff_secret().await;
    let src_owner = "did:spacekit:owner:bilateral";
    let dst_owner = "did:spacekit:owner:bilateral-dst";
    src.create_workspace(WorkspaceContent {
        workspace_id: "bi-ws".into(),
        owner_did: src_owner.into(),
        collaborators: vec![],
        associated_repos: vec![],
        quotas: Default::default(),
        default_access_policy: AccessPolicy::Public,
        status: WorkspaceStatus::Active,
        created_at: 1,
        updated_at: 1,
    })
    .await
    .unwrap();
    if let (Some(src_cas), Some(dst_cas)) = (src.cas_data_dir(), dst.cas_data_dir()) {
        let kp =
            spacekit_storage_node::migration::load_or_create_operator_keypair(src_cas).unwrap();
        std::fs::write(
            dst_cas.join(".operator_sphincs_keypair"),
            serde_json::to_vec_pretty(&kp).unwrap(),
        )
        .unwrap();
        let owner_kp = spacekit_storage_node::migration::load_or_create_migration_signer_keypair(
            src_cas, src_owner,
        )
        .unwrap();
        let owner_path =
            spacekit_storage_node::migration::migration_signer_key_path(dst_cas, src_owner);
        if let Some(parent) = owner_path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&owner_path, serde_json::to_vec_pretty(&owner_kp).unwrap()).unwrap();
        use spacekit_storage_node::operator_manifest::{
            build_operator_fact_package, operator_fact_storage_path, OperatorManifestContent,
        };
        let op = "did:spacekit:test:operator";
        let content = OperatorManifestContent {
            operator_did: op.into(),
            display_name: "Test Op".into(),
            storage_http_url: "http://127.0.0.1:3030".into(),
            blob_fact_auth: "hybrid".into(),
            content_policy_uri: None,
            supported_features: vec![],
            published_at: 1,
            supported_migration_versions: vec!["v1".into(), "v2".into()],
            did_signature_capable: true,
            sphincs_public_key_hex: Some(hex::encode(&kp.public_key)),
        };
        let pkg = build_operator_fact_package(content).unwrap();
        let path = operator_fact_storage_path(dst_cas, op);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, serde_json::to_vec(&pkg).unwrap()).unwrap();
    }
    let mut bundle = src
        .export_workspace(src_owner, "bi-ws")
        .await
        .unwrap()
        .unwrap();
    let mig = bundle.migration_manifest.as_mut().unwrap();
    spacekit_storage_node::migration::sign_manifest_role(
        mig,
        "workspace_owner",
        src_owner,
        &spacekit_storage_node::migration::load_migration_signer_keypair(
            src.cas_data_dir().unwrap(),
            src_owner,
        )
        .unwrap(),
        99,
    )
    .unwrap();
    std::env::set_var("SPACEKIT_MIGRATION_SCENARIO", "bilateral");
    let result = dst
        .import_workspace(
            dst_owner,
            bundle,
            WorkspaceImportConflict::Reject,
            Some(dst_owner.into()),
            None,
            None,
        )
        .await
        .unwrap();
    std::env::remove_var("SPACEKIT_MIGRATION_SCENARIO");
    assert!(result.created);
    assert!(result.migration_record_fact_id.is_some());
}

#[test]
fn operator_manifest_v1_builds() {
    use spacekit_primitives::v1::fact::FactContent;
    use spacekit_storage_node::operator_manifest::{
        build_operator_fact_package, OperatorManifestContent, SCHEMA_OPERATOR_V1,
    };
    let content = OperatorManifestContent {
        operator_did: "did:spacekit:op:test".into(),
        display_name: "Test Op".into(),
        storage_http_url: "http://127.0.0.1:3030".into(),
        blob_fact_auth: "hybrid".into(),
        content_policy_uri: None,
        supported_features: vec!["workspaces".into()],
        published_at: 1,
        supported_migration_versions: vec!["v1".into()],
        did_signature_capable: false,
        sphincs_public_key_hex: None,
    };
    let pkg = build_operator_fact_package(content).unwrap();
    match &pkg.content {
        FactContent::Json { schema, .. } => assert_eq!(schema.as_deref(), Some(SCHEMA_OPERATOR_V1)),
        _ => panic!("expected json fact"),
    }
}

#[test]
fn verification_message_is_deterministic() {
    let content = WorkspaceContent {
        workspace_id: "ws".into(),
        owner_did: "did:spacekit:owner:ws".into(),
        collaborators: vec![],
        associated_repos: vec![],
        quotas: Default::default(),
        default_access_policy: AccessPolicy::Public,
        status: WorkspaceStatus::Active,
        created_at: 1,
        updated_at: 1,
    };
    let pkg = build_workspace_fact_package(content).unwrap();
    let a = create_fact_verification_message(&pkg).unwrap();
    let b = create_fact_verification_message(&pkg).unwrap();
    assert_eq!(a, b);
}

#[test]
fn enable_real_transactions_defaults_true() {
    std::env::remove_var("SPACEKIT_ENABLE_REAL_TRANSACTIONS");
    assert!(resolve_enable_real_transactions(true));
    assert!(!resolve_enable_real_transactions(false));
    std::env::set_var("SPACEKIT_ENABLE_REAL_TRANSACTIONS", "false");
    assert!(!resolve_enable_real_transactions(true));
    std::env::remove_var("SPACEKIT_ENABLE_REAL_TRANSACTIONS");
}

#[test]
fn upload_token_hex_secret_normalizes() {
    let hex = "d3cd02d37fa48a49997213fb50592d44df663a13e02120a43c9b29ea3c7410c3";
    let bytes = spacekit_storage_node::upload_token::normalize_secret_bytes(hex);
    assert_eq!(bytes.len(), 32);
}

#[test]
fn upload_token_mint_and_blob_write_scope() {
    let secret = b"test-upload-secret-material";
    let issuer = "did:spacekit:uploader";
    let hash = hex::encode([7u8; 32]);
    let minted = mint_upload_token(
        secret,
        issuer,
        &MintUploadTokenRequest {
            operation: UploadOp::PutBlob,
            resource: hash.clone(),
            ttl_seconds: 120,
        },
        1_000,
    )
    .unwrap();
    let auth = format!("UploadToken {}", minted.token);
    assert!(authorize_blob_write(Some(&auth), &hash, Some(secret), 1_050).is_some());
    assert!(authorize_blob_write(Some(&auth), "deadbeef", Some(secret), 1_050).is_none());
    let claims = verify_upload_token(secret, &minted.token, 1_050).unwrap();
    assert_eq!(claims.sub, issuer);
    assert!(verify_upload_token(secret, &minted.token, 2_000).is_err());
}

#[test]
fn conditional_time_window_policy() {
    use spacekit_primitives::v1::fact::AccessPolicy;
    let author = "did:spacekit:author";
    let reader = "did:spacekit:reader";
    let policy = AccessPolicy::Conditional(vec![AccessCondition {
        condition_type: ConditionType::TimeWindow,
        parameters: HashMap::from([
            ("not_before".to_string(), "100".to_string()),
            ("not_after".to_string(), "200".to_string()),
        ]),
    }]);
    assert!(fact_allows_reader_with_roles(
        &policy, reader, author, None, 150
    ));
    assert!(!fact_allows_reader_with_roles(
        &policy, reader, author, None, 50
    ));
}

#[test]
fn role_based_policy_with_registry() {
    let author = "did:spacekit:author";
    let editor = "did:spacekit:editor";
    let mut roles = HashMap::new();
    roles.insert(editor.to_string(), HashSet::from(["editor".to_string()]));
    let policy = AccessPolicy::RoleBased(HashSet::from(["editor".to_string()]));
    assert!(fact_allows_reader_with_roles(
        &policy,
        editor,
        author,
        Some(&roles),
        0
    ));
    assert!(!fact_allows_reader_with_roles(
        &policy,
        "did:spacekit:stranger",
        author,
        Some(&roles),
        0
    ));
}

#[test]
fn blob_fact_auth_mode_parse_and_env() {
    assert_eq!(
        BlobFactAuthMode::parse("strict"),
        Some(BlobFactAuthMode::Strict)
    );
    assert_eq!(
        BlobFactAuthMode::parse("HYBRID"),
        Some(BlobFactAuthMode::Hybrid)
    );
    std::env::remove_var("SPACEKIT_BLOB_FACT_AUTH");
    assert_eq!(BlobFactAuthMode::from_env(), BlobFactAuthMode::Permissive);
    std::env::set_var("SPACEKIT_BLOB_FACT_AUTH", "hybrid");
    assert_eq!(BlobFactAuthMode::from_env(), BlobFactAuthMode::Hybrid);
    std::env::remove_var("SPACEKIT_BLOB_FACT_AUTH");
}

#[tokio::test]
async fn workspace_import_export_roundtrip() {
    let (_dir, facade) = temp_facade().await;
    let src_owner = "did:spacekit:src:owner";
    let dst_owner = "did:spacekit:dst:owner";
    let ws_id = "handoff-ws";
    facade
        .create_workspace(WorkspaceContent {
            workspace_id: ws_id.into(),
            owner_did: src_owner.into(),
            collaborators: vec![],
            associated_repos: vec!["repo-a".into()],
            quotas: Default::default(),
            default_access_policy: AccessPolicy::Public,
            status: WorkspaceStatus::Active,
            created_at: 10,
            updated_at: 10,
        })
        .await
        .unwrap();
    let bundle = facade
        .export_workspace(src_owner, ws_id)
        .await
        .unwrap()
        .unwrap();
    let imported = facade
        .import_workspace(
            dst_owner,
            bundle,
            WorkspaceImportConflict::Reject,
            Some(dst_owner.into()),
            None,
            None,
        )
        .await
        .unwrap();
    assert!(imported.created);
    assert!(!imported.replaced);
    let got = facade
        .get_workspace(dst_owner, ws_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(got.owner_did, dst_owner);
    assert_eq!(got.associated_repos, vec!["repo-a".to_string()]);
    let again = facade
        .import_workspace(
            dst_owner,
            facade
                .export_workspace(dst_owner, ws_id)
                .await
                .unwrap()
                .unwrap(),
            WorkspaceImportConflict::Reject,
            None,
            None,
            None,
        )
        .await;
    assert!(again.is_err());
}

#[tokio::test]
async fn workspace_export_includes_repo_blob_hashes() {
    let (dir, facade) = temp_facade().await;
    let owner = "did:spacekit:export:hashes";
    let hash = hex::encode([8u8; 32]);
    let tree = BTreeMap::from([("f".into(), hash.clone())]);
    let commit =
        spacekit_storage_node::repo_commit::commit_from_tree(tree, "init".into(), "a".into(), 1);
    apply_repo_tree(
        dir.path(),
        &facade.database,
        owner,
        "r1",
        "main",
        commit,
        &[],
    )
    .await
    .unwrap();
    facade
        .create_workspace(WorkspaceContent {
            workspace_id: "ws-h".into(),
            owner_did: owner.into(),
            collaborators: vec![],
            associated_repos: vec!["r1".into()],
            quotas: Default::default(),
            default_access_policy: AccessPolicy::Public,
            status: WorkspaceStatus::Active,
            created_at: 1,
            updated_at: 1,
        })
        .await
        .unwrap();
    let bundle = facade
        .export_workspace(owner, "ws-h")
        .await
        .unwrap()
        .unwrap();
    assert!(bundle.referenced_blob_hashes.contains(&hash));
}

#[tokio::test]
async fn workspace_export_bundle() {
    let (_dir, facade) = temp_facade().await;
    let owner = "did:spacekit:export:owner";
    let ws_id = "export-ws";
    facade
        .create_workspace(WorkspaceContent {
            workspace_id: ws_id.into(),
            owner_did: owner.into(),
            collaborators: vec![],
            associated_repos: vec!["r1".into()],
            quotas: Default::default(),
            default_access_policy: AccessPolicy::Public,
            status: WorkspaceStatus::Active,
            created_at: 1,
            updated_at: 1,
        })
        .await
        .unwrap();
    let bundle = facade
        .export_workspace(owner, ws_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(bundle.workspace_id, ws_id);
    assert_eq!(bundle.schema, SCHEMA_WORKSPACE_V1);
    assert!(!bundle.fact_id.is_empty());
}

#[test]
fn prometheus_metrics_include_auth_mode() {
    let h = spacekit_storage_node::storage_facade::AgenticHealth {
        enable_real_transactions: true,
        tx_commits_stub_finalize_total: 0,
        tx_commits_real_apply_ok_total: 1,
        tx_commits_real_apply_err_total: 0,
        idempotency_cached_hits_total: 0,
        idempotency_fresh_proceeds_total: 1,
        idempotency_cache_hit_rate: 0.0,
        did_rate_limit_rejections_total: 0,
        did_rate_limit_rejections_last_60s: 0,
        change_feed_live_subscribers: 0,
        change_feed_dropped_subscribers_total: 0,
        change_feed_current_seq: 0,
        sandboxes_total: 0,
        sandboxes_active: 0,
        sandboxes_committing: 0,
        sandboxes_committed: 0,
        sandboxes_discarded: 0,
        sandboxes_expired: 0,
        sandboxes_failed: 0,
        sandboxes_quota_bytes_written: 0,
        sandboxes_quota_vector_ops: 0,
        sandboxes_quota_fact_puts: 0,
        upload_tokens_configured: true,
        blob_fact_auth_mode: "hybrid".to_string(),
        handoff_signing_configured: true,
        require_handoff_signature: false,
        migration_signing_configured: true,
    };
    let text = spacekit_storage_node::operator_metrics::render_prometheus(&h);
    assert!(text.contains("spacekit_blob_fact_auth_mode{mode=\"hybrid\"}"));
    assert!(text.contains("spacekit_upload_tokens_configured 1"));
}

#[tokio::test]
async fn repo_tree_apply_updates_ref() {
    let dir = tempfile::tempdir().unwrap();
    let data_dir = dir.path().to_path_buf();
    let db = Database::new(data_dir.join("db").to_str().expect("utf8 path")).unwrap();
    db.initialize().unwrap();
    let owner = "did:spacekit:repo:test";
    // Valid 32-byte BLAKE3 hex (64 chars)
    let hash = hex::encode([0u8; 32]);
    let tree = BTreeMap::from([("README.md".to_string(), hash.clone())]);
    let commit = commit_from_tree(tree, "test commit".into(), "tester".into(), 1_700_000_000);
    let (fact_id, _) = apply_repo_tree(&data_dir, &db, owner, "demo", "main", commit, &[])
        .await
        .unwrap();
    assert_eq!(fact_id.len(), 64);
    let collection = spacekit_storage_node::repo_commit::ref_collection("demo");
    let ref_id = spacekit_storage_node::repo_commit::ref_document_id("main");
    let doc = db
        .get_document(owner, &collection, &ref_id)
        .unwrap()
        .unwrap();
    assert_eq!(doc.data["tip"].as_str().unwrap(), fact_id);
}

#[tokio::test]
async fn workspace_fact_roundtrip() {
    let content = WorkspaceContent {
        workspace_id: "ws-test".into(),
        owner_did: "did:spacekit:owner:ws".into(),
        collaborators: vec![],
        associated_repos: vec!["demo".into()],
        quotas: Default::default(),
        default_access_policy: AccessPolicy::Public,
        status: WorkspaceStatus::Active,
        created_at: 1,
        updated_at: 1,
    };
    let pkg = build_workspace_fact_package(content.clone()).unwrap();
    let parsed = spacekit_storage_node::workspace::parse_workspace_from_fact(&pkg).unwrap();
    assert_eq!(parsed.workspace_id, "ws-test");
    match &pkg.content {
        spacekit_primitives::v1::fact::FactContent::Json { schema, .. } => {
            assert_eq!(schema.as_deref(), Some(SCHEMA_WORKSPACE_V1));
        }
        _ => panic!("expected json"),
    }
}

#[test]
fn workspace_cap_sandbox_config() {
    let mut cfg = SandboxConfig {
        ttl_seconds: 3600,
        max_bytes_written: 100 * 1024 * 1024,
        max_vector_ops: 10_000,
        max_fact_puts: 1_000,
    };
    cap_sandbox_config(
        &mut cfg,
        &WorkspaceQuotas {
            max_sandbox_bytes: 4096,
            max_storage_bytes: 0,
        },
    );
    assert_eq!(cfg.max_bytes_written, 4096);
}

#[tokio::test]
async fn workspace_quota_enforced_on_sandbox_create() {
    let (_dir, facade) = temp_facade().await;
    let owner = "did:spacekit:ws:owner";
    let ws_id = "quota-ws";
    let content = WorkspaceContent {
        workspace_id: ws_id.into(),
        owner_did: owner.into(),
        collaborators: vec![],
        associated_repos: vec![],
        quotas: WorkspaceQuotas {
            max_sandbox_bytes: 8192,
            max_storage_bytes: 1_000_000,
        },
        default_access_policy: AccessPolicy::Public,
        status: WorkspaceStatus::Active,
        created_at: 1,
        updated_at: 1,
    };
    facade.create_workspace(content).await.unwrap();

    let mut cfg = SandboxConfig::default();
    cfg.max_bytes_written = 50 * 1024 * 1024;
    let sb = facade
        .create_sandbox(owner, owner, cfg, None, vec![], Some(ws_id.into()))
        .await
        .unwrap();
    assert_eq!(sb.config.max_bytes_written, 8192);
    assert_eq!(sb.workspace_id.as_deref(), Some(ws_id));
}

#[tokio::test]
async fn sandbox_commit_with_repo_tree_via_facade() {
    let (_dir, facade) = temp_facade().await;
    let owner = "did:spacekit:sandbox:repo";
    let sb = facade
        .create_sandbox(owner, owner, SandboxConfig::default(), None, vec![], None)
        .await
        .unwrap();
    let hash = hex::encode([1u8; 32]);
    let tree = BTreeMap::from([("file.txt".to_string(), hash)]);
    let commit = commit_from_tree(tree, "sb commit".into(), "agent".into(), 2);
    let tx_id = facade.begin_transaction(None, Some(120)).await.unwrap();
    let modification = TransactionModification::RepoTree {
        owner_did: owner.to_string(),
        repo_name: "sb-repo".to_string(),
        branch: "main".to_string(),
        commit,
        parent_fact_ids: vec![],
        old_ref: None,
        applied_fact_id_hex: None,
    };
    facade
        .record_transaction_modification(
            &tx_id,
            modification.clone(),
            spacekit_storage_node::sandbox::ConflictPolicy::ThreeWayMerge,
            0,
            Some(&sb.id),
            Some(owner),
        )
        .await
        .unwrap();
    facade
        .sandboxes
        .commit(&sb.id, facade.transactions.clone(), false)
        .await
        .unwrap();
    let collection = spacekit_storage_node::repo_commit::ref_collection("sb-repo");
    let ref_id = spacekit_storage_node::repo_commit::ref_document_id("main");
    let doc = facade
        .database
        .get_document(owner, &collection, &ref_id)
        .unwrap()
        .unwrap();
    assert!(doc.data.get("tip").is_some());
}

#[test]
fn content_grants_ppv_and_channel_subscription() {
    let dir = tempfile::tempdir().unwrap();
    let store = spacekit_storage_node::content_grants::ContentGrantStore::new(dir.path());
    let user = "did:spacekit:viewer";
    let content_id = "abcd".repeat(8);
    let channel = "did:spacekit:channel:test";
    assert!(!store.has_content_grant(user, &content_id));
    store
        .grant_content_ppv(user, &content_id, Some("pay-1".into()), None)
        .unwrap();
    assert!(store.has_content_grant(user, &content_id));
    store
        .grant_channel_subscription(user, channel, None, None)
        .unwrap();
    assert!(store.has_channel_subscription(user, channel));
    let listed = store.list_for_requester(user).unwrap();
    assert_eq!(listed.len(), 2);
}

#[test]
fn content_access_payment_required_without_grant() {
    use spacekit_primitives::v1::crypto::quantum::SPHINCSSignature;
    use spacekit_primitives::v1::fact::{
        AccessCondition, AccessPolicy, CollectionMethod, ConditionType, DataSource, FactCategory,
        FactContent, FactMetadata, FactPackage, KnowledgeDomain, LicenseType, ProofType,
        VerificationLevel, VerificationProof,
    };
    use spacekit_primitives::v1::identity::QuantumDID;
    use spacekit_storage_node::content_access::{evaluate_content_access, ContentAccessDecision};
    use spacekit_storage_node::content_grants::ContentGrantStore;
    use std::collections::HashMap;

    let dir = tempfile::tempdir().unwrap();
    let grants = ContentGrantStore::new(dir.path());
    let author = QuantumDID::parse("did:spacekit:author").unwrap();
    let fact_id = [7u8; 32];
    let mut params = HashMap::new();
    params.insert("price".into(), "10".into());
    params.insert("currency".into(), "ASTRA".into());
    params.insert("content_id".into(), hex::encode(fact_id));
    let fact = FactPackage {
        fact_id,
        version: 1,
        created_at: 1,
        expires_at: None,
        content: FactContent::Binary {
            data: vec![1, 2, 3],
            mime_type: "text/plain".into(),
            hash: [0u8; 32],
        },
        metadata: FactMetadata {
            category: FactCategory::UserGenerated,
            tags: vec!["content".into()],
            domain: KnowledgeDomain::Custom("x".into()),
            source: DataSource::UserInput {
                application: author.clone(),
                user: author.clone(),
            },
            collection_method: CollectionMethod::Manual,
            verification_level: VerificationLevel::SelfClaimed,
            license: LicenseType::Proprietary,
            size_bytes: 3,
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
    };
    let viewer = "did:spacekit:viewer";
    match evaluate_content_access(&fact, viewer, &grants).unwrap() {
        ContentAccessDecision::PaymentRequired { currency, .. } => assert_eq!(currency, "ASTRA"),
        other => panic!("expected PaymentRequired, got {:?}", other),
    }
    grants
        .grant_content_ppv(viewer, &hex::encode(fact_id), None, None)
        .unwrap();
    assert!(matches!(
        evaluate_content_access(&fact, viewer, &grants).unwrap(),
        ContentAccessDecision::Allowed
    ));
}

#[tokio::test]
async fn migration_v1_export_imports_to_v2_capable_destination() {
    let _lock = migration_scenario_test_lock();
    std::env::remove_var("SPACEKIT_MIGRATION_SCENARIO");
    std::env::remove_var("SPACEKIT_MIGRATION_DEST_URL");
    std::env::remove_var("SPACEKIT_REQUIRE_MIGRATION_ATTESTATION");
    let (_src_dir, src) = temp_facade_with_handoff_secret().await;
    let (_dst_dir, dst) = temp_facade_with_handoff_secret().await;
    let owner = "did:spacekit:owner:v1-to-v2-matrix";
    src.create_workspace(WorkspaceContent {
        workspace_id: "v1v2-ws".into(),
        owner_did: owner.into(),
        collaborators: vec![],
        associated_repos: vec![],
        quotas: Default::default(),
        default_access_policy: AccessPolicy::Public,
        status: WorkspaceStatus::Active,
        created_at: 1,
        updated_at: 1,
    })
    .await
    .unwrap();
    let mut bundle = src
        .export_workspace(owner, "v1v2-ws")
        .await
        .unwrap()
        .unwrap();
    let mut mig = bundle.migration_manifest.take().unwrap();
    mig.schema_version = spacekit_storage_node::migration::SCHEMA_VERSION_V1.to_string();
    mig.did_signatures.clear();
    bundle.migration_manifest = Some(mig);
    if let (Some(_src_cas), Some(dst_cas)) = (src.cas_data_dir(), dst.cas_data_dir()) {
        let kp =
            spacekit_storage_node::migration::load_or_create_operator_keypair(dst_cas).unwrap();
        use spacekit_storage_node::operator_manifest::{
            build_operator_fact_package, operator_fact_storage_path, OperatorManifestContent,
        };
        let op = "did:spacekit:test:operator";
        let content = OperatorManifestContent {
            operator_did: op.into(),
            display_name: "V2 Op".into(),
            storage_http_url: "http://127.0.0.1:3030".into(),
            blob_fact_auth: "hybrid".into(),
            content_policy_uri: None,
            supported_features: vec![],
            published_at: 1,
            supported_migration_versions: vec!["v1".into(), "v2".into()],
            did_signature_capable: true,
            sphincs_public_key_hex: Some(hex::encode(&kp.public_key)),
        };
        let pkg = build_operator_fact_package(content).unwrap();
        let path = operator_fact_storage_path(dst_cas, op);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, serde_json::to_vec(&pkg).unwrap()).unwrap();
    }
    let result = dst
        .import_workspace(
            owner,
            bundle,
            WorkspaceImportConflict::Reject,
            None,
            None,
            None,
        )
        .await
        .unwrap();
    assert!(result.created);
}

#[tokio::test]
async fn import_rejects_v2_without_signatures_when_attestation_required() {
    let _lock = migration_scenario_test_lock();
    std::env::set_var("SPACEKIT_REQUIRE_MIGRATION_ATTESTATION", "true");
    let (_src_dir, src) = temp_facade_with_handoff_secret().await;
    let (_dst_dir, dst) = temp_facade_with_handoff_secret().await;
    let owner = "did:spacekit:owner:attest-req";
    src.create_workspace(WorkspaceContent {
        workspace_id: "attest-ws".into(),
        owner_did: owner.into(),
        collaborators: vec![],
        associated_repos: vec![],
        quotas: Default::default(),
        default_access_policy: AccessPolicy::Public,
        status: WorkspaceStatus::Active,
        created_at: 1,
        updated_at: 1,
    })
    .await
    .unwrap();
    let mut bundle = src
        .export_workspace(owner, "attest-ws")
        .await
        .unwrap()
        .unwrap();
    let mut mig = bundle.migration_manifest.as_mut().unwrap();
    mig.did_signatures.clear();
    mig.schema_version = spacekit_storage_node::migration::SCHEMA_VERSION_V2.to_string();
    let err = dst
        .import_workspace(
            owner,
            bundle,
            WorkspaceImportConflict::Reject,
            None,
            None,
            None,
        )
        .await
        .unwrap_err();
    assert!(err.to_string().contains("did_signatures") || err.to_string().contains("signature"));
    std::env::remove_var("SPACEKIT_REQUIRE_MIGRATION_ATTESTATION");
}

#[tokio::test]
async fn import_rejects_invalid_migration_signature_on_bundle() {
    let (_src_dir, src) = temp_facade_with_handoff_secret().await;
    let (_dst_dir, dst) = temp_facade_with_handoff_secret().await;
    let owner = "did:spacekit:owner:bad-sig";
    src.create_workspace(WorkspaceContent {
        workspace_id: "bad-sig-ws".into(),
        owner_did: owner.into(),
        collaborators: vec![],
        associated_repos: vec![],
        quotas: Default::default(),
        default_access_policy: AccessPolicy::Public,
        status: WorkspaceStatus::Active,
        created_at: 1,
        updated_at: 1,
    })
    .await
    .unwrap();
    if let Some(cas) = src.cas_data_dir() {
        let _ = spacekit_storage_node::migration::load_or_create_operator_keypair(cas).unwrap();
    }
    let mut bundle = src
        .export_workspace(owner, "bad-sig-ws")
        .await
        .unwrap()
        .unwrap();
    if let Some(mig) = bundle.migration_manifest.as_mut() {
        if let Some(sig) = mig.did_signatures.first_mut() {
            sig.signature = "00".repeat(64);
        }
    }
    assert!(dst
        .import_workspace(
            owner,
            bundle,
            WorkspaceImportConflict::Reject,
            None,
            None,
            None
        )
        .await
        .is_err());
}

#[tokio::test]
async fn migration_replay_import_rejected_when_workspace_exists() {
    let (_src_dir, src) = temp_facade_with_handoff_secret().await;
    let (_dst_dir, dst) = temp_facade_with_handoff_secret().await;
    let owner = "did:spacekit:owner:replay";
    src.create_workspace(WorkspaceContent {
        workspace_id: "replay-ws".into(),
        owner_did: owner.into(),
        collaborators: vec![],
        associated_repos: vec![],
        quotas: Default::default(),
        default_access_policy: AccessPolicy::Public,
        status: WorkspaceStatus::Active,
        created_at: 1,
        updated_at: 1,
    })
    .await
    .unwrap();
    let bundle = src
        .export_workspace(owner, "replay-ws")
        .await
        .unwrap()
        .unwrap();
    dst.import_workspace(
        owner,
        bundle.clone(),
        WorkspaceImportConflict::Reject,
        None,
        None,
        None,
    )
    .await
    .unwrap();
    let replay = dst
        .import_workspace(
            owner,
            bundle,
            WorkspaceImportConflict::Reject,
            None,
            None,
            None,
        )
        .await;
    assert!(replay.is_err());
    assert!(replay.unwrap_err().to_string().contains("CONFLICT"));
}
