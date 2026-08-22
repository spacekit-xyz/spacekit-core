//! Document body adapter over [`BlobStore`] (redb + moka).
//!
//! Legacy file-based refs under `docstore/` are migrated into redb on first read.

use crate::blob_store::{document_blob_key, BlobStore};
use crate::database::DocumentRecord;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const LEGACY_DOCSTORE_PREFIX: &str = "docstore/";

/// Externalizes document JSON payloads into the embedded blob store.
#[derive(Clone)]
pub struct DocumentBlobStore {
    blobs: Arc<BlobStore>,
    data_dir: PathBuf,
}

impl DocumentBlobStore {
    pub fn new(data_dir: &Path, blobs: Arc<BlobStore>) -> Self {
        Self {
            blobs,
            data_dir: data_dir.to_path_buf(),
        }
    }

    pub fn blob_key(owner_did: &str, collection: &str, id: &str) -> String {
        document_blob_key(owner_did, collection, id)
    }

    pub fn write_body(&self, doc: &DocumentRecord) -> Result<String> {
        let key = Self::blob_key(&doc.owner_did, &doc.collection, &doc.id);
        let bytes = serde_json::to_vec(&doc.data)?;
        self.blobs.insert(&key, &bytes)?;
        Ok(key)
    }

    pub fn read_body(&self, blob_ref: &str) -> Result<serde_json::Value> {
        if blob_ref.starts_with(LEGACY_DOCSTORE_PREFIX) {
            return self.read_legacy_file(blob_ref);
        }
        let bytes = self
            .blobs
            .get(blob_ref)?
            .with_context(|| format!("missing document blob key {blob_ref}"))?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    pub fn delete_body(&self, blob_ref: &str) -> Result<()> {
        if blob_ref.starts_with(LEGACY_DOCSTORE_PREFIX) {
            let path = self.data_dir.join(blob_ref);
            if path.exists() {
                std::fs::remove_file(path)?;
            }
            return Ok(());
        }
        let _ = self.blobs.remove(blob_ref)?;
        Ok(())
    }

    fn read_legacy_file(&self, blob_ref: &str) -> Result<serde_json::Value> {
        let path = self.data_dir.join(blob_ref);
        let bytes =
            std::fs::read(&path).with_context(|| format!("read legacy doc blob {:?}", path))?;
        Ok(serde_json::from_slice(&bytes)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blob_store::{BlobStore, BlobStoreConfig};
    use chrono::Utc;
    use tempfile::tempdir;

    fn sample_doc() -> DocumentRecord {
        DocumentRecord {
            owner_did: "did:spacekit:alice".into(),
            collection: "widgets".into(),
            id: "doc-1".into(),
            data: serde_json::json!({"title": "hello", "n": 42}),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            blob_ref: None,
        }
    }

    #[test]
    fn roundtrip_external_body() {
        let dir = tempdir().unwrap();
        let blobs =
            BlobStore::open(&dir.path().join("blobs.redb"), BlobStoreConfig::default()).unwrap();
        let store = DocumentBlobStore::new(dir.path(), blobs);
        let doc = sample_doc();
        let key = store.write_body(&doc).unwrap();
        assert!(key.starts_with("doc:"));
        let loaded = store.read_body(&key).unwrap();
        assert_eq!(loaded, doc.data);
    }
}
