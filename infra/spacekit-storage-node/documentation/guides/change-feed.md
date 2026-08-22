# Change Feed (Phase 4)

Reactive agents need to react to state changes, not poll. The change feed
publishes a monotonically increasing stream of events that subscribers
consume over **Server-Sent Events** (SSE) at `GET /api/changes`.

## Wire Format

```
event: ...
id: <seq>
data: {"seq": 42, "occurred_at": "...", "kind": "tx.committed", "key": "<tx-id>", ...}

```

`id` is the monotonic `seq`. Subscribers MUST resume by passing
`Last-Event-ID: <last_seq>` (or the equivalent `?since-seq=<seq>` query
parameter) on reconnect. Missing this means subscribers may have gaps in
their view; the storage node has no way to backfill from the in-memory
buffer once a `seq` has aged out.

## Event Vocabulary

Phase 0/1 wire up the following `kind`s; future phases extend the
vocabulary:

| Kind                | When                                          | Key                  |
|---------------------|-----------------------------------------------|----------------------|
| `tx.committed`      | Successful `commit_transaction`               | transaction id       |
| `tx.rolled_back`    | Successful `rollback_transaction`             | transaction id       |
| `sandbox.created`   | `POST /api/sandboxes`                         | sandbox id           |
| `sandbox.committed` | Sandbox commit (not dry-run)                  | sandbox id           |
| `sandbox.discarded` | `POST /api/sandboxes/{id}/discard`            | sandbox id           |

Future:

- `doc.put` / `doc.delete` — document store mutations.
- `repo.commit` — new repository commit landed.
- `vector.upserted` — vector index updates.
- `fts.indexed` — full-text index updates.

## Backpressure & Slow Consumers

A `tokio::sync::broadcast` would silently drop messages for slow
consumers; that's catastrophic for agents (a missed `sandbox.committed`
means the agent waits forever). Instead:

- Each subscriber has a **bounded `mpsc` queue** (default 64).
- Slow subscribers (full queue) are **disconnected** by the publisher.
- Subscribers MUST handle disconnects and resume by `seq`.

Disconnect is signalled by the SSE stream closing. The agent reconnects with
`Last-Event-ID: <last_seq>` and the storage node replays buffered events with
`seq > last_seq`.

## Durability vs throughput (`change_log.jsonl`)

Each published event is appended to `<data_dir>/change_log.jsonl` with
`flush` + `fsync` **before** the publisher fans out to SSE subscribers, so
slow disks cannot cause agents to observe a `seq` that is not on disk. That
per-event `fsync` is the correct durability default and can become the
throughput bottleneck under very high publish rates (for example hundreds of
commits per second sustained).

**Operational guidance:** benchmark publish QPS on your storage media before
opening bursty agent fleets. If `fsync` dominates, the usual next step is a
**group commit** policy (flush every *N* events or every *T* milliseconds,
whichever comes first) while preserving monotonic `seq` assignment and only
signalling subscribers after the batch is durable — same ordering guarantees,
lower write amplification.

## Disk ring buffer (Future)

The in-process ring buffer (default 2048 events) is bounded; the JSONL file
already survives restarts for `Last-Event-ID` resume. A binary ring segment
with mmap is a possible future optimization for very large backlogs.

## Filter Globs

`?kind=tx.*,sandbox.*` filters the stream. Globs match a dotted prefix:

- `*` — match anything.
- `tx.*` — match `tx.committed`, `tx.rolled_back`.
- `tx.committed` — exact match.

Multiple globs are comma-separated.

## Gossipsub Federation (Future)

A future enhancement publishes the same events on the libp2p gossipsub
topic `spacekit/changes/v1` so multi-node deployments can share a single
change view. Phase 4 ships the SSE endpoint; the gossipsub bridge is a
deferred follow-up that re-publishes events between nodes when they're on
the same swarm.
