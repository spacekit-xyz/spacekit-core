//! App License NFT Contract
//! 
//! Each app has its own NFT contract instance
//! Owning an NFT = owning a license to use the app
//! 
//! Features:
//! - Standard NFT operations (mint, transfer, burn)
//! - License metadata (purchase price, version, expiry)
//! - Ownership verification
//! - Transferable licenses (for resale)
//! - Update access (NFT holders get free updates)

#![no_std]

extern crate alloc;
use alloc::string::String;
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

#[cfg(target_arch = "wasm32")]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

/// License types
#[repr(u8)]
pub enum LicenseType {
    Personal = 0,      // Single user, non-transferable
    Commercial = 1,    // Transferable, commercial use allowed
    Enterprise = 2,    // Multi-seat, transferable
}

/// External functions
#[link(wasm_import_module = "env")]
extern "C" {
    fn storage_read(key_ptr: *const u8, key_len: usize, output_ptr: *mut u8, max_len: usize) -> i32;
    fn storage_write(key_ptr: *const u8, key_len: usize, value_ptr: *const u8, value_len: usize) -> i32;
    fn get_caller_did(output_ptr: *mut u8, max_len: usize) -> usize;
    fn log_output(ptr: *const u8, len: usize);
    fn emit_event(event_type_ptr: *const u8, event_type_len: usize, data_ptr: *const u8, data_len: usize);
    fn get_timestamp() -> u64;
}

/// Initialize the NFT contract
/// Called once when deploying a new app's NFT contract
#[no_mangle]
pub extern "C" fn initialize(
    app_id: u64,
    app_name_ptr: *const u8,
    app_name_len: usize,
    license_type: u8,
) -> i32 {
    // Store contract metadata
    unsafe {
        storage_write(b"app_id".as_ptr(), 6, &app_id as *const u64 as *const u8, 8);
        storage_write(b"license_type".as_ptr(), 12, &license_type as *const u8, 1);
        
        // Initialize token counter
        let counter: u64 = 0;
        storage_write(b"next_token_id".as_ptr(), 13, &counter as *const u64 as *const u8, 8);
    }
    
    unsafe {
        log_output(b"[SUCCESS] License NFT contract initialized\0".as_ptr(), 44);
    }
    
    1 // Success
}

/// Mint a new license NFT
/// Only callable by AppStore contract
#[no_mangle]
pub extern "C" fn mint(
    owner_did_ptr: *const u8,
    owner_did_len: usize,
    purchase_price: u64,
    version_ptr: *const u8,
    version_len: usize,
) -> u64 {
    // Get caller (should be AppStore contract)
    let mut caller_did_buf = [0u8; 256];
    let caller_did_len = unsafe { get_caller_did(caller_did_buf.as_mut_ptr(), 256) };
    let caller_did = unsafe {
        core::str::from_utf8_unchecked(&caller_did_buf[..caller_did_len])
    };
    
    // Verify caller is authorized (AppStore contract)
    // In production: check caller == appstore_contract_address
    // For demo, allow any caller
    
    // Get next token ID
    let mut token_id: u64 = 0;
    unsafe {
        let mut id_buf = [0u8; 8];
        let read_len = storage_read(b"next_token_id".as_ptr(), 13, id_buf.as_mut_ptr(), 8);
        if read_len == 8 {
            token_id = u64::from_le_bytes(id_buf);
        }
    }
    
    let new_token_id = token_id + 1;
    
    // Store token ownership
    let owner_key = format_token_key(b"owner:", new_token_id);
    unsafe {
        storage_write(
            owner_key.as_ptr(),
            owner_key.len(),
            owner_did_ptr,
            owner_did_len,
        );
    }
    
    // Store license metadata: price(8) + timestamp(8) + content_id bytes (from version field)
    let metadata_key = format_token_key(b"metadata:", new_token_id);
    let timestamp = unsafe { get_timestamp() };
    let mut metadata = Vec::new();
    metadata.extend_from_slice(&purchase_price.to_le_bytes());
    metadata.extend_from_slice(&timestamp.to_le_bytes());
    let content_id = unsafe { core::slice::from_raw_parts(version_ptr, version_len) };
    metadata.extend_from_slice(&(content_id.len() as u16).to_le_bytes());
    metadata.extend_from_slice(content_id);
    
    unsafe {
        storage_write(
            metadata_key.as_ptr(),
            metadata_key.len(),
            metadata.as_ptr(),
            metadata.len(),
        );
    }
    
    // Add to owner's token list
    let owner_tokens_key = format_owner_tokens_key(owner_did_ptr, owner_did_len, new_token_id);
    unsafe {
        storage_write(
            owner_tokens_key.as_ptr(),
            owner_tokens_key.len(),
            &new_token_id as *const u64 as *const u8,
            8,
        );
    }
    
    // Update counter
    unsafe {
        storage_write(b"next_token_id".as_ptr(), 13, &new_token_id as *const u64 as *const u8, 8);
    }
    
    // Emit Transfer event (mint = transfer from 0x0)
    unsafe {
        emit_event(
            b"Transfer".as_ptr(),
            8,
            &new_token_id as *const u64 as *const u8,
            8,
        );
    }
    
    unsafe {
        log_output(b"[SUCCESS] License NFT minted\0".as_ptr(), 30);
    }
    
    new_token_id
}

