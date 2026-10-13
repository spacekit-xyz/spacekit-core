//! `spacekit plugin` — hash, sign, verify and publish SpaceKit plugin descriptors
//! (`spacekit-plugin.json`), and generate descriptors for Growformer brains.
//!
//! Byte-compatible with the web SDK (`@spacekit/sdk/plugins`: `descriptor.ts`,
//! `signature.ts`) and its `spacekit-plugin` command. Spec:
//! `sdks/spacekit-apps/SPACEKIT-PLUGIN-SPEC.md`.
//!
//! Descriptor signature (`spacekit-plugin.sig.json`): Ed25519 over the UTF-8 bytes of
//! `"SpaceKit plugin descriptor v1\n" + hex(sha256(<exact descriptor file bytes>))`, in the
//! same `{ v, alg, did, public_key, signature }` envelope as SPKG publisher signatures
//! (`crate::spkg_signature`), whose did:key and key-file helpers are reused here.

use std::io::{Read, Write};
use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};
use clap::Subcommand;
use colored::Colorize;
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::spkg_signature::{did_key_for, ed25519_key_matches_did, PublisherSignature};

use super::{resolve_effective_did, resolve_remote_storage_base_url, Cli, CliContext};

pub(crate) const PLUGIN_SIGNATURE_DOMAIN: &str = "SpaceKit plugin descriptor v1\n";
pub(crate) const DESCRIPTOR_FILE: &str = "spacekit-plugin.json";

/// Brain file roles (same as marketplace listing artifacts and `BRAIN_FILE_ROLES` in the SDK).
pub(crate) const BRAIN_FILE_ROLES: &[&str] = &[
    "bin",
    "inference_toml",
    "guardrails_jsonl",
    "fragments_jsonl",
    "topic_graph",
    "topic_graph_overlay",
    "grounding_toml",
    "companion_ui",
    // growformer-llm / growformer-system1 project files
    "label_model",
    "system1",
    "lm_checkpoint",
    "tokenizer",
];

const ID_RE: &str =
    r"^[a-z0-9](?:[a-z0-9-]{0,62}[a-z0-9])?(?:\.[a-z0-9](?:[a-z0-9-]{0,62}[a-z0-9])?){1,4}$";
const SEMVER_RE: &str = r"^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-[0-9A-Za-z.-]+)?$";
const RANGE_RE: &str = r"^[\^~]?(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-[0-9A-Za-z.-]+)?$";
const DID_RE: &str = r"^did:[a-z0-9]+:[^\s]+$";
const ENGINE_RE: &str = r"^[a-z0-9][a-z0-9-]{0,63}$";

const BRAIN_MAGIC: &[u8; 8] = b"GWFBRPKG";
const COMPRESSED_MAGIC: &[u8; 8] = b"GWFCMPKG";
const MAX_HEADER_BYTES: u64 = 16 * 1024 * 1024;
const PUBLIC_INDEX_DID: &str = "did:spacekit:admin:website-api";

fn re_match(pattern: &str, s: &str) -> bool {
    regex::Regex::new(pattern).map(|r| r.is_match(s)).unwrap_or(false)
}

pub(crate) fn is_plugin_id(s: &str) -> bool {
    re_match(ID_RE, s)
}

#[derive(Subcommand, Debug)]
pub(crate) enum PluginCommands {
    /// Print `files` entries (path, role, sha256, size) for a descriptor: `hash bin=pets.bin inference_toml=inference.toml`
    Hash {
        /// `<role>=<file>` pairs
        #[arg(value_name = "ROLE=FILE", required = true)]
        files: Vec<String>,
    },

    /// Sign a descriptor with an Ed25519 seed; writes the `spacekit-plugin.sig.json` envelope
    Sign {
        /// Descriptor file
        #[arg(long, default_value = DESCRIPTOR_FILE)]
        descriptor: String,
        /// File holding the 32-byte Ed25519 seed as hex (same as `app package --sign-key`)
        #[arg(long)]
        key: String,
        /// Signature output (default: `<descriptor minus .json>.sig.json`)
        #[arg(long)]
        out: Option<String>,
    },

    /// Verify a descriptor signature and compare the signer with `publisher` (exit 1 when invalid)
    Verify {
        #[arg(long, default_value = DESCRIPTOR_FILE)]
        descriptor: String,
        /// Signature file (default: `<descriptor minus .json>.sig.json`)
        #[arg(long)]
        sig: Option<String>,
    },

    /// Generate a `kind: brain` descriptor from a Growformer brain package and its sidecar files
    BrainDescriptor {
        /// Brain package (.bin; GWFBRPKG, GWFCMPKG or legacy checkpoint). Optional with --project.
        #[arg(value_name = "BRAIN_BIN")]
        brain: Option<String>,
        /// Growformer project manifest (`*.gf.toml`): takes the brain, inference rules,
        /// guardrails, topic graph (+ sentiment overlay) and grounding paths, and the
        /// project name and description, from it. Explicit flags win.
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        inference_toml: Option<String>,
        /// Accepted and pinned, but the browser Growformer runtime does not load guardrails
        #[arg(long)]
        guardrails_jsonl: Option<String>,
        #[arg(long)]
        fragments_jsonl: Option<String>,
        #[arg(long)]
        topic_graph: Option<String>,
        #[arg(long)]
        topic_graph_overlay: Option<String>,
        #[arg(long)]
        grounding_toml: Option<String>,
        /// Publisher DID (should be the did:key of the signing key)
        #[arg(long)]
        publisher: String,
        /// Plugin id (default: `<publisher handle or "brain">.<brain header id>`)
        #[arg(long)]
        id: Option<String>,
        /// Display name (default: brain header name)
        #[arg(long)]
        name: Option<String>,
        /// Plugin version (default: brain header version; legacy files: 1.0.0)
        #[arg(long)]
        plugin_version: Option<String>,
        /// One plain sentence (default: brain header description)
        #[arg(long)]
        summary: Option<String>,
        /// Longer description (default: brain header description)
        #[arg(long)]
        description: Option<String>,
        #[arg(long, default_value = "growformer")]
        engine: String,
        /// Engine version or ^/~ range, e.g. ^0.4.0
        #[arg(long)]
        engine_version: Option<String>,
        /// Price in ASTRA (one-time); omit or "free" for free
        #[arg(long)]
        price_astra: Option<String>,
        /// SPDX identifier or "proprietary"
        #[arg(long)]
        license: Option<String>,
        /// Where it runs: browser, node, compute-node
        #[arg(long, value_delimiter = ',', default_value = "browser")]
        runs_on: Vec<String>,
        /// generation, converse, codegen
        #[arg(long, value_delimiter = ',', default_value = "converse")]
        abilities: Vec<String>,
        /// What the brain was trained on (shown on install screens)
        #[arg(long)]
        training_summary: Option<String>,
        /// Set when the brain was trained on conversations captured through Growformer capture
        #[arg(long)]
        captured_conversations: bool,
        #[arg(long, default_value = DESCRIPTOR_FILE)]
        out: String,
    },

