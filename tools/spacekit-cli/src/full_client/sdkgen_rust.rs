//! Rust SDK emitter (consumes the same `SpecModel` IR as Python / TypeScript).

use std::collections::HashSet;
use std::path::PathBuf;
use std::process::Command;

use super::{Auth, FormField, Loc, Method, PageKind, Pagination, SpecModel, TypeRef};

pub fn emit_rust(model: &SpecModel, crate_name: &str) -> Vec<(String, String)> {
    vec![
        ("Cargo.toml".to_string(), rs_cargo_toml(model, crate_name)),
        ("src/lib.rs".to_string(), rs_lib(model)),
        ("src/client.rs".to_string(), rs_client(model)),
        ("src/models.rs".to_string(), rs_models(model)),
        ("src/resources.rs".to_string(), rs_resources(model)),
    ]
}

pub fn rs_check(pkg_dir: &PathBuf) -> Result<bool, String> {
    match Command::new("cargo")
        .arg("check")
        .current_dir(pkg_dir)
        .output()
    {
        Ok(out) if out.status.success() => Ok(true),
        Ok(out) => Err(format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )),
        Err(_) => Ok(false),
    }
}

fn rs_cargo_toml(model: &SpecModel, crate_name: &str) -> String {
    format!(
        r#"[package]
name = "{crate_name}"
version = "{version}"
edition = "2021"
description = "{desc}"
license = "MIT"

[workspace]

[dependencies]
reqwest = {{ version = "0.12", default-features = false, features = ["json", "multipart", "rustls-tls", "stream"] }}
serde = {{ version = "1", features = ["derive"] }}
serde_json = "1"
tokio = {{ version = "1", features = ["rt-multi-thread", "macros", "time"] }}
futures-util = "0.3"
hmac = "0.12"
sha2 = "0.10"
base64 = "0.22"
thiserror = "1"
"#,
        crate_name = crate_name,
        version = model.version,
        desc = format!("{} SDK (generated)", model.title),
    )
}

fn rs_lib(model: &SpecModel) -> String {
    let mut out = format!(
        "//! {} v{} — generated SDK.\n\npub mod models;\npub mod resources;\npub mod client;\n\n",
        model.title, model.version
    );
    out.push_str("pub use client::{Client, ClientOptions, APIError};\n");
    out.push_str("pub use models::*;\n");
    if !model.resources.is_empty() || model.has_webhooks {
        out.push_str("pub use resources::*;\n");
    }
    out
}

const RS_CLIENT_TEMPLATE: &str = r#"//! Generated HTTP client.

use std::collections::HashMap;
use std::time::Duration;

use futures_util::StreamExt;
use reqwest::multipart;
use reqwest::{Client as HttpClient, Method, RequestBuilder};
use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio::time::sleep;

__RESOURCE_IMPORTS__
pub const DEFAULT_BASE_URL: &str = "__BASE_URL__";
const RETRY_STATUS: &[u16] = &[429, 500, 502, 503, 504];

#[derive(Debug, thiserror::Error)]
pub enum APIError {
    #[error("[{status}] {message}")]
    Http {
        status: u16,
        message: String,
        request_id: Option<String>,
        body: Option<Value>,
    },
    #[error("{0}")]
    Other(String),
}

impl From<reqwest::Error> for APIError {
    fn from(err: reqwest::Error) -> Self {
        APIError::Other(err.to_string())
    }
}

impl From<serde_json::Error> for APIError {
    fn from(err: serde_json::Error) -> Self {
        APIError::Other(err.to_string())
    }
}

#[derive(Clone, Debug)]
pub struct ClientOptions {
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub timeout: Option<Duration>,
    pub max_retries: u32,
    pub webhook_secret: Option<String>,
}

impl Default for ClientOptions {
    fn default() -> Self {
        Self {
            api_key: None,
            base_url: None,
            timeout: Some(Duration::from_secs(30)),
            max_retries: 2,
            webhook_secret: None,
        }
    }
}

/// Shared HTTP state (cloneable; held by `Client` and resource helpers).
#[derive(Clone)]
pub struct ClientCore {
    http: HttpClient,
    pub api_key: Option<String>,
    pub base_url: String,
    timeout: Duration,
    max_retries: u32,
    pub webhook_secret: Option<String>,
}

#[derive(Clone)]
pub struct Client {
    core: ClientCore,
__RESOURCE_FIELDS__
}

