//! SpaceKit Growformer Sentiment Analysis — **contract WASM only**
//!
//! This crate compiles to `spacekit_growformer_sentiment_analysis.wasm` (Spacekit VM contract).
//! It calls **host imports** from `spacekit_contract_sdk` (`growformer_generation`, etc.). It does
//! **not** embed the Growformer engine: inference runs in the JS host’s separate wasm-bindgen
//! bundle (`spacekit-js/growformer-pkg/`). Rebuild from local Neurokit sources with
//! `npm run build:growformer-wasm` in `spacekit-js` (default `../neurokit/growformer`).
//!
//! The brain `.bin` is stored in VM storage under `AGENT_BRAIN`; the host loads it via
//! `growformer_load_brain_from_storage_key` before inference. Seed that key at deploy or via
//! storage later.
//!
//! **Extensibility (Phase 5):** deployments may pin `{ locale, brain_key, optional_rescore_wasm_hash }`
//! so retrieval policy ships as data/WASM instead of hardcoding English in Neurokit. See Neurokit
//! `growformer/docs/RETRIEVAL_EXTENSIBILITY.md` and declarative `sentiment_crypto_rescore.toml`.
//!
//! Input wire format: length-prefixed UTF-8 string only (no leading op byte).

#![no_std]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;

use spacekit_contract_sdk::{
    ContractError, ContractErrorCode, SpacekitContract, spacekit_contract, emit_event_bytes,
    growformer_generation, growformer_load_brain_from_storage_key,
    wire::read_string,
};

#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

#[cfg(all(target_arch = "wasm32", not(test)))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

struct SpacekitGrowformerAgent;

// TODO: expose as a contract parameter
const MAX_RESPONSE_LEN: usize = 4096;

/// VM storage key for the sentiment brain blob (same bytes as `sentiment-brain.bin`).
pub const AGENT_BRAIN: &str = "sentiment_brain";

impl SpacekitContract for SpacekitGrowformerAgent {
    type Error = ContractError;

    fn init() -> Self {
        SpacekitGrowformerAgent
    }

    fn handle(&mut self, input: &[u8]) -> Result<Vec<u8>, ContractError> {
        handle(input)
    }
}

spacekit_contract!(SpacekitGrowformerAgent);

fn handle(input: &[u8]) -> Result<Vec<u8>, ContractError> {
    let mut cursor = 0usize;
    let content = read_string(input, &mut cursor)?;
    // Host (`spacekit-js` `loadGrowformerBrain`) skips `growformer_load_brain` when storage bytes are
    // unchanged, so we keep this call every turn without re-parsing the brain on each prompt.
    growformer_load_brain_from_storage_key(AGENT_BRAIN)?;
    let analysis = agent_analyze_sentiment(&content)?;
    emit_event_bytes("spacekit.agent.sentiment_analysis", content.as_bytes());
    Ok(analysis.into_bytes())
}

fn agent_analyze_sentiment(content: &str) -> Result<String, ContractError> {
    let prompt = format!("{}", content);
    growformer_generation(&prompt, MAX_RESPONSE_LEN)
}