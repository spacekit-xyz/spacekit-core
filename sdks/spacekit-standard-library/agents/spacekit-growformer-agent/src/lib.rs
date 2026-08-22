//! SpaceKit Growformer **chat agent** — contract WASM (Growformer host + `chat_brain` key).
//!
//! Mirrors the crypto contract's shape: single-purpose, brain-load on every handle, length-prefixed
//! UTF-8 wire format. Multi-turn state is held by the Growformer host between calls within the
//! same VM lifetime; reset is an explicit one-byte op. Brain bytes live in VM storage at
//! `chat_brain` (e.g. `chat-brain.bin` from storage node deploy).
//!
//! Wire format:
//!   - chat turn:       [0x01][len: u16 LE][message: utf8]
//!   - reset session:   [0x02]
//!
//! The 0x01 prefix lets us add ops later without breaking deployed clients. A bare
//! length-prefixed message (no op byte) would also work if you never need reset; the op byte
//! costs one byte and buys forward compatibility.

#![no_std]

extern crate alloc;

use crate::alloc::vec::Vec;

use spacekit_contract_sdk::{
    emit_event_bytes, growformer_converse, growformer_load_brain_from_storage_key,
    growformer_reset_conversation, spacekit_contract, ContractError, ContractErrorCode,
    SpacekitContract,
    wire::{read_string, read_u8},
};

#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

#[cfg(all(target_arch = "wasm32", not(test)))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

struct SpacekitGrowformerChatAgent;

const MAX_RESPONSE_LEN: usize = 4096;

pub const AGENT_BRAIN: &str = "chat_brain";

const OP_CHAT: u8 = 1;
const OP_RESET: u8 = 2;

impl SpacekitContract for SpacekitGrowformerChatAgent {
    type Error = ContractError;

    fn init() -> Self {
        SpacekitGrowformerChatAgent
    }

    fn handle(&mut self, input: &[u8]) -> Result<Vec<u8>, ContractError> {
        handle(input)
    }
}

spacekit_contract!(SpacekitGrowformerChatAgent);

fn handle(input: &[u8]) -> Result<Vec<u8>, ContractError> {
    let mut cursor = 0usize;
    let op = read_u8(input, &mut cursor)?;

    // Brain load is idempotent on the host; calling on every entry mirrors the crypto contract
    // and removes any "is the brain loaded yet?" ambiguity from clients.
    growformer_load_brain_from_storage_key(AGENT_BRAIN)?;

    match op {
        OP_CHAT => {
            let message = read_string(input, &mut cursor)?;
            let response = growformer_converse(&message, MAX_RESPONSE_LEN)?;
            // Emit only a length marker — content stays out of the event stream. Flip to
            // `message.as_bytes()` if your event channel is local-VM-only and you want it.
            let len_bytes = (message.len() as u32).to_le_bytes();
            emit_event_bytes("spacekit.agent.chat", &len_bytes);
            Ok(response.into_bytes())
        }
        OP_RESET => {
            growformer_reset_conversation();
            emit_event_bytes("spacekit.agent.chat_reset", &[]);
            Ok(b"ok".to_vec())
        }
        _ => Err(ContractError::InvalidInput),
    }
}
