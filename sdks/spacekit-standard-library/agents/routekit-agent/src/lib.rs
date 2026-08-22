//! RouteKit agent — Growformer routing + vault + messaging + remote storage.
//! Full wire format, opcodes, and build instructions: **README.md** in this crate root.
//!
//! Incremental spec implementation: local **COMPLETE**, search→reply **PIPELINE**, **SEARCH** legacy
//! and **V1** (search effects + routed generation), **CONVERSE** with client-held refs, **CONFIGURE** blob,
//! **FRONTIER_SEND**, operator **PING**.
//!
//! Wire format (**little-endian** `u16`):
//!
//! | Op | Opcode | Payload |
//! |----|--------|---------|
//! | HEALTH | `0x10` | (empty) |
//! | COMPLETE | `0x01` | `[max_resp u16][prompt_len u16][prompt_utf8]` |
//! | PIPELINE_SEARCH_THEN_REPLY | `0x02` | `[max_resp u16][sq_len u16][search_query_utf8][uq_len u16][user_question_utf8]` |
//! | SEARCH legacy | `0x03` | UTF-8 query only (charges search tier; returns JSON hits UTF-8) |
//! | SEARCH v1 | `0x03` then `1u8` | `[1][max_gen u16][q_len u16][query_utf8]` — search+local, then routed reply |
//! | CONVERSE | `0x04` | `[hist_ref_len u16][hist_ref_utf8][msg_len u16][msg_utf8]` |
//! | FRONTIER_SEND | `0x05` | `[recipient_len u16][recipient_utf8][payload_len u16][payload_bytes]` |
//! | PING | `0x11` | (same as FRONTIER but only ping event; vault not charged here) |
//! | BRAIN_INFO | `0x12` | (empty body after opcode) |
//! | CONFIGURE | `0x20` | `[prefs_len u16][prefs_utf8]` → returns `[ref_len u16][ref_utf8]` |

#![no_std]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use spacekit_contract_sdk::{
    emit_event_bytes, get_caller_did_string, growformer_brain_info, growformer_generation,
    growformer_host_status, growformer_load_brain_from_storage_key, messaging::messaging_send,
    payments::payment_vault_charge, remote_storage::remote_storage_get,
    remote_storage::remote_storage_put, spacekit_contract,     tools::web_search, ContractError, ContractErrorCode, SpacekitContract,
};

#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

#[cfg(all(target_arch = "wasm32", not(test)))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

struct RouteKitAgent;

pub const ROUTER_BRAIN_KEY: &str = "routekit_router";

const OP_COMPLETE: u8 = 0x01;
const OP_PIPELINE: u8 = 0x02;
const OP_SEARCH: u8 = 0x03;
const OP_CONVERSE: u8 = 0x04;
const OP_FRONTIER: u8 = 0x05;
const OP_PING_OPERATOR: u8 = 0x11;
const OP_BRAIN_INFO: u8 = 0x12;
const OP_HEALTH: u8 = 0x10;
const OP_CONFIGURE: u8 = 0x20;

const COST_LOCAL: &str = "100";
const COST_SEARCH: &str = "200";
const COST_PIPE: &str = "300";
const COST_SEARCH_AND_LOCAL: &str = "300";
const COST_FRONTIER: &str = "5000";

const SEARCH_WIRE_V1: u8 = 1;
const DEFAULT_SEARCH_MAX_JSON: usize = 64 * 1024;
const REF_OUT_MAX: usize = 512;
const CONVERSE_HIST_GET_MAX: usize = 96 * 1024;

impl SpacekitContract for RouteKitAgent {
    type Error = ContractError;

    fn init() -> Self {
        RouteKitAgent
    }

    fn handle(&mut self, input: &[u8]) -> Result<Vec<u8>, ContractError> {
        if input.is_empty() {
            return Err(ContractError::InvalidInput);
        }
        match input[0] {
            OP_HEALTH => Ok(health_json()),
            OP_COMPLETE => handle_complete(&input[1..]),
            OP_PIPELINE => handle_pipeline(&input[1..]),
            OP_SEARCH => handle_search_dispatch(&input[1..]),
            OP_CONVERSE => handle_converse(&input[1..]),
            OP_FRONTIER => handle_frontier(&input[1..]),
            OP_PING_OPERATOR => handle_ping(&input[1..]),
            OP_BRAIN_INFO => handle_brain_info(),
            OP_CONFIGURE => handle_configure(&input[1..]),
            _ => Err(ContractError::InvalidInput),
        }
    }
}

spacekit_contract!(RouteKitAgent);

fn beneficiary() -> String {
    get_caller_did_string().unwrap_or_else(|_| String::from("did:spacekit:anonymous"))
}

fn health_json() -> Vec<u8> {
    let gs = growformer_host_status();
    let brain_ok = growformer_load_brain_from_storage_key(ROUTER_BRAIN_KEY).is_ok();
    format!(
        r#"{{"status":"ok","agent":"routekit-agent","growformer_status":{gs},"router_brain_seeded":{brain}}}"#,
        gs = gs,
        brain = if brain_ok { "true" } else { "false" }
    )
    .into_bytes()
}

