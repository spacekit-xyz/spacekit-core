//! Prometheus text exposition for operator dashboards (Stream F).

#![deny(clippy::all)]

use crate::storage_facade::AgenticHealth;

/// Render `AgenticHealth` counters as Prometheus 0.0.4 text.
pub fn render_prometheus(health: &AgenticHealth) -> String {
    let mut out = String::new();
    macro_rules! metric {
        ($name:expr, $help:expr, $val:expr) => {
            out.push_str(&format!(
                "# HELP spacekit_{} {}\n# TYPE spacekit_{} gauge\nspacekit_{} {}\n",
                $name, $help, $name, $name, $val
            ));
        };
    }
    metric!(
        "enable_real_transactions",
        "1 when persisted transaction apply is enabled",
        if health.enable_real_transactions {
            1
        } else {
            0
        }
    );
    metric!(
        "upload_tokens_configured",
        "1 when upload token signing is configured",
        if health.upload_tokens_configured {
            1
        } else {
            0
        }
    );
    metric!(
        "handoff_signing_configured",
        "1 when workspace handoff HMAC is configured",
        if health.handoff_signing_configured {
            1
        } else {
            0
        }
    );
    metric!(
        "require_handoff_signature",
        "1 when import requires handoff_signature",
        if health.require_handoff_signature {
            1
        } else {
            0
        }
    );
    out.push_str(&format!(
        "# HELP spacekit_blob_fact_auth_mode Blob/fact auth mode label (1=active)\n# TYPE spacekit_blob_fact_auth_mode gauge\nspacekit_blob_fact_auth_mode{{mode=\"{}\"}} 1\n",
        health.blob_fact_auth_mode
    ));
    metric!(
        "tx_commits_stub_finalize_total",
        "Transaction commits using stub finalize path",
        health.tx_commits_stub_finalize_total
    );
    metric!(
        "tx_commits_real_apply_ok_total",
        "Transaction commits with successful real apply",
        health.tx_commits_real_apply_ok_total
    );
    metric!(
        "tx_commits_real_apply_err_total",
        "Transaction commits with failed real apply",
        health.tx_commits_real_apply_err_total
    );
    metric!(
        "idempotency_cached_hits_total",
        "Idempotency cache hits",
        health.idempotency_cached_hits_total
    );
    metric!(
        "idempotency_fresh_proceeds_total",
        "Idempotency fresh proceeds",
        health.idempotency_fresh_proceeds_total
    );
    metric!(
        "idempotency_cache_hit_rate",
        "Idempotency cache hit rate",
        health.idempotency_cache_hit_rate
    );
    metric!(
        "did_rate_limit_rejections_total",
        "Per-DID rate limit rejections",
        health.did_rate_limit_rejections_total
    );
    metric!(
        "change_feed_live_subscribers",
        "Active change-feed SSE subscribers",
        health.change_feed_live_subscribers
    );
    metric!(
        "change_feed_current_seq",
        "Change-feed sequence number",
        health.change_feed_current_seq
    );
    metric!("sandboxes_total", "Sandboxes total", health.sandboxes_total);
    metric!(
        "sandboxes_active",
        "Sandboxes active",
        health.sandboxes_active
    );
    metric!(
        "sandboxes_committed",
        "Sandboxes committed",
        health.sandboxes_committed
    );
    metric!(
        "sandboxes_failed",
        "Sandboxes failed",
        health.sandboxes_failed
    );
    metric!(
        "sandboxes_quota_bytes_written",
        "Sum of sandbox bytes_written quotas",
        health.sandboxes_quota_bytes_written
    );
    out
}