    /// Attach a descriptor (+ signature) to an existing marketplace listing (`app_listings.plugin`)
    Publish {
        /// Listing app_id
        #[arg(long)]
        app_id: String,
        #[arg(long, default_value = DESCRIPTOR_FILE)]
        descriptor: String,
        /// Signature file (default: `<descriptor minus .json>.sig.json` if it exists)
        #[arg(long)]
        sig: Option<String>,
        /// Storage node base URL (default: `spacekit connect storage`)
        #[arg(long)]
        storage_url: Option<String>,
    },
}

// ── File entries ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PluginFile {
    pub path: String,
    pub role: String,
    pub sha256: String,
    pub size: u64,
}

fn basename(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(path)
        .to_string()
}

/// SHA-256 and size of a file, streamed. `path` in the entry is the file's basename.
pub(crate) fn hash_file(role: &str, path: &str) -> Result<PluginFile> {
    let mut f = std::fs::File::open(path).with_context(|| format!("open {path}"))?;
    let mut hasher = Sha256::new();
    let size = std::io::copy(&mut f, &mut hasher).with_context(|| format!("read {path}"))?;
    Ok(PluginFile {
        path: basename(path),
        role: role.to_string(),
        sha256: hex::encode(hasher.finalize()),
        size,
    })
}

fn parse_role_file(arg: &str) -> Result<(String, String)> {
    match arg.find('=') {
        Some(i) if i > 0 && i + 1 < arg.len() => Ok((arg[..i].to_string(), arg[i + 1..].to_string())),
        _ => bail!("expected <role>=<file>, got {arg}"),
    }
}

/// Pretty JSON with 2-space indent and a trailing newline (same as the SDK CLI).
pub(crate) fn pretty<T: Serialize>(v: &T) -> Result<String> {
    Ok(format!("{}\n", serde_json::to_string_pretty(v)?))
}

// ── Signatures ───────────────────────────────────────────────────────────────

/// The exact bytes a publisher signs for a descriptor.
pub(crate) fn signing_message(descriptor_bytes: &[u8]) -> Vec<u8> {
    format!("{PLUGIN_SIGNATURE_DOMAIN}{}", hex::encode(Sha256::digest(descriptor_bytes))).into_bytes()
}

pub(crate) fn sign_descriptor(descriptor_bytes: &[u8], key: &SigningKey) -> PublisherSignature {
    let public_key = key.verifying_key().to_bytes();
    let signature = key.sign(&signing_message(descriptor_bytes));
    PublisherSignature {
        v: 1,
        alg: "ed25519".into(),
        did: did_key_for(&public_key),
        public_key: hex::encode(public_key),
        signature: hex::encode(signature.to_bytes()),
    }
}

/// Verify a descriptor signature envelope; returns the signer DID (bound to the key).
pub(crate) fn verify_descriptor_signature(descriptor_bytes: &[u8], sig_json: &[u8]) -> Result<String> {
    let sig: Value = serde_json::from_slice(sig_json).context("signature is not JSON")?;
    let field = |k: &str| sig.get(k).and_then(|v| v.as_str()).map(str::to_string);
    if sig.get("v").and_then(|v| v.as_u64()) != Some(1) {
        bail!("unsupported version {}", sig.get("v").cloned().unwrap_or(Value::Null));
    }
    if !field("alg").map(|a| a.eq_ignore_ascii_case("ed25519")).unwrap_or(false) {
        bail!("unsupported algorithm {}", sig.get("alg").cloned().unwrap_or(Value::Null));
    }
    let did = field("did").ok_or_else(|| anyhow!("malformed signature"))?;
    let pk = field("public_key").and_then(|h| hex::decode(h.trim()).ok()).filter(|b| b.len() == 32);
    let raw = field("signature").and_then(|h| hex::decode(h.trim()).ok()).filter(|b| b.len() == 64);
    let (Some(pk), Some(raw)) = (pk, raw) else {
        bail!("malformed signature");
    };
    if !ed25519_key_matches_did(&did, &pk) {
        bail!("DID does not match the public key");
    }
    let pk: [u8; 32] = pk.try_into().expect("32 bytes");
    let raw: [u8; 64] = raw.try_into().expect("64 bytes");
    VerifyingKey::from_bytes(&pk)
        .map_err(|_| anyhow!("invalid public key"))?
        .verify_strict(&signing_message(descriptor_bytes), &ed25519_dalek::Signature::from_bytes(&raw))
        .map_err(|_| anyhow!("signature does not match"))?;
    Ok(did)
}

/// Minimal descriptor checks done before signing or publishing.
pub(crate) fn check_descriptor_minimal(bytes: &[u8]) -> Result<Value> {
    let v: Value = serde_json::from_slice(bytes).context("descriptor is not valid JSON")?;
    let obj = v.as_object().ok_or_else(|| anyhow!("descriptor must be a JSON object"))?;
    let mut errors = Vec::new();
    if obj.get("spacekit_plugin").and_then(|x| x.as_u64()) != Some(1) {
        errors.push("spacekit_plugin must be 1".to_string());
    }
    for k in ["id", "name", "version", "kind", "publisher", "summary"] {
        if obj.get(k).and_then(|x| x.as_str()).map(|s| s.trim().is_empty()).unwrap_or(true) {
            errors.push(format!("{k} is required"));
        }
    }
    if !errors.is_empty() {
        bail!("invalid descriptor: {}", errors.join("; "));
    }
    Ok(v)
}

fn default_sig_path(descriptor: &str) -> String {
    format!("{}.sig.json", descriptor.strip_suffix(".json").unwrap_or(descriptor))
}

// ── Brain package header ─────────────────────────────────────────────────────

fn read_u32(r: &mut impl Read) -> Result<u32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b).context("brain package truncated")?;
    Ok(u32::from_le_bytes(b))
}

fn read_u64(r: &mut impl Read) -> Result<u64> {
    let mut b = [0u8; 8];
    r.read_exact(&mut b).context("brain package truncated")?;
    Ok(u64::from_le_bytes(b))
}

