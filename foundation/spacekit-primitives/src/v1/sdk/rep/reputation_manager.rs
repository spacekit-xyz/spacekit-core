use alloy::contract::{ContractInstance, Interface};
use alloy::dyn_abi::DynSolValue;
use alloy::json_abi::JsonAbi;
use alloy::network::Ethereum;
use alloy::providers::Provider;
use alloy::signers::SignerSync;

use alloy::primitives::{Address, Bytes, FixedBytes, U256};
use std::sync::Arc;

pub struct ReputationManagerClient<P: Provider> {
    pub contract: ContractInstance<Arc<P>, Ethereum>,
    pub wallet: Box<dyn SignerSync>,
}

impl<P: Provider> ReputationManagerClient<P> {
    pub fn new(address: Address, provider: Arc<P>, wallet: Box<dyn SignerSync>) -> Self {
        let abi_str = r#"[ { "inputs": [], "name": "InvalidInitialization", "type": "error" }, { "inputs": [], "name": "NotInitializing", "type": "error" }, { "inputs": [ { "internalType": "address", "name": "owner", "type": "address" } ], "name": "OwnableInvalidOwner", "type": "error" }, { "inputs": [ { "internalType": "address", "name": "account", "type": "address" } ], "name": "OwnableUnauthorizedAccount", "type": "error" }, { "anonymous": false, "inputs": [ { "indexed": true, "internalType": "address", "name": "did", "type": "address" }, { "indexed": true, "internalType": "bytes32", "name": "actionType", "type": "bytes32" }, { "indexed": false, "internalType": "uint256", "name": "weight", "type": "uint256" } ], "name": "ActionWeightSet", "type": "event" }, { "anonymous": false, "inputs": [ { "indexed": false, "internalType": "uint64", "name": "version", "type": "uint64" } ], "name": "Initialized", "type": "event" }, { "anonymous": false, "inputs": [ { "indexed": true, "internalType": "address", "name": "previousOwner", "type": "address" }, { "indexed": true, "internalType": "address", "name": "newOwner", "type": "address" } ], "name": "OwnershipTransferred", "type": "event" }, { "anonymous": false, "inputs": [ { "indexed": true, "internalType": "address", "name": "did", "type": "address" }, { "indexed": true, "internalType": "bytes32", "name": "productHash", "type": "bytes32" }, { "indexed": false, "internalType": "uint256", "name": "newScore", "type": "uint256" } ], "name": "ProductScoreUpdated", "type": "event" }, { "anonymous": false, "inputs": [ { "indexed": true, "internalType": "address", "name": "did", "type": "address" }, { "indexed": false, "internalType": "bool", "name": "isProducer", "type": "bool" }, { "indexed": false, "internalType": "uint256", "name": "newScore", "type": "uint256" } ], "name": "ScoreUpdated", "type": "event" }, { "inputs": [], "name": "erc20Escrow", "outputs": [ { "internalType": "contract ERC20ReputableEscrow", "name": "", "type": "address" } ], "stateMutability": "view", "type": "function" }, { "inputs": [], "name": "erc721Escrow", "outputs": [ { "internalType": "contract ERC721ReputableEscrow", "name": "", "type": "address" } ], "stateMutability": "view", "type": "function" }, { "inputs": [], "name": "ethEscrow", "outputs": [ { "internalType": "contract ReputableEscrow", "name": "", "type": "address" } ], "stateMutability": "view", "type": "function" }, { "inputs": [ { "internalType": "address", "name": "did", "type": "address" } ], "name": "getCompleteProfile", "outputs": [ { "internalType": "uint256", "name": "", "type": "uint256" }, { "internalType": "uint256", "name": "", "type": "uint256" }, { "internalType": "uint256", "name": "", "type": "uint256" } ], "stateMutability": "view", "type": "function" }, { "inputs": [ { "internalType": "address", "name": "did", "type": "address" }, { "internalType": "bytes32", "name": "productHash", "type": "bytes32" } ], "name": "getProductScore", "outputs": [ { "internalType": "uint256", "name": "", "type": "uint256" } ], "stateMutability": "view", "type": "function" }, { "inputs": [], "name": "identityManager", "outputs": [ { "internalType": "contract IdentityManager", "name": "", "type": "address" } ], "stateMutability": "view", "type": "function" }, { "inputs": [ { "internalType": "address", "name": "_identityManagerAddress", "type": "address" }, { "internalType": "address", "name": "_ethEscrow", "type": "address" }, { "internalType": "address", "name": "_erc20Escrow", "type": "address" }, { "internalType": "address", "name": "_erc721Escrow", "type": "address" } ], "name": "initialize", "outputs": [], "stateMutability": "nonpayable", "type": "function" }, { "inputs": [ { "internalType": "uint256", "name": "amount", "type": "uint256" } ], "name": "initiateERC20Escrow", "outputs": [], "stateMutability": "nonpayable", "type": "function" }, { "inputs": [], "name": "initiateERC721Escrow", "outputs": [], "stateMutability": "nonpayable", "type": "function" }, { "inputs": [], "name": "initiateEscrow", "outputs": [], "stateMutability": "payable", "type": "function" }, { "inputs": [], "name": "owner", "outputs": [ { "internalType": "address", "name": "", "type": "address" } ], "stateMutability": "view", "type": "function" }, { "inputs": [ { "internalType": "address", "name": "", "type": "address" } ], "name": "participantScores", "outputs": [ { "components": [ { "internalType": "uint256", "name": "score", "type": "uint256" }, { "internalType": "uint256", "name": "lastUpdateTimestamp", "type": "uint256" } ], "internalType": "struct ReputationScoreLib.Score", "name": "asConsumer", "type": "tuple" }, { "components": [ { "internalType": "uint256", "name": "score", "type": "uint256" }, { "internalType": "uint256", "name": "lastUpdateTimestamp", "type": "uint256" } ], "internalType": "struct ReputationScoreLib.Score", "name": "asProducer", "type": "tuple" } ], "stateMutability": "view", "type": "function" }, { "inputs": [], "name": "refundERC20Escrow", "outputs": [], "stateMutability": "nonpayable", "type": "function" }, { "inputs": [], "name": "refundERC721Escrow", "outputs": [], "stateMutability": "nonpayable", "type": "function" }, { "inputs": [], "name": "refundEscrow", "outputs": [], "stateMutability": "nonpayable", "type": "function" }, { "inputs": [], "name": "releaseERC20Escrow", "outputs": [], "stateMutability": "nonpayable", "type": "function" }, { "inputs": [], "name": "releaseERC721Escrow", "outputs": [], "stateMutability": "nonpayable", "type": "function" }, { "inputs": [], "name": "releaseEscrow", "outputs": [], "stateMutability": "nonpayable", "type": "function" }, { "inputs": [], "name": "renounceOwnership", "outputs": [], "stateMutability": "nonpayable", "type": "function" }, { "inputs": [ { "internalType": "address", "name": "did", "type": "address" }, { "internalType": "bytes32", "name": "actionType", "type": "bytes32" }, { "internalType": "uint256", "name": "weight", "type": "uint256" } ], "name": "setActionWeight", "outputs": [], "stateMutability": "nonpayable", "type": "function" }, { "inputs": [ { "internalType": "address", "name": "_newERC20Escrow", "type": "address" } ], "name": "setERC20Escrow", "outputs": [], "stateMutability": "nonpayable", "type": "function" }, { "inputs": [ { "internalType": "address", "name": "_newERC721Escrow", "type": "address" } ], "name": "setERC721Escrow", "outputs": [], "stateMutability": "nonpayable", "type": "function" }, { "inputs": [ { "internalType": "address", "name": "_newEthEscrow", "type": "address" } ], "name": "setEthEscrow", "outputs": [], "stateMutability": "nonpayable", "type": "function" }, { "inputs": [ { "internalType": "address", "name": "_newIdentityManager", "type": "address" } ], "name": "setIdentityManager", "outputs": [], "stateMutability": "nonpayable", "type": "function" }, { "inputs": [ { "internalType": "address", "name": "newOwner", "type": "address" } ], "name": "transferOwnership", "outputs": [], "stateMutability": "nonpayable", "type": "function" }, { "inputs": [ { "internalType": "address", "name": "did", "type": "address" }, { "internalType": "bytes32", "name": "productHash", "type": "bytes32" }, { "internalType": "uint256", "name": "newScore", "type": "uint256" } ], "name": "updateProductScore", "outputs": [], "stateMutability": "nonpayable", "type": "function" }, { "inputs": [ { "internalType": "address", "name": "did", "type": "address" }, { "internalType": "bool", "name": "isProducer", "type": "bool" }, { "internalType": "bytes32", "name": "actionType", "type": "bytes32" }, { "internalType": "bool", "name": "success", "type": "bool" } ], "name": "updateScore", "outputs": [], "stateMutability": "nonpayable", "type": "function" } ]"#;
        let abi = serde_json::from_str::<JsonAbi>(abi_str).unwrap();
        let contract = ContractInstance::new(address, Arc::clone(&provider), Interface::new(abi));
        Self { contract, wallet }
    }

