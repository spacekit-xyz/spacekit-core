#![no_std]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::vec;
use core::ptr;

pub trait ContractErrorCode {
    fn code(self) -> i32;
}

impl ContractErrorCode for i32 {
    fn code(self) -> i32 {
        self
    }
}

#[repr(i32)]
#[derive(Debug, Clone, Copy)]
pub enum ContractError {
    Failed = -1,
    InvalidInput = -2,
    StorageError = -3,
    HostError = -4,
    LlmError = -5,
    LlmNotReady = -6,
    GrowformerNotReady = -7,
    GrowformerError = -8,
    InsufficientPayment = -9,
    InsufficientBalance = -10,
    Unauthorized = -11,
    ToolNotConfigured = -12,
    /// Nested `contract_call` exceeded host max depth (Rust VM / JS host use 8).
    NestedContractDepth = -13,
    CapExceeded = -14,
    AlreadyInitialized = -15,
    NotInitialized = -16,
    /// Host must re-run WASM after resolving pending tool effects (`STATUS_NEEDS_TOOLS` on JS VM).
    NeedsTools = -100,
}

impl ContractErrorCode for ContractError {
    fn code(self) -> i32 {
        self as i32
    }
}

/// Little-endian length-prefixed wire helpers for parsing and building `handle` payloads.
pub mod wire;

pub trait SpacekitContract {
    type Error: ContractErrorCode + Copy;
    fn init() -> Self;
    fn handle(&mut self, input: &[u8]) -> Result<Vec<u8>, Self::Error>;
}

static mut LAST_RESULT: Option<Vec<u8>> = None;

pub fn set_result(data: Vec<u8>) {
    unsafe {
        core::ptr::addr_of_mut!(LAST_RESULT).write(Some(data));
    }
}

pub fn result_len() -> i32 {
    unsafe {
        let ptr = core::ptr::addr_of!(LAST_RESULT);
        match &*ptr {
            Some(d) => d.len() as i32,
            None => 0,
        }
    }
}

pub fn copy_result(dest_ptr: i32, max_len: i32) -> i32 {
    unsafe {
        let ptr = core::ptr::addr_of!(LAST_RESULT);
        if let Some(data) = &*ptr {
            let len = core::cmp::min(data.len(), max_len as usize);
            ptr::copy_nonoverlapping(data.as_ptr(), dest_ptr as *mut u8, len);
            return len as i32;
        }
    }
    0
}

// ═══════════════════════════════════════════════════════════════════════════════
// Core environment host functions
// ═══════════════════════════════════════════════════════════════════════════════

#[link(wasm_import_module = "env")]
extern "C" {
    fn emit_event(event_type_ptr: *const u8, event_type_len: usize, data_ptr: *const u8, data_len: usize);
    fn get_caller_did(output_ptr: *mut u8, max_len: usize) -> i32;
    fn verify_did(did_ptr: *const u8, did_len: usize) -> i32;
    fn msg_value() -> i64;
    fn get_balance(address_ptr: *const u8) -> i64;
    fn transfer(to_ptr: *const u8, amount: i64) -> i32;
    fn get_timestamp() -> i64;
}

// ═══════════════════════════════════════════════════════════════════════════════
// LLM host functions - for AI-powered smart contracts
// ═══════════════════════════════════════════════════════════════════════════════

#[link(wasm_import_module = "spacekit_llm")]
extern "C" {
    /// Call the LLM with a prompt and write response to dest buffer.
    /// Returns: >0 = response length written, -1 = LLM not ready, -2 = inference error
    fn llm_inference(
        prompt_ptr: *const u8,
        prompt_len: usize,
        dest_ptr: *mut u8,
        max_len: usize,
        temperature: u32,  // temperature * 100 (e.g., 70 = 0.7)
        max_tokens: u32,
    ) -> i32;
    
    /// Check LLM status: 0 = not loaded, 1 = ready, 2 = loading
    fn llm_status() -> i32;
}

/// LLM status codes
pub mod llm_status {
    pub const NOT_LOADED: i32 = 0;
    pub const READY: i32 = 1;
    pub const LOADING: i32 = 2;
}

/// Check if the LLM is ready for inference
pub fn llm_is_ready() -> bool {
    unsafe { llm_status() == llm_status::READY }
}

/// Get the current LLM status
pub fn llm_get_status() -> i32 {
    unsafe { llm_status() }
}

