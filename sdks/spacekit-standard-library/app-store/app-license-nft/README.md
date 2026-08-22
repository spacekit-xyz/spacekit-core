# App License NFT (`spacekit-app-license-nft`)

Per-content license tokens for SpaceKit content monetization. Implements SwtchVM `main` / `get_result` with opcodes aligned to `spacekit-storage-node` `content_license.rs`:

| Opcode | Value | Purpose |
|--------|-------|---------|
| `OP_MINT` | `0x01` | Mint license; metadata stores `content_id_hex` |
| `OP_HAS_LICENSE` | `0x02` | Returns `1` if buyer owns license for content |

Source: [`../app_license_nft.rs`](../app_license_nft.rs)

## Build

From **workspace root** (`spacekit-standard-library/`):

```bash
cargo build -p spacekit-app-license-nft --release --target wasm32-unknown-unknown
```

Or use the app-store script (builds AppStore + license NFT):

```bash
./app-store/build.sh
```

**Output:** `target/wasm32-unknown-unknown/release/spacekit_app_license_nft.wasm`

## Deploy

```bash
spacekit contract deploy \
  --wasm target/wasm32-unknown-unknown/release/spacekit_app_license_nft.wasm
export SPACEKIT_LICENSE_CONTRACT_ID=<contract-id>
```
