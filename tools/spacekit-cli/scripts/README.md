# SpaceKit CLI scripts

## `cli-smoke-test.sh`

End-to-end smoke test for the `spacekit` CLI. Runs commands, saves stdout/stderr under a scratch directory, and checks exit codes and key log patterns.

### Prerequisites

1. **Initialize identity** (once):

   ```bash
   spacekit init --algorithm kyber1024
   ```

2. **Network profile** (once; optional if defaults are fine):

   ```bash
   spacekit network init
   # custom ports: spacekit network init --storage-port 4030 --no-compute
   ```

3. **Start the local network** (keep running in another terminal):

   ```bash
   spacekit network up
   # subset: spacekit network up --only storage,messaging
   ```

4. **Build the CLI** (if not already built):

   ```bash
   cargo build -p spacekit
   ```

### Run

From the repo root:

```bash
chmod +x spacekit-cli/scripts/cli-smoke-test.sh
./spacekit-cli/scripts/cli-smoke-test.sh
```

Environment variables:

| Variable | Default | Purpose |
|----------|---------|---------|
| `SPACEKIT_BIN` | `target/debug/spacekit` | CLI binary path |
| `SPACEKIT_CLI_SCRATCH` | `spacekit-cli/scratch/cli-smoke-<timestamp>` | Output directory |
| `SPACEKIT_STORAGE_URL` | `http://127.0.0.1:3030` | Storage API base URL |
| `SPACEKIT_GATEWAY_URL` | `http://127.0.0.1:9000` | Gateway / compute URL for `connect compute` |
| `SPACEKIT_MESSAGING_BOOTSTRAP` | `/ip4/127.0.0.1/tcp/7000` | Messaging bootstrap multiaddr |
| `CLI_SMOKE_CONFIGURE_CONNECT` | `1` | Set `0` to skip writing `[connections]` in `~/.spacekit/config.toml` |
| `SKIP_MESSAGE` | `0` | Set `1` to skip `message list` |
| `BUILD_HELLO_WASM` | `0` | Set to `1` to build and test `contract deploy` / `call` |
| `STORAGE_NODE_BIN` | `target/debug/spacekit-storage-node` | Storage-node binary for MCP tests |
| `COMPUTE_NODE_BIN` | `target/debug/spacekit-compute-node` | Compute-node binary for MCP tests |

When `CLI_SMOKE_CONFIGURE_CONNECT=1` (default), the script configures `connect storage`, `connect compute`, and `connect messaging` to local `network up` defaults, then runs `connect test` for each.

### What it exercises

| Area | Commands |
|------|----------|
| Identity | `--help`, `did list`, `vm balance` |
| Network | `network status` (log pattern check) |
| Connect | `connect status`; optional configure + `connect test storage\|compute\|messaging` |
| Crypto | `keypair`, `encrypt`/`decrypt` (ECIES), `encapsulate` |
| Storage | `storage stats`, `storage store` |
| Repo | `repo init`, `add`, `status`, `commit`, `push` (push may skip if auth missing) |
| Workspace | `workspace create`, `show`, `list`, `export`, `import --replace` |
| Message | `message list` (skipped if messaging not running or `SKIP_MESSAGE=1`) |
| Contract | `vm fund`, `contract deploy`, `contract call` (optional, needs WASM) |
| MCP | Storage-node MCP: `tools/list`, `initialize`, `ping` via stdio |
| MCP | Compute-node MCP: `tools/list`, `initialize`, `block_latest.v1`, unknown-method `-32601` |

MCP tests run standalone (each spawns its own in-process node) and do **not** require `network up`. Set `STORAGE_NODE_BIN` / `COMPUTE_NODE_BIN` to override binary paths.

**Not covered here** (use dedicated soaks instead): `content soak`, `growformer-*-soak`. Prerequisite is still `spacekit network up`.

### Output layout

```text
spacekit-cli/scratch/cli-smoke-YYYYMMDD-HHMMSS/
  SUMMARY.md
  logs/           # one .log + .exit per step
  artifacts/      # test files, encrypted blobs, contract id
  repo-work/      # ephemeral repo checkout
  keys/           # ECIES keys generated for roundtrip test
```

Exit code `0` if all non-skipped steps pass; `1` if any step failed.

### Windows

Run under **Git Bash**, **WSL**, or similar. Native `cmd.exe` is not supported by this script. Paths with spaces should be quoted when setting env vars.

The `spacekit` binary itself runs on Windows; use the same `spacekit init` and `spacekit network up` flow, then run this script from Git Bash with `SPACEKIT_BIN` pointing at `spacekit.exe`.

---

## `content-monetization-soak.sh`

End-to-end **paid content** soak (publish → pay → record-payment → settlement → view). Wrapper around [`../../spacekit-storage-node/scripts/content-monetization-soak.sh`](../../spacekit-storage-node/scripts/content-monetization-soak.sh).

### Prerequisites

Same as `cli-smoke-test.sh`: `spacekit init`, `spacekit network up` (other terminal), built CLI.

### Run

```bash
# Preferred (from repo root)
spacekit content soak dev

# Or directly
chmod +x spacekit-cli/scripts/content-monetization-soak.sh
./spacekit-cli/scripts/content-monetization-soak.sh dev
```

| Variable | Default | Purpose |
|----------|---------|---------|
| `SPACEKIT` / `SPACEKIT_BIN` | `target/release/spacekit` | CLI binary |
| `SPACEKIT_STORAGE_URL` | `http://127.0.0.1:3030` | Storage health |
| `SPACEKIT_ENTITLEMENT_CONTRACT_ID` | (unset) | Required for `live` mode |

**Pass:** `Soak summary: 5 passed, 0 failed`

Full reference: [spacekit-storage-node/scripts/README.md](../../spacekit-storage-node/scripts/README.md)
