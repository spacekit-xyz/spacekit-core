// Integration tests for LayerZero Bridge
// These tests verify the bridge manager functionality

use spacekit_compute_node::layerzero_bridge::{
    BridgeFeeConfig, BridgeGasLimits, BridgeStatus, CrossChainExecutionConfig,
    LayerZeroBridgeConfig, LayerZeroBridgeManager, SupportedChain, TokenBridgeMapping,
};
use std::collections::HashMap;

#[tokio::test]
async fn test_bridge_manager_initialization() {
    let config = create_test_config();
    let bridge = LayerZeroBridgeManager::new(config);

    let result = bridge.initialize().await;
    assert!(result.is_ok(), "Bridge initialization should succeed");
}

#[tokio::test]
async fn test_oft_mode_configuration() {
    let mut config = create_test_config();
    config.use_oft = true;

    let bridge = LayerZeroBridgeManager::new(config);
    assert!(bridge.initialize().await.is_ok());
}

#[tokio::test]
async fn test_supported_chains() {
    let config = create_test_config();
    let bridge = LayerZeroBridgeManager::new(config);

    let chains = bridge.get_supported_chains();
    assert!(!chains.is_empty(), "Should have supported chains");
    assert!(chains.contains(&SupportedChain::Ethereum));
    assert!(chains.contains(&SupportedChain::Arbitrum));
}

#[tokio::test]
async fn test_bridge_statistics() {
    let config = create_test_config();
    let bridge = LayerZeroBridgeManager::new(config);
    bridge.initialize().await.unwrap();

    let stats = bridge.get_bridge_statistics().await;
    assert_eq!(stats.total_token_transfers, 0);
    assert_eq!(stats.completed_token_transfers, 0);
}

#[tokio::test]
async fn test_chain_endpoint_id_conversion() {
    assert_eq!(SupportedChain::Ethereum.endpoint_id(), 30101);
    assert_eq!(SupportedChain::Arbitrum.endpoint_id(), 30110);
    assert_eq!(SupportedChain::Polygon.endpoint_id(), 30109);

    assert_eq!(
        SupportedChain::from_endpoint_id(30101),
        Some(SupportedChain::Ethereum)
    );
    assert_eq!(SupportedChain::from_endpoint_id(99999), None);
}

#[tokio::test]
async fn test_chain_names() {
    assert_eq!(SupportedChain::Ethereum.name(), "Ethereum");
    assert_eq!(SupportedChain::Arbitrum.name(), "Arbitrum");
    assert_eq!(SupportedChain::Base.name(), "Base");
}

#[tokio::test]
async fn test_config_validation_with_oft_mode() {
    let mut config = create_test_config();
    config.use_oft = true;

    // In OFT mode, wrapped_astra can be None
    for (_, mapping) in config.token_mappings.iter_mut() {
        mapping.wrapped_astra = None;
    }

    let bridge = LayerZeroBridgeManager::new(config);
    assert!(bridge.initialize().await.is_ok());
}

#[tokio::test]
async fn test_config_validation_without_oft_mode() {
    let mut config = create_test_config();
    config.use_oft = false;

    // Non-OFT mode requires wrapped_astra
    for (_, mapping) in config.token_mappings.iter_mut() {
        mapping.wrapped_astra = Some("0xWrappedAddress".to_string());
    }

    let bridge = LayerZeroBridgeManager::new(config);
    assert!(bridge.initialize().await.is_ok());
}

// Helper function to create test configuration
fn create_test_config() -> LayerZeroBridgeConfig {
    let mut bridge_contracts = HashMap::new();
    bridge_contracts.insert(
        SupportedChain::Ethereum,
        "0x1111111111111111111111111111111111111111".to_string(),
    );
    bridge_contracts.insert(
        SupportedChain::Arbitrum,
        "0x2222222222222222222222222222222222222222".to_string(),
    );

    let mut token_mappings = HashMap::new();
    token_mappings.insert(
        SupportedChain::Ethereum,
        TokenBridgeMapping {
            astra_token: "0xETH_ASTRA".to_string(),
            wrapped_astra: Some("0xETH_WASTRA".to_string()),
            usdc_token: "0xUSDC_ETH".to_string(),
            supported_tokens: HashMap::new(),
        },
    );
    token_mappings.insert(
        SupportedChain::Arbitrum,
        TokenBridgeMapping {
            astra_token: "0xARB_ASTRA".to_string(),
            wrapped_astra: Some("0xARB_WASTRA".to_string()),
            usdc_token: "0xUSDC_ARB".to_string(),
            supported_tokens: HashMap::new(),
        },
    );

    let mut oft_contracts = HashMap::new();
    oft_contracts.insert(SupportedChain::Ethereum, "0xETH_OFT".to_string());
    oft_contracts.insert(SupportedChain::Arbitrum, "0xARB_OFT".to_string());

    let mut rpc_endpoints = HashMap::new();
    rpc_endpoints.insert(SupportedChain::Ethereum, "https://test-eth.rpc".to_string());
    rpc_endpoints.insert(SupportedChain::Arbitrum, "https://test-arb.rpc".to_string());

    LayerZeroBridgeConfig {
        mock_chain_transactions: false,
        enabled: true,
        spacekit_endpoint_id: 40000,
        bridge_contracts,
        token_mappings,
        gas_limits: BridgeGasLimits {
            bridge_token: 200000,
            execute_task: 500000,
            distribute_reward: 150000,
            status_update: 100000,
        },
        bridge_fees: BridgeFeeConfig {
            base_fee_percentage: 0.001,
            message_fee_buffer: 0.1,
            minimum_bridge_amount: 1000000000000000000,
            maximum_bridge_amount: 1000000000000000000000000,
        },
        cross_chain_execution: CrossChainExecutionConfig {
            enabled: true,
            max_execution_time: 3600,
            supported_runtimes: vec!["wasm".to_string(), "gpu".to_string()],
            auto_retry: true,
            max_retries: 3,
        },
        use_oft: true,
        oft_contracts,
        rpc_endpoints,
        signer_private_key: None, // Tests don't need real signing
    }
}
