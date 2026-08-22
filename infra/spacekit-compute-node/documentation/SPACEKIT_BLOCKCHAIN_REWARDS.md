# SpaceKit Blockchain Rewards

> **Canonical economics:** [`spacekit-tokenomics`](../../../economics/spacekit-tokenomics/) — production path is **SRA → AstraRewards** ([`SERVICE_REWARD_ACCUMULATOR_SPEC.md`](../../../economics/spacekit-tokenomics/SERVICE_REWARD_ACCUMULATOR_SPEC.md)). This file covers **legacy testnet** compute reward formulas until SRA ships.

## 🎯 **Overview**

The SpaceKit Network implements a comprehensive, merit-based reward system that automatically compensates node operators for providing compute, storage, and network services. The reward system is designed to incentivize high-quality service delivery, efficiency, and long-term network participation.

## 💰 **Multi-Tier Reward Structure**

### **🏆 Base Reward System**
- **Task Completion Rewards**: Earned for every successfully completed compute task
- **Resource-Based Calculation**: Rewards scale with compute units, memory usage, and execution time
- **Runtime Multipliers**: Different reward rates for CPU, GPU, and hybrid execution paths
- **Daily Limits**: Configurable maximum daily rewards to ensure fair distribution

### **🛡️ VPoS (Verifiable Proof of Service) Enhanced Rewards**
- **Cryptographic Service Verification**: Quantum-resistant proof generation for completed tasks
- **Quality-Based Bonuses**: Additional rewards based on service quality metrics
- **Service Type Multipliers**: Higher rewards for specialized services (AI/ML, hybrid computing)
- **Anti-Fraud Protection**: Comprehensive verification system prevents reward manipulation

### **⚡ Efficiency & Performance Bonuses**
- **Resource Efficiency**: Up to 2x multiplier for optimal resource utilization
- **Execution Speed**: Bonuses for faster task completion times
- **Energy Efficiency**: Additional rewards for low energy consumption
- **Network Quality**: Bonuses for reliable network connectivity and uptime

### **🔐 Quantum Security Incentives**
- **Quantum Resistance Bonus**: 20% additional rewards for quantum-resistant operations
- **Algorithm Diversity**: Rewards for supporting multiple post-quantum algorithms
- **Security Compliance**: Bonuses for maintaining high security standards

---

## 📊 **Reward Configuration**

### **Default Token Reward Configuration**
```toml
[token_reward_config]
base_reward_per_unit = 0.001        # 0.001 ASTRA per compute unit
gpu_multiplier = 2.0                # GPU tasks get 2x reward  
hybrid_multiplier = 1.5             # Hybrid tasks get 1.5x reward
cpu_multiplier = 1.0                # CPU tasks get base reward
quantum_bonus = 1.2                 # 20% bonus for quantum encryption
max_efficiency_bonus = 2.0          # Up to 2x bonus for high efficiency
min_efficiency_penalty = 0.5        # Minimum 0.5x reward (even for inefficient tasks)
max_daily_rewards = 100.0           # 100 ASTRA max per day
enable_token_minting = true         # Enable actual token minting
```

### **VPoS Service Type Multipliers**
| Service Type | Multiplier | Description |
|--------------|------------|-------------|
| **Hybrid Computing** | 1.8x | CPU+GPU workloads, AI/ML inference |
| **AI/ML Services** | 1.5x | Machine learning, neural networks |
| **Storage Services** | 1.2x | Quantum-safe file storage, retrieval |
| **Basic Compute** | 1.0x | Standard WebAssembly execution |

### **Quality Score Multipliers**
| Performance Level | Score Range | Multiplier | Description |
|------------------|-------------|------------|-------------|
| **Excellent** | 95-100% | 2.0x | Perfect service delivery |
| **Good** | 85-94% | 1.5x | High-quality service |
| **Standard** | 75-84% | 1.0x | Acceptable service |
| **Poor** | <75% | 0.5x | Below standard service |

---

## 🧮 **Reward Calculation Formula**

### **Base Calculation**
```
Final Reward = Base Reward × Runtime Multiplier × Efficiency Bonus × Quantum Bonus × VPoS Multiplier
```

