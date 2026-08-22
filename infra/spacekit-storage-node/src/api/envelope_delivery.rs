//! Bounded-memory artifact delivery: header-only DEK re-wrap + chunked ciphertext stream.

use crate::envelope::{self, EnvelopeHeader, KeySource};
use crate::streaming::{self, ByteRange, StreamingConfig};
use bytes::Bytes;
use futures::stream::{self, StreamExt};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use warp::hyper::Body;

/// Max on-disk bytes allowed for the legacy full-buffer decrypt/re-encrypt path.
/// Larger artifacts must use header-only or chunked streaming delivery.
pub const MAX_LEGACY_FULL_BUFFER_BYTES: u64 = 4 * 1024 * 1024;

/// Server key material needed for header-only re-wrap.
#[derive(Clone, Copy)]
pub struct ServerKeyMaterial<'a> {
    pub secret_key: &'a [u8],
    pub algorithm: &'a str,
    pub key_source: Option<KeySource>,
}

/// Ensure `data_path` is a single-layer PQ envelope by peeling nested wrappers on disk.
/// Peak RAM stays O(chunk); mutates the file in place when nested layers are found.
#[cfg(feature = "quantum")]
async fn ensure_single_layer_on_disk(
    data_path: &Path,
    server_kp: &ServerKeyMaterial<'_>,
) -> Result<(), envelope::EnvelopeError> {
    let peeled = envelope::peel_nested_envelope_on_disk(
        data_path,
        server_kp.secret_key,
        server_kp.algorithm,
        server_kp.key_source,
        envelope::SERVER_ENVELOPE_PEEL_MAX_LAYERS,
    )
    .await?;
    if peeled > 0 {
        tracing::info!(
            "Normalized nested PQ envelope at {} (peeled {} layer(s))",
            data_path.display(),
            peeled
        );
    }
    Ok(())
}

