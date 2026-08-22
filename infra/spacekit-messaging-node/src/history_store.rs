//! Durable message history in redb with bounded cache and batched writes.

use anyhow::{Context, Result};
use moka::sync::Cache;
use redb::{Database as RedbDatabase, ReadableTable, TableDefinition};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::warn;

const HISTORY: TableDefinition<&str, &[u8]> = TableDefinition::new("history");
const MESSAGE_INDEX: TableDefinition<&str, &str> = TableDefinition::new("message_index");

#[derive(Debug, Clone)]
pub struct HistoryStoreConfig {
    pub cache_conversations: u64,
    pub batch_size: usize,
}

impl Default for HistoryStoreConfig {
    fn default() -> Self {
        Self {
            cache_conversations: 64,
            batch_size: 256,
        }
    }
}

#[derive(Debug)]
struct HistoryWrite {
    conversation_id: String,
    message_id: String,
    json: Vec<u8>,
}

#[derive(Debug)]
pub struct HistoryStore {
    path: PathBuf,
    db: RedbDatabase,
    conversation_cache: Cache<String, Arc<Vec<Vec<u8>>>>,
    batch_size: usize,
}

impl HistoryStore {
    pub fn open(path: &Path, config: HistoryStoreConfig) -> Result<Arc<Self>> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let db = RedbDatabase::create(path)
            .with_context(|| format!("open redb history store {:?}", path))?;
        let tx = db.begin_write()?;
        {
            tx.open_table(HISTORY)?;
            tx.open_table(MESSAGE_INDEX)?;
        }
        tx.commit()?;
        let conversation_cache = Cache::builder()
            .max_capacity(config.cache_conversations)
            .build();
        Ok(Arc::new(Self {
            path: path.to_path_buf(),
            db,
            conversation_cache,
            batch_size: config.batch_size,
        }))
    }

    pub fn spawn_batched_writer(self: &Arc<Self>) -> HistoryStoreWriter {
        let (tx, mut rx) = mpsc::channel::<HistoryWrite>(10_000);
        let store = Arc::clone(self);
        tokio::spawn(async move {
            let mut batch = Vec::with_capacity(store.batch_size);
            loop {
                rx.recv_many(&mut batch, store.batch_size).await;
                if batch.is_empty() {
                    continue;
                }
                if let Err(e) = store.commit_batch(&batch) {
                    warn!("history batch commit failed: {}", e);
                }
                batch.clear();
            }
        });
        HistoryStoreWriter { tx }
    }

    fn entry_key(conversation_id: &str, message_id: &str) -> String {
        format!("{conversation_id}\0{message_id}")
    }

    fn commit_batch(&self, batch: &[HistoryWrite]) -> Result<()> {
        let tx = self.db.begin_write()?;
        {
            let mut history = tx.open_table(HISTORY)?;
            let mut index = tx.open_table(MESSAGE_INDEX)?;
            for item in batch {
                let key = Self::entry_key(&item.conversation_id, &item.message_id);
                history.insert(key.as_str(), item.json.as_slice())?;
                index.insert(item.message_id.as_str(), item.conversation_id.as_str())?;
                self.conversation_cache.invalidate(&item.conversation_id);
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn append(&self, conversation_id: &str, message_id: &str, json: &[u8]) -> Result<()> {
        let tx = self.db.begin_write()?;
        {
            let mut history = tx.open_table(HISTORY)?;
            let mut index = tx.open_table(MESSAGE_INDEX)?;
            let key = Self::entry_key(conversation_id, message_id);
            history.insert(key.as_str(), json)?;
            index.insert(message_id, conversation_id)?;
        }
        tx.commit()?;
        self.conversation_cache.invalidate(conversation_id);
        Ok(())
    }

    pub fn append_batch(&self, items: &[(String, String, Vec<u8>)]) -> Result<()> {
        if items.is_empty() {
            return Ok(());
        }
        let tx = self.db.begin_write()?;
        {
            let mut history = tx.open_table(HISTORY)?;
            let mut index = tx.open_table(MESSAGE_INDEX)?;
            for (conversation_id, message_id, json) in items {
                let key = Self::entry_key(conversation_id, message_id);
                history.insert(key.as_str(), json.as_slice())?;
                index.insert(message_id.as_str(), conversation_id.as_str())?;
                self.conversation_cache.invalidate(conversation_id);
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn load_conversation(&self, conversation_id: &str) -> Result<Vec<Vec<u8>>> {
        if let Some(cached) = self.conversation_cache.get(conversation_id) {
            return Ok((*cached).clone());
        }
        let prefix = format!("{conversation_id}\0");
        let mut entries = Vec::new();
        let tx = self.db.begin_read()?;
        let table = tx.open_table(HISTORY)?;
        for item in table.iter()? {
            let (key, value) = item?;
            if !key.value().starts_with(prefix.as_str()) {
                continue;
            }
            entries.push(value.value().to_vec());
        }
        self.conversation_cache
            .insert(conversation_id.to_string(), Arc::new(entries.clone()));
        Ok(entries)
    }

    pub fn find_by_message_id(&self, message_id: &str) -> Result<Option<Vec<u8>>> {
        let tx = self.db.begin_read()?;
        let index = tx.open_table(MESSAGE_INDEX)?;
        let Some(conversation_id) = index.get(message_id)? else {
            return Ok(None);
        };
        let conv = conversation_id.value();
        let history = tx.open_table(HISTORY)?;
        let key = Self::entry_key(conv, message_id);
        Ok(history.get(key.as_str())?.map(|v| v.value().to_vec()))
    }

    pub fn list_conversation_ids(&self) -> Result<Vec<String>> {
        let mut ids = std::collections::HashSet::new();
        let tx = self.db.begin_read()?;
        let table = tx.open_table(HISTORY)?;
        for item in table.iter()? {
            let (key, _) = item?;
            if let Some(conv) = key.value().split('\0').next() {
                ids.insert(conv.to_string());
            }
        }
        Ok(ids.into_iter().collect())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Import legacy `{conversation}.jsonl` files when redb is empty.
    pub fn migrate_jsonl_dir(&self, history_dir: &Path) -> Result<usize> {
        if !history_dir.exists() {
            return Ok(0);
        }
        let mut count = 0usize;
        for entry in std::fs::read_dir(history_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let conversation_id = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string();
            let content = std::fs::read_to_string(&path)?;
            for line in content.lines() {
                if line.trim().is_empty() {
                    continue;
                }
                let v: serde_json::Value = serde_json::from_str(line)?;
                let message_id = v
                    .get("message_id")
                    .and_then(|m| m.as_str())
                    .unwrap_or_default()
                    .to_string();
                if message_id.is_empty() {
                    continue;
                }
                self.append(&conversation_id, &message_id, line.as_bytes())?;
                count += 1;
            }
        }
        Ok(count)
    }
}

#[derive(Clone, Debug)]
pub struct HistoryStoreWriter {
    tx: mpsc::Sender<HistoryWrite>,
}

impl HistoryStoreWriter {
    pub async fn append(
        &self,
        conversation_id: &str,
        message_id: &str,
        json: Vec<u8>,
    ) -> Result<()> {
        self.tx
            .send(HistoryWrite {
                conversation_id: conversation_id.to_string(),
                message_id: message_id.to_string(),
                json,
            })
            .await
            .map_err(|e| anyhow::anyhow!("history writer channel closed: {}", e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn append_and_load_conversation() {
        let dir = tempdir().unwrap();
        let store = HistoryStore::open(
            &dir.path().join("history.redb"),
            HistoryStoreConfig::default(),
        )
        .unwrap();
        store
            .append("conv-a", "m1", br#"{"message_id":"m1"}"#)
            .unwrap();
        let loaded = store.load_conversation("conv-a").unwrap();
        assert_eq!(loaded.len(), 1);
        assert!(store.find_by_message_id("m1").unwrap().is_some());
    }
}
