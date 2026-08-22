#!/bin/bash
# Build SpaceKit Command Line Interface for Linux amd64
# Run from macOS to build Linux compatible binaries

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Parent repo root: path deps (e.g. ../spacekit-storage-node) are siblings of this crate.
SPACEKIT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
CLI_CRATE="$(basename "${SCRIPT_DIR}")"
PROJECT_ROOT="${SCRIPT_DIR}"
OUTPUT_DIR="${SCRIPT_DIR}/build"
BINARY_NAME="spacekit"
BUILD_CONTEXT="${PROJECT_ROOT}/.build-context"
SK_BUILD_DIR="${BUILD_CONTEXT}/spacekit"
# Cargo.toml uses ../../neurokit/growformer (sibling of the spacekit repo).
MONOREPO_PARENT="$(dirname "${SPACEKIT_ROOT}")"
GROWFORMER_PATH="${GROWFORMER_DIR:-${MONOREPO_PARENT}/neurokit/growformer}"
GROWFORMER_PATH="$(cd "${GROWFORMER_PATH}" 2>/dev/null && pwd || true)"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

log_info() { echo -e "${BLUE}[INFO]${NC} $1"; }
log_success() { echo -e "${GREEN}[SUCCESS]${NC} $1"; }
log_warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
log_error() { echo -e "${RED}[ERROR]${NC} $1"; }

check_path_dependencies() {
    local missing=0
    for crate in \
        spacekit-did \
        spacekit-primitives \
        spacekit-compute-node \
        spacekit-storage-node \
        spacekit-messaging-node \
        spacekit-repo; do
        if [[ ! -f "${SPACEKIT_ROOT}/${crate}/Cargo.toml" ]]; then
            log_error "Missing path dependency: ${SPACEKIT_ROOT}/${crate}/Cargo.toml"
            missing=1
        fi
    done
    if [[ -z "${GROWFORMER_PATH}" || ! -f "${GROWFORMER_PATH}/Cargo.toml" ]]; then
        log_error "Missing growformer dependency: ${MONOREPO_PARENT}/neurokit/growformer/Cargo.toml"
        log_error "Clone neurokit next to spacekit, or set GROWFORMER_DIR=/path/to/growformer"
        missing=1
    fi
    if [[ "${missing}" -ne 0 ]]; then
        exit 1
    fi
}

check_requirements() {
    log_info "Checking build requirements..."

    if ! command -v rsync &>/dev/null; then
        log_error "rsync not found (required to stage minimal build context)"
        exit 1
    fi

    if ! command -v cargo &>/dev/null; then
        log_error "Rust/Cargo not found. Install from https://rustup.rs"
        exit 1
    fi

    check_path_dependencies

    log_success "Build requirements satisfied"
}

# Copy only what Cargo needs to compile — not runtime data, scratch, or target trees.
stage_crate_minimal() {
    local name="$1"
    local src="$2"
    local dest="${SK_BUILD_DIR}/${name}"
    mkdir -p "${dest}"

    cp "${src}/Cargo.toml" "${dest}/"
    if [[ -f "${src}/Cargo.lock" ]]; then
        cp "${src}/Cargo.lock" "${dest}/"
    fi
    if [[ -f "${src}/build.rs" ]]; then
        cp "${src}/build.rs" "${dest}/"
    fi
    if [[ -d "${src}/src" ]]; then
        mkdir -p "${dest}/src"
        rsync -a "${src}/src/" "${dest}/src/"
    fi
    # spacekit-diff keeps lib sources at crate root.
    for file in lib.rs blob.rs tree.rs types.rs integration.rs; do
        if [[ -f "${src}/${file}" ]]; then
            cp "${src}/${file}" "${dest}/"
        fi
    done
    # spacekit-primitives path deps (pqcrypto-*).
    if [[ -d "${src}/vendor" ]]; then
        mkdir -p "${dest}/vendor"
        rsync -a "${src}/vendor/" "${dest}/vendor/"
    fi
}