### **Detailed Breakdown**
```rust
// 1. Calculate base reward from compute units
let base_reward = compute_units_used × base_reward_per_unit;

// 2. Apply runtime multiplier
let runtime_multiplier = match task_runtime {
    "gpu" => 2.0,
    "hybrid" => 1.5,
    _ => 1.0,
};

// 3. Calculate efficiency bonus (0.5x to 2.0x)
let efficiency_bonus = calculate_efficiency_bonus(resource_metrics);

// 4. Apply quantum security bonus
let quantum_bonus = if quantum_enabled { 1.2 } else { 1.0 };

// 5. Apply VPoS service multiplier
let vpos_multiplier = get_service_type_multiplier(service_type);

// 6. Calculate final reward (converted to wei - 18 decimals)
let final_reward = (base_reward × runtime_multiplier × efficiency_bonus × quantum_bonus × vpos_multiplier) × 1e18;
```

### **Efficiency Bonus Calculation**
```rust
// Time efficiency (lower execution time = higher efficiency)
let time_efficiency = (10.0 / execution_time_seconds).min(2.0);

// Energy efficiency (lower energy = higher efficiency)  
let energy_efficiency = (0.01 / energy_consumed_kwh).min(1.5);

// Combined efficiency score
let efficiency_bonus = ((time_efficiency + energy_efficiency) / 2.0).max(0.5);
```

---

## 💸 **Reward Distribution**

### **🎯 Primary Storage: Node Status**
Rewards accumulate in the node's local status:
```rust
pub struct NodeStatus {
    pub earned_tokens: u128,           // Total ASTRA tokens earned (in wei)
    pub total_compute_units: u64,      // Total compute units processed
    pub tasks_completed: u32,          // Number of successful tasks
    pub tasks_failed: u32,             // Number of failed tasks
    // ... other metrics
}
```

### **🌐 Cross-Chain Distribution via LayerZero Bridge**
Rewards are automatically distributed across multiple blockchain networks:

| Network | Contract Address | Features |
|---------|------------------|----------|
| **Ethereum** | `0x742d35cc6cf138459f2dd82ee5b16b319b8b1234` | Primary network, highest security |
| **Avalanche** | Auto-bridged | Low fees, fast finality |
| **Arbitrum** | Auto-bridged | Layer 2, very low gas costs |
| **Polygon** | Auto-bridged | Fast transactions, DeFi integration |
| **Solana** | Auto-bridged | High throughput, low latency |
| **Cosmos** | Auto-bridged | Interchain compatibility |

### **Bridge Configuration**
```rust
pub struct LayerZeroBridgeConfig {
    pub enabled: bool,
    pub source_chain: SupportedChain,
    pub supported_chains: Vec<SupportedChain>,
    pub bridge_fee_percentage: f64,     // 0.1% bridge fee
    pub min_bridge_amount: u128,        // Minimum amount to bridge
    pub auto_bridge_threshold: u128,    // Auto-bridge when balance exceeds
}
```

---

## 🏆 **VPoS (Verifiable Proof of Service) System**

### **Proof Generation Process**
1. **Task Completion**: Compute task finishes successfully
2. **Metrics Collection**: Resource usage, execution time, quality metrics
3. **Cryptographic Proof**: Quantum-resistant signature generation
4. **Network Submission**: Proof submitted to SpaceKit network for verification
5. **Enhanced Reward**: Additional bonus calculated based on proof quality

### **VPoS Reward Enhancement**
```rust
// VPoS bonus calculation
let vpos_bonus = calculate_vpos_bonus(proof);
let enhanced_reward = vpos_reward.max(base_token_reward);

// Additional bonuses for:
// - Proof generation efficiency
// - Service timeliness  
// - Cryptographic proof strength
// - Network reputation score
```

### **Service Verification Metrics**
- **Computation Accuracy**: Result verification against expected outputs
- **Resource Efficiency**: CPU, memory, and energy utilization
- **Network Performance**: Response time, availability, throughput
- **Security Compliance**: Quantum-resistant encryption usage
- **Reputation History**: Long-term service quality track record

---

## 📈 **Reward Examples & Calculations**

### **🖥️ CPU Task Example**
```
Task Details:
- Compute Units: 100
- Runtime: CPU (1.0x multiplier)
- Execution Time: 2.5 seconds (efficiency: 85%)
- Quantum Security: Enabled (1.2x bonus)
- VPoS Quality: Good (1.5x multiplier)

Calculation:
Base Reward = 100 × 0.001 = 0.1 ASTRA
Runtime Multiplier = 1.0x (CPU)
Efficiency Bonus = 1.7x (good efficiency)
Quantum Bonus = 1.2x
VPoS Multiplier = 1.5x

Final Reward = 0.1 × 1.0 × 1.7 × 1.2 × 1.5 = 0.306 ASTRA tokens
```