    // Scoring interface

    pub async fn update_score(
        &self,
        did: Address,
        is_producer: bool,
        action_type: Bytes,
        success: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let fixed_bytes = FixedBytes::<32>::from_slice(&action_type);
        let args = [
            DynSolValue::Address(did),
            DynSolValue::Bool(is_producer),
            DynSolValue::FixedBytes(fixed_bytes, 32),
            DynSolValue::Bool(success),
        ];

        let _result = self
            .contract
            .function("updateScore", &args)
            .unwrap()
            .call()
            .await?;
        Ok(())
    }

    pub async fn set_action_weight(
        &self,
        did: Address,
        action_type: Bytes,
        weight: U256,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let fixed_bytes = FixedBytes::<32>::from_slice(&action_type);
        let args = [
            DynSolValue::Address(did),
            DynSolValue::FixedBytes(fixed_bytes, 32),
            DynSolValue::Uint(weight, 256),
        ];
        let _result = self
            .contract
            .function("setActionWeight", &args)
            .unwrap()
            .call()
            .await?;
        Ok(())
    }

    pub async fn update_product_score(
        &self,
        did: Address,
        product_hash: Bytes,
        new_score: U256,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let fixed_bytes = FixedBytes::<32>::from_slice(&product_hash);
        let args = [
            DynSolValue::Address(did),
            DynSolValue::FixedBytes(fixed_bytes, 32),
            DynSolValue::Uint(new_score, 256),
        ];
        let _result = self
            .contract
            .function("updateProductScore", &args)
            .unwrap()
            .call()
            .await?;
        Ok(())
    }

