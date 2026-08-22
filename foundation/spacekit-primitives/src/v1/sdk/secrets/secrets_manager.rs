use alloy::contract::{ContractInstance, Interface};
use alloy::dyn_abi::DynSolValue;
use alloy::json_abi::JsonAbi;
use alloy::network::Ethereum;
use alloy::providers::Provider;
use alloy::signers::SignerSync;

use alloy::primitives::{Address, U256};
use std::sync::Arc;

// use crate::v1::identity::DIDIdentity;

pub struct SecretsManagerClient<P: Provider> {
    pub contract: ContractInstance<Arc<P>, Ethereum>,
    pub wallet: Box<dyn SignerSync>,
}

impl<P: Provider> SecretsManagerClient<P> {
    pub fn new(address: Address, provider: Arc<P>, wallet: Box<dyn SignerSync>) -> Self {
        let abi_str = r#"[ { "inputs": [ { "internalType": "uint256", "name": "fee_", "type": "uint256" }, { "internalType": "address", "name": "didRegistryAddress_", "type": "address" } ], "stateMutability": "nonpayable", "type": "constructor" }, { "anonymous": false, "inputs": [ { "indexed": true, "internalType": "address", "name": "owner", "type": "address" }, { "indexed": true, "internalType": "address", "name": "deployed", "type": "address" } ], "name": "SpaceAdded", "type": "event" }, { "anonymous": false, "inputs": [ { "indexed": true, "internalType": "address", "name": "owner", "type": "address" }, { "indexed": true, "internalType": "address", "name": "subOwner", "type": "address" }, { "indexed": true, "internalType": "address", "name": "deployed", "type": "address" } ], "name": "SubSpaceAdded", "type": "event" }, { "inputs": [ { "internalType": "address", "name": "userDID", "type": "address" } ], "name": "addSpace", "outputs": [], "stateMutability": "nonpayable", "type": "function" }, { "inputs": [ { "internalType": "address", "name": "userDID", "type": "address" }, { "internalType": "address", "name": "subUserDID", "type": "address" } ], "name": "addSubSpace", "outputs": [], "stateMutability": "nonpayable", "type": "function" }, { "inputs": [ { "internalType": "address", "name": "userDID", "type": "address" } ], "name": "disableSpace", "outputs": [], "stateMutability": "nonpayable", "type": "function" }, { "inputs": [], "name": "getFee", "outputs": [ { "internalType": "uint256", "name": "", "type": "uint256" } ], "stateMutability": "view", "type": "function" }, { "inputs": [ { "internalType": "address", "name": "userDID", "type": "address" } ], "name": "getSpace", "outputs": [ { "internalType": "address", "name": "", "type": "address" } ], "stateMutability": "view", "type": "function" }, { "inputs": [ { "internalType": "address", "name": "userDID", "type": "address" } ], "name": "getSubSpaces", "outputs": [ { "internalType": "address[]", "name": "", "type": "address[]" } ], "stateMutability": "view", "type": "function" }, { "inputs": [], "name": "owner", "outputs": [ { "internalType": "address", "name": "", "type": "address" } ], "stateMutability": "view", "type": "function" }, { "inputs": [ { "internalType": "address", "name": "", "type": "address" } ], "name": "spaces", "outputs": [ { "internalType": "address", "name": "owner", "type": "address" }, { "internalType": "address", "name": "deployed", "type": "address" }, { "internalType": "bool", "name": "enabled", "type": "bool" } ], "stateMutability": "view", "type": "function" }, { "inputs": [ { "internalType": "address", "name": "", "type": "address" }, { "internalType": "uint256", "name": "", "type": "uint256" } ], "name": "subspaces", "outputs": [ { "internalType": "address", "name": "", "type": "address" } ], "stateMutability": "view", "type": "function" } ]"#;
        let abi = serde_json::from_str::<JsonAbi>(abi_str).unwrap();
        let contract = ContractInstance::new(address, Arc::clone(&provider), Interface::new(abi));
        Self { contract, wallet }
    }

    // Fees

    pub async fn get_fee(&self) -> Result<U256, Box<dyn std::error::Error>> {
        let result = self
            .contract
            .function("getFee", &[])
            .unwrap()
            .call()
            .await?;
        let fee = result
            .get(0)
            .ok_or("No fee value returned")?
            .as_uint()
            .ok_or("Could not convert to U256")?;
        Ok(fee.0)
    }

    // Spaces

    pub async fn get_space(
        &self,
        user_did: Address,
    ) -> Result<Address, Box<dyn std::error::Error>> {
        let args = [DynSolValue::Address(user_did)];
        let result = self
            .contract
            .function("getSpace", &args)
            .unwrap()
            .call()
            .await?;
        let address = result
            .get(0)
            .ok_or("No address value returned")?
            .as_address()
            .ok_or("Could not convert to Address")?;
        Ok(address)
    }

    pub async fn get_subspaces(
        &self,
        user_did: Address,
    ) -> Result<Vec<Address>, Box<dyn std::error::Error>> {
        let args = [DynSolValue::Address(user_did)];
        let result = self
            .contract
            .function("getSubSpaces", &args)
            .unwrap()
            .call()
            .await?;
        let addresses = result
            .get(0)
            .ok_or("No address array returned")?
            .as_array()
            .ok_or("Could not convert to array")?
            .iter()
            .map(|v| v.as_address().ok_or("Invalid address in array"))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(addresses)
    }

    pub async fn add_space(&self, user_did: Address) -> Result<(), Box<dyn std::error::Error>> {
        let args = [DynSolValue::Address(user_did)];
        let _ = self
            .contract
            .function("addSpace", &args)
            .unwrap()
            .send()
            .await?;
        Ok(())
    }

    pub async fn add_subspace(
        &self,
        user_did: Address,
        sub_user_did: Address,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let args = [
            DynSolValue::Address(user_did),
            DynSolValue::Address(sub_user_did),
        ];
        let _ = self
            .contract
            .function("addSubSpace", &args)
            .unwrap()
            .send()
            .await?;
        Ok(())
    }

    pub async fn disable_space(&self, user_did: Address) -> Result<(), Box<dyn std::error::Error>> {
        let args = [DynSolValue::Address(user_did)];
        let _ = self
            .contract
            .function("disableSpace", &args)
            .unwrap()
            .send()
            .await?;
        Ok(())
    }
}