### **🎮 GPU Task Example**
```
Task Details:
- Compute Units: 150
- Runtime: GPU (2.0x multiplier)
- Execution Time: 1.2 seconds (efficiency: 95%)
- Quantum Security: Enabled (1.2x bonus)
- VPoS Quality: Excellent (2.0x multiplier)

Calculation:
Base Reward = 150 × 0.001 = 0.15 ASTRA
Runtime Multiplier = 2.0x (GPU)
Efficiency Bonus = 1.9x (excellent efficiency)
Quantum Bonus = 1.2x
VPoS Multiplier = 2.0x

Final Reward = 0.15 × 2.0 × 1.9 × 1.2 × 2.0 = 1.368 ASTRA tokens
```

### **🔥 Hybrid AI/ML Task Example**
```
Task Details:
- Compute Units: 300
- Runtime: Hybrid (1.5x multiplier)
- Execution Time: 0.8 seconds (efficiency: 98%)
- Quantum Security: Enabled (1.2x bonus)
- VPoS Quality: Excellent (2.0x multiplier)
- Service Type: AI/ML (1.5x additional)

Calculation:
Base Reward = 300 × 0.001 = 0.3 ASTRA
Runtime Multiplier = 1.5x (Hybrid)
Efficiency Bonus = 2.0x (maximum efficiency)
Quantum Bonus = 1.2x
VPoS Multiplier = 2.0x (excellent)
AI/ML Bonus = 1.5x

Final Reward = 0.3 × 1.5 × 2.0 × 1.2 × 2.0 × 1.5 = 3.24 ASTRA tokens
```

---

## 💰 **Daily Earning Potential**

### **Conservative Estimates (Average Performance)**
| Node Type | Tasks/Day | Avg Reward/Task | Daily Earnings |
|-----------|-----------|-----------------|----------------|
| **CPU Node** | 50-100 | 0.2-0.4 ASTRA | 10-40 ASTRA |
| **GPU Node** | 30-60 | 0.8-1.5 ASTRA | 25-90 ASTRA |
| **Hybrid Node** | 25-50 | 1.5-3.0 ASTRA | 40-100 ASTRA |

### **Optimal Performance (High Efficiency + VPoS)**
| Node Type | Tasks/Day | Avg Reward/Task | Daily Earnings |
|-----------|-----------|-----------------|----------------|
| **High-End CPU** | 80-120 | 0.5-0.8 ASTRA | 40-95 ASTRA |
| **High-End GPU** | 40-70 | 1.2-2.5 ASTRA | 50-100 ASTRA |
| **Premium Hybrid** | 30-50 | 2.0-4.0 ASTRA | 60-100 ASTRA |

*Note: Daily earnings are capped at 100 ASTRA tokens per node by default configuration*

---

## 🔐 **Accessing Your Rewards**

### **1. Local Node Status Check**
```bash
# Check current earned tokens via API
curl http://localhost:9000/api/status

# Response includes:
{
  "earned_tokens": "50000000000000000000",  // 50 ASTRA in wei
  "total_compute_units": 5000,
  "tasks_completed": 247,
  "success_rate": 98.8
}
```

### **2. DID-Based Wallet Integration**
Your node's DID is automatically converted to blockchain wallet addresses:
```rust
// Automatic DID to address conversion
let recipient_address = did_to_address(provider_did)?;
SpaceKitVM.setup_account_balance(&recipient_address, reward_amount).await?;
```

### **3. Cross-Chain Reward Access**
Access your rewards on any supported blockchain:
```bash
# Ethereum (primary)
ETH Address: 0x[your_did_derived_address]

# Other chains (auto-bridged)
Avalanche: Automatic LayerZero bridge
Arbitrum: Layer 2 access with low fees  
Polygon: Fast transaction processing
Solana: High-speed token operations
```

### **4. API Endpoints for Reward Management**
```bash
# Get current balance
GET /api/rewards/balance

# Get reward history
GET /api/rewards/history

# Get VPoS proofs
GET /api/vpos/proofs

# Get cross-chain bridge status
GET /api/bridge/status
```

---

## 🌐 **Sigmoid Bonding Curve Pricing**

The network implements dynamic pricing that affects reward value:

