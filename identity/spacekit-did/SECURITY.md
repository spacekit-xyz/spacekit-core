# Security Policy

## Supported versions

| Version | Supported |
| ------- | --------- |
| 0.1.x   | Yes       |

Security fixes are provided for the latest `0.1.x` release on [crates.io](https://crates.io/crates/spacekit-did) and the default branch of this repository.

## Reporting a vulnerability

**Please do not open a public GitHub issue for security vulnerabilities.**

Report security issues privately to:

- **Email:** security@spacekit.xyz

Include:

- A description of the issue and impact
- Steps to reproduce
- Affected versions and components (`spacekit-did` library; optionally `spacekit-did-onchain`)
- Proof-of-concept code or logs if available

We aim to acknowledge reports within **3 business days** and provide an initial assessment within **10 business days**.

## System context

Identity in SpaceKit spans several repositories. See [ARCHITECTURE.md](ARCHITECTURE.md):

- **`spacekit init`** (CLI) — local `~/.spacekit` and placeholder `did:spacekit:user:{uuid}`
- **`spacekit did create`** — registry-format `did:spacekit:testnet:…` and optional `POST /v1/did/register`
- **`spacekit-compute-node`** — validates registration when testnet is running; persistent registry via VM is still in progress
- **This crate** — SPHINCS+ wallet and credential primitives used by the above

Report integration bugs (e.g. init DID vs registry DID, register without persistence) with enough detail to reproduce across components.

## Scope

### In scope

- `spacekit-did` Rust crate (`src/`, cryptographic primitives, wallet, credential issuance/verification)
- Memory safety issues, signature forgery, key handling flaws, and incorrect verification logic in the library

### Out of scope (unless combined with a library flaw)

- **`spacekit-did-onchain`** (EVM contracts, Solana program, bridge crate) — tracked separately; on-chain SPHINCS+ verification is not implemented. See [spacekit-did-onchain](../spacekit-did-onchain).
- Third-party dependencies (report upstream when appropriate; we will coordinate releases)
- Social engineering, physical access, or compromised host environments
- Issues in unreleased or forked deployments without a reproducible impact on this repository

## Safe use

- Generate and store **private keys** only in trusted, offline or HSM-backed environments.
- Always **verify credentials and signatures** before trusting claims; use `VcVerifier` and `SphincsPlus::verify` for VPN access VCs.
- Treat `did:spacekit:*` as a **custom method** — not a claim of full W3C DID / VC conformance.
- For chain deployments, use this library for verification; see `spacekit-did-onchain` for experimental contracts/programs.

## Disclosure

We follow coordinated disclosure. After a fix is available, we will credit reporters who wish to be acknowledged (unless you request anonymity).
