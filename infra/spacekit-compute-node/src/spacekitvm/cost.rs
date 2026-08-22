// Cargo.toml dependencies:
// [dependencies]
// wasmtime = { version = "25.0", features = ["component-model"] }
// serde = { version = "1.0", features = ["derive"] }
// serde_json = "1.0"
// tokio = { version = "1.0", features = ["full"] }
// anyhow = "1.0"
// tracing = "0.1"

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use wasmtime::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionCost {
    pub cpu_cycles: u64,
    pub memory_bytes: u64,
    pub execution_time_ns: u64,
    pub gas_used: u64,
    pub instruction_count: u64,
    pub memory_accesses: u64,
    pub function_calls: u64,
    pub total_cost_units: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostConfig {
    pub cpu_cycle_cost: f64,
    pub memory_byte_cost: f64,
    pub time_ns_cost: f64,
    pub instruction_cost: f64,
    pub memory_access_cost: f64,
    pub function_call_cost: f64,
    pub base_cost: f64,
}

impl Default for CostConfig {
    fn default() -> Self {
        Self {
            cpu_cycle_cost: 0.001,      // Cost per CPU cycle
            memory_byte_cost: 0.0001,   // Cost per byte of memory used
            time_ns_cost: 0.000001,     // Cost per nanosecond of execution
            instruction_cost: 0.01,     // Cost per WASM instruction
            memory_access_cost: 0.005,  // Cost per memory read/write
            function_call_cost: 0.1,    // Cost per function call
            base_cost: 1.0,             // Base cost for any execution
        }
    }
}

#[derive(Debug)]
pub struct MeteredStore {
    pub store: Store<ExecutionMetrics>,
}

#[derive(Debug, Clone, Default)]
pub struct ExecutionMetrics {
    pub start_time: Option<Instant>,
    pub instruction_count: u64,
    pub memory_accesses: u64,
    pub function_calls: u64,
    pub peak_memory: u64,
    pub gas_consumed: u64,
}

impl MeteredStore {
    pub fn new(engine: &Engine) -> Self {
        let mut store = Store::new(engine, ExecutionMetrics::default());
        
        // Set up execution limits and metering
        store.limiter(|metrics| &mut *metrics);
        store.set_fuel(1_000_000).unwrap(); // Set initial fuel
        
        // Add epoch interruption for time-based metering
        store.set_epoch_deadline(1);
        
        Self { store }
    }
}

impl ResourceLimiter for ExecutionMetrics {
    fn memory_growing(&mut self, current: usize, desired: usize, maximum: Option<usize>) -> Result<bool, anyhow::Error> {
        self.memory_accesses += 1;
        self.peak_memory = self.peak_memory.max(desired as u64);
        
        // Allow growth up to 100MB
        Ok(desired <= maximum.unwrap_or(100 * 1024 * 1024))
    }
    
    fn table_growing(&mut self, current: u32, desired: u32, maximum: Option<u32>) -> Result<bool, anyhow::Error> {
        self.memory_accesses += 1;
        Ok(desired <= maximum.unwrap_or(10000))
    }
}

pub struct WasmCostCalculator {
    engine: Engine,
    cost_config: CostConfig,
}

impl WasmCostCalculator {
    pub fn new(cost_config: Option<CostConfig>) -> Result<Self> {
        let mut config = Config::new();
        config.consume_fuel(true);
        config.epoch_interruption(true);
        config.cranelift_opt_level(OptLevel::Speed);
        
        let engine = Engine::new(&config)?;
        
        Ok(Self {
            engine,
            cost_config: cost_config.unwrap_or_default(),
        })
    }
    
    pub async fn calculate_execution_cost(
        &self,
        wasm_bytes: &[u8],
        function_name: &str,
        args: &[wasmtime::Val],
    ) -> Result<ExecutionCost> {
        // Create module
        let module = Module::new(&self.engine, wasm_bytes)?;
        
        // Create metered store
        let mut store = MeteredStore::new(&self.engine);
        store.store.data_mut().start_time = Some(Instant::now());
        
        // Create instance
        let instance = Instance::new(&mut store.store, &module, &[])?;
        
        // Get the function to execute
        let func = instance
            .get_func(&mut store.store, function_name)
            .ok_or_else(|| anyhow::anyhow!("Function '{}' not found", function_name))?;
        
        // Prepare result storage
        let mut results = vec![Value::I32(0); func.ty(&store.store).results().len()];
        
        // Start metering
        let start_fuel = store.store.fuel_consumed().unwrap_or(0);
        let start_time = Instant::now();
        
        // Execute function with metering
        let execution_result = func.call(&mut store.store, args, &mut results);
        
        // Calculate metrics
        let end_time = Instant::now();
        let execution_time = end_time.duration_since(start_time);
        let fuel_consumed = store.store.fuel_consumed().unwrap_or(0) - start_fuel;
        
        // Get metrics from store
        let metrics = store.store.data().clone();
        
        // Handle execution result
        match execution_result {
            Ok(_) => {
                let cost = self.calculate_cost_from_metrics(&metrics, execution_time, fuel_consumed);
                Ok(cost)
            }
            Err(e) => {
                // Still calculate cost for failed executions
                let cost = self.calculate_cost_from_metrics(&metrics, execution_time, fuel_consumed);
                tracing::warn!("Execution failed but cost calculated: {:?}, Error: {}", cost, e);
                Ok(cost)
            }
        }
    }
    
    pub async fn calculate_command_cost(
        &self,
        command: &str,
        input_data: &str,
    ) -> Result<ExecutionCost> {
        // For the Pyodide SQL model case
        let wasm_bytes = self.load_pyodide_wasm().await?;
        
        // Prepare arguments for the WASM function
        let args = vec![
            Value::I32(command.as_ptr() as i32),
            Value::I32(command.len() as i32),
            Value::I32(input_data.as_ptr() as i32),
            Value::I32(input_data.len() as i32),
        ];
        
        self.calculate_execution_cost(&wasm_bytes, "execute_command", &args).await
    }
    
    pub fn estimate_cost_before_execution(
        &self,
        estimated_instructions: u64,
        estimated_memory: u64,
        estimated_time_ms: u64,
    ) -> ExecutionCost {
        let estimated_time_ns = estimated_time_ms * 1_000_000;
        
        ExecutionCost {
            cpu_cycles: estimated_instructions / 4, // Rough approximation
            memory_bytes: estimated_memory,
            execution_time_ns: estimated_time_ns,
            gas_used: estimated_instructions * 10, // Gas approximation
            instruction_count: estimated_instructions,
            memory_accesses: estimated_memory / 8, // Assume 8-byte accesses
            function_calls: estimated_instructions / 100, // Rough estimate
            total_cost_units: self.calculate_total_cost(
                estimated_instructions / 4,
                estimated_memory,
                estimated_time_ns,
                estimated_instructions * 10,
                estimated_instructions,
                estimated_memory / 8,
                estimated_instructions / 100,
            ),
        }
    }
    
    fn calculate_cost_from_metrics(
        &self,
        metrics: &ExecutionMetrics,
        execution_time: Duration,
        fuel_consumed: u64,
    ) -> ExecutionCost {
        let execution_time_ns = execution_time.as_nanos() as u64;
        let cpu_cycles = fuel_consumed * 2; // Approximate CPU cycles from fuel
        
        let total_cost = self.calculate_total_cost(
            cpu_cycles,
            metrics.peak_memory,
            execution_time_ns,
            fuel_consumed,
            metrics.instruction_count,
            metrics.memory_accesses,
            metrics.function_calls,
        );
        
        ExecutionCost {
            cpu_cycles,
            memory_bytes: metrics.peak_memory,
            execution_time_ns,
            gas_used: fuel_consumed,
            instruction_count: metrics.instruction_count,
            memory_accesses: metrics.memory_accesses,
            function_calls: metrics.function_calls,
            total_cost_units: total_cost,
        }
    }
    
    fn calculate_total_cost(
        &self,
        cpu_cycles: u64,
        memory_bytes: u64,
        execution_time_ns: u64,
        gas_used: u64,
        instruction_count: u64,
        memory_accesses: u64,
        function_calls: u64,
    ) -> f64 {
        self.cost_config.base_cost
            + (cpu_cycles as f64 * self.cost_config.cpu_cycle_cost)
            + (memory_bytes as f64 * self.cost_config.memory_byte_cost)
            + (execution_time_ns as f64 * self.cost_config.time_ns_cost)
            + (instruction_count as f64 * self.cost_config.instruction_cost)
            + (memory_accesses as f64 * self.cost_config.memory_access_cost)
            + (function_calls as f64 * self.cost_config.function_call_cost)
    }
    
    async fn load_pyodide_wasm(&self) -> Result<Vec<u8>> {
        // In practice, you'd load this from file or embed it
        // For now, return a placeholder
        Ok(include_bytes!("../assets/pyodide_sql_model.wasm").to_vec())
    }
}

// Cost-aware execution manager
pub struct CostAwareExecutor {
    calculator: WasmCostCalculator,
    cost_limits: HashMap<String, f64>,
    execution_history: Vec<(String, ExecutionCost)>,
}

impl CostAwareExecutor {
    pub fn new(cost_config: Option<CostConfig>) -> Result<Self> {
        let calculator = WasmCostCalculator::new(cost_config)?;
        
        Ok(Self {
            calculator,
            cost_limits: HashMap::new(),
            execution_history: Vec::new(),
        })
    }
    
    pub fn set_cost_limit(&mut self, command_type: String, max_cost: f64) {
        self.cost_limits.insert(command_type, max_cost);
    }
    
    pub async fn execute_with_cost_check(
        &mut self,
        command: &str,
        input_data: &str,
        command_type: &str,
    ) -> Result<(bool, ExecutionCost)> {
        // Calculate cost
        let cost = self.calculator.calculate_command_cost(command, input_data).await?;
        
        // Check against limits
        let allowed = if let Some(&limit) = self.cost_limits.get(command_type) {
            cost.total_cost_units <= limit
        } else {
            true // No limit set
        };
        
        // Record execution
        self.execution_history.push((command_type.to_string(), cost.clone()));
        
        tracing::info!(
            "Command: {}, Cost: {:.4} units, Allowed: {}",
            command_type,
            cost.total_cost_units,
            allowed
        );
        
        Ok((allowed, cost))
    }
    
    pub fn get_average_cost(&self, command_type: &str) -> Option<f64> {
        let costs: Vec<f64> = self.execution_history
            .iter()
            .filter(|(cmd_type, _)| cmd_type == command_type)
            .map(|(_, cost)| cost.total_cost_units)
            .collect();
        
        if costs.is_empty() {
            None
        } else {
            Some(costs.iter().sum::<f64>() / costs.len() as f64)
        }
    }
    
    pub fn get_cost_breakdown(&self) -> HashMap<String, Vec<ExecutionCost>> {
        let mut breakdown = HashMap::new();
        
        for (cmd_type, cost) in &self.execution_history {
            breakdown.entry(cmd_type.clone()).or_insert_with(Vec::new).push(cost.clone());
        }
        
        breakdown
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::init();
    
    // Create cost-aware executor
    let mut executor = CostAwareExecutor::new(None)?;
    
    // Set cost limits for different command types
    executor.set_cost_limit("sql_generation".to_string(), 100.0);
    executor.set_cost_limit("model_training".to_string(), 1000.0);
    executor.set_cost_limit("data_analysis".to_string(), 500.0);
    
    // Test commands
    let test_cases = vec![
        ("generate_sql", "Get all employees in Engineering", "sql_generation"),
        ("train_model", "Update with new data", "model_training"),
        ("analyze_data", "Compute statistics", "data_analysis"),
    ];
    
    for (command, input, cmd_type) in test_cases {
        match executor.execute_with_cost_check(command, input, cmd_type).await {
            Ok((allowed, cost)) => {
                println!("Command: {}", command);
                println!("Allowed: {}", allowed);
                println!("Cost breakdown:");
                println!("  CPU cycles: {}", cost.cpu_cycles);
                println!("  Memory bytes: {}", cost.memory_bytes);
                println!("  Execution time: {}ns", cost.execution_time_ns);
                println!("  Instructions: {}", cost.instruction_count);
                println!("  Total cost: {:.4} units", cost.total_cost_units);
                println!("---");
            }
            Err(e) => {
                eprintln!("Error executing {}: {}", command, e);
            }
        }
    }
    
    // Show cost analysis
    println!("Cost Analysis:");
    for cmd_type in ["sql_generation", "model_training", "data_analysis"] {
        if let Some(avg_cost) = executor.get_average_cost(cmd_type) {
            println!("Average cost for {}: {:.4} units", cmd_type, avg_cost);
        }
    }
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_cost_calculation() -> Result<()> {
        let calculator = WasmCostCalculator::new(None)?;
        
        // Test with simple WASM module
        let wasm_bytes = wat::parse_str(r#"
            (module
                (func (export "add") (param i32 i32) (result i32)
                    local.get 0
                    local.get 1
                    i32.add
                )
            )
        "#)?;
        
        let args = vec![Value::I32(5), Value::I32(3)];
        let cost = calculator.calculate_execution_cost(&wasm_bytes, "add", &args).await?;
        
        assert!(cost.total_cost_units > 0.0);
        assert!(cost.instruction_count > 0);
        
        Ok(())
    }
    
    #[tokio::test]
    async fn test_cost_limits() -> Result<()> {
        let mut executor = CostAwareExecutor::new(None)?;
        executor.set_cost_limit("test".to_string(), 50.0);
        
        // This would need actual WASM execution in a real test
        // For now, just test the structure
        
        Ok(())
    }
}