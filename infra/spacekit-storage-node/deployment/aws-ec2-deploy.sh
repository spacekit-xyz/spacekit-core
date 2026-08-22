#!/bin/bash
# Deploy SpaceKit Storage Node to AWS EC2
#
# Same host as spacekit-simulator (e.g. 3.233.90.162) is OK:
#   - Simulator HTTP gateway: 8080 (proxy), 8000 (gateway storage routes), 9000, 17000, 50051 (gRPC).
#   - Orchestration embeds storage nodes with API default port 3030 (spacekit-simulator orchestration.rs).
#     A standalone storage node on the same VM must use a different HTTP port — default here is 3031.
#     Override: STORAGE_HTTP_PORT=3040 ./deployment/aws-ec2-deploy.sh ...
#   - Libp2p: simulator/storage often use 4001; this script defaults STORAGE_P2P_PORT=4002 (--p2p-port).
#     Standalone nodes often run HTTP-only: DISABLE_P2P=1 (default) adds --disable-p2p (no libp2p listeners).
#
# SSH: simulator docker/deploy.sh uses ubuntu@ by default. Amazon Linux AMIs use ec2-user@.
#   export SSH_USER=ec2-user   # if needed
#
# Required for most EC2 logins: pass your key explicitly (same as simulator deploy):
#   SSH_KEY=/path/to/key.pem SSH_USER=ubuntu ./deployment/aws-ec2-deploy.sh ...
#
# AWS Secrets Manager (build with aws-secrets — see build-docker-aws.sh):
#   • PQ **server** keypair for `/stream` + envelope decrypt:
#       QUANTUM_KEYPAIR_SECRET_NAME=…
#     SecretString JSON must match QuantumKeypair in aws_secrets.rs (public_key / private_key, base64 or hex).
#     When set, this takes priority over legacy data/server_keypair.json (IAM: secretsmanager:GetSecretValue).
#   • **Database** KEM (separate from server PQ keypair):
#       DATABASE_KEM_SECRET_NAME=…
#   On the instance (example — adjust secret names and DID):
#        sudo tee /etc/default/spacekit-storage-node <<'EOF'
#        AWS_REGION=us-east-1
#        QUANTUM_KEYPAIR_SECRET_NAME=prod/Testnet/Storage
#        SPACEKIT_NODE_DID=did:spacekit:storage:aws-node
#        DATABASE_KEM_SECRET_NAME=spacekit/prod/storage-node-database-keys
#        EOF
#        sudo chmod 600 /etc/default/spacekit-storage-node
#        sudo systemctl daemon-reload && sudo systemctl restart spacekit-storage-node
#   Without QUANTUM_KEYPAIR_SECRET_NAME the binary falls back to legacy paths; without any key
#   it may generate a NEW server keypair. Without SPACEKIT_NODE_DID that write may be PLAINTEXT JSON.
#   systemd unit from this script: EnvironmentFile=-/etc/default/spacekit-storage-node (leading - = optional).
#   Verify vars reached the process (as root): sudo tr '\\0' '\\n' < /proc/$(pgrep -f 'spacekit-storage-node start' | head -1)/environ | grep -E 'QUANTUM|AWS_REGION|SPACEKIT_NODE'
#
# errno 13 on startup: usually wrong filesystem ownership — data dir must be writable by User=spacekit:
#   sudo chown -R spacekit:spacekit /opt/spacekit-storage-node/data
#
# External CLI/uploads (e.g. spacekit storage deploy): open the API port on the instance security group.
#   Inbound rule: TCP on the storage HTTP port (default 3031) from your IP (or VPN). Without it, clients see connect timeouts.
#   The HTTP server binds 0.0.0.0 unless HOST= is set in the environment.
#
set -e

INSTANCE_IP="${1:-}"
BINARY_PATH="${2:-./dist/spacekit-storage-node}"
SSH_USER="${SSH_USER:-ubuntu}"
SSH_KEY="${SSH_KEY:-}"
# Default 3031: avoids clash with simulator orchestration embedded StorageNode API on 3030.
STORAGE_HTTP_PORT="${STORAGE_HTTP_PORT:-3031}"
# Avoid collision with spacekit-simulator / other nodes on 4001 (see journal "Address already in use")
STORAGE_P2P_PORT="${STORAGE_P2P_PORT:-4002}"
# 1 = HTTP API only (recommended when co-located or P2P not needed). Set DISABLE_P2P=0 to enable libp2p.
DISABLE_P2P="${DISABLE_P2P:-1}"

