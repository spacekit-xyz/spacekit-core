# P-08: Controlled binary/config rollback

This procedure rolls back an operator deployment. It is not a consensus-state rollback.

Before an upgrade, while the node is stopped:

```bash
spacekit network down
CONFIG="$(spacekit network config path)"
cp "$CONFIG" "$CONFIG.pre-upgrade"
spacekit network config show
```

Create service-consistent data backups using the storage/compute service procedures; copying live data is not guaranteed consistent. Record binary digests:

```bash
shasum -a 256 "$(command -v spacekit)"
```

To roll back, keep the node stopped, restore the approved binary and reviewed profile backup, then validate before startup:

```bash
spacekit network down
CONFIG="$(spacekit network config path)"
cp "$CONFIG.pre-upgrade" "$CONFIG"
spacekit network --help
spacekit network config show
spacekit network up -d
spacekit network doctor
spacekit network status --detailed
```

Restore data only from a service-consistent backup that matches the selected binary/schema. `network reset --data` deletes local data and is not rollback. SpaceKit does not automatically select versions, restore snapshots, revert finalized blocks, or coordinate cluster-wide rollback.
