//! Completion-only production API.

use axum::{
    body::Body,
    extract::{DefaultBodyLimit, State},
    http::{header, HeaderMap, HeaderValue, Method, Response, StatusCode},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Instant;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use crate::auth::{AuthError, AuthService};
use crate::config::{ProvidersConfig, SafetyConfig};
use crate::cost_tracker::{self, ReceiptContext, SharedCostTracker};
use crate::prices::SharedPrices;
use crate::providers::{self, CompletionRequest, ProviderConfigs, ProviderStream};
use crate::router;
use crate::storage_client::StorageClient;

#[derive(Clone)]
pub struct AppState {
    pub prices: SharedPrices,
    pub started_at: Instant,
    pub providers: ProvidersConfig,
    pub cost_tracker: SharedCostTracker,
    pub http_client: reqwest::Client,
    pub safety: SafetyConfig,
    pub storage: Option<StorageClient>,
    pub storage_required: bool,
    pub auth: Arc<AuthService>,
    pub stream_semaphore: Arc<tokio::sync::Semaphore>,
}

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub uptime_secs: u64,
}

#[derive(Serialize)]
pub struct ReadyResponse {
    pub status: &'static str,
    pub providers_configured: usize,
    pub prices_loaded: bool,
    pub storage_ok: bool,
    pub auth_configured: bool,
    pub routes_mode: &'static str,
}

#[derive(Serialize)]
pub struct ErrorResponse {
    pub error_code: &'static str,
    pub error_message: String,
}

#[derive(Deserialize)]
pub struct CompleteRequest {
    #[serde(default)]
    pub messages: Vec<Message>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub task: Option<String>,
    #[serde(alias = "task_hint")]
    #[serde(default)]
    pub task_hint: Option<String>,
}

#[derive(Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

type ApiError = (StatusCode, Json<ErrorResponse>);

pub fn router(state: AppState) -> Router {
    let cors = cors_layer(&state.safety.cors_origins);
    let body_limit = state.safety.max_body_bytes;

    Router::new()
        .route("/health", get(health))
        .route("/ready", get(ready))
        .route("/v1/complete", post(complete))
        .layer(DefaultBodyLimit::max(body_limit))
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        .with_state(state)
}

pub fn internal_router(state: AppState) -> Router {
    Router::new()
        .route("/internal/metrics", get(metrics))
        .with_state(state)
}

fn cors_layer(origins: &[String]) -> CorsLayer {
    let parsed: Vec<HeaderValue> = origins
        .iter()
        .filter_map(|origin| match origin.parse() {
            Ok(value) => Some(value),
            Err(error) => {
                tracing::warn!(origin, error = %error, "ignoring invalid CORS origin");
                None
            }
        })
        .collect();

    let layer = CorsLayer::new()
        .allow_methods([Method::GET, Method::POST])
        .allow_headers([
            header::AUTHORIZATION,
            header::CONTENT_TYPE,
            header::HeaderName::from_static("x-request-id"),
        ]);
    if parsed.is_empty() {
        layer
    } else {
        layer.allow_origin(parsed)
    }
}

async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        uptime_secs: state.started_at.elapsed().as_secs(),
    })
}

