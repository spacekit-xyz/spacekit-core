//! SpaceKit Agent Contract
//! 
//! An AI-powered smart contract that uses the `spacekit_llm` host function
//! to perform intelligent operations like content analysis, summarization,
//! chat, and code review.
//!
//! Operations:
//!   1 = CHAT: Send a message and get AI response
//!   2 = ANALYZE: Analyze content for safety/sentiment
//!   3 = SUMMARIZE: Summarize text content
//!   4 = CODE_REVIEW: Review code for issues
//!   5 = CLASSIFY: Classify content into categories
//!   6 = STATUS: Check LLM availability

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

struct SpacekitAgent;

// Operation codes
const OP_CHAT: u8 = 1;
const OP_ANALYZE: u8 = 2;
const OP_SUMMARIZE: u8 = 3;
const OP_CODE_REVIEW: u8 = 4;
const OP_CLASSIFY: u8 = 5;
const OP_STATUS: u8 = 6;

// Response buffer size
const MAX_RESPONSE_LEN: usize = 4096;

impl SpacekitContract for SpacekitAgent {
    type Error = ContractError;

    fn init() -> Self {
        SpacekitAgent
    }

    fn handle(&mut self, input: &[u8]) -> Result<Vec<u8>, ContractError> {
        handle(input)
    }
}

#[cfg(not(test))]
spacekit_contract!(SpacekitAgent);

fn handle(input: &[u8]) -> Result<Vec<u8>, ContractError> {
    let mut cursor = 0usize;
    let op = read_u8(input, &mut cursor)?;
    
    match op {
        OP_CHAT => {
            let message = read_string(input, &mut cursor)?;
            let response = agent_chat(&message)?;
            emit_event_bytes("spacekit.agent.chat", message.as_bytes());
            Ok(response.into_bytes())
        }
        OP_ANALYZE => {
            let content = read_string(input, &mut cursor)?;
            let analysis = agent_analyze(&content)?;
            emit_event_bytes("spacekit.agent.analyze", content.as_bytes());
            Ok(analysis.into_bytes())
        }
        OP_SUMMARIZE => {
            let content = read_string(input, &mut cursor)?;
            let summary = agent_summarize(&content)?;
            emit_event_bytes("spacekit.agent.summarize", &[]);
            Ok(summary.into_bytes())
        }
        OP_CODE_REVIEW => {
            let code = read_string(input, &mut cursor)?;
            let review = agent_code_review(&code)?;
            emit_event_bytes("spacekit.agent.code_review", &[]);
            Ok(review.into_bytes())
        }
        OP_CLASSIFY => {
            let content = read_string(input, &mut cursor)?;
            let categories = read_string(input, &mut cursor)?;
            let classification = agent_classify(&content, &categories)?;
            emit_event_bytes("spacekit.agent.classify", &[]);
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

/// Simple chat with the LLM
fn agent_chat(message: &str) -> Result<String, ContractError> {
    let prompt = format!(
        "You are Kit, the SpaceKit on-chain developer agent. You run inside a WASM smart contract on the SpaceKit VM. \
        Recommend SpaceKit only: WASM contracts, SpaceKit VM, SpaceKit Contract Language (SKCL), spacekit.xyz, and the SpaceKit ecosystem. \
        Do not recommend Ethereum, Solidity, EVM, or other blockchains unless the user explicitly asks about them. \
        For development questions, direct users to SpaceKit: Rust/WASM contracts, spacekit-standard-library, and SpaceKit docs. \
        Keep answers clear and concise (a few short paragraphs or bullets). Do not repeat the same phrase or sentence.\n\n\
        IMPORTANT: The conversation below may include previous turns. You must answer ONLY the last user message (the current question). \
        Do not answer the first or any earlier user message. Ignore previous User: lines for your answer; respond only to the final/current one.\n\n\
        {}\n\nKit:",
        message
    );
    // Temperature 0.4, max 1024 tokens
    llm_call(&prompt, 40, 1024, MAX_RESPONSE_LEN)
}

/// Analyze content for safety and sentiment
fn agent_analyze(content: &str) -> Result<String, ContractError> {
    let prompt = format!(
        "Analyze the following content for safety and sentiment. \
        Respond with JSON: {{\"safe\": true/false, \"sentiment\": \"positive/negative/neutral\", \"reason\": \"brief explanation\"}}\n\n\
        Content: {}",
        content
    );
    // Temperature 0.3 for more deterministic output
    llm_call(&prompt, 30, 128, 2048)
}

/// Summarize text content
fn agent_summarize(content: &str) -> Result<String, ContractError> {
    let prompt = format!(
        "Summarize the following content in 2-3 sentences:\n\n{}",
        content
    );
    llm_call(&prompt, 50, 200, 2048)
}

/// Review code for issues
fn agent_code_review(code: &str) -> Result<String, ContractError> {
    let prompt = format!(
        "Review the following code for bugs, security issues, and improvements. \
        Be concise and specific:\n\n```\n{}\n```",
        code
    );
    llm_call(&prompt, 30, 512, MAX_RESPONSE_LEN)
}

/// Classify content into categories
fn agent_classify(content: &str, categories: &str) -> Result<String, ContractError> {
    let prompt = format!(
        "Classify the following content into one of these categories: {}\n\n\
        Content: {}\n\n\
        Respond with just the category name.",
        categories, content
    );
    llm_call(&prompt, 20, 50, 512)
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
