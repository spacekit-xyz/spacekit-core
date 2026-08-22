// src/context/config.rs
use crate::v1::sdk::spacekit::SdkError;
use serde::{Deserialize, Serialize};
use std::fs::File;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockchainConfig {
    pub chain_type: ChainType,
    pub network: NetworkType,
    pub port: u16,
    pub provider_url: String,
    // Add any other necessary blockchain-specific configs
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletConfig {
    pub public_key: String,
    pub private_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub blockchain: BlockchainConfig,
    pub wallet: WalletConfig,
    // Add any other global configs
}

impl Config {
    pub fn new(blockchain: BlockchainConfig, wallet: WalletConfig) -> Self {
        Self { blockchain, wallet }
    }

    pub fn load_from_file(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let file = File::open(path)?;
        let config = serde_json::from_reader(file)?;
        Ok(config)
    }

    pub fn save_to_file(&self, path: &str) -> Result<(), Box<dyn std::error::Error>> {
        let file = File::create(path)?;
        serde_json::to_writer_pretty(file, self)?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChainType {
    Ethereum,
    Polygon,
    Avalanche,
    Solana,
    Bitcoin,
    // Add other supported chains
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NetworkType {
    Mainnet,
    Testnet(TestnetType),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TestnetType {
    // Ethereum testnets
    Sepolia,
    Goerli,
    // Polygon testnets
    Mumbai,
    // Avalanche testnets
    Fuji,
    // Solana testnets
    Devnet,
    // Hardhat testnet
    Simulated,
    // Add other testnets as needed
    Other(String), // For flexibility
}