stage_did_onchain() {
    local src="${SPACEKIT_ROOT}/spacekit-did-onchain"
    local dest="${SK_BUILD_DIR}/spacekit-did-onchain"
    mkdir -p "${dest}/bridges/src"
    cp "${src}/Cargo.toml" "${dest}/"
    cp "${src}/bridges/Cargo.toml" "${dest}/bridges/"
    rsync -a "${src}/bridges/src/" "${dest}/bridges/src/"
}

stage_growformer() {
    local dest="${BUILD_CONTEXT}/neurokit/growformer"
    mkdir -p "${dest}/src"
    cp "${GROWFORMER_PATH}/Cargo.toml" "${dest}/"
    cp "${GROWFORMER_PATH}/build.rs" "${dest}/"
    rsync -a "${GROWFORMER_PATH}/src/" "${dest}/src/"
    # spacekit-cli embeds inference TOML at compile time; growformer tests embed more.
    for sub in inference crypto fintech sentiment; do
        if [[ -d "${GROWFORMER_PATH}/data/${sub}" ]]; then
            mkdir -p "${dest}/data/${sub}"
            rsync -a "${GROWFORMER_PATH}/data/${sub}/" "${dest}/data/${sub}/"
        fi
    done
}

prepare_build_context() {
    log_info "Staging minimal build context (src + Cargo.toml only, no runtime data)..."

    rm -rf "${BUILD_CONTEXT}"
    mkdir -p "${SK_BUILD_DIR}" "${BUILD_CONTEXT}/neurokit"

    stage_crate_minimal spacekit-cli "${PROJECT_ROOT}"
    stage_crate_minimal spacekit-did "${SPACEKIT_ROOT}/spacekit-did"
    stage_crate_minimal spacekit-primitives "${SPACEKIT_ROOT}/spacekit-primitives"
    stage_crate_minimal spacekit-compute-node "${SPACEKIT_ROOT}/spacekit-compute-node"
    stage_crate_minimal spacekit-storage-node "${SPACEKIT_ROOT}/spacekit-storage-node"
    stage_crate_minimal spacekit-messaging-node "${SPACEKIT_ROOT}/spacekit-messaging-node"
    stage_crate_minimal spacekit-repo "${SPACEKIT_ROOT}/spacekit-repo"
    stage_crate_minimal spacekit-diff "${SPACEKIT_ROOT}/spacekit-diff"
    stage_crate_minimal spacekit-quantum-verkle "${SPACEKIT_ROOT}/spacekit-quantum-verkle"
    stage_crate_minimal spacekit-payments "${SPACEKIT_ROOT}/spacekit-payments"
    stage_crate_minimal spacekit-service-rewards "${SPACEKIT_ROOT}/spacekit-service-rewards"
    stage_crate_minimal spacekit-log "${SPACEKIT_ROOT}/spacekit-log"
    stage_crate_minimal spacekit-spacetime-consensus "${SPACEKIT_ROOT}/spacekit-spacetime-consensus"
    stage_crate_minimal spacekit-unified-consensus "${SPACEKIT_ROOT}/spacekit-unified-consensus"
    stage_did_onchain
    stage_growformer

    if [[ ! -f "${BUILD_CONTEXT}/neurokit/growformer/Cargo.toml" ]]; then
        log_error "Staged growformer crate missing at ${BUILD_CONTEXT}/neurokit/growformer"
        exit 1
    fi

    log_success "Build context ready ($(du -sh "${BUILD_CONTEXT}" | awk '{print $1}'))"
}

cleanup_build_context() {
    rm -rf "${BUILD_CONTEXT}"
}