/// Transfer a license NFT
#[no_mangle]
pub extern "C" fn transfer(
    token_id: u64,
    to_did_ptr: *const u8,
    to_did_len: usize,
) -> i32 {
    // Get caller
    let mut caller_did_buf = [0u8; 256];
    let caller_did_len = unsafe { get_caller_did(caller_did_buf.as_mut_ptr(), 256) };
    let caller_did = unsafe {
        core::str::from_utf8_unchecked(&caller_did_buf[..caller_did_len])
    };
    
    // Check if caller owns the token
    let owner_key = format_token_key(b"owner:", token_id);
    let mut current_owner_buf = [0u8; 256];
    let owner_len = unsafe {
        storage_read(owner_key.as_ptr(), owner_key.len(), current_owner_buf.as_mut_ptr(), 256)
    };
    
    if owner_len < 0 {
        return -1; // Token doesn't exist
    }
    
    let current_owner = unsafe {
        core::str::from_utf8_unchecked(&current_owner_buf[..owner_len as usize])
    };
    
    if current_owner != caller_did {
        return -2; // Caller doesn't own this token
    }
    
    // Check if license is transferable
    let mut license_type: u8 = 0;
    unsafe {
        let mut type_buf = [0u8; 1];
        storage_read(b"license_type".as_ptr(), 12, type_buf.as_mut_ptr(), 1);
        license_type = type_buf[0];
    }
    
    if license_type == LicenseType::Personal as u8 {
        unsafe {
            log_output(b"[ERROR] Personal licenses are non-transferable\0".as_ptr(), 49);
        }
        return -3; // Personal licenses cannot be transferred
    }
    
    // Remove from old owner's token list
    // (In production: maintain proper index)
    
    // Update ownership
    let to_did = unsafe {
        core::str::from_utf8_unchecked(core::slice::from_raw_parts(to_did_ptr, to_did_len))
    };
    
    unsafe {
        storage_write(
            owner_key.as_ptr(),
            owner_key.len(),
            to_did_ptr,
            to_did_len,
        );
    }
    
    // Add to new owner's token list
    let new_owner_tokens_key = format_owner_tokens_key(to_did_ptr, to_did_len, token_id);
    unsafe {
        storage_write(
            new_owner_tokens_key.as_ptr(),
            new_owner_tokens_key.len(),
            &token_id as *const u64 as *const u8,
            8,
        );
    }
    
    // Emit Transfer event
    unsafe {
        emit_event(
            b"Transfer".as_ptr(),
            8,
            &token_id as *const u64 as *const u8,
            8,
        );
    }
    
    unsafe {
        log_output(b"[SUCCESS] License transferred\0".as_ptr(), 31);
    }
    
    1 // Success
}

/// Get owner of a token
#[no_mangle]
pub extern "C" fn owner_of(token_id: u64, output_ptr: *mut u8, max_len: usize) -> i32 {
    let owner_key = format_token_key(b"owner:", token_id);
    
    unsafe {
        storage_read(owner_key.as_ptr(), owner_key.len(), output_ptr, max_len)
    }
}

/// Check if a DID owns any token (has license)
#[no_mangle]
pub extern "C" fn has_license(did_ptr: *const u8, did_len: usize) -> i32 {
    if did_owner_has_any_token(did_ptr, did_len) {
        1
    } else {
        0
    }
}

/// Check if a DID owns a license for a specific content_id (stored in mint metadata).
#[no_mangle]
pub extern "C" fn has_license_for_content(
    did_ptr: *const u8,
    did_len: usize,
    content_id_ptr: *const u8,
    content_id_len: usize,
) -> i32 {
    if did_owns_content_license(did_ptr, did_len, content_id_ptr, content_id_len) {
        1
    } else {
        0
    }
}

