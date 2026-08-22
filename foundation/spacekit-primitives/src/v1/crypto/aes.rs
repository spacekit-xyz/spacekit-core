use aes_gcm::{
    aead::{generic_array::typenum::U12, Aead, KeyInit},
    Aes256Gcm, Key, Nonce,
};
#[cfg(feature = "quantum")]
use oqs::kem::SharedSecret;
use rand::rngs::OsRng;
use rand::RngCore;
use std::{
    fs::File,
    io::{Read, Write},
};

pub fn generate_nonce() -> Nonce<U12> {
    let mut nonce_bytes = [0u8; 12]; // (96-bits/12-bytes)
    OsRng.fill_bytes(&mut nonce_bytes);
    Nonce::clone_from_slice(&nonce_bytes)
}

pub fn encrypt_message(
    secret_message: &[u8; 24],
    cipher: &Aes256Gcm,
    nonce: Nonce<U12>,
) -> Vec<u8> {
    cipher.encrypt(&nonce, secret_message.as_ref()).unwrap()
}

pub fn decrypt_message(ciphertext: Vec<u8>, cipher: &Aes256Gcm, nonce: Nonce<U12>) -> Vec<u8> {
    cipher.decrypt(&nonce, ciphertext.as_ref()).unwrap()
}

pub fn encrypt_file(input_path: &str, output_path: &str, cipher: Aes256Gcm) {
    let mut file = File::open(input_path).unwrap();
    let mut file_data = Vec::new();
    file.read_to_end(&mut file_data).unwrap();

    // Encrypt the data
    let nonce = generate_nonce();

    // Encrypt the data
    let ciphertext = cipher.encrypt(&nonce, file_data.as_ref()).unwrap();

    // Write the encrypted data and nonce to a file
    let mut encrypted_file = File::create(output_path).unwrap();
    encrypted_file.write_all(&nonce.as_ref()).unwrap();
    encrypted_file.write_all(&ciphertext).unwrap();
}

pub fn decrypt_file(input_path: &str, output_path: &str, cipher: Aes256Gcm) {
    let mut encrypted_file = File::open(input_path).unwrap();
    let mut nonce_bytes = [0u8; 12];
    encrypted_file.read_exact(&mut nonce_bytes).unwrap();
    let nonce = Nonce::clone_from_slice(&nonce_bytes);

    let mut ciphertext = Vec::new();
    encrypted_file.read_to_end(&mut ciphertext).unwrap();

    let decrypted_data = cipher.decrypt(&nonce, ciphertext.as_ref()).unwrap();

    let mut decrypted_file = File::create(output_path).unwrap();
    decrypted_file.write_all(&decrypted_data).unwrap();
}

#[cfg(feature = "quantum")]
pub fn generate_cipher(
    shared_secret: &SharedSecret,
) -> Result<(Aes256Gcm, Nonce<U12>), Box<dyn std::error::Error>> {
    let shared_secret_slice = shared_secret.as_ref();
    let key = Key::<Aes256Gcm>::clone_from_slice(shared_secret_slice);
    let cipher = Aes256Gcm::new(&key);
    let nonce = generate_nonce();

    Ok((cipher, nonce))
}

#[cfg(not(feature = "quantum"))]
pub fn generate_cipher(
    _shared_secret: &[u8],
) -> Result<(Aes256Gcm, Nonce<U12>), Box<dyn std::error::Error>> {
    let key = Key::<Aes256Gcm>::clone_from_slice(&[0u8; 32]);
    let cipher = Aes256Gcm::new(&key);
    let nonce = generate_nonce();

    Ok((cipher, nonce))
}

#[cfg(feature = "quantum")]
#[cfg(test)]
mod tests {
    use super::*;
    use hex;
    use oqs::kem::Algorithm::Kyber512;
    use oqs::kem::Kem;

    #[test]
    fn test_generate_nonce() {
        let nonce = generate_nonce();
        println!("nonce: {:?}", hex::encode(nonce.as_slice()));
        assert_eq!(nonce.as_slice().len(), 12);
    }

    #[test]
    fn test_generate_cipher() {
        let kem = oqs::kem::Kem::new(oqs::kem::Algorithm::Kyber512).unwrap();
        let (pk, sk) = kem.keypair().unwrap();
        let (ct, shared_secret) = kem.encapsulate(&pk).unwrap();
        let (cipher, nonce) = generate_cipher(&shared_secret).unwrap();
        assert_eq!(nonce.as_slice().len(), 12);
    }

    #[test]
    fn test_encrypt_decrypt() {
        let key = Aes256Gcm::generate_key(&mut OsRng);
        // Create a 24-byte message by padding with spaces
        let mut secret_message = [0u8; 24];
        let message = b"Hello, world!";
        secret_message[..message.len()].copy_from_slice(message);

        let cipher = Aes256Gcm::new(&key);
        let nonce = generate_nonce();
        let encrypted = encrypt_message(&secret_message, &cipher, nonce);
        let decrypted = decrypt_message(encrypted, &cipher, nonce);
        assert_eq!(&decrypted[..message.len()], message);
    }
}