impl Client {
    pub fn new(options: ClientOptions) -> Result<Self, APIError> {
        let timeout = options.timeout.unwrap_or(Duration::from_secs(30));
        let http = HttpClient::builder()
            .timeout(timeout)
            .build()?;
        let base_url = options
            .base_url
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string())
            .trim_end_matches('/')
            .to_string();
        let core = ClientCore {
            http,
            api_key: options.api_key,
            base_url,
            timeout,
            max_retries: options.max_retries,
            webhook_secret: options.webhook_secret,
        };
        Ok(Self {
            core: core.clone(),
__RESOURCE_INIT__
        })
    }

    pub fn api_key(&self) -> Option<&str> {
        self.core.api_key.as_deref()
    }

    pub fn webhook_secret(&self) -> Option<&str> {
        self.core.webhook_secret.as_deref()
    }
}

impl ClientCore {
    fn apply_auth(&self, req: RequestBuilder) -> RequestBuilder {
        let mut req = req;
__AUTH_CODE__
        req
    }

    fn build_url(&self, path: &str, query: Option<&HashMap<String, String>>) -> String {
        let mut url = format!("{}{}", self.base_url, path);
        if let Some(q) = query {
            let pairs: Vec<_> = q
                .iter()
                .filter(|(_, v)| !v.is_empty())
                .map(|(k, v)| format!("{}={}", urlencoding_lite(k), urlencoding_lite(v)))
                .collect();
            if !pairs.is_empty() {
                url.push('?');
                url.push_str(&pairs.join("&"));
            }
        }
        url
    }

    pub async fn request<T: DeserializeOwned>(
        &self,
        method: &str,
        path: &str,
        query: Option<HashMap<String, String>>,
        body: Option<Value>,
    ) -> Result<T, APIError> {
        let url = self.build_url(path, query.as_ref());
        let mut attempt = 0u32;
        loop {
            let mut req = self.http.request(
                Method::from_bytes(method.as_bytes()).unwrap_or(Method::GET),
                &url,
            );
            req = req
                .header("Accept", "application/json")
                .header("Content-Type", "application/json");
            req = self.apply_auth(req);
            if let Some(ref payload) = body {
                req = req.json(payload);
            }
            match req.send().await {
                Ok(resp) => {
                    let status = resp.status().as_u16();
                    let request_id = resp
                        .headers()
                        .get("x-request-id")
                        .and_then(|v| v.to_str().ok())
                        .map(|s| s.to_string());
                    if !resp.status().is_success() {
                        if RETRY_STATUS.contains(&status) && attempt < self.max_retries {
                            sleep(backoff(attempt)).await;
                            attempt += 1;
                            continue;
                        }
                        let parsed: Option<Value> = resp.json().await.ok();
                        let message = parsed
                            .as_ref()
                            .and_then(|v| {
                                v.get("message")
                                    .or_else(|| v.get("error"))
                                    .and_then(|m| m.as_str())
                            })
                            .unwrap_or("request failed")
                            .to_string();
                        return Err(APIError::Http {
                            status,
                            message,
                            request_id,
                            body: parsed,
                        });
                    }
                    if status == 204 {
                        return Ok(serde_json::from_value(Value::Null).map_err(APIError::from)?);
                    }
                    let text = resp.text().await?;
                    if text.is_empty() {
                        return Ok(serde_json::from_value(Value::Null).map_err(APIError::from)?);
                    }
                    let value: Value = serde_json::from_str(&text)?;
                    return Ok(serde_json::from_value(value).map_err(APIError::from)?);
                }
                Err(_err) if attempt < self.max_retries => {
                    sleep(backoff(attempt)).await;
                    attempt += 1;
                }
                Err(err) => return Err(APIError::Other(err.to_string())),
            }
        }
    }

    pub async fn request_raw(
        &self,
        method: &str,
        path: &str,
        query: Option<HashMap<String, String>>,
        body: Option<Value>,
    ) -> Result<Value, APIError> {
        self.request(method, path, query, body).await
    }