docker_volume_args() {
    # Staged layout mirrors host path deps from /build/spacekit/spacekit-cli:
    #   ../spacekit-*              -> /build/spacekit/spacekit-*
    #   ../../neurokit/growformer  -> /build/neurokit/growformer
    echo \
        -v "${BUILD_CONTEXT}:/build" \
        -v "${PROJECT_ROOT}/target:/build/spacekit/spacekit-cli/target"
}

build_with_cross() {
    log_info "Building with 'cross' (Docker-based)..."

    if ! command -v cross &>/dev/null; then
        log_info "Installing cross..."
        cargo install cross
    fi

    prepare_build_context

    local staged_cli="${SK_BUILD_DIR}/spacekit-cli"
    local cross_config="${PROJECT_ROOT}/.cross-linux-build.toml"
    cat > "${cross_config}" << EOF
[target.x86_64-unknown-linux-gnu]
volumes = [
    "${BUILD_CONTEXT}:${BUILD_CONTEXT}:ro",
    "${PROJECT_ROOT}/target:${PROJECT_ROOT}/target"
]
EOF

    log_info "Building ${BINARY_NAME} for Linux amd64..."
    cd "${staged_cli}"
    CARGO_TARGET_DIR="${PROJECT_ROOT}/target" \
        CROSS_CONFIG="${cross_config}" \
        cross build --release --target x86_64-unknown-linux-gnu
    rm -f "${cross_config}"
    cleanup_build_context

    mkdir -p "${OUTPUT_DIR}"
    cp "${PROJECT_ROOT}/target/x86_64-unknown-linux-gnu/release/spacekit" "${OUTPUT_DIR}/${BINARY_NAME}"
    log_success "Binary copied to ${OUTPUT_DIR}/${BINARY_NAME}"
}

build_with_docker() {
    log_info "Building with Docker (linux/amd64 platform)..."

    if ! command -v docker &>/dev/null; then
        log_error "Docker not found. Install Docker Desktop."
        exit 1
    fi

    if ! docker info &>/dev/null; then
        log_error "Docker is not running. Please start Docker."
        exit 1
    fi

    mkdir -p "${OUTPUT_DIR}" "${PROJECT_ROOT}/target"
    prepare_build_context

    log_info "Building ${BINARY_NAME} for Linux amd64..."
    log_info "Uses staged sources only; first build may take 15-30 minutes on Apple Silicon..."

    # shellcheck disable=SC2046
    docker run --rm \
        --platform linux/amd64 \
        $(docker_volume_args) \
        -w "/build/spacekit/spacekit-cli" \
        rust:1.91-slim-bookworm \
        bash -c "
            set -e
            echo '[INFO] Installing build dependencies...'
            apt-get update -qq
            apt-get install -y -qq \
                pkg-config libssl-dev build-essential \
                clang llvm libclang-dev cmake ninja-build git

            echo '[INFO] Building ${BINARY_NAME}...'
            export CARGO_TARGET_DIR=/build/spacekit/spacekit-cli/target
            cargo build --release

            echo '[SUCCESS] Build complete!'
        "

    cleanup_build_context
    copy_binary
}

build_local() {
    log_info "Building for local platform (development)..."

    cd "${PROJECT_ROOT}"
    cargo build --release

    copy_binary
}

copy_binary() {
    log_info "Copying binary to output directory..."
    mkdir -p "${OUTPUT_DIR}"

    local bin="${PROJECT_ROOT}/target/release/spacekit"

    if [ -f "${bin}" ]; then
        cp "${bin}" "${OUTPUT_DIR}/${BINARY_NAME}"
        chmod +x "${OUTPUT_DIR}/${BINARY_NAME}"
        log_success "Copied ${BINARY_NAME}"
    else
        log_error "Binary not found at ${bin}"
        exit 1
    fi

    log_info "Build output:"
    ls -la "${OUTPUT_DIR}/"

    log_info "Binary size:"
    du -h "${OUTPUT_DIR}/${BINARY_NAME}"
}

