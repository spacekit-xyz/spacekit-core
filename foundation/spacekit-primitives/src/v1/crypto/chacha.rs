use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Key, Nonce,
};
#[cfg(feature = "quantum")]
use oqs::kem::SharedSecret;
use rand::rngs::OsRng;
use rand::RngCore;
use std::{
    fs::File,
    io::{Read, Write},
};

pub fn generate_nonce() -> Nonce {
    let mut nonce_bytes = [0u8; 12]; // (96-bits/12-bytes)
    OsRng.fill_bytes(&mut nonce_bytes);
    *Nonce::from_slice(&nonce_bytes)
}

/// Utility to encrypt a message with the ChaCha20Poly1305 cipher
/// Example message, let secret_message = b"All_UR_DATA_R_BELONG_2_U";
pub fn encrypt_message(secret_message: &[u8], cipher: ChaCha20Poly1305, nonce: Nonce) -> Vec<u8> {
    // Encrypt the secret_message
    let ciphertext = cipher.encrypt(&nonce, secret_message.as_ref()).unwrap();
    ciphertext
}

/// Utility to decrypt a message with the ChaCha20Poly1305 cipher
pub fn decrypt_message(ciphertext: Vec<u8>, cipher: ChaCha20Poly1305, nonce: Nonce) -> Vec<u8> {
    // Decrypt the ciphertext
    let decrypted_message = cipher.decrypt(&nonce, ciphertext.as_ref()).unwrap();
    decrypted_message
}

pub fn encrypt_file(input_path: &str, output_path: &str, cipher: ChaCha20Poly1305) {
    let mut file = File::open(input_path).unwrap();
    let mut file_data = Vec::new();
    file.read_to_end(&mut file_data).unwrap();

    // Generate a random nonce for each file
    let nonce = generate_nonce();

    // Encrypt the data
    let ciphertext = cipher.encrypt(&nonce, file_data.as_ref()).unwrap();

    // Write the encrypted data and nonce to a file
    let mut encrypted_file = File::create(output_path).unwrap();
    encrypted_file.write_all(&nonce.as_slice()).unwrap();
    encrypted_file.write_all(&ciphertext).unwrap();
}

pub fn decrypt_file(input_path: &str, output_path: &str, cipher: ChaCha20Poly1305) {
    let mut encrypted_file = File::open(input_path).unwrap();
    let mut nonce_bytes = [0u8; 12];
    encrypted_file.read_exact(&mut nonce_bytes).unwrap();
    let nonce = Nonce::from_slice(&nonce_bytes);

    let mut ciphertext = Vec::new();
    encrypted_file.read_to_end(&mut ciphertext).unwrap();

    let decrypted_data = cipher.decrypt(&nonce, ciphertext.as_ref()).unwrap();

    let mut decrypted_file = File::create(output_path).unwrap();
    decrypted_file.write_all(&decrypted_data).unwrap();
}

#[cfg(feature = "quantum")]
pub fn generate_cipher(
    shared_secret: &SharedSecret,
) -> Result<(ChaCha20Poly1305, Nonce), Box<dyn std::error::Error>> {
    let shared_secret_slice = shared_secret.as_ref();
    let key = Key::from_slice(shared_secret_slice);
    let cipher = ChaCha20Poly1305::new(key);
    let nonce = generate_nonce();

    Ok((cipher, nonce))
}

#[cfg(not(feature = "quantum"))]
pub fn generate_cipher(
    _shared_secret: &[u8],
) -> Result<(ChaCha20Poly1305, Nonce), Box<dyn std::error::Error>> {
    let key = Key::from_slice(&[0u8; 32]);
    let cipher = ChaCha20Poly1305::new(key);
    let nonce = generate_nonce();

    Ok((cipher, nonce))
}

#[cfg(feature = "quantum")]
#[cfg(test)]
mod tests {
    use super::*;
    use chacha20poly1305::Key;

    #[test]
    fn test_generate_cipher() {
        let kem = oqs::kem::Kem::new(oqs::kem::Algorithm::Kyber512).unwrap();
        let (pk, sk) = kem.keypair().unwrap();
        let (ct, shared_secret) = kem.encapsulate(&pk).unwrap();
        let (cipher, nonce) = generate_cipher(&shared_secret).unwrap();
        assert_eq!(nonce.as_slice().len(), 12);
    }

    #[test]
    fn test_encrypt_decrypt_message() {
        let mut message = [0u8; 24];
        let text = b"Hello, world!";
        message[..text.len()].copy_from_slice(text);

        let cipher = ChaCha20Poly1305::new(&Key::from_slice(&[0u8; 32]));
        let nonce = generate_nonce();
        let encrypted_message = encrypt_message(&message, cipher.clone(), nonce);
        let decrypted_message = decrypt_message(encrypted_message, cipher.clone(), nonce);

        assert_eq!(&message[..], &decrypted_message[..]);
    }
}
