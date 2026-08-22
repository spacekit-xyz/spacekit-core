use std::collections::{HashMap, VecDeque};

use parking_lot::Mutex;

use crate::types::Hex32;

pub struct RateLimiter {
    max: usize,
    window_s: i64,
    stamps: Mutex<HashMap<Hex32, VecDeque<i64>>>,
}

impl RateLimiter {
    pub fn new(max: usize, window_s: i64) -> Self {
        Self {
            max,
            window_s,
            stamps: Mutex::new(HashMap::new()),
        }
    }

    /// Production defaults: 3 decrypts per subject per hour. Override with env vars.
    /// `KEYMASTER_DEV=1` → 50 per 60s (local roundtrip / CI).
    pub fn from_env() -> Self {
        if std::env::var("KEYMASTER_DEV").ok().as_deref() == Some("1") {
            return Self::new(50, 60);
        }
        let max = std::env::var("KEYMASTER_RATE_LIMIT_MAX")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(3);
        let window_s = std::env::var("KEYMASTER_RATE_LIMIT_WINDOW_S")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(3600);
        Self::new(max, window_s)
    }

    pub fn allow(&self, subject: &str, now: i64) -> bool {
        let mut map = self.stamps.lock();
        let q = map.entry(subject.to_string()).or_default();
        while q.front().is_some_and(|t| now - *t > self.window_s) {
            q.pop_front();
        }
        if q.len() >= self.max {
            return false;
        }
        q.push_back(now);
        true
    }
}
