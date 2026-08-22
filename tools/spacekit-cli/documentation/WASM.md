# SpaceKit WebAssembly Smart Contracts Guide

🚀 **Complete Guide to Building, Deploying, and Interacting with WASM Smart Contracts on SpaceKit Network** - From development to production deployment using quantum-resistant distributed computing.

## 🎯 **Overview**

SpaceKit Network provides a quantum-resistant distributed computing platform where WebAssembly (WASM) smart contracts can be deployed, executed, and managed using post-quantum cryptography. This guide covers the complete development lifecycle.

### **🔧 Why WASM on SpaceKit?**
- **⚡ High Performance**: Near-native execution speed
- **🔐 Quantum-Safe**: Protected by post-quantum cryptography
- **🌐 Distributed**: Executed across decentralized compute nodes
- **🛡️ Secure**: Sandboxed execution environment
- **🔄 Portable**: Write once, run anywhere
- **💰 Cost-Effective**: Pay-per-execution model

## 🏗️ **Setting Up Your Development Environment**

### **Prerequisites**

```bash
# Install Rust and WebAssembly target
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup target add wasm32-unknown-unknown

# Install WebAssembly tools
cargo install wasm-pack
cargo install wabt

# Initialize SpaceKit workspace
spacekit init --algorithm kyber768 --name my-wasm-project --validate
cd my-wasm-project
```

### **Project Structure**

After running `spacekit init`, you'll have:

```
my-wasm-project/
├── contracts/          # WASM smart contracts
│   ├── counter.rs      # Example contract (generated)
│   ├── calculator.rs   # Mathematical operations
│   └── storage.rs      # Data persistence
├── scripts/            # Deployment scripts
│   └── deploy.sh       # Auto-generated deployment
├── storage/            # Local data storage
├── tests/              # Contract tests
└── swtch.toml         # Project configuration
```

## 💻 **Creating Your First WASM Smart Contract**

### **Example 1: Simple Counter Contract**

Create `contracts/counter.rs`:

```rust
use std::sync::atomic::{AtomicU64, Ordering};

// Global state - in production, this would be persisted
static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Increment the counter and return the new value
#[no_mangle]
pub extern "C" fn increment() -> u64 {
    COUNTER.fetch_add(1, Ordering::SeqCst) + 1
}

/// Get the current counter value
#[no_mangle]
pub extern "C" fn get_value() -> u64 {
    COUNTER.load(Ordering::SeqCst)
}

/// Reset the counter to zero
#[no_mangle]
pub extern "C" fn reset() -> u64 {
    COUNTER.store(0, Ordering::SeqCst);
    0
}

/// Set the counter to a specific value
#[no_mangle]
pub extern "C" fn set_value(value: u64) -> u64 {
    COUNTER.store(value, Ordering::SeqCst);
    value
}

/// Main entry point for SpaceKit Network execution
#[no_mangle]
pub extern "C" fn main() -> u64 {
    increment()
}
```

### **Example 2: Calculator Contract**

Create `contracts/calculator.rs`:

```rust
/// Mathematical operations contract for SpaceKit Network

#[no_mangle]
pub extern "C" fn add(a: i64, b: i64) -> i64 {
    a + b
}

#[no_mangle]
pub extern "C" fn subtract(a: i64, b: i64) -> i64 {
    a - b
}

#[no_mangle]
pub extern "C" fn multiply(a: i64, b: i64) -> i64 {
    a * b
}

#[no_mangle]
pub extern "C" fn divide(a: i64, b: i64) -> i64 {
    if b == 0 {
        return -1; // Error: division by zero
    }
    a / b
}

#[no_mangle]
pub extern "C" fn factorial(n: u64) -> u64 {
    if n <= 1 {
        1
    } else {
        n * factorial(n - 1)
    }
}

#[no_mangle]
pub extern "C" fn fibonacci(n: u64) -> u64 {
    if n <= 1 {
        n
    } else {
        fibonacci(n - 1) + fibonacci(n - 2)
    }
}

/// Main entry point - calculates factorial of 10
#[no_mangle]
pub extern "C" fn main() -> u64 {
    factorial(10)
}
```

### **Example 3: Data Processing Contract**

Create `contracts/data_processor.rs`:

```rust
use std::collections::HashMap;
use std::sync::Mutex;

// Global data store - in production, use SpaceKit storage integration
static DATA_STORE: Mutex<HashMap<u64, Vec<u8>>> = Mutex::new(HashMap::new());

/// Store data with a given key
#[no_mangle]
pub extern "C" fn store_data(key: u64, data_ptr: *const u8, data_len: usize) -> u64 {
    unsafe {
        let data = std::slice::from_raw_parts(data_ptr, data_len).to_vec();
        let mut store = DATA_STORE.lock().unwrap();
        store.insert(key, data);
        key
    }
}

/// Retrieve data by key (returns length)
#[no_mangle]
pub extern "C" fn get_data_length(key: u64) -> usize {
    let store = DATA_STORE.lock().unwrap();
    store.get(&key).map(|v| v.len()).unwrap_or(0)
}

/// Process data: calculate sum of bytes
#[no_mangle]
pub extern "C" fn process_sum(key: u64) -> u64 {
    let store = DATA_STORE.lock().unwrap();
    if let Some(data) = store.get(&key) {
        data.iter().map(|&b| b as u64).sum()
    } else {
        0
    }
}

/// Main entry point
#[no_mangle]
pub extern "C" fn main() -> u64 {
    // Default processing
    42
}
```

## 🔨 **Compiling WASM Contracts**

### **Build Configuration**

Create `Cargo.toml` in your project root:

