use crate::v1::utils::file_ops::{load_from_file, save_to_file};
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    Key, XChaCha20Poly1305, XNonce,
};
#[cfg(feature = "quantum")]
use oqs::kem::SharedSecret;
use rand::rngs::OsRng;
use rand::RngCore;
use std::{
    fs::File,
    io::{Read, Write},
};

pub fn generate_nonce() -> XNonce {
    let mut nonce_bytes = [0u8; 24]; // (192-bits/24-bytes)
    OsRng.fill_bytes(&mut nonce_bytes);
    *XNonce::from_slice(&nonce_bytes)
}

#[cfg(feature = "quantum")]
pub fn generate_cipher(
    shared_secret: &SharedSecret,
) -> Result<XChaCha20Poly1305, Box<dyn std::error::Error>> {
    let shared_secret_slice = shared_secret.as_ref();
    let key = Key::from_slice(shared_secret_slice);
    let cipher = XChaCha20Poly1305::new(key);

    Ok(cipher)
}

#[cfg(not(feature = "quantum"))]
pub fn generate_cipher(
    _shared_secret: &[u8],
) -> Result<XChaCha20Poly1305, Box<dyn std::error::Error>> {
    let key = Key::from_slice(&[0u8; 32]);
    let cipher = XChaCha20Poly1305::new(key);
    Ok(cipher)
}

pub fn encrypt_file(input_path: &str, output_path: &str, cipher: XChaCha20Poly1305) {
    let mut file = File::open(input_path).unwrap();
    let mut file_data = Vec::new();
    file.read_to_end(&mut file_data).unwrap();

    // Generate a random nonce for each file
    let nonce = generate_nonce();

    // Encrypt the data
    let ciphertext = cipher.encrypt(&nonce, file_data.as_ref()).unwrap();

    // Write the nonce and encrypted data to a file
    let mut encrypted_file = File::create(output_path).unwrap();
    encrypted_file.write_all(&nonce).unwrap();
    encrypted_file.write_all(&ciphertext).unwrap();
}

pub fn decrypt_file(
    input_path: &str,
    output_path: &str,
    cipher: XChaCha20Poly1305,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut encrypted_file = File::open(input_path)?;
    let mut encrypted_data = Vec::new();
    encrypted_file.read_to_end(&mut encrypted_data)?;

    // Extract the nonce (first 24 bytes) and ciphertext
    let nonce = XNonce::from_slice(&encrypted_data[..24]);
    let ciphertext = &encrypted_data[24..];

    // Decrypt the data
    let decrypted_data = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| format!("Decryption failed: {}", e))?;

    // Write the decrypted data to the output file
    let mut output_file = File::create(output_path)?;
    output_file.write_all(&decrypted_data)?;

    Ok(())
}

/// Utility to encrypt a message with the XChaCha20Poly1305 cipher
pub fn encrypt_message(secret_message: &[u8], cipher: XChaCha20Poly1305, nonce: XNonce) -> Vec<u8> {
    cipher.encrypt(&nonce, secret_message).unwrap()
}

/// Utility to decrypt a message with the XChaCha20Poly1305 cipher
pub fn decrypt_message(ciphertext: Vec<u8>, cipher: XChaCha20Poly1305, nonce: XNonce) -> Vec<u8> {
    cipher.decrypt(&nonce, ciphertext.as_ref()).unwrap()
}

#[cfg(feature = "quantum")]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_cipher() {
        let kem = oqs::kem::Kem::new(oqs::kem::Algorithm::Kyber512).unwrap();
        let (pk, sk) = kem.keypair().unwrap();
        let (ct, shared_secret) = kem.encapsulate(&pk).unwrap();
        let cipher = generate_cipher(&shared_secret).unwrap();
        let nonce = generate_nonce();
        assert_eq!(nonce.as_slice().len(), 24);
    }

    #[test]
    fn test_encrypt_decrypt_message() {
        let message = "Hello, world!";
        let mut message_bytes = [0u8; 24];
        message_bytes[..message.len()].copy_from_slice(message.as_bytes());

        let kem = oqs::kem::Kem::new(oqs::kem::Algorithm::Kyber512).unwrap();
        let (pk, sk) = kem.keypair().unwrap();
        let (ct, shared_secret) = kem.encapsulate(&pk).unwrap();
        let cipher = generate_cipher(&shared_secret).unwrap();

        // Use the same nonce for encryption and decryption
        let nonce = generate_nonce();
        let encrypted_message = encrypt_message(&message_bytes, cipher.clone(), nonce);
        let decrypted_message = decrypt_message(encrypted_message, cipher.clone(), nonce);

        assert_eq!(
            message,
            String::from_utf8_lossy(&decrypted_message[..message.len()])
        );
    }
}
