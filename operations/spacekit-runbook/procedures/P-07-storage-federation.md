# P-07: Storage federation failure

Capture both source and destination evidence before retrying:

```bash
spacekit network status --detailed
spacekit network doctor
spacekit network logs --service storage --lines 300
spacekit network config show
spacekit storage stats --storage-url "$SPACEKIT_STORAGE_URL"
curl --fail --show-error --silent "$SPACEKIT_STORAGE_URL/health"
```

Run the same checks against the destination URL. For a workspace handoff, preserve the exported bundle and verify source access before importing again:

```bash
spacekit workspace export WORKSPACE_ID -o handoff.json
spacekit workspace import handoff.json \
  --source-url "$SPACEKIT_STORAGE_URL" \
  --owner-did "$DESTINATION_OWNER_DID"
```

Add destination `--storage-url` or authentication flags required by `spacekit workspace import --help`. Do not bypass handoff signatures, blob/fact authentication, ownership checks, or content hashes. A successful retry does not prove every object replicated; verify the destination workspace/repository and required blobs explicitly.
