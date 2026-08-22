//! Native immutable SPKG archive upload and download routes.

use bytes::Bytes;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use warp::http::{Response, StatusCode};
use warp::hyper::Body;
use warp::{Filter, Rejection};
use zip::{CompressionMethod, ZipArchive};

const MAX_ARCHIVE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_ENTRIES: usize = 10_000;
const MEDIA_TYPE: &str = "application/vnd.spacekit.spkg+zip";
const CACHE_CONTROL: &str = "public, max-age=31536000, immutable";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SpkgMetadata {
    pub app_id: String,
}

fn package_dir(data_dir: &Path, hash: &str) -> PathBuf {
    data_dir.join("packages").join(&hash[..2])
}

fn package_path(data_dir: &Path, hash: &str) -> PathBuf {
    package_dir(data_dir, hash).join(hash)
}

fn app_alias_path(data_dir: &Path, app_id: &str) -> PathBuf {
    data_dir.join("packages").join("by-app").join(app_id)
}

fn valid_package_hash(hash: &str) -> bool {
    hash.len() == 64
        && hash
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn normalized_app_id(app_id: &str) -> Option<String> {
    (app_id.len() == 64 && app_id.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| app_id.to_ascii_lowercase())
}

fn normalized_entry_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.ends_with('/')
        && !path.contains('\\')
        && !path.contains('\0')
        && path
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
}

fn parse_hash(value: &Value, field: &str) -> Result<[u8; 32], String> {
    if let Some(text) = value.as_str() {
        if text.len() != 64 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(format!("{field} must be a 64-character SHA-256 hex string"));
        }
        let bytes = hex::decode(text).map_err(|_| format!("{field} is not valid hex"))?;
        return bytes
            .try_into()
            .map_err(|_| format!("{field} must contain 32 bytes"));
    }

    let values = value
        .as_array()
        .ok_or_else(|| format!("{field} must be a 32-byte array or hex string"))?;
    if values.len() != 32 {
        return Err(format!("{field} must contain 32 bytes"));
    }
    let mut result = [0_u8; 32];
    for (index, value) in values.iter().enumerate() {
        let byte = value
            .as_u64()
            .filter(|value| *value <= u8::MAX as u64)
            .ok_or_else(|| format!("{field} contains a non-byte value"))?;
        result[index] = byte as u8;
    }
    Ok(result)
}

