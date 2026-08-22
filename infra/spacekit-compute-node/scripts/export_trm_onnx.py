#!/usr/bin/env python3
"""
TRM to ONNX Export Script for SpaceKit Compute Node

This script exports a trained TRM (Tiny Recursive Model) to ONNX format
for use with the SpaceKit Compute Node's ORT inference engine.

Usage:
    python export_trm_onnx.py --checkpoint path/to/checkpoint --output trm_model.onnx

Requirements:
    - torch
    - onnx
    - TinyRecursiveModels repository (for model definitions)

The exported ONNX model can be used with:
    - SpaceKit Compute Node (Rust + ORT)
    - Any ONNX-compatible runtime
"""

import os
import sys
import argparse
from pathlib import Path
from typing import Dict, Any, Tuple, Optional

import torch
import torch.nn as nn
import torch.onnx


def find_trm_repo() -> Optional[Path]:
    """Find TinyRecursiveModels repository"""
    # Check common locations
    search_paths = [
        Path("../TinyRecursiveModels"),
        Path("../../swtchx-tools/TinyRecursiveModels"),
        Path(os.environ.get("TRM_REPO_PATH", "")),
        Path.home() / "TinyRecursiveModels",
    ]
    
    for path in search_paths:
        if path.exists() and (path / "models" / "recursive_reasoning" / "trm.py").exists():
            return path.resolve()
    
    return None


def import_trm_model():
    """Import TRM model classes from TinyRecursiveModels repo"""
    trm_repo = find_trm_repo()
    if trm_repo is None:
        print("❌ TinyRecursiveModels repository not found!")
        print("   Please set TRM_REPO_PATH environment variable or clone the repo.")
        sys.exit(1)
    
    print(f"📦 Found TRM repo at: {trm_repo}")
    sys.path.insert(0, str(trm_repo))
    
    from models.recursive_reasoning.trm import (
        TinyRecursiveReasoningModel_ACTV1,
        TinyRecursiveReasoningModel_ACTV1InnerCarry,
        TinyRecursiveReasoningModel_ACTV1Config,
    )
    
    return TinyRecursiveReasoningModel_ACTV1, TinyRecursiveReasoningModel_ACTV1InnerCarry


def create_default_config(variant: str = "default") -> Dict[str, Any]:
    """Create TRM config for different variants"""
    
    configs = {
        "default": {
            "batch_size": 1,
            "seq_len": 1024,
            "vocab_size": 256,
            "hidden_size": 384,
            "expansion": 2.0,
            "num_heads": 6,
            "pos_encodings": "rope",
            "H_cycles": 3,
            "L_cycles": 6,
            "H_layers": 2,
            "L_layers": 2,
            "halt_max_steps": 16,
            "halt_exploration_prob": 0.0,
            "num_puzzle_identifiers": 1000,
            "puzzle_emb_ndim": 0,
            "puzzle_emb_len": 16,
            "rms_norm_eps": 1e-5,
            "rope_theta": 10000.0,
            "forward_dtype": "float32",
            "mlp_t": False,
            "no_ACT_continue": True,
        },
        "arc_agi": {
            "batch_size": 1,
            "seq_len": 900,  # 30x30 max
            "vocab_size": 11,  # 0-9 colors + padding
            "hidden_size": 384,
            "expansion": 2.0,
            "num_heads": 6,
            "pos_encodings": "rope",
            "H_cycles": 3,
            "L_cycles": 4,
            "H_layers": 2,
            "L_layers": 2,
            "halt_max_steps": 16,
            "halt_exploration_prob": 0.0,
            "num_puzzle_identifiers": 1000,
            "puzzle_emb_ndim": 0,
            "puzzle_emb_len": 16,
            "rms_norm_eps": 1e-5,
            "rope_theta": 10000.0,
            "forward_dtype": "float32",
            "mlp_t": False,
            "no_ACT_continue": True,
        },
        "sudoku": {
            "batch_size": 1,
            "seq_len": 81,  # 9x9
            "vocab_size": 10,  # 0-9
            "hidden_size": 384,
            "expansion": 2.0,
            "num_heads": 6,
            "pos_encodings": "rope",
            "H_cycles": 3,
            "L_cycles": 6,
            "H_layers": 2,
            "L_layers": 2,
            "halt_max_steps": 32,
            "halt_exploration_prob": 0.0,
            "num_puzzle_identifiers": 1000,
            "puzzle_emb_ndim": 0,
            "puzzle_emb_len": 16,
            "rms_norm_eps": 1e-5,
            "rope_theta": 10000.0,
            "forward_dtype": "float32",
            "mlp_t": True,  # MLP-T for Sudoku
            "no_ACT_continue": True,
        },
        "maze": {
            "batch_size": 1,
            "seq_len": 900,  # 30x30 max
            "vocab_size": 4,  # wall, path, start, end
            "hidden_size": 384,
            "expansion": 2.0,
            "num_heads": 6,
            "pos_encodings": "rope",
            "H_cycles": 3,
            "L_cycles": 4,
            "H_layers": 2,
            "L_layers": 2,
            "halt_max_steps": 24,
            "halt_exploration_prob": 0.0,
            "num_puzzle_identifiers": 1000,
            "puzzle_emb_ndim": 0,
            "puzzle_emb_len": 16,
            "rms_norm_eps": 1e-5,
            "rope_theta": 10000.0,
            "forward_dtype": "float32",
            "mlp_t": False,
            "no_ACT_continue": True,
        },
    }
    
    return configs.get(variant, configs["default"])


