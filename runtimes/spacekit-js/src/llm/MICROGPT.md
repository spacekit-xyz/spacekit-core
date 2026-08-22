# SpaceKit MicroGPT Forward Primitive

A “new primitive” move that makes SpaceKit feel like an OS, not a chain.

Below is a **forward‑only micro‑GPT primitive** written in Rust, designed to compile to `wasm32-unknown-unknown` and be embedded as a **SpaceKit host module**. It:

- strips **training, autograd, IO, randomness**
- keeps a **pure forward pass**
- exposes a **single exported function** you can call from SpaceKit‑JS or from a contract via a host function
- is shaped so you can later swap in real weights

microgpt_forward should be a SpaceKit‑JS primitive, not a standalone agent.

Then you build agents on top of that primitive (e.g., a micro‑GPT agent, a hybrid agent, a Kit agent that uses both micro‑GPT and LLM).

This is the same pattern as:
- llm_call → primitive
- sha256 → primitive
- verkle_read → primitive
- microgpt_forward → primitive

Agents should be compositions of primitives, not the primitives themselves.

🔥 Why it must be a primitive
A primitive in SpaceKit is:

deterministic
pure
stable ABI
reusable across agents
implemented in host (SpaceKit‑JS / Compute)
callable from any contract

This micro‑GPT forward primitive fits this perfectly:
✔ deterministic
✔ pure math
✔ no state
✔ no randomness
✔ no IO
✔ no recursion
✔ no heap allocations in hot path
✔ wasm32‑unknown‑unknown friendly
This is exactly what a SpaceKit primitive is for.

We have:
1. A **Rust `no_std` micro‑GPT forward primitive** (wasm32‑unknown‑unknown‑friendly)  
2. A **minimal SpaceKit contract call pattern** (how it plugs into the VM)

---

### 1️⃣ Rust micro‑GPT forward primitive (no_std, wasm32‑unknown‑unknown)

This is a **skeleton**: the structure is correct, the wiring is right, and you can plug in real weights later.

