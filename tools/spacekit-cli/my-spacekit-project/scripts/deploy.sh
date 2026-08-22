#!/bin/bash
# Example SpaceKit task submission script
# Generated for project in /Users/astor/Projects/2026/spacekit/spacekit-cli/my-spacekit-project

echo "🔨 Building WASM contract..."

# Compile to WASM (requires rust wasm32 target)
if command -v rustc >/dev/null 2>&1; then
    rustc --target wasm32-unknown-unknown -O contracts/counter.rs -o counter.wasm
    echo "✅ Contract compiled to counter.wasm"
else
    echo "⚠️  Rust not found. Install Rust to compile contracts."
    echo "Visit: https://rustup.rs/"
fi

echo "🚀 Submitting task to SpaceKit network..."

# Submit to SpaceKit network
spacekit task submit \
  --file counter.wasm \
  --runtime wasm \
  --owner-did did:spacekit:user:95cfbe56-0df8-4aa8-af2a-046ddf8a3312 \
  --encryption Kyber1024

echo "📊 Check task status:"
echo "spacekit task status <task-id>"
