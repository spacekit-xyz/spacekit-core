pub mod aes;
pub mod chacha;
pub mod lattice_vc;
pub mod quantum;
pub mod sis_vc;
pub mod utils;
pub mod xchacha;

// Re-export core quantum functionality
pub use lattice_vc::*;
pub use quantum::*;
pub use sis_vc::*;

// Re-export AES-GCM for compatibility
pub use aes_gcm::*;

// Re-export utils (no name collisions)
pub use utils::*;

// Cipher APIs have overlapping function names (encrypt_message, decrypt_message, etc.).
// To avoid ambiguous glob re-exports, re-export each family under a distinct prefix.
pub use aes as aes_cipher;
pub use chacha as chacha_cipher;
pub use xchacha as xchacha_cipher;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quantum_module_loads() {
        // Basic test to ensure modules load correctly
        assert!(true);
    }
}