### **Mathematical Model**
```
P = k * [1 / (1 + e^(-a * (U - 0.5)))]

Where:
- P = Token price
- k = Scaling constant (max price: 10 ASTRA)
- a = Curve steepness (5.0)
- U = Network utilization ratio (0 to 1)
```

### **Price Impact on Rewards**
```toml
[sigmoid_bonding_curve]
enabled = true
scaling_constant = 10.0              # Maximum price - 10 ASTRA
steepness = 5.0                      # Curve steepness
min_price = 0.1                      # Minimum floor price - 0.1 ASTRA
max_price = 10.0                     # Maximum ceiling price - 10 ASTRA
price_update_interval = 300          # Update every 5 minutes
```

### **Reward Value Optimization**
- **Low Network Utilization**: Higher token rewards, lower token value
- **High Network Utilization**: Lower token rewards, higher token value
- **Balanced Economics**: Automatic market equilibrium

---

## 🚀 **Advanced Reward Features**

### **🎯 Stake-Based Service Provision**
```toml
[token]
minimum_stake = 10000                # 10,000 ASTRA tokens required
service_fee_percent = 2.5            # 2.5% service fee
settlement_interval_seconds = 3600   # Hourly settlement
```

### **🔒 Reputation-Based Rewards**
- **High Reputation (90%+)**: 25% discount on service costs, premium task assignment
- **Good Reputation (70%+)**: 15% discount, priority in task queue
- **Standard Reputation (50%+)**: 5% discount, normal task assignment
- **Low Reputation (<50%)**: No discounts, limited task access

### **🏆 Long-Term Staking Bonuses**
- **1+ Year Staking**: 10% bonus on all rewards
- **2+ Year Staking**: 20% bonus on all rewards
- **3+ Year Staking**: 30% bonus on all rewards

### **💎 Network Contribution Multipliers**
- **Genesis Node Operator**: 50% permanent bonus
- **Early Adopter (First 1000 nodes)**: 25% bonus for first year
- **Community Builder**: Additional rewards for network growth

---

## 📊 **Production Metrics & Analytics**

### **Real-Time Performance Dashboard**
```
┌─────────────────────────────────────┐
│ SpaceKit Node Reward Analytics      │
├─────────────────────────────────────┤
│ Total Earned:        47.3 ASTRA     │
│ Today's Earnings:    8.2 ASTRA      │
│ Success Rate:        98.8%          │
│ Efficiency Score:    94.5%          │
│ VPoS Bonus:          +24.8%         │
│ Network Rank:        #247/5,420     │
└─────────────────────────────────────┘
```

### **Reward Optimization Recommendations**
- **Hardware Upgrades**: GPU acceleration recommendations
- **Efficiency Improvements**: Resource optimization suggestions
- **Network Quality**: Connectivity and uptime improvements
- **Service Diversification**: Recommended service types for maximum rewards

---

## 🛠️ **Technical Implementation**

### **Reward Calculation Engine**
```rust
impl ComputeNode {
    async fn calculate_token_reward(&self, metrics: &ResourceMetrics, runtime: &str) -> u128 {
        let config = &self.config.token_reward_config;
        
        // Base reward calculation
        let base_reward = (metrics.compute_units_used as f64) * config.base_reward_per_unit;
        
        // Apply all multipliers
        let runtime_multiplier = match runtime {
            "gpu" => config.gpu_multiplier,
            "hybrid" => config.hybrid_multiplier,
            _ => config.cpu_multiplier,
        };
        
        let efficiency_bonus = self.calculate_efficiency_bonus(metrics, config);
        let quantum_bonus = if self.config.quantum_security_enabled { 
            config.quantum_bonus 
        } else { 
            1.0 
        };
        
        // Convert to wei (18 decimals)
        let final_reward = base_reward * runtime_multiplier * efficiency_bonus * quantum_bonus;
        let reward_wei = (final_reward * 1e18) as u128;
        
        // Apply daily limits
        reward_wei.min(config.max_daily_rewards / 100)
    }
}
```

