# Contributing to SpaceKit Core

SpaceKit Core is the public infrastructure monorepo for reusable SpaceKit
protocols, nodes, runtimes, contracts, SDKs, tools, and operational guidance.

## Before contributing

Read:

- `README.md` for domain ownership and dependency direction;
- `docs/README.md` for documentation authority;
- `SECURITY.md` before reporting vulnerabilities or handling credentials.

The repository is migrating from independent projects into bounded domains.
Avoid unrelated moves or dependency rewrites in feature pull requests.

## Public and private boundaries

Public infrastructure belongs in this repository. Websites, website APIs,
consumer applications, certificate material, and unpublished product contracts
remain in their private repositories.

`infra/spacekit-storage-node` is part of the public monorepo. Its generated
state, uploaded content, databases, credentials, and keys must never be
committed.

## Change requirements

- Keep dependencies pointed from higher layers to lower layers.
- Add or update tests for behavior changes.
- Update commands, ports, schemas, and runbooks when executable behavior
  changes.
- Keep compatibility simulators clearly labeled as noncanonical.
- Do not commit generated build output, local network state, model data, keys,
  certificates, populated environment files, or smoke-test reports.
- Keep crate and package licenses explicit and compatible with their
  dependencies.

## Documentation

Executable behavior and generated configuration take precedence over prose.
Use links to implementation-adjacent references instead of copying them into
central documentation. Historical and draft documents must carry an explicit
status notice and must not be linked as current operator guidance.

Network documentation changes should be checked with:

```bash
python3 tools/spacekit-cli/scripts/check-network-docs.py \
  --spacekit-bin target/debug/spacekit \
  --doc docs/guides/developer-network-setup.md \
  --fixture tools/spacekit-cli/configs/network-public.manifest.json
```

## Local checks

Run the checks supported by the projects you changed. The target end state is:

```bash
cargo fmt --all --check
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings

npm install
npm run build --workspaces --if-present
npm test --workspaces --if-present
```

The root workspaces are not established yet, so during migration run equivalent
checks from each affected project.

## Pull requests

Keep changes reviewable and explain:

- the domain and public API affected;
- tests and documentation checks performed;
- migrations or compatibility implications;
- security-sensitive behavior or generated artifacts involved.
