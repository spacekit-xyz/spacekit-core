# SpaceKit Inference

Love this direction—let’s make it concrete but still lightweight enough to iterate.

I’ll sketch:

- A **Rust server** (Axum) that:
  - Tracks donors and jobs in memory.
  - Exposes REST for job submission.
  - Uses WebSocket to push jobs to donors and receive results.
- A **React + TypeScript client** that:
  - Registers as a donor.
  - Connects via WebSocket.
  - Runs a placeholder WebGPU “inference” kernel.
  - Sends back results.

You can plug your own DID/encryption into the marked areas.

---

### 1. Shared protocol (conceptual)

#### Message types (TypeScript)

```ts
// shared/protocol.ts

export type DID = string; // you’ll replace with your own type

export type JobId = string;

export interface DonorHello {
  type: "donor_hello";
  did: DID;
  capabilities: {
    gpu: boolean;
    maxBatchSize: number;
    modelIds: string[]; // hashes or IDs of supported models
  };
}

export interface JobAssignment {
  type: "job_assignment";
  jobId: JobId;
  modelId: string;
  input: unknown; // e.g. token IDs, image tensor, etc.
}

export interface JobResult {
  type: "job_result";
  jobId: JobId;
  output: unknown;
  // optional: proof, trace, timing, etc.
}

export interface JobError {
  type: "job_error";
  jobId: JobId;
  error: string;
}

export type DonorInbound = JobAssignment;
export type DonorOutbound = DonorHello | JobResult | JobError;
```

You’ll mirror these in Rust.

---

### 2. Rust server (Axum skeleton)

#### Cargo.toml

```toml
[package]
name = "webgpu_mesh_server"
version = "0.1.0"
edition = "2021"

[dependencies]
axum = { version = "0.7", features = ["ws", "json"] }
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
uuid = { version = "1", features = ["v4"] }
tower = "0.4"
tracing = "0.1"
tracing-subscriber = "0.3"
```

#### Types and state

```rust
// src/types.rs
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub type JobId = String;
pub type Did = String;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum DonorInbound {
    JobResult {
        jobId: JobId,
        output: serde_json::Value,
    },
    JobError {
        jobId: JobId,
        error: String,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum DonorOutbound {
    DonorHello {
        did: Did,
        capabilities: DonorCapabilities,
    },
    JobAssignment {
        jobId: JobId,
        modelId: String,
        input: serde_json::Value,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DonorCapabilities {
    pub gpu: bool,
    pub maxBatchSize: u32,
    pub modelIds: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Donor {
    pub did: Did,
    pub capabilities: DonorCapabilities,
    // handle to sender for this donor’s WebSocket
    pub tx: tokio::sync::mpsc::UnboundedSender<DonorOutbound>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JobRequest {
    pub modelId: String,
    pub input: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JobResponse {
    pub job_id: JobId,
}
```

#### Global state

```rust
// src/state.rs
use crate::types::{Donor, JobId};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone, Default)]
pub struct AppState {
    pub donors: Arc<RwLock<HashMap<String, Donor>>>, // did -> donor
    pub pending_jobs: Arc<RwLock<HashMap<JobId, tokio::sync::oneshot::Sender<serde_json::Value>>>>,
}
```

#### Main + routes