class TRMONNXWrapper(nn.Module):
    """
    Wrapper that exposes TRM inner model for ONNX export.
    
    The TRM model uses stateful inference with carry (z_H, z_L).
    This wrapper takes carry as input and outputs new carry + predictions.
    """
    
    def __init__(self, model, InnerCarryClass):
        super().__init__()
        self.inner = model.inner
        self.config = model.config
        self.InnerCarryClass = InnerCarryClass
        
    def forward(
        self,
        z_h: torch.Tensor,
        z_l: torch.Tensor,
        inputs: torch.Tensor,
        puzzle_ids: torch.Tensor,
    ) -> Tuple[torch.Tensor, torch.Tensor, torch.Tensor, torch.Tensor, torch.Tensor]:
        """
        Single TRM inference step.
        
        Args:
            z_h: H-level latent state [batch, total_seq_len, hidden_size]
            z_l: L-level latent state [batch, total_seq_len, hidden_size]
            inputs: Input tokens [batch, seq_len]
            puzzle_ids: Puzzle identifiers [batch, 1]
            
        Returns:
            new_z_h: Updated H-level state
            new_z_l: Updated L-level state
            logits: Output logits [batch, seq_len, vocab_size]
            q_halt: Q-value for halting
            q_continue: Q-value for continuing
        """
        # Reconstruct carry
        carry = self.InnerCarryClass(
            z_H=z_h,
            z_L=z_l,
        )
        
        # Construct batch dict
        batch = {
            "inputs": inputs,
            "puzzle_identifiers": puzzle_ids.squeeze(-1),
        }
        
        # Run inner model forward
        new_carry, logits, (q_halt, q_continue) = self.inner(carry, batch)
        
        return (
            new_carry.z_H,
            new_carry.z_L,
            logits,
            q_halt.unsqueeze(0),  # Ensure batch dim
            q_continue.unsqueeze(0),
        )


