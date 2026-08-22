pub mod aes;
pub mod bitcoin;
pub mod chacha;
pub mod evm;
pub mod mnemonic;
pub mod quantum;
pub mod solana;
pub mod xchacha;
use serde::{Deserialize, Serialize};

use clap::ValueEnum;
#[cfg(feature = "quantum")]
use oqs::kem::{self, Kem};
use std::error::Error;
use std::fmt::Display;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, ValueEnum, Serialize, Deserialize)]
pub enum EncryptionAlgorithm {
    ECIES,
    // Quantum algorithms - all 19 variants supported
    BikeL1,
    BikeL3,
    BikeL5,
    Kyber512,
    Kyber768,
    Kyber1024,
    NtruPrimeSntrup761,
    FrodoKem1344Aes,
    FrodoKem1344Shake,
    ClassicMcEliece348864,
    ClassicMcEliece348864f,
    ClassicMcEliece460896,
    ClassicMcEliece460896f,
    ClassicMcEliece6688128,
    ClassicMcEliece6688128f,
    ClassicMcEliece6960119,
    ClassicMcEliece6960119f,
    ClassicMcEliece8192128,
    ClassicMcEliece8192128f,
}

impl Display for EncryptionAlgorithm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl FromStr for EncryptionAlgorithm {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "ecies" => Ok(EncryptionAlgorithm::ECIES),
            "bikel1" => Ok(EncryptionAlgorithm::BikeL1),
            "bikel3" => Ok(EncryptionAlgorithm::BikeL3),
            "bikel5" => Ok(EncryptionAlgorithm::BikeL5),
            "kyber512" => Ok(EncryptionAlgorithm::Kyber512),
            "kyber768" => Ok(EncryptionAlgorithm::Kyber768),
            "kyber1024" => Ok(EncryptionAlgorithm::Kyber1024),
            "ntruprimesntrup761" | "ntru-prime-sntrup761" => {
                Ok(EncryptionAlgorithm::NtruPrimeSntrup761)
            }
            "frodokem1344aes" => Ok(EncryptionAlgorithm::FrodoKem1344Aes),
            "frodokem1344shake" => Ok(EncryptionAlgorithm::FrodoKem1344Shake),
            "classicmceliece348864" => Ok(EncryptionAlgorithm::ClassicMcEliece348864),
            "classicmceliece348864f" => Ok(EncryptionAlgorithm::ClassicMcEliece348864f),
            "classicmceliece460896" => Ok(EncryptionAlgorithm::ClassicMcEliece460896),
            "classicmceliece460896f" => Ok(EncryptionAlgorithm::ClassicMcEliece460896f),
            "classicmceliece6688128" => Ok(EncryptionAlgorithm::ClassicMcEliece6688128),
            "classicmceliece6688128f" => Ok(EncryptionAlgorithm::ClassicMcEliece6688128f),
            "classicmceliece6960119" => Ok(EncryptionAlgorithm::ClassicMcEliece6960119),
            "classicmceliece6960119f" => Ok(EncryptionAlgorithm::ClassicMcEliece6960119f),
            "classicmceliece8192128" => Ok(EncryptionAlgorithm::ClassicMcEliece8192128),
            "classicmceliece8192128f" => Ok(EncryptionAlgorithm::ClassicMcEliece8192128f),
            _ => Err(format!("'{}' is not a valid algorithm", s)),
        }
    }
}

impl EncryptionAlgorithm {
    #[cfg(feature = "quantum")]
    pub fn to_oqs_kem_algorithm(self) -> oqs::kem::Algorithm {
        match self {
            EncryptionAlgorithm::ECIES => panic!("ECIES is not a quantum algorithm"),
            EncryptionAlgorithm::BikeL1 => oqs::kem::Algorithm::BikeL1,
            EncryptionAlgorithm::BikeL3 => oqs::kem::Algorithm::BikeL3,
            EncryptionAlgorithm::BikeL5 => oqs::kem::Algorithm::BikeL5,
            EncryptionAlgorithm::Kyber512 => oqs::kem::Algorithm::Kyber512,
            EncryptionAlgorithm::Kyber768 => oqs::kem::Algorithm::Kyber768,
            EncryptionAlgorithm::Kyber1024 => oqs::kem::Algorithm::Kyber1024,
            EncryptionAlgorithm::NtruPrimeSntrup761 => oqs::kem::Algorithm::NtruPrimeSntrup761,
            EncryptionAlgorithm::FrodoKem1344Aes => oqs::kem::Algorithm::FrodoKem1344Aes,
            EncryptionAlgorithm::FrodoKem1344Shake => oqs::kem::Algorithm::FrodoKem1344Shake,
            EncryptionAlgorithm::ClassicMcEliece348864 => {
                oqs::kem::Algorithm::ClassicMcEliece348864
            }
            EncryptionAlgorithm::ClassicMcEliece348864f => {
                oqs::kem::Algorithm::ClassicMcEliece348864f
            }
            EncryptionAlgorithm::ClassicMcEliece460896 => {
                oqs::kem::Algorithm::ClassicMcEliece460896
            }
            EncryptionAlgorithm::ClassicMcEliece460896f => {
                oqs::kem::Algorithm::ClassicMcEliece460896f
            }
            EncryptionAlgorithm::ClassicMcEliece6688128 => {
                oqs::kem::Algorithm::ClassicMcEliece6688128
            }
            EncryptionAlgorithm::ClassicMcEliece6688128f => {
                oqs::kem::Algorithm::ClassicMcEliece6688128f
            }
            EncryptionAlgorithm::ClassicMcEliece6960119 => {
                oqs::kem::Algorithm::ClassicMcEliece6960119
            }
            EncryptionAlgorithm::ClassicMcEliece6960119f => {
                oqs::kem::Algorithm::ClassicMcEliece6960119f
            }
            EncryptionAlgorithm::ClassicMcEliece8192128 => {
                oqs::kem::Algorithm::ClassicMcEliece8192128
            }
            EncryptionAlgorithm::ClassicMcEliece8192128f => {
                oqs::kem::Algorithm::ClassicMcEliece8192128f
            }
        }
    }
}

#[cfg(feature = "quantum")]
pub fn generate_kem(selected_algo: EncryptionAlgorithm) -> Result<Kem, Box<dyn Error>> {
    let algo = selected_algo.to_oqs_kem_algorithm();
    let kem = kem::Kem::new(algo).map_err(|e| format!("Failed to create KEM: {}", e))?;
    Ok(kem)
}

#[cfg(not(feature = "quantum"))]
pub fn generate_kem(_selected_algo: EncryptionAlgorithm) -> Result<(), Box<dyn Error>> {
    Err("Quantum features are disabled".into())
}
