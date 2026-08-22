# P-01: Startup triage

Use the same identity home and `SPACEKIT_NETWORK_CONFIG` that the failed node uses.

```bash
spacekit network config path
spacekit network config show
spacekit network doctor
spacekit network status --detailed
spacekit network logs --lines 200
```

If the profile uses a manifest, verify the exact configured file:

```bash
spacekit network manifest verify /absolute/path/to/manifest.json
```

Correct only the diagnosed profile, identity, admission, manifest, endpoint, or port problem. Then:

```bash
spacekit network down
spacekit network up -d
spacekit network status --detailed
```

`network status` can report a stopped network without failing. Treat `network doctor`, startup exit status, logs, and live endpoint probes as the evidence. Do not delete data during triage.