fn handle_complete(body: &[u8]) -> Result<Vec<u8>, ContractError> {
    let (max_resp, prompt) = decode_u16_utf8(body)?;
    if prompt.is_empty() {
        return Err(ContractError::InvalidInput);
    }
    payment_vault_charge(COST_LOCAL, beneficiary().as_str())?;
    growformer_load_brain_from_storage_key(ROUTER_BRAIN_KEY)?;
    let out = growformer_generation(prompt.as_str(), max_resp)?;
    emit_event_bytes("routekit.complete", &(out.len() as u32).to_le_bytes());
    Ok(out.into_bytes())
}

fn handle_pipeline(body: &[u8]) -> Result<Vec<u8>, ContractError> {
    let (max_resp, rest) = read_u16(body)?;
    let (sq_bytes, rest) = read_blob_u16(rest)?;
    let (uq_bytes, rest) = read_blob_u16(rest)?;
    if !rest.is_empty() || sq_bytes.is_empty() || uq_bytes.is_empty() {
        return Err(ContractError::InvalidInput);
    }
    payment_vault_charge(COST_PIPE, beneficiary().as_str())?;
    let sq = core::str::from_utf8(&sq_bytes).map_err(|_| ContractError::InvalidInput)?;
    let uq = core::str::from_utf8(&uq_bytes).map_err(|_| ContractError::InvalidInput)?;
    let hits = web_search(sq, 5, DEFAULT_SEARCH_MAX_JSON)?;
    let enriched = format!(
        "Web results (JSON):\n{hits}\n\n---\nUser question:\n{uq}\n---\nReply concisely."
    );
    growformer_load_brain_from_storage_key(ROUTER_BRAIN_KEY)?;
    let out = growformer_generation(enriched.as_str(), max_resp)?;
    emit_event_bytes("routekit.pipeline", &(out.len() as u32).to_le_bytes());
    Ok(out.into_bytes())
}

fn handle_search_dispatch(q: &[u8]) -> Result<Vec<u8>, ContractError> {
    if q.is_empty() {
        return Err(ContractError::InvalidInput);
    }
    if q[0] == SEARCH_WIRE_V1 {
        return handle_search_v1(&q[1..]);
    }
    handle_search_legacy_raw(q)
}

fn handle_search_legacy_raw(query_bytes: &[u8]) -> Result<Vec<u8>, ContractError> {
    let prompt = core::str::from_utf8(query_bytes).map_err(|_| ContractError::InvalidInput)?;
    if prompt.is_empty() {
        return Err(ContractError::InvalidInput);
    }
    payment_vault_charge(COST_SEARCH, beneficiary().as_str())?;
    emit_event_bytes(
        "routekit.search.start",
        &(prompt.len() as u32).to_le_bytes(),
    );
    let hits = web_search(prompt, 5, DEFAULT_SEARCH_MAX_JSON)?;
    emit_event_bytes("routekit.search.done", &(hits.len() as u32).to_le_bytes());
    Ok(hits.into_bytes())
}

fn handle_search_v1(body: &[u8]) -> Result<Vec<u8>, ContractError> {
    let (max_gen, rest) = read_u16(body)?;
    let (q_bytes, tail) = read_blob_u16(rest)?;
    if !tail.is_empty() || q_bytes.is_empty() {
        return Err(ContractError::InvalidInput);
    }
    let query = core::str::from_utf8(&q_bytes).map_err(|_| ContractError::InvalidInput)?;
    payment_vault_charge(COST_SEARCH_AND_LOCAL, beneficiary().as_str())?;
    let hits = web_search(query, 5, DEFAULT_SEARCH_MAX_JSON)?;
    growformer_load_brain_from_storage_key(ROUTER_BRAIN_KEY)?;
    let enriched =
        format!("Use the JSON search snippets below.\nSnippets:\n{hits}\n\nTask:\n{query}\nAnswer:");
    let out = growformer_generation(enriched.as_str(), max_gen.max(512) as usize)?;
    emit_event_bytes("routekit.search_v1.done", &(out.len() as u32).to_le_bytes());
    Ok(out.into_bytes())
}

fn handle_converse(body: &[u8]) -> Result<Vec<u8>, ContractError> {
    let (hist_ref_bytes, rest) = read_blob_u16(body)?;
    let (msg_bytes, rest) = read_blob_u16(rest)?;
    if !rest.is_empty() || msg_bytes.is_empty() {
        return Err(ContractError::InvalidInput);
    }
    let hist_ref = core::str::from_utf8(&hist_ref_bytes).map_err(|_| ContractError::InvalidInput)?;
    let msg = core::str::from_utf8(&msg_bytes).map_err(|_| ContractError::InvalidInput)?;

    payment_vault_charge(COST_LOCAL, beneficiary().as_str())?;

    let mut transcript = String::new();
    if !hist_ref.is_empty() {
        let prev = remote_storage_get(hist_ref, CONVERSE_HIST_GET_MAX)?;
        transcript = core::str::from_utf8(&prev)
            .map_err(|_| ContractError::InvalidInput)?
            .into();
    }

    growformer_load_brain_from_storage_key(ROUTER_BRAIN_KEY)?;
    let block = format!("{transcript}\nUser: {msg}\nAssistant:");
    let reply = growformer_generation(block.as_str(), 3072)?;

    let new_transcript = format!("{transcript}\nUser: {msg}\nAssistant: {reply}\n");
    let new_ref = remote_storage_put(new_transcript.as_bytes(), REF_OUT_MAX)?;

    let mut out: Vec<u8> = Vec::new();
    push_blob_u16(&mut out, new_ref.as_bytes());
    push_blob_u16(&mut out, reply.as_bytes());
    emit_event_bytes("routekit.converse", &(reply.len() as u32).to_le_bytes());
    Ok(out)
}

