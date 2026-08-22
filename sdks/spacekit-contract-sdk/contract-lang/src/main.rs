// Copyright (c) 2026 SWTCH Labs LLC
// SPDX-License-Identifier: Apache-2.0

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use sha3::{Digest, Keccak256};
use serde::{Deserialize, Serialize};

/// Contract definition struct
/// Contains the contract name, storage, events, and functions
#[derive(Debug, Clone)]
struct ContractDef {
    name: String,
    storage: Vec<StorageField>,
    events: Vec<EventDef>,
    functions: Vec<FunctionDef>,
}

/// Storage field struct
/// Contains the name and type of a storage field
#[derive(Debug, Clone)]
struct StorageField {
    name: String,
    ty: TypeDef,
}

/// Function definition struct
/// Contains the function name, arguments, return type, opcode, event, and DID requirement
#[derive(Debug, Clone)]
struct FunctionDef {
    name: String,
    args: Vec<ParamDef>,
    return_ty: TypeDef,
    opcode: Option<u8>,
    emit_event: Option<String>,
    require_did: bool,
}

/// Parameter definition struct
/// Contains the parameter name and type
#[derive(Debug, Clone)]
struct ParamDef {
    name: String,
    ty: TypeDef,
}

/// Event definition struct
/// Contains the event name and fields
#[derive(Debug, Clone)]
struct EventDef {
    name: String,
    fields: Vec<ParamDef>,
}

/// Contract policy struct
/// Contains the selectors and opcodes that require DID verification
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ContractPolicy {
    require_did_selectors: Vec<String>,
    require_did_opcodes: Vec<u8>,
}

/// Type definition enum
/// Contains the possible types that can be used in a contract
#[allow(dead_code)]
#[derive(Debug, Clone)]
enum TypeDef {
    String,
    Address,
    Bool,
    U64,
    U128,
    Map(Box<TypeDef>, Box<TypeDef>),
    Void,
}

/// SKCL version constant
/// Contains the version of the SKCL compiler
const SKCL_VERSION: &str = "1.0.0";

/// Main function for the compiler
fn main() -> Result<(), String> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        return Err(format!(
            "Usage: {} <contract.scl> <output_dir>",
            args.get(0).cloned().unwrap_or_else(|| "spacekit-contractc".to_string())
        ));
    }

    let input_path = PathBuf::from(&args[1]);
    let output_dir = PathBuf::from(&args[2]);
    let input = fs::read_to_string(&input_path)
        .map_err(|e| format!("Failed to read {}: {}", input_path.display(), e))?;

    let contract = parse_contract(&input)?;
    write_contract(&contract, &output_dir)?;

    println!(
        "✅ SKCL v{} generated contract '{}' in {}",
        SKCL_VERSION,
        contract.name,
        output_dir.join(&contract.name).display()
    );
    Ok(())
}

/// Parse a contract from a string
/// Returns a `ContractDef` struct containing the contract name, storage, events, and functions
fn parse_contract(input: &str) -> Result<ContractDef, String> {
    let mut name = None;
    let mut storage = Vec::new();
    let mut events = Vec::new();
    let mut functions = Vec::new();
    let mut section = "";

    for (line_idx, raw_line) in input.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with("//") || line.starts_with('#') {
            continue;
        }

        if line.starts_with("contract ") {
            name = Some(line["contract ".len()..].trim().to_string());
            continue;
        }

        if line == "storage:" {
            section = "storage";
            continue;
        }

        if line == "events:" {
            section = "events";
            continue;
        }

        if line == "functions:" {
            section = "functions";
            continue;
        }

        match section {
            "storage" => {
                let (field_name, ty_str) = split_once(line, ':')
                    .ok_or_else(|| format!("Invalid storage line at {}: {}", line_idx + 1, line))?;
                let ty = parse_type(ty_str.trim())?;
                storage.push(StorageField {
                    name: field_name.trim().to_string(),
                    ty,
                });
            }
            "events" => {
                let (evt_name, args) = parse_signature(line)
                    .ok_or_else(|| format!("Invalid event line at {}: {}", line_idx + 1, line))?;
                let fields = parse_args(args)?;
                events.push(EventDef {
                    name: evt_name,
                    fields,
                });
            }
            "functions" => {
                let (sig, ret) = split_once(line, '-')
                    .ok_or_else(|| format!("Invalid function line at {}: {}", line_idx + 1, line))?;
                if !ret.trim().starts_with('>') {
                    return Err(format!("Invalid function return at {}: {}", line_idx + 1, line));
                }
                let (ret_ty, opcode, emit_event, require_did) = parse_return_opcode_and_emit(ret.trim().trim_start_matches('>').trim())?;

                let (fn_name, arg_str) = parse_signature(sig.trim())
                    .ok_or_else(|| format!("Invalid function signature at {}: {}", line_idx + 1, line))?;
                let args = parse_args(arg_str)?;

                functions.push(FunctionDef {
                    name: fn_name,
                    args,
                    return_ty: ret_ty,
                    opcode,
                    emit_event,
                    require_did,
                });
            }
            _ => {
                return Err(format!(
                    "Unexpected line outside of sections at {}: {}",
                    line_idx + 1,
                    line
                ));
            }
        }
    }

    let name = name.ok_or_else(|| "Missing contract name".to_string())?;
    validate_ident(&name, "contract name")?;

    if functions.is_empty() {
        return Err("No functions defined".to_string());
    }

    Ok(ContractDef { name, storage, events, functions })
}
/// Parse a function signature from a string
/// Returns a tuple containing the function name and arguments
fn parse_signature(sig: &str) -> Option<(String, String)> {
    let open = sig.find('(')?;
    let close = sig.rfind(')')?;
    if close < open {
        return None;
    }
    let name = sig[..open].trim().to_string();
    let args = sig[open + 1..close].trim().to_string();
    Some((name, args))
}

