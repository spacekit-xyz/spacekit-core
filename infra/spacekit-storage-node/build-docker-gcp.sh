#!/bin/bash
# Build SpaceKit Storage Node binary using Docker
# This creates a Linux x86_64 binary compatible with Google Cloud Platform (GCP)
# GCP typically uses Debian-based images, but Ubuntu 22.04 is compatible

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
# PROJECT_ROOT is the parent directory (spacekit), where path deps are siblings
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
STORAGE_NODE_DIR="$SCRIPT_DIR"
SPACEKIT_DIR="$PROJECT_ROOT/spacekit-primitives"
REPO_DIR="$PROJECT_ROOT/spacekit-repo"
DIFF_DIR="$PROJECT_ROOT/spacekit-diff"
OUTPUT_DIR="$SCRIPT_DIR/dist-gcp"
BUILD_CONTEXT="$SCRIPT_DIR/.build-context-gcp"

echo "=== SpaceKit Storage Node Docker Build (GCP) ==="
echo "Storage node source: $STORAGE_NODE_DIR"
echo "Workspace crates:    spacekit-diff, spacekit-primitives, spacekit-repo, spacekit-storage-node"
echo "Output:              $OUTPUT_DIR"
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

rsync_workspace_crate spacekit-diff "$DIFF_DIR"
rsync_workspace_crate spacekit-primitives "$SPACEKIT_DIR"
rsync_workspace_crate spacekit-repo "$REPO_DIR"
# Exclude runtime data and local build outputs (see .gitignore)
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

# Verify Cargo.toml path deps still match the staged layout
if ! grep -q 'path = "../spacekit-primitives"' "$BUILD_CONTEXT/spacekit-storage-node/Cargo.toml"; then
    echo "⚠️  Warning: Cargo.toml path may need adjustment (spacekit-primitives)"
    echo "   Expected: path = \"../spacekit-primitives\""
fi
if ! grep -q 'path = "../spacekit-repo"' "$BUILD_CONTEXT/spacekit-storage-node/Cargo.toml"; then
    echo "⚠️  Warning: Cargo.toml path may need adjustment (spacekit-repo)"
    echo "   Expected: path = \"../spacekit-repo\""
fi

echo "Build context ready"
ls -la "$BUILD_CONTEXT"
echo ""

# Create Dockerfile in build context (optimized for GCP)
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
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Install Rust with stable toolchain
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
ENV PATH="/root/.cargo/bin:${PATH}"

WORKDIR /build

# Copy only the workspace crates staged by build-docker-gcp.sh (not the full monorepo)
COPY spacekit-diff/ spacekit-diff/
COPY spacekit-primitives/ spacekit-primitives/
COPY spacekit-repo/ spacekit-repo/
COPY spacekit-storage-node/ spacekit-storage-node/

# Build release binary with all features for production
WORKDIR /build/spacekit-storage-node
RUN cargo build --release --features "api-server,database,p2p,quantum,standalone"

# Verify binary was built
RUN ls -lh /build/spacekit-storage-node/target/release/spacekit-storage-node || \
    (echo "Binary not found! Listing release directory:" && \
     ls -la /build/spacekit-storage-node/target/release/ && \
     exit 1)

# Strip binary to reduce size
RUN strip /build/spacekit-storage-node/target/release/spacekit-storage-node

# Output stage - just the binary
FROM scratch AS export
COPY --from=builder /build/spacekit-storage-node/target/release/spacekit-storage-node /
EOF

# Build using Docker
echo "Building with Docker for GCP..."
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
if [ -f "$OUTPUT_DIR/spacekit-storage-node" ]; then
    echo ""
    echo "✅ Build successful!"
    ls -lh "$OUTPUT_DIR/spacekit-storage-node"
    file "$OUTPUT_DIR/spacekit-storage-node"
    
    # Check binary dependencies (should be minimal for GCP)
    echo ""
    echo "📦 Binary dependencies:"
    ldd "$OUTPUT_DIR/spacekit-storage-node" 2>/dev/null || echo "Static binary (no dependencies)"
else
    echo "❌ Build failed - binary not found"
    exit 1
fi

echo ""
echo "Binary ready for GCP deployment: $OUTPUT_DIR/spacekit-storage-node"
echo ""
echo "🚀 GCP Deployment Instructions:"
echo "1. Upload binary to GCP Storage:"
echo "   gsutil cp $OUTPUT_DIR/spacekit-storage-node gs://your-bucket/"
echo ""
echo "2. Deploy to Compute Engine:"
echo "   gcloud compute instances create storage-node \\"
echo "     --image-family=ubuntu-2204-lts \\"
echo "     --image-project=ubuntu-os-cloud \\"
echo "     --machine-type=e2-medium \\"
echo "     --zone=us-central1-a"
echo ""
echo "3. Or deploy to Cloud Run (containerized):"
echo "   See build-docker-gcp-container.sh for container image build"
