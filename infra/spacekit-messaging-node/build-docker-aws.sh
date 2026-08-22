#!/bin/bash
# Build SpaceKit Messaging Node binaries using Docker
# This creates Linux x86_64 binaries compatible with Ubuntu 22.04 (AWS EC2)

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
# PROJECT_ROOT is the parent directory (spacekit), where path deps are siblings
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
MESSAGING_NODE_DIR="$SCRIPT_DIR"
SPACEKIT_DIR="$PROJECT_ROOT/spacekit-primitives"
RECOVERY_DIR="$PROJECT_ROOT/spacekit-recovery"
OUTPUT_DIR="$SCRIPT_DIR/dist"
BUILD_CONTEXT="$SCRIPT_DIR/.build-context"

echo "=== SpaceKit Messaging Node Docker Build ==="
echo "Messaging node source: $MESSAGING_NODE_DIR"
echo "Workspace crates:        spacekit-messaging-node, spacekit-primitives, spacekit-recovery"
echo "Output:                  $OUTPUT_DIR"
echo ""

# Create output and build context directories
mkdir -p "$OUTPUT_DIR"
rm -rf "$BUILD_CONTEXT"
mkdir -p "$BUILD_CONTEXT"

# Copy only workspace crates required by path dependencies in Cargo.toml
echo "Preparing build context..."
rsync_workspace_crate() {
    local name="$1"
    local src="$2"
    shift 2
    rsync -a --exclude '.git' --exclude 'target' "$@" "$src/" "$BUILD_CONTEXT/$name/"
}

rsync_workspace_crate spacekit-primitives "$SPACEKIT_DIR"
rsync_workspace_crate spacekit-recovery "$RECOVERY_DIR"
rsync_workspace_crate spacekit-messaging-node "$MESSAGING_NODE_DIR" \
    --exclude '.build-context*' \
    --exclude 'dist/' \
    --exclude 'data/' \
    --exclude '.env' \
    --exclude '.env.*'

# Verify Cargo.toml path deps still match the staged layout
if ! grep -q 'path = "../spacekit-primitives"' "$BUILD_CONTEXT/spacekit-messaging-node/Cargo.toml"; then
    echo "⚠️  Warning: Cargo.toml path may need adjustment (spacekit-primitives)"
    echo "   Expected: path = \"../spacekit-primitives\""
fi
if ! grep -q 'path = "../spacekit-recovery"' "$BUILD_CONTEXT/spacekit-messaging-node/Cargo.toml"; then
    echo "⚠️  Warning: Cargo.toml path may need adjustment (spacekit-recovery)"
    echo "   Expected: path = \"../spacekit-recovery\""
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

# Copy only the workspace crates staged by build-docker-aws.sh (not the full monorepo)
COPY spacekit-messaging-node/ spacekit-messaging-node/
COPY spacekit-primitives/ spacekit-primitives/
COPY spacekit-recovery/ spacekit-recovery/

# Build release binaries with standalone features
WORKDIR /build/spacekit-messaging-node
RUN cargo build --release --features "standalone"

# Verify binaries were built
RUN ls -lh /build/spacekit-messaging-node/target/release/spacekit-messaging-node || \
    (echo "Binary not found! Listing release directory:" && \
     ls -la /build/spacekit-messaging-node/target/release/ && \
     exit 1)
RUN ls -lh /build/spacekit-messaging-node/target/release/spacekit-messaging-http || \
    (echo "HTTP gateway binary not found! Listing release directory:" && \
     ls -la /build/spacekit-messaging-node/target/release/ && \
     exit 1)

# Output stage - just the binaries
FROM scratch AS export
COPY --from=builder /build/spacekit-messaging-node/target/release/spacekit-messaging-node /
COPY --from=builder /build/spacekit-messaging-node/target/release/spacekit-messaging-http /
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

# Verify the binaries
if [ -f "$OUTPUT_DIR/spacekit-messaging-node" ] && [ -f "$OUTPUT_DIR/spacekit-messaging-http" ]; then
    echo ""
    echo "✅ Build successful!"
    ls -lh "$OUTPUT_DIR/spacekit-messaging-node" "$OUTPUT_DIR/spacekit-messaging-http"
    file "$OUTPUT_DIR/spacekit-messaging-node"
    file "$OUTPUT_DIR/spacekit-messaging-http"
else
    echo "❌ Build failed - binaries not found"
    exit 1
fi

echo ""
echo "Binaries ready for deployment: $OUTPUT_DIR/spacekit-messaging-node, $OUTPUT_DIR/spacekit-messaging-http"