/// Call the LLM with a prompt and return the response
/// 
/// # Arguments
/// * `prompt` - The text prompt to send to the LLM
/// * `temperature` - Temperature * 100 (e.g., 70 for 0.7)
/// * `max_tokens` - Maximum tokens to generate
/// * `max_response_len` - Maximum response buffer size
/// 
/// # Returns
/// * `Ok(String)` - The LLM response
/// * `Err(ContractError::LlmNotReady)` - LLM not loaded
/// * `Err(ContractError::LlmError)` - Inference failed
pub fn llm_call(
    prompt: &str,
    temperature: u32,
    max_tokens: u32,
    max_response_len: usize,
) -> Result<String, ContractError> {
    let mut buffer = vec![0u8; max_response_len];
    
    let result = unsafe {
        llm_inference(
            prompt.as_ptr(),
            prompt.len(),
            buffer.as_mut_ptr(),
            buffer.len(),
            temperature,
            max_tokens,
        )
    };
    
    if result == -1 {
        return Err(ContractError::LlmNotReady);
    }
    if result < 0 {
        return Err(ContractError::LlmError);
    }
    
    buffer.truncate(result as usize);
    String::from_utf8(buffer).map_err(|_| ContractError::LlmError)
}

/// Convenience function: call LLM with default parameters
/// Temperature: 0.7, Max tokens: 256, Max response: 4096 bytes
pub fn llm_chat(prompt: &str) -> Result<String, ContractError> {
    llm_call(prompt, 70, 256, 4096)
}

/// Convenience function: call LLM for analysis (lower temperature)
/// Temperature: 0.3, Max tokens: 128, Max response: 2048 bytes
pub fn llm_analyze(prompt: &str) -> Result<String, ContractError> {
    llm_call(prompt, 30, 128, 2048)
}

