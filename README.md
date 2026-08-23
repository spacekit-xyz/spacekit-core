# SpaceKit Core

SpaceKit Core is the public infrastructure monorepo for the SpaceKit network. It
contains the reusable protocols, runtimes, nodes, SDKs, smart contracts, machine
learning integrations, command-line tools, and operational documentation
required to build and operate SpaceKit networks.

> **Status:** monorepo migration and validation are in progress. Package
> metadata and dependency paths target this repository; release guarantees
> remain package-specific until CI is green.

## Repository organization

```text
spacekit-core/
├── consensus/
│   ├── spacekit-spacetime-consensus/
│   └── spacekit-unified-consensus/
├── data/
│   ├── spacekit-compressor/
│   ├── spacekit-diff/
│   ├── spacekit-mempool/
│   ├── spacekit-memsearcher/
│   ├── spacekit-quantum-verkle/
│   └── spacekit-repo/
├── economics/
│   ├── routekit/
│   ├── spacekit-payments/
│   ├── spacekit-service-rewards/
│   └── spacekit-tokenomics/          # TypeScript
├── foundation/
│   ├── spacekit-contract-language/
│   └── spacekit-primitives/
├── identity/
│   ├── spacekit-behavioral/
│   ├── spacekit-did/
│   ├── spacekit-did-onchain/         # nested workspace
│   ├── spacekit-recovery/
│   └── wasm-did/                     # wasm32
├── infra/
│   ├── spacekit-compute-node/
│   ├── spacekit-gateway/
│   ├── spacekit-keymaster/           # nested workspace
│   ├── spacekit-messaging-node/
│   ├── spacekit-pay/                 # TypeScript
│   └── spacekit-storage-node/
├── observability/
│   └── spacekit-log/
├── operations/
│   └── spacekit-runbook/
├── runtimes/
│   └── spacekit-js/                  # TypeScript
├── sdks/
│   ├── spacekit-apps/                # TypeScript
│   ├── spacekit-contract-sdk/        # nested workspace
│   ├── spacekit-sdk/                 # TypeScript
│   └── spacekit-standard-library/    # nested workspace
├── tools/
│   └── spacekit-cli/
└── docs/
```

Directory names describe ownership and responsibility, not implementation
language. Deployable services belong in `infra`; reusable domain logic belongs
in the relevant domain directory.

### Domain boundaries

- `foundation` contains stable cross-network types and cryptographic primitives.
  It must not depend on higher-level domains.
- `data` contains repository semantics, diffing, compression, and state-integrity
  structures. These are not generic utilities.
- `identity` owns DID creation, verification, bridges, and browser/WASM identity.
- `consensus` owns consensus algorithms and extensions, not node process wiring.
- `economics` owns payment, routing, accounting, and reward policy.
- `observability` owns structured protocol and operational event logging.
- `infra` contains binaries and process-level integration for network services.
- `runtimes` executes SpaceKit contracts in non-node environments.
- `sdks` owns the contract SDK, reusable contracts, and stable developer-facing
  integration APIs.
- `tools` contains operator and developer control-plane applications.
- `operations` contains runbooks, incident procedures, and operational fixtures.

## Dependency direction

Layers may depend on anything below them, never on anything above:

1. `foundation` — depends on nothing else in this repository.
2. `data`, `identity`, `observability` — depend only on `foundation`.
3. `consensus`, `economics`, `sdks`, `runtimes` — depend on layers 1–2.
4. `infra` — depends on layers 1–3.
5. `tools` — depends on any layer below.

Foundation and reusable domain crates must not import node binaries, the CLI,
private websites, or deployment-specific configuration. A dependency that
inverts this order is a design error, not a build problem to be worked around.

## Workspaces

`spacekit-core/Cargo.toml` is the Cargo workspace root for most Rust crates in
this repository. Build, check, or test any member by package name from the
repository root:

```bash
cargo build -p spacekit-gateway
cargo check --workspace --all-targets
cargo test  --workspace
```

Three package names differ from their directory names. `-p` takes the package
name:

| directory                               | package name                   |
|-----------------------------------------|--------------------------------|
| `tools/spacekit-cli`                    | `spacekit`                     |
| `foundation/spacekit-contract-language` | `spacekit-contract-lang`       |
| `data/spacekit-quantum-verkle`          | `spacekit-quantum-verkle-tree` |

`cargo metadata --no-deps --format-version 1 | jq -r '.packages[].name'` prints
the authoritative list.

### Nested workspaces

Cargo does not support nested workspaces. These four directories are separate
workspace roots with their own lockfiles and dependency graphs. They are not
reachable by `-p` from the repository root; `cd` into them first:

- `sdks/spacekit-standard-library`
- `sdks/spacekit-contract-sdk`
- `identity/spacekit-did-onchain`
- `infra/spacekit-keymaster`

### Excluded crates

The following are excluded from the root workspace and built by their own
scripts or toolchains:

- `identity/wasm-did`, `data/spacekit-quantum-verkle/wasm-quantum-verkle`,
  `foundation/spacekit-primitives/wasm-kyber`, and
  `foundation/spacekit-primitives/wasm-sphincs` target `wasm32-unknown-unknown`.
- `foundation/spacekit-primitives/vendor/pqcrypto-*` is vendored third-party
  source. It is excluded so that `--workspace` lint, format, and test runs skip
  it; `[patch.crates-io]` in the root manifest still redirects to these paths.
- `fuzz/` directories follow the cargo-fuzz convention and carry their own
  lockfiles.

### Internal dependencies

Internal crate dependencies are declared once in `[workspace.dependencies]` in
the root manifest and inherited by members:

