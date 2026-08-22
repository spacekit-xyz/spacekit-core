# SpaceKit security audit scope

> Status: Draft requiring revalidation  
> Original scope date: 2026-01-23

This planning artifact has not been reconciled with every current node,
runtime, contract, and network profile. It must not be cited as evidence that
an audit occurred or that a component is production-ready.

## Proposed scope

- SpaceKit JS transaction validation and signature verification;
- receipt hashing, Merkle proofs, headers, and chain linkage;
- compute-node authentication and policy-gated host calls;
- browser and node storage adapters at their public boundaries;
- JSON-RPC and HTTP route authorization;
- deterministic inference and asynchronous-to-synchronous host bridges;
- cross-language canonical signing vectors;
- manifest admission, role enforcement, and key rotation;
- contract deployment, upgrade, and entitlement boundaries.

## Required artifacts

- current threat model;
- signed transaction and replay test vectors;
- host ABI and deterministic-execution specification;
- network profile and manifest schemas;
- recovery, rollback, and key-rotation procedures;
- reproducible test and build records.

## Explicit exclusions

Any exclusion must be approved for the audit revision and documented with its
residual risk. Proprietary storage internals, third-party model weights, and
external settlement providers require separate owner evidence even when their
public integration boundaries remain in scope.
