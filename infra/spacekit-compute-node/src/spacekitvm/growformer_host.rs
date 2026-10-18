//! Growformer host holding one brain per contract, on one worker thread per process.
//!
//! `growformer::runtime::Runtime` is not `Sync`, and the inference configuration it reads
//! (rules, topic graph, grounding graph) is process-wide on native builds. Each brain is a
//! [`growformer::brain_instances::BrainInstance`], which installs its own configuration
//! before every call; that is only safe when every instance lives on the same thread. So all
//! [`GrowformerThreadHost`]s in a process share a single worker thread, started on first use,
//! and each host's brains are kept apart by a host id.
//!
//! Brains are keyed by [`GrowformerSlot`]: one per executing contract, plus one for
//! host-side inference (the consensus agent). Each host keeps at most `max_brains` loaded;
//! loading one more frees that host's least recently used brain, and
//! [`GrowformerThreadHost::load_brain`] reports which slot was freed so the VM can mark it
//! not ready.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, OnceLock};

use growformer::brain_instances::BrainInstance;

/// Default number of brains each host keeps loaded (`SPACEKIT_GROWFORMER_MAX_BRAINS`).
pub const DEFAULT_MAX_BRAINS: usize = 8;

/// Which brain a call is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GrowformerSlot {
    /// Host-side inference with no contract (consensus agent).
    Host,
    /// The brain a contract loaded, keyed by the contract's 20-byte address.
    Contract([u8; 20]),
}

/// Result of a successful load: the brain's byte length and the slot freed to make room.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GrowformerLoaded {
    pub bytes: usize,
    pub evicted: Option<GrowformerSlot>,
}

type Reply<T> = mpsc::Sender<Result<T, String>>;

enum Call {
    Load {
        data: Vec<u8>,
        reply: Reply<GrowformerLoaded>,
    },
    Unload,
    Prompt {
        text: String,
        reply: Reply<String>,
    },
    Converse {
        text: String,
        reply: Reply<String>,
    },
    Codegen {
        text: String,
        reply: Reply<String>,
    },
    BrainInfo {
        reply: Reply<String>,
    },
    Reset,
}

enum Op {
    Call {
        host: u64,
        max: usize,
        slot: GrowformerSlot,
        call: Call,
    },
    /// A host was dropped: free all its brains.
    DropHost { host: u64 },
}

type Key = (u64, GrowformerSlot);

/// The brains owned by the worker thread.
#[derive(Default)]
struct Brains {
    tick: u64,
    loaded: HashMap<Key, (BrainInstance, u64)>,
}

impl Brains {
    fn load(
        &mut self,
        host: u64,
        max: usize,
        slot: GrowformerSlot,
        data: &[u8],
    ) -> Result<GrowformerLoaded, String> {
        // A failed load leaves the slot empty rather than serving the previous brain.
        self.loaded.remove(&(host, slot));
        let inst = BrainInstance::from_brain_bytes(data)?;
        let mut evicted = None;
        let in_use = self.loaded.keys().filter(|(h, _)| *h == host).count();
        if in_use >= max.max(1) {
            if let Some(oldest) = self
                .loaded
                .iter()
                .filter(|((h, _), _)| *h == host)
                .min_by_key(|(_, (_, used))| *used)
                .map(|(k, _)| *k)
            {
                self.loaded.remove(&oldest);
                evicted = Some(oldest.1);
            }
        }
        self.tick += 1;
        self.loaded.insert((host, slot), (inst, self.tick));
        Ok(GrowformerLoaded {
            bytes: data.len(),
            evicted,
        })
    }

    fn with<R>(
        &mut self,
        key: Key,
        f: impl FnOnce(&mut growformer::runtime::Runtime) -> Result<R, String>,
    ) -> Result<R, String> {
        self.tick += 1;
        let tick = self.tick;
        let (inst, used) = self
            .loaded
            .get_mut(&key)
            .ok_or_else(|| "growformer brain not loaded".to_string())?;
        *used = tick;
        inst.with(f)
    }
}

fn to_json<T: serde::Serialize>(value: &T) -> Result<String, String> {
    serde_json::to_string(value).map_err(|e| e.to_string())
}