```toml
[package]
name = "swtch-wasm-contracts"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
# Add any dependencies your contracts need
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"

[profile.release]
opt-level = "s"  # Optimize for size
lto = true       # Link-time optimization
panic = "abort"  # Smaller binary size
```

### **Compilation Commands**

```bash
# Compile individual contracts
rustc --target wasm32-unknown-unknown -O contracts/counter.rs -o counter.wasm
rustc --target wasm32-unknown-unknown -O contracts/calculator.rs -o calculator.wasm
rustc --target wasm32-unknown-unknown -O contracts/data_processor.rs -o data_processor.wasm

# Or use cargo for more complex projects
cargo build --target wasm32-unknown-unknown --release

# Optimize WASM binaries (optional)
wasm-opt -Os counter.wasm -o counter_optimized.wasm
```

### **Automated Build Script**

Create `scripts/build_contracts.sh`:

```bash
#!/bin/bash
set -e

echo "🔨 Building SpaceKit WASM Smart Contracts..."

# Create build directory
mkdir -p build/

# Get the current DID from config
DID=$(grep 'did =' ~/.spacekit/config.toml | cut -d'"' -f2)
echo "📋 Building for DID: $DID"

# Build contracts
echo "🏗️  Compiling counter contract..."
rustc --target wasm32-unknown-unknown -O contracts/counter.rs -o build/counter.wasm

echo "🧮 Compiling calculator contract..."
rustc --target wasm32-unknown-unknown -O contracts/calculator.rs -o build/calculator.wasm

echo "💾 Compiling data processor contract..."
rustc --target wasm32-unknown-unknown -O contracts/data_processor.rs -o build/data_processor.wasm

# Verify builds
echo "✅ Build Summary:"
for wasm in build/*.wasm; do
    if [ -f "$wasm" ]; then
        size=$(wc -c < "$wasm")
        echo "   $(basename "$wasm"): ${size} bytes"
    fi
done

echo "🚀 Ready for deployment with: spacekit task submit --file build/CONTRACT.wasm"
```

Make it executable:
```bash
chmod +x scripts/build_contracts.sh
```

## 🚀 **Deploying Contracts to SpaceKit Network**

### **Basic Deployment**

```bash
# Build your contracts first
./scripts/build_contracts.sh

# Get your DID from config
DID=$(grep 'did =' ~/.spacekit/config.toml | cut -d'"' -f2)

# Deploy counter contract
spacekit task submit \
  --file build/counter.wasm \
  --runtime wasm \
  --owner-did "$DID" \
  --encryption kyber768 \
  --max-cost 0.001

# Expected output:
# ✅ Task submitted successfully!
# 🆔 Task ID: task_counter_1704067200_abc123
# 📋 Status: Queued
# 💰 Estimated cost: 0.0008 SpaceKit
```

### **Advanced Deployment with Input Data**

```bash
# Create input data for calculator contract
echo '{"operation": "factorial", "value": 10}' > input/calc_input.json

# Deploy with input data
spacekit task submit \
  --file build/calculator.wasm \
  --runtime wasm \
  --owner-did "$DID" \
  --input input/calc_input.json \
  --encryption kyber1024 \
  --max-cost 0.005

# Deploy data processor with custom configuration
spacekit task submit \
  --file build/data_processor.wasm \
  --runtime wasm \
  --owner-did "$DID" \
  --encryption kyber768 \
  --max-cost 0.002
```

### **Batch Deployment Script**

Create `scripts/deploy_all.sh`:

```bash
#!/bin/bash
set -e

echo "🚀 Deploying SpaceKit WASM Smart Contracts..."

# Get DID
DID=$(grep 'did =' ~/.spacekit/config.toml | cut -d'"' -f2)
echo "👤 Deploying as: $DID"

# Build contracts first
./scripts/build_contracts.sh

# Deploy all contracts
echo "📦 Deploying counter contract..."
COUNTER_TASK=$(spacekit task submit \
  --file build/counter.wasm \
  --runtime wasm \
  --owner-did "$DID" \
  --encryption kyber768 | grep "Task ID:" | cut -d' ' -f3)

echo "🧮 Deploying calculator contract..."
CALC_TASK=$(spacekit task submit \
  --file build/calculator.wasm \
  --runtime wasm \
  --owner-did "$DID" \
  --encryption kyber768 | grep "Task ID:" | cut -d' ' -f3)

echo "💾 Deploying data processor contract..."
DATA_TASK=$(spacekit task submit \
  --file build/data_processor.wasm \
  --runtime wasm \
  --owner-did "$DID" \
  --encryption kyber768 | grep "Task ID:" | cut -d' ' -f3)

# Save task IDs for later use
echo "COUNTER_TASK=$COUNTER_TASK" > .task_ids
echo "CALC_TASK=$CALC_TASK" >> .task_ids
echo "DATA_TASK=$DATA_TASK" >> .task_ids

echo "✅ All contracts deployed!"
echo "📋 Task IDs saved to .task_ids"
echo "💡 Monitor with: spacekit task watch TASK_ID"
```

## 🔍 **Monitoring and Interacting with Deployed Contracts**

### **Check Deployment Status**

```bash
# Check specific task status
spacekit task status task_counter_1704067200_abc123

# Watch real-time progress
spacekit task watch task_counter_1704067200_abc123 --interval 3

# List all your tasks
spacekit task list --owned-by-me --status completed
```

### **Retrieve Contract Results**

```bash
# Get execution results
spacekit task result task_counter_1704067200_abc123 --output results/counter_result.bin

# View results (if text-based)
spacekit task result task_calc_1704067300_def456 --output results/calc_result.json
cat results/calc_result.json

# Monitor multiple contracts
for task_id in $(cat .task_ids | cut -d'=' -f2); do
    echo "📊 Checking $task_id..."
    spacekit task status "$task_id"
done
```