# SpaceKit repo root (…/spacekit); sibling layout …/spacekit/pems/*.pem is common.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SPACEKIT_REPO="$(cd "${SCRIPT_DIR}/../.." && pwd)"
STORAGE_NODE_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
if [[ -z "${SSH_KEY}" ]]; then
    _guess="${SPACEKIT_REPO}/pems/spacekit-testnet.pem"
    if [[ -f "${_guess}" ]]; then
        SSH_KEY="${_guess}"
        echo "[info] Using SSH_KEY=${SSH_KEY} (auto-detected). Set SSH_KEY= to override."
    fi
fi

SSH_OPTS=( -o StrictHostKeyChecking=accept-new -o ConnectTimeout=30 -o IdentitiesOnly=yes -o BatchMode=yes )

remote_scp() {
    # scp [opts] src dst
    if [[ -n "${SSH_KEY}" ]]; then
        scp -i "${SSH_KEY}" "${SSH_OPTS[@]}" "$@"
    else
        scp "${SSH_OPTS[@]}" "$@"
    fi
}

remote_ssh() {
    if [[ -n "${SSH_KEY}" ]]; then
        ssh -i "${SSH_KEY}" "${SSH_OPTS[@]}" "$@"
    else
        ssh "${SSH_OPTS[@]}" "$@"
    fi
}

# Preflight only (optional -v for debugging)
remote_ssh_test() {
    local v=()
    [[ "${DEPLOY_SSH_VERBOSE:-}" == "1" ]] && v=( -v )
    if [[ -n "${SSH_KEY}" ]]; then
        ssh "${v[@]}" -i "${SSH_KEY}" "${SSH_OPTS[@]}" "$@"
    else
        ssh "${v[@]}" "${SSH_OPTS[@]}" "$@"
    fi
}

if [ -z "$INSTANCE_IP" ]; then
    echo "Usage: $0 <EC2_INSTANCE_IP> [BINARY_PATH]"
    echo "Example:"
    echo "  SSH_KEY=~/keys/my.pem $0 3.233.90.162 ./dist/spacekit-storage-node"
    echo ""
    echo "Environment:"
    echo "  SSH_KEY (recommended)  Path to private key (.pem). Required unless ssh-agent has the right key."
    echo "  SSH_USER               SSH login (default: ubuntu; use ec2-user on Amazon Linux)"
    echo "  STORAGE_HTTP_PORT      API port (default: 3031; use another if 3031 is taken)"
    echo "  STORAGE_P2P_PORT       Libp2p TCP port (used when DISABLE_P2P=0; default: 4002)"
    echo "  DISABLE_P2P            1 = --disable-p2p (default); 0 = start libp2p on STORAGE_P2P_PORT"
    echo "  DOCUMENT_INLINE_MAX_BYTES  Inline JSON threshold before redb externalization (default: 4096)"
    echo "  BLOB_CACHE_MAX_ENTRIES     Hot blob cache size (default: 4096)"
    echo "  DEPLOY_SSH_VERBOSE=1   Print ssh -v on connection test"
    exit 1
fi

if [ ! -f "$BINARY_PATH" ]; then
    echo "❌ Binary not found: $BINARY_PATH"
    echo "   Run ./build-docker-aws.sh first to build the binary"
    exit 1
fi

if [[ -n "${SSH_KEY}" ]] && [[ ! -f "${SSH_KEY}" ]]; then
    echo "❌ SSH_KEY file not found: ${SSH_KEY}"
    exit 1
fi

if [[ -n "${SSH_KEY}" ]]; then
    # ssh refuses overly permissive keys
    _perm="$(stat -f '%Lp' "${SSH_KEY}" 2>/dev/null || stat -c '%a' "${SSH_KEY}" 2>/dev/null || echo '')"
    if [[ -n "${_perm}" && "${_perm}" != "400" && "${_perm}" != "600" ]]; then
        echo "[warn] Key permissions ${_perm} — if SSH fails, run: chmod 600 \"${SSH_KEY}\""
    fi
fi

echo "=== Deploying SpaceKit Storage Node to AWS EC2 ==="
echo "Instance IP: $INSTANCE_IP"
echo "SSH user:    $SSH_USER"
echo "SSH key:     ${SSH_KEY:-"(default identities / agent)"}"
echo "Binary:      $BINARY_PATH"
echo "HTTP port:   $STORAGE_HTTP_PORT"
echo "P2P port:    $STORAGE_P2P_PORT"
echo "Disable P2P: $DISABLE_P2P (1 = HTTP-only)"
echo ""

