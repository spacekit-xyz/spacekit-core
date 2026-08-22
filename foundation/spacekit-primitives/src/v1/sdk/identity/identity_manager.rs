// src/v1/sdk/identity/identity_manager.rs

use alloy::contract::{ContractInstance, Interface};
use alloy::dyn_abi::DynSolValue;
use alloy::json_abi::JsonAbi;
use alloy::network::Ethereum;
use alloy::providers::Provider;
use alloy::signers::Signature;
use alloy::signers::SignerSync;

use alloy::primitives::{keccak256, Address, U256};
use alloy::rpc::types::TransactionReceipt;

use std::str::FromStr;
use std::sync::Arc;

use crate::v1::identity::DIDIdentity;
use alloy::primitives::Signature as AlloySignature;

/// Identity Manager Client
pub struct IdentityManagerClient<P: Provider> {
    pub contract: ContractInstance<Arc<P>, Ethereum>,
    pub wallet: Box<dyn SignerSync>,
}

impl<P: Provider> IdentityManagerClient<P> {
    pub fn new(address: Address, provider: Arc<P>, wallet: Box<dyn SignerSync>) -> Self {
        let abi_str = r#"[ { "inputs": [], "name": "InvalidInitialization", "type": "error" }, { "inputs": [], "name": "NotInitializing", "type": "error" }, { "inputs": [ { "internalType": "address", "name": "owner", "type": "address" } ], "name": "OwnableInvalidOwner", "type": "error" }, { "inputs": [ { "internalType": "address", "name": "account", "type": "address" } ], "name": "OwnableUnauthorizedAccount", "type": "error" }, { "anonymous": false, "inputs": [ { "indexed": true, "internalType": "address", "name": "did", "type": "address" }, { "indexed": false, "internalType": "string", "name": "issuer", "type": "string" }, { "indexed": false, "internalType": "string", "name": "claim", "type": "string" }, { "indexed": false, "internalType": "uint256", "name": "issuedAt", "type": "uint256" } ], "name": "AttestationAdded", "type": "event" }, { "anonymous": false, "inputs": [ { "indexed": true, "internalType": "address", "name": "did", "type": "address" }, { "indexed": false, "internalType": "address", "name": "delegate", "type": "address" }, { "indexed": false, "internalType": "bool", "name": "enabled", "type": "bool" } ], "name": "DelegateUpdated", "type": "event" }, { "anonymous": false, "inputs": [ { "indexed": true, "internalType": "address", "name": "did", "type": "address" } ], "name": "IdentityUpdated", "type": "event" }, { "anonymous": false, "inputs": [ { "indexed": false, "internalType": "uint64", "name": "version", "type": "uint64" } ], "name": "Initialized", "type": "event" }, { "anonymous": false, "inputs": [ { "indexed": true, "internalType": "address", "name": "previousOwner", "type": "address" }, { "indexed": true, "internalType": "address", "name": "newOwner", "type": "address" } ], "name": "OwnershipTransferred", "type": "event" }, { "inputs": [ { "internalType": "address", "name": "did", "type": "address" }, { "internalType": "string", "name": "issuer", "type": "string" }, { "internalType": "string", "name": "claim", "type": "string" } ], "name": "addAttestation", "outputs": [], "stateMutability": "nonpayable", "type": "function" }, { "inputs": [ { "internalType": "address", "name": "did", "type": "address" }, { "internalType": "address", "name": "delegate", "type": "address" } ], "name": "addDelegate", "outputs": [], "stateMutability": "nonpayable", "type": "function" }, { "inputs": [ { "internalType": "address", "name": "did", "type": "address" } ], "name": "getAttestations", "outputs": [ { "components": [ { "internalType": "string", "name": "issuer", "type": "string" }, { "internalType": "string", "name": "claim", "type": "string" }, { "internalType": "uint256", "name": "issuedAt", "type": "uint256" } ], "internalType": "struct IdentityManager.Attestation[]", "name": "", "type": "tuple[]" } ], "stateMutability": "view", "type": "function" }, { "inputs": [ { "internalType": "address", "name": "", "type": "address" } ], "name": "identities", "outputs": [ { "internalType": "address", "name": "owner", "type": "address" }, { "internalType": "address", "name": "claimsContract", "type": "address" }, { "internalType": "string", "name": "didDocument", "type": "string" } ], "stateMutability": "view", "type": "function" }, { "inputs": [], "name": "initialize", "outputs": [], "stateMutability": "nonpayable", "type": "function" }, { "inputs": [ { "internalType": "address", "name": "did", "type": "address" }, { "internalType": "address", "name": "user", "type": "address" } ], "name": "isOwnerOrDelegate", "outputs": [ { "internalType": "bool", "name": "", "type": "bool" } ], "stateMutability": "view", "type": "function" }, { "inputs": [], "name": "owner", "outputs": [ { "internalType": "address", "name": "", "type": "address" } ], "stateMutability": "view", "type": "function" }, { "inputs": [ { "internalType": "address", "name": "did", "type": "address" }, { "internalType": "address", "name": "owner", "type": "address" }, { "internalType": "string", "name": "documentHash", "type": "string" } ], "name": "registerIdentity", "outputs": [], "stateMutability": "nonpayable", "type": "function" }, { "inputs": [ { "internalType": "address", "name": "did", "type": "address" }, { "internalType": "address", "name": "delegate", "type": "address" } ], "name": "removeDelegate", "outputs": [], "stateMutability": "nonpayable", "type": "function" }, { "inputs": [], "name": "renounceOwnership", "outputs": [], "stateMutability": "nonpayable", "type": "function" }, { "inputs": [ { "internalType": "address", "name": "did", "type": "address" }, { "internalType": "string", "name": "documentHash", "type": "string" } ], "name": "setDIDDocument", "outputs": [], "stateMutability": "nonpayable", "type": "function" }, { "inputs": [ { "internalType": "address", "name": "newOwner", "type": "address" } ], "name": "transferOwnership", "outputs": [], "stateMutability": "nonpayable", "type": "function" }, { "inputs": [ { "internalType": "address", "name": "did", "type": "address" }, { "internalType": "string", "name": "issuer", "type": "string" }, { "internalType": "string", "name": "claim", "type": "string" } ], "name": "verifyAttestation", "outputs": [ { "internalType": "bool", "name": "", "type": "bool" } ], "stateMutability": "view", "type": "function" } ]"#;
        let abi = serde_json::from_str::<JsonAbi>(abi_str).unwrap();
        let contract = ContractInstance::new(address, Arc::clone(&provider), Interface::new(abi));
        Self { contract, wallet }
    }

