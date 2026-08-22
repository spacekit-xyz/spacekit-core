# Web Search Tool for SpaceKit-JS

Let’s design it like a real crate, then adapt it cleanly for native + wasm.

---

## 1. Architecture: core + platform adapters

At a high level:

- **Core crate (`search-core`)**
  - Knows about: queries, results, engines, parsing.
  - Does **not** know about: how HTTP is done, which runtime, or whether it’s native/wasm.
- **Platform crate(s) (`search-http`)**
  - Provide an `HttpClient` implementation using `reqwest` (native, WASI, or browser WASM).
- **App / UI**
  - Browser WASM, CLI, server, etc., all just call `SearchEngine::search`.

This keeps your search logic testable and your wasm story sane.

---

## 2. Core crate: traits and types

`search-core/Cargo.toml`:

```toml
[package]
name = "search-core"
version = "0.1.0"
edition = "2021"

[dependencies]
scraper = "0.18"
serde = { version = "1", features = ["derive"] }
```

`src/lib.rs`:

```rust
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SearchQuery<'a> {
    pub text: &'a str,
    pub page: u32,
}

#[async_trait::async_trait]
pub trait HttpClient: Send + Sync {
    async fn get_text(&self, url: &str) -> Result<String, HttpError>;
}

#[derive(Debug, thiserror::Error)]
pub enum HttpError {
    #[error("network error: {0}")]
    Network(String),
    #[error("status error: {0}")]
    Status(String),
}

#[async_trait::async_trait]
pub trait SearchEngine: Send + Sync {
    async fn search(
        &self,
        client: &dyn HttpClient,
        query: SearchQuery<'_>,
    ) -> Result<Vec<SearchResult>, SearchError>;
}

#[derive(Debug, thiserror::Error)]
pub enum SearchError {
    #[error("http error: {0}")]
    Http(#[from] HttpError),
    #[error("parse error: {0}")]
    Parse(String),
}
```

Now a concrete engine, e.g. a “DuckDuckGo-like HTML” engine (selectors are illustrative—you’d tune them to the real DOM):

```rust
pub struct DuckDuckGoEngine;

#[async_trait::async_trait]
impl SearchEngine for DuckDuckGoEngine {
    async fn search(
        &self,
        client: &dyn HttpClient,
        query: SearchQuery<'_>,
    ) -> Result<Vec<SearchResult>, SearchError> {
        let encoded = urlencoding::encode(query.text);
        let url = format!("https://duckduckgo.com/html/?q={encoded}&s={}", query.page * 10);

        let html = client.get_text(&url).await?;
        Ok(parse_duckduckgo(&html)?)
    }
}

fn parse_duckduckgo(html: &str) -> Result<Vec<SearchResult>, SearchError> {
    let doc = Html::parse_document(html);

    let result_sel = Selector::parse(".result").unwrap();
    let title_sel = Selector::parse("a.result__a").unwrap();
    let snippet_sel = Selector::parse(".result__snippet").unwrap();

    let mut results = Vec::new();

    for node in doc.select(&result_sel) {
        let title_node = match node.select(&title_sel).next() {
            Some(n) => n,
            None => continue,
        };

        let title = title_node.text().collect::<String>().trim().to_string();
        let url = title_node
            .value()
            .attr("href")
            .unwrap_or("")
            .to_string();

        if title.is_empty() || url.is_empty() {
            continue;
        }

        let snippet = node
            .select(&snippet_sel)
            .next()
            .map(|n| n.text().collect::<String>().trim().to_string())
            .filter(|s| !s.is_empty());

        results.push(SearchResult { title, url, snippet });
    }

    Ok(results)
}
```

Core crate is now **runtime-agnostic** and **platform-agnostic**.

---

## 3. HTTP client crate with native + wasm support

`search-http/Cargo.toml`:

```toml
[package]
name = "search-http"
version = "0.1.0"
edition = "2021"

[dependencies]
search-core = { path = "../search-core" }
async-trait = "0.1"
thiserror = "1"

# Shared
reqwest = { version = "0.12", default-features = false, features = ["json"] }

# Native / WASI async runtime
tokio = { version = "1", features = ["rt-multi-thread", "macros"], optional = true }

[features]
default = ["native"]
native = ["tokio", "reqwest/rustls-tls"]
wasm = []
```

`src/lib.rs`:

