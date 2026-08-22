# SpaceKit network manifest v1

`network-manifest-v1.schema.json` describes private and public network identity,
genesis, bootstrap endpoints, admitted node roles, and detached signature metadata.

## Canonical signing payload

1. Serialize the manifest as compact UTF-8 JSON.
2. Omit the top-level `signature` member.
3. Sort every JSON object's keys lexicographically, recursively.
4. Preserve array order and JSON scalar values.

`NetworkManifest::canonical_unsigned_bytes()` implements these rules. The signature
metadata records `sphincs128f`, `hex` or `base64` encoding, a DID key ID, the raw
verification key, and an optional signing timestamp. Loading a public manifest
cryptographically verifies the signature with SpaceKit's existing SPHINCS-128f
identity primitive; shape-only validation is not an admission decision.

Public manifests require a signature, at least one P2P bootstrap, at least one RPC
bootstrap, and one or more of `subscriber`, `operator`, or `validator`. Private
manifests may be unsigned when trust is distributed out of band.

Private manifests embed `genesis.document`; its canonical BLAKE3 hash must equal
`genesis.hash`. `members` grants roles to DIDs. Public subscribers need no member
entry, while public operators and validators require an explicit member grant and
a reachable published operator service fact.

Sign with:

`spacekit network manifest sign manifest.json --key-id did:spacekit:...#key-1 --public-key public.hex --secret-key secret.hex`

The key files are raw SPHINCS-128f bytes encoded as hex. This intentionally uses
the current SpaceKit DID primitive. A later DID resolver can replace the embedded
`public_key` after key-document resolution is available; `key_id` is already
mandatory so that migration does not change the signed payload or admission model.