/// SHA-256 of raw Kyber public key bytes (matches entitlement ledger at OP_PURCHASE).
pub fn buyer_public_key_hash(pk_bytes: &[u8]) -> [u8; 32] {
    let digest = Sha256::digest(pk_bytes);
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

#[cfg(feature = "quantum")]
struct OutputChunkPlan {
    nonce_hex: String,
    encrypted_size: u32,
    offset: u64,
}

/// Verify plaintext BLAKE3 by decrypting source chunks one at a time (bounded RAM).
#[cfg(feature = "quantum")]
async fn verify_envelope_plaintext_hash(
    data_path: &Path,
    header_size: usize,
    header: &EnvelopeHeader,
    file_key: &[u8; 32],
) -> Result<(), envelope::EnvelopeError> {
    let mut hasher = blake3::Hasher::new();
    let base = header_size as u64;
    for chunk_meta in &header.chunks {
        let encrypted = envelope::read_file_byte_range(
            data_path,
            base + chunk_meta.offset as u64,
            chunk_meta.encrypted_size as u64,
        )
        .await?;
        let pt = envelope::decrypt_chunk_with_key(file_key, &encrypted, &chunk_meta.nonce_hex)?;
        hasher.update(&pt);
    }
    let computed = hex::encode(hasher.finalize().as_bytes());
    if computed != header.plaintext_hash {
        return Err(envelope::EnvelopeError::IntegrityMismatch {
            expected: header.plaintext_hash.clone(),
            computed,
        });
    }
    Ok(())
}

/// Plan output chunk metadata for a pqcrypto re-wrap (encrypt each plaintext chunk once).
#[cfg(feature = "quantum")]
async fn plan_pqcrypto_output_chunks(
    data_path: &Path,
    header_size: usize,
    source_header: &EnvelopeHeader,
    source_file_key: &[u8; 32],
    dest_file_key: &[u8; 32],
) -> Result<Vec<OutputChunkPlan>, envelope::EnvelopeError> {
    let mut plans = Vec::with_capacity(source_header.chunks.len());
    let mut data_offset: u64 = 0;
    let base = header_size as u64;
    for chunk_meta in &source_header.chunks {
        let encrypted = envelope::read_file_byte_range(
            data_path,
            base + chunk_meta.offset as u64,
            chunk_meta.encrypted_size as u64,
        )
        .await?;
        let pt =
            envelope::decrypt_chunk_with_key(source_file_key, &encrypted, &chunk_meta.nonce_hex)?;
        let (out_enc, nonce_hex) =
            envelope::encrypt_chunk_with_key_generated_nonce(dest_file_key, &pt)?;
        plans.push(OutputChunkPlan {
            nonce_hex,
            encrypted_size: out_enc.len() as u32,
            offset: data_offset,
        });
        data_offset += out_enc.len() as u64;
    }
    Ok(plans)
}

/// Try header-only re-wrap + stream ciphertext tail. Returns `None` when the blob is not a
/// single-layer PQ envelope (after optional nested peel).
#[cfg(feature = "quantum")]
pub async fn try_stream_rewrapped_envelope(
    data_path: &Path,
    server_kp: ServerKeyMaterial<'_>,
    recipient_pk: &[u8],
) -> Result<Option<warp::http::Response<Body>>, envelope::EnvelopeError> {
    // Nested misconfigured uploads: peel to a single layer on disk (O(chunk) RAM), then
    // continue with header-only DEK re-wrap — never fall through to full-file buffers.
    if let Err(e) = ensure_single_layer_on_disk(data_path, &server_kp).await {
        match e {
            envelope::EnvelopeError::Kem(_) => {
                // Not an envelope (or unreadable header) — let caller try other paths.
                return Ok(None);
            }
            other => return Err(other),
        }
    }

    let file_meta = tokio::fs::metadata(data_path)
        .await
        .map_err(|e| envelope::EnvelopeError::Kem(format!("metadata: {}", e)))?;
    let file_size = file_meta.len();

    let (header_prefix, header_size) = match envelope::read_envelope_header_prefix(data_path).await
    {
        Ok(h) => h,
        Err(_) => return Ok(None),
    };

    let (new_header, _old_header_size) = envelope::rewrap_envelope_header_to_pqcrypto_recipient(
        &header_prefix,
        server_kp.secret_key,
        server_kp.algorithm,
        server_kp.key_source,
        recipient_pk,
    )?;

    let data_len = file_size.saturating_sub(header_size as u64);
    let content_length = new_header.len() as u64 + data_len;

    let (tail, _) = streaming::file_stream(
        data_path,
        StreamingConfig::default(),
        Some(ByteRange {
            start: header_size as u64,
            end: None,
        }),
    )
    .await
    .map_err(|e| envelope::EnvelopeError::Kem(format!("file_stream: {}", e)))?;

    let body_stream = streaming::prepend_bytes(Bytes::from(new_header), tail);
    let body = Body::wrap_stream(body_stream);

    let resp = warp::http::Response::builder()
        .status(warp::http::StatusCode::OK)
        .header("content-type", "application/octet-stream")
        .header("x-envelope-version", "1")
        .header("content-length", content_length.to_string())
        .body(body)
        .map_err(|e| envelope::EnvelopeError::Kem(format!("response build: {}", e)))?;

    tracing::info!(
        "Streaming header-rewrapped envelope from {} ({} byte body, {} byte header)",
        data_path.display(),
        content_length,
        header_size
    );

    Ok(Some(resp))
}

/// Bounded-memory chunked decrypt → pqcrypto re-encrypt for stream delivery.
/// Used when header-only re-wrap is unavailable (KEM mismatch, etc.).
#[cfg(feature = "quantum")]
pub async fn try_stream_chunked_rewrap_to_pqcrypto_recipient(
    data_path: &Path,
    server_kp: ServerKeyMaterial<'_>,
    recipient_pk: &[u8],
) -> Result<Option<warp::http::Response<Body>>, envelope::EnvelopeError> {
    if let Err(e) = ensure_single_layer_on_disk(data_path, &server_kp).await {
        match e {
            envelope::EnvelopeError::Kem(_) => return Ok(None),
            other => return Err(other),
        }
    }

    let (header_prefix, header_size) = match envelope::read_envelope_header_prefix(data_path).await
    {
        Ok(h) => h,
        Err(_) => return Ok(None),
    };

    let (source_header, _) = envelope::deserialize_header(&header_prefix)?;
    let source_file_key = envelope::server_file_key_from_header(
        &source_header,
        server_kp.secret_key,
        server_kp.algorithm,
        server_kp.key_source,
    )?;

    verify_envelope_plaintext_hash(data_path, header_size, &source_header, &source_file_key)
        .await?;

    let dest_file_key: [u8; 32] = {
        use aes_gcm::aead::rand_core::RngCore;
        let mut key = [0u8; 32];
        aes_gcm::aead::OsRng.fill_bytes(&mut key);
        key
    };
    let encrypted_file_key = envelope::pqcrypto_kem_encrypt_bytes(&dest_file_key, recipient_pk)?;
    let out_plans = plan_pqcrypto_output_chunks(
        data_path,
        header_size,
        &source_header,
        &source_file_key,
        &dest_file_key,
    )
    .await?;

    let out_chunks_meta: Vec<envelope::ChunkMeta> = out_plans
        .iter()
        .map(|p| envelope::ChunkMeta {
            offset: p.offset,
            encrypted_size: p.encrypted_size,
            nonce_hex: p.nonce_hex.clone(),
        })
        .collect();

    let out_header = EnvelopeHeader {
        version: envelope::ENVELOPE_VERSION,
        kem_algorithm: "Kyber1024".to_string(),
        cipher_suite: "AES-256-GCM".to_string(),
        encrypted_file_key,
        chunk_size: source_header.chunk_size,
        total_chunks: out_chunks_meta.len() as u32,
        total_plaintext_size: source_header.total_plaintext_size,
        plaintext_hash: source_header.plaintext_hash.clone(),
        chunks: out_chunks_meta,
    };

    let header_bytes = envelope::serialize_header(&out_header)
        .map_err(envelope::EnvelopeError::HeaderSerialize)?;
    let body_len = header_bytes.len() as u64
        + out_plans
            .iter()
            .map(|p| p.encrypted_size as u64)
            .sum::<u64>();

    let path = data_path.to_path_buf();
    let source_chunks = source_header.chunks.clone();
    let out_nonces: Vec<String> = out_plans.iter().map(|p| p.nonce_hex.clone()).collect();
    let header_size_u64 = header_size as u64;
    let dest_file_key = dest_file_key;
    let source_file_key = source_file_key;

    let chunk_stream = stream::unfold(0usize, move |idx| {
        let path = path.clone();
        let source_chunks = source_chunks.clone();
        let out_nonces = out_nonces.clone();
        let dest_file_key = dest_file_key;
        let source_file_key = source_file_key;
        let header_size_u64 = header_size_u64;
        async move {
            if idx >= source_chunks.len() {
                return None;
            }
            let chunk_meta = &source_chunks[idx];
            let encrypted = match envelope::read_file_byte_range(
                &path,
                header_size_u64 + chunk_meta.offset as u64,
                chunk_meta.encrypted_size as u64,
            )
            .await
            {
                Ok(b) => b,
                Err(e) => {
                    return Some((
                        Err(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            e.to_string(),
                        )),
                        idx,
                    ));
                }
            };
            let pt = match envelope::decrypt_chunk_with_key(
                &source_file_key,
                &encrypted,
                &chunk_meta.nonce_hex,
            ) {
                Ok(p) => p,
                Err(e) => {
                    return Some((
                        Err(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            e.to_string(),
                        )),
                        idx,
                    ));
                }
            };
            let out_enc =
                match envelope::encrypt_chunk_with_key(&dest_file_key, &pt, &out_nonces[idx]) {
                    Ok(b) => b,
                    Err(e) => {
                        return Some((
                            Err(std::io::Error::new(
                                std::io::ErrorKind::Other,
                                e.to_string(),
                            )),
                            idx,
                        ));
                    }
                };
            Some((Ok(Bytes::from(out_enc)), idx + 1))
        }
    });

    let body_stream = streaming::prepend_bytes(Bytes::from(header_bytes), chunk_stream);
    let body = Body::wrap_stream(body_stream);

    let resp = warp::http::Response::builder()
        .status(warp::http::StatusCode::OK)
        .header("content-type", "application/octet-stream")
        .header("x-envelope-version", "1")
        .header("content-length", body_len.to_string())
        .body(body)
        .map_err(|e| envelope::EnvelopeError::Kem(format!("response build: {}", e)))?;

    tracing::info!(
        "Streaming chunked re-wrapped envelope from {} ({} bytes)",
        data_path.display(),
        body_len
    );

    Ok(Some(resp))
}

