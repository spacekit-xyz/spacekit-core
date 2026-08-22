//! End-to-end check that `EntitlementReader` can read a real deployment.
//!
//! The unit tests in `src/entitlements.rs` seed the cache directly, so they
//! never exercise ABI encoding, return decoding, the confirmation delay, or the
//! chain-id guard. A mismatch in any of those is invisible until a node is live
//! and refusing every paid request — exactly the failure this test catches.
//!
//! Requires a chain with the entitlement registry deployed. Bring one up with:
//!
//! ```text
//! anvil --chain-id 31337 --block-time 1
//! cd spacekit.xyz-contracts && forge script \
//!   script/DeploySpaceKitEntitlementLocal.s.sol:DeploySpaceKitEntitlementLocal \
//!   --rpc-url http://127.0.0.1:8545 --broadcast
//! ```
//!
//! then export the variables the deploy script prints and run:
//!
//! ```text
//! cargo test -p spacekit-compute-node --test entitlements_onchain -- --nocapture
//! ```
//!
//! Without those variables the test skips, so it is safe in offline CI.

use spacekit_compute_node::entitlements::{EntitlementConfig, EntitlementError, EntitlementReader};

/// DID funded by the local deploy script.
const FUNDED_DID: &str = "did:spacekit:testnet:alice";

fn reader_or_skip() -> Option<EntitlementReader> {
    let config = EntitlementConfig::from_env();
    if !config.enabled {
        eprintln!(
            "skipping: set SPACEKIT_ENTITLEMENT_CONTRACT and SPACEKIT_ENTITLEMENT_RPC_URLS \
             to run this test against a live chain"
        );
        return None;
    }
    Some(EntitlementReader::new(config).expect("entitlement config should be valid"))
}

#[tokio::test]
async fn reads_a_funded_subject_from_chain() {
    let Some(reader) = reader_or_skip() else {
        return;
    };

    let view = reader
        .view(FUNDED_DID)
        .await
        .expect("funded subject should be readable");

    println!("on-chain entitlement: {view:?}");
    assert!(!view.stale, "a live chain read must not be served as stale");
    assert!(
        view.deposited_units > 0,
        "expected a funded subject; did the deploy script run?"
    );
    assert_eq!(view.pending_units, 0);
    assert_eq!(
        view.available_units,
        view.deposited_units - view.consumed_units
    );
    assert!(view.block_number > 0, "read must be confirmation-delayed");
}

/// A DID nobody bound must read as zero rather than erroring, so an unfunded
/// user gets "payment required" instead of a 500.
#[tokio::test]
async fn unbound_subject_reads_as_zero() {
    let Some(reader) = reader_or_skip() else {
        return;
    };

    let view = reader
        .view("did:spacekit:testnet:nobody-has-this-did")
        .await
        .expect("unbound subject should read, not error");
    assert_eq!(view.deposited_units, 0);
    assert_eq!(view.available_units, 0);
}

/// The whole point of the rewrite: the node cannot authorize more than the
/// chain says was deposited.
#[tokio::test]
async fn reserve_is_capped_by_on_chain_balance() {
    let Some(reader) = reader_or_skip() else {
        return;
    };

    let available = reader.view(FUNDED_DID).await.unwrap().available_units;

    let err = reader
        .reserve(FUNDED_DID, available + 1, "over".into())
        .await
        .expect_err("must not authorize more than was deposited");
    assert!(
        matches!(err, EntitlementError::Insufficient { .. }),
        "unexpected error: {err}"
    );

    reader
        .reserve(FUNDED_DID, available, "exact".into())
        .await
        .expect("spending the full balance should be allowed");

    // With the balance fully held, even one more micro-USD must fail.
    assert!(reader.reserve(FUNDED_DID, 1, "extra".into()).await.is_err());

    reader.release(FUNDED_DID, "exact").await;
    reader
        .reserve(FUNDED_DID, available, "again".into())
        .await
        .expect("releasing should return the allowance");
}

/// An RPC pointed at a different network must be rejected outright, so a
/// testnet endpoint cannot be substituted for mainnet to fabricate balances.
#[tokio::test]
async fn wrong_chain_id_is_rejected() {
    let Some(_) = reader_or_skip() else {
        return;
    };

    let mut config = EntitlementConfig::from_env();
    config.chain_id = config.chain_id.wrapping_add(1);
    let reader = EntitlementReader::new(config).unwrap();

    let err = reader
        .view(FUNDED_DID)
        .await
        .expect_err("a chain-id mismatch must fail the read");
    assert!(
        matches!(err, EntitlementError::NoQuorum(_)),
        "unexpected error: {err}"
    );
}

/// Quorum must not be satisfiable by a single endpoint when two are required.
#[tokio::test]
async fn quorum_is_enforced_against_unreachable_endpoints() {
    let Some(_) = reader_or_skip() else {
        return;
    };

    let mut config = EntitlementConfig::from_env();
    config.rpc_endpoints.push("http://127.0.0.1:1/".into());
    config.min_rpc_agreement = config.rpc_endpoints.len();
    let reader = EntitlementReader::new(config).unwrap();

    let err = reader
        .view(FUNDED_DID)
        .await
        .expect_err("an unreachable endpoint must break quorum");
    assert!(
        matches!(err, EntitlementError::NoQuorum(_)),
        "unexpected error: {err}"
    );
}
