#!/bin/bash
# Build SpaceKit Compute Node binary using Docker
# This creates a Linux x86_64 binary compatible with Ubuntu 22.04 (AWS EC2)

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
# PROJECT_ROOT is the parent directory (spacekit), where path deps are siblings
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
# growformer lives at ../../neurokit/growformer from spacekit-compute-node (sibling repo).
# Example: …/2026/spacekit + …/2026/neurokit/growformer → MONOREPO_PARENT=…/2026
MONOREPO_PARENT="${MONOREPO_PARENT:-$(dirname "$PROJECT_ROOT")}"
GROWFORMER_DIR="${GROWFORMER_DIR:-$MONOREPO_PARENT/neurokit/growformer}"
COMPUTE_NODE_DIR="$SCRIPT_DIR"
SPACEKIT_DIR="$PROJECT_ROOT/spacekit-primitives"
SPACEKIT_DID_DIR="$PROJECT_ROOT/spacekit-did"
SPACEKIT_DID_ONCHAIN_DIR="$PROJECT_ROOT/spacekit-did-onchain"
SPACEKIT_QUANTUM_VERKLE_DIR="$PROJECT_ROOT/spacekit-quantum-verkle"
STORAGE_NODE_DIR="$PROJECT_ROOT/spacekit-storage-node"
MESSAGING_NODE_DIR="$PROJECT_ROOT/spacekit-messaging-node"
RECOVERY_DIR="$PROJECT_ROOT/spacekit-recovery"
PAYMENTS_DIR="$PROJECT_ROOT/spacekit-payments"
REPO_DIR="$PROJECT_ROOT/spacekit-repo"
DIFF_DIR="$PROJECT_ROOT/spacekit-diff"
SERVICE_REWARDS_DIR="$PROJECT_ROOT/spacekit-service-rewards"
LOG_DIR="$PROJECT_ROOT/spacekit-log"
SPACETIME_CONSENSUS_DIR="$PROJECT_ROOT/spacekit-spacetime-consensus"
UNIFIED_CONSENSUS_DIR="$PROJECT_ROOT/spacekit-unified-consensus"
OUTPUT_DIR="$SCRIPT_DIR/dist"
BUILD_CONTEXT="$SCRIPT_DIR/.build-context"
SK_BUILD_DIR="$BUILD_CONTEXT/spacekit"

if [[ ! -f "$GROWFORMER_DIR/Cargo.toml" ]]; then
    echo "❌ Missing neurokit/growformer (path dependency in Cargo.toml)."
    echo "   Expected: $GROWFORMER_DIR/Cargo.toml"
    echo "   Clone neurokit beside the spacekit repo, or set GROWFORMER_DIR=/path/to/growformer"
    exit 1
fi

echo "=== SpaceKit Compute Node Docker Build ==="
echo "Compute node source: $COMPUTE_NODE_DIR"
echo "Growformer source:   $GROWFORMER_DIR"
echo "Workspace crates:    spacekit-compute-node and its path dependencies"
echo "Output:              $OUTPUT_DIR"
echo ""

# Create output and build context directories
mkdir -p "$OUTPUT_DIR"
rm -rf "$BUILD_CONTEXT"
mkdir -p "$SK_BUILD_DIR" "$BUILD_CONTEXT/neurokit"

# Copy only workspace crates required by path dependencies in Cargo.toml.
# Layout mirrors the real repo: spacekit/* crates + neurokit/growformer so
# `path = "../../neurokit/growformer"` resolves inside the container.
echo "Preparing build context..."
rsync_workspace_crate() {
    local name="$1"
    local src="$2"
    shift 2
    rsync -a --exclude '.git' --exclude 'target' "$@" "$src/" "$SK_BUILD_DIR/$name/"
}

rsync_workspace_crate spacekit-diff "$DIFF_DIR"
rsync_workspace_crate spacekit-did "$SPACEKIT_DID_DIR"
rsync_workspace_crate spacekit-did-onchain "$SPACEKIT_DID_ONCHAIN_DIR"
rsync_workspace_crate spacekit-log "$LOG_DIR"
rsync_workspace_crate spacekit-primitives "$SPACEKIT_DIR"
rsync_workspace_crate spacekit-quantum-verkle "$SPACEKIT_QUANTUM_VERKLE_DIR"
rsync_workspace_crate spacekit-recovery "$RECOVERY_DIR"
rsync_workspace_crate spacekit-repo "$REPO_DIR"
rsync_workspace_crate spacekit-service-rewards "$SERVICE_REWARDS_DIR"
rsync_workspace_crate spacekit-spacetime-consensus "$SPACETIME_CONSENSUS_DIR"
rsync_workspace_crate spacekit-unified-consensus "$UNIFIED_CONSENSUS_DIR"
rsync_workspace_crate spacekit-payments "$PAYMENTS_DIR"
rsync_workspace_crate spacekit-messaging-node "$MESSAGING_NODE_DIR" \
    --exclude '.build-context*' \
    --exclude 'dist/' \
    --exclude 'data/' \
    --exclude '.env' \
    --exclude '.env.*'
