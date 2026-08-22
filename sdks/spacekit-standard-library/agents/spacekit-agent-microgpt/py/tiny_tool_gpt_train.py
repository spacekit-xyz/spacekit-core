# tiny_tool_gpt_train.py
#
# Trains a tiny GPT on a tool-call DSL:
#
#   T0 = search
#   T1 = summarize
#   T2 = classify
#   T3 = code_review
#   T4 = arg_start
#   T5 = arg_end
#   T6 = sep
#   T7 = eos
#   T8 = pad   (training only)
#
# Exports weights in Rust array format for microgpt_forward.rs
#
# Requirements:
#   pip install torch numpy

import math
import random
import torch
import torch.nn as nn
import torch.optim as optim

# ─────────────────────────────────────────────────────────────
# Model config — must match Rust primitive
# ─────────────────────────────────────────────────────────────

VOCAB_SIZE = 9      # 0..8
N_EMBD = 4
N_HEAD = 1
N_LAYER = 1
BLOCK_SIZE = 8
HEAD_DIM = N_EMBD // N_HEAD
DEVICE = "cpu"

# ─────────────────────────────────────────────────────────────
# DSL tokens
# ─────────────────────────────────────────────────────────────

T_SEARCH      = 0
T_SUMMARIZE   = 1
T_CLASSIFY    = 2
T_CODE_REVIEW = 3
T_ARG_START   = 4
T_ARG_END     = 5
T_SEP         = 6
T_EOS         = 7
T_PAD         = 8   # training only

TOOLS = [T_SEARCH, T_SUMMARIZE, T_CLASSIFY, T_CODE_REVIEW]

# ─────────────────────────────────────────────────────────────
# Dataset generation
# ─────────────────────────────────────────────────────────────

def make_example():
    """
    Generates sequences like:

    search(arg)
    summarize(arg)
    classify(arg)
    code_review(arg)

    Or multi-tool sequences:

    search → summarize
    summarize → classify
    classify → code_review
    """

    # randomly choose 1–2 tools
    chosen = []
    for t in TOOLS:
        if random.random() < 0.4:
            chosen.append(t)

    if len(chosen) == 0:
        chosen.append(random.choice(TOOLS))

    seq = []

    for i, tool in enumerate(chosen):
        seq += [tool, T_ARG_START, T_ARG_END]
        if i < len(chosen) - 1:
            seq += [T_SEP]

    seq += [T_EOS]

    # pad to BLOCK_SIZE
    if len(seq) < BLOCK_SIZE:
        seq += [T_PAD] * (BLOCK_SIZE - len(seq))

    return seq


def build_dataset(n_samples=1024):
    return torch.tensor([make_example() for _ in range(n_samples)], dtype=torch.long)

# ─────────────────────────────────────────────────────────────
# Tiny GPT model
# ─────────────────────────────────────────────────────────────

class TinyGPT(nn.Module):
    def __init__(self):
        super().__init__()
        self.tok_emb = nn.Embedding(VOCAB_SIZE, N_EMBD)
        self.pos_emb = nn.Embedding(BLOCK_SIZE, N_EMBD)

        self.attn_wq = nn.Linear(N_EMBD, N_EMBD, bias=False)
        self.attn_wk = nn.Linear(N_EMBD, N_EMBD, bias=False)
        self.attn_wv = nn.Linear(N_EMBD, N_EMBD, bias=False)
        self.attn_wo = nn.Linear(N_EMBD, N_EMBD, bias=False)

        self.mlp_fc1 = nn.Linear(N_EMBD, 4 * N_EMBD)
        self.mlp_fc2 = nn.Linear(4 * N_EMBD, N_EMBD)

        self.ln1 = nn.LayerNorm(N_EMBD)
        self.ln2 = nn.LayerNorm(N_EMBD)

        self.lm_head = nn.Linear(N_EMBD, VOCAB_SIZE, bias=False)

    def forward(self, idx):
        B, T = idx.shape
        pos = torch.arange(T, device=idx.device).unsqueeze(0).expand(B, T)

        x = self.tok_emb(idx) + self.pos_emb(pos)

        # single transformer block
        x_res = x
        x = self.ln1(x)

        q = self.attn_wq(x)
        k = self.attn_wk(x)
        v = self.attn_wv(x)

        q = q.view(B, T, N_HEAD, HEAD_DIM)
        k = k.view(B, T, N_HEAD, HEAD_DIM)
        v = v.view(B, T, N_HEAD, HEAD_DIM)

        att = torch.einsum("bthd,bshd->bhts", q, k) / math.sqrt(HEAD_DIM)

        mask = torch.tril(torch.ones(T, T, device=idx.device)).unsqueeze(0).unsqueeze(0)
        att = att.masked_fill(mask == 0, float("-inf"))
        att = torch.softmax(att, dim=-1)

        y = torch.einsum("bhts,bshd->bthd", att, v)
        y = y.reshape(B, T, N_EMBD)

        y = self.attn_wo(y)
        x = x_res + y

        x_res = x
        x = self.ln2(x)
        x = self.mlp_fc1(x)
        x = torch.relu(x)
        x = self.mlp_fc2(x)
        x = x_res + x

        logits = self.lm_head(x)
        return logits

