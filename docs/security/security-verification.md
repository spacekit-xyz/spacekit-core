# Security verification

Use this guide to verify SpaceKit compute-node and JavaScript-runtime security
properties before and after a release.

This document describes verification goals, not proof that a release is secure.
Record the revision, binaries, configuration, and results for every run.

## Offline checks

Run project-local tests from the current migration layout:

```bash
# Compute-node cryptography, authentication, consensus, and VM metering
cd infra/spacekit-compute-node
cargo test --lib

# JavaScript canonical signing and secure defaults
cd ../../../runtimes/spacekit-js
npm test
```

Contract tests that use Anvil and Foundry are compatibility checks. They do not
prove SpaceKit consensus or public-network admission.

Cross-language canonical encoders must produce identical signing bytes. A
schema change is incomplete until Rust and TypeScript vectors are updated
together.

## Live compute verification

Use the compute-node security harness when available:

```bash
cd infra/spacekit-compute-node
./scripts/security-verification.sh
```

The harness must first prove the service and positive control are healthy. It
should then verify:

- mutating endpoints reject unauthenticated, stale, replayed, and malformed
  requests;
- keymaster responses and logs contain no private key material;
- forged or absent intent signatures are refused;
- unlisted web origins receive no CORS grant;
- production-shaped services bind to the intended interface;
- entitlement reads enforce chain ID, confirmation, quorum, and reserve limits.

A rejection-only test is invalid if the service or positive control is down.
Confirm the harness is exercising the binary built from the revision under
review rather than a stale `CARGO_TARGET_DIR`.

## Profile-driven network verification

```bash
spacekit init
spacekit network init --profile local --force
spacekit network up
spacekit network doctor
spacekit network status --detailed
```

Canonical defaults are storage `:3030`, compute `:9000`, messaging listen
`:7100`, messaging HTTP `:17000`, and gateway `:8080`. Consult the
[developer network guide](../guides/developer-network-setup.md) instead of
historical examples.

Anvil at `:8545` is valid only for EVM entitlement and contract tests.

## Production configuration review

Confirm at minimum:

| Setting | Required posture |
|---|---|
| `SPACEKIT_DEV_MODE` | unset |
| `SPACEKIT_KEYMASTER_SECRET` | generated and loaded from a secret manager |
| `SPACEKIT_ADMIN_DIDS` | explicit operator DIDs |
| `SPACEKIT_API_ALLOWED_ORIGINS` | explicit trusted origins, never `*` |
| `SPACEKIT_ENTITLEMENT_MIN_AGREEMENT` | independent providers, at least 2 |
| `SPACEKIT_ENTITLEMENT_CONFIRMATIONS` | suitable for the settlement chain |
| `SPACEKIT_MIN_VALIDATOR_STAKE_UNITS` | reviewed against Sybil cost |
| listener bind addresses | loopback or protected private interfaces |

The proprietary storage-node implementation has a separate private security
scope. Public interfaces and integration assumptions still belong in the
network threat model.

## Known verification gaps

Do not represent the following as proven without current test evidence:

- multi-node consensus under partition, equivocation, and restart;
- fraud-proof re-execution and challenge windows;
- sustained hostile-contract load and memory behavior;
- external audit of settlement and entitlement contracts;
- browser storage hardening against device or extension compromise;
- production key custody, revocation, and disaster recovery.

Track operational exercises in the
[runbook](../../operations/spacekit-runbook/README.md). Security-sensitive
findings must follow the private process in [`SECURITY.md`](../../SECURITY.md).