    pub async fn stream(
        &self,
        method: &str,
        path: &str,
        query: Option<HashMap<String, String>>,
        body: Option<Value>,
    ) -> Result<impl futures_util::Stream<Item = Result<Value, APIError>>, APIError> {
        let url = self.build_url(path, query.as_ref());
        let mut req = self.http.request(
            Method::from_bytes(method.as_bytes()).unwrap_or(Method::GET),
            &url,
        );
        req = req.header("Accept", "text/event-stream");
        req = self.apply_auth(req);
        if let Some(payload) = body {
            req = req.json(&payload);
        }
        let resp = req.send().await?;
        if !resp.status().is_success() {
            return Err(APIError::Http {
                status: resp.status().as_u16(),
                message: resp.status().canonical_reason().unwrap_or("error").to_string(),
                request_id: resp
                    .headers()
                    .get("x-request-id")
                    .and_then(|v| v.to_str().ok())
                    .map(|s| s.to_string()),
                body: None,
            });
        }
        let stream = resp.bytes_stream();
        Ok(futures_util::stream::unfold(
            (stream, String::new()),
            |(mut stream, mut buffer)| async move {
                loop {
                    if let Some(pos) = buffer.find('\n') {
                        let line = buffer[..pos].trim_end().to_string();
                        buffer = buffer[pos + 1..].to_string();
                        if line.starts_with("data:") {
                            let data = line[5..].trim();
                            if data == "[DONE]" {
                                return None;
                            }
                            let parsed: Result<Value, APIError> =
                                serde_json::from_str(data).map_err(APIError::from);
                            return Some((parsed, (stream, buffer)));
                        }
                        continue;
                    }
                    match stream.next().await {
                        Some(Ok(chunk)) => {
                            buffer.push_str(&String::from_utf8_lossy(&chunk));
                        }
                        Some(Err(e)) => {
                            return Some((Err(APIError::Other(e.to_string())), (stream, buffer)))
                        }
                        None => return None,
                    }
                }
            },
        ))
    }

    pub async fn request_multipart<T: DeserializeOwned>(
        &self,
        method: &str,
        path: &str,
        query: Option<HashMap<String, String>>,
        form: multipart::Form,
    ) -> Result<T, APIError> {
        let url = self.build_url(path, query.as_ref());
        let mut req = self.http.request(
            Method::from_bytes(method.as_bytes()).unwrap_or(Method::POST),
            &url,
        );
        req = req.multipart(form);
        req = self.apply_auth(req);
        let resp = req.send().await?;
        let status = resp.status().as_u16();
        let request_id = resp
            .headers()
            .get("x-request-id")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        if !resp.status().is_success() {
            let parsed: Option<Value> = resp.json().await.ok();
            let message = parsed
                .as_ref()
                .and_then(|v| {
                    v.get("message")
                        .or_else(|| v.get("error"))
                        .and_then(|m| m.as_str())
                })
                .unwrap_or("request failed")
                .to_string();
            return Err(APIError::Http {
                status,
                message,
                request_id,
                body: parsed,
            });
        }
        let text = resp.text().await?;
        if text.is_empty() {
            return Ok(serde_json::from_value(Value::Null).map_err(APIError::from)?);
        }
        let value: Value = serde_json::from_str(&text)?;
        Ok(serde_json::from_value(value).map_err(APIError::from)?)
    }
}

fn backoff(attempt: u32) -> Duration {
    Duration::from_millis((500u64 * 2u64.pow(attempt)).min(8000))
}

fn urlencoding_lite(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
            _ => format!("%{:02X}", c as u8),
        })
        .collect()
}
"#;

fn rs_client(model: &SpecModel) -> String {
    let mut fields: Vec<String> = model
        .resources
        .iter()
        .map(|r| format!("    pub {}: {},", rs_snake(&r.attr), r.class))
        .collect();
    if model.has_webhooks {
        fields.push("    pub webhooks: Webhooks,".to_string());
    }
    let resource_fields = if fields.is_empty() {
        String::new()
    } else {
        format!("\n{}", fields.join("\n"))
    };

    let mut init: Vec<String> = model
        .resources
        .iter()
        .map(|r| {
            format!(
                "            {}: {}::new(core.clone()),",
                rs_snake(&r.attr),
                r.class
            )
        })
        .collect();
    if model.has_webhooks {
        init.push("            webhooks: Webhooks::new(core.clone()),".to_string());
    }
    let resource_init = if init.is_empty() {
        String::new()
    } else {
        format!("\n{}", init.join("\n"))
    };

    let mut import_types: Vec<String> = model.resources.iter().map(|r| r.class.clone()).collect();
    if model.has_webhooks {
        import_types.push("Webhooks".to_string());
    }
    let resource_imports = if import_types.is_empty() {
        String::new()
    } else {
        format!("use crate::resources::{{{}}};\n\n", import_types.join(", "))
    };

    let auth_code = match &model.auth {
        Auth::None => "        // no auth scheme declared in spec".to_string(),
        Auth::Bearer => {
            "        if let Some(ref key) = self.api_key {\n            req = req.header(\"Authorization\", format!(\"Bearer {}\", key));\n        }"
                .to_string()
        }
        Auth::ApiKeyHeader(name) => format!(
            "        if let Some(ref key) = self.api_key {{\n            req = req.header(\"{name}\", key);\n        }}"
        ),
    };

    RS_CLIENT_TEMPLATE
        .replace("__BASE_URL__", &model.base_url)
        .replace("__RESOURCE_IMPORTS__", &resource_imports)
        .replace("__RESOURCE_FIELDS__", &resource_fields)
        .replace("__RESOURCE_INIT__", &resource_init)
        .replace("__AUTH_CODE__", &auth_code)
}

