# routekit-agent

**RouteKit** is a SpaceKit WASM smart contract that wires **Growformer** inference, **web search**, **vault billing**, **cross-DID messaging**, and **remote transcript storage** into one deterministic agent. Each operation is charged via `payment_vault_charge` before work runs, and the VM emits structured events for receipts and indexers.

This crate is the **reference integration** for the agent host surface in
[`spacekit-contract-sdk`](../../../spacekit-contract-sdk/) (`spacekit_agent`,
`spacekit_tools`, `spacekit_messaging`, `spacekit_remote_storage`,
`spacekit_payments`).

---

## Contents

- [Features](#features)
- [Build](#build)
- [Wire format](#wire-format)
- [Opcodes](#opcodes)
- [Vault costs](#vault-costs)
- [Growformer brain key](#growformer-brain-key)
- [Events](#events)
- [Host imports](#host-imports)
- [Limits](#limits)
- [Roadmap](#roadmap)

---

## Features

| Area | Behavior |
|------|----------|
| **Local completion** | Single-shot Growformer generation on a stored router brain. |
| **Search → reply** | `web_search` JSON hits + Growformer reply in one transaction. |
| **Search** | Legacy: UTF-8 query only → JSON hits. **v1**: search + Growformer synthesis. |
| **Converse** | Multi-turn: load prior transcript by ref, reply, persist new transcript ref + reply. |
| **Frontier** | Vault-tier charge + `messaging_send` to a recipient DID (returns `pending`). |
| **Ping** | Same messaging path as frontier without vault charge (operator / connectivity checks). |
| **Configure** | Store a prefs blob in remote storage; return a short ref. |
| **Health / brain info** | JSON health + loaded brain metadata. |

---

## Build

From the `spacekit-standard-library` workspace root:

```bash
cargo build -p routekit-agent --release --target wasm32-unknown-unknown
```

Or check only:

```bash
cargo check -p routekit-agent --target wasm32-unknown-unknown
```

This crate depends on `spacekit-contract-sdk` via a **path** dependency in `Cargo.toml` (local monorepo). Published clones may use the git dependency like other standard-library crates.

---

## Wire format

All multi-byte integers are **little-endian**. Length-prefixed UTF-8 blobs use a leading **`u16` length** (same pattern as other SpaceKit agents).

| Op | Opcode | Payload | Response (summary) |
|----|--------|---------|---------------------|
| **HEALTH** | `0x10` | *(empty after opcode)* | UTF-8 JSON: status, growformer host, router brain seeded |
| **COMPLETE** | `0x01` | `[max_resp: u16][prompt_len: u16][prompt_utf8]` | UTF-8 model output |
| **PIPELINE** (search then reply) | `0x02` | `[max_resp: u16][sq_len: u16][search_query_utf8][uq_len: u16][user_question_utf8]` | UTF-8 reply |
| **SEARCH** legacy | `0x03` | UTF-8 query only | UTF-8 JSON hits |
| **SEARCH** v1 | `0x03` + `0x01` | `[1][max_gen: u16][q_len: u16][query_utf8]` | UTF-8 synthesized answer |
| **CONVERSE** | `0x04` | `[hist_ref_len: u16][hist_ref_utf8][msg_len: u16][msg_utf8]` | `[new_ref_len: u16][new_ref][reply_len: u16][reply_utf8]` |
| **FRONTIER_SEND** | `0x05` | `[recipient_len: u16][recipient_utf8][payload_len: u16][payload_bytes]` | ASCII `pending` |
| **PING** | `0x11` | Same layout as FRONTIER | ASCII `sent` (no vault charge) |
| **BRAIN_INFO** | `0x12` | *(empty after opcode)* | UTF-8 JSON brain metadata |
| **CONFIGURE** | `0x20` | `[prefs_len: u16][prefs_utf8]` | `[ref_len: u16][ref_utf8]` |

**SEARCH v1 discriminator:** first byte of the SEARCH payload is `0x01`; legacy SEARCH is any payload that does **not** start with `0x01` (typically begins with query text).

---

## Opcodes

### `0x01` — COMPLETE

Local Growformer generation: decode `max_resp` (used as max generation length) and prompt, charge local tier, load `ROUTER_BRAIN_KEY`, call `growformer_generation`.

### `0x02` — PIPELINE

Web search (`max_results = 5`, up to 64 KiB JSON) then Growformer on an enriched prompt containing hits + user question.

### `0x03` — SEARCH

- **Legacy:** entire body after opcode is UTF-8 query; returns raw JSON string from `web_search`.
- **v1:** body starts with `0x01`, then `max_gen`, then length-prefixed query; charges search+local tier; returns Growformer answer grounded in snippets.

### `0x04` — CONVERSE

If `hist_ref` is non-empty, loads prior transcript from `remote_storage_get`. Appends user message, generates assistant reply, writes full transcript with `remote_storage_put`, returns new ref + reply blobs.

### `0x05` — FRONTIER_SEND

Charges frontier tier, sends binary payload to recipient DID via `messaging_send`. Response is the placeholder `pending` (async delivery is host / node responsibility).

### `0x11` — PING

Same wire as FRONTIER but **no** `payment_vault_charge`; emits `routekit.ping.sent` with an 8-byte length marker.

### `0x12` — BRAIN_INFO

Loads router brain, returns `growformer_brain_info` JSON (up to 4096 bytes).

### `0x10` — HEALTH

Static JSON including `growformer_status` and whether `ROUTER_BRAIN_KEY` loads.

### `0x20` — CONFIGURE

Stores prefs UTF-8 blob; returns a ref string for clients to persist.

---

## Vault costs

Amounts are **decimal string** tiers passed to `payment_vault_charge` (host interprets against the caller’s vault / policy).

| Tier constant | Value | Used by |
|---------------|-------|---------|
| Local | `"100"` | COMPLETE, CONVERSE |
| Search | `"200"` | SEARCH legacy |
| Pipeline / search+local | `"300"` | PIPELINE, SEARCH v1 |
| Frontier | `"5000"` | FRONTIER_SEND |

Beneficiary for charges is the **caller DID** from `get_caller_did_string`, or `did:spacekit:anonymous` if unset.

---

## Growformer brain key

```rust
pub const ROUTER_BRAIN_KEY: &str = "routekit_router";
```

Deploy or seed the router brain bytes in VM / storage under this key before expecting COMPLETE, PIPELINE, SEARCH v1, or CONVERSE to succeed past `growformer_load_brain_from_storage_key`.

---

## Events

| Event | When | Payload (conceptual) |
|-------|------|----------------------|
| `routekit.complete` | After COMPLETE | 4-byte LE output length |
| `routekit.pipeline` | After PIPELINE | 4-byte LE output length |
| `routekit.search.start` | Legacy search start | 4-byte LE query length |
| `routekit.search.done` | Legacy search done | 4-byte LE hits length |
| `routekit.search_v1.done` | SEARCH v1 done | 4-byte LE output length |
| `routekit.converse` | After CONVERSE | 4-byte LE reply length |
| `routekit.configure` | After CONFIGURE | 4-byte LE prefs length |
| `routekit.frontier.sent` | After FRONTIER | 4-byte LE payload length |
| `routekit.ping.sent` | After PING | 8 bytes: recipient len + payload len (LE u32 each) |

---

## Host imports

| WASM module | Functions used |
|-------------|------------------|
| `spacekit_agent` | Growformer status, load brain, generation, brain info |
| `spacekit_tools` | `web_search` |
| `spacekit_messaging` | `messaging_send` |
| `spacekit_remote_storage` | `remote_storage_put`, `remote_storage_get` |
| `spacekit_payments` | `payment_vault_charge` |
| `env` | `get_caller_did`, `emit_event` |

The SpaceKit JS VM implements these under `host.ts` / contract SDK parity docs.

---

## Limits

| Constant | Value | Role |
|----------|-------|------|
| `DEFAULT_SEARCH_MAX_JSON` | 64 KiB | Cap on `web_search` buffer |
| `CONVERSE_HIST_GET_MAX` | 96 KiB | Max transcript fetch |
| `REF_OUT_MAX` | 512 | Max bytes for returned storage refs in CONVERSE / CONFIGURE |
| Brain info buffer | 4096 | `growformer_brain_info` cap |

---

## Roadmap

Not implemented in this crate today (see git history / design sketches for earlier ideas):

- Separate **classifier** / **router** brains as distinct storage keys with automatic task routing  
- **`agent_agent`**-style calls into other WASM agent contracts  
- **Compound** pipelines with async frontier reconciliation (request IDs, topics, callback protocol)  
- Replacing FRONTIER’s literal `"pending"` with a typed correlation ID once messaging supports it  

Incremental behavior is intentional: the shipped opcodes above are the **contract of record**.

---

## Related documentation

- [`spacekit-contract-sdk` README](../../../spacekit-contract-sdk/README.md) — host ABI
- `spacekit-js` — `CONTRACT_SDK_HOST_REFERENCE.md`, brain registry docs  
- [swtch.ai RouteKit contract docs](https://swtch.ai/docs) — product-facing opcode summary  
