#!/bin/bash

# Build app store WASM contracts

set -e  # Exit on error

echo "🔨 Building SpaceKit AppStore Contracts..."
echo ""

# Clean previous builds
echo "🧹 Cleaning previous builds..."
cargo clean 2>/dev/null || true

# Build from workspace root (app-store + app-license-nft)
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
echo ""
echo "📦 Building AppStore + App License NFT..."
cargo build -p app_store_contract -p spacekit-app-license-nft \
  --target wasm32-unknown-unknown --release --manifest-path "$ROOT/Cargo.toml"

WASM_DIR="$ROOT/target/wasm32-unknown-unknown/release"

if [ -f "$WASM_DIR/app_store_contract.wasm" ]; then
    echo "✅ AppStore: $WASM_DIR/app_store_contract.wasm"
    ls -lh "$WASM_DIR/app_store_contract.wasm" | awk '{print "   📏 Size:", $5}'
else
    echo "❌ AppStore WASM not found"
    exit 1
fi

if [ -f "$WASM_DIR/spacekit_app_license_nft.wasm" ]; then
    echo "✅ App License NFT: $WASM_DIR/spacekit_app_license_nft.wasm"
    ls -lh "$WASM_DIR/spacekit_app_license_nft.wasm" | awk '{print "   📏 Size:", $5}'
else
    echo "❌ App License NFT WASM not found"
    exit 1
fi

echo ""
echo "✅ Build complete!"
echo ""
echo "📋 Built contracts (workspace target):"
echo "   ✓ app_store_contract.wasm"
echo "   ✓ spacekit_app_license_nft.wasm"
echo ""
