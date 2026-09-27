//! Authentication endpoints (see `crate::request_auth`).
//!
//! ```text
//! POST /api/auth/challenge      { did }                              -> { challenge, message, expires_at }
//! POST /api/auth/session        { did, challenge, public_key_hex,
//!                                 signature_hex, algorithm? }       -> { token, did, expires_at }
//! POST /api/auth/delegate       Authorization: Bearer <session>
//!                               { app_id, act_as?, ttl_seconds? }   -> { token, did, expires_at, scope }
//! POST /api/auth/service-token  X-Storage-Secret: <secret>
//!                               { did, app_id?, act_as?, ttl_seconds? } -> { token, ... }
//! GET  /api/auth/whoami         Authorization: Bearer <token>       -> { did, scope, mode }
//! ```
//!
//! The login message a client signs is exactly
//! `"SpaceKit storage login\nDID: {did}\nChallenge: {challenge}"`.

use crate::request_auth::{self, DelegateRequest, ServiceTokenRequest, SessionLoginRequest};
use serde::Deserialize;
use warp::http::StatusCode;
use warp::reply::Response;
use warp::{Filter, Reply};

// SLH-DSA-SHA2-192s signatures are 16 KiB (32 KiB as hex).
const MAX_AUTH_BODY_BYTES: u64 = 64 * 1024;

#[derive(Debug, Deserialize)]
struct ChallengeRequest {
    did: String,
}

fn json(status: StatusCode, value: serde_json::Value) -> Response {
    warp::reply::with_status(warp::reply::json(&value), status).into_response()
}

fn error(status: StatusCode, message: impl std::fmt::Display) -> Response {
    json(status, serde_json::json!({ "error": message.to_string() }))
}

fn bearer_token(header: Option<&str>) -> Option<&str> {
    let h = header?.trim();
    let t = h.strip_prefix("Bearer ").or_else(|| h.strip_prefix("SpaceKit "))?.trim();
    request_auth::is_token(t).then_some(t)
}

pub fn routes() -> warp::filters::BoxedFilter<(Response,)> {
    let challenge = warp::path!("api" / "auth" / "challenge")
        .and(warp::post())
        .and(warp::body::content_length_limit(MAX_AUTH_BODY_BYTES))
        .and(warp::body::json())
        .map(|req: ChallengeRequest| {
            let cfg = request_auth::config();
            match request_auth::mint_challenge(cfg, req.did.trim(), request_auth::unix_now()) {
                Ok((challenge, expires_at)) => json(
                    StatusCode::OK,
                    serde_json::json!({
                        "challenge": challenge,
                        "message": request_auth::login_message(req.did.trim(), &challenge),
                        "expires_at": expires_at,
                    }),
                ),
                Err(e) => error(StatusCode::BAD_REQUEST, e),
            }
        });

    let session = warp::path!("api" / "auth" / "session")
        .and(warp::post())
        .and(warp::body::content_length_limit(MAX_AUTH_BODY_BYTES))
        .and(warp::body::json())
        .map(|req: SessionLoginRequest| {
            match request_auth::login(request_auth::config(), &req, request_auth::unix_now()) {
                Ok(issued) => json(StatusCode::OK, serde_json::to_value(issued).unwrap_or_default()),
                Err(e) => error(StatusCode::UNAUTHORIZED, e),
            }
        });

    let delegate = warp::path!("api" / "auth" / "delegate")
        .and(warp::post())
        .and(warp::header::optional::<String>("authorization"))
        .and(warp::body::content_length_limit(MAX_AUTH_BODY_BYTES))
        .and(warp::body::json())
        .map(|auth: Option<String>, req: DelegateRequest| {
            let cfg = request_auth::config();
            let now = request_auth::unix_now();
            let Some(token) = bearer_token(auth.as_deref()) else {
                return error(StatusCode::UNAUTHORIZED, "a session token is required");
            };
            let parent = match request_auth::verify_token(cfg, token, now) {
                Ok(c) => c,
                Err(e) => return error(StatusCode::UNAUTHORIZED, e),
            };
            match request_auth::delegate(cfg, &parent, &req, now) {
                Ok(issued) => json(StatusCode::OK, serde_json::to_value(issued).unwrap_or_default()),
                Err(e) => error(StatusCode::FORBIDDEN, e),
            }
        });

    let service_token = warp::path!("api" / "auth" / "service-token")
        .and(warp::post())
        .and(warp::header::optional::<String>("x-storage-secret"))
        .and(warp::body::content_length_limit(MAX_AUTH_BODY_BYTES))
        .and(warp::body::json())
        .map(|secret: Option<String>, req: ServiceTokenRequest| {
            let cfg = request_auth::config();
            if !cfg.service_secret_matches(secret.as_deref()) {
                return error(StatusCode::FORBIDDEN, "valid X-Storage-Secret required");
            }
            match request_auth::service_token(cfg, &req, request_auth::unix_now()) {
                Ok(issued) => json(StatusCode::OK, serde_json::to_value(issued).unwrap_or_default()),
                Err(e) => error(StatusCode::BAD_REQUEST, e),
            }
        });

    let whoami = warp::path!("api" / "auth" / "whoami")
        .and(warp::get())
        .and(warp::header::optional::<String>("authorization"))
        .map(|auth: Option<String>| {
            let cfg = request_auth::config();
            let Some(token) = bearer_token(auth.as_deref()) else {
                return error(StatusCode::UNAUTHORIZED, "a token is required");
            };
            match request_auth::verify_token(cfg, token, request_auth::unix_now()) {
                Ok(c) => json(
                    StatusCode::OK,
                    serde_json::json!({
                        "did": c.sub,
                        "scope": c.scope,
                        "expires_at": c.exp,
                        "mode": cfg.mode.as_str(),
                    }),
                ),
                Err(e) => error(StatusCode::UNAUTHORIZED, e),
            }
        });

    challenge
        .or(session)
        .unify()
        .or(delegate)
        .unify()
        .or(service_token)
        .unify()
        .or(whoami)
        .unify()
        .boxed()
}
