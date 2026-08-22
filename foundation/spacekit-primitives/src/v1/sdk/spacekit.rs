use crate::v1::identity::Identity;
use crate::v1::sdk::cache::IdentityCache;
use crate::v1::sdk::context::{
    BlockchainConfig, ChainType, Config, ContextManager, NetworkType, TestnetType, WalletConfig,
};
use crate::v1::sdk::identity::identity_manager::IdentityManagerClient;
use std::error::Error as StdError;
use thiserror::Error;

use alloy::providers::fillers::{FillProvider, JoinFill, RecommendedFillers};
use alloy::providers::Identity as AlloyIdentity;
use alloy::providers::{Provider, ProviderBuilder, RootProvider};
use alloy::transports::{http::Http, BoxTransport, Transport};
use alloy_primitives::Address;
use alloy_primitives::FixedBytes;

use alloy::network::Ethereum;
use alloy::signers::local::PrivateKeySigner;
use alloy::signers::{Signer, SignerSync};

use std::fs::File;
use std::path::Path;
use std::sync::Arc;
// use std::error::Error;

/// Spacekit SDK
/// TODO: Add support for other chains
pub struct SpaceKitSDK {
    context_manager: ContextManager,
    identity_manager: Option<
        IdentityManagerClient<
            FillProvider<
                JoinFill<AlloyIdentity, <Ethereum as RecommendedFillers>::RecommendedFillers>,
                RootProvider<Ethereum>,
            >,
        >,
    >,
    cache: Option<IdentityCache>,
}

