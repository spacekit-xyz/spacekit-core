//! Accumulate token usage and cost from completion streams. Exposed via /metrics.

use crate::providers::ProviderStream;
use futures_util::Stream;
use std::pin::Pin;
use std::sync::RwLock;
use std::task::{Context, Poll};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::storage_client::{CompletionReceipt, StorageClient};

/// In-memory aggregate of completion usage and cost.
#[derive(Debug, Default)]
pub struct CostTracker {
    pub request_count: u64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cost_usd: f64,
}

pub type SharedCostTracker = std::sync::Arc<RwLock<CostTracker>>;

#[derive(Clone)]
pub struct ReceiptContext {
    pub storage: StorageClient,
    pub request_id: String,
    pub key_id: String,
    pub owner_did: String,
    pub provider: String,
    pub model: String,
    pub task: String,
}

impl CostTracker {
    pub fn shared() -> SharedCostTracker {
        std::sync::Arc::new(RwLock::new(CostTracker::default()))
    }

    pub fn record(&mut self, input_tokens: u64, output_tokens: u64, cost_usd: f64) {
        self.request_count = self.request_count.saturating_add(1);
        self.total_input_tokens = self.total_input_tokens.saturating_add(input_tokens);
        self.total_output_tokens = self.total_output_tokens.saturating_add(output_tokens);
        self.total_cost_usd += cost_usd;
    }
}

/// Parse SSE chunk for OpenAI-style "usage": {"prompt_tokens": N, "completion_tokens": M}.
pub fn parse_usage_from_sse(chunk: &[u8]) -> Option<(u64, u64)> {
    let s = std::str::from_utf8(chunk).ok()?;
    let usage_start = s.find("\"usage\"")?;
    let rest = &s[usage_start..];
    let pt = rest.find("\"prompt_tokens\"")?;
    let after_pt = &rest[pt + 16..];
    let pt_val_end = after_pt.find(',').or_else(|| after_pt.find('}'))?;
    let pt_val: u64 = after_pt[..pt_val_end].trim().parse().ok()?;
    let ct = rest.find("\"completion_tokens\"")?;
    let after_ct = &rest[ct + 19..];
    let ct_val_end = after_ct.find(',').or_else(|| after_ct.find('}'))?;
    let ct_val: u64 = after_ct[..ct_val_end].trim().parse().ok()?;
    Some((pt_val, ct_val))
}

/// Wraps a ProviderStream to parse usage from SSE and record to CostTracker. Forwards all bytes unchanged.
pub fn wrap_stream_usage(
    stream: ProviderStream,
    input_cost_per_token: f64,
    output_cost_per_token: f64,
    tracker: SharedCostTracker,
    receipt: Option<ReceiptContext>,
) -> ProviderStream {
    struct UsageStream {
        inner: ProviderStream,
        input_cost: f64,
        output_cost: f64,
        tracker: SharedCostTracker,
        seen_usage: bool,
        input_tokens: u64,
        output_tokens: u64,
        receipt: Option<ReceiptContext>,
        persisted: bool,
    }
    impl UsageStream {
        fn persist(&mut self, status: &'static str) {
            if self.persisted {
                return;
            }
            if !self.seen_usage {
                if let Ok(mut tracker) = self.tracker.write() {
                    tracker.record(0, 0, 0.0);
                }
            }
            let input_tokens = self.input_tokens;
            let output_tokens = self.output_tokens;
            let cost =
                self.input_cost * input_tokens as f64 + self.output_cost * output_tokens as f64;
            if let Some(context) = self.receipt.take() {
                tokio::spawn(async move {
                    let receipt = CompletionReceipt {
                        request_id: context.request_id,
                        key_id: context.key_id,
                        owner_did: context.owner_did,
                        provider: context.provider,
                        model: context.model,
                        task: context.task,
                        input_tokens,
                        output_tokens,
                        cost_usd: cost,
                        status: status.to_string(),
                        finished_at_unix: unix_now(),
                    };
                    if let Err(error) = context.storage.put_completion(&receipt).await {
                        tracing::error!(error = %error, request_id = %receipt.request_id, "failed to persist completion receipt");
                    }
                });
            }
            self.persisted = true;
        }
    }
    impl Drop for UsageStream {
        fn drop(&mut self) {
            self.persist("aborted");
        }
    }
    impl Stream for UsageStream {
        type Item = Result<bytes::Bytes, anyhow::Error>;
        fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            let p = Pin::new(&mut self.inner);
            match p.poll_next(cx) {
                Poll::Ready(Some(Ok(chunk))) => {
                    if !self.seen_usage {
                        if let Some((in_t, out_t)) = parse_usage_from_sse(&chunk) {
                            let cost =
                                self.input_cost * (in_t as f64) + self.output_cost * (out_t as f64);
                            if let Ok(mut t) = self.tracker.write() {
                                t.record(in_t, out_t, cost);
                            }
                            self.input_tokens = in_t;
                            self.output_tokens = out_t;
                            self.seen_usage = true;
                        }
                    }
                    Poll::Ready(Some(Ok(chunk)))
                }
                Poll::Ready(None) => {
                    self.persist("completed");
                    Poll::Ready(None)
                }
                other => other,
            }
        }
    }
    Box::pin(UsageStream {
        inner: stream,
        input_cost: input_cost_per_token,
        output_cost: output_cost_per_token,
        tracker,
        seen_usage: false,
        input_tokens: 0,
        output_tokens: 0,
        receipt,
        persisted: false,
    })
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}
