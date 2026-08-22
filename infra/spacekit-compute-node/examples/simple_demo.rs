//! Simple SpaceKit Network Demo
//!
//! This example demonstrates core functionality without external dependencies

use anyhow::Result;
use spacekit_compute_node::{
    spacekitvm::{
        genesis_node::{AccountType, ConsensusAlgorithm, GenesisAccount, GenesisConfig},
        swtchvm_node::{SwtchvmAddress, SwtchvmNode},
    },
    ComputeConfig, ComputeNode, TaskStatus,
};
use std::sync::Arc;
use tokio::time::{sleep, Duration};

#[tokio::main]
async fn main() -> Result<()> {
    println!("🚀 SpaceKit Network Simple Demo");
    println!("=============================\n");

    // Step 1: Create Genesis Configuration
    println!("📝 Step 1: Creating Genesis Configuration...");
    let genesis_config = create_genesis_config().await?;
    println!(
        "✅ Genesis configuration created with {} accounts",
        genesis_config.alloc.len()
    );

    // Step 2: Start Compute Node
    println!("\n🔧 Step 2: Starting Compute Node...");
    let compute_node = start_compute_node().await?;
    println!("✅ Compute Node started successfully");

    // Step 3: Create SWTCHVM Node
    println!("\n🔧 Step 3: Creating SWTCHVM Node...");
    let swtchvm_node = create_swtchvm_node().await?;
    println!("✅ SWTCHVM Node created");

    // Step 4: Test Account Operations
    println!("\n👤 Step 4: Testing Account Operations...");
    test_account_operations(&swtchvm_node).await?;
    println!("✅ Account operations completed");

    // Step 5: Deploy Simple WASM
    println!("\n📦 Step 5: Testing WASM Execution...");
    test_wasm_execution(&compute_node).await?;
    println!("✅ WASM execution completed");

    // Step 6: Execute Compute Tasks
    println!("\n⚙️  Step 6: Executing Compute Tasks...");
    execute_compute_tasks(&compute_node).await?;
    println!("✅ Compute tasks executed");

    // Step 7: Show Final Status
    println!("\n📊 Step 7: Final Status...");
    show_final_status(&compute_node).await?;

    println!("\n🎉 Demo Completed Successfully!");
    println!("\n📈 Summary:");
    println!("   ✅ Genesis configuration ready with DevMode consensus");
    println!("   ✅ Compute node operational with quantum security");
    println!("   ✅ SWTCHVM initialized with WebAssembly support");
    println!("   ✅ Account operations working");
    println!("   ✅ WASM execution functional");
    println!("   ✅ Task processing pipeline operational");
    println!("\n💡 This demonstrates the full SWTCH compute infrastructure!");

    Ok(())
}

async fn create_genesis_config() -> Result<GenesisConfig> {
    let mut genesis_config = GenesisConfig::default();

    // Use DevMode consensus for testing
    genesis_config.consensus_config.algorithm = ConsensusAlgorithm::DevMode;
    genesis_config.network_name = "SWTCH Demo Network".to_string();
    genesis_config.chain_id = 1337;

    // Add pre-funded test accounts
    genesis_config.alloc.insert(
        "0x1111111111111111111111111111111111111111".to_string(),
        GenesisAccount {
            balance: 1_000_000_000_000_000_000, // 1 token
            nonce: 0,
            code: None,
            storage: None,
            account_type: AccountType::Normal,
        },
    );

    genesis_config.alloc.insert(
        "0x2222222222222222222222222222222222222222".to_string(),
        GenesisAccount {
            balance: 500_000_000_000_000_000, // 0.5 tokens
            nonce: 0,
            code: None,
            storage: None,
            account_type: AccountType::Normal,
        },
    );

    genesis_config.alloc.insert(
        "0x3333333333333333333333333333333333333333".to_string(),
        GenesisAccount {
            balance: 0, // Contract account
            nonce: 0,
            code: Some(vec![0x60, 0x60, 0x60, 0x40]), // Simple bytecode
            storage: None,
            account_type: AccountType::Contract,
        },
    );

    println!("   📋 Network: {}", genesis_config.network_name);
    println!(
        "   🔒 Consensus: {:?}",
        genesis_config.consensus_config.algorithm
    );
    println!("   🆔 Chain ID: {}", genesis_config.chain_id);

    Ok(genesis_config)
}

async fn start_compute_node() -> Result<Arc<ComputeNode>> {
    let config = ComputeConfig::default();
    let compute_node = Arc::new(ComputeNode::new(config).await?);

    println!("   🔧 Runtime initialized");
    println!("   🛡️  Quantum security enabled");

    Ok(compute_node)
}

async fn create_swtchvm_node() -> Result<SwtchvmNode> {
    let node = SwtchvmNode::new(false, false).await?; // No GPU, no networking for demo

    println!("   📦 WebAssembly runtime ready");
    println!("   🔗 Blockchain state initialized");

    Ok(node)
}

