# TRM (Tiny Recursive Model) Integration Guide

## Overview

TRM (Tiny Recursive Model) is a revolutionary recursive reasoning model that achieves **45% on ARC-AGI-1** and **8% on ARC-AGI-2** using only **7M parameters**. This guide explains how to integrate TRM with the SpaceKit Compute Node for distributed recursive reasoning tasks.

## Key Features

- **Recursive Self-Refinement**: Model iteratively improves its predictions
- **Adaptive Computation Time (ACT)**: Q-learning based halting mechanism
- **Minimal Parameters**: Only 7M parameters (vs billions in LLMs)
- **ARC-AGI Specialist**: Optimized for visual reasoning puzzles
- **Multiple Variants**: ARC-AGI, Sudoku, Maze solving

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                    TRM Recursive Reasoning                      │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│   Input Grid (x)    ─────►  Token Embedding  ─────►             │
│                                    │                            │
│                                    ▼                            │
│   ┌──────────────────────────────────────────────────────────┐  │
│   │                   Recursive Reasoning Loop               │  │
│   │                                                          │  │
│   │   for step in 1..max_steps:                              │  │
│   │     ┌─────────────────────────────────────────────────┐  │  │
│   │     │  L-Level (H_cycles × L_cycles iterations)       │  │  │
│   │     │    z_L = L_level(z_L, z_H + x)                  │  │  │
│   │     │    z_H = L_level(z_H, z_L)                      │  │  │
│   │     └─────────────────────────────────────────────────┘  │  │
│   │                         │                                │  │
│   │                         ▼                                │  │
│   │     ┌─────────────────────────────────────────────────┐  │  │
│   │     │  ACT Halting Decision                           │  │  │
│   │     │    q_halt, q_continue = Q_head(z_H)             │  │  │
│   │     │    if sigmoid(q_halt) > 0.5: HALT               │  │  │
│   │     └─────────────────────────────────────────────────┘  │  │
│   └──────────────────────────────────────────────────────────┘  │
│                              │                                  │
│                              ▼                                  │
│   Output Logits  ◄─────  LM Head(z_H)                           │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

## Installation

### 1. Enable ORT Feature

Ensure the `ort` feature is enabled in your `Cargo.toml`:

```toml
[dependencies]
spacekit-compute-node = { version = "0.2", features = ["ort"] }
```

### 2. Export TRM Model to ONNX

First, export your trained TRM model from PyTorch to ONNX:

```bash
# Set path to TinyRecursiveModels repo
export TRM_REPO_PATH=/path/to/TinyRecursiveModels

# Export with default config
python scripts/export_trm_onnx.py \
    --checkpoint checkpoints/arc1concept/step_50000 \
    --output models/trm/trm_arc_agi.onnx \
    --variant arc_agi
```

Available variants:
- `default`: General purpose (seq_len=1024, vocab=256)
- `arc_agi`: ARC-AGI puzzles (seq_len=900, vocab=11)
- `sudoku`: Sudoku solving (seq_len=81, vocab=10)
- `maze`: Maze solving (seq_len=900, vocab=4)

### 3. Place Model File

```bash
mkdir -p models/trm
mv trm_arc_agi.onnx models/trm/
```

## Usage

### Rust API

```rust
use spacekit_compute_node::{
    TRMInferenceManager, TRMConfig, TRMRequest,
};
use std::path::PathBuf;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Create manager
    let manager = TRMInferenceManager::new();
    
    // Register ARC-AGI model
    let config = TRMConfig {
        model_path: PathBuf::from("models/trm/trm_arc_agi.onnx"),
        seq_len: 900,
        hidden_size: 384,
        vocab_size: 11,
        h_cycles: 3,
        l_cycles: 4,
        max_halt_steps: 16,
        puzzle_emb_len: 16,
    };
    
    manager.register_model("arc_agi", config).await?;
    
    // Create request
    let request = TRMRequest {
        operation: "solve_puzzle".to_string(),
        input_grid: vec![
            vec![0, 0, 0, 1, 1],
            vec![0, 0, 0, 1, 1],
            vec![0, 0, 0, 0, 0],
            vec![2, 2, 0, 0, 0],
            vec![2, 2, 0, 0, 0],
        ],
        output_shape: Some((5, 5)),
        max_steps: Some(16),
        puzzle_id: None,
        model_variant: Some("arc_agi".to_string()),
    };
    
    // Execute
    let response = manager.process_request(&request).await?;
    
    println!("Success: {}", response.success);
    println!("Output: {:?}", response.output_grid);
    println!("Steps: {}", response.reasoning_steps);
    println!("Confidence: {:.2}", response.halt_confidence);
    
    Ok(())
}
```

### ML Operations API

