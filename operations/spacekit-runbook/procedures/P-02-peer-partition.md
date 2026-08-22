# P-02: Peer partition diagnosis

Run on each affected node with that node's `HOME` and profile environment:

```bash
spacekit network status --detailed
spacekit network peers --detailed
spacekit network doctor
spacekit network logs --service messaging --lines 300
spacekit network config show
```

Compare `messaging.bootstrap_peers`, listen addresses, manifest network/chain/genesis values, and the live peer output across nodes. `network peers` returns an error when no configured service exposes peer state; it does not synthesize connectivity.

Test the advertised host/port from another host with an approved network tool, then fix routing, firewall, DNS, or bootstrap configuration outside SpaceKit. Restart only the isolated node after the path is restored:

```bash
spacekit network down
spacekit network up -d
spacekit network peers --detailed
```

Do not change genesis, admit new validators, or reset data to work around a partition.
