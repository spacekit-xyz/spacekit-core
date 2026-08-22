//! Smoke-test **standard-library** contract WASMs on the Rust compute VM.
//!
//! Build artifacts (from repo root):
//! `cd spacekit-standard-library && cargo build --release --target wasm32-unknown-unknown`
//!
//! Output: `spacekit-standard-library/target/wasm32-unknown-unknown/release/*.wasm`
//!
//! Override directory: `SPACEKIT_STDLIB_WASM_DIR=/path/to/release`.

use std::env;
use std::fs;
use std::path::PathBuf;

use spacekit_compute_node::spacekitvm::swtchvm_node::SwtchvmRuntime;

fn stdlib_wasm_release_dir() -> PathBuf {
    if let Ok(p) = env::var("SPACEKIT_STDLIB_WASM_DIR") {
        return PathBuf::from(p);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("spacekit-standard-library")
        .join("target")
        .join("wasm32-unknown-unknown")
        .join("release")
}

fn try_read_wasm(name: &str) -> Option<Vec<u8>> {
    let path = stdlib_wasm_release_dir().join(name);
    if !path.is_file() {
        return None;
    }
    fs::read(&path).ok()
}

/// `routekit-agent`: opcode **HEALTH** `0x10` (empty body). Uses Growformer + vault host stubs on the VM.
#[tokio::test]
async fn routekit_agent_health_stdlib_wasm() -> anyhow::Result<()> {
    let Some(wasm) = try_read_wasm("routekit_agent.wasm") else {
        eprintln!(
            "SKIP routekit_agent.wasm (set SPACEKIT_STDLIB_WASM_DIR or build stdlib wasm release)"
        );
        return Ok(());
    };

    let runtime = SwtchvmRuntime::new(false)?;
    let out = runtime.execute_wasm_direct(&wasm, &[0x10]).await?;
    assert!(out.success, "routekit HEALTH should succeed: {:?}", out);
    let text = String::from_utf8_lossy(&out.return_data);
    assert!(
        text.contains("routekit-agent") && text.contains("growformer_status"),
        "unexpected HEALTH JSON: {text}"
    );
    Ok(())
}

/// `spacekit-agent`: opcode **STATUS** `6` (no extra payload after op byte).
#[tokio::test]
async fn spacekit_agent_status_stdlib_wasm() -> anyhow::Result<()> {
    let Some(wasm) = try_read_wasm("spacekit_agent.wasm") else {
        eprintln!("SKIP spacekit_agent.wasm — build stdlib wasm release first");
        return Ok(());
    };

    let runtime = SwtchvmRuntime::new(false)?;
    let out = runtime.execute_wasm_direct(&wasm, &[6u8]).await;
    match out {
        Ok(r) => {
            assert!(r.success, "spacekit_agent STATUS: {:?}", r);
            let s = String::from_utf8_lossy(&r.return_data);
            assert!(
                s == "not_loaded" || s == "ready" || s == "loading" || s == "unknown",
                "unexpected llm status string: {s:?}"
            );
        }
        Err(e) => {
            // `spacekit_llm` is deprecated on the Rust VM (not linked); older `spacekit_agent.wasm` may still import it.
            let msg = e.to_string();
            if msg.contains("import") || msg.contains("unknown import") {
                eprintln!(
                    "SKIP spacekit_agent (deprecated/missing imports, often spacekit_llm): {msg}"
                );
                return Ok(());
            }
            return Err(e);
        }
    }
    Ok(())
}
