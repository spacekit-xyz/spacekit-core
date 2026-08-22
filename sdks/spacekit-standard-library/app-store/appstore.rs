//! SpaceKit AppStore Smart Contract
//! 
//! Decentralized app marketplace with NFT-based licensing
//! 
//! Features:
//! - App publishing with fact package integration
//! - NFT license minting (ownership = license to use app)
//! - Revenue distribution (99% creator, 1% platform)
//! - Featured apps and discovery
//! - Reputation-gated publishing
//! - Version management and updates
//! - Category-based browsing
//! - Free and paid apps

#![no_std]

extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use core::alloc::{GlobalAlloc, Layout};

// Bump allocator for WASM
struct BumpAllocator;
static mut HEAP: [u8; 128 * 1024] = [0; 128 * 1024]; // 128KB heap
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

/// App categories
#[repr(u8)]
#[derive(Clone, Copy, PartialEq)]
pub enum AppCategory {
    Productivity = 0,
    Gaming = 1,
    DeFi = 2,
    Social = 3,
    DevTools = 4,
    Media = 5,
}

/// License types
#[repr(u8)]
#[derive(Clone, Copy)]
pub enum LicenseType {
    Personal = 0,      // Single user, non-transferable
    Commercial = 1,    // Transferable, commercial use
    Enterprise = 2,    // Multi-seat, transferable
}

/// App listing status
#[repr(u8)]
pub enum AppStatus {
    Active = 0,
    Suspended = 1,
    Deprecated = 2,
}

/// Platform fee (in basis points: 100 = 1%)
const PLATFORM_FEE_BPS: u64 = 100; // 1%
const BASIS_POINTS: u64 = 10000;

/// Minimum reputation to publish apps
const MIN_PUBLISHER_REPUTATION: i64 = 100;

/// External functions from SpaceKitVM
#[link(wasm_import_module = "env")]
extern "C" {
    fn storage_read(key_ptr: *const u8, key_len: usize, output_ptr: *mut u8, max_len: usize) -> i32;
    fn storage_write(key_ptr: *const u8, key_len: usize, value_ptr: *const u8, value_len: usize) -> i32;
    fn get_caller_did(output_ptr: *mut u8, max_len: usize) -> usize;
    fn log_output(ptr: *const u8, len: usize);
    fn emit_event(event_type_ptr: *const u8, event_type_len: usize, data_ptr: *const u8, data_len: usize);
}

#[link(wasm_import_module = "spacekit_reputation")]
extern "C" {
    fn reputation_get_score(did_ptr: *const u8, did_len: usize, rep_type: u8) -> i64;
}

#[link(wasm_import_module = "astra_erc20")]
extern "C" {
    fn token_transfer(from_ptr: *const u8, from_len: usize, to_ptr: *const u8, to_len: usize, amount: u64) -> i32;
    fn token_balance(did_ptr: *const u8, did_len: usize) -> u64;
}

#[link(wasm_import_module = "astra_erc721")]
extern "C" {
    fn nft_mint(contract_id_ptr: *const u8, contract_id_len: usize, owner_ptr: *const u8, owner_len: usize) -> u64;
    fn nft_owner_of(contract_id_ptr: *const u8, contract_id_len: usize, token_id: u64, output_ptr: *mut u8, max_len: usize) -> i32;
    fn nft_transfer(contract_id_ptr: *const u8, contract_id_len: usize, token_id: u64, to_ptr: *const u8, to_len: usize) -> i32;
}

#[link(wasm_import_module = "spacekit_fact")]
extern "C" {
    fn fact_verify_hash(package_id_ptr: *const u8, package_id_len: usize, hash_ptr: *const u8, hash_len: usize) -> i32;
    fn fact_package_exists(package_id_ptr: *const u8, package_id_len: usize) -> i32;
}

