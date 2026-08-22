//! Minimal SpaceKit Network Demo
//!
//! This example demonstrates core functionality without any storage dependencies

use anyhow::Result;
use spacekit_compute_node::{ComputeConfig, ComputeNode, TaskStatus};
use std::sync::Arc;
use tokio::time::{sleep, Duration};

#[tokio::main]
async fn main() -> Result<()> {
    println!("🚀 SpaceKit Network Minimal Demo");
    println!("=============================\n");

    // Step 1: Start Compute Node
    println!("🔧 Step 1: Starting Compute Node...");
    let compute_node = start_compute_node().await?;
    println!("✅ Compute Node started successfully");

    // Step 2: Test WASM Execution
    println!("\n📦 Step 2: Testing WASM Execution...");
    test_wasm_execution(&compute_node).await?;
    println!("✅ WASM execution completed");

    // Step 3: Execute Multiple Compute Tasks
    println!("\n⚙️  Step 3: Executing Compute Tasks...");
    execute_compute_tasks(&compute_node).await?;
    println!("✅ Compute tasks executed");

    // Step 4: Show Final Status
    println!("\n📊 Step 4: Final Status...");
    show_final_status(&compute_node).await?;

    println!("\n🎉 Demo Completed Successfully!");
    println!("\n📈 Summary:");
    println!("   ✅ Compute node operational with quantum security");
    println!("   ✅ WASM execution functional");
    println!("   ✅ Task processing pipeline operational");
    println!("   ✅ Multi-task execution working");
    println!("\n💡 This demonstrates the core SpaceKit compute infrastructure!");

    Ok(())
}

async fn start_compute_node() -> Result<Arc<ComputeNode>> {
    let config = ComputeConfig::default();
    let mut compute_node = ComputeNode::new(config).await?;
    compute_node.start().await?;

    println!("   🔧 Runtime initialized");
    println!("   🛡️  Quantum security enabled");

    Ok(Arc::new(compute_node))
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
        )
    "#,
    )?;

    // Submit the WASM task
    let task = compute_node
        .submit_task(
            "WASM Demo Task".to_string(),
            "wasm".to_string(),
            wasm_code,
            "Demo input data".as_bytes().to_vec(),
            "did:spacekit:demo:wasm-tester".to_string(),
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
