// microgpt_forward.ts
// Forward-only micro-GPT primitive for SpaceKit-JS.
// No autograd, no training, no IO, no randomness.
// Matches MICROGPT.md: deterministic, pure, stable ABI, callable from contracts.

/** Vocabulary size (must match contract / WASM if you add Rust impl later). */
export const MICROGPT_VOCAB_SIZE = 8;

const N_EMBD = 4;
const N_HEAD = 1;
const HEAD_DIM = N_EMBD / N_HEAD;
const VOCAB_SIZE = 8;
const BLOCK_SIZE = 8;

// Tiny, hand-crafted weights (toy, but non-zero) — from MICROGPT.md
const WTE: number[][] = [
  [0.1, 0.05, -0.02, 0.03],
  [0.02, 0.08, 0.01, -0.04],
  [-0.03, 0.06, 0.07, 0.01],
  [0.04, -0.01, 0.05, 0.02],
  [0.06, 0.02, -0.05, 0.03],
  [-0.02, 0.03, 0.04, 0.06],
  [0.01, -0.04, 0.02, 0.07],
  [0.05, 0.01, 0.03, -0.02],
];

const WPE: number[][] = [
  [0.01, 0.0, 0.0, 0.0],
  [0.0, 0.01, 0.0, 0.0],
  [0.0, 0.0, 0.01, 0.0],
  [0.0, 0.0, 0.0, 0.01],
  [0.01, 0.01, 0.0, 0.0],
  [0.0, 0.01, 0.01, 0.0],
  [0.0, 0.0, 0.01, 0.01],
  [0.01, 0.0, 0.0, 0.01],
];

const LM_HEAD: number[][] = [
  [0.1, 0.02, -0.01, 0.03],
  [0.03, 0.08, 0.0, -0.02],
  [-0.02, 0.05, 0.06, 0.01],
  [0.04, -0.01, 0.04, 0.02],
  [0.05, 0.01, -0.04, 0.02],
  [-0.01, 0.02, 0.03, 0.05],
  [0.02, -0.03, 0.01, 0.06],
  [0.04, 0.0, 0.02, -0.01],
];

const LAYER0_ATTN_WQ: number[][] = [
  [0.1, 0.02, 0.01, -0.01],
  [0.0, 0.08, -0.02, 0.03],
  [-0.01, 0.03, 0.09, 0.01],
  [0.02, -0.01, 0.02, 0.07],
];

const LAYER0_ATTN_WK: number[][] = [
  [0.09, 0.01, 0.0, -0.02],
  [0.01, 0.07, -0.01, 0.02],
  [-0.02, 0.02, 0.08, 0.0],
  [0.03, -0.01, 0.01, 0.06],
];

const LAYER0_ATTN_WV: number[][] = [
  [0.08, 0.02, -0.01, 0.01],
  [0.0, 0.06, 0.0, 0.02],
  [-0.01, 0.01, 0.07, 0.0],
  [0.02, 0.0, 0.01, 0.05],
];

const LAYER0_ATTN_WO: number[][] = [
  [0.09, 0.01, 0.0, -0.01],
  [0.01, 0.07, -0.01, 0.02],
  [-0.01, 0.02, 0.08, 0.0],
  [0.02, -0.01, 0.01, 0.06],
];

const LAYER0_MLP_FC1: number[][] = [
  [0.1, 0.02, 0.01, -0.01],
  [0.0, 0.08, -0.02, 0.03],
  [-0.01, 0.03, 0.09, 0.01],
  [0.02, -0.01, 0.02, 0.07],
  [0.05, 0.01, -0.03, 0.02],
  [-0.02, 0.02, 0.04, 0.05],
  [0.03, -0.02, 0.02, 0.06],
  [0.04, 0.0, 0.01, -0.01],
  [0.06, 0.02, -0.04, 0.03],
  [-0.01, 0.03, 0.05, 0.04],
  [0.02, -0.03, 0.01, 0.07],
  [0.03, 0.01, 0.02, -0.02],
  [0.04, 0.02, -0.01, 0.03],
  [-0.02, 0.01, 0.03, 0.05],
  [0.01, -0.01, 0.02, 0.06],
  [0.05, 0.0, 0.01, -0.01],
];

