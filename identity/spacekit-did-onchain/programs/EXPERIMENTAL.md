# Experimental: Solana on-chain components

The Anchor program in `quantum-did-solana/` is a **reference integration**, not production-ready on-chain verification.

## Limitations

- **SPHINCS+ signature verification is not implemented on-chain.** Registration and credential instructions that call `verify_quantum_signature` will return `OnChainVerificationUnsupported` until a verifier (precompile, zk proof, or oracle) is integrated.
- Use the **`spacekit-did` Rust library** (or your own verifier) to validate quantum signatures before trusting identity or credentials.
- Program ID, account layout, and instruction set may change without a major version bump while this component remains experimental.

## Safe deployment

1. Verify all quantum signatures off-chain before submitting transactions that assume validity.
2. Do not use this program for access control, slashing, or high-value operations without a completed on-chain verification design and audit.
3. Report security issues per [spacekit-did/SECURITY.md](../../spacekit-did/SECURITY.md).

## Related

- EVM contracts: [../quantum-evm-contracts/EXPERIMENTAL.md](../quantum-evm-contracts/EXPERIMENTAL.md)
- Off-chain library (supported): [../../spacekit-did/README.md](../../spacekit-did/README.md)
- Bridge helpers: [../bridges/](../bridges/)
