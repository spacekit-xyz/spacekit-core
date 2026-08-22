//! Canonical intent signing and verification.
//!
//! An intent authorizes payments and contract execution on behalf of its
//! `actor`, so the signature must cover the whole intent. The previous scheme
//! signed only `intent_id` — a random 16-byte value — which meant a relay (or
//! anyone who observed an intent in flight) could keep the signature and
//! rewrite the actions, amounts, beneficiaries, and expiry.
//!
//! ## Canonical payload
//!
//! ```text
//! SPACEKIT-INTENT-v1\n
//! {version}\n
//! {intent_id}\n
//! {actor}\n
//! {agent}\n            (empty string when absent)
//! {chain}\n
//! {nonce}\n
//! {expiry}\n
//! {sha256_hex(canonical_json(actions))}\n
//! {sha256_hex(canonical_json(constraints))}
//! ```
//!
//! `canonical_json` is RFC 8785-style: object keys sorted by their Unicode
//! code points, no insignificant whitespace. Both this module and the
//! TypeScript SDK (`spacekit-js/src/intent_canonical.ts`) must produce
//! byte-identical output; the shared vectors in the tests of both files pin
//! that down.

use serde_json::Value;
use sha2::{Digest, Sha256};

pub const INTENT_DOMAIN: &str = "SPACEKIT-INTENT-v1";

/// Deterministic JSON encoding: sorted keys, no whitespace.
pub fn canonical_json(value: &Value) -> String {
    let mut out = String::new();
    write_canonical(value, &mut out);
    out
}

fn write_canonical(value: &Value, out: &mut String) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => out.push_str(&n.to_string()),
        Value::String(s) => write_json_string(s, out),
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_canonical(item, out);
            }
            out.push(']');
        }
        Value::Object(map) => {
            // Sort by UTF-16 code units to match JavaScript's `Array.sort()`
            // on the SDK side. For all-BMP keys this equals code-point order;
            // the difference only shows up with astral-plane characters.
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_by_key(|k| k.encode_utf16().collect::<Vec<u16>>());
            out.push('{');
            for (i, key) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_json_string(key, out);
                out.push(':');
                write_canonical(&map[*key], out);
            }
            out.push('}');
        }
    }
}

fn write_json_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn hash_field(value: Option<&Value>) -> String {
    let json = canonical_json(value.unwrap_or(&Value::Null));
    hex::encode(Sha256::digest(json.as_bytes()))
}

/// Build the exact bytes an actor must sign for `intent`.
///
/// `intent` is the raw JSON object, so a field the node does not understand
/// still contributes to the hash via `actions`/`constraints` rather than being
/// silently unsigned.
pub fn canonical_intent_payload(intent: &Value) -> Vec<u8> {
    let s = |key: &str| intent.get(key).and_then(Value::as_str).unwrap_or("");
    let expiry = intent.get("expiry").and_then(Value::as_i64).unwrap_or(0);

    format!(
        "{INTENT_DOMAIN}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
        s("version"),
        s("intent_id"),
        s("actor"),
        s("agent"),
        s("chain"),
        s("nonce"),
        expiry,
        hash_field(intent.get("actions")),
        hash_field(intent.get("constraints")),
    )
    .into_bytes()
}

#[derive(Debug, thiserror::Error)]
pub enum IntentAuthError {
    #[error("intent is missing the `{0}` field")]
    MissingField(&'static str),
    #[error("unsupported signature type `{0}`")]
    UnsupportedSigType(String),
    #[error("signature is not valid hex")]
    MalformedSignature,
    #[error("actor DID {0} is not registered on this node")]
    UnknownActor(String),
    #[error("intent signature verification failed")]
    BadSignature,
    #[error("intent expired at {expiry} (now {now})")]
    Expired { expiry: i64, now: i64 },
    #[error("intent expiry {expiry} is further than {max_secs}s in the future")]
    ExpiryTooFar { expiry: i64, max_secs: i64 },
}

/// Longest an intent may remain valid. An unbounded expiry turns a single
/// signature into a permanent authorization.
pub const MAX_INTENT_LIFETIME_SECS: i64 = 3600;

/// Verify a signed intent against the actor's registered SPHINCS+ key.
///
/// `resolve_key` maps an actor DID to its public key; returning `None` rejects
/// the intent rather than accepting it unverified.
pub fn verify_signed_intent(
    intent: &Value,
    signature_hex: &str,
    sig_type: &str,
    now: i64,
    resolve_key: impl FnOnce(&str) -> Option<Vec<u8>>,
) -> Result<String, IntentAuthError> {
    let actor = intent
        .get("actor")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or(IntentAuthError::MissingField("actor"))?;

    let expiry = intent
        .get("expiry")
        .and_then(Value::as_i64)
        .ok_or(IntentAuthError::MissingField("expiry"))?;

    if expiry <= now {
        return Err(IntentAuthError::Expired { expiry, now });
    }
    if expiry - now > MAX_INTENT_LIFETIME_SECS {
        return Err(IntentAuthError::ExpiryTooFar {
            expiry,
            max_secs: MAX_INTENT_LIFETIME_SECS,
        });
    }

    if !sig_type.eq_ignore_ascii_case("sphincs+") && !sig_type.eq_ignore_ascii_case("sphincs") {
        return Err(IntentAuthError::UnsupportedSigType(sig_type.to_string()));
    }

    let signature = hex::decode(signature_hex).map_err(|_| IntentAuthError::MalformedSignature)?;
    let public_key =
        resolve_key(actor).ok_or_else(|| IntentAuthError::UnknownActor(actor.to_string()))?;

    let payload = canonical_intent_payload(intent);
    if !spacekit_did::sphincs::SphincsPlus::verify(&public_key, &payload, &signature) {
        return Err(IntentAuthError::BadSignature);
    }

    Ok(actor.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn canonical_json_sorts_keys() {
        let v = json!({ "b": 1, "a": 2, "c": { "z": 1, "y": 2 } });
        assert_eq!(canonical_json(&v), r#"{"a":2,"b":1,"c":{"y":2,"z":1}}"#);
    }

    #[test]
    fn canonical_json_preserves_array_order() {
        let v = json!([3, 1, 2]);
        assert_eq!(canonical_json(&v), "[3,1,2]");
    }

    #[test]
    fn canonical_json_escapes_control_characters() {
        let v = json!({ "k": "a\nb\"c\\d\te" });
        assert_eq!(canonical_json(&v), r#"{"k":"a\nb\"c\\d\te"}"#);
    }

    /// Shared vector — `spacekit-js/src/intent_canonical.ts` asserts the same
    /// string. If this changes, change both.
    #[test]
    fn cross_language_canonical_vector() {
        let intent = json!({
            "version": "1.0",
            "intent_id": "0123456789abcdef0123456789abcdef",
            "actor": "did:spacekit:testnet:alice",
            "chain": "spacekit:testnet",
            "nonce": "1",
            "expiry": 1700000000_i64,
            "actions": [{ "type": "transfer", "to": "did:bob", "amount": "500" }],
            "constraints": { "max_fee_astra": "100" }
        });
        let payload = String::from_utf8(canonical_intent_payload(&intent)).unwrap();
        let lines: Vec<&str> = payload.split('\n').collect();

        assert_eq!(lines[0], "SPACEKIT-INTENT-v1");
        assert_eq!(lines[1], "1.0");
        assert_eq!(lines[2], "0123456789abcdef0123456789abcdef");
        assert_eq!(lines[3], "did:spacekit:testnet:alice");
        assert_eq!(
            lines[4], "",
            "absent `agent` must serialize as an empty line"
        );
        assert_eq!(lines[5], "spacekit:testnet");
        assert_eq!(lines[6], "1");
        assert_eq!(lines[7], "1700000000");

        // Keys inside actions and constraints are sorted before hashing.
        assert_eq!(
            lines[8],
            hex::encode(Sha256::digest(
                r#"[{"amount":"500","to":"did:bob","type":"transfer"}]"#.as_bytes()
            ))
        );
        assert_eq!(
            lines[9],
            hex::encode(Sha256::digest(r#"{"max_fee_astra":"100"}"#.as_bytes()))
        );
    }

    #[test]
    fn payload_changes_when_any_field_changes() {
        let base = json!({
            "version": "1.0", "intent_id": "a", "actor": "did:a", "chain": "c",
            "nonce": "1", "expiry": 100, "actions": [], "constraints": {}
        });
        let baseline = canonical_intent_payload(&base);

        for (field, replacement) in [
            ("actor", json!("did:mallory")),
            ("nonce", json!("2")),
            ("expiry", json!(200)),
            ("actions", json!([{ "type": "transfer" }])),
            ("constraints", json!({ "max_fee_astra": "1" })),
            ("chain", json!("other")),
        ] {
            let mut mutated = base.clone();
            mutated[field] = replacement;
            assert_ne!(
                baseline,
                canonical_intent_payload(&mutated),
                "changing `{field}` must change the signing payload"
            );
        }
    }

    #[test]
    fn expired_intent_is_rejected() {
        let intent = json!({ "actor": "did:a", "expiry": 100 });
        let err =
            verify_signed_intent(&intent, "00", "sphincs+", 200, |_| Some(vec![])).unwrap_err();
        assert!(matches!(err, IntentAuthError::Expired { .. }));
    }

    #[test]
    fn far_future_expiry_is_rejected() {
        let intent = json!({ "actor": "did:a", "expiry": 100_000 });
        let err = verify_signed_intent(&intent, "00", "sphincs+", 0, |_| Some(vec![])).unwrap_err();
        assert!(matches!(err, IntentAuthError::ExpiryTooFar { .. }));
    }

    #[test]
    fn unknown_actor_is_rejected() {
        let intent = json!({ "actor": "did:a", "expiry": 100 });
        let err = verify_signed_intent(&intent, "00", "sphincs+", 50, |_| None).unwrap_err();
        assert!(matches!(err, IntentAuthError::UnknownActor(_)));
    }

    #[test]
    fn unsupported_sig_type_is_rejected() {
        let intent = json!({ "actor": "did:a", "expiry": 100 });
        let err = verify_signed_intent(&intent, "00", "ecdsa", 50, |_| Some(vec![])).unwrap_err();
        assert!(matches!(err, IntentAuthError::UnsupportedSigType(_)));
    }

    #[test]
    fn missing_actor_is_rejected() {
        let intent = json!({ "expiry": 100 });
        let err =
            verify_signed_intent(&intent, "00", "sphincs+", 50, |_| Some(vec![])).unwrap_err();
        assert!(matches!(err, IntentAuthError::MissingField("actor")));
    }

    #[test]
    fn valid_intent_round_trips() {
        use spacekit_did::sphincs::SphincsPlus;
        let kp = SphincsPlus::generate_keypair();
        let intent = json!({
            "version": "1.0", "intent_id": "abc", "actor": "did:alice", "chain": "c",
            "nonce": "1", "expiry": 1_000, "actions": [], "constraints": {}
        });
        let sig = SphincsPlus::sign(&kp.private_key, &canonical_intent_payload(&intent)).unwrap();

        let actor = verify_signed_intent(&intent, &hex::encode(&sig), "sphincs+", 500, |_| {
            Some(kp.public_key.clone())
        })
        .unwrap();
        assert_eq!(actor, "did:alice");

        // Tampering with the amount invalidates the signature.
        let mut tampered = intent.clone();
        tampered["actions"] = json!([{ "type": "transfer", "amount": "999999" }]);
        assert!(matches!(
            verify_signed_intent(&tampered, &hex::encode(&sig), "sphincs+", 500, |_| Some(
                kp.public_key.clone()
            )),
            Err(IntentAuthError::BadSignature)
        ));
    }
}