    pub async fn load_identity(
        &self,
        did: Address,
    ) -> Result<DIDIdentity, Box<dyn std::error::Error>> {
        let args = [DynSolValue::Address(did)];
        let result = self
            .contract
            .function("identities", &args)
            .unwrap()
            .call()
            .await?;

        let owner = result[0].as_address().unwrap();
        let claims_contract = result[1].as_address().unwrap();
        let did_document = result[2].as_str().unwrap();

        Ok(DIDIdentity {
            address: did,
            owner,
            claims_contract,
            did_document: did_document.to_string(),
        })
    }

    pub async fn register_identity(
        &self,
        did: Address,
        owner: Address,
        document_hash: String,
    ) -> Result<TransactionReceipt, Box<dyn std::error::Error>> {
        let args = [
            DynSolValue::Address(did),
            DynSolValue::Address(owner),
            DynSolValue::String(document_hash),
        ];
        let tx = self.contract.function("registerIdentity", &args).unwrap();
        let pending_tx = tx.send().await?;
        Ok(pending_tx
            .with_required_confirmations(1)
            .get_receipt()
            .await?)
    }

    pub async fn set_did_document(
        &self,
        did: Address,
        document_hash: String,
    ) -> Result<TransactionReceipt, Box<dyn std::error::Error>> {
        let args = [
            DynSolValue::Address(did),
            DynSolValue::String(document_hash),
        ];
        let tx = self.contract.function("setDIDDocument", &args).unwrap();
        let pending_tx = tx.send().await?;
        Ok(pending_tx
            .with_required_confirmations(1)
            .get_receipt()
            .await?)
    }