/// Parse a list of function arguments from a string
/// Returns a vector of `ParamDef` structs containing the argument name and type
fn parse_args(args: String) -> Result<Vec<ParamDef>, String> {
    if args.is_empty() {
        return Ok(Vec::new());
    }
    let mut params = Vec::new();
    for part in args.split(',') {
        let part = part.trim();
        let (name, ty) = split_once(part, ':')
            .ok_or_else(|| format!("Invalid arg '{}'", part))?;
        params.push(ParamDef {
            name: name.trim().to_string(),
            ty: parse_type(ty.trim())?,
        });
    }
    Ok(params)
}

/// Parse a return type, opcode, event, and DID requirement from a string
/// Returns a tuple containing the return type, opcode, event, and DID requirement
fn parse_return_opcode_and_emit(input: &str) -> Result<(TypeDef, Option<u8>, Option<String>, bool), String> {
    let mut ret_part = input.trim();
    let mut opcode = None;
    let mut emit_event = None;
    let mut require_did = false;

    if let Some((left, _)) = split_once_str(ret_part, " require did") {
        ret_part = left.trim();
        require_did = true;
    }

    if let Some((left, right)) = split_once_str(ret_part, " emit ") {
        ret_part = left.trim();
        let evt = right.trim();
        if !evt.is_empty() {
            emit_event = Some(evt.to_string());
        }
    }

    if let Some((left, right)) = split_once(ret_part, '@') {
        ret_part = left.trim();
        let tag = right.trim();
        if let Some(op_str) = tag.strip_prefix("opcode") {
            let op_value = op_str.trim().trim_start_matches('=').trim();
            let parsed: u8 = op_value.parse().map_err(|_| format!("Invalid opcode '{}'", op_value))?;
            opcode = Some(parsed);
        } else {
            return Err(format!("Invalid opcode tag '@{}'", tag));
        }
    }

    Ok((parse_type(ret_part)?, opcode, emit_event, require_did))
}

/// Parse a type from a string
/// Returns a `TypeDef` enum containing the type
fn parse_type(ty: &str) -> Result<TypeDef, String> {
    match ty {
        "string" => Ok(TypeDef::String),
        "address" => Ok(TypeDef::Address),
        "bool" => Ok(TypeDef::Bool),
        "u64" => Ok(TypeDef::U64),
        "u128" => Ok(TypeDef::U128),
        "void" => Ok(TypeDef::Void),
        _ if ty.starts_with("map<") && ty.ends_with('>') => {
            let inner = &ty["map<".len()..ty.len() - 1];
            let (k, v) = split_once(inner, ',')
                .ok_or_else(|| format!("Invalid map type '{}'", ty))?;
            Ok(TypeDef::Map(Box::new(parse_type(k.trim())?), Box::new(parse_type(v.trim())?)))
        }
        _ => Err(format!("Unsupported type '{}'", ty)),
    }
}

/// Split a string once at the first occurrence of a character
/// Returns a tuple containing the left and right parts of the split
fn split_once<'a>(s: &'a str, c: char) -> Option<(&'a str, &'a str)> {
    let idx = s.find(c)?;
    Some((&s[..idx], &s[idx + 1..]))
}

/// Split a string once at the first occurrence of a token
/// Returns a tuple containing the left and right parts of the split
fn split_once_str<'a>(s: &'a str, token: &str) -> Option<(&'a str, &'a str)> {
    let idx = s.find(token)?;
    Some((&s[..idx], &s[idx + token.len()..]))
}

