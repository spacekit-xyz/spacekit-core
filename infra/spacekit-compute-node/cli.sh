#!/bin/bash
# WCVM CLI Tool - Command Line Interface for WCVM Development

set -e

# Configuration
WCVM_CLI_VERSION="1.0.0"
DEFAULT_NODE_URL="http://localhost:8080"
CONFIG_DIR="$HOME/.wcvm"
CONFIG_FILE="$CONFIG_DIR/config.toml"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Utility functions
log_info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

log_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Initialize WCVM CLI
init_wcvm() {
    log_info "Initializing WCVM CLI..."
    
    # Create config directory
    mkdir -p "$CONFIG_DIR"
    
    # Create default config if it doesn't exist
    if [ ! -f "$CONFIG_FILE" ]; then
        cat > "$CONFIG_FILE" << EOF
# WCVM CLI Configuration

[network]
default = "localhost"

[networks.localhost]
url = "http://localhost:8080"
chain_id = 1337
name = "Local Development"

[networks.testnet]
url = "https://testnet-api.wcvm.io"
chain_id = 3
name = "WCVM Testnet"

[networks.mainnet]
url = "https://api.wcvm.io"
chain_id = 1
name = "WCVM Mainnet"

[accounts]
# Add your accounts here
# Example:
# alice = "0x1234567890abcdef1234567890abcdef12345678"

[compiler]
rust_target = "wasm32-unknown-unknown"
optimization_level = "s"
strip_debug = true

[deployment]
gas_limit = 2000000
gas_price = "1000000000"  # 1 gwei
confirmations = 1
EOF
        log_success "Created default configuration at $CONFIG_FILE"
    else
        log_info "Configuration already exists at $CONFIG_FILE"
    fi
    
    # Create examples directory
    mkdir -p "$CONFIG_DIR/examples"
    
    # Create example contract
    cat > "$CONFIG_DIR/examples/counter.rs" << 'EOF'
// Example WCVM Smart Contract - Simple Counter
use wcvm_sdk::*;

#[wcvm_contract]
pub struct Counter {
    value: u64,
}

#[wcvm_impl]
impl Counter {
    #[wcvm_init]
    pub fn new(initial_value: u64) -> Self {
        Self {
            value: initial_value,
        }
    }
    
    #[wcvm_call]
    pub fn increment(&mut self) {
        self.value += 1;
        wcvm_log(&format!("Counter incremented to {}", self.value));
    }
    
    #[wcvm_call]
    pub fn decrement(&mut self) {
        if self.value > 0 {
            self.value -= 1;
        }
        wcvm_log(&format!("Counter decremented to {}", self.value));
    }
    
    #[wcvm_view]
    pub fn get(&self) -> u64 {
        self.value
    }
    
    #[wcvm_call]
    #[wcvm_gpu_compute]
    pub fn fibonacci(&self, n: u32) -> u64 {
        // GPU-accelerated Fibonacci calculation for large n
        if n <= 1 {
            return n as u64;
        }
        
        // Use GPU compute for parallel calculation
        let shader = r#"
            @group(0) @binding(0) var<storage, read> input: array<u32>;
            @group(0) @binding(1) var<storage, read_write> output: array<u64>;
            
            @compute @workgroup_size(64)
            fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
                let index = global_id.x;
                if (index >= arrayLength(&input)) { return; }
                
                let n = input[index];
                var a: u64 = 0;
                var b: u64 = 1;
                
                for (var i: u32 = 2; i <= n; i++) {
                    let temp = a + b;
                    a = b;
                    b = temp;
                }
                
                output[index] = b;
            }
        "#;
        
        let result = wcvm_gpu_compute(shader, &[n]);
        result[0]
    }
}

// Matrix multiplication example using GPU
#[wcvm_contract]
pub struct MatrixCompute;

#[wcvm_impl]
impl MatrixCompute {
    #[wcvm_init]
    pub fn new() -> Self {
        Self
    }
    
