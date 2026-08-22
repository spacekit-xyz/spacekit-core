#!/bin/bash
set -e

echo "Building spacekit-intent-classifier contract..."

# Build the WASM contract
cargo build --release --target wasm32-unknown-unknown

# Copy to artifacts
cp target/wasm32-unknown-unknown/release/spacekit_intent_classifier.wasm ../artifacts/spacekit_intent_classifier.wasm

# Also copy to website public folder for direct loading
WEBSITE_PUBLIC="../../../spacekit.xyz-website/public/wasm"
if [ -d "$WEBSITE_PUBLIC" ]; then
    cp target/wasm32-unknown-unknown/release/spacekit_intent_classifier.wasm ../../../spacekit.xyz-website/public/wasm
    echo "Copied to website public folder"
fi

# Get file size
SIZE=$(wc -c < target/wasm32-unknown-unknown/release/spacekit_intent_classifier.wasm)
echo "Built spacekit_intent_classifier.wasm ($SIZE bytes)"