/// Write a contract to a directory
/// Returns a `Result` containing an error message if the contract fails to write
fn write_contract(contract: &ContractDef, output_dir: &Path) -> Result<(), String> {
    validate_contract(contract)?;
    let contract_dir = output_dir.join(&contract.name);
    let src_dir = contract_dir.join("src");
    fs::create_dir_all(&src_dir).map_err(|e| format!("Failed to create dirs: {}", e))?;

    let cargo = format!(
        "[package]\nname = \"{}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\ncrate-type = [\"cdylib\"]\n\n[dependencies]\nwee_alloc = \"0.4.5\"\nspacekit-contract-sdk = {{ path = \"../../spacekit-contract-sdk\" }}\n\n[profile.release]\nopt-level = \"z\"\nlto = true\ncodegen-units = 1\npanic = \"abort\"\nstrip = true\n",
        contract.name.to_lowercase()
    );
    fs::write(contract_dir.join("Cargo.toml"), cargo)
        .map_err(|e| format!("Failed to write Cargo.toml: {}", e))?;

    let source = generate_contract_source(contract);
    fs::write(src_dir.join("lib.rs"), source)
        .map_err(|e| format!("Failed to write lib.rs: {}", e))?;

    let abi = generate_abi_json(contract);
    fs::write(contract_dir.join("abi.json"), abi)
        .map_err(|e| format!("Failed to write abi.json: {}", e))?;

    let policy_json = generate_policy_json(contract)?;
    fs::write(contract_dir.join("contract_policies.json"), &policy_json)
        .map_err(|e| format!("Failed to write contract_policies.json: {}", e))?;
    fs::write(output_dir.join("contract_policies.json"), &policy_json)
        .map_err(|e| format!("Failed to write top-level contract_policies.json: {}", e))?;

    merge_policy_json(output_dir, &policy_json)
        .map_err(|e| format!("Failed to merge contract_policies.json: {}", e))?;

    Ok(())
}

/// Generate the contract source code from a `ContractDef` struct
/// Returns a string containing the contract source code
fn generate_contract_source(contract: &ContractDef) -> String {
    let struct_name = format!("{}Contract", contract.name);

    let mut out = String::new();
    out.push_str("//! Generated by spacekit-contractc\n");
    out.push_str("#![no_std]\n\n");
    out.push_str("extern crate alloc;\n\n");
    out.push_str("use alloc::string::{String, ToString};\n");
    out.push_str("use alloc::vec::Vec;\n");
    out.push_str("use alloc::vec;\n");
    out.push_str("use spacekit_contract_sdk::{ContractError, SpacekitContract, emit_event_bytes, get_caller_did_string, verify_did_string};\n");
    out.push_str("use spacekit_contract_sdk::spacekit_contract;\n\n");
    out.push_str("#[global_allocator]\n");
    out.push_str("static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;\n\n");
    out.push_str("#[panic_handler]\n");
    out.push_str("fn panic(_info: &core::panic::PanicInfo) -> ! { loop {} }\n\n");
    out.push_str("#[link(wasm_import_module = \"spacekit_storage\")]\n");
    out.push_str("extern \"C\" {\n");
    out.push_str("    fn storage_save(key_ptr: *const u8, key_len: usize, data_ptr: *const u8, data_len: usize) -> i32;\n");
    out.push_str("    fn storage_load(key_ptr: *const u8, key_len: usize, dest_ptr: *mut u8, max_len: usize) -> usize;\n");
    out.push_str("}\n\n");

    out.push_str(&format!("struct {};\n\n", struct_name));

    out.push_str(&format!("impl SpacekitContract for {} {{\n", struct_name));
    out.push_str("    type Error = ContractError;\n\n");
    out.push_str("    fn init() -> Self { Self }\n\n");
    out.push_str("    fn handle(&mut self, input: &[u8]) -> Result<Vec<u8>, ContractError> {\n");
    out.push_str("        let mut cursor = 0usize;\n");
    out.push_str("        let op = read_u8(input, &mut cursor)?;\n\n");
    out.push_str("        match op {\n");

    for (idx, func) in contract.functions.iter().enumerate() {
        let opcode = func.opcode.unwrap_or((idx + 1) as u8);
        out.push_str(&format!("            {} => {{\n", opcode));
        for arg in &func.args {
            out.push_str(&format!(
                "                let {} = {}(input, &mut cursor)?;\n",
                arg.name,
                read_fn_for_type(&arg.ty)
            ));
        }
        if func.require_did {
            out.push_str("                let caller_did = get_caller_did_string()?;\n");
            out.push_str("                if !verify_did_string(&caller_did) { return Err(ContractError::InvalidInput); }\n");
        }
        out.push_str(&format!(
            "                let result = self.{}({});\n",
            func.name,
            func.args.iter().map(|a| a.name.clone()).collect::<Vec<_>>().join(", ")
        ));
        if let Some(evt) = &func.emit_event {
            out.push_str("                let evt_payload = ");
            out.push_str(&format!("encode_event_{}(", evt.to_lowercase()));
            let evt_args = event_fields_for_emit(contract, evt)
                .iter()
                .map(|f| f.name.clone())
                .collect::<Vec<_>>()
                .join(", ");
            out.push_str(&evt_args);
            out.push_str(")?;\n");
            out.push_str("                emit_event_bytes(\"");
            out.push_str(&event_signature(contract, evt));
            out.push_str("\", &evt_payload);\n");
        }
        out.push_str("                encode_return(result?, ");
        out.push_str(&format!("ReturnType::{}))\n", return_type_variant(&func.return_ty)));
        out.push_str("            }\n");
    }

    out.push_str("            _ => Err(ContractError::InvalidInput),\n");
    out.push_str("        }\n");
    out.push_str("    }\n");
    out.push_str("}\n\n");

    out.push_str("spacekit_contract!(");
    out.push_str(&struct_name);
    out.push_str(");\n\n");

    for func in &contract.functions {
        out.push_str(&format!(
            "impl {} {{\n    fn {}(&mut self, {}) -> Result<{}, ContractError> {{\n        // TODO: implement\n        Ok({})\n    }}\n}}\n\n",
            struct_name,
            func.name,
            func.args.iter().map(|a| format!("{}: {}", a.name, rust_type(&a.ty))).collect::<Vec<_>>().join(", "),
            rust_type(&func.return_ty),
            default_for_type(&func.return_ty)
        ));
    }

    for event in &contract.events {
        out.push_str(&format!(
            "fn encode_event_{}({}) -> Result<Vec<u8>, ContractError> {{\n",
            event.name.to_lowercase(),
            event.fields.iter().map(|f| format!("{}: {}", f.name, rust_type(&f.ty))).collect::<Vec<_>>().join(", ")
        ));
        out.push_str("    let mut head: Vec<u8> = Vec::new();\n");
        out.push_str("    let mut tail: Vec<u8> = Vec::new();\n");
        out.push_str("    let mut offset: usize = 32 * ");
        out.push_str(&event.fields.len().to_string());
        out.push_str(";\n");
        for field in &event.fields {
            out.push_str(&encode_abi_head_tail_line(&field.name, &field.ty));
        }
        out.push_str("    head.extend_from_slice(&tail);\n");
        out.push_str("    Ok(head)\n");
        out.push_str("}\n\n");
    }

    out.push_str(r#"
enum ReturnType {
    Void,
    Bool,
    U64,
    U128,
    String,
}

fn encode_return(value: impl IntoReturn, ty: ReturnType) -> Result<Vec<u8>, ContractError> {
    value.into_abi(ty)
}

trait IntoReturn {
    fn into_abi(self, ty: ReturnType) -> Result<Vec<u8>, ContractError>;
}

impl IntoReturn for () {
    fn into_abi(self, _ty: ReturnType) -> Result<Vec<u8>, ContractError> {
        Ok(Vec::new())
    }
}

impl IntoReturn for bool {
    fn into_abi(self, _ty: ReturnType) -> Result<Vec<u8>, ContractError> {
        Ok(abi_encode_bool(self))
    }
}

impl IntoReturn for u64 {
    fn into_abi(self, _ty: ReturnType) -> Result<Vec<u8>, ContractError> {
        Ok(abi_encode_u64(self))
    }
}

impl IntoReturn for u128 {
    fn into_abi(self, _ty: ReturnType) -> Result<Vec<u8>, ContractError> {
        Ok(abi_encode_u128(self))
    }
}

impl IntoReturn for String {
    fn into_abi(self, _ty: ReturnType) -> Result<Vec<u8>, ContractError> {
        let mut out = Vec::new();
        out.extend_from_slice(&abi_encode_offset(32));
        out.extend_from_slice(&abi_encode_string(&self));
        Ok(out)
    }
}

fn abi_encode_bool(value: bool) -> Vec<u8> {
    let mut out = vec![0u8; 32];
    out[31] = if value { 1 } else { 0 };
    out
}

fn abi_encode_u64(value: u64) -> Vec<u8> {
    let mut out = vec![0u8; 32];
    out[24..32].copy_from_slice(&value.to_be_bytes());
    out
}

fn abi_encode_u128(value: u128) -> Vec<u8> {
    let mut out = vec![0u8; 32];
    out[16..32].copy_from_slice(&value.to_be_bytes());
    out
}

fn abi_encode_offset(offset: usize) -> Vec<u8> {
    abi_encode_u64(offset as u64)
}

fn abi_encode_string(value: &str) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&abi_encode_u64(value.len() as u64));
    out.extend_from_slice(value.as_bytes());
    let padding = (32 - (value.len() % 32)) % 32;
    out.extend_from_slice(&vec![0u8; padding]);
    out
}

