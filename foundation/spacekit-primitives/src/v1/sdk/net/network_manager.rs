use alloy::contract::{ContractInstance, Interface};
use alloy::dyn_abi::DynSolValue;
use alloy::json_abi::JsonAbi;
use alloy::network::Ethereum;
use alloy::providers::Provider;
use alloy::signers::SignerSync;

use alloy::primitives::{Address, Bytes, FixedBytes};
use std::sync::Arc;

use crate::v1::network::NetworkService;

pub struct NetworkManagerClient<P: Provider> {
    pub contract: ContractInstance<Arc<P>, Ethereum>,
    pub wallet: Box<dyn SignerSync>,
}

impl<P: Provider> NetworkManagerClient<P> {
    pub fn new(address: Address, provider: Arc<P>, wallet: Box<dyn SignerSync>) -> Self {
        let abi_str = r#"[ { "inputs": [], "name": "InvalidInitialization", "type": "error" }, { "inputs": [], "name": "NotInitializing", "type": "error" }, { "inputs": [ { "internalType": "address", "name": "owner", "type": "address" } ], "name": "OwnableInvalidOwner", "type": "error" }, { "inputs": [ { "internalType": "address", "name": "account", "type": "address" } ], "name": "OwnableUnauthorizedAccount", "type": "error" }, { "anonymous": false, "inputs": [ { "indexed": false, "internalType": "uint64", "name": "version", "type": "uint64" } ], "name": "Initialized", "type": "event" }, { "anonymous": false, "inputs": [ { "indexed": true, "internalType": "address", "name": "provider", "type": "address" }, { "indexed": false, "internalType": "string", "name": "serviceDetails", "type": "string" } ], "name": "NetworkAdded", "type": "event" }, { "anonymous": false, "inputs": [ { "indexed": true, "internalType": "address", "name": "provider", "type": "address" }, { "indexed": false, "internalType": "string", "name": "serviceDetails", "type": "string" } ], "name": "NetworkRemoved", "type": "event" }, { "anonymous": false, "inputs": [ { "indexed": true, "internalType": "address", "name": "provider", "type": "address" }, { "indexed": false, "internalType": "string", "name": "oldServiceDetails", "type": "string" }, { "indexed": false, "internalType": "string", "name": "newServiceDetails", "type": "string" } ], "name": "NetworkUpdated", "type": "event" }, { "anonymous": false, "inputs": [ { "indexed": true, "internalType": "address", "name": "previousOwner", "type": "address" }, { "indexed": true, "internalType": "address", "name": "newOwner", "type": "address" } ], "name": "OwnershipTransferred", "type": "event" }, { "inputs": [ { "internalType": "address", "name": "provider", "type": "address" }, { "internalType": "string", "name": "serviceDetails", "type": "string" } ], "name": "addNetworkService", "outputs": [], "stateMutability": "nonpayable", "type": "function" }, { "inputs": [ { "internalType": "address", "name": "provider", "type": "address" } ], "name": "getNetworkService", "outputs": [ { "components": [ { "internalType": "address", "name": "owner", "type": "address" }, { "internalType": "string", "name": "serviceDetails", "type": "string" }, { "internalType": "bool", "name": "isActive", "type": "bool" } ], "internalType": "struct NetworkManager.NetworkService", "name": "", "type": "tuple" } ], "stateMutability": "view", "type": "function" }, { "inputs": [], "name": "getServiceProviders", "outputs": [ { "internalType": "address[]", "name": "", "type": "address[]" } ], "stateMutability": "view", "type": "function" }, { "inputs": [ { "internalType": "address", "name": "didRegistryAddress_", "type": "address" } ], "name": "initialize", "outputs": [], "stateMutability": "nonpayable", "type": "function" }, { "inputs": [ { "internalType": "address", "name": "provider", "type": "address" } ], "name": "isServiceProvider", "outputs": [ { "internalType": "bool", "name": "", "type": "bool" } ], "stateMutability": "view", "type": "function" }, { "inputs": [ { "internalType": "address", "name": "", "type": "address" } ], "name": "networkServices", "outputs": [ { "internalType": "address", "name": "owner", "type": "address" }, { "internalType": "string", "name": "serviceDetails", "type": "string" }, { "internalType": "bool", "name": "isActive", "type": "bool" } ], "stateMutability": "view", "type": "function" }, { "inputs": [], "name": "owner", "outputs": [ { "internalType": "address", "name": "", "type": "address" } ], "stateMutability": "view", "type": "function" }, { "inputs": [ { "internalType": "address", "name": "provider", "type": "address" } ], "name": "removeNetworkService", "outputs": [], "stateMutability": "nonpayable", "type": "function" }, { "inputs": [], "name": "renounceOwnership", "outputs": [], "stateMutability": "nonpayable", "type": "function" }, { "inputs": [ { "internalType": "uint256", "name": "", "type": "uint256" } ], "name": "serviceProviders", "outputs": [ { "internalType": "address", "name": "", "type": "address" } ], "stateMutability": "view", "type": "function" }, { "inputs": [ { "internalType": "address", "name": "newOwner", "type": "address" } ], "name": "transferOwnership", "outputs": [], "stateMutability": "nonpayable", "type": "function" }, { "inputs": [ { "internalType": "address", "name": "provider", "type": "address" }, { "internalType": "string", "name": "newServiceDetails", "type": "string" } ], "name": "updateNetworkService", "outputs": [], "stateMutability": "nonpayable", "type": "function" } ]"#;
        let abi = serde_json::from_str::<JsonAbi>(abi_str).unwrap();
        let contract = ContractInstance::new(address, Arc::clone(&provider), Interface::new(abi));
        Self { contract, wallet }
    }

    pub async fn add_network_service(
        &self,
        provider: Address,
        service_details: String,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let args = [
            DynSolValue::Address(provider),
            DynSolValue::String(service_details),
        ];
        let _result = self
            .contract
            .function("addNetworkService", &args)
            .unwrap()
            .call()
            .await?;
        Ok(())
    }

    pub async fn get_network_service(
        &self,
        provider: Address,
    ) -> Result<NetworkService, Box<dyn std::error::Error>> {
        let args = [DynSolValue::Address(provider)];
        let result = self
            .contract
            .function("getNetworkService", &args)
            .unwrap()
            .call()
            .await?;

        let tuple = result[0].as_tuple().unwrap();
        Ok(NetworkService {
            address: tuple[0].as_address().unwrap(),
            service_details: tuple[1].as_str().unwrap().to_string(),
            is_active: tuple[2].as_bool().unwrap(),
        })
    }

    pub async fn update_network_service(
        &self,
        provider: Address,
        service_details: String,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let args = [
            DynSolValue::Address(provider),
            DynSolValue::String(service_details),
        ];
        let _result = self
            .contract
            .function("updateNetworkService", &args)
            .unwrap()
            .call()
            .await?;
        Ok(())
    }

    pub async fn remove_network_service(
        &self,
        provider: Address,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let args = [DynSolValue::Address(provider)];
        let _result = self
            .contract
            .function("removeNetworkService", &args)
            .unwrap()
            .call()
            .await?;
        Ok(())
    }

    pub async fn is_service_provider(
        &self,
        provider: Address,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let args = [DynSolValue::Address(provider)];
        let result = self
            .contract
            .function("isServiceProvider", &args)
            .unwrap()
            .call()
            .await?;
        Ok(result[0].as_bool().unwrap())
    }

    pub async fn get_service_providers(&self) -> Result<Vec<Address>, Box<dyn std::error::Error>> {
        let args = [];
        let result = self
            .contract
            .function("getServiceProviders", &args)
            .unwrap()
            .call()
            .await?;
        Ok(result[0]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_address().unwrap())
            .collect())
    }
}
