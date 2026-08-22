#[cfg(feature = "quantum")]
use oqs::kem::SharedSecret;
#[cfg(feature = "quantum")]
use oqs::*;

use chacha20poly1305::{
    aead::{Aead, KeyInit},
    Key, XChaCha20Poly1305, XNonce,
};
use rand::rngs::OsRng;
use rand::RngCore;
use std::{
    fs::File,
    io::{Read, Write},
};

/// Utility to encrypt a message with the ChaCha20Poly1305 cipher
/// Example message, let secret_message = b"All_UR_DATA_R_BELONG_2_U";
pub fn encrypt_message(
    secret_message: &[u8; 24],
    cipher: XChaCha20Poly1305,
    nonce: XNonce,
) -> Vec<u8> {
    // Encrypt the secret_message
    let ciphertext = cipher.encrypt(&nonce, secret_message.as_ref()).unwrap();
    ciphertext
}

/// Utility to decrypt a message with the ChaCha20Poly1305 cipher
pub fn decrypt_message(ciphertext: Vec<u8>, cipher: XChaCha20Poly1305, nonce: XNonce) -> Vec<u8> {
    // Decrypt the ciphertext
    let decrypted_message = cipher.decrypt(&nonce, ciphertext.as_ref()).unwrap();
    decrypted_message
}

pub fn encrypt_file(input_path: &str, output_path: &str, cipher: XChaCha20Poly1305, nonce: XNonce) {
    let mut file = File::open(input_path).unwrap();
    let mut file_data = Vec::new();
    file.read_to_end(&mut file_data).unwrap();

    // Encrypt the data
    let ciphertext = cipher.encrypt(&nonce, file_data.as_ref()).unwrap();

    // Write the encrypted data and nonce to a file
    let mut encrypted_file = File::create(output_path).unwrap();
    encrypted_file.write_all(&nonce).unwrap();
    encrypted_file.write_all(&ciphertext).unwrap();
}

pub fn decrypt_file(input_path: &str, output_path: &str, cipher: XChaCha20Poly1305) {
    // To decrypt, read the encrypted file and nonce
    let mut encrypted_file = File::open(input_path).unwrap();
    let mut nonce = [0u8; 24]; // (192-bits/24-bytes)
    let mut ciphertext = Vec::new();
    encrypted_file.read_exact(&mut nonce).unwrap(); // recover nonce
    encrypted_file.read_to_end(&mut ciphertext).unwrap();

    // Decrypt the ciphertext
    let decrypted_data = cipher
        .decrypt(&XNonce::from_slice(&nonce), ciphertext.as_ref())
        .unwrap();

    // Write the decrypted data back to a file
    let mut decrypted_file = File::create(output_path).unwrap();
    decrypted_file.write_all(&decrypted_data).unwrap();
}

#[cfg(feature = "quantum")]
pub fn generate_cipher(shared_secret: &SharedSecret) -> Result<(XChaCha20Poly1305, XNonce)> {
    // Here we use XChaCha20Poly1305 for this purpose of symmetric encryption/decryption
    // using the shared secret

    // Convert the shared secret to a byte slice
    let shared_secret_slice = shared_secret.as_ref();
    // Alternatively, the key can be transformed directly from a byte slice
    // (panicks on length mismatch):
    let key = Key::from_slice(shared_secret_slice);
    // The encryption key can be generated from alice shared secret
    let cipher = XChaCha20Poly1305::new(&key);

    // Generate a nonce
    let mut nonce_bytes = [0u8; 24]; // Create an array to hold the nonce
    OsRng.fill_bytes(&mut nonce_bytes); // Fill the nonce array with random bytes
                                        //let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng); // 96-bits; unique per message
    let nonce = XNonce::from_slice(&nonce_bytes);
    println!("nonce:{:?}", hex::encode(nonce));

    Ok((cipher, *nonce))
}
