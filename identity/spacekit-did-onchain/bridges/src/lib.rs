//! Off-chain EVM and Solana bridge helpers for SpaceKit quantum DIDs.
//!
//! Depends on [`spacekit_did`] for wallets and SPHINCS+ crypto. On-chain programs and
//! contracts live in this repository under `programs/` and `quantum-evm-contracts/`.

pub mod evm;
pub mod solana;

pub use evm::evm_integration::utils as evm_utils;
pub use evm::evm_integration::{EVMQuantumBridge, EVMQuantumDID, QuantumCredentialProof};

pub use solana::solana_integration::utils as solana_utils;
pub use solana::solana_integration::{
    SolanaCredentialProof, SolanaQuantumBridge, SolanaQuantumDID, SolanaTransactionData,
};
