//! Append-only change feed (Phase 4).
//!
//! Architecture:
//!
//! - **Disk-backed JSONL** (`<data_dir>/change_log.jsonl`): each event is
//!   appended, flushed, and `fsync`'d before subscribers receive it (write
//!   amplification tradeoff; see guide for group-commit follow-up).
//! - **Per-subscriber bounded queue** with **disconnect-on-overflow** —
//!   slow consumers don't slow the publisher and don't drop events
//!   silently. They get disconnected; subscribers MUST handle disconnects
//!   and resume from the disk ring buffer using `Last-Event-ID` /
//!   `since-seq` query.
//! - **SSE endpoint** `GET /api/changes` (added in [`crate::api`])
//!   streams events to subscribers; gossipsub topic
//!   `spacekit/changes/v1` federates across nodes.
//!
//! This is a Phase 4 deliverable; Phase 0 publishes a few events
//! (`tx.committed`, `tx.rolled_back`) so the wiring is exercised end-to-end
//! before the full event vocabulary lands.
//!
//! **Throughput:** `append_to_disk` does one `fsync` per published event (see
//! guide). No in-tree sustained-QPS number is checked into CI yet — run a
//! synthetic burst (e.g. loop `publish` in `--release`) on your hardware and
//! record the result in ops dashboards or in this comment when you have it.

#![deny(clippy::all)]

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tracing::{debug, warn};

/// One change event. `seq` is monotonic; subscribers resume by passing
/// `Last-Event-ID: <seq>` (or the `since-seq=<seq>` query parameter).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeEvent {
    pub seq: u64,
    pub occurred_at: DateTime<Utc>,
    /// DID associated with the change, if known. Subscribers may filter by
    /// DID (operator dashboards typically don't; per-tenant agents do).
    pub did: Option<String>,
    /// Dotted change kind: `tx.committed`, `tx.rolled_back`,
    /// `sandbox.committed`, `sandbox.discarded`, `doc.put`, `repo.commit`,
    /// `vector.upserted`, `fts.indexed`, etc.
    pub kind: String,
    /// Logical key the event refers to (transaction id, fact id, document
    /// path, etc).
    pub key: String,
    /// Optional payload (small). Large payloads should use the CAS and
    /// reference a hash here.
    pub payload: Option<serde_json::Value>,
}

/// In-memory ring buffer (Phase 4 will wire the disk-backed version). Even
/// the in-memory variant uses a bounded VecDeque + monotonic counter so the
/// HTTP `?since-seq` resume contract stays valid for the lifetime of a
/// node process.
pub struct ChangeFeed {
    inner: Arc<Mutex<VecDeque<ChangeEvent>>>,
    capacity: usize,
    seq_counter: AtomicU64,
    subscriber_id_counter: AtomicU64,
    subscribers: RwLock<Vec<Subscriber>>,
    /// Optional disk-backed JSONL append file at `<data_dir>/change_log.jsonl`.
    /// Phase 4 persistence — when set, every published event is appended,
    /// flushed, and `fsync`'d before subscribers see it. The on-disk log
    /// survives node restarts so subscribers can resume by `seq` after a
    /// reconnect window longer than the in-memory ring buffer.
    disk_path: RwLock<Option<std::path::PathBuf>>,
    /// Slow subscribers removed since process start (`publish` disconnect-on-overflow).
    dropped_subscribers_total: AtomicU64,
}

#[derive(Clone)]
struct Subscriber {
    id: u64,
    sender: tokio::sync::mpsc::Sender<ChangeEvent>,
    /// Filter: only deliver events whose `kind` matches one of these globs.
    /// Empty = match all.
    kind_globs: Vec<String>,
}

