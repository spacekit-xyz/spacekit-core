use wasm_bindgen::prelude::*;
use js_sys::{Array, Uint8Array};
use spacekit_quantum_verkle::{
    QuantumTree,
    QuantumProof,
    QuantumMultiProof,
    commitment::{NistSisScheme, SisOpening, setup_sis_params, SisProfile, SisSecurityLevel},
};
use alloy_primitives::{Address, B256, U256};

#[wasm_bindgen]
pub struct QuantumVerkleWasm {
    tree: QuantumTree<NistSisScheme>,
}

#[wasm_bindgen]
impl QuantumVerkleWasm {
    #[wasm_bindgen(constructor)]
    pub fn new() -> QuantumVerkleWasm {
        QuantumVerkleWasm {
            tree: QuantumTree::<NistSisScheme>::new(),
        }
    }

    #[wasm_bindgen]
    pub fn root_hex(&self) -> String {
        bytes_to_hex(self.tree.root().as_slice())
    }

    #[wasm_bindgen]
    pub fn set(&mut self, address_hex: &str, key_hex: &str, value_hex: &str, aux_hex: Option<String>) -> Result<(), JsValue> {
        let address = parse_address(address_hex)?;
        let key = parse_b256(key_hex)?;
        let value = parse_u256(value_hex)?;
        if let Some(aux_hex) = aux_hex {
            let aux = hex_to_bytes(&aux_hex)?;
            self.tree.set_with_aux(&address, &key, value, &aux);
        } else {
            self.tree.set(&address, &key, value);
        }
        Ok(())
    }

    #[wasm_bindgen]
    pub fn get(&self, address_hex: &str, key_hex: &str) -> Result<JsValue, JsValue> {
        let address = parse_address(address_hex)?;
        let key = parse_b256(key_hex)?;
        match self.tree.get(&address, &key) {
            Ok(value) => Ok(JsValue::from_str(&bytes_to_hex(&value.to_be_bytes::<32>()))),
            Err(_) => Ok(JsValue::NULL),
        }
    }

    #[wasm_bindgen]
    pub fn create_proof(&self, address_hex: &str, key_hex: &str) -> Result<Uint8Array, JsValue> {
        let address = parse_address(address_hex)?;
        let key = parse_b256(key_hex)?;
        let proof = self.tree.create_proof(&address, &key).map_err(|e| JsValue::from_str(&format!("{:?}", e)))?;
        let bytes = proof.to_bytes::<NistSisScheme>();
        Ok(Uint8Array::from(bytes.as_slice()))
    }

    #[wasm_bindgen]
    pub fn verify_proof(&self, proof_bytes: Uint8Array, address_hex: &str, key_hex: &str, value_hex: &str) -> Result<bool, JsValue> {
        let address = parse_address(address_hex)?;
        let key = parse_b256(key_hex)?;
        let value = parse_u256(value_hex)?;
        let proof_bytes = proof_bytes.to_vec();
        let proof = QuantumProof::<SisOpening>::from_bytes::<NistSisScheme>(&proof_bytes)
            .map_err(|_| JsValue::from_str("invalid proof"))?;
        Ok(self.tree.verify_proof(&proof, &address, &key, value))
    }

    #[wasm_bindgen]
    pub fn create_multi_proof(&self, addresses: Array, keys: Array) -> Result<Uint8Array, JsValue> {
        let pairs = parse_key_pairs(addresses, keys)?;
        let proof = self.tree.create_multi_proof(pairs).map_err(|e| JsValue::from_str(&format!("{:?}", e)))?;
        let bytes = proof.to_bytes::<NistSisScheme>();
        Ok(Uint8Array::from(bytes.as_slice()))
    }

    #[wasm_bindgen]
    pub fn verify_multi_proof(&self, proof_bytes: Uint8Array, addresses: Array, keys: Array, values: Array) -> Result<bool, JsValue> {
        let pairs = parse_key_pairs(addresses, keys)?;
        let values = parse_values(values)?;
        let proof = QuantumMultiProof::<Vec<SisOpening>>::from_bytes::<NistSisScheme>(&proof_bytes.to_vec())
            .map_err(|_| JsValue::from_str("invalid multiproof"))?;
        Ok(self.tree.verify_multi_proof(&proof, pairs, values))
    }
}

