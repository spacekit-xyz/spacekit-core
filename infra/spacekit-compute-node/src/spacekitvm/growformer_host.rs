//! Dedicated-thread Growformer host: `growformer::runtime::Runtime` is not `Sync`, so inference runs
//! on a single worker (same pattern as `spacekit-simulator/src/growformer_model_manager.rs`).

use std::sync::mpsc;
use std::thread::JoinHandle;

enum GrowformerOp {
    Load {
        data: Vec<u8>,
        reply: mpsc::Sender<Result<usize, String>>,
    },
    Prompt {
        text: String,
        reply: mpsc::Sender<Result<String, String>>,
    },
    Converse {
        text: String,
        reply: mpsc::Sender<Result<String, String>>,
    },
    Codegen {
        text: String,
        reply: mpsc::Sender<Result<String, String>>,
    },
    BrainInfo {
        reply: mpsc::Sender<Result<String, String>>,
    },
    Reset,
}

/// Handle to a background thread that owns `growformer::runtime::Runtime`.
pub struct GrowformerThreadHost {
    tx: mpsc::Sender<GrowformerOp>,
    _join: JoinHandle<()>,
}

impl GrowformerThreadHost {
    pub fn spawn() -> Self {
        let (tx, rx) = mpsc::channel::<GrowformerOp>();
        let join = std::thread::Builder::new()
            .name("swtchvm-growformer".into())
            .spawn(move || {
                let mut rt: Option<growformer::runtime::Runtime> = None;
                while let Ok(op) = rx.recv() {
                    match op {
                        GrowformerOp::Load { data, reply } => {
                            let res = match growformer::runtime::Runtime::from_brain_bytes(&data) {
                                Ok(g) => {
                                    rt = Some(g);
                                    Ok(data.len())
                                }
                                Err(e) => {
                                    rt = None;
                                    Err(e)
                                }
                            };
                            let _ = reply.send(res);
                        }
                        GrowformerOp::Prompt { text, reply } => {
                            let out = match rt.as_mut() {
                                None => Err("growformer brain not loaded".to_string()),
                                Some(g) => {
                                    g.reset_conversation();
                                    let r = g.prompt(&text).and_then(|resp| {
                                        serde_json::to_string(&resp).map_err(|e| e.to_string())
                                    });
                                    g.reset_conversation();
                                    r
                                }
                            };
                            let _ = reply.send(out);
                        }
                        GrowformerOp::Converse { text, reply } => {
                            let out = match rt.as_mut() {
                                None => Err("growformer brain not loaded".to_string()),
                                Some(g) => g.converse(&text).and_then(|resp| {
                                    serde_json::to_string(&resp).map_err(|e| e.to_string())
                                }),
                            };
                            let _ = reply.send(out);
                        }
                        GrowformerOp::Codegen { text, reply } => {
                            let out = match rt.as_mut() {
                                None => Err("growformer brain not loaded".to_string()),
                                Some(g) => {
                                    g.reset_conversation();
                                    let r = g.codegen(&text).and_then(|code| {
                                        serde_json::to_string(&code).map_err(|e| e.to_string())
                                    });
                                    g.reset_conversation();
                                    r
                                }
                            };
                            let _ = reply.send(out);
                        }
                        GrowformerOp::BrainInfo { reply } => {
                            let out = match rt.as_ref() {
                                None => Err("growformer brain not loaded".to_string()),
                                Some(g) => {
                                    let info = g.brain_info();
                                    serde_json::to_string(&info).map_err(|e| e.to_string())
                                }
                            };
                            let _ = reply.send(out);
                        }
                        GrowformerOp::Reset => {
                            if let Some(g) = rt.as_mut() {
                                g.reset_conversation();
                            }
                        }
                    }
                }
            })
            .expect("spawn growformer worker");
        Self { tx, _join: join }
    }

    pub fn load_brain(&self, data: Vec<u8>) -> Result<usize, String> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.tx
            .send(GrowformerOp::Load {
                data,
                reply: reply_tx,
            })
            .map_err(|_| "growformer worker disconnected".to_string())?;
        reply_rx
            .recv()
            .map_err(|_| "growformer worker dropped reply")?
    }

    pub fn prompt_json(&self, text: &str) -> Result<String, String> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.tx
            .send(GrowformerOp::Prompt {
                text: text.to_string(),
                reply: reply_tx,
            })
            .map_err(|_| "growformer worker disconnected".to_string())?;
        reply_rx
            .recv()
            .map_err(|_| "growformer worker dropped reply")?
    }

    pub fn converse_json(&self, text: &str) -> Result<String, String> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.tx
            .send(GrowformerOp::Converse {
                text: text.to_string(),
                reply: reply_tx,
            })
            .map_err(|_| "growformer worker disconnected".to_string())?;
        reply_rx
            .recv()
            .map_err(|_| "growformer worker dropped reply")?
    }

    pub fn codegen_json(&self, text: &str) -> Result<String, String> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.tx
            .send(GrowformerOp::Codegen {
                text: text.to_string(),
                reply: reply_tx,
            })
            .map_err(|_| "growformer worker disconnected".to_string())?;
        reply_rx
            .recv()
            .map_err(|_| "growformer worker dropped reply")?
    }

    pub fn brain_info_json(&self) -> Result<String, String> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.tx
            .send(GrowformerOp::BrainInfo { reply: reply_tx })
            .map_err(|_| "growformer worker disconnected".to_string())?;
        reply_rx
            .recv()
            .map_err(|_| "growformer worker dropped reply")?
    }

    pub fn reset_conversation(&self) {
        let _ = self.tx.send(GrowformerOp::Reset);
    }
}
