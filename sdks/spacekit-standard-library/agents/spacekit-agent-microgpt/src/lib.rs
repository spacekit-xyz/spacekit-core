//! SpaceKit Micro-GPT Agent Contract
//!
//! Uses the `spacekit_microgpt` host primitive (microgpt_forward) for deterministic
//! next-token prediction. No LLM; pure on-chain forward pass.
//!
//! Operations:
//!   OP_NEXT_TOKEN (1): [op][token_id:u32 LE][pos_id:u32 LE] -> [next_token_id:u8]
//!   OP_CHAT_STEP (2):  same as OP_NEXT_TOKEN (alias for host-driven chat loops)

#![no_std]

extern crate alloc;
extern crate libm;

use alloc::vec;
use alloc::vec::Vec;
use libm::expf;

use spacekit_contract_sdk::{
    spacekit_contract, ContractError, ContractErrorCode, SpacekitContract,
};

#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

#[cfg(all(target_arch = "wasm32", not(test)))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

// Host: spacekit_microgpt.microgpt_forward(token_id, pos_id, out_ptr) writes VOCAB_SIZE f32s
#[link(wasm_import_module = "spacekit_microgpt")]
extern "C" {
    fn microgpt_forward(token_id: u32, pos_id: u32, out_ptr: *mut u8);
}

const VOCAB_SIZE: usize = 8;
const LOGITS_BYTES: usize = VOCAB_SIZE * 4; // 32 bytes

const OP_NEXT_TOKEN: u8 = 1;
const OP_CHAT_STEP: u8 = 2;

struct MicroGptAgent;

impl SpacekitContract for MicroGptAgent {
    type Error = ContractError;

    fn init() -> Self {
        MicroGptAgent
    }

    fn handle(&mut self, input: &[u8]) -> Result<Vec<u8>, ContractError> {
        if input.len() < 9 {
            return Err(ContractError::InvalidInput);
        }
        let op = input[0];
        if op != OP_NEXT_TOKEN && op != OP_CHAT_STEP {
            return Err(ContractError::InvalidInput);
        }
        let token_id = u32::from_le_bytes([input[1], input[2], input[3], input[4]]);
        let pos_id = u32::from_le_bytes([input[5], input[6], input[7], input[8]]);

        let mut logits_buf = vec![0u8; LOGITS_BYTES];
        unsafe {
            microgpt_forward(token_id, pos_id, logits_buf.as_mut_ptr());
        }

        let logits: Vec<f32> = logits_buf
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        let next_id = argmax(&softmax_in_place(logits));
        Ok(vec![next_id as u8])
    }
}

#[cfg(not(test))]
spacekit_contract!(MicroGptAgent);

fn softmax_in_place(mut logits: Vec<f32>) -> Vec<f32> {
    let max = logits.iter().fold(core::f32::NEG_INFINITY, |a, &b| libm::fmaxf(a, b));
    let sum: f32 = logits.iter().map(|&x| expf(x - max)).sum();
    for x in logits.iter_mut() {
        *x = expf(*x - max) / sum;
    }
    logits
}

fn argmax(logits: &[f32]) -> usize {
    let mut best = 0usize;
    let mut best_v = logits[0];
    for (i, &v) in logits.iter().enumerate().skip(1) {
        if v > best_v {
            best_v = v;
            best = i;
        }
    }
    best
}