/// Publish a new app to the marketplace
/// 
/// Flow:
/// 1. Verify publisher has sufficient reputation
/// 2. Validate fact package exists and hash matches
/// 3. Deploy NFT contract for this app
/// 4. Register app in marketplace
/// 5. Emit AppPublished event
#[no_mangle]
pub extern "C" fn publish_app(
    name_ptr: *const u8,
    name_len: usize,
    description_ptr: *const u8,
    description_len: usize,
    category: u8,
    version_ptr: *const u8,
    version_len: usize,
    price: u64, // In ASTRA tokens (smallest unit)
    fact_package_id_ptr: *const u8,
    fact_package_id_len: usize,
    wasm_hash_ptr: *const u8,
    wasm_hash_len: usize,
    license_type: u8,
) -> i64 {
    // Get caller's DID
    let mut caller_did_buf = [0u8; 256];
    let caller_did_len = unsafe { get_caller_did(caller_did_buf.as_mut_ptr(), 256) };
    let caller_did = unsafe {
        core::str::from_utf8_unchecked(&caller_did_buf[..caller_did_len])
    };
    
    // Check publisher reputation (Application reputation type = 2)
    let publisher_rep = unsafe {
        reputation_get_score(caller_did.as_ptr(), caller_did.len(), 2)
    };
    
    if publisher_rep < MIN_PUBLISHER_REPUTATION {
        unsafe {
            log_output(b"[ERROR] Insufficient reputation to publish apps\0".as_ptr(), 49);
        }
        return -1; // Insufficient reputation
    }
    
    // Verify fact package exists
    let package_exists = unsafe {
        fact_package_exists(fact_package_id_ptr, fact_package_id_len)
    };
    
    if package_exists != 1 {
        unsafe {
            log_output(b"[ERROR] Fact package not found\0".as_ptr(), 32);
        }
        return -2; // Package not found
    }
    
    // Verify WASM hash matches fact package
    let hash_valid = unsafe {
        fact_verify_hash(fact_package_id_ptr, fact_package_id_len, wasm_hash_ptr, wasm_hash_len)
    };
    
    if hash_valid != 1 {
        unsafe {
            log_output(b"[ERROR] WASM hash verification failed\0".as_ptr(), 40);
        }
        return -3; // Hash mismatch
    }
    
    // Generate app ID (simple hash for demo)
    let app_id = generate_app_id(name_ptr, name_len, caller_did);
    
    // Create NFT contract ID for this app
    let nft_contract_id = format_nft_contract_id(app_id);
    
    // Store app metadata on blockchain
    // Key: "app:{app_id}"
    // Value: Metadata (would be serialized struct in production)
    let storage_key = format_storage_key(b"app:", app_id);
    
    // For demo, we'll just store the app_id (in production, store full metadata)
    unsafe {
        storage_write(
            storage_key.as_ptr(),
            storage_key.len(),
            &app_id as *const u64 as *const u8,
            8,
        );
    }
    
    // Index by category
    let category_key = format_category_key(category, app_id);
    unsafe {
        storage_write(
            category_key.as_ptr(),
            category_key.len(),
            &app_id as *const u64 as *const u8,
            8,
        );
    }
    
    // Index by author
    let author_key = format_author_key(caller_did, app_id);
    unsafe {
        storage_write(
            author_key.as_ptr(),
            author_key.len(),
            &app_id as *const u64 as *const u8,
            8,
        );
    }
    
    // Emit AppPublished event
    unsafe {
        emit_event(
            b"AppPublished".as_ptr(),
            12,
            &app_id as *const u64 as *const u8,
            8,
        );
    }
    
    unsafe {
        log_output(b"[SUCCESS] App published successfully\0".as_ptr(), 38);
    }
    
    app_id as i64
}

