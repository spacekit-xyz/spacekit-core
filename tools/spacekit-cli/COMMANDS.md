# SpaceKit CLI — command reference

The full CLI is built from `spacekit-cli/src/full_client.rs`. Run `spacekit <command> --help` for flags and subcommands not listed here.

Global options (most commands):

| Flag | Default | Meaning |
|------|---------|---------|
| `-c, --chain` | `spacekit` | Chain context (`spacekit`, `ethereum`, `solana`, `bitcoin`) |
| `-n, --network` | `localhost` | Network label (`mainnet`, `testnet`, `localhost`) |
| `--did` | from `~/.spacekit/config.toml` | Identity DID for deploy, call, storage, `vm`, etc. (aliases: `--owner-did`, `--caller-did`) |

Config paths:

- Identity & keys: `~/.spacekit/config.toml`, `~/.spacekit/keys/`
- Network overlay: `~/.spacekit/network/config.toml` (or `SPACEKIT_NETWORK_CONFIG`)

---

## Top-level commands

| Command | Description |
|---------|-------------|
| `encrypt` / `decrypt` | File encryption (ECIES or post-quantum KEM) |
| `keypair` | Generate a keypair |
| `encapsulate` / `decapsulate` | KEM shared-secret exchange (quantum algorithms) |
| `init` | Create `~/.spacekit` (DID, keys, config) |
| `new` | Create a project directory (requires `init`) |
| `storage` | Store, retrieve, envelope upload/download, etc. |
| `did` | DID and credential management |
| `network` | Local network profile, `up` / `down`, discovery |
| `nft` | NFT storage and collections |
| `contract` / `vm` | SpaceKitVM contracts and ledger helpers |
| `connect` | Remote compute / storage / messaging / simulator URLs |
| `message` | Messaging and chat |
| `content` | Content publishing and channels |
| `app` | App packages (package, deploy, undeploy, list) |
| `agent` | Growformer brains (train / load / infer) |
| `brain-registry` | Publish brain manifests to storage |
| `repo` | Git-like CAS repo (`/blobs`, `/facts`, refs) |
| `fact` | Build/submit/fetch `FactPackage` (`POST /facts`) |
| `workspace` | Agent workspaces (`/api/workspaces`, export/import) |
| `operator` | Federation discovery manifest (`spacekit:operator:v1`) |
| `migration` | Verify/sign DID `migration_manifest` in export bundles |

**Not exposed in the current build**: 
`task`, `consensus`, `collaborative`, `metrics`.

---

## Getting started

```bash
# One-time environment (~/.spacekit, Kyber keys by default)
spacekit init --algorithm kyber1024

# Optional project folder (see `--kind` templates below)
spacekit new my-app --kind webapp --app-name "My App"
```

`init` writes:

- `~/.spacekit/keys/public_key.hex` — hex-encoded **KEM public key** (post-quantum)
- `~/.spacekit/keys/private_key.hex` — hex-encoded **KEM secret key** (post-quantum)
- `~/.spacekit/config.toml`

These keys are **not** the same format as classical ECIES (secp256k1) keys.

### Identity defaults (`CliContext`)

After `spacekit init`, most commands load **`~/.spacekit` once per invocation** into an internal context:

| Field | Source | CLI override |
|-------|--------|----------------|
| DID | `identity.did` in config; if placeholder `did:spacekit:user:…`, derived from `public_key.hex` as `did:spacekit:testnet:0x…` | global `--did`, or subcommand `--did` |
| Public key file | `identity.public_key_path` (default `keys/public_key.hex`) | `-p` / `--public-key-path` on encrypt, encapsulate |
| Private key file | `identity.private_key_path` (default `keys/private_key.hex`) | `-s` / `--secret-key-path` on decrypt, decapsulate |

**Precedence for DID:** subcommand `--did` → global `--did` → config-derived DID.

You do **not** need to pass `--owner-did` / `--caller-did` on every command if you ran `init`. Legacy names still work as aliases of `--did`.

```bash
spacekit init --algorithm kyber1024

# Uses config DID + keys automatically:
spacekit vm balance
spacekit contract deploy --contract hello_world.wasm --name HELLO_WORLD
spacekit storage deploy --package deploy.toml
# or: spacekit storage deploy --wasm agent.wasm --bin brain.bin --receipt receipt.json

# Override for one invocation:
spacekit --did did:spacekit:testnet:0xabc contract call --contract-id 0x… --function spacekit_handle --args '["hi"]'
```

`encrypt` / `keypair` / `encapsulate` work without `init` (they fall back to `./public_key.hex` in the current directory). Other commands require `init` and error with *Run `spacekit init` or pass `--did`* when identity is missing.

---

### `new`

Creates `./<name>/` in the **current working directory** with `spacekit.toml`, a layout matched to `--kind`, and executable scripts under `scripts/` (`build.sh`, `package.sh`, `deploy.sh`, `undeploy.sh` where applicable).

Requires `spacekit init` first (project `did` in `spacekit.toml` comes from your identity).

| Flag | Default | Meaning |
|------|---------|---------|
| `--kind` | `contracts` | Project template (see table below) |
| `--app-name` | title-cased `<name>` | Marketplace / package display name (webapp, agent, defi) |
| `--network` | `testnet` | Stored as default network label when `--validate` runs |
| `--validate` | off | Run post-create validation |

**Kinds:**

| `--kind` | Layout | Scripts |
|----------|--------|---------|
| `contracts` | `contracts/` Cargo cdylib → `hello_world.wasm` | build, package, deploy (`contract deploy`), undeploy (info) |
| `agent` | `agent/`, `data/`, `ui/`, `{name}.gf.toml`, `deploy.toml` (Luna-style companion) | build (stdlib WASM + growformer train), package, deploy (`storage deploy`), undeploy |
| `webapp` | Root `index.html` + `package.json` (static HTML, like `apps/webapp`) | build, package, deploy, undeploy |
| `webapp-react` | `ui/` with Vite + TypeScript (like `apps/io`) | build, package, deploy, undeploy |
| `defi` | `contracts/` vault WASM + `ui/` dashboard + `deploy.toml` (fintech agent from standard-library) | build, package, deploy (contract + agent + web app), undeploy |

**Examples:**

```bash
spacekit new my-vault --kind contracts
spacekit new luna-bot --kind agent --app-name "Luna Bot"
spacekit new hello-app --kind webapp
spacekit new my-studio --kind webapp-react --app-name "My Studio"
spacekit new my-defi --kind defi

cd hello-app
./scripts/build.sh && ./scripts/package.sh && ./scripts/deploy.sh
```