fn rs_models(model: &SpecModel) -> String {
    let mut out = String::from("//! Generated models.\n\nuse serde::{Deserialize, Serialize};\n\n");
    if model.schemas.is_empty() && model.aliases.is_empty() {
        return out;
    }

    for (name, ty) in &model.aliases {
        out.push_str(&rs_emit_alias(name, ty));
        out.push('\n');
    }

    for s in &model.schemas {
        out.push_str("#[derive(Debug, Clone, Serialize, Deserialize)]\n");
        out.push_str(&format!("pub struct {} {{\n", s.name));
        if s.fields.is_empty() {
            out.push_str("}\n\n");
            continue;
        }
        for f in &s.fields {
            let rust_field = rs_snake(&f.name);
            let ty_str = rs_field_type(&f.ty, &s.name, f.required);
            if f.name != rust_field {
                out.push_str(&format!("    #[serde(rename = {:?})]\n", f.name));
            }
            if !f.required {
                out.push_str("    #[serde(default)]\n");
                out.push_str("    #[serde(skip_serializing_if = \"Option::is_none\")]\n");
            }
            out.push_str(&format!("    pub {}: {},\n", rust_field, ty_str));
        }
        out.push_str("}\n\n");
    }
    out
}

fn rs_emit_alias(name: &str, ty: &TypeRef) -> String {
    match ty {
        TypeRef::Enum(vals) => {
            let mut out = format!(
                "#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]\n\
                 pub enum {name} {{\n"
            );
            for v in vals {
                let variant = rs_enum_variant(v);
                out.push_str(&format!("    #[serde(rename = {v:?})]\n    {variant},\n"));
            }
            out.push_str("}\n");
            out
        }
        TypeRef::Union(items) => {
            let refs: Vec<String> = items
                .iter()
                .filter_map(|t| match t {
                    TypeRef::Ref(n) => Some(n.clone()),
                    _ => None,
                })
                .collect();
            if refs.len() == items.len() && refs.len() >= 2 {
                let mut out = format!(
                    "#[derive(Debug, Clone, Serialize, Deserialize)]\n\
                     #[serde(untagged)]\n\
                     pub enum {name} {{\n"
                );
                for r in &refs {
                    out.push_str(&format!("    {r}({r}),\n"));
                }
                out.push_str("}\n");
                out
            } else {
                format!("pub type {name} = serde_json::Value;\n")
            }
        }
        other => format!("pub type {name} = {};\n", rs_type(other)),
    }
}

fn rs_field_type(ty: &TypeRef, struct_name: &str, required: bool) -> String {
    let base = rs_type_inner(ty, struct_name);
    if required || base.starts_with("Option<") {
        base
    } else {
        format!("Option<{}>", base)
    }
}

fn rs_type_names(model: &SpecModel) -> HashSet<String> {
    let mut names: HashSet<String> = model.schemas.iter().map(|s| s.name.clone()).collect();
    for (name, _) in &model.aliases {
        names.insert(name.clone());
    }
    names
}

fn rs_type_inner(ty: &TypeRef, struct_name: &str) -> String {
    match ty {
        TypeRef::Str => "String".to_string(),
        TypeRef::Int => "i64".to_string(),
        TypeRef::Float => "f64".to_string(),
        TypeRef::Bool => "bool".to_string(),
        TypeRef::Any => "serde_json::Value".to_string(),
        TypeRef::Null => "()".to_string(),
        TypeRef::Ref(n) if n == struct_name => format!("Box<{}>", n),
        TypeRef::Ref(n) => n.clone(),
        TypeRef::Array(inner) => format!("Vec<{}>", rs_type_inner(inner, struct_name)),
        TypeRef::Map(inner) => format!(
            "std::collections::HashMap<String, {}>",
            rs_type_inner(inner, struct_name)
        ),
        TypeRef::Enum(_) => "String".to_string(), // inline enum on field — use alias when possible
        TypeRef::Union(items) => {
            if items.iter().any(|t| matches!(t, TypeRef::Null)) {
                let other = items.iter().find(|t| !matches!(t, TypeRef::Null)).unwrap();
                return format!("Option<{}>", rs_type_inner(other, struct_name));
            }
            "serde_json::Value".to_string()
        }
    }
}

fn rs_type(ty: &TypeRef) -> String {
    rs_type_inner(ty, "")
}