rsync_workspace_crate spacekit-storage-node "$STORAGE_NODE_DIR" \
    --exclude '.build-context*' \
    --exclude 'dist/' \
    --exclude 'dist-gcp/' \
    --exclude 'storage_data/' \
    --exclude 'demo_storage/' \
    --exclude 'nft_demo_storage/' \
    --exclude 'fact_storage/' \
    --exclude 'omnicash_storage_data/' \
    --exclude 'logs/' \
    --exclude '.env' \
    --exclude '.env.*'
rsync_workspace_crate spacekit-compute-node "$COMPUTE_NODE_DIR" \
    --exclude '.build-context*' \
    --exclude 'dist/' \
    --exclude 'compute_storage/' \
    --exclude 'temp_blockchain_storage/' \
    --exclude 'secrets/' \
    --exclude '.env' \
    --exclude '.env.*'

# growformer is a path dep (../../neurokit/growformer). Copy only what cargo needs —
# not agent-data (8G+), target, MNIST, pathology, visualizer, etc.
stage_growformer() {
    local dest="$BUILD_CONTEXT/neurokit/growformer"
    mkdir -p "$dest/src"
    cp "$GROWFORMER_DIR/Cargo.toml" "$dest/"
    cp "$GROWFORMER_DIR/build.rs" "$dest/"
    rsync -a "$GROWFORMER_DIR/src/" "$dest/src/"
    # `include_str!` embeds in inference modules (runtime lib build)
    for sub in inference crypto fintech sentiment; do
        if [[ -d "$GROWFORMER_DIR/data/$sub" ]]; then
            mkdir -p "$dest/data/$sub"
            rsync -a "$GROWFORMER_DIR/data/$sub/" "$dest/data/$sub/"
        fi
    done
}
stage_growformer

# Verify Cargo.toml path deps still match the staged layout
if ! grep -q 'path = "../spacekit-primitives"' "$SK_BUILD_DIR/spacekit-compute-node/Cargo.toml"; then
    echo "⚠️  Warning: Cargo.toml path may need adjustment (spacekit-primitives)"
fi
if ! grep -q 'path = "../spacekit-did"' "$SK_BUILD_DIR/spacekit-compute-node/Cargo.toml"; then
    echo "⚠️  Warning: Cargo.toml path may need adjustment (spacekit-did)"
fi
if ! grep -q 'path = "../spacekit-did-onchain/bridges"' "$SK_BUILD_DIR/spacekit-compute-node/Cargo.toml"; then
    echo "⚠️  Warning: Cargo.toml path may need adjustment (spacekit-did-onchain/bridges)"
fi
if ! grep -q 'path = "../spacekit-quantum-verkle"' "$SK_BUILD_DIR/spacekit-compute-node/Cargo.toml"; then
    echo "⚠️  Warning: Cargo.toml path may need adjustment (spacekit-quantum-verkle)"
fi
if ! grep -q 'path = "../spacekit-storage-node"' "$SK_BUILD_DIR/spacekit-compute-node/Cargo.toml"; then
    echo "⚠️  Warning: Cargo.toml path may need adjustment (spacekit-storage-node)"
fi
if ! grep -q 'path = "../spacekit-messaging-node"' "$SK_BUILD_DIR/spacekit-compute-node/Cargo.toml"; then
    echo "⚠️  Warning: Cargo.toml path may need adjustment (spacekit-messaging-node)"
fi
if ! grep -q 'path = "../spacekit-payments"' "$SK_BUILD_DIR/spacekit-compute-node/Cargo.toml"; then
    echo "⚠️  Warning: Cargo.toml path may need adjustment (spacekit-payments)"
fi
if ! grep -q 'path = "../spacekit-service-rewards"' "$SK_BUILD_DIR/spacekit-compute-node/Cargo.toml"; then
    echo "⚠️  Warning: Cargo.toml path may need adjustment (spacekit-service-rewards)"
