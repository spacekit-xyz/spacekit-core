# Astra Access Control

Role-based access control (RBAC) WASM smart contract for the SpaceKit VM. Uses `spacekit_storage` to persist roles and admins.

## Operations

| Op | Name        | Description |
|----|-------------|-------------|
| 1  | GRANT_ROLE  | Grant role to account (role, account) |
| 2  | REVOKE_ROLE | Revoke role from account |
| 3  | HAS_ROLE    | Check if account has role → returns has: u8 |
| 4  | SET_ADMIN   | Set admin for role |
| 5  | GET_ADMIN   | Get admin for role |

Input format: op byte, then length-prefixed UTF-8 strings per operation (see `src/lib.rs`).

## Building

From this directory or workspace root:

```bash
cargo build --target wasm32-unknown-unknown --release
```

Output: `target/wasm32-unknown-unknown/release/astra_access_control.wasm` (or similar from workspace).

## License

Apache 2.0. See the root of the SpaceKit Standard Library for details.