fn rs_resources(model: &SpecModel) -> String {
    let mut out = String::from("//! Generated resources.\n\n");
    out.push_str("use std::collections::HashMap;\n\n");
    out.push_str("use serde::Serialize;\n");
    out.push_str("use serde_json::Value;\n\n");
    out.push_str("use crate::client::{APIError, ClientCore};\n");
    if !model.schemas.is_empty() || !model.aliases.is_empty() {
        out.push_str("use crate::models::*;\n");
    }
    out.push_str(
        "\nfn query_param_string<T: Serialize>(v: &T) -> String {\n\
            match serde_json::to_value(v).unwrap_or(Value::Null) {\n\
             Value::String(s) => s,\n\
             Value::Number(n) => n.to_string(),\n\
             Value::Bool(b) => b.to_string(),\n\
             other => other.to_string(),\n\
         }\n\
         }\n\n",
    );

    let type_names = rs_type_names(model);

    for r in &model.resources {
        out.push_str(&format!("#[derive(Clone)]\npub struct {} {{\n", r.class));
        out.push_str("    core: ClientCore,\n");
        out.push_str("}\n\n");
        out.push_str(&format!("impl {} {{\n", r.class));
        out.push_str("    pub(crate) fn new(core: ClientCore) -> Self {\n");
        out.push_str("        Self { core }\n");
        out.push_str("    }\n\n");
        for m in &r.methods {
            out.push_str(&rs_method(m, &type_names));
            out.push('\n');
        }
        out.push_str("}\n\n");
    }

    if model.has_webhooks {
        out.push_str(RS_WEBHOOKS);
    }
    out
}

// WEBHOOKS emitted when spec declares webhooks.
const RS_WEBHOOKS: &str = r##"#[derive(Clone)]
pub struct Webhooks {
    core: ClientCore,
}

impl Webhooks {
    pub(crate) fn new(core: ClientCore) -> Self {
        Self { core }
    }

    /// Verify (Standard Webhooks HMAC-SHA256) and parse an incoming webhook.
    pub fn unwrap(
        &self,
        payload: &str,
        headers: &HashMap<String, String>,
        secret: Option<&str>,
    ) -> Result<Value, APIError> {
        let key = secret
            .map(|s| s.to_string())
            .or_else(|| self.core.webhook_secret.clone());
        if let Some(ref k) = key {
            Self::verify(payload, headers, k)?;
        }
        serde_json::from_str(payload).map_err(APIError::from)
    }

    pub fn verify(
        payload: &str,
        headers: &HashMap<String, String>,
        secret: &str,
    ) -> Result<(), APIError> {
        fn header(headers: &HashMap<String, String>, name: &str) -> Option<String> {
            headers
                .get(name)
                .or_else(|| headers.get(&name.to_lowercase()))
                .cloned()
        }
        let msg_id = header(headers, "webhook-id").ok_or_else(|| {
            APIError::Other("missing webhook signature headers".into())
        })?;
        let timestamp = header(headers, "webhook-timestamp").ok_or_else(|| {
            APIError::Other("missing webhook signature headers".into())
        })?;
        let signature = header(headers, "webhook-signature").ok_or_else(|| {
            APIError::Other("missing webhook signature headers".into())
        })?;
        let signed = format!("{}.{}.{}", msg_id, timestamp, payload);
        let key_bytes = if secret.starts_with("whsec_") {
            base64::Engine::decode(
                &base64::engine::general_purpose::STANDARD,
                &secret[6..],
            )
            .map_err(|e| APIError::Other(e.to_string()))?
        } else {
            secret.as_bytes().to_vec()
        };
        let mut mac = hmac::Hmac::<sha2::Sha256>::new_from_slice(&key_bytes)
            .map_err(|e| APIError::Other(e.to_string()))?;
        use hmac::Mac;
        mac.update(signed.as_bytes());
        let digest = mac.finalize().into_bytes();
        let expected = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            digest,
        );
        let provided: Vec<&str> = signature
            .split_whitespace()
            .map(|p| p.split_once(',').map(|(_, s)| s).unwrap_or(p))
            .collect();
        if !provided.iter().any(|p| *p == expected) {
            return Err(APIError::Other("webhook signature mismatch".into()));
        }
        Ok(())
    }
}
"##;

fn rs_method(m: &Method, type_names: &HashSet<String>) -> String {
    if m.streaming {
        return rs_streaming_method(m);
    }
    if let Some(fields) = &m.multipart {
        return rs_multipart_method(m, fields, type_names);
    }
    if let Some(pg) = &m.pagination {
        return rs_paginated_method(m, pg);
    }
    rs_regular_method(m, type_names)
}

