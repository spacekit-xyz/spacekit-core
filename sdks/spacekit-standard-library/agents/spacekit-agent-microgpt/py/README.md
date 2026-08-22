# **agent-microgpt — Python Training Pipeline**

This directory contains the Python training code used to generate weights for the **microGPT** primitive.  
The resulting weights are exported in Rust array format and pasted directly into the `microgpt_forward.rs` primitive used by the SpaceKit agent.

microGPT is a tiny, deterministic GPT‑style model trained on a **tool‑call DSL**, enabling local, on‑device routing for SpaceKit agents.

---

## **📦 Setup**

Create a virtual environment and install dependencies:

```bash
python -m venv .venv
source .venv/bin/activate
pip install -r requirements.txt
```

---

## **🧠 Training the tiny GPT**

To train the model on the tool‑call DSL:

```bash
python tiny_tool_gpt_train.py
```

This will:

- generate a synthetic dataset of tool‑call sequences  
- train a minimal GPT (1 layer, 1 head, embd=4)  
- print Rust‑formatted weight arrays to stdout  

---

## **📤 Exporting weights for the Rust primitive**

To save the exported weights into a file:

```bash
python tiny_tool_gpt_train.py > rust_weights.txt
```

The resulting `rust_weights.txt` file contains:

- `WTE`
- `WPE`
- `LM_HEAD`
- `LAYER0_ATTN_WQ`
- `LAYER0_ATTN_WK`
- `LAYER0_ATTN_WV`
- `LAYER0_ATTN_WO`
- `LAYER0_MLP_FC1`
- `LAYER0_MLP_FC2`

Paste these arrays directly into your `microgpt_forward.rs` implementation.

---

## **🧩 DSL Used for Training**

The tiny GPT is trained on a compressed tool‑call DSL:

| Token | Meaning        |
|-------|----------------|
| `0`   | `search`       |
| `1`   | `summarize`    |
| `2`   | `classify`     |
| `3`   | `code_review`  |
| `4`   | `arg_start`    |
| `5`   | `arg_end`      |
| `6`   | `sep`          |
| `7`   | `eos`          |
| `8`   | `pad` (training only) |

The model learns to generate sequences like:

```
search(arg) → summarize(arg)
classify(arg)
code_review(arg)
search(arg) → classify(arg)
```

These sequences are later decoded by the SpaceKit agent to determine which tool to call.

---
## **🔗 Embedding Weights into the Rust Primitive**

Open your Rust primitive:
```rust
microgpt_forward.rs
```

Replace the placeholder arrays with the generated arrays from rust_weights.txt.

Example:
```rust
// Paste these into microgpt_forward.rs
static WTE: [[f32; 4]; 9] = [
    [ 0.12345, -0.04421, ... ],
    ...
];
```

Rebuild your primitive:

```bash
cargo build --target wasm32-unknown-unknown --release
```

---

## **🛠️ Integration with SpaceKit**

Once weights are pasted into the Rust primitive:

- `microgpt_forward` becomes a **SpaceKit‑JS primitive**
- The AssemblyScript agent calls it to generate next‑token predictions
- The agent decodes the token sequence into a **tool call**
- The tool is executed (search, summarize, classify, code_review, etc.)

This creates a **deterministic, local, DSPy‑style tool‑calling policy**.

In your SpaceKit‑JS host:

```ts
import { loadWasm } from "./microgpt_forward_wasm.js";

const wasm = await loadWasm();
const { memory, microgpt_forward } = wasm.instance.exports;

vm.registerHostFunction("microgpt_forward", (tokenId, posId) => {
  const vocab = 9;
  const bytes = vocab * 4;
  const ptr = vm.alloc(bytes);

  microgpt_forward(tokenId, posId, ptr);

  const view = new Float32Array(memory.buffer, ptr, vocab);
  const out = new Float32Array(vocab);
  out.set(view);

  vm.free(ptr, bytes);
  return out;
});
```
Now the primitive is available to all SpaceKit agents.

---

## **📁 Files**

```
tiny_tool_gpt_train.py   # training script
requirements.txt         # Python dependencies
rust_weights.txt         # (generated) Rust arrays for microgpt_forward.rs
```