fn abi_encode_address(value: &str) -> Result<Vec<u8>, ContractError> {
    let trimmed = value.trim_start_matches("0x");
    if trimmed.len() != 40 {
        return Err(ContractError::InvalidInput);
    }
    let mut bytes = [0u8; 20];
    let mut i = 0;
    while i < 20 {
        let idx = i * 2;
        let hi = hex_val(trimmed.as_bytes()[idx])?;
        let lo = hex_val(trimmed.as_bytes()[idx + 1])?;
        bytes[i] = (hi << 4) | lo;
        i += 1;
    }
    let mut out = vec![0u8; 32];
    out[12..32].copy_from_slice(&bytes);
    Ok(out)
}

fn hex_val(b: u8) -> Result<u8, ContractError> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => Err(ContractError::InvalidInput),
    }
}

fn storage_save_bytes(key: &str, data: &[u8]) -> Result<(), ContractError> {
    let result = unsafe { storage_save(key.as_ptr(), key.len(), data.as_ptr(), data.len()) };
    if result >= 0 { Ok(()) } else { Err(ContractError::StorageError) }
}

fn storage_load_bytes(key: &str, max_len: usize) -> Result<Vec<u8>, ContractError> {
    let mut buffer = vec![0u8; max_len];
    let read_len = unsafe { storage_load(key.as_ptr(), key.len(), buffer.as_mut_ptr(), max_len) };
    if read_len == 0 {
        return Err(ContractError::StorageError);
    }
    buffer.truncate(read_len);
    Ok(buffer)
}

