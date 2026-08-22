// WCVM Developer Usage Examples
// Complete examples showing how to deploy and interact with WCVM contracts

import { 
  WcvmHttpProvider, 
  WcvmContract, 
  WcvmContractFactory,
  WcvmUtils,
  WcvmPrivateKeyWallet 
} from '@swtch/wcvm-sdk';

// Example 1: Simple Contract Deployment and Interaction
async function example1_basicDeployment() {
  console.log('=== Example 1: Basic Contract Deployment ===');
  
  // 1. Connect to WCVM network
  const provider = new WcvmHttpProvider('https://testnet-api.wcvm.io/v1', 'your-api-key');
  
  // 2. Create wallet
  const privateKey = '0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef';
  const wallet = new WcvmPrivateKeyWallet(privateKey, provider);
  
  console.log(`Wallet address: ${wallet.address}`);
  console.log(`Balance: ${WcvmUtils.formatEther(await wallet.getBalance())} WCVM`);
  
  // 3. Contract ABI (would be generated from Rust contract)
  const counterABI = {
    functions: {
      new: {
        name: "new",
        type: "constructor",
        inputs: [{ name: "initial_value", type: "uint64" }],
        outputs: [],
        stateMutability: "nonpayable"
      },
      increment: {
        name: "increment",
        type: "function",
        inputs: [],
        outputs: [],
        stateMutability: "nonpayable",
        computeIntensive: false
      },
      get: {
        name: "get",
        type: "function",
        inputs: [],
        outputs: [{ name: "value", type: "uint64" }],
        stateMutability: "view",
        computeIntensive: false
      },
      fibonacci: {
        name: "fibonacci",
        type: "function",
        inputs: [{ name: "n", type: "uint32" }],
        outputs: [{ name: "result", type: "uint64" }],
        stateMutability: "view",
        computeIntensive: true,
        gpuOptimized: true
      }
    },
    events: {},
    constructor: {
      name: "new",
      type: "constructor",
      inputs: [{ name: "initial_value", type: "uint64" }],
      outputs: [],
      stateMutability: "nonpayable"
    }
  };
  
  // 4. Contract bytecode (would be compiled from Rust)
  const counterBytecode = '0x0061736d01000000...'; // WASM bytecode
  
  // 5. Deploy contract
  console.log('Deploying contract...');
  const factory = new WcvmContractFactory(counterABI, counterBytecode, wallet);
  
  // Estimate deployment cost first
  const deploymentCost = await factory.estimateDeploymentCost(0); // initial_value = 0
  console.log(`Deployment cost: ${WcvmUtils.formatEther(deploymentCost.totalCost)} WCVM`);
  console.log(`Estimated time: ${deploymentCost.estimatedTimeMs}ms`);
  
  // Deploy with initial value of 0
  const contract = await factory.deploy(0);
  console.log(`Contract deployed at: ${contract.address}`);
  
  // 6. Interact with contract
  console.log('Interacting with contract...');
  
  // Read current value
  const currentValue = await contract.call('get');
  console.log(`Current value: ${currentValue}`);
  
  // Increment the counter
  const incrementTx = await contract.send('increment');
  console.log(`Increment transaction: ${incrementTx.transactionHash}`);
  
  // Read updated value
  const newValue = await contract.call('get');
  console.log(`New value: ${newValue}`);
  
  return contract;
}

// Example 2: GPU-Accelerated Computing
async function example2_gpuCompute() {
  console.log('=== Example 2: GPU-Accelerated Computing ===');
  
  const provider = new WcvmHttpProvider('https://testnet-api.wcvm.io/v1');
  const wallet = new WcvmPrivateKeyWallet(process.env.PRIVATE_KEY!, provider);
  
  // Check GPU availability
  const gpuInfo = await provider.getGpuInfo();
  console.log('Available GPUs:');
  gpuInfo.forEach(gpu => {
    console.log(`  ${gpu.name}: ${gpu.memoryGB}GB, Available: ${gpu.available}`);
  });
  
  // Deploy matrix multiplication contract
  const matrixABI = {
    functions: {
      new: {
        name: "new",
        type: "constructor",
        inputs: [],
        outputs: [],
        stateMutability: "nonpayable"
      },
      matrix_multiply: {
        name: "matrix_multiply",
        type: "function",
        inputs: [
          { name: "matrix_a", type: "float32[]" },
          { name: "matrix_b", type: "float32[]" },
          { name: "size", type: "uint32" }
        ],
        outputs: [{ name: "result", type: "float32[]" }],
        stateMutability: "view",
        computeIntensive: true,
        gpuOptimized: true
      }
    },
    events: {},
    constructor: {
      name: "new",
      type: "constructor",
      inputs: [],
      outputs: [],
      stateMutability: "nonpayable"
    }
  };
  
  const matrixBytecode = '0x0061736d01000000...'; // Matrix multiplication WASM
  
  const factory = new WcvmContractFactory(matrixABI, matrixBytecode, wallet);
  const contract = await factory.deploy();
  
  console.log(`Matrix contract deployed at: ${contract.address}`);
  
  // Prepare test matrices (64x64)
  const size = 64;
  const matrixA = Array.from({ length: size * size }, (_, i) => Math.random());
  const matrixB = Array.from({ length: size * size }, (_, i) => Math.random());
  
  // Estimate compute cost
  const computeCost = await contract.estimateComputeCost('matrix_multiply', [matrixA, matrixB, size], {
    preferredBackend: 'gpu'
  });
  
  console.log(`Compute cost estimate:`);
  console.log(`  Total: ${WcvmUtils.formatEther(computeCost.totalCost)} WCVM`);
  console.log(`  GPU cost: ${WcvmUtils.formatEther(computeCost.breakdown.gpuCost || 0n)} WCVM`);
  console.log(`  Estimated time: ${computeCost.estimatedTimeMs}ms`);
  console.log(`  Recommended backend: ${computeCost.recommendedBackend}`);
  
  // Execute matrix multiplication
  console.log('Executing matrix multiplication on GPU...');
  const startTime = Date.now();
  
  const result = await contract.call('matrix_multiply', [matrixA, matrixB, size]);
  
  const executionTime = Date.now() - startTime;
  console.log(`Matrix multiplication completed in ${executionTime}ms`);
  console.log(`Result size: ${result.length} elements`);
  
  return { contract, result, executionTime };
}

// Example 3: Cost Optimization and Backend Selection
async function example3_costOptimization() {
  console.log('=== Example 3: Cost Optimization ===');
  
  const provider = new WcvmHttpProvider('https://testnet-api.wcvm.io/v1');
  
  // Test different computational workloads
  const workloads = [
    {
      name: 'Small calculation',
      code: '0x...', // Small WASM program
      input: new Uint8Array(1024), // 1KB input
    },
    {
      name: 'Medium calculation', 
      code: '0x...', // Medium WASM program
      input: new Uint8Array(1024 * 1024), // 1MB input
    },
    {
      name: 'Large calculation',
      code: '0x...', // Large WASM program
      input: new Uint8Array(1024 * 1024 * 1024), // 1GB input
    }
  ];
  
  const wallet = new WcvmPrivateKeyWallet(process.env.PRIVATE_KEY!, provider);
}