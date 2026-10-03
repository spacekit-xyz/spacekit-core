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

Contract tests that use Anvil and Foundry are EVM compatibility checks only.
They do not prove SpaceKit consensus or public-network admission, and they
cover no SpaceKit payment path: ASTRA is the only currency and lives only in
native balances on the SpaceKit chain (see
[`PAYMENTS_AND_CURRENCY.md`](../PAYMENTS_AND_CURRENCY.md)).

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
- `POST /v1/payments/verify` refuses a failed transaction, a wrong payee, an
  amount below the price, and a transaction hash it has already accepted;
- contracts refuse non-ASTRA assets (`-22`) and `payment_vault_charge`, and a
  contract cannot spend its caller's balance;
- entitlement checks use read-only `OP_VERIFY_LISTING` + `OP_GET_LISTING` calls
  to the `astra-entitlement-ledger` contract;
- on a consensus (PoA/PoS) network, the faucet, rollup settlement, PoTW awards
  and treasury host-bridge disbursement are refused.

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

Anvil at `:8545` is valid only for EVM compatibility tests. The Ethereum
DAI/USDC entitlement deposit reader has been removed.

## Production configuration review

Confirm at minimum:

| Setting | Required posture |
|---|---|
| `SPACEKIT_DEV_MODE` | unset |
| `SPACEKIT_KEYMASTER_SECRET` | generated and loaded from a secret manager |
| `SPACEKIT_ADMIN_DIDS` | explicit operator DIDs |
| `SPACEKIT_API_ALLOWED_ORIGINS` | explicit trusted origins, never `*` |
| `SPACEKIT_MIN_VALIDATOR_STAKE_WEI` | native ASTRA, reviewed against Sybil cost (default 10,000 ASTRA) |
| `SPACEKIT_GENESIS_ALLOC_FILE` | unset unless the genesis allocation was reviewed (height 0 only) |
| listener bind addresses | loopback or protected private interfaces |

The storage-node source is included in the repository under proprietary terms.
Its authorization, encryption, persistence, and integration assumptions belong
in the repository security scope and network threat model.

## Known verification gaps

Do not represent the following as proven without current test evidence:

- multi-node consensus under partition, equivocation, and restart;
- fraud-proof re-execution and challenge windows;
- sustained hostile-contract load and memory behavior;
- external audit of native rewards execution and of the treasury, paymaster and
  entitlement contracts;
- browser storage hardening against device or extension compromise;
- production key custody, revocation, and disaster recovery.

Track operational exercises in the
[runbook](../../operations/spacekit-runbook/README.md). Security-sensitive
findings must follow the private process in [`SECURITY.md`](../../SECURITY.md).
