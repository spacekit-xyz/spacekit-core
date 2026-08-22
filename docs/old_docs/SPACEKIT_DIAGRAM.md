# SpaceKit Architecture

SpaceKit is a decentralized network of compute, storage, messaging, and networking nodes. It is designed to be a quantum-safe, AI-native, DID-centric orchestration platform.

## Local Environment
Each user's browser, IoT device, or Node.js server is a node on the network. It runs a local blockchain (ComputeVM) with local storage and messaging. It can optionally join a global network of compute, storage, and messaging nodes.

### Local Environment Diagram
```mermaid
flowchart TD

    subgraph Browser["Browser / IoT / Node.js"]
        VM["ComputeVM Node (Local Blockchain VM)"]
        StorageLocal["Local Storage Node"]
        MsgLocal["Local Messaging (WebRTC Pub/Sub)"]
    end

    Browser -->|Optional Join| Sync["Quantum-Safe Verkle Stateless Sync"]

    subgraph GlobalNet["Global SpaceKit Networks (Public / Private)"]
        ComputeNode["Decentralized Compute Nodes"]
        StorageNode["Decentralized Storage Nodes"]
        MsgNode["Decentralized Messaging Nodes"]
    end

    Sync --> ComputeNode
    Sync --> StorageNode
    Sync --> MsgNode

    ComputeNode --> Chat["Decentralized Chat"]
    StorageNode --> Media["Decentralized Storage & Content Delivery"]
    ComputeNode --> DCompute["Decentralized Compute"]

    MsgNode --> Chat
    MsgNode --> DCompute
```



## Stateless Sync
Stateless sync is a technique where the local blockchain is synced with the global network by requesting the state root and proofs from the global network. The local blockchain is then updated with the proofs and state.

### Stateless Sync Sequence Diagram
```mermaid
sequenceDiagram
    participant Browser as Browser VM (Local Blockchain)
    participant Sync as Verkle Sync
    participant Compute as Compute Node
    participant Storage as Storage Node
    participant Msg as Messaging Node

    Browser->>Browser: Start local blockchain (ComputeVM)
    Browser->>Browser: Initialize local storage + messaging

    Browser->>Sync: Optional: Join network
    Sync->>Compute: Request state root + proofs
    Sync->>Storage: Request content metadata
    Sync->>Msg: Join pub/sub channels

    Compute-->>Browser: Verkle proofs + state
    Storage-->>Browser: Content references
    Msg-->>Browser: Messaging channels

    Browser->>Browser: Local state updated (stateless sync)
```