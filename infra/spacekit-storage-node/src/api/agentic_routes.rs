//! Agentic-readiness HTTP routes (Phase 0/1/3/4).
//!
//! Mounted by [`crate::api::ApiServer`] alongside the legacy routes:
//!
//! - `/api/transactions/*` — begin / record modification / commit / rollback / savepoints / trace
//! - `/api/sandboxes/*` — create / get / commit / discard / extend / journal
//! - `/api/changes` — Server-Sent Events stream of the change feed
//! - `/api/agentic/health` — operator snapshot (tx path counters, idempotency, sandboxes, change feed)
//!
//! Idempotency and per-DID rate limits are applied by the same module via
//! filters that wrap any write route.

#![deny(clippy::all)]

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use warp::{Filter, Rejection, Reply};

use crate::idempotency::{
    CachedResponse, Decision, DidRateLimiter, IdempotencyCache, IdempotencyConfig,
    DEFAULT_INFLIGHT_WAIT_MS,
};
use crate::memory_diagnostic::{collect_memory_report, MemoryDiagnosticSources};
use crate::sandbox::{
    caller_may_access_sandbox, ConflictPolicy, SandboxAccess, SandboxConfig, SandboxConfigSerde,
};
use crate::storage_facade::Facade;
use crate::transaction::IsolationLevel;
use crate::StorageNodeConfig;

/// Live handles for `GET /api/agentic/memory`.
#[derive(Clone)]
pub struct AgenticMemoryRouteState {
    pub config: StorageNodeConfig,
    pub database: Arc<crate::database::Database>,
    pub sources: MemoryDiagnosticSources,
}

#[derive(Debug, Deserialize)]
pub struct RecordTxModificationRequest {
    pub modification: crate::transaction::TransactionModification,
    #[serde(default)]
    pub conflict_policy: ConflictPolicy,
    #[serde(default)]
    pub bytes_written: Option<u64>,
}

// ---- Begin / commit / rollback / savepoints ----