fn read_u8(input: &[u8], cursor: &mut usize) -> Result<u8, ContractError> {
    if *cursor >= input.len() { return Err(ContractError::InvalidInput); }
    let value = input[*cursor];
    *cursor += 1;
    Ok(value)
}

fn read_u16(input: &[u8], cursor: &mut usize) -> Result<u16, ContractError> {
    if *cursor + 2 > input.len() { return Err(ContractError::InvalidInput); }
    let bytes = [input[*cursor], input[*cursor + 1]];
    *cursor += 2;
    Ok(u16::from_le_bytes(bytes))
}

fn read_u64(input: &[u8], cursor: &mut usize) -> Result<u64, ContractError> {
    if *cursor + 8 > input.len() { return Err(ContractError::InvalidInput); }
    let bytes = [
        input[*cursor], input[*cursor + 1], input[*cursor + 2], input[*cursor + 3],
        input[*cursor + 4], input[*cursor + 5], input[*cursor + 6], input[*cursor + 7],
    ];
    *cursor += 8;
    Ok(u64::from_be_bytes(bytes))
}

fn read_u128(input: &[u8], cursor: &mut usize) -> Result<u128, ContractError> {
    if *cursor + 16 > input.len() { return Err(ContractError::InvalidInput); }
    let bytes = [
        input[*cursor], input[*cursor + 1], input[*cursor + 2], input[*cursor + 3],
        input[*cursor + 4], input[*cursor + 5], input[*cursor + 6], input[*cursor + 7],
        input[*cursor + 8], input[*cursor + 9], input[*cursor + 10], input[*cursor + 11],
        input[*cursor + 12], input[*cursor + 13], input[*cursor + 14], input[*cursor + 15],
    ];
    *cursor += 16;
    Ok(u128::from_be_bytes(bytes))
}

fn read_bool(input: &[u8], cursor: &mut usize) -> Result<bool, ContractError> {
    Ok(read_u8(input, cursor)? != 0)
}

fn read_string(input: &[u8], cursor: &mut usize) -> Result<String, ContractError> {
    let len = read_u16(input, cursor)? as usize;
    if *cursor + len > input.len() { return Err(ContractError::InvalidInput); }
    let slice = &input[*cursor..*cursor + len];
    *cursor += len;
    core::str::from_utf8(slice).map(|s| s.to_string()).map_err(|_| ContractError::InvalidInput)
}
"#);

    out
}

/// Generate the ABI head and tail lines for a given field and type
/// Returns a string containing the ABI head and tail lines
fn encode_abi_head_tail_line(name: &str, ty: &TypeDef) -> String {
    match ty {
        TypeDef::Bool => format!("    head.extend_from_slice(&abi_encode_bool({}));\n", name),
        TypeDef::U64 => format!("    head.extend_from_slice(&abi_encode_u64({}));\n", name),
        TypeDef::U128 => format!("    head.extend_from_slice(&abi_encode_u128({}));\n", name),
        TypeDef::Address => format!(
            "    head.extend_from_slice(&abi_encode_address({})?);\n",
            name
        ),
        TypeDef::String => {
            let mut s = String::new();
            s.push_str("    head.extend_from_slice(&abi_encode_offset(offset));\n");
            s.push_str(&format!("    let encoded = abi_encode_string(&{});\n", name));
            s.push_str("    offset += encoded.len();\n");
            s.push_str("    tail.extend_from_slice(&encoded);\n");
            s
        }
        TypeDef::Map(_, _) => "    // map types not yet supported in events\n".to_string(),
        TypeDef::Void => "".to_string(),
    }
}

/// Generate the event fields for a given event name
/// Returns a slice of `ParamDef` structs containing the event fields
fn event_fields_for_emit<'a>(contract: &'a ContractDef, name: &str) -> &'a [ParamDef] {
    contract
        .events
        .iter()
        .find(|e| e.name == name)
        .map(|e| e.fields.as_slice())
        .unwrap_or(&[])
}