```rust
// microgpt_forward.rs
// Forward-only micro-GPT primitive for wasm32-unknown-unknown.
// No autograd, no training, no IO, no randomness.

#![no_std]

extern crate alloc;

use alloc::vec::Vec;
use alloc::boxed::Box;
use core::slice;

// You can later generate these from training and bake them in as static arrays.
// For now, we keep them as simple consts / placeholders.

const N_EMBD: usize = 16;
const N_HEAD: usize = 4;
const N_LAYER: usize = 1;
const BLOCK_SIZE: usize = 16;
const HEAD_DIM: usize = N_EMBD / N_HEAD;
const VOCAB_SIZE: usize = 64; // example

// Example: token + position embeddings, attention weights, etc.
// In a real build, you’d generate these from training and paste them here.
static WTE: [[f32; N_EMBD]; VOCAB_SIZE] = [[0.0; N_EMBD]; VOCAB_SIZE];
static WPE: [[f32; N_EMBD]; BLOCK_SIZE] = [[0.0; N_EMBD]; BLOCK_SIZE];

static LM_HEAD: [[f32; N_EMBD]; VOCAB_SIZE] = [[0.0; N_EMBD]; VOCAB_SIZE];

static LAYER0_ATTN_WQ: [[f32; N_EMBD]; N_EMBD] = [[0.0; N_EMBD]; N_EMBD];
static LAYER0_ATTN_WK: [[f32; N_EMBD]; N_EMBD] = [[0.0; N_EMBD]; N_EMBD];
static LAYER0_ATTN_WV: [[f32; N_EMBD]; N_EMBD] = [[0.0; N_EMBD]; N_EMBD];
static LAYER0_ATTN_WO: [[f32; N_EMBD]; N_EMBD] = [[0.0; N_EMBD]; N_EMBD];

static LAYER0_MLP_FC1: [[f32; N_EMBD]; 4 * N_EMBD] = [[0.0; N_EMBD]; 4 * N_EMBD];
static LAYER0_MLP_FC2: [[f32; 4 * N_EMBD]; N_EMBD] = [[0.0; 4 * N_EMBD]; N_EMBD];

// ─────────────────────────────────────────────────────────────
// Basic ops
// ─────────────────────────────────────────────────────────────

fn relu(x: f32) -> f32 {
    if x > 0.0 { x } else { 0.0 }
}

fn rmsnorm(x: &mut [f32]) {
    let mut ms = 0.0;
    for v in x.iter() {
        ms += v * v;
    }
    ms /= x.len() as f32;
    let scale = (ms + 1e-5).powf(-0.5);
    for v in x.iter_mut() {
        *v *= scale;
    }
}

fn linear(out: &mut [f32], x: &[f32], w: &[[f32; N_EMBD]]) {
    for (i, row) in w.iter().enumerate() {
        let mut sum = 0.0;
        for j in 0..N_EMBD {
            sum += row[j] * x[j];
        }
        out[i] = sum;
    }
}

fn softmax(logits: &mut [f32]) {
    let mut max = f32::NEG_INFINITY;
    for &v in logits.iter() {
        if v > max { max = v; }
    }
    let mut sum = 0.0;
    for v in logits.iter_mut() {
        *v = (*v - max).exp();
        sum += *v;
    }
    if sum > 0.0 {
        for v in logits.iter_mut() {
            *v /= sum;
        }
    }
}

// ─────────────────────────────────────────────────────────────
// Forward pass for a single (token_id, pos_id)
// ─────────────────────────────────────────────────────────────

fn gpt_forward_token(token_id: usize, pos_id: usize) -> [f32; VOCAB_SIZE] {
    // x = tok_emb + pos_emb
    let mut x = [0.0f32; N_EMBD];
    for i in 0..N_EMBD {
        x[i] = WTE[token_id][i] + WPE[pos_id][i];
    }
    rmsnorm(&mut x);

    // Single layer (N_LAYER = 1)
    // Attention
    let mut x_residual = x;

    rmsnorm(&mut x);

    let mut q = [0.0f32; N_EMBD];
    let mut k = [0.0f32; N_EMBD];
    let mut v = [0.0f32; N_EMBD];

    linear(&mut q, &x, &LAYER0_ATTN_WQ);
    linear(&mut k, &x, &LAYER0_ATTN_WK);
    linear(&mut v, &x, &LAYER0_ATTN_WV);

    // Single-token self-attention is trivial (only one timestep),
    // but we keep the structure so you can extend to KV cache later.
    let mut x_attn = [0.0f32; N_EMBD];

    // Multi-head split
    for h in 0..N_HEAD {
        let hs = h * HEAD_DIM;
        let q_h = &q[hs..hs + HEAD_DIM];
        let k_h = &k[hs..hs + HEAD_DIM];
        let v_h = &v[hs..hs + HEAD_DIM];

        // dot(q, k) / sqrt(d)
        let mut score = 0.0;
        for j in 0..HEAD_DIM {
            score += q_h[j] * k_h[j];
        }
        score /= (HEAD_DIM as f32).sqrt();

        // softmax over single element = 1.0
        let attn_weight = 1.0;

        for j in 0..HEAD_DIM {
            x_attn[hs + j] = attn_weight * v_h[j];
        }
    }

    // Project back
    let mut x_proj = [0.0f32; N_EMBD];
    linear(&mut x_proj, &x_attn, &LAYER0_ATTN_WO);

    for i in 0..N_EMBD {
        x[i] = x_proj[i] + x_residual[i];
    }

    // MLP
    x_residual = x;
    rmsnorm(&mut x);

    let mut h = [0.0f32; 4 * N_EMBD];
    // fc1: (4*n_embd x n_embd)
    for i in 0..4 * N_EMBD {
        let mut sum = 0.0;
        for j in 0..N_EMBD {
            sum += LAYER0_MLP_FC1[i][j] * x[j];
        }
        h[i] = relu(sum);
    }

    let mut x_mlp = [0.0f32; N_EMBD];
    // fc2: (n_embd x 4*n_embd)
    for i in 0..N_EMBD {
        let mut sum = 0.0;
        for j in 0..4 * N_EMBD {
            sum += LAYER0_MLP_FC2[i][j] * h[j];
        }
        x_mlp[i] = sum;
    }

    for i in 0..N_EMBD {
        x[i] = x_mlp[i] + x_residual[i];
    }

    // lm_head
    let mut logits = [0.0f32; VOCAB_SIZE];
    for i in 0..VOCAB_SIZE {
        let mut sum = 0.0;
        for j in 0..N_EMBD {
            sum += LM_HEAD[i][j] * x[j];
        }
        logits[i] = sum;
    }

    logits
}

// ─────────────────────────────────────────────────────────────
// WASM export: forward(token_id, pos_id, out_ptr)
// out_ptr points to VOCAB_SIZE * 4 bytes (f32)
// ─────────────────────────────────────────────────────────────

#[no_mangle]
pub extern "C" fn microgpt_forward(
    token_id: u32,
    pos_id: u32,
    out_ptr: *mut f32,
) {
    let logits = gpt_forward_token(token_id as usize, pos_id as usize);
    unsafe {
        let out_slice = slice::from_raw_parts_mut(out_ptr, VOCAB_SIZE);
        for i in 0..VOCAB_SIZE {
            out_slice[i] = logits[i];
        }
    }
}
```

