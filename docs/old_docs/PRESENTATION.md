# 🎨 **Mermaid Diagrams for Each Slide Section**

---

# **Slide 3 — The Macro Shift**
```mermaid
flowchart LR
    A(Global Monetary Expansion) --> D(Need for Scarce Assets)
    B(Demand for Transparency) --> E(Programmable Finance)
    C(Globalization of Markets) --> F(24/7 Settlement)
    D --> G(Crypto Adoption)
    E --> G
    F --> G
```

---

# **Slide 4 — Trustless Technology**
```mermaid
flowchart LR
    A(Centralized Trust) -- replaced by --> B(Cryptographic Trust)
    B --> C(Open Verification)
    C --> D(Immutable State)
    D --> E(Trustless Systems)
```

---

# **Slide 5 — Bitcoin: Digital Gold**
```mermaid
flowchart LR
    A(Fixed Supply 21M) --> D(Digital Scarcity)
    B(Proof-of-Work) --> E(Network Security)
    C(Decentralized Consensus) --> F(No Central Authority)
    D --> G(Bitcoin as Digital Gold)
    E --> G
    F --> G
```

---

# **Slide 6 — Bitcoin Mechanics**
```mermaid
sequenceDiagram
    participant Miner
    participant Network
    Miner->>Network: Propose Block (~10 min)
    Network->>Network: Difficulty Adjustment
    Network->>Miner: Reward Issuance (Predictable)
```

---

# **Slide 7 — Ethereum Settlement Layer**
```mermaid
flowchart LR
    A(Users) --> B(Smart Contracts)
    B --> C(Ethereum Virtual Machine)
    C --> D(Global Settlement)
    D --> E(DeFi / NFTs / DAOs / RWAs)
```

---

# **Slide 8 — Smart Contracts**
```mermaid
flowchart LR
    A(Inputs) --> B(Code Logic)
    B --> C(Deterministic Execution)
    C --> D(Guaranteed Outcome)
```

---

# **Slide 9 — DeFi Landscape**
```mermaid
flowchart LR
    A(Lending) --> E(DeFi Ecosystem)
    B(Liquidity Pools) --> E
    C(Derivatives) --> E
    D(RWA Tokenization) --> E
```

---

# **Slide 10 — Compound & Aave**
```mermaid
flowchart LR
    A(Depositors) --> B(Liquidity Pool)
    C(Borrowers) --> B
    B --> D(Algorithmic Interest Rates)
```

---

# **Slide 11 — Blockchain Trilemma**
```mermaid
graph TD
    A(Scalability)
    B(Security)
    C(Decentralization)
    A --- B
    B --- C
    C --- A
```

---

# **Slide 12 — Layer‑2 Overview**
```mermaid
flowchart LR
    A(Users) --> B(Layer 2 Execution)
    B --> C(Cryptographic Proofs)
    C --> D(Layer 1 Settlement)
```

---

# **Slide 13 — Payment Channels**
```mermaid
sequenceDiagram
    participant Alice
    participant Bob
    participant Blockchain

    Alice->>Blockchain: Open Channel (Lock Funds)
    Alice->>Bob: Off-chain Signed Payment
    Bob->>Alice: Off-chain Signed Payment
    Alice->>Blockchain: Close Channel (Final Settlement)
```

---

# **Slide 14 — Rollups**
```mermaid
flowchart LR
    A(Off-chain Execution) --> D(Rollup State)
    B(Batch Transactions) --> D
    C(Proofs: Fault or Validity) --> E(L1 Verification)
    D --> E
```

---

# **Slide 15 — RealT Tokenization**
```mermaid
flowchart LR
    A(Real Estate Asset) --> B(Tokenization)
    B --> C(Fractional Ownership Tokens)
    C --> D(On-chain Rental Income)
    D --> E(Global Investors)
```

---

# **Slide 16 — AI + Blockchain**
```mermaid
flowchart LR
    A(AI Agents) --> C(Execute Logic)
    B(Blockchain) --> D(Verify & Record)
    C --> E(Autonomous Systems)
    D --> E
```

---

# **Slide 17 — SpaceKitJS**
```mermaid
flowchart LR
    A(Browser VM) --> B(WASM Smart Contracts)
    B --> C(Quantum-Safe Storage)
    B --> D(LLM Reasoning)
    B --> E(Reputation Scoring)
    C --> F(Proofs & Receipts)
    D --> F
    E --> F
```

---

# **Slide 18 — OmniChain Payments (Case Study)**
```mermaid
flowchart LR
    A(User Payment) --> B(Tokenized Receipt)
    B --> C(Omni-chain Settlement)
    C --> D(Merchant Dashboard)
    B --> E(Loyalty / Rewards)
```

---

# **Slide 19 — The Future**
```mermaid
flowchart LR
    A(Bitcoin: Scarcity) --> F(Future Financial Stack)
    B(Ethereum: Settlement) --> F
    C(L2s: Scalability) --> F
    D(DeFi: Automation) --> F
    E(AI: Intelligence) --> F
```
