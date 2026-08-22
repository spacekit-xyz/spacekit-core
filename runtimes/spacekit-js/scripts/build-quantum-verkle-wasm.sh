#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SPACEKIT_JS_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
ROOT="$(cd "$SPACEKIT_JS_ROOT/.." && pwd)"
VERKLE_WASM="$ROOT/spacekit-quantum-verkle/wasm-quantum-verkle"
OUT_DIST="$SPACEKIT_JS_ROOT/dist/wasm"
WEBSITE_OUT="${WEBSITE_WASM_OUT:-}"

if ! command -v rustup >/dev/null 2>&1; then
  echo "rustup not found; install Rust toolchain first."
  exit 1
fi

if ! rustup target list --installed | grep -q "wasm32-unknown-unknown"; then
  rustup target add wasm32-unknown-unknown
fi

if ! command -v wasm-bindgen >/dev/null 2>&1; then
  cargo install wasm-bindgen-cli --version 0.2.108
else
  WASM_BINDGEN_VERSION="$(wasm-bindgen --version 2>/dev/null | awk '{print $2}')"
  if [ "$WASM_BINDGEN_VERSION" != "0.2.108" ]; then
    cargo install -f wasm-bindgen-cli --version 0.2.108
  fi
fi

cd "$VERKLE_WASM"
cargo build --release --target wasm32-unknown-unknown

mkdir -p "$OUT_DIST"
wasm-bindgen --target web --out-dir "$OUT_DIST" --out-name quantum_verkle_wasm \
  target/wasm32-unknown-unknown/release/spacekit_quantum_verkle_wasm.wasm

if [ -n "$WEBSITE_OUT" ]; then
  mkdir -p "$WEBSITE_OUT"
  cp "$OUT_DIST/quantum_verkle_wasm.js" "$WEBSITE_OUT/"
  cp "$OUT_DIST/quantum_verkle_wasm_bg.wasm" "$WEBSITE_OUT/"
fi

echo "WASM artifacts written to $OUT_DIST"
