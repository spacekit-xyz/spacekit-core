//! One-time migrations into redb-backed stores.

use crate::blob_store::BlobStore;
use crate::database::DocumentRecord;
use crate::document_blob_store::DocumentBlobStore;
use crate::meta_store::DocumentMetaStore;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::UNIX_EPOCH;
use tracing::{info, warn};

const LEGACY_DOCSTORE: &str = "docstore";

/// Move inline + legacy file document bodies into `blobs.redb`, metadata into `meta.redb`.
pub fn migrate_storage_layout(
    data_dir: &Path,
    blob_store: Arc<BlobStore>,
    meta_store: &DocumentMetaStore,
    inline_documents: &HashMap<String, DocumentRecord>,
) -> Result<MigrateStorageReport> {
    let mut report = MigrateStorageReport::default();
    let doc_blobs = DocumentBlobStore::new(data_dir, Arc::clone(&blob_store));

    for (key, doc) in inline_documents {
        if meta_store.get(key)?.is_some() {
            continue;
        }
        let stored = if doc.blob_ref.is_some() {
            doc.clone()
        } else {
            let blob_ref = doc_blobs.write_body(doc)?;
            DocumentRecord {
                data: serde_json::Value::Null,
                blob_ref: Some(blob_ref),
                ..doc.clone()
            }
        };
        meta_store.upsert(key, &stored)?;
        report.documents_migrated += 1;
    }

    report.legacy_files_migrated = migrate_legacy_docstore_files(data_dir, &blob_store)?;
    Ok(report)
}

fn migrate_legacy_docstore_files(data_dir: &Path, blob_store: &BlobStore) -> Result<usize> {
    let root = data_dir.join(LEGACY_DOCSTORE);
    if !root.exists() {
        return Ok(0);
    }
    let mut count = 0usize;
    for entry in walkdir_light(&root)? {
        if !entry.ends_with(".json") {
            continue;
        }
        let rel = entry
            .strip_prefix(data_dir)
            .unwrap_or(&entry)
            .to_string_lossy()
            .replace('\\', "/");
        if blob_store.contains_key(&rel)? {
            continue;
        }
        let bytes = fs::read(&entry)?;
        blob_store.insert(&rel, &bytes)?;
        count += 1;
    }
    if count > 0 {
        info!("Migrated {} legacy docstore files into blobs.redb", count);
    }
    Ok(count)
}

fn walkdir_light(root: &Path) -> Result<Vec<PathBuf>> {
    let mut stack = vec![root.to_path_buf()];
    let mut files = Vec::new();
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                files.push(path);
            }
        }
    }
    Ok(files)
}

/// Import JSONL conversation history into `history.redb`.
pub fn migrate_jsonl_history(
    history_dir: &Path,
    append: impl Fn(&str, &str, &[u8]) -> Result<()>,
) -> Result<usize> {
    if !history_dir.exists() {
        return Ok(0);
    }
    let mut count = 0usize;
    for entry in fs::read_dir(history_dir)? {
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
        let content = fs::read_to_string(&path)?;
        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let v: serde_json::Value = serde_json::from_str(line)
                .with_context(|| format!("parse history line in {:?}", path))?;
            let message_id = v
                .get("message_id")
                .and_then(|m| m.as_str())
                .unwrap_or_default()
                .to_string();
            if message_id.is_empty() {
                warn!("Skipping history line without message_id in {:?}", path);
                continue;
            }
            append(&conversation_id, &message_id, line.as_bytes())?;
            count += 1;
        }
    }
    if count > 0 {
        info!("Migrated {} JSONL history entries into history.redb", count);
    }
    Ok(count)
}

const INGEST_CACHE_FILE: &str = ".ingest-cache.json";

/// Tracks source path → blake3 hash for unchanged files (skip re-hashing on re-ingest).
#[derive(Debug, Default, Serialize, Deserialize)]
struct IngestCacheFile {
    entries: HashMap<String, String>,
}

#[derive(Debug)]
struct IngestCache {
    path: PathBuf,
    entries: HashMap<String, String>,
    dirty: bool,
}

impl IngestCache {
    fn load(data_dir: &Path) -> Result<Self> {
        let path = data_dir.join(INGEST_CACHE_FILE);
        let entries = if path.exists() {
            fs::read_to_string(&path)
                .ok()
                .and_then(|raw| serde_json::from_str::<IngestCacheFile>(&raw).ok())
                .map(|c| c.entries)
                .unwrap_or_default()
        } else {
            HashMap::new()
        };
        Ok(Self {
            path,
            entries,
            dirty: false,
        })
    }

    fn save(&mut self) -> Result<()> {
        if !self.dirty {
            return Ok(());
        }
        let payload = IngestCacheFile {
            entries: self.entries.clone(),
        };
        let bytes = serde_json::to_vec_pretty(&payload)?;
        let tmp = self.path.with_extension("json.part");
        fs::write(&tmp, &bytes)?;
        fs::rename(&tmp, &self.path)?;
        self.dirty = false;
        Ok(())
    }

    fn lookup(&self, source: &Path) -> Result<Option<String>> {
        let key = ingest_cache_key(source)?;
        Ok(self.entries.get(&key).cloned())
    }

