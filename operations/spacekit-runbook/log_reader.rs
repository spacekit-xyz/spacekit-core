//! Streaming reader for spacekit-log JSONL files.
//!
//! Production logs can be GBs; we stream rather than load into memory.

use anyhow::{Context, Result};
use spacekit_log::LogEvent;
use std::io::{BufRead, BufReader};
use std::path::Path;

pub struct LogReader<R: BufRead> {
    inner: R,
    line_number: usize,
    source: String,
}

impl LogReader<BufReader<std::fs::File>> {
    pub fn from_file(path: &Path) -> Result<Self> {
        let file = std::fs::File::open(path).with_context(|| format!("opening {:?}", path))?;
        Ok(Self {
            inner: BufReader::with_capacity(64 * 1024, file),
            line_number: 0,
            source: format!("{:?}", path),
        })
    }
}

impl<R: BufRead> LogReader<R> {
    /// Read the next event, skipping malformed lines with a warning.
    pub fn next_event(&mut self) -> Result<Option<LogEvent>> {
        loop {
            let mut line = String::new();
            let n = self.inner.read_line(&mut line)?;
            if n == 0 {
                return Ok(None);
            }
            self.line_number += 1;
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            match serde_json::from_str::<LogEvent>(line) {
                Ok(event) => return Ok(Some(event)),
                Err(e) => {
                    eprintln!(
                        "WARNING: {}:{}: malformed log line: {}",
                        self.source, self.line_number, e
                    );
                    continue;
                }
            }
        }
    }
}

/// Iterate over all log files in a directory, yielding events in file order.
pub fn iter_logs(dir: &Path, mut callback: impl FnMut(LogEvent) -> Result<()>) -> Result<()> {
    let mut paths: Vec<_> = walkdir::WalkDir::new(dir)
        .max_depth(3)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("jsonl"))
        .map(|e| e.path().to_path_buf())
        .collect();
    paths.sort(); // deterministic order

    for path in &paths {
        eprintln!("Reading {:?}", path);
        let mut reader = LogReader::from_file(path)?;
        while let Some(event) = reader.next_event()? {
            callback(event)?;
        }
    }
    Ok(())
}
