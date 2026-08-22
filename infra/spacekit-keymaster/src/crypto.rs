use aes_gcm::{
    aead::{Aead, KeyInit, Payload},
    Aes256Gcm, Nonce,
};
use anyhow::{anyhow, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use hkdf::Hkdf;
use sha2::Sha256;
use zeroize::Zeroizing;

use crate::types::{Envelope, Hex32, SealedBlob};

pub fn b64_encode(data: &[u8]) -> String {
    B64.encode(data)
}

pub fn b64_decode(s: &str) -> Result<Vec<u8>> {
    B64.decode(s).map_err(|e| anyhow!("base64 decode: {e}"))
}

pub fn hex32_from_bytes(b: &[u8]) -> Hex32 {
    format!("0x{}", hex::encode(b))
}

pub fn envelope_aad(e: &Envelope) -> Vec<u8> {
    format!(
        "{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
        e.v,
        e.suite,
        e.keystore_id,
        e.shard_index,
        e.n,
        e.t,
        e.subject,
        e.guardian_kid,
        e.kem_ct,
        e.nonce,
        e.created_at
    )
    .into_bytes()
}

pub fn blob_aad(b: &SealedBlob) -> Vec<u8> {
    format!(
        "{}|{}|{}|{}|{}",
        b.v, b.suite, b.keystore_id, b.subject, b.nonce
    )
    .into_bytes()
}

pub fn shard_info(subject: &str, keystore_id: &str, shard_index: u32) -> Vec<u8> {
    format!("skkm/v1|{subject}|{keystore_id}|{shard_index}").into_bytes()
}

pub fn session_info(session_id: &str, shard_index: u32) -> Vec<u8> {
    format!("skkm/v1/session|{session_id}|{shard_index}").into_bytes()
}

pub fn hkdf32(ikm: &[u8], info: &[u8]) -> Result<Zeroizing<[u8; 32]>> {
    let hk = Hkdf::<Sha256>::new(Some(&[0u8; 32]), ikm);
    let mut okm = Zeroizing::new([0u8; 32]);
    hk.expand(info, okm.as_mut())
        .map_err(|_| anyhow!("hkdf expand failed"))?;
    Ok(okm)
}

pub fn aes_gcm_decrypt(key: &[u8; 32], nonce: &[u8], ct: &[u8], aad: &[u8]) -> Result<Vec<u8>> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| anyhow!("{e}"))?;
    let nonce = Nonce::from_slice(nonce);
    cipher
        .decrypt(nonce, Payload { msg: ct, aad })
        .map_err(|_| anyhow!("aes-gcm decrypt failed"))
}

pub fn aes_gcm_encrypt(key: &[u8; 32], plaintext: &[u8], aad: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| anyhow!("{e}"))?;
    let nonce = rand::random::<[u8; 12]>();
    let ct = cipher
        .encrypt(Nonce::from_slice(&nonce), Payload { msg: plaintext, aad })
        .map_err(|_| anyhow!("aes-gcm encrypt failed"))?;
    Ok((nonce.to_vec(), ct))
}

pub fn derive_shard_key(ss: &[u8], subject: &str, keystore_id: &str, shard_index: u32) -> Result<Zeroizing<[u8; 32]>> {
    hkdf32(ss, &shard_info(subject, keystore_id, shard_index))
}

pub fn derive_session_key(ss: &[u8], session_id: &str, shard_index: u32) -> Result<Zeroizing<[u8; 32]>> {
    hkdf32(ss, &session_info(session_id, shard_index))
}
