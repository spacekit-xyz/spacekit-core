# SpaceKit Storage Node - Deployment Guide

## Overview

This directory contains deployment configurations and scripts for deploying the SpaceKit Storage Node to production environments on AWS EC2 and Google Cloud Platform.

## Prerequisites

### AWS Deployment
- AWS CLI configured with appropriate credentials
- EC2 Key Pair created
- Binary built using `../build-docker-aws.sh`

### GCP Deployment
- Google Cloud SDK (`gcloud`) installed and configured
- Project ID with billing enabled
- Binary built using `../build-docker-gcp.sh`

## Quick Start

### AWS EC2 Deployment

1. **Build the binary:**
   ```bash
   cd ..
   ./build-docker-aws.sh
   ```

2. **Deploy to EC2:**
   ```bash
   ./aws-ec2-deploy.sh <EC2_INSTANCE_IP>
   ```

3. **Or use CloudFormation:**
   ```bash
   aws cloudformation create-stack \
     --stack-name spacekit-storage-node \
     --template-body file://aws-cloudformation.yaml \
     --parameters ParameterKey=KeyPairName,ParameterValue=your-key-pair
   ```

### Google Cloud Platform Deployment

1. **Build the binary:**
   ```bash
   cd ..
   ./build-docker-gcp.sh
   ```

2. **Deploy to Compute Engine:**
   ```bash
   ./gcp-compute-deploy.sh spacekit-storage-node us-central1-a ./dist-gcp/spacekit-storage-node <PROJECT_ID>
   ```

3. **Or use Deployment Manager:**
   ```bash
   gcloud deployment-manager deployments create spacekit-storage-node \
     --config gcp-deployment-manager.yaml
   ```

4. **Or deploy to Cloud Run (serverless):**
   ```bash
   # First, build container image
   docker build -t gcr.io/PROJECT_ID/spacekit-storage-node:latest .
   docker push gcr.io/PROJECT_ID/spacekit-storage-node:latest
   
   # Deploy to Cloud Run
   gcloud run deploy spacekit-storage-node \
     --image gcr.io/PROJECT_ID/spacekit-storage-node:latest \
     --platform managed \
     --region us-central1 \
     --allow-unauthenticated
   ```

## Configuration

### Environment Variables

- `NODE_DID` - Decentralized Identifier for the node
- `API_PORT` - HTTP API port (default: 3030)
- `DATA_DIR` - Data storage directory
- `MAX_STORAGE_BYTES` - Maximum storage capacity

### Firewall Rules

**AWS:**
- Port 3030: HTTP API
- Port 4001: P2P Network
- Port 22: SSH

**GCP:**
- Port 3030: HTTP API
- Port 4001: P2P Network
- Port 22: SSH

## Monitoring

### Systemd Service

```bash
# Check status
sudo systemctl status spacekit-storage-node

# View logs
sudo journalctl -u spacekit-storage-node -f

# Restart service
sudo systemctl restart spacekit-storage-node
```

### Health Check

```bash
curl http://<INSTANCE_IP>:3030/api/health
```

## High Availability

For production deployments, consider:

1. **Load Balancing**: Use AWS ELB or GCP Load Balancer
2. **Auto Scaling**: Configure auto-scaling groups
3. **Database Replication**: Set up multiple storage nodes
4. **Backup Strategy**: Regular backups of data directory
5. **Monitoring**: CloudWatch (AWS) or Cloud Monitoring (GCP)

## Security

- Firewall rules restrict access to necessary ports only
- Systemd service runs as non-root user (`spacekit`)
- Security hardening applied (NoNewPrivileges, PrivateTmp, etc.)
- Rate limiting enabled on API endpoints

## Troubleshooting

### Binary not found
- Ensure you've run the build script first
- Check binary path in deployment script

### Service won't start
- Check logs: `sudo journalctl -u spacekit-storage-node -n 50`
- Verify binary permissions: `ls -l /opt/spacekit-storage-node/bin/`
- Check config file: `cat /opt/spacekit-storage-node/config/config.toml`

### Port already in use
- Check what's using the port: `sudo lsof -i :3030`
- Update config to use different port

## Support

For issues or questions, see the main project documentation in `../documentation/`.