fi
if ! grep -q 'path = "../spacekit-log"' "$SK_BUILD_DIR/spacekit-compute-node/Cargo.toml"; then
    echo "⚠️  Warning: Cargo.toml path may need adjustment (spacekit-log)"
fi
if ! grep -q 'path = "../../neurokit/growformer"' "$SK_BUILD_DIR/spacekit-compute-node/Cargo.toml"; then
    echo "⚠️  Warning: Cargo.toml path may need adjustment (neurokit/growformer)"
fi
if [[ ! -f "$BUILD_CONTEXT/neurokit/growformer/Cargo.toml" ]]; then
    echo "❌ Staged growformer crate missing at $BUILD_CONTEXT/neurokit/growformer"
    exit 1
fi

echo "Build context ready"
ls -la "$BUILD_CONTEXT"
echo ""

# Create Dockerfile in build context
cat > "$BUILD_CONTEXT/Dockerfile" << 'EOF'
FROM ubuntu:22.04 AS builder

# Prevent interactive prompts
ENV DEBIAN_FRONTEND=noninteractive

# Install build dependencies and Rust
RUN apt-get update && apt-get install -y \
    curl \
    build-essential \
    pkg-config \
    libssl-dev \
    clang \
    llvm \
    libclang-dev \
    cmake \
    ninja-build \
    && rm -rf /var/lib/apt/lists/*

# Install Rust
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
ENV PATH="/root/.cargo/bin:${PATH}"

WORKDIR /build

# Copy staged layout: spacekit/* path deps + neurokit/growformer (../../ from compute-node)
COPY neurokit/growformer/ neurokit/growformer/
COPY spacekit/spacekit-diff/ spacekit/spacekit-diff/
COPY spacekit/spacekit-did-onchain/ spacekit/spacekit-did-onchain/
COPY spacekit/spacekit-did/ spacekit/spacekit-did/
COPY spacekit/spacekit-log/ spacekit/spacekit-log/
COPY spacekit/spacekit-messaging-node/ spacekit/spacekit-messaging-node/
COPY spacekit/spacekit-payments/ spacekit/spacekit-payments/
COPY spacekit/spacekit-primitives/ spacekit/spacekit-primitives/
COPY spacekit/spacekit-quantum-verkle/ spacekit/spacekit-quantum-verkle/
COPY spacekit/spacekit-recovery/ spacekit/spacekit-recovery/
COPY spacekit/spacekit-repo/ spacekit/spacekit-repo/
COPY spacekit/spacekit-service-rewards/ spacekit/spacekit-service-rewards/
COPY spacekit/spacekit-spacetime-consensus/ spacekit/spacekit-spacetime-consensus/
COPY spacekit/spacekit-storage-node/ spacekit/spacekit-storage-node/
COPY spacekit/spacekit-unified-consensus/ spacekit/spacekit-unified-consensus/
COPY spacekit/spacekit-compute-node/ spacekit/spacekit-compute-node/

# Build release binary with standalone feature set (avoid heavy defaults)
WORKDIR /build/spacekit/spacekit-compute-node
RUN cargo build --release --no-default-features --features "standalone"

# Verify binary was built
RUN ls -lh /build/spacekit/spacekit-compute-node/target/release/spacekit-compute-node || \
    (echo "Binary not found! Listing release directory:" && \
     ls -la /build/spacekit/spacekit-compute-node/target/release/ && \
     exit 1)

# Output stage - just the binary
FROM scratch AS export
COPY --from=builder /build/spacekit/spacekit-compute-node/target/release/spacekit-compute-node /
EOF

# Build using Docker
echo "Building with Docker..."
cd "$BUILD_CONTEXT"

docker buildx build \
    --platform linux/amd64 \
    --target export \
    --output type=local,dest="$OUTPUT_DIR" \
    -f Dockerfile \
    .

# Cleanup build context
rm -rf "$BUILD_CONTEXT"

# Verify the binary
if [ -f "$OUTPUT_DIR/spacekit-compute-node" ]; then
    echo ""
    echo "✅ Build successful!"
    ls -lh "$OUTPUT_DIR/spacekit-compute-node"
    file "$OUTPUT_DIR/spacekit-compute-node"
else
    echo "❌ Build failed - binary not found"
    exit 1
fi

echo ""
echo "Binary ready for deployment: $OUTPUT_DIR/spacekit-compute-node"