async fn ready(State(state): State<AppState>) -> (StatusCode, Json<ReadyResponse>) {
    let providers_configured = provider_count(&state.providers);
    let prices_loaded = !state.prices.read().await.is_empty();
    let storage_ok = match &state.storage {
        Some(storage) => storage.health().await.is_ok(),
        None => !state.storage_required,
    };
    let auth_configured = state.auth.is_configured();
    let is_ready = providers_configured > 0 && storage_ok && auth_configured;
    let status = if is_ready {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (
        status,
        Json(ReadyResponse {
            status: if is_ready { "ready" } else { "not_ready" },
            providers_configured,
            prices_loaded,
            storage_ok,
            auth_configured,
            routes_mode: "complete-only",
        }),
    )
}

async fn metrics(State(state): State<AppState>) -> Response<Body> {
    let tracker = state
        .cost_tracker
        .read()
        .unwrap_or_else(|error| error.into_inner());
    let body = format!(
        "# TYPE routekit_complete_requests_total counter\n\
         routekit_complete_requests_total {}\n\
         # TYPE routekit_input_tokens_total counter\n\
         routekit_input_tokens_total {}\n\
         # TYPE routekit_output_tokens_total counter\n\
         routekit_output_tokens_total {}\n\
         # TYPE routekit_cost_usd_total counter\n\
         routekit_cost_usd_total {}\n",
        tracker.request_count,
        tracker.total_input_tokens,
        tracker.total_output_tokens,
        tracker.total_cost_usd,
    );
    Response::builder()
        .header(header::CONTENT_TYPE, "text/plain; version=0.0.4")
        .body(Body::from(body))
        .expect("static metrics response is valid")
}

async fn complete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CompleteRequest>,
) -> Result<Response<Body>, ApiError> {
    let authorization = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok());
    let auth = state
        .auth
        .authenticate(authorization)
        .await
        .map_err(auth_error)?;

    let task_hint = body
        .task
        .as_deref()
        .or(body.task_hint.as_deref())
        .ok_or_else(|| bad_request("TASK_REQUIRED", "task or task_hint is required"))?;
    let task = router::TaskType::from_str(task_hint)
        .map_err(|_| bad_request("TASK_INVALID", "task hint is not supported"))?;
    validate_messages(&body.messages, &state.safety)?;

    let messages: Vec<providers::ChatMessage> = body
        .messages
        .into_iter()
        .map(|message| providers::ChatMessage {
            role: message.role,
            content: message.content,
        })
        .collect();

    let candidates = {
        let prices = state.prices.read().await;
        router::route_candidates(&state.providers, &prices, task, body.model.as_deref())
    };
    if candidates.is_empty() {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse {
                error_code: "NO_PROVIDER",
                error_message: "no configured provider can serve this request".to_string(),
            }),
        ));
    }

    let permit = state
        .stream_semaphore
        .clone()
        .try_acquire_owned()
        .map_err(|_| {
            (
                StatusCode::TOO_MANY_REQUESTS,
                Json(ErrorResponse {
                    error_code: "CONCURRENCY_LIMIT",
                    error_message: "too many active completion streams".to_string(),
                }),
            )
        })?;

    let provider_configs = ProviderConfigs::from(&state.providers);
    let request_id = uuid::Uuid::new_v4().simple().to_string();
    let max_attempts = state
        .safety
        .max_failover_attempts
        .max(1)
        .min(candidates.len());
    let mut last_error = None;

    for (attempt, decision) in candidates.into_iter().take(max_attempts).enumerate() {
        let request = CompletionRequest {
            model: decision.model.clone(),
            messages: messages.clone(),
            max_tokens: Some(state.safety.max_output_tokens),
        };

        match providers::stream_completion(
            &state.http_client,
            decision.provider,
            request,
            &provider_configs,
        )
        .await
        {
            Ok(stream) => {
                let (input_cost, output_cost) = state
                    .prices
                    .read()
                    .await
                    .get(&decision.model)
                    .map(|entry| (entry.input_cost_per_token, entry.output_cost_per_token))
                    .unwrap_or((0.0, 0.0));
                let receipt = state.storage.clone().map(|storage| ReceiptContext {
                    storage,
                    request_id: request_id.clone(),
                    key_id: auth.key_id.clone(),
                    owner_did: auth.owner_did.clone(),
                    provider: format!("{:?}", decision.provider),
                    model: decision.model.clone(),
                    task: format!("{:?}", decision.task),
                });
                let stream = cost_tracker::wrap_stream_usage(
                    stream,
                    input_cost,
                    output_cost,
                    state.cost_tracker.clone(),
                    receipt,
                );
                let stream = with_idle_timeout(
                    stream,
                    std::time::Duration::from_secs(state.safety.stream_idle_timeout_secs),
                );
                let stream = hold_permit(stream, permit);
                tracing::info!(
                    request_id,
                    key_id = auth.key_id,
                    provider = ?decision.provider,
                    model = decision.model,
                    task = ?decision.task,
                    failover_attempt = attempt,
                    "completion stream started"
                );

                return Response::builder()
                    .header(header::CONTENT_TYPE, "text/event-stream")
                    .header("cache-control", "no-cache")
                    .header("x-request-id", request_id)
                    .header("x-routekit-provider", format!("{:?}", decision.provider))
                    .header("x-routekit-model", &decision.model)
                    .header("x-routekit-task", format!("{:?}", decision.task))
                    .header("x-routekit-failover-attempt", attempt.to_string())
                    .body(Body::from_stream(stream))
                    .map_err(|error| {
                        (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(ErrorResponse {
                                error_code: "RESPONSE_BUILD_FAILED",
                                error_message: error.to_string(),
                            }),
                        )
                    });
            }
            Err(error) => {
                let retryable = error.is_retryable();
                tracing::warn!(
                    request_id,
                    provider = ?decision.provider,
                    attempt,
                    retryable,
                    error = %error,
                    "provider attempt failed"
                );
                last_error = Some(error.to_string());
                if !retryable {
                    break;
                }
            }
        }
    }

    Err((
        StatusCode::BAD_GATEWAY,
        Json(ErrorResponse {
            error_code: "PROVIDERS_UNAVAILABLE",
            error_message: last_error.unwrap_or_else(|| "all provider attempts failed".to_string()),
        }),
    ))
}

