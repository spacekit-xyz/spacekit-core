//! SwtchVM on-disk snapshot + L1-style manifest round-trip.

use spacekit_compute_node::spacekitvm::l1_checkpoint::SnapshotManifest;
use spacekit_compute_node::spacekitvm::{
    manifest_path_for_snapshot, L1PersistenceConfig, SwtchvmAddress, SwtchvmRuntime,
};

#[tokio::test]
async fn snapshot_roundtrip_height_increments() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let bin = dir.path().join("state.bin");
    let l1 = L1PersistenceConfig {
        chain_id: "integration-test-chain".into(),
        strict_manifest_verify: false,
        proposer_did: Some("did:spacekit:integration-node".into()),
    };

    {
        let rt = SwtchvmRuntime::new_with_l1_persistence(false, Some(bin.clone()), l1.clone())?;
        rt.setup_account_balance(&SwtchvmAddress::zero(), 123)
            .await?;
    }

    {
        let rt = SwtchvmRuntime::new_with_l1_persistence(false, Some(bin.clone()), l1.clone())?;
        let b = rt.get_account_balance(&SwtchvmAddress::zero()).await?;
        assert_eq!(b, 123);
        rt.setup_account_balance(&SwtchvmAddress::zero(), 456)
            .await?;
    }

    let man_json = std::fs::read_to_string(manifest_path_for_snapshot(&bin))?;
    let man: SnapshotManifest = serde_json::from_str(&man_json)?;
    assert_eq!(man.chain_id, "integration-test-chain");
    assert_eq!(
        man.proposer_did.as_deref(),
        Some("did:spacekit:integration-node")
    );
    assert_eq!(man.checkpoint.height, 1);

    let rt = SwtchvmRuntime::new_with_l1_persistence(false, Some(bin.clone()), l1)?;
    assert_eq!(rt.get_account_balance(&SwtchvmAddress::zero()).await?, 456);

    Ok(())
}
