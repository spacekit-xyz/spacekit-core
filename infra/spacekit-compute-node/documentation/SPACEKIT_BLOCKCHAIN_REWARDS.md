# SpaceKit Blockchain Rewards

> **Canonical economics:** [`spacekit-tokenomics`](../../../economics/spacekit-tokenomics/) — production path is the **Service Reward Accumulator (SRA) → native rewards** ([`SERVICE_REWARD_ACCUMULATOR_SPEC.md`](../../../economics/spacekit-tokenomics/SERVICE_REWARD_ACCUMULATOR_SPEC.md), [`ASTRA_EMISSION.md`](../../../economics/spacekit-tokenomics/ASTRA_EMISSION.md)). This file covers **legacy testnet** compute reward formulas.
>
> **How rewards work now.** ASTRA is created only inside blocks. The SRA measures service and puts rewards system calls to `0x…0003` at the start of a block; the node executes them natively (`native_rewards.rs`) and every importer re-derives and checks them. `CREDIT` mints straight into the recipient address's native balance; during PoA, `CREDIT_LOCKED` locks authority and affiliated-operator rewards (nothing before 365 days from genesis, then linear to 1,095 days). Total emission is capped at 2,000,000,000 ASTRA. There is no AstraRewards contract ledger, no off-chain minting, no bridge distribution, no token price curve and no fee deduction from rewards. See [`ASTRA_LEDGER.md`](../ASTRA_LEDGER.md) and [`GOVERNANCE.md`](../GOVERNANCE.md). Sections below that describe bridges, price curves, ETH fees, earnings estimates or staking bonuses are marked as removed.

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
enable_token_minting = true         # Legacy testnet only; on a consensus network ASTRA is minted only by in-block CREDIT
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

This local counter is a metric only. The reward itself is the in-block `CREDIT` the SRA settles each epoch, minted into the native balance of the operator's address.

### **🌐 Cross-Chain Distribution (removed)**
Rewards are not bridged or distributed to other chains. ASTRA exists only as native balances on the SpaceKit chain.

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

## 💰 **Daily Earning Potential (removed)**

This document gives no earnings estimates. What an operator receives depends on the emission schedule ([`ASTRA_EMISSION.md`](../../../economics/spacekit-tokenomics/ASTRA_EMISSION.md)) and the service the SRA measures for it. ASTRA is a utility token for using the network, not an investment.

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

### **2. DID-Based Address**
Your node's DID designates a 20-byte address on the SpaceKit chain (`did:spacekit:<address hex>`). Rewards are minted to that address by the in-block `CREDIT` call; nothing writes balances outside blocks. A SPHINCS+ DID spends its ASTRA with SPHINCS+-signed transfers (`POST /v1/transfer`).

### **3. Cross-Chain Reward Access (removed)**
There is no bridge. Rewards are native ASTRA balances on the SpaceKit chain only.

### **4. API Endpoints for Reward Management**
```bash
# Native balance (balance_wei, locked_wei, nonce)
GET /v1/balance/{address}

# Get reward history
GET /api/rewards/history

# Get VPoS proofs
GET /api/vpos/proofs
```

---

## 🌐 **Sigmoid Bonding Curve Pricing (removed)**

The network sets no token price and has no bonding curve, price oracle or exchange rate. Emission follows the schedule in [`ASTRA_EMISSION.md`](../../../economics/spacekit-tokenomics/ASTRA_EMISSION.md).

---

## 🚀 **Advanced Reward Features**

### **🎯 Stake-Based Service Provision**
Validator stake is native: holdings are the address's balance plus locked rewards not yet released, and effective stake is `min(bonded, holdings − unbonding)`. The default minimum is 10,000 ASTRA (`min_stake_astra` in the genesis file; `SPACEKIT_MIN_VALIDATOR_STAKE_WEI` for `register-validator` on networks without a genesis). Staking gives block-production and governance weight; it is not a yield. See [`GOVERNANCE.md`](../GOVERNANCE.md).

### **🔒 Reputation-Based Rewards**
- **High Reputation (90%+)**: 25% discount on service costs, premium task assignment
- **Good Reputation (70%+)**: 15% discount, priority in task queue
- **Standard Reputation (50%+)**: 5% discount, normal task assignment
- **Low Reputation (<50%)**: No discounts, limited task access

### **🏆 Long-Term Staking Bonuses (removed)**
There are no staking bonuses on rewards.

### **💎 Network Contribution Multipliers (removed)**
There are no genesis-operator or early-adopter bonuses. During PoA, authority and affiliated-operator rewards are locked instead (`CREDIT_LOCKED`).

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

On a consensus (PoA/PoS) network, the node does not mint from this path: the measured service feeds the SRA, and ASTRA is minted only by the in-block `CREDIT` system call.

### **Cross-Chain Bridge Integration (removed)**
There is no cross-chain reward distribution.

---

## 💳 **Transaction Fee Handling (removed)**

Rewards are minted in-block by `CREDIT`; there is no distribution transaction, no bridge and no fee deducted from a reward. Fees on the SpaceKit chain are gas paid in ASTRA by whoever sends a transaction (`gas_limit × gas_price`; unused gas is refunded and used gas is burned). The earlier ETH/bridge fee model (fee deductions, minimum thresholds, chain selection) is removed.

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
1. **Staking**: Bond ASTRA you hold if you want to produce blocks and vote after the lift to proof of stake (see [`GOVERNANCE.md`](../GOVERNANCE.md)); staking carries no bonus
2. **Gas**: Keep enough native ASTRA at your address to pay gas for the transactions you send

---

## 📅 **Quarterly Reward Accrual System (removed)**

There is no batching or quarterly payout. The SRA settles rewards once per epoch (`SPACEKIT_SRA_EPOCH_SECS`, default 86,400) as in-block `CREDIT` calls, so a reward reaches the operator's native balance in the first block after the epoch ends, with no distribution fee.

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

*The SpaceKit Network reward system is designed to fairly compensate node operators while maintaining network security, efficiency, and long-term sustainability. Rewards are measured by the SRA, minted only inside blocks, and checked by every node that imports the block.*