#[derive(Debug, Deserialize)]
pub struct BeginTxRequest {
    pub isolation: Option<String>,
    pub timeout_seconds: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct BeginTxResponse {
    pub transaction_id: String,
    pub isolation: String,
    pub real_apply_enabled: bool,
}

#[derive(Debug, Deserialize, Default)]
pub struct SavepointRequest {
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct SavepointResponse {
    pub savepoint_id: String,
    pub name: String,
}

// ---- Sandbox payloads ----

#[derive(Debug, Deserialize, Default)]
pub struct CreateSandboxRequest {
    pub owner_did: Option<String>,
    #[serde(default)]
    pub collaborator_dids: Option<Vec<String>>,
    pub ttl_seconds: Option<u64>,
    pub max_bytes_written: Option<u64>,
    pub max_vector_ops: Option<u64>,
    pub max_fact_puts: Option<u64>,
    pub base_snapshot: Option<String>,
    /// When set, sandbox limits are capped by the workspace fact (`spacekit:workspace:v1`).
    pub workspace_id: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct CommitSandboxQuery {
    #[serde(default)]
    pub dry_run: bool,
}

// ---- Filters ----

fn with_facade(
    facade: Arc<Facade>,
) -> impl Filter<Extract = (Arc<Facade>,), Error = Infallible> + Clone {
    warp::any().map(move || facade.clone())
}

/// Extract the requesting DID from `Authorization: DID <did>`. Falls back to
/// the `X-DID` header for tests.
fn with_did() -> impl Filter<Extract = (Option<String>,), Error = Rejection> + Clone {
    warp::header::optional::<String>("authorization")
        .and(warp::header::optional::<String>("x-did"))
        .map(|auth: Option<String>, x_did: Option<String>| {
            if let Some(value) = auth {
                if let Some(rest) = value.strip_prefix("DID ") {
                    return Some(rest.trim().to_string());
                }
                if let Some(rest) = value.strip_prefix("Bearer ") {
                    if rest.starts_with("did:") {
                        return Some(rest.to_string());
                    }
                }
            }
            x_did
        })
}

fn with_idempotency_key() -> impl Filter<Extract = (Option<String>,), Error = Rejection> + Clone {
    warp::header::optional::<String>("idempotency-key")
}

// ---- Idempotency + rate-limit wrapper ----

/// Run a handler under the idempotency cache + per-DID rate limiter. Generic
/// over the body-bytes so we fingerprint exactly what the agent sent.
pub async fn run_idempotent<F, Fut>(
    cache: Arc<IdempotencyCache>,
    rate_limiter: Arc<DidRateLimiter>,
    did: Option<String>,
    route: &str,
    key: Option<String>,
    body_bytes: &[u8],
    handler: F,
) -> Result<warp::reply::Response, Rejection>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<warp::reply::Response, Rejection>>,
{
    let did = did.unwrap_or_default();
    if !did.is_empty() {
        if let Err(retry_after) = rate_limiter.check(&did).await {
            let mut resp = warp::reply::Response::new(
                serde_json::json!({
                    "error": "rate_limited",
                    "retry_after_seconds": retry_after.as_secs_f64(),
                })
                .to_string()
                .into(),
            );
            *resp.status_mut() = warp::http::StatusCode::TOO_MANY_REQUESTS;
            resp.headers_mut().insert(
                "Retry-After",
                warp::http::HeaderValue::from_str(&format!("{}", retry_after.as_secs().max(1)))
                    .unwrap(),
            );
            return Ok(resp);
        }
    }

    let key = match key {
        Some(k) if !k.trim().is_empty() => k,
        _ => return handler().await,
    };

    let fp = IdempotencyCache::fingerprint(body_bytes);
    let cfg = cache.route_config(route).await;
    let _ = cfg; // configured per-route below

    match cache.check(&did, route, &key, fp).await {
        Decision::CachedHit(cached) => Ok(cached_to_response(cached)),
        Decision::FingerprintMismatch { expected, got } => {
            let mut resp = warp::reply::Response::new(
                serde_json::json!({
                    "error": "idempotency_fingerprint_mismatch",
                    "expected_fingerprint": hex::encode(expected),
                    "got_fingerprint": hex::encode(got),
                })
                .to_string()
                .into(),
            );
            *resp.status_mut() = warp::http::StatusCode::UNPROCESSABLE_ENTITY;
            Ok(resp)
        }
        Decision::InFlightWait {
            notify,
            wait_timeout_ms,
        } => {
            let waited = IdempotencyCache::wait_for_inflight(
                notify,
                Duration::from_millis(wait_timeout_ms.max(DEFAULT_INFLIGHT_WAIT_MS)),
            )
            .await;
            if !waited {
                let mut resp = warp::reply::Response::new(
                    serde_json::json!({
                        "error": "idempotency_in_flight_timeout",
                    })
                    .to_string()
                    .into(),
                );
                *resp.status_mut() = warp::http::StatusCode::CONFLICT;
                return Ok(resp);
            }
            // The original request finished — re-check, which will be CachedHit.
            match cache.check(&did, route, &key, fp).await {
                Decision::CachedHit(cached) => Ok(cached_to_response(cached)),
                _ => handler().await,
            }
        }
        Decision::Proceed => match handler().await {
            Ok(resp) => {
                let (parts, body) = resp.into_parts();
                let body_bytes_resp = match hyper::body::to_bytes(body).await {
                    Ok(b) => b.to_vec(),
                    Err(_) => Vec::new(),
                };
                let cached = CachedResponse {
                    status: parts.status.as_u16(),
                    body: body_bytes_resp.clone(),
                    headers: parts
                        .headers
                        .iter()
                        .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
                        .collect(),
                    fingerprint: fp,
                    stored_at: chrono::Utc::now(),
                    ttl_seconds: cache.route_config(route).await.ttl_seconds,
                };
                let _ = cache.store(&did, route, &key, cached).await;
                let mut new_resp = warp::reply::Response::new(body_bytes_resp.into());
                *new_resp.status_mut() = parts.status;
                *new_resp.headers_mut() = parts.headers;
                Ok(new_resp)
            }
            Err(rej) => {
                cache.cancel(&did, route, &key).await;
                Err(rej)
            }
        },
    }
}

fn cached_to_response(c: CachedResponse) -> warp::reply::Response {
    let mut resp = warp::reply::Response::new(c.body.into());
    *resp.status_mut() =
        warp::http::StatusCode::from_u16(c.status).unwrap_or(warp::http::StatusCode::OK);
    for (k, v) in c.headers {
        if let (Ok(name), Ok(value)) = (
            warp::http::HeaderName::from_bytes(k.as_bytes()),
            warp::http::HeaderValue::from_str(&v),
        ) {
            resp.headers_mut().insert(name, value);
        }
    }
    resp.headers_mut().insert(
        "X-Idempotent-Replay",
        warp::http::HeaderValue::from_static("true"),
    );
    resp
}

// ---- Build the agentic router ----

pub fn build_routes(
    facade: Arc<Facade>,
    memory: Option<AgenticMemoryRouteState>,
) -> impl Filter<Extract = (Box<dyn Reply>,), Error = Rejection> + Clone {
    // Configure idempotency cache TTLs per route. These are sane defaults that
    // can be overridden by operator config.
    {
        let cache = facade.idempotency.clone();
        tokio::spawn(async move {
            cache
                .configure_route(
                    "POST /api/transactions",
                    IdempotencyConfig {
                        ttl_seconds: 24 * 60 * 60,
                        wait_timeout_ms: DEFAULT_INFLIGHT_WAIT_MS,
                    },
                )
                .await;
            cache
                .configure_route(
                    "POST /api/transactions/modifications",
                    IdempotencyConfig {
                        ttl_seconds: 24 * 60 * 60,
                        wait_timeout_ms: DEFAULT_INFLIGHT_WAIT_MS,
                    },
                )
                .await;
            cache
                .configure_route(
                    "POST /api/sandboxes",
                    IdempotencyConfig {
                        ttl_seconds: 24 * 60 * 60,
                        wait_timeout_ms: DEFAULT_INFLIGHT_WAIT_MS,
                    },
                )
                .await;
            cache
                .configure_route(
                    "POST /api/sandboxes/commit",
                    IdempotencyConfig {
                        ttl_seconds: 7 * 24 * 60 * 60,
                        wait_timeout_ms: 60_000,
                    },
                )
                .await;
        });
    }

    // GET /api/agentic/health — operator snapshot (no auth; restrict at the edge).
    let agentic_health = warp::path!("api" / "agentic" / "health")
        .and(warp::get())
        .and(with_facade(facade.clone()))
        .and_then(handle_agentic_health);

    let agentic_metrics = warp::path!("api" / "agentic" / "metrics")
        .and(warp::get())
        .and(with_facade(facade.clone()))
        .and_then(handle_agentic_metrics);

    let mem_state = memory;
    let agentic_memory = warp::path!("api" / "agentic" / "memory")
        .and(warp::get())
        .and(with_facade(facade.clone()))
        .and(warp::any().map(move || mem_state.clone()))
        .and_then(handle_agentic_memory_opt);

    // GET /api/operators/self — federation discovery (public; cache at edge)
    let operator_self = warp::path!("api" / "operators" / "self")
        .and(warp::get())
        .and(warp::query::<OperatorSelfQuery>())
        .and(with_facade(facade.clone()))
        .and_then(handle_operator_self);

    // POST /api/transactions
    let begin = warp::path!("api" / "transactions")
        .and(warp::post())
        .and(with_facade(facade.clone()))
        .and(with_did())
        .and(with_idempotency_key())
        .and(warp::body::bytes())
        .and_then(handle_begin_tx);

    // POST /api/transactions/{id}/modifications  (optional `X-Sandbox-Id` for journal mirror)
    let record_tx_mod = warp::path!("api" / "transactions" / String / "modifications")
        .and(warp::post())
        .and(with_facade(facade.clone()))
        .and(with_did())
        .and(with_idempotency_key())
        .and(warp::header::optional::<String>("x-sandbox-id"))
        .and(warp::body::bytes())
        .and_then(handle_record_tx_modification);

    // GET /api/transactions/{id}
    let get_tx = warp::path!("api" / "transactions" / String)
        .and(warp::get())
        .and(with_facade(facade.clone()))
        .and_then(handle_get_tx);

    // POST /api/transactions/{id}/commit
    let commit = warp::path!("api" / "transactions" / String / "commit")
        .and(warp::post())
        .and(with_facade(facade.clone()))
        .and_then(handle_commit_tx);

    // POST /api/transactions/{id}/rollback
    let rollback = warp::path!("api" / "transactions" / String / "rollback")
        .and(warp::post())
        .and(with_facade(facade.clone()))
        .and_then(handle_rollback_tx);

    // POST /api/transactions/{id}/savepoints
    let savepoint = warp::path!("api" / "transactions" / String / "savepoints")
        .and(warp::post())
        .and(with_facade(facade.clone()))
        .and(warp::body::json())
        .and_then(handle_create_savepoint);

    // POST /api/transactions/{id}/savepoints/{name}/rollback
    let savepoint_rollback =
        warp::path!("api" / "transactions" / String / "savepoints" / String / "rollback")
            .and(warp::post())
            .and(with_facade(facade.clone()))
            .and_then(handle_rollback_to_savepoint);

    // GET /api/transactions/{id}/trace
    let trace = warp::path!("api" / "transactions" / String / "trace")
        .and(warp::get())
        .and(with_facade(facade.clone()))
        .and_then(handle_get_trace);

    // POST /api/sandboxes
    let create_sb = warp::path!("api" / "sandboxes")
        .and(warp::post())
        .and(with_facade(facade.clone()))
        .and(with_did())
        .and(with_idempotency_key())
        .and(warp::body::bytes())
        .and_then(handle_create_sandbox);

    // GET /api/sandboxes/{id}
    let get_sb = warp::path!("api" / "sandboxes" / String)
        .and(warp::get())
        .and(with_facade(facade.clone()))
        .and(with_did())
        .and_then(handle_get_sandbox);

    // POST /api/sandboxes/{id}/commit?dry_run=bool
    let commit_sb = warp::path!("api" / "sandboxes" / String / "commit")
        .and(warp::post())
        .and(warp::query::<CommitSandboxQuery>())
        .and(with_facade(facade.clone()))
        .and(with_did())
        .and_then(handle_commit_sandbox);

    // POST /api/sandboxes/{id}/discard
    let discard_sb = warp::path!("api" / "sandboxes" / String / "discard")
        .and(warp::post())
        .and(with_facade(facade.clone()))
        .and(with_did())
        .and_then(handle_discard_sandbox);

    // POST /api/sandboxes/{id}/extend
    let extend_sb = warp::path!("api" / "sandboxes" / String / "extend")
        .and(warp::post())
        .and(warp::body::json())
        .and(with_facade(facade.clone()))
        .and(with_did())
        .and_then(handle_extend_sandbox);

    // GET /api/sandboxes/{id}/journal
    let journal_sb = warp::path!("api" / "sandboxes" / String / "journal")
        .and(warp::get())
        .and(with_facade(facade.clone()))
        .and(with_did())
        .and_then(handle_get_journal);

    // GET /api/changes  (SSE)
    let changes = warp::path!("api" / "changes")
        .and(warp::get())
        .and(warp::query::<ChangesQuery>())
        .and(warp::header::optional::<String>("last-event-id"))
        .and(with_facade(facade.clone()))
        .and_then(handle_changes_sse);

    // POST /api/upload-tokens
    let mint_upload_token = warp::path!("api" / "upload-tokens")
        .and(warp::post())
        .and(with_facade(facade.clone()))
        .and(with_did())
        .and(warp::body::json())
        .and_then(handle_mint_upload_token);

    // POST /api/workspaces
    let create_ws = warp::path!("api" / "workspaces")
        .and(warp::post())
        .and(with_facade(facade.clone()))
        .and(with_did())
        .and(warp::body::json())
        .and_then(handle_create_workspace);

    // GET /api/workspaces/{id}
    let get_ws = warp::path!("api" / "workspaces" / String)
        .and(warp::get())
        .and(with_facade(facade.clone()))
        .and(with_did())
        .and_then(handle_get_workspace);

    // PUT /api/workspaces/{id}
    let update_ws = warp::path!("api" / "workspaces" / String)
        .and(warp::put())
        .and(with_facade(facade.clone()))
        .and(with_did())
        .and(warp::body::json())
        .and_then(handle_update_workspace);

    // GET /api/workspaces/{id}/export — federation handoff bundle
    let export_ws = warp::path!("api" / "workspaces" / String / "export")
        .and(warp::get())
        .and(with_facade(facade.clone()))
        .and(with_did())
        .and_then(handle_export_workspace);

    // POST /api/blobs/replicate — pull CAS objects from a remote node
    let replicate_blobs = warp::path!("api" / "blobs" / "replicate")
        .and(warp::post())
        .and(with_facade(facade.clone()))
        .and(warp::body::json())
        .and_then(handle_replicate_blobs);

    // POST /api/workspaces/import — federation destination
    let import_ws = warp::path!("api" / "workspaces" / "import")
        .and(warp::post())
        .and(with_facade(facade.clone()))
        .and(with_did())
        .and(warp::body::json())
        .and_then(handle_import_workspace);

    // GET /api/workspaces?owner_did=...
    let list_ws = warp::path!("api" / "workspaces")
        .and(warp::get())
        .and(warp::query::<ListWorkspacesQuery>())
        .and(with_facade(facade.clone()))
        .and_then(handle_list_workspaces);

    let routes = agentic_health
        .or(agentic_metrics)
        .or(agentic_memory)
        .or(operator_self)
        .or(begin)
        .or(record_tx_mod)
        .or(get_tx)
        .or(commit)
        .or(rollback)
        .or(savepoint)
        .or(savepoint_rollback)
        .or(trace)
        .or(create_sb)
        .or(get_sb)
        .or(commit_sb)
        .or(discard_sb)
        .or(extend_sb)
        .or(journal_sb)
        .or(changes)
        .or(mint_upload_token)
        .or(create_ws)
        .or(get_ws)
        .or(update_ws)
        .or(export_ws)
        .or(import_ws)
        .or(replicate_blobs)
        .or(list_ws)
        .or(crate::api::content_routes::build_routes(facade.clone()))
        .boxed();
    routes.map(|reply| -> Box<dyn Reply> { Box::new(reply) })
}

// ---- Handlers ----

async fn handle_begin_tx(
    facade: Arc<Facade>,
    did: Option<String>,
    key: Option<String>,
    body: bytes::Bytes,
) -> Result<warp::reply::Response, Rejection> {
    let cache = facade.idempotency.clone();
    let rate = facade.did_rate_limiter.clone();
    let body_bytes = body.to_vec();
    let body_for_handler = body_bytes.clone();
    let facade_for_handler = facade.clone();
    run_idempotent(
        cache,
        rate,
        did,
        "POST /api/transactions",
        key,
        &body_bytes,
        move || async move {
            let req: BeginTxRequest = if body_for_handler.is_empty() {
                BeginTxRequest {
                    isolation: None,
                    timeout_seconds: None,
                }
            } else {
                serde_json::from_slice(&body_for_handler).map_err(|_| warp::reject::reject())?
            };
            let isolation = req
                .isolation
                .as_deref()
                .map(parse_isolation)
                .transpose()
                .map_err(|_| warp::reject::reject())?;
            let id = facade_for_handler
                .begin_transaction(isolation, req.timeout_seconds)
                .await
                .map_err(|_| warp::reject::reject())?;
            let resp = BeginTxResponse {
                transaction_id: id,
                isolation: format!("{:?}", isolation.unwrap_or(IsolationLevel::Serializable)),
                real_apply_enabled: facade_for_handler.transactions.real_apply_enabled(),
            };
            Ok(json_response(&resp, warp::http::StatusCode::CREATED))
        },
    )
    .await
}

async fn handle_record_tx_modification(
    tx_id: String,
    facade: Arc<Facade>,
    did: Option<String>,
    key: Option<String>,
    sandbox_id: Option<String>,
    body: bytes::Bytes,
) -> Result<warp::reply::Response, Rejection> {
    let cache = facade.idempotency.clone();
    let rate = facade.did_rate_limiter.clone();
    let body_bytes = body.to_vec();
    let body_for_handler = body_bytes.clone();
    let facade_for_handler = facade.clone();
    let tx_id_for = tx_id;
    let sandbox_for = sandbox_id.clone();
    let did_for = did.clone();
    run_idempotent(
        cache,
        rate,
        did,
        "POST /api/transactions/modifications",
        key,
        &body_bytes,
        move || async move {
            let req: RecordTxModificationRequest = match serde_json::from_slice(&body_for_handler) {
                Ok(r) => r,
                Err(e) => {
                    return Ok(json_response(
                        &serde_json::json!({"error": format!("invalid body: {e}")}),
                        warp::http::StatusCode::BAD_REQUEST,
                    ));
                }
            };
            let bytes = req.bytes_written.unwrap_or(0);
            let sand = sandbox_for.as_deref();
            match facade_for_handler
                .record_transaction_modification(
                    &tx_id_for,
                    req.modification,
                    req.conflict_policy,
                    bytes,
                    sand,
                    did_for.as_deref(),
                )
                .await
            {
                Ok(()) => Ok(json_response(
                    &serde_json::json!({"recorded": true, "transaction_id": tx_id_for}),
                    warp::http::StatusCode::OK,
                )),
                Err(e) => {
                    let msg = e.to_string();
                    let status = if msg.starts_with("FORBIDDEN:") {
                        warp::http::StatusCode::FORBIDDEN
                    } else if msg.starts_with("NOTFOUND:") {
                        warp::http::StatusCode::NOT_FOUND
                    } else {
                        warp::http::StatusCode::CONFLICT
                    };
                    Ok(json_response(
                        &serde_json::json!({"error": msg, "transaction_id": tx_id_for}),
                        status,
                    ))
                }
            }
        },
    )
    .await
}

#[derive(Debug, Deserialize, Default)]
struct OperatorSelfQuery {
    /// Override public HTTP base (default `http://127.0.0.1:3030` or `SPACEKIT_PUBLIC_HTTP_URL`).
    pub public_url: Option<String>,
}

async fn handle_operator_self(
    query: OperatorSelfQuery,
    facade: Arc<Facade>,
) -> Result<warp::reply::Response, Rejection> {
    let base = query
        .public_url
        .or_else(|| std::env::var("SPACEKIT_PUBLIC_HTTP_URL").ok())
        .unwrap_or_else(|| "http://127.0.0.1:3030".to_string());
    let base = base.trim_end_matches('/').to_string();
    match facade.operator_self(base).await {
        Ok(body) => {
            let mut resp = json_response(&body, warp::http::StatusCode::OK);
            resp.headers_mut().insert(
                warp::http::header::CACHE_CONTROL,
                warp::http::HeaderValue::from_static("public, max-age=300"),
            );
            Ok(resp)
        }
        Err(e) => Ok(json_response(
            &serde_json::json!({"error": e.to_string()}),
            warp::http::StatusCode::SERVICE_UNAVAILABLE,
        )),
    }
}

async fn handle_agentic_health(facade: Arc<Facade>) -> Result<warp::reply::Response, Rejection> {
    let h = facade.agentic_health().await;
    Ok(json_response(&h, warp::http::StatusCode::OK))
}

async fn handle_agentic_memory_opt(
    facade: Arc<Facade>,
    mem: Option<AgenticMemoryRouteState>,
) -> Result<warp::reply::Response, Rejection> {
    let Some(mem) = mem else {
        return Ok(json_response(
            &serde_json::json!({
                "error": "memory diagnostic not wired (restart storage after upgrade)"
            }),
            warp::http::StatusCode::SERVICE_UNAVAILABLE,
        ));
    };
    let report = collect_memory_report(
        &mem.config,
        &mem.database,
        &facade,
        &mem.sources,
        None,
        None,
    )
    .await;
    Ok(json_response(&report, warp::http::StatusCode::OK))
}

async fn handle_agentic_metrics(facade: Arc<Facade>) -> Result<warp::reply::Response, Rejection> {
    let h = facade.agentic_health().await;
    let body = crate::operator_metrics::render_prometheus(&h);
    let mut resp = warp::reply::Response::new(body.into());
    resp.headers_mut().insert(
        warp::http::header::CONTENT_TYPE,
        warp::http::HeaderValue::from_static("text/plain; version=0.0.4; charset=utf-8"),
    );
    Ok(resp)
}

async fn handle_replicate_blobs(
    facade: Arc<Facade>,
    req: ReplicateBlobsRequest,
) -> Result<warp::reply::Response, Rejection> {
    let cas = match facade.cas_data_dir() {
        Some(d) => d,
        None => {
            return Ok(json_response(
                &serde_json::json!({"error": "cas_data_dir not configured"}),
                warp::http::StatusCode::SERVICE_UNAVAILABLE,
            ));
        }
    };
    match crate::federation::replicate_blobs_from_source(
        cas,
        &req.source_url,
        &req.hashes,
        req.source_authorization.as_deref(),
    )
    .await
    {
        Ok(report) => Ok(json_response(&report, warp::http::StatusCode::OK)),
        Err(e) => Ok(json_response(
            &serde_json::json!({"error": e.to_string()}),
            warp::http::StatusCode::BAD_REQUEST,
        )),
    }
}

async fn handle_import_workspace(
    facade: Arc<Facade>,
    did: Option<String>,
    req: ImportWorkspaceRequest,
) -> Result<warp::reply::Response, Rejection> {
    let caller = did.ok_or_else(|| warp::reject::reject())?;
    let conflict = req
        .on_conflict
        .as_deref()
        .and_then(crate::workspace::WorkspaceImportConflict::parse)
        .unwrap_or_default();
    match facade
        .import_workspace(
            &caller,
            req.bundle,
            conflict,
            req.owner_did,
            req.replicate_blobs_from.as_deref(),
            req.replicate_source_authorization.as_deref(),
        )
        .await
    {
        Ok(result) => {
            let status = if result.created {
                warp::http::StatusCode::CREATED
            } else {
                warp::http::StatusCode::OK
            };
            Ok(json_response(&result, status))
        }
        Err(e) => {
            let msg = e.to_string();
            let status = if msg.starts_with("CONFLICT:") {
                warp::http::StatusCode::CONFLICT
            } else if msg.starts_with("FORBIDDEN:") {
                warp::http::StatusCode::FORBIDDEN
            } else {
                warp::http::StatusCode::BAD_REQUEST
            };
            Ok(json_response(&serde_json::json!({"error": msg}), status))
        }
    }
}

async fn handle_export_workspace(
    workspace_id: String,
    facade: Arc<Facade>,
    did: Option<String>,
) -> Result<warp::reply::Response, Rejection> {
    let owner = did.ok_or_else(|| warp::reject::reject())?;
    match facade.export_workspace(&owner, &workspace_id).await {
        Ok(Some(bundle)) => Ok(json_response(&bundle, warp::http::StatusCode::OK)),
        Ok(None) => Ok(empty_response(warp::http::StatusCode::NOT_FOUND)),
        Err(e) => Ok(json_response(
            &serde_json::json!({"error": e.to_string()}),
            warp::http::StatusCode::CONFLICT,
        )),
    }
}

async fn handle_get_tx(
    id: String,
    facade: Arc<Facade>,
) -> Result<warp::reply::Response, Rejection> {
    match facade.get_transaction(&id).await {
        Some(tx) => Ok(json_response(
            &serde_json::json!({
                "id": tx.id,
                "state": format!("{:?}", tx.state),
                "isolation": format!("{:?}", tx.isolation_level),
                "created_at": tx.created_at,
                "modifications": tx.modifications.len(),
                "savepoints": tx.savepoints.len(),
                "trace_length": tx.trace.len(),
            }),
            warp::http::StatusCode::OK,
        )),
        None => Ok(empty_response(warp::http::StatusCode::NOT_FOUND)),
    }
}

async fn handle_commit_tx(
    id: String,
    facade: Arc<Facade>,
) -> Result<warp::reply::Response, Rejection> {
    match facade.commit_transaction(&id).await {
        Ok(()) => Ok(json_response(
            &serde_json::json!({"committed": true, "transaction_id": id}),
            warp::http::StatusCode::OK,
        )),
        Err(e) => Ok(json_response(
            &serde_json::json!({"committed": false, "error": e.to_string()}),
            warp::http::StatusCode::CONFLICT,
        )),
    }
}

async fn handle_rollback_tx(
    id: String,
    facade: Arc<Facade>,
) -> Result<warp::reply::Response, Rejection> {
    match facade.rollback_transaction(&id).await {
        Ok(()) => Ok(json_response(
            &serde_json::json!({"rolled_back": true, "transaction_id": id}),
            warp::http::StatusCode::OK,
        )),
        Err(e) => Ok(json_response(
            &serde_json::json!({"rolled_back": false, "error": e.to_string()}),
            warp::http::StatusCode::CONFLICT,
        )),
    }
}

async fn handle_create_savepoint(
    id: String,
    facade: Arc<Facade>,
    req: SavepointRequest,
) -> Result<warp::reply::Response, Rejection> {
    match facade.transactions.savepoint(&id, req.name.clone()).await {
        Ok(savepoint_id) => Ok(json_response(
            &SavepointResponse {
                savepoint_id,
                name: req.name,
            },
            warp::http::StatusCode::CREATED,
        )),
        Err(e) => Ok(json_response(
            &serde_json::json!({"error": e.to_string()}),
            warp::http::StatusCode::CONFLICT,
        )),
    }
}

async fn handle_rollback_to_savepoint(
    id: String,
    name: String,
    facade: Arc<Facade>,
) -> Result<warp::reply::Response, Rejection> {
    match facade.transactions.rollback_to_savepoint(&id, &name).await {
        Ok(()) => Ok(json_response(
            &serde_json::json!({"rolled_back_to": name}),
            warp::http::StatusCode::OK,
        )),
        Err(e) => Ok(json_response(
            &serde_json::json!({"error": e.to_string()}),
            warp::http::StatusCode::CONFLICT,
        )),
    }
}

async fn handle_get_trace(
    id: String,
    facade: Arc<Facade>,
) -> Result<warp::reply::Response, Rejection> {
    match facade.get_transaction(&id).await {
        Some(tx) => Ok(json_response(
            &serde_json::json!({
                "transaction_id": tx.id,
                "state": format!("{:?}", tx.state),
                "trace": tx.trace,
            }),
            warp::http::StatusCode::OK,
        )),
        None => Ok(empty_response(warp::http::StatusCode::NOT_FOUND)),
    }
}

async fn handle_create_sandbox(
    facade: Arc<Facade>,
    did: Option<String>,
    key: Option<String>,
    body: bytes::Bytes,
) -> Result<warp::reply::Response, Rejection> {
    let cache = facade.idempotency.clone();
    let rate = facade.did_rate_limiter.clone();
    let did_clone = did.clone();
    let body_bytes = body.to_vec();
    let body_for_handler = body_bytes.clone();
    let facade_for_handler = facade.clone();
    run_idempotent(
        cache,
        rate,
        did,
        "POST /api/sandboxes",
        key,
        &body_bytes,
        move || async move {
            let req: CreateSandboxRequest = if body_for_handler.is_empty() {
                CreateSandboxRequest::default()
            } else {
                serde_json::from_slice(&body_for_handler).unwrap_or_default()
            };
            let caller_auth = did_clone.clone();
            let owner = req
                .owner_did
                .or(did_clone)
                .unwrap_or_else(|| "did:spacekit:anonymous".to_string());
            let cfg = SandboxConfig {
                ttl_seconds: req
                    .ttl_seconds
                    .unwrap_or(SandboxConfig::default().ttl_seconds),
                max_bytes_written: req
                    .max_bytes_written
                    .unwrap_or(SandboxConfig::default().max_bytes_written),
                max_vector_ops: req
                    .max_vector_ops
                    .unwrap_or(SandboxConfig::default().max_vector_ops),
                max_fact_puts: req
                    .max_fact_puts
                    .unwrap_or(SandboxConfig::default().max_fact_puts),
            };
            let collaborators = req.collaborator_dids.unwrap_or_default();
            let caller = caller_auth.as_deref().unwrap_or(owner.as_str());
            match facade_for_handler
                .create_sandbox(
                    &owner,
                    caller,
                    cfg,
                    req.base_snapshot,
                    collaborators,
                    req.workspace_id,
                )
                .await
            {
                Ok(sb) => Ok(json_response(&sb, warp::http::StatusCode::CREATED)),
                Err(e) => {
                    let msg = e.to_string();
                    let status = if msg.starts_with("FORBIDDEN:") {
                        warp::http::StatusCode::FORBIDDEN
                    } else if msg.starts_with("NOTFOUND:") {
                        warp::http::StatusCode::NOT_FOUND
                    } else {
                        warp::http::StatusCode::INTERNAL_SERVER_ERROR
                    };
                    Ok(json_response(&serde_json::json!({"error": msg}), status))
                }
            }
        },
    )
    .await
}

async fn handle_get_sandbox(
    id: String,
    facade: Arc<Facade>,
    did: Option<String>,
) -> Result<warp::reply::Response, Rejection> {
    match facade.sandboxes.get(&id).await {
        Some(sb) => {
            if !caller_may_access_sandbox(did.as_deref(), &sb, SandboxAccess::Read) {
                return Ok(json_response(
                    &serde_json::json!({"error": "forbidden"}),
                    warp::http::StatusCode::FORBIDDEN,
                ));
            }
            Ok(json_response(&sb, warp::http::StatusCode::OK))
        }
        None => Ok(empty_response(warp::http::StatusCode::NOT_FOUND)),
    }
}

async fn handle_commit_sandbox(
    id: String,
    q: CommitSandboxQuery,
    facade: Arc<Facade>,
    did: Option<String>,
) -> Result<warp::reply::Response, Rejection> {
    let Some(sb) = facade.sandboxes.get(&id).await else {
        return Ok(empty_response(warp::http::StatusCode::NOT_FOUND));
    };
    if !caller_may_access_sandbox(did.as_deref(), &sb, SandboxAccess::OwnerWrite) {
        return Ok(json_response(
            &serde_json::json!({"error": "forbidden"}),
            warp::http::StatusCode::FORBIDDEN,
        ));
    }
    match facade
        .sandboxes
        .commit(&id, facade.transactions.clone(), q.dry_run)
        .await
    {
        Ok(report) => Ok(json_response(&report, warp::http::StatusCode::OK)),
        Err(e) => Ok(json_response(
            &serde_json::json!({"error": e.to_string()}),
            warp::http::StatusCode::CONFLICT,
        )),
    }
}

async fn handle_discard_sandbox(
    id: String,
    facade: Arc<Facade>,
    did: Option<String>,
) -> Result<warp::reply::Response, Rejection> {
    let Some(sb) = facade.sandboxes.get(&id).await else {
        return Ok(empty_response(warp::http::StatusCode::NOT_FOUND));
    };
    if !caller_may_access_sandbox(did.as_deref(), &sb, SandboxAccess::OwnerWrite) {
        return Ok(json_response(
            &serde_json::json!({"error": "forbidden"}),
            warp::http::StatusCode::FORBIDDEN,
        ));
    }
    match facade.sandboxes.discard(&id).await {
        Ok(()) => Ok(json_response(
            &serde_json::json!({"discarded": true, "id": id}),
            warp::http::StatusCode::OK,
        )),
        Err(e) => Ok(json_response(
            &serde_json::json!({"error": e.to_string()}),
            warp::http::StatusCode::CONFLICT,
        )),
    }
}

#[derive(Debug, Deserialize)]
pub struct ExtendBody {
    pub ttl_seconds: u64,
}

async fn handle_extend_sandbox(
    id: String,
    body: ExtendBody,
    facade: Arc<Facade>,
    did: Option<String>,
) -> Result<warp::reply::Response, Rejection> {
    let Some(sb) = facade.sandboxes.get(&id).await else {
        return Ok(empty_response(warp::http::StatusCode::NOT_FOUND));
    };
    if !caller_may_access_sandbox(did.as_deref(), &sb, SandboxAccess::ExtendTtl) {
        return Ok(json_response(
            &serde_json::json!({"error": "forbidden"}),
            warp::http::StatusCode::FORBIDDEN,
        ));
    }
    match facade.sandboxes.extend(&id, body.ttl_seconds).await {
        Ok(expires_at) => Ok(json_response(
            &serde_json::json!({"expires_at": expires_at}),
            warp::http::StatusCode::OK,
        )),
        Err(e) => Ok(json_response(
            &serde_json::json!({"error": e.to_string()}),
            warp::http::StatusCode::CONFLICT,
        )),
    }
}

async fn handle_get_journal(
    id: String,
    facade: Arc<Facade>,
    did: Option<String>,
) -> Result<warp::reply::Response, Rejection> {
    match facade.sandboxes.get(&id).await {
        Some(sb) => {
            if !caller_may_access_sandbox(did.as_deref(), &sb, SandboxAccess::Read) {
                return Ok(json_response(
                    &serde_json::json!({"error": "forbidden"}),
                    warp::http::StatusCode::FORBIDDEN,
                ));
            }
            Ok(json_response(
                &serde_json::json!({
                    "sandbox_id": sb.id,
                    "owner_did": sb.owner_did,
                    "collaborator_dids": sb.collaborator_dids,
                    "state": sb.state,
                    "quotas": sb.quotas,
                    "config": sb.config,
                    "journal": sb.journal,
                }),
                warp::http::StatusCode::OK,
            ))
        }
        None => Ok(empty_response(warp::http::StatusCode::NOT_FOUND)),
    }
}

#[derive(Debug, Deserialize)]
pub struct ChangesQuery {
    #[serde(default)]
    pub since_seq: Option<u64>,
    #[serde(default)]
    pub kind: Option<String>,
}

async fn handle_changes_sse(
    q: ChangesQuery,
    last_event_id: Option<String>,
    facade: Arc<Facade>,
) -> Result<warp::reply::Response, Rejection> {
    use futures::stream::StreamExt;
    let since = q
        .since_seq
        .or_else(|| last_event_id.as_ref().and_then(|s| s.parse::<u64>().ok()));
    let globs = q
        .kind
        .map(|k| {
            k.split(',')
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let rx = facade.change_feed.subscribe(globs, since, 64).await;
    let stream = tokio_stream::wrappers::ReceiverStream::new(rx).map(|ev| {
        let payload = serde_json::to_string(&ev).unwrap_or_else(|_| "{}".into());
        Ok::<_, Infallible>(
            warp::sse::Event::default()
                .id(ev.seq.to_string())
                .data(payload),
        )
    });
    let sse = warp::sse::reply(warp::sse::keep_alive().stream(stream));
    Ok(sse.into_response())
}

#[derive(Debug, Deserialize)]
pub struct CreateWorkspaceRequest {
    pub workspace_id: String,
    #[serde(default)]
    pub collaborators: Vec<crate::workspace::WorkspaceCollaborator>,
    #[serde(default)]
    pub associated_repos: Vec<String>,
    #[serde(default)]
    pub quotas: Option<crate::workspace::WorkspaceQuotas>,
    /// `public` (default) or `private`
    #[serde(default)]
    pub visibility: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateWorkspaceRequest {
    #[serde(default)]
    pub collaborators: Vec<crate::workspace::WorkspaceCollaborator>,
    #[serde(default)]
    pub associated_repos: Vec<String>,
    #[serde(default)]
    pub quotas: Option<crate::workspace::WorkspaceQuotas>,
    /// `public` (default) or `private`
    #[serde(default)]
    pub visibility: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ListWorkspacesQuery {
    pub owner_did: String,
}

#[derive(Debug, Deserialize)]
pub struct ImportWorkspaceRequest {
    pub bundle: crate::workspace::WorkspaceExportBundle,
    #[serde(default)]
    pub on_conflict: Option<String>,
    /// Destination owner on this node (defaults to bundle.owner_did).
    pub owner_did: Option<String>,
    /// Pull `referenced_blob_hashes` from a source storage node after import.
    pub replicate_blobs_from: Option<String>,
    /// Optional `Authorization` header value forwarded to the source node.
    pub replicate_source_authorization: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ReplicateBlobsRequest {
    pub source_url: String,
    pub hashes: Vec<String>,
    pub source_authorization: Option<String>,
}

async fn handle_mint_upload_token(
    facade: Arc<Facade>,
    did: Option<String>,
    req: crate::upload_token::MintUploadTokenRequest,
) -> Result<warp::reply::Response, Rejection> {
    let issuer = did.ok_or_else(|| warp::reject::reject())?;
    let secret = match facade
        .upload_signing_secret()
        .map(|s| s.to_vec())
        .or_else(|| crate::upload_token::load_signing_secret(facade.cas_data_dir()))
    {
        Some(s) => s,
        None => {
            let hint = facade
                .cas_data_dir()
                .map(|d| format!(
                    "set SPACEKIT_UPLOAD_TOKEN_SECRET before starting the node, or write {}/.upload_token_secret, then restart",
                    d.display()
                ))
                .unwrap_or_else(|| {
                    "set SPACEKIT_UPLOAD_TOKEN_SECRET before starting the node (export in the same shell as `spacekit network up`), then restart"
                        .to_string()
                });
            return Ok(json_response(
                &serde_json::json!({
                    "error": "upload token signing not configured",
                    "hint": hint,
                }),
                warp::http::StatusCode::SERVICE_UNAVAILABLE,
            ));
        }
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    match crate::upload_token::mint_upload_token(&secret, &issuer, &req, now) {
        Ok(resp) => Ok(json_response(&resp, warp::http::StatusCode::CREATED)),
        Err(e) => Ok(json_response(
            &serde_json::json!({"error": e.to_string()}),
            warp::http::StatusCode::BAD_REQUEST,
        )),
    }
}

async fn handle_create_workspace(
    facade: Arc<Facade>,
    did: Option<String>,
    req: CreateWorkspaceRequest,
) -> Result<warp::reply::Response, Rejection> {
    let owner = did.ok_or_else(|| warp::reject::reject())?;
    let now = chrono::Utc::now().timestamp() as u64;
    let workspace_id = req.workspace_id.clone();
    let content = crate::workspace::WorkspaceContent {
        workspace_id: workspace_id.clone(),
        owner_did: owner,
        collaborators: req.collaborators,
        associated_repos: req.associated_repos,
        quotas: req.quotas.unwrap_or_default(),
        default_access_policy: parse_workspace_visibility(req.visibility.as_deref()),
        status: crate::workspace::WorkspaceStatus::Active,
        created_at: now,
        updated_at: now,
    };
    match facade.create_workspace(content).await {
        Ok(fact_id) => Ok(json_response(
            &serde_json::json!({"fact_id": fact_id, "workspace_id": workspace_id}),
            warp::http::StatusCode::CREATED,
        )),
        Err(e) => Ok(json_response(
            &serde_json::json!({"error": e.to_string()}),
            warp::http::StatusCode::CONFLICT,
        )),
    }
}

async fn handle_update_workspace(
    workspace_id: String,
    facade: Arc<Facade>,
    did: Option<String>,
    req: UpdateWorkspaceRequest,
) -> Result<warp::reply::Response, Rejection> {
    let owner = did.ok_or_else(|| warp::reject::reject())?;
    let existing = match facade.get_workspace(&owner, &workspace_id).await {
        Ok(Some(ws)) => ws,
        Ok(None) => {
            return Ok(json_response(
                &serde_json::json!({"error": "workspace not found"}),
                warp::http::StatusCode::NOT_FOUND,
            ));
        }
        Err(e) => {
            return Ok(json_response(
                &serde_json::json!({"error": e.to_string()}),
                warp::http::StatusCode::CONFLICT,
            ));
        }
    };
    let now = chrono::Utc::now().timestamp() as u64;
    let quotas = req.quotas.unwrap_or(existing.quotas);
    let content = crate::workspace::WorkspaceContent {
        workspace_id: workspace_id.clone(),
        owner_did: owner,
        collaborators: if req.collaborators.is_empty() {
            existing.collaborators
        } else {
            req.collaborators
        },
        associated_repos: if req.associated_repos.is_empty() {
            existing.associated_repos
        } else {
            req.associated_repos
        },
        quotas,
        default_access_policy: req
            .visibility
            .as_deref()
            .map(|v| parse_workspace_visibility(Some(v)))
            .unwrap_or(existing.default_access_policy),
        status: existing.status,
        created_at: existing.created_at,
        updated_at: now,
    };
    match facade.update_workspace(content).await {
        Ok(fact_id) => Ok(json_response(
            &serde_json::json!({"fact_id": fact_id, "workspace_id": workspace_id}),
            warp::http::StatusCode::OK,
        )),
        Err(e) => Ok(json_response(
            &serde_json::json!({"error": e.to_string()}),
            warp::http::StatusCode::NOT_FOUND,
        )),
    }
}

fn parse_workspace_visibility(
    visibility: Option<&str>,
) -> spacekit_primitives::v1::fact::AccessPolicy {
    match visibility
        .unwrap_or("public")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "private" => {
            spacekit_primitives::v1::fact::AccessPolicy::Private(std::collections::HashSet::new())
        }
        _ => spacekit_primitives::v1::fact::AccessPolicy::Public,
    }
}

async fn handle_get_workspace(
    workspace_id: String,
    facade: Arc<Facade>,
    did: Option<String>,
) -> Result<warp::reply::Response, Rejection> {
    let owner = did.ok_or_else(|| warp::reject::reject())?;
    match facade.get_workspace(&owner, &workspace_id).await {
        Ok(Some(ws)) => Ok(json_response(&ws, warp::http::StatusCode::OK)),
        Ok(None) => Ok(empty_response(warp::http::StatusCode::NOT_FOUND)),
        Err(e) => Ok(json_response(
            &serde_json::json!({"error": e.to_string()}),
            warp::http::StatusCode::CONFLICT,
        )),
    }
}

async fn handle_list_workspaces(
    q: ListWorkspacesQuery,
    facade: Arc<Facade>,
) -> Result<warp::reply::Response, Rejection> {
    match facade.list_workspaces_for_owner(&q.owner_did).await {
        Ok(list) => Ok(json_response(
            &serde_json::json!({"workspaces": list}),
            warp::http::StatusCode::OK,
        )),
        Err(e) => Ok(json_response(
            &serde_json::json!({"error": e.to_string()}),
            warp::http::StatusCode::CONFLICT,
        )),
    }
}

fn parse_isolation(s: &str) -> Result<IsolationLevel, ()> {
    match s.to_ascii_lowercase().as_str() {
        "read_committed" | "rc" => Ok(IsolationLevel::ReadCommitted),
        "repeatable_read" | "rr" => Ok(IsolationLevel::RepeatableRead),
        "serializable" | "ser" => Ok(IsolationLevel::Serializable),
        _ => Err(()),
    }
}

fn json_response<T: Serialize>(value: &T, status: warp::http::StatusCode) -> warp::reply::Response {
    let body = serde_json::to_vec(value).unwrap_or_default();
    let mut resp = warp::reply::Response::new(body.into());
    *resp.status_mut() = status;
    resp.headers_mut().insert(
        warp::http::header::CONTENT_TYPE,
        warp::http::HeaderValue::from_static("application/json"),
    );
    resp
}

fn empty_response(status: warp::http::StatusCode) -> warp::reply::Response {
    let mut resp = warp::reply::Response::new(Vec::<u8>::new().into());
    *resp.status_mut() = status;
    resp
}

#[allow(dead_code)]
fn _serde_check(c: SandboxConfigSerde) -> SandboxConfigSerde {
    c
}
