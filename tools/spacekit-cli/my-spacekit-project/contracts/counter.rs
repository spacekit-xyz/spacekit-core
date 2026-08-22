
// Example SpaceKit Compute Contract
// Generated for DID: did:spacekit:user:95cfbe56-0df8-4aa8-af2a-046ddf8a3312
// Using algorithm: Kyber1024

#[no_mangle]
pub extern "C" fn counter_increment() -> u64 {
    // Simple counter implementation
    static mut COUNTER: u64 = 0;
    unsafe {
        COUNTER += 1;
        COUNTER
    }
}

#[no_mangle]
pub extern "C" fn counter_get() -> u64 {
    unsafe { COUNTER }
}

// Example main function for testing
#[no_mangle]
pub extern "C" fn main() -> u64 {
    counter_increment()
}

// Global counter variable
static mut COUNTER: u64 = 0;
