//! Envelope encryption: client-side encrypt/decrypt with chunked streaming support.
//!
//! **Security model**: the storage node never sees plaintext or private keys.
//!
//! ## Upload (client-side)
//! 1. Generate a random 32-byte file key.
//! 2. KEM-encrypt the file key to the owner's public key → `EncryptedFileKey`.
//! 3. Split plaintext into fixed-size chunks, AES-256-GCM encrypt each with the file key.
//! 4. Serialize [`EnvelopeHeader`] + encrypted chunks → send to server as opaque blob.
//!
//! ## Download (client-side)
//! 1. Authenticate via challenge-response (server encrypts a nonce with the owner pubkey,
//!    client decrypts to prove key possession).
//! 2. Server streams the opaque envelope bytes.
//! 3. Client decrypts `encrypted_file_key` from the header (KEM+AES hybrid) → file key.
//! 4. Decrypt chunks incrementally with the file key.
//!
//! ## Browser compatibility
//! The `EncryptedFileKey` uses three separate hex fields (`kem_ciphertext_hex`,
//! `nonce_hex`, `ciphertext_hex`) matching the Kyber WASM API that returns
//! `{kemCiphertextBase64, nonceBase64, ciphertextBase64}`. Both the Rust CLI
//! (OQS KEM + AES-GCM) and the browser (kyber_wasm + WebCrypto) can produce
//! and consume this format.

use serde::{Deserialize, Serialize};

// ───────────────────── Envelope on-disk / wire format ─────────────────────

pub const ENVELOPE_VERSION: u8 = 1;

/// Default plaintext chunk size: 256 KiB.
pub const DEFAULT_CHUNK_SIZE: u32 = 256 * 1024;

pub const NONCE_SIZE: usize = 12;
pub const TAG_SIZE: usize = 16;

/// The file key encrypted with the owner's KEM public key.
///
/// Format mirrors the Kyber WASM output so both Rust and browser clients
/// can produce / consume it without format conversion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedFileKey {
    /// KEM ciphertext from encapsulation (hex).
    pub kem_ciphertext_hex: String,
    /// AES-256-GCM nonce used to encrypt the file key (hex, 12 bytes).
    pub nonce_hex: String,
    /// AES-256-GCM ciphertext of the 32-byte file key (hex, 48 bytes = 32 + 16 tag).
    pub ciphertext_hex: String,
}

/// Per-chunk metadata stored in the envelope header.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkMeta {
    pub offset: u64,
    pub encrypted_size: u32,
    pub nonce_hex: String,
}

/// Envelope header: serialised as JSON, length-prefixed on disk.
///
/// Wire layout: `[header_len: u64 LE][header JSON][chunk_0 bytes][chunk_1 bytes]…`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvelopeHeader {
    pub version: u8,
    pub kem_algorithm: String,
    pub cipher_suite: String,
    /// The file encryption key, encrypted with the owner's KEM public key.
    pub encrypted_file_key: EncryptedFileKey,
    pub chunk_size: u32,
    pub total_chunks: u32,
    pub total_plaintext_size: u64,
    /// BLAKE3 hash of the original plaintext (hex).
    pub plaintext_hash: String,
    pub chunks: Vec<ChunkMeta>,
}

// ───────────────────── Challenge-response auth ─────────────────────

/// Stored server-side for a pending challenge.
#[derive(Debug, Clone)]
pub struct PendingChallenge {
    pub challenge_id: String,
    pub file_id: String,
    /// The plaintext challenge nonce the client must return to prove key possession.
    pub challenge_nonce: Vec<u8>,
    /// The requester's public key (raw bytes) for re-encryption on stream.
    pub requester_public_key: Vec<u8>,
    pub created_at: std::time::SystemTime,
    pub expires_at: std::time::SystemTime,
}

/// The encrypted challenge that the client must decrypt.
/// Uses the same KEM+AES hybrid as `EncryptedFileKey` so both Rust and browser
/// clients can decrypt it with their existing Kyber decrypt path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedChallenge {
    /// KEM ciphertext (hex).
    pub kem_ciphertext_hex: String,
    /// AES-GCM nonce (hex).
    pub nonce_hex: String,
    /// AES-GCM ciphertext of the challenge nonce (hex).
    pub ciphertext_hex: String,
}

/// JSON response from `GET /files/{id}/challenge`.
#[derive(Debug, Serialize, Deserialize)]
pub struct ChallengeResponse {
    pub success: bool,
    pub challenge_id: Option<String>,
    /// The encrypted challenge the client must decrypt and return.
    pub encrypted_challenge: Option<EncryptedChallenge>,
    pub error: Option<String>,
}

// ───────────────────── Helpers ─────────────────────

/// Serialize an [`EnvelopeHeader`] into a length-prefixed frame.
pub fn serialize_header(header: &EnvelopeHeader) -> Result<Vec<u8>, serde_json::Error> {
    let json = serde_json::to_vec(header)?;
    let len = (json.len() as u64).to_le_bytes();
    let mut buf = Vec::with_capacity(8 + json.len());
    buf.extend_from_slice(&len);
    buf.extend_from_slice(&json);
    Ok(buf)
}

/// Deserialize an [`EnvelopeHeader`] from a length-prefixed frame.
pub fn deserialize_header(data: &[u8]) -> Result<(EnvelopeHeader, usize), EnvelopeError> {
    if data.len() < 8 {
        return Err(EnvelopeError::HeaderTooShort);
    }
    let len = u64::from_le_bytes(data[..8].try_into().unwrap()) as usize;
    if data.len() < 8 + len {
        return Err(EnvelopeError::HeaderTruncated {
            expected: 8 + len,
            got: data.len(),
        });
    }
    let header: EnvelopeHeader =
        serde_json::from_slice(&data[8..8 + len]).map_err(EnvelopeError::HeaderJson)?;
    if header.version != ENVELOPE_VERSION {
        return Err(EnvelopeError::UnsupportedVersion(header.version));
    }
    Ok((header, 8 + len))
}

// ───────────────────── Encrypt / Decrypt (requires `quantum` + `aes-gcm`) ─────────────────────

/// Encrypt plaintext into a complete envelope byte blob (header + encrypted chunks).
///
/// Runs **on the client** (Rust CLI). The browser does the equivalent in JS/WASM.
#[cfg(feature = "quantum")]
pub fn encrypt_envelope(
    plaintext: &[u8],
    owner_public_key: &[u8],
    kem_algorithm: &str,
    chunk_size: Option<u32>,
) -> Result<Vec<u8>, EnvelopeError> {
    encrypt_envelope_sourced(plaintext, owner_public_key, kem_algorithm, chunk_size, None)
}

/// Like [`encrypt_envelope`] but with an explicit key-source hint.
#[cfg(feature = "quantum")]
pub fn encrypt_envelope_sourced(
    plaintext: &[u8],
    owner_public_key: &[u8],
    kem_algorithm: &str,
    chunk_size: Option<u32>,
    key_source: Option<KeySource>,
) -> Result<Vec<u8>, EnvelopeError> {
    use aes_gcm::{
        aead::{Aead, AeadCore, KeyInit, OsRng},
        Aes256Gcm,
    };

    let chunk_sz = chunk_size.unwrap_or(DEFAULT_CHUNK_SIZE) as usize;

    let file_key: [u8; 32] = {
        let mut key = [0u8; 32];
        aes_gcm::aead::OsRng.fill_bytes(&mut key);
        key
    };

    let encrypted_file_key =
        kem_encrypt_bytes_hybrid_sourced(&file_key, owner_public_key, kem_algorithm, key_source)?;

    let cipher = Aes256Gcm::new_from_slice(&file_key)
        .map_err(|e| EnvelopeError::Cipher(format!("AES init: {}", e)))?;

    let plaintext_hash = hex::encode(blake3::hash(plaintext).as_bytes());

    let mut chunks_meta = Vec::new();
    let mut chunks_data = Vec::new();
    let mut data_offset: u64 = 0;

    for chunk_plaintext in plaintext.chunks(chunk_sz) {
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let encrypted = cipher
            .encrypt(&nonce, chunk_plaintext)
            .map_err(|e| EnvelopeError::Cipher(format!("AES encrypt: {}", e)))?;

        chunks_meta.push(ChunkMeta {
            offset: data_offset,
            encrypted_size: encrypted.len() as u32,
            nonce_hex: hex::encode(nonce),
        });
        data_offset += encrypted.len() as u64;
        chunks_data.push(encrypted);
    }

    let header = EnvelopeHeader {
        version: ENVELOPE_VERSION,
        kem_algorithm: kem_algorithm.to_string(),
        cipher_suite: "AES-256-GCM".to_string(),
        encrypted_file_key,
        chunk_size: chunk_sz as u32,
        total_chunks: chunks_meta.len() as u32,
        total_plaintext_size: plaintext.len() as u64,
        plaintext_hash,
        chunks: chunks_meta,
    };

    let mut envelope = serialize_header(&header).map_err(EnvelopeError::HeaderSerialize)?;
    for chunk in chunks_data {
        envelope.extend_from_slice(&chunk);
    }
    Ok(envelope)
}

