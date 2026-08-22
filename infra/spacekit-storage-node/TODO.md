# TODO

This file tracks concrete milestones. See `ENTERPRISE_GRADE_ROADMAP.md` for full detail.

## Milestone 1.0: Public Release Baseline
- [x] Enforce ACLs/invitations on all protected endpoints
- [x] Gate file retrieval on explicit authz checks
- [x] Implement decrypt-on-retrieve for user-encrypted content
- [x] Bind signatures to DID-owned public keys (DID resolution)
- [x] Add request timeouts or document required reverse proxy
- [x] Add connection limits or document required reverse proxy
- [x] Add distributed/persistent rate limiting for multi-node
- [x] Validate WAL recovery + implement consistent snapshotting
- [x] Align docs to implemented vs planned features
- [x] CI: run full test/clippy/audit with release checklist

## Milestone 1.1: DB-Competitive Core
- [ ] Integrate `query_planner.rs` with execution paths
- [ ] Use `indexes.rs` in real query execution
- [ ] Add isolation levels + multi-statement transactions
- [ ] EXPLAIN/ANALYZE API endpoint + metrics
- [ ] Implement SQL subset parser for supported features

## Milestone 1.2: Object-Store Competitive
- [ ] Replication or erasure coding with clear SLOs
- [ ] Multi-region replication + consistency model
- [ ] Bucket/object API alongside fact packages
- [ ] Lifecycle policies (TTL, archival, retention)

## Milestone 1.3: Decentralized-Storage Competitive
- [ ] Incentive model (proof-of-storage + retrieval economics)
- [ ] Content addressing + verifiable replication
- [ ] Hardened gossip messaging + network health controls

## Backlog: Known Gaps (by area)

### Security / Crypto / Access Control
- `src/quantum.rs`: placeholder keys/encryption when `quantum` is off
- `src/api/mod.rs`: group-shared file access depends on group membership data

### Multi-node Ops
- Use SpaceKit distributed rate limiting:
  - Enable `rate-limit-spacekit`
  - Set `SPACEKIT_RATE_LIMIT_URL` to the coordinator base URL
  - On the coordinator, set `SPACEKIT_RATE_LIMIT_ENABLE_SERVICE=1`
- `src/fact_storage.rs`: placeholders for DID resolution, KMS, policy enforcement
- `src/nft_storage.rs`: demo-grade signature (new keypair per NFT)

### Networking / Messaging / HA / Sharding
- `src/network.rs`: async response handling + messaging node queries
- `src/server_message_routing.rs`: gossipsub publish/forward
- `src/server_routing.rs`: gossipsub subscribe/unsubscribe + send
- `src/ha.rs`: health checks, heartbeat, voting, split-brain
- `src/sharding.rs`: data migration

### Storage / Transactions / Rewards
- `src/transaction.rs`: proper snapshot
- `src/rewards.rs`: reputation/uptime/retrieval bonuses + chain integration

### CLI / Ops
- `src/bin/standalone.rs`: placeholder status check