fn read_full_or_eof(r: &mut impl Read, buf: &mut [u8]) -> Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match r.read(&mut buf[n..])? {
            0 => break,
            k => n += k,
        }
    }
    Ok(n)
}

/// Read the `BrainPackageHeader` JSON of a Growformer brain (GWFBRPKG v1/v2, optionally inside
/// a gzip GWFCMPKG v1 envelope). `Ok(None)` for legacy files that carry no header. Only the
/// header is read/decompressed.
pub(crate) fn read_brain_header(r: &mut impl Read) -> Result<Option<Value>> {
    let mut magic = [0u8; 8];
    if read_full_or_eof(r, &mut magic)? < 8 {
        return Ok(None);
    }
    if &magic == COMPRESSED_MAGIC {
        let version = read_u32(r)?;
        if version != 1 {
            bail!("compressed brain package: unsupported version {version}");
        }
        let mut codec = [0u8; 4];
        r.read_exact(&mut codec).context("brain package truncated")?;
        if codec[0] != 1 {
            bail!("compressed brain package: unsupported codec {}", codec[0]);
        }
        let _raw_len = read_u64(r)?;
        let payload_len = read_u64(r)?;
        let mut inner = flate2::read::GzDecoder::new(r.take(payload_len));
        let mut inner_magic = [0u8; 8];
        inner.read_exact(&mut inner_magic).context("compressed brain package: bad payload")?;
        if &inner_magic != BRAIN_MAGIC {
            bail!("compressed brain package does not contain a GWFBRPKG package");
        }
        return read_brain_body(&mut inner).map(Some);
    }
    if &magic == BRAIN_MAGIC {
        return read_brain_body(r).map(Some);
    }
    Ok(None)
}

fn read_brain_body(r: &mut impl Read) -> Result<Value> {
    let format_version = read_u32(r)?;
    if format_version != 1 && format_version != 2 {
        bail!("brain package: unsupported format version {format_version} (expected 1 or 2)");
    }
    let header_len = read_u32(r)? as u64;
    let _ckpt_len = read_u32(r)?;
    if header_len > MAX_HEADER_BYTES {
        bail!("brain package: header is {header_len} bytes, too large");
    }
    let mut buf = vec![0u8; header_len as usize];
    r.read_exact(&mut buf).context("brain package header truncated")?;
    serde_json::from_slice(&buf).context("brain package header is not valid JSON")
}

pub(crate) fn read_brain_header_file(path: &str) -> Result<Option<Value>> {
    let f = std::fs::File::open(path).with_context(|| format!("open {path}"))?;
    read_brain_header(&mut std::io::BufReader::new(f))
}

fn semver_of(v: &Value) -> Option<String> {
    if let Some(s) = v.as_str() {
        return re_match(SEMVER_RE, s).then(|| s.to_string());
    }
    let n = |k: &str| v.get(k).and_then(|x| x.as_u64());
    Some(format!("{}.{}.{}", n("major")?, n("minor")?, n("patch")?))
}