### **VPoS Integration**
```rust
// Enhanced reward with VPoS verification
if let Some(identity) = &self.identity {
    let vpos_manager = VPoSManager::new(identity.clone(), Algorithm::Kyber512).await?;
    
    // Generate cryptographic proof
    let proof = vpos_manager.generate_service_proof(&task, &result, &metrics, &owner_did).await?;
    
    // Verify and calculate enhanced reward
    if let Some(vpos_reward) = vpos_manager.verify_and_calculate_reward(&proof).await? {
        let enhanced_reward = vpos_reward.max(base_reward);
        
        // Submit proof to network
        let tx_hash = vpos_manager.submit_proof_to_network(&proof).await?;
        
        // Mint enhanced tokens
        self.mint_task_reward(task_id, &node_did, enhanced_reward, &metrics).await?;
    }
}
```

### **Cross-Chain Bridge Integration**
```rust
// Automatic cross-chain reward distribution
if enhanced_reward > auto_bridge_threshold {
    bridge_manager.distribute_cross_chain_reward(
        task_id,
        provider_did,
        enhanced_reward,
        SupportedChain::Ethereum,    // Source
        user_preferred_chain,        // Destination
        RewardType::ComputeTask,
    ).await?;
}
```

---

## 💳 **Transaction Fee Handling - CRITICAL UPDATE**

### **🚨 User Pays All Transaction Fees**

**IMPORTANT**: All blockchain transaction fees are **deducted from the user's reward amount** before distribution. The network does **NOT** pay gas fees on behalf of users.

### **Fee Structure & Deductions**

#### **Fee Components Deducted from Rewards**
```rust
pub struct RewardDistributionFees {
    pub base_gas_fee: u128,              // Basic transaction fee (~0.00042 ETH)
    pub bridge_fee: u128,                // LayerZero cross-chain fees (~0.065 ETH)
    pub network_fee: u128,               // SpaceKit network service fee (0.1%)
    pub total_fees: u128,                // Sum of all fees
    pub minimum_reward_threshold: u128,   // 5x total fees for economic viability
}
```

#### **Real-World Fee Examples**
| Network | Gas Fee | Bridge Fee | Service Fee | Total Deducted |
|---------|---------|------------|-------------|----------------|
| **Ethereum** | 0.0004 ETH | 0.065 ETH | 0.1% of reward | ~0.0654+ ETH |
| **Arbitrum** | 0.0001 ETH | 0.045 ETH | 0.1% of reward | ~0.0451+ ETH |
| **Polygon** | 0.001 MATIC | 0.035 ETH | 0.1% of reward | ~0.0351+ ETH |
| **Avalanche** | 0.002 AVAX | 0.040 ETH | 0.1% of reward | ~0.0401+ ETH |

### **Minimum Reward Thresholds**

#### **Economic Viability Checks**
```rust
// Reward must be 5x fees to be economical
let minimum_threshold = total_fees * 5;

if reward_amount <= total_fees {
    // Accumulate for batch distribution instead
    add_to_pending_rewards(provider_did, reward_amount).await;
    return;
}

// Deduct fees from user's reward
let net_reward = reward_amount - total_fees;
```

#### **Minimum Thresholds by Network**
| Network | Min Gas | Min Bridge | Min Reward | Example Net Reward |
|---------|---------|------------|------------|-------------------|
| **Ethereum** | 0.0004 ETH | 0.065 ETH | **0.33 ETH** | 0.265 ETH after fees |
| **Arbitrum** | 0.0001 ETH | 0.045 ETH | **0.23 ETH** | 0.185 ETH after fees |
| **Polygon** | 0.001 MATIC | 0.035 ETH | **0.18 ETH** | 0.145 ETH after fees |
| **Avalanche** | 0.002 AVAX | 0.040 ETH | **0.20 ETH** | 0.160 ETH after fees |

*Note: All amounts shown are equivalent ASTRA token values*

### **Fee-Aware Reward Processing**

#### **1. Pre-Distribution Fee Calculation**
```rust
async fn mint_task_reward(&self, reward_amount: u128) -> Result<TokenMintResult> {
    // STEP 1: Calculate all fees first
    let fee_result = self.calculate_reward_distribution_fees(reward_amount).await?;
    
    // STEP 2: Check economic viability
    if reward_amount <= fee_result.total_fees {
        // Too small - add to batch for later processing
        return self.add_to_pending_rewards(provider_did, reward_amount).await;
    }
    
    // STEP 3: Deduct fees from user's reward (USER PAYS!)
    let net_reward_amount = reward_amount - fee_result.total_fees;
    
    // STEP 4: Distribute net amount to user
    distribute_reward(provider_did, net_reward_amount).await?;
}
```