fn validate_messages(messages: &[Message], safety: &SafetyConfig) -> Result<(), ApiError> {
    if messages.is_empty() {
        return Err(bad_request(
            "MESSAGES_REQUIRED",
            "messages must be non-empty",
        ));
    }
    if messages.len() > safety.max_messages {
        return Err(bad_request(
            "TOO_MANY_MESSAGES",
            "message count exceeds the configured limit",
        ));
    }
    if messages
        .iter()
        .any(|message| message.content.len() > safety.max_message_bytes)
    {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(ErrorResponse {
                error_code: "MESSAGE_TOO_LARGE",
                error_message: "one or more messages exceed the configured limit".to_string(),
            }),
        ));
    }
    Ok(())
}

fn auth_error(error: AuthError) -> ApiError {
    let (status, code) = match error {
        AuthError::Missing => (StatusCode::UNAUTHORIZED, "AUTH_MISSING"),
        AuthError::Invalid | AuthError::Disabled => (StatusCode::UNAUTHORIZED, "AUTH_INVALID"),
        AuthError::RateLimited => (StatusCode::TOO_MANY_REQUESTS, "RATE_LIMITED"),
        AuthError::Unavailable => (StatusCode::SERVICE_UNAVAILABLE, "AUTH_UNAVAILABLE"),
    };
    (
        status,
        Json(ErrorResponse {
            error_code: code,
            error_message: error.to_string(),
        }),
    )
}

fn bad_request(code: &'static str, message: &str) -> ApiError {
    (
        StatusCode::BAD_REQUEST,
        Json(ErrorResponse {
            error_code: code,
            error_message: message.to_string(),
        }),
    )
}

fn provider_count(providers: &ProvidersConfig) -> usize {
    [
        providers.openai.is_some(),
        providers.anthropic.is_some(),
        providers.mistral.is_some(),
    ]
    .into_iter()
    .filter(|configured| *configured)
    .count()
}

