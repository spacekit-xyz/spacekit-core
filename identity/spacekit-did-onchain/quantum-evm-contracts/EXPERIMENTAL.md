# Experimental: EVM on-chain components

Solidity contracts in this directory are a **reference integration** for storing DID metadata and credential hashes on EVM chains. They are **not** production-ready for quantum signature enforcement on-chain.

## Limitations

- **`verifyQuantumSignature` does not perform SPHINCS+ verification.** It reverts with `OnChainVerificationUnsupported`. Validate signatures off-chain with the `spacekit-did` crate before relying on registry state.
- Gas cost of full SPHINCS+ verify on EVM is prohibitive without a precompile or hybrid (optimistic + challenge) design.
- Contract interfaces and storage layout may change while marked experimental.

## Safe deployment

1. Run signature verification in your backend or client using `spacekit_did::SphincsPlus::verify`.
2. Treat on-chain records as **attestations of hashes and metadata**, not as cryptographic proof of quantum signatures until documented otherwise.
3. Complete an independent audit before mainnet deployment with user funds or access control.

## Related

- Solana program: [../programs/EXPERIMENTAL.md](../programs/EXPERIMENTAL.md)
- Off-chain library (supported): [../../spacekit-did/README.md](../../spacekit-did/README.md)
- Bridge helpers: [../bridges/](../bridges/)
