//! Financial Analysis Agent — built on the RouteKit reference architecture.
//!
//! Provides: MARKET_SNAPSHOT, RISK_METRICS, FACTOR_EXPOSURE,
//! SENTIMENT_SIGNAL, BACKTEST_QUERY, CONFIGURE, HEALTH, BRAIN_INFO.
//!
//! Wire format: little-endian u16 framing, identical to RouteKit.

#![no_std]

extern crate alloc;

use alloc::{string::String, vec::Vec, format};

use spacekit_contract_sdk::{
    emit_event_bytes, get_caller_did_string,
    growformer_brain_info, growformer_generation, growformer_host_status,
    growformer_load_brain_from_storage_key,
    payments::payment_vault_charge,
    remote_storage::{remote_storage_get, remote_storage_put},
    tools::web_search,
    messaging::messaging_send,
    ContractError, SpacekitContract, spacekit_contract,
};

#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

#[cfg(all(target_arch = "wasm32", not(test)))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

struct FinancialAgent;

pub const BRAIN_KEY: &str = "financial_brain";

// -------------------------
// Opcodes
// -------------------------
const OP_HEALTH: u8 = 0x10;
const OP_CONFIGURE: u8 = 0x20;
const OP_BRAIN_INFO: u8 = 0x12;

// Financial ops
const OP_MARKET_SNAPSHOT: u8 = 0x30;
const OP_RISK_METRICS: u8 = 0x31;
const OP_FACTOR_EXPOSURE: u8 = 0x32;
const OP_SENTIMENT_SIGNAL: u8 = 0x33;
const OP_BACKTEST_QUERY: u8 = 0x34;

// -------------------------
// Costs
// -------------------------
const COST_DATA: &str = "150";
const COST_RISK: &str = "400";
const COST_FACTOR: &str = "300";
const COST_SENTIMENT: &str = "350";
const COST_BACKTEST: &str = "600";

// -------------------------
// Limits
// -------------------------
const MAX_JSON: usize = 64 * 1024;
const REF_MAX: usize = 512;

// -------------------------
// Contract entry
// -------------------------
impl SpacekitContract for FinancialAgent {
    type Error = ContractError;

    fn init() -> Self {
        FinancialAgent
    }

    fn handle(&mut self, input: &[u8]) -> Result<Vec<u8>, ContractError> {
        if input.is_empty() {
            return Err(ContractError::InvalidInput);
        }

        match input[0] {
            OP_HEALTH => Ok(health_json()),
            OP_CONFIGURE => handle_configure(&input[1..]),
            OP_BRAIN_INFO => handle_brain_info(),

            OP_MARKET_SNAPSHOT => handle_market_snapshot(&input[1..]),
            OP_RISK_METRICS => handle_risk_metrics(&input[1..]),
            OP_FACTOR_EXPOSURE => handle_factor_exposure(&input[1..]),
            OP_SENTIMENT_SIGNAL => handle_sentiment_signal(&input[1..]),
            OP_BACKTEST_QUERY => handle_backtest_query(&input[1..]),

            _ => Err(ContractError::InvalidInput),
        }
    }
}

spacekit_contract!(FinancialAgent);

// -------------------------
// Helpers
// -------------------------
fn beneficiary() -> String {
    get_caller_did_string().unwrap_or_else(|_| String::from("did:spacekit:anonymous"))
}

fn health_json() -> Vec<u8> {
    let gs = growformer_host_status();
    let brain_ok = growformer_load_brain_from_storage_key(BRAIN_KEY).is_ok();
    format!(
        r#"{{"status":"ok","agent":"financial-agent","growformer_status":{gs},"brain_seeded":{brain}}}"#,
        gs = gs,
        brain = if brain_ok { "true" } else { "false" }
    )
    .into_bytes()
}

// -------------------------
// CONFIGURE
// -------------------------
fn handle_configure(body: &[u8]) -> Result<Vec<u8>, ContractError> {
    let (prefs, tail) = read_blob_u16(body)?;
    if !tail.is_empty() || prefs.is_empty() {
        return Err(ContractError::InvalidInput);
    }
    let r = remote_storage_put(&prefs, REF_MAX)?;
    let mut out = Vec::new();
    push_blob_u16(&mut out, r.as_bytes());
    emit_event_bytes("financial.configure", &(prefs.len() as u32).to_le_bytes());
    Ok(out)
}

// -------------------------
// MARKET_SNAPSHOT
// -------------------------
fn handle_market_snapshot(body: &[u8]) -> Result<Vec<u8>, ContractError> {
    let (universe_bytes, tail) = read_blob_u16(body)?;
    if !tail.is_empty() || universe_bytes.is_empty() {
        return Err(ContractError::InvalidInput);
    }

    payment_vault_charge(COST_DATA, beneficiary().as_str())?;

    let universe = core::str::from_utf8(&universe_bytes)
        .map_err(|_| ContractError::InvalidInput)?;

    let data = web_search(universe, 5, MAX_JSON)?;
    emit_event_bytes("financial.market_snapshot", &(data.len() as u32).to_le_bytes());
    Ok(data.into_bytes())
}