### **Interactive Monitoring Script**

Create `scripts/monitor_contracts.sh`:

```bash
#!/bin/bash

echo "📊 SpaceKit Contract Monitoring Dashboard"
echo "======================================="

# Load task IDs
if [ -f ".task_ids" ]; then
    source .task_ids
else
    echo "❌ No task IDs found. Run deploy_all.sh first."
    exit 1
fi

# Function to check task status
check_status() {
    local task_id=$1
    local name=$2
    
    echo "🔍 $name ($task_id):"
    status_output=$(spacekit task status "$task_id" 2>/dev/null || echo "Status: Unknown")
    echo "   $status_output"
    echo ""
}

# Monitor all contracts
while true; do
    clear
    echo "📊 SpaceKit Contract Status Dashboard - $(date)"
    echo "=============================================="
    echo ""
    
    check_status "$COUNTER_TASK" "Counter Contract"
    check_status "$CALC_TASK" "Calculator Contract"
    check_status "$DATA_TASK" "Data Processor Contract"
    
    echo "🔄 Refreshing in 10 seconds... (Ctrl+C to exit)"
    sleep 10
done
```

## 🧪 **Testing WASM Contracts**

### **Local Testing Before Deployment**

Create `tests/contract_tests.rs`:

```rust
use std::process::Command;

#[test]
fn test_counter_contract() {
    // Compile the contract
    let output = Command::new("rustc")
        .args(&[
            "--target", "wasm32-unknown-unknown",
            "-O", "contracts/counter.rs",
            "-o", "test_counter.wasm"
        ])
        .output()
        .expect("Failed to compile counter contract");
    
    assert!(output.status.success(), "Counter compilation failed");
    
    // Test with WASM runtime (wasmtime, wasmer, etc.)
    // This is a simplified test - in practice, you'd use a WASM runtime
    println!("✅ Counter contract compiled successfully");
}

#[test]
fn test_calculator_contract() {
    let output = Command::new("rustc")
        .args(&[
            "--target", "wasm32-unknown-unknown",
            "-O", "contracts/calculator.rs",
            "-o", "test_calculator.wasm"
        ])
        .output()
        .expect("Failed to compile calculator contract");
    
    assert!(output.status.success(), "Calculator compilation failed");
    println!("✅ Calculator contract compiled successfully");
}
```

### **Integration Testing with SpaceKit Network**

Create `scripts/test_deployment.sh`:

```bash
#!/bin/bash
set -e

echo "🧪 Testing SpaceKit WASM Contract Deployment..."

# Get DID
DID=$(grep 'did =' ~/.spacekit/config.toml | cut -d'"' -f2)

# Build test contract
echo "🔨 Building test contract..."
rustc --target wasm32-unknown-unknown -O contracts/counter.rs -o test_counter.wasm

# Submit test task
echo "🚀 Submitting test task..."
TASK_ID=$(spacekit task submit \
  --file test_counter.wasm \
  --runtime wasm \
  --owner-did "$DID" \
  --encryption kyber768 \
  --max-cost 0.001 | grep "Task ID:" | cut -d' ' -f3)

echo "📋 Test task ID: $TASK_ID"

# Wait for completion (with timeout)
echo "⏳ Waiting for task completion..."
timeout=60
elapsed=0

while [ $elapsed -lt $timeout ]; do
    status=$(spacekit task status "$TASK_ID" | grep "Status:" | cut -d' ' -f2 || echo "Unknown")
    
    case $status in
        "Completed")
            echo "✅ Test completed successfully!"
            spacekit task result "$TASK_ID" --output test_result.bin
            echo "📊 Result saved to test_result.bin"
            break
            ;;
        "Failed")
            echo "❌ Test failed!"
            exit 1
            ;;
        "Cancelled")
            echo "🚫 Test was cancelled!"
            exit 1
            ;;
        *)
            echo "⏳ Status: $status (${elapsed}s elapsed)"
            sleep 5
            elapsed=$((elapsed + 5))
            ;;
    esac
done

if [ $elapsed -ge $timeout ]; then
    echo "⏰ Test timed out after ${timeout}s"
    exit 1
fi

# Cleanup
rm -f test_counter.wasm test_result.bin

echo "🎉 Integration test completed successfully!"
```

## 🔧 **Advanced WASM Contract Patterns**

### **State Management Contract**

Create `contracts/state_manager.rs`:

```rust
use std::collections::HashMap;
use std::sync::RwLock;

// Global state storage
static STATE: RwLock<HashMap<String, Vec<u8>>> = RwLock::new(HashMap::new());

/// Store key-value data
#[no_mangle]
pub extern "C" fn store(key_ptr: *const u8, key_len: usize, value_ptr: *const u8, value_len: usize) -> u32 {
    unsafe {
        let key = String::from_utf8_lossy(std::slice::from_raw_parts(key_ptr, key_len)).to_string();
        let value = std::slice::from_raw_parts(value_ptr, value_len).to_vec();
        
        let mut state = STATE.write().unwrap();
        state.insert(key, value);
        1 // Success
    }
}

/// Retrieve data by key
#[no_mangle]
pub extern "C" fn get(key_ptr: *const u8, key_len: usize) -> u32 {
    unsafe {
        let key = String::from_utf8_lossy(std::slice::from_raw_parts(key_ptr, key_len));
        let state = STATE.read().unwrap();
        
        match state.get(key.as_ref()) {
            Some(_) => 1, // Found
            None => 0,    // Not found
        }
    }
}

/// Get size of stored value
#[no_mangle]
pub extern "C" fn size(key_ptr: *const u8, key_len: usize) -> usize {
    unsafe {
        let key = String::from_utf8_lossy(std::slice::from_raw_parts(key_ptr, key_len));
        let state = STATE.read().unwrap();
        
        state.get(key.as_ref()).map(|v| v.len()).unwrap_or(0)
    }
}

#[no_mangle]
pub extern "C" fn main() -> u32 {
    // Initialize with some default data
    let key = "initialized";
    let value = b"true";
    
    unsafe {
        store(key.as_ptr(), key.len(), value.as_ptr(), value.len())
    }
}
```