#### **2. Batch Processing for Small Rewards**
```rust
// Small rewards accumulate until batch threshold is met
async fn add_to_pending_rewards(&self, provider_did: &str, amount: u128) -> Result<()> {
    let pending_total = get_pending_rewards(provider_did).await + amount;
    
    if pending_total >= minimum_threshold {
        // Process entire batch as single transaction
        let net_amount = pending_total - calculate_fees(pending_total).await;
        distribute_reward(provider_did, net_amount).await?;
        clear_pending_rewards(provider_did).await;
    }
}
```

### **Fee Optimization Strategies**

#### **🎯 For Node Operators**
1. **Hardware Investment**: Higher performance = larger rewards = better fee ratio
2. **Efficiency Optimization**: Better efficiency bonuses = larger rewards
3. **Service Specialization**: AI/ML and GPU tasks have higher reward multipliers
4. **Batch Accumulation**: Let small rewards accumulate before withdrawal

#### **🌐 Network-Level Optimizations**
1. **Dynamic Chain Selection**: Route rewards to lowest-cost chains
2. **Batch Distribution**: Combine multiple small rewards into single transactions
3. **Gas Price Optimization**: Time transactions for lower gas periods
4. **Layer 2 Prioritization**: Default to Arbitrum/Polygon for lower fees

### **Fee Transparency & Reporting**

#### **Real-Time Fee Display**
```bash
# API endpoint showing current fees
GET /api/rewards/fees

{
  "current_gas_price": "20 gwei",
  "estimated_fees": {
    "ethereum": "0.0654 ETH",
    "arbitrum": "0.0451 ETH", 
    "polygon": "0.0351 ETH"
  },
  "minimum_thresholds": {
    "ethereum": "0.33 ASTRA",
    "arbitrum": "0.23 ASTRA",
    "polygon": "0.18 ASTRA"
  }
}
```

#### **Reward Breakdown in Node Status**
```bash
curl http://localhost:9000/api/rewards/breakdown

{
  "total_earned": "47.3 ASTRA",
  "total_fees_paid": "3.2 ETH",
  "net_received": "44.1 ASTRA",
  "pending_batch": "0.05 ASTRA",
  "efficiency_ratio": "93.2%"
}
```

### **Configuration Examples**

#### **Low-Fee Node Configuration**
```toml
[reward_distribution]
enabled = true
auto_batch_small_rewards = true
minimum_batch_size = 0.5            # 0.5 ASTRA minimum batch
preferred_chains = ["arbitrum", "polygon", "avalanche"]
avoid_ethereum_unless_large = true  # Only use ETH for rewards >1 ASTRA

[fee_optimization] 
monitor_gas_prices = true
max_gas_price_gwei = 30             # Don't distribute if gas >30 gwei
retry_on_high_gas = true
gas_price_check_interval = 300      # Check every 5 minutes
```

#### **High-Performance Node Configuration**
```toml
[reward_distribution]
enabled = true
auto_batch_small_rewards = false    # Immediate distribution
minimum_reward_threshold = 1.0      # 1 ASTRA minimum
preferred_chains = ["ethereum"]     # Use primary network
distribute_immediately = true       # No batching delay

[fee_acceptance]
max_fee_percentage = 15.0           # Accept up to 15% fees on rewards
prioritize_speed_over_cost = true   # Faster distribution over lower fees
```

---

## 🔍 **Fee Impact Analysis**

### **Conservative Node (CPU-only)**
```
Daily Rewards: 20 ASTRA
Daily Fees: 2-3 ETH (10-15% of rewards)
Net Daily: 17-18 ASTRA
Monthly Net: 510-540 ASTRA
```

### **High-Performance Node (GPU)**
```
Daily Rewards: 80 ASTRA  
Daily Fees: 3-5 ETH (4-6% of rewards)
Net Daily: 75-77 ASTRA
Monthly Net: 2,250-2,310 SWTCH
```

### **Premium Node (Hybrid AI/ML)**
```
Daily Rewards: 100 ASTRA
Daily Fees: 4-6 ETH (4-6% of rewards)  
Net Daily: 94-96 ASTRA
Monthly Net: 2,820-2,880 ASTRA
```

**🎯 Key Insight**: Higher-performance nodes have better fee efficiency ratios due to larger individual rewards exceeding minimum thresholds more easily.

---

## 📋 **Configuration Examples**