fn read_entry(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    index: usize,
    expected_size: u64,
) -> Result<Vec<u8>, String> {
    let capacity = usize::try_from(expected_size)
        .map_err(|_| "ZIP entry is too large for this platform".to_string())?;
    let mut bytes = Vec::with_capacity(capacity);
    let mut entry = archive
        .by_index(index)
        .map_err(|error| format!("cannot open ZIP entry: {error}"))?;
    (&mut entry)
        .take(expected_size.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read ZIP entry: {error}"))?;
    if bytes.len() as u64 != expected_size {
        return Err("ZIP entry size differs from its declared size".to_string());
    }
    Ok(bytes)
}

/// Validate an SPKG in place without extracting any archive entry.
pub(crate) fn validate_spkg(bytes: &[u8]) -> Result<SpkgMetadata, String> {
    let mut archive =
        ZipArchive::new(Cursor::new(bytes)).map_err(|error| format!("invalid ZIP: {error}"))?;
    if archive.len() == 0 || archive.len() > MAX_ENTRIES {
        return Err(format!(
            "SPKG must contain between 1 and {MAX_ENTRIES} entries"
        ));
    }

    let mut entries = HashMap::<String, (usize, u64)>::new();
    let mut total_uncompressed = 0_u64;
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|error| format!("cannot inspect ZIP entry: {error}"))?;
        let name = std::str::from_utf8(entry.name_raw())
            .map_err(|_| "ZIP entry names must be UTF-8".to_string())?
            .to_string();
        let is_symlink = entry
            .unix_mode()
            .map(|mode| mode & 0o170000 == 0o120000)
            .unwrap_or(false);
        if !normalized_entry_path(&name) || entry.is_dir() || is_symlink {
            return Err(format!("unsafe or non-file ZIP entry path: {name:?}"));
        }
        if !matches!(
            entry.compression(),
            CompressionMethod::Stored | CompressionMethod::Deflated
        ) {
            return Err(format!("unsupported ZIP compression for entry: {name}"));
        }
        total_uncompressed = total_uncompressed
            .checked_add(entry.size())
            .ok_or_else(|| "total uncompressed size overflow".to_string())?;
        if total_uncompressed > MAX_ARCHIVE_BYTES {
            return Err("SPKG exceeds 512 MiB uncompressed".to_string());
        }
        if entries
            .insert(name.clone(), (index, entry.size()))
            .is_some()
        {
            return Err(format!("duplicate ZIP entry: {name}"));
        }
        if index == 0
            && (name != "mimetype"
                || entry.compression() != CompressionMethod::Stored
                || entry.size() != MEDIA_TYPE.len() as u64)
        {
            return Err("mimetype must be the first, stored ZIP entry".to_string());
        }
    }

    let &(mimetype_index, mimetype_size) = entries
        .get("mimetype")
        .ok_or_else(|| "missing mimetype entry".to_string())?;
    if read_entry(&mut archive, mimetype_index, mimetype_size)? != MEDIA_TYPE.as_bytes() {
        return Err("mimetype entry has invalid contents".to_string());
    }

    let &(manifest_index, manifest_size) = entries
        .get("manifest.json")
        .ok_or_else(|| "missing manifest.json entry".to_string())?;
    let manifest_bytes = read_entry(&mut archive, manifest_index, manifest_size)?;
    let package: Value = serde_json::from_slice(&manifest_bytes)
        .map_err(|error| format!("manifest.json is not valid JSON: {error}"))?;
    let package = package
        .as_object()
        .ok_or_else(|| "manifest.json must contain a JSON object".to_string())?;
    let app_id = package
        .get("app_id")
        .ok_or_else(|| "manifest.json is missing app_id".to_string())?;
    let app_id = hex::encode(parse_hash(app_id, "app_id")?);
    let refs = package
        .get("content_refs")
        .and_then(Value::as_array)
        .ok_or_else(|| "manifest.json is missing content_refs".to_string())?;
    let manifest = package
        .get("manifest")
        .and_then(Value::as_object)
        .ok_or_else(|| "manifest.json is missing manifest".to_string())?;
    let expected_checksum = parse_hash(
        manifest
            .get("checksum")
            .ok_or_else(|| "manifest is missing checksum".to_string())?,
        "manifest.checksum",
    )?;

    let mut referenced_payloads = HashSet::new();
    let mut aggregate = Sha256::new();
    let mut referenced_size = 0_u64;
    for (position, content_ref) in refs.iter().enumerate() {
        let content_ref = content_ref
            .as_object()
            .ok_or_else(|| format!("content_refs[{position}] must be an object"))?;
        let path = content_ref
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("content_refs[{position}].path is missing"))?;
        if !normalized_entry_path(path) {
            return Err(format!(
                "content_refs[{position}].path is not normalized: {path:?}"
            ));
        }
        let payload_path = format!("payload/{path}");
        if !referenced_payloads.insert(payload_path.clone()) {
            return Err(format!("duplicate content reference: {path}"));
        }
        let expected_size = content_ref
            .get("size")
            .and_then(Value::as_u64)
            .ok_or_else(|| format!("content_refs[{position}].size is missing"))?;
        referenced_size = referenced_size
            .checked_add(expected_size)
            .ok_or_else(|| "content reference size overflow".to_string())?;
        let expected_hash = parse_hash(
            content_ref
                .get("hash")
                .ok_or_else(|| format!("content_refs[{position}].hash is missing"))?,
            &format!("content_refs[{position}].hash"),
        )?;
        aggregate.update(expected_hash);

        let &(entry_index, entry_size) = entries
            .get(&payload_path)
            .ok_or_else(|| format!("missing payload entry: {payload_path}"))?;
        if entry_size != expected_size {
            return Err(format!("payload size mismatch: {payload_path}"));
        }

        let mut entry = archive
            .by_index(entry_index)
            .map_err(|error| format!("cannot open {payload_path}: {error}"))?;
        let mut hasher = Sha256::new();
        let mut buffer = [0_u8; 64 * 1024];
        let mut bytes_read = 0_u64;
        loop {
            let count = entry
                .read(&mut buffer)
                .map_err(|error| format!("cannot read {payload_path}: {error}"))?;
            if count == 0 {
                break;
            }
            bytes_read += count as u64;
            if bytes_read > expected_size {
                return Err(format!("payload size mismatch: {payload_path}"));
            }
            hasher.update(&buffer[..count]);
        }
        if bytes_read != expected_size || hasher.finalize().as_slice() != expected_hash {
            return Err(format!("payload hash mismatch: {payload_path}"));
        }
    }

    for name in entries.keys().filter(|name| name.starts_with("payload/")) {
        if !referenced_payloads.contains(name) {
            return Err(format!("unreferenced payload entry: {name}"));
        }
    }
    if let Some(total_size) = manifest.get("total_size").and_then(Value::as_u64) {
        if total_size != referenced_size {
            return Err("manifest.total_size does not match content_refs".to_string());
        }
    }
    if aggregate.finalize().as_slice() != expected_checksum {
        return Err("manifest checksum mismatch".to_string());
    }
    Ok(SpkgMetadata { app_id })
}

