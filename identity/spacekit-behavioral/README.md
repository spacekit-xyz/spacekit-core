# SWTCH Behavioral Cryptography Simulation

This simulation implements and tests the behavioral cryptography concepts from the [SWTCH Behavioral Cryptography whitepaper](https://github.com/swtchlabs/swtch-network-whitepaper/blob/main/Behavioral-Cryptography.md), demonstrating keyless identity recovery through network participation patterns.

## Overview

The simulation creates a diverse population of users across different archetypes and simulates their behavior over time to test:

- **Behavioral Pattern Development**: How users build unique behavioral fingerprints
- **Identity Recovery**: Keyless recovery through behavioral challenges
- **Fraud Detection**: Identification of malicious behavior patterns
- **Cross-Archetype Analysis**: Performance differences across user types

## User Archetypes

The simulation includes 7 distinct user archetypes, each with unique behavioral characteristics:

### 1. **Base User** (40% of population)
- Typical network user with moderate activity
- Uses basic services (compute, storage, messaging)
- Business hours activity pattern
- Conservative economic engagement

### 2. **Validator** (15% of population) 
- 24/7 network infrastructure operator
- Highest security consciousness and consistency
- Aggressive staking and governance participation
- Universal cross-chain activity

### 3. **Developer** (20% of population)
- Software builder with irregular but intense patterns
- High collaboration and innovation adoption
- Variable consistency, flexible hours
- Multi-chain usage for testing

### 4. **Researcher** (10% of population)
- Academic/industry researcher
- Data-focused with high consistency
- Collaborative interaction style
- Strong compliance adherence

### 5. **Investor** (8% of population)
- Financial participant focused on economics
- Market-driven variable activity
- Competitive behavior
- Extreme risk tolerance

### 6. **Regulator** (2% of population)
- Compliance and monitoring entity
- Extremely consistent business hours
- Highest security requirements
- Conservative innovation adoption

### 7. **Other** (5% of population)
- Miscellaneous users with unpredictable patterns
- Variable and experimental behavior
- Low economic engagement

## Behavioral Parameters

Each user develops a unique profile across multiple dimensions:

- **Activity Patterns**: Frequency, timing, and consistency of network usage
- **Service Preferences**: Which SWTCH services they primarily use
- **Interaction Style**: Collaborative, competitive, supportive, independent, or suspicious
- **Economic Behavior**: Staking, transactions, governance participation
- **Cross-Chain Activity**: Usage across different blockchain networks
- **Security Compliance**: Adherence to security best practices

## Installation and Setup

### Prerequisites
- Rust 1.70+ 
- Cargo package manager

### Install Dependencies
```bash
cd swtch-behavioral
cargo build --release
```

## Running the Simulation

### Quick Test Run
```bash
cargo run -- --quick
```
This runs a small simulation (100 users, 7 days) for testing.

### Full Simulation
```bash
cargo run -- --users 1000 --days 30
```

### Custom Configuration
```bash
cargo run -- \
  --users 500 \
  --days 14 \
  --fraud-percentage 0.05 \
  --confidence-threshold 0.8 \
  --output custom_results.json \
  --seed 12345
```

### Command Line Options

| Option | Description | Default |
|--------|-------------|---------|
| `--users, -u` | Number of users to simulate | 1000 |
| `--days, -d` | Simulation duration in days | 30 |
| `--fraud-percentage, -f` | Percentage of fraudulent users (0.0-1.0) | 0.05 |
| `--confidence-threshold, -c` | Minimum confidence for recovery eligibility | 0.8 |
| `--output, -o` | JSON output file | simulation_results.json |
| `--seed, -s` | Random seed for reproducibility | Random |
| `--quick, -q` | Quick test run (100 users, 7 days) | false |

## Understanding Results

The simulation generates several output files:

### 1. Console Output
Real-time progress and summary statistics including:
- Recovery success rates by archetype
- Fraud detection effectiveness
- Confidence score evolution
- Network health metrics

### 2. JSON Results (`simulation_results.json`)
Complete simulation data including:
- Individual user behavioral patterns
- Recovery attempt histories
- Timeline snapshots
- Archetype performance metrics

### 3. Summary Report (`behavioral_cryptography_report.md`)
Detailed analysis report with:
- Executive summary
- Archetype performance comparison
- Key findings and insights
- Technical configuration details

## Key Metrics

### Recovery Performance
- **Recovery Success Rate**: Percentage of successful identity recoveries
- **Eligibility Rate**: Percentage of users meeting confidence threshold
- **Challenge Success**: Success rates by challenge type

### Security Effectiveness
- **Fraud Detection Rate**: Percentage of fraudulent users identified
- **False Positive Rate**: Legitimate users incorrectly flagged
- **Pattern Stability**: Consistency of behavioral patterns over time

### Network Health
- **Average Confidence**: Overall network confidence score
- **Participation Growth**: Evolution of user engagement
- **Cross-Archetype Performance**: Comparative analysis

## Expected Results

Based on the whitepaper theoretical framework, expected outcomes include:

- **80-95% recovery success** for eligible users with established patterns
- **70-90% fraud detection** through behavioral anomaly analysis
- **Validators and Developers** showing highest confidence scores
- **Pattern stability improvement** over time with network participation

## Behavioral Cryptography Validation

The simulation validates key behavioral cryptography concepts:

1. **Keyless Recovery**: Users can recover identity through behavioral patterns alone
2. **Fraud Resistance**: Malicious behavior is detectable through pattern analysis
3. **Privacy Preservation**: Recovery works without exposing personal data
4. **Scalability**: System works across diverse user populations
5. **Network Effects**: Longer participation improves security

## Research Applications

This simulation framework supports research into:

- **Behavioral Pattern Analysis**: Understanding user behavior in decentralized networks
- **Identity Security Models**: Alternatives to traditional authentication
- **Fraud Detection**: ML-based anomaly detection in blockchain systems
- **Network Economics**: Incentive alignment for security
- **Cross-Chain Behavior**: Multi-blockchain user patterns

## Contributing

To extend the simulation:

1. **Add New Archetypes**: Modify `src/archetypes.rs`
2. **Enhance Behavioral Models**: Update `src/behavioral_engine.rs`
3. **Implement New Challenges**: Extend `src/confidence_recovery.rs`
4. **Add Analysis Tools**: Enhance output generation and visualization

## Documentation

- [Behavioral Cryptography Whitepaper](https://github.com/swtchlabs/swtch-network-whitepaper/blob/main/Behavioral-Cryptography.md)
- [SWTCH Network Protocol](https://github.com/swtchlabs/swtch-network-whitepaper/blob/main/SWTCH-Whitepaper-Concise.md)

## License

This simulation is part of the SWTCH Network research and development effort.