```rust
// src/main.rs
mod state;
mod types;

use axum::{
    extract::{State, WebSocketUpgrade},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use state::AppState;
use types::{DonorInbound, DonorOutbound, JobRequest, JobResponse};
use uuid::Uuid;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let state = AppState::default();

    let app = Router::new()
        .route("/api/jobs", post(submit_job))
        .route("/ws/donor", get(donor_ws_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080")
        .await
        .unwrap();
    tracing::info!("listening on 0.0.0.0:8080");
    axum::serve(listener, app).await.unwrap();
}

// Borrower submits a job
async fn submit_job(
    State(state): State<AppState>,
    Json(req): Json<JobRequest>,
) -> impl IntoResponse {
    let job_id = Uuid::new_v4().to_string();

    // create oneshot channel to await result
    let (tx, rx) = tokio::sync::oneshot::channel();

    {
        let mut pending = state.pending_jobs.write().await;
        pending.insert(job_id.clone(), tx);
    }

    // naive: pick any donor that supports this model
    let donors = state.donors.read().await;
    if let Some((_did, donor)) = donors
        .iter()
        .find(|(_, d)| d.capabilities.modelIds.contains(&req.modelId))
    {
        let _ = donor.tx.send(DonorOutbound::JobAssignment {
            jobId: job_id.clone(),
            modelId: req.modelId.clone(),
            input: req.input.clone(),
        });
    } else {
        // no donor available
        // in real code: return 503 or queue
    }

    // respond immediately with job_id; client can poll another endpoint or use WS
    Json(JobResponse { job_id })
}

// Donor WebSocket
async fn donor_ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_donor_socket(socket, state))
}

use axum::extract::ws::{Message, WebSocket};
use futures::{SinkExt, StreamExt};

async fn handle_donor_socket(stream: WebSocket, state: AppState) {
    let (mut sender, mut receiver) = stream.split();

    // first message from donor should be DonorHello
    let hello_msg = receiver.next().await;
    let Some(Ok(Message::Text(text))) = hello_msg else {
        return;
    };

    let hello: DonorOutbound = match serde_json::from_str(&text) {
        Ok(h) => h,
        Err(_) => return,
    };

    let (did, capabilities) = match hello {
        DonorOutbound::DonorHello { did, capabilities } => (did, capabilities),
        _ => return,
    };

    // channel to send messages to this donor
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

    // register donor
    {
        let mut donors = state.donors.write().await;
        donors.insert(
            did.clone(),
            crate::types::Donor {
                did: did.clone(),
                capabilities: capabilities.clone(),
                tx: tx.clone(),
            },
        );
    }

    // task: forward messages from server to donor
    let mut sender_clone = sender.clone();
    tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if let Ok(text) = serde_json::to_string(&msg) {
                if sender_clone.send(Message::Text(text)).await.is_err() {
                    break;
                }
            }
        }
    });

    // task: receive results from donor
    while let Some(Ok(msg)) = receiver.next().await {
        if let Message::Text(text) = msg {
            if let Ok(inbound) = serde_json::from_str::<DonorInbound>(&text) {
                match inbound {
                    DonorInbound::JobResult { jobId, output } => {
                        let tx_opt = {
                            let mut pending = state.pending_jobs.write().await;
                            pending.remove(&jobId)
                        };
                        if let Some(tx) = tx_opt {
                            let _ = tx.send(output);
                        }
                    }
                    DonorInbound::JobError { jobId, error } => {
                        tracing::warn!("job {} failed from donor {}: {}", jobId, did, error);
                        // handle error, retry, etc.
                    }
                }
            }
        }
    }

    // cleanup on disconnect
    {
        let mut donors = state.donors.write().await;
        donors.remove(&did);
    }
}
```

You’ll wire your **DID auth + encryption** into:

- The `DonorHello` message (signed payload, encrypted channel).
- Possibly a custom WebSocket upgrade with auth headers.

---

### 3. React + TypeScript donor client

Assume Vite + React + TS.

#### WebGPU helper (very simplified)

```ts
// src/webgpu/infer.ts

export async function runDummyInference(input: unknown): Promise<unknown> {
  if (!("gpu" in navigator)) {
    throw new Error("WebGPU not supported");
  }

  const adapter = await (navigator as any).gpu.requestAdapter();
  const device = await adapter.requestDevice();

  // This is a placeholder: in reality you’d load shaders, weights, etc.
  // For now, just echo the input with a timestamp to prove work happened.
  // Replace with real compute pipeline.
  await device.queue.onSubmittedWorkDone();

  return {
    echo: input,
    processedAt: new Date().toISOString(),
  };
}
```

#### WebSocket donor hook

```ts
// src/hooks/useDonorNode.ts
import { useEffect, useRef, useState } from "react";
import { runDummyInference } from "../webgpu/infer";

type DID = string;

interface DonorCapabilities {
  gpu: boolean;
  maxBatchSize: number;
  modelIds: string[];
}

interface JobAssignment {
  type: "job_assignment";
  jobId: string;
  modelId: string;
  input: unknown;
}

interface DonorHello {
  type: "donor_hello";
  did: DID;
  capabilities: DonorCapabilities;
}

interface JobResult {
  type: "job_result";
  jobId: string;
  output: unknown;
}

interface JobError {
  type: "job_error";
  jobId: string;
  error: string;
}

type Outbound = DonorHello | JobResult | JobError;

export function useDonorNode(did: DID | null) {
  const [connected, setConnected] = useState(false);
  const [lastJobId, setLastJobId] = useState<string | null>(null);
  const wsRef = useRef<WebSocket | null>(null);

  useEffect(() => {
    if (!did) return;

    const ws = new WebSocket("ws://localhost:8080/ws/donor");
    wsRef.current = ws;

    ws.onopen = () => {
      setConnected(true);

      const hello: DonorHello = {
        type: "donor_hello",
        did,
        capabilities: {
          gpu: "gpu" in navigator,
          maxBatchSize: 1,
          modelIds: ["demo-model-1"], // advertise supported models
        },
      };

      ws.send(JSON.stringify(hello));
    };

    ws.onmessage = async (event) => {
      try {
        const msg = JSON.parse(event.data) as JobAssignment;
        if (msg.type === "job_assignment") {
          setLastJobId(msg.jobId);
          try {
            const output = await runDummyInference(msg.input);
            const result: JobResult = {
              type: "job_result",
              jobId: msg.jobId,
              output,
            };
            ws.send(JSON.stringify(result));
          } catch (err: any) {
            const errorMsg: JobError = {
              type: "job_error",
              jobId: msg.jobId,
              error: err?.message ?? "unknown error",
            };
            ws.send(JSON.stringify(errorMsg));
          }
        }
      } catch (e) {
        console.error("Failed to handle message", e);
      }
    };

    ws.onclose = () => {
      setConnected(false);
      wsRef.current = null;
    };

    return () => {
      ws.close();
    };
  }, [did]);

  return { connected, lastJobId };
}
```