fn rs_regular_method(m: &Method, type_names: &HashSet<String>) -> String {
    let (params, query_build) = rs_params(m, false, None);
    let ret = if m.response.is_none() {
        "()".to_string()
    } else {
        rs_return_type(&m.response, type_names)
    };
    let body_param = m.body.as_ref().map(|(ty, req)| {
        let t = rs_type(ty);
        if *req {
            format!("body: {}", t)
        } else {
            format!("body: Option<{}>", t)
        }
    });

    let mut sig = params;
    if let Some(b) = body_param {
        sig.push(b);
    }

    let mut out = String::new();
    if let Some(s) = &m.summary {
        out.push_str("    /// ");
        out.push_str(s);
        out.push_str("\n");
    }
    out.push_str(&format!(
        "    pub async fn {}(&self, {}) -> Result<{}, APIError> {{\n",
        rs_snake(&m.name),
        sig.join(", "),
        ret
    ));
    out.push_str(&format!("        let path = {};\n", rs_path_expr(&m.path)));
    out.push_str(&query_build);
    let body_arg = if m.body.is_some() {
        "Some(serde_json::to_value(body).map_err(APIError::from)?)"
    } else {
        "None"
    };
    if m.response.is_none() {
        out.push_str(&format!(
            "        self.core.request_raw(\"{}\", &path, query, ",
            m.http
        ));
        out.push_str(body_arg);
        out.push_str(").await?;\n        Ok(())\n");
    } else {
        out.push_str(&format!(
            "        let resp: Value = self.core.request(\"{}\", &path, query, ",
            m.http
        ));
        out.push_str(body_arg);
        out.push_str(").await?;\n");
        out.push_str(&format!(
            "        {}\n",
            rs_wrap_response("resp", &m.response, type_names)
        ));
    }
    out.push_str("    }\n");
    out
}

fn rs_paginated_method(m: &Method, pg: &Pagination) -> String {
    let (params, _) = rs_params(m, true, Some(pg));
    let mut out = String::new();
    let doc = m
        .summary
        .clone()
        .unwrap_or_else(|| "Auto-paginated list.".to_string());
    out.push_str(&format!("    /// {} (collects all pages)\n", doc));
    out.push_str(&format!(
        "    pub async fn {}(&self, {}) -> Result<Vec<Value>, APIError> {{\n",
        rs_snake(&m.name),
        params.join(", ")
    ));
    out.push_str(&format!("        let path = {};\n", rs_path_expr(&m.path)));
    out.push_str("        let mut base_query: HashMap<String, String> = HashMap::new();\n");
    for p in &m.params {
        if p.loc != Loc::Query {
            continue;
        }
        if Some(&p.name) == pg.cursor_param.as_ref() || Some(&p.name) == pg.offset_param.as_ref() {
            continue;
        }
        let local = rs_snake(&p.name);
        out.push_str(&format!(
            "        if let Some(ref v) = {local} {{ base_query.insert({name:?}.to_string(), query_param_string(v)); }}\n",
            name = p.name,
        ));
    }
    out.push_str("        let mut all: Vec<Value> = Vec::new();\n");
    let data = &pg.data_field;
    match pg.kind {
        PageKind::Cursor => {
            let cursor_param = pg
                .cursor_param
                .clone()
                .unwrap_or_else(|| "cursor".to_string());
            let next = pg
                .next_field
                .clone()
                .unwrap_or_else(|| "next_cursor".to_string());
            out.push_str("        let mut cursor: Option<String> = None;\n");
            out.push_str("        loop {\n");
            out.push_str("            let mut query = base_query.clone();\n");
            out.push_str(&format!(
                "            if let Some(ref c) = cursor {{ query.insert({cursor_param:?}.to_string(), c.clone()); }}\n"
            ));
            out.push_str(&format!(
                "            let page: Value = self.core.request(\"{}\", &path, Some(query), None).await?;\n",
                m.http
            ));
            out.push_str(&format!(
                "            if let Some(items) = page.get({data:?}).and_then(|v| v.as_array()) {{\n"
            ));
            out.push_str("                all.extend(items.iter().cloned());\n");
            out.push_str("            }\n");
            out.push_str(&format!(
                "            cursor = page.get({next:?}).and_then(|v| v.as_str()).map(|s| s.to_string());\n"
            ));
            out.push_str("            if cursor.is_none() { break; }\n");
            out.push_str("        }\n");
        }
        PageKind::Offset => {
            let offset_param = pg
                .offset_param
                .clone()
                .unwrap_or_else(|| "offset".to_string());
            out.push_str("        let mut offset: i64 = 0;\n");
            out.push_str("        loop {\n");
            out.push_str("            let mut query = base_query.clone();\n");
            out.push_str(&format!(
                "            query.insert({offset_param:?}.to_string(), offset.to_string());\n"
            ));
            out.push_str(&format!(
                "            let page: Value = self.core.request(\"{}\", &path, Some(query), None).await?;\n",
                m.http
            ));
            out.push_str(&format!(
                "            let items = page.get({data:?}).and_then(|v| v.as_array()).cloned().unwrap_or_default();\n"
            ));
            out.push_str("            if items.is_empty() { break; }\n");
            out.push_str("            let n = items.len();\n");
            out.push_str("            all.extend(items);\n");
            out.push_str("            offset += n as i64;\n");
            out.push_str("        }\n");
        }
    }
    out.push_str("        Ok(all)\n");
    out.push_str("    }\n");
    out
}