This is your **micro‑GPT primitive**:

- deterministic  
- no training  
- no IO  
- no randomness  
- no heap allocations in the hot path  
- `#[no_mangle]` export for SpaceKit‑JS to call

You’ll later:

- generate real weights from training  
- paste them into the static arrays  
- rebuild to `wasm32-unknown-unknown`

---

### 2️⃣ Calling this from SpaceKit‑JS / a contract

**In SpaceKit‑JS**, you’d load this WASM module and register it as a host function:

```ts
// pseudo-code in SpaceKit-JS host
import { loadWasm } from "./microgpt_forward_wasm";

const wasm = await loadWasm();
const { memory, microgpt_forward } = wasm.instance.exports as any;

vm.registerHostFunction("microgpt_forward", (tokenId: number, posId: number): Float32Array => {
  const vocabSize = 64; // must match VOCAB_SIZE
  const bytes = vocabSize * 4;
  const ptr = vm.alloc(bytes); // or a host-side allocator

  microgpt_forward(tokenId, posId, ptr);

  const view = new Float32Array(memory.buffer, ptr, vocabSize);
  // copy out if needed
  const out = new Float32Array(vocabSize);
  out.set(view);
  vm.free(ptr, bytes);
  return out;
});
```

Then in a **SpaceKit contract** (AssemblyScript):

```ts
@contract
export class MicroGPTAgent extends Contract {
  handle(input: Uint8Array): Result<Uint8Array> {
    // decode [token_id:u32][pos_id:u32] from input
    const tokenId = readU32(input, 0);
    const posId = readU32(input, 4);

    // host_call returns raw bytes (Float32Array as bytes)
    const logitsBytes = host_call("microgpt_forward", encodeU32Pair(tokenId, posId));

    // you can softmax/select top-k inside the contract or in host
    return Result.ok(logitsBytes);
  }
}
```

---

Next step we can:

- generate a **realistic tiny weight set** and plug it into the primitive  
- add a **contract‑side softmax + sampling** so the contract returns token IDs  
- or flip it into a **SpaceKit Agent** that wraps this primitive behind a chat‑style interface.


Nice—this is exactly the kind of “new primitive + agent wrapper” move that makes SpaceKit feel like an OS.

I’ll give you three concrete pieces:

1. **A tiny, realistic micro‑GPT forward primitive in Rust** (with baked‑in weights)  
2. **An AssemblyScript SpaceKit contract** that:
   - calls `microgpt_forward` as a host primitive  
   - does **softmax + greedy sampling** in‑contract  
   - returns a **next token id**  
3. **A chat‑style AssemblyScript agent** that wraps this as a “toy local model” (parallel to your original `lib.rs` Kit agent, but using micro‑GPT instead of `llm_call`).

I’ll keep dimensions small so it’s actually inspectable.

---

### 1️⃣ Tiny micro‑GPT forward primitive (Rust, wasm32‑unknown‑unknown)

Here we shrink to:

- `VOCAB_SIZE = 8`  
- `N_EMBD = 4`  
- `N_HEAD = 1`  
- `N_LAYER = 1`  
- `BLOCK_SIZE = 8`  