### **Event Logging Contract**

Create `contracts/event_logger.rs`:

```rust
use std::sync::Mutex;

#[derive(Debug)]
struct Event {
    timestamp: u64,
    event_type: u32,
    data: Vec<u8>,
}

static EVENTS: Mutex<Vec<Event>> = Mutex::new(Vec::new());

/// Log an event
#[no_mangle]
pub extern "C" fn log_event(event_type: u32, data_ptr: *const u8, data_len: usize) -> u64 {
    unsafe {
        let data = std::slice::from_raw_parts(data_ptr, data_len).to_vec();
        let timestamp = get_timestamp(); // Mock timestamp
        
        let event = Event {
            timestamp,
            event_type,
            data,
        };
        
        let mut events = EVENTS.lock().unwrap();
        events.push(event);
        timestamp
    }
}

/// Get number of logged events
#[no_mangle]
pub extern "C" fn get_event_count() -> usize {
    let events = EVENTS.lock().unwrap();
    events.len()
}

/// Get event by index
#[no_mangle]
pub extern "C" fn get_event_timestamp(index: usize) -> u64 {
    let events = EVENTS.lock().unwrap();
    events.get(index).map(|e| e.timestamp).unwrap_or(0)
}

// Mock timestamp function
fn get_timestamp() -> u64 {
    // In a real implementation, this would get actual timestamp
    // For now, use a simple counter
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1000);
    COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
}

#[no_mangle]
pub extern "C" fn main() -> u64 {
    // Log initialization event
    let init_data = b"Contract initialized";
    unsafe {
        log_event(1, init_data.as_ptr(), init_data.len())
    }
}
```

## 📊 **Performance Optimization**

### **Size Optimization**

```bash
# Optimize for size
rustc --target wasm32-unknown-unknown -C opt-level=s contracts/counter.rs -o counter_small.wasm

# Use wasm-opt for further optimization
wasm-opt -Os counter_small.wasm -o counter_optimized.wasm

# Strip debug information
wasm-strip counter_optimized.wasm

# Compare sizes
ls -la *.wasm
```

### **Performance Benchmarking**

Create `scripts/benchmark_contracts.sh`:

```bash
#!/bin/bash

echo "📊 SpaceKit WASM Contract Performance Benchmark"
echo "============================================="

DID=$(grep 'did =' ~/.spacekit/config.toml | cut -d'"' -f2)

# Benchmark function
benchmark_contract() {
    local contract=$1
    local name=$2
    
    echo "🔍 Benchmarking $name..."
    
    # Submit task and measure time
    start_time=$(date +%s)
    
    TASK_ID=$(spacekit task submit \
      --file "$contract" \
      --runtime wasm \
      --owner-did "$DID" \
      --encryption kyber768 | grep "Task ID:" | cut -d' ' -f3)
    
    echo "  📋 Task ID: $TASK_ID"
    
    # Wait for completion
    while true; do
        status=$(spacekit task status "$TASK_ID" | grep "Status:" | cut -d' ' -f2)
        
        if [ "$status" = "Completed" ]; then
            end_time=$(date +%s)
            duration=$((end_time - start_time))
            echo "  ✅ Completed in ${duration}s"
            
            # Get file size
            size=$(wc -c < "$contract")
            echo "  📦 Size: ${size} bytes"
            break
        elif [ "$status" = "Failed" ]; then
            echo "  ❌ Failed"
            break
        fi
        
        sleep 2
    done
    
    echo ""
}

# Run benchmarks
benchmark_contract "build/counter.wasm" "Counter Contract"
benchmark_contract "build/calculator.wasm" "Calculator Contract"
benchmark_contract "build/data_processor.wasm" "Data Processor Contract"

echo "🏁 Benchmark completed!"
```

## 🔒 **Security Best Practices**

### **Secure Contract Development**

```rust
// Example: Input validation contract
#[no_mangle]
pub extern "C" fn safe_divide(a: i64, b: i64) -> i64 {
    // Input validation
    if b == 0 {
        return -1; // Error code for division by zero
    }
    
    // Overflow protection
    if a == i64::MIN && b == -1 {
        return i64::MAX; // Prevent overflow
    }
    
    a / b
}

#[no_mangle]
pub extern "C" fn safe_array_access(index: usize, max_size: usize) -> u32 {
    // Bounds checking
    if index >= max_size {
        return 0; // Error: index out of bounds
    }
    
    // Safe access logic here
    1 // Success
}
```

### **Access Control Contract**

```rust
static AUTHORIZED_OWNER: &str = "did:spacekit:user:authorized_owner";

#[no_mangle]
pub extern "C" fn admin_function(caller_ptr: *const u8, caller_len: usize) -> u32 {
    unsafe {
        let caller = std::str::from_utf8_unchecked(
            std::slice::from_raw_parts(caller_ptr, caller_len)
        );
        
        // Access control check
        if caller != AUTHORIZED_OWNER {
            return 0; // Unauthorized
        }
        
        // Admin logic here
        1 // Success
    }
}
```

## 🤖 **AI/ML WASM Contracts with Python**

