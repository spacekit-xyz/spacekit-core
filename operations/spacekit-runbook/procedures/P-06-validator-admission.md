# P-06: Validator admission

Admission is a reviewed manifest change, not a local role edit.

1. Verify the candidate DID and operational readiness.
2. Add the DID with role `validator` to an unsigned manifest and include it in the enabled top-level roles.
3. Review chain ID, genesis, protocol, bootstrap endpoints, and the complete member set.
4. Sign and verify the new manifest:

```bash
spacekit network manifest sign unsigned-manifest.json \
  --key-id did:spacekit:authority#network-signing \
  --public-key manifest.pub.hex \
  --secret-key manifest.sec.hex \
  --output approved-manifest.json
spacekit network manifest verify approved-manifest.json
```

On the candidate:

```bash
spacekit network join --manifest approved-manifest.json --role validator --force
spacekit network config show
spacekit network up -d
spacekit network doctor
spacekit network status --detailed
spacekit network peers --detailed
```

Private validator startup requires the local DID in both manifest membership-derived admission and `[blockchain.validators].peers`. Public validator join additionally probes manifest RPC endpoints for a published operator service fact. The CLI does not coordinate activation height or quorum changes automatically.
