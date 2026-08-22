//! Persisted reverse index: `file_id` ← document artifacts in catalog collections.
//!
//! Maintained on document upsert/delete so orphan GC and safe `DELETE /files` do not
//! require scanning all listings.

use crate::database::DocumentRecord;
use anyhow::{Context, Result};
use redb::{Database as RedbDatabase, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const FILE_REFS: TableDefinition<&str, &[u8]> = TableDefinition::new("file_refs");
const DOC_ANCHORS: TableDefinition<&str, &[u8]> = TableDefinition::new("doc_anchors");
const FILE_HASH: TableDefinition<&str, &[u8]> = TableDefinition::new("file_hash");
const INDEX_META: TableDefinition<&str, &[u8]> = TableDefinition::new("index_meta");

const INDEX_VERSION: &str = "artifact_refs_v1";
const SEP: char = '\x1f';

/// Document collections whose JSON bodies may reference legacy `/files` blobs.
pub const ARTIFACT_REF_COLLECTIONS: &[&str] = &["app_listings", "deployments", "content_listings"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArtifactRefEntry {
    pub collection: String,
    pub doc_id: String,
    pub role: Option<String>,
    pub owner_did: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct DocAnchor {
    pub file_id: String,
    pub role: Option<String>,
}

#[derive(Debug)]
pub struct ArtifactRefIndex {
    path: PathBuf,
    db: RedbDatabase,
}

impl ArtifactRefIndex {
    pub fn open(path: &Path) -> Result<Arc<Self>> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let db = RedbDatabase::create(path)
            .with_context(|| format!("open artifact ref index {:?}", path))?;
        {
            let tx = db.begin_write()?;
            tx.open_table(FILE_REFS)?;
            tx.open_table(DOC_ANCHORS)?;
            tx.open_table(FILE_HASH)?;
            tx.open_table(INDEX_META)?;
            tx.commit()?;
        }
        Ok(Arc::new(Self {
            path: path.to_path_buf(),
            db,
        }))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn is_built(&self) -> Result<bool> {
        let tx = self.db.begin_read()?;
        let table = tx.open_table(INDEX_META)?;
        Ok(table.get(INDEX_VERSION)?.is_some())
    }

    pub fn mark_built(&self) -> Result<()> {
        let tx = self.db.begin_write()?;
        {
            let mut table = tx.open_table(INDEX_META)?;
            table.insert(INDEX_VERSION, "1".as_bytes())?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn clear_all(&self) -> Result<()> {
        let tx = self.db.begin_write()?;
        {
            let mut file_refs = tx.open_table(FILE_REFS)?;
            while file_refs.first()?.is_some() {
                file_refs.pop_first()?;
            }
            let mut anchors = tx.open_table(DOC_ANCHORS)?;
            while anchors.first()?.is_some() {
                anchors.pop_first()?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn document_anchor_key(owner_did: &str, collection: &str, doc_id: &str) -> String {
        format!("{owner_did}{SEP}{collection}{SEP}{doc_id}")
    }

    pub fn owner_hash_key(owner_did: &str, plaintext_hash: &str) -> String {
        format!("{owner_did}{SEP}{plaintext_hash}")
    }

    pub fn collection_tracks_refs(collection: &str) -> bool {
        ARTIFACT_REF_COLLECTIONS.contains(&collection)
    }

    /// Extract `(file_id, role)` pairs from listing/deploy JSON bodies.
    pub fn extract_file_refs(data: &serde_json::Value) -> Vec<(String, Option<String>)> {
        let mut out = Vec::new();
        for key in ["artifacts", "files"] {
            let Some(arr) = data.get(key).and_then(|v| v.as_array()) else {
                continue;
            };
            for item in arr {
                if let Some(obj) = item.as_object() {
                    if let Some(fid) = obj.get("file_id").and_then(|v| v.as_str()) {
                        if !fid.is_empty() {
                            let role = obj
                                .get("role")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string());
                            out.push((fid.to_string(), role));
                        }
                    }
                }
            }
        }
        out
    }

    pub fn sync_document(&self, doc: &DocumentRecord) -> Result<()> {
        if !Self::collection_tracks_refs(&doc.collection) {
            return Ok(());
        }
        self.remove_document(&doc.owner_did, &doc.collection, &doc.id)?;
        let refs = Self::extract_file_refs(&doc.data);
        if refs.is_empty() {
            return Ok(());
        }
        let anchor_key = Self::document_anchor_key(&doc.owner_did, &doc.collection, &doc.id);
        let anchors: Vec<DocAnchor> = refs
            .iter()
            .map(|(fid, role)| DocAnchor {
                file_id: fid.clone(),
                role: role.clone(),
            })
            .collect();
        let anchor_bytes = serde_json::to_vec(&anchors)?;

        let tx = self.db.begin_write()?;
        {
            let mut doc_table = tx.open_table(DOC_ANCHORS)?;
            doc_table.insert(anchor_key.as_str(), anchor_bytes.as_slice())?;

            let mut file_table = tx.open_table(FILE_REFS)?;
            for (file_id, role) in refs {
                let entry = ArtifactRefEntry {
                    collection: doc.collection.clone(),
                    doc_id: doc.id.clone(),
                    role,
                    owner_did: doc.owner_did.clone(),
                };
                let mut entries = self.read_file_refs_inner(&file_table, &file_id)?;
                if !entries.iter().any(|e| e == &entry) {
                    entries.push(entry);
                }
                let bytes = serde_json::to_vec(&entries)?;
                file_table.insert(file_id.as_str(), bytes.as_slice())?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn remove_document(&self, owner_did: &str, collection: &str, doc_id: &str) -> Result<()> {
        if !Self::collection_tracks_refs(collection) {
            return Ok(());
        }
        let anchor_key = Self::document_anchor_key(owner_did, collection, doc_id);
        let tx = self.db.begin_write()?;
        let anchor_bytes = {
            let mut doc_table = tx.open_table(DOC_ANCHORS)?;
            let removed = doc_table.remove(anchor_key.as_str())?;
            if let Some(guard) = removed {
                let copied = guard.value().to_vec();
                Some(copied)
            } else {
                None
            }
        };
        let Some(anchor_bytes) = anchor_bytes else {
            tx.commit()?;
            return Ok(());
        };
        let anchors: Vec<DocAnchor> = serde_json::from_slice(&anchor_bytes)?;
        {
            let mut file_table = tx.open_table(FILE_REFS)?;
            for anchor in anchors {
                self.remove_one_ref_inner(
                    &mut file_table,
                    &anchor.file_id,
                    owner_did,
                    collection,
                    doc_id,
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    fn remove_one_ref_inner(
        &self,
        file_table: &mut redb::Table<'_, &str, &[u8]>,
        file_id: &str,
        owner_did: &str,
        collection: &str,
        doc_id: &str,
    ) -> Result<()> {
        let mut entries = self.read_file_refs_inner(file_table, file_id)?;
        let before = entries.len();
        entries.retain(|e| {
            !(e.owner_did == owner_did && e.collection == collection && e.doc_id == doc_id)
        });
        if entries.len() == before {
            return Ok(());
        }
        if entries.is_empty() {
            file_table.remove(file_id)?;
        } else {
            let bytes = serde_json::to_vec(&entries)?;
            file_table.insert(file_id, bytes.as_slice())?;
        }
        Ok(())
    }

    fn read_file_refs_inner(
        &self,
        table: &redb::Table<'_, &str, &[u8]>,
        file_id: &str,
    ) -> Result<Vec<ArtifactRefEntry>> {
        let Some(value) = table.get(file_id)? else {
            return Ok(Vec::new());
        };
        Ok(serde_json::from_slice(value.value())?)
    }

    pub fn refs_for_file(&self, file_id: &str) -> Result<Vec<ArtifactRefEntry>> {
        let tx = self.db.begin_read()?;
        let table = tx.open_table(FILE_REFS)?;
        let Some(value) = table.get(file_id)? else {
            return Ok(Vec::new());
        };
        Ok(serde_json::from_slice(value.value())?)
    }

    pub fn ref_count(&self, file_id: &str) -> Result<usize> {
        Ok(self.refs_for_file(file_id)?.len())
    }

    pub fn index_owner_hash(
        &self,
        owner_did: &str,
        plaintext_hash: &str,
        file_id: &str,
    ) -> Result<()> {
        let key = Self::owner_hash_key(owner_did, plaintext_hash);
        let tx = self.db.begin_write()?;
        {
            let mut table = tx.open_table(FILE_HASH)?;
            table.insert(key.as_str(), file_id.as_bytes())?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn lookup_file_by_owner_hash(
        &self,
        owner_did: &str,
        plaintext_hash: &str,
    ) -> Result<Option<String>> {
        let key = Self::owner_hash_key(owner_did, plaintext_hash);
        let tx = self.db.begin_read()?;
        let table = tx.open_table(FILE_HASH)?;
        let Some(value) = table.get(key.as_str())? else {
            return Ok(None);
        };
        Ok(Some(String::from_utf8(value.value().to_vec())?))
    }

    pub fn remove_file_hash(&self, owner_did: &str, plaintext_hash: &str) -> Result<()> {
        let key = Self::owner_hash_key(owner_did, plaintext_hash);
        let tx = self.db.begin_write()?;
        {
            let mut table = tx.open_table(FILE_HASH)?;
            table.remove(key.as_str())?;
        }
        tx.commit()?;
        Ok(())
    }

    /// File IDs in metadata with zero catalog references for the given owner.
    pub fn orphan_file_ids_for_owner(&self, files: &[(String, u64)]) -> Result<Vec<(String, u64)>> {
        let mut orphans = Vec::new();
        for (file_id, size) in files {
            if self.ref_count(file_id)? == 0 {
                orphans.push((file_id.clone(), *size));
            }
        }
        Ok(orphans)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use tempfile::tempdir;

    fn sample_doc(data: serde_json::Value) -> DocumentRecord {
        DocumentRecord {
            owner_did: "did:spacekit:user:alice".to_string(),
            collection: "app_listings".to_string(),
            id: "app-1".to_string(),
            data,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            blob_ref: None,
        }
    }

    #[test]
    fn sync_and_remove_document_refs() {
        let dir = tempdir().unwrap();
        let idx = ArtifactRefIndex::open(&dir.path().join("refs.redb")).unwrap();
        let doc = sample_doc(serde_json::json!({
            "artifacts": [
                {"role": "wasm", "file_id": "aaa-111"},
                {"role": "bin", "file_id": "bbb-222"}
            ]
        }));
        idx.sync_document(&doc).unwrap();
        assert_eq!(idx.ref_count("aaa-111").unwrap(), 1);
        assert_eq!(idx.ref_count("bbb-222").unwrap(), 1);
        assert_eq!(idx.ref_count("ccc-333").unwrap(), 0);

        idx.remove_document("did:spacekit:user:alice", "app_listings", "app-1")
            .unwrap();
        assert_eq!(idx.ref_count("aaa-111").unwrap(), 0);
        assert_eq!(idx.ref_count("bbb-222").unwrap(), 0);
    }

    #[test]
    fn replace_document_refs_on_update() {
        let dir = tempdir().unwrap();
        let idx = ArtifactRefIndex::open(&dir.path().join("refs.redb")).unwrap();
        let doc_v1 = sample_doc(serde_json::json!({
            "artifacts": [{"role": "wasm", "file_id": "old-wasm"}]
        }));
        idx.sync_document(&doc_v1).unwrap();
        assert_eq!(idx.ref_count("old-wasm").unwrap(), 1);

        let doc_v2 = sample_doc(serde_json::json!({
            "artifacts": [{"role": "wasm", "file_id": "new-wasm"}]
        }));
        idx.sync_document(&doc_v2).unwrap();
        assert_eq!(idx.ref_count("old-wasm").unwrap(), 0);
        assert_eq!(idx.ref_count("new-wasm").unwrap(), 1);
    }

    #[test]
    fn owner_hash_lookup() {
        let dir = tempdir().unwrap();
        let idx = ArtifactRefIndex::open(&dir.path().join("refs.redb")).unwrap();
        idx.index_owner_hash("did:spacekit:user:alice", "abc123", "file-1")
            .unwrap();
        assert_eq!(
            idx.lookup_file_by_owner_hash("did:spacekit:user:alice", "abc123")
                .unwrap(),
            Some("file-1".to_string())
        );
    }
}