### **Python to WASM Compilation**

SpaceKit Network supports AI/ML workloads by compiling Python code to WebAssembly, enabling quantum-resistant distributed machine learning.

#### **Method 1: Using Pyodide (Recommended for AI/ML)**

Install Pyodide for Python-in-WASM:

```bash
# Install Node.js and npm if not already installed
npm install -g pyodide-cli

# Create AI project structure
mkdir -p contracts/ai/
mkdir -p models/
mkdir -p data/
```

#### **Method 2: Using Emscripten + Python**

```bash
# Install Emscripten
git clone https://github.com/emscripten-core/emsdk.git
cd emsdk
./emsdk install latest
./emsdk activate latest
source ./emsdk_env.sh
```

### **Example 1: Simple ML Model Inference**

Create `contracts/ai/ml_classifier.py`:

```python
"""
Simple ML classifier for SpaceKit Network
Quantum-resistant distributed machine learning
"""
import math
import json

class SimpleNeuralNetwork:
    def __init__(self, weights_hidden, weights_output, bias_hidden, bias_output):
        self.weights_hidden = weights_hidden
        self.weights_output = weights_output
        self.bias_hidden = bias_hidden
        self.bias_output = bias_output
    
    def sigmoid(self, x):
        return 1 / (1 + math.exp(-x))
    
    def predict(self, inputs):
        # Hidden layer
        hidden = []
        for i in range(len(self.weights_hidden[0])):
            h = self.bias_hidden[i]
            for j in range(len(inputs)):
                h += inputs[j] * self.weights_hidden[j][i]
            hidden.append(self.sigmoid(h))
        
        # Output layer
        output = []
        for i in range(len(self.weights_output[0])):
            o = self.bias_output[i]
            for j in range(len(hidden)):
                o += hidden[j] * self.weights_output[j][i]
            output.append(self.sigmoid(o))
        
        return output

# Pre-trained model weights (simplified example)
MODEL_WEIGHTS = {
    "hidden": [[0.5, -0.3], [0.2, 0.8], [-0.1, 0.4]],
    "output": [[0.9], [-0.7]],
    "bias_hidden": [0.1, -0.05],
    "bias_output": [0.3]
}

def classify_data(data_json):
    """Main classification function"""
    try:
        data = json.loads(data_json)
        inputs = data.get("features", [])
        
        if len(inputs) != 3:
            return json.dumps({"error": "Expected 3 input features"})
        
        # Create and run model
        model = SimpleNeuralNetwork(
            MODEL_WEIGHTS["hidden"],
            MODEL_WEIGHTS["output"], 
            MODEL_WEIGHTS["bias_hidden"],
            MODEL_WEIGHTS["bias_output"]
        )
        
        prediction = model.predict(inputs)
        confidence = max(prediction)
        predicted_class = prediction.index(confidence)
        
        result = {
            "prediction": predicted_class,
            "confidence": round(confidence, 4),
            "raw_output": [round(x, 4) for x in prediction]
        }
        
        return json.dumps(result)
        
    except Exception as e:
        return json.dumps({"error": str(e)})

def batch_classify(batch_json):
    """Batch classification for multiple samples"""
    try:
        batch = json.loads(batch_json)
        results = []
        
        for sample in batch.get("samples", []):
            sample_json = json.dumps({"features": sample})
            result = json.loads(classify_data(sample_json))
            results.append(result)
        
        return json.dumps({
            "batch_size": len(results),
            "results": results,
            "success_count": len([r for r in results if "error" not in r])
        })
        
    except Exception as e:
        return json.dumps({"error": str(e)})

# WASM entry point
def main():
    # Test with sample data
    test_data = '{"features": [0.5, 0.3, 0.8]}'
    return classify_data(test_data)

if __name__ == "__main__":
    print(main())
```

### **Example 2: Computer Vision Processing**

Create `contracts/ai/image_processor.py`:

```python
"""
Image processing contract for SpaceKit Network
Quantum-resistant distributed computer vision
"""
import json
import math

class ImageProcessor:
    @staticmethod
    def apply_filter(image_data, filter_type="blur"):
        """Apply simple image filters"""
        width = image_data["width"]
        height = image_data["height"] 
        pixels = image_data["pixels"]
        
        if filter_type == "blur":
            return ImageProcessor.blur_filter(pixels, width, height)
        elif filter_type == "edge":
            return ImageProcessor.edge_detection(pixels, width, height)
        elif filter_type == "brightness":
            brightness = image_data.get("brightness", 1.2)
            return ImageProcessor.adjust_brightness(pixels, brightness)
        else:
            return pixels
    
    @staticmethod
    def blur_filter(pixels, width, height):
        """Simple 3x3 blur filter"""
        blurred = [0] * len(pixels)
        
        for y in range(1, height - 1):
            for x in range(1, width - 1):
                idx = y * width + x
                total = 0
                count = 0
                
                # 3x3 kernel
                for dy in [-1, 0, 1]:
                    for dx in [-1, 0, 1]:
                        neighbor_idx = (y + dy) * width + (x + dx)
                        if 0 <= neighbor_idx < len(pixels):
                            total += pixels[neighbor_idx]
                            count += 1
                
                blurred[idx] = total // count if count > 0 else pixels[idx]
        
        return blurred
    
    @staticmethod
    def edge_detection(pixels, width, height):
        """Simple edge detection using gradient"""
        edges = [0] * len(pixels)
        
        for y in range(1, height - 1):
            for x in range(1, width - 1):
                idx = y * width + x
                
                # Sobel operator approximation
                gx = (pixels[idx + 1] - pixels[idx - 1]) / 2
                gy = (pixels[idx + width] - pixels[idx - width]) / 2
                
                gradient = math.sqrt(gx*gx + gy*gy)
                edges[idx] = min(255, int(gradient))
        
        return edges
    
    @staticmethod
    def adjust_brightness(pixels, factor):
        """Adjust image brightness"""
        return [min(255, max(0, int(p * factor))) for p in pixels]

def process_image(image_json):
    """Main image processing function"""
    try:
        data = json.loads(image_json)
        
        required_fields = ["width", "height", "pixels"]
        for field in required_fields:
            if field not in data:
                return json.dumps({"error": f"Missing required field: {field}"})
        
        filter_type = data.get("filter", "blur")
        processed_pixels = ImageProcessor.apply_filter(data, filter_type)
        
        result = {
            "width": data["width"],
            "height": data["height"],
            "pixels": processed_pixels,
            "filter_applied": filter_type,
            "processed": True
        }
        
        return json.dumps(result)
        
    except Exception as e:
        return json.dumps({"error": str(e)})

def analyze_image_stats(image_json):
    """Analyze image statistics"""
    try:
        data = json.loads(image_json)
        pixels = data["pixels"]
        
        if not pixels:
            return json.dumps({"error": "No pixel data"})
        
        # Calculate statistics
        pixel_sum = sum(pixels)
        pixel_count = len(pixels)
        mean = pixel_sum / pixel_count
        
        variance = sum((p - mean) ** 2 for p in pixels) / pixel_count
        std_dev = math.sqrt(variance)
        
        pixel_min = min(pixels)
        pixel_max = max(pixels)
        
        # Histogram (simplified)
        histogram = [0] * 256
        for pixel in pixels:
            if 0 <= pixel <= 255:
                histogram[pixel] += 1
        
        result = {
            "mean": round(mean, 2),
            "std_dev": round(std_dev, 2),
            "min": pixel_min,
            "max": pixel_max,
            "total_pixels": pixel_count,
            "histogram": histogram[:10]  # First 10 bins only
        }
        
        return json.dumps(result)
        
    except Exception as e:
        return json.dumps({"error": str(e)})

def main():
    # Test with sample 4x4 image
    test_image = {
        "width": 4,
        "height": 4,
        "pixels": [100, 150, 200, 120, 
                  110, 160, 210, 130,
                  120, 170, 220, 140,
                  130, 180, 230, 150],
        "filter": "blur"
    }
    return process_image(json.dumps(test_image))

if __name__ == "__main__":
    print(main())
```

### **Example 3: Natural Language Processing**

Create `contracts/ai/text_processor.py`:

```python
"""
Text processing and sentiment analysis for SpaceKit Network
Quantum-resistant distributed NLP
"""
import json
import re

class TextProcessor:
    # Simple sentiment words (in production, use proper ML models)
    POSITIVE_WORDS = ["good", "great", "excellent", "amazing", "wonderful", 
                     "fantastic", "awesome", "brilliant", "outstanding", "perfect"]
    NEGATIVE_WORDS = ["bad", "terrible", "awful", "horrible", "worst", 
                     "disappointing", "poor", "hate", "disgusting", "useless"]
    
    @staticmethod
    def clean_text(text):
        """Clean and normalize text"""
        # Convert to lowercase
        text = text.lower()
        # Remove special characters but keep spaces
        text = re.sub(r'[^a-zA-Z0-9\s]', '', text)
        # Remove extra whitespace
        text = ' '.join(text.split())
        return text
    
    @staticmethod
    def tokenize(text):
        """Simple tokenization"""
        return TextProcessor.clean_text(text).split()
    
    @staticmethod
    def sentiment_score(text):
        """Calculate simple sentiment score"""
        tokens = TextProcessor.tokenize(text)
        positive_count = sum(1 for token in tokens if token in TextProcessor.POSITIVE_WORDS)
        negative_count = sum(1 for token in tokens if token in TextProcessor.NEGATIVE_WORDS)
        
        if positive_count + negative_count == 0:
            return 0.0  # Neutral
        
        return (positive_count - negative_count) / (positive_count + negative_count)
    
    @staticmethod
    def extract_keywords(text, max_keywords=10):
        """Extract simple keywords (most frequent words)"""
        tokens = TextProcessor.tokenize(text)
        
        # Remove common stop words
        stop_words = {"the", "a", "an", "and", "or", "but", "in", "on", "at", 
                     "to", "for", "of", "with", "by", "is", "are", "was", "were"}
        
        filtered_tokens = [token for token in tokens if token not in stop_words and len(token) > 2]
        
        # Count frequency
        word_freq = {}
        for token in filtered_tokens:
            word_freq[token] = word_freq.get(token, 0) + 1
        
        # Sort by frequency
        keywords = sorted(word_freq.items(), key=lambda x: x[1], reverse=True)
        return keywords[:max_keywords]

def analyze_text(text_json):
    """Main text analysis function"""
    try:
        data = json.loads(text_json)
        text = data.get("text", "")
        
        if not text.strip():
            return json.dumps({"error": "No text provided"})
        
        # Perform analysis
        sentiment = TextProcessor.sentiment_score(text)
        keywords = TextProcessor.extract_keywords(text)
        tokens = TextProcessor.tokenize(text)
        
        # Classify sentiment
        if sentiment > 0.1:
            sentiment_label = "positive"
        elif sentiment < -0.1:
            sentiment_label = "negative" 
        else:
            sentiment_label = "neutral"
        
        result = {
            "sentiment_score": round(sentiment, 3),
            "sentiment_label": sentiment_label,
            "keywords": keywords,
            "word_count": len(tokens),
            "character_count": len(text),
            "processed": True
        }
        
        return json.dumps(result)
        
    except Exception as e:
        return json.dumps({"error": str(e)})

def batch_analyze_texts(batch_json):
    """Analyze multiple texts in batch"""
    try:
        data = json.loads(batch_json)
        texts = data.get("texts", [])
        
        results = []
        for i, text in enumerate(texts):
            text_data = json.dumps({"text": text})
            result = json.loads(analyze_text(text_data))
            result["text_id"] = i
            results.append(result)
        
        # Calculate batch statistics
        sentiments = [r.get("sentiment_score", 0) for r in results if "error" not in r]
        avg_sentiment = sum(sentiments) / len(sentiments) if sentiments else 0
        
        batch_result = {
            "batch_size": len(texts),
            "results": results,
            "average_sentiment": round(avg_sentiment, 3),
            "success_count": len([r for r in results if "error" not in r])
        }
        
        return json.dumps(batch_result)
        
    except Exception as e:
        return json.dumps({"error": str(e)})

def main():
    # Test with sample text
    test_data = {
        "text": "This is an amazing product! I love how great it works. Excellent quality and fantastic performance."
    }
    return analyze_text(json.dumps(test_data))

if __name__ == "__main__":
    print(main())
```