/// Decrypt a complete envelope byte blob back to plaintext.
///
/// Runs **on the client** (Rust CLI). The browser does the equivalent in JS/WASM.
#[cfg(feature = "quantum")]
pub fn decrypt_envelope(
    envelope_bytes: &[u8],
    private_key: &[u8],
) -> Result<Vec<u8>, EnvelopeError> {
    use aes_gcm::{
        aead::{Aead, KeyInit},
        Aes256Gcm, Nonce,
    };

    let (header, header_size) = deserialize_header(envelope_bytes)?;
    let data_section = &envelope_bytes[header_size..];

    // Decrypt the file key
    let file_key = kem_decrypt_file_key_hybrid(
        &header.encrypted_file_key,
        private_key,
        &header.kem_algorithm,
    )?;

    let cipher = Aes256Gcm::new_from_slice(&file_key)
        .map_err(|e| EnvelopeError::Cipher(format!("AES init: {}", e)))?;

    let mut plaintext = Vec::with_capacity(header.total_plaintext_size as usize);

    for chunk_meta in &header.chunks {
        let start = chunk_meta.offset as usize;
        let end = start + chunk_meta.encrypted_size as usize;
        if end > data_section.len() {
            return Err(EnvelopeError::ChunkOutOfBounds {
                chunk_end: end,
                data_len: data_section.len(),
            });
        }
        let encrypted_chunk = &data_section[start..end];
        let nonce_bytes = hex::decode(&chunk_meta.nonce_hex)
            .map_err(|e| EnvelopeError::Cipher(format!("nonce hex: {}", e)))?;
        let nonce = Nonce::from_slice(&nonce_bytes);
        let decrypted = cipher
            .decrypt(nonce, encrypted_chunk)
            .map_err(|e| EnvelopeError::Cipher(format!("AES decrypt chunk: {}", e)))?;
        plaintext.extend_from_slice(&decrypted);
    }

    let computed_hash = hex::encode(blake3::hash(&plaintext).as_bytes());
    if computed_hash != header.plaintext_hash {
        return Err(EnvelopeError::IntegrityMismatch {
            expected: header.plaintext_hash,
            computed: computed_hash,
        });
    }

    Ok(plaintext)
}

/// Encrypt a single chunk given the file key. For streaming encryption.
#[cfg(feature = "quantum")]
pub fn encrypt_chunk_with_key(
    file_key: &[u8; 32],
    plaintext_chunk: &[u8],
    nonce_hex: &str,
) -> Result<Vec<u8>, EnvelopeError> {
    use aes_gcm::{
        aead::{Aead, KeyInit},
        Aes256Gcm, Nonce,
    };

    let cipher = Aes256Gcm::new_from_slice(file_key)
        .map_err(|e| EnvelopeError::Cipher(format!("AES init: {}", e)))?;
    let nonce_bytes =
        hex::decode(nonce_hex).map_err(|e| EnvelopeError::Cipher(format!("nonce hex: {}", e)))?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    cipher
        .encrypt(nonce, plaintext_chunk)
        .map_err(|e| EnvelopeError::Cipher(format!("AES encrypt: {}", e)))
}

/// Encrypt a single chunk, generating a fresh nonce. Returns (ciphertext, nonce_hex).
#[cfg(feature = "quantum")]
pub fn encrypt_chunk_with_key_generated_nonce(
    file_key: &[u8; 32],
    plaintext_chunk: &[u8],
) -> Result<(Vec<u8>, String), EnvelopeError> {
    use aes_gcm::{
        aead::{Aead, AeadCore, KeyInit, OsRng},
        Aes256Gcm,
    };

    let cipher = Aes256Gcm::new_from_slice(file_key)
        .map_err(|e| EnvelopeError::Cipher(format!("AES init: {}", e)))?;
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let encrypted = cipher
        .encrypt(&nonce, plaintext_chunk)
        .map_err(|e| EnvelopeError::Cipher(format!("AES encrypt: {}", e)))?;
    Ok((encrypted, hex::encode(nonce)))
}

/// Decrypt a single chunk given the file key. For streaming decryption.
#[cfg(feature = "quantum")]
pub fn decrypt_chunk_with_key(
    file_key: &[u8; 32],
    encrypted_chunk: &[u8],
    nonce_hex: &str,
) -> Result<Vec<u8>, EnvelopeError> {
    use aes_gcm::{
        aead::{Aead, KeyInit},
        Aes256Gcm, Nonce,
    };

    let cipher = Aes256Gcm::new_from_slice(file_key)
        .map_err(|e| EnvelopeError::Cipher(format!("AES init: {}", e)))?;
    let nonce_bytes =
        hex::decode(nonce_hex).map_err(|e| EnvelopeError::Cipher(format!("nonce hex: {}", e)))?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    cipher
        .decrypt(nonce, encrypted_chunk)
        .map_err(|e| EnvelopeError::Cipher(format!("AES decrypt: {}", e)))
}

// ───────────────────── KEM + AES hybrid helpers ─────────────────────

/// KEM-encrypt a small payload (e.g. file key or challenge nonce).
/// Returns the three components separately for browser WASM compatibility.
#[cfg(feature = "quantum")]
pub fn kem_encrypt_bytes(
    data: &[u8],
    public_key: &[u8],
    kem_algorithm: &str,
) -> Result<EncryptedFileKey, EnvelopeError> {
    use aes_gcm::{
        aead::{Aead, AeadCore, KeyInit, OsRng},
        Aes256Gcm, Nonce,
    };

    let oqs_alg = parse_oqs_algorithm(kem_algorithm)?;
    let kem = oqs::kem::Kem::new(oqs_alg)
        .map_err(|e| EnvelopeError::Kem(format!("create KEM: {}", e)))?;
    let pk = kem
        .public_key_from_bytes(public_key)
        .ok_or_else(|| EnvelopeError::Kem("invalid public key".into()))?;
    let (kem_ct, shared_secret) = kem
        .encapsulate(&pk)
        .map_err(|e| EnvelopeError::Kem(format!("encapsulate: {}", e)))?;

    // Derive AES key from shared secret (same derivation as quantum.rs)
    let ss = shared_secret.as_ref();
    let aes_key: Vec<u8> = if ss.len() >= 32 {
        ss[..32].to_vec()
    } else {
        let mut k = ss.to_vec();
        k.extend_from_slice(&blake3::hash(ss).as_bytes()[..32 - ss.len()]);
        k
    };

    let cipher = Aes256Gcm::new_from_slice(&aes_key)
        .map_err(|e| EnvelopeError::Cipher(format!("AES init: {}", e)))?;
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let ciphertext = cipher
        .encrypt(&nonce, data)
        .map_err(|e| EnvelopeError::Cipher(format!("AES encrypt: {}", e)))?;

    Ok(EncryptedFileKey {
        kem_ciphertext_hex: hex::encode(kem_ct.as_ref()),
        nonce_hex: hex::encode(nonce),
        ciphertext_hex: hex::encode(ciphertext),
    })
}