/// Bounded-memory server-side decrypt stream for admin-stream (plaintext out).
#[cfg(feature = "quantum")]
pub async fn try_stream_admin_plaintext(
    data_path: &Path,
    server_kp: ServerKeyMaterial<'_>,
) -> Result<Option<warp::http::Response<Body>>, envelope::EnvelopeError> {
    if let Err(e) = ensure_single_layer_on_disk(data_path, &server_kp).await {
        match e {
            envelope::EnvelopeError::Kem(_) => return Ok(None),
            other => return Err(other),
        }
    }

    let (header_prefix, header_size) = match envelope::read_envelope_header_prefix(data_path).await
    {
        Ok(h) => h,
        Err(_) => return Ok(None),
    };

    let (source_header, _) = envelope::deserialize_header(&header_prefix)?;
    let file_key = envelope::server_file_key_from_header(
        &source_header,
        server_kp.secret_key,
        server_kp.algorithm,
        server_kp.key_source,
    )?;

    let path: PathBuf = data_path.to_path_buf();
    let chunks = source_header.chunks.clone();
    let header_size_u64 = header_size as u64;
    let plaintext_len = source_header.total_plaintext_size;

    let chunk_stream = stream::unfold(0usize, move |idx| {
        let path = path.clone();
        let chunks = chunks.clone();
        let file_key = file_key;
        let header_size_u64 = header_size_u64;
        async move {
            if idx >= chunks.len() {
                return None;
            }
            let chunk_meta = &chunks[idx];
            let encrypted = match envelope::read_file_byte_range(
                &path,
                header_size_u64 + chunk_meta.offset as u64,
                chunk_meta.encrypted_size as u64,
            )
            .await
            {
                Ok(b) => b,
                Err(e) => {
                    return Some((
                        Err(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            e.to_string(),
                        )),
                        idx,
                    ));
                }
            };
            let pt = match envelope::decrypt_chunk_with_key(
                &file_key,
                &encrypted,
                &chunk_meta.nonce_hex,
            ) {
                Ok(p) => p,
                Err(e) => {
                    return Some((
                        Err(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            e.to_string(),
                        )),
                        idx,
                    ));
                }
            };
            Some((Ok(Bytes::from(pt)), idx + 1))
        }
    });

    let body = Body::wrap_stream(chunk_stream);
    let resp = warp::http::Response::builder()
        .status(warp::http::StatusCode::OK)
        .header("content-type", "application/octet-stream")
        .header("content-length", plaintext_len.to_string())
        .body(body)
        .map_err(|e| envelope::EnvelopeError::Kem(format!("response build: {}", e)))?;

    tracing::info!(
        "Admin-stream: streaming plaintext from {} ({} bytes)",
        data_path.display(),
        plaintext_len
    );

    Ok(Some(resp))
}