/// Get token metadata
#[no_mangle]
pub extern "C" fn get_metadata(token_id: u64, output_ptr: *mut u8, max_len: usize) -> i32 {
    let metadata_key = format_token_key(b"metadata:", token_id);
    
    unsafe {
        storage_read(metadata_key.as_ptr(), metadata_key.len(), output_ptr, max_len)
    }
}

/// Get all tokens owned by a DID
#[no_mangle]
pub extern "C" fn tokens_of(
    owner_did_ptr: *const u8,
    owner_did_len: usize,
    output_ptr: *mut u8,
    max_len: usize,
) -> i32 {
    // In production: query and return array of token IDs
    // For demo, return empty array
    
    0
}

/// Burn a token (revoke license)
/// Only callable by owner or authorized party
#[no_mangle]
pub extern "C" fn burn(token_id: u64) -> i32 {
    // Get caller
    let mut caller_did_buf = [0u8; 256];
    let caller_did_len = unsafe { get_caller_did(caller_did_buf.as_mut_ptr(), 256) };
    let caller_did = unsafe {
        core::str::from_utf8_unchecked(&caller_did_buf[..caller_did_len])
    };
    
    // Verify ownership
    let owner_key = format_token_key(b"owner:", token_id);
    let mut owner_buf = [0u8; 256];
    let owner_len = unsafe {
        storage_read(owner_key.as_ptr(), owner_key.len(), owner_buf.as_mut_ptr(), 256)
    };
    
    if owner_len < 0 {
        return -1; // Token doesn't exist
    }
    
    let owner = unsafe {
        core::str::from_utf8_unchecked(&owner_buf[..owner_len as usize])
    };
    
    if owner != caller_did {
        return -2; // Only owner can burn
    }
    
    // Delete token data
    // In production: properly clean up all storage
    
    unsafe {
        emit_event(
            b"Burn".as_ptr(),
            4,
            &token_id as *const u64 as *const u8,
            8,
        );
    }
    
    1 // Success
}

/// Get total supply of licenses
#[no_mangle]
pub extern "C" fn total_supply() -> u64 {
    let mut supply: u64 = 0;
    
    unsafe {
        let mut supply_buf = [0u8; 8];
        let read_len = storage_read(b"next_token_id".as_ptr(), 13, supply_buf.as_mut_ptr(), 8);
        if read_len == 8 {
            supply = u64::from_le_bytes(supply_buf);
        }
    }
    
    supply
}

// ============================================================================
// Helper Functions
// ============================================================================

fn format_token_key(prefix: &[u8], token_id: u64) -> Vec<u8> {
    let mut key = Vec::new();
    key.extend_from_slice(prefix);
    key.extend_from_slice(&token_id.to_le_bytes());
    key
}

static mut RESULT_BUF: [u8; 512] = [0u8; 512];
static mut RESULT_LEN: usize = 0;

fn set_result(bytes: &[u8]) {
    unsafe {
        RESULT_LEN = bytes.len().min(RESULT_BUF.len());
        RESULT_BUF[..RESULT_LEN].copy_from_slice(&bytes[..RESULT_LEN]);
    }
}

/// SwtchVM entry: opcode dispatch (see `content_license` module in storage-node).
/// OP 0x01 mint, OP 0x02 has_license_for_content.
#[no_mangle]
pub extern "C" fn main(call_data_ptr: *const u8, call_data_len: usize) -> i32 {
    if call_data_len == 0 {
        return -1;
    }
    let data = unsafe { core::slice::from_raw_parts(call_data_ptr, call_data_len) };
    match data.first().copied() {
        Some(0x01) => handle_op_mint(&data[1..]),
        Some(0x02) => handle_op_has_license(&data[1..]),
        _ => -1,
    }
}

#[no_mangle]
pub extern "C" fn get_result(out_ptr: *mut u8, max_len: usize) -> i32 {
    unsafe {
        let n = RESULT_LEN.min(max_len);
        core::ptr::copy_nonoverlapping(RESULT_BUF.as_ptr(), out_ptr, n);
        n as i32
    }
}

fn read_string<'a>(data: &'a [u8], off: &mut usize) -> Option<&'a [u8]> {
    if *off + 2 > data.len() {
        return None;
    }
    let len = u16::from_le_bytes([data[*off], data[*off + 1]]) as usize;
    *off += 2;
    if *off + len > data.len() {
        return None;
    }
    let s = &data[*off..*off + len];
    *off += len;
    Some(s)
}

