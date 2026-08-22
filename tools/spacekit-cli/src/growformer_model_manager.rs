use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use tokio::sync::{mpsc, oneshot};

use growformer::runtime::Runtime as GrowformerRuntime;

enum BrainCmd {
    Load {
        name: String,
        data: Vec<u8>,
        reply: oneshot::Sender<Result<BrainMeta>>,
    },
    Unload {
        name: String,
        reply: oneshot::Sender<()>,
    },
    Generate {
        name: String,
        prompt: String,
        reply: oneshot::Sender<Result<String>>,
    },
}

#[derive(Clone, Debug)]
pub struct BrainMeta {
    pub agent_name: String,
    pub num_groups: usize,
}

/// Manages growformer brains on a dedicated thread (Runtime is !Send).
pub struct GrowformerModelManager {
    tx: mpsc::UnboundedSender<BrainCmd>,
    loaded: Arc<RwLock<HashMap<String, BrainMeta>>>,
}

impl GrowformerModelManager {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let loaded: Arc<RwLock<HashMap<String, BrainMeta>>> = Arc::new(RwLock::new(HashMap::new()));
        let loaded_clone = loaded.clone();

        std::thread::Builder::new()
            .name("growformer-brains".into())
            .spawn(move || Self::brain_thread(rx, loaded_clone))
            .expect("spawn growformer brain thread");

        Self { tx, loaded }
    }

    fn brain_thread(
        mut rx: mpsc::UnboundedReceiver<BrainCmd>,
        loaded: Arc<RwLock<HashMap<String, BrainMeta>>>,
    ) {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("growformer tokio runtime");

        rt.block_on(async move {
            let mut brains: HashMap<String, GrowformerRuntime> = HashMap::new();

            while let Some(cmd) = rx.recv().await {
                match cmd {
                    BrainCmd::Load { name, data, reply } => {
                        let result = match GrowformerRuntime::from_brain_bytes(&data) {
                            Ok(runtime) => {
                                let info = runtime.brain_info();
                                let meta = BrainMeta {
                                    agent_name: info.agent_name.clone(),
                                    num_groups: info.num_groups,
                                };
                                brains.insert(name.clone(), runtime);
                                loaded.write().unwrap().insert(name, meta.clone());
                                Ok(meta)
                            }
                            Err(e) => Err(anyhow::anyhow!("load brain: {}", e)),
                        };
                        let _ = reply.send(result);
                    }
                    BrainCmd::Unload { name, reply } => {
                        brains.remove(&name);
                        loaded.write().unwrap().remove(&name);
                        let _ = reply.send(());
                    }
                    BrainCmd::Generate {
                        name,
                        prompt,
                        reply,
                    } => {
                        let result = match brains.get_mut(&name) {
                            Some(rt) => rt
                                .converse(&prompt)
                                .map(|r| r.text)
                                .map_err(|e| anyhow::anyhow!("inference: {}", e)),
                            None => Err(anyhow::anyhow!("brain not loaded: {}", name)),
                        };
                        let _ = reply.send(result);
                    }
                }
            }
        });
    }

    pub async fn load_model(&self, name: &str, path: PathBuf) -> Result<()> {
        let data = tokio::fs::read(&path)
            .await
            .map_err(|e| anyhow::anyhow!("read {:?}: {}", path, e))?;

        let (tx, rx) = oneshot::channel();
        self.tx
            .send(BrainCmd::Load {
                name: name.to_string(),
                data,
                reply: tx,
            })
            .map_err(|_| anyhow::anyhow!("brain thread gone"))?;

        let meta = rx
            .await
            .map_err(|_| anyhow::anyhow!("brain thread dropped"))??;
        tracing::info!(
            "Loaded growformer brain '{}': agent={}, num_groups={}",
            name,
            meta.agent_name,
            meta.num_groups,
        );
        Ok(())
    }

    pub async fn is_loaded(&self, name: &str) -> bool {
        self.loaded.read().unwrap().contains_key(name)
    }

    pub async fn unload_model(&self, name: &str) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(BrainCmd::Unload {
                name: name.to_string(),
                reply: tx,
            })
            .map_err(|_| anyhow::anyhow!("brain thread gone"))?;
        rx.await
            .map_err(|_| anyhow::anyhow!("brain thread dropped"))?;
        tracing::info!("Unloaded growformer brain: {}", name);
        Ok(())
    }

    pub async fn generate_text(
        &self,
        name: &str,
        prompt: &str,
        _max_tokens: usize,
        _temperature: f32,
    ) -> Result<String> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(BrainCmd::Generate {
                name: name.to_string(),
                prompt: prompt.to_string(),
                reply: tx,
            })
            .map_err(|_| anyhow::anyhow!("brain thread gone"))?;
        rx.await
            .map_err(|_| anyhow::anyhow!("brain thread dropped"))?
    }

    pub async fn list_models(&self) -> Vec<String> {
        self.loaded.read().unwrap().keys().cloned().collect()
    }
}

/// Inspect a `.bin` brain without registering it with the manager.
pub fn peek_brain_path(path: impl AsRef<std::path::Path>) -> Result<BrainMeta> {
    let data = std::fs::read(path.as_ref()).with_context(|| format!("read {:?}", path.as_ref()))?;
    peek_brain_bytes(&data)
}

pub fn peek_brain_bytes(data: &[u8]) -> Result<BrainMeta> {
    let rt = GrowformerRuntime::from_brain_bytes(data).map_err(|e| anyhow::anyhow!("{}", e))?;
    let info = rt.brain_info();
    Ok(BrainMeta {
        agent_name: info.agent_name.clone(),
        num_groups: info.num_groups,
    })
}

impl Default for GrowformerModelManager {
    fn default() -> Self {
        Self::new()
    }
}