// -------------------------
// RISK_METRICS
// -------------------------
fn handle_risk_metrics(body: &[u8]) -> Result<Vec<u8>, ContractError> {
    let (portfolio_ref_bytes, rest) = read_blob_u16(body)?;
    let (params_bytes, tail) = read_blob_u16(rest)?;
    if !tail.is_empty() {
        return Err(ContractError::InvalidInput);
    }

    payment_vault_charge(COST_RISK, beneficiary().as_str())?;

    let pref = core::str::from_utf8(&portfolio_ref_bytes)
        .map_err(|_| ContractError::InvalidInput)?;
    let params = core::str::from_utf8(&params_bytes)
        .map_err(|_| ContractError::InvalidInput)?;

    let portfolio_blob = remote_storage_get(pref, MAX_JSON)?;
    let portfolio = core::str::from_utf8(&portfolio_blob)
        .map_err(|_| ContractError::InvalidInput)?;

    growformer_load_brain_from_storage_key(BRAIN_KEY)?;
    let prompt = format!(
        "Compute risk metrics.\nPortfolio:\n{portfolio}\nParams:\n{params}\nReturn JSON."
    );
    let out = growformer_generation(prompt.as_str(), 4096)?;
    emit_event_bytes("financial.risk", &(out.len() as u32).to_le_bytes());
    Ok(out.into_bytes())
}

// -------------------------
// FACTOR_EXPOSURE
// -------------------------
fn handle_factor_exposure(body: &[u8]) -> Result<Vec<u8>, ContractError> {
    let (portfolio_ref_bytes, tail) = read_blob_u16(body)?;
    if !tail.is_empty() {
        return Err(ContractError::InvalidInput);
    }

    payment_vault_charge(COST_FACTOR, beneficiary().as_str())?;

    let pref = core::str::from_utf8(&portfolio_ref_bytes)
        .map_err(|_| ContractError::InvalidInput)?;
    let portfolio_blob = remote_storage_get(pref, MAX_JSON)?;
    let portfolio = core::str::from_utf8(&portfolio_blob)
        .map_err(|_| ContractError::InvalidInput)?;

    growformer_load_brain_from_storage_key(BRAIN_KEY)?;
    let prompt = format!(
        "Compute factor exposures for this portfolio.\n{portfolio}\nReturn JSON."
    );
    let out = growformer_generation(prompt.as_str(), 4096)?;
    emit_event_bytes("financial.factor", &(out.len() as u32).to_le_bytes());
    Ok(out.into_bytes())
}

// -------------------------
// SENTIMENT_SIGNAL
// -------------------------
fn handle_sentiment_signal(body: &[u8]) -> Result<Vec<u8>, ContractError> {
    let (universe_bytes, tail) = read_blob_u16(body)?;
    if !tail.is_empty() || universe_bytes.is_empty() {
        return Err(ContractError::InvalidInput);
    }

    payment_vault_charge(COST_SENTIMENT, beneficiary().as_str())?;

    let universe = core::str::from_utf8(&universe_bytes)
        .map_err(|_| ContractError::InvalidInput)?;

    let hits = web_search(universe, 5, MAX_JSON)?;
    growformer_load_brain_from_storage_key(BRAIN_KEY)?;

    let enriched = format!(
        "Compute sentiment signals.\nNews snippets:\n{hits}\nUniverse:\n{universe}\nReturn JSON."
    );

    let out = growformer_generation(enriched.as_str(), 4096)?;
    emit_event_bytes("financial.sentiment", &(out.len() as u32).to_le_bytes());
    Ok(out.into_bytes())
}

// -------------------------
// BACKTEST_QUERY
// -------------------------
fn handle_backtest_query(body: &[u8]) -> Result<Vec<u8>, ContractError> {
    let (spec_bytes, tail) = read_blob_u16(body)?;
    if !tail.is_empty() || spec_bytes.is_empty() {
        return Err(ContractError::InvalidInput);
    }

    payment_vault_charge(COST_BACKTEST, beneficiary().as_str())?;

    let spec = core::str::from_utf8(&spec_bytes)
        .map_err(|_| ContractError::InvalidInput)?;

    let data = web_search(spec, 5, MAX_JSON)?;
    growformer_load_brain_from_storage_key(BRAIN_KEY)?;

    let prompt = format!(
        "Run a backtest.\nSpec:\n{spec}\nHistorical data:\n{data}\nReturn JSON summary."
    );

    let out = growformer_generation(prompt.as_str(), 4096)?;
    emit_event_bytes("financial.backtest", &(out.len() as u32).to_le_bytes());
    Ok(out.into_bytes())
}

// -------------------------
// BRAIN_INFO
// -------------------------
fn handle_brain_info() -> Result<Vec<u8>, ContractError> {
    growformer_load_brain_from_storage_key(BRAIN_KEY)?;
    let info = growformer_brain_info(4096)?;
    Ok(info.into_bytes())
}

// -------------------------
// Binary helpers
// -------------------------
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

fn push_blob_u16(out: &mut Vec<u8>, blob: &[u8]) {
    let n = blob.len().min(u16::MAX as usize) as u16;
    out.extend_from_slice(&n.to_le_bytes());
    out.extend_from_slice(&blob[..n as usize]);
}
