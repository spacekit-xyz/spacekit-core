use anyhow::{anyhow, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::{Deserialize, Serialize};

use crate::pq_crypto::{sign, verify};

pub const USER_SIG_SEP: &str = "|";
pub const TICKET_TTL_S: i64 = 1800;

pub fn user_sig_message(
    session_id: &str,
    subject: &str,
    keystore_id: &str,
    shard_index: u32,
    client_eph_pk_b64: &str,
) -> Vec<u8> {
    format!(
        "{session_id}{USER_SIG_SEP}{subject}{USER_SIG_SEP}{keystore_id}{USER_SIG_SEP}{shard_index}{USER_SIG_SEP}{client_eph_pk_b64}"
    )
    .into_bytes()
}

pub fn verify_user_sig(signer_pk_b64: &str, message: &[u8], sig_b64: &str) -> Result<()> {
    let pk = B64.decode(signer_pk_b64).map_err(|e| anyhow!("bad signer pk: {e}"))?;
    let sig = B64.decode(sig_b64).map_err(|e| anyhow!("bad user_sig: {e}"))?;
    verify(&pk, message, &sig).map_err(|_| anyhow!("invalid user signature"))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordinatorTicketBody {
    pub v: u8,
    pub session_id: String,
    pub subject: String,
    pub guardian_kid: String,
    pub keystore_id: String,
    pub issued_at: i64,
    pub expires_at: i64,
    pub nonce: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordinatorTicket {
    pub body: CoordinatorTicketBody,
    pub sig: String,
}

pub fn sign_coordinator_ticket(body: &CoordinatorTicketBody, coordinator_sk: &[u8]) -> Result<String> {
    let payload = serde_json::to_vec(body)?;
    let sig = sign(coordinator_sk, &payload)?;
    let ticket = CoordinatorTicket {
        body: body.clone(),
        sig: B64.encode(sig),
    };
    Ok(B64.encode(serde_json::to_vec(&ticket)?))
}

pub fn verify_coordinator_ticket(
    ticket_b64: &str,
    coordinator_pk: &[u8],
    req: &CoordinatorTicketBody,
    now: i64,
) -> Result<()> {
    let raw = B64.decode(ticket_b64).map_err(|e| anyhow!("bad ticket b64: {e}"))?;
    let ticket: CoordinatorTicket =
        serde_json::from_slice(&raw).map_err(|_| anyhow!("invalid ticket format"))?;
    if ticket.body.v != 1 {
        return Err(anyhow!("unsupported ticket version"));
    }
    let payload = serde_json::to_vec(&ticket.body)?;
    let sig = B64.decode(&ticket.sig).map_err(|e| anyhow!("bad ticket sig: {e}"))?;
    verify(coordinator_pk, &payload, &sig).map_err(|_| anyhow!("invalid coordinator ticket signature"))?;

    if ticket.body.session_id != req.session_id {
        return Err(anyhow!("ticket session mismatch"));
    }
    if ticket.body.subject != req.subject {
        return Err(anyhow!("ticket subject mismatch"));
    }
    if ticket.body.guardian_kid != req.guardian_kid {
        return Err(anyhow!("ticket guardian mismatch"));
    }
    if ticket.body.keystore_id != req.keystore_id {
        return Err(anyhow!("ticket keystore mismatch"));
    }
    if now < ticket.body.issued_at - 60 {
        return Err(anyhow!("ticket not yet valid"));
    }
    if now > ticket.body.expires_at {
        return Err(anyhow!("ticket expired"));
    }
    Ok(())
}

pub fn verify_manifest_sig(manifest_body_json: &str, signer_pk_b64: &str, sig_b64: &str) -> Result<()> {
    let sig = B64.decode(sig_b64).map_err(|e| anyhow!("bad manifest sig: {e}"))?;
    let pk = B64.decode(signer_pk_b64).map_err(|e| anyhow!("bad signer pk: {e}"))?;
    verify(&pk, manifest_body_json.as_bytes(), &sig).map_err(|_| anyhow!("invalid manifest signature"))
}

pub fn sign_audit_record(payload_json: &str, sk: &[u8]) -> Result<String> {
    let sig = sign(sk, payload_json.as_bytes())?;
    Ok(B64.encode(sig))
}
