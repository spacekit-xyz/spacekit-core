//! Nested `spacekit_contract.contract_call`: caller WASM invokes callee WASM via host import.

use spacekit_compute_node::spacekitvm::swtchvm_node::{SwtchvmAddress, SwtchvmRuntime};

fn decode_i32_le(b: &[u8]) -> i32 {
    let a: [u8; 4] = b[..4].try_into().expect("4-byte code");
    i32::from_le_bytes(a)
}

/// Callee: `main` ignores input, returns length 2; `get_result` copies `"B!"` from linear memory.
fn wasm_callee() -> Vec<u8> {
    let wat = r#"
(module
  (memory 1)
  (data (i32.const 1024) "B!")
  (export "memory" (memory 0))
  (export "main" (func $main))
  (export "get_result" (func $get_result))
  (func $main (param i32 i32) (result i32)
    (i32.const 2)
  )
  (func $get_result (param i32 i32) (result i32)
    (memory.copy (local.get 0) (i32.const 1024) (i32.const 2))
    (i32.const 2)
  )
)
"#;
    wat::parse_str(wat).expect("callee WAT")
}

/// Caller: imports `spacekit_contract.contract_call`, calls fixed callee hex + `{}` input, returns bytes to outer host.
fn wasm_caller(callee_hex: &str) -> Vec<u8> {
    assert!(
        callee_hex.len() <= 64,
        "callee id must fit inline data (pad contract_call_nested if needed)"
    );
    let wat = format!(
        r#"
(module
  (import "spacekit_contract" "contract_call"
    (func $cc (param i32 i32 i32 i32 i32 i32) (result i32)))
  (memory 1)
  (data (i32.const 16) "{callee_hex}")
  (data (i32.const 64) "{{}}")
  (global $outlen (mut i32) (i32.const 0))
  (func $main (param i32 i32) (result i32)
    (global.set $outlen
      (call $cc
        (i32.const 16)
        (i32.const {})
        (i32.const 64)
        (i32.const 2)
        (i32.const 4096)
        (i32.const 256)))
    (global.get $outlen)
  )
  (func $get_result (param i32 i32) (result i32)
    (memory.copy (local.get 0) (i32.const 4096) (global.get $outlen))
    (global.get $outlen)
  )
  (export "memory" (memory 0))
  (export "main" (func $main))
  (export "get_result" (func $get_result))
)
"#,
        callee_hex.len(),
        callee_hex = callee_hex,
    );
    wat::parse_str(&wat).expect("caller WAT")
}

#[tokio::test]
async fn contract_call_nested_invokes_callee() -> anyhow::Result<()> {
    let callee_addr = SwtchvmAddress::new([9u8; 20]);
    let callee_hex = callee_addr.to_string();

    let runtime = SwtchvmRuntime::new(false)?;
    {
        let state_arc = runtime.get_state();
        let mut state = state_arc.write().await;
        let acc = state.get_account_mut(&callee_addr);
        acc.code = Some(wasm_callee());
    }

    let caller_wasm = wasm_caller(&callee_hex);
    let out = runtime.execute_wasm_direct(&caller_wasm, b"x").await?;
    assert!(out.success, "nested call should succeed: {:?}", out);
    assert_eq!(out.return_data, b"B!");

    Ok(())
}

#[tokio::test]
async fn contract_call_missing_callee_returns_minus_one() -> anyhow::Result<()> {
    let missing = SwtchvmAddress::zero();
    let runtime = SwtchvmRuntime::new(false)?;
    // Account may exist with no code — host returns -1.
    let caller_wasm = wasm_caller(&missing.to_string());
    let out = runtime.execute_wasm_direct(&caller_wasm, b"").await?;
    assert!(!out.success, "expected failure: {:?}", out);
    assert_eq!(decode_i32_le(&out.return_data), -1);
    Ok(())
}

#[tokio::test]
async fn contract_call_invalid_contract_id_returns_minus_two() -> anyhow::Result<()> {
    let bad_id = "0xQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQQ";
    let runtime = SwtchvmRuntime::new(false)?;
    let caller_wasm = wasm_caller(bad_id);
    let out = runtime.execute_wasm_direct(&caller_wasm, b"").await?;
    assert!(!out.success);
    assert_eq!(decode_i32_le(&out.return_data), -2);
    Ok(())
}

/// Self-calling module: each `main` invokes `contract_call` on the same address until depth limit.
#[tokio::test]
async fn contract_call_depth_limit_returns_minus_three() -> anyhow::Result<()> {
    let addr = SwtchvmAddress::new([7u8; 20]);
    let hex = addr.to_string();
    let wasm = wasm_caller(&hex);

    let runtime = SwtchvmRuntime::new(false)?;
    {
        let state_arc = runtime.get_state();
        let mut state = state_arc.write().await;
        let acc = state.get_account_mut(&addr);
        acc.code = Some(wasm.clone());
    }

    let out = runtime.execute_wasm_direct(&wasm, b"").await?;
    assert!(
        !out.success,
        "9th nested host call should fail with -3: {:?}",
        out
    );
    assert_eq!(decode_i32_le(&out.return_data), -3);
    Ok(())
}