#### React component

```tsx
// src/App.tsx
import React, { useState } from "react";
import { useDonorNode } from "./hooks/useDonorNode";

function generateDemoDid(): string {
  // placeholder – plug in your DID system
  return "did:demo:" + Math.random().toString(36).slice(2);
}

const App: React.FC = () => {
  const [did, setDid] = useState<string | null>(null);
  const { connected, lastJobId } = useDonorNode(did);

  const handleStart = () => {
    if (!did) {
      setDid(generateDemoDid());
    }
  };

  return (
    <div style={{ padding: 24, fontFamily: "system-ui" }}>
      <h1>WebGPU Donor Node</h1>
      <p>DID: {did ?? "not initialized"}</p>
      <p>Status: {connected ? "Connected" : "Disconnected"}</p>
      <p>Last job: {lastJobId ?? "none yet"}</p>

      <button onClick={handleStart} disabled={!!did}>
        {did ? "Node running…" : "Start donating GPU"}
      </button>

      <p style={{ marginTop: 16, maxWidth: 480 }}>
        This tab will receive inference jobs from the coordinator, run them via
        WebGPU, and return results.
      </p>
    </div>
  );
};

export default App;
```

---

### 4. Borrower: simple job submission example

You can test the flow with a tiny script or a React page.

```ts
// src/api/submitJob.ts
export async function submitJob(modelId: string, input: unknown) {
  const res = await fetch("http://localhost:8080/api/jobs", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ modelId, input }),
  });

  if (!res.ok) {
    throw new Error(`Job submit failed: ${res.status}`);
  }

  return (await res.json()) as { job_id: string };
}
```

---

### 5. Where you plug in your stack

You can swap in your own primitives at these seams:

- **DID + auth:**
  - Replace `generateDemoDid()` with your DID issuance/selection.
  - Sign the `DonorHello` payload and verify it server-side.
- **Encryption:**
  - Wrap WebSocket in your own encrypted tunnel, or:
  - Encrypt `JobAssignment`/`JobResult` payloads at the message level.
- **Model + inference:**
  - Replace `runDummyInference` with:
    - Model loading (weights from CDN/IPFS/your storage).
    - WebGPU compute pipelines.
    - Optional caching in IndexedDB.

---

Nice—WebLLM simplifies a ton. Now your “GPU donor” is basically “a browser tab running a WebLLM engine.”

I’ll keep the Rust server from before mostly intact and just swap the client-side `runDummyInference` for a WebLLM-backed `runWebLLMInference`.

---

### 1. Rust server recap (no change needed for WebLLM)

You can keep the Axum server skeleton as-is:

- `/api/jobs` — borrower submits `{ modelId, input }`.
- `/ws/donor` — donors connect via WebSocket, send `donor_hello`, receive `job_assignment`, reply with `job_result`.

As long as:

- `modelId` matches what donors advertise (e.g. `"Llama-3-8B-Instruct-q4f16_1"`).
- `input` is something like `{ prompt: string }`.

You don’t need to change the server for WebLLM specifically.

---

### 2. WebLLM setup in the donor client

Assuming you’re using `@mlc-ai/web-llm`.

#### Install

```bash
npm install @mlc-ai/web-llm
```

---

### 3. WebLLM engine wrapper

