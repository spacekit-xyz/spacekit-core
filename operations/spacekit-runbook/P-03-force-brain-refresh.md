# P-03: Force Brain Refresh

Triggered when:
- S-001 diagnosis identifies on-disk brain corruption
- S-005 confirms this node's brain disagrees with network canonical
- Operator wants to verify brain integrity proactively

## When NOT to run this

- If the on-disk brain is fine and only a remote inference shows mismatch
  (that's a different issue — see S-005 procedures for remote inference)
- Before confirming the network-canonical hash is signed by the genesis
  authority key (otherwise this procedure imports whatever the storage
  node serves, with no verification)

## Required state before running

- `SPACEKIT_API_KEY` set and valid
- `SPACEKIT_STORAGE_URL` reachable (verify with `curl -sS $SPACEKIT_STORAGE_URL/health`)
- Operator key file present at `~/.spacekit/operator.key`
- Genesis authority public key embedded in the node binary (compiled in;
  if missing, the binary itself is compromised — STOP)

## Procedure

### Step 1: Confirm signature on the proposed brain

The storage node serves a signed manifest at
`$SPACEKIT_STORAGE_URL/agent/latest.json`. The manifest is:

```json
{
  "model_hash": "0x...",
  "brain_url": "https://.../agent-v3.brain",
  "size_bytes": 12582912,
  "signed_at": 1700000000,
  "signature": "0x..."
}
```

Verify the signature locally BEFORE fetching the brain bytes:

```bash
spacekit-cli agent verify-manifest --url $SPACEKIT_STORAGE_URL/agent/latest.json
```

Expected output:
```
Manifest signature: VALID
Signing key: genesis-authority-v1
Manifest model_hash: 0x...
```

**If signature is INVALID: STOP. Do not proceed. This is a supply-chain
attack indicator. Escalate to S-005 procedure P-11.**

### Step 2: Compare manifest hash against last-known-good

```bash
spacekit-cli agent compare-hash --against-history --against-peer-majority
```

Expected output:
```
Manifest model_hash matches last-ratified network model_hash: YES
Manifest model_hash matches peer majority: YES (18/20 peers)
```

**If either is NO: STOP. The manifest may be valid but stale or
incongruous with the rest of the network. Investigate before proceeding.**

### Step 3: Fetch and verify the brain bytes

```bash
spacekit-cli agent fetch \
  --url $(cat manifest.json | jq -r .brain_url) \
  --expected-hash $(cat manifest.json | jq -r .model_hash) \
  --output /tmp/agent-pending.brain
```

The fetch:
- Downloads the brain bytes from `brain_url`
- Computes their hash
- Verifies the hash matches `model_hash` from the manifest
- Aborts on mismatch

Expected output:
```
Downloaded: 12582912 bytes
Computed hash: 0x... (matches expected)
Brain saved to /tmp/agent-pending.brain
```

### Step 4: Encrypt and install

```bash
spacekit-cli agent install \
  --source /tmp/agent-pending.brain \
  --operator-key ~/.spacekit/operator.key
```

This:
- Encrypts the brain bytes with the operator key
- Writes to `~/.spacekit/agent/current.brain.enc` atomically (write-temp,
  fsync, rename)
- Backs up the previous brain to `~/.spacekit/agent/previous.brain.enc`

### Step 5: Restart and verify

```bash
systemctl restart spacekit-node
sleep 10
spacekit-cli agent brain-status
```

Expected output:
```
On-disk brain hash: 0x... (matches network)
Brain loaded: YES
Inference test: PASS
```

### Step 6: Clean up

```bash
rm /tmp/agent-pending.brain
rm manifest.json
```

The previous brain at `~/.spacekit/agent/previous.brain.enc` is kept for
one challenge window in case a rollback is needed. Do NOT delete it
manually.

## Failure modes

| Symptom | Likely cause | Next step |
|---------|--------------|-----------|
| `Manifest signature: INVALID` | Storage node compromised OR genesis authority key changed | Escalate S-005 P-11 |
| `Hash mismatch on fetch` | Brain bytes corrupted in transit OR storage node serving wrong bytes | Retry once; if persists, escalate |
| `Node won't restart after install` | Operator key may have changed; cannot decrypt new brain | Check operator key path, retry install with `--debug` |
| `Brain loaded but inference fails` | Brain format compatibility issue (model version vs runtime version) | Check runtime version against brain's training metadata |

## Required log events

After running this procedure, the following events must appear in
`spacekit-log`:

- `AgentBrainFetched` (Info) — Step 3 completion
- `AgentBrainLoaded` (Info) — Step 5 success
OR:
- `AgentBrainHashMismatch` (Critical) — Step 3 failure (should not happen
  if Step 1 + 2 were performed correctly)

If neither pair appears, the procedure didn't complete; check the
underlying CLI output for errors.

## Time budget

Expected: 2-5 minutes including network fetch.
If exceeded 15 minutes: storage node likely unreachable; abort and
escalate to S-015.