/// Validate a contract
/// Returns a `Result` containing an error message if the contract is invalid
fn validate_contract(contract: &ContractDef) -> Result<(), String> {
    let mut function_names = std::collections::HashSet::new();
    let mut event_names = std::collections::HashSet::new();
    let mut storage_names = std::collections::HashSet::new();
    let mut opcode_set = std::collections::HashSet::new();

    for field in &contract.storage {
        validate_ident(&field.name, "storage field")?;
        if !storage_names.insert(field.name.clone()) {
            return Err(format!("Duplicate storage field '{}'", field.name));
        }
        if matches!(field.ty, TypeDef::Void) {
            return Err(format!("Storage field '{}' cannot be void", field.name));
        }
    }

    for event in &contract.events {
        validate_ident(&event.name, "event name")?;
        if !event_names.insert(event.name.clone()) {
            return Err(format!("Duplicate event '{}'", event.name));
        }
        let mut field_names = std::collections::HashSet::new();
        for field in &event.fields {
            validate_ident(&field.name, "event field")?;
            if !field_names.insert(field.name.clone()) {
                return Err(format!(
                    "Duplicate field '{}' in event '{}'",
                    field.name, event.name
                ));
            }
            if matches!(field.ty, TypeDef::Map(_, _)) {
                return Err(format!(
                    "Event '{}' uses unsupported map field '{}'",
                    event.name, field.name
                ));
            }
            if matches!(field.ty, TypeDef::Void) {
                return Err(format!(
                    "Event '{}' uses invalid void field '{}'",
                    event.name, field.name
                ));
            }
        }
    }

    for (idx, func) in contract.functions.iter().enumerate() {
        validate_ident(&func.name, "function name")?;
        if !function_names.insert(func.name.clone()) {
            return Err(format!("Duplicate function '{}'", func.name));
        }
        if func.args.len() > 255 {
            return Err(format!(
                "Function '{}' has too many args (max 255)",
                func.name
            ));
        }
        for arg in &func.args {
            validate_ident(&arg.name, "argument name")?;
            if matches!(arg.ty, TypeDef::Void) {
                return Err(format!(
                    "Function '{}' has invalid void arg '{}'",
                    func.name, arg.name
                ));
            }
        }
        if matches!(func.return_ty, TypeDef::Map(_, _)) {
            return Err(format!(
                "Function '{}' has unsupported map return type",
                func.name
            ));
        }
        let resolved_opcode = if let Some(opcode) = func.opcode {
            opcode
        } else {
            if idx >= 255 {
                return Err("Too many functions for auto opcodes (max 255)".to_string());
            }
            (idx + 1) as u8
        };
        if !opcode_set.insert(resolved_opcode) {
            return Err(format!(
                "Duplicate opcode '{}' in function '{}'",
                resolved_opcode, func.name
            ));
        }

        if let Some(evt) = &func.emit_event {
            let event = contract.events.iter().find(|e| &e.name == evt)
                .ok_or_else(|| format!("Function '{}' emits unknown event '{}'", func.name, evt))?;
            for field in &event.fields {
                if !func.args.iter().any(|a| a.name == field.name) {
                    return Err(format!(
                        "Function '{}' emits event '{}' but is missing arg '{}' (required by event)",
                        func.name, evt, field.name
                    ));
                }
            }
        }
    }
    Ok(())
}

/// Generate the ABI JSON for a given contract
/// Returns a string containing the ABI JSON
fn generate_abi_json(contract: &ContractDef) -> String {
    let mut json = String::new();
    json.push_str("{\n");
    json.push_str(&format!("  \"contract\": \"{}\",\n", contract.name));
    json.push_str("  \"functions\": [\n");

    for (idx, func) in contract.functions.iter().enumerate() {
        let opcode = func.opcode.unwrap_or((idx + 1) as u8);
        let signature = function_signature(func);
        let selector = keccak_selector_hex(&signature);
        json.push_str("    {\n");
        json.push_str(&format!("      \"name\": \"{}\",\n", func.name));
        json.push_str(&format!("      \"opcode\": {},\n", opcode));
        json.push_str(&format!("      \"signature\": \"{}\",\n", signature));
        json.push_str(&format!("      \"selector\": \"0x{}\",\n", selector));
        json.push_str("      \"args\": [");
        for (i, arg) in func.args.iter().enumerate() {
            json.push_str(&format!(
                "{{\"name\":\"{}\",\"type\":\"{}\"}}",
                arg.name,
                abi_type(&arg.ty)
            ));
            if i + 1 < func.args.len() {
                json.push_str(", ");
            }
        }
        json.push_str("],\n");
        if let Some(evt) = &func.emit_event {
            json.push_str(&format!("      \"emits\": \"{}\",\n", evt));
        }
        if func.require_did {
            json.push_str("      \"requires_did\": true,\n");
        }
        json.push_str(&format!("      \"returns\": \"{}\"\n", abi_type(&func.return_ty)));
        json.push_str("    }");
        if idx + 1 < contract.functions.len() {
            json.push_str(",");
        }
        json.push_str("\n");
    }
    json.push_str("  ],\n");
    json.push_str("  \"events\": [\n");
    for (idx, evt) in contract.events.iter().enumerate() {
        json.push_str("    {\n");
        json.push_str(&format!("      \"name\": \"{}\",\n", evt.name));
        let signature = event_signature(contract, &evt.name);
        let topic = keccak_topic_hex(&signature);
        json.push_str(&format!("      \"signature\": \"{}\",\n", signature));
        json.push_str(&format!("      \"topic\": \"0x{}\",\n", topic));
        json.push_str("      \"fields\": [");
        for (i, field) in evt.fields.iter().enumerate() {
            json.push_str(&format!(
                "{{\"name\":\"{}\",\"type\":\"{}\"}}",
                field.name,
                abi_type(&field.ty)
            ));
            if i + 1 < evt.fields.len() {
                json.push_str(", ");
            }
        }
        json.push_str("]\n");
        json.push_str("    }");
        if idx + 1 < contract.events.len() {
            json.push_str(",");
        }
        json.push_str("\n");
    }
    json.push_str("  ]\n");
    json.push_str("}\n");
    json
}