### **High-Performance Node Configuration**
```toml
[compute]
max_concurrent_tasks = 20
max_memory_mb = 32768
max_cpu_cores = 16
gpu_enabled = true
quantum_security_enabled = true

[token_reward_config]
base_reward_per_unit = 0.002        # Higher base rate
gpu_multiplier = 2.5                # Increased GPU bonus
quantum_bonus = 1.3                 # Higher quantum bonus
max_daily_rewards = 150.0           # Increased daily limit

[vpos]
enabled = true
auto_submit_proofs = true
min_quality_score = 0.9
reputation_weight = 0.8
```

### **Energy-Efficient Node Configuration**
```toml
[compute]
max_concurrent_tasks = 8
max_memory_mb = 8192
quantum_security_enabled = true

[token_reward_config]
base_reward_per_unit = 0.0015
max_efficiency_bonus = 2.5          # Higher efficiency rewards
min_efficiency_penalty = 0.7        # Less penalty for slower tasks

[energy_optimization]
max_power_consumption = 200          # Watts
efficiency_target = 0.95
sleep_mode_enabled = true
```

---

## 🎯 **Best Practices for Maximum Rewards**

### **🏆 Performance Optimization**
1. **Hardware Selection**: Invest in high-performance GPUs for maximum multipliers
2. **Network Quality**: Ensure stable, high-bandwidth internet connection
3. **Resource Management**: Optimize CPU and memory usage for efficiency bonuses
4. **Task Specialization**: Focus on high-value services (AI/ML, hybrid computing)

### **🛡️ VPoS Optimization**
1. **Quality Maintenance**: Maintain >95% success rate for excellent VPoS scores
2. **Proof Generation**: Enable automatic VPoS proof submission
3. **Service Diversification**: Offer multiple service types for varied multipliers
4. **Reputation Building**: Consistent, long-term operation builds network reputation

### **🔐 Security Best Practices**
1. **Quantum Security**: Always enable quantum-resistant encryption for bonuses
2. **Algorithm Support**: Support multiple post-quantum algorithms
3. **Key Management**: Secure private key storage and rotation
4. **Network Security**: Use VPNs and secure connections for node operation

### **💰 Economic Optimization**
1. **Staking Strategy**: Stake tokens for service provision and long-term bonuses
2. **Cross-Chain Planning**: Choose optimal chains for reward withdrawal
3. **Market Timing**: Monitor sigmoid curve pricing for optimal service timing
4. **Reinvestment**: Use earned tokens to upgrade hardware and increase capacity

---

## 📅 **Quarterly Reward Accrual System**

### **🎯 Overview**

Node operators can choose to **accumulate smaller rewards** and receive them in **quarterly batches** to significantly reduce transaction fees and increase net rewards.

### **📋 How Quarterly Accrual Works**

#### **1. Automatic Accumulation**
```rust
// When reward is too small for individual distribution
if reward_amount <= transaction_fees {
    // Automatically added to quarterly batch
    add_to_pending_rewards(provider_did, reward_amount).await?;
}
```

#### **2. Quarterly Distribution Schedule**
- **Distribution Dates**: 15th of March, June, September, December
- **Minimum Batch**: 1 ASTRA token accumulated
- **Maximum Batch**: 100 ASTRA tokens (forced distribution)
- **Grace Period**: 30 days to claim after distribution

#### **3. User Access & Management**
```rust
// Check your pending rewards
let pending = node.get_pending_rewards("did:spacekit:provider:your_did").await;

// Manually trigger distribution (if above minimum)
let results = node.process_batch_rewards("did:spacekit:provider:your_did").await?;

// Check all rewards ready for distribution
let ready_rewards = node.get_rewards_ready_for_distribution().await;
```

### **💰 Economic Benefits**

#### **Individual vs. Quarterly Distribution**
| Method | Reward Size | Transaction Fees | Net Reward | Efficiency |
|--------|-------------|------------------|------------|------------|
| **Individual** | 0.1 ASTRA | 0.065 ETH (~$130) | **LOSS** | ❌ -130% |
| **Quarterly** | 12 ASTRA | 0.065 ETH (~$130) | **11.87 ASTRA** | ✅ +99% |

#### **Quarterly Accumulation Example**
```rust
// 90 days of small rewards
Task 1: 0.1 ASTRA (accumulated)
Task 2: 0.05 ASTRA (accumulated)
Task 3: 0.2 ASTRA (accumulated)
...
Task 150: 0.08 ASTRA (accumulated)

// Quarterly distribution
Total Accumulated: 12.5 ASTRA
Transaction Fee: 0.065 ETH (~$130)
Net Reward: 12.435 ASTRA (99.5% efficiency)
```

