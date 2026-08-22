# Compression Service

WASM contract that exposes compression via the host. Uses the `swtch_compress` host module (e.g. Python SWTCH compressor). Exports `main` (input → compressed output, adaptive mode) and `compress_with_mode` (input + mode string). Does not use the SpaceKit contract SDK; has a custom allocator and direct host imports.

## Entry points

| Function            | Description |
|---------------------|-------------|
| `main`             | Compress input buffer (adaptive mode); returns compressed length or 0 on failure. |
| `compress_with_mode` | Compress with a given mode string; returns compressed length. |

Host: `swtch_compress.python_compress`, `env.log_output`. Input/output are raw pointers and lengths (see `src/lib.rs`).

## Building

From this directory or workspace root:

```bash
cargo build --target wasm32-unknown-unknown --release
```

Output: `target/wasm32-unknown-unknown/release/compression_service_contract.wasm` (or similar from workspace).

## License

Apache 2.0. See the root of the SpaceKit Standard Library for details.