/// KEM-decrypt a small payload encrypted with [`kem_encrypt_bytes`].
#[cfg(feature = "quantum")]
pub fn kem_decrypt_bytes(
    encrypted: &EncryptedFileKey,
    private_key: &[u8],
    kem_algorithm: &str,
) -> Result<Vec<u8>, EnvelopeError> {
    use aes_gcm::{
        aead::{Aead, KeyInit},
        Aes256Gcm, Nonce,
    };

    let oqs_alg = parse_oqs_algorithm(kem_algorithm)?;
    let kem = oqs::kem::Kem::new(oqs_alg)
        .map_err(|e| EnvelopeError::Kem(format!("create KEM: {}", e)))?;
    let kem_ct_bytes = hex::decode(&encrypted.kem_ciphertext_hex)
        .map_err(|e| EnvelopeError::Kem(format!("kem_ct hex: {}", e)))?;
    tracing::debug!(
        "kem_decrypt_bytes: algo={}, sk_len={}, ct_len={}, ct_fingerprint={}",
        kem_algorithm,
        private_key.len(),
        kem_ct_bytes.len(),
        hex::encode(&blake3::hash(&kem_ct_bytes).as_bytes()[..8])
    );
    let kem_ct = kem
        .ciphertext_from_bytes(&kem_ct_bytes)
        .ok_or_else(|| EnvelopeError::Kem("invalid KEM ciphertext".into()))?;
    let sk = kem
        .secret_key_from_bytes(private_key)
        .ok_or_else(|| EnvelopeError::Kem("invalid secret key".into()))?;
    let shared_secret = kem
        .decapsulate(&sk, &kem_ct)
        .map_err(|e| EnvelopeError::Kem(format!("decapsulate: {}", e)))?;

    let ss = shared_secret.as_ref();
    tracing::debug!(
        "kem_decrypt_bytes: shared_secret_len={}, ss_fingerprint={}, sk_fingerprint={}",
        ss.len(),
        hex::encode(&blake3::hash(ss).as_bytes()[..8]),
        hex::encode(&blake3::hash(private_key).as_bytes()[..8])
    );
    let aes_key: Vec<u8> = if ss.len() >= 32 {
        ss[..32].to_vec()
    } else {
        let mut k = ss.to_vec();
        k.extend_from_slice(&blake3::hash(ss).as_bytes()[..32 - ss.len()]);
        k
    };

    let cipher = Aes256Gcm::new_from_slice(&aes_key)
        .map_err(|e| EnvelopeError::Cipher(format!("AES init: {}", e)))?;
    let nonce_bytes = hex::decode(&encrypted.nonce_hex)
        .map_err(|e| EnvelopeError::Cipher(format!("nonce hex: {}", e)))?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ct_bytes = hex::decode(&encrypted.ciphertext_hex)
        .map_err(|e| EnvelopeError::Cipher(format!("ct hex: {}", e)))?;
    cipher
        .decrypt(nonce, ct_bytes.as_ref())
        .map_err(|e| EnvelopeError::Cipher(format!("AES decrypt: {}", e)))
}

// ───────────────────── Browser-compatible KEM (pqcrypto-kyber) ─────────────────────
//
// OQS and pqcrypto-kyber implement different versions of Kyber with
// incompatible KEM ciphertexts/shared secrets. Since the browser WASM uses
// pqcrypto-kyber, the server must use the same library when encrypting
// *to the browser* (challenge nonces, re-encrypted envelopes).

/// KEM-encrypt a small payload using pqcrypto-kyber (browser-compatible).
#[cfg(feature = "quantum")]
pub fn pqcrypto_kem_encrypt_bytes(
    data: &[u8],
    public_key: &[u8],
) -> Result<EncryptedFileKey, EnvelopeError> {
    use aes_gcm::{
        aead::{Aead, AeadCore, KeyInit, OsRng},
        Aes256Gcm, Nonce,
    };
    use pqcrypto_kyber::kyber1024;
    use pqcrypto_traits::kem::{Ciphertext, PublicKey, SharedSecret};

    let pk = kyber1024::PublicKey::from_bytes(public_key)
        .map_err(|e| EnvelopeError::Kem(format!("pqcrypto pk decode: {:?}", e)))?;
    let (shared_secret, kem_ct) = kyber1024::encapsulate(&pk);

    let ss = shared_secret.as_bytes();
    let aes_key: Vec<u8> = if ss.len() >= 32 {
        ss[..32].to_vec()
    } else {
        let mut k = ss.to_vec();
        k.extend_from_slice(&blake3::hash(ss).as_bytes()[..32 - ss.len()]);
        k
    };

    let cipher = Aes256Gcm::new_from_slice(&aes_key)
        .map_err(|e| EnvelopeError::Cipher(format!("AES init: {}", e)))?;
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let ciphertext = cipher
        .encrypt(&nonce, data)
        .map_err(|e| EnvelopeError::Cipher(format!("AES encrypt: {}", e)))?;

    Ok(EncryptedFileKey {
        kem_ciphertext_hex: hex::encode(kem_ct.as_bytes()),
        nonce_hex: hex::encode(nonce),
        ciphertext_hex: hex::encode(ciphertext),
    })
}

/// KEM-decrypt using pqcrypto-kyber (matches [`pqcrypto_kem_encrypt_bytes`] and browser WASM).
#[cfg(feature = "quantum")]
pub fn pqcrypto_kem_decrypt_bytes(
    encrypted: &EncryptedFileKey,
    private_key: &[u8],
) -> Result<Vec<u8>, EnvelopeError> {
    use aes_gcm::{
        aead::{Aead, KeyInit},
        Aes256Gcm, Nonce,
    };
    use pqcrypto_kyber::kyber1024;
    use pqcrypto_traits::kem::{Ciphertext, SecretKey, SharedSecret};

    let sk = kyber1024::SecretKey::from_bytes(private_key)
        .map_err(|e| EnvelopeError::Kem(format!("pqcrypto sk decode: {:?}", e)))?;
    let kem_ct_bytes = hex::decode(&encrypted.kem_ciphertext_hex)
        .map_err(|e| EnvelopeError::Kem(format!("kem_ct hex: {}", e)))?;
    let kem_ct = kyber1024::Ciphertext::from_bytes(&kem_ct_bytes)
        .map_err(|e| EnvelopeError::Kem(format!("pqcrypto kem ct: {:?}", e)))?;
    let shared_secret = kyber1024::decapsulate(&kem_ct, &sk);
    let ss = shared_secret.as_bytes();
    let aes_key: Vec<u8> = if ss.len() >= 32 {
        ss[..32].to_vec()
    } else {
        let mut k = ss.to_vec();
        k.extend_from_slice(&blake3::hash(ss).as_bytes()[..32 - ss.len()]);
        k
    };

    let cipher = Aes256Gcm::new_from_slice(&aes_key)
        .map_err(|e| EnvelopeError::Cipher(format!("AES init: {}", e)))?;
    let nonce_bytes = hex::decode(&encrypted.nonce_hex)
        .map_err(|e| EnvelopeError::Cipher(format!("nonce hex: {}", e)))?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ct_bytes = hex::decode(&encrypted.ciphertext_hex)
        .map_err(|e| EnvelopeError::Cipher(format!("ct hex: {}", e)))?;
    cipher
        .decrypt(nonce, ct_bytes.as_ref())
        .map_err(|e| EnvelopeError::Cipher(format!("AES decrypt: {}", e)))
}

/// Key source hint: which KEM library produced the key material.
/// When known, the hybrid functions skip the wrong library entirely, avoiding
/// silent shared-secret mismatches between OQS and pqcrypto-kyber.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KeySource {
    Oqs,
    Pqcrypto,
}