```toml
# root Cargo.toml
[workspace.dependencies]
spacekit-primitives = { path = "foundation/spacekit-primitives" }
spacekit-log        = { path = "observability/spacekit-log" }
```

```toml
# member Cargo.toml
[dependencies]
spacekit-primitives = { workspace = true }
```

Do not add new `path = "../../<domain>/<crate>"` dependencies to member
manifests. Relative paths encode the current directory layout into every
dependent crate and break whenever a crate moves between domains. The same
applies to shared external dependencies and to `[workspace.package]` fields
(`edition`, `rust-version`, `license`), which members inherit with
`edition.workspace = true`.

## Overflow checks are a consensus invariant

The root workspace sets:

```toml
[profile.release]
overflow-checks = true
```

Balance, stake, and reward arithmetic must never wrap silently. A panic on
overflow takes down one node; a silent wrap corrupts consensus state across the
whole network and is not detectable after the fact.

**Cargo profile settings apply only within a single workspace.** Every nested
workspace root listed above must set `overflow-checks = true` in its own
manifest. This matters most in `sdks/spacekit-standard-library`, whose
`payments/`, `rewards/`, `tokens/`, and `marketplace/` contracts perform exactly
the arithmetic this rule protects. A `[profile]` section in a workspace *member*
is ignored with a warning and provides no protection.

## AI and machine learning

Public AI libraries live in the separate
[SpaceKit AI monorepo](https://github.com/spacekit-xyz/spacekit-ai).
`spacekit-core` consumes Growformer as a pinned Git dependency for CLI and
optional compute-node inference; it does not duplicate AI source.

`spacekit-compressor` remains under `data/` because compression is shared
infrastructure used by storage, messaging, and AI rather than an AI-only concern.

Growformer dependencies must identify the SpaceKit AI repository and a tested
revision:

```toml
[dependencies.growformer]
git = "https://github.com/spacekit-xyz/spacekit-ai"
rev = "d2afc97406fa04f4e5662717afd3e36465e3e5a6"
default-features = false
```

Model checkpoints, trained brain files, corpora, and generated experiment
artifacts are release assets or external datasets; they should not be committed
to the source repository unless they are deliberately small test fixtures.

## Migration and validation order

Projects are moved and made green in dependency waves:

1. **Leaves:** primitives, diff, compressor, DID, quantum Verkle, contract SDK,
   log, payments, gateway, and RouteKit.
2. **First-level libraries:** repo, messaging, service rewards, spacetime
   consensus, DID bridges, and WASM DID.
3. **Compositions:** storage, unified consensus, standard library, and protocol
   contracts.
4. **Infrastructure:** compute node, keymaster, and remaining network services.
5. **Control plane:** SpaceKit CLI and operational runbook.
6. **JavaScript distribution:** SpaceKit JS runtime, then SpaceKit SDK.

Because these projects share one public monorepo, source migration can happen in
one branch. The order above controls when each package receives independent CI
and release guarantees.

## Local development

Expected baseline tooling:

- Rust and Cargo 1.82 or later (pinned by `rust-version` in
  `[workspace.package]`);
- the `wasm32-unknown-unknown` Rust target;
- Node.js 20 or later;
- npm or the package manager selected by the root workspace;
- `wasm-pack` for browser identity and selected runtime artifacts.

For crates in the root workspace, run checks from the repository root:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets
cargo check --workspace --all-targets
cargo test -p <package-name>
```

For nested workspaces and excluded crates, run from inside the crate or pass
`--manifest-path`:

```bash
cd sdks/spacekit-standard-library && cargo test --workspace
cargo check --manifest-path infra/spacekit-keymaster/Cargo.toml
```

JavaScript packages:

```bash
cd runtimes/spacekit-js
npm install
npm run build --if-present
npm test --if-present
```

Feature-heavy crates such as storage, compute, and Growformer should also have
targeted CI jobs rather than forcing every optional dependency into the baseline
workspace check.

`.build-context/` and `.build-context-smoke/` under `infra/spacekit-compute-node/`
are generated Docker build contexts, not source. They are gitignored and safe to
delete; Docker regenerates them.

### Adding a crate

1. Place it in the directory matching its domain responsibility, not its
   language or its consumer.
2. Confirm it appears in the workspace:
   `cargo metadata --no-deps --format-version 1 | jq -r '.packages[].name'`.
   Category directories are globbed, so a new crate is picked up automatically —
   but a directory under a category with no `Cargo.toml` will fail the entire
   workspace until it is excluded.
3. Declare internal dependencies through `[workspace.dependencies]`.
4. Inherit `edition`, `rust-version`, and `license` from `[workspace.package]`.
5. Verify the dependency direction above is not inverted.

## Releases

- Tag releases from a clean, fully tested commit.
- Publish leaf crates and packages before their dependents.
- Use semantic versions for public Cargo and npm interfaces.
- Record WASM artifact hashes and source revisions in release notes.
- Private applications must consume tags or published packages, not `main`.
- Do not publish generated models, secrets, local network state, or `.env` files.

## Licensing and security

Every migrated project must have an explicit license compatible with public
distribution. 

Before the first push:

1. scan source and history for credentials and private keys;
2. exclude `.env`, local node state, model artifacts, and build output;
3. rotate any credentials that have appeared in the migration workspace;
4. follow the vulnerability reporting process in `SECURITY.md`;
5. add branch protection and required CI checks;
6. add `CODEOWNERS` for domain-level maintainership.

## Contributing

Contributions should preserve domain boundaries, include tests for behavior
changes, and update public documentation when commands, configuration, ports, or
wire formats change. Detailed contribution and security policies will live in
`CONTRIBUTING.md` and `SECURITY.md` as the migration is completed.