fn hold_permit(
    stream: ProviderStream,
    permit: tokio::sync::OwnedSemaphorePermit,
) -> ProviderStream {
    struct PermitStream {
        inner: ProviderStream,
        _permit: tokio::sync::OwnedSemaphorePermit,
    }

    impl futures_util::Stream for PermitStream {
        type Item = Result<bytes::Bytes, anyhow::Error>;

        fn poll_next(
            mut self: std::pin::Pin<&mut Self>,
            context: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Option<Self::Item>> {
            std::pin::Pin::new(&mut self.inner).poll_next(context)
        }
    }

    Box::pin(PermitStream {
        inner: stream,
        _permit: permit,
    })
}

fn with_idle_timeout(stream: ProviderStream, timeout: std::time::Duration) -> ProviderStream {
    let timed = tokio_stream::StreamExt::timeout(stream, timeout);
    let mapped = futures_util::StreamExt::map(timed, |result| match result {
        Ok(item) => item,
        Err(_) => Err(anyhow::anyhow!("provider stream idle timeout")),
    });
    Box::pin(mapped)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use std::time::Duration;
    use tokio::sync::RwLock;
    use tower::ServiceExt;

    use crate::config::ProviderEntry;

    const TEST_KEY: &str = "sk-routekit-test-01234567890123456789";

    fn test_state() -> AppState {
        let auth = AuthService::new(None, &[TEST_KEY.to_string()], Duration::from_secs(60), 60);
        AppState {
            prices: Arc::new(RwLock::new(Default::default())),
            started_at: Instant::now(),
            providers: ProvidersConfig::default(),
            cost_tracker: crate::cost_tracker::CostTracker::shared(),
            http_client: reqwest::Client::new(),
            safety: SafetyConfig::default(),
            storage: None,
            storage_required: false,
            auth: Arc::new(auth),
            stream_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
        }
    }

    #[tokio::test]
    async fn complete_requires_authentication() {
        let response = router(test_state())
            .oneshot(
                Request::post("/v1/complete")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"task":"chat","messages":[{"role":"user","content":"hello"}]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn complete_requires_explicit_task() {
        let response = router(test_state())
            .oneshot(
                Request::post("/v1/complete")
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {TEST_KEY}"))
                    .body(Body::from(
                        r#"{"messages":[{"role":"user","content":"hello"}]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn financial_and_intent_routes_are_absent() {
        for path in [
            "/v1/intent",
            "/v1/charge",
            "/v1/charge-intent",
            "/v1/activity/0x0",
        ] {
            let response = router(test_state())
                .oneshot(Request::post(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
        }
    }

    #[tokio::test]
    async fn readiness_fails_without_provider() {
        let response = router(test_state())
            .oneshot(Request::get("/ready").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        assert!(std::str::from_utf8(&bytes).unwrap().contains("not_ready"));
    }

    #[tokio::test]
    async fn complete_enforces_body_limit() {
        let oversized = "x".repeat(300 * 1024);
        let body = serde_json::json!({
            "task": "chat",
            "messages": [{"role": "user", "content": oversized}]
        });
        let response = router(test_state())
            .oneshot(
                Request::post("/v1/complete")
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {TEST_KEY}"))
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn complete_fails_over_on_upstream_server_error() {
        let failing = spawn_provider(false).await;
        let healthy = spawn_provider(true).await;
        let mut state = test_state();
        state.providers.openai = Some(ProviderEntry {
            api_key: "test".to_string(),
            base_url: Some(failing),
            models: vec!["fast-primary".to_string()],
        });
        state.providers.mistral = Some(ProviderEntry {
            api_key: "test".to_string(),
            base_url: Some(healthy),
            models: vec!["fast-secondary".to_string()],
        });

        let response = router(state)
            .oneshot(
                Request::post("/v1/complete")
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {TEST_KEY}"))
                    .body(Body::from(
                        r#"{"task":"search","messages":[{"role":"user","content":"hello"}]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get("x-routekit-failover-attempt")
                .unwrap(),
            "1"
        );
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert!(std::str::from_utf8(&body).unwrap().contains("[DONE]"));
    }

    async fn spawn_provider(healthy: bool) -> String {
        let app = Router::new().route(
            "/chat/completions",
            post(move || async move {
                if healthy {
                    Response::builder()
                        .status(StatusCode::OK)
                        .header(header::CONTENT_TYPE, "text/event-stream")
                        .body(Body::from(
                            "data: {\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":1}}\n\ndata: [DONE]\n\n",
                        ))
                        .unwrap()
                } else {
                    Response::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .body(Body::from("upstream failed"))
                        .unwrap()
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        format!("http://{address}")
    }
}