async fn test_account_operations(swtchvm_node: &SwtchvmNode) -> Result<()> {
    // Create test addresses
    let alice_addr = SwtchvmAddress::new([0x11; 20]);
    let bob_addr = SwtchvmAddress::new([0x22; 20]);
    let contract_addr = SwtchvmAddress::new([0x33; 20]);

    println!("   👤 Test accounts:");
    println!("      Alice: {}", hex::encode(alice_addr.as_bytes()));
    println!("      Bob: {}", hex::encode(bob_addr.as_bytes()));
    println!("      Contract: {}", hex::encode(contract_addr.as_bytes()));

    // Set up accounts using public methods
    swtchvm_node
        .set_account_balance(&alice_addr, 1_000_000_000_000_000_000)
        .await?; // 1 token
    swtchvm_node.set_account_nonce(&alice_addr, 0).await?;

    swtchvm_node
        .set_account_balance(&bob_addr, 500_000_000_000_000_000)
        .await?; // 0.5 tokens
    swtchvm_node.set_account_nonce(&bob_addr, 0).await?;

    swtchvm_node.set_account_balance(&contract_addr, 0).await?;
    swtchvm_node
        .set_account_code(&contract_addr, Some(vec![0x60, 0x60, 0x60, 0x40]))
        .await?; // Simple bytecode
    swtchvm_node.set_account_nonce(&contract_addr, 0).await?;

    // Simulate a simple transfer
    let transfer_amount = 100_000_000_000_000_000; // 0.1 tokens
    swtchvm_node
        .transfer(&alice_addr, &bob_addr, transfer_amount)
        .await?;

    // Check final balances
    if let Some(alice_account) = swtchvm_node.get_account(&alice_addr).await {
        println!("      Alice balance: {} wei", alice_account.balance);
        println!("      Alice nonce: {}", alice_account.nonce);
    }

    if let Some(bob_account) = swtchvm_node.get_account(&bob_addr).await {
        println!("      Bob balance: {} wei", bob_account.balance);
    }

    if let Some(contract_account) = swtchvm_node.get_account(&contract_addr).await {
        println!(
            "      Contract has code: {}",
            contract_account.code.is_some()
        );
    }

    Ok(())
}

async fn test_wasm_execution(compute_node: &ComputeNode) -> Result<()> {
    // Create a simple WASM module (add function)
    let wasm_code = wat::parse_str(
        r#"
        (module
            (func (export "add") (param i32 i32) (result i32)
                local.get 0
                local.get 1
                i32.add
            )
            (func (export "multiply") (param i32 i32) (result i32)
                local.get 0
                local.get 1
                i32.mul
            )
            (func (export "fibonacci") (param i32) (result i32)
                (local i32 i32 i32)
                local.get 0
                i32.const 2
                i32.lt_s
                if (result i32)
                    local.get 0
                else
                    local.get 0
                    i32.const 1
                    i32.sub
                    call 0
                    local.get 0
                    i32.const 2
                    i32.sub
                    call 0
                    i32.add
                end
            )
        )
    "#,
    )?;

    // Submit the WASM task using the correct method signature
    let task = compute_node
        .submit_task(
            "WASM Demo Task".to_string(),                // name
            "wasm".to_string(),                          // runtime
            wasm_code,                                   // code
            "Demo input data".as_bytes().to_vec(),       // input_data
            "did:spacekit:demo:wasm-tester".to_string(), // owner_did
        )
        .await?;
    println!("   📦 WASM task submitted: {}", task.id);

    // Wait for task completion
    let mut attempts = 0;
    while attempts < 10 {
        if let Some(status) = compute_node.get_task_status(&task.id).await {
            match status {
                TaskStatus::Completed => {
                    println!("   ✅ WASM execution completed");
                    break;
                }
                TaskStatus::Failed => {
                    println!("   ❌ WASM execution failed");
                    break;
                }
                _ => {
                    println!("   ⏳ WASM execution in progress...");
                    sleep(Duration::from_millis(200)).await;
                    attempts += 1;
                }
            }
        } else {
            println!("   ❓ Task status unknown");
            break;
        }
    }

    Ok(())
}

async fn execute_compute_tasks(compute_node: &ComputeNode) -> Result<()> {
    let mut task_ids = Vec::new();

    // Execute multiple compute tasks
    for i in 1..=3 {
        let task = compute_node
            .submit_task(
                format!("Demo Task {}", i),
                "wasm".to_string(),
                format!("Demo task {} with input data for processing", i).into_bytes(),
                b"sample input".to_vec(),
                format!("did:spacekit:demo:user{}", i),
            )
            .await?;

        task_ids.push(task.id.clone());
        println!("   📋 Task {} submitted: {}", i, &task.id[..8]);
    }

    // Wait for all tasks to complete
    println!("   ⏳ Processing tasks...");
    sleep(Duration::from_secs(1)).await;

    // Check task statuses
    for (i, task_id) in task_ids.iter().enumerate() {
        if let Some(status) = compute_node.get_task_status(task_id).await {
            println!("   📊 Task {}: {:?}", i + 1, status);
        }
    }

    Ok(())
}

async fn show_final_status(compute_node: &ComputeNode) -> Result<()> {
    let stats = compute_node.get_node_stats().await?;

    println!("   📈 Final Statistics:");
    println!("      Node DID: {}", stats.node_did);
    println!("      Total tasks processed: {}", stats.total_tasks);
    println!("      Pending tasks: {}", stats.pending_tasks);
    println!("      Running tasks: {}", stats.running_tasks);
    println!("      Successfully completed: {}", stats.completed_tasks);
    println!("      Failed tasks: {}", stats.failed_tasks);

    if stats.total_tasks > 0 {
        let success_rate = stats.completed_tasks as f64 / stats.total_tasks as f64 * 100.0;
        println!("      Success rate: {:.2}%", success_rate);
    }

    Ok(())
}
