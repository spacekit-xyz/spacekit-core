use rand::prelude::*;

use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, Nonce}; // AES-GCM with 256-bit keys
use rand::RngCore;
use secp256k1::ecdh::SharedSecret;
use secp256k1::{PublicKey, Secp256k1, SecretKey};

use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{Read, Write};

/// Generate a new keypair
pub fn new_keypair() -> (SecretKey, PublicKey) {
    let secp = Secp256k1::new();
    let mut rng = rand::thread_rng();
    secp.generate_keypair(&mut rng)
}

/// Encrypt data using ECIES
fn ecies_encrypt(
    receiver_pubkey: &PublicKey,
    message: &[u8],
) -> Result<(Vec<u8>, PublicKey, Vec<u8>), Box<dyn std::error::Error>> {
    let secp = Secp256k1::new();

    // Generate ephemeral key pair
    let ephemeral_secret = SecretKey::from_byte_array(&mut rand::thread_rng().gen());
    let ephemeral_pubkey = PublicKey::from_secret_key(&secp, &ephemeral_secret.unwrap());

    // Compute the shared secret
    let shared_secret = SharedSecret::new(receiver_pubkey, &ephemeral_secret.unwrap());
    let shared_secret_hash = Sha256::digest(&shared_secret.secret_bytes());

    // Derive the encryption key
    let encryption_key = shared_secret_hash.as_slice();

    // Encrypt the message using AES-GCM
    let cipher = Aes256Gcm::new_from_slice(encryption_key)?;
    let mut nonce = [0u8; 12];
    OsRng.fill_bytes(&mut nonce); // Generate a random nonce
    let nonce = Nonce::from_slice(&nonce);

    let ciphertext = cipher
        .encrypt(nonce, message)
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;

    Ok((ciphertext, ephemeral_pubkey, nonce.to_vec()))
}

/// Decrypt data using ECIES
fn ecies_decrypt(
    receiver_secret: &SecretKey,
    ephemeral_pubkey: &PublicKey,
    ciphertext: &[u8],
    nonce: &[u8],
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    // let secp = Secp256k1::new();

    // Compute the shared secret
    let shared_secret = SharedSecret::new(ephemeral_pubkey, receiver_secret);
    let shared_secret_hash = Sha256::digest(&shared_secret.secret_bytes());

    // Derive the decryption key
    let decryption_key = shared_secret_hash.as_slice();

    // Decrypt the ciphertext using AES-GCM
    let cipher = Aes256Gcm::new_from_slice(decryption_key)?;
    let nonce = Nonce::from_slice(nonce);

    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;

    Ok(plaintext)
}

/// Encrypt message using ECIES
pub fn encrypt_message(
    receiver_pubkey: &PublicKey,
    message: &[u8],
) -> Result<(Vec<u8>, PublicKey, Vec<u8>), Box<dyn std::error::Error>> {
    ecies_encrypt(receiver_pubkey, message)
}

/// Decrypt message using ECIES
pub fn decrypt_message(
    receiver_secret: &SecretKey,
    ephemeral_pubkey: &PublicKey,
    ciphertext: &[u8],
    nonce: &[u8],
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    ecies_decrypt(receiver_secret, ephemeral_pubkey, ciphertext, nonce)
}

/// Encrypt file using ECIES
pub fn encrypt_file(
    file_path: &str,
    public_key_path: &str,
    output_path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Encrypting file: {}", file_path);

    // Read the binary file
    let mut file = File::open(file_path)?;
    let mut file_data = Vec::new();
    file.read_to_end(&mut file_data)?;

    // Read the hex string from the file and convert to PublicKey
    let hex_string = crate::v1::utils::read_hex_from_file(public_key_path)?;
    println!("Public key read from file: {}", hex_string);
    let public_key_bytes = hex::decode(&hex_string)?;
    let public_key = PublicKey::from_slice(&public_key_bytes)?;

    // Encrypt the data using our existing ECIES implementation
    let (encrypted_data, ephemeral_pubkey, nonce) = ecies_encrypt(&public_key, &file_data)?;

    // Create the final encrypted payload that includes the ephemeral pubkey and nonce
    let mut final_payload = Vec::new();
    final_payload.extend_from_slice(&ephemeral_pubkey.serialize()); // 33 bytes
    final_payload.extend_from_slice(&nonce); // 12 bytes
    final_payload.extend_from_slice(&encrypted_data); // rest of the data

    // Write the encrypted data to the output file
    let mut output = File::create(output_path)?;
    output.write_all(&final_payload)?;

    Ok(())
}

/// Decrypt file using ECIES
pub fn decrypt_file(
    file_path: &str,
    secret_key_path: &str,
    output_path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Decrypting file: {}", file_path);

    // Read the encrypted file
    let mut file = File::open(file_path)?;
    let mut encrypted_data = Vec::new();
    file.read_to_end(&mut encrypted_data)?;

    // Read the hex string from the file and convert to SecretKey
    let hex_string = crate::v1::utils::read_hex_from_file(secret_key_path)?;
    println!("Secret key read from file: {}", hex_string);
    let secret_key_bytes = hex::decode(&hex_string)?;
    let secret_key = SecretKey::from_slice(&secret_key_bytes)?;

    // Extract the components from the encrypted payload
    let ephemeral_pubkey = PublicKey::from_slice(&encrypted_data[0..33])?; // First 33 bytes
    let nonce = &encrypted_data[33..45]; // Next 12 bytes
    let ciphertext = &encrypted_data[45..]; // Rest of the data

    // Decrypt the data using our existing ECIES implementation
    let decrypted_data = ecies_decrypt(&secret_key, &ephemeral_pubkey, ciphertext, nonce)?;

    // Write the decrypted data to the output file
    let mut output = File::create(output_path)?;
    output.write_all(&decrypted_data)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt() {
        let secret_key = SecretKey::from_byte_array(&mut rand::thread_rng().gen()).unwrap();
        println!("Bitcoin Secret key: {}", hex::encode(secret_key.as_ref()));

        let secp = Secp256k1::new();
        let public_key = PublicKey::from_secret_key(&secp, &secret_key);
        println!("Bitcoin Public key: {}", public_key.to_string());

        // Test encryption/decryption
        let (ciphertext, ephemeral_pubkey, nonce) =
            encrypt_message(&public_key, b"Hello, BTC world!").unwrap();
        let decrypted =
            decrypt_message(&secret_key, &ephemeral_pubkey, &ciphertext, &nonce).unwrap();
        assert_eq!(decrypted, b"Hello, BTC world!");
    }
}