/// Convenience function: call LLM for summarization
/// Temperature: 0.5, Max tokens: 200, Max response: 2048 bytes
pub fn llm_summarize(prompt: &str) -> Result<String, ContractError> {
    llm_call(prompt, 50, 200, 2048)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Growformer agent host (`spacekit_agent`) — in-browser brain WASM via JS host
// ═══════════════════════════════════════════════════════════════════════════════

#[link(wasm_import_module = "spacekit_agent")]
extern "C" {
    /// 0 = not ready, 1 = ready (brain loaded and runtime initialized on host).
    fn agent_growformer_status() -> i32;
    /// Load brain bytes from VM `storage` under UTF-8 key (same as `storage_get`). Returns >0 = ok (byte len), -2 = error, -3 = missing, -4 = runtime not loaded.
    fn agent_growformer_load_brain_from_storage(key_ptr: *const u8, key_len: usize) -> i32;
    fn agent_growformer_generation(
        prompt_ptr: *const u8,
        prompt_len: usize,
        dest_ptr: *mut u8,
        max_len: usize,
    ) -> i32;
    fn agent_growformer_converse(
        prompt_ptr: *const u8,
        prompt_len: usize,
        dest_ptr: *mut u8,
        max_len: usize,
    ) -> i32;
    fn agent_growformer_codegen(
        prompt_ptr: *const u8,
        prompt_len: usize,
        dest_ptr: *mut u8,
        max_len: usize,
    ) -> i32;
    fn agent_growformer_brain_info(dest_ptr: *mut u8, max_len: usize) -> i32;
    fn agent_growformer_reset_conversation();
}

pub mod growformer_status {
    pub const NOT_READY: i32 = 0;
    pub const READY: i32 = 1;
}

pub fn growformer_host_status() -> i32 {
    unsafe { agent_growformer_status() }
}

pub fn growformer_is_ready() -> bool {
    unsafe { agent_growformer_status() == growformer_status::READY }
}

/// Load the Growformer brain from persistent VM storage (host reads `storage_load` key space).
/// Call before `growformer_generation` when the brain was seeded at deploy time (e.g. from chain storage later).
pub fn growformer_load_brain_from_storage_key(key: &str) -> Result<(), ContractError> {
    let n = unsafe { agent_growformer_load_brain_from_storage(key.as_ptr(), key.len()) };
    if n > 0 {
        return Ok(());
    }
    match n {
        -3 => Err(ContractError::StorageError),
        -4 => Err(ContractError::GrowformerNotReady),
        _ => Err(ContractError::GrowformerError),
    }
}

fn growformer_finish_response(result: i32, mut buffer: Vec<u8>) -> Result<String, ContractError> {
    if result == -1 {
        return Err(ContractError::GrowformerNotReady);
    }
    if result < 0 {
        return Err(ContractError::GrowformerError);
    }
    buffer.truncate(result as usize);
    String::from_utf8(buffer).map_err(|_| ContractError::GrowformerError)
}

/// Single-shot Growformer generation; response is UTF-8 JSON (see Growformer JS API).
pub fn growformer_generation(prompt: &str, max_response_len: usize) -> Result<String, ContractError> {
    let mut buffer = vec![0u8; max_response_len];
    let result = unsafe {
        agent_growformer_generation(
            prompt.as_ptr(),
            prompt.len(),
            buffer.as_mut_ptr(),
            buffer.len(),
        )
    };
    growformer_finish_response(result, buffer)
}

/// Multi-turn conversational generation; host keeps conversation state.
pub fn growformer_converse(prompt: &str, max_response_len: usize) -> Result<String, ContractError> {
    let mut buffer = vec![0u8; max_response_len];
    let result = unsafe {
        agent_growformer_converse(
            prompt.as_ptr(),
            prompt.len(),
            buffer.as_mut_ptr(),
            buffer.len(),
        )
    };
    growformer_finish_response(result, buffer)
}

/// Code generation; response is UTF-8 JSON.
pub fn growformer_codegen(prompt: &str, max_response_len: usize) -> Result<String, ContractError> {
    let mut buffer = vec![0u8; max_response_len];
    let result = unsafe {
        agent_growformer_codegen(
            prompt.as_ptr(),
            prompt.len(),
            buffer.as_mut_ptr(),
            buffer.len(),
        )
    };
    growformer_finish_response(result, buffer)
}

/// Brain metadata as UTF-8 JSON.
pub fn growformer_brain_info(max_len: usize) -> Result<String, ContractError> {
    let mut buffer = vec![0u8; max_len];
    let result = unsafe { agent_growformer_brain_info(buffer.as_mut_ptr(), buffer.len()) };
    growformer_finish_response(result, buffer)
}

pub fn growformer_reset_conversation() {
    unsafe { agent_growformer_reset_conversation() };
}

pub fn emit_event_bytes(event_type: &str, data: &[u8]) {
    unsafe {
        emit_event(event_type.as_ptr(), event_type.len(), data.as_ptr(), data.len());
    }
}

pub fn emit_event_signature(signature: &str, data: &[u8]) {
    emit_event_bytes(signature, data)
}

pub fn get_caller_did_string() -> Result<alloc::string::String, ContractError> {
    let mut buffer = alloc::vec![0u8; 256];
    let len = unsafe { get_caller_did(buffer.as_mut_ptr(), buffer.len()) };
    if len <= 0 {
        return Err(ContractError::HostError);
    }
    buffer.truncate(len as usize);
    core::str::from_utf8(&buffer)
        .map(|s| s.to_string())
        .map_err(|_| ContractError::HostError)
}

pub fn verify_did_string(did: &str) -> bool {
    let res = unsafe { verify_did(did.as_ptr(), did.len()) };
    res == 1
}

pub fn msg_value_u64() -> u64 {
    unsafe { msg_value() as u64 }
}

/// Get the native ASTRA balance of a 20-byte address.
/// The address is read as raw bytes from the given slice (first 20 bytes used).
pub fn get_balance_of(address: &[u8; 20]) -> u64 {
    unsafe { get_balance(address.as_ptr()) as u64 }
}

/// Transfer `amount` native ASTRA from the calling contract to the 20-byte
/// destination address. Returns `Ok(())` on success.
pub fn transfer_to(to_address: &[u8; 20], amount: u64) -> Result<(), ContractError> {
    let result = unsafe { transfer(to_address.as_ptr(), amount as i64) };
    if result == 0 {
        Ok(())
    } else {
        Err(ContractError::HostError)
    }
}

/// Require that the caller attached at least `min_amount` native ASTRA to
/// this transaction. Returns the actual attached value on success.
pub fn require_payment(min_amount: u64) -> Result<u64, ContractError> {
    let val = msg_value_u64();
    if val < min_amount {
        Err(ContractError::InsufficientPayment)
    } else {
        Ok(val)
    }
}

/// Get the current block timestamp (seconds since Unix epoch).
pub fn block_timestamp() -> u64 {
    unsafe { get_timestamp() as u64 }
}

mod agent_host;
pub use agent_host::{entitlement, messaging, paymaster, payments, remote_storage, session_keys, tools};

// ═══════════════════════════════════════════════════════════════════════════════
// Storage host functions
// ═══════════════════════════════════════════════════════════════════════════════

#[link(wasm_import_module = "spacekit_storage")]
extern "C" {
    fn storage_save(key_ptr: *const u8, key_len: usize, value_ptr: *const u8, value_len: usize) -> i32;
    fn storage_load(key_ptr: *const u8, key_len: usize, output_ptr: *mut u8, max_len: usize) -> i32;
}

/// Save data to persistent storage
pub fn storage_set(key: &[u8], value: &[u8]) -> Result<(), ContractError> {
    let result = unsafe {
        storage_save(key.as_ptr(), key.len(), value.as_ptr(), value.len())
    };
    if result < 0 {
        Err(ContractError::StorageError)
    } else {
        Ok(())
    }
}

/// Load data from persistent storage
/// Returns None if key doesn't exist
pub fn storage_get(key: &[u8], max_len: usize) -> Option<Vec<u8>> {
    let mut buffer = vec![0u8; max_len];
    let result = unsafe {
        storage_load(key.as_ptr(), key.len(), buffer.as_mut_ptr(), buffer.len())
    };
    if result <= 0 {
        None
    } else {
        buffer.truncate(result as usize);
        Some(buffer)
    }
}

/// Convenience: save a string to storage
pub fn storage_set_string(key: &str, value: &str) -> Result<(), ContractError> {
    storage_set(key.as_bytes(), value.as_bytes())
}

/// Convenience: load a string from storage
pub fn storage_get_string(key: &str, max_len: usize) -> Option<String> {
    storage_get(key.as_bytes(), max_len)
        .and_then(|bytes| String::from_utf8(bytes).ok())
}

/// String-key storage helpers used by AstraRewards and similar contracts.
pub mod spacekit_storage {
    use super::*;

    pub fn storage_save(key: &str, value: &[u8]) -> Result<(), ContractError> {
        storage_set(key.as_bytes(), value)
    }

    pub fn storage_load(key: &str) -> Result<Vec<u8>, ContractError> {
        storage_get(key.as_bytes(), 128).ok_or(ContractError::StorageError)
    }
}

/// Deterministic 32-byte DID hash for contract storage keys (FNV-1a over UTF-8 DID).
pub fn get_caller_did_hash() -> Result<[u8; 32], ContractError> {
    let did = get_caller_did_string()?;
    Ok(hash_did_bytes(did.as_bytes()))
}

/// Hash an arbitrary DID string to a 32-byte key (same algorithm as `get_caller_did_hash`).
pub fn hash_did_bytes(did: &[u8]) -> [u8; 32] {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &b in did {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    let mut out = [0u8; 32];
    out[..8].copy_from_slice(&hash.to_le_bytes());
    for (i, &b) in did.iter().enumerate() {
        out[8 + (i % 24)] ^= b;
    }
    out
}

#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "spacekit_contract")]
extern "C" {
    /// Cross-contract synchronous call (SpaceKit VM host). Returns bytes written to `output`,
    /// or a negative error code (`-1` missing callee / code, `-2` host/WASM error, `-3` max depth 8,
    /// `-4` I/O bounds). Matches `spacekit-js` `host.ts` `spacekit_contract.contract_call`.
    fn contract_call(
        contract_id_ptr: *const u8,
        contract_id_len: usize,
        input_ptr: *const u8,
        input_len: usize,
        output_ptr: *mut u8,
        max_len: usize,
    ) -> i32;
}

#[cfg(not(target_arch = "wasm32"))]
unsafe fn contract_call(
    _contract_id_ptr: *const u8,
    _contract_id_len: usize,
    _input_ptr: *const u8,
    _input_len: usize,
    _output_ptr: *mut u8,
    _max_len: usize,
) -> i32 {
    -1
}

/// Invoke another contract by **20-byte address string** (hex, optional `0x`) with an arbitrary
/// input payload (e.g. JSON matching `JsonContractCodec` in `spacekit-js`).
pub fn contract_call_raw(
    contract_id: &str,
    input: &[u8],
    output: &mut [u8],
) -> Result<usize, ContractError> {
    let n = unsafe {
        contract_call(
            contract_id.as_ptr(),
            contract_id.len(),
            input.as_ptr(),
            input.len(),
            output.as_mut_ptr(),
            output.len(),
        )
    };
    match n {
        i if i > 0 => Ok(i as usize),
        -1 => Err(ContractError::Failed),
        -2 => Err(ContractError::HostError),
        -3 => Err(ContractError::NestedContractDepth),
        -4 => Err(ContractError::InvalidInput),
        _ if n < 0 => Err(ContractError::HostError),
        _ => Ok(0),
    }
}

fn json_string_for_contract_call(s: &str) -> String {
    let mut o = String::with_capacity(s.len().saturating_add(2));
    o.push('"');
    for ch in s.chars() {
        match ch {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                use alloc::format;
                o.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

pub mod prelude {
    extern crate alloc;
    pub use alloc::string::{String, ToString};
    pub use alloc::vec::Vec;
    pub use alloc::vec;
    pub use alloc::format;
    pub use alloc::collections::{BTreeMap as Map, BTreeSet as Set};

    pub type Did = String;
    pub type Address = String;

    pub mod env {
        use super::{Did, String, vec};

        pub fn caller() -> Did {
            crate::get_caller_did_string().unwrap_or_default()
        }

        pub fn block_timestamp() -> u64 {
            crate::block_timestamp()
        }

        pub fn msg_value() -> u64 {
            crate::msg_value_u64()
        }

        /// Require minimum payment attached to the transaction.
        pub fn require_payment(min: u64) -> Result<u64, crate::ContractError> {
            crate::require_payment(min)
        }

        pub fn transfer(to: &[u8; 20], amount: u64) -> Result<(), crate::ContractError> {
            crate::transfer_to(to, amount)
        }

        pub fn balance_of(address: &[u8; 20]) -> u64 {
            crate::get_balance_of(address)
        }

        pub fn emit<T>(_event: &str, _data: &T) {
            crate::emit_event_bytes(_event, &[]);
        }

        /// Cross-contract call using the same JSON envelope as `spacekit-js` `JsonContractCodec.encode`.
        /// Returns the callee output bytes (possibly empty if the callee returned zero length).
        pub fn call_with_output(
            address: String,
            method: &str,
            arg: &Did,
        ) -> Result<super::Vec<u8>, crate::ContractError> {
            use alloc::format;
            let body = format!(
                "{{\"method\":{},\"args\":[{}]}}",
                crate::json_string_for_contract_call(method),
                crate::json_string_for_contract_call(arg)
            );
            let mut buf = vec![0u8; 65536];
            let n = crate::contract_call_raw(address.trim(), body.as_bytes(), &mut buf)?;
            Ok(buf[..n].to_vec())
        }

        /// Same as [`call_with_output`] but only reports whether the host considered the call successful.
        pub fn call(address: String, method: &str, arg: &Did) -> bool {
            call_with_output(address, method, arg).is_ok()
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Macros
// ═══════════════════════════════════════════════════════════════════════════════

/// Main contract macro - exports `main` and `get_result` functions
#[macro_export]
macro_rules! spacekit_contract {
    ($contract_type:ty) => {
        static mut CONTRACT_INSTANCE: core::option::Option<$contract_type> = None;

        #[no_mangle]
        pub extern "C" fn main(input_ptr: i32, input_len: i32) -> i32 {
            let input = unsafe {
                core::slice::from_raw_parts(input_ptr as *const u8, input_len as usize)
            };

            let contract = unsafe {
                if CONTRACT_INSTANCE.is_none() {
                    CONTRACT_INSTANCE = Some(<$contract_type as $crate::SpacekitContract>::init());
                }
                CONTRACT_INSTANCE.as_mut().unwrap()
            };

            match contract.handle(input) {
                Ok(output) => {
                    $crate::set_result(output);
                    $crate::result_len()
                }
                Err(err) => err.code(),
            }
        }

        #[no_mangle]
        pub extern "C" fn get_result(dest_ptr: i32, max_len: i32) -> i32 {
            $crate::copy_result(dest_ptr, max_len)
        }
    };
}
