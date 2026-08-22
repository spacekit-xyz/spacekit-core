# SpaceKit Storage Node Tokenomics & Earning Guide

**Last Updated:** May 2026  
**Status:** Implementation guide (storage-specific rates)  
**Version:** 1.1.0  
**Published By:** SpaceKit Labs LLC

> **Canonical economics:** [`spacekit-tokenomics`](../../../spacekit-tokenomics/) — production: **SRA → AstraRewards** ([`Service_Reward_Accumulator_Spec.md`](../../../spacekit-tokenomics/Service_Reward_Accumulator_Spec.md)); storage earns **20%** of annual operator emission. This file covers **legacy testnet** storage reward formulas until SRA ships.

---

## 🎯 Executive Summary

Storage node operators earn **ASTRA tokens** (the native currency of the SpaceKit Network) by providing:
- Distributed file storage with quantum-safe encryption
- Fact Package hosting and verification
- NFT storage and management
- P2P replication and network participation

**Average Monthly Income:** 300-800 ASTRA per TB of storage

---

## 💰 Reward Structure

### Base Rewards

| Storage Type | Rate (ASTRA/GB/day) | Monthly (100GB) | Monthly (1TB) |
|--------------|---------------------|-----------------|---------------|
| **Standard Storage** | 0.01 | 30 ASTRA | 307 ASTRA |
| **Hot Storage (Facts)** | 0.015 | 45 ASTRA | 461 ASTRA |
| **NFT Storage** | 0.025 | 75 ASTRA | 768 ASTRA |

### Bonus Multipliers

#### 1. **Quantum Encryption Bonus** (+20%)
Using post-quantum algorithms: Kyber512/768/1024, NTRU, FrodoKEM

```rust
StorageRewardConfig {
    quantum_encryption_bonus: 1.2, // +20%
}
```

#### 2. **Replication Bonus** (+10% per copy)
P2P replication across network nodes

- 3 replicas = +30% total
- 5 replicas = +50% total

```rust
StorageRewardConfig {
    replication_bonus_per_copy: 1.1, // +10% per copy
}
```

#### 3. **High Availability Bonus** (+30%)
99%+ uptime

```rust
StorageRewardConfig {
    uptime_bonus_threshold: 0.99,
    uptime_bonus: 1.3, // +30%
}
```

#### 4. **Fast Retrieval Bonus** (+15%)
< 100ms average retrieval time

```rust
StorageRewardConfig {
    fast_retrieval_bonus: 1.15, // +15%
}
```

#### 5. **Reputation Bonus** (+25%)
High reputation score (> 70%)

```rust
StorageRewardConfig {
    min_reputation_for_bonus: 0.7,
    reputation_multiplier: 1.25, // +25%
}
```

#### 6. **P2P Contribution Bonus** (+10%)
Active participation in distributed network

```rust
StorageRewardConfig {
    p2p_contribution_bonus: 1.1, // +10%
}
```

#### 7. **Fact Verification Bonus** (+5%)
Hosting and verifying Fact Packages

```rust
StorageRewardConfig {
    fact_verification_bonus: 1.05, // +5%
}
```

---

## 📊 Real-World Examples

### Example 1: Basic Storage Node
```
Storage: 500GB standard storage
Uptime: 95%
Quantum: No
P2P: No

Calculation:
- Base: 500GB × 0.01 × 30 days = 150 ASTRA/month
- No bonuses applied

Monthly Income: 150 ASTRA (~$150 at $1/token)
```

### Example 2: Optimized Storage Node
```
Storage: 500GB (300GB standard + 200GB facts)
Uptime: 99.5%
Quantum: Kyber1024
P2P: 3 replicas

Calculation:
- Standard: 300GB × 0.01 × 30 = 90 ASTRA
- Facts: 200GB × 0.015 × 30 = 90 ASTRA
- Base total: 180 ASTRA

Bonuses:
- Quantum (+20%): 36 ASTRA
- Replication (+30%): 54 ASTRA
- Uptime (+30%): 54 ASTRA
- Fast retrieval (+15%): 27 ASTRA
- P2P contribution (+10%): 18 ASTRA
- Fact verification (+5%): 9 ASTRA

Total Bonuses: 198 ASTRA
Monthly Income: 378 ASTRA (~$378 at $1/token)
```

