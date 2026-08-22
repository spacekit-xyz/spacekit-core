# P-04: Manifest mismatch or tampering

Stop before replacing or re-signing anything:

```bash
spacekit network down
spacekit network config show
spacekit network manifest verify /absolute/path/to/manifest.json
shasum -a 256 /absolute/path/to/manifest.json
```

Compare the manifest digest, `signature.key_id`, public-key fingerprint, network ID, chain ID, protocol, genesis hash, bootstrap endpoints, roles, and members with the approved release through an independent channel.

For an authorized new manifest, verify it first and then rebuild the profile through the admission path:

```bash
spacekit network manifest verify approved-manifest.json
spacekit network join --manifest approved-manifest.json --role subscriber --force
spacekit network up -d
spacekit network doctor
```

Use the node's actual approved role in place of `subscriber`. Public operator/validator joins additionally require a reachable published operator service fact. Never “fix” a signature by editing signed JSON or by trusting a replacement key delivered with the suspect manifest.
