//! MIME types for `GET /facts/{id}/stream` — web packages need correct types for ES modules.

use spacekit_primitives::v1::fact::{FactContent, FactPackage};
use std::path::Path;

/// Map a file path or filename extension to an HTTP Content-Type.
pub fn mime_from_path(path: &str) -> String {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "html" | "htm" => "text/html; charset=utf-8".to_string(),
        "css" => "text/css; charset=utf-8".to_string(),
        "js" | "mjs" => "application/javascript; charset=utf-8".to_string(),
        "json" => "application/json; charset=utf-8".to_string(),
        "wasm" => "application/wasm".to_string(),
        "svg" => "image/svg+xml".to_string(),
        "png" => "image/png".to_string(),
        "jpg" | "jpeg" => "image/jpeg".to_string(),
        "gif" => "image/gif".to_string(),
        "webp" => "image/webp".to_string(),
        "woff" => "font/woff".to_string(),
        "woff2" => "font/woff2".to_string(),
        "ttf" => "font/ttf".to_string(),
        "otf" => "font/otf".to_string(),
        "txt" => "text/plain; charset=utf-8".to_string(),
        "md" | "markdown" => "text/markdown; charset=utf-8".to_string(),
        "xml" => "application/xml; charset=utf-8".to_string(),
        "pdf" => "application/pdf".to_string(),
        "mp4" | "webm" | "mov" => "video/mp4".to_string(),
        "mp3" | "wav" | "ogg" => "audio/mpeg".to_string(),
        _ => "application/octet-stream".to_string(),
    }
}

/// True when the stored MIME is missing or too generic to serve to browsers.
pub fn is_generic_stream_mime(mime: &str) -> bool {
    let m = mime.trim().to_ascii_lowercase();
    m.is_empty() || m == "application/octet-stream" || m == "binary" || m == "octet-stream"
}

/// Prefer a concrete MIME; fall back to path hints, then octet-stream.
pub fn resolve_stream_mime(stored_mime: &str, path_hints: &[&str]) -> String {
    if !is_generic_stream_mime(stored_mime) {
        return stored_mime.trim().to_string();
    }
    for path in path_hints {
        if path.is_empty() {
            continue;
        }
        let guessed = mime_from_path(path);
        if !is_generic_stream_mime(&guessed) {
            return guessed;
        }
    }
    "application/octet-stream".to_string()
}

/// Collect filename/path hints from fact metadata tags (`title:`, `filename:`, `path:`).
pub fn path_hints_from_fact_tags(tags: &[String]) -> Vec<String> {
    let mut hints = Vec::new();
    for tag in tags {
        if let Some(v) = tag.strip_prefix("title:") {
            hints.push(v.to_string());
        } else if let Some(v) = tag.strip_prefix("filename:") {
            hints.push(v.to_string());
        } else if let Some(v) = tag.strip_prefix("path:") {
            hints.push(v.to_string());
        }
    }
    hints
}

pub fn resolve_stream_mime_for_fact(fact: &FactPackage) -> String {
    let stored = match &fact.content {
        FactContent::Binary { mime_type, .. } => mime_type.as_str(),
        _ => "",
    };
    let tag_paths = path_hints_from_fact_tags(&fact.metadata.tags);
    let hints: Vec<&str> = tag_paths.iter().map(String::as_str).collect();
    resolve_stream_mime(stored, &hints)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn js_from_path() {
        assert_eq!(
            mime_from_path("assets/index-CWQBblDe.js"),
            "application/javascript; charset=utf-8"
        );
    }

    #[test]
    fn prefers_stored_mime() {
        assert_eq!(
            resolve_stream_mime("text/css; charset=utf-8", &["ignored.js"]),
            "text/css; charset=utf-8"
        );
    }

    #[test]
    fn infers_from_path_when_generic() {
        assert_eq!(
            resolve_stream_mime("application/octet-stream", &["assets/app.mjs"]),
            "application/javascript; charset=utf-8"
        );
    }
}
