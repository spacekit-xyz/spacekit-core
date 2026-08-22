# P-05: Key rotation

Identity rotation and manifest-authority rotation are separate operations.

Rotate an owned DID only after backing up its current keys and confirming the DID:

```bash
spacekit did list --owned-by-me --detailed
spacekit did update did:spacekit:example --rotate-keys
spacekit did verify did:spacekit:example --detailed
```

For a manifest authority, generate a new signing pair without overwriting the old files, update the unsigned manifest's key identity through review, then sign and verify:

```bash
spacekit network manifest keygen \
  --public-key manifest-next.pub.hex \
  --secret-key manifest-next.sec.hex
spacekit network manifest sign unsigned-manifest.json \
  --key-id did:spacekit:authority#network-signing-next \
  --public-key manifest-next.pub.hex \
  --secret-key manifest-next.sec.hex \
  --output manifest-next.json
spacekit network manifest verify manifest-next.json
```

Distribute and pin the new public-key fingerprint out of band before nodes rejoin. The CLI does not automatically publish DID updates, distribute manifests, overlap trust roots, revoke old keys, or restart nodes.