### Example 3: Premium NFT Storage Node
```
Storage: 1TB (200GB NFTs + 800GB mixed)
Uptime: 99.9%
Quantum: Kyber1024
P2P: 5 replicas
Reputation: 85%

Calculation:
- NFTs: 200GB × 0.025 × 30 = 150 ASTRA
- Facts: 500GB × 0.015 × 30 = 225 ASTRA
- Standard: 300GB × 0.01 × 30 = 90 ASTRA
- Base total: 465 ASTRA

Bonuses:
- Quantum (+20%): 93 ASTRA
- Replication (+50%): 232.5 ASTRA
- Uptime (+30%): 139.5 ASTRA
- Fast retrieval (+15%): 69.75 ASTRA
- Reputation (+25%): 116.25 ASTRA
- P2P contribution (+10%): 46.5 ASTRA
- Fact verification (+5%): 23.25 ASTRA

Total Bonuses: 720.75 ASTRA
Monthly Income: 1,185.75 ASTRA (~$1,186 at $1/token)
```

---

## 🚀 Getting Started

### 1. Setup Storage Node

```bash
# Build with reward system
cd spacekit-storage-node
cargo build --release --features "p2p,api-server,rewards"

# Start node
./target/release/spacekit-storage-node start \
  --did "did:spacekit:storage:your-node" \
  --data-dir ./storage \
  --max-storage-gb 1000 \
  --algorithm kyber1024 \
  --enable-rewards true
```

### 2. Configure Rewards

```rust
use spacekit_storage_node::{StorageNode, StorageRewardCalculator, StorageRewardConfig};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    // Create storage node
    let node = Arc::new(StorageNode::new(config).await?);
    
    // Configure rewards
    let reward_config = StorageRewardConfig {
        base_reward_per_gb_day: 0.01,
        quantum_encryption_bonus: 1.2,
        enable_token_minting: true,
        reward_interval_hours: 24,
        ..Default::default()
    };
    
    // Create reward calculator
    let mut calculator = StorageRewardCalculator::new(reward_config, node);
    
    // Calculate current rewards
    let calculation = calculator.calculate_rewards().await?;
    println!("Current rewards: {} ASTRA", calculation.final_reward);
    
    // Process reward payment
    if let Some(record) = calculator.process_reward_payment().await? {
        println!("Paid {} ASTRA to {}", 
                 record.amount_ASTRA, 
                 record.node_did);
    }
    
    // Get analytics
    let analytics = calculator.get_reward_analytics().await?;
    println!("Estimated monthly: {} ASTRA", analytics.estimated_monthly_income);
    
    Ok(())
}
```

### 3. Monitor Earnings

```rust
// Get reward history
let history = calculator.get_reward_history();
for record in history {
    println!("{}: {} ASTRA", 
             record.timestamp, 
             record.amount_astra);
}

// Get total earnings
let total = calculator.get_total_rewards_earned();
println!("Total earned: {} ASTRA", total);

// Estimate monthly income
let monthly = calculator.estimate_monthly_income().await?;
println!("Est. monthly: {} ASTRA", monthly);
```

---

## 📈 Optimization Strategies

### 1. Maximize Storage Utilization
- Target 80-90% capacity
- Mix storage types (standard, facts, NFTs)
- Focus on high-value NFT storage (2.5x multiplier)

### 2. Achieve High Availability
- 99%+ uptime earns +30% bonus
- Use redundant power and internet
- Monitor node health continuously

### 3. Enable Quantum Encryption
- Use Kyber1024 for +20% bonus
- No performance penalty
- Future-proof security

### 4. Participate in P2P Network
- Enable replication (+10% per copy)
- Share storage with network
- Build reputation over time

