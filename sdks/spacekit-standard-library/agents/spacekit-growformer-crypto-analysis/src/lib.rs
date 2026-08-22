//! SpaceKit Growformer **crypto analysis** — contract WASM (Growformer host + `crypto_brain` key).
//!
//! Wire format matches sentiment: length-prefixed UTF-8 only. Brain bytes live in VM storage at
//! `crypto_brain` (e.g. `crypto-brain.bin` from storage node deploy).

#![no_std]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use spacekit_contract_sdk::{
    emit_event_bytes, growformer_generation, growformer_load_brain_from_storage_key, spacekit_contract,
    ContractError, ContractErrorCode, SpacekitContract,
    wire::read_string,
};

#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

#[cfg(all(target_arch = "wasm32", not(test)))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

struct SpacekitGrowformerCryptoAgent;

const MAX_RESPONSE_LEN: usize = 4096;

pub const AGENT_BRAIN: &str = "crypto_brain";

impl SpacekitContract for SpacekitGrowformerCryptoAgent {
    type Error = ContractError;

    fn init() -> Self {
        SpacekitGrowformerCryptoAgent
    }

    fn handle(&mut self, input: &[u8]) -> Result<Vec<u8>, ContractError> {
        handle(input)
    }
}

spacekit_contract!(SpacekitGrowformerCryptoAgent);

fn handle(input: &[u8]) -> Result<Vec<u8>, ContractError> {
    let mut cursor = 0usize;
    let content = read_string(input, &mut cursor)?;
    growformer_load_brain_from_storage_key(AGENT_BRAIN)?;
    let analysis = agent_analyze_crypto(&content)?;
    emit_event_bytes("spacekit.agent.crypto_analysis", content.as_bytes());
    Ok(analysis.into_bytes())
}

fn agent_analyze_crypto(content: &str) -> Result<String, ContractError> {
    let prompt = format!("{}", content);
    growformer_generation(&prompt, MAX_RESPONSE_LEN)
}