#[derive(Error, Debug)]
pub enum SdkError {
    #[error("Identity manager not initialized")]
    IdentityManagerNotInitialized,
    #[error("Contract connection failed: {0}")]
    ContractConnectionError(String),
    #[error("Cache error: {0}")]
    CacheError(#[from] Box<dyn StdError>),
    #[error("Identity not found")]
    IdentityNotFound,
    #[error("Config error: {0}")]
    ConfigError(String),
}

#[derive(Debug, Clone)]
pub struct BaseConfig<Ext = ()> {
    pub name: String,
    pub chain: String,
    pub network: String,
    pub provider_url: String,
    pub public_key: String,
    pub private_key: String,
    pub extra: Ext,
}

impl<Ext> BaseConfig<Ext> {
    pub fn new(
        name: String,
        chain: String,
        network: String,
        provider_url: String,
        public_key: String,
        private_key: String,
        extra: Ext,
    ) -> Self {
        Self {
            name,
            chain,
            network,
            provider_url,
            public_key,
            private_key,
            extra,
        }
    }
}

// #[derive(Debug, Clone)]
pub struct SwtchEvmConfig<Ext = ()> {
    pub base: BaseConfig<Ext>,
    pub chain_type: ChainType,
    pub network_type: NetworkType,
}

impl<Ext> SwtchEvmConfig<Ext> {
    pub fn new(base: BaseConfig<Ext>, chain_type: ChainType, network_type: NetworkType) -> Self {
        Self {
            base,
            chain_type,
            network_type,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SwtchSolanaConfig<Ext = ()> {
    pub base: BaseConfig<Ext>,
    pub network_type: NetworkType,
}

impl<Ext> SwtchSolanaConfig<Ext> {
    pub fn new(base: BaseConfig<Ext>, network_type: NetworkType) -> Self {
        Self { base, network_type }
    }
}

#[derive(Debug, Clone)]
pub struct SwtchBitcoinConfig<Ext = ()> {
    pub base: BaseConfig<Ext>,
    pub network_type: NetworkType,
}

impl<Ext> SwtchBitcoinConfig<Ext> {
    pub fn new(base: BaseConfig<Ext>, network_type: NetworkType) -> Self {
        Self { base, network_type }
    }
}

/// Trait for building chain-specific configurations
pub trait ChainConfigBuilder {
    fn build_config(&self, name: &str) -> Result<Config, String>;
}

impl<Ext> ChainConfigBuilder for SwtchEvmConfig<Ext> {
    fn build_config(&self, name: &str) -> Result<Config, String> {
        Ok(Config::new(
            BlockchainConfig {
                chain_type: self.chain_type.clone(),
                network: self.network_type.clone(),
                port: 8545, // Default port for EVM chains
                provider_url: self.base.provider_url.clone(),
            },
            WalletConfig {
                public_key: self.base.public_key.clone(),
                private_key: self.base.private_key.clone(),
            },
        ))
    }
}

impl<Ext> ChainConfigBuilder for SwtchSolanaConfig<Ext> {
    fn build_config(&self, name: &str) -> Result<Config, String> {
        Ok(Config::new(
            BlockchainConfig {
                chain_type: ChainType::Solana,
                network: self.network_type.clone(),
                port: 8899, // Default port for Solana
                provider_url: self.base.provider_url.clone(),
            },
            WalletConfig {
                public_key: self.base.public_key.clone(),
                private_key: self.base.private_key.clone(),
            },
        ))
    }
}

impl<Ext> ChainConfigBuilder for SwtchBitcoinConfig<Ext> {
    fn build_config(&self, name: &str) -> Result<Config, String> {
        Ok(Config::new(
            BlockchainConfig {
                chain_type: ChainType::Bitcoin,
                network: self.network_type.clone(),
                port: 8332, // Default port for Bitcoin
                provider_url: self.base.provider_url.clone(),
            },
            WalletConfig {
                public_key: self.base.public_key.clone(),
                private_key: self.base.private_key.clone(),
            },
        ))
    }
}

impl SpaceKitSDK {
    pub fn new() -> Self {
        Self {
            context_manager: ContextManager::new(),
            identity_manager: None,
            cache: None,
        }
    }

    /// Add a configuration to the context manager
    /// This function will add a configuration to the context manager
    /// and set it as the active configuration
    pub fn add_configuration(
        &mut self,
        name: &str,
        chain: &str,
        network: &str,
        provider_url: &str,
        public_key: &str,
        private_key: &str,
    ) -> Result<(), String> {
        let chain_type = match chain.to_lowercase().as_str() {
            "ethereum" => ChainType::Ethereum,
            "polygon" => ChainType::Polygon,
            "avalanche" => ChainType::Avalanche,
            "solana" => ChainType::Solana,
            _ => return Err(format!("Unsupported chain type: {}", chain)),
        };

        let network_type = match network.to_lowercase().as_str() {
            "mainnet" => NetworkType::Mainnet,
            "sepolia" => NetworkType::Testnet(TestnetType::Sepolia),
            "goerli" => NetworkType::Testnet(TestnetType::Goerli),
            "mumbai" => NetworkType::Testnet(TestnetType::Mumbai),
            "fuji" => NetworkType::Testnet(TestnetType::Fuji),
            "simulated" => NetworkType::Testnet(TestnetType::Simulated),
            "devnet" => NetworkType::Testnet(TestnetType::Devnet),
            _ => NetworkType::Testnet(TestnetType::Other(network.to_string())),
        };

        let config = Config::new(
            BlockchainConfig {
                chain_type,
                network: network_type,
                port: 8545, // Default port, can be made configurable
                provider_url: provider_url.to_string(),
            },
            WalletConfig {
                public_key: public_key.to_string(),
                private_key: private_key.to_string(),
            },
        );

        self.context_manager.add_config(name, config);

        Ok(())
    }

    pub fn use_configuration(&mut self, name: &str) -> Result<(), String> {
        self.context_manager.set_active_config(name)
    }

    pub fn save_configuration(&mut self, path: &str) -> Result<(), Box<dyn std::error::Error>> {
        let config = self
            .context_manager
            .get_active_config()
            .ok_or(SdkError::ConfigError("No active configuration".into()))?;
        config.save_to_file(path)?;
        Ok(())
    }

    pub fn load_configuration(&mut self, path: &str) -> Result<(), Box<dyn std::error::Error>> {
        let config = Config::load_from_file(path)?;
        let name = Path::new(path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("default")
            .to_string();
        self.context_manager.add_config(&name, config);
        Ok(())
    }

    /// Initialize the identity manager
    /// This function will initialize the identity manager with the given contract address
    /// and the active configuration
    pub async fn initialize_identity_manager(
        &mut self,
        contract_addr: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let contract_address: Address = contract_addr.parse().expect("Invalid address");
        let config = self
            .context_manager
            .get_active_config()
            .ok_or("No active configuration")?;

        let rpc_url = config.blockchain.provider_url.parse().map_err(|e| {
            SdkError::ContractConnectionError(format!("Invalid provider URL: {}", e))
        })?;
        let provider = ProviderBuilder::new()
            .network::<Ethereum>()
            .connect_http(rpc_url);

        let client = Arc::new(provider);
        let bytes = hex::decode(&config.wallet.private_key)?;
        let private_key = FixedBytes::<32>::from_slice(&bytes);
        let wallet = Box::new(PrivateKeySigner::from_bytes(&private_key)?);

        self.identity_manager = Some(IdentityManagerClient::new(contract_address, client, wallet));
        Ok(())
    }

    pub fn with_cache(cache_path: &str) -> Result<Self, SdkError> {
        Ok(Self {
            context_manager: ContextManager::new(),
            identity_manager: None,
            cache: Some(IdentityCache::new(cache_path).map_err(SdkError::CacheError)?),
        })
    }

    pub async fn load_identity(&self, did_addr: &str) -> Result<Identity, SdkError> {
        // Try cache first
        if let Some(cache) = &self.cache {
            if let Ok(Some(cached)) = cache.get_cached_identity(did_addr) {
                return Ok(cached);
            }
        }

        // If not in cache, load from contract
        let identity = self.load_identity_from_contract(did_addr).await?;

        // Cache the result
        if let Some(cache) = &self.cache {
            cache
                .cache_identity(&identity)
                .map_err(SdkError::CacheError)?;
        }

        Ok(identity)
    }

    async fn load_identity_from_contract(&self, did_addr: &str) -> Result<Identity, SdkError> {
        let manager = self
            .identity_manager
            .as_ref()
            .ok_or(SdkError::IdentityManagerNotInitialized)?;

        let did_identity = manager
            .load_identity(
                did_addr
                    .parse::<Address>()
                    .map_err(|e| SdkError::ContractConnectionError(e.to_string()))?,
            )
            .await
            .map_err(|e| SdkError::ContractConnectionError(e.to_string()))?;

        Ok(Identity {
            did: did_identity.address.to_string(),
            username: did_identity.did_document.clone(),
            master_password: String::new(),
            default_profile: false,
            profiles: Vec::new(),
            authenticated: false,
            key_pairs: Vec::new(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        })
    }

    /// Add an Ethereum (or other EVM) configuration
    pub fn add_evm_configuration<Ext>(
        &mut self,
        config: SwtchEvmConfig<Ext>,
        name: &str,
    ) -> Result<(), String> {
        let sdk_config = config.build_config(name)?;
        self.context_manager.add_config(name, sdk_config);
        Ok(())
    }

    /// Add a Solana configuration
    pub fn add_solana_configuration<Ext>(
        &mut self,
        config: SwtchSolanaConfig<Ext>,
        name: &str,
    ) -> Result<(), String> {
        let sdk_config = config.build_config(name)?;
        self.context_manager.add_config(name, sdk_config);
        Ok(())
    }

    /// Add a Bitcoin configuration
    pub fn add_bitcoin_configuration<Ext>(
        &mut self,
        config: SwtchBitcoinConfig<Ext>,
        name: &str,
    ) -> Result<(), String> {
        let sdk_config = config.build_config(name)?;
        self.context_manager.add_config(name, sdk_config);
        Ok(())
    }

    // Helper to create an Ethereum configuration
    pub fn create_ethereum_config<Ext>(
        &self,
        name: String,
        network: &str,
        provider_url: String,
        public_key: String,
        private_key: String,
        extra: Ext,
    ) -> Result<SwtchEvmConfig<Ext>, String> {
        let network_type = match network.to_lowercase().as_str() {
            "mainnet" => NetworkType::Mainnet,
            "sepolia" => NetworkType::Testnet(TestnetType::Sepolia),
            "goerli" => NetworkType::Testnet(TestnetType::Goerli),
            _ => NetworkType::Testnet(TestnetType::Other(network.to_string())),
        };

        Ok(SwtchEvmConfig::new(
            BaseConfig::new(
                name,
                "ethereum".to_string(),
                network.to_string(),
                provider_url,
                public_key,
                private_key,
                extra,
            ),
            ChainType::Ethereum,
            network_type,
        ))
    }

    // Helper to create a Polygon configuration
    pub fn create_polygon_config<Ext>(
        &self,
        name: String,
        network: &str,
        provider_url: String,
        public_key: String,
        private_key: String,
        extra: Ext,
    ) -> Result<SwtchEvmConfig<Ext>, String> {
        let network_type = match network.to_lowercase().as_str() {
            "mainnet" => NetworkType::Mainnet,
            "mumbai" => NetworkType::Testnet(TestnetType::Mumbai),
            _ => NetworkType::Testnet(TestnetType::Other(network.to_string())),
        };

        Ok(SwtchEvmConfig::new(
            BaseConfig::new(
                name,
                "polygon".to_string(),
                network.to_string(),
                provider_url,
                public_key,
                private_key,
                extra,
            ),
            ChainType::Polygon,
            network_type,
        ))
    }

    // Helper to create an Avalanche configuration
    pub fn create_avalanche_config<Ext>(
        &self,
        name: String,
        network: &str,
        provider_url: String,
        public_key: String,
        private_key: String,
        extra: Ext,
    ) -> Result<SwtchEvmConfig<Ext>, String> {
        let network_type = match network.to_lowercase().as_str() {
            "mainnet" => NetworkType::Mainnet,
            "fuji" => NetworkType::Testnet(TestnetType::Fuji),
            _ => NetworkType::Testnet(TestnetType::Other(network.to_string())),
        };

        Ok(SwtchEvmConfig::new(
            BaseConfig::new(
                name,
                "avalanche".to_string(),
                network.to_string(),
                provider_url,
                public_key,
                private_key,
                extra,
            ),
            ChainType::Avalanche,
            network_type,
        ))
    }

    // Helper to create a Solana configuration
    pub fn create_solana_config<Ext>(
        &self,
        name: String,
        network: &str,
        provider_url: String,
        public_key: String,
        private_key: String,
        extra: Ext,
    ) -> Result<SwtchSolanaConfig<Ext>, String> {
        let network_type = match network.to_lowercase().as_str() {
            "mainnet" => NetworkType::Mainnet,
            "devnet" => NetworkType::Testnet(TestnetType::Devnet),
            _ => NetworkType::Testnet(TestnetType::Other(network.to_string())),
        };

        Ok(SwtchSolanaConfig::new(
            BaseConfig::new(
                name,
                "solana".to_string(),
                network.to_string(),
                provider_url,
                public_key,
                private_key,
                extra,
            ),
            network_type,
        ))
    }

    // Helper to create a Bitcoin configuration
    pub fn create_bitcoin_config<Ext>(
        &self,
        name: String,
        network: &str,
        provider_url: String,
        public_key: String,
        private_key: String,
        extra: Ext,
    ) -> Result<SwtchBitcoinConfig<Ext>, String> {
        let network_type = match network.to_lowercase().as_str() {
            "mainnet" => NetworkType::Mainnet,
            "testnet" => NetworkType::Testnet(TestnetType::Other("testnet".to_string())),
            "regtest" => NetworkType::Testnet(TestnetType::Other("regtest".to_string())),
            _ => NetworkType::Testnet(TestnetType::Other(network.to_string())),
        };

        Ok(SwtchBitcoinConfig::new(
            BaseConfig::new(
                name,
                "bitcoin".to_string(),
                network.to_string(),
                provider_url,
                public_key,
                private_key,
                extra,
            ),
            network_type,
        ))
    }

    // Generate a new keypair based on the active chain type
    pub fn generate_keypair(&self) -> Result<(String, String), Box<dyn std::error::Error>> {
        let chain_type = self
            .context_manager
            .get_current_chain_type()
            .ok_or_else(|| SdkError::ConfigError("No active configuration".into()))?;

        match chain_type {
            ChainType::Solana => self.generate_solana_keypair(),
            _ => self.generate_evm_keypair(), // Default to EVM for Ethereum, Polygon, Avalanche
        }
    }

    // Generate an EVM-compatible keypair
    pub fn generate_evm_keypair(&self) -> Result<(String, String), Box<dyn std::error::Error>> {
        use crate::v1::crypto::evm::new_keypair as new_keypair_evm;

        let (priv_key, pub_key) = new_keypair_evm()?;
        let public_key_hex = hex::encode(pub_key.serialize());
        let private_key_hex = hex::encode(priv_key.serialize());

        Ok((private_key_hex, public_key_hex))
    }

    // Generate a Solana-compatible keypair
    pub fn generate_solana_keypair(&self) -> Result<(String, String), Box<dyn std::error::Error>> {
        use crate::v1::crypto::solana::{key_to_base58, new_keypair as new_keypair_solana};

        let (priv_key, pub_key) = new_keypair_solana()?;
        let public_key_base58 = key_to_base58(pub_key.as_bytes());
        let private_key_base58 = key_to_base58(priv_key.as_bytes());

        Ok((private_key_base58, public_key_base58))
    }

    // Encrypt a file using the active chain's encryption method
    pub fn encrypt_file(
        &self,
        file_path: &str,
        public_key_path: &str,
        output_path: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let chain_type = self
            .context_manager
            .get_current_chain_type()
            .ok_or_else(|| SdkError::ConfigError("No active configuration".into()))?;

        match chain_type {
            ChainType::Solana => {
                use crate::v1::crypto::solana::encrypt_file as encrypt_file_solana;
                encrypt_file_solana(file_path, public_key_path, output_path)?;
            }
            _ => {
                // Default to EVM for Ethereum, Polygon, Avalanche
                use crate::v1::crypto::evm::encrypt_file as encrypt_file_evm;
                encrypt_file_evm(file_path, public_key_path, output_path)?;
            }
        }

        Ok(())
    }

    // Decrypt a file using the active chain's encryption method
    pub fn decrypt_file(
        &self,
        file_path: &str,
        secret_key_path: &str,
        output_path: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let chain_type = self
            .context_manager
            .get_current_chain_type()
            .ok_or_else(|| SdkError::ConfigError("No active configuration".into()))?;

        match chain_type {
            ChainType::Solana => {
                use crate::v1::crypto::solana::decrypt_file as decrypt_file_solana;
                decrypt_file_solana(file_path, secret_key_path, output_path)?;
            }
            _ => {
                // Default to EVM for Ethereum, Polygon, Avalanche
                use crate::v1::crypto::evm::decrypt_file as decrypt_file_evm;
                decrypt_file_evm(file_path, secret_key_path, output_path)?;
            }
        }

        Ok(())
    }
}

// Example usage:
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_multi_chain_config() {
        let mut sdk = SpaceKitSDK::new();

        // Create Ethereum configuration with custom extra data
        let eth_config = sdk
            .create_ethereum_config(
                "ethereum-mainnet".to_string(),
                "mainnet",
                "https://mainnet.infura.io/v3/your-api-key".to_string(),
                "0x...".to_string(),
                "...".to_string(),
                ExtraEthConfig { gas_limit: 100000 },
            )
            .unwrap();

        // Add the Ethereum configuration to the SDK
        sdk.add_evm_configuration(eth_config, "ethereum-mainnet")
            .unwrap();

        // Create and add Solana configuration
        let solana_config = sdk
            .create_solana_config(
                "solana-devnet".to_string(),
                "devnet",
                "https://api.devnet.solana.com".to_string(),
                "...".to_string(),
                "...".to_string(),
                (), // No extra config
            )
            .unwrap();

        sdk.add_solana_configuration(solana_config, "solana-devnet")
            .unwrap();

        // Create and add Bitcoin configuration
        let bitcoin_config = sdk
            .create_bitcoin_config(
                "bitcoin-testnet".to_string(),
                "testnet",
                "http://localhost:18332".to_string(),
                "...".to_string(),
                "...".to_string(),
                ExtraBitcoinConfig { fee_rate: 5 },
            )
            .unwrap();

        sdk.add_bitcoin_configuration(bitcoin_config, "bitcoin-testnet")
            .unwrap();

        // Switch between configurations
        sdk.use_configuration("ethereum-mainnet").unwrap();
        // Do Ethereum operations...

        sdk.use_configuration("solana-devnet").unwrap();
        // Do Solana operations...

        sdk.use_configuration("bitcoin-testnet").unwrap();
        // Do Bitcoin operations...
    }

    // Example extra configuration structs
    #[derive(Debug, Clone)]
    struct ExtraEthConfig {
        gas_limit: u64,
    }

    #[derive(Debug, Clone)]
    struct ExtraBitcoinConfig {
        fee_rate: u64,
    }
}