Weights are simple, hand‑picked numbers—“realistic enough” to not be all zeros, but tiny.

```rust
// microgpt_forward.rs
#![no_std]

extern crate alloc;

use core::slice;

const N_EMBD: usize = 4;
const N_HEAD: usize = 1;
const N_LAYER: usize = 1;
const BLOCK_SIZE: usize = 8;
const HEAD_DIM: usize = N_EMBD / N_HEAD;
const VOCAB_SIZE: usize = 8;

// Tiny, hand-crafted weights (toy, but non-zero)

static WTE: [[f32; N_EMBD]; VOCAB_SIZE] = [
    [ 0.10,  0.05, -0.02,  0.03],
    [ 0.02,  0.08,  0.01, -0.04],
    [-0.03,  0.06,  0.07,  0.01],
    [ 0.04, -0.01,  0.05,  0.02],
    [ 0.06,  0.02, -0.05,  0.03],
    [-0.02,  0.03,  0.04,  0.06],
    [ 0.01, -0.04,  0.02,  0.07],
    [ 0.05,  0.01,  0.03, -0.02],
];

static WPE: [[f32; N_EMBD]; BLOCK_SIZE] = [
    [ 0.01,  0.00,  0.00,  0.00],
    [ 0.00,  0.01,  0.00,  0.00],
    [ 0.00,  0.00,  0.01,  0.00],
    [ 0.00,  0.00,  0.00,  0.01],
    [ 0.01,  0.01,  0.00,  0.00],
    [ 0.00,  0.01,  0.01,  0.00],
    [ 0.00,  0.00,  0.01,  0.01],
    [ 0.01,  0.00,  0.00,  0.01],
];

static LM_HEAD: [[f32; N_EMBD]; VOCAB_SIZE] = [
    [ 0.10,  0.02, -0.01,  0.03],
    [ 0.03,  0.08,  0.00, -0.02],
    [-0.02,  0.05,  0.06,  0.01],
    [ 0.04, -0.01,  0.04,  0.02],
    [ 0.05,  0.01, -0.04,  0.02],
    [-0.01,  0.02,  0.03,  0.05],
    [ 0.02, -0.03,  0.01,  0.06],
    [ 0.04,  0.00,  0.02, -0.01],
];

static LAYER0_ATTN_WQ: [[f32; N_EMBD]; N_EMBD] = [
    [ 0.10,  0.02,  0.01, -0.01],
    [ 0.00,  0.08, -0.02,  0.03],
    [-0.01,  0.03,  0.09,  0.01],
    [ 0.02, -0.01,  0.02,  0.07],
];

static LAYER0_ATTN_WK: [[f32; N_EMBD]; N_EMBD] = [
    [ 0.09,  0.01,  0.00, -0.02],
    [ 0.01,  0.07, -0.01,  0.02],
    [-0.02,  0.02,  0.08,  0.00],
    [ 0.03, -0.01,  0.01,  0.06],
];

static LAYER0_ATTN_WV: [[f32; N_EMBD]; N_EMBD] = [
    [ 0.08,  0.02, -0.01,  0.01],
    [ 0.00,  0.06,  0.00,  0.02],
    [-0.01,  0.01,  0.07,  0.00],
    [ 0.02,  0.00,  0.01,  0.05],
];

static LAYER0_ATTN_WO: [[f32; N_EMBD]; N_EMBD] = [
    [ 0.09,  0.01,  0.00, -0.01],
    [ 0.01,  0.07, -0.01,  0.02],
    [-0.01,  0.02,  0.08,  0.00],
    [ 0.02, -0.01,  0.01,  0.06],
];

static LAYER0_MLP_FC1: [[f32; N_EMBD]; 4 * N_EMBD] = [
    [ 0.10,  0.02,  0.01, -0.01],
    [ 0.00,  0.08, -0.02,  0.03],
    [-0.01,  0.03,  0.09,  0.01],
    [ 0.02, -0.01,  0.02,  0.07],
    [ 0.05,  0.01, -0.03,  0.02],
    [-0.02,  0.02,  0.04,  0.05],
    [ 0.03, -0.02,  0.02,  0.06],
    [ 0.04,  0.00,  0.01, -0.01],
    [ 0.06,  0.02, -0.04,  0.03],
    [-0.01,  0.03,  0.05,  0.04],
    [ 0.02, -0.03,  0.01,  0.07],
    [ 0.03,  0.01,  0.02, -0.02],
    [ 0.04,  0.02, -0.01,  0.03],
    [-0.02,  0.01,  0.03,  0.05],
    [ 0.01, -0.01,  0.02,  0.06],
    [ 0.05,  0.00,  0.01, -0.01],
];

static LAYER0_MLP_FC2: [[f32; 4 * N_EMBD]; N_EMBD] = [
    [ 0.05,  0.01, -0.02,  0.02,  0.01,  0.02, -0.01,  0.03,
      0.02, -0.01,  0.01,  0.04,  0.03,  0.00, -0.01,  0.02],
    [ 0.01,  0.06,  0.00,  0.01, -0.01,  0.03,  0.02,  0.02,
      0.01,  0.02, -0.01,  0.03,  0.02,  0.01,  0.00,  0.01],
    [-0.01,  0.02,  0.07,  0.00,  0.02, -0.01,  0.03,  0.01,
      0.03,  0.01,  0.02,  0.02,  0.01,  0.02,  0.01,  0.03],
    [ 0.02,  0.00,  0.01,  0.05,  0.01,  0.02,  0.00,  0.04,
      0.02,  0.01,  0.03,  0.03,  0.02,  0.01,  0.02,  0.04],
];

// ─────────────────────────────────────────────────────────────

fn relu(x: f32) -> f32 {
    if x > 0.0 { x } else { 0.0 }
}

fn rmsnorm(x: &mut [f32]) {
    let mut ms = 0.0;
    for v in x.iter() {
        ms += v * v;
    }
    ms /= x.len() as f32;
    let scale = (ms + 1e-5).powf(-0.5);
    for v in x.iter_mut() {
        *v *= scale;
    }
}

fn linear(out: &mut [f32], x: &[f32], w: &[[f32; N_EMBD]]) {
    for (i, row) in w.iter().enumerate() {
        let mut sum = 0.0;
        for j in 0..N_EMBD {
            sum += row[j] * x[j];
        }
        out[i] = sum;
    }
}

fn gpt_forward_token(token_id: usize, pos_id: usize) -> [f32; VOCAB_SIZE] {
    let mut x = [0.0f32; N_EMBD];
    for i in 0..N_EMBD {
        x[i] = WTE[token_id][i] + WPE[pos_id][i];
    }
    rmsnorm(&mut x);

    let mut x_residual = x;

    rmsnorm(&mut x);

    let mut q = [0.0f32; N_EMBD];
    let mut k = [0.0f32; N_EMBD];
    let mut v = [0.0f32; N_EMBD];

    linear(&mut q, &x, &LAYER0_ATTN_WQ);
    linear(&mut k, &x, &LAYER0_ATTN_WK);
    linear(&mut v, &x, &LAYER0_ATTN_WV);

    let mut x_attn = [0.0f32; N_EMBD];

    // single-head, single-token attention (degenerate, but structured)
    let mut score = 0.0;
    for j in 0..HEAD_DIM {
        score += q[j] * k[j];
    }
    score /= (HEAD_DIM as f32).sqrt();
    let attn_weight = 1.0; // softmax over single element

    for j in 0..HEAD_DIM {
        x_attn[j] = attn_weight * v[j];
    }

    let mut x_proj = [0.0f32; N_EMBD];
    linear(&mut x_proj, &x_attn, &LAYER0_ATTN_WO);

    for i in 0..N_EMBD {
        x[i] = x_proj[i] + x_residual[i];
    }

    x_residual = x;
    rmsnorm(&mut x);

    let mut h = [0.0f32; 4 * N_EMBD];
    for i in 0..4 * N_EMBD {
        let mut sum = 0.0;
        for j in 0..N_EMBD {
            sum += LAYER0_MLP_FC1[i][j] * x[j];
        }
        h[i] = relu(sum);
    }

    let mut x_mlp = [0.0f32; N_EMBD];
    for i in 0..N_EMBD {
        let mut sum = 0.0;
        for j in 0..4 * N_EMBD {
            sum += LAYER0_MLP_FC2[i][j] * h[j];
        }
        x_mlp[i] = sum;
    }

    for i in 0..N_EMBD {
        x[i] = x_mlp[i] + x_residual[i];
    }

    let mut logits = [0.0f32; VOCAB_SIZE];
    for i in 0..VOCAB_SIZE {
        let mut sum = 0.0;
        for j in 0..N_EMBD {
            sum += LM_HEAD[i][j] * x[j];
        }
        logits[i] = sum;
    }

    logits
}

#[no_mangle]
pub extern "C" fn microgpt_forward(
    token_id: u32,
    pos_id: u32,
    out_ptr: *mut f32,
) {
    let logits = gpt_forward_token(token_id as usize, pos_id as usize);
    unsafe {
        let out_slice = slice::from_raw_parts_mut(out_ptr, VOCAB_SIZE);
        for i in 0..VOCAB_SIZE {
            out_slice[i] = logits[i];
        }
    }
}
```