```ts
// src/webllm/engine.ts
import * as webllm from "@mlc-ai/web-llm";

let enginePromise: Promise<webllm.MLCEngine> | null = null;

export type WebLLMInput = {
  prompt: string;
  // extend with temperature, max_tokens, etc. if you like
};

export type WebLLMOutput = {
  text: string;
};

export async function getEngine(modelId: string): Promise<webllm.MLCEngine> {
  if (!enginePromise) {
    // You can map modelId → actual WebLLM model config here
    const initProgressCallback = (progress: webllm.InitProgressReport) => {
      console.log("WebLLM init progress", progress);
    };

    enginePromise = webllm.CreateMLCEngine(modelId, {
      initProgressCallback,
    });
  }
  return enginePromise;
}

export async function runWebLLMInference(
  modelId: string,
  input: WebLLMInput
): Promise<WebLLMOutput> {
  const engine = await getEngine(modelId);

  const chunks: string[] = [];
  await engine.chat.completions.create({
    messages: [{ role: "user", content: input.prompt }],
    stream: true,
    stream_callback: (chunk) => {
      const delta = chunk.choices[0]?.delta?.content ?? "";
      if (delta) chunks.push(delta);
    },
  });

  return { text: chunks.join("") };
}
```

You can later:

- Cache multiple engines.
- Map `modelId` to different WebLLM models.

---

### 4. Donor WebSocket hook using WebLLM

```ts
// src/hooks/useDonorNode.ts
import { useEffect, useRef, useState } from "react";
import { runWebLLMInference, WebLLMInput, WebLLMOutput } from "../webllm/engine";

type DID = string;

interface DonorCapabilities {
  gpu: boolean;
  maxBatchSize: number;
  modelIds: string[];
}

interface JobAssignment {
  type: "job_assignment";
  jobId: string;
  modelId: string;
  input: WebLLMInput; // we expect { prompt: string }
}

interface DonorHello {
  type: "donor_hello";
  did: DID;
  capabilities: DonorCapabilities;
}

interface JobResult {
  type: "job_result";
  jobId: string;
  output: WebLLMOutput;
}

interface JobError {
  type: "job_error";
  jobId: string;
  error: string;
}

type Outbound = DonorHello | JobResult | JobError;

export function useDonorNode(did: DID | null) {
  const [connected, setConnected] = useState(false);
  const [lastJobId, setLastJobId] = useState<string | null>(null);
  const [lastOutput, setLastOutput] = useState<string | null>(null);
  const wsRef = useRef<WebSocket | null>(null);

  useEffect(() => {
    if (!did) return;

    const ws = new WebSocket("ws://localhost:8080/ws/donor");
    wsRef.current = ws;

    ws.onopen = () => {
      setConnected(true);

      const hello: DonorHello = {
        type: "donor_hello",
        did,
        capabilities: {
          gpu: "gpu" in navigator,
          maxBatchSize: 1,
          // advertise the WebLLM model you’re willing to host
          modelIds: ["Llama-3-8B-Instruct-q4f16_1"],
        },
      };

      ws.send(JSON.stringify(hello));
    };

    ws.onmessage = async (event) => {
      try {
        const msg = JSON.parse(event.data) as JobAssignment;
        if (msg.type === "job_assignment") {
          setLastJobId(msg.jobId);
          try {
            const output = await runWebLLMInference(msg.modelId, msg.input);
            setLastOutput(output.text);

            const result: JobResult = {
              type: "job_result",
              jobId: msg.jobId,
              output,
            };
            ws.send(JSON.stringify(result));
          } catch (err: any) {
            const errorMsg: JobError = {
              type: "job_error",
              jobId: msg.jobId,
              error: err?.message ?? "unknown error",
            };
            ws.send(JSON.stringify(errorMsg));
          }
        }
      } catch (e) {
        console.error("Failed to handle message", e);
      }
    };

    ws.onclose = () => {
      setConnected(false);
      wsRef.current = null;
    };

    return () => {
      ws.close();
    };
  }, [did]);

  return { connected, lastJobId, lastOutput };
}
```

---

### 5. Donor React component

```tsx
// src/App.tsx
import React, { useState } from "react";
import { useDonorNode } from "./hooks/useDonorNode";

function generateDemoDid(): string {
  // plug in your DID system here
  return "did:demo:" + Math.random().toString(36).slice(2);
}

const App: React.FC = () => {
  const [did, setDid] = useState<string | null>(null);
  const { connected, lastJobId, lastOutput } = useDonorNode(did);

  const handleStart = () => {
    if (!did) {
      setDid(generateDemoDid());
    }
  };

  return (
    <div style={{ padding: 24, fontFamily: "system-ui" }}>
      <h1>WebLLM Donor Node</h1>
      <p>DID: {did ?? "not initialized"}</p>
      <p>Status: {connected ? "Connected" : "Disconnected"}</p>
      <p>Last job: {lastJobId ?? "none yet"}</p>
      {lastOutput && (
        <div style={{ marginTop: 12 }}>
          <strong>Last output:</strong>
          <pre
            style={{
              background: "#111",
              color: "#0f0",
              padding: 12,
              borderRadius: 4,
              whiteSpace: "pre-wrap",
            }}
          >
            {lastOutput}
          </pre>
        </div>
      )}

      <button onClick={handleStart} disabled={!!did}>
        {did ? "Node running…" : "Start donating WebLLM GPU"}
      </button>

      <p style={{ marginTop: 16, maxWidth: 480 }}>
        This tab loads a WebLLM model and serves remote inference jobs from the
        coordinator.
      </p>
    </div>
  );
};

export default App;
```

