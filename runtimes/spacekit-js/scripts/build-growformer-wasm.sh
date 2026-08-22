#!/usr/bin/env bash
#
# Rebuild spacekit-js/growformer-pkg/ (growformer.js + growformer_bg.wasm) from the Neurokit
# Growformer Rust crate. Browser inference loads these via SpacekitVmContext / bundledWasm (?url).
#
# Layout (typical):
#   …/2026/neurokit/growformer   ← Rust source, wasm-bindgen lib (src/wasm.rs, Cargo.toml)
#   …/2026/spacekit/spacekit-js/growformer-pkg   ← output of this script
#
# Override source tree:
#   GROWFORMER_ROOT=/Users/astor/Projects/2026/neurokit/growformer npm run build:growformer-wasm
#
# Default GROWFORMER_ROOT = <parent of spacekit repo>/../neurokit/growformer
# (MONOREPO_ROOT is the directory containing spacekit-js/, i.e. the spacekit checkout).
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SPACEKIT_JS_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
MONOREPO_ROOT="$(cd "$SPACEKIT_JS_ROOT/.." && pwd)"
GROWFORMER_ROOT="${GROWFORMER_ROOT:-$MONOREPO_ROOT/../neurokit/growformer}"
OUT_PKG="$SPACEKIT_JS_ROOT/growformer-pkg"
WBGEN_VERSION="0.2.115"

if [ ! -f "$GROWFORMER_ROOT/Cargo.toml" ]; then
  echo "Growformer repo not found at: $GROWFORMER_ROOT"
  echo "Set GROWFORMER_ROOT to your neurokit/growformer checkout."
  exit 1
fi

if ! command -v rustup >/dev/null 2>&1; then
  echo "rustup not found; install Rust first."
  exit 1
fi

if ! rustup target list --installed | grep -q "wasm32-unknown-unknown"; then
  rustup target add wasm32-unknown-unknown
fi

if ! command -v wasm-bindgen >/dev/null 2>&1; then
  echo "Installing wasm-bindgen-cli $WBGEN_VERSION (must match growformer/Cargo.toml)..."
  cargo install wasm-bindgen-cli --version "$WBGEN_VERSION"
else
  HAVE="$(wasm-bindgen --version 2>/dev/null | awk '{print $2}')"
  if [ "$HAVE" != "$WBGEN_VERSION" ]; then
    echo "wasm-bindgen is $HAVE; growformer expects $WBGEN_VERSION. Run:"
    echo "  cargo install -f wasm-bindgen-cli --version $WBGEN_VERSION"
    exit 1
  fi
fi

cd "$GROWFORMER_ROOT"
cargo build --release --target wasm32-unknown-unknown --no-default-features --features wasm-bindgen,categorical --lib

mkdir -p "$OUT_PKG"
wasm-bindgen target/wasm32-unknown-unknown/release/growformer.wasm \
  --out-dir "$OUT_PKG" \
  --target web \
  --out-name growformer

echo "Wrote $OUT_PKG/growformer.js and growformer_bg.wasm"