/// Decrypt the file key using the matching KEM library.
/// When `key_source` is `Some`, only the matching library is tried.
/// When `None`, tries OQS first then pqcrypto (backwards-compatible probe).
#[cfg(feature = "quantum")]
fn kem_decrypt_file_key_hybrid(
    encrypted: &EncryptedFileKey,
    private_key: &[u8],
    kem_algorithm: &str,
) -> Result<Vec<u8>, EnvelopeError> {
    kem_decrypt_file_key_hybrid_sourced(encrypted, private_key, kem_algorithm, None)
}

#[cfg(feature = "quantum")]
pub fn kem_decrypt_file_key_hybrid_sourced(
    encrypted: &EncryptedFileKey,
    private_key: &[u8],
    kem_algorithm: &str,
    key_source: Option<KeySource>,
) -> Result<Vec<u8>, EnvelopeError> {
    let lower = kem_algorithm.to_ascii_lowercase();
    let is_kyber1024 = lower.contains("kyber1024");

    match key_source {
        Some(KeySource::Oqs) => {
            return kem_decrypt_bytes(encrypted, private_key, kem_algorithm);
        }
        Some(KeySource::Pqcrypto) if is_kyber1024 => {
            return pqcrypto_kem_decrypt_bytes(encrypted, private_key);
        }
        Some(KeySource::Pqcrypto) => {
            return kem_decrypt_bytes(encrypted, private_key, kem_algorithm);
        }
        None => {}
    }

    // Unknown source: try OQS first (most keys are OQS-generated), then pqcrypto
    match kem_decrypt_bytes(encrypted, private_key, kem_algorithm) {
        Ok(pt) => return Ok(pt),
        Err(e) => {
            if is_kyber1024 {
                tracing::debug!(
                    "envelope decrypt: OQS file-key decrypt failed (trying pqcrypto): {}",
                    e
                );
            } else {
                return Err(e);
            }
        }
    }
    pqcrypto_kem_decrypt_bytes(encrypted, private_key)
}

/// KEM-encrypt the file key using the matching library.
/// When `key_source` is `Some`, only the matching library is tried.
/// When `None`, tries OQS first then pqcrypto (backwards-compatible probe).
#[cfg(feature = "quantum")]
fn kem_encrypt_bytes_hybrid(
    data: &[u8],
    public_key: &[u8],
    kem_algorithm: &str,
) -> Result<EncryptedFileKey, EnvelopeError> {
    kem_encrypt_bytes_hybrid_sourced(data, public_key, kem_algorithm, None)
}

#[cfg(feature = "quantum")]
pub fn kem_encrypt_bytes_hybrid_sourced(
    data: &[u8],
    public_key: &[u8],
    kem_algorithm: &str,
    key_source: Option<KeySource>,
) -> Result<EncryptedFileKey, EnvelopeError> {
    let lower = kem_algorithm.to_ascii_lowercase();
    let is_kyber1024 = lower.contains("kyber1024");

    match key_source {
        Some(KeySource::Oqs) => {
            return kem_encrypt_bytes(data, public_key, kem_algorithm);
        }
        Some(KeySource::Pqcrypto) if is_kyber1024 => {
            return pqcrypto_kem_encrypt_bytes(data, public_key);
        }
        Some(KeySource::Pqcrypto) => {
            return kem_encrypt_bytes(data, public_key, kem_algorithm);
        }
        None => {}
    }

    // Unknown source: try OQS first (most keys are OQS-generated), then pqcrypto
    match kem_encrypt_bytes(data, public_key, kem_algorithm) {
        Ok(enc) => return Ok(enc),
        Err(e) => {
            if is_kyber1024 {
                tracing::debug!("envelope encrypt: OQS KEM failed (trying pqcrypto): {}", e);
            } else {
                return Err(e);
            }
        }
    }
    pqcrypto_kem_encrypt_bytes(data, public_key)
}

/// Encrypt plaintext into a complete envelope using pqcrypto-kyber KEM (browser-compatible).
/// Used when re-encrypting server-side decrypted data for a browser requester.
#[cfg(feature = "quantum")]
pub fn pqcrypto_encrypt_envelope(
    plaintext: &[u8],
    requester_public_key: &[u8],
    chunk_size: Option<u32>,
) -> Result<Vec<u8>, EnvelopeError> {
    use aes_gcm::{
        aead::{Aead, AeadCore, KeyInit, OsRng},
        Aes256Gcm,
    };

    let chunk_sz = chunk_size.unwrap_or(DEFAULT_CHUNK_SIZE) as usize;

    let file_key: [u8; 32] = {
        let mut key = [0u8; 32];
        aes_gcm::aead::OsRng.fill_bytes(&mut key);
        key
    };

    let encrypted_file_key = pqcrypto_kem_encrypt_bytes(&file_key, requester_public_key)?;

    let cipher = Aes256Gcm::new_from_slice(&file_key)
        .map_err(|e| EnvelopeError::Cipher(format!("AES init: {}", e)))?;

    let plaintext_hash = hex::encode(blake3::hash(plaintext).as_bytes());

    let mut chunks_meta = Vec::new();
    let mut chunks_data = Vec::new();
    let mut data_offset: u64 = 0;

    for chunk_plaintext in plaintext.chunks(chunk_sz) {
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let encrypted = cipher
            .encrypt(&nonce, chunk_plaintext)
            .map_err(|e| EnvelopeError::Cipher(format!("AES encrypt: {}", e)))?;

        chunks_meta.push(ChunkMeta {
            offset: data_offset,
            encrypted_size: encrypted.len() as u32,
            nonce_hex: hex::encode(nonce),
        });
        data_offset += encrypted.len() as u64;
        chunks_data.push(encrypted);
    }

    let header = EnvelopeHeader {
        version: ENVELOPE_VERSION,
        kem_algorithm: "Kyber1024".to_string(),
        cipher_suite: "AES-256-GCM".to_string(),
        encrypted_file_key,
        chunk_size: chunk_sz as u32,
        total_chunks: chunks_meta.len() as u32,
        total_plaintext_size: plaintext.len() as u64,
        plaintext_hash,
        chunks: chunks_meta,
    };

    let mut envelope = serialize_header(&header).map_err(EnvelopeError::HeaderSerialize)?;
    for chunk in chunks_data {
        envelope.extend_from_slice(&chunk);
    }
    Ok(envelope)
}

// ───────────────────── OQS algorithm mapping ─────────────────────

#[cfg(feature = "quantum")]
fn parse_oqs_algorithm(name: &str) -> Result<oqs::kem::Algorithm, EnvelopeError> {
    let n = name.trim();
    let n = n.strip_prefix("Algorithm::").unwrap_or(n);
    match n {
        "Kyber512" | "kyber512" => Ok(oqs::kem::Algorithm::Kyber512),
        "Kyber768" | "kyber768" => Ok(oqs::kem::Algorithm::Kyber768),
        "Kyber1024" | "kyber1024" => Ok(oqs::kem::Algorithm::Kyber1024),
        _ => Err(EnvelopeError::Kem(format!(
            "unsupported KEM algorithm: {}",
            name
        ))),
    }
}

#[cfg(feature = "quantum")]
use aes_gcm::aead::rand_core::RngCore;

// ───────────────────── Key encoding helpers ─────────────────────

/// Decode a public key from either hex or base64.
/// Browser WASM clients send base64; Rust CLI clients send hex.
pub fn decode_public_key_flexible(key_str: &str) -> Result<Vec<u8>, EnvelopeError> {
    if let Ok(bytes) = hex::decode(key_str) {
        if !bytes.is_empty() {
            return Ok(bytes);
        }
    }
    use base64::Engine;
    if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(key_str) {
        if !bytes.is_empty() {
            return Ok(bytes);
        }
    }
    if let Ok(bytes) = base64::engine::general_purpose::URL_SAFE.decode(key_str) {
        if !bytes.is_empty() {
            return Ok(bytes);
        }
    }
    Err(EnvelopeError::Kem(format!(
        "could not decode public key as hex or base64 (len={})",
        key_str.len()
    )))
}