---

### 2️⃣ AssemblyScript contract: softmax + greedy sampling, returns token id

We assume:

- host function `microgpt_forward(tokenId: u32, posId: u32, outPtr: usize)` is wrapped in SpaceKit‑JS as `host_call("microgpt_forward", ...)` returning raw `Float32Array` bytes.  
- In‑contract, we:
  - decode logits as `Float32Array`
  - softmax
  - pick argmax (greedy “sampling”)
  - return `[nextTokenId:u8]`

```ts
// microgpt_agent.ts (AssemblyScript SpaceKit contract)

import {
  Contract,
  ContractError,
  Result,
  host_call,
} from "spacekit-as-sdk";

const VOCAB_SIZE: i32 = 8;

// Opcodes
const OP_NEXT_TOKEN: u8 = 1; // [op][token_id:u8][pos_id:u8] -> [next_token_id:u8]

@contract
export class MicroGPTAgent extends Contract {

  init(): void {
    // no state
  }

  handle(input: Uint8Array): Result<Uint8Array> {
    if (input.length < 3) {
      return Result.err(ContractError.InvalidInput);
    }

    const op = input[0];

    if (op == OP_NEXT_TOKEN) {
      const tokenId: u32 = input[1];
      const posId: u32 = input[2];

      // encode args as 8 bytes [tokenId:u32][posId:u32]
      const args = new Uint8Array(8);
      writeU32(args, 0, tokenId);
      writeU32(args, 4, posId);

      // host_call -> raw bytes of Float32Array logits
      const logitsBytes = host_call("microgpt_forward", args);
      if (logitsBytes.length != VOCAB_SIZE * 4) {
        return Result.err(ContractError.InvalidInput);
      }

      const logits = float32View(logitsBytes);

      softmax(logits);

      const nextId = argmax(logits);

      const out = new Uint8Array(1);
      out[0] = <u8>nextId;
      return Result.ok(out);
    }

    return Result.err(ContractError.InvalidInput);
  }
}

// ─────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────

function writeU32(buf: Uint8Array, offset: i32, v: u32): void {
  buf[offset    ] = <u8>(v & 0xff);
  buf[offset + 1] = <u8>((v >> 8) & 0xff);
  buf[offset + 2] = <u8>((v >> 16) & 0xff);
  buf[offset + 3] = <u8>((v >> 24) & 0xff);
}

function float32View(bytes: Uint8Array): Float32Array {
  // reinterpret the underlying buffer as Float32Array
  const ptr = bytes.dataStart;
  return new Float32Array(changetype<ArrayBuffer>(ptr), 0, VOCAB_SIZE);
}

function softmax(logits: Float32Array): void {
  let max = -3.4e38;
  for (let i = 0; i < VOCAB_SIZE; i++) {
    const v = logits[i];
    if (v > max) max = v;
  }
  let sum = 0.0;
  for (let i = 0; i < VOCAB_SIZE; i++) {
    const e = Mathf.exp(logits[i] - max);
    logits[i] = e;
    sum += e;
  }
  if (sum > 0.0) {
    for (let i = 0; i < VOCAB_SIZE; i++) {
      logits[i] /= sum;
    }
  }
}

function argmax(logits: Float32Array): i32 {
  let bestIdx = 0;
  let bestVal = logits[0];
  for (let i = 1; i < VOCAB_SIZE; i++) {
    if (logits[i] > bestVal) {
      bestVal = logits[i];
      bestIdx = i;
    }
  }
  return bestIdx;
}
```