    pub async fn add_delegate(
        &self,
        did: Address,
        delegate: Address,
    ) -> Result<TransactionReceipt, Box<dyn std::error::Error>> {
        let args = [DynSolValue::Address(did), DynSolValue::Address(delegate)];
        let tx = self.contract.function("addDelegate", &args).unwrap();
        let pending_tx = tx.send().await?;
        Ok(pending_tx
            .with_required_confirmations(1)
            .get_receipt()
            .await?)
    }

    pub async fn remove_delegate(
        &self,
        did: Address,
        delegate: Address,
    ) -> Result<TransactionReceipt, Box<dyn std::error::Error>> {
        let args = [DynSolValue::Address(did), DynSolValue::Address(delegate)];
        let tx = self.contract.function("removeDelegate", &args).unwrap();
        let pending_tx = tx.send().await?;
        Ok(pending_tx
            .with_required_confirmations(1)
            .get_receipt()
            .await?)
    }

    pub async fn is_owner_or_delegate(
        &self,
        did: Address,
        user: Address,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let args = [DynSolValue::Address(did), DynSolValue::Address(user)];
        let result = self
            .contract
            .function("isOwnerOrDelegate", &args)
            .unwrap()
            .call()
            .await?;
        Ok(result[0].as_bool().unwrap())
    }
    pub async fn add_attestation(
        &self,
        did: Address,
        issuer: String,
        claim: String,
    ) -> Result<TransactionReceipt, Box<dyn std::error::Error>> {
        let args = [
            DynSolValue::Address(did),
            DynSolValue::String(issuer),
            DynSolValue::String(claim),
        ];
        let tx = self.contract.function("addAttestation", &args).unwrap();
        let pending_tx = tx.send().await?;
        Ok(pending_tx
            .with_required_confirmations(1)
            .get_receipt()
            .await?)
    }

    pub async fn get_attestations(
        &self,
        did: Address,
    ) -> Result<Vec<(String, String, U256)>, Box<dyn std::error::Error>> {
        let args = [DynSolValue::Address(did)];
        let result = self
            .contract
            .function("getAttestations", &args)
            .unwrap()
            .call()
            .await?;

        Ok(result[0]
            .as_tuple()
            .unwrap()
            .iter()
            .map(|tuple| {
                let values = tuple.as_tuple().unwrap();
                (
                    values[0].as_str().unwrap().to_string(),
                    values[1].as_str().unwrap().to_string(),
                    values[2].as_uint().unwrap().0,
                )
            })
            .collect())
    }

    pub async fn verify_attestation(
        &self,
        did: Address,
        issuer: String,
        claim: String,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let args = [
            DynSolValue::Address(did),
            DynSolValue::String(issuer),
            DynSolValue::String(claim),
        ];
        let result = self
            .contract
            .function("verifyAttestation", &args)
            .unwrap()
            .call()
            .await?;
        Ok(result[0].as_bool().unwrap())
    }

    pub async fn sign_message(
        &self,
        message: &[u8],
    ) -> Result<Signature, Box<dyn std::error::Error>> {
        Ok(self.wallet.sign_message_sync(message)?)
    }

    pub fn verify_signature(
        &self,
        message: &[u8],
        signature: &Signature,
        expected_signer: Address,
    ) -> bool {
        let msg = keccak256(message);
        if let Ok(sig) = AlloySignature::from_str(&hex::encode(signature.as_bytes())) {
            // Use recover_address_from_prehash to get the signer's address and compare
            match sig.recover_address_from_prehash(&msg) {
                Ok(recovered_signer) => recovered_signer == expected_signer,
                Err(_) => false,
            }
        } else {
            false
        }
    }
}
