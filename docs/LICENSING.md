# SpaceKit workspace licensing

This repository is a **workspace of multiple crates** with **different rightsholders and SPDX identifiers**. There is no single license for the entire tree—always check each crate’s **`Cargo.toml`** (`license` / `license-file`), **crate `LICENSE`**, and **crate README** before redistributing or combining components.

## Strategy (compute / network-facing nodes)

For **network-service-shaped** execution infrastructure, SpaceKit aims to keep node and execution-layer code **strong copyleft** so improvements flow back to operators and users who interact with deployed services.

**`spacekit-compute-node`** is released under **GNU AGPL-3.0 or later** (library + binary + examples in that crate—see [`spacekit-compute-node/LICENSE`](spacekit-compute-node/LICENSE)). Running a modified version as a **network service** typically requires you to **offer corresponding source** to interacting users under the AGPL; read the license text and seek counsel for your deployment model.

Other crates (storage node, messaging, payments, SDKs, compilers) may use **permissive**, **proprietary**, or **other** terms—**do not assume parity** with the compute node.

## Illustrative per-crate identifiers (verify before shipping)

Values below are taken from crate manifests in this workspace and may drift—**re-read `Cargo.toml`** on the branch you ship.

| Crate / area | SPDX / license-file (indicative) | Notes |
|----------------|-------------------------------------|--------|
| `spacekit-compute-node` | `AGPL-3.0-or-later` | Strong copyleft for this crate’s sources |
| `spacekit-storage-node` | Proprietary | Source-available posture described in that crate’s README |
| `spacekit-contract-sdk` | Apache-2.0 | Contract authoring SDK |
| `spacekit-contract-lang` (SKCL compiler) | See `contract-lang/LICENSE` in **spacekit-contract-sdk** | Compiler crate uses `license-file`; lives in workspace member `contract-lang/` |
| `spacekit-payments`, `spacekit-diff`, `spacekit-repo` | Apache-2.0 | Permissive |
| `spacekit-did`, `spacekit-quantum-verkle` | MIT OR Apache-2.0 | Dual permissive |
| `spacekit-messaging-node` | MIT | Permissive |
| `spacekit-primitives` | *(often unset in manifest—confirm)* | Defaults are not SPDX-clear until declared |

Workspace roots and whitepapers may use separate documentation terms; treat specs and prose independently of Rust crate licenses.

## Commercial use and embedding

AGPL obligations attach to **distribution** and **network interaction** in ways LGPL does not. If you need **alternative terms** (e.g. proprietary deployment without AGPL source-offer obligations), contact **[SpaceKit](https://spacekit.xyz)**.

## Trademarks

**SpaceKit™** and related marks are trademarks of their respective owners. Open-source or source-available **code licenses do not grant a trademark license**; forks should avoid implying endorsement or confusing similarity unless permitted.

## Contributing

Contribution licensing is governed per repository policy. There is **no monorepo-wide CLA** checked into this tree yet—follow instructions in each project’s contributing guide when added.