fn json_response(status: StatusCode, value: Value) -> Response<Body> {
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Body::from(value.to_string()))
        .expect("static response headers are valid")
}

async fn write_app_alias(data_dir: &Path, app_id: &str, hash: &str) -> std::io::Result<()> {
    let alias = app_alias_path(data_dir, app_id);
    let directory = alias
        .parent()
        .expect("an app alias always has a parent directory");
    tokio::fs::create_dir_all(directory).await?;
    let temporary = directory.join(format!(".{app_id}.{}.tmp", uuid::Uuid::new_v4()));
    tokio::fs::write(&temporary, hash.as_bytes()).await?;
    if let Err(error) = tokio::fs::rename(&temporary, &alias).await {
        let _ = tokio::fs::remove_file(&temporary).await;
        return Err(error);
    }
    Ok(())
}

async fn resolve_app_alias(data_dir: &Path, app_id: &str) -> Option<String> {
    let alias = app_alias_path(data_dir, app_id);
    let metadata = tokio::fs::metadata(&alias).await.ok()?;
    if !metadata.is_file() || metadata.len() != 64 {
        return None;
    }
    let hash = tokio::fs::read_to_string(alias).await.ok()?;
    valid_package_hash(&hash).then_some(hash)
}

fn package_headers(
    builder: warp::http::response::Builder,
    hash: &str,
    length: u64,
    include_package_hash: bool,
) -> warp::http::response::Builder {
    let builder = builder
        .header("content-type", MEDIA_TYPE)
        .header("content-length", length)
        .header("etag", format!("\"{hash}\""))
        .header("cache-control", CACHE_CONTROL)
        .header("accept-ranges", "bytes");
    if include_package_hash {
        builder.header("x-spacekit-package-hash", hash)
    } else {
        builder
    }
}