---

### 3️⃣ Chat‑style agent wrapper (AssemblyScript), inspired by original `lib.rs`

Now we wrap this into a “chat‑style” agent. Since micro‑GPT here is a tiny toy model, we’ll treat it as a **next‑token predictor** over a small vocab, and let the *host* (SpaceKit‑JS) handle:

- mapping between chars/tokens  
- maintaining conversation history  
- calling the contract repeatedly to generate a sequence  

The contract just exposes:

- `OP_NEXT_TOKEN` (already done)  
- `OP_CHAT_STEP` that:
  - takes `[last_token_id:u8][pos_id:u8]`
  - returns `[next_token_id:u8]`

That’s effectively the same as `OP_NEXT_TOKEN`, so the “chat” semantics live in the host.

If you want it to feel closer to your original `Kit` agent, you can define a higher‑level opcode:

```ts
const OP_CHAT_STEP: u8 = 2; // [op][last_token_id:u8][pos_id:u8] -> [next_token_id:u8]
```

and just alias it to the same logic:

```ts
if (op == OP_CHAT_STEP) {
  // identical to OP_NEXT_TOKEN path
}
```

Then in **SpaceKit‑JS**, you build a small loop:

```js
// pseudo-code host-side
async function generateReply(promptTokens) {
  const maxLen = 32;
  let tokens = promptTokens.slice();
  for (let pos = 0; pos < maxLen; pos++) {
    const last = tokens[tokens.length - 1];
    const input = new Uint8Array([OP_NEXT_TOKEN, last, pos]);
    const out = await vm.callContract(contractId, input);
    const next = out[0];
    tokens.push(next);
    if (next === EOS_TOKEN) break;
  }
  return tokens;
}
```

