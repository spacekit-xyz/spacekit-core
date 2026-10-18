#!/usr/bin/env bash
#
# Rebuild spacekit-js/growformer-pkg/ (growformer.js + growformer_bg.wasm) from the
# Growformer Rust crate in the spacekit-ai repo. Browser inference loads these via
# SpacekitVmContext / bundledWasm (?url).
#
# Layout (typical):
#   …/2026/spacekit/spacekit-ai/growformer                  ← Rust source (src/wasm.rs, Cargo.toml)
#   …/2026/spacekit/spacekit-core/runtimes/spacekit-js      ← this package
#   …/2026/spacekit/spacekit-core/runtimes/spacekit-js/growformer-pkg  ← output of this script
#
# Override source tree:
#   GROWFORMER_ROOT=/path/to/spacekit-ai/growformer npm run build:growformer-wasm
#
# Default GROWFORMER_ROOT = <spacekit-core>/../spacekit-ai/growformer
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SPACEKIT_JS_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
SPACEKIT_CORE_ROOT="$(cd "$SPACEKIT_JS_ROOT/../.." && pwd)"
GROWFORMER_ROOT="${GROWFORMER_ROOT:-$SPACEKIT_CORE_ROOT/../spacekit-ai/growformer}"
OUT_PKG="$SPACEKIT_JS_ROOT/growformer-pkg"
WBGEN_VERSION="0.2.115"

if [ ! -f "$GROWFORMER_ROOT/Cargo.toml" ]; then
  echo "Growformer crate not found at: $GROWFORMER_ROOT"
  echo "Set GROWFORMER_ROOT to your spacekit-ai/growformer checkout."
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

# growformer is a member of the spacekit-ai workspace, so the build output lands in the
# workspace target directory (or $CARGO_TARGET_DIR), not in growformer/target.
TARGET_DIR="$(cargo metadata --format-version 1 --no-deps | sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p')"
WASM="$TARGET_DIR/wasm32-unknown-unknown/release/growformer.wasm"
if [ ! -f "$WASM" ]; then
  echo "Build output not found at: $WASM"
  exit 1
fi

mkdir -p "$OUT_PKG"
wasm-bindgen "$WASM" \
  --out-dir "$OUT_PKG" \
  --target web \
  --out-name growformer

# Fail loudly if the build lacks the multi-brain API that src/growformer/instances.ts uses.
if ! grep -q "growformer_instance_create" "$OUT_PKG/growformer.d.ts"; then
  echo "error: $OUT_PKG/growformer.d.ts has no growformer_instance_create; GrowformerInstance will not work"
  exit 1
fi

echo "Wrote $OUT_PKG/growformer.js and growformer_bg.wasm from $GROWFORMER_ROOT"