/// Stream on-disk ciphertext bytes without loading the full file into RAM.
#[cfg(feature = "quantum")]
pub async fn try_stream_ciphertext_file(
    data_path: &Path,
) -> Result<warp::http::Response<Body>, std::io::Error> {
    let (tail, meta) = streaming::file_stream(data_path, StreamingConfig::default(), None).await?;
    let body = Body::wrap_stream(tail);
    warp::http::Response::builder()
        .status(warp::http::StatusCode::OK)
        .header("content-type", "application/octet-stream")
        .header("content-length", meta.length.to_string())
        .body(body)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
}

/// Preferred bounded delivery path for `/stream` and `/rewrap`:
/// peel nested layers on disk → header-only DEK re-wrap → chunked re-wrap.
#[cfg(feature = "quantum")]
pub async fn try_stream_delivery_to_pqcrypto_recipient(
    data_path: &Path,
    server_kp: ServerKeyMaterial<'_>,
    recipient_pk: &[u8],
) -> Result<Option<warp::http::Response<Body>>, envelope::EnvelopeError> {
    match try_stream_rewrapped_envelope(data_path, server_kp, recipient_pk).await {
        Ok(Some(resp)) => return Ok(Some(resp)),
        Ok(None) => {
            tracing::info!(
                "Header-only rewrap unavailable for {} — trying chunked rewrap",
                data_path.display()
            );
        }
        Err(e) => {
            tracing::warn!(
                "Header-only rewrap failed for {}: {} — trying chunked rewrap",
                data_path.display(),
                e
            );
        }
    }

    try_stream_chunked_rewrap_to_pqcrypto_recipient(data_path, server_kp, recipient_pk).await
}