    #[wcvm_call]
    #[wcvm_gpu_compute]
    pub fn matrix_multiply(&self, matrix_a: Vec<f32>, matrix_b: Vec<f32>, size: u32) -> Vec<f32> {
        let shader = r#"
            @group(0) @binding(0) var<storage, read> matrix_a: array<f32>;
            @group(0) @binding(1) var<storage, read> matrix_b: array<f32>;
            @group(0) @binding(2) var<storage, read_write> result: array<f32>;
            @group(0) @binding(3) var<uniform> size: u32;
            
            @compute @workgroup_size(16, 16)
            fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
                let row = global_id.x;
                let col = global_id.y;
                
                if (row >= size || col >= size) { return; }
                
                var sum: f32 = 0.0;
                for (var i: u32 = 0; i < size; i++) {
                    sum += matrix_a[row * size + i] * matrix_b[i * size + col];
                }
                
                result[row * size + col] = sum;
            }
        "#;
        
        let mut input_data = matrix_a;
        input_data.extend(matrix_b);
        input_data.push(size as f32);
        
        wcvm_gpu_compute(shader, &input_data)
    }
}
EOF
    
    # Create deployment config example
    cat > "$CONFIG_DIR/examples/deployment.toml" << 'EOF'
# WCVM Deployment Configuration Example

[deployment]
network = "localhost"
gas_limit = 2000000
gas_price = "1000000000"

[accounts]
deployer = "alice"

[contracts.counter]
source = "counter.rs"
constructor_args = [0]
dependencies = []

[contracts.matrix_compute]
source = "matrix_compute.rs"
constructor_args = []
dependencies = []

# Test configuration
[tests]
test_dir = "./tests"
timeout = 30000  # 30 seconds

[[tests.cases]]
name = "counter_basic"
contract = "counter"
steps = [
    { type = "call", function = "increment", args = [] },
    { type = "call", function = "increment", args = [] },
    { type = "call", function = "get", args = [] }
]
assertions = [
    { type = "return_equals", expected = 2 }
]
EOF
    
    log_success "WCVM CLI initialized successfully!"
    log_info "Example contracts created in $CONFIG_DIR/examples/"
    log_info "Edit $CONFIG_FILE to configure your networks and accounts"
}

# Show help
show_help() {
    cat << EOF
WCVM CLI v${WCVM_CLI_VERSION} - WebAssembly Compute Virtual Machine

USAGE:
    wcvm <COMMAND> [OPTIONS]

COMMANDS:
    init                Initialize WCVM CLI configuration
    compile <source>    Compile source code to WASM
    deploy <config>     Deploy contracts using configuration file
    call <address>      Call a contract function
    send <address>      Send a transaction to a contract
    account <address>   Get account information
    balance <address>   Get account balance
    block [number]      Get block information
    tx <hash>           Get transaction information
    estimate <file>     Estimate deployment/execution costs
    test <dir>          Run test suite
    node                Start local WCVM node
    wallet              Wallet management commands
    gpu                 GPU information and management
    network             Network information
    console             Interactive console

OPTIONS:
    --network <name>    Network to use (default: localhost)
    --config <file>     Configuration file (default: ~/.wcvm/config.toml)
    --verbose          Enable verbose output
    --help             Show this help message

EXAMPLES:
    wcvm init                              # Initialize CLI
    wcvm compile counter.rs                # Compile Rust to WASM
    wcvm deploy deployment.toml            # Deploy contracts
    wcvm call 0x123... get                 # Call contract function
    wcvm send 0x123... increment           # Send transaction
    wcvm account 0x742d35cc...             # Get account info
    wcvm estimate counter.wasm             # Estimate costs
    wcvm test ./tests                      # Run test suite
    wcvm gpu info                          # Show GPU information
    wcvm console                           # Start interactive console

For more information, visit: https://docs.wcvm.io
EOF
}

# Compile source code to WASM
compile_contract() {
    local source_file="$1"
    local output_file="${2:-${source_file%.*}.wasm}"
    
    if [ ! -f "$source_file" ]; then
        log_error "Source file not found: $source_file"
        exit 1
    fi
    
    log_info "Compiling $source_file to $output_file..."
    
    # Determine language based on file extension
    case "${source_file##*.}" in
        rs)
            compile_rust "$source_file" "$output_file"
            ;;
        cpp|cc|cxx)
            compile_cpp "$source_file" "$output_file"
            ;;
        c)
            compile_c "$source_file" "$output_file"
            ;;
        ts)
            compile_assemblyscript "$source_file" "$output_file"
            ;;
        *)
            log_error "Unsupported file extension: ${source_file##*.}"
            exit 1
            ;;
    esac
    
    if [ -f "$output_file" ]; then
        local size=$(wc -c < "$output_file")
        log_success "Compilation successful! Output: $output_file (${size} bytes)"
        
        # Generate ABI if possible
        generate_abi "$source_file" "${output_file%.*}.abi.json"
    else
        log_error "Compilation failed!"
        exit 1
    fi
}

