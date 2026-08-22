#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SPACEKIT_JS_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
ROOT="$(cd "$SPACEKIT_JS_ROOT/.." && pwd)"
PRIMITIVES="$ROOT/spacekit-primitives/wasm-sphincs"
OUT_DIST="$SPACEKIT_JS_ROOT/dist/wasm"
WORKING="$SPACEKIT_JS_ROOT/working"
WEBSITE_OUT="${WEBSITE_WASM_OUT:-}"

# Look for wasi-sdk in multiple locations
WASI_SDK_PATH_DEFAULT=""

# 1. Check working folder first (manual install)
for sdk in "$WORKING/wasi-sdk-"*; do
  if [ -d "$sdk/bin" ]; then
    WASI_SDK_PATH_DEFAULT="$sdk"
    echo "Found wasi-sdk at: $sdk"
    break
  fi
done

# 2. Check homebrew
if [ -z "$WASI_SDK_PATH_DEFAULT" ] && command -v brew >/dev/null 2>&1; then
  WASI_SDK_PATH_DEFAULT="$(brew --prefix wasi-sdk 2>/dev/null || true)"
fi

# Use env override if set, otherwise use detected path
WASI_SDK_PATH="${WASI_SDK_PATH:-$WASI_SDK_PATH_DEFAULT}"
WASI_BIN=""
WASI_SYSROOT=""

if [ -n "$WASI_SDK_PATH" ] && [ -d "$WASI_SDK_PATH" ]; then
  if [ -d "$WASI_SDK_PATH/bin" ]; then
    WASI_BIN="$WASI_SDK_PATH/bin"
    WASI_SYSROOT="$WASI_SDK_PATH/share/wasi-sysroot"
  elif [ -d "$WASI_SDK_PATH/share/wasi-sdk" ]; then
    WASI_BIN="$WASI_SDK_PATH/share/wasi-sdk/bin"
    WASI_SYSROOT="$WASI_SDK_PATH/share/wasi-sdk/share/wasi-sysroot"
  fi
fi

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

cd "$PRIMITIVES"
if [ -z "$WASI_BIN" ] || [ ! -d "$WASI_SYSROOT" ]; then
  echo "wasi-sdk not found. Install via 'brew install --cask wasi-sdk' or set WASI_SDK_PATH." >&2
  exit 1
fi

export CC_wasm32_unknown_unknown="$WASI_BIN/clang"
export AR_wasm32_unknown_unknown="$WASI_BIN/llvm-ar"
export CFLAGS_wasm32_unknown_unknown="--target=wasm32-wasi --sysroot=$WASI_SYSROOT -isystem $WASI_SYSROOT/include/wasm32-wasi -isystem $WASI_SYSROOT/include"

echo "Building with wasi-sdk from: $WASI_SDK_PATH"
RUSTFLAGS='--cfg getrandom_backend="wasm_js"' cargo build --release --target wasm32-unknown-unknown

mkdir -p "$OUT_DIST"
wasm-bindgen --target web --out-dir "$OUT_DIST" --out-name sphincs_wasm \
  target/wasm32-unknown-unknown/release/spacekit_sphincs_wasm.wasm

# Optionally copy to a website public folder if WEBSITE_WASM_OUT is set
if [ -n "$WEBSITE_OUT" ]; then
  mkdir -p "$WEBSITE_OUT"
  cp "$OUT_DIST/sphincs_wasm.js" "$WEBSITE_OUT/"
  cp "$OUT_DIST/sphincs_wasm_bg.wasm" "$WEBSITE_OUT/"
fi

echo "WASM artifacts written to $OUT_DIST"