impl ChangeFeed {
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(VecDeque::with_capacity(capacity))),
            capacity,
            seq_counter: AtomicU64::new(0),
            subscriber_id_counter: AtomicU64::new(0),
            subscribers: RwLock::new(Vec::new()),
            disk_path: RwLock::new(None),
            dropped_subscribers_total: AtomicU64::new(0),
        }
    }

    pub fn dropped_subscribers_total(&self) -> u64 {
        self.dropped_subscribers_total.load(Ordering::Relaxed)
    }

    pub async fn live_subscriber_count(&self) -> usize {
        self.subscribers.read().await.len()
    }

    /// Configure a disk-backed JSONL log path. On startup the manager scans
    /// the file and resets `seq_counter` to the highest seq found so the
    /// monotonic invariant holds across restarts.
    pub async fn enable_disk_persistence(&self, path: std::path::PathBuf) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // Find the highest seq in the existing file (if any).
        if path.exists() {
            let content = std::fs::read_to_string(&path)?;
            let mut max_seq: u64 = 0;
            for line in content.lines() {
                if let Ok(ev) = serde_json::from_str::<ChangeEvent>(line) {
                    if ev.seq > max_seq {
                        max_seq = ev.seq;
                    }
                }
            }
            if max_seq > 0 {
                self.seq_counter.store(max_seq, Ordering::Relaxed);
            }
        }
        *self.disk_path.write().await = Some(path);
        Ok(())
    }

    /// Read events with `seq > since` directly from the disk log. Used by
    /// the SSE handler to backfill subscribers that resume past the
    /// in-memory ring buffer's window.
    pub async fn read_persisted(&self, since: u64) -> std::io::Result<Vec<ChangeEvent>> {
        let path = match self.disk_path.read().await.clone() {
            Some(p) => p,
            None => return Ok(Vec::new()),
        };
        if !path.exists() {
            return Ok(Vec::new());
        }
        let content = std::fs::read_to_string(&path)?;
        let mut out = Vec::new();
        for line in content.lines() {
            if let Ok(ev) = serde_json::from_str::<ChangeEvent>(line) {
                if ev.seq > since {
                    out.push(ev);
                }
            }
        }
        Ok(out)
    }

    /// Publish a new event; assigns a fresh `seq` and fans out to subscribers.
    /// Slow subscribers (full mpsc) are disconnected — they MUST resume from
    /// the disk ring buffer.
    pub async fn publish(&self, mut event: ChangeEvent) -> u64 {
        let seq = self.seq_counter.fetch_add(1, Ordering::Relaxed) + 1;
        event.seq = seq;
        {
            let mut buf = self.inner.lock().await;
            if buf.len() >= self.capacity {
                buf.pop_front();
            }
            buf.push_back(event.clone());
        }
        // Persist to disk *before* fanning out so subscribers cannot observe
        // an event the disk log doesn't have.
        if let Some(path) = self.disk_path.read().await.clone() {
            if let Err(e) = self.append_to_disk(&path, &event).await {
                warn!(error = %e, "change_feed: disk append failed");
            }
        }
        let mut to_drop: Vec<u64> = Vec::new();
        {
            let subs = self.subscribers.read().await;
            for sub in subs.iter() {
                if !sub.kind_globs.is_empty()
                    && !sub.kind_globs.iter().any(|g| glob_matches(g, &event.kind))
                {
                    continue;
                }
                if sub.sender.try_send(event.clone()).is_err() {
                    to_drop.push(sub.id);
                }
            }
        }
        if !to_drop.is_empty() {
            self.dropped_subscribers_total
                .fetch_add(to_drop.len() as u64, Ordering::Relaxed);
            let mut subs = self.subscribers.write().await;
            subs.retain(|s| !to_drop.contains(&s.id));
            warn!(
                "change_feed: dropped {} slow subscribers (must resume by seq)",
                to_drop.len()
            );
        }
        debug!(seq, kind = %event.kind, key = %event.key, "change_feed: published");
        seq
    }

    /// Subscribe to the live stream.
    /// `since_seq` replays buffered events with `seq > since_seq` first.
    pub async fn subscribe(
        &self,
        kind_globs: Vec<String>,
        since_seq: Option<u64>,
        buffer: usize,
    ) -> tokio::sync::mpsc::Receiver<ChangeEvent> {
        let (tx, rx) = tokio::sync::mpsc::channel::<ChangeEvent>(buffer);
        if let Some(since) = since_seq {
            let buf = self.inner.lock().await;
            for ev in buf.iter() {
                if ev.seq > since
                    && (kind_globs.is_empty()
                        || kind_globs.iter().any(|g| glob_matches(g, &ev.kind)))
                {
                    let _ = tx.send(ev.clone()).await;
                }
            }
        }
        let id = self.subscriber_id_counter.fetch_add(1, Ordering::Relaxed);
        self.subscribers.write().await.push(Subscriber {
            id,
            sender: tx,
            kind_globs,
        });
        rx
    }

    /// Look up the most recent `seq`. Useful for `Last-Event-ID` echo.
    pub fn current_seq(&self) -> u64 {
        self.seq_counter.load(Ordering::Relaxed)
    }

    async fn append_to_disk(
        &self,
        path: &std::path::Path,
        event: &ChangeEvent,
    ) -> std::io::Result<()> {
        use std::io::Write;
        let serialized = serde_json::to_vec(event).unwrap_or_default();
        let mut buf = serialized;
        buf.push(b'\n');
        // Spawn-blocking so the runtime stays unblocked during fsync.
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || -> std::io::Result<()> {
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)?;
            f.write_all(&buf)?;
            f.sync_data()?;
            Ok(())
        })
        .await
        .map_err(|e| std::io::Error::other(e.to_string()))??;
        Ok(())
    }
}

fn glob_matches(pattern: &str, value: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix(".*") {
        return value.starts_with(prefix)
            && (value.len() == prefix.len() || value[prefix.len()..].starts_with('.'));
    }
    pattern == value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn publish_assigns_monotonic_seq() {
        let feed = ChangeFeed::new(8);
        let s1 = feed
            .publish(ChangeEvent {
                seq: 0,
                occurred_at: Utc::now(),
                did: None,
                kind: "tx.committed".into(),
                key: "abc".into(),
                payload: None,
            })
            .await;
        let s2 = feed
            .publish(ChangeEvent {
                seq: 0,
                occurred_at: Utc::now(),
                did: None,
                kind: "tx.committed".into(),
                key: "def".into(),
                payload: None,
            })
            .await;
        assert!(s2 > s1);
    }

    #[test]
    fn glob_matches_simple_prefix() {
        assert!(glob_matches("*", "anything"));
        assert!(glob_matches("tx.*", "tx.committed"));
        assert!(glob_matches("tx.*", "tx.rolled_back"));
        assert!(!glob_matches("tx.*", "sandbox.committed"));
        assert!(glob_matches("tx.committed", "tx.committed"));
        assert!(!glob_matches("tx.committed", "tx.rolled_back"));
    }
}
