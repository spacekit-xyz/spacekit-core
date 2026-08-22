#!/bin/bash
# Deploy SpaceKit Storage Node to Google Cloud Platform Compute Engine
# This script sets up the storage node on a GCP VM instance

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
STORAGE_NODE_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

INSTANCE_NAME="${1:-spacekit-storage-node}"
ZONE="${2:-us-central1-a}"
BINARY_PATH="${3:-./dist-gcp/spacekit-storage-node}"
PROJECT_ID="${4:-}"

if [ -z "$PROJECT_ID" ]; then
    echo "Usage: $0 <INSTANCE_NAME> [ZONE] [BINARY_PATH] <PROJECT_ID>"
    echo "Example: $0 spacekit-storage-node us-central1-a ./dist-gcp/spacekit-storage-node my-project-id"
    exit 1
fi

if [ ! -f "$BINARY_PATH" ]; then
    echo "❌ Binary not found: $BINARY_PATH"
    echo "   Run ./build-docker-gcp.sh first to build the binary"
    exit 1
fi

echo "=== Deploying SpaceKit Storage Node to GCP Compute Engine ==="
echo "Instance: $INSTANCE_NAME"
echo "Zone: $ZONE"
echo "Project: $PROJECT_ID"
echo "Binary: $BINARY_PATH"
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

if [[ -d "${STORAGE_NODE_ROOT}/scripts" ]]; then
    cp "${STORAGE_NODE_ROOT}/scripts/"*.sh "$DEPLOY_DIR/scripts/" 2>/dev/null || true
    chmod +x "$DEPLOY_DIR/scripts/"*.sh 2>/dev/null || true
fi

# Create systemd service file
cat > "$DEPLOY_DIR/spacekit-storage-node.service" << 'EOF'
[Unit]
Description=SpaceKit Storage Node
After=network.target

[Service]
Type=simple
User=spacekit
Group=spacekit
WorkingDirectory=/opt/spacekit-storage-node
ExecStart=/opt/spacekit-storage-node/bin/spacekit-storage-node \
    --config /opt/spacekit-storage-node/config/config.toml \
    --data-dir /opt/spacekit-storage-node/data \
    --log-dir /opt/spacekit-storage-node/logs
Restart=always
RestartSec=10
StandardOutput=journal
StandardError=journal

# Security hardening
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/opt/spacekit-storage-node/data /opt/spacekit-storage-node/logs

[Install]
WantedBy=multi-user.target
EOF

# Create default config
cat > "$DEPLOY_DIR/config/config.toml" << 'EOF'
# SpaceKit Storage Node Configuration

[node]
did = "did:spacekit:storage:gcp-node"
listen_port = 3030

[storage]
max_storage_bytes = 107374182400  # 100GB
data_dir = "/opt/spacekit-storage-node/data"

[api]
enabled = true
port = 3030
enable_cors = true

[network]
listen_port = 4001
max_connections = 100
replication_factor = 3
chunk_size = 1048576  # 1MB

[quantum]
algorithm = "kyber1024"
cipher_suite = "AES256"

[security]
rate_limit_requests = 100
rate_limit_window_seconds = 60
EOF

# Create deployment script
cat > "$DEPLOY_DIR/deploy.sh" << 'DEPLOY_SCRIPT'
#!/bin/bash
set -e

echo "🚀 Deploying SpaceKit Storage Node..."

# Create user if doesn't exist
if ! id "spacekit" &>/dev/null; then
    sudo useradd -r -s /bin/false -d /opt/spacekit-storage-node spacekit
fi

# Create directories
sudo mkdir -p /opt/spacekit-storage-node/{bin,config,data,logs,scripts}
sudo chown -R spacekit:spacekit /opt/spacekit-storage-node