def export_trm_to_onnx(
    checkpoint_path: str,
    output_path: str,
    variant: str = "default",
    opset_version: int = 17,
):
    """
    Export TRM model to ONNX format.
    
    Args:
        checkpoint_path: Path to TRM checkpoint (.pt file)
        output_path: Output ONNX file path
        variant: Model variant (default, arc_agi, sudoku, maze)
        opset_version: ONNX opset version
    """
    print(f"🔧 TRM to ONNX Exporter")
    print(f"   Checkpoint: {checkpoint_path}")
    print(f"   Output: {output_path}")
    print(f"   Variant: {variant}")
    print()
    
    # Import TRM model
    TinyRecursiveReasoningModel_ACTV1, InnerCarryClass = import_trm_model()
    
    # Create config
    config = create_default_config(variant)
    print(f"📋 Model config:")
    print(f"   seq_len: {config['seq_len']}")
    print(f"   hidden_size: {config['hidden_size']}")
    print(f"   vocab_size: {config['vocab_size']}")
    print(f"   H_cycles × L_cycles: {config['H_cycles']} × {config['L_cycles']}")
    print()
    
    # Create model
    model = TinyRecursiveReasoningModel_ACTV1(config)
    
    # Load weights
    if checkpoint_path and Path(checkpoint_path).exists():
        print(f"📦 Loading checkpoint from: {checkpoint_path}")
        state_dict = torch.load(checkpoint_path, map_location="cpu", weights_only=True)
        model.load_state_dict(state_dict, strict=False)
        print("✅ Loaded checkpoint weights")
    else:
        print("⚠️  No checkpoint provided, using random weights")
    
    model.eval()
    
    # Create wrapper for ONNX export
    wrapper = TRMONNXWrapper(model, InnerCarryClass)
    wrapper.eval()
    
    # Create dummy inputs
    batch_size = 1
    seq_len = config["seq_len"]
    puzzle_emb_len = config["puzzle_emb_len"]
    hidden_size = config["hidden_size"]
    vocab_size = config["vocab_size"]
    total_seq_len = seq_len + puzzle_emb_len
    
    print(f"📐 Creating dummy inputs:")
    print(f"   z_h/z_l shape: [{batch_size}, {total_seq_len}, {hidden_size}]")
    print(f"   inputs shape: [{batch_size}, {seq_len}]")
    print()
    
    dummy_z_h = torch.randn(batch_size, total_seq_len, hidden_size, dtype=torch.float32)
    dummy_z_l = torch.randn(batch_size, total_seq_len, hidden_size, dtype=torch.float32)
    dummy_inputs = torch.randint(0, vocab_size, (batch_size, seq_len), dtype=torch.int64)
    dummy_puzzle_ids = torch.zeros(batch_size, 1, dtype=torch.int64)
    
    # Test forward pass
    print("🧪 Testing forward pass...")
    with torch.no_grad():
        new_z_h, new_z_l, logits, q_halt, q_continue = wrapper(
            dummy_z_h, dummy_z_l, dummy_inputs, dummy_puzzle_ids
        )
    print(f"   Output logits shape: {logits.shape}")
    print(f"   Q-halt: {q_halt.item():.4f}")
    print("✅ Forward pass successful")
    print()
    
    # Export to ONNX
    print(f"📤 Exporting to ONNX (opset {opset_version})...")
    
    # Ensure output directory exists
    output_dir = Path(output_path).parent
    output_dir.mkdir(parents=True, exist_ok=True)
    
    torch.onnx.export(
        wrapper,
        (dummy_z_h, dummy_z_l, dummy_inputs, dummy_puzzle_ids),
        output_path,
        input_names=["z_h", "z_l", "inputs", "puzzle_ids"],
        output_names=["new_z_h", "new_z_l", "logits", "q_halt", "q_continue"],
        dynamic_axes={
            "inputs": {0: "batch", 1: "seq_len"},
            "z_h": {0: "batch", 1: "total_seq_len"},
            "z_l": {0: "batch", 1: "total_seq_len"},
            "puzzle_ids": {0: "batch"},
            "new_z_h": {0: "batch", 1: "total_seq_len"},
            "new_z_l": {0: "batch", 1: "total_seq_len"},
            "logits": {0: "batch", 1: "seq_len"},
            "q_halt": {0: "batch"},
            "q_continue": {0: "batch"},
        },
        opset_version=opset_version,
        do_constant_folding=True,
        export_params=True,
    )
    
    # Report file size
    output_size = Path(output_path).stat().st_size
    print()
    print(f"✅ Exported TRM model to: {output_path}")
    print(f"   📊 Model size: {output_size / 1024 / 1024:.2f} MB")
    print()
    
    # Validate ONNX model
    try:
        import onnx
        print("🔍 Validating ONNX model...")
        onnx_model = onnx.load(output_path)
        onnx.checker.check_model(onnx_model)
        print("✅ ONNX model validation passed")
    except ImportError:
        print("⚠️  onnx package not installed, skipping validation")
    except Exception as e:
        print(f"⚠️  ONNX validation warning: {e}")
    
    # Print usage instructions
    print()
    print("=" * 60)
    print("📝 Usage with SpaceKit Compute Node:")
    print("=" * 60)
    print(f"""
1. Copy the ONNX model to your compute node:
   cp {output_path} /path/to/spacekit-compute-node/models/trm/

2. Register the model in your Rust code:
   ```rust
   use spacekit_compute_node::{{TRMInferenceManager, TRMConfig}};
   
   let config = TRMConfig {{
       model_path: PathBuf::from("models/trm/{Path(output_path).name}"),
       seq_len: {seq_len},
       hidden_size: {hidden_size},
       vocab_size: {vocab_size},
       h_cycles: {config["H_cycles"]},
       l_cycles: {config["L_cycles"]},
       max_halt_steps: {config["halt_max_steps"]},
       puzzle_emb_len: {puzzle_emb_len},
   }};
   
   let manager = TRMInferenceManager::new();
   manager.register_model("{variant}", config).await?;
   ```

3. Use via ML Operations API:
   ```json
   {{
       "operation": "trm-recursive-reasoning",
       "input_grid": [[1, 2, 3], [4, 5, 6], [7, 8, 9]],
       "max_steps": 16,
       "model_variant": "{variant}"
   }}
   ```
""")


def main():
    parser = argparse.ArgumentParser(
        description="Export TRM (Tiny Recursive Model) to ONNX for SpaceKit"
    )
    parser.add_argument(
        "--checkpoint",
        type=str,
        required=False,
        default=None,
        help="Path to TRM checkpoint (.pt file)",
    )
    parser.add_argument(
        "--output",
        type=str,
        default="trm_model.onnx",
        help="Output ONNX file path",
    )
    parser.add_argument(
        "--variant",
        type=str,
        choices=["default", "arc_agi", "sudoku", "maze"],
        default="default",
        help="Model variant/configuration",
    )
    parser.add_argument(
        "--opset",
        type=int,
        default=17,
        help="ONNX opset version",
    )
    
    args = parser.parse_args()
    
    export_trm_to_onnx(
        checkpoint_path=args.checkpoint,
        output_path=args.output,
        variant=args.variant,
        opset_version=args.opset,
    )


if __name__ == "__main__":
    main()