```rust
use spacekit_compute_node::ml_operations::TRMRecursiveReasoningOperation;
use spacekit_compute_node::ml_operation_registry::MLOperation;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let op = TRMRecursiveReasoningOperation::new();
    
    let input = serde_json::json!({
        "input_grid": [[0, 1, 2], [3, 4, 5], [6, 7, 8]],
        "max_steps": 16
    });
    
    let result = op.execute(&input).await?;
    let response: serde_json::Value = serde_json::from_slice(&result)?;
    
    println!("{}", serde_json::to_string_pretty(&response)?);
    
    Ok(())
}
```

### Via WASM Contract

```json
{
    "operation": "trm-recursive-reasoning",
    "input_grid": [
        [0, 0, 0, 1, 1],
        [0, 0, 0, 1, 1],
        [0, 0, 0, 0, 0],
        [2, 2, 0, 0, 0],
        [2, 2, 0, 0, 0]
    ],
    "max_steps": 16,
    "model_variant": "arc_agi"
}
```

### Via RPC

```bash
curl -X POST http://localhost:9000/ml/execute \
  -H "Content-Type: application/json" \
  -d '{
    "operation_id": "trm-recursive-reasoning",
    "input": {
      "input_grid": [[0, 0, 1], [0, 1, 0], [1, 0, 0]],
      "max_steps": 8
    }
  }'
```

## Response Format

```json
{
    "success": true,
    "output_grid": [
        [1, 1, 0, 0, 0],
        [1, 1, 0, 0, 0],
        [0, 0, 0, 0, 0],
        [0, 0, 0, 2, 2],
        [0, 0, 0, 2, 2]
    ],
    "reasoning_steps": 8,
    "halt_confidence": 0.87,
    "inference_time_ms": 342.5,
    "model_used": "TRM-arc_agi",
    "metrics": {
        "total_steps": 8,
        "halt_confidence": 0.87,
        "h_cycles": 3,
        "l_cycles": 4
    }
}
```

## Model Variants

| Variant | Task | Grid Size | Vocab | Steps | Model Size |
|---------|------|-----------|-------|-------|------------|
| default | General | 32×32 | 256 | 16 | ~30MB |
| arc_agi | ARC-AGI | 30×30 | 11 | 16 | ~28MB |
| sudoku | Sudoku | 9×9 | 10 | 32 | ~28MB |
| maze | Maze | 30×30 | 4 | 24 | ~28MB |

## Performance

### Benchmarks (Single GPU - RTX 4090)

| Task | Accuracy | Steps (avg) | Latency |
|------|----------|-------------|---------|
| ARC-AGI-1 | 45% | 12 | ~500ms |
| ARC-AGI-2 | 8% | 14 | ~600ms |
| Sudoku | 87% | 18 | ~800ms |
| Maze | 92% | 10 | ~400ms |

### CPU Performance

TRM is efficient enough to run on CPU:
- ~50ms per reasoning step on modern CPU
- Total latency: 400-800ms for typical puzzles

## Pricing

TRM operations are priced based on compute requirements:

| Operation | Base Cost | Per-Step | Typical Total |
|-----------|-----------|----------|---------------|
| solve_puzzle | 5000 | 200 | 8200 |
| grid_transform | 5000 | 200 | 7000 |
| pattern_completion | 5000 | 200 | 6000 |

*Costs in SWTCHX micro-units (1 SWTCHX = 1,000,000 micro)*

## Troubleshooting

### Model Not Found

```
Error: TRM model file not found: models/trm/trm_arc_agi.onnx
```

Ensure you've exported and placed the ONNX model:
```bash
python scripts/export_trm_onnx.py --checkpoint <path> --output models/trm/trm_arc_agi.onnx
```

### ORT Feature Not Enabled

```
Error: ORT feature not enabled
```

Add the `ort` feature to your dependency:
```toml
spacekit-compute-node = { version = "0.2", features = ["ort"] }
```

### Inference Timeout

TRM uses recursive reasoning which can take many steps. Adjust timeouts:
```rust
let request = TRMRequest {
    max_steps: Some(8),  // Reduce max steps
    ..Default::default()
};
```

### Memory Issues

For large grids, consider:
- Using smaller batch sizes
- Reducing max_steps
- Using CPU inference (more memory efficient)

## References

- [TRM Paper: "Less is More: Recursive Reasoning with Tiny Networks"](https://arxiv.org/abs/2510.04871)
- [TinyRecursiveModels GitHub](https://github.com/SamsungSAILMontreal/TinyRecursiveModels)
- [ARC-AGI Benchmark](https://arcprize.org/)

## License

TRM integration is part of SpaceKit Compute Node, licensed under the SpaceKit License.
The TRM model architecture is from Samsung AI Lab Montreal.

