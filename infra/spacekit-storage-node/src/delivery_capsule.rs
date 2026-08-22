//! Owner-supplied delivery capsules for true E2E entitlement delivery.
//!
//! When a file envelope is encrypted to the **owner** Kyber key (not the storage
//! server), the node cannot unwrap the DEK. After `OP_GRANT` / `OP_PURCHASE`, the
//! owner posts an [`EncryptedFileKey`] wrapped to the recipient. `/rewrap` then
//! streams `[header with capsule EFK] ++ ciphertext` without ever seeing plaintext.

use crate::envelope::EncryptedFileKey;
use anyhow::{anyhow, Result};
use std::path::{Path, PathBuf};

/// Directory holding per-entitlement capsules for a file: `{file_id}.capsules/`.
pub fn capsules_dir(data_dir: &Path, file_id: &str) -> PathBuf {
    data_dir.join(format!("{file_id}.capsules"))
}

pub fn capsule_path(data_dir: &Path, file_id: &str, entitlement_id_hex: &str) -> PathBuf {
    let hex = entitlement_id_hex
        .trim_start_matches("0x")
        .to_ascii_lowercase();
    capsules_dir(data_dir, file_id).join(hex)
}

pub async fn store_delivery_capsule(
    data_dir: &Path,
    file_id: &str,
    entitlement_id_hex: &str,
    encrypted_file_key: &EncryptedFileKey,
) -> Result<PathBuf> {
    let dir = capsules_dir(data_dir, file_id);
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| anyhow!("create capsules dir: {e}"))?;
    let path = capsule_path(data_dir, file_id, entitlement_id_hex);
    let json =
        serde_json::to_vec(encrypted_file_key).map_err(|e| anyhow!("serialize capsule: {e}"))?;
    let tmp = path.with_extension("tmp");
    tokio::fs::write(&tmp, &json)
        .await
        .map_err(|e| anyhow!("write capsule temp: {e}"))?;
    tokio::fs::rename(&tmp, &path)
        .await
        .map_err(|e| anyhow!("rename capsule: {e}"))?;
    Ok(path)
}

pub async fn load_delivery_capsule(
    data_dir: &Path,
    file_id: &str,
    entitlement_id_hex: &str,
) -> Result<Option<EncryptedFileKey>> {
    let path = capsule_path(data_dir, file_id, entitlement_id_hex);
    match tokio::fs::read(&path).await {
        Ok(bytes) => {
            let efk: EncryptedFileKey = serde_json::from_slice(&bytes)
                .map_err(|e| anyhow!("parse capsule {}: {e}", path.display()))?;
            if efk.kem_ciphertext_hex.is_empty()
                || efk.nonce_hex.is_empty()
                || efk.ciphertext_hex.is_empty()
            {
                return Err(anyhow!("capsule missing EncryptedFileKey fields"));
            }
            Ok(Some(efk))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(anyhow!("read capsule: {e}")),
    }
}