# ─────────────────────────────────────────────────────────────
# Training
# ─────────────────────────────────────────────────────────────

def train_model(steps=2000, lr=1e-2, batch_size=32):
    data = build_dataset(1024).to(DEVICE)
    model = TinyGPT().to(DEVICE)
    opt = optim.AdamW(model.parameters(), lr=lr)

    for step in range(steps):
        idx = torch.randint(0, data.size(0), (batch_size,), device=DEVICE)
        batch = data[idx]

        x = batch[:, :-1]
        y = batch[:, 1:]

        logits = model(x)
        loss = nn.functional.cross_entropy(
            logits.reshape(-1, VOCAB_SIZE),
            y.reshape(-1),
            ignore_index=T_PAD,
        )

        opt.zero_grad()
        loss.backward()
        opt.step()

        if (step + 1) % 200 == 0:
            print(f"step {step+1}/{steps} loss={loss.item():.4f}")

    return model

# ─────────────────────────────────────────────────────────────
# Export weights → Rust arrays
# ─────────────────────────────────────────────────────────────

def tensor_to_rust_2d(name, t):
    t = t.detach().cpu().numpy()
    rows, cols = t.shape
    lines = []
    lines.append(f"static {name}: [[f32; {cols}]; {rows}] = [")
    for r in range(rows):
        row_vals = ", ".join(f"{t[r, c]: .6f}" for c in range(cols))
        lines.append(f"    [{row_vals}],")
    lines.append("];")
    return "\n".join(lines)

def export_to_rust(model: TinyGPT):
    sd = model.state_dict()

    print("// Paste these into microgpt_forward.rs\n")

    print(tensor_to_rust_2d("WTE", sd["tok_emb.weight"]))
    print()
    print(tensor_to_rust_2d("WPE", sd["pos_emb.weight"]))
    print()
    print(tensor_to_rust_2d("LM_HEAD", sd["lm_head.weight"]))
    print()
    print(tensor_to_rust_2d("LAYER0_ATTN_WQ", sd["attn_wq.weight"]))
    print()
    print(tensor_to_rust_2d("LAYER0_ATTN_WK", sd["attn_wk.weight"]))
    print()
    print(tensor_to_rust_2d("LAYER0_ATTN_WV", sd["attn_wv.weight"]))
    print()
    print(tensor_to_rust_2d("LAYER0_ATTN_WO", sd["attn_wo.weight"]))
    print()
    print(tensor_to_rust_2d("LAYER0_MLP_FC1", sd["mlp_fc1.weight"]))
    print()
    print(tensor_to_rust_2d("LAYER0_MLP_FC2", sd["mlp_fc2.weight"]))

# ─────────────────────────────────────────────────────────────
# Main
# ─────────────────────────────────────────────────────────────

if __name__ == "__main__":
    random.seed(42)
    torch.manual_seed(42)

    model = train_model(steps=2000, lr=1e-2, batch_size=32)
    export_to_rust(model)