async fn put_package(
    hash: String,
    body: Bytes,
    data_dir: Option<PathBuf>,
    semaphore: Arc<tokio::sync::Semaphore>,
    auth_mode: crate::access_policy::BlobFactAuthMode,
    auth_header: Option<String>,
) -> Result<Response<Body>, Rejection> {
    let _permit = semaphore.acquire().await.map_err(|_| warp::reject())?;
    if !valid_package_hash(&hash) {
        return Ok(json_response(
            StatusCode::BAD_REQUEST,
            serde_json::json!({"error": "invalid package SHA-256"}),
        ));
    }

    let secret = crate::upload_token::load_signing_secret(data_dir.as_deref());
    if auth_mode.blobs_require_did_on_write()
        && crate::upload_token::authorize_blob_write(
            auth_header.as_deref(),
            &hash,
            secret.as_deref(),
            super::unix_now_secs(),
        )
        .is_none()
    {
        return Ok(json_response(
            StatusCode::UNAUTHORIZED,
            serde_json::json!({"error": "authorization required (DID or PutBlob UploadToken)"}),
        ));
    }

    let Some(data_dir) = data_dir else {
        return Ok(json_response(
            StatusCode::SERVICE_UNAVAILABLE,
            serde_json::json!({"error": "storage not configured"}),
        ));
    };
    let actual_hash = hex::encode(Sha256::digest(&body));
    if actual_hash != hash {
        return Ok(json_response(
            StatusCode::BAD_REQUEST,
            serde_json::json!({"error": "archive SHA-256 mismatch", "expected": hash, "actual": actual_hash}),
        ));
    }

    let validation_bytes = body.clone();
    let validation = tokio::task::spawn_blocking(move || validate_spkg(&validation_bytes))
        .await
        .map_err(|_| warp::reject())?;
    let spkg = match validation {
        Ok(metadata) => metadata,
        Err(error) => {
            return Ok(json_response(
                StatusCode::UNPROCESSABLE_ENTITY,
                serde_json::json!({"error": error}),
            ));
        }
    };

    let path = package_path(&data_dir, &hash);
    let existed = tokio::fs::metadata(&path).await.is_ok();
    if !existed {
        let dir = package_dir(&data_dir, &hash);
        tokio::fs::create_dir_all(&dir)
            .await
            .map_err(|_| warp::reject())?;
        let temporary = dir.join(format!(".{hash}.{}.tmp", uuid::Uuid::new_v4()));
        if let Err(error) = tokio::fs::write(&temporary, &body).await {
            return Ok(json_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({"error": format!("package write failed: {error}")}),
            ));
        }
        if let Err(error) = tokio::fs::rename(&temporary, &path).await {
            let _ = tokio::fs::remove_file(&temporary).await;
            if tokio::fs::metadata(&path).await.is_err() {
                return Ok(json_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    serde_json::json!({"error": format!("package install failed: {error}")}),
                ));
            }
        }
    }
    if let Err(error) = write_app_alias(&data_dir, &spkg.app_id, &hash).await {
        return Ok(json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({"error": format!("app alias update failed: {error}")}),
        ));
    }
    Ok(json_response(
        if existed {
            StatusCode::OK
        } else {
            StatusCode::CREATED
        },
        serde_json::json!({
            "app_id": spkg.app_id,
            "hash": hash,
            "size": body.len(),
            "status": if existed { "exists" } else { "created" },
        }),
    ))
}

