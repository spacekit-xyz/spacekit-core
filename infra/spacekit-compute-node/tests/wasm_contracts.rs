use std::fs;
use std::path::PathBuf;

use spacekit_compute_node::spacekitvm::swtchvm_node::SwtchvmRuntime;

const ERC20_MINT: u8 = 1;
const ERC20_TRANSFER: u8 = 2;
const ERC20_BALANCE: u8 = 3;
const ERC20_TOTAL_SUPPLY: u8 = 4;

const ERC721_MINT: u8 = 1;
const ERC721_TRANSFER: u8 = 2;
const ERC721_OWNER_OF: u8 = 3;
const ERC721_SET_URI: u8 = 4;
const ERC721_TOKEN_URI: u8 = 5;
const ERC721_TOTAL_SUPPLY: u8 = 6;

fn artifacts_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("contracts")
        .join("artifacts")
}

fn load_wasm(name: &str) -> Vec<u8> {
    let path = artifacts_dir().join(name);
    fs::read(&path).unwrap_or_else(|_| {
        panic!(
            "Missing wasm artifact: {} (run scripts/build_contracts.sh)",
            path.display()
        )
    })
}

fn encode_string(out: &mut Vec<u8>, value: &str) {
    let len = value.len() as u16;
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(value.as_bytes());
}

fn encode_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn decode_u64(data: &[u8]) -> u64 {
    let bytes = [
        data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
    ];
    u64::from_le_bytes(bytes)
}

#[tokio::test]
async fn astra_erc20_contract_smoke() -> anyhow::Result<()> {
    let wasm = load_wasm("astra_erc20_contract.wasm");
    let runtime = SwtchvmRuntime::new(false)?;

    let mut mint_input = vec![ERC20_MINT];
    encode_string(&mut mint_input, "did:astra:alice");
    encode_u64(&mut mint_input, 1_000_000);

    let mint_result = runtime.execute_wasm_direct(&wasm, &mint_input).await?;
    assert!(mint_result.success);
    assert_eq!(mint_result.return_data, vec![1u8]);

    let mut balance_input = vec![ERC20_BALANCE];
    encode_string(&mut balance_input, "did:astra:alice");

    let balance_result = runtime.execute_wasm_direct(&wasm, &balance_input).await?;
    assert!(balance_result.success);
    assert_eq!(decode_u64(&balance_result.return_data), 1_000_000);

    let total_result = runtime
        .execute_wasm_direct(&wasm, &[ERC20_TOTAL_SUPPLY])
        .await?;
    assert!(total_result.success);
    assert_eq!(decode_u64(&total_result.return_data), 1_000_000);

    let mut transfer_input = vec![ERC20_TRANSFER];
    encode_string(&mut transfer_input, "did:astra:alice");
    encode_string(&mut transfer_input, "did:astra:bob");
    encode_u64(&mut transfer_input, 250_000);

    let transfer_result = runtime.execute_wasm_direct(&wasm, &transfer_input).await?;
    assert!(transfer_result.success);
    assert_eq!(transfer_result.return_data, vec![1u8]);

    let mut bob_balance_input = vec![ERC20_BALANCE];
    encode_string(&mut bob_balance_input, "did:astra:bob");
    let bob_balance = runtime
        .execute_wasm_direct(&wasm, &bob_balance_input)
        .await?;
    assert_eq!(decode_u64(&bob_balance.return_data), 250_000);

    Ok(())
}

#[tokio::test]
async fn astra_erc721_contract_smoke() -> anyhow::Result<()> {
    let wasm = load_wasm("astra_erc721_contract.wasm");
    let runtime = SwtchvmRuntime::new(false)?;

    let mut mint_input = vec![ERC721_MINT];
    encode_u64(&mut mint_input, 1);
    encode_string(&mut mint_input, "did:astra:alice");

    let mint_result = runtime.execute_wasm_direct(&wasm, &mint_input).await?;
    assert!(mint_result.success);
    assert_eq!(mint_result.return_data, vec![1u8]);

    let mut owner_input = vec![ERC721_OWNER_OF];
    encode_u64(&mut owner_input, 1);
    let owner_result = runtime.execute_wasm_direct(&wasm, &owner_input).await?;
    assert!(owner_result.success);
    assert_eq!(owner_result.return_data, b"did:astra:alice".to_vec());

    let mut uri_set = vec![ERC721_SET_URI];
    encode_u64(&mut uri_set, 1);
    encode_string(&mut uri_set, "ipfs://astra/1");
    let uri_set_result = runtime.execute_wasm_direct(&wasm, &uri_set).await?;
    assert!(uri_set_result.success);

    let mut uri_get = vec![ERC721_TOKEN_URI];
    encode_u64(&mut uri_get, 1);
    let uri_get_result = runtime.execute_wasm_direct(&wasm, &uri_get).await?;
    assert_eq!(uri_get_result.return_data, b"ipfs://astra/1".to_vec());

    let supply_result = runtime
        .execute_wasm_direct(&wasm, &[ERC721_TOTAL_SUPPLY])
        .await?;
    assert_eq!(decode_u64(&supply_result.return_data), 1);

    Ok(())
}
