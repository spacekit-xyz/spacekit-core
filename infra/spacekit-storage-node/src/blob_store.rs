//! Embedded blob store: `redb` on disk + bounded `moka` hot cache.
//!
//! Reads are lazy by nature (mmap + OS page cache). Only touched keys fault pages in.

use anyhow::{Context, Result};
use moka::sync::Cache;
use redb::{Database as RedbDatabase, ReadableTable, TableDefinition};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const BLOBS: TableDefinition<&str, &[u8]> = TableDefinition::new("blobs");

/// Configuration for the embedded blob store.
#[derive(Debug, Clone)]
pub struct BlobStoreConfig {
    /// Max bytes kept in the moka hot cache (LRU-ish eviction by weight).
    pub cache_max_bytes: u64,
}

impl Default for BlobStoreConfig {
    fn default() -> Self {
        Self {
            cache_max_bytes: 32 * 1024 * 1024,
        }
    }
}

/// Blobs larger than this are never inserted into the moka cache (stream from redb only).
const BLOB_CACHE_INSERT_MAX_BYTES: usize = 1024 * 1024;

/// Disk-backed blob store with a bounded in-memory cache in front.
#[derive(Debug)]
pub struct BlobStore {
    path: PathBuf,
    db: RedbDatabase,
    cache: Cache<String, Arc<Vec<u8>>>,
}

impl BlobStore {
    pub fn open(path: &Path, config: BlobStoreConfig) -> Result<Arc<Self>> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let db = RedbDatabase::create(path)
            .with_context(|| format!("open redb blob store {:?}", path))?;
        {
            let tx = db.begin_write()?;
            tx.open_table(BLOBS)?;
            tx.commit()?;
        }
        let cache = Cache::builder()
            .max_capacity(config.cache_max_bytes)
            .weigher(|_k, v: &Arc<Vec<u8>>| -> u32 { v.len().min(u32::MAX as usize) as u32 })
            .build();
        Ok(Arc::new(Self {
            path: path.to_path_buf(),
            db,
            cache,
        }))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        if let Some(cached) = self.cache.get(key) {
            return Ok(Some((*cached).clone()));
        }
        let tx = self.db.begin_read()?;
        let table = tx.open_table(BLOBS)?;
        let Some(value) = table.get(key)? else {
            return Ok(None);
        };
        let bytes = value.value().to_vec();
        if bytes.len() <= BLOB_CACHE_INSERT_MAX_BYTES {
            self.cache.insert(key.to_string(), Arc::new(bytes.clone()));
        }
        Ok(Some(bytes))
    }

    pub fn insert(&self, key: &str, value: &[u8]) -> Result<()> {
        let tx = self.db.begin_write()?;
        {
            let mut table = tx.open_table(BLOBS)?;
            table.insert(key, value)?;
        }
        tx.commit()?;
        if value.len() <= BLOB_CACHE_INSERT_MAX_BYTES {
            self.cache.insert(key.to_string(), Arc::new(value.to_vec()));
        }
        Ok(())
    }

    pub fn remove(&self, key: &str) -> Result<bool> {
        let tx = self.db.begin_write()?;
        let removed = {
            let mut table = tx.open_table(BLOBS)?;
            let removed = matches!(table.remove(key)?, Some(_));
            removed
        };
        tx.commit()?;
        if removed {
            self.cache.invalidate(key);
        }
        Ok(removed)
    }

    pub fn contains_key(&self, key: &str) -> Result<bool> {
        if self.cache.contains_key(key) {
            return Ok(true);
        }
        let tx = self.db.begin_read()?;
        let table = tx.open_table(BLOBS)?;
        Ok(table.get(key)?.is_some())
    }
}

/// Stable redb key for externalized document JSON bodies.
pub fn document_blob_key(owner_did: &str, collection: &str, id: &str) -> String {
    format!("doc:{}:{}:{}", owner_did, collection, id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn roundtrip_blob() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("blobs.redb");
        let store = BlobStore::open(&path, BlobStoreConfig::default()).unwrap();
        store.insert("user:42", b"hello").unwrap();
        let v = store.get("user:42").unwrap().unwrap();
        assert_eq!(v, b"hello");
        assert!(store.remove("user:42").unwrap());
        assert!(store.get("user:42").unwrap().is_none());
    }

    #[test]
    fn cache_survives_reopen() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("blobs.redb");
        {
            let store = BlobStore::open(&path, BlobStoreConfig::default()).unwrap();
            store.insert("k", b"v").unwrap();
        }
        let store = BlobStore::open(&path, BlobStoreConfig::default()).unwrap();
        assert_eq!(store.get("k").unwrap().unwrap(), b"v");
    }
}