/// Purchase an app (or get free app)
/// 
/// Flow:
/// 1. Get app listing
/// 2. If paid app, transfer ASTRA tokens (99% to creator, 1% to platform)
/// 3. Mint NFT license to buyer
/// 4. Update download stats
/// 5. Emit AppPurchased event
#[no_mangle]
pub extern "C" fn purchase_app(app_id: u64) -> i64 {
    // Get caller's DID
    let mut caller_did_buf = [0u8; 256];
    let caller_did_len = unsafe { get_caller_did(caller_did_buf.as_mut_ptr(), 256) };
    let caller_did = unsafe {
        core::str::from_utf8_unchecked(&caller_did_buf[..caller_did_len])
    };
    
    // Load app metadata from storage
    let storage_key = format_storage_key(b"app:", app_id);
    let mut metadata_buf = [0u8; 1024];
    let metadata_len = unsafe {
        storage_read(storage_key.as_ptr(), storage_key.len(), metadata_buf.as_mut_ptr(), 1024)
    };
    
    if metadata_len < 0 {
        unsafe {
            log_output(b"[ERROR] App not found\0".as_ptr(), 23);
        }
        return -1; // App not found
    }
    
    // In production, deserialize metadata to get price, author, etc.
    // For demo, assume we have these values
    let price: u64 = 0; // Would be loaded from metadata
    let author_did = "did:spacekit:author:example"; // Would be loaded from metadata
    
    // Check if user already owns this app
    let nft_contract_id = format_nft_contract_id(app_id);
    // Would query NFT ownership here
    
    // Process payment if app is not free
    if price > 0 {
        // Check buyer has sufficient balance
        let buyer_balance = unsafe {
            token_balance(caller_did.as_ptr(), caller_did.len())
        };
        
        if buyer_balance < price {
            unsafe {
                log_output(b"[ERROR] Insufficient balance\0".as_ptr(), 30);
            }
            return -2; // Insufficient balance
        }
        
        // Calculate revenue split
        let platform_fee = (price * PLATFORM_FEE_BPS) / BASIS_POINTS;
        let creator_amount = price - platform_fee;
        
        // Transfer to creator (99%)
        let transfer_result = unsafe {
            token_transfer(
                caller_did.as_ptr(),
                caller_did.len(),
                author_did.as_ptr(),
                author_did.len(),
                creator_amount,
            )
        };
        
        if transfer_result != 1 {
            unsafe {
                log_output(b"[ERROR] Payment transfer failed\0".as_ptr(), 33);
            }
            return -3; // Transfer failed
        }
        
        // Transfer platform fee (1%)
        unsafe {
            token_transfer(
                caller_did.as_ptr(),
                caller_did.len(),
                b"did:spacekit:platform:treasury".as_ptr(),
                27,
                platform_fee,
            );
        }
        
        // Emit RevenueDistributed event
        unsafe {
            emit_event(
                b"RevenueDistributed".as_ptr(),
                18,
                &app_id as *const u64 as *const u8,
                8,
            );
        }
    }
    
    // Mint NFT license to buyer
    let token_id = unsafe {
        nft_mint(
            nft_contract_id.as_ptr(),
            nft_contract_id.len(),
            caller_did.as_ptr(),
            caller_did.len(),
        )
    };
    
    if token_id == 0 {
        unsafe {
            log_output(b"[ERROR] NFT minting failed\0".as_ptr(), 28);
        }
        return -4; // NFT mint failed
    }
    
    // Update download count
    let downloads_key = format_storage_key(b"downloads:", app_id);
    let mut download_count: u64 = 0;
    unsafe {
        let mut count_buf = [0u8; 8];
        let read_len = storage_read(downloads_key.as_ptr(), downloads_key.len(), count_buf.as_mut_ptr(), 8);
        if read_len == 8 {
            download_count = u64::from_le_bytes(count_buf);
        }
        download_count += 1;
        storage_write(
            downloads_key.as_ptr(),
            downloads_key.len(),
            &download_count as *const u64 as *const u8,
            8,
        );
    }
    
    // Emit AppPurchased event
    unsafe {
        emit_event(
            b"AppPurchased".as_ptr(),
            12,
            &token_id as *const u64 as *const u8,
            8,
        );
    }
    
    unsafe {
        log_output(b"[SUCCESS] App purchased - NFT license minted\0".as_ptr(), 46);
    }
    
    token_id as i64
}

