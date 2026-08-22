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
│   └── spacekit-service-rewards/
├── foundation/
│   ├── spacekit-contract-language/
│   └── spacekit-primitives/
├── identity/
│   ├── spacekit-behavioral/
│   ├── spacekit-did/
│   ├── spacekit-did-onchain/
│   ├── spacekit-recovery/
│   └── wasm-did/
├── infra/
│   ├── spacekit-compute-node/
│   ├── spacekit-gateway/
│   ├── spacekit-keymaster/
│   ├── spacekit-messaging-node/
│   └── spacekit-storage-node/
├── observability/
│   └── spacekit-log/
├── operations/
│   └── spacekit-runbook/
├── runtimes/
│   └── spacekit-js/
├── sdks/
│   ├── spacekit-contract-sdk/
│   └── spacekit-standard-library/
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
[dependencies]
growformer = {
  git = "https://github.com/spacekit-xyz/spacekit-ai",
  rev = "d2afc97406fa04f4e5662717afd3e36465e3e5a6",
  default-features = false
}
```

Model checkpoints, trained brain files, corpora, and generated experiment
artifacts are release assets or external datasets; they should not be committed
to the source repository unless they are deliberately small test fixtures.

## Dependency direction

The intended build direction is:

```text
foundation
  ├── data / identity / observability
  │     ├── consensus / economics / sdks
  │     │     └── infra
  │     │           └── tools
  │     └── runtimes
  └─────────────────────└── sdks
```

Higher layers may depend on lower layers. Foundation and reusable domain crates
must not import node binaries, the CLI, private websites, or deployment-specific
configuration.

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

## Workspaces

Rust projects currently use package-level and domain-level Cargo workspaces.
The standard library has its own workspace; node, data, consensus, identity,
and tooling crates can be checked with `--manifest-path`. The JavaScript runtime
maintains its own package workspace.

During migration, internal dependencies should use monorepo-relative paths.
Published packages should also declare repository metadata and compatible
versions so private consumers can depend on tagged releases rather than local
filesystem paths.

## Local development

Expected baseline tooling:

- current stable Rust and Cargo;
- the `wasm32-unknown-unknown` Rust target;
- Node.js 20 or later;
- npm or the package manager selected by the root workspace;
- `wasm-pack` for browser identity and selected runtime artifacts.

Run checks for the package or domain being changed:

```bash
cargo fmt --manifest-path path/to/Cargo.toml --check
cargo check --manifest-path path/to/Cargo.toml
cargo test --manifest-path path/to/Cargo.toml

cd runtimes/spacekit-js
npm install
npm run build --if-present
npm test --if-present
```

Feature-heavy crates such as storage, compute, and Growformer should also have
targeted CI jobs rather than forcing every optional dependency into the baseline
workspace check.

## Releases

- Tag releases from a clean, fully tested commit.
- Publish leaf crates and packages before their dependents.
- Use semantic versions for public Cargo and npm interfaces.
- Record WASM artifact hashes and source revisions in release notes.
- Private applications must consume tags or published packages, not `main`.
- Do not publish generated models, secrets, local network state, or `.env` files.

## Licensing and security

Every migrated project must have an explicit license compatible with public
distribution. License inconsistencies must be resolved before the first public
release; in particular, code marked `Proprietary` cannot silently become open
source by being copied into this repository.

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
