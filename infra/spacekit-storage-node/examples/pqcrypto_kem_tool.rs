//! Emit JSON [`EncryptedFileKey`] using the same pqcrypto-kyber + AES-GCM path as the storage node
//! (`envelope::pqcrypto_kem_encrypt_bytes`), for verifying browser WASM can decrypt.
//!
//! Build: `cd spacekit-storage-node && cargo build --release --example pqcrypto_kem_tool`
//!
//! Usage: `pqcrypto_kem_tool <recipient_public_key_hex> <plaintext_utf8>`
//! Output: one line JSON with `kem_ciphertext_hex`, `nonce_hex`, `ciphertext_hex`.

use spacekit_storage_node::envelope::pqcrypto_kem_encrypt_bytes;

fn main() {
    let mut it = std::env::args().skip(1);
    let pk_hex = it.next().expect("missing recipient_public_key_hex");
    let plain = it.next().expect("missing plaintext_utf8");
    let pk_hex = pk_hex.trim().strip_prefix("0x").unwrap_or(pk_hex.trim());
    let pk = hex::decode(pk_hex).expect("invalid public key hex");
    let enc = pqcrypto_kem_encrypt_bytes(plain.as_bytes(), &pk).expect("encrypt");
    println!("{}", serde_json::to_string(&enc).expect("json"));
}