fn handle_op_mint(data: &[u8]) -> i32 {
    let mut off = 0;
    let owner = match read_string(data, &mut off) {
        Some(s) => s,
        None => return -1,
    };
    let content_id = match read_string(data, &mut off) {
        Some(s) => s,
        None => return -1,
    };
    if off + 8 > data.len() {
        return -1;
    }
    let price = u64::from_le_bytes([
        data[off],
        data[off + 1],
        data[off + 2],
        data[off + 3],
        data[off + 4],
        data[off + 5],
        data[off + 6],
        data[off + 7],
    ]);
    let token_id = mint(
        owner.as_ptr(),
        owner.len(),
        price,
        content_id.as_ptr(),
        content_id.len(),
    );
    set_result(&token_id.to_le_bytes());
    8
}

fn handle_op_has_license(data: &[u8]) -> i32 {
    let mut off = 0;
    let owner = match read_string(data, &mut off) {
        Some(s) => s,
        None => return -1,
    };
    let content_id = match read_string(data, &mut off) {
        Some(s) => s,
        None => return -1,
    };
    let ok = did_owns_content_license(
        owner.as_ptr(),
        owner.len(),
        content_id.as_ptr(),
        content_id.len(),
    );
    set_result(&[if ok { 1u8 } else { 0u8 }]);
    1
}

fn total_supply_count() -> u64 {
    let mut supply: u64 = 0;
    unsafe {
        let mut supply_buf = [0u8; 8];
        let read_len = storage_read(b"next_token_id".as_ptr(), 13, supply_buf.as_mut_ptr(), 8);
        if read_len == 8 {
            supply = u64::from_le_bytes(supply_buf);
        }
    }
    supply
}

fn did_matches_owner(token_id: u64, did_ptr: *const u8, did_len: usize) -> bool {
    let owner_key = format_token_key(b"owner:", token_id);
    let mut owner_buf = [0u8; 256];
    let owner_len = unsafe {
        storage_read(
            owner_key.as_ptr(),
            owner_key.len(),
            owner_buf.as_mut_ptr(),
            256,
        )
    };
    if owner_len <= 0 {
        return false;
    }
    let owner = unsafe { core::slice::from_raw_parts(did_ptr, did_len) };
    let stored = unsafe { core::slice::from_raw_parts(owner_buf.as_ptr(), owner_len as usize) };
    owner == stored
}

fn metadata_content_id(token_id: u64) -> Option<Vec<u8>> {
    let metadata_key = format_token_key(b"metadata:", token_id);
    let mut buf = [0u8; 512];
    let read_len =
        unsafe { storage_read(metadata_key.as_ptr(), metadata_key.len(), buf.as_mut_ptr(), 512) };
    if read_len < 18 {
        return None;
    }
    let len = u16::from_le_bytes([buf[16], buf[17]]) as usize;
    if read_len as usize >= 18 + len {
        Some(buf[18..18 + len].to_vec())
    } else {
        None
    }
}

fn did_owner_has_any_token(did_ptr: *const u8, did_len: usize) -> bool {
    let supply = total_supply_count();
    for token_id in 1..=supply {
        if did_matches_owner(token_id, did_ptr, did_len) {
            return true;
        }
    }
    false
}

fn did_owns_content_license(
    did_ptr: *const u8,
    did_len: usize,
    content_id_ptr: *const u8,
    content_id_len: usize,
) -> bool {
    let want = unsafe { core::slice::from_raw_parts(content_id_ptr, content_id_len) };
    let supply = total_supply_count();
    for token_id in 1..=supply {
        if !did_matches_owner(token_id, did_ptr, did_len) {
            continue;
        }
        if let Some(cid) = metadata_content_id(token_id) {
            if cid.as_slice() == want {
                return true;
            }
        }
    }
    false
}

fn format_owner_tokens_key(owner_did_ptr: *const u8, owner_did_len: usize, token_id: u64) -> Vec<u8> {
    let mut key = Vec::new();
    key.extend_from_slice(b"owner_tokens:");
    
    let owner_did = unsafe {
        core::slice::from_raw_parts(owner_did_ptr, owner_did_len)
    };
    key.extend_from_slice(owner_did);
    key.push(b':');
    key.extend_from_slice(&token_id.to_le_bytes());
    
    key
}