fn rs_multipart_method(m: &Method, fields: &[FormField], type_names: &HashSet<String>) -> String {
    let (mut params, query_build) = rs_params(m, false, None);
    for f in fields {
        let local = rs_snake(&f.name);
        let ty = if f.is_file {
            "Vec<u8>".to_string()
        } else {
            rs_type(&f.ty)
        };
        if f.required {
            params.push(format!("{}: {}", local, ty));
        } else {
            params.push(format!("{}: Option<{}>", local, ty));
        }
    }
    let ret = rs_return_type(&m.response, type_names);
    let mut out = String::new();
    if let Some(s) = &m.summary {
        out.push_str("    /// ");
        out.push_str(s);
        out.push_str("\n");
    }
    out.push_str(&format!(
        "    pub async fn {}(&self, {}) -> Result<{}, APIError> {{\n",
        rs_snake(&m.name),
        params.join(", "),
        ret
    ));
    out.push_str(&format!("        let path = {};\n", rs_path_expr(&m.path)));
    out.push_str(&query_build);
    out.push_str("        let mut form = reqwest::multipart::Form::new();\n");
    for f in fields {
        let local = rs_snake(&f.name);
        if f.is_file {
            if f.required {
                out.push_str(&format!(
                    "        form = form.part({name:?}, reqwest::multipart::Part::bytes({local}).file_name({name:?}));\n",
                    name = f.name,
                ));
            } else {
                out.push_str(&format!(
                    "        if let Some(ref data) = {local} {{\n            form = form.part({name:?}, reqwest::multipart::Part::bytes(data.clone()).file_name({name:?}));\n        }}\n",
                    name = f.name,
                ));
            }
        } else if f.required {
            out.push_str(&format!(
                "        form = form.text({name:?}, {local}.to_string());\n",
                name = f.name,
            ));
        } else {
            out.push_str(&format!(
                "        if let Some(ref v) = {local} {{ form = form.text({name:?}, v.to_string()); }}\n",
                name = f.name,
            ));
        }
    }
    out.push_str(&format!(
        "        let resp: Value = self.core.request_multipart(\"{}\", &path, query, form).await?;\n",
        m.http
    ));
    out.push_str(&format!(
        "        {}\n",
        rs_wrap_response("resp", &m.response, type_names)
    ));
    out.push_str("    }\n");
    out
}

fn rs_streaming_method(m: &Method) -> String {
    let (params, query_build) = rs_params(m, false, None);
    let mut sig = params;
    if let Some((ty, req)) = &m.body {
        let t = rs_type(ty);
        sig.push(if *req {
            format!("body: {}", t)
        } else {
            format!("body: Option<{}>", t)
        });
    }
    let mut out = String::new();
    let doc = m
        .summary
        .clone()
        .unwrap_or_else(|| "Stream server-sent events.".to_string());
    out.push_str(&format!("    /// {} (server-sent events)\n", doc));
    out.push_str(&format!(
        "    pub async fn {}(&self, {}) -> Result<impl futures_util::Stream<Item = Result<Value, APIError>>, APIError> {{\n",
        rs_snake(&m.name),
        sig.join(", ")
    ));
    out.push_str(&format!("        let path = {};\n", rs_path_expr(&m.path)));
    out.push_str(&query_build);
    let body_arg = if m.body.is_some() {
        "Some(serde_json::to_value(body).map_err(APIError::from)?)"
    } else {
        "None"
    };
    out.push_str(&format!(
        "        self.core.stream(\"{}\", &path, query, ",
        m.http
    ));
    out.push_str(body_arg);
    out.push_str(").await\n");
    out.push_str("    }\n");
    out
}