# Copy files
sudo cp bin/spacekit-storage-node /opt/spacekit-storage-node/bin/
sudo cp config/config.toml /opt/spacekit-storage-node/config/
sudo chmod +x /opt/spacekit-storage-node/bin/spacekit-storage-node
if [[ -d scripts ]]; then
    sudo cp scripts/*.sh /opt/spacekit-storage-node/scripts/ 2>/dev/null || true
    sudo chmod +x /opt/spacekit-storage-node/scripts/*.sh 2>/dev/null || true
fi

# Install systemd service
sudo cp spacekit-storage-node.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable spacekit-storage-node
sudo systemctl start spacekit-storage-node

echo "✅ Deployment complete!"
echo "   Status: sudo systemctl status spacekit-storage-node"
echo "   Logs: sudo journalctl -u spacekit-storage-node -f"
DEPLOY_SCRIPT

chmod +x "$DEPLOY_DIR/deploy.sh"

# Create tarball
echo "📦 Creating deployment package..."
cd "$TEMP_DIR"
tar czf spacekit-storage-node-deploy.tar.gz spacekit-storage-node/

# Upload to GCS bucket (temporary)
BUCKET_NAME="spacekit-storage-node-deploy-$(date +%s)"
echo "📤 Uploading to GCS..."
gsutil mb -p "$PROJECT_ID" -l us-central1 "gs://$BUCKET_NAME" 2>/dev/null || true
gsutil cp spacekit-storage-node-deploy.tar.gz "gs://$BUCKET_NAME/"

# Create or get instance
echo "🖥️  Setting up GCP instance..."
if ! gcloud compute instances describe "$INSTANCE_NAME" --zone="$ZONE" --project="$PROJECT_ID" &>/dev/null; then
    echo "   Creating new instance..."
    gcloud compute instances create "$INSTANCE_NAME" \
        --zone="$ZONE" \
        --project="$PROJECT_ID" \
        --image-family=ubuntu-2204-lts \
        --image-project=ubuntu-os-cloud \
        --machine-type=e2-medium \
        --boot-disk-size=50GB \
        --tags=spacekit-storage-node \
        --metadata=startup-script="#!/bin/bash
            apt-get update
            apt-get install -y curl
            curl -o /tmp/deploy.tar.gz https://storage.googleapis.com/$BUCKET_NAME/spacekit-storage-node-deploy.tar.gz
            cd /tmp
            tar xzf deploy.tar.gz
            cd spacekit-storage-node
            chmod +x deploy.sh
            ./deploy.sh
        "
else
    echo "   Instance exists, uploading deployment package..."
    gcloud compute scp spacekit-storage-node-deploy.tar.gz "$INSTANCE_NAME:/tmp/" \
        --zone="$ZONE" --project="$PROJECT_ID"
    
    gcloud compute ssh "$INSTANCE_NAME" \
        --zone="$ZONE" \
        --project="$PROJECT_ID" \
        --command="cd /tmp && tar xzf spacekit-storage-node-deploy.tar.gz && cd spacekit-storage-node && sudo ./deploy.sh"
fi

# Configure firewall
echo "🔥 Configuring firewall..."
gcloud compute firewall-rules create allow-spacekit-storage-node \
    --project="$PROJECT_ID" \
    --allow tcp:3030 \
    --source-ranges 0.0.0.0/0 \
    --target-tags spacekit-storage-node \
    --description "Allow SpaceKit Storage Node API" 2>/dev/null || echo "   Firewall rule may already exist"

# Get instance IP
INSTANCE_IP=$(gcloud compute instances describe "$INSTANCE_NAME" \
    --zone="$ZONE" \
    --project="$PROJECT_ID" \
    --format="get(networkInterfaces[0].accessConfigs[0].natIP)")

# Cleanup
rm -rf "$TEMP_DIR"
gsutil rm "gs://$BUCKET_NAME/spacekit-storage-node-deploy.tar.gz" 2>/dev/null || true
gsutil rb "gs://$BUCKET_NAME" 2>/dev/null || true

echo ""
echo "✅ Deployment complete!"
echo ""
echo "📋 Instance Information:"
echo "   Name: $INSTANCE_NAME"
echo "   Zone: $ZONE"
echo "   IP: $INSTANCE_IP"
echo ""
echo "📋 Next steps:"
echo "   1. Check status: gcloud compute ssh $INSTANCE_NAME --zone=$ZONE --project=$PROJECT_ID --command='sudo systemctl status spacekit-storage-node'"
echo "   2. View logs: gcloud compute ssh $INSTANCE_NAME --zone=$ZONE --project=$PROJECT_ID --command='sudo journalctl -u spacekit-storage-node -f'"
echo "   3. Test API: curl http://$INSTANCE_IP:3030/api/health"
echo "   4. SSH access: gcloud compute ssh $INSTANCE_NAME --zone=$ZONE --project=$PROJECT_ID"

