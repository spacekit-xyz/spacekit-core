//! SpaceKit Smart Contract - Base contract for all SpaceKit contracts
#![no_std]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::ptr;
use serde::{Deserialize, Serialize};

// Set up global allocator
#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

// ========== HOST FUNCTION IMPORTS ==========

#[link(wasm_import_module = "swtch_llm")]
extern "C" {
    fn llm_inference(
        model_id_ptr: *const u8,
        model_id_len: usize,
        prompt_ptr: *const u8,
        prompt_len: usize,
        max_tokens: i32,
        temperature: f32,
    ) -> i32;
    fn llm_response_len() -> i32;
    fn llm_response_copy(dest_ptr: *mut u8, max_len: i32) -> i32;
}

#[link(wasm_import_module = "swtch_storage")]
extern "C" {
    fn storage_save(
        key_ptr: *const u8,
        key_len: usize,
        data_ptr: *const u8,
        data_len: usize,
    ) -> i32;
    fn storage_load(key_ptr: *const u8, key_len: usize, dest_ptr: *mut u8, max_len: usize)
        -> usize;
}

#[derive(Clone)]
pub struct SpacekitDatasource {
    pub name: String,
    pub url: String,
}

/// Base contract for all SpaceKit contracts
pub struct SpacekitContract {
    pub contract_address: String,
    pub contract_wasm: Vec<u8>,
    // Data sources for the contract to access API endpoints for the contract to access external data
    pub datasources: Vec<SpacekitDatasource>,
}

/// Standard contract error codes returned to the runtime.
#[derive(Debug, Clone, Copy)]
pub enum ContractError {
    /// Generic failure
    Failed = -1,
    /// Invalid input
    InvalidInput = -2,
    /// Storage error
    StorageError = -3,
    /// Host call failed
    HostError = -4,
}

impl ContractError {
    pub fn code(self) -> i32 {
        self as i32
    }
}

/// Trait implemented by WASM contracts that run in SpaceKit compute nodes.
pub trait SpacekitContractTrait {
    /// Initialize the contract (called once on first entry).
    fn init() -> Self;
    /// Handle an invocation payload.
    fn handle(&mut self, input: &[u8]) -> Result<Vec<u8>, ContractError>;
}

// ========= RESULT BUFFER HELPERS =========

static mut LAST_RESULT: Option<Vec<u8>> = None;

pub fn set_result(data: Vec<u8>) {
    unsafe {
        LAST_RESULT = Some(data);
    }
}

pub fn result_len() -> i32 {
    unsafe {
        LAST_RESULT
            .as_ref()
            .map(|data| data.len() as i32)
            .unwrap_or(0)
    }
}

pub fn copy_result(dest_ptr: i32, max_len: i32) -> i32 {
    unsafe {
        if let Some(data) = LAST_RESULT.as_ref() {
            let len = core::cmp::min(data.len(), max_len as usize);
            ptr::copy_nonoverlapping(data.as_ptr(), dest_ptr as *mut u8, len);
            return len as i32;
        }
    }
    0
}

/// Macro to export standard entrypoints for SpaceKit contracts.
///
/// Exports:
/// - `main(i32, i32) -> i32` for invocation
/// - `get_result(i32, i32) -> i32` for result retrieval
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
                    CONTRACT_INSTANCE = Some(<$contract_type as $crate::v1::contracts::spacekit::SpacekitContractTrait>::init());
                }
                CONTRACT_INSTANCE.as_mut().unwrap()
            };

            match contract.handle(input) {
                Ok(output) => {
                    $crate::v1::contracts::spacekit::set_result(output);
                    $crate::v1::contracts::spacekit::result_len()
                }
                Err(err) => err.code(),
            }
        }

        #[no_mangle]
        pub extern "C" fn get_result(dest_ptr: i32, max_len: i32) -> i32 {
            $crate::v1::contracts::spacekit::copy_result(dest_ptr, max_len)
        }
    };
}

// ========= HOST HELPERS =========

pub fn storage_save_bytes(key: &str, data: &[u8]) -> Result<(), ContractError> {
    let result = unsafe { storage_save(key.as_ptr(), key.len(), data.as_ptr(), data.len()) };
    if result >= 0 {
        Ok(())
    } else {
        Err(ContractError::StorageError)
    }
}

pub fn storage_load_bytes(key: &str, max_len: usize) -> Result<Vec<u8>, ContractError> {
    let mut buffer = vec![0u8; max_len];
    let read_len = unsafe { storage_load(key.as_ptr(), key.len(), buffer.as_mut_ptr(), max_len) };
    if read_len == 0 {
        return Err(ContractError::StorageError);
    }
    buffer.truncate(read_len);
    Ok(buffer)
}

pub fn llm_infer(
    model_id: &str,
    prompt: &str,
    max_tokens: i32,
    temperature: f32,
) -> Result<String, ContractError> {
    let status = unsafe {
        llm_inference(
            model_id.as_ptr(),
            model_id.len(),
            prompt.as_ptr(),
            prompt.len(),
            max_tokens,
            temperature,
        )
    };
    if status < 0 {
        return Err(ContractError::HostError);
    }

    let len = unsafe { llm_response_len() };
    if len <= 0 {
        return Err(ContractError::HostError);
    }

    let mut buffer = vec![0u8; len as usize];
    let copied = unsafe { llm_response_copy(buffer.as_mut_ptr(), len) };
    if copied <= 0 {
        return Err(ContractError::HostError);
    }

    buffer.truncate(copied as usize);
    let response = core::str::from_utf8(&buffer)
        .map_err(|_| ContractError::HostError)?
        .to_string();
    Ok(response)
}

impl SpacekitContract {
    pub fn new(
        contract_address: String,
        contract_wasm: Vec<u8>,
        datasources: Vec<SpacekitDatasource>,
    ) -> Self {
        Self {
            contract_address,
            contract_wasm,
            datasources,
        }
    }

    pub fn get_contract_address(&self) -> String {
        self.contract_address.clone()
    }

    pub fn get_contract_wasm(&self) -> Vec<u8> {
        self.contract_wasm.clone()
    }

    pub fn get_datasources(&self) -> Vec<SpacekitDatasource> {
        self.datasources.clone()
    }

    pub fn add_datasource(&mut self, datasource: SpacekitDatasource) {
        self.datasources.push(datasource);
    }

    pub fn remove_datasource(&mut self, datasource: SpacekitDatasource) {
        self.datasources.retain(|d| d.name != datasource.name);
    }
}
