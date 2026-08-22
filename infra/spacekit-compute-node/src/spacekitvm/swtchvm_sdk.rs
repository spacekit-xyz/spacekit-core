// SWTCHVM Client SDK - Easy interaction with SWTCHVM nodes
// Similar to web3.js/ethers.js for Ethereum

use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

// Re-import types from the node
use crate::{
    SwtchvmAccount, SwtchvmAddress, SwtchvmBlock, SwtchvmTransaction, TransactionSignature,
};

#[derive(Debug, Clone)]
pub struct SwtchvmClient {
    http_client: Client,
    node_url: String,
    default_gas_price: u128,
    default_gas_limit: u128,
}

impl SwtchvmClient {
    pub fn new(node_url: String) -> Self {
        Self {
            http_client: Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .unwrap(),
            node_url,
            default_gas_price: 1,
            default_gas_limit: 100_000,
        }
    }

    // Account operations
    pub async fn get_account(&self, address: &SwtchvmAddress) -> Result<Option<SwtchvmAccount>> {
        let url = format!(
            "{}/account/{}",
            self.node_url,
            hex::encode(address.as_bytes())
        );

        let response = self.http_client.get(&url).send().await?;

        if response.status().is_success() {
            let account: SwtchvmAccount = response.json().await?;
            Ok(Some(account))
        } else if response.status() == 404 {
            Ok(None)
        } else {
            Err(anyhow::anyhow!(
                "Failed to get account: {}",
                response.status()
            ))
        }
    }

    pub async fn get_balance(&self, address: &SwtchvmAddress) -> Result<u128> {
        match self.get_account(address).await? {
            Some(account) => Ok(account.balance),
            None => Ok(0),
        }
    }

    pub async fn get_nonce(&self, address: &SwtchvmAddress) -> Result<u64> {
        match self.get_account(address).await? {
            Some(account) => Ok(account.nonce),
            None => Ok(0),
        }
    }

    // Transaction operations
    pub async fn send_transaction(&self, tx: &SwtchvmTransaction) -> Result<String> {
        let url = format!("{}/transaction", self.node_url);

        let response = self.http_client.post(&url).json(tx).send().await?;

        if response.status().is_success() {
            let hash: String = response.json().await?;
            Ok(hash)
        } else {
            Err(anyhow::anyhow!(
                "Failed to send transaction: {}",
                response.status()
            ))
        }
    }

    // Smart contract operations
    pub async fn deploy_contract(
        &self,
        from: &SwtchvmAddress,
        bytecode: Vec<u8>,
        private_key: &[u8; 32],
    ) -> Result<ContractDeployment> {
        let nonce = self.get_nonce(from).await?;

        let tx = SwtchvmTransaction {
            from: *from,
            to: None,
            data: bytecode,
            gas_limit: self.default_gas_limit * 10, // More gas for deployment, TODO: Review this value
            gas_price: self.default_gas_price,
            value: 0,
            nonce,
            signature: self.sign_transaction(private_key, &[], nonce)?,
        };

        let tx_hash = self.send_transaction(&tx).await?;

        // Calculate contract address (simplified)
        let contract_address = self.calculate_contract_address(from, nonce);

        Ok(ContractDeployment {
            transaction_hash: tx_hash,
            contract_address,
            gas_used: 0, // Would be filled after mining
        })
    }

    pub async fn call_contract(
        &self,
        from: &SwtchvmAddress,
        contract: &SwtchvmAddress,
        data: Vec<u8>,
        value: u128,
        private_key: &[u8; 32],
    ) -> Result<String> {
        let nonce = self.get_nonce(from).await?;

        let tx = SwtchvmTransaction {
            from: *from,
            to: Some(*contract),
            data: data.clone(),
            gas_limit: self.default_gas_limit,
            gas_price: self.default_gas_price,
            value,
            nonce,
            signature: self.sign_transaction(private_key, &data, nonce)?,
        };

        self.send_transaction(&tx).await
    }

    // Block operations
    pub async fn get_block(&self, block_number: u64) -> Result<Option<SwtchvmBlock>> {
        let url = format!("{}/block/{}", self.node_url, block_number);

        let response = self.http_client.get(&url).send().await?;

        if response.status().is_success() {
            let block: SwtchvmBlock = response.json().await?;
            Ok(Some(block))
        } else if response.status() == 404 {
            Ok(None)
        } else {
            Err(anyhow::anyhow!(
                "Failed to get block: {}",
                response.status()
            ))
        }
    }