---

### 6. Borrower: submit a WebLLM job

```ts
// src/api/submitJob.ts
import type { WebLLMInput } from "../webllm/engine";

export async function submitWebLLMJob(prompt: string) {
  const body = {
    modelId: "Llama-3-8B-Instruct-q4f16_1",
    input: { prompt } satisfies WebLLMInput,
  };

  const res = await fetch("http://localhost:8080/api/jobs", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });

  if (!res.ok) {
    throw new Error(`Job submit failed: ${res.status}`);
  }

  return (await res.json()) as { job_id: string };
}
```

---

### 7. Where your DID/encryption slots in

- **DID:**
  - Replace `generateDemoDid()` with your real DID.
  - Sign the `donor_hello` payload and verify it in the Rust server before registering the donor.

- **Encryption:**
  - Either:
    - Use your own secure tunnel for the WebSocket URL, or
    - Encrypt `JobAssignment`/`JobResult` payloads at the message level with your scheme.

If you tell me your preferred WebLLM model config (exact ID + quantization) and how you want prompts/results structured, I can tighten the types and the server’s `JobRequest`/`JobAssignment` shape to match your protocol exactly.

---

You’re basically describing an **on‑chain coordination layer** for that WebGPU/WebLLM mesh—so SpaceKit‑JS can treat the mesh like a first‑class primitive:

- **Who can run what?** → donor registry  
- **What jobs exist?** → job registry  
- **Who ran what, and what was the result?** → job lifecycle  

Let’s turn that into a **single, minimal SpaceKit contract** you can drop into the VM, plus a clean ABI that your Rust Axum server + browser donors can both speak.

---

### 1. Contract: `SpaceKitInferenceMesh`

**Responsibilities**

- Register donor nodes + capabilities (model IDs, max batch size, etc.)
- Register inference jobs (submitted by borrowers)
- Record which donor took which job
- Record job result or error
- Expose read APIs so:
  - Axum coordinator can pick donors
  - SpaceKit‑JS can show job status/history

> **Important:** actual WebSocket routing stays off‑chain (Axum).  
> This contract is the **source of truth** for donors + jobs.

---

### 2. ABI (opcodes + payloads)

All strings are encoded as:

- `len:u16 (LE)` + UTF‑8 bytes

All outputs that have a `status` start with:

- `status:u8` (`1 = ok`, `0 = not found/empty`, `>1 = future use`)

#### Opcodes

| Opcode | Name                 | Direction | Purpose                                  |
|--------|----------------------|-----------|------------------------------------------|
| 1      | REGISTER_DONOR       | write     | Register/update donor capabilities       |
| 2      | GET_DONOR            | read      | Get donor capabilities                   |
| 3      | CREATE_JOB           | write     | Register a new inference job             |
| 4      | ASSIGN_JOB           | write     | Mark a job as assigned to a donor        |
| 5      | SET_JOB_RESULT       | write     | Store job result (success or error)      |
| 6      | GET_JOB              | read      | Get full job record                      |

---

#### 2.1 `REGISTER_DONOR` (1)

- **Input:**

  - `donor_did:string`
  - `gpu:u8` (`0/1`)
  - `max_batch_size:u8`
  - `model_ids_csv:string` (e.g. `"Llama-3-8B,...`)

- **Output:**

  - `status:u8`

> Called by the **donor browser** (or Axum on its behalf) when it connects.

---

#### 2.2 `GET_DONOR` (2)

- **Input:**

  - `donor_did:string`

- **Output:**

  - `status:u8 (0=not found,1=ok)`  
  - `gpu:u8`  
  - `max_batch_size:u8`  
  - `model_ids_csv:string`

> Called by Axum to inspect donors if needed (you’ll mostly cache this off‑chain).

---

#### 2.3 `CREATE_JOB` (3)

- **Input:**

  - `job_id:string` (UUID from Axum)
  - `borrower_did:string`
  - `model_id:string`
  - `input_uri:string` (where the prompt/payload lives; can be encrypted blob)
  - `max_price:u64` (optional economics later; set `0` for now)

- **Output:**

  - `status:u8`

> Called by Axum when a borrower hits `POST /api/jobs`.

---

#### 2.4 `ASSIGN_JOB` (4)

- **Input:**

  - `job_id:string`
  - `donor_did:string`

