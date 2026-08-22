use alloy_primitives::Address;
use ecies::{decrypt, encrypt, utils::generate_keypair, PublicKey, SecretKey};

use std::error::Error;
use std::fs::File;
use std::io::{Read, Write};

use crate::v1::utils::read_hex_from_file;

/// Generate New PrivateKey and PublicKey
pub fn new_keypair() -> Result<(SecretKey, PublicKey), Box<dyn Error>> {
    let (sk, pk) = generate_keypair();

    // Validate key sizes
    if sk.serialize().len() != 32 {
        return Err("Invalid private key length".into());
    }
    if pk.serialize().len() != 65 {
        return Err("Invalid public key length".into());
    }

    Ok((sk, pk))
}

/// Ethereum address (EIP-55 checksummed) derived from the uncompressed secp256k1 public key,
/// same convention as standard wallets (Keccak-256 of 64-byte `x || y`, last 20 bytes).
pub fn ethereum_address_from_ecies_public_key(
    pub_key: &PublicKey,
) -> Result<String, Box<dyn Error>> {
    let bytes = pub_key.serialize();
    if bytes.len() != 65 {
        return Err("invalid public key length for EVM address derivation".into());
    }
    if bytes[0] != 0x04 {
        return Err("expected uncompressed public key (0x04 prefix)".into());
    }
    Ok(Address::from_raw_public_key(&bytes[1..]).to_string())
}

/// Encrypt message using ECIES
pub fn ecies_encrypt(
    receiver_pubkey: &PublicKey,
    message: &[u8],
) -> Result<(Vec<u8>, PublicKey, Vec<u8>), Box<dyn std::error::Error>> {
    let encrypted = encrypt(&receiver_pubkey.serialize(), message).map_err(|e| {
        Box::new(std::io::Error::new(
            std::io::ErrorKind::Other,
            e.to_string(),
        ))
    })?;
    Ok((encrypted, *receiver_pubkey, vec![]))
}

/// Decrypt message using ECIES
pub fn ecies_decrypt(
    receiver_secret: &SecretKey,
    _ephemeral_pubkey: &PublicKey,
    ciphertext: &[u8],
    _nonce: &[u8],
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    Ok(
        decrypt(&receiver_secret.serialize(), ciphertext).map_err(|e| {
            Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            ))
        })?,
    )
}

pub fn encrypt_message(
    receiver_pubkey: &PublicKey,
    message: &[u8],
) -> Result<(Vec<u8>, PublicKey, Vec<u8>), Box<dyn std::error::Error>> {
    ecies_encrypt(receiver_pubkey, message)
}

pub fn decrypt_message(
    receiver_secret: &SecretKey,
    ephemeral_pubkey: &PublicKey,
    ciphertext: &[u8],
    nonce: &[u8],
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    ecies_decrypt(receiver_secret, ephemeral_pubkey, ciphertext, nonce)
}

/// Encrypt File with Public Key
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

    // Read the hex string from the file
    let hex_string = read_hex_from_file(public_key_path)?;
    println!("Public key read from file: {}", hex_string);
    let public_key_bytes = hex::decode(hex_string.trim())
        .map_err(|e| format!("Failed to decode public key hex: {}", e))?;
    if public_key_bytes.len() != 33 && public_key_bytes.len() != 65 {
        return Err(format!(
            "ECIES public key must be 33 or 65 bytes (got {}); use `spacekit keypair -a ecies --save` or pass `-p` to an ECIES key file",
            public_key_bytes.len()
        )
        .into());
    }

    let encrypted_data = encrypt(&public_key_bytes, &file_data)
        .map_err(|e| format!("ECIES encrypt failed: {:?}", e))?;

    // Write the encrypted data to a file
    let mut output = File::create(output_path)?;
    output.write_all(&encrypted_data)?;

    Ok(())
}

/// Decrypt File with Private Key
pub fn decrypt_file(
    file_path: &str,
    secret_key_path: &str,
    output_path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Decrypting file: {}", file_path);

    // Read the binary file
    let mut file = File::open(file_path)?;
    let mut file_data = Vec::new();
    file.read_to_end(&mut file_data)?;

    // Read the hex string from the file
    let hex_string = read_hex_from_file(secret_key_path)?;
    println!("Secret key read from file: {}", hex_string);
    let secret_key_bytes = hex::decode(hex_string.trim())
        .map_err(|e| format!("Failed to decode secret key hex: {}", e))?;
    if secret_key_bytes.len() != 32 {
        return Err(format!(
            "ECIES secret key must be 32 bytes (got {}); use `spacekit keypair -a ecies --save` or pass the matching ECIES private key",
            secret_key_bytes.len()
        )
        .into());
    }

    let decrypted_data = decrypt(&secret_key_bytes, &file_data)
        .map_err(|e| format!("ECIES decrypt failed: {:?}", e))?;

    // Write the decrypted data to a file
    let mut decrypted_output = File::create(output_path)?;
    decrypted_output.write_all(&decrypted_data)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt() {
        let (secret_key, public_key) = new_keypair().unwrap();
        let message = b"Hello, EVM world!";
        let (ciphertext, ephemeral_pubkey, nonce) = encrypt_message(&public_key, message).unwrap();
        let decrypted =
            decrypt_message(&secret_key, &ephemeral_pubkey, &ciphertext, &nonce).unwrap();
        assert_eq!(decrypted, message);
    }
}