/// Update an existing app (publish new version)
#[no_mangle]
pub extern "C" fn update_app(
    app_id: u64,
    new_version_ptr: *const u8,
    new_version_len: usize,
    new_fact_package_id_ptr: *const u8,
    new_fact_package_id_len: usize,
    new_wasm_hash_ptr: *const u8,
    new_wasm_hash_len: usize,
) -> i32 {
    // Get caller's DID
    let mut caller_did_buf = [0u8; 256];
    let caller_did_len = unsafe { get_caller_did(caller_did_buf.as_mut_ptr(), 256) };
    let caller_did = unsafe {
        core::str::from_utf8_unchecked(&caller_did_buf[..caller_did_len])
    };
    
    // Load app metadata to verify ownership
    let storage_key = format_storage_key(b"app:", app_id);
    let mut metadata_buf = [0u8; 1024];
    let metadata_len = unsafe {
        storage_read(storage_key.as_ptr(), storage_key.len(), metadata_buf.as_mut_ptr(), 1024)
    };
    
    if metadata_len < 0 {
        return -1; // App not found
    }
    
    // In production: verify caller is app author
    // For demo, we'll skip this check
    
    // Verify new fact package exists
    let package_exists = unsafe {
        fact_package_exists(new_fact_package_id_ptr, new_fact_package_id_len)
    };
    
    if package_exists != 1 {
        return -2; // Package not found
    }
    
    // Verify new WASM hash
    let hash_valid = unsafe {
        fact_verify_hash(new_fact_package_id_ptr, new_fact_package_id_len, new_wasm_hash_ptr, new_wasm_hash_len)
    };
    
    if hash_valid != 1 {
        return -3; // Hash mismatch
    }
    
    // Update app metadata with new version
    // In production: update version, fact_package_id, wasm_hash, updated_at
    
    unsafe {
        emit_event(
            b"AppUpdated".as_ptr(),
            10,
            &app_id as *const u64 as *const u8,
            8,
        );
    }
    
    unsafe {
        log_output(b"[SUCCESS] App updated to new version\0".as_ptr(), 38);
    }
    
    1 // Success
}

/// Check if user has license for an app
#[no_mangle]
pub extern "C" fn has_license(app_id: u64, user_did_ptr: *const u8, user_did_len: usize) -> i32 {
    let user_did = unsafe {
        core::str::from_utf8_unchecked(core::slice::from_raw_parts(user_did_ptr, user_did_len))
    };
    
    // Query NFT contract to check if user owns any token for this app
    let nft_contract_id = format_nft_contract_id(app_id);
    
    // In production: query NFT contract for ownership
    // For demo, return 0 (not owned)
    
    0 // Does not have license
}

/// Get featured apps
#[no_mangle]
pub extern "C" fn get_featured_apps(output_ptr: *mut u8, max_len: usize) -> i32 {
    // Query storage for featured apps
    // Would return array of app IDs
    
    // For demo, return 0 apps
    0
}

/// Get popular apps (by downloads)
#[no_mangle]
pub extern "C" fn get_popular_apps(limit: u32, output_ptr: *mut u8, max_len: usize) -> i32 {
    // Query and sort apps by download count
    // Return top N apps
    
    0
}

/// Get apps by category
#[no_mangle]
pub extern "C" fn get_apps_by_category(category: u8, output_ptr: *mut u8, max_len: usize) -> i32 {
    // Query category index
    // Return all app IDs in category
    
    0
}

/// Get apps by author
#[no_mangle]
pub extern "C" fn get_apps_by_author(
    author_did_ptr: *const u8,
    author_did_len: usize,
    output_ptr: *mut u8,
    max_len: usize,
) -> i32 {
    // Query author index
    // Return all app IDs by this author
    
    0
}

/// Search apps by name
#[no_mangle]
pub extern "C" fn search_apps(
    query_ptr: *const u8,
    query_len: usize,
    output_ptr: *mut u8,
    max_len: usize,
) -> i32 {
    // Full-text search across app names and descriptions
    // Return matching app IDs
    
    0
}

/// Set app as featured (admin only)
#[no_mangle]
pub extern "C" fn set_featured(app_id: u64, is_featured: u8, banner_ptr: *const u8, banner_len: usize) -> i32 {
    // Get caller's DID
    let mut caller_did_buf = [0u8; 256];
    let caller_did_len = unsafe { get_caller_did(caller_did_buf.as_mut_ptr(), 256) };
    
    // Verify caller is admin (would check DID against admin list)
    // For demo, allow anyone
    
    // Update app's featured status
    let featured_key = format_storage_key(b"featured:", app_id);
    unsafe {
        storage_write(
            featured_key.as_ptr(),
            featured_key.len(),
            &is_featured as *const u8,
            1,
        );
    }
    
    1 // Success
}

