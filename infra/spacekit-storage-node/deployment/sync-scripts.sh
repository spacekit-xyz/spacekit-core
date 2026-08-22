#!/bin/bash
# Copy ops scripts to an already-deployed storage node (no binary rebuild).
#
# Usage:
#   SSH_KEY=~/pems/spacekit-testnet.pem ./deployment/sync-scripts.sh <server-ip>
#
set -euo pipefail

INSTANCE_IP="${1:-}"
SSH_USER="${SSH_USER:-ubuntu}"
SSH_KEY="${SSH_KEY:-}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
STORAGE_NODE_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

if [[ -z "${INSTANCE_IP}" ]]; then
    echo "Usage: $0 <server-ip>"
    echo "  SSH_KEY=~/path/to.pem SSH_USER=ubuntu $0 3.233.90.162"
    exit 1
fi

if [[ ! -d "${STORAGE_NODE_ROOT}/scripts" ]]; then
    echo "❌ No scripts/ directory at ${STORAGE_NODE_ROOT}/scripts"
    exit 1
fi

SSH_OPTS=( -o StrictHostKeyChecking=accept-new -o ConnectTimeout=30 )
SCP=( scp "${SSH_OPTS[@]}" )
SSH=( ssh "${SSH_OPTS[@]}" )
if [[ -n "${SSH_KEY}" ]]; then
    SCP=( scp -i "${SSH_KEY}" "${SSH_OPTS[@]}" )
    SSH=( ssh -i "${SSH_KEY}" "${SSH_OPTS[@]}" )
fi

TMP="/tmp/spacekit-storage-scripts-$$"
mkdir -p "${TMP}"
cp "${STORAGE_NODE_ROOT}/scripts/"*.sh "${TMP}/"
chmod +x "${TMP}/"*.sh

echo "📤 Uploading scripts to ${SSH_USER}@${INSTANCE_IP} ..."
"${SCP[@]}" "${TMP}/"*.sh "${SSH_USER}@${INSTANCE_IP}:/tmp/"

"${SSH[@]}" "${SSH_USER}@${INSTANCE_IP}" bash -s <<'REMOTE'
set -e
sudo mkdir -p /opt/spacekit-storage-node/scripts
sudo mv /tmp/*.sh /opt/spacekit-storage-node/scripts/ 2>/dev/null || true
sudo chmod +x /opt/spacekit-storage-node/scripts/*.sh
ls -la /opt/spacekit-storage-node/scripts/
REMOTE

rm -rf "${TMP}"
echo ""
echo "✅ Scripts installed under /opt/spacekit-storage-node/scripts/"
echo ""
echo "Example:"
echo "  cd /opt/spacekit-storage-node"
echo "  STORAGE_NODE=http://127.0.0.1:3031 OWNER_DID=did:spacekit:user:astor ./scripts/prune-orphan-file-blobs.sh"
