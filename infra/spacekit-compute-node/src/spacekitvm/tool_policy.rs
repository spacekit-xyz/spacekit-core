//! SKTCS v0.1 — Tool Manifest types and Policy Gate.
//!
//! Parses the `spacekit:tools` WASM custom section and enforces parameter
//! validation, constraint checking, and DID-scoped key rewriting at the
//! host-import boundary.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── SKTCS error codes (returned to WASM as negative i32) ────────────────

pub const SKTCS_MISSING_PARAM: i32 = -10;
pub const SKTCS_INVALID_TYPE: i32 = -11;
pub const SKTCS_MAX_BYTES_EXCEEDED: i32 = -12;
pub const SKTCS_OUT_OF_RANGE: i32 = -13;
pub const SKTCS_INVALID_FORMAT: i32 = -14;
pub const SKTCS_MISSING_CALLER_DID: i32 = -15;
pub const SKTCS_RATE_LIMIT_EXCEEDED: i32 = -16;
pub const SKTCS_MAX_EFFECTS_EXCEEDED: i32 = -17;
pub const SKTCS_RECIPIENT_BLOCKED: i32 = -18;
pub const SKTCS_BENEFICIARY_MISMATCH: i32 = -19;
pub const SKTCS_VAULT_CHARGE_FAILED: i32 = -20;
pub const SKTCS_SIZE_LIMIT_EXCEEDED: i32 = -21;

// ── Manifest types ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolManifest {
    pub version: String,
    pub contract_id: String,
    pub tools: HashMap<String, ToolDef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDef {
    pub module: String,
    pub function: String,
    pub pattern: String,
    #[serde(default)]
    pub params: HashMap<String, ParamDef>,
    #[serde(default)]
    pub constraints: ConstraintDef,
    #[serde(default)]
    pub version: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParamDef {
    #[serde(rename = "type")]
    pub param_type: String,
    pub max_bytes: Option<u64>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    #[serde(default)]
    pub required: bool,
    pub default: Option<serde_json::Value>,
    pub sanitize: Option<String>,
    pub validate: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConstraintDef {
    pub cost: Option<String>,
    pub cost_unit: Option<String>,
    pub rate_limit: Option<String>,
    pub max_effects_per_execution: Option<u32>,
    #[serde(default)]
    pub requires_caller_did: bool,
    pub storage_key_prefix: Option<String>,
    pub allowed_recipients: Option<Vec<String>>,
    pub blocked_recipients: Option<Vec<String>>,
    #[serde(default)]
    pub beneficiary_must_match_caller: bool,
    pub max_input_plus_output_bytes: Option<u64>,
}

/// Audit record emitted for every tool invocation (fulfilled or rejected).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolEffectRecord {
    pub tool_id: String,
    pub caller_did: String,
    pub params_hash: String,
    pub result_hash: Option<String>,
    pub cost_charged: String,
    pub timestamp: u64,
    pub effect_round: u32,
    pub status: String,
    pub reason: Option<String>,
}

// ── Manifest extraction from WASM ───────────────────────────────────────

const CUSTOM_SECTION_NAME: &[u8] = b"spacekit:tools";

/// Extract and parse the SKTCS tool manifest from raw WASM bytes.
///
/// Wasmtime 25 does not expose `Module::custom_sections()`, so we parse the
/// binary directly. Custom sections have id=0 in the WASM binary format:
/// `0x00 <name_len:u32> <name:utf8> <data_len:u32> <data:bytes>`.
///
/// Returns `None` for legacy contracts without a manifest.
pub fn parse_manifest_from_wasm(wasm_bytes: &[u8]) -> Option<ToolManifest> {
    extract_custom_section(wasm_bytes, CUSTOM_SECTION_NAME).and_then(|data| {
        match serde_json::from_slice::<ToolManifest>(data) {
            Ok(manifest) => Some(manifest),
            Err(e) => {
                log::warn!("Failed to parse SKTCS manifest: {}", e);
                None
            }
        }
    })
}

/// Low-level WASM custom section extractor.
/// Scans a WASM binary for a custom section (section id = 0) with the given name.
fn extract_custom_section<'a>(wasm: &'a [u8], section_name: &[u8]) -> Option<&'a [u8]> {
    if wasm.len() < 8 || &wasm[0..4] != b"\x00asm" {
        return None;
    }
    let mut offset = 8; // skip magic + version
    while offset < wasm.len() {
        let section_id = wasm[offset];
        offset += 1;
        let (section_len, consumed) = read_leb128_u32(&wasm[offset..])?;
        offset += consumed;
        let section_end = offset + section_len as usize;
        if section_end > wasm.len() {
            return None;
        }

        if section_id == 0 {
            // Custom section: first field is the name (LEB128 length + UTF-8 bytes)
            let (name_len, name_consumed) = read_leb128_u32(&wasm[offset..])?;
            let name_start = offset + name_consumed;
            let name_end = name_start + name_len as usize;
            if name_end > section_end {
                offset = section_end;
                continue;
            }
            if &wasm[name_start..name_end] == section_name {
                return Some(&wasm[name_end..section_end]);
            }
        }
        offset = section_end;
    }
    None
}

