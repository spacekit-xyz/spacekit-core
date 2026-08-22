#![no_std]

extern crate alloc;

use core::alloc::{GlobalAlloc, Layout};

// Simple bump allocator for WASM
struct BumpAllocator;

static mut HEAP: [u8; 64 * 1024] = [0; 64 * 1024]; // 64KB heap
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
    
    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        // Bump allocator doesn't free
    }
}

#[global_allocator]
static ALLOCATOR: BumpAllocator = BumpAllocator;

#[cfg(all(target_arch = "wasm32", not(test)))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[link(wasm_import_module = "swtch_compress")]
extern "C" {
    /// Call Python SWTCH compressor
    fn python_compress(
        input_ptr: *const u8,
        input_len: usize,
        mode_ptr: *const u8,
        mode_len: usize,
        output_ptr: *mut u8,
        output_max_len: usize,
    ) -> i32;
}

#[link(wasm_import_module = "env")]
extern "C" {
    fn log_output(ptr: *const u8, len: usize);
}

/// Log helper
fn log(msg: &str) {
    unsafe {
        log_output(msg.as_ptr(), msg.len());
    }
}

/// Main entry point
#[no_mangle]
pub extern "C" fn main(input_ptr: i32, input_len: i32) -> i32 {
    // Get input data from WASM memory
    let input_data = unsafe {
        core::slice::from_raw_parts(input_ptr as *const u8, input_len as usize)
    };
    
    // Default mode: adaptive
    let mode = "adaptive";
    
    // Output buffer
    const MAX_OUTPUT: usize = 65536; // 64KB
    let mut output = [0u8; MAX_OUTPUT];
    
    // Call Python SWTCH compressor
    let compressed_len = unsafe {
        python_compress(
            input_data.as_ptr(),
            input_data.len(),
            mode.as_ptr(),
            mode.len(),
            output.as_mut_ptr(),
            MAX_OUTPUT,
        )
    };
    
    if compressed_len > 0 {
        compressed_len
    } else {
        0 // Compression failed
    }
}

/// Compress with specific mode
#[no_mangle]
pub extern "C" fn compress_with_mode(
    input_ptr: i32,
    input_len: i32,
    mode_ptr: i32,
    mode_len: i32,
) -> i32 {
    let input_data = unsafe {
        core::slice::from_raw_parts(input_ptr as *const u8, input_len as usize)
    };
    
    let mode = unsafe {
        core::str::from_utf8_unchecked(
            core::slice::from_raw_parts(mode_ptr as *const u8, mode_len as usize)
        )
    };
    
    let mut output = [0u8; 65536];
    
    let compressed_len = unsafe {
        python_compress(
            input_data.as_ptr(),
            input_data.len(),
            mode.as_ptr(),
            mode.len(),
            output.as_mut_ptr(),
            65536,
        )
    };
    
    compressed_len
}