echo "🔑 Testing SSH..."
if ! err="$(remote_ssh_test "${SSH_USER}@${INSTANCE_IP}" "echo ok" 2>&1)"; then
    echo "❌ SSH failed: Permission denied (publickey) or unreachable."
    echo "${err}"
    echo ""
    echo "Fix: pass your PEM explicitly, same as for simulator deploy:"
    echo "  SSH_KEY=/path/to/spacekit-blockchain.pem SSH_USER=ubuntu $0 ${INSTANCE_IP} ${BINARY_PATH}"
    exit 1
fi
echo ""

# Create deployment directory structure
echo "📦 Preparing deployment package..."
TEMP_DIR=$(mktemp -d)
DEPLOY_DIR="$TEMP_DIR/spacekit-storage-node"

mkdir -p "$DEPLOY_DIR/bin"
mkdir -p "$DEPLOY_DIR/config"
mkdir -p "$DEPLOY_DIR/data"
mkdir -p "$DEPLOY_DIR/logs"
mkdir -p "$DEPLOY_DIR/scripts"

# Copy binary
cp "$BINARY_PATH" "$DEPLOY_DIR/bin/spacekit-storage-node"
chmod +x "$DEPLOY_DIR/bin/spacekit-storage-node"

# Ops scripts (orphan prune, catalog dedupe, soaks)
if [[ -d "${STORAGE_NODE_ROOT}/scripts" ]]; then
    cp "${STORAGE_NODE_ROOT}/scripts/"*.sh "$DEPLOY_DIR/scripts/" 2>/dev/null || true
    chmod +x "$DEPLOY_DIR/scripts/"*.sh 2>/dev/null || true
    echo "   Included $(find "$DEPLOY_DIR/scripts" -name '*.sh' | wc -l | tr -d ' ') script(s) in package"
fi

# systemd: standalone binary uses `start` subcommand (see src/bin/standalone.rs)
cat > "$DEPLOY_DIR/spacekit-storage-node.service" << EOF
[Unit]
Description=SpaceKit Storage Node
After=network.target

[Service]
Type=simple
User=spacekit
Group=spacekit
WorkingDirectory=/opt/spacekit-storage-node
EnvironmentFile=-/etc/default/spacekit-storage-node
ExecStart=/opt/spacekit-storage-node/bin/spacekit-storage-node start --data-dir /opt/spacekit-storage-node/data --port ${STORAGE_HTTP_PORT} --p2p-port ${STORAGE_P2P_PORT}$([[ "${DISABLE_P2P}" == "1" ]] && echo ' --disable-p2p') --max-storage-gb 100 --algorithm kyber1024
Restart=always
RestartSec=10
StandardOutput=journal
StandardError=journal

NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/opt/spacekit-storage-node/data /opt/spacekit-storage-node/logs

[Install]
WantedBy=multi-user.target
EOF

# Reference config (operator tuning); the standalone `start` path builds config from CLI flags above.
cat > "$DEPLOY_DIR/config/config.toml" << EOF
# Reference only — live settings match ExecStart in spacekit-storage-node.service

[node]
did = "did:spacekit:storage:aws-node"

[storage]
max_storage_bytes = 107374182400
data_dir = "/opt/spacekit-storage-node/data"

[api]
enabled = true
port = ${STORAGE_HTTP_PORT}
enable_cors = true

[network]
listen_port = ${STORAGE_P2P_PORT}

[quantum]
algorithm = "kyber1024"
cipher_suite = "AES256"

[security]
rate_limit_requests = 100
rate_limit_window_seconds = 60
EOF

# Template for /etc/default/spacekit-storage-node (systemd EnvironmentFile); copy on instance if missing.
cat > "$DEPLOY_DIR/config/spacekit-storage-node.default.env" << 'ENVEOF'
# Install: sudo cp /opt/spacekit-storage-node/config/spacekit-storage-node.default.env /etc/default/spacekit-storage-node
#          sudo chmod 600 /etc/default/spacekit-storage-node
#          sudo systemctl daemon-reload && sudo systemctl restart spacekit-storage-node
# Systemd reads this as root; the service user does not need read access to the file.

AWS_REGION=us-east-1
QUANTUM_KEYPAIR_SECRET_NAME=prod/Testnet/Storage
SPACEKIT_NODE_DID=did:spacekit:storage:aws-node
# DATABASE_KEM_SECRET_NAME=
ENVEOF

# Create deployment script
cat > "$DEPLOY_DIR/deploy.sh" << 'DEPLOY_SCRIPT'
#!/bin/bash
set -e

echo "🚀 Deploying SpaceKit Storage Node..."