// ───────────────────── Server-side envelope decryption ─────────────────────

/// Max nested PQ envelopes peeled in `/stream` and admin-stream (same server key per layer).
pub const SERVER_ENVELOPE_PEEL_MAX_LAYERS: usize = 16;

/// Decrypt an envelope blob using the server's private key.
/// Returns the plaintext bytes.
#[cfg(feature = "quantum")]
pub fn decrypt_envelope_server_side(
    envelope_bytes: &[u8],
    server_secret_key: &[u8],
    kem_algorithm: &str,
) -> Result<Vec<u8>, EnvelopeError> {
    decrypt_envelope_server_side_sourced(envelope_bytes, server_secret_key, kem_algorithm, None)
}

/// Like [`decrypt_envelope_server_side`] but with an explicit key-source hint.
#[cfg(feature = "quantum")]
pub fn decrypt_envelope_server_side_sourced(
    envelope_bytes: &[u8],
    server_secret_key: &[u8],
    kem_algorithm: &str,
    key_source: Option<KeySource>,
) -> Result<Vec<u8>, EnvelopeError> {
    use aes_gcm::{
        aead::{Aead, KeyInit},
        Aes256Gcm, Nonce,
    };

    let (header, header_size) = deserialize_header(envelope_bytes)?;
    let data_section = &envelope_bytes[header_size..];

    let file_key = kem_decrypt_file_key_hybrid_sourced(
        &header.encrypted_file_key,
        server_secret_key,
        kem_algorithm,
        key_source,
    )?;

    let cipher = Aes256Gcm::new_from_slice(&file_key)
        .map_err(|e| EnvelopeError::Cipher(format!("AES init: {}", e)))?;

    let mut plaintext = Vec::with_capacity(header.total_plaintext_size as usize);

    for chunk_meta in &header.chunks {
        let start = chunk_meta.offset as usize;
        let end = start + chunk_meta.encrypted_size as usize;
        if end > data_section.len() {
            return Err(EnvelopeError::ChunkOutOfBounds {
                chunk_end: end,
                data_len: data_section.len(),
            });
        }
        let encrypted_chunk = &data_section[start..end];
        let nonce_bytes = hex::decode(&chunk_meta.nonce_hex)
            .map_err(|e| EnvelopeError::Cipher(format!("nonce hex: {}", e)))?;
        let nonce = Nonce::from_slice(&nonce_bytes);
        let decrypted = cipher
            .decrypt(nonce, encrypted_chunk)
            .map_err(|e| EnvelopeError::Cipher(format!("AES decrypt chunk: {}", e)))?;
        plaintext.extend_from_slice(&decrypted);
    }

    let computed_hash = hex::encode(blake3::hash(&plaintext).as_bytes());
    if computed_hash != header.plaintext_hash {
        return Err(EnvelopeError::IntegrityMismatch {
            expected: header.plaintext_hash,
            computed: computed_hash,
        });
    }

    Ok(plaintext)
}

/// Like [`decrypt_envelope_server_side`], but if the decrypted bytes are still a PQ envelope
/// (same server key), decrypt again until the payload is not envelope-shaped or `max_layers`
/// is reached.
///
/// This handles on-disk blobs that were envelope-encrypted more than once to the storage key
/// (misconfigured uploads / legacy paths). Without peeling, `/stream` would re-wrap the inner
/// envelope and the browser would see ciphertext-after-one-client-decrypt.
#[cfg(feature = "quantum")]
pub fn decrypt_envelope_server_side_peel(
    envelope_bytes: &[u8],
    server_secret_key: &[u8],
    kem_algorithm: &str,
    max_layers: usize,
) -> Result<Vec<u8>, EnvelopeError> {
    decrypt_envelope_server_side_peel_sourced(
        envelope_bytes,
        server_secret_key,
        kem_algorithm,
        max_layers,
        None,
    )
}

#[cfg(feature = "quantum")]
pub fn decrypt_envelope_server_side_peel_sourced(
    envelope_bytes: &[u8],
    server_secret_key: &[u8],
    kem_algorithm: &str,
    max_layers: usize,
    key_source: Option<KeySource>,
) -> Result<Vec<u8>, EnvelopeError> {
    if max_layers == 0 {
        return Err(EnvelopeError::NestedEnvelopeTooDeep { max_layers: 0 });
    }
    let mut current = envelope_bytes.to_vec();
    for layer in 0..max_layers {
        let pt = decrypt_envelope_server_side_sourced(
            &current,
            server_secret_key,
            kem_algorithm,
            key_source,
        )?;
        if deserialize_header(&pt).is_err() {
            if layer > 0 {
                tracing::info!(
                    "Peeled {} nested PQ envelope layer(s); final plaintext {} bytes",
                    layer,
                    pt.len()
                );
            }
            return Ok(pt);
        }
        tracing::info!(
            "Peeling PQ envelope layer {} ({} bytes) — inner still envelope-shaped",
            layer + 1,
            current.len()
        );
        current = pt;
    }
    Err(EnvelopeError::NestedEnvelopeTooDeep { max_layers })
}

// ───────────────────── Header-only re-wrap (delivery path) ─────────────────────

/// Re-wrap only the envelope header's file key to a recipient's pqcrypto Kyber PK.
/// The encrypted chunk data section is unchanged — O(recipient_pk) not O(plaintext).
#[cfg(feature = "quantum")]
pub fn rewrap_envelope_header_to_pqcrypto_recipient(
    envelope_bytes: &[u8],
    server_secret_key: &[u8],
    kem_algorithm: &str,
    key_source: Option<KeySource>,
    recipient_public_key: &[u8],
) -> Result<(Vec<u8>, usize), EnvelopeError> {
    let (mut header, header_size) = deserialize_header(envelope_bytes)?;

    let file_key = kem_decrypt_file_key_hybrid_sourced(
        &header.encrypted_file_key,
        server_secret_key,
        kem_algorithm,
        key_source,
    )?;

    header.encrypted_file_key = pqcrypto_kem_encrypt_bytes(&file_key, recipient_public_key)?;
    header.kem_algorithm = "Kyber1024".to_string();

    let new_header = serialize_header(&header).map_err(EnvelopeError::HeaderSerialize)?;
    Ok((new_header, header_size))
}

/// Read an exact byte range from a file (for chunked envelope I/O).
#[cfg(feature = "quantum")]
pub async fn read_file_byte_range(
    path: &std::path::Path,
    offset: u64,
    len: u64,
) -> Result<Vec<u8>, EnvelopeError> {
    use tokio::io::{AsyncReadExt, AsyncSeekExt, SeekFrom};

    if len == 0 {
        return Ok(Vec::new());
    }
    if len > usize::MAX as u64 {
        return Err(EnvelopeError::Kem(format!("byte range too large: {len}")));
    }
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|e| EnvelopeError::Kem(format!("open {}: {}", path.display(), e)))?;
    file.seek(SeekFrom::Start(offset))
        .await
        .map_err(|e| EnvelopeError::Kem(format!("seek {}: {}", path.display(), e)))?;
    let mut buf = vec![0u8; len as usize];
    file.read_exact(&mut buf)
        .await
        .map_err(|e| EnvelopeError::Kem(format!("read {} bytes @ {offset}: {}", len, e)))?;
    Ok(buf)
}

/// Decrypt the per-file AES key from an envelope header using the server secret key.
#[cfg(feature = "quantum")]
pub fn server_file_key_from_header(
    header: &EnvelopeHeader,
    server_secret_key: &[u8],
    kem_algorithm: &str,
    key_source: Option<KeySource>,
) -> Result<[u8; 32], EnvelopeError> {
    let key = kem_decrypt_file_key_hybrid_sourced(
        &header.encrypted_file_key,
        server_secret_key,
        kem_algorithm,
        key_source,
    )?;
    if key.len() != 32 {
        return Err(EnvelopeError::Kem(format!(
            "file key length {} (expected 32)",
            key.len()
        )));
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&key);
    Ok(out)
}

