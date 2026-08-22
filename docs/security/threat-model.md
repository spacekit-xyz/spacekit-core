# SpaceKit threat model

> Status: Draft requiring revalidation  
> Original model date: 2026-01-23

This document is a starting point, not a completed security assessment.

## Assets

- identity, wallet, validator, manifest-signing, and encryption keys;
- contract, ledger, repository, and operator state;
- transaction authenticity, ordering, and replay protection;
- admission manifests, genesis configuration, and role grants;
- stored artifacts, messages, model inputs, and generated outputs;
- release artifacts and dependency integrity.

## Trust assumptions

- browsers, extensions, and end-user devices can be compromised;
- networks are hostile to confidentiality, integrity, and availability;
- remote RPC and bootstrap providers can lie, equivocate, or become stale;
- node operators can be faulty or malicious;
- contracts and host modules must reject nondeterminism and excessive resource
  use;
- compatibility simulators and local Anvil chains are not production trust
  roots;
- proprietary storage internals require a separate private assessment.

## Primary threats

- key exfiltration, accidental publication, or weak custody;
- forged signatures, replay, nonce manipulation, and chain-ID confusion;
- malicious manifests, authority substitution, and unauthorized role changes;
- invalid blocks, roots, receipts, proofs, or RPC responses;
- state poisoning, rollback, fork, and federation conflicts;
- unsafe host calls, nondeterministic inference, and resource exhaustion;
- exposed operator APIs, permissive CORS, and public bind addresses;
- dependency, build, release, and generated-artifact compromise.

## Required controls

- domain-separated signatures, nonces, timestamps, and chain IDs;
- independently pinned manifest-authority fingerprints;
- fail-closed authorization and explicit trusted origins;
- deterministic VM limits, receipt proofs, and verified block replay;
- encrypted transport, host isolation, and secret-manager-backed custody;
- reproducible builds and source-to-artifact hashes;
- tested key rotation, manifest recovery, rollback, and incident procedures.

## Open risks

The current public documentation does not prove multi-node Byzantine behavior,
fraud-proof re-execution, sustained hostile load, browser key hardening,
production HSM custody, or external contract audit completion. These remain
release gates until supported by current evidence.