#[wasm_bindgen]
pub fn setup_params(level: u8, profile: u8) -> Result<JsValue, JsValue> {
    let level = match level {
        1 => SisSecurityLevel::L1,
        3 => SisSecurityLevel::L3,
        _ => return Err(JsValue::from_str("invalid security level")),
    };
    let profile = match profile {
        0 => SisProfile::Binding,
        1 => SisProfile::Hiding,
        _ => return Err(JsValue::from_str("invalid profile")),
    };
    let (params_id, blob) = setup_sis_params(level, profile);
    let obj = js_sys::Object::new();
    js_sys::Reflect::set(&obj, &JsValue::from_str("paramsId"), &JsValue::from_str(&params_id))?;
    js_sys::Reflect::set(&obj, &JsValue::from_str("paramsBlobHex"), &JsValue::from_str(&bytes_to_hex(&blob)))?;
    Ok(obj.into())
}

fn parse_key_pairs(addresses: Array, keys: Array) -> Result<Vec<(Address, B256)>, JsValue> {
    if addresses.length() != keys.length() {
        return Err(JsValue::from_str("addresses and keys length mismatch"));
    }
    let mut out = Vec::with_capacity(addresses.length() as usize);
    for idx in 0..addresses.length() {
        let address_hex = addresses.get(idx).as_string().ok_or_else(|| JsValue::from_str("address must be string"))?;
        let key_hex = keys.get(idx).as_string().ok_or_else(|| JsValue::from_str("key must be string"))?;
        out.push((parse_address(&address_hex)?, parse_b256(&key_hex)?));
    }
    Ok(out)
}

fn parse_values(values: Array) -> Result<Vec<U256>, JsValue> {
    let mut out = Vec::with_capacity(values.length() as usize);
    for idx in 0..values.length() {
        let value_hex = values.get(idx).as_string().ok_or_else(|| JsValue::from_str("value must be string"))?;
        out.push(parse_u256(&value_hex)?);
    }
    Ok(out)
}

fn parse_address(hex: &str) -> Result<Address, JsValue> {
    let bytes = hex_to_bytes_fixed(hex, 20)?;
    Ok(Address::from_slice(&bytes))
}

fn parse_b256(hex: &str) -> Result<B256, JsValue> {
    let bytes = hex_to_bytes_fixed(hex, 32)?;
    Ok(B256::from_slice(&bytes))
}

fn parse_u256(hex: &str) -> Result<U256, JsValue> {
    let bytes = hex_to_bytes_fixed(hex, 32)?;
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    Ok(U256::from_be_bytes::<32>(arr))
}

fn hex_to_bytes_fixed(hex: &str, len: usize) -> Result<Vec<u8>, JsValue> {
    let mut bytes = hex_to_bytes(hex)?;
    if bytes.len() > len {
        return Err(JsValue::from_str("hex too long"));
    }
    if bytes.len() < len {
        let mut padded = vec![0u8; len - bytes.len()];
        padded.append(&mut bytes);
        bytes = padded;
    }
    Ok(bytes)
}

fn hex_to_bytes(hex: &str) -> Result<Vec<u8>, JsValue> {
    let hex = hex.strip_prefix("0x").unwrap_or(hex);
    if hex.len() % 2 != 0 {
        return Err(JsValue::from_str("invalid hex length"));
    }
    let mut out = Vec::with_capacity(hex.len() / 2);
    let bytes = hex.as_bytes();
    for i in (0..bytes.len()).step_by(2) {
        let hi = from_hex_char(bytes[i])?;
        let lo = from_hex_char(bytes[i + 1])?;
        out.push((hi << 4) | lo);
    }
    Ok(out)
}

fn from_hex_char(c: u8) -> Result<u8, JsValue> {
    match c {
        b'0'..=b'9' => Ok(c - b'0'),
        b'a'..=b'f' => Ok(c - b'a' + 10),
        b'A'..=b'F' => Ok(c - b'A' + 10),
        _ => Err(JsValue::from_str("invalid hex character")),
    }
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{:02x}", b));
    }
    out
}