    fn remember(&mut self, source: &Path, hash: &str) -> Result<()> {
        let key = ingest_cache_key(source)?;
        if self.entries.get(&key).map(|h| h.as_str()) == Some(hash) {
            return Ok(());
        }
        self.entries.insert(key, hash.to_string());
        self.dirty = true;
        Ok(())
    }
}

fn ingest_cache_key(source: &Path) -> Result<String> {
    let canonical = source
        .canonicalize()
        .with_context(|| format!("canonicalize source path {:?}", source))?;
    let meta = fs::metadata(&canonical)?;
    let modified = meta
        .modified()
        .unwrap_or(UNIX_EPOCH)
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    Ok(format!(
        "{}:{}:{}",
        canonical.display(),
        meta.len(),
        modified
    ))
}

fn cas_blob_path(data_dir: &Path, hash: &str) -> PathBuf {
    let prefix = hash.get(..2).unwrap_or(hash);
    data_dir.join("blobs").join(prefix).join(hash)
}

fn hash_file_streaming(source: &Path) -> Result<String> {
    let mut file =
        fs::File::open(source).with_context(|| format!("open source file {:?}", source))?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize().as_bytes()))
}

fn atomic_copy_to(source: &Path, dest: &Path) -> Result<()> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp_path = dest.with_extension(format!("{}.part", uuid::Uuid::new_v4()));
    {
        let mut in_file = fs::File::open(source)?;
        let mut out = fs::File::create(&tmp_path)?;
        std::io::copy(&mut in_file, &mut out)?;
    }
    fs::rename(&tmp_path, dest)?;
    Ok(())
}

/// Stream-ingest large file(s) into CAS layout `{data_dir}/blobs/{aa}/{hash}` without full RAM load.
pub fn ingest_files_as_cas_blobs(
    data_dir: &Path,
    sources: &[PathBuf],
) -> Result<Vec<IngestReport>> {
    let mut cache = IngestCache::load(data_dir)?;
    let mut reports = Vec::with_capacity(sources.len());
    for source in sources {
        reports.push(ingest_file_with_cache(data_dir, source, &mut cache)?);
    }
    cache.save()?;
    Ok(reports)
}

/// Stream-ingest a single file (opens and saves the ingest cache each call).
pub fn ingest_file_as_cas_blob(data_dir: &Path, source: &Path) -> Result<IngestReport> {
    let mut cache = IngestCache::load(data_dir)?;
    let report = ingest_file_with_cache(data_dir, source, &mut cache)?;
    cache.save()?;
    Ok(report)
}

fn ingest_file_with_cache(
    data_dir: &Path,
    source: &Path,
    cache: &mut IngestCache,
) -> Result<IngestReport> {
    if let Some(hash) = cache.lookup(source)? {
        let dest = cas_blob_path(data_dir, &hash);
        if dest.exists() {
            return Ok(IngestReport {
                hash,
                bytes: fs::metadata(&dest)?.len(),
                path: dest,
                skipped: true,
                cache_hit: true,
            });
        }
    }

    let hash = hash_file_streaming(source)?;
    let dest = cas_blob_path(data_dir, &hash);
    let bytes = fs::metadata(source)?.len();
    let skipped = if dest.exists() {
        true
    } else {
        atomic_copy_to(source, &dest)?;
        false
    };
    cache.remember(source, &hash)?;
    Ok(IngestReport {
        hash,
        bytes,
        path: dest,
        skipped,
        cache_hit: false,
    })
}

#[cfg(test)]
mod ingest_tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn dedup_skips_second_copy() {
        let dir = tempdir().unwrap();
        let source = dir.path().join("video.bin");
        fs::write(&source, vec![0u8; 1024 * 1024]).unwrap();

        let first = ingest_file_as_cas_blob(dir.path(), &source).unwrap();
        assert!(!first.skipped);
        assert!(first.path.exists());

        let second = ingest_file_as_cas_blob(dir.path(), &source).unwrap();
        assert!(second.skipped);
        assert!(second.cache_hit);
        assert_eq!(first.hash, second.hash);
    }

    #[test]
    fn hash_first_skips_copy_when_blob_exists() {
        let dir = tempdir().unwrap();
        let source = dir.path().join("clip.mp4");
        fs::write(&source, b"fake video bytes").unwrap();

        let hash = hash_file_streaming(&source).unwrap();
        let dest = cas_blob_path(dir.path(), &hash);
        atomic_copy_to(&source, &dest).unwrap();

        let report = ingest_file_as_cas_blob(dir.path(), &source).unwrap();
        assert!(report.skipped);
        assert!(!report.cache_hit);
        assert_eq!(report.hash, hash);
    }
}

#[derive(Debug, Default, Clone)]
pub struct MigrateStorageReport {
    pub documents_migrated: usize,
    pub legacy_files_migrated: usize,
}

#[derive(Debug, Clone)]
pub struct IngestReport {
    pub hash: String,
    pub bytes: u64,
    pub path: PathBuf,
    /// Blob already present at the content hash path (no copy performed).
    pub skipped: bool,
    /// Skipped via `.ingest-cache.json` without re-hashing the source file.
    pub cache_hit: bool,
}