fn run_worker(rx: mpsc::Receiver<Op>) {
    let mut brains = Brains::default();
    while let Ok(op) = rx.recv() {
        let (host, max, slot, call) = match op {
            Op::DropHost { host } => {
                brains.loaded.retain(|(h, _), _| *h != host);
                continue;
            }
            Op::Call {
                host,
                max,
                slot,
                call,
            } => (host, max, slot, call),
        };
        let key = (host, slot);
        match call {
            Call::Load { data, reply } => {
                let _ = reply.send(brains.load(host, max, slot, &data));
            }
            Call::Unload => {
                brains.loaded.remove(&key);
            }
            Call::Prompt { text, reply } => {
                let out = brains.with(key, |g| {
                    g.reset_conversation();
                    let r = g.prompt(&text).and_then(|resp| to_json(&resp));
                    g.reset_conversation();
                    r
                });
                let _ = reply.send(out);
            }
            Call::Converse { text, reply } => {
                let out = brains.with(key, |g| g.converse(&text).and_then(|resp| to_json(&resp)));
                let _ = reply.send(out);
            }
            Call::Codegen { text, reply } => {
                let out = brains.with(key, |g| {
                    g.reset_conversation();
                    let r = g.codegen(&text).and_then(|code| to_json(&code));
                    g.reset_conversation();
                    r
                });
                let _ = reply.send(out);
            }
            Call::BrainInfo { reply } => {
                let out = brains.with(key, |g| to_json(&g.brain_info()));
                let _ = reply.send(out);
            }
            Call::Reset => {
                let _ = brains.with(key, |g| {
                    g.reset_conversation();
                    Ok(())
                });
            }
        }
    }
}

/// The process-wide worker, started on first use.
fn worker() -> &'static mpsc::Sender<Op> {
    static WORKER: OnceLock<mpsc::Sender<Op>> = OnceLock::new();
    WORKER.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Op>();
        std::thread::Builder::new()
            .name("swtchvm-growformer".into())
            .spawn(move || run_worker(rx))
            .expect("spawn growformer worker");
        tx
    })
}

/// One VM's view of the shared worker: its own brains, its own brain limit.
pub struct GrowformerThreadHost {
    id: u64,
    max_brains: usize,
}

impl GrowformerThreadHost {
    /// A host with the brain limit from `SPACEKIT_GROWFORMER_MAX_BRAINS` (default 8).
    pub fn spawn() -> Self {
        let max = std::env::var("SPACEKIT_GROWFORMER_MAX_BRAINS")
            .ok()
            .and_then(|v| v.trim().parse::<usize>().ok())
            .filter(|n| *n > 0)
            .unwrap_or(DEFAULT_MAX_BRAINS);
        Self::spawn_with_limit(max)
    }

    pub fn spawn_with_limit(max_brains: usize) -> Self {
        static NEXT_HOST: AtomicU64 = AtomicU64::new(1);
        Self {
            id: NEXT_HOST.fetch_add(1, Ordering::Relaxed),
            max_brains: max_brains.max(1),
        }
    }

    fn send(&self, slot: GrowformerSlot, call: Call) -> Result<(), String> {
        worker()
            .send(Op::Call {
                host: self.id,
                max: self.max_brains,
                slot,
                call,
            })
            .map_err(|_| "growformer worker disconnected".to_string())
    }

    fn request<T>(
        &self,
        slot: GrowformerSlot,
        make: impl FnOnce(Reply<T>) -> Call,
    ) -> Result<T, String> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.send(slot, make(reply_tx))?;
        reply_rx
            .recv()
            .map_err(|_| "growformer worker dropped reply".to_string())?
    }

    /// Load (or replace) the brain for `slot`. A failed load leaves `slot` empty.
    pub fn load_brain(
        &self,
        slot: GrowformerSlot,
        data: Vec<u8>,
    ) -> Result<GrowformerLoaded, String> {
        self.request(slot, |reply| Call::Load { data, reply })
    }

    /// Free the brain for `slot`, if any.
    pub fn unload_brain(&self, slot: GrowformerSlot) {
        let _ = self.send(slot, Call::Unload);
    }

    pub fn prompt_json(&self, slot: GrowformerSlot, text: &str) -> Result<String, String> {
        let text = text.to_string();
        self.request(slot, |reply| Call::Prompt { text, reply })
    }

    pub fn converse_json(&self, slot: GrowformerSlot, text: &str) -> Result<String, String> {
        let text = text.to_string();
        self.request(slot, |reply| Call::Converse { text, reply })
    }

    pub fn codegen_json(&self, slot: GrowformerSlot, text: &str) -> Result<String, String> {
        let text = text.to_string();
        self.request(slot, |reply| Call::Codegen { text, reply })
    }

    pub fn brain_info_json(&self, slot: GrowformerSlot) -> Result<String, String> {
        self.request(slot, |reply| Call::BrainInfo { reply })
    }

    pub fn reset_conversation(&self, slot: GrowformerSlot) {
        let _ = self.send(slot, Call::Reset);
    }
}

impl Drop for GrowformerThreadHost {
    fn drop(&mut self) {
        let _ = worker().send(Op::DropHost { host: self.id });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_slots_report_not_loaded() {
        let host = GrowformerThreadHost::spawn_with_limit(2);
        let a = GrowformerSlot::Contract([1; 20]);
        assert!(host.prompt_json(a, "hi").is_err());
        assert!(host.brain_info_json(GrowformerSlot::Host).is_err());
        assert!(host.load_brain(a, vec![0, 1, 2]).is_err());
        assert!(host.converse_json(a, "hi").is_err());
    }
}