fn rs_params(m: &Method, paginated: bool, pg: Option<&Pagination>) -> (Vec<String>, String) {
    let mut params = Vec::new();
    for p in &m.params {
        if p.loc == Loc::Header {
            continue;
        }
        if paginated {
            if p.loc == Loc::Query {
                if let Some(pg) = pg {
                    if Some(&p.name) == pg.cursor_param.as_ref()
                        || Some(&p.name) == pg.offset_param.as_ref()
                    {
                        continue;
                    }
                }
                let local = rs_snake(&p.name);
                params.push(format!("{}: Option<{}>", local, rs_type(&p.ty)));
                continue;
            }
        }
        let local = rs_snake(&p.name);
        let ty = rs_type(&p.ty);
        if p.required {
            params.push(format!("{}: {}", local, ty));
        } else {
            params.push(format!("{}: Option<{}>", local, ty));
        }
    }

    let query_lines: Vec<String> = m
        .params
        .iter()
        .filter(|p| p.loc == Loc::Query)
        .filter(|p| {
            if paginated {
                if let Some(pg) = pg {
                    return Some(&p.name) != pg.cursor_param.as_ref()
                        && Some(&p.name) != pg.offset_param.as_ref();
                }
            }
            true
        })
        .map(|p| {
            let local = rs_snake(&p.name);
            format!(
                "        if let Some(ref v) = {local} {{ query.insert({name:?}.to_string(), query_param_string(v)); }}",
                name = p.name,
            )
        })
        .collect();

    let query_build = if query_lines.is_empty() && !paginated {
        "        let query: Option<HashMap<String, String>> = None;\n".to_string()
    } else if query_lines.is_empty() {
        "        let query: Option<HashMap<String, String>> = None;\n".to_string()
    } else {
        format!(
            "        let mut query: HashMap<String, String> = HashMap::new();\n{}\n        let query = Some(query);\n",
            query_lines.join("\n")
        )
    };

    (params, query_build)
}

fn rs_return_type(ty: &Option<TypeRef>, type_names: &HashSet<String>) -> String {
    match ty {
        Some(TypeRef::Ref(n)) if type_names.contains(n) => n.clone(),
        Some(TypeRef::Array(inner)) => match inner.as_ref() {
            TypeRef::Ref(n) if type_names.contains(n) => format!("Vec<{}>", n),
            inner => format!("Vec<{}>", rs_type(inner)),
        },
        Some(t) => rs_type(t),
        None => "Value".to_string(),
    }
}

fn rs_wrap_response(var: &str, ty: &Option<TypeRef>, type_names: &HashSet<String>) -> String {
    match ty {
        Some(TypeRef::Ref(n)) if type_names.contains(n) => {
            format!("Ok(serde_json::from_value({var}).map_err(APIError::from)?)")
        }
        Some(TypeRef::Array(inner)) => match inner.as_ref() {
            TypeRef::Ref(n) if type_names.contains(n) => {
                format!("Ok(serde_json::from_value({var}).map_err(APIError::from)?)")
            }
            _ => format!("Ok({var})"),
        },
        _ => format!("Ok({var})"),
    }
}

fn rs_path_expr(path: &str) -> String {
    if !path.contains('{') {
        return format!("\"{path}\".to_string()");
    }
    let mut fmt = String::new();
    let mut args = Vec::new();
    let mut chars = path.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '{' {
            let mut name = String::new();
            for nc in chars.by_ref() {
                if nc == '}' {
                    break;
                }
                name.push(nc);
            }
            fmt.push_str("{}");
            args.push(rs_snake(&name));
        } else {
            fmt.push(c);
        }
    }
    if args.is_empty() {
        format!("\"{path}\".to_string()")
    } else {
        format!("format!(\"{fmt}\", {})", args.join(", "))
    }
}

fn rs_enum_variant(value: &str) -> String {
    let mut out = String::new();
    let mut upper = true;
    for c in value.chars() {
        if c.is_ascii_alphanumeric() {
            if upper {
                out.push(c.to_ascii_uppercase());
                upper = false;
            } else {
                out.push(c);
            }
        } else {
            upper = true;
        }
    }
    if out.is_empty()
        || out
            .chars()
            .next()
            .map(|c| c.is_ascii_digit())
            .unwrap_or(false)
    {
        format!("V{out}")
    } else if RS_KEYWORDS.contains(&out.as_str()) {
        format!("{out}Variant")
    } else {
        out
    }
}

fn rs_snake(s: &str) -> String {
    let mut out = String::new();
    let mut prev_lower = false;
    for c in s.chars() {
        if c.is_ascii_uppercase() {
            if prev_lower {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
            prev_lower = false;
        } else if c.is_ascii_alphanumeric() {
            out.push(c);
            prev_lower = c.is_ascii_lowercase() || c.is_ascii_digit();
        } else if !out.ends_with('_') {
            out.push('_');
            prev_lower = false;
        }
    }
    let out = out.trim_matches('_').to_string();
    let out = if out.is_empty() {
        "field".to_string()
    } else {
        out
    };
    if RS_KEYWORDS.contains(&out.as_str()) {
        format!("{out}_")
    } else {
        out
    }
}

const RS_KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern",
    "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
    "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true", "type",
    "unsafe", "use", "where", "while",
];
