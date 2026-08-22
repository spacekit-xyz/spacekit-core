//! SpaceKit Intent Classifier Contract
//! 
//! An AI-powered smart contract that uses the `spacekit_llm` host function
//! to classify intent of a message into a category.
//!
//! Operations:
//!   1 = CLASSIFY: Classify content into categories
//!   2 = STATUS: Check LLM availability

#![no_std]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::format;

use spacekit_contract_sdk::{
    ContractError, ContractErrorCode, SpacekitContract, spacekit_contract, emit_event_bytes,
    llm_call, llm_get_status, llm_status,
};

#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

#[cfg(all(target_arch = "wasm32", not(test)))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

struct SpacekitIntentClassifier;

// Operation codes
const OP_CLASSIFY: u8 = 1;
const OP_STATUS: u8 = 2;

// Response buffer size
const MAX_RESPONSE_LEN: usize = 2048;

impl SpacekitContract for SpacekitIntentClassifier {
    type Error = ContractError;

    fn init() -> Self {
        SpacekitIntentClassifier
    }

    fn handle(&mut self, input: &[u8]) -> Result<Vec<u8>, ContractError> {
        handle(input)
    }
}

#[cfg(not(test))]
spacekit_contract!(SpacekitIntentClassifier);

fn handle(input: &[u8]) -> Result<Vec<u8>, ContractError> {
    let mut cursor = 0usize;
    let op = read_u8(input, &mut cursor)?;
    
    match op {
        OP_CLASSIFY => {
            let content = read_string(input, &mut cursor)?;
            let classification = intent_classifier_classify(&content)?;
            emit_event_bytes("spacekit.intent_classifier.classify", &[]);
            Ok(classification.into_bytes())
        }
        OP_STATUS => {
            let status = llm_get_status();
            // Return status as a single byte + human readable
            let msg = match status {
                s if s == llm_status::NOT_LOADED => "not_loaded",
                s if s == llm_status::READY => "ready",
                s if s == llm_status::LOADING => "loading",
                _ => "unknown",
            };
            Ok(msg.as_bytes().to_vec())
        }
        _ => Err(ContractError::InvalidInput),
    }
}

/// Classify intent of a message
fn intent_classifier_classify(message: &str) -> Result<String, ContractError> { 
    let prompt = format!(
        r#"[MODE=CLASSIFY]
You are an Intent Classifier.
Output ONLY a JSON object with exactly two fields: intent and confidence.
No explanations. No entities. No examples. No extra text.

Valid intents:
- classify
- ask_contract
- ask_llm
- ask_payment
- ask_storage
- ask_identity
- ask_error
- spacekit_message
- ask_unknown

Output template: {{"intent":"...one of the valid intents...", "confidence":<0.0 to 1.0>}} \n\

RULES: \n\
- Output ONLY a valid JSON object. \n\
- No explanations. No entities. No examples. No extra text. \n\
- Do NOT output entities or any additional fields. \n\
- Only one of the valid intents is allowed. \n\
- Confidence must be between 0.0 and 1.0. \n\
- Intent must be one of the valid intents. \n\
- Intent must be a valid JSON object. \n\
- Output template must be followed exactly. \n\
- If the user asks what you can do or asks about your abilities → classify \n\
- If the message starts with "spacetime:" → spacekit_message \n\
- If the user describes sending, depositing, transferring, or paying tokens → ask_payment \n\
- If the user asks about contracts, deploying, calling, or inspecting a contract → ask_contract \n\
- If the user asks about models or LLMs → ask_llm \n\
- If the user asks about files, uploading, retrieving, or storage → ask_storage \n\
- If the user asks about identity, keys, signatures, or DIDs → ask_identity \n\
- If the user asks about an error, bug, crash, or stack trace → ask_error \n\
- Otherwise → ask_unknown \n\

\n\
Only respond with the JSON object. Do not include any other text.
\n\
USER: {message}\n\ 
OUTPUT:"#
    ); // Tiny model friendly: low temperature, short output
    llm_call(&prompt, 15, 128, MAX_RESPONSE_LEN)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Input parsing helpers
// ═══════════════════════════════════════════════════════════════════════════════

fn read_u8(input: &[u8], cursor: &mut usize) -> Result<u8, ContractError> {
    if *cursor >= input.len() {
        return Err(ContractError::InvalidInput);
    }
    let value = input[*cursor];
    *cursor += 1;
    Ok(value)
}

fn read_u16(input: &[u8], cursor: &mut usize) -> Result<u16, ContractError> {
    if *cursor + 2 > input.len() {
        return Err(ContractError::InvalidInput);
    }
    let bytes = [input[*cursor], input[*cursor + 1]];
    *cursor += 2;
    Ok(u16::from_le_bytes(bytes))
}

fn read_string(input: &[u8], cursor: &mut usize) -> Result<String, ContractError> {
    let len = read_u16(input, cursor)? as usize;
    if *cursor + len > input.len() {
        return Err(ContractError::InvalidInput);
    }
    let slice = &input[*cursor..*cursor + len];
    *cursor += len;
    core::str::from_utf8(slice)
        .map(|s| s.to_string())
        .map_err(|_| ContractError::InvalidInput)
}