if ! id "spacekit" &>/dev/null; then
    sudo useradd -r -s /bin/false -d /opt/spacekit-storage-node spacekit
fi

sudo mkdir -p /opt/spacekit-storage-node/{bin,config,data,logs,scripts}
sudo chown -R spacekit:spacekit /opt/spacekit-storage-node

# Overwriting the in-use binary fails with "Text file busy" while the service runs — stop first.
sudo systemctl stop spacekit-storage-node 2>/dev/null || true

sudo cp bin/spacekit-storage-node /opt/spacekit-storage-node/bin/
sudo cp config/config.toml /opt/spacekit-storage-node/config/
sudo cp config/spacekit-storage-node.default.env /opt/spacekit-storage-node/config/
sudo chmod +x /opt/spacekit-storage-node/bin/spacekit-storage-node
if [[ -d scripts ]]; then
    sudo cp scripts/*.sh /opt/spacekit-storage-node/scripts/ 2>/dev/null || true
    sudo chmod +x /opt/spacekit-storage-node/scripts/*.sh 2>/dev/null || true
fi

sudo cp spacekit-storage-node.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable spacekit-storage-node
sudo systemctl start spacekit-storage-node

if [[ ! -f /etc/default/spacekit-storage-node ]]; then
    echo ""
    echo "⚠️  /etc/default/spacekit-storage-node is missing — QUANTUM_KEYPAIR_SECRET_NAME and AWS_REGION are not loaded."
    echo "    The node will use legacy data/server_keypair.json if present (not Secrets Manager-only mode)."
    echo "    Example: sudo cp /opt/spacekit-storage-node/config/spacekit-storage-node.default.env /etc/default/spacekit-storage-node"
    echo "    Edit paths/DID, chmod 600, then: sudo systemctl restart spacekit-storage-node"
fi

echo "✅ Deployment complete!"
echo "   Status: sudo systemctl status spacekit-storage-node"
echo "   Logs: sudo journalctl -u spacekit-storage-node -f"
DEPLOY_SCRIPT

chmod +x "$DEPLOY_DIR/deploy.sh"

echo "📦 Creating deployment package..."
cd "$TEMP_DIR"
tar czf spacekit-storage-node-deploy.tar.gz spacekit-storage-node/

echo "📤 Uploading to ${SSH_USER}@${INSTANCE_IP} ..."
remote_scp spacekit-storage-node-deploy.tar.gz "${SSH_USER}@${INSTANCE_IP}:/tmp/"

echo "🔧 Installing on remote instance..."
remote_ssh "${SSH_USER}@${INSTANCE_IP}" << REMOTE_SCRIPT
    cd /tmp
    tar xzf spacekit-storage-node-deploy.tar.gz
    cd spacekit-storage-node
    sudo ./deploy.sh
    rm -rf /tmp/spacekit-storage-node*
REMOTE_SCRIPT

rm -rf "$TEMP_DIR"

echo ""
echo "✅ Deployment complete!"
echo ""
echo "📋 Same host as simulator? Keep clients straight:"
echo "   - Simulator unified API / gateway: ports 8080, 8000 (gateway storage routes), 9000, 17000"
echo "   - This storage node HTTP API: port ${STORAGE_HTTP_PORT}"
echo ""
echo "📋 Next steps:"
echo "   If you use AWS Secrets Manager for the PQ keypair, ensure /etc/default/spacekit-storage-node exists on the instance"
echo "   (see config/spacekit-storage-node.default.env in the tarball) and the unit has EnvironmentFile=-/etc/default/spacekit-storage-node"
echo "   sudo ufw allow ${STORAGE_HTTP_PORT}/tcp   # if using UFW"
echo "   AWS: security group inbound TCP ${STORAGE_HTTP_PORT} (HTTP) and ${STORAGE_P2P_PORT} (libp2p) if peers reach this host"
echo "   curl http://${INSTANCE_IP}:${STORAGE_HTTP_PORT}/health"
_keyhint=""
[[ -n "${SSH_KEY}" ]] && _keyhint=" -i ${SSH_KEY}"
echo "   ssh${_keyhint} ${SSH_USER}@${INSTANCE_IP} 'sudo systemctl status spacekit-storage-node'"
echo ""
echo "   Ops scripts (on instance after deploy):"
echo "     cd /opt/spacekit-storage-node"
echo "     STORAGE_NODE=http://127.0.0.1:${STORAGE_HTTP_PORT} OWNER_DID=did:spacekit:user:you ./scripts/prune-orphan-file-blobs.sh"
