use alloy::contract::{ContractInstance, Interface};
use alloy::dyn_abi::DynSolValue;
use alloy::json_abi::JsonAbi;
use alloy::providers::Provider;
use alloy::signers::SignerSync;
use alloy::transports::Transport;

use alloy::network::Ethereum;

use alloy::primitives::{Address, U256};

// use std::str::FromStr;
use std::sync::Arc;
// TODO: Migrate to the WASM Secrets Space Manager Contract
pub struct SecretsSpaceManagerClient<P: Provider> {
    pub contract: ContractInstance<Arc<P>, Ethereum>,
    pub wallet: Box<dyn SignerSync>,
}

impl<P: Provider> SecretsSpaceManagerClient<P> {
    pub fn new(address: Address, provider: Arc<P>, wallet: Box<dyn SignerSync>) -> Self {
        let abi_str = r#"[ { "inputs": [ { "internalType": "uint256", "name": "fee_", "type": "uint256" } ], "stateMutability": "nonpayable", "type": "constructor" }, { "inputs": [], "name": "AccessControlBadConfirmation", "type": "error" }, { "inputs": [ { "internalType": "address", "name": "account", "type": "address" }, { "internalType": "bytes32", "name": "neededRole", "type": "bytes32" } ], "name": "AccessControlUnauthorizedAccount", "type": "error" }, { "anonymous": false, "inputs": [ { "indexed": false, "internalType": "address", "name": "delegate", "type": "address" }, { "indexed": false, "internalType": "bytes", "name": "identifier", "type": "bytes" } ], "name": "DelegateAuthorized", "type": "event" }, { "anonymous": false, "inputs": [ { "indexed": false, "internalType": "address", "name": "delegate", "type": "address" }, { "indexed": false, "internalType": "bytes", "name": "identifier", "type": "bytes" } ], "name": "DelegateRevoked", "type": "event" }, { "anonymous": false, "inputs": [ { "indexed": false, "internalType": "uint256", "name": "oldSecretFee", "type": "uint256" }, { "indexed": false, "internalType": "uint256", "name": "newSecretFee", "type": "uint256" } ], "name": "FeesAdjusted", "type": "event" }, { "anonymous": false, "inputs": [ { "indexed": true, "internalType": "bytes32", "name": "role", "type": "bytes32" }, { "indexed": true, "internalType": "bytes32", "name": "previousAdminRole", "type": "bytes32" }, { "indexed": true, "internalType": "bytes32", "name": "newAdminRole", "type": "bytes32" } ], "name": "RoleAdminChanged", "type": "event" }, { "anonymous": false, "inputs": [ { "indexed": true, "internalType": "bytes32", "name": "role", "type": "bytes32" }, { "indexed": true, "internalType": "address", "name": "account", "type": "address" }, { "indexed": true, "internalType": "address", "name": "sender", "type": "address" } ], "name": "RoleGranted", "type": "event" }, { "anonymous": false, "inputs": [ { "indexed": true, "internalType": "bytes32", "name": "role", "type": "bytes32" }, { "indexed": true, "internalType": "address", "name": "account", "type": "address" }, { "indexed": true, "internalType": "address", "name": "sender", "type": "address" } ], "name": "RoleRevoked", "type": "event" }, { "anonymous": false, "inputs": [ { "indexed": false, "internalType": "address", "name": "accessedBy", "type": "address" }, { "indexed": false, "internalType": "bytes", "name": "identifier", "type": "bytes" } ], "name": "SecretAccessed", "type": "event" }, { "anonymous": false, "inputs": [ { "indexed": true, "internalType": "bytes", "name": "identifier", "type": "bytes" } ], "name": "SecretAdded", "type": "event" }, { "inputs": [], "name": "ADMIN_ROLE", "outputs": [ { "internalType": "bytes32", "name": "", "type": "bytes32" } ], "stateMutability": "view", "type": "function" }, { "inputs": [], "name": "AUDITOR_ROLE", "outputs": [ { "internalType": "bytes32", "name": "", "type": "bytes32" } ], "stateMutability": "view", "type": "function" }, { "inputs": [], "name": "DEFAULT_ADMIN_ROLE", "outputs": [ { "internalType": "bytes32", "name": "", "type": "bytes32" } ], "stateMutability": "view", "type": "function" }, { "inputs": [ { "internalType": "bytes", "name": "identifier", "type": "bytes" }, { "internalType": "bytes", "name": "secretValue", "type": "bytes" } ], "name": "addSecret", "outputs": [], "stateMutability": "payable", "type": "function" }, { "inputs": [ { "internalType": "uint256", "name": "newFee", "type": "uint256" } ], "name": "adjustFees", "outputs": [], "stateMutability": "nonpayable", "type": "function" }, { "inputs": [ { "internalType": "address", "name": "delegate", "type": "address" }, { "internalType": "bytes", "name": "identifier", "type": "bytes" } ], "name": "authorizeDelegate", "outputs": [], "stateMutability": "nonpayable", "type": "function" }, { "inputs": [ { "internalType": "address", "name": "", "type": "address" }, { "internalType": "bytes", "name": "", "type": "bytes" } ], "name": "delegatePermissions", "outputs": [ { "internalType": "bool", "name": "", "type": "bool" } ], "stateMutability": "view", "type": "function" }, { "inputs": [ { "internalType": "bytes", "name": "identifier", "type": "bytes" } ], "name": "deleteSecret", "outputs": [], "stateMutability": "nonpayable", "type": "function" }, { "inputs": [], "name": "feesCollected", "outputs": [ { "internalType": "uint256", "name": "", "type": "uint256" } ], "stateMutability": "view", "type": "function" }, { "inputs": [], "name": "getFee", "outputs": [ { "internalType": "uint256", "name": "", "type": "uint256" } ], "stateMutability": "view", "type": "function" }, { "inputs": [ { "internalType": "bytes32", "name": "role", "type": "bytes32" } ], "name": "getRoleAdmin", "outputs": [ { "internalType": "bytes32", "name": "", "type": "bytes32" } ], "stateMutability": "view", "type": "function" }, { "inputs": [ { "internalType": "bytes", "name": "identifier", "type": "bytes" } ], "name": "getSecret", "outputs": [ { "internalType": "bytes", "name": "", "type": "bytes" } ], "stateMutability": "nonpayable", "type": "function" }, { "inputs": [ { "internalType": "bytes32", "name": "role", "type": "bytes32" }, { "internalType": "address", "name": "account", "type": "address" } ], "name": "grantRole", "outputs": [], "stateMutability": "nonpayable", "type": "function" }, { "inputs": [ { "internalType": "bytes32", "name": "role", "type": "bytes32" }, { "internalType": "address", "name": "account", "type": "address" } ], "name": "hasRole", "outputs": [ { "internalType": "bool", "name": "", "type": "bool" } ], "stateMutability": "view", "type": "function" }, { "inputs": [], "name": "owner", "outputs": [ { "internalType": "address", "name": "", "type": "address" } ], "stateMutability": "view", "type": "function" }, { "inputs": [ { "internalType": "bytes32", "name": "role", "type": "bytes32" }, { "internalType": "address", "name": "callerConfirmation", "type": "address" } ], "name": "renounceRole", "outputs": [], "stateMutability": "nonpayable", "type": "function" }, { "inputs": [ { "internalType": "address", "name": "delegate", "type": "address" }, { "internalType": "bytes", "name": "identifier", "type": "bytes" } ], "name": "revokeDelegate", "outputs": [], "stateMutability": "nonpayable", "type": "function" }, { "inputs": [ { "internalType": "bytes32", "name": "role", "type": "bytes32" }, { "internalType": "address", "name": "account", "type": "address" } ], "name": "revokeRole", "outputs": [], "stateMutability": "nonpayable", "type": "function" }, { "inputs": [ { "internalType": "bytes4", "name": "interfaceId", "type": "bytes4" } ], "name": "supportsInterface", "outputs": [ { "internalType": "bool", "name": "", "type": "bool" } ], "stateMutability": "view", "type": "function" }, { "inputs": [ { "internalType": "address payable", "name": "recipient", "type": "address" }, { "internalType": "uint256", "name": "amount", "type": "uint256" } ], "name": "withdrawFees", "outputs": [], "stateMutability": "nonpayable", "type": "function" } ]"#;
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

    pub async fn adjust_fees(&self, new_fee: U256) -> Result<(), Box<dyn std::error::Error>> {
        let args = [DynSolValue::Uint(new_fee, 256)];
        let _ = self
            .contract
            .function("adjustFees", &args)
            .unwrap()
            .send()
            .await?;
        Ok(())
    }

    pub async fn fees_collected(&self) -> Result<U256, Box<dyn std::error::Error>> {
        let result = self
            .contract
            .function("feesCollected", &[])
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

    pub async fn withdraw_fees(
        &self,
        recipient: Address,
        amount: U256,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let args = [
            DynSolValue::Address(recipient),
            DynSolValue::Uint(amount, 256),
        ];
        let _ = self
            .contract
            .function("withdrawFees", &args)
            .unwrap()
            .send()
            .await?;
        Ok(())
    }

    // Secrets

    pub async fn add_secret(
        &self,
        identifier: &str,
        secret_value: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let args = [
            DynSolValue::Bytes(identifier.as_bytes().to_vec()),
            DynSolValue::Bytes(secret_value.as_bytes().to_vec()),
        ];
        let _ = self
            .contract
            .function("addSecret", &args)
            .unwrap()
            .send()
            .await?;
        Ok(())
    }

    pub async fn get_secret(
        &self,
        identifier: &str,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let args = [DynSolValue::Bytes(identifier.as_bytes().to_vec())];
        let result = self
            .contract
            .function("getSecret", &args)
            .unwrap()
            .call()
            .await?;
        let secret = result
            .get(0)
            .ok_or("No secret value returned")?
            .as_bytes()
            .ok_or("Could not convert to bytes")?;
        Ok(secret.to_vec())
    }

    pub async fn delete_secret(&self, identifier: &str) -> Result<(), Box<dyn std::error::Error>> {
        let args = [DynSolValue::Bytes(identifier.as_bytes().to_vec())];
        let _ = self
            .contract
            .function("deleteSecret", &args)
            .unwrap()
            .send()
            .await?;
        Ok(())
    }

    // Delegates

    pub async fn authorize_delegate(
        &self,
        delegate: Address,
        identifier: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let args = [
            DynSolValue::Address(delegate),
            DynSolValue::Bytes(identifier.as_bytes().to_vec()),
        ];
        let _ = self
            .contract
            .function("authorizeDelegate", &args)
            .unwrap()
            .send()
            .await?;
        Ok(())
    }

    pub async fn revoke_delegate(
        &self,
        delegate: Address,
        identifier: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let args = [
            DynSolValue::Address(delegate),
            DynSolValue::Bytes(identifier.as_bytes().to_vec()),
        ];
        let _ = self
            .contract
            .function("revokeDelegate", &args)
            .unwrap()
            .send()
            .await?;
        Ok(())
    }
}
