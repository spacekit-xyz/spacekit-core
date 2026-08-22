# SpaceKit developer network setup

This is the operational reference for the profile-driven `spacekit network`
implementation. Run `spacekit <command> --help` before copying commands from
older documents.

> The storage-node source is included under package-specific proprietary terms.
> Build it from `infra/spacekit-storage-node`, or select an existing binary
> through `SPACEKIT_STORAGE_BIN`.

## Prerequisites

- stable Rust and Cargo;
- `curl`, `jq`, and Python 3.11 or later;
- free ports for enabled services;
- built `spacekit`, compute, and messaging binaries;
- an authorized storage binary when storage is enabled.

During the monorepo migration, build each project from its own directory until
the root Cargo workspace is established. Export explicit sidecar paths:

```bash
export SPACEKIT_COMPUTE_BIN=/absolute/path/to/spacekit-compute-node
export SPACEKIT_MESSAGING_BIN=/absolute/path/to/spacekit-messaging-http
export SPACEKIT_STORAGE_BIN=/absolute/path/to/authorized/spacekit-storage-node
spacekit --help
spacekit network --help
```

## Canonical local profile

`spacekit init` creates identity configuration. `spacekit network init` creates
the separate network profile.

```bash
spacekit init --algorithm kyber1024 --network testnet --validate
spacekit network init --profile local --role validator --force
spacekit network config show
spacekit network up -d
spacekit network doctor
spacekit network status --detailed
spacekit network down
```

The canonical profile is the output of `spacekit network config show`, not a
copied TOML fragment. Override its path with `SPACEKIT_NETWORK_CONFIG`.
`network up --full` additionally enables the configured gateway and blockchain
services.

## Default port map

All profile allocations are collision-checked. `--port-offset` shifts every
entry.

<!-- network-port-map:start -->
| Profile key | Default port | Surface |
|---|---:|---|
| `storage_http` | 3030 | storage HTTP |
| `storage_p2p` | 4001 | storage libp2p |
| `compute_http` | 9000 | compute HTTP |
| `compute_p2p` | 9001 | compute P2P |
| `messaging_listen` | 7100 | messaging listen socket |
| `messaging_bootstrap` | 7000 | default messaging bootstrap |
| `messaging_http` | 17000 | messaging HTTP/SSE |
| `gateway_http` | 8080 | gateway HTTP |
| `status_http` | 9100 | supervisor status HTTP |
| `keymaster_coordinator` | 8780 | keymaster coordinator |
| `keymaster_registry` | 8770 | keymaster registry |
| `keymaster_guardian_base` | 8781 | first guardian |
<!-- network-port-map:end -->

Compute HTTP uses `:9000`. Port `:8080` belongs to the gateway.

## Permissioned private cluster

Use isolated homes and a reviewed private manifest. The checked-in fixture is
for development and is not a production trust root.

```bash
export SPACEKIT_BIN=/absolute/path/to/spacekit
export CLUSTER_ROOT="$PWD/target/private-cluster"
export MANIFEST="$PWD/tools/spacekit-cli/configs/network-private-3-node.manifest.json"
mkdir -p "$CLUSTER_ROOT"

for node in a b c; do
  HOME="$CLUSTER_ROOT/node-$node" "$SPACEKIT_BIN" init \
    --did "did:spacekit:private:node-$node" --network testnet
  HOME="$CLUSTER_ROOT/node-$node" "$SPACEKIT_BIN" network join \
    --manifest "$MANIFEST" --role validator --force
done

SPACEKIT_BIN="$SPACEKIT_BIN" CLUSTER_ROOT="$CLUSTER_ROOT" \
  ./operations/spacekit-runbook/procedures/configure-private-cluster-ports.sh

for node in a b c; do
  HOME="$CLUSTER_ROOT/node-$node" "$SPACEKIT_BIN" network up -d
  HOME="$CLUSTER_ROOT/node-$node" "$SPACEKIT_BIN" network doctor
done
```

Offsets `0`, `20000`, and `40000` keep profile ranges disjoint. The manifest
enforces membership, genesis, chain ID, protocol, and role grants. It does not
provision hosts, firewalls, DNS, certificates, or load balancers.

## Signed public/testnet join

A public join is defined by a `profile: "public"` manifest carrying a valid
SPHINCS-128f signature. The `testnet` identity label alone does not establish
admission or trust.

```bash
spacekit network manifest keygen \
  --public-key network-manifest.pub.hex \
  --secret-key network-manifest.sec.hex
spacekit network manifest sign unsigned-public-manifest.json \
  --key-id did:spacekit:testnet:authority#network-signing \
  --public-key network-manifest.pub.hex \
  --secret-key network-manifest.sec.hex \
  --output signed-public-manifest.json
spacekit network manifest verify signed-public-manifest.json

spacekit network join \
  --manifest signed-public-manifest.json --role subscriber --force
spacekit network config show
spacekit network up
```

Subscribers may be unlisted where the public manifest permits it. Operators and
validators require explicit matching member roles. Pin the authority key
fingerprint through an independent trusted channel.

## Roles

- `subscriber` consumes public services;
- `operator` runs explicitly admitted services;
- `validator` participates in consensus after explicit admission.

Changing a local role cannot grant authority absent from the manifest.

## Reset, tests, and reports

```bash
spacekit network down
spacekit network reset --data

spacekit network test --suite local --report target/network-e2e/local.json
spacekit network test --suite private --report target/network-e2e/private.xml
spacekit network test --suite public --report target/network-e2e/public.json
spacekit network test --suite all --report target/network-e2e/all.xml
```

Reset removes configured local service data. It does not revoke identities,
rotate keys, modify a remote network, or perform a consensus rollback.

Check this guide against a built CLI:

```bash
python3 tools/spacekit-cli/scripts/check-network-docs.py \
  --spacekit-bin target/debug/spacekit \
  --doc docs/guides/developer-network-setup.md \
  --fixture tools/spacekit-cli/configs/network-public.manifest.json
```

## Security boundaries

- Keep identity and manifest-signing keys separate.
- A manifest is admission policy, not transport security.
- Do not expose `0.0.0.0` listeners without network controls and TLS.
- Never edit a signed manifest; review an unsigned copy and sign it again.
- Back up service state according to each service's consistency requirements.
- Follow [the operational runbook](../../operations/spacekit-runbook/README.md)
  for manifest integrity, key rotation, federation, and rollback incidents.

## Compatibility lanes

`spacekit-simulator` and `connect simulator` are compatibility/testing surfaces,
not the canonical profile-driven runtime. Anvil is used by specific EVM
contract and entitlement tests; it is not SpaceKit consensus, federation,
admission, or public-network membership.