create_deploy_script() {
    log_info "Creating deploy script..."

    cat > "${OUTPUT_DIR}/deploy.sh" << 'DEPLOY_EOF'
#!/bin/bash
# Deploy SpaceKit Command Line Interface to server
set -e

if [ -z "$1" ]; then
    echo "Usage: $0 <server-ip> [ssh-key] [user]"
    echo "  server-ip: IP address of the server"
    echo "  ssh-key:   Path to SSH private key (optional)"
    echo "  user:      SSH user (default: ubuntu)"
    exit 1
fi

SERVER="$1"
SSH_KEY="${2:-}"
SSH_USER="${3:-ubuntu}"
SSH_OPTS=""

if [ -n "${SSH_KEY}" ]; then
    SSH_OPTS="-i ${SSH_KEY}"
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

echo "[INFO] Creating spacekit user on server..."
ssh ${SSH_OPTS} ${SSH_USER}@${SERVER} "id -u spacekit &>/dev/null || sudo useradd -r -s /bin/false spacekit"

echo "[INFO] Creating directory structure..."
ssh ${SSH_OPTS} ${SSH_USER}@${SERVER} "sudo mkdir -p /opt/spacekit && sudo chown spacekit:spacekit /opt/spacekit"

echo "[INFO] Copying binary..."
scp ${SSH_OPTS} "${SCRIPT_DIR}/spacekit" ${SSH_USER}@${SERVER}:/tmp/

echo "[INFO] Moving binary to /opt/spacekit..."
ssh ${SSH_OPTS} ${SSH_USER}@${SERVER} "sudo mv /tmp/spacekit /opt/spacekit/ && sudo chmod +x /opt/spacekit/spacekit && sudo chown spacekit:spacekit /opt/spacekit/spacekit"

echo "[INFO] Copying systemd service..."
scp ${SSH_OPTS} "${SCRIPT_DIR}/spacekit.service" ${SSH_USER}@${SERVER}:/tmp/

echo "[INFO] Installing systemd service..."
ssh ${SSH_OPTS} ${SSH_USER}@${SERVER} "sudo mv /tmp/spacekit.service /etc/systemd/system/ && sudo systemctl daemon-reload"

echo ""
echo "[SUCCESS] Deployment complete!"
echo ""
echo "Next steps on the server:"
echo "  1. Create /opt/spacekit/.env with your configuration:"
echo "     sudo nano /opt/spacekit/.env"
echo "  2. sudo systemctl enable spacekit"
echo "  3. sudo systemctl start spacekit-website-api"
echo "  4. sudo systemctl status spacekit"
echo "  5. sudo journalctl -u spacekit -f"
DEPLOY_EOF

    chmod +x "${OUTPUT_DIR}/deploy.sh"
    log_success "Created deploy script"
}

main() {
    echo "=========================================="
    echo "  SpaceKit CLI Build Script"
    echo "=========================================="
    echo ""

    check_requirements

    case "${1:-docker}" in
        docker)
            build_with_docker
            ;;
        cross)
            build_with_cross
            ;;
        local)
            build_local
            ;;
        *)
            echo "Usage: $0 {docker|cross|local}"
            echo ""
            echo "  docker - Use Docker build (recommended, produces linux/amd64)"
            echo "  cross  - Use 'cross' tool for cross-compilation"
            echo "  local  - Build for current platform (for local testing)"
            exit 1
            ;;
    esac

    # create_deploy_script

    echo ""
    log_success "Build complete!"
    echo ""
    echo "Output directory: ${OUTPUT_DIR}"
    echo ""
    echo "Files created:"
    ls -la "${OUTPUT_DIR}/"
    echo ""
    echo "Next steps:"
    echo "  1. Deploy to server:  ./build/deploy.sh <server-ip>"
    echo "  2. Configure move spacekit binary to /usr/local/bin or add to your PATH, make it executable"
    echo "  3. Test the binary:     spacekit --help"
}

main "$@"