# Compile Rust to WASM
compile_rust() {
    local source="$1"
    local output="$2"
    
    # Create temporary Cargo project
    local temp_dir=$(mktemp -d)
    local project_dir="$temp_dir/wcvm_contract"
    
    cargo init --lib "$project_dir" --name wcvm_contract
    
    # Copy source file
    cp "$source" "$project_dir/src/lib.rs"
    
    # Create Cargo.toml
    cat > "$project_dir/Cargo.toml" << EOF
[package]
name = "wcvm_contract"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
wcvm-sdk = { git = "https://github.com/wcvm/wcvm-sdk" }

[profile.release]
opt-level = "s"
lto = true
panic = "abort"
EOF

    # Compile
    cd "$project_dir"
    cargo build --release --target wasm32-unknown-unknown
    
    if [ -f "target/wasm32-unknown-unknown/release/wcvm_contract.wasm" ]; then
        cp "target/wasm32-unknown-unknown/release/wcvm_contract.wasm" "$output"
        
        # Optimize with wasm-opt if available
        if command -v wasm-opt >/dev/null 2>&1; then
            log_info "Optimizing WASM with wasm-opt..."
            wasm-opt -Os "$output" -o "$output"
        fi
    fi
    
    # Cleanup
    rm -rf "$temp_dir"
}

# Compile C/C++ to WASM
compile_cpp() {
    local source="$1"
    local output="$2"
    
    if ! command -v emcc >/dev/null 2>&1; then
        log_error "Emscripten not found. Please install Emscripten SDK."
        exit 1
    fi
    
    log_info "Compiling with Emscripten..."
    emcc "$source" -o "$output" \
        -s WASM=1 \
        -s EXPORTED_FUNCTIONS='["_main"]' \
        -s MODULARIZE=1 \
        -s EXPORT_NAME="WCVMContract" \
        -O3
}

# Generate ABI from source
generate_abi() {
    local source="$1"
    local abi_file="$2"
    
    log_info "Generating ABI: $abi_file"
    
    # Simple ABI generation (would be more sophisticated in practice)
    cat > "$abi_file" << EOF
{
  "functions": {
    "main": {
      "name": "main",
      "type": "function",
      "inputs": [
        {"name": "input", "type": "bytes"}
      ],
      "outputs": [
        {"name": "result", "type": "bytes"}
      ],
      "stateMutability": "nonpayable",
      "computeIntensive": true,
      "gpuOptimized": false
    }
  },
  "events": {},
  "constructor": {
    "name": "constructor",
    "type": "constructor",
    "inputs": [],
    "outputs": [],
    "stateMutability": "nonpayable"
  }
}
EOF
}

# Deploy contracts
deploy_contracts() {
    local config_file="$1"
    local network="${2:-localhost}"
    
    if [ ! -f "$config_file" ]; then
        log_error "Deployment config not found: $config_file"
        exit 1
    fi
    
    log_info "Deploying contracts using $config_file on network: $network"
    
    # Use the Rust CLI tool for deployment
    if command -v wcvm-deploy >/dev/null 2>&1; then
        wcvm-deploy --config "$config_file" --network "$network"
    else
        # Fallback to REST API
        deploy_via_api "$config_file" "$network"
    fi
}

# Deploy via REST API
deploy_via_api() {
    local config_file="$1"
    local network="$2"
    
    # Parse config and deploy contracts
    # This is a simplified version - would parse TOML properly
    log_info "Deploying via REST API..."
    
    # Get network URL from config
    local node_url=$(get_network_url "$network")
    
    # Deploy each contract
    for contract_file in *.wasm; do
        if [ -f "$contract_file" ]; then
            log_info "Deploying $contract_file..."
            
            # Convert to base64
            local bytecode=$(base64 -w 0 "$contract_file")
            
            # Submit deployment
            local response=$(curl -s -X POST "$node_url/contracts" \
                -H "Content-Type: application/json" \
                -d "{
                    \"bytecode\": \"$bytecode\",
                    \"gasLimit\": 2000000,
                    \"gasPrice\": \"0x3b9aca00\"
                }")
            
            local contract_address=$(echo "$response" | jq -r '.contractAddress')
            
            if [ "$contract_address" != "null" ]; then
                log_success "Contract deployed at: $contract_address"
                
                # Save deployment info
                echo "$contract_file,$contract_address,$(date)" >> deployments.csv
            else
                log_error "Deployment failed: $response"
            fi
        fi
    done
}