### 5. Optimize Retrieval Speed
- Use SSD storage for hot data
- Implement caching strategies
- Maintain low latency network

### 6. Build Reputation
- Consistent uptime
- Fast response times
- Accurate fact verification
- Long-term network participation

---

## 🔧 Configuration Reference

### Full Reward Configuration

```rust
StorageRewardConfig {
    // Base rates
    base_reward_per_gb_day: 0.01,
    
    // Storage type multipliers
    hot_storage_multiplier: 2.0,
    fact_storage_multiplier: 1.5,
    nft_storage_multiplier: 2.5,
    
    // Bonus multipliers
    quantum_encryption_bonus: 1.2,
    replication_bonus_per_copy: 1.1,
    high_availability_bonus: 1.3,
    
    // Reputation bonuses
    min_reputation_for_bonus: 0.7,
    reputation_multiplier: 1.25,
    
    // Performance bonuses
    fast_retrieval_bonus: 1.15,
    uptime_bonus_threshold: 0.99,
    uptime_bonus: 1.2,
    
    // Network bonuses
    p2p_contribution_bonus: 1.1,
    fact_verification_bonus: 1.05,
    
    // Limits
    max_daily_rewards: 100_000_000_000_000_000_000, // 100 ASTRA
    min_storage_gb_for_rewards: 10,
    
    // Settings
    enable_token_minting: true,
    reward_interval_hours: 24,
}
```

---

## 💡 Pro Tips

1. **Start Small, Scale Smart**
   - Begin with 100GB to learn the system
   - Monitor performance and earnings
   - Scale up as you optimize

2. **Focus on Quality Over Quantity**
   - Better to have 100GB optimized than 1TB poorly managed
   - High availability > high capacity

3. **Diversify Storage Types**
   - Mix standard, facts, and NFTs
   - NFT storage pays 2.5x but requires faster hardware
   - Fact storage is a sweet spot (1.5x with good demand)

4. **Long-Term Strategy**
   - Build reputation over months
   - Reputation bonus (+25%) is significant
   - Genesis node operators get +50% permanently

5. **Monitor and Adjust**
   - Track earnings daily
   - Optimize based on analytics
   - Adjust storage mix based on demand

---

## 🎓 Advanced Topics

### NFT Storage Economics

NFTs pay premium rates (2.5x) but require:
- Fast SSD storage
- High bandwidth
- Low latency
- Premium hardware

**ROI Calculation:**
```
Hardware: $500 (1TB NVMe SSD)
Monthly NFT income: 768 ASTRA × $1 = $768
ROI: < 1 month
```

### Fact Package Verification

Earn bonuses by:
- Storing Fact Packages (+1.5x rate)
- Verifying content integrity (+5% bonus)
- Contributing to knowledge graph
- Building expertise reputation

### Reputation Building

Reputation compounds over time:
- Month 1-3: Build baseline
- Month 4-6: Achieve 70%+ for bonus
- Month 7-12: Reach 85%+ for maximum earnings
- Year 2+: Long-term staking bonuses apply

---

## 📞 Support & Resources

- **Documentation:** https://docs.spacekit.xyz/storage
- **API Reference:** https://api.spacekit.xyz/storage
- **X:** https://x.com/swtch_ai
- **GitHub:** https://github.com/spacekit-xyz

---

## 🔮 Future Enhancements

### Coming Soon

1. **Staking Multipliers**
   - Lock ASTRA for higher rewards
   - 1 year = +10%
   - 2 years = +20%
   - 3 years = +30%

2. **Geographic Bonuses**
   - Underserved regions get +15%
   - Network diversity bonus

3. **Specialized Storage**
   - Medical records (HIPAA) = 3x
   - Scientific data = 2x
   - Academic research = 2.5x

4. **Dynamic Pricing**
   - Market-based rates
   - Supply/demand adjustments
   - Peak hour bonuses

---

**Ready to start earning?** Set up your storage node and join the quantum-safe future of decentralized storage!
