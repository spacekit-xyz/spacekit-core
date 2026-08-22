use bs58;
use ed25519_dalek::{SigningKey, VerifyingKey};
use rand::rngs::OsRng;
use std::error::Error;
use std::fs::File;
use std::io::{Read, Write};

use crate::v1::utils::read_from_file;

// A very simple XOR-based encryption just for demonstration
// In production, use a secure encryption library with proper key handling
fn xor_encrypt(data: &[u8], key: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(data.len());
    for (i, &byte) in data.iter().enumerate() {
        result.push(byte ^ key[i % key.len()]);
    }
    result
}

/// Generate New PrivateKey and PublicKey for Solana
/// This is just a stub - in a real implementation, you would
/// integrate with ed25519-dalek properly
pub fn new_keypair() -> Result<(SigningKey, VerifyingKey), Box<dyn Error>> {
    // Generate a real Ed25519 signing key.
    // NOTE: This module still uses XOR "encryption" for demo purposes only.
    let signing_key = SigningKey::generate(&mut OsRng);
    let verifying_key = signing_key.verifying_key();

    Ok((signing_key, verifying_key))
}

/// Convert a key to Base58 string
pub fn key_to_base58(bytes: &[u8]) -> String {
    bs58::encode(bytes).into_string()
}

/// Convert a Base58 string to bytes
pub fn base58_to_bytes(base58_str: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    Ok(bs58::decode(base58_str).into_vec()?)
}

/// Encrypt message with key
pub fn encrypt_message(message: &[u8], key_bytes: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
    // This is a simplified demo implementation - in production use proper encryption
    let encrypted = xor_encrypt(message, key_bytes);
    Ok(encrypted)
}

/// Decrypt message with key (XOR is its own inverse)
pub fn decrypt_message(ciphertext: &[u8], key_bytes: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
    // This is a simplified demo implementation - in production use proper encryption
    let decrypted = xor_encrypt(ciphertext, key_bytes);
    Ok(decrypted)
}

/// Encrypt File with Solana Public Key
pub fn encrypt_file(
    file_path: &str,
    public_key_path: &str,
    output_path: &str,
) -> Result<(), Box<dyn Error>> {
    println!("Encrypting file for Solana: {}", file_path);

    // Read the binary file
    let mut file = File::open(file_path)?;
    let mut file_data = Vec::new();
    file.read_to_end(&mut file_data)?;

    // Read the Base58 string from the file
    let base58_string = read_from_file(public_key_path)?;
    println!("Public key read from file (Base58): {}", base58_string);

    // Convert Base58 to bytes
    let public_key_bytes = base58_to_bytes(&base58_string)?;

    // Encrypt data using the public key bytes as an encryption key
    // In a real implementation, you'd use a proper key exchange protocol
    let encrypted_data = encrypt_message(&file_data, &public_key_bytes)?;

    // Write the encrypted data to a file
    let mut output = File::create(output_path)?;
    output.write_all(&encrypted_data)?;

    Ok(())
}

/// Decrypt File with Solana Private Key
pub fn decrypt_file(
    file_path: &str,
    secret_key_path: &str,
    output_path: &str,
) -> Result<(), Box<dyn Error>> {
    println!("Decrypting file for Solana: {}", file_path);

    // Read the encrypted file
    let mut file = File::open(file_path)?;
    let mut file_data = Vec::new();
    file.read_to_end(&mut file_data)?;

    // Read the Base58 string from the secret key file
    let base58_string = read_from_file(secret_key_path)?;
    println!("Secret key read from file (Base58): {}", base58_string);

    // Convert Base58 to bytes
    let secret_key_bytes = base58_to_bytes(&base58_string)?;

    // Decrypt data using the secret key bytes
    let decrypted_data = decrypt_message(&file_data, &secret_key_bytes)?;

    // Write the decrypted data to a file
    let mut output = File::create(output_path)?;
    output.write_all(&decrypted_data)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt() {
        let secret_bytes = [42u8; 32];
        let message = b"Hello, Solana world!";

        let encrypted = encrypt_message(message, &secret_bytes).unwrap();
        let decrypted = decrypt_message(&encrypted, &secret_bytes).unwrap();

        assert_eq!(&decrypted, message);
    }

    #[test]
    fn test_base58_conversion() {
        let original = vec![1, 2, 3, 4, 5];
        let base58 = key_to_base58(&original);
        let decoded = base58_to_bytes(&base58).unwrap();
        assert_eq!(original, decoded);
    }
}