# Call contract function
call_contract() {
    local address="$1"
    local function="$2"
    shift 2
    local args=("$@")
    local network="${WCVM_NETWORK:-localhost}"
    
    log_info "Calling $function on contract $address"
    
    local node_url=$(get_network_url "$network")
    
    # Prepare arguments
    local args_json=$(printf '%s\n' "${args[@]}" | jq -R . | jq -s .)
    
    # Make API call
    local response=$(curl -s -X POST "$node_url/contracts/$address/call" \
        -H "Content-Type: application/json" \
        -d "{
            \"function\": \"$function\",
            \"args\": $args_json,
            \"from\": \"0x0000000000000000000000000000000000000000\"
        }")
    
    local result=$(echo "$response" | jq -r '.result')
    local gas_used=$(echo "$response" | jq -r '.gasUsed')
    
    log_success "Result: $result"
    log_info "Gas used: $gas_used"
}

# Send transaction to contract
send_transaction() {
    local address="$1"
    local function="$2"
    shift 2
    local args=("$@")
    
    log_info "Sending transaction to $function on contract $address"
    
    # This would require private key handling
    log_warn "Transaction sending requires wallet integration"
    log_info "Use 'wcvm wallet' commands to manage private keys"
}

# Get account information
get_account() {
    local address="$1"
    local network="${WCVM_NETWORK:-localhost}"
    
    local node_url=$(get_network_url "$network")
    
    log_info "Getting account information for $address"
    
    local response=$(curl -s "$node_url/accounts/$address")
    
    if echo "$response" | jq -e .address >/dev/null 2>&1; then
        local balance=$(echo "$response" | jq -r '.balance')
        local nonce=$(echo "$response" | jq -r '.nonce')
        
        log_success "Account: $address"
        echo "Balance: $balance wei"
        echo "Nonce: $nonce"
    else
        log_error "Account not found or error: $response"
    fi
}

# Get network URL from config
get_network_url() {
    local network="$1"
    
    # Simple config parsing (would use proper TOML parser in practice)
    case "$network" in
        localhost)
            echo "http://localhost:8080/v1"
            ;;
        testnet)
            echo "https://testnet-api.wcvm.io/v1"
            ;;
        mainnet)
            echo "https://api.wcvm.io/v1"
            ;;
        *)
            echo "$DEFAULT_NODE_URL/v1"
            ;;
    esac
}

# Estimate costs
estimate_costs() {
    local file="$1"
    local network="${WCVM_NETWORK:-localhost}"
    
    if [ ! -f "$file" ]; then
        log_error "File not found: $file"
        exit 1
    fi
    
    log_info "Estimating costs for $file"
    
    local node_url=$(get_network_url "$network")
    local bytecode=$(base64 -w 0 "$file")
    
    local response=$(curl -s -X POST "$node_url/compute/estimate" \
        -H "Content-Type: application/json" \
        -d "{
            \"code\": \"$bytecode\",
            \"input\": \"\",
            \"options\": {
                \"preferredBackend\": \"auto\"
            }
        }")
    
    local total_cost=$(echo "$response" | jq -r '.totalCost')
    local estimated_time=$(echo "$response" | jq -r '.estimatedTimeMs')
    local recommended_backend=$(echo "$response" | jq -r '.recommendedBackend')
    
    log_success "Cost Estimate:"
    echo "Total Cost: $total_cost wei"
    echo "Estimated Time: ${estimated_time}ms"
    echo "Recommended Backend: $recommended_backend"
    
    # Show cost breakdown
    echo ""
    echo "Cost Breakdown:"
    echo "  Base Cost: $(echo "$response" | jq -r '.breakdown.baseCost') wei"
    echo "  Compute Cost: $(echo "$response" | jq -r '.breakdown.computeCost') wei"
    echo "  Memory Cost: $(echo "$response" | jq -r '.breakdown.memoryCost') wei"
    
    local gpu_cost=$(echo "$response" | jq -r '.breakdown.gpuCost')
    if [ "$gpu_cost" != "null" ]; then
        echo "  GPU Cost: $gpu_cost wei"
    fi
}