/// Read the length-prefixed envelope header from disk (bounded; does not load ciphertext).
#[cfg(feature = "quantum")]
pub async fn read_envelope_header_prefix(
    path: &std::path::Path,
) -> Result<(Vec<u8>, usize), EnvelopeError> {
    use tokio::io::AsyncReadExt;

    const MAX_HEADER_JSON: usize = 4 * 1024 * 1024;

    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|e| EnvelopeError::Kem(format!("open envelope: {}", e)))?;
    let mut len_buf = [0u8; 8];
    file.read_exact(&mut len_buf)
        .await
        .map_err(|e| EnvelopeError::Kem(format!("read header len: {}", e)))?;
    let json_len = u64::from_le_bytes(len_buf) as usize;
    if json_len > MAX_HEADER_JSON {
        return Err(EnvelopeError::Kem(format!(
            "envelope header JSON too large: {} bytes",
            json_len
        )));
    }
    let header_size = 8 + json_len;
    let mut prefix = vec![0u8; header_size];
    prefix[..8].copy_from_slice(&len_buf);
    file.read_exact(&mut prefix[8..])
        .await
        .map_err(|e| EnvelopeError::Kem(format!("read header JSON: {}", e)))?;
    Ok((prefix, header_size))
}

#[cfg(feature = "quantum")]
fn decrypt_chunk_ciphertext(
    chunk_meta: &ChunkMeta,
    encrypted_chunk: &[u8],
    file_key: &[u8],
) -> Result<Vec<u8>, EnvelopeError> {
    use aes_gcm::{
        aead::{Aead, KeyInit},
        Aes256Gcm, Nonce,
    };

    let cipher = Aes256Gcm::new_from_slice(file_key)
        .map_err(|e| EnvelopeError::Cipher(format!("AES init: {}", e)))?;
    let nonce_bytes = hex::decode(&chunk_meta.nonce_hex)
        .map_err(|e| EnvelopeError::Cipher(format!("nonce hex: {}", e)))?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    cipher
        .decrypt(nonce, encrypted_chunk)
        .map_err(|e| EnvelopeError::Cipher(format!("AES decrypt chunk: {}", e)))
}

/// True when the first decrypted chunk is itself a PQ envelope (double-wrapped blob).
/// Single-layer deploy blobs (normal brains) return false.
#[cfg(feature = "quantum")]
pub async fn envelope_on_disk_is_nested(
    path: &std::path::Path,
    header_prefix: &[u8],
    header_size: usize,
    server_secret_key: &[u8],
    kem_algorithm: &str,
    key_source: Option<KeySource>,
) -> Result<bool, EnvelopeError> {
    use tokio::io::{AsyncReadExt, AsyncSeekExt, SeekFrom};

    let (header, _) = deserialize_header(header_prefix)?;
    if header.chunks.is_empty() {
        return Ok(false);
    }
    let chunk_meta = &header.chunks[0];
    let enc_size = chunk_meta.encrypted_size as usize;

    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|e| EnvelopeError::Kem(format!("open nested probe: {}", e)))?;
    file.seek(SeekFrom::Start(
        (header_size + chunk_meta.offset as usize) as u64,
    ))
    .await
    .map_err(|e| EnvelopeError::Kem(format!("seek nested probe: {}", e)))?;
    let mut encrypted_chunk = vec![0u8; enc_size];
    file.read_exact(&mut encrypted_chunk)
        .await
        .map_err(|e| EnvelopeError::Kem(format!("read first chunk: {}", e)))?;

    let file_key = kem_decrypt_file_key_hybrid_sourced(
        &header.encrypted_file_key,
        server_secret_key,
        kem_algorithm,
        key_source,
    )?;
    let first_plain = decrypt_chunk_ciphertext(chunk_meta, &encrypted_chunk, &file_key)?;
    Ok(deserialize_header(&first_plain).is_ok())
}

/// Peel one outer PQ envelope layer on disk into a temp file, then atomically replace `path`.
///
/// Writes the outer plaintext (the inner envelope) chunk-by-chunk so peak RAM stays O(chunk).
/// Caller must have already verified the blob is nested.
#[cfg(feature = "quantum")]
async fn peel_one_nested_layer_on_disk(
    path: &std::path::Path,
    header_prefix: &[u8],
    header_size: usize,
    server_secret_key: &[u8],
    kem_algorithm: &str,
    key_source: Option<KeySource>,
) -> Result<(), EnvelopeError> {
    use tokio::io::AsyncWriteExt;

    let (header, _) = deserialize_header(header_prefix)?;
    let file_key =
        server_file_key_from_header(&header, server_secret_key, kem_algorithm, key_source)?;

    let parent = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("envelope");
    let tmp = parent.join(format!(".{}.peel-{}.tmp", file_name, uuid::Uuid::new_v4()));

    let write_result: Result<(), EnvelopeError> = async {
        let mut out = tokio::fs::File::create(&tmp)
            .await
            .map_err(|e| EnvelopeError::Kem(format!("create peel temp: {}", e)))?;
        let mut hasher = blake3::Hasher::new();
        let base = header_size as u64;

        for chunk_meta in &header.chunks {
            let encrypted = read_file_byte_range(
                path,
                base + chunk_meta.offset as u64,
                chunk_meta.encrypted_size as u64,
            )
            .await?;
            let pt = decrypt_chunk_with_key(&file_key, &encrypted, &chunk_meta.nonce_hex)?;
            hasher.update(&pt);
            out.write_all(&pt)
                .await
                .map_err(|e| EnvelopeError::Kem(format!("write peel temp: {}", e)))?;
        }
        out.flush()
            .await
            .map_err(|e| EnvelopeError::Kem(format!("flush peel temp: {}", e)))?;

        let computed = hex::encode(hasher.finalize().as_bytes());
        if computed != header.plaintext_hash {
            return Err(EnvelopeError::IntegrityMismatch {
                expected: header.plaintext_hash.clone(),
                computed,
            });
        }
        Ok(())
    }
    .await;

    if let Err(e) = write_result {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(e);
    }

    tokio::fs::rename(&tmp, path).await.map_err(|e| {
        EnvelopeError::Kem(format!("rename peel temp over {}: {}", path.display(), e))
    })?;

    tracing::info!(
        "Peeled one nested PQ envelope layer on disk at {} (bounded RAM)",
        path.display()
    );
    Ok(())
}

/// Peel nested PQ envelope layers on disk until a single-layer envelope remains.
///
/// Returns the number of layers removed. No-op (returns 0) when already single-layer.
/// Peak memory is O(chunk size), not O(file size).
#[cfg(feature = "quantum")]
pub async fn peel_nested_envelope_on_disk(
    path: &std::path::Path,
    server_secret_key: &[u8],
    kem_algorithm: &str,
    key_source: Option<KeySource>,
    max_layers: usize,
) -> Result<usize, EnvelopeError> {
    if max_layers == 0 {
        return Err(EnvelopeError::NestedEnvelopeTooDeep { max_layers: 0 });
    }

    let mut peeled = 0usize;
    for _ in 0..max_layers {
        let (header_prefix, header_size) = read_envelope_header_prefix(path).await?;
        if !envelope_on_disk_is_nested(
            path,
            &header_prefix,
            header_size,
            server_secret_key,
            kem_algorithm,
            key_source,
        )
        .await?
        {
            return Ok(peeled);
        }
        peel_one_nested_layer_on_disk(
            path,
            &header_prefix,
            header_size,
            server_secret_key,
            kem_algorithm,
            key_source,
        )
        .await?;
        peeled += 1;
    }

    let (header_prefix, header_size) = read_envelope_header_prefix(path).await?;
    if envelope_on_disk_is_nested(
        path,
        &header_prefix,
        header_size,
        server_secret_key,
        kem_algorithm,
        key_source,
    )
    .await?
    {
        return Err(EnvelopeError::NestedEnvelopeTooDeep { max_layers });
    }
    Ok(peeled)
}

