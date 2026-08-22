# Build and Deployment Guide

## Binary Builds for Cloud Platforms

### AWS EC2 Build

**Script**: `build-docker-aws.sh`

**Target**: Linux x86_64 (Ubuntu 22.04 compatible)

**Usage**:
```bash
./build-docker-aws.sh
```

**Output**: `dist/spacekit-storage-node`

**Deployment to AWS EC2**:
```bash
# Upload to S3
aws s3 cp dist/spacekit-storage-node s3://your-bucket/binaries/

# On EC2 instance
aws s3 cp s3://your-bucket/binaries/spacekit-storage-node /usr/local/bin/
chmod +x /usr/local/bin/spacekit-storage-node

# Create systemd service
sudo nano /etc/systemd/system/spacekit-storage-node.service
```

**Systemd Service Example**:
```ini
[Unit]
Description=SpaceKit Storage Node
After=network.target

[Service]
Type=simple
User=spacekit
WorkingDirectory=/var/lib/spacekit-storage
ExecStart=/usr/local/bin/spacekit-storage-node
Restart=always
RestartSec=10

[Install]
WantedBy=multi-user.target
```

### Google Cloud Platform Build

**Script**: `build-docker-gcp.sh`

**Target**: Linux x86_64 (GCP compatible)

**Usage**:
```bash
./build-docker-gcp.sh
```

**Output**: `dist-gcp/spacekit-storage-node`

**Deployment Options**:

#### Option 1: Compute Engine
```bash
# Upload to GCS
gsutil cp dist-gcp/spacekit-storage-node gs://your-bucket/binaries/

# Create VM
gcloud compute instances create storage-node \
  --image-family=ubuntu-2204-lts \
  --image-project=ubuntu-os-cloud \
  --machine-type=e2-medium \
  --zone=us-central1-a \
  --metadata=startup-script='#!/bin/bash
    gsutil cp gs://your-bucket/binaries/spacekit-storage-node /usr/local/bin/
    chmod +x /usr/local/bin/spacekit-storage-node
    systemctl enable spacekit-storage-node
    systemctl start spacekit-storage-node'
```

#### Option 2: Cloud Run (Containerized)
See `build-docker-gcp-container.sh` for container image build.

## Build Requirements

### Prerequisites
- Docker with buildx support
- rsync (for copying source files)
- 5GB+ free disk space

### Build Time
- First build: ~15-20 minutes (compiling dependencies)
- Subsequent builds: ~5-10 minutes (incremental)

## Binary Verification

After building, verify the binary:

```bash
# Check binary type
file dist/spacekit-storage-node

# Check dependencies (should be minimal)
ldd dist/spacekit-storage-node

# Test binary
./dist/spacekit-storage-node --help
```

## Platform-Specific Notes

### AWS EC2
- Uses Ubuntu 22.04 base image
- Compatible with t3, t3a, m5, m5a instance types
- Requires libssl1.1 or newer

### Google Cloud Platform
- Compatible with Ubuntu 22.04 LTS images
- Works on e2, n2, n2d instance types
- Compatible with Cloud Run (containerized)

## Troubleshooting

### Build Fails with "No space left on device"
- Clean Docker: `docker system prune -a`
- Increase Docker disk space allocation

### Binary doesn't run on target platform
- Ensure target platform matches build platform (linux/amd64)
- Check glibc version compatibility (Ubuntu 22.04 uses glibc 2.35)

### Missing dependencies on target
- Most dependencies are statically linked
- May need libssl1.1 on older systems
- Install: `sudo apt-get install libssl1.1`