const LAYER0_MLP_FC2: number[][] = [
  [
    0.05, 0.01, -0.02, 0.02, 0.01, 0.02, -0.01, 0.03, 0.02, -0.01, 0.01, 0.04,
    0.03, 0.0, -0.01, 0.02,
  ],
  [
    0.01, 0.06, 0.0, 0.01, -0.01, 0.03, 0.02, 0.02, 0.01, 0.02, -0.01, 0.03,
    0.02, 0.01, 0.0, 0.01,
  ],
  [
    -0.01, 0.02, 0.07, 0.0, 0.02, -0.01, 0.03, 0.01, 0.03, 0.01, 0.02, 0.02,
    0.01, 0.02, 0.01, 0.03,
  ],
  [
    0.02, 0.0, 0.01, 0.05, 0.01, 0.02, 0.0, 0.04, 0.02, 0.01, 0.03, 0.03,
    0.02, 0.01, 0.02, 0.04,
  ],
];

function relu(x: number): number {
  return x > 0 ? x : 0;
}

function rmsnorm(x: number[]): void {
  let ms = 0;
  for (let i = 0; i < x.length; i++) ms += x[i] * x[i];
  ms /= x.length;
  const scale = (ms + 1e-5) ** -0.5;
  for (let i = 0; i < x.length; i++) x[i] *= scale;
}

function linear(
  out: number[],
  x: number[],
  w: number[][],
  inDim: number,
  outDim: number
): void {
  for (let i = 0; i < outDim; i++) {
    let sum = 0;
    for (let j = 0; j < inDim; j++) sum += w[i][j] * x[j];
    out[i] = sum;
  }
}

/**
 * Pure forward pass for one (token_id, pos_id). Returns logits of shape [VOCAB_SIZE].
 * Deterministic, no IO, no randomness — suitable as a SpaceKit host primitive.
 */
export function microgpt_forward(token_id: number, pos_id: number): Float32Array {
  const tid = Math.max(0, Math.min(token_id | 0, VOCAB_SIZE - 1));
  const pid = Math.max(0, Math.min(pos_id | 0, BLOCK_SIZE - 1));

  const x = new Array<number>(N_EMBD);
  for (let i = 0; i < N_EMBD; i++) x[i] = WTE[tid][i] + WPE[pid][i];
  rmsnorm(x);

  const xResidual = x.slice();

  rmsnorm(x);

  const q = new Array<number>(N_EMBD);
  const k = new Array<number>(N_EMBD);
  const v = new Array<number>(N_EMBD);
  linear(q, x, LAYER0_ATTN_WQ, N_EMBD, N_EMBD);
  linear(k, x, LAYER0_ATTN_WK, N_EMBD, N_EMBD);
  linear(v, x, LAYER0_ATTN_WV, N_EMBD, N_EMBD);

  const xAttn = new Array<number>(N_EMBD);
  let score = 0;
  for (let j = 0; j < HEAD_DIM; j++) score += q[j] * k[j];
  score /= Math.sqrt(HEAD_DIM);
  const attnWeight = 1.0;
  for (let j = 0; j < HEAD_DIM; j++) xAttn[j] = attnWeight * v[j];

  const xProj = new Array<number>(N_EMBD);
  linear(xProj, xAttn, LAYER0_ATTN_WO, N_EMBD, N_EMBD);
  for (let i = 0; i < N_EMBD; i++) x[i] = xProj[i] + xResidual[i];

  const xResidual2 = x.slice();
  rmsnorm(x);

  const h = new Array<number>(4 * N_EMBD);
  for (let i = 0; i < 4 * N_EMBD; i++) {
    let sum = 0;
    for (let j = 0; j < N_EMBD; j++) sum += LAYER0_MLP_FC1[i][j] * x[j];
    h[i] = relu(sum);
  }

  const xMlp = new Array<number>(N_EMBD);
  for (let i = 0; i < N_EMBD; i++) {
    let sum = 0;
    for (let j = 0; j < 4 * N_EMBD; j++) sum += LAYER0_MLP_FC2[i][j] * h[j];
    xMlp[i] = sum;
  }
  for (let i = 0; i < N_EMBD; i++) x[i] = xMlp[i] + xResidual2[i];

  const logits = new Float32Array(VOCAB_SIZE);
  for (let i = 0; i < VOCAB_SIZE; i++) {
    let sum = 0;
    for (let j = 0; j < N_EMBD; j++) sum += LM_HEAD[i][j] * x[j];
    logits[i] = sum;
  }
  return logits;
}
