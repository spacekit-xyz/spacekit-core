//! Document metadata index in redb (payloads live in [`crate::blob_store::BlobStore`]).

use crate::database::DocumentRecord;
use anyhow::{Context, Result};
use redb::{Database as RedbDatabase, ReadableTable, TableDefinition};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const DOCUMENTS_META: TableDefinition<&str, &[u8]> = TableDefinition::new("documents_meta");

#[derive(Debug)]
pub struct DocumentMetaStore {
    path: PathBuf,
    db: RedbDatabase,
}

impl DocumentMetaStore {
    pub fn open(path: &Path) -> Result<Arc<Self>> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let db = RedbDatabase::create(path)
            .with_context(|| format!("open document meta store {:?}", path))?;
        {
            let tx = db.begin_write()?;
            tx.open_table(DOCUMENTS_META)?;
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

    pub fn upsert(&self, key: &str, record: &DocumentRecord) -> Result<()> {
        let mut meta = record.clone();
        if meta.blob_ref.is_some() {
            meta.data = serde_json::Value::Null;
        }
        let bytes = serde_json::to_vec(&meta)?;
        let tx = self.db.begin_write()?;
        {
            let mut table = tx.open_table(DOCUMENTS_META)?;
            table.insert(key, bytes.as_slice())?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn get(&self, key: &str) -> Result<Option<DocumentRecord>> {
        let tx = self.db.begin_read()?;
        let table = tx.open_table(DOCUMENTS_META)?;
        let Some(value) = table.get(key)? else {
            return Ok(None);
        };
        Ok(Some(serde_json::from_slice(value.value())?))
    }

    pub fn delete(&self, key: &str) -> Result<bool> {
        let tx = self.db.begin_write()?;
        let removed = {
            let mut table = tx.open_table(DOCUMENTS_META)?;
            let removed = matches!(table.remove(key)?, Some(_));
            removed
        };
        tx.commit()?;
        Ok(removed)
    }

    pub fn count(&self) -> Result<usize> {
        let tx = self.db.begin_read()?;
        let table = tx.open_table(DOCUMENTS_META)?;
        Ok(table.iter()?.count())
    }

    pub fn list_matching<F>(&self, mut pred: F) -> Result<Vec<DocumentRecord>>
    where
        F: FnMut(&DocumentRecord) -> bool,
    {
        let tx = self.db.begin_read()?;
        let table = tx.open_table(DOCUMENTS_META)?;
        let mut out = Vec::new();
        for item in table.iter()? {
            let (_, value) = item?;
            let record: DocumentRecord = serde_json::from_slice(value.value())?;
            if pred(&record) {
                out.push(record);
            }
        }
        Ok(out)
    }

    pub fn import_map(
        &self,
        documents: &std::collections::HashMap<String, DocumentRecord>,
    ) -> Result<usize> {
        let tx = self.db.begin_write()?;
        let mut count = 0usize;
        {
            let mut table = tx.open_table(DOCUMENTS_META)?;
            for (key, record) in documents {
                let mut meta = record.clone();
                if meta.blob_ref.is_some() {
                    meta.data = serde_json::Value::Null;
                }
                let bytes = serde_json::to_vec(&meta)?;
                table.insert(key.as_str(), bytes.as_slice())?;
                count += 1;
            }
        }
        tx.commit()?;
        Ok(count)
    }
}