async fn stream_package(
    hash: String,
    range_header: Option<String>,
    data_dir: Option<PathBuf>,
    semaphore: Arc<tokio::sync::Semaphore>,
    include_package_hash: bool,
) -> Result<Response<Body>, Rejection> {
    let _permit = semaphore.acquire().await.map_err(|_| warp::reject())?;
    if !valid_package_hash(&hash) {
        return Ok(json_response(
            StatusCode::BAD_REQUEST,
            serde_json::json!({"error": "invalid package SHA-256"}),
        ));
    }
    let Some(data_dir) = data_dir else {
        return Ok(json_response(
            StatusCode::SERVICE_UNAVAILABLE,
            serde_json::json!({"error": "storage not configured"}),
        ));
    };
    let path = package_path(&data_dir, &hash);
    let metadata = match tokio::fs::metadata(&path).await {
        Ok(metadata) => metadata,
        Err(_) => {
            return Ok(json_response(
                StatusCode::NOT_FOUND,
                serde_json::json!({"error": "package not found"}),
            ));
        }
    };
    let range = match range_header.as_deref() {
        Some(header) => match crate::streaming::ByteRange::parse(header, metadata.len()) {
            Some(range) => Some(range),
            None => {
                return Ok(Response::builder()
                    .status(StatusCode::RANGE_NOT_SATISFIABLE)
                    .header("content-range", format!("bytes */{}", metadata.len()))
                    .body(Body::empty())
                    .expect("valid range response"));
            }
        },
        None => None,
    };
    let (stream, stream_meta) =
        crate::streaming::file_stream(&path, crate::streaming::StreamingConfig::default(), range)
            .await
            .map_err(|_| warp::reject())?;
    let status = if range.is_some() {
        StatusCode::PARTIAL_CONTENT
    } else {
        StatusCode::OK
    };
    let mut builder = package_headers(
        Response::builder().status(status),
        &hash,
        stream_meta.length,
        include_package_hash,
    );
    if range.is_some() {
        builder = builder.header("content-range", stream_meta.content_range());
    }
    Ok(builder
        .body(Body::wrap_stream(stream))
        .expect("valid package response"))
}

async fn head_package_response(
    hash: String,
    data_dir: Option<PathBuf>,
    include_package_hash: bool,
) -> Result<Response<Body>, Rejection> {
    if !valid_package_hash(&hash) {
        return Ok(Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .body(Body::empty())
            .expect("valid response"));
    }
    let Some(data_dir) = data_dir else {
        return Ok(Response::builder()
            .status(StatusCode::SERVICE_UNAVAILABLE)
            .body(Body::empty())
            .expect("valid response"));
    };
    match tokio::fs::metadata(package_path(&data_dir, &hash)).await {
        Ok(metadata) => Ok(package_headers(
            Response::builder().status(StatusCode::OK),
            &hash,
            metadata.len(),
            include_package_hash,
        )
        .body(Body::empty())
        .expect("valid package response")),
        Err(_) => Ok(Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::empty())
            .expect("valid response")),
    }
}

async fn get_app_package(
    app_id: String,
    range_header: Option<String>,
    data_dir: Option<PathBuf>,
    semaphore: Arc<tokio::sync::Semaphore>,
) -> Result<Response<Body>, Rejection> {
    let Some(app_id) = normalized_app_id(&app_id) else {
        return Ok(json_response(
            StatusCode::BAD_REQUEST,
            serde_json::json!({"error": "invalid app ID"}),
        ));
    };
    let Some(data_dir) = data_dir else {
        return Ok(json_response(
            StatusCode::SERVICE_UNAVAILABLE,
            serde_json::json!({"error": "storage not configured"}),
        ));
    };
    let Some(hash) = resolve_app_alias(&data_dir, &app_id).await else {
        return Ok(json_response(
            StatusCode::NOT_FOUND,
            serde_json::json!({"error": "app package not found"}),
        ));
    };
    stream_package(hash, range_header, Some(data_dir), semaphore, true).await
}

async fn head_app_package(
    app_id: String,
    data_dir: Option<PathBuf>,
) -> Result<Response<Body>, Rejection> {
    let Some(app_id) = normalized_app_id(&app_id) else {
        return Ok(Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .body(Body::empty())
            .expect("valid response"));
    };
    let Some(data_dir) = data_dir else {
        return Ok(Response::builder()
            .status(StatusCode::SERVICE_UNAVAILABLE)
            .body(Body::empty())
            .expect("valid response"));
    };
    let Some(hash) = resolve_app_alias(&data_dir, &app_id).await else {
        return Ok(Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::empty())
            .expect("valid response"));
    };
    head_package_response(hash, Some(data_dir), true).await
}

