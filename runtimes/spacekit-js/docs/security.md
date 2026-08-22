# Security

## Keys and signing
- Ed25519 is the default signing path for transactions and bundles.
- PQ signing (SPHINCS+) is optional and policy-driven.

## Rollup policy
Compute-node can enforce key expiry and revocation via policy file.

## API access
Rollup endpoints can require API keys or JWTs. Configure with:
- `SPACEKIT_ROLLUP_API_KEY`
- `SPACEKIT_ROLLUP_JWT_SECRET`

## Extension wallet
Keys are encrypted with PBKDF2 + AES-GCM and stored in extension local storage.