/// Generate the policy JSON for a given contract
/// Returns a `Result` containing a string containing the policy JSON
fn generate_policy_json(contract: &ContractDef) -> Result<String, String> {
    let mut policy = ContractPolicy::default();
    for (idx, func) in contract.functions.iter().enumerate() {
        if func.require_did {
            let opcode = func.opcode.unwrap_or((idx + 1) as u8);
            policy.require_did_opcodes.push(opcode);

            let selector = keccak_selector_hex(&function_signature(func));
            policy
                .require_did_selectors
                .push(format!("0x{}", selector));
        }
    }

    let mut map = std::collections::HashMap::new();
    map.insert("default".to_string(), policy.clone());
    map.insert(format!("$contract:{}", contract.name), policy);
    serde_json::to_string_pretty(&map).map_err(|e| format!("Policy JSON error: {}", e))
}

/// Merge the policy JSON for a given contract
/// Returns a `Result` containing an error message if the policy JSON fails to merge
fn merge_policy_json(output_dir: &Path, policy_json: &str) -> Result<(), String> {
    let target_path = output_dir.join("contract_policies.merged.json");
    let mut merged: std::collections::HashMap<String, ContractPolicy> = if target_path.exists() {
        let existing = std::fs::read_to_string(&target_path)
            .map_err(|e| format!("Read merge file failed: {}", e))?;
        serde_json::from_str(&existing)
            .map_err(|e| format!("Parse merge file failed: {}", e))?
    } else {
        std::collections::HashMap::new()
    };

    let incoming: std::collections::HashMap<String, ContractPolicy> =
        serde_json::from_str(policy_json).map_err(|e| format!("Parse policy failed: {}", e))?;

    for (key, value) in incoming {
        merged.insert(key, value);
    }

    let out = serde_json::to_string_pretty(&merged)
        .map_err(|e| format!("Serialize merge failed: {}", e))?;
    std::fs::write(target_path, out).map_err(|e| format!("Write merge failed: {}", e))?;
    Ok(())
}

/// Generate the Rust type for a given type
/// Returns a string containing the Rust type
fn rust_type(ty: &TypeDef) -> String {
    match ty {
        TypeDef::String | TypeDef::Address => "String".to_string(),
        TypeDef::Bool => "bool".to_string(),
        TypeDef::U64 => "u64".to_string(),
        TypeDef::U128 => "u128".to_string(),
        TypeDef::Map(_, _) => "String".to_string(),
        TypeDef::Void => "()".to_string(),
    }
}

/// Generate the default value for a given type
/// Returns a string containing the default value
fn default_for_type(ty: &TypeDef) -> String {
    match ty {
        TypeDef::String | TypeDef::Address => "String::new()".to_string(),
        TypeDef::Bool => "false".to_string(),
        TypeDef::U64 => "0".to_string(),
        TypeDef::U128 => "0".to_string(),
        TypeDef::Map(_, _) => "String::new()".to_string(),
        TypeDef::Void => "()".to_string(),
    }
}

/// Generate the function signature for a given function
/// Returns a string containing the function signature
fn function_signature(func: &FunctionDef) -> String {
    let args = func.args.iter().map(|a| abi_type(&a.ty)).collect::<Vec<_>>().join(",");
    format!("{}({})", func.name, args)
}

/// Generate the event signature for a given event name
/// Returns a string containing the event signature
fn event_signature(contract: &ContractDef, event_name: &str) -> String {
    let evt = contract.events.iter().find(|e| e.name == event_name);
    if let Some(evt) = evt {
        let args = evt.fields.iter().map(|f| abi_type(&f.ty)).collect::<Vec<_>>().join(",");
        format!("{}({})", evt.name, args)
    } else {
        event_name.to_string()
    }
}

/// Generate the Keccak selector hex for a given signature
/// Returns a string containing the Keccak selector hex
fn keccak_selector_hex(signature: &str) -> String {
    let mut hasher = Keccak256::new();
    hasher.update(signature.as_bytes());
    let hash = hasher.finalize();
    hex::encode(&hash[..4])
}

/// Generate the Keccak topic hex for a given signature
/// Returns a string containing the Keccak topic hex
fn keccak_topic_hex(signature: &str) -> String {
    let mut hasher = Keccak256::new();
    hasher.update(signature.as_bytes());
    let hash = hasher.finalize();
    hex::encode(hash)
}

/// Generate the return type variant for a given type
/// Returns a string containing the return type variant
fn return_type_variant(ty: &TypeDef) -> &'static str {
    match ty {
        TypeDef::Void => "Void",
        TypeDef::Bool => "Bool",
        TypeDef::U64 => "U64",
        TypeDef::U128 => "U128",
        TypeDef::String | TypeDef::Address => "String",
        TypeDef::Map(_, _) => "String",
    }
}

/// Generate the ABI type for a given type
/// Returns a string containing the ABI type
fn abi_type(ty: &TypeDef) -> &'static str {
    match ty {
        TypeDef::String => "string",
        TypeDef::Address => "address",
        TypeDef::Bool => "bool",
        TypeDef::U64 => "u64",
        TypeDef::U128 => "u128",
        TypeDef::Map(_, _) => "map",
        TypeDef::Void => "void",
    }
}

