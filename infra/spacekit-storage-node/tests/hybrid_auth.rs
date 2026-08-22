//! Hybrid blob/fact auth semantics (Stream A staging).

use spacekit_storage_node::access_policy::{fact_post_allowed, BlobFactAuthMode};
use spacekit_storage_node::upload_token::{
    authorize_blob_write, mint_upload_token, MintUploadTokenRequest, UploadOp,
};

#[test]
fn hybrid_mode_flags() {
    let hybrid = BlobFactAuthMode::Hybrid;
    assert!(hybrid.facts_require_did());
    assert!(!hybrid.blobs_require_did_on_read());
    assert!(hybrid.blobs_require_did_on_write());
    let permissive = BlobFactAuthMode::Permissive;
    assert!(!permissive.facts_require_did());
    assert!(!permissive.blobs_require_did_on_write());
}

#[test]
fn hybrid_blob_write_requires_did_or_upload_token() {
    let hybrid = BlobFactAuthMode::Hybrid;
    assert!(hybrid.blobs_require_did_on_write());
    let hash = hex::encode([0u8; 32]);
    assert!(authorize_blob_write(None, &hash, None, 0).is_none());
    let secret = b"hybrid-test-secret";
    let hash = hex::encode([1u8; 32]);
    let token = mint_upload_token(
        secret,
        "did:spacekit:writer",
        &MintUploadTokenRequest {
            operation: UploadOp::PutBlob,
            resource: hash.clone(),
            ttl_seconds: 60,
        },
        100,
    )
    .unwrap();
    let auth = format!("UploadToken {}", token.token);
    assert!(authorize_blob_write(Some(&auth), &hash, Some(secret), 120).is_some());
    let wrong_hash = hex::encode([2u8; 32]);
    assert!(authorize_blob_write(Some(&auth), &wrong_hash, Some(secret), 120).is_none());
}

#[test]
fn hybrid_fact_post_requires_matching_author() {
    let author = "did:spacekit:author";
    let other = "did:spacekit:other";
    assert!(fact_post_allowed(author, author));
    assert!(!fact_post_allowed(author, other));
}

#[test]
fn strict_blob_read_requires_auth() {
    let strict = BlobFactAuthMode::Strict;
    assert!(strict.blobs_require_did_on_read());
}