### **Compiling Python to WASM**

Create `scripts/build_ai_contracts.sh`:

```bash
#!/bin/bash
set -e

echo "🤖 Building SpaceKit AI/ML WASM Contracts..."

# Create build directories
mkdir -p build/ai/
mkdir -p build/models/

# Get current DID
DID=$(grep 'did =' ~/.spacekit/config.toml | cut -d'"' -f2)
echo "📋 Building AI contracts for DID: $DID"

# Method 1: Using Pyodide (recommended for Python AI)
if command -v pyodide &> /dev/null; then
    echo "🐍 Compiling Python AI contracts with Pyodide..."
    
    # ML Classifier
    pyodide build-wasm \
        --input contracts/ai/ml_classifier.py \
        --output build/ai/ml_classifier.wasm \
        --optimize
    
    # Image Processor  
    pyodide build-wasm \
        --input contracts/ai/image_processor.py \
        --output build/ai/image_processor.wasm \
        --optimize
    
    # Text Processor
    pyodide build-wasm \
        --input contracts/ai/text_processor.py \
        --output build/ai/text_processor.wasm \
        --optimize
        
else
    echo "⚠️  Pyodide not found. Using alternative compilation..."
    
    # Method 2: Using Emscripten + Python
    if [ -f "emsdk/emsdk_env.sh" ]; then
        source emsdk/emsdk_env.sh
        
        echo "🔧 Compiling with Emscripten..."
        
        # Create C wrapper for Python (simplified)
        cat > build/ai/python_wrapper.c << 'EOF'
#include <Python.h>
#include <emscripten.h>

EMSCRIPTEN_KEEPALIVE
char* run_python_code(const char* code) {
    Py_Initialize();
    PyObject* result = PyRun_String(code, Py_eval_input, PyImport_GetModuleDict(), NULL);
    
    if (result) {
        PyObject* str = PyObject_Str(result);
        char* output = PyUnicode_AsUTF8(str);
        Py_DECREF(str);
        Py_DECREF(result);
        Py_Finalize();
        return output;
    }
    
    Py_Finalize();
    return "Error executing Python code";
}
EOF
        
        # Compile with Emscripten (this is a simplified example)
        emcc build/ai/python_wrapper.c -o build/ai/python_ai.wasm \
            -s WASM=1 -s EXPORTED_FUNCTIONS='["_run_python_code"]' \
            -s EXTRA_EXPORTED_RUNTIME_METHODS='["ccall", "cwrap"]'
            
    else
        echo "❌ Neither Pyodide nor Emscripten found!"
        echo "📚 Install with:"
        echo "   npm install -g pyodide-cli"
        echo "   OR"
        echo "   git clone https://github.com/emscripten-core/emsdk.git && cd emsdk && ./emsdk install latest"
        exit 1
    fi
fi

# Verify builds
echo "✅ AI/ML Contract Build Summary:"
for wasm in build/ai/*.wasm; do
    if [ -f "$wasm" ]; then
        size=$(wc -c < "$wasm")
        echo "   $(basename "$wasm"): ${size} bytes"
    fi
done

echo "🤖 AI contracts ready for deployment!"
echo "🚀 Deploy with: spacekit task submit --file build/ai/CONTRACT.wasm --runtime wasm"
```

### **Deploying AI Contracts**

```bash
# Build AI contracts
chmod +x scripts/build_ai_contracts.sh
./scripts/build_ai_contracts.sh

# Get DID
DID=$(grep 'did =' ~/.spacekit/config.toml | cut -d'"' -f2)

# Deploy ML classifier
spacekit task submit \
  --file build/ai/ml_classifier.wasm \
  --runtime wasm \
  --owner-did "$DID" \
  --input data/sample_features.json \
  --encryption kyber768 \
  --max-cost 0.01

# Deploy image processor
spacekit task submit \
  --file build/ai/image_processor.wasm \
  --runtime wasm \
  --owner-did "$DID" \
  --input data/sample_image.json \
  --encryption kyber768 \
  --max-cost 0.02

# Deploy text processor for sentiment analysis
spacekit task submit \
  --file build/ai/text_processor.wasm \
  --runtime wasm \
  --owner-did "$DID" \
  --input data/sample_text.json \
  --encryption kyber768 \
  --max-cost 0.015
```

### **Creating Test Data**

Create sample input files:

```bash
# Sample ML features
cat > data/sample_features.json << 'EOF'
{
  "features": [0.6, 0.4, 0.8],
  "model_version": "1.0"
}
EOF

# Sample image data (4x4 grayscale)
cat > data/sample_image.json << 'EOF'
{
  "width": 4,
  "height": 4,
  "pixels": [100, 150, 200, 120, 110, 160, 210, 130, 120, 170, 220, 140, 130, 180, 230, 150],
  "filter": "edge"
}
EOF

# Sample text for analysis
cat > data/sample_text.json << 'EOF'
{
  "text": "I absolutely love this quantum-resistant distributed computing platform! The AI capabilities are fantastic and the security is outstanding. This is truly excellent technology."
}
EOF
```

### **AI Contract Monitoring**

Create `scripts/monitor_ai_contracts.sh`:

```bash
#!/bin/bash

echo "🤖 SpaceKit AI/ML Contract Monitoring"
echo "==================================="

# Load AI task IDs (you'll need to save these from deployment)
if [ -f ".ai_task_ids" ]; then
    source .ai_task_ids
else
    echo "❌ No AI task IDs found. Deploy AI contracts first."
    exit 1
fi

# Monitor AI tasks
monitor_ai_task() {
    local task_id=$1
    local name=$2
    
    echo "🔍 $name ($task_id):"
    status=$(spacekit task status "$task_id" | grep "Status:" | cut -d' ' -f2)
    
    case $status in
        "Completed")
            echo "   ✅ Status: Completed"
            # Get results for AI analysis
            spacekit task result "$task_id" --output "results/${name}_result.json"
            echo "   📊 Results saved to results/${name}_result.json"
            ;;
        "Running")
            echo "   🔄 Status: Processing AI/ML workload..."
            ;;
        "Failed")
            echo "   ❌ Status: Failed"
            ;;
        *)
            echo "   ⏳ Status: $status"
            ;;
    esac
    echo ""
}

# Create results directory
mkdir -p results/

while true; do
    clear
    echo "🤖 SpaceKit AI/ML Contract Dashboard - $(date)"
    echo "=============================================="
    echo ""
    
    monitor_ai_task "$ML_CLASSIFIER_TASK" "ML_Classifier"
    monitor_ai_task "$IMAGE_PROCESSOR_TASK" "Image_Processor" 
    monitor_ai_task "$TEXT_PROCESSOR_TASK" "Text_Processor"
    
    echo "🔄 Refreshing in 15 seconds... (Ctrl+C to exit)"
    sleep 15
done
```

### **AI Performance Considerations**

**Optimization Tips:**
- Use lightweight models for WASM deployment
- Implement model quantization for smaller sizes
- Consider splitting large models across multiple contracts
- Use efficient data serialization (JSON, MessagePack)
- Implement caching for frequently used models

**Memory Management:**
- Python WASM has memory limitations
- Optimize data structures for WASM constraints
- Use streaming processing for large datasets
- Implement garbage collection strategies

## 🎯 **Production Deployment Checklist**

### **Pre-Deployment Checklist**

- [ ] ✅ **Code Review**: Peer review of contract logic
- [ ] 🧪 **Testing**: Comprehensive unit and integration tests
- [ ] 🔒 **Security Audit**: Input validation and bounds checking
- [ ] 📦 **Optimization**: Size and performance optimization
- [ ] 🔐 **Encryption**: Appropriate quantum-resistant algorithms
- [ ] 💰 **Cost Estimation**: Gas/cost analysis
- [ ] 📚 **Documentation**: Complete API documentation
- [ ] 🔄 **Backup**: Contract source code backup

### **Deployment Process**

```bash
# 1. Final build and optimization
./scripts/build_contracts.sh

# 2. Run tests
cargo test
./scripts/test_deployment.sh

# 3. Deploy to testnet first
export SpaceKit_NETWORK=testnet
./scripts/deploy_all.sh

# 4. Verify testnet deployment
./scripts/monitor_contracts.sh

# 5. Deploy to mainnet
export SpaceKit_NETWORK=mainnet
./scripts/deploy_all.sh
```

## 📚 **Additional Resources**

### **Documentation Links**
- **🌐 SpaceKit Network**: [https://spacekit.xyz](https://spacekit.xyz)
- **📖 SpaceKit SDK Documentation**: [https://docs.spacekit.xyz/sdk](https://docs.spacekit.xyz/sdk)
- **🔧 WebAssembly Guide**: [https://webassembly.org](https://webassembly.org)
- **🦀 Rust WASM Book**: [https://rustwasm.github.io/docs/book/](https://rustwasm.github.io/docs/book/)

### **Community Support**
- **💬 Discord**: [SpaceKit Network Discord](https://discord.gg/swtch)
- **📱 Telegram**: [SpaceKit Developers](https://t.me/swtch_devs)
- **🐛 GitHub Issues**: [Report bugs and feature requests](https://github.com/swtchlabs/swtch-network)

### **Example Repositories**
- **🎯 SpaceKit WASM Examples**: [https://github.com/swtchlabs/wasm-examples](https://github.com/swtchlabs/wasm-examples)
- **🔧 Advanced Contracts**: [https://github.com/swtchlabs/advanced-wasm](https://github.com/swtchlabs/advanced-wasm)

## 🏆 **Success Stories**

> *"We deployed our quantum-resistant supply chain tracking system using SpaceKit WASM contracts. The performance is incredible, and knowing it's protected against quantum attacks gives us confidence for the future."*
> 
> **— Enterprise Customer**

> *"The SpaceKit CLI made deploying our DeFi protocol seamless. From development to production in hours, not days."*
> 
> **— DeFi Protocol Developer**

---

**🚀 Ready to build the future with quantum-resistant smart contracts? Start with `spacekit init` and deploy your first WASM contract today!**