fn read_leb128_u32(bytes: &[u8]) -> Option<(u32, usize)> {
    let mut result: u32 = 0;
    let mut shift = 0;
    for (i, &byte) in bytes.iter().enumerate() {
        result |= ((byte & 0x7F) as u32) << shift;
        if byte & 0x80 == 0 {
            return Some((result, i + 1));
        }
        shift += 7;
        if shift >= 35 {
            return None;
        }
    }
    None
}

// ── Constraint state ────────────────────────────────────────────────────

#[derive(Debug, Default)]
pub struct ConstraintState {
    pub effect_counts: HashMap<String, u32>,
    pub rate_state: HashMap<String, RateEntry>,
}

#[derive(Debug)]
pub struct RateEntry {
    pub count: u32,
    pub window_start_ms: u64,
}

impl ConstraintState {
    pub fn new() -> Self {
        Self::default()
    }
}

// ── Parameter validation ────────────────────────────────────────────────

pub fn validate_tool_params(
    tool_def: &ToolDef,
    actual_params: &HashMap<String, serde_json::Value>,
) -> Result<(), (i32, String)> {
    for (name, def) in &tool_def.params {
        let value = actual_params.get(name).or(def.default.as_ref());

        match value {
            None | Some(serde_json::Value::Null) => {
                if def.required {
                    return Err((
                        SKTCS_MISSING_PARAM,
                        format!("required param \"{}\" missing", name),
                    ));
                }
                continue;
            }
            Some(v) => {
                check_param_type(name, &def.param_type, v)?;

                if let Some(max_bytes) = def.max_bytes {
                    let byte_len = json_byte_len(v);
                    if byte_len > max_bytes {
                        return Err((
                            SKTCS_MAX_BYTES_EXCEEDED,
                            format!(
                                "param \"{}\" exceeds max_bytes ({} > {})",
                                name, byte_len, max_bytes
                            ),
                        ));
                    }
                }

                if def.min.is_some() || def.max.is_some() {
                    if let Some(num) = v.as_f64() {
                        if let Some(min) = def.min {
                            if num < min {
                                return Err((
                                    SKTCS_OUT_OF_RANGE,
                                    format!("param \"{}\" below min ({} < {})", name, num, min),
                                ));
                            }
                        }
                        if let Some(max) = def.max {
                            if num > max {
                                return Err((
                                    SKTCS_OUT_OF_RANGE,
                                    format!("param \"{}\" above max ({} > {})", name, num, max),
                                ));
                            }
                        }
                    }
                }

                if let Some(ref validate_mode) = def.validate {
                    if validate_mode != "none" {
                        check_format(name, validate_mode, v)?;
                    }
                }
            }
        }
    }
    Ok(())
}

fn check_param_type(
    name: &str,
    expected: &str,
    value: &serde_json::Value,
) -> Result<(), (i32, String)> {
    let ok = match expected {
        "string" | "did" => value.is_string(),
        "u32" | "u64" => value.is_number(),
        "bool" => value.is_boolean(),
        "bytes" => value.is_string(), // base64 or hex encoded
        _ => true,
    };
    if !ok {
        return Err((
            SKTCS_INVALID_TYPE,
            format!("param \"{}\" type mismatch (expected {})", name, expected),
        ));
    }
    Ok(())
}

fn check_format(name: &str, mode: &str, value: &serde_json::Value) -> Result<(), (i32, String)> {
    let s = value.as_str().unwrap_or("");
    match mode {
        "did_format" => {
            if !s.starts_with("did:") {
                return Err((
                    SKTCS_INVALID_FORMAT,
                    format!("param \"{}\" is not a valid DID", name),
                ));
            }
        }
        "numeric_string" => {
            if s.parse::<f64>().is_err() {
                return Err((
                    SKTCS_INVALID_FORMAT,
                    format!("param \"{}\" is not a numeric string", name),
                ));
            }
        }
        _ => {}
    }
    Ok(())
}