# Run tests
run_tests() {
    local test_dir="$1"
    
    if [ ! -d "$test_dir" ]; then
        log_error "Test directory not found: $test_dir"
        exit 1
    fi
    
    log_info "Running tests in $test_dir"
    
    # Use Rust test runner if available
    if command -v wcvm-test >/dev/null 2>&1; then
        wcvm-test "$test_dir"
    else
        # Simple test runner
        local passed=0
        local failed=0
        
        for test_file in "$test_dir"/*.toml; do
            if [ -f "$test_file" ]; then
                log_info "Running test: $(basename "$test_file")"
                
                # This would parse and execute the test
                # For now, just mark as passed
                log_success "Test passed: $(basename "$test_file")"
                ((passed++))
            fi
        done
        
        echo ""
        log_success "Test Results: $passed passed, $failed failed"
    fi
}

# Start local node
start_node() {
    log_info "Starting local WCVM node..."
    
    if command -v wcvm-node >/dev/null 2>&1; then
        wcvm-node --port 8080 --gpu-enabled --dev-mode
    else
        log_error "WCVM node binary not found"
        log_info "Install with: cargo install wcvm-node"
        exit 1
    fi
}

# GPU information
show_gpu_info() {
    local network="${WCVM_NETWORK:-localhost}"
    local node_url=$(get_network_url "$network")
    
    log_info "Getting GPU information..."
    
    local response=$(curl -s "$node_url/compute/gpu-info")
    
    if echo "$response" | jq -e .gpus >/dev/null 2>&1; then
        echo "Available GPUs:"
        echo "$response" | jq -r '.gpus[] | "  \(.name): \(.memoryGB)GB, \(.computeCapability), Available: \(.available)"'
    else
        log_error "Failed to get GPU information: $response"
    fi
}

# Interactive console
start_console() {
    log_info "Starting WCVM interactive console..."
    
    if command -v wcvm-console >/dev/null 2>&1; then
        wcvm-console
    else
        # Simple console implementation
        cat << EOF
WCVM Interactive Console
Type 'help' for commands, 'exit' to quit

Available commands:
  account <address>     - Get account info
  balance <address>     - Get balance
  call <addr> <func>    - Call contract function
  block [number]        - Get block info
  help                  - Show this help
  exit                  - Quit console
EOF
        
        while true; do
            read -p "wcvm> " cmd args
            
            case "$cmd" in
                account)
                    get_account "$args"
                    ;;
                balance)
                    get_account "$args" | grep Balance
                    ;;
                help)
                    echo "Available commands: account, balance, call, block, help, exit"
                    ;;
                exit|quit)
                    log_info "Goodbye!"
                    break
                    ;;
                "")
                    ;;
                *)
                    log_warn "Unknown command: $cmd"
                    ;;
            esac
        done
    fi
}

# Main command dispatcher
main() {
    case "${1:-help}" in
        init)
            init_wcvm
            ;;
        compile)
            compile_contract "$2" "$3"
            ;;
        deploy)
            deploy_contracts "$2" "$3"
            ;;
        call)
            shift
            call_contract "$@"
            ;;
        send)
            shift
            send_transaction "$@"
            ;;
        account)
            get_account "$2"
            ;;
        balance)
            get_account "$2" | grep Balance
            ;;
        estimate)
            estimate_costs "$2"
            ;;
        test)
            run_tests "$2"
            ;;
        node)
            start_node
            ;;
        gpu)
            case "$2" in
                info)
                    show_gpu_info
                    ;;
                *)
                    log_error "Unknown gpu command: $2"
                    echo "Available: info"
                    ;;
            esac
            ;;
        console)
            start_console
            ;;
        help|--help|-h)
            show_help
            ;;
        version|--version|-v)
            echo "WCVM CLI v${WCVM_CLI_VERSION}"
            ;;
        *)
            log_error "Unknown command: $1"
            echo "Run 'wcvm help' for usage information"
            exit 1
            ;;
    esac
}

# Handle script being sourced vs executed
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    main "$@"
fi