pub fn routes(
    data_dir: Option<PathBuf>,
    auth_mode: crate::access_policy::BlobFactAuthMode,
    semaphore: Arc<tokio::sync::Semaphore>,
) -> warp::filters::BoxedFilter<(Response<Body>,)> {
    let put_data_dir = data_dir.clone();
    let put_semaphore = semaphore.clone();
    let put = warp::path!("packages" / String)
        .and(warp::put())
        .and(warp::body::content_length_limit(MAX_ARCHIVE_BYTES))
        .and(warp::body::bytes())
        .and(warp::any().map(move || put_data_dir.clone()))
        .and(warp::any().map(move || put_semaphore.clone()))
        .and(warp::any().map(move || auth_mode))
        .and(warp::header::optional::<String>("authorization"))
        .and_then(put_package);

    // App aliases must be registered before the generic package hash routes.
    let app_get_data_dir = data_dir.clone();
    let app_get_semaphore = semaphore.clone();
    let app_get = warp::path!("packages" / "apps" / String)
        .and(warp::get())
        .and(warp::header::optional::<String>("range"))
        .and(warp::any().map(move || app_get_data_dir.clone()))
        .and(warp::any().map(move || app_get_semaphore.clone()))
        .and_then(get_app_package);

    let app_head_data_dir = data_dir.clone();
    let app_head = warp::path!("packages" / "apps" / String)
        .and(warp::head())
        .and(warp::any().map(move || app_head_data_dir.clone()))
        .and_then(head_app_package);

    let get_data_dir = data_dir.clone();
    let get_semaphore = semaphore;
    let get = warp::path!("packages" / String)
        .and(warp::get())
        .and(warp::header::optional::<String>("range"))
        .and(warp::any().map(move || get_data_dir.clone()))
        .and(warp::any().map(move || get_semaphore.clone()))
        .and(warp::any().map(|| false))
        .and_then(stream_package);

    let head = warp::path!("packages" / String)
        .and(warp::head())
        .and(warp::any().map(move || data_dir.clone()))
        .and(warp::any().map(|| false))
        .and_then(head_package_response);

    put.or(app_get)
        .unify()
        .or(app_head)
        .unify()
        .or(get)
        .unify()
        .or(head)
        .unify()
        .boxed()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::FileOptions;
    use zip::ZipWriter;

    fn package(payload_path: &str, payload: &[u8], claimed_hash: [u8; 32]) -> Vec<u8> {
        let checksum: [u8; 32] = Sha256::digest(claimed_hash).into();
        let manifest = serde_json::json!({
            "app_id": vec![0_u8; 32],
            "content_refs": [{
                "path": payload_path,
                "size": payload.len(),
                "hash": claimed_hash,
            }],
            "manifest": {
                "total_size": payload.len(),
                "checksum": checksum,
            }
        });
        let cursor = Cursor::new(Vec::new());
        let mut writer = ZipWriter::new(cursor);
        writer
            .start_file(
                "mimetype",
                FileOptions::default().compression_method(CompressionMethod::Stored),
            )
            .unwrap();
        writer.write_all(MEDIA_TYPE.as_bytes()).unwrap();
        writer
            .start_file(
                "manifest.json",
                FileOptions::default().compression_method(CompressionMethod::Deflated),
            )
            .unwrap();
        writer
            .write_all(serde_json::to_string(&manifest).unwrap().as_bytes())
            .unwrap();
        writer
            .start_file(
                format!("payload/{payload_path}"),
                FileOptions::default().compression_method(CompressionMethod::Deflated),
            )
            .unwrap();
        writer.write_all(payload).unwrap();
        writer.finish().unwrap().into_inner()
    }

    #[test]
    fn accepts_valid_spkg() {
        let payload = b"hello spacekit";
        let hash: [u8; 32] = Sha256::digest(payload).into();
        assert_eq!(
            validate_spkg(&package("index.html", payload, hash)),
            Ok(SpkgMetadata {
                app_id: "0".repeat(64)
            })
        );
    }

    #[test]
    fn rejects_unsafe_payload_path() {
        let payload = b"bad";
        let hash: [u8; 32] = Sha256::digest(payload).into();
        let error = validate_spkg(&package("../bad", payload, hash)).unwrap_err();
        assert!(error.contains("unsafe") || error.contains("normalized"));
    }

    #[test]
    fn rejects_payload_hash_mismatch() {
        let error = validate_spkg(&package("app.js", b"actual", [7; 32])).unwrap_err();
        assert!(error.contains("payload hash mismatch"));
    }

    #[test]
    fn package_hash_requires_lowercase_sha256() {
        assert!(valid_package_hash(&"a".repeat(64)));
        assert!(!valid_package_hash(&"A".repeat(64)));
        assert!(!valid_package_hash("abc"));
    }

    #[tokio::test]
    async fn upload_creates_public_app_alias_routes() {
        let temp = tempfile::tempdir().unwrap();
        let payload = b"alias payload";
        let payload_hash: [u8; 32] = Sha256::digest(payload).into();
        let archive = package("index.html", payload, payload_hash);
        let package_hash = hex::encode(Sha256::digest(&archive));
        let app_id = "0".repeat(64);
        let routes = routes(
            Some(temp.path().to_path_buf()),
            crate::access_policy::BlobFactAuthMode::Permissive,
            Arc::new(tokio::sync::Semaphore::new(2)),
        );

        let uploaded = warp::test::request()
            .method("PUT")
            .path(&format!("/packages/{package_hash}"))
            .body(archive.clone())
            .reply(&routes)
            .await;
        assert_eq!(uploaded.status(), StatusCode::CREATED);
        let uploaded_json: Value = serde_json::from_slice(uploaded.body()).unwrap();
        assert_eq!(uploaded_json["app_id"], app_id);

        let existing = warp::test::request()
            .method("PUT")
            .path(&format!("/packages/{package_hash}"))
            .body(archive.clone())
            .reply(&routes)
            .await;
        assert_eq!(existing.status(), StatusCode::OK);
        let existing_json: Value = serde_json::from_slice(existing.body()).unwrap();
        assert_eq!(existing_json["app_id"], app_id);

        let downloaded = warp::test::request()
            .method("GET")
            .path(&format!("/packages/apps/{}", app_id.to_ascii_uppercase()))
            .reply(&routes)
            .await;
        assert_eq!(downloaded.status(), StatusCode::OK);
        assert_eq!(downloaded.body().as_ref(), archive);
        assert_eq!(
            downloaded.headers()["x-spacekit-package-hash"],
            package_hash
        );
        assert_eq!(downloaded.headers()["etag"], format!("\"{package_hash}\""));

        let head = warp::test::request()
            .method("HEAD")
            .path(&format!("/packages/apps/{app_id}"))
            .reply(&routes)
            .await;
        assert_eq!(head.status(), StatusCode::OK);
        assert!(head.body().is_empty());
        assert_eq!(head.headers()["x-spacekit-package-hash"], package_hash);
        assert_eq!(head.headers()["content-length"], archive.len().to_string());
    }

    #[tokio::test]
    async fn bad_or_invalid_app_alias_is_rejected_safely() {
        let temp = tempfile::tempdir().unwrap();
        let app_id = "1".repeat(64);
        tokio::fs::create_dir_all(temp.path().join("packages/by-app"))
            .await
            .unwrap();
        tokio::fs::write(
            temp.path().join("packages/by-app").join(&app_id),
            b"not-a-package-hash",
        )
        .await
        .unwrap();
        let routes = routes(
            Some(temp.path().to_path_buf()),
            crate::access_policy::BlobFactAuthMode::Permissive,
            Arc::new(tokio::sync::Semaphore::new(1)),
        );

        let bad_alias = warp::test::request()
            .method("GET")
            .path(&format!("/packages/apps/{app_id}"))
            .reply(&routes)
            .await;
        assert_eq!(bad_alias.status(), StatusCode::NOT_FOUND);

        let invalid_id = warp::test::request()
            .method("GET")
            .path("/packages/apps/not-a-hash")
            .reply(&routes)
            .await;
        assert_eq!(invalid_id.status(), StatusCode::BAD_REQUEST);
    }
}