Agent and defi templates expect **`SPACEKIT_STANDARD_LIBRARY`** to point at a checkout of [spacekit-standard-library](https://github.com/spacekit-xyz/spacekit-standard-library) for agent WASM builds.

Generated webapp scripts call **`spacekit app undeploy`** on redeploy and in `scripts/undeploy.sh` so marketplace listings are removed from both `app_listings` and the federated marketplace index fact.

---

## Crypto: `encrypt` and `decrypt`

### Two modes

| Mode | Algorithm flag | Key files | Input files |
|------|----------------|-----------|-------------|
| **Classical (ECIES)** | `--algorithm ecies` (or `-a ecies`) | secp256k1 public/private hex (not the default `init` keys) | **Binary OK** (e.g. `.wasm`) |
| **Post-quantum (default)** | `kyber512`, `kyber768`, `kyber1024`, … (default: `kyber1024`) | See workflow below | **Text only today** (see [Troubleshooting](#troubleshooting-encrypting-wasm-or-other-binary-files)) |

Default behavior uses the post-quantum path whenever the algorithm is not `ecies`.

### Post-quantum encrypt (default)

```bash
spacekit encrypt <FILE> \
  --kem-secret <SHARED_SECRET_FILE> \
  [-a kyber1024] \
  [--cipher aes]
# -p defaults to ~/.spacekit/keys/public_key.hex when init has been run
```

| Flag | Short | Required (quantum) | Meaning |
|------|-------|-------------------|---------|
| `FILE` | — | yes | Path to plaintext |
| `--kem-secret` | — | **yes** | Path to a **KEM shared secret** (see workflow) — **not** the same as `private_key.hex` from `init` |
| `--public-key-path` | `-p` | no | Recipient public key; **default:** `identity.public_key_path` from config (`~/.spacekit/keys/public_key.hex`) |
| `--output-path` | `-o` | no* | Output ciphertext path (intended; see limitations) |
| `--algorithm` | `-a` | no | KEM algorithm (default `kyber1024`) |
| `--cipher` | — | no | `aes`, `chacha`, `xchacha` (default `aes`) |

\* **Current implementation note:** the quantum handler writes outputs next to the input file and does not yet wire `-p` / `-o` / `--kem-secret` into the low-level encrypt call. It always produces:

- `<FILE>.enc` — ciphertext  
- `<FILE>.kem` — KEM ciphertext  
- `<FILE>.pub` — ephemeral public key material  

Decrypt expects the `.kem` sidecar beside the `.enc` file.

### Recommended post-quantum workflow

**1. Encapsulate** a shared secret against the recipient’s public key (`-p` defaults to config public key):

```bash
spacekit encapsulate \
  --kem-ciphertext-output routekit.kem.ct \
  --kem-secret-output routekit.kem.secret
```

**2. Encrypt** (quantum path — requires `--kem-secret`):

```bash
spacekit encrypt routekit_pinned.wasm \
  --kem-secret routekit.kem.secret
```

**3. Decrypt** (recipient uses their KEM secret key + `.kem` sidecar):

```bash
spacekit decrypt routekit_pinned.wasm.enc \
  -o routekit_pinned.dec.wasm
# Secret key: default -s → ~/.spacekit/keys/private_key.hex; needs <file>.kem sidecar beside .enc
```

### Classical ECIES (binary-safe)

Use when you have **ECIES** key files (e.g. from `spacekit keypair -a ecies --save`):

```bash
spacekit keypair -a ecies --save \
  --public-key-path ecies_public.hex \
  --secret-key-path ecies_secret.hex

spacekit encrypt routekit_pinned.wasm \
  -a ecies \
  -p ecies_public.hex \
  -o routekit_wasm.enc
```

```bash
spacekit decrypt routekit_wasm.enc \
  -a ecies \
  --secret-key-path ecies_secret.hex \
  -o routekit_pinned.dec.wasm
```

`-p` and `-o` are honored on the ECIES path.

### Key naming cheat sheet

| File | What it is |
|------|------------|
| `~/.spacekit/keys/public_key.hex` | Long-term KEM **public** key (from `init`) |
| `~/.spacekit/keys/private_key.hex` | Long-term KEM **secret** key (from `init`) |
| Output of `encapsulate --kem-secret-output` | One-time **shared secret** bytes (for encrypt’s `--kem-secret`) |
| Output of `encapsulate` ciphertext file | KEM ciphertext (paired with shared secret) |
| `<file>.kem` (from encrypt) | KEM ciphertext sidecar written beside input |

Do **not** pass `private_key.hex` as `--kem-secret` on **encrypt** unless you have run `encapsulate` and saved the shared secret to that path. The flag name means “shared secret from encapsulation,” not “my private key.”

---

## Command reference (with examples)

Most flows assume:

```bash
spacekit init --algorithm kyber1024
spacekit network up    # storage + messaging + compute supervisor (second terminal for deploy/call)
```

Identity (DID + keys) is read from `~/.spacekit` automatically. Use `spacekit did list` to inspect, or `--did` to override.

Storage and `repo` commands default to `connections.storage` in `~/.spacekit/config.toml` (set with `spacekit connect storage`) or `http://127.0.0.1:3030`.

---

### `contract` and `vm`

**`contract`** — deploy and interact with WASM on the **SpaceKitVM ledger in this CLI process** (`~/.spacekit/swtchvm/state.bin`). This is separate from the in-memory ledger inside `spacekit network up`, but deploy also **pins WASM** to the storage node the CLI uses.

**`vm`** — fund or inspect **ledger balances** for an owner DID (gas for deploy/call). Deploy/call auto-credit enough gas in many cases; use `vm fund` when you want explicit headroom.

| Subcommand | Purpose |
|------------|---------|
| `contract deploy` | Register WASM + metadata on SwtchVM |
| `contract call` | Execute a function (JSON args) |
| `contract state` | Read contract state (optional `--key`) |
| `contract list` | List contracts (`--owner` filter) |
| `contract history` | Execution history (`--limit`) |
| `vm fund` | Credit owner account (atomic ASTRA units) |
| `vm balance` | Show owner balance |

**Hello-world style deploy and call** (after `cargo build --target wasm32-unknown-unknown --release`):

```bash
# Optional: explicit gas credit (DID from config)
spacekit vm fund
spacekit vm balance

spacekit contract deploy \
  --contract target/wasm32-unknown-unknown/release/hello_world.wasm \
  --name HELLO_WORLD

export CONTRACT_ID="0x025a26fe3f95e74fe07d687b8f6a497fbc2d5c78"   # from deploy output

# SKCL / spacekit_contract! samples use wire-encoded strings via spacekit_handle:
spacekit contract call \
  --contract-id "$CONTRACT_ID" \
  --function spacekit_handle \
  --args '["World"]'

spacekit contract state "$CONTRACT_ID"
spacekit contract history "$CONTRACT_ID" --limit 5
```

**Notes:**

- `--args` is a **JSON array** passed to the runtime. For `spacekit_handle`, one string element is typical; `[]` and `[""]` are different on the wire.
- `--function` can be any exported name when the contract parses the JSON envelope itself; SKCL hello-world uses **`spacekit_handle`**.
- `--initial-balance` on deploy funds the contract account (default `0`).
- `--gas-limit` on call defaults to `1000000`.

---

### `connect`

Persist remote endpoints in `~/.spacekit/config.toml` under `connections`. Many commands (`storage deploy`, `repo push`, `storage fetch`) read **`connections.storage.url`** when `--storage-url` is omitted.

| Subcommand | Purpose |
|------------|---------|
| `connect simulator` | Simulator / dev stack URL |
| `connect compute` | Remote compute node |
| `connect storage` | Remote storage node API |
| `connect messaging` | Bootstrap peer multiaddr |
| `connect status` | Show saved connections |
| `connect test` | Probe a connection (`simulator`, `compute`, `storage`, `messaging`) |

**Local development** (typical ports after `spacekit network up` — adjust to your profile):

```bash
spacekit connect storage \
  --url http://127.0.0.1:3030 \
  --node-did did:spacekit:storage:local \
  --quantum-encrypted

spacekit connect compute \
  --url http://127.0.0.1:9000 \
  --node-did did:spacekit:compute:local \
  --quantum-encrypted

spacekit connect messaging \
  --peer /ip4/127.0.0.1/tcp/7000 \
  --replace

spacekit connect simulator \
  --url http://127.0.0.1:50051 \
  --quantum-encrypted \
  --set-default

spacekit connect status
spacekit connect test storage
```

`--set-default` on simulator marks the default connection type for tooling that respects it.

---

### `agent` and `brain-registry`

Run **`spacekit agent --help`** and **`spacekit brain-registry --help`** for full flag lists, examples, and platform notes (Windows / macOS / Linux).

**`agent`** — Growformer `.bin` brains. The full CLI **embeds growformer** (no
separate `growformer` install or `GROWFORMER_BIN`). Training, merge, and
inference may be gated by the configured storage provider's content-entitlement
policy.

#### Growformer workflow (via `spacekit agent`)

Growformer is a **library inside the `spacekit` binary**, not a separate executable. You invoke it through **`spacekit agent`** subcommands (recommended) or **`spacekit agent exec <growformer flags>`** for anything not wrapped yet. A **`--` separator is optional** — growformer flags pass through directly; use **`agent exec -- --help`** only when you want growformer’s help (without it, `--help` shows spacekit exec help).

**Prerequisites**

```bash
spacekit init
spacekit network up    # storage node for entitlement + install records
```

**1 — Obtain entitlement** (once per machine/DID; no growformer binary download)

```bash
# Recommended: feature grant (publisher runs content publish-feature first)
spacekit content access --feature growformer

# Or view a known feature fact id (records install in storage DB)
spacekit content view --content-id <64-hex growformer_content_id>

# Dev / CI only
export SPACEKIT_GROWFORMER_SKIP_ENTITLEMENT=1
export GROWFORMER_CONTENT_ID=<64-hex>   # optional pin when multiple installs exist
```

Verify end-to-end: `spacekit content growformer-soak`

**2 — Train, infer, merge** (entitlement checked on each gated operation)

| Goal | SpaceKit command | Maps to growformer |
|------|------------------|------------------|
| Train from project | `spacekit agent train --project PATH.gf.toml` | `--train-brain --project …` |
| Train (shorthand) | `spacekit agent -t --project PATH.gf.toml` | same |
| Auto-tune training | add `--auto` | `--auto` |
| Custom output path | `--brain-output PATH.bin` | `--brain-output` |
| Custom data dir | `--data-dir DIR` | `--data-dir` |
| Infer (one-off file) | `spacekit agent infer --brain X.bin --prompt "…"` | `--infer --brain … --prompt …` |
| Infer (verbose / project) | add `-v`, `--project PATH.gf.toml` | `-v`, `--project` |
| Load for repeat infer | `spacekit agent load --name N --brain X.bin` | (SpaceKit in-process cache) |
| Infer loaded brain | `spacekit agent infer --name N --prompt "…"` | (SpaceKit runtime; entitlement required) |
| Merge two brains | `spacekit agent merge --brain BASE --overlay-brain OVER --brain-output OUT` | `--merge-brain --brain … --overlay-brain … --brain-output …` |
| Brain metadata | `spacekit agent info PATH.bin` | peek header only |
| Advanced / all flags | `spacekit agent exec -- --help` | full embedded growformer CLI |

**There is no `merge` subcommand** in growformer — use **`spacekit agent merge`** or **`agent exec --merge-brain …`**, not `agent exec merge …`.

**Growformer uses flags, not verb subcommands** (except `init`):

| Wrong (`agent exec …`) | Right |
|------------------------|--------|
| `infer …` | `--infer …` |
| `merge …` | `--merge-brain …` |
| `train …` | `--train-brain …` |

Example infer via exec:

```bash
spacekit agent exec \
  --infer \
  --brain agent/luna-v2.bin \
  --prompt "the vacuum is about to start"
```

Prefer the wrapper when available: `spacekit agent infer --brain agent/luna-v2.bin --prompt "…"`

**Typical developer session**

```bash
# Train
spacekit agent train --project agent-data/crypto-analysis/crypto-analysis.gf.toml
spacekit agent train --project agent-data/causal-analysis/causal-analysis.gf.toml --auto

# Inspect
spacekit agent info agent-data/crypto-analysis/crypto-brain.bin

# One-shot inference
spacekit agent infer --brain agent-data/crypto-analysis/crypto-brain.bin \
  --prompt "Summarize BTC sentiment"

# Fast repeat inference (same terminal / same spacekit process)
spacekit agent load --name crypto --brain agent-data/crypto-analysis/crypto-brain.bin
spacekit agent infer --name crypto --prompt "ETH outlook"
spacekit agent list
spacekit agent unload crypto

# Merge overlay brain into base (preferred)
spacekit agent merge \
  --brain agent-data/crypto-analysis/crypto-brain.bin \
  --overlay-brain agent-data/causal-analysis/causal-brain.bin \
  --brain-output agent-data/crypto-analysis/crypto-causal.bin

# Same merge via exec (equivalent flags)
spacekit agent exec \
  --merge-brain \
  --brain agent-data/crypto-analysis/crypto-brain.bin \
  --overlay-brain agent-data/causal-analysis/causal-brain.bin \
  --brain-output agent-data/crypto-analysis/crypto-causal.bin
```

**3 — Ship brain + WASM to storage** (after training)

```bash
# Recommended: manifest with hub + marketplace settings
spacekit storage deploy --package deploy.toml

# Or flags only
spacekit storage deploy \
  --wasm target/wasm32-unknown-unknown/release/my_agent.wasm \
  --bin agent-data/crypto-analysis/crypto-causal.bin \
  --owner-did "<YOUR_DID>" \
  --receipt deploy-receipt.json \
  --agent-id ca-008 \
  --publish \
  --brain-key crypto_brain
```

Set `growformer.brain_storage_key` in `.gf.toml` to match the contract’s `growformer_load_brain_from_storage_key(...)`. See [training-brains](https://docs.spacekit.xyz/docs/training-brains).

**`agent exec` — growformer flags reference**

Growformer flags after `exec` are passed through to the embedded CLI (with entitlement enforcement). **`--` is optional** for flags and subcommands like `init`; use **`spacekit agent exec -- --help`** when you need growformer’s full flag list (plain `agent exec --help` is spacekit’s exec help).

**Only growformer subcommand:** `init` (e.g. `spacekit agent exec init scripts/foo.gf.toml`). All other operations use **long flags** (`--infer`, `--train-brain`, `--merge-brain`, …).

| Flag | Purpose |
|------|---------|
| `--train-brain` | End-to-end training |
| `--validate-brain-training` | Quick training validation run |
| `--infer` | Run inference (omit `--prompt` for interactive REPL) |
| `--merge-brain` | Merge overlay into base brain |
| `--retrain-gen N` | Retrain one generation group |
| `--repack` | Repack brain with updated inference TOML |
| `--rules-info` | Print inference rule counts and exit |
| `--debug-embedding` | Dump encoder embedding for `--prompt` |
| `--project PATH.gf.toml` | Project manifest (train/infer context) |
| `--brain PATH.bin` | Input brain for infer / merge / retrain |
| `--overlay-brain PATH.bin` | Overlay for `--merge-brain` |
| `--brain-output PATH.bin` | Output path for train / merge |
| `--brain-name` / `--brain-description` / `--brain-author` | Metadata embedded in exported brain |
| `--prompt TEXT` | Single-shot inference prompt |
| `--prompt-file PATH` | Read prompt from file (avoids shell `$` expansion) |
| `--prompts-file PATH` | Batch inference (one prompt per line) |
| `--data-dir DIR` | Training JSONL corpus directory |
| `--auto` | Auto-configure training from dataset |
| `--brain-epochs N` | Router + classifier epochs (default 30) |
| `--brain-gen-epochs N` | Per-group generation epochs (0 = auto) |
| `--brain-gen-replicas K` | Parallel replicas per gen task |
| `--brain-max-samples N` | Cap training samples (validation / quick runs) |
| `--brain-quick-gen-epochs N` | Cap gen epochs when validating |
| `--train-code-lattice` | Enable code lattice / MetaCodebook training |
| `--categorical-data DIR` | Categorical composer bootstrap JSONL |
| `--categorical-steps N` | Steps for categorical bootstrap |
| `--brain-plugins-toml PATH` | Inference plugins manifest embedded in brain |
| `--inference-toml PATH` | Inference shortcut rules TOML |
| `--inference-defaults-toml PATH` | Baseline inference TOML for empty rule arrays |
| `--inference-guardrails-jsonl PATH` | Guardrails JSONL merged after TOML |
| `--no-progress` | Disable stderr progress bars (CI) |
| `-v` / `--verbose` | Verbose inference traces |
| `init [PATH]` | Write starter `.gf.toml` |

**Not available via `agent exec`** (SpaceKit-only wrappers):

| SpaceKit command | Why not `exec` |
|------------------|----------------|
| `agent load` / `unload` / `list` | In-process brain cache in this CLI |
| `agent infer --name` | Uses loaded brain via `GrowformerModelManager` |
| `agent info` | Reads brain header without full growformer dispatch |

Full list: `spacekit agent exec -- --help`

**Execution modes**

| Mode | Commands | Notes |
|------|----------|--------|
| Embedded growformer | `train`, `merge`, `infer --brain`, `exec`, `-t --project` | In-process growformer CLI with entitlement enforcement |
| In-process cache | `load`, `infer --name`, `list`, `unload`, `info` | Fast repeat inference; `infer --name` also requires entitlement; cache is **per `spacekit` process** |

| Subcommand / flag | Purpose |
|-------------------|---------|
| `agent train` | Train from `.gf.toml` project |
| `agent -t` / `--train` | Shorthand (requires `--project`) |
| `agent infer` | `--brain` file or `--name` loaded brain |
| `agent load` / `unload` / `list` | In-process brain cache |
| `agent info` | Metadata from a `.bin` |
| `agent merge` | Merge two brains (`--brain`, `--overlay-brain`, `--brain-output`) |
| `agent exec` | Pass growformer **flags** after `--` (not subcommands like `merge`) |
| `--content-id` / `--app growformer` | Optional on `agent` / `exec` (entitlement context) |

**Environment**

| Variable | Purpose |
|----------|---------|
| `SPACEKIT_GROWFORMER_SKIP_ENTITLEMENT=1` | Dev bypass entitlement checks |
| `GROWFORMER_CONTENT_ID=<64-hex>` | Pin growformer feature fact for entitlement resolution |

**Windows:** Same commands; quote paths with spaces (e.g. `--project "C:\projects\my-agent.gf.toml"`). Forward or backslashes both work.

**`brain-registry`** — BRAIN_REGISTRY manifest v1: build JSON from `.gf.toml` + `storage deploy` receipt, publish to storage `PUT /api/documents/...`. Requires `spacekit init` and a running storage node.

| Subcommand | Purpose |
|------------|---------|
| `brain-registry build` | Manifest from `.gf.toml` + receipt |
| `brain-registry publish` | Upload manifest to storage API |

```bash
spacekit brain-registry build \
  --gf-toml my-agent/my-agent.gf.toml \
  --receipt deploy-receipt.json \
  --out brain-manifest.json

spacekit brain-registry publish \
  --manifest brain-manifest.json \
  --storage-url http://127.0.0.1:3030
```

**Windows:** HTTP to the storage URL works the same; use quoted paths for `--gf-toml` / `--manifest` when needed.

Set `growformer.brain_storage_key` in `.gf.toml` to match your contract (e.g. `crypto_brain`, `routekit_router`). See [training-brains](https://docs.spacekit.xyz/docs/training-brains).

---

### `content`

Channel-style **content publishing**: files become `FactPackage` records on the storage node, with optional on-chain registration (compute node) and Gossipsub notifications (messaging / simulator). Distinct from `storage deploy` (WASM/agent shipping) and `fact` (generic schema facts).

**Prerequisites:**

```bash
spacekit init
spacekit network up                    # local storage + compute
spacekit connect storage --url http://127.0.0.1:3030   # optional; default port 3030
spacekit connect compute --url http://127.0.0.1:9000   # optional; for governance registration
spacekit connect simulator --url <URL>                 # optional; Gossipsub notifications
```

Uses identity DID and KEM keys from `~/.spacekit` (`spacekit init`). Channel IDs passed to `--channel` must be valid **Quantum DIDs** (same format as `--did`).

| Subcommand | Purpose |
|------------|---------|
| `content create-channel` | Persist channel as `spacekit:channel:v1` fact; prints channel DID + fact id |
| `content publish` | Read file → signed `FactPackage` → `FactStorageEngine` (encrypts paid content) |
| `content publish-feature` | Publish `spacekit:licensed_feature:v1` (growformer entitlements; no binary) |
| `content subscribe` | Record local channel subscription grant (MVP; on-chain entitlement TODO) |
| `content list-channels` | Query facts tagged `channel` |
| `content list-content` | Query facts tagged `content` + `published` |
| `content view` | Fact retrieval + access policy; materializes under storage-node data dir and records install in DB (`content_installs`) |
| `content installs` | List DB installs (path, entitlement, app slug) for your DID |
| `agent exec --app growformer` | Run embedded growformer CLI (after `content view`; no materialized `.bin`) |
| `agent exec --content-id <hex>` | Same, keyed by content id |
| `content access` | Verify payment (`--payment-ref`) then grant PPV and/or channel access; `--feature growformer` for library-embedded features |
| `content growformer-soak` | E2E soak: publish-feature → access (free tier) → agent exec (requires `network up`) |
| `content growformer-paid-soak` | E2E soak: publish-feature → pay `--tier personal` → settle → agent exec |
| `content renew` | Extend or recreate access (`--content-id` or `--channel`, `--extend-secs`) |
| `content list-access` | List active local grants for your DID |
| `content record-payment` | Dev: register a verified payment receipt (simulates SpaceKit Pay) |
| `content unpublish` | Remove catalog entry; with `--purge`, delete fact blobs. **App manifests** (`spacekit:app-package:v1`) also remove `app_listings` + marketplace index entries |

**Pricing (`--pricing`):**

| Value | Access policy |
|-------|----------------|
| `free` (default) | `AccessPolicy::Public` |
| `pay_per_view` | `Conditional` + `PaymentRequired` (`--price` in **ASTRA**) |
| `subscription` | `Conditional` + channel subscription required |
| `mixed` | Subscription **or** pay-per-view (OR semantics on access) |

**Typical flow:**

```bash
# 1) Channel (prints channel DID from create-channel output)
spacekit content create-channel \
  --name "My Channel" \
  --description "Demo videos" \
  --pricing free

export CHANNEL_DID=did:spacekit:channel:my-channel:...   # from command output

# 2) Publish media
spacekit content publish \
  --channel "$CHANNEL_DID" \
  --file ./episode-01.mp4 \
  --title "Episode 1" \
  --description "Pilot" \
  --pricing free

# Output includes Content ID / Fact Package id (64-hex) — save for list/view

# 3) Discover
spacekit content list-channels
spacekit content list-channels --detailed
spacekit content list-content --channel "$CHANNEL_DID" --limit 20

# 4) Download
spacekit content view --content-id <64-hex-fact-id> --output ./episode-01-copy.mp4
```

**What `publish` does (in order):**

1. Build `FactContent::Binary` + metadata tags (`content`, `published`, mime-based `video`/`image`/`audio`).
2. `FactStorageEngine::store_fact` under the embedded storage node data dir (`fact_storage/`).
3. `register_content_with_governance` on compute (contract id `storage_governance_{channel}`) — warns if contract not deployed.
4. Gossipsub notification when simulator connection exists — otherwise prints setup hint.

**Paid content example:**

```bash
spacekit content publish \
  --channel "$CHANNEL_DID" \
  --file ./premium.mp4 \
  --title "Premium clip" \
  --pricing pay_per_view \
  --price 10.0

# Consumer (different DID / machine): view blocked until access granted
spacekit content view --content-id <64-hex-from-publish>

# Production monetization (Sprint 3): pay → settle → OP_PURCHASE → view
export SPACEKIT_ENTITLEMENT_CONTRACT_ID=<ledger-contract-hex>
export SPACEKIT_COMPUTE_URL=http://127.0.0.1:8545

spacekit content view --content-id <64-hex> --pay
# → pending id + quote; pay publisher off-band / SpaceKit Pay

spacekit content settle \
  --pending-id pending-<uuid> \
  --tx-hash <payment-tx> \
  --amount 10
# → POST /v1/payments/verify, OP_PURCHASE, local grant

spacekit content view --content-id <64-hex> --output ./premium-copy.mp4

# Manual on-chain purchase (testing, no pending flow):
spacekit content purchase --content-id <64-hex>

# Dev: record-payment also writes settlements_inbox (enables pay --await-settlement)
spacekit content record-payment \
  --reference tx-abc --recipient did:spacekit:publisher:you \
  --scope "content:<64-hex>" --amount 10
spacekit content pay --content-id <64-hex> --await-settlement
# Or legacy grant without OP_PURCHASE:
spacekit content access --content-id <64-hex> --payment-ref tx-abc

# Paid channel:
spacekit content pay --channel "$CHANNEL_DID" --publisher did:spacekit:publisher:you --price 25
```

**Subscription example:**

```bash
spacekit content subscribe --channel "$CHANNEL_DID"
spacekit content list-access
```

**Renewal:**

```bash
spacekit content renew --content-id <64-hex> --extend-secs 2592000 --payment-ref tx-renew-1
spacekit content renew --channel did:spacekit:channel:... --extend-secs 604800
```

**Sprint 3 monetization subcommands:**

| Subcommand | Purpose |
|------------|---------|
| `content pay --content-id` | Create pending PPV purchase + quote |
| `content pay --channel --price --publisher` | Pending channel subscription + quote |
| `content pay --tx-hash --amount` | Quote + settle + `OP_PURCHASE` in one command |
| `content pay --await-settlement` | Try matching `settlements_inbox.jsonl` then auto-complete |
| `content access --content-id --pay` | Same as `view --pay` (initiate quote, no grant yet) |
| `content settle --pending-id --tx-hash --amount` | Verify on compute, `OP_PURCHASE`, grant |
| `content purchase --content-id` | Manual `OP_PURCHASE` (no pending flow) |
| `content listen-settlements` | Poll inbox; auto `OP_PURCHASE` + grant for open pending |
| `content listen-settlements --once` | Single listener pass (CI / soak helper) |
| `content soak dev` | Full dev monetization soak (5 checks; needs `network up`) |
| `content soak live` | Live soak (requires `SPACEKIT_ENTITLEMENT_CONTRACT_ID`) |

Publish with `SPACEKIT_ENTITLEMENT_CONTRACT_ID` set registers `OP_CREATE_LISTING` for paid content.

**Env (monetization):**

| Variable | Purpose |
|----------|---------|
| `SPACEKIT_ENTITLEMENT_CONTRACT_ID` | Entitlement-ledger contract on compute |
| `SPACEKIT_LICENSE_CONTRACT_ID` | AppLicenseNFT WASM (`main` opcodes 0x01 mint, 0x02 has_license) |
| `SPACEKIT_ESCROW_CONTRACT_ID` | astra-escrow WASM (OP_CREATE on pay quote, OP_RELEASE on grant, OP_REFUND on failure) |
| `SPACEKIT_ESCROW_ARBITER_DID` | Arbiter DID for escrow create (default `did:spacekit:treasury`) |
| `SPACEKIT_ESCROW_TOKEN` | Token label in escrow record (default `ASTRA`) |
| `SPACEKIT_COMPUTE_URL` | HTTP base for `/v1/payments/verify` and contract calls |
| `SPACEKIT_CONTENT_GRANTS_FILE` | Override local grants JSON |
| `SPACEKIT_CONTENT_PAYMENTS_FILE` | Override verified payments JSON |
| `SPACEKIT_SETTLEMENT_POLL_MS` | Poll interval for `pay --await-settlement` (default 500) |
| `SPACEKIT_SETTLEMENT_TIMEOUT_SECS` | Wait timeout for `pay --await-settlement` (default 120) |
| `SPACEKIT_CONTENT_SETTLEMENT_SECRET` | Shared secret for `POST /api/content/settlements` (storage + compute webhook) |
| `SPACEKIT_STORAGE_NODE_URL` | Compute forwards verified `content:`/`channel:` payments to storage inbox |

**Settlement modes (soak):**

| Mode | Command | Path |
|------|---------|------|
| Dev | `content soak dev` | `record-payment` → inbox → `listen-settlements` |
| Router | `content soak router` | `content settle` → `/v1/payments/verify` → storage inbox (no `record-payment`) |
| Live | `content soak live` | On-chain `OP_PURCHASE` + contracts |

With `spacekit network up` (storage + compute), a background settlement listener polls the inbox every 5s (`SPACEKIT_SETTLEMENT_LISTENER_SECS`). Disable with `SPACEKIT_CONTENT_SETTLEMENT_LISTENER=0`.

**Limitations (current build):**

- Live SpaceKit Pay router auto-settlement requires compute `SPACEKIT_STORAGE_NODE_URL` (set automatically on `network up`) so `/v1/payments/verify` forwards to `POST /api/content/settlements`.
- Legacy path still works: **receipt file** (`content_payments/verified.json`) or **64-char entitlement id** (OP_VERIFY).
- Local grants remain the cache layer; on-chain entitlement re-check runs at `view` when grant stores `entitlement_id_hex`.
- AppLicenseNFT: build `-p spacekit-app-license-nft`, deploy `spacekit_app_license_nft.wasm` (`main` opcodes 0x01/0x02); set `SPACEKIT_LICENSE_CONTRACT_ID`. Mint runs on settle when configured.
- astra-escrow: `OP_CREATE` on pay quote, `OP_RELEASE` after grant; failed grants call **escrow `OP_REFUND` first** (set `SPACEKIT_ESCROW_REQUIRED=1` to fail closed), then local `refund_log.json` audit + unconsumed payment reference.
- AppLicenseNFT: `view` checks on-chain `has_license` when `SPACEKIT_LICENSE_CONTRACT_ID` is set; mint on settle stores `license_token_id` on the grant.
- `list-content` does not filter by `--channel` in the storage query (tags include `channel:<did>` on each fact).
- Gossipsub publish notifications and governance contract registration remain best-effort.
- Remote storage nodes: set `SPACEKIT_CONTENT_GRANTS_FILE` or replicate grants dir for shared access state.

**Env:** `SPACEKIT_CONTENT_GRANTS_FILE` — override grants JSON path (storage node + CLI local node).

**E2E soak (before paid launch):**

```bash
cargo test --test content_e2e_soak -p spacekit-storage-node
spacekit network up   # other terminal
spacekit content soak dev
# or: ./spacekit-cli/scripts/content-monetization-soak.sh dev
# live: see content-monetization-live-deploy.md then:
# export SPACEKIT_ENTITLEMENT_CONTRACT_ID=... && ./scripts/content-monetization-soak.sh live
```

Storage-provider soak, deployment, publishing, and subscription procedures are
maintained with the authorized proprietary storage implementation and are not
published in this repository.

**Related public docs:**

- Generic facts: [`fact`](#fact) below · agent shipping: [`storage deploy`](#storage)

Implementation: `spacekit-cli/src/content_integration.rs`, handlers in `full_client.rs` (`handle_content_command`).

---

### `repo`

Git-like workflow on the **storage node CAS**: blobs (`PUT /blobs/{hash}`), commit facts (`POST /facts`), branch refs (`PUT /api/documents/repos/.../refs/heads/...`). Local state: `.spacekit/repo/` (`HEAD`, `index.json`, `refs/heads/*`, `objects/commits/`).

| Subcommand | Purpose |
|------------|---------|
| `repo init` | Create `.spacekit/repo` (`--name`, `--remote`) |
| `repo add` | Stage paths (default: all tracked files under cwd) |
| `repo status` | Index vs working tree |
| `repo commit -m` | Commit staged index |
| `repo push` / `pull` | Sync with remote (`-b` branch, `--storage-url`) |
| `repo log` | Local commit history |
| `repo diff` | Tree diff between commits (`--a`, `--b`) |
| `repo branch` | List / create / delete (`-d`) branches |
| `repo checkout` | Switch branch; refresh files from CAS |
| `repo clone` | `init` + `pull` into new directory |

Skipped directories: `.spacekit`, `.git`, `target`, `node_modules`.

**Typical session:**

```bash
cd my-project
spacekit repo init --name myproject --remote http://127.0.0.1:3030

spacekit repo add src/ README.md
spacekit repo status
spacekit repo commit -m "Initial checkpoint"

spacekit repo push --storage-url http://127.0.0.1:3030
spacekit repo log --limit 10

# Branch workflow
spacekit repo branch feature-x
spacekit repo checkout feature-x
# … edit files …
spacekit repo add .
spacekit repo commit -m "Feature work"
spacekit repo push -b feature-x

# Clone elsewhere
spacekit repo clone http://127.0.0.1:3030 myproject ./myproject-checkout
```

`pull` is **fast-forward only** (no merge). `diff` compares **path → BLAKE3** trees, not line-level text. Ref docs: `spacekit-storage-node/documentation/guides/spacekit-repository-hosting.md`.

---

### `workspace`

First-class **workspace** documents (`spacekit:workspace:v1`) on the storage node agentic API. Names an owner, collaborators (human/agent DIDs), associated repos, and quotas.

| Subcommand | Purpose |
|------------|---------|
| `workspace create <id>` | `POST /api/workspaces` |
| `workspace show <id>` | `GET /api/workspaces/{id}` (owner auth) |
| `workspace list` | `GET /api/workspaces?owner_did=…` |
| `workspace export <id>` | `GET /api/workspaces/{id}/export` → JSON file |
| `workspace import <file>` | `POST /api/workspaces/import` (federation destination) |

```bash
spacekit workspace create team-alpha \
  --storage-url http://127.0.0.1:3030 \
  --collaborator did:spacekit:agent:bot:agent \
  --repo myproject

spacekit workspace show team-alpha
spacekit workspace list --owner-did did:spacekit:user:alice

spacekit workspace export team-alpha -o /tmp/team-alpha.json
spacekit workspace import /tmp/team-alpha.json --owner-did did:spacekit:dest:owner

# With CAS replication from source node:
spacekit workspace import /tmp/team-alpha.json \
  --owner-did did:spacekit:dest:owner \
  --source-url http://127.0.0.1:3030 \
  --source-auth "DID did:spacekit:source-owner"

# Replace existing workspace on destination:
spacekit workspace import /tmp/team-alpha.json --replace
```

Ref: `spacekit-storage-node/documentation/guides/workspaces.md` · federation handoff: `spacekit-storage-node/documentation/guides/federation-workspace-handoff.md`.

---

### `operator`

Federation **operator discovery** — publish and read `spacekit:operator:v1` manifests (HTTP base URL, auth mode, content policy link).

| Subcommand | Purpose |
|------------|---------|
| `operator publish` | Build + `POST /facts` operator manifest (`--sign` for strict nodes) |
| `operator show` | `GET /api/operators/self` (published fact or runtime fallback) |
| `operator fact-id` | Print deterministic manifest fact id (64-hex) for operator DID |

```bash
# After spacekit network up (sets SPACEKIT_NODE_DID + SPACEKIT_PUBLIC_HTTP_URL)
spacekit operator publish --display-name "Dev Node" \
  --storage-url http://127.0.0.1:3030 \
  --policy-uri https://example.com/spacekit-policy.json \
  --blob-fact-auth hybrid \
  --feature workspaces --feature federation_export \
  --sign

spacekit operator show
spacekit operator fact-id

curl -s http://127.0.0.1:3030/api/agentic/health | jq '{blob_fact_auth_mode, upload_tokens_configured}'
```

Refs: `spacekit-storage-node/documentation/guides/operator-discovery.md` · `operator-abuse-policy.md` · `federation-design.md`.

---

### `migration`

Verify **layer 2** migration attestations embedded in workspace export JSON (`migration_manifest` field). Layer 1 HMAC remains `handoff_signature`.

| Subcommand | Purpose |
|------------|---------|
| `migration verify <bundle.json>` | Check canonical payload + SPHINCS+ signatures |
| `migration sign <bundle.json>` | Append a role signature (`--role`, `--signer-did`) |
| `migration keygen --signer-did <did>` | Create `workspace_owner` key under `.migration_signer_keys/` |

```bash
spacekit workspace export team-alpha -o /tmp/handoff.json
spacekit migration verify /tmp/handoff.json
spacekit migration sign /tmp/handoff.json --role destination_operator
```

Export version negotiation: set `SPACEKIT_MIGRATION_DEST_URL` to the destination operator base URL before export; the node fetches `/api/operators/self` and negotiates v1 vs v2.

Requires source operator `sphincs_public_key_hex` in published operator manifest (see `spacekit operator publish` after node created `.operator_sphincs_keypair`).

Spec: `spacekit-storage-node/DID-MIGRATION.md` · guide: `spacekit-storage-node/documentation/guides/did-signed-migration.md`.

---

### `app`

Package static web UIs (HTML/Vite `dist/`) into signed **`.spkg`** manifests, upload to a storage node, and publish to the marketplace.

| Subcommand | Purpose |
|------------|---------|
| `app package` | Build manifest from a directory (`--name`, `--entry`, `--version`, `-o`) |
| `app deploy` | Upload `.spkg` to storage; `--publish` writes `app_listings` + marketplace index |
| `app undeploy` | Remove deployment: `app_listings`, marketplace index, manifest + bundle facts (`--purge` default) |
| `app list` / `app info` / `app download` / `app verify` / `app run` | Discovery and local preview |

**Typical webapp flow** (also generated by `spacekit new --kind webapp` / `webapp-react`):

```bash
spacekit app package ./dist \
  --name "My App" \
  --entry index.html \
  --version 1.0.0 \
  -o my-app-1.0.0.spkg

spacekit app deploy my-app-1.0.0.spkg \
  --storage-node http://127.0.0.1:3030 \
  --publish

# Later: remove from marketplace + storage
spacekit app undeploy <app-id-hex> \
  --storage-node http://127.0.0.1:3030 \
  --purge
```

**Marketplace cleanup:** `app deploy --publish` writes to two places — `app_listings` documents and the well-known **marketplace index fact** (`18069b98…`). `app undeploy` and `content unpublish` on an app manifest remove both, so undeployed apps disappear from `/marketplace`. The website API also filters listings whose manifest fact is missing on storage.

**Notes:**

- `--name` is part of the stable app id (`sha256(creator_did || name)`). Do not change it after publishing or installs break.
- Embedded apps load from blob URLs; Vite builds should use `base: "./"` and a single bundle (`inlineDynamicImports`) — see generated `webapp-react` templates.

---

### `storage`

Talks to a **storage node HTTP API** (embedded node from `network up` or remote via `connect storage`). Use this for **binary artifacts** (WASM, `.bin` brains), receipts, and envelope encryption.

| Subcommand | Purpose |
|------------|---------|
| `storage store` | Upload file with quantum envelope (`--owner-did`) |
| `storage retrieve` | Download by `file_id` (`--embedded` / `--local` for CLI embedded node) |
| `storage fetch` | HTTP session-key + content download |
| `storage list` | List files (`--owner`, `--owned-by-me`, `--details`) |
| `storage share` / `revoke` | ACL by DID |
| `storage stats` | Node statistics |
| `storage deploy` | Upload **WASM + companion .bin**; emit receipt JSON; optional `--package deploy.toml` |
| `storage verify-receipt` | Verify local bytes against receipt BLAKE3 hashes |
| `storage sync-receipt` | Pull wasm/bin from remote using receipt `file_id`s |
| `storage envelope-upload` | Zero-knowledge envelope upload |
| `storage envelope-fetch` | Zero-knowledge download by `file_id` |
| `storage node` | `start` / `stop` / `status` embedded storage node |

**Agent shipping (WASM + brain)** — preferred path for Growformer contracts.

**Option A — deploy manifest (recommended):** keep paths, Agent Hub config, and marketplace settings in `deploy.toml`:

```bash
cargo build -p my-agent --release --target wasm32-unknown-unknown

spacekit storage deploy --package deploy.toml
# CLI flags override manifest values, e.g.:
# spacekit storage deploy --package deploy.toml --publish --brain-key crypto_brain
```

Example `deploy.toml` (also at `spacekit-cli/examples/deploy.toml`):

```toml
[artifacts]
wasm = "target/wasm32-unknown-unknown/release/my_agent.wasm"
bin = "agent-data/my-agent/my-agent-causal.bin"

[agent]
id = "ca-008"

[project]
gf_toml = "crypto-analysis.gf.toml"   # optional; fills brain_key from growformer.brain_storage_key

[receipt]
path = "deploy-receipt.json"

[hub]
brain_key = "crypto_brain"              # VM storage key the WASM contract reads
capabilities = ["Narrative tone", "Protocol context"]
tag_label = "CRYPTO"
tag_color = "#fbbf24"
hub_response_format = "growformer"      # growformer | plain

[marketplace]
publish = true
title = "Crypto Analysis"
description = "Growformer-backed analysis for digital-asset text."
category = "ai"
access = "public"
price = "0.002"                         # pay-per-use aUSD, or "free"
marketplace_id = "default"
```

When `--publish` is set (CLI or `[marketplace].publish = true`), the deploy receipt and marketplace listing include **`hub_config`** (brain key, capabilities, tags). Agent Hub loads agents from `GET /api/agents` — no hardcoded frontend catalog.

**Option B — flags only:**

```bash
spacekit storage deploy \
  --wasm target/wasm32-unknown-unknown/release/my_agent.wasm \
  --bin agent-data/crypto-analysis/crypto-causal.bin \
  --receipt deploy-receipt.json \
  --agent-id ca-008 \
  --publish \
  --title "Crypto Analysis" \
  --description "Demo agent" \
  --category ai \
  --brain-key crypto_brain \
  --capabilities "Narrative tone,Protocol context" \
  --tag-label CRYPTO \
  --tag-color "#fbbf24" \
  --price 0.002

spacekit storage verify-receipt --receipt deploy-receipt.json
spacekit storage list --owned-by-me --details
```

Set `growformer.brain_storage_key` in `.gf.toml` to match `[hub].brain_key` / `--brain-key` and the contract’s `growformer_load_brain_from_storage_key(...)`. See [training-brains](https://docs.spacekit.xyz/docs/training-brains).

**End-to-end after deploy:**

```bash
spacekit brain-registry build \
  --gf-toml crypto-analysis.gf.toml \
  --receipt deploy-receipt.json \
  --out brain-manifest.json
```

**Fetch artifacts** (remote node or after deploy):

```bash
spacekit storage fetch <FILE_UUID> -o ./downloaded.wasm \
  --storage-url http://127.0.0.1:3030

# Pull both wasm and bin paths from a receipt (e.g. for a static site)
spacekit storage sync-receipt \
  --receipt deploy-receipt.json \
  --wasm-out ./public/agent.wasm \
  --bin-out ./public/agent.bin
```

**Retrieve from embedded CLI storage** (e.g. WASM pinned during `contract deploy` when no standalone API is up):

```bash
spacekit storage retrieve <FILE_ID> -o hello_world.wasm --embedded
# same as: SPACEKIT_STORAGE_RETRIEVE_EMBEDDED=1
```

**Envelope upload** (private key stays local; good for arbitrary binary blobs):

```bash
spacekit storage envelope-upload ./routekit_pinned.wasm \
  --content-type application/wasm \
  --storage-url http://127.0.0.1:3030

spacekit storage envelope-fetch <FILE_UUID> -o routekit_pinned.wasm
```

`--storage-url` overrides config; default is `connections.storage` or `http://127.0.0.1:3030`. `--owner-key-algorithm` on deploy must match your `public_key.hex` KEM (e.g. `Kyber1024`).

---

### `did`

Quantum-resistant **DIDs**, resolution, and **verifiable credentials**. `spacekit init` creates your primary identity; `did` subcommands manage additional DIDs and credentials.

| Subcommand | Purpose |
|------------|---------|
| `did create` | New DID + keys (`--algorithm`, `--save`, `--identifier`) |
| `did list` | List known DIDs (`--owned-by-me`, `--method`, `--detailed`) |
| `did verify` | Verify a DID (`--credentials`, `--detailed`) |
| `did resolve` | Resolve DID document (`--format json`, `--verify`) |
| `did update` | Add keys, `--rotate-keys`, update document |
| `did issue` | Issue verifiable credential (`--to`, `--credential-type`, `--claims`) |
| `did verify-credential` | Verify credential file (`-c` / `--credential-file`) |

**Inspect identity from init:**

```bash
spacekit did list
spacekit did list --owned-by-me --detailed --with-credentials
```

**Create a secondary DID and save keys:**

```bash
spacekit did create \
  --algorithm kyber1024 \
  --save \
  --identifier alice \
  --format text
```

**Resolve and verify:**

```bash
spacekit did resolve "$YOUR_DID" --format json --verify
spacekit did verify "$YOUR_DID" --credentials --detailed
```

**Issue and verify a credential:**

```bash
spacekit did issue \
  --to "$YOUR_DID" \
  --credential-type MembershipCredential \
  --claims '{"tier":"builder","project":"my-agent"}' \
  --validity-days 365 \
  --output membership.json

spacekit did verify-credential --credential-file membership.json --detailed
```

DIDs appear as `did:spacekit:testnet:…`, `did:spacekit:user:…`, etc., depending on how they were created. The effective DID for commands is **`--did`** (global or per-subcommand); legacy `--owner-did` / `--caller-did` are aliases. Use that DID for storage ACLs and `Authorization: DID …` on document APIs.

---

### `fact`

Submit structured **`FactPackage`** JSON to the storage node (same CAS layer as `repo` commits, but for arbitrary schemas).

| Subcommand | Purpose |
|------------|---------|
| `fact create` | Build from `--data` (JSON), `--file` (binary), or `--package` (existing JSON); POST to `/facts` |
| `fact get` | `GET /facts/{fact_id}` |
| `fact id` | Preview deterministic or unique `fact_id` for a JSON payload |

```bash
# JSON fact
spacekit fact create --schema spacekit:my:event:v1 --data ./event.json

# Binary artifact (inline in fact body — prefer repo + /blobs for large trees)
spacekit fact create --schema spacekit:artifact:v1 --file ./manifest.json

# Submit pre-built package
spacekit fact create --package ./fact.json

# Dry-run + save locally
spacekit fact create --schema spacekit:my:event:v1 --data ./event.json \
  --output ./out.json --dry-run

# Preview fact_id (deterministic)
spacekit fact id --schema spacekit:my:event:v1 --data ./event.json --deterministic

# Fetch
spacekit fact get <64-hex-fact-id> --storage-url http://127.0.0.1:3030
```

**Repo vs fact:** `repo commit` builds `spacekit:repo:commit:v1` facts with blob CAS; `fact create` is for **custom schemas** and one-off records. **`storage store`** uses envelope files (`/files/...`) and does **not** auto-create facts.

---

### `network`

Profile file: `~/.spacekit/network/config.toml` (override with `SPACEKIT_NETWORK_CONFIG`). Version 3 adds `profile`, `role`, manifest trust policy, and private/public admission settings to `[services]`, `[ports]`, `[urls]`, `[messaging]`, and `[runtime]`. URLs are derived from ports when omitted.
Canonical developer and operator setup:
[`../../docs/guides/developer-network-setup.md`](../../docs/guides/developer-network-setup.md).

| Subcommand | Purpose |
|------------|---------|
| `network init` | Write network profile (ports, which services to run) |
| `network up` | Start embedded services (storage + messaging + compute) — **no blockchain** |
| `network up --full` | All services + gateway + **blockchain** (operator rewards ledger; higher RSS) |
| `network up --only storage,messaging` | Start a subset for this run |
| `network up -d` | Detached supervisor |
| `network memory` | RSS + storage cache diagnostic (`--json`, `--watch`, `--sample`) |
| `network start storage` | Start one service (supervisor not already running) |
| `network stop` / `down` | Stop supervisor (all services) |
| `network status --detailed` | Runtime state plus live `/status` and service endpoint queries |
| `network doctor` | Validate profile/runtime and probe enabled services |
| `network test --suite local\|private\|public\|all --report FILE` | Run isolated network acceptance gates; `.json` writes JSON and `.xml` writes JUnit |
| `network logs [--service ...]` | Read detached supervisor/sidecar logs |
| `network reset --data [--force]` | Delete configured local data, requiring confirmation unless forced |
| `network join --manifest FILE --role ROLE` | Build a private/public profile from a signed manifest |
| `network manifest keygen\|sign\|verify` | Manage and verify SPHINCS-128f network-manifest signatures |
| `network config show\|set\|enable\|disable\|path` | Inspect or modify the active profile |
| `network discover` | List and optionally probe signed-manifest/configured endpoints (not global mock discovery) |
| `network peers` | Query live peer/state endpoints; errors when none are exposed |
| `network reputation` / `reputation-watch` | Query live reputation endpoints; never synthesizes scores |

**Custom ports example:**

```bash
spacekit network init --force \
  --storage-port 4030 \
  --compute-port 9001 \
  --messaging-listen-port 7101 \
  --no-compute
spacekit network up
spacekit storage stats --storage-url http://127.0.0.1:4030
```

**External nodes** (no embedded spawn; health-check URLs only):

```bash
spacekit network init --mode external \
  --storage-url http://192.168.1.10:3030 \
  --compute-url http://192.168.1.10:9000
spacekit network up
```

Run `spacekit network up` before `storage deploy`, `repo push`, `workspace`, `operator publish`, and full-stack local tests. Keep it running in one terminal; run deploy/call in another.

**`network up` vs `network up --full`**

| Command | Blockchain | Typical use |
|---------|------------|-------------|
| `spacekit network up` | off | Agents, storage, compute, deploy/call — **default for local dev** |
| `spacekit network up --full` | on | Operator rewards, genesis ledger, gateway — **heavier; can grow RSS** |

`--full` enables the compute sidecar's blockchain service and durable SwtchVM state. It does not create a second in-process ledger in the CLI supervisor.

**Deterministic E2E gates**

```bash
# Every change: isolated one-node stack, real storage/messaging/SwtchVM endpoints
cargo run -p spacekit -- network test --suite local --report target/network-e2e/local.json

# Scheduled/deeper trust and topology gates
cargo run -p spacekit -- network test --suite all --report target/network-e2e/all.xml

# Also probe deployed website surfaces in the local suite
cargo run -p spacekit -- network test --suite local \
  --website-url https://spacekit.xyz \
  --api-url https://api.spacekit.xyz \
  --report target/network-e2e/deployed.json
```

Each invocation creates an isolated `HOME`, network profile, service data roots and unique
ports under `<report-stem>.artifacts/`. It stops its supervisor before returning and saves
the generated configs/manifests and command logs. Gates use live HTTP responses and exact
round trips; unavailable protocol surfaces are explicit skips in the report.

Tune in `~/.spacekit/network/config.toml`:

```toml
[blockchain]
enabled = false              # plain `network up` leaves this false; `--full` sets true
block_time_ms = 10000        # default 10s local dev (was 2s)
persist_interval_blocks = 100  # ledger flush cadence (was every 10 blocks)
persist_state = true         # `--full` enables persistence
```

Env overrides: `SPACEKIT_BLOCK_TIME_MS`, `SPACEKIT_BLOCKCHAIN_PERSIST_EVERY`.

**`[runtime]` auth and tokens** (optional in `~/.spacekit/network/config.toml`):

```toml
[runtime]
upload_token_secret = "64-hex-or-passphrase"
blob_fact_auth = "hybrid"   # permissive | hybrid | strict
```

The supervisor sets `SPACEKIT_BLOB_FACT_AUTH`, writes `.upload_token_secret` under the storage data dir, and sets `SPACEKIT_PUBLIC_HTTP_URL` (used by `GET /api/operators/self`). You can also export before `network up`:

```bash
export SPACEKIT_BLOB_FACT_AUTH=hybrid
export SPACEKIT_UPLOAD_TOKEN_SECRET="$(openssl rand -hex 32)"
spacekit network down && spacekit network up
```

Soak tests (storage-node examples): `hybrid_auth_soak`, `strict_auth_soak` — see `spacekit-storage-node/documentation/guides/blob-fact-auth-staging.md`.

### CLI smoke test script

Automated command exercise with logs and artifacts under `spacekit-cli/scratch/`:

```bash
# Terminal 1
spacekit network up

# Terminal 2 (after spacekit init && cargo build -p spacekit)
./spacekit-cli/scripts/cli-smoke-test.sh
```

See [scripts/README.md](scripts/README.md) for env vars (`SPACEKIT_BIN`, `BUILD_HELLO_WASM`, `CLI_SMOKE_CONFIGURE_CONNECT`, `SPACEKIT_GATEWAY_URL`, etc.).

Exercises `network status`, `connect` (configure + test), `repo`, `workspace` (create → export → import), and optional `message list`. Content monetization remains under `content soak dev|router|live`.

Local `network up` serves compute on **port 9000** and the optional gateway on **port 8080**. The smoke script's historical `SPACEKIT_GATEWAY_URL` name points at the compute-compatible surface.

`spacekit-simulator` / `connect simulator` and Anvil-based tests are compatibility lanes. They do not replace profile admission, SpaceKit consensus, storage federation, or signed public/testnet joins.

---

## Troubleshooting: encrypting WASM or other binary files

### Symptom

```text
spacekit encrypt routekit_pinned.wasm -p ~/.spacekit/keys/public_key.hex \
  -o routekit_wasm.enc --kem-secret ~/.spacekit/keys/private_key.hex

❌ Encryption failed: stream did not contain valid UTF-8
```

### Cause

With the default algorithm (`kyber1024`), the CLI uses the **post-quantum** code path. That path reads the input via `read_to_string()` in `spacekit-primitives/src/v1/crypto/quantum.rs`, which **requires valid UTF-8**. A `.wasm` file is binary, so Rust returns `InvalidData` (“stream did not contain valid UTF-8”).

This is **not** caused by your public key path or WASM being “invalid”; it is a **text-only input limitation** in the quantum encrypt implementation.

### What does *not* fix it

- Using `-p ~/.spacekit/keys/public_key.hex` — correct key type for KEM, but input is still read as UTF-8 text.
- Using `--kem-secret ~/.spacekit/keys/private_key.hex` — wrong secret type for encrypt (that file is the long-term KEM **secret key**, not an encapsulation **shared secret**), and the quantum encrypt handler does not use that path today anyway.

### Workarounds

1. **ECIES + dedicated ECIES keypair** (binary-safe, honors `-o`):

   ```bash
   spacekit keypair -a ecies --save -p ecies_pub.hex --secret-key-path ecies_sec.hex
   spacekit encrypt routekit_pinned.wasm -a ecies -p ecies_pub.hex -o routekit_wasm.enc
   ```

2. **Storage envelope API** for uploads to a storage node (binary-safe, zero-knowledge):

   ```bash
   spacekit storage upload-envelope --help
   ```

3. **Wait for / contribute** a fix: change quantum `handle_encryption` to `fs::read()` and encrypt `&[u8]` instead of `&str` (same for decrypt output).

### After a successful quantum encrypt (text files only today)

Outputs are created **next to the input file**, not necessarily at `-o`:

```text
routekit_pinned.wasm.enc
routekit_pinned.wasm.kem
routekit_pinned.wasm.pub
```

---

## Quick reference: `spacekit --help`

<details>
<summary>Example top-level help (may drift from build)</summary>

```text
Usage: spacekit [OPTIONS] <COMMAND>

Commands:
  encrypt         Encrypts a file using ECIES or quantum algorithms
  decrypt         Decrypts a file using ECIES or quantum algorithms
  keypair         Generates a keypair
  encapsulate     Encapsulate shared secret with public key (quantum KEM)
  decapsulate     Decapsulate shared secret with secret key (quantum KEM)
  init            Initialize SpaceKit environment
  new             Create a new project directory
  storage         Storage management commands
  did             DID management commands
  network         Network operations and service discovery
  nft             NFT storage and collection management
  contract        Smart contract deployment and execution
  vm              SpaceKitVM ledger helpers
  connect         Configure connection to remote nodes
  message         Messaging and chat commands
  content         Content publishing and channel management
  app             App package management
  agent           Agent brains (growformer)
  brain-registry  Brain registry manifest publish
  repo            Git-like repo (CAS blobs, facts, refs)
  fact            FactPackage create/get/id
  workspace       Agent workspaces and federation handoff
  operator        Operator discovery manifest
  migration       DID-signed migration manifest verify/sign
  content         Content publishing and channel management
  help            Print help
```

</details>

Further docs:
[`../../docs/guides/developer-network-setup.md`](../../docs/guides/developer-network-setup.md)
· https://docs.spacekit.xyz
