use aes_gcm::{
    aead::{generic_array::typenum::U12, Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Key, Nonce,
};
#[cfg(feature = "quantum")]
use oqs::kem::SharedSecret;
use std::{
    fs::File,
    io::{Read, Write},
};

pub fn encrypt_message(secret_message: &[u8; 24], cipher: Aes256Gcm, nonce: Nonce<U12>) -> Vec<u8> {
    let ciphertext = cipher.encrypt(&nonce, secret_message.as_ref()).unwrap();
    ciphertext
}

pub fn decrypt_message(ciphertext: Vec<u8>, cipher: Aes256Gcm, nonce: Nonce<U12>) -> Vec<u8> {
    let decrypted_message = cipher.decrypt(&nonce, ciphertext.as_ref()).unwrap();
    decrypted_message
}

pub fn encrypt_file(input_path: &str, output_path: &str, cipher: Aes256Gcm, nonce: Nonce<U12>) {
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

pub fn decrypt_file(input_path: &str, output_path: &str, cipher: Aes256Gcm) {
    // To decrypt, read the encrypted file and nonce
    let mut encrypted_file = File::open(input_path).unwrap();
    let mut nonce = [0u8; 12]; // 96 bits
    let mut ciphertext = Vec::new();
    encrypted_file.read_exact(&mut nonce).unwrap(); // recover nonce
    encrypted_file.read_to_end(&mut ciphertext).unwrap();

    // Decrypt the ciphertext
    let decrypted_data = cipher
        .decrypt(&Nonce::from_slice(&nonce), ciphertext.as_ref())
        .unwrap();

    // Write the decrypted data back to a file
    let mut decrypted_file = File::create(output_path).unwrap();
    decrypted_file.write_all(&decrypted_data).unwrap();
}

#[cfg(feature = "quantum")]
pub fn generate_cipher(
    shared_secret: &SharedSecret,
) -> Result<(Aes256Gcm, Nonce<U12>), Box<dyn std::error::Error>> {
    // Here we use AES-GCM for this purpose of symmetric encryption/decryption
    // using the shared secret

    // Convert the shared secret to a byte slice
    let shared_secret_slice = shared_secret.as_ref();
    // Alternatively, the key can be transformed directly from a byte slice
    // (panicks on length mismatch):
    let key = Key::<Aes256Gcm>::from_slice(shared_secret_slice);
    // The encryption key can be generated from alice shared secret
    let cipher = Aes256Gcm::new(&key);
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng); // 96-bits; unique per message

    println!("nonce:{:?}", hex::encode(nonce));

    Ok((cipher, nonce))
}