    pub async fn get_latest_block(&self) -> Result<SwtchvmBlock> {
        // This would need a separate endpoint in practice
        let mut block_number = 0;
        loop {
            match self.get_block(block_number).await? {
                Some(block) => {
                    // Check if next block exists
                    if self.get_block(block_number + 1).await?.is_none() {
                        return Ok(block);
                    }
                    block_number += 1;
                }
                None => {
                    if block_number == 0 {
                        return Err(anyhow::anyhow!("No blocks found"));
                    }
                    return self
                        .get_block(block_number - 1)
                        .await?
                        .ok_or_else(|| anyhow::anyhow!("Latest block not found"));
                }
            }
        }
    }

    // Mining (for testing)
    pub async fn mine_block(&self) -> Result<SwtchvmBlock> {
        let url = format!("{}/mine", self.node_url);

        let response = self.http_client.post(&url).send().await?;

        if response.status().is_success() {
            let block: SwtchvmBlock = response.json().await?;
            Ok(block)
        } else {
            Err(anyhow::anyhow!(
                "Failed to mine block: {}",
                response.status()
            ))
        }
    }

    // Utility functions
    fn sign_transaction(
        &self,
        private_key: &[u8; 32],
        data: &[u8],
        nonce: u64,
    ) -> Result<TransactionSignature> {
        // Simplified signature - in practice use proper cryptography
        Ok(TransactionSignature {
            v: 27,
            r: *private_key,
            s: [0u8; 32],
        })
    }

    fn calculate_contract_address(&self, creator: &SwtchvmAddress, nonce: u64) -> SwtchvmAddress {
        use sha3::{Digest, Keccak256};

        let mut hasher = Keccak256::new();
        hasher.update(creator.as_bytes());
        hasher.update(&nonce.to_be_bytes());
        let hash = hasher.finalize();

        let mut addr = [0u8; 20];
        addr.copy_from_slice(&hash[12..]);
        SwtchvmAddress::new(addr)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractDeployment {
    pub transaction_hash: String,
    pub contract_address: SwtchvmAddress,
    pub gas_used: u64,
}

// High-level contract interface
pub struct SwtchvmContract {
    client: SwtchvmClient,
    address: SwtchvmAddress,
    abi: ContractAbi,
}

impl SwtchvmContract {
    pub fn new(client: SwtchvmClient, address: SwtchvmAddress, abi: ContractAbi) -> Self {
        Self {
            client,
            address,
            abi,
        }
    }

    pub async fn call(
        &self,
        from: &SwtchvmAddress,
        function_name: &str,
        params: Vec<SwtchvmValue>,
        private_key: &[u8; 32],
    ) -> Result<String> {
        let function = self
            .abi
            .functions
            .get(function_name)
            .ok_or_else(|| anyhow::anyhow!("Function not found: {}", function_name))?;

        let call_data = self.encode_function_call(function, params)?;

        self.client
            .call_contract(from, &self.address, call_data, 0, private_key)
            .await
    }

    fn encode_function_call(
        &self,
        function: &AbiFunction,
        params: Vec<SwtchvmValue>,
    ) -> Result<Vec<u8>> {
        // Simplified encoding - in practice would use proper ABI encoding
        let mut data = Vec::new();

        // Function selector (first 4 bytes of function signature hash)
        let signature = format!(
            "{}({})",
            function.name,
            function
                .inputs
                .iter()
                .map(|i| i.type_name.as_str())
                .collect::<Vec<_>>()
                .join(",")
        );
        use sha3::{Digest, Keccak256};
        let hash = Keccak256::digest(signature.as_bytes());
        data.extend_from_slice(&hash[..4]);

        // Encode parameters
        for (param, value) in function.inputs.iter().zip(params.iter()) {
            data.extend(self.encode_value(value)?);
        }

        Ok(data)
    }

    fn encode_value(&self, value: &SwtchvmValue) -> Result<Vec<u8>> {
        match value {
            SwtchvmValue::U64(n) => Ok(n.to_be_bytes().to_vec()),
            SwtchvmValue::I64(n) => Ok(n.to_be_bytes().to_vec()),
            SwtchvmValue::Bytes(b) => Ok(b.clone()),
            SwtchvmValue::String(s) => Ok(s.as_bytes().to_vec()),
            SwtchvmValue::Bool(b) => Ok(if *b { vec![1] } else { vec![0] }),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractAbi {
    pub functions: HashMap<String, AbiFunction>,
    pub events: HashMap<String, AbiEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbiFunction {
    pub name: String,
    pub inputs: Vec<AbiParameter>,
    pub outputs: Vec<AbiParameter>,
    pub payable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbiEvent {
    pub name: String,
    pub inputs: Vec<AbiParameter>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbiParameter {
    pub name: String,
    pub type_name: String,
    pub indexed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SwtchvmValue {
    U64(u64),
    I64(i64),
    Bytes(Vec<u8>),
    String(String),
    Bool(bool),
}
