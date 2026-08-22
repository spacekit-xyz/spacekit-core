//! GPU Compute Demo
//!
//! Demonstrates how to use the SpaceKit GPU compute system with WebGPU backend.

use anyhow::Result;
use spacekit_compute_node::spacekitvm::calculation::{
    ComputeBackend, ComputeRequest, HybridGpuManager,
};

#[tokio::main]
async fn main() -> Result<()> {
    // Example usage
    let mut gpu_manager = HybridGpuManager::new(true).await?;

    // Simple compute shader example
    let compute_shader = r#"
        @group(0) @binding(0) var<storage, read> input_data: array<f32>;
        @group(0) @binding(1) var<storage, read_write> output_data: array<f32>;
        
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
            let index = global_id.x;
            if (index >= arrayLength(&input_data)) {
                return;
            }
            
            // Simple operation: square each element
            output_data[index] = input_data[index] * input_data[index];
        }
    "#;

    // Prepare input data
    let input_data: Vec<f32> = (0..1024).map(|i| i as f32).collect();
    let input_bytes: Vec<u8> = input_data.iter().flat_map(|f| f.to_le_bytes()).collect();

    let compute_request = ComputeRequest {
        preferred_backend: ComputeBackend::WebGPU,
        shader_code: compute_shader.to_string(),
        kernel_name: None,
        input_data: input_bytes,
        workgroup_size: (64, 1, 1),
        grid_size: None,
        block_size: None,
    };

    // Execute compute
    match gpu_manager
        .execute_optimal_compute("user1", compute_request)
        .await
    {
        Ok((result, cost)) => {
            println!("GPU Execution completed!");
            println!("Result size: {} bytes", result.len());
            println!("GPU Cost: ${:.4}", cost.total_cost);
            println!("Execution time: {:.3}s", cost.gpu_time_seconds);
            println!("Memory usage: {:.3} GB-seconds", cost.gpu_memory_gb_seconds);
            println!("Power consumption: {:.6} kWh", cost.power_consumption_kwh);
        }
        Err(e) => {
            println!("GPU execution failed: {}", e);
        }
    }

    // Show GPU utilization
    let utilization = gpu_manager.wgpu_manager.get_gpu_utilization().await;
    println!("GPU Utilization: {:?}", utilization);

    let available_gpus = gpu_manager.wgpu_manager.get_available_gpus().await;
    println!("Available GPUs: {}", available_gpus.len());

    Ok(())
}
