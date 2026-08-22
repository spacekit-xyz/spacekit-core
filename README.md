# SpaceKit Core

SpaceKit Core is the public infrastructure monorepo for the SpaceKit network. It
contains the reusable protocols, runtimes, nodes, SDKs, smart contracts, machine
learning libraries, command-line tools, and operational documentation required
to build and operate SpaceKit networks.

> **Status:** migration in progress. The directory structure is established,
> but projects are being moved and validated from the leaves of the dependency
> graph upward.
>
> **Public boundary:** `infra/spacekit-storage-node` is proprietary and is
> excluded from the public monorepo. It may exist in a local migration
> workspace, but it is not a public package or supported source release.

## Repository organization

```text
spacekit-core/
├── ai/
│   ├── growformer/
│   ├── growformer-llm/
│   └── growformer-ledger/
├── consensus/
│   ├── spacekit-spacetime-consensus/
│   └── spacekit-unified-consensus/
├── contracts/
│   ├── spacekit-contract-sdk/
│   ├── spacekit-standard-library/
│   └── kit-protocol/
├── data/
│   ├── spacekit-compressor/
│   ├── spacekit-diff/
│   ├── spacekit-quantum-verkle/
│   └── spacekit-repo/
├── economics/
│   ├── routekit/
│   ├── spacekit-payments/
│   └── spacekit-service-rewards/
├── foundation/
│   └── spacekit-primitives/
├── identity/
│   ├── spacekit-did/
│   ├── spacekit-did-onchain/
│   └── wasm-did/
├── infra/
│   ├── spacekit-compute-node/
│   ├── spacekit-gateway/
│   ├── spacekit-keymaster/
│   ├── spacekit-messaging-node/
│   └── (storage integration interfaces; implementation remains private)
├── observability/
│   └── spacekit-log/
├── operations/
│   └── spacekit-runbook/
├── runtimes/
│   └── spacekit-js/
├── sdk/
│   └── spacekit-sdk/
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
- `contracts` owns the contract SDK, reusable contracts, and protocol contracts.
- `infra` contains binaries and process-level integration for network services.
  The proprietary storage-node implementation is maintained outside the public
  repository.
- `runtimes` executes SpaceKit contracts in non-node environments.
- `sdk` exposes stable developer-facing integration APIs.
- `tools` contains operator and developer control-plane applications.
- `operations` contains runbooks, incident procedures, and operational fixtures.

## AI and machine learning

The public AI libraries live under `ai/`, with explicit boundaries:

- `growformer` is the deterministic promote-freeze neural substrate and local
  inference/training library. It is the only Growformer dependency required by
  the SpaceKit CLI and optional compute-node inference.
- `growformer-llm` is the small-domain language-model and chatbot layer. It may
  use Growformer brain memory, but it is not required by the CLI or core network.
- `growformer-ledger` is the append-only experiment and evaluation ledger used
  by `growformer-llm`.

`spacekit-compressor` remains under `data/` because compression is shared
infrastructure used by storage, messaging, and AI rather than an AI-only concern.

The CLI's Growformer integration should be feature-gated:

```toml
[features]
default = []
growformer = ["dep:growformer"]

[dependencies]
growformer = {
  path = "../../ai/growformer",
  optional = true,
  default-features = false
}
```

This keeps the default CLI build small and allows networking, identity, storage,
and contract workflows to compile without the machine-learning toolchain.

Model checkpoints, trained brain files, corpora, and generated experiment
artifacts are release assets or external datasets; they should not be committed
to the source repository unless they are deliberately small test fixtures.

## Dependency direction

The intended build direction is:

```text
foundation
  ├── data / identity / observability
  │     ├── consensus / economics / contracts / ai
  │     │     └── infra
  │     │           └── tools
  │     └── runtimes
  └─────────────────────└── sdk
```

Higher layers may depend on lower layers. Foundation and reusable domain crates
must not import node binaries, the CLI, private websites, or deployment-specific
configuration.

## Migration and validation order

Projects are moved and made green in dependency waves:

1. **Leaves:** primitives, diff, compressor, DID, quantum Verkle, contract SDK,
   log, payments, gateway, and RouteKit.
2. **First-level libraries:** repo, messaging, service rewards, spacetime
   consensus, DID bridges, Growformer, and WASM DID.
3. **Compositions:** storage, unified consensus, standard library, kit protocol,
   Growformer ledger, and Growformer LLM.
4. **Infrastructure:** compute node, keymaster, and remaining network services.
5. **Control plane:** SpaceKit CLI and operational runbook.
6. **JavaScript distribution:** SpaceKit JS runtime, then SpaceKit SDK.

Because these projects share one public monorepo, source migration can happen in
one branch. The order above controls when each package receives independent CI
and release guarantees.

## Workspaces

The repository should expose:

- one root Cargo workspace for Rust crates and binaries;
- one root npm workspace for JavaScript packages;
- a single lockfile per package ecosystem where practical;
- path-filtered CI so unrelated projects do not rebuild on every change.

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

Once the root workspaces are established, the baseline checks should be:

```bash
cargo fmt --all --check
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings

npm install
npm run build --workspaces --if-present
npm test --workspaces --if-present
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
4. verify the proprietary storage-node tree is excluded;
5. follow the vulnerability reporting process in `SECURITY.md`;
6. add branch protection and required CI checks;
7. add `CODEOWNERS` for domain-level maintainership.

## Contributing

Contributions should preserve domain boundaries, include tests for behavior
changes, and update public documentation when commands, configuration, ports, or
wire formats change. Detailed contribution and security policies will live in
`CONTRIBUTING.md` and `SECURITY.md` as the migration is completed.
