#!/bin/bash

# Build compression service WASM contract

echo "🔨 Building Compression Service Contract..."

cargo build --target wasm32-unknown-unknown --release

if [ $? -eq 0 ]; then
    echo "✅ Build successful!"
    echo "📦 WASM at: target/wasm32-unknown-unknown/release/compression_service_contract.wasm"
    
    # Show file size
    ls -lh target/wasm32-unknown-unknown/release/compression_service_contract.wasm
else
    echo "❌ Build failed"
    exit 1
fi