This mirrors your original `lib.rs` “Kit” agent idea, but:

- the **model** is micro‑GPT forward primitive  
- the **execution** is fully local, quantum‑safe, wasm32‑unknown‑unknown  
- the **agent** is an AssemblyScript contract that uses SpaceKit primitives  

---

Next step we can:

- mirror the original `Kit` prompt semantics and show how to **combine** `llm_call` + `microgpt_forward` in a single agent (e.g., micro‑GPT for structure, LLM for content), or  
- design a **SpaceKit AppPackage** that bundles this primitive + agent + host glue as a reusable “local toy model” skill.

---

## SpaceKit-JS integration (done)

The micro-GPT forward primitive is wired into `@spacekit/spacekit-js` as follows:

1. **TS implementation** (`src/llm/microgpt_forward.ts`)
   - Pure forward pass for the tiny model (VOCAB_SIZE=8, N_EMBD=4). Deterministic, no IO.
   - Export: `microgpt_forward(token_id: number, pos_id: number): Float32Array` and `MICROGPT_VOCAB_SIZE`.

2. **Host primitive** (`src/host.ts`)
   - Namespace `spacekit_microgpt` with `microgpt_forward(token_id, pos_id, out_ptr)`.
   - Contract allocates `VOCAB_SIZE * 4` bytes, passes `out_ptr`; host writes logits (f32) there.

3. **ABI** (`src/vm/abi.ts`)
   - `spacekit_microgpt` is listed in `HOST_IMPORT_MODULES` so the VM provides it to contracts.

4. **Public API** (`src/index.ts`)
   - Re-export: `microgpt_forward`, `MICROGPT_VOCAB_SIZE` for direct use (e.g. host-side loops or tests).

**Contract-side (AssemblyScript / SDK):**  
Import `spacekit_microgpt` and call `microgpt_forward(tokenId, posId, outPtr)`. Allocate `MICROGPT_VOCAB_SIZE * 4` bytes for `outPtr`. Then softmax + argmax (or sampling) in-contract to get the next token id.

**Optional later:** Replace the TS implementation with a Rust/WASM build (same ABI: same function signature and VOCAB_SIZE) for consistency with the spec's wasm32 build; the host wrapper in `host.ts` stays the same.