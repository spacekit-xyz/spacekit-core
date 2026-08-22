# SpaceKit Core documentation

This directory indexes supported, cross-cutting SpaceKit infrastructure
documentation. Implementation-specific references remain beside the project
they describe.

## Source precedence

When two sources disagree, use this order:

1. executable behavior: `spacekit <command> --help`, generated network
   configuration, manifests, schemas, tests, and service defaults;
2. operator guidance: the developer network guide, CLI command reference, and
   operational runbook;
3. trust and admission: the network manifest schema and signed fixtures;
4. service contracts: compute, messaging, gateway, keymaster, runtime, and
   contract documentation;
5. economics: `economics/spacekit-tokenomics/`;
6. technical narrative: the canonical SpaceKit whitepaper;
7. historical, draft, marketing, or generated material.

Lower-ranked material must not override executable behavior.

## Current guides

- [Developer network setup](guides/developer-network-setup.md) — local,
  permissioned-private, and public/testnet participation.
- [Security verification](security/security-verification.md) — offline and
  live checks for compute and runtime security boundaries.
- [CLI command reference](../tools/spacekit-cli/COMMANDS.md).
- [Network manifest schema](../tools/spacekit-cli/docs/schema/network-manifest-v1.md).
- [Operational runbook](../operations/spacekit-runbook/README.md).

## Technical specifications

- [Tool call specification](SPACEKIT-TOOL-CALL-SPEC.md).
- [SpaceKit whitepaper](spacekit-whitepaper/SpaceKit-Whitepaper.md).
- [Compute VM parity](../infra/spacekit-compute-node/documentation/VM_PARITY.md).
- [Consensus implementation notes](../infra/spacekit-compute-node/documentation/SPACEKIT_CONSENSUS_UNIFIED.md).
- [JavaScript host ABI](../runtimes/spacekit-js/docs/host-abi-contracts.md).
- [Canonical economics](../economics/spacekit-tokenomics/README.md).

## Document status

- `old_docs/` is historical and is not current implementation or operations
  guidance.
- `SPACETIME_DOCS_INPROGRESS/` contains unreviewed drafts.
- Historical whitepaper chapters, pitch material, and status reports are
  retained for research only.
- CLI `scratch/`, build output, generated reports, and model experiment logs
  are not documentation and are excluded from publication.

The alternate `SPACEKIT-WHITEPAPER.md` is a noncanonical draft pending
editorial consolidation. The sole canonical narrative is
`spacekit-whitepaper/SpaceKit-Whitepaper.md`.

## Public/private boundary

The storage-node source is included in the repository and covered by the
security-reporting policy, while retaining the package-specific proprietary
license declared in its manifest.

Private websites, website APIs, consumer apps, and unpublished product
contracts keep their own documentation outside this repository.