```rust
use async_trait::async_trait;
use search_core::{HttpClient, HttpError};

pub struct ReqwestClient {
    inner: reqwest::Client,
}

impl ReqwestClient {
    pub fn new() -> Self {
        Self {
            inner: reqwest::Client::new(),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[async_trait]
impl HttpClient for ReqwestClient {
    async fn get_text(&self, url: &str) -> Result<String, HttpError> {
        let res = self
            .inner
            .get(url)
            .header("User-Agent", "search-http/0.1")
            .send()
            .await
            .map_err(|e| HttpError::Network(e.to_string()))?;

        let status = res.status();
        if !status.is_success() {
            return Err(HttpError::Status(status.to_string()));
        }

        res.text()
            .await
            .map_err(|e| HttpError::Network(e.to_string()))
    }
}
```

For **wasm32-unknown-unknown** (browser), `reqwest` already swaps to a Fetch-based backend. You just need the `wasm32` impl and a way to drive the future:

```rust
#[cfg(target_arch = "wasm32")]
#[async_trait]
impl HttpClient for ReqwestClient {
    async fn get_text(&self, url: &str) -> Result<String, HttpError> {
        let res = self
            .inner
            .get(url)
            .header("User-Agent", "search-http/0.1") // may be ignored by browser
            .send()
            .await
            .map_err(|e| HttpError::Network(e.to_string()))?;

        let status = res.status();
        if !status.is_success() {
            return Err(HttpError::Status(status.to_string()));
        }

        res.text()
            .await
            .map_err(|e| HttpError::Network(e.to_string()))
    }
}
```

Key points:

- Same `ReqwestClient` type, different impl behind `cfg`.
- No mention of tokio in wasm; the browser’s event loop + `wasm-bindgen-futures` will drive it.

---

## 4. Browser WASM entrypoint

Example `search-wasm` crate that exposes a JS-friendly API.

`Cargo.toml`:

```toml
[package]
name = "search-wasm"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
wasm-bindgen = "0.2"
wasm-bindgen-futures = "0.4"
search-core = { path = "../search-core" }
search-http = { path = "../search-http", features = ["wasm"] }
serde = { version = "1", features = ["derive"] }
serde_wasm_bindgen = "0.6"
```

`src/lib.rs`:

```rust
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::future_to_promise;

use search_core::{SearchQuery, SearchEngine};
use search_http::ReqwestClient;

mod engines;
use engines::DuckDuckGoEngine;

#[wasm_bindgen]
pub fn search(query: String, page: u32) -> js_sys::Promise {
    // Move everything into the async block
    future_to_promise(async move {
        let client = ReqwestClient::new();
        let engine = DuckDuckGoEngine;

        let q = SearchQuery { text: &query, page };

        let results = engine
            .search(&client, q)
            .await
            .map_err(|e| JsValue::from_str(&format!("search error: {e}")))?;

        let js_value = serde_wasm_bindgen::to_value(&results)
            .map_err(|e| JsValue::from_str(&format!("serde error: {e}")))?;

        Ok(js_value)
    })
}
```

From JS:

```js
import init, { search } from "./search_wasm.js";

await init();

const results = await search("rust wasm http", 0);
console.log(results);
```

### CORS reality check

In the browser:

- You **cannot** just hit arbitrary search engines from WASM; CORS will block you.
- You either:
  - **Own the search backend** (your own HTML endpoint or API), or
  - Run a **server-side proxy** that your WASM calls, and the proxy talks to the search engine.

The architecture above still holds: the core crate doesn’t care whether the URL is `https://duckduckgo.com/html` or `https://your-proxy/search`.

---

## 5. Native / WASI usage

CLI example (native):

```rust
use search_core::{SearchQuery, SearchEngine};
use search_http::ReqwestClient;
use your_engines_crate::DuckDuckGoEngine;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = ReqwestClient::new();
    let engine = DuckDuckGoEngine;

    let q = SearchQuery { text: "rust wasm", page: 0 };
    let results = engine.search(&client, q).await?;

    for (i, r) in results.iter().enumerate() {
        println!("{}. {}\n   {}", i + 1, r.title, r.url);
    }

    Ok(())
}
```

For **WASI** (e.g., WasmEdge), you can:

- Target `wasm32-wasip1`.
- Use a WASI-capable runtime that allows outbound HTTP.
- Either patch `reqwest`/`tokio` as in WasmEdge docs, or use their pre-patched crates.

The nice part: you still implement `HttpClient` once per environment, and the rest of the stack doesn’t change.

---

## 6. Where to deepen further

If you want to push this toward “real library” quality:

- **Engine abstraction:**
  - Multiple engines implementing `SearchEngine`.
  - A `CompositeEngine` that fans out to several and merges results.
- **Pagination & cursors:**
  - Instead of `page: u32`, use an opaque `Cursor` type per engine.
- **Rate limiting & politeness:**
  - Per-engine throttling, backoff, and robots.txt awareness (for your own endpoints).
- **Error taxonomy:**
  - Distinguish parse drift (DOM changed) vs. network vs. quota vs. CORS.