// ── Brain descriptor ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub(crate) struct BrainDescriptorOptions {
    pub publisher: String,
    pub id: Option<String>,
    pub name: Option<String>,
    pub version: Option<String>,
    pub summary: Option<String>,
    pub description: Option<String>,
    pub engine: String,
    pub engine_version: Option<String>,
    pub price_astra: Option<String>,
    pub license: Option<String>,
    pub runs_on: Vec<String>,
    pub abilities: Vec<String>,
    pub training_summary: Option<String>,
    pub captured_conversations: bool,
    /// Used for the default id/name when the brain has no header (e.g. a listing app_id).
    pub fallback_name: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct Dependency {
    pub id: String,
    pub version: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct Training {
    pub summary: String,
    pub includes_captured_conversations: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct BrainSpec {
    pub engine: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engine_version: Option<String>,
    pub runs_on: Vec<String>,
    pub abilities: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base: Option<Dependency>,
    pub training: Training,
}

#[derive(Debug, Serialize)]
pub(crate) struct Pricing {
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount_wei: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct BrainDescriptor {
    pub spacekit_plugin: u8,
    pub id: String,
    pub name: String,
    pub version: String,
    pub kind: String,
    pub publisher: String,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    pub uses: Vec<String>,
    pub brain: BrainSpec,
    pub files: Vec<PluginFile>,
    pub pricing: Pricing,
}

/// Lower-case, `[a-z0-9-]` only, no leading/trailing/double dashes, at most 64 chars.
pub(crate) fn sanitize_segment(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars().flat_map(|c| c.to_lowercase()) {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let mut out: String = out.trim_matches('-').chars().take(64).collect();
    while out.ends_with('-') {
        out.pop();
    }
    out
}

/// A short handle from a publisher DID, or None (did:key and long ids give no readable handle).
pub(crate) fn publisher_handle(did: &str) -> Option<String> {
    if did.starts_with("did:key:") {
        return None;
    }
    let last = did.rsplit(':').next()?;
    let last = last.split('.').next().unwrap_or(last);
    let h = sanitize_segment(last);
    (!h.is_empty() && h.len() <= 32).then_some(h)
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut t: String = s.chars().take(max - 1).collect();
    t = t.trim_end().to_string();
    t.push('…');
    t
}

fn price_to_pricing(price: Option<&str>) -> Result<Pricing> {
    match price.map(str::trim) {
        None | Some("") | Some("free") | Some("0") => Ok(Pricing { model: "free".into(), amount_wei: None }),
        Some(p) => {
            let astra = p
                .parse::<f64>()
                .ok()
                .filter(|v| v.is_finite() && *v >= 0.0)
                .ok_or_else(|| anyhow!("price must be \"free\" or an amount of ASTRA, got {p}"))?;
            let wei = crate::content_monetization::astra_to_wei(astra);
            if wei == 0 {
                return Ok(Pricing { model: "free".into(), amount_wei: None });
            }
            Ok(Pricing { model: "one_time".into(), amount_wei: Some(wei.to_string()) })
        }
    }
}

/// Build a `kind: brain` descriptor. `header` is the brain's `BrainPackageHeader` JSON (None for
/// legacy files). `notes` collects things the user should know.
/// Paths and metadata from a Growformer project manifest (`*.gf.toml`, schema 1).
/// Paths are resolved relative to the manifest's directory, as Growformer does.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct GfProject {
    pub name: Option<String>,
    pub description: Option<String>,
    pub brain: Option<String>,
    pub inference_toml: Option<String>,
    pub guardrails_jsonl: Option<String>,
    pub fragments_jsonl: Option<String>,
    pub topic_graph: Option<String>,
    pub topic_graph_overlay: Option<String>,
    pub grounding_toml: Option<String>,
    /// `[system1].brain_output` (growformer-llm label model / decisions).
    pub system1: Option<String>,
    /// `agent/label-model.json`, written next to the brain by the tutorial's train script.
    pub label_model: Option<String>,
}

/// Growformer merges this overlay after the base topic graph when it sits next to it.
const GF_SENTIMENT_OVERLAY: &str = "knowledge_graph_sentiment_overlay.toml";

impl GfProject {
    pub(crate) fn load(path: &str) -> Result<Self> {
        let text = std::fs::read_to_string(path).with_context(|| format!("read {path}"))?;
        let dir = Path::new(path).parent().map(Path::to_path_buf).unwrap_or_default();
        Self::parse(&text, &dir)
    }

    pub(crate) fn parse(text: &str, dir: &Path) -> Result<Self> {
        let doc: toml::Value = toml::from_str(text).context("parse *.gf.toml")?;
        if let Some(v) = doc.get("schema_version").and_then(|v| v.as_integer()) {
            if v != 1 {
                bail!("unsupported *.gf.toml schema_version {v} (this CLI reads 1)");
            }
        }
        let get = |table: &str, key: &str| {
            doc.get(table)
                .and_then(|t| t.get(key))
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        };
        let resolve = |rel: Option<String>| {
            rel.map(|r| {
                let p = Path::new(&r);
                if p.is_absolute() { r } else { dir.join(p).to_string_lossy().into_owned() }
            })
        };
        let topic_graph = resolve(get("inference", "topic_graph"));
        let topic_graph_overlay = topic_graph.as_ref().and_then(|base| {
            let candidate = Path::new(base).parent()?.join(GF_SENTIMENT_OVERLAY);
            candidate.is_file().then(|| candidate.to_string_lossy().into_owned())
        });
        Ok(Self {
            name: get("project", "name"),
            description: get("project", "description"),
            brain: resolve(get("infer", "brain").or_else(|| get("train", "brain_output"))),
            inference_toml: resolve(get("inference", "toml")),
            guardrails_jsonl: resolve(get("inference", "guardrails_jsonl")),
            fragments_jsonl: resolve(get("inference", "fragments_jsonl")),
            topic_graph,
            topic_graph_overlay,
            grounding_toml: resolve(get("inference", "grounding_toml")),
            system1: resolve(get("system1", "brain_output")),
            label_model: {
                let candidate = dir.join("agent").join("label-model.json");
                candidate.is_file().then(|| candidate.to_string_lossy().into_owned())
            },
        })
    }
}

pub(crate) fn build_brain_descriptor(
    header: Option<&Value>,
    files: Vec<PluginFile>,
    opts: &BrainDescriptorOptions,
    notes: &mut Vec<String>,
) -> Result<BrainDescriptor> {
    let hstr = |k: &str| {
        header
            .and_then(|h| h.get(k))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    let nonempty = |o: &Option<String>| o.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);

    let publisher = opts.publisher.trim().to_string();
    if !re_match(DID_RE, &publisher) {
        bail!("--publisher must be a DID, got {publisher:?}");
    }

    let id = match nonempty(&opts.id) {
        Some(id) => id,
        None => {
            let seg = hstr("id")
                .or_else(|| opts.fallback_name.clone())
                .map(|s| sanitize_segment(&s))
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow!("this brain has no header id: pass --id (e.g. acme.pets-brain)"))?;
            let handle = publisher_handle(&publisher).unwrap_or_else(|| "brain".into());
            format!("{handle}.{seg}")
        }
    };
    if !is_plugin_id(&id) {
        bail!("plugin id {id:?} is not lower-case dotted segments (e.g. acme.pets-brain): pass --id");
    }

    let name = nonempty(&opts.name)
        .or_else(|| hstr("name"))
        .or_else(|| opts.fallback_name.clone())
        .ok_or_else(|| anyhow!("pass --name (the brain has no header name)"))?;
    if name.chars().count() > 80 {
        notes.push("name shortened to 80 characters".into());
    }
    let name = truncate_chars(&name, 80);

    let version = match nonempty(&opts.version) {
        Some(v) => v,
        None => match header.and_then(|h| h.get("version")).and_then(semver_of) {
            Some(v) => v,
            None => {
                notes.push("no version in the brain header; using 1.0.0 (override with --plugin-version)".into());
                "1.0.0".into()
            }
        },
    };
    if !re_match(SEMVER_RE, &version) {
        bail!("version must be semantic (e.g. 1.0.0), got {version:?}");
    }

    let header_description = hstr("description");
    let summary = match nonempty(&opts.summary) {
        Some(s) => s,
        None if header.is_none() => bail!("this brain has no header: pass --summary (one plain sentence)"),
        None => header_description
            .as_deref()
            .and_then(|d| d.lines().map(str::trim).find(|l| !l.is_empty()))
            .map(str::to_string)
            .ok_or_else(|| anyhow!("the brain header has no description: pass --summary"))?,
    };
    if summary.chars().count() > 200 {
        notes.push("summary shortened to 200 characters".into());
    }
    let summary = truncate_chars(&summary, 200);
    let description = nonempty(&opts.description).or(header_description).filter(|d| *d != summary);

    let engine = opts.engine.trim().to_string();
    if !re_match(ENGINE_RE, &engine) {
        bail!("--engine must name the AI engine, e.g. growformer");
    }
    let engine_version = nonempty(&opts.engine_version);
    if let Some(ev) = &engine_version {
        if !re_match(RANGE_RE, ev) {
            bail!("--engine-version must be a version or ^/~ range, got {ev:?}");
        }
    }
    let clean_list = |l: &[String]| -> Vec<String> {
        l.iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
    };
    let mut runs_on = clean_list(&opts.runs_on);
    if runs_on.is_empty() {
        runs_on.push("browser".into());
    }
    if let Some(bad) = runs_on.iter().find(|r| !["browser", "node", "compute-node"].contains(&r.as_str())) {
        bail!("--runs-on: unknown target {bad:?} (browser, node, compute-node)");
    }
    let mut abilities = clean_list(&opts.abilities);
    if abilities.is_empty() {
        abilities.push("converse".into());
    }
    if let Some(bad) = abilities.iter().find(|a| !["generation", "converse", "codegen"].contains(&a.as_str())) {
        bail!("--abilities: unknown ability {bad:?} (generation, converse, codegen)");
    }

    let base = match header.and_then(|h| h.get("base_brain")).filter(|b| !b.is_null()) {
        None => None,
        Some(b) => {
            let bid = b.get("id").and_then(|v| v.as_str()).map(str::trim).unwrap_or("");
            let bver = b.get("version").and_then(semver_of);
            match bver {
                Some(bver) if is_plugin_id(bid) => Some(Dependency { id: bid.to_string(), version: bver }),
                _ => {
                    notes.push(format!(
                        "base brain {bid:?} is not a plugin id (or has no version); brain.base omitted"
                    ));
                    None
                }
            }
        }
    };

    if files.iter().filter(|f| f.role == "bin").count() != 1 {
        bail!("a brain needs exactly one file with role bin");
    }
    for role in BRAIN_FILE_ROLES {
        if files.iter().filter(|f| f.role == *role).count() > 1 {
            bail!("role {role} is listed twice");
        }
    }
    if files.iter().any(|f| f.role == "guardrails_jsonl") {
        notes.push("guardrails_jsonl is pinned, but the browser Growformer runtime does not load guardrails".into());
    }

    Ok(BrainDescriptor {
        spacekit_plugin: 1,
        id,
        name,
        version,
        kind: "brain".into(),
        publisher,
        summary,
        description,
        license: nonempty(&opts.license),
        uses: vec!["ai".into()],
        brain: BrainSpec {
            engine,
            engine_version,
            runs_on,
            abilities,
            base,
            training: Training {
                summary: nonempty(&opts.training_summary).unwrap_or_else(|| "Not specified".into()),
                includes_captured_conversations: opts.captured_conversations,
            },
        },
        files,
        pricing: price_to_pricing(opts.price_astra.as_deref())?,
    })
}

// ── Listing checks ───────────────────────────────────────────────────────────

/// For `kind: brain`, compare descriptor `files` with listing `artifacts` by role (and size when
/// the listing records it). Returns one line per mismatch.
pub(crate) fn listing_file_mismatches(descriptor: &Value, listing: &Value) -> Vec<String> {
    if descriptor.get("kind").and_then(|k| k.as_str()) != Some("brain") {
        return Vec::new();
    }
    let files: Vec<&Value> = descriptor.get("files").and_then(|f| f.as_array()).map(|a| a.iter().collect()).unwrap_or_default();
    let artifacts: Vec<&Value> = listing
        .get("artifacts")
        .and_then(|a| a.as_array())
        .map(|a| {
            a.iter()
                .filter(|x| x.get("role").and_then(|r| r.as_str()).map(|r| BRAIN_FILE_ROLES.contains(&r)).unwrap_or(false))
                .collect()
        })
        .unwrap_or_default();
    let role = |v: &Value| v.get("role").and_then(|r| r.as_str()).unwrap_or("").to_string();
    let mut out = Vec::new();
    for f in &files {
        let r = role(f);
        let size = f.get("size").and_then(|s| s.as_u64());
        match artifacts.iter().find(|a| role(a) == r) {
            None => out.push(format!("role {r}: in the descriptor but not in the listing artifacts")),
            Some(a) => {
                if let (Some(ls), Some(ds)) = (a.get("size_bytes").and_then(|s| s.as_u64()), size) {
                    if ls != ds {
                        out.push(format!("role {r}: descriptor size {ds} bytes, listing size_bytes {ls}"));
                    }
                }
            }
        }
    }
    for a in &artifacts {
        let r = role(a);
        if !files.iter().any(|f| role(f) == r) {
            out.push(format!("role {r}: in the listing artifacts but not in the descriptor"));
        }
    }
    out
}

// ── Command handlers ─────────────────────────────────────────────────────────

fn write_text(path: &str, text: &str) -> Result<()> {
    std::fs::write(path, text).with_context(|| format!("write {path}"))
}

pub(crate) async fn handle_plugin_command(
    cli: &Cli,
    ctx: Option<&CliContext>,
    cmd: &PluginCommands,
) -> Result<(), Box<dyn std::error::Error>> {
    // Plain one-line errors (anyhow's Debug form would print a backtrace).
    run_plugin_command(cli, ctx, cmd).await.map_err(|e| format!("{e:#}").into())
}

fn boxed(e: Box<dyn std::error::Error>) -> anyhow::Error {
    anyhow!("{e}")
}

async fn run_plugin_command(cli: &Cli, ctx: Option<&CliContext>, cmd: &PluginCommands) -> Result<()> {
    match cmd {
        PluginCommands::Hash { files } => {
            let mut entries = Vec::new();
            for arg in files {
                let (role, file) = parse_role_file(arg)?;
                entries.push(hash_file(&role, &file)?);
            }
            print!("{}", pretty(&serde_json::json!({ "files": entries }))?);
            Ok(())
        }
        PluginCommands::Sign { descriptor, key, out } => {
            let bytes = std::fs::read(descriptor).with_context(|| format!("read {descriptor}"))?;
            let parsed = check_descriptor_minimal(&bytes)?;
            let key = crate::spkg_signature::load_signing_key(Path::new(key))?;
            let sig = sign_descriptor(&bytes, &key);
            let publisher = parsed.get("publisher").and_then(|p| p.as_str()).unwrap_or("");
            if publisher != sig.did {
                println!(
                    "{}",
                    format!("warning: publisher is {publisher}, but this key is {}", sig.did).yellow()
                );
            }
            let out = out.clone().unwrap_or_else(|| default_sig_path(descriptor));
            write_text(&out, &pretty(&sig)?)?;
            println!("Signed {descriptor} as {} -> {}", sig.did.cyan(), out.green());
            Ok(())
        }
        PluginCommands::Verify { descriptor, sig } => {
            let bytes = std::fs::read(descriptor).with_context(|| format!("read {descriptor}"))?;
            let sig_path = sig.clone().unwrap_or_else(|| default_sig_path(descriptor));
            let sig_bytes = std::fs::read(&sig_path).with_context(|| format!("read {sig_path}"))?;
            let publisher = serde_json::from_slice::<Value>(&bytes)
                .ok()
                .and_then(|v| v.get("publisher").and_then(|p| p.as_str()).map(str::to_string));
            match verify_descriptor_signature(&bytes, &sig_bytes) {
                Ok(did) => {
                    println!("{} {descriptor}", "valid".green());
                    println!("signer:    {did}");
                    match publisher {
                        Some(p) if p == did => println!("publisher: {p} (matches signer)"),
                        Some(p) => println!("{}", format!("publisher: {p} (does NOT match signer)").yellow()),
                        None => println!("{}", "publisher: (missing)".yellow()),
                    }
                    Ok(())
                }
                Err(e) => {
                    println!("{} {descriptor}: {e}", "invalid".red());
                    std::process::exit(1);
                }
            }
        }
        PluginCommands::BrainDescriptor {
            brain,
            project,
            inference_toml,
            guardrails_jsonl,
            fragments_jsonl,
            topic_graph,
            topic_graph_overlay,
            grounding_toml,
            publisher,
            id,
            name,
            plugin_version,
            summary,
            description,
            engine,
            engine_version,
            price_astra,
            license,
            runs_on,
            abilities,
            training_summary,
            captured_conversations,
            out,
        } => {
            let project = project.as_deref().map(GfProject::load).transpose()?;
            let pick = |flag: &Option<String>, from_project: Option<&String>| flag.clone().or_else(|| from_project.cloned());
            let brain = pick(brain, project.as_ref().and_then(|p| p.brain.as_ref()))
                .ok_or_else(|| anyhow!("pass the brain .bin, or --project with [infer].brain or [train].brain_output"))?;
            let brain = &brain;
            let header = read_brain_header_file(brain)?;
            if header.is_none() {
                println!("note: {brain} has no GWFBRPKG header (legacy checkpoint)");
            }
            let mut files = vec![hash_file("bin", brain)?];
            let p = project.as_ref();
            for (role, path) in [
                ("inference_toml", pick(inference_toml, p.and_then(|p| p.inference_toml.as_ref()))),
                ("guardrails_jsonl", pick(guardrails_jsonl, p.and_then(|p| p.guardrails_jsonl.as_ref()))),
                ("fragments_jsonl", pick(fragments_jsonl, p.and_then(|p| p.fragments_jsonl.as_ref()))),
                ("topic_graph", pick(topic_graph, p.and_then(|p| p.topic_graph.as_ref()))),
                ("topic_graph_overlay", pick(topic_graph_overlay, p.and_then(|p| p.topic_graph_overlay.as_ref()))),
                ("grounding_toml", pick(grounding_toml, p.and_then(|p| p.grounding_toml.as_ref()))),
            ] {
                if let Some(path) = path {
                    files.push(hash_file(role, &path)?);
                }
            }
            // growformer-llm also uses the label model and the System 1 brain; the browser
            // engine does not, so they are only pinned for that engine.
            if engine == "growformer-llm" {
                for (role, path) in [
                    ("label_model", p.and_then(|p| p.label_model.clone())),
                    ("system1", p.and_then(|p| p.system1.clone()).filter(|f| Path::new(f).is_file())),
                ] {
                    if let Some(path) = path {
                        files.push(hash_file(role, &path)?);
                    }
                }
            }
            let name = name.clone().or_else(|| p.and_then(|p| p.name.clone()));
            let description = description.clone().or_else(|| p.and_then(|p| p.description.clone()));
            // Legacy files have no header name: default to the file name.
            let name = name.clone().or_else(|| {
                header.is_none().then(|| Path::new(brain).file_stem().and_then(|s| s.to_str()).unwrap_or("Brain").to_string())
            });
            let opts = BrainDescriptorOptions {
                publisher: publisher.clone(),
                id: id.clone(),
                name,
                version: plugin_version.clone(),
                summary: summary.clone(),
                description,
                engine: engine.clone(),
                engine_version: engine_version.clone(),
                price_astra: price_astra.clone(),
                license: license.clone(),
                runs_on: runs_on.clone(),
                abilities: abilities.clone(),
                training_summary: training_summary.clone(),
                captured_conversations: *captured_conversations,
                fallback_name: None,
            };
            let mut notes = Vec::new();
            let d = build_brain_descriptor(header.as_ref(), files, &opts, &mut notes)?;
            for n in &notes {
                println!("note: {n}");
            }
            write_text(out, &pretty(&d)?)?;
            println!("Wrote {} ({} {} v{}, {} files)", out.green(), d.kind, d.id.cyan(), d.version, d.files.len());
            println!("Next: spacekit plugin sign --descriptor {out} --key <seed-hex-file>");
            Ok(())
        }
        PluginCommands::Publish { app_id, descriptor, sig, storage_url } => {
            let ctx = ctx.ok_or_else(|| anyhow!("No SpaceKit config. Run `spacekit init` first."))?;
            let owner_did = resolve_effective_did(cli, ctx, None).map_err(boxed)?;
            let descriptor_text =
                std::fs::read_to_string(descriptor).with_context(|| format!("read {descriptor}"))?;
            let parsed = check_descriptor_minimal(descriptor_text.as_bytes())?;
            let sig_text = match sig {
                Some(p) => Some(std::fs::read_to_string(p).with_context(|| format!("read {p}"))?),
                None => {
                    let p = default_sig_path(descriptor);
                    if Path::new(&p).is_file() {
                        Some(std::fs::read_to_string(&p).with_context(|| format!("read {p}"))?)
                    } else {
                        println!("{}", format!("warning: no {p}; publishing an unsigned descriptor").yellow());
                        None
                    }
                }
            };
            if let Some(s) = &sig_text {
                let did = verify_descriptor_signature(descriptor_text.as_bytes(), s.as_bytes())
                    .map_err(|e| anyhow!("signature is invalid: {e}"))?;
                let publisher = parsed.get("publisher").and_then(|p| p.as_str()).unwrap_or("");
                if publisher != did {
                    println!("{}", format!("warning: publisher is {publisher}, but the signer is {did}").yellow());
                }
            }

            let (base_url, _) = resolve_remote_storage_base_url(storage_url.as_deref()).map_err(boxed)?;
            let listing_url = format!("{}/api/documents/app_listings/{}", base_url.trim_end_matches('/'), app_id);
            let client = reqwest::Client::new();
            let resp = client
                .get(&listing_url)
                .header("Authorization", format!("DID {}", owner_did))
                .send()
                .await?;
            if resp.status() == reqwest::StatusCode::NOT_FOUND {
                bail!("no app_listings/{app_id}: publish the listing first");
            }
            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                bail!("Failed to fetch app_listings/{app_id} (HTTP {status}): {body}");
            }
            let mut listing: Value = resp.json().await?;
            let mismatches = listing_file_mismatches(&parsed, &listing);
            if !mismatches.is_empty() {
                for m in &mismatches {
                    println!("{} {m}", "mismatch:".red());
                }
                bail!("descriptor files do not match the listing artifacts; not publishing");
            }
            let obj = listing.as_object_mut().ok_or_else(|| anyhow!("app listing is not a JSON object"))?;
            let mut plugin = serde_json::Map::new();
            plugin.insert("descriptor".into(), Value::String(descriptor_text));
            if let Some(s) = sig_text {
                plugin.insert("signature".into(), Value::String(s));
            }
            obj.insert("plugin".into(), Value::Object(plugin));
            obj.insert("updated_at".into(), Value::String(chrono::Utc::now().to_rfc3339()));
            let body = serde_json::to_string_pretty(&listing)?;
            println!("📱 Attaching plugin descriptor to {}", listing_url.dimmed());
            for (did, label) in [(owner_did.as_str(), "owner"), (PUBLIC_INDEX_DID, "api-index")] {
                let resp = client
                    .put(&listing_url)
                    .header("Authorization", format!("DID {}", did))
                    .header("content-type", "application/json")
                    .body(body.clone())
                    .send()
                    .await?;
                if resp.status().is_success() {
                    println!("   ✅ Listing updated [{label}] (app_id: {})", app_id.cyan());
                } else {
                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_default();
                    if label == "owner" {
                        bail!("Failed to update listing (HTTP {status}): {text}");
                    }
                    eprintln!("   ⚠️  Failed to update listing [{label}] (HTTP {status}): {text}");
                }
            }
            Ok(())
        }
    }
}

/// Used by `storage deploy --plugin-key`: builds, serializes and signs a brain descriptor whose
/// file paths are the uploaded storage file ids. Returns `{ descriptor, signature }` texts.
pub(crate) fn build_signed_listing_plugin(
    header: Option<&Value>,
    files: Vec<PluginFile>,
    opts: &BrainDescriptorOptions,
    key: &SigningKey,
) -> Result<Value> {
    let mut notes = Vec::new();
    let d = build_brain_descriptor(header, files, opts, &mut notes)?;
    for n in &notes {
        println!("   note: {n}");
    }
    let text = pretty(&d)?;
    let sig = sign_descriptor(text.as_bytes(), key);
    if sig.did != d.publisher {
        println!(
            "   {}",
            format!("warning: plugin publisher is {}, but the key is {}", d.publisher, sig.did).yellow()
        );
    }
    println!("   🔏 Plugin descriptor {} v{} signed by {}", d.id.cyan(), d.version, sig.did.cyan());
    Ok(serde_json::json!({ "descriptor": text, "signature": pretty(&sig)? }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{write::GzEncoder, Compression};

    fn header_json() -> Value {
        serde_json::json!({
            "header_schema": 1,
            "id": "Pets_Brain",
            "version": { "major": 1, "minor": 2, "patch": 3 },
            "name": "Pets brain",
            "description": "Answers questions about pet care.\nMore detail.",
            "author": { "name": "Acme", "contact": null },
            "base_brain": { "id": "spacekit.growformer-base", "version": { "major": 1, "minor": 0, "patch": 0 } },
            "topics": [],
            "signature": {},
            "merged_from": [],
            "inference_profile": null
        })
    }

    fn brain_v2() -> Vec<u8> {
        let header = serde_json::to_vec(&header_json()).unwrap();
        let ckpt = b"{\"weights\":[1,2,3]}";
        let mut out = Vec::new();
        out.extend_from_slice(BRAIN_MAGIC);
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&(header.len() as u32).to_le_bytes());
        out.extend_from_slice(&(ckpt.len() as u32).to_le_bytes());
        out.extend_from_slice(&header);
        out.extend_from_slice(ckpt);
        out.extend_from_slice(&0u32.to_le_bytes()); // personality
        out.extend_from_slice(&0u32.to_le_bytes()); // plugins
        out
    }

    fn compress(data: &[u8]) -> Vec<u8> {
        let mut enc = GzEncoder::new(Vec::new(), Compression::default());
        enc.write_all(data).unwrap();
        let payload = enc.finish().unwrap();
        let mut out = Vec::new();
        out.extend_from_slice(COMPRESSED_MAGIC);
        out.extend_from_slice(&1u32.to_le_bytes());
        out.push(1);
        out.extend_from_slice(&[0, 0, 0]);
        out.extend_from_slice(&(data.len() as u64).to_le_bytes());
        out.extend_from_slice(&(payload.len() as u64).to_le_bytes());
        out.extend_from_slice(&payload);
        out
    }

    fn opts() -> BrainDescriptorOptions {
        BrainDescriptorOptions {
            publisher: "did:key:z6MkiTBz1ymuepAQ4HEHYSF1H8quG5GLVVQR3djdX3mDooWp".into(),
            engine: "growformer".into(),
            price_astra: Some("5".into()),
            license: Some("CC-BY-4.0".into()),
            ..Default::default()
        }
    }

    fn bin_file() -> PluginFile {
        PluginFile { path: "pets.bin".into(), role: "bin".into(), sha256: "00".repeat(32), size: 10 }
    }

    #[test]
    fn did_key_known_vector() {
        let key = SigningKey::from_bytes(&[0u8; 32]);
        let pk = key.verifying_key().to_bytes();
        assert_eq!(hex::encode(pk), "3b6a27bcceb6a42d62a3a8d02a6f0d73653215771de243a63ac048a18b59da29");
        assert_eq!(did_key_for(&pk), "did:key:z6MkiTBz1ymuepAQ4HEHYSF1H8quG5GLVVQR3djdX3mDooWp");
    }

    #[test]
    fn reads_v2_and_compressed_headers() {
        let raw = brain_v2();
        let h = read_brain_header(&mut raw.as_slice()).unwrap().unwrap();
        assert_eq!(h["id"], "Pets_Brain");
        let gz = compress(&raw);
        let h2 = read_brain_header(&mut gz.as_slice()).unwrap().unwrap();
        assert_eq!(h, h2);
        assert!(read_brain_header(&mut &b"{\"legacy\":true}"[..]).unwrap().is_none());
        assert!(read_brain_header(&mut &b"abc"[..]).unwrap().is_none());
    }

    #[test]
    fn builds_brain_descriptor_from_header() {
        let raw = brain_v2();
        let h = read_brain_header(&mut compress(&raw).as_slice()).unwrap().unwrap();
        let mut notes = Vec::new();
        let d = build_brain_descriptor(Some(&h), vec![bin_file()], &opts(), &mut notes).unwrap();
        assert_eq!(d.id, "brain.pets-brain");
        assert!(is_plugin_id(&d.id));
        assert_eq!(d.name, "Pets brain");
        assert_eq!(d.version, "1.2.3");
        assert_eq!(d.summary, "Answers questions about pet care.");
        assert_eq!(d.brain.base.as_ref().unwrap().id, "spacekit.growformer-base");
        assert_eq!(d.brain.base.as_ref().unwrap().version, "1.0.0");
        assert_eq!(d.brain.runs_on, vec!["browser"]);
        assert_eq!(d.brain.abilities, vec!["converse"]);
        assert_eq!(d.pricing.model, "one_time");
        assert_eq!(d.pricing.amount_wei.as_deref(), Some("5000000000000000000"));
        let text = pretty(&d).unwrap();
        assert!(text.ends_with("}\n"));
        assert!(text.starts_with("{\n  \"spacekit_plugin\": 1,\n  \"id\""));
        check_descriptor_minimal(text.as_bytes()).unwrap();
    }

    #[test]
    fn legacy_brain_requires_id_and_summary() {
        let mut notes = Vec::new();
        let mut o = opts();
        o.name = Some("Old".into());
        assert!(build_brain_descriptor(None, vec![bin_file()], &o, &mut notes).is_err());
        o.id = Some("acme.old-brain".into());
        assert!(build_brain_descriptor(None, vec![bin_file()], &o, &mut notes).is_err());
        o.summary = Some("An old brain.".into());
        let d = build_brain_descriptor(None, vec![bin_file()], &o, &mut notes).unwrap();
        assert_eq!(d.version, "1.0.0");
        o.id = Some("Bad Id".into());
        assert!(build_brain_descriptor(None, vec![bin_file()], &o, &mut notes).is_err());
    }

    #[test]
    fn non_plugin_base_is_omitted() {
        let mut h = header_json();
        h["base_brain"]["id"] = "Some Base".into();
        let mut notes = Vec::new();
        let d = build_brain_descriptor(Some(&h), vec![bin_file()], &opts(), &mut notes).unwrap();
        assert!(d.brain.base.is_none());
        assert!(notes.iter().any(|n| n.contains("brain.base omitted")));
    }

    #[test]
    fn sha256_file_entries() {
        let dir = std::env::temp_dir().join(format!("skplug-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("inference.toml");
        std::fs::write(&p, b"abc").unwrap();
        let f = hash_file("inference_toml", p.to_str().unwrap()).unwrap();
        assert_eq!(f.path, "inference.toml");
        assert_eq!(f.size, 3);
        assert_eq!(f.sha256, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn sign_verify_round_trip() {
        let key = SigningKey::from_bytes(&[7u8; 32]);
        let descriptor = b"{\"spacekit_plugin\":1}\n";
        let sig = sign_descriptor(descriptor, &key);
        let text = pretty(&sig).unwrap();
        assert_eq!(verify_descriptor_signature(descriptor, text.as_bytes()).unwrap(), sig.did);
        assert!(verify_descriptor_signature(b"{\"spacekit_plugin\":2}\n", text.as_bytes()).is_err());
        let mut forged = sig.clone();
        forged.did = did_key_for(&SigningKey::from_bytes(&[8u8; 32]).verifying_key().to_bytes());
        assert!(verify_descriptor_signature(descriptor, pretty(&forged).unwrap().as_bytes()).is_err());
        // Package signatures use a different domain and must not verify as plugin signatures.
        let pkg = crate::spkg_signature::sign(descriptor, &key);
        assert!(verify_descriptor_signature(descriptor, pretty(&pkg).unwrap().as_bytes()).is_err());
    }

    #[test]
    fn listing_mismatch_detection() {
        let d = serde_json::json!({ "kind": "brain", "files": [
            { "path": "a", "role": "bin", "sha256": "", "size": 10 },
            { "path": "b", "role": "inference_toml", "sha256": "", "size": 3 } ] });
        let ok = serde_json::json!({ "artifacts": [
            { "role": "wasm", "file_id": "w" },
            { "role": "bin", "file_id": "a", "size_bytes": 10 },
            { "role": "inference_toml", "file_id": "b" } ] });
        assert!(listing_file_mismatches(&d, &ok).is_empty());
        let bad = serde_json::json!({ "artifacts": [
            { "role": "bin", "file_id": "a", "size_bytes": 11 },
            { "role": "topic_graph", "file_id": "t" } ] });
        assert_eq!(listing_file_mismatches(&d, &bad).len(), 3);
    }

    #[test]
    fn reads_growformer_project_manifest() {
        let dir = std::env::temp_dir().join(format!("gfproj-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("data")).unwrap();
        std::fs::write(dir.join("data").join(GF_SENTIMENT_OVERLAY), "").unwrap();
        let text = r#"
schema_version = 1
[project]
name = "Crypto DeFi Sentiment Analysis"
description = "Crypto sentiment micro-brain."
[train]
brain_output = "agent/crypto-brain.bin"
[inference]
toml = "data/inference_crypto.toml"
guardrails_jsonl = "data/inference_guardrails.jsonl"
topic_graph = "data/knowledge_graph.toml"
grounding_toml = "/abs/world_grounding.toml"
[infer]
brain = "agent/infer-brain.bin"
"#;
        let p = GfProject::parse(text, &dir).unwrap();
        let at = |rel: &str| dir.join(rel).to_string_lossy().into_owned();
        assert_eq!(p.name.as_deref(), Some("Crypto DeFi Sentiment Analysis"));
        assert_eq!(p.brain, Some(at("agent/infer-brain.bin")));
        assert_eq!(p.inference_toml, Some(at("data/inference_crypto.toml")));
        assert_eq!(p.guardrails_jsonl, Some(at("data/inference_guardrails.jsonl")));
        assert_eq!(p.topic_graph_overlay, Some(at(&format!("data/{GF_SENTIMENT_OVERLAY}"))));
        assert_eq!(p.grounding_toml.as_deref(), Some("/abs/world_grounding.toml"));
        assert!(p.fragments_jsonl.is_none());
        assert!(p.system1.is_none());
        assert!(p.label_model.is_none());
        let no_infer = GfProject::parse("[train]\nbrain_output = \"b.bin\"\n", &dir).unwrap();
        assert_eq!(no_infer.brain, Some(at("b.bin")));
        assert!(GfProject::parse("schema_version = 2\n", &dir).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn sanitizes_ids() {
        assert_eq!(sanitize_segment("My__Brain v2!"), "my-brain-v2");
        assert_eq!(publisher_handle("did:web:acme.example"), Some("acme".into()));
        assert_eq!(publisher_handle("did:key:z6Mk"), None);
    }
}
