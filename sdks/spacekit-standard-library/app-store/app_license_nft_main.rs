// App License NFT Contract Main Entry Point
// This is a wrapper to build app_license_nft.rs as a binary

#![no_std]
#![no_main]

extern crate alloc;
use alloc::vec::Vec;
use core::alloc::{GlobalAlloc, Layout};

// Bump allocator
struct BumpAllocator;
static mut HEAP: [u8; 64 * 1024] = [0; 64 * 1024];
static mut HEAP_POS: usize = 0;

unsafe impl GlobalAlloc for BumpAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pos = HEAP_POS;
        let align = layout.align();
        let size = layout.size();
        let aligned_pos = (pos + align - 1) & !(align - 1);
        let new_pos = aligned_pos + size;
        if new_pos > HEAP.len() {
            return core::ptr::null_mut();
        }
        HEAP_POS = new_pos;
        HEAP.as_mut_ptr().add(aligned_pos)
    }
    
    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}

#[global_allocator]
static ALLOCATOR: BumpAllocator = BumpAllocator;

#[cfg(all(target_arch = "wasm32", not(test)))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

// License types
#[repr(u8)]
pub enum LicenseType {
    Personal = 0,      // Single user, non-transferable
    Commercial = 1,    // Transferable, commercial use allowed
    Enterprise = 2,    // Multi-seat, transferable
}

// External functions
#[link(wasm_import_module = "env")]
extern "C" {
    fn storage_read(key_ptr: *const u8, key_len: usize, output_ptr: *mut u8, max_len: usize) -> i32;
    fn storage_write(key_ptr: *const u8, key_len: usize, value_ptr: *const u8, value_len: usize) -> i32;
    fn get_caller_did(output_ptr: *mut u8, max_len: usize) -> usize;
    fn log_output(ptr: *const u8, len: usize);
    fn emit_event(event_type_ptr: *const u8, event_type_len: usize, data_ptr: *const u8, data_len: usize);
    fn get_timestamp() -> u64;
}

// Entry point for WASM binary
#[no_mangle]
pub extern "C" fn _start() {
    // WASM contracts don't need a main function
    // They are called via exported functions
}

// Main entry function (required for binary)
fn main() {}

// Re-export all functions from app_license_nft.rs
mod license_nft {
    include!("app_license_nft_lib.rs");
}

pub use license_nft::*;

