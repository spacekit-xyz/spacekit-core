#[cfg(feature = "quantum")]
use oqs::kem::Kem;
#[cfg(feature = "quantum")]
use oqs::kem::PublicKey;
#[cfg(feature = "quantum")]
use oqs::kem::SecretKey;
#[cfg(feature = "quantum")]
use oqs::kem::{Ciphertext, SharedSecret};
#[cfg(feature = "quantum")]
use oqs::*;

// A key generation algorithm, Generate, which generates a public key and a private key (a keypair).
#[cfg(feature = "quantum")]
pub fn generate_kem(selected_algo: &str) -> Result<Kem> {
    let algo: oqs::kem::Algorithm = match selected_algo {
        "BikeL1" => kem::Algorithm::BikeL1,
        "BikeL3" => kem::Algorithm::BikeL3,
        "BikeL5" => kem::Algorithm::BikeL5,
        "Kyber512" => kem::Algorithm::Kyber512,
        "Kyber768" => kem::Algorithm::Kyber768,
        "Kyber1024" => kem::Algorithm::Kyber1024,
        "NtruPrimeSntrup761" => kem::Algorithm::NtruPrimeSntrup761,
        "FrodoKem1344Aes" => kem::Algorithm::FrodoKem1344Aes,
        "FrodoKem1344Shake" => kem::Algorithm::FrodoKem1344Shake,
        "ClassicMcEliece348864" => kem::Algorithm::ClassicMcEliece348864,
        "ClassicMcEliece348864f" => kem::Algorithm::ClassicMcEliece348864f,
        "ClassicMcEliece460896" => kem::Algorithm::ClassicMcEliece460896,
        "ClassicMcEliece460896f" => kem::Algorithm::ClassicMcEliece460896f,
        "ClassicMcEliece6688128" => kem::Algorithm::ClassicMcEliece6688128,
        "ClassicMcEliece6688128f" => kem::Algorithm::ClassicMcEliece6688128f,
        "ClassicMcEliece6960119" => kem::Algorithm::ClassicMcEliece6960119,
        "ClassicMcEliece6960119f" => kem::Algorithm::ClassicMcEliece6960119f,
        "ClassicMcEliece8192128" => kem::Algorithm::ClassicMcEliece8192128,
        "ClassicMcEliece8192128f" => kem::Algorithm::ClassicMcEliece8192128f,
        _ => kem::Algorithm::Kyber1024,
    };

    let kem = kem::Kem::new(algo).unwrap();
    Ok(kem)
}

// An encapsulation algorithm, Encapsulate, which takes as input a public key,
// and outputs a shared secret value and an “encapsulation” (a ciphertext) of this secret value.
#[cfg(feature = "quantum")]
pub fn encapsulate(kem: &Kem, public_key: &PublicKey) -> Result<(Ciphertext, SharedSecret)> {
    // BOB gets Alice's public key and generates a shared secret and ciphertext
    // Encapsulate (ciphertext, shared secret) with public key reference
    let (kem_ciphertext, b_kem_ss) = kem.encapsulate(public_key).unwrap();
    Ok((kem_ciphertext, b_kem_ss))
}

// A decapsulation algorithm, Decapsulate, which takes as input the encapsulation and the private key,
// and outputs the shared secret value.
#[cfg(feature = "quantum")]
pub fn decapsulate(
    kem: &Kem,
    kem_ciphertext: &Ciphertext,
    private_key: &SecretKey,
) -> Result<SharedSecret> {
    kem.decapsulate(private_key, kem_ciphertext)
}

#[cfg(feature = "quantum")]
pub mod utils {
    use super::*;

    pub fn print_keys(pk: &PublicKey, sk: &SecretKey) {
        // Print the public key in hexadecimal format
        println!("public_key: {}\n", hex::encode(pk.as_ref()));
        // Print the private key in hexadecimal format
        println!("private_key: {}\n", hex::encode(sk.as_ref()));
    }

    pub fn print_kem_artifacts(kem_ciphertext: &Ciphertext, b_kem_ss: &SharedSecret) {
        // Print the kem ciphertext in hexadecimal format
        println!("kem ciphertext: {}\n", hex::encode(kem_ciphertext.as_ref()));
        // Print the kem shared secret in hexadecimal format
        println!("kem shared secret: {}\n", hex::encode(b_kem_ss.as_ref()));
    }
}