/// Transfer app license NFT
#[no_mangle]
pub extern "C" fn transfer_license(
    app_id: u64,
    token_id: u64,
    to_did_ptr: *const u8,
    to_did_len: usize,
) -> i32 {
    let nft_contract_id = format_nft_contract_id(app_id);
    
    // Transfer NFT (will verify caller owns it)
    let result = unsafe {
        nft_transfer(
            nft_contract_id.as_ptr(),
            nft_contract_id.len(),
            token_id,
            to_did_ptr,
            to_did_len,
        )
    };
    
    if result == 1 {
        unsafe {
            emit_event(
                b"LicenseTransferred".as_ptr(),
                18,
                &token_id as *const u64 as *const u8,
                8,
            );
        }
    }
    
    result
}

/// Get total revenue for an app
#[no_mangle]
pub extern "C" fn get_app_revenue(app_id: u64) -> u64 {
    let revenue_key = format_storage_key(b"revenue:", app_id);
    let mut revenue: u64 = 0;
    
    unsafe {
        let mut revenue_buf = [0u8; 8];
        let read_len = storage_read(revenue_key.as_ptr(), revenue_key.len(), revenue_buf.as_mut_ptr(), 8);
        if read_len == 8 {
            revenue = u64::from_le_bytes(revenue_buf);
        }
    }
    
    revenue
}

/// Get download count for an app
#[no_mangle]
pub extern "C" fn get_download_count(app_id: u64) -> u64 {
    let downloads_key = format_storage_key(b"downloads:", app_id);
    let mut count: u64 = 0;
    
    unsafe {
        let mut count_buf = [0u8; 8];
        let read_len = storage_read(downloads_key.as_ptr(), downloads_key.len(), count_buf.as_mut_ptr(), 8);
        if read_len == 8 {
            count = u64::from_le_bytes(count_buf);
        }
    }
    
    count
}

// ============================================================================
// Helper Functions
// ============================================================================

fn generate_app_id(name_ptr: *const u8, name_len: usize, creator_did: &str) -> u64 {
    // Simple hash for demo (use proper hash in production)
    let mut id = name_len as u64;
    id = id.wrapping_mul(31).wrapping_add(creator_did.len() as u64);
    
    // Add timestamp-like component (would use actual timestamp in production)
    id = id.wrapping_mul(1000000);
    
    id
}

fn format_nft_contract_id(app_id: u64) -> Vec<u8> {
    let mut contract_id = Vec::new();
    contract_id.extend_from_slice(b"nft_app_");
    
    // Convert app_id to string-like representation
    let mut id = app_id;
    let mut digits = [0u8; 20];
    let mut pos = 0;
    
    if id == 0 {
        digits[0] = b'0';
        pos = 1;
    } else {
        while id > 0 {
            digits[pos] = (id % 10) as u8 + b'0';
            id /= 10;
            pos += 1;
        }
    }
    
    // Reverse digits
    for i in 0..pos {
        contract_id.push(digits[pos - 1 - i]);
    }
    
    contract_id
}

fn format_storage_key(prefix: &[u8], id: u64) -> Vec<u8> {
    let mut key = Vec::new();
    key.extend_from_slice(prefix);
    
    // Append ID as bytes
    key.extend_from_slice(&id.to_le_bytes());
    
    key
}

fn format_category_key(category: u8, app_id: u64) -> Vec<u8> {
    let mut key = Vec::new();
    key.extend_from_slice(b"category:");
    key.push(category);
    key.push(b':');
    key.extend_from_slice(&app_id.to_le_bytes());
    key
}

fn format_author_key(author_did: &str, app_id: u64) -> Vec<u8> {
    let mut key = Vec::new();
    key.extend_from_slice(b"author:");
    key.extend_from_slice(author_did.as_bytes());
    key.push(b':');
    key.extend_from_slice(&app_id.to_le_bytes());
    key
}

