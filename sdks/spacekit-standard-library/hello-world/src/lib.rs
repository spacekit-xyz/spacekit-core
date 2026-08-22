//! SpaceKit Standard Library - Hello World
//!
//! This library provides a simple hello world contract for the SpaceKit platform.
//! It demonstrates the basic functionality of the SpaceKit contract SDK.
//!
//! # Examples
//!
//! let mut input = Vec::new();
//! input.extend_from_slice(&5u16.to_le_bytes()); // length prefix
//! input.extend_from_slice(b"World");
//! let result = handle(&input).unwrap();
//! assert_eq!(result, b"Hello, World!");

#![no_std]

extern crate alloc;

use spacekit_contract_sdk::{
    emit_event_bytes, spacekit_contract, 
    SpacekitContract, ContractError, ContractErrorCode,
    wire::read_string
};

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

#[cfg(all(target_arch = "wasm32", not(test)))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

struct SpacekitHelloWorldContract;

impl SpacekitContract for SpacekitHelloWorldContract {
    type Error = ContractError;

    fn init() -> Self {
        SpacekitHelloWorldContract
    }

    fn handle(&mut self, input: &[u8]) -> Result<Vec<u8>, ContractError> {
        handle(input)
    }
}

spacekit_contract!(SpacekitHelloWorldContract);

fn handle(input: &[u8]) -> Result<Vec<u8>, ContractError> {
    let mut cursor = 0usize;
    let content = read_string(input, &mut cursor)?;
    let result = format!("Hello, {}!", content);
    emit_event_bytes("spacekit.hello_world", result.as_bytes());
    Ok(result.into_bytes().into())
}
