//! Integration: [`build_subscriber_sync_bundle`] against a live [`SwtchvmNode`].

use spacekit_compute_node::spacekitvm::SwtchvmNode;
use spacekit_compute_node::subscriber_sync::{
    build_subscriber_sync_bundle, SUBSCRIBER_SYNC_WIRE_VERSION,
};

#[tokio::test]
async fn subscriber_sync_bundle_includes_head() {
    let vm = SwtchvmNode::new(false, false)
        .await
        .expect("SwtchvmNode::new");
    let bundle = build_subscriber_sync_bundle(&vm, "integration-chain");
    assert_eq!(bundle.wire_version, SUBSCRIBER_SYNC_WIRE_VERSION);
    assert_eq!(bundle.chain_id, "integration-chain");
    assert!(bundle.head.hash_hex.starts_with("0x"));
    assert!(bundle.head.state_root_hex.starts_with("0x"));
}
