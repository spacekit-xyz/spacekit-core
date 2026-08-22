# Quantum DID — EVM contracts (experimental)

Solidity reference contracts for storing quantum DID metadata on EVM chains.

**Not production-ready for on-chain signature verification.** Read [EXPERIMENTAL.md](EXPERIMENTAL.md) before deploying.

## Contracts

| Contract | Purpose |
|----------|---------|
| `QuantumDIDRegistry.sol` | DID registration, credential hash registry |
| `KeyBackupSLA.sol` | Key backup SLA metadata (client-side encryption) |

## Development

```bash
npm ci
npx hardhat compile
npx hardhat test
```

Deploy script: `deploy-quantum-did.js` (configure network in `hardhat.config.ts`).

## Verification

Validate SPHINCS+ signatures with the [`spacekit-did`](../../spacekit-did/README.md) Rust crate before trusting on-chain state. On-chain `verifyQuantumSignature` intentionally reverts with `OnChainVerificationUnsupported`.