/// Generate the read function for a given type
/// Returns a string containing the read function
fn read_fn_for_type(ty: &TypeDef) -> &'static str {
    match ty {
        TypeDef::String | TypeDef::Address => "read_string",
        TypeDef::Bool => "read_bool",
        TypeDef::U64 => "read_u64",
        TypeDef::U128 => "read_u128",
        TypeDef::Map(_, _) => "read_string",
        TypeDef::Void => "read_string",
    }
}

/// Validate an identifier
/// Returns a `Result` containing an error message if the identifier is invalid
fn validate_ident(name: &str, context: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err(format!("Invalid {}: empty name", context));
    }
    let mut chars = name.chars();
    let first = chars.next().unwrap();
    if !(first.is_ascii_alphabetic() || first == '_') {
        return Err(format!(
            "Invalid {} '{}': must start with a letter or underscore",
            context, name
        ));
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(format!(
            "Invalid {} '{}': only letters, numbers, and underscores allowed",
            context, name
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_and_validate(input: &str) -> Result<ContractDef, String> {
        let contract = parse_contract(input)?;
        validate_contract(&contract)?;
        Ok(contract)
    }

    #[test]
    fn selector_matches_erc20_transfer() {
        let selector = keccak_selector_hex("transfer(address,uint256)");
        assert_eq!(selector, "a9059cbb");
    }

    #[test]
    fn topic_matches_erc20_transfer() {
        let topic = keccak_topic_hex("Transfer(address,address,uint256)");
        assert_eq!(topic, "ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef");
    }

    #[test]
    fn parses_valid_minimal_contract() {
        let input = r#"
contract Token

storage:
  total_supply: u64

events:
  Transfer(from: address, to: address, amount: u64)

functions:
  mint(from: address, to: address, amount: u64) -> bool emit Transfer
  total_supply() -> u64
"#;
        let contract = parse_and_validate(input).unwrap();
        assert_eq!(contract.name, "Token");
        assert_eq!(contract.functions.len(), 2);
    }

    #[test]
    fn rejects_invalid_contract_name() {
        let input = r#"
contract 123bad

functions:
  ping() -> bool
"#;
        let err = parse_contract(input).unwrap_err();
        assert!(err.contains("Invalid contract name"));
    }

    #[test]
    fn rejects_duplicate_function_names() {
        let input = r#"
contract Token

functions:
  ping() -> bool
  ping() -> bool
"#;
        let err = parse_and_validate(input).unwrap_err();
        assert!(err.contains("Duplicate function"));
    }

    #[test]
    fn rejects_duplicate_event_names() {
        let input = r#"
contract Token

events:
  Transfer(from: address)
  Transfer(to: address)

functions:
  mint(to: address) -> bool emit Transfer
"#;
        let err = parse_and_validate(input).unwrap_err();
        assert!(err.contains("Duplicate event"));
    }

    #[test]
    fn rejects_duplicate_storage_fields() {
        let input = r#"
contract Token

storage:
  total_supply: u64
  total_supply: u64

functions:
  total_supply() -> u64
"#;
        let err = parse_and_validate(input).unwrap_err();
        assert!(err.contains("Duplicate storage field"));
    }

    #[test]
    fn rejects_void_argument() {
        let input = r#"
contract Token

functions:
  ping(x: void) -> bool
"#;
        let err = parse_and_validate(input).unwrap_err();
        assert!(err.contains("invalid void arg"));
    }

    #[test]
    fn rejects_map_return_type() {
        let input = r#"
contract Token

functions:
  read() -> map<string,u64>
"#;
        let err = parse_and_validate(input).unwrap_err();
        assert!(err.contains("unsupported map return type"));
    }

    #[test]
    fn rejects_map_event_field() {
        let input = r#"
contract Token

events:
  Transfer(meta: map<string,u64>)

functions:
  mint(meta: map<string,u64>) -> bool emit Transfer
"#;
        let err = parse_and_validate(input).unwrap_err();
        assert!(err.contains("map") || err.contains("unsupported"));
    }

    #[test]
    fn rejects_duplicate_opcodes() {
        let input = r#"
contract Token

functions:
  mint(to: address) -> bool @opcode 1
  burn(from: address) -> bool @opcode 1
"#;
        let err = parse_and_validate(input).unwrap_err();
        assert!(err.contains("Duplicate opcode"));
    }

    #[test]
    fn rejects_too_many_functions_for_auto_opcodes() {
        let mut input = String::from("contract Token\n\nfunctions:\n");
        for idx in 0..256 {
            input.push_str(&format!("  f{}() -> bool\n", idx));
        }
        let err = parse_and_validate(&input).unwrap_err();
        assert!(err.contains("Too many functions for auto opcodes"));
    }

    #[test]
    fn generated_readers_use_big_endian() {
        let input = r#"
contract Token

functions:
  total_supply() -> u64
"#;
        let contract = parse_and_validate(input).unwrap();
        let source = generate_contract_source(&contract);
        assert!(source.contains("u64::from_be_bytes"));
        assert!(source.contains("u128::from_be_bytes"));
    }
}