// ───────────────────── Key rotation ─────────────────────

/// Re-wrap an envelope's file key from the old server key to a new server key.
///
/// Only the header is rewritten; the encrypted chunk data is untouched since
/// it uses the same symmetric file key.  Returns the new envelope bytes.
#[cfg(feature = "quantum")]
pub fn rewrap_envelope(
    envelope_bytes: &[u8],
    old_secret_key: &[u8],
    new_public_key: &[u8],
    kem_algorithm: &str,
) -> Result<Vec<u8>, EnvelopeError> {
    let (mut header, header_size) = deserialize_header(envelope_bytes)?;
    let data_section = &envelope_bytes[header_size..];

    let file_key = kem_decrypt_bytes(&header.encrypted_file_key, old_secret_key, kem_algorithm)?;

    header.encrypted_file_key = kem_encrypt_bytes(&file_key, new_public_key, kem_algorithm)?;

    let mut out = serialize_header(&header).map_err(|e| EnvelopeError::HeaderSerialize(e))?;
    out.extend_from_slice(data_section);
    Ok(out)
}

// ───────────────────── Errors ─────────────────────

#[derive(Debug, thiserror::Error)]
pub enum EnvelopeError {
    #[error("envelope header too short (need at least 8 bytes)")]
    HeaderTooShort,
    #[error("envelope header truncated: expected {expected} bytes, got {got}")]
    HeaderTruncated { expected: usize, got: usize },
    #[error("envelope header JSON: {0}")]
    HeaderJson(serde_json::Error),
    #[error("envelope header serialization: {0}")]
    HeaderSerialize(serde_json::Error),
    #[error("unsupported envelope version: {0}")]
    UnsupportedVersion(u8),
    #[error("KEM error: {0}")]
    Kem(String),
    #[error("cipher error: {0}")]
    Cipher(String),
    #[error("chunk out of bounds: chunk ends at {chunk_end}, data section is {data_len} bytes")]
    ChunkOutOfBounds { chunk_end: usize, data_len: usize },
    #[error("integrity mismatch: expected {expected}, computed {computed}")]
    IntegrityMismatch { expected: String, computed: String },
    #[error("PQ envelope nested deeper than max_layers ({max_layers})")]
    NestedEnvelopeTooDeep { max_layers: usize },
}

#[cfg(all(test, feature = "quantum"))]
mod server_side_peel_tests {
    use super::*;
    use oqs::kem::{Algorithm, Kem};

    fn kyber1024_keypair() -> (Vec<u8>, Vec<u8>) {
        let kem = Kem::new(Algorithm::Kyber1024).expect("Kyber1024 KEM");
        let (pk, sk) = kem.keypair().expect("keypair");
        (pk.into_vec(), sk.into_vec())
    }

    #[tokio::test]
    async fn peel_nested_envelope_on_disk_two_layers() {
        let (pk, sk) = kyber1024_keypair();
        let algo = "kyber1024";
        let inner_plain = b"pretend-wasm-payload-on-disk".to_vec();

        let inner_env = encrypt_envelope(&inner_plain, &pk, algo, None).expect("inner encrypt");
        let outer_env = encrypt_envelope(&inner_env, &pk, algo, None).expect("outer encrypt");

        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("nested.bin");
        tokio::fs::write(&path, &outer_env).await.expect("write");

        let peeled_layers = peel_nested_envelope_on_disk(
            &path,
            &sk,
            algo,
            Some(KeySource::Oqs),
            SERVER_ENVELOPE_PEEL_MAX_LAYERS,
        )
        .await
        .expect("peel on disk");
        assert_eq!(peeled_layers, 1);

        let on_disk = tokio::fs::read(&path).await.expect("read peeled");
        assert_eq!(on_disk, inner_env);

        let again = peel_nested_envelope_on_disk(
            &path,
            &sk,
            algo,
            Some(KeySource::Oqs),
            SERVER_ENVELOPE_PEEL_MAX_LAYERS,
        )
        .await
        .expect("second peel is no-op");
        assert_eq!(again, 0);

        let pt = decrypt_envelope_server_side(&on_disk, &sk, algo).expect("decrypt single layer");
        assert_eq!(pt, inner_plain);
    }

    /// Double-wrapped on-disk blobs (two PQ layers to the same server key) must peel to real
    /// plaintext before `/stream` re-wraps — otherwise the client still sees an inner envelope after
    /// one `decryptEnvelope`.
    #[test]
    fn peel_nested_envelope_two_layers() {
        let (pk, sk) = kyber1024_keypair();
        let algo = "kyber1024";
        let inner_plain = b"pretend-wasm-payload".to_vec();

        let inner_env = encrypt_envelope(&inner_plain, &pk, algo, None).expect("inner encrypt");
        let outer_env = encrypt_envelope(&inner_env, &pk, algo, None).expect("outer encrypt");

        let one_layer = decrypt_envelope_server_side(&outer_env, &sk, algo).expect("one layer");
        assert!(
            deserialize_header(&one_layer).is_ok(),
            "first decrypt should leave inner envelope bytes"
        );

        let peeled = decrypt_envelope_server_side_peel(
            &outer_env,
            &sk,
            algo,
            SERVER_ENVELOPE_PEEL_MAX_LAYERS,
        )
        .expect("peel");
        assert_eq!(peeled, inner_plain);
    }

    #[test]
    fn peel_single_layer_is_identity() {
        let (pk, sk) = kyber1024_keypair();
        let algo = "kyber1024";
        let plain: Vec<u8> = (0u8..=63).collect();
        let env = encrypt_envelope(&plain, &pk, algo, None).expect("encrypt");
        let out =
            decrypt_envelope_server_side_peel(&env, &sk, algo, SERVER_ENVELOPE_PEEL_MAX_LAYERS)
                .expect("peel");
        assert_eq!(out, plain);
    }

    #[test]
    fn peel_respects_max_layers() {
        let (pk, sk) = kyber1024_keypair();
        let algo = "kyber1024";
        let mut blob = encrypt_envelope(b"x", &pk, algo, None).unwrap();
        for _ in 0..3 {
            blob = encrypt_envelope(&blob, &pk, algo, None).unwrap();
        }
        let err = decrypt_envelope_server_side_peel(&blob, &sk, algo, 2).unwrap_err();
        assert!(matches!(err, EnvelopeError::NestedEnvelopeTooDeep { .. }));
    }

    /// AWS SM / browser upload use pqcrypto Kyber1024; server-side peel must decapsulate with pqcrypto.
    #[test]
    fn peel_pqcrypto_server_key_envelope() {
        use pqcrypto_kyber::kyber1024;
        use pqcrypto_traits::kem::{PublicKey, SecretKey};
        let (pk, sk) = kyber1024::keypair();
        let pk_b = pk.as_bytes().to_vec();
        let sk_b = sk.as_bytes().to_vec();
        let plain = b"browser-style-envelope-to-pqcrypto-pk";
        let env = pqcrypto_encrypt_envelope(plain.as_slice(), &pk_b, None).expect("encrypt");
        let out = decrypt_envelope_server_side_peel(
            &env,
            &sk_b,
            "kyber1024",
            SERVER_ENVELOPE_PEEL_MAX_LAYERS,
        )
        .expect("peel");
        assert_eq!(out, plain.as_slice());
    }