fn json_byte_len(v: &serde_json::Value) -> u64 {
    match v {
        serde_json::Value::String(s) => s.len() as u64,
        _ => serde_json::to_string(v)
            .map(|s| s.len() as u64)
            .unwrap_or(0),
    }
}

// ── Constraint checking ─────────────────────────────────────────────────

pub fn check_constraints(
    tool_name: &str,
    tool_def: &ToolDef,
    caller_did: &str,
    state: &mut ConstraintState,
    actual_params: Option<&HashMap<String, serde_json::Value>>,
) -> Result<(), (i32, String)> {
    let c = &tool_def.constraints;

    if c.requires_caller_did
        && (caller_did.is_empty() || caller_did == "did:spacekit:browser:anonymous")
    {
        return Err((
            SKTCS_MISSING_CALLER_DID,
            "tool requires authenticated caller DID".into(),
        ));
    }

    if let Some(max) = c.max_effects_per_execution {
        let current = state.effect_counts.get(tool_name).copied().unwrap_or(0);
        if current >= max {
            return Err((
                SKTCS_MAX_EFFECTS_EXCEEDED,
                format!("tool \"{}\" max effects reached ({})", tool_name, max),
            ));
        }
    }

    if let Some(ref rate_str) = c.rate_limit {
        check_rate_limit(tool_name, caller_did, rate_str, state)?;
    }

    if let Some(ref blocked) = c.blocked_recipients {
        if let Some(params) = actual_params {
            if let Some(recipient) = extract_recipient(params) {
                if blocked.iter().any(|p| glob_match(p, &recipient)) {
                    return Err((
                        SKTCS_RECIPIENT_BLOCKED,
                        format!("recipient \"{}\" is blocked", recipient),
                    ));
                }
            }
        }
    }

    if let Some(ref allowed) = c.allowed_recipients {
        if let Some(params) = actual_params {
            if let Some(recipient) = extract_recipient(params) {
                if !allowed.iter().any(|p| glob_match(p, &recipient)) {
                    return Err((
                        SKTCS_RECIPIENT_BLOCKED,
                        format!("recipient \"{}\" not in allowed list", recipient),
                    ));
                }
            }
        }
    }

    if c.beneficiary_must_match_caller {
        if let Some(params) = actual_params {
            let beneficiary = params.get("beneficiary").or_else(|| params.get("to"));
            if let Some(serde_json::Value::String(b)) = beneficiary {
                if b != caller_did {
                    return Err((
                        SKTCS_BENEFICIARY_MISMATCH,
                        format!(
                            "beneficiary \"{}\" does not match caller \"{}\"",
                            b, caller_did
                        ),
                    ));
                }
            }
        }
    }

    // Record the effect
    *state
        .effect_counts
        .entry(tool_name.to_string())
        .or_insert(0) += 1;

    Ok(())
}

fn check_rate_limit(
    tool_name: &str,
    caller_did: &str,
    rate_str: &str,
    state: &mut ConstraintState,
) -> Result<(), (i32, String)> {
    let (limit, window_ms) = match parse_rate_limit(rate_str) {
        Some(v) => v,
        None => return Ok(()),
    };

    let key = format!("{}:{}", caller_did, tool_name);
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let entry = state.rate_state.entry(key).or_insert(RateEntry {
        count: 0,
        window_start_ms: now_ms,
    });

    if now_ms - entry.window_start_ms > window_ms {
        entry.count = 1;
        entry.window_start_ms = now_ms;
        return Ok(());
    }

    if entry.count >= limit {
        return Err((
            SKTCS_RATE_LIMIT_EXCEEDED,
            format!("tool \"{}\" rate limit exceeded ({})", tool_name, rate_str),
        ));
    }

    entry.count += 1;
    Ok(())
}

fn parse_rate_limit(s: &str) -> Option<(u32, u64)> {
    let parts: Vec<&str> = s.splitn(2, '/').collect();
    if parts.len() != 2 {
        return None;
    }
    let limit: u32 = parts[0].parse().ok()?;
    let window_ms = match parts[1] {
        "sec" => 1_000,
        "min" => 60_000,
        "hour" => 3_600_000,
        _ => return None,
    };
    Some((limit, window_ms))
}

fn extract_recipient(params: &HashMap<String, serde_json::Value>) -> Option<String> {
    params
        .get("recipient")
        .or_else(|| params.get("recipientDid"))
        .or_else(|| params.get("to"))
        .and_then(|v| v.as_str())
        .map(String::from)
}

fn glob_match(pattern: &str, value: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if !pattern.contains('*') {
        return pattern == value;
    }
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.is_empty() {
        return true;
    }
    let mut remaining = value;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        if i == 0 {
            if !remaining.starts_with(part) {
                return false;
            }
            remaining = &remaining[part.len()..];
        } else if i == parts.len() - 1 {
            if !remaining.ends_with(part) {
                return false;
            }
            return true;
        } else {
            match remaining.find(part) {
                Some(pos) => remaining = &remaining[pos + part.len()..],
                None => return false,
            }
        }
    }
    true
}

// ── Storage key prefix (DID scoping) ────────────────────────────────────

pub fn rewrite_storage_key(key: &[u8], caller_did: &str, constraints: &ConstraintDef) -> Vec<u8> {
    match constraints.storage_key_prefix.as_deref() {
        Some("{caller_did}") => {
            let mut prefixed = format!("{}:", caller_did).into_bytes();
            prefixed.extend_from_slice(key);
            prefixed
        }
        Some(prefix) => {
            let mut prefixed = format!("{}:", prefix).into_bytes();
            prefixed.extend_from_slice(key);
            prefixed
        }
        None => key.to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_rate_limit_works() {
        assert_eq!(parse_rate_limit("20/min"), Some((20, 60_000)));
        assert_eq!(parse_rate_limit("5/sec"), Some((5, 1_000)));
        assert_eq!(parse_rate_limit("100/hour"), Some((100, 3_600_000)));
        assert_eq!(parse_rate_limit("bad"), None);
    }

    #[test]
    fn rewrite_storage_key_caller_did() {
        let c = ConstraintDef {
            storage_key_prefix: Some("{caller_did}".into()),
            ..Default::default()
        };
        let result = rewrite_storage_key(b"mykey", "did:spacekit:alice", &c);
        assert_eq!(result, b"did:spacekit:alice:mykey");
    }

    #[test]
    fn glob_match_works() {
        assert!(glob_match("*", "anything"));
        assert!(glob_match("did:spacekit:*", "did:spacekit:alice"));
        assert!(!glob_match("did:spacekit:bob", "did:spacekit:alice"));
    }

    #[test]
    fn validate_required_param() {
        let mut params_def = HashMap::new();
        params_def.insert(
            "query".to_string(),
            ParamDef {
                param_type: "string".into(),
                max_bytes: None,
                min: None,
                max: None,
                required: true,
                default: None,
                sanitize: None,
                validate: None,
            },
        );
        let tool = ToolDef {
            module: "test".into(),
            function: "test".into(),
            pattern: "effect_queue".into(),
            params: params_def,
            constraints: ConstraintDef::default(),
            version: None,
        };
        let empty: HashMap<String, serde_json::Value> = HashMap::new();
        assert!(validate_tool_params(&tool, &empty).is_err());
    }

    #[test]
    fn leb128_roundtrip() {
        assert_eq!(read_leb128_u32(&[0x00]), Some((0, 1)));
        assert_eq!(read_leb128_u32(&[0x7F]), Some((127, 1)));
        assert_eq!(read_leb128_u32(&[0x80, 0x01]), Some((128, 2)));
        assert_eq!(read_leb128_u32(&[0xE5, 0x8E, 0x26]), Some((624485, 3)));
    }

    #[test]
    fn extract_custom_section_from_minimal_wasm() {
        // Build a minimal WASM module with a custom section named "spacekit:tools"
        let manifest_json = br#"{"version":"0.1","contract_id":"test","tools":{}}"#;
        let section_name = b"spacekit:tools";

        let mut wasm = Vec::new();
        wasm.extend_from_slice(b"\x00asm"); // magic
        wasm.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // version 1

        // Custom section (id=0)
        let name_len = section_name.len();
        let payload_len = 1 + name_len + manifest_json.len(); // 1 byte for name length LEB128
        wasm.push(0x00); // section id = 0 (custom)
        write_leb128(&mut wasm, payload_len as u32);
        write_leb128(&mut wasm, name_len as u32);
        wasm.extend_from_slice(section_name);
        wasm.extend_from_slice(manifest_json);

        let manifest = parse_manifest_from_wasm(&wasm);
        assert!(manifest.is_some());
        let m = manifest.unwrap();
        assert_eq!(m.version, "0.1");
        assert_eq!(m.contract_id, "test");
    }

    fn write_leb128(out: &mut Vec<u8>, mut value: u32) {
        loop {
            let mut byte = (value & 0x7F) as u8;
            value >>= 7;
            if value != 0 {
                byte |= 0x80;
            }
            out.push(byte);
            if value == 0 {
                break;
            }
        }
    }
}
