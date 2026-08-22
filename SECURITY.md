# Security policy

SpaceKit Core contains network, cryptographic, identity, contract, and
operator-facing software. Treat security reports as confidential until a fix
and disclosure schedule are agreed.

## Reporting a vulnerability

Do not open a public issue for a suspected vulnerability. Contact the
maintainers through the private security-reporting channel configured for the
repository. Include:

- the affected package and revision;
- reproduction steps or a minimal proof of concept;
- the expected and observed security boundary;
- the likely impact and any known mitigations.

If no private reporting channel is visible after the repository is published,
contact SWTCH Labs directly and request a security contact before sharing
technical details.

## Supported code

Only code indexed by the root README and `docs/README.md` is part of the
supported public surface. Archived documents, incubator contracts, generated
artifacts, private applications, and compatibility simulators are not
production security claims.

`infra/spacekit-storage-node` is proprietary and intentionally excluded from
this public repository. Its presence in a local migration workspace does not
make it part of the public support or disclosure scope.

## Repository safety

Never commit:

- private keys, recovery codes, certificates, tokens, or populated `.env`
  files;
- local network state, wallets, node databases, or contract deployment state;
- model checkpoints, training corpora, generated brains, or experiment output;
- build directories, generated WASM, packaged releases, or smoke-test reports.

Small deterministic fixtures are allowed only when they contain no production
material and their purpose is documented.

## Key exposure response

If secret material enters the working tree or Git history:

1. remove it from the source tree without reproducing it in logs or issues;
2. rotate or revoke the affected credential immediately;
3. inspect downstream environments for use of the exposed credential;
4. rewrite unpublished history before the first push, or coordinate a public
   history purge if it has already been published;
5. document the incident without including the secret value.

Deleting a file does not make an exposed key safe. Rotation is mandatory.

## Release verification

Before a public release:

- run secret and dependency scans against source and history;
- verify network behavior using `docs/guides/developer-network-setup.md`;
- run the relevant contract, node, runtime, and CLI tests;
- confirm generated artifacts can be reproduced from the tagged source;
- review all security claims against executable behavior and current runbooks.