    /// OQS keypair + explicit KeySource::Oqs roundtrip.
    /// This is the production path: SM key is OQS, CLI knows key_source.
    #[test]
    fn oqs_key_source_roundtrip() {
        let (pk, sk) = kyber1024_keypair();
        let plain = b"oqs-sourced-payload-for-server";
        let env = encrypt_envelope_sourced(
            plain.as_slice(),
            &pk,
            "Kyber1024",
            None,
            Some(KeySource::Oqs),
        )
        .expect("encrypt");
        let out =
            decrypt_envelope_server_side_sourced(&env, &sk, "Kyber1024", Some(KeySource::Oqs))
                .expect("decrypt");
        assert_eq!(out, plain.as_slice());
    }

    /// pqcrypto keypair + explicit KeySource::Pqcrypto roundtrip.
    #[test]
    fn pqcrypto_key_source_roundtrip() {
        use pqcrypto_kyber::kyber1024;
        use pqcrypto_traits::kem::{PublicKey, SecretKey};
        let (pk, sk) = kyber1024::keypair();
        let pk_b = pk.as_bytes().to_vec();
        let sk_b = sk.as_bytes().to_vec();
        let plain = b"pqcrypto-sourced-payload";
        let env = encrypt_envelope_sourced(
            plain.as_slice(),
            &pk_b,
            "Kyber1024",
            None,
            Some(KeySource::Pqcrypto),
        )
        .expect("encrypt");
        let out = decrypt_envelope_server_side_sourced(
            &env,
            &sk_b,
            "Kyber1024",
            Some(KeySource::Pqcrypto),
        )
        .expect("decrypt");
        assert_eq!(out, plain.as_slice());
    }

    /// OQS keypair + None source (probe mode) should work because probe tries OQS first.
    #[test]
    fn oqs_key_none_source_probe_roundtrip() {
        let (pk, sk) = kyber1024_keypair();
        let plain = b"probe-mode-should-still-work";
        let env = encrypt_envelope_sourced(plain.as_slice(), &pk, "Kyber1024", None, None)
            .expect("encrypt with None source");
        let out = decrypt_envelope_server_side_sourced(&env, &sk, "Kyber1024", None)
            .expect("decrypt with None source");
        assert_eq!(out, plain.as_slice());
    }

    /// OQS and pqcrypto both implement CRYSTALS-Kyber and are binary-compatible.
    /// Encrypting with pqcrypto to an OQS public key and decrypting with OQS works.
    /// The key_source tagging is still valuable as a correctness/clarity measure.
    #[test]
    fn cross_library_interop_works() {
        let (oqs_pk, oqs_sk) = kyber1024_keypair();
        let plain = b"cross-library-roundtrip";
        let env = encrypt_envelope_sourced(
            plain.as_slice(),
            &oqs_pk,
            "Kyber1024",
            None,
            Some(KeySource::Pqcrypto),
        )
        .expect("pqcrypto encrypt accepts OQS pk bytes");
        let out =
            decrypt_envelope_server_side_sourced(&env, &oqs_sk, "Kyber1024", Some(KeySource::Oqs))
                .expect("OQS can decrypt pqcrypto-encrypted envelope (same CRYSTALS-Kyber spec)");
        assert_eq!(out, plain.as_slice());
    }

    /// Full production path: OQS keygen → hex encode → decode_key_material → encrypt → decrypt.
    /// This mirrors the exact SM key lifecycle:
    ///   `spacekit keypair` (OQS hex) → SM stores hex → `decode_key_material` reads hex →
    ///   CLI `encrypt_envelope_sourced` with decoded pk → server `decrypt_envelope_server_side_sourced` with decoded sk.
    #[test]
    fn full_sm_hex_roundtrip_oqs() {
        let (pk, sk) = kyber1024_keypair();
        assert_eq!(pk.len(), 1568, "Kyber1024 pk must be 1568 bytes");
        assert_eq!(sk.len(), 3168, "Kyber1024 sk must be 3168 bytes");

        let pk_hex = hex::encode(&pk);
        let sk_hex = hex::encode(&sk);
        assert_eq!(pk_hex.len(), 3136);
        assert_eq!(sk_hex.len(), 6336);

        #[cfg(feature = "aws-secrets")]
        {
            let decoded_pk =
                crate::aws_secrets::decode_key_material(&pk_hex).expect("decode pk hex");
            let decoded_sk =
                crate::aws_secrets::decode_key_material(&sk_hex).expect("decode sk hex");
            assert_eq!(
                decoded_pk, pk,
                "decode_key_material must preserve pk bytes exactly"
            );
            assert_eq!(
                decoded_sk, sk,
                "decode_key_material must preserve sk bytes exactly"
            );
        }

        let plain = b"end-to-end SM roundtrip test payload: WASM module bytes here";

        let env = encrypt_envelope_sourced(
            plain.as_slice(),
            &pk,
            "Kyber1024",
            None,
            Some(KeySource::Oqs),
        )
        .expect("encrypt with OQS pk");

        let header = deserialize_header(&env).expect("header parse").0;
        assert_eq!(header.kem_algorithm, "Kyber1024");
        assert_eq!(header.total_plaintext_size, plain.len() as u64);

        let out =
            decrypt_envelope_server_side_sourced(&env, &sk, "Kyber1024", Some(KeySource::Oqs))
                .expect("decrypt with OQS sk must succeed");
        assert_eq!(out, plain.as_slice());
    }

    /// Same as above but tests the full peel path (the actual code path used in /stream).
    #[test]
    fn full_peel_path_roundtrip() {
        let (pk, sk) = kyber1024_keypair();
        let plain = b"peel-path-e2e-test-wasm-14465-bytes-would-go-here";

        let env = encrypt_envelope_sourced(
            plain.as_slice(),
            &pk,
            "Kyber1024",
            None,
            Some(KeySource::Oqs),
        )
        .expect("encrypt");

        let out = decrypt_envelope_server_side_peel_sourced(
            &env,
            &sk,
            "Kyber1024",
            SERVER_ENVELOPE_PEEL_MAX_LAYERS,
            Some(KeySource::Oqs),
        )
        .expect("peel decrypt must succeed");
        assert_eq!(out, plain.as_slice());
    }

    /// Verify KEM encrypt/decrypt roundtrip with hex-encoded keys (the file key exchange path).
    #[test]
    fn kem_encrypt_decrypt_file_key_roundtrip() {
        let (pk, sk) = kyber1024_keypair();
        let file_key: [u8; 32] = [0xAB; 32];

        let encrypted = kem_encrypt_bytes(&file_key, &pk, "Kyber1024").expect("kem encrypt");
        let decrypted = kem_decrypt_bytes(&encrypted, &sk, "Kyber1024").expect("kem decrypt");
        assert_eq!(decrypted, file_key.as_slice());
    }

    /// Header-only re-wrap preserves ciphertext chunks; recipient can decrypt without server re-encrypting payload.
    #[test]
    fn header_rewrap_to_pqcrypto_recipient_preserves_plaintext() {
        use pqcrypto_kyber::kyber1024;
        use pqcrypto_traits::kem::{PublicKey, SecretKey};

        let (server_pk, server_sk) = kyber1024::keypair();
        let (buyer_pk, buyer_sk) = kyber1024::keypair();
        let plain = vec![0x42u8; 500_000];

        let env = encrypt_envelope_sourced(
            &plain,
            server_pk.as_bytes(),
            "Kyber1024",
            None,
            Some(KeySource::Pqcrypto),
        )
        .expect("encrypt to server");

        let (new_header, old_header_size) = rewrap_envelope_header_to_pqcrypto_recipient(
            &env,
            server_sk.as_bytes(),
            "Kyber1024",
            Some(KeySource::Pqcrypto),
            buyer_pk.as_bytes(),
        )
        .expect("header rewrap");

        let header_len = new_header.len();
        let mut rewrapped = new_header;
        rewrapped.extend_from_slice(&env[old_header_size..]);
        assert_eq!(&rewrapped[header_len..], &env[old_header_size..]);

        let out = decrypt_envelope(&rewrapped, buyer_sk.as_bytes()).expect("buyer decrypt");
        assert_eq!(out, plain);
    }
}