### **🔧 Configuration Options**

#### **Quarterly Reward Config**
```rust
pub struct QuarterlyRewardConfig {
    pub enabled: bool,                        // Enable quarterly batching
    pub distribution_frequency_days: u32,     // 90 days (quarterly)
    pub minimum_batch_amount: u128,           // 1 ASTRA minimum
    pub maximum_batch_amount: u128,           // 100 ASTRA max
    pub distribution_dates: Vec<u32>,         // [15, 15, 15, 15] (quarterly)
    pub claim_grace_period_days: u32,         // 30 days to claim
    pub auto_distribute: bool,                // Automatic distribution
}
```

### **📊 Reward Status Tracking**

#### **Pending Reward States**
- **Accumulating**: Collecting rewards until threshold
- **ReadyForDistribution**: Meets minimum or scheduled date
- **DistributionScheduled**: Queued for next batch
- **Distributed**: Successfully sent to blockchain
- **Claimed**: User has accessed their rewards

#### **User Dashboard Information**
```rust
pub struct PendingReward {
    pub accumulated_amount: u128,         // Total ASTRA pending
    pub task_count: u32,                  // Number of contributing tasks
    pub next_distribution_date: DateTime, // Next scheduled payout
    pub status: PendingRewardStatus,      // Current status
}
```

### **🚀 Forced Distribution Protection**

#### **Maximum Threshold Protection**
```rust
// Automatic distribution when accumulation gets too large
if accumulated_amount >= 100 ASTRA {
    // Force immediate distribution to prevent loss
    schedule_immediate_distribution().await?;
}
```

### **🌉 Cross-Chain Quarterly Distribution**

#### **Automatic Cross-Chain Batching**
```rust
// Quarterly rewards automatically distributed across chains
if quarterly_reward_config.auto_distribute {
    distribute_cross_chain_reward(
        batch_id,
        provider_did,
        net_amount,
        SupportedChain::Ethereum,    // Source
        SupportedChain::Polygon,     // Destination (lower fees)
        RewardType::ComputeTask,
    ).await?;
}
```

#### **Cross-Chain Fee Benefits**
- **Ethereum**: High security, higher fees
- **Polygon**: Lower fees for smaller amounts
- **Arbitrum**: Optimized for frequent transactions
- **Optimism**: Balanced fees and security

### **🎯 Quarterly Accrual Benefits Summary**

#### **✅ Why Use Quarterly Accrual?**
1. **Fee Efficiency**: 99%+ efficiency vs. 0% for small individual rewards
2. **Automatic Management**: Zero user intervention required
3. **Cross-Chain Optimization**: Automatically routes to lowest-cost chains
4. **Compound Growth**: Rewards accumulate without immediate fee deductions
5. **Predictable Payouts**: Scheduled quarterly distribution dates

#### **📱 User Experience**
```bash
# Check pending rewards
curl http://localhost:9000/api/rewards/pending

{
  "accumulated_amount": "8.45 ASTRA",
  "task_count": 127,
  "next_distribution_date": "2024-06-15T00:00:00Z",
  "status": "Accumulating",
  "days_until_distribution": 23
}
```

#### **💡 Smart Distribution Logic**
- **Small Rewards**: Automatically accumulate for quarterly batch
- **Large Rewards**: Distribute immediately if above threshold
- **Emergency Override**: Force distribution if accumulation exceeds 100 ASTRA
- **Gas Optimization**: Monitor gas prices for optimal distribution timing

---

## 📞 **Support & Resources**

### **Documentation**
- **Technical Docs**: `/documentation/`
- **API Reference**: `/api/docs`
- **Configuration Guide**: `/examples/config.toml`

### **Community**
- **Discord**: SpaceKit Node Operators
- **Telegram**: @SpaceKitNetwork
- **GitHub**: Issues and feature requests

### **Monitoring Tools**
- **Node Dashboard**: `http://localhost:9000/dashboard`
- **Metrics API**: `http://localhost:9000/api/metrics`
- **Health Check**: `http://localhost:9000/api/health`

---

*The SpaceKit Network reward system is designed to fairly compensate node operators while maintaining network security, efficiency, and long-term sustainability. All rewards are automatically calculated, verified, and distributed using quantum-resistant cryptographic proofs.*