- **Output:**

  - `status:u8`

> Called by Axum when it decides which donor to send the WebSocket `job_assignment` to.

---

#### 2.5 `SET_JOB_RESULT` (5)

- **Input:**

  - `job_id:string`
  - `success:u8` (`1=ok`, `0=error`)
  - `result_uri:string` (where output or error JSON is stored)

- **Output:**

  - `status:u8`

> Called by Axum when it receives `job_result` or `job_error` from the donor.

---

#### 2.6 `GET_JOB` (6)

- **Input:**

  - `job_id:string`

- **Output:**

  - `status:u8 (0=not found,1=ok)`  
  - `borrower_did:string`  
  - `model_id:string`  
  - `input_uri:string`  
  - `max_price:u64`  
  - `job_status:u8`  
    - `1 = created`  
    - `2 = assigned`  
    - `3 = completed`  
    - `4 = failed`  
  - `donor_did:string` (empty if not assigned)  
  - `result_uri:string` (empty if not completed/failed)

> Called by:
> - Borrower (via REST → Axum → SpaceKit‑JS) to check status  
> - Indexers / dashboards to show mesh activity  

---

### 3. Rust Contract (no_std, SpaceKit‑style)

```rust
#![no_std]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::vec;

use spacekit_contract_sdk::{ContractError, SpacekitContract};
use spacekit_contract_sdk::spacekit_contract;

#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { loop {} }

#[link(wasm_import_module = "spacekit_storage")]
extern "C" {
    fn storage_save(key_ptr: *const u8, key_len: usize, data_ptr: *const u8, data_len: usize) -> i32;
    fn storage_load(key_ptr: *const u8, key_len: usize, dest_ptr: *mut u8, max_len: usize) -> usize;
}

struct SpaceKitInferenceMesh;

const OP_REGISTER_DONOR: u8 = 1;
const OP_GET_DONOR: u8      = 2;
const OP_CREATE_JOB: u8     = 3;
const OP_ASSIGN_JOB: u8     = 4;
const OP_SET_JOB_RESULT: u8 = 5;
const OP_GET_JOB: u8        = 6;

impl SpacekitContract for SpaceKitInferenceMesh {
    type Error = ContractError;

    fn init() -> Self { SpaceKitInferenceMesh }

    fn handle(&mut self, input: &[u8]) -> Result<Vec<u8>, ContractError> {
        handle(input)
    }
}

spacekit_contract!(SpaceKitInferenceMesh);

fn handle(input: &[u8]) -> Result<Vec<u8>, ContractError> {
    let mut cursor = 0usize;
    let op = read_u8(input, &mut cursor)?;

    match op {
        // REGISTER_DONOR
        OP_REGISTER_DONOR => {
            let did = read_string(input, &mut cursor)?;
            let gpu = read_u8(input, &mut cursor)?;
            let max_batch = read_u8(input, &mut cursor)?;
            let model_ids_csv = read_string(input, &mut cursor)?;

            let mut buf = Vec::new();
            buf.push(gpu);
            buf.push(max_batch);
            write_string(&mut buf, &model_ids_csv)?;

            storage_save_bytes(&donor_key(&did), &buf)?;
            Ok(vec![1u8])
        }

        // GET_DONOR
        OP_GET_DONOR => {
            let did = read_string(input, &mut cursor)?;
            let data = match storage_load_bytes(&donor_key(&did), 1024) {
                Ok(d) => d,
                Err(_) => return Ok(vec![0u8]), // not found
            };

            let mut out = Vec::new();
            out.push(1u8); // status
            // data layout: [gpu:u8][max_batch:u8][model_ids_csv:string]
            let mut dc = 0usize;
            let gpu = data[dc]; dc += 1;
            let max_batch = data[dc]; dc += 1;

            out.push(gpu);
            out.push(max_batch);
            out.extend_from_slice(&data[dc..]);
            Ok(out)
        }

        // CREATE_JOB
        OP_CREATE_JOB => {
            let job_id = read_string(input, &mut cursor)?;
            let borrower_did = read_string(input, &mut cursor)?;
            let model_id = read_string(input, &mut cursor)?;
            let input_uri = read_string(input, &mut cursor)?;
            let max_price = read_u64(input, &mut cursor)?;

            let mut buf = Vec::new();
            write_string(&mut buf, &borrower_did)?;
            write_string(&mut buf, &model_id)?;
            write_string(&mut buf, &input_uri)?;
            buf.extend_from_slice(&max_price.to_le_bytes());
            buf.push(1u8); // job_status = created
            write_string(&mut buf, ""); // donor_did empty
            write_string(&mut buf, ""); // result_uri empty

            storage_save_bytes(&job_key(&job_id), &buf)?;
            Ok(vec![1u8])
        }

        // ASSIGN_JOB
        OP_ASSIGN_JOB => {
            let job_id = read_string(input, &mut cursor)?;
            let donor_did = read_string(input, &mut cursor)?;

            let mut data = storage_load_bytes(&job_key(&job_id), 2048)?;
            let mut jc = 0usize;

            let _borrower = read_string_from(&data, &mut jc)?;
            let _model_id = read_string_from(&data, &mut jc)?;
            let _input_uri = read_string_from(&data, &mut jc)?;
            let _max_price = read_u64_from(&data, &mut jc)?;
            // overwrite job_status
            if jc >= data.len() { return Err(ContractError::InvalidInput); }
            data[jc] = 2u8; // assigned
            jc += 1;

            // overwrite donor_did + result_uri
            let _old_donor = read_string_from(&data, &mut jc)?;
            let _old_result = read_string_from(&data, &mut jc)?;

            let mut new_buf = Vec::new();
            let mut tmpc = 0usize;
            let borrower = read_string_from(&data, &mut tmpc)?;
            let model_id = read_string_from(&data, &mut tmpc)?;
            let input_uri = read_string_from(&data, &mut tmpc)?;
            let max_price = read_u64_from(&data, &mut tmpc)?;
            let job_status = data[tmpc]; tmpc += 1;

            write_string(&mut new_buf, &borrower)?;
            write_string(&mut new_buf, &model_id)?;
            write_string(&mut new_buf, &input_uri)?;
            new_buf.extend_from_slice(&max_price.to_le_bytes());
            new_buf.push(job_status);
            write_string(&mut new_buf, &donor_did)?;
            write_string(&mut new_buf, "")?;

            storage_save_bytes(&job_key(&job_id), &new_buf)?;
            Ok(vec![1u8])
        }

        // SET_JOB_RESULT
        OP_SET_JOB_RESULT => {
            let job_id = read_string(input, &mut cursor)?;
            let success = read_u8(input, &mut cursor)?;
            let result_uri = read_string(input, &mut cursor)?;

            let mut data = storage_load_bytes(&job_key(&job_id), 2048)?;
            let mut jc = 0usize;

            let borrower = read_string_from(&data, &mut jc)?;
            let model_id = read_string_from(&data, &mut jc)?;
            let input_uri = read_string_from(&data, &mut jc)?;
            let max_price = read_u64_from(&data, &mut jc)?;
            let _old_status = data[jc]; jc += 1;
            let donor_did = read_string_from(&data, &mut jc)?;
            let _old_result = read_string_from(&data, &mut jc)?;

            let new_status = if success == 1 { 3u8 } else { 4u8 };

            let mut new_buf = Vec::new();
            write_string(&mut new_buf, &borrower)?;
            write_string(&mut new_buf, &model_id)?;
            write_string(&mut new_buf, &input_uri)?;
            new_buf.extend_from_slice(&max_price.to_le_bytes());
            new_buf.push(new_status);
            write_string(&mut new_buf, &donor_did)?;
            write_string(&mut new_buf, &result_uri)?;

            storage_save_bytes(&job_key(&job_id), &new_buf)?;
            Ok(vec![1u8])
        }

        // GET_JOB
        OP_GET_JOB => {
            let job_id = read_string(input, &mut cursor)?;
            let data = match storage_load_bytes(&job_key(&job_id), 2048) {
                Ok(d) => d,
                Err(_) => return Ok(vec![0u8]),
            };

            let mut jc = 0usize;
            let borrower = read_string_from(&data, &mut jc)?;
            let model_id = read_string_from(&data, &mut jc)?;
            let input_uri = read_string_from(&data, &mut jc)?;
            let max_price = read_u64_from(&data, &mut jc)?;
            let job_status = data[jc]; jc += 1;
            let donor_did = read_string_from(&data, &mut jc)?;
            let result_uri = read_string_from(&data, &mut jc)?;

            let mut out = Vec::new();
            out.push(1u8); // status
            write_string(&mut out, &borrower)?;
            write_string(&mut out, &model_id)?;
            write_string(&mut out, &input_uri)?;
            out.extend_from_slice(&max_price.to_le_bytes());
            out.push(job_status);
            write_string(&mut out, &donor_did)?;
            write_string(&mut out, &result_uri)?;
            Ok(out)
        }

        _ => Err(ContractError::InvalidInput),
    }
}

fn donor_key(did: &str) -> String {
    let mut k = String::from("mesh:donor:");
    k.push_str(did);
    k
}

fn job_key(job_id: &str) -> String {
    let mut k = String::from("mesh:job:");
    k.push_str(job_id);
    k
}

fn storage_save_bytes(key: &str, data: &[u8]) -> Result<(), ContractError> {
    let r = unsafe { storage_save(key.as_ptr(), key.len(), data.as_ptr(), data.len()) };
    if r >= 0 { Ok(()) } else { Err(ContractError::StorageError) }
}

fn storage_load_bytes(key: &str, max_len: usize) -> Result<Vec<u8>, ContractError> {
    let mut buf = vec![0u8; max_len];
    let n = unsafe { storage_load(key.as_ptr(), key.len(), buf.as_mut_ptr(), max_len) };
    if n == 0 { return Err(ContractError::StorageError); }
    buf.truncate(n);
    Ok(buf)
}

fn read_u8(input: &[u8], cursor: &mut usize) -> Result<u8, ContractError> {
    if *cursor >= input.len() { return Err(ContractError::InvalidInput); }
    let v = input[*cursor]; *cursor += 1; Ok(v)
}

fn read_u16(input: &[u8], cursor: &mut usize) -> Result<u16, ContractError> {
    if *cursor + 2 > input.len() { return Err(ContractError::InvalidInput); }
    let b = [input[*cursor], input[*cursor + 1]]; *cursor += 2; Ok(u16::from_le_bytes(b))
}

fn read_u64(input: &[u8], cursor: &mut usize) -> Result<u64, ContractError> {
    if *cursor + 8 > input.len() { return Err(ContractError::InvalidInput); }
    let b = [
        input[*cursor], input[*cursor + 1], input[*cursor + 2], input[*cursor + 3],
        input[*cursor + 4], input[*cursor + 5], input[*cursor + 6], input[*cursor + 7],
    ];
    *cursor += 8;
    Ok(u64::from_le_bytes(b))
}

fn read_string(input: &[u8], cursor: &mut usize) -> Result<String, ContractError> {
    let len = read_u16(input, cursor)? as usize;
    if *cursor + len > input.len() { return Err(ContractError::InvalidInput); }
    let slice = &input[*cursor..*cursor + len]; *cursor += len;
    core::str::from_utf8(slice).map(|s| s.to_string()).map_err(|_| ContractError::InvalidInput)
}

// helpers for parsing from an in-memory buffer
fn read_string_from(buf: &[u8], cursor: &mut usize) -> Result<String, ContractError> {
    if *cursor + 2 > buf.len() { return Err(ContractError::InvalidInput); }
    let len = u16::from_le_bytes([buf[*cursor], buf[*cursor + 1]]) as usize;
    *cursor += 2;
    if *cursor + len > buf.len() { return Err(ContractError::InvalidInput); }
    let slice = &buf[*cursor..*cursor + len];
    *cursor += len;
    core::str::from_utf8(slice).map(|s| s.to_string()).map_err(|_| ContractError::InvalidInput)
}

fn read_u64_from(buf: &[u8], cursor: &mut usize) -> Result<u64, ContractError> {
    if *cursor + 8 > buf.len() { return Err(ContractError::InvalidInput); }
    let b = [
        buf[*cursor], buf[*cursor + 1], buf[*cursor + 2], buf[*cursor + 3],
        buf[*cursor + 4], buf[*cursor + 5], buf[*cursor + 6], buf[*cursor + 7],
    ];
    *cursor += 8;
    Ok(u64::from_le_bytes(b))
}

fn write_u16(out: &mut Vec<u8>, v: u16) { out.extend_from_slice(&v.to_le_bytes()); }

fn write_string(out: &mut Vec<u8>, s: &str) -> Result<(), ContractError> {
    let len = s.len();
    if len > u16::MAX as usize { return Err(ContractError::InvalidInput); }
    write_u16(out, len as u16);
    out.extend_from_slice(s.as_bytes());
    Ok(())
}
```

---

### 4. How this plugs into your existing spec

- **Donor browser**:
  - On WebSocket connect → send `donor_hello` to Axum
  - Axum verifies DID, then calls `REGISTER_DONOR` on this contract

- **Borrower**:
  - Calls `POST /api/jobs`
  - Axum:
    - Generates `job_id`
    - Calls `CREATE_JOB`
    - Picks donor
    - Calls `ASSIGN_JOB`
    - Sends `job_assignment` over WebSocket

- **Donor**:
  - Runs WebLLM
  - Sends `job_result` / `job_error` to Axum
  - Axum calls `SET_JOB_RESULT`

- **Borrower / UI**:
  - Polls `GET_JOB` (via SpaceKit‑JS) to see status + result URI

---

If you want, next step I can:

- Sketch the **SpaceKit‑JS wrapper** for this contract (`InferenceMesh` class).  
- Show how your Axum server would call into SpaceKit‑JS for each step.