    pub async fn get_complete_profile(
        &self,
        did: Address,
    ) -> Result<(U256, U256, U256), Box<dyn std::error::Error>> {
        let args = [DynSolValue::Address(did)];
        let result = self
            .contract
            .function("getCompleteProfile", &args)
            .unwrap()
            .call()
            .await?;
        let consumer_score = result[0].as_uint().unwrap().0;
        let producer_score = result[1].as_uint().unwrap().0;
        let escrow_balance = result[2].as_uint().unwrap().0;
        Ok((consumer_score, producer_score, escrow_balance))
    }

    pub async fn get_product_score(
        &self,
        did: Address,
        product_hash: Bytes,
    ) -> Result<U256, Box<dyn std::error::Error>> {
        let fixed_bytes = FixedBytes::<32>::from_slice(&product_hash);
        let args = [
            DynSolValue::Address(did),
            DynSolValue::FixedBytes(fixed_bytes, 32),
        ];
        let result = self
            .contract
            .function("getProductScore", &args)
            .unwrap()
            .call()
            .await?;
        let product_score = result[0].as_uint().unwrap().0;
        Ok(product_score)
    }

    // Escrow interface

    pub async fn initiate_escrow(&self, amount: U256) -> Result<(), Box<dyn std::error::Error>> {
        let args = [DynSolValue::Uint(amount, 256)];
        let _result = self
            .contract
            .function("initiateEscrow", &args)
            .unwrap()
            .call()
            .await?;
        Ok(())
    }

    // Caller must be owner
    pub async fn release_escrow(&self) -> Result<(), Box<dyn std::error::Error>> {
        let args = [];
        let _result = self
            .contract
            .function("releaseEscrow", &args)
            .unwrap()
            .call()
            .await?;
        Ok(())
    }

    // Caller must be owner
    pub async fn refund_escrow(&self) -> Result<(), Box<dyn std::error::Error>> {
        let args = [];
        let _result = self
            .contract
            .function("refundEscrow", &args)
            .unwrap()
            .call()
            .await?;
        Ok(())
    }

    // ERC20 Escrow interface

    pub async fn initiate_erc20_escrow(
        &self,
        amount: U256,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let args = [DynSolValue::Uint(amount, 256)];
        let _result = self
            .contract
            .function("initiateERC20Escrow", &args)
            .unwrap()
            .call()
            .await?;
        Ok(())
    }

    // Caller must be owner
    pub async fn release_erc20_escrow(&self) -> Result<(), Box<dyn std::error::Error>> {
        let args = [];
        let _result = self
            .contract
            .function("releaseERC20Escrow", &args)
            .unwrap()
            .call()
            .await?;
        Ok(())
    }

    // Caller must be owner
    pub async fn refund_erc20_escrow(&self) -> Result<(), Box<dyn std::error::Error>> {
        let args = [];
        let _result = self
            .contract
            .function("refundERC20Escrow", &args)
            .unwrap()
            .call()
            .await?;
        Ok(())
    }

    // ERC721 Escrow interface

    pub async fn initiate_erc721_escrow(&self) -> Result<(), Box<dyn std::error::Error>> {
        let args = [];
        let _result = self
            .contract
            .function("initiateERC721Escrow", &args)
            .unwrap()
            .call()
            .await?;
        Ok(())
    }

    // Caller must be owner
    pub async fn release_erc721_escrow(&self) -> Result<(), Box<dyn std::error::Error>> {
        let args = [];
        let _result = self
            .contract
            .function("releaseERC721Escrow", &args)
            .unwrap()
            .call()
            .await?;
        Ok(())
    }

    // Caller must be owner
    pub async fn refund_erc721_escrow(&self) -> Result<(), Box<dyn std::error::Error>> {
        let args = [];
        let _result = self
            .contract
            .function("refundERC721Escrow", &args)
            .unwrap()
            .call()
            .await?;
        Ok(())
    }

    // Administrative interface

    pub async fn set_identity_manager(
        &self,
        identity_manager: Address,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let args = [DynSolValue::Address(identity_manager)];
        let _result = self
            .contract
            .function("setIdentityManager", &args)
            .unwrap()
            .call()
            .await?;
        Ok(())
    }

    pub async fn set_eth_escrow(
        &self,
        eth_escrow: Address,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let args = [DynSolValue::Address(eth_escrow)];
        let _result = self
            .contract
            .function("setEthEscrow", &args)
            .unwrap()
            .call()
            .await?;
        Ok(())
    }

    pub async fn set_erc20_escrow(
        &self,
        erc20_escrow: Address,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let args = [DynSolValue::Address(erc20_escrow)];
        let _result = self
            .contract
            .function("setERC20Escrow", &args)
            .unwrap()
            .call()
            .await?;
        Ok(())
    }

    pub async fn set_erc721_escrow(
        &self,
        erc721_escrow: Address,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let args = [DynSolValue::Address(erc721_escrow)];
        let _result = self
            .contract
            .function("setERC721Escrow", &args)
            .unwrap()
            .call()
            .await?;
        Ok(())
    }
}
