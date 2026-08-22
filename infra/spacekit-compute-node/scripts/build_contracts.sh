#!/bin/bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONTRACTS_DIR="$ROOT_DIR/contracts"
ARTIFACTS_DIR="$CONTRACTS_DIR/artifacts"

mkdir -p "$ARTIFACTS_DIR"

build_contract() {
  local manifest_path="$1"
  local wasm_path="$2"
  local name="$3"

  echo "🔨 Building $name..."
  cargo build --manifest-path "$manifest_path" --target wasm32-unknown-unknown --release

  if [ ! -f "$wasm_path" ]; then
    echo "❌ Build failed: $wasm_path not found"
    exit 1
  fi

  cp "$wasm_path" "$ARTIFACTS_DIR/"
  echo "✅ $name -> $(basename "$wasm_path")"
}

echo "📦 Building SpaceKit WASM contracts..."

build_contract "$CONTRACTS_DIR/app-store/Cargo.toml" \
  "$CONTRACTS_DIR/app-store/target/wasm32-unknown-unknown/release/app_store_contract.wasm" \
  "AppStore"

build_contract "$CONTRACTS_DIR/compression-service/Cargo.toml" \
  "$CONTRACTS_DIR/compression-service/target/wasm32-unknown-unknown/release/compression_service_contract.wasm" \
  "CompressionService"

build_contract "$CONTRACTS_DIR/astra-erc20/Cargo.toml" \
  "$CONTRACTS_DIR/astra-erc20/target/wasm32-unknown-unknown/release/astra_erc20_contract.wasm" \
  "ASTRA ERC-20"

build_contract "$CONTRACTS_DIR/astra-erc721/Cargo.toml" \
  "$CONTRACTS_DIR/astra-erc721/target/wasm32-unknown-unknown/release/astra_erc721_contract.wasm" \
  "ASTRA ERC-721"

build_contract "$CONTRACTS_DIR/ausd-stablecoin/Cargo.toml" \
  "$CONTRACTS_DIR/ausd-stablecoin/target/wasm32-unknown-unknown/release/ausd_stablecoin_contract.wasm" \
  "aUSD Stablecoin"

echo ""
echo "📁 Artifacts in: $ARTIFACTS_DIR"
ls -lh "$ARTIFACTS_DIR"/*.wasm