/// True when the on-disk blob is too large for the legacy full-buffer decrypt/re-encrypt path.
pub async fn exceeds_legacy_full_buffer_limit(data_path: &Path) -> Result<bool, std::io::Error> {
    let len = tokio::fs::metadata(data_path).await?.len();
    Ok(len > MAX_LEGACY_FULL_BUFFER_BYTES)
}

/// True E2E delivery: replace header `encrypted_file_key` with an owner-posted capsule
/// and stream the immutable ciphertext tail. Storage never unwraps the DEK.
#[cfg(feature = "quantum")]
pub async fn try_stream_capsule_envelope(
    data_path: &Path,
    capsule_efk: envelope::EncryptedFileKey,
) -> Result<Option<warp::http::Response<Body>>, envelope::EnvelopeError> {
    let file_meta = tokio::fs::metadata(data_path)
        .await
        .map_err(|e| envelope::EnvelopeError::Kem(format!("metadata: {}", e)))?;
    let file_size = file_meta.len();

    let (header_prefix, header_size) = match envelope::read_envelope_header_prefix(data_path).await
    {
        Ok(h) => h,
        Err(_) => return Ok(None),
    };

    let (mut header, _) = envelope::deserialize_header(&header_prefix)?;
    header.encrypted_file_key = capsule_efk;
    header.kem_algorithm = "Kyber1024".to_string();

    let new_header =
        envelope::serialize_header(&header).map_err(envelope::EnvelopeError::HeaderSerialize)?;
    let data_len = file_size.saturating_sub(header_size as u64);
    let content_length = new_header.len() as u64 + data_len;

    let (tail, _) = streaming::file_stream(
        data_path,
        StreamingConfig::default(),
        Some(ByteRange {
            start: header_size as u64,
            end: None,
        }),
    )
    .await
    .map_err(|e| envelope::EnvelopeError::Kem(format!("file_stream: {}", e)))?;

    let body_stream = streaming::prepend_bytes(Bytes::from(new_header), tail);
    let body = Body::wrap_stream(body_stream);

    let resp = warp::http::Response::builder()
        .status(warp::http::StatusCode::OK)
        .header("content-type", "application/octet-stream")
        .header("x-envelope-version", "1")
        .header("x-delivery-mode", "e2e-capsule")
        .header("content-length", content_length.to_string())
        .body(body)
        .map_err(|e| envelope::EnvelopeError::Kem(format!("response build: {}", e)))?;

    tracing::info!(
        "Streaming E2E capsule envelope from {} ({} byte body)",
        data_path.display(),
        content_length
    );

    Ok(Some(resp))
}