fn handle_configure(body: &[u8]) -> Result<Vec<u8>, ContractError> {
    let (prefs, tail) = read_blob_u16(body)?;
    if !tail.is_empty() || prefs.is_empty() {
        return Err(ContractError::InvalidInput);
    }
    let ref_str = remote_storage_put(&prefs, REF_OUT_MAX)?;
    let mut out: Vec<u8> = Vec::new();
    push_blob_u16(&mut out, ref_str.as_bytes());
    emit_event_bytes("routekit.configure", &(prefs.len() as u32).to_le_bytes());
    Ok(out)
}

fn handle_frontier(body: &[u8]) -> Result<Vec<u8>, ContractError> {
    let (recip_bytes, rest) = read_blob_u16(body)?;
    let (pld, tail) = read_blob_u16(rest)?;
    if !tail.is_empty() || recip_bytes.is_empty() || pld.is_empty() {
        return Err(ContractError::InvalidInput);
    }
    payment_vault_charge(COST_FRONTIER, beneficiary().as_str())?;
    let recipient =
        core::str::from_utf8(&recip_bytes).map_err(|_| ContractError::InvalidInput)?;
    messaging_send(recipient, &pld)?;
    emit_event_bytes("routekit.frontier.sent", &(pld.len() as u32).to_le_bytes());
    Ok(b"pending".to_vec())
}

fn handle_brain_info() -> Result<Vec<u8>, ContractError> {
    growformer_load_brain_from_storage_key(ROUTER_BRAIN_KEY)?;
    let info = growformer_brain_info(4096)?;
    Ok(info.into_bytes())
}

fn handle_ping(cursor: &[u8]) -> Result<Vec<u8>, ContractError> {
    let (recipient_bytes, rest) = read_blob_u16(cursor)?;
    let (payload, tail) = read_blob_u16(rest)?;
    if !tail.is_empty() || recipient_bytes.is_empty() || payload.is_empty() {
        return Err(ContractError::InvalidInput);
    }
    let recipient =
        core::str::from_utf8(&recipient_bytes).map_err(|_| ContractError::InvalidInput)?;
    messaging_send(recipient, &payload)?;
    let marker = receipt_len_marker(recipient.len(), payload.len());
    emit_event_bytes("routekit.ping.sent", &marker);
    Ok(b"sent".to_vec())
}

fn receipt_len_marker(a: usize, b: usize) -> [u8; 8] {
    let mut out = [0u8; 8];
    out[..4].copy_from_slice(&(a as u32).to_le_bytes());
    out[4..].copy_from_slice(&(b as u32).to_le_bytes());
    out
}

fn read_u16(cursor: &[u8]) -> Result<(usize, &[u8]), ContractError> {
    if cursor.len() < 2 {
        return Err(ContractError::InvalidInput);
    }
    Ok((usize::from(u16::from_le_bytes([cursor[0], cursor[1]])), &cursor[2..]))
}

fn read_blob_u16(cursor: &[u8]) -> Result<(Vec<u8>, &[u8]), ContractError> {
    let (len, rest) = read_u16(cursor)?;
    if rest.len() < len {
        return Err(ContractError::InvalidInput);
    }
    Ok((rest[..len].to_vec(), &rest[len..]))
}

fn decode_u16_utf8(cursor: &[u8]) -> Result<(usize, String), ContractError> {
    let (max_resp, rest) = read_u16(cursor)?;
    let (raw, tail) = read_blob_u16_from_slice(rest)?;
    if !tail.is_empty() {
        return Err(ContractError::InvalidInput);
    }
    let s =
        core::str::from_utf8(raw).map_err(|_| ContractError::InvalidInput)?;
    Ok((max_resp, String::from(s)))
}

fn read_blob_u16_from_slice(rest: &[u8]) -> Result<(&[u8], &[u8]), ContractError> {
    let (len, r) = read_u16(rest)?;
    if r.len() < len {
        return Err(ContractError::InvalidInput);
    }
    Ok((&r[..len], &r[len..]))
}

fn push_blob_u16(out: &mut Vec<u8>, blob: &[u8]) {
    let n = blob.len().min(u16::MAX as usize) as u16;
    out.extend_from_slice(&n.to_le_bytes());
    out.extend_from_slice(&blob[..n as usize]);
}
