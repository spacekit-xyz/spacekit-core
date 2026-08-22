# SpaceKit Testnet Deploy Checklist (Terraform + Config)

> Scope: AWS testnet infrastructure for compute-node, storage-node, snapshots, STUN/TURN, and P2P bootstrap/signaling.

---

## 1) Terraform Modules (Suggested)

- `networking/vpc`  
  - VPC, public subnets, NAT gateway (if needed)
- `security/groups`  
  - Ingress for RPC, WS, storage API, TURN, bootstrap/signaling
- `compute-node/service`  
  - ECS or EC2, RPC port (e.g., 8545), `/faucet` enabled
- `storage-node/service`  
  - API port (e.g., 8080), persistence volume
- `p2p/bootstrap`  
  - WebSocket bootstrap server (9050)
- `p2p/signaling`  
  - WebRTC signaling server (9051)
- `turn/coturn`  
  - STUN/TURN with static credentials
- `snapshots/s3`  
  - S3 + CloudFront for snapshot manifest and chunks
- `observability`  
  - CloudWatch logs/metrics, alarms

---

## 2) Required AWS Outputs

- `compute_rpc_url`
- `compute_ws_url` (optional)
- `storage_api_url`
- `p2p_bootstrap_url`
- `p2p_signaling_url`
- `turn_url` + `turn_username` + `turn_credential`
- `snapshot_base_url`

---

## 3) Compute Node Config (env)

- `SPACEKIT_NODE_DID`
- `SPACEKIT_RPC_PORT`
- `SPACEKIT_CHAIN_ID`
- `SPACEKIT_FAUCET_ENABLED=true`
- `SPACEKIT_FAUCET_AMOUNT` (uASTRA)
- `SPACEKIT_FAUCET_COOLDOWN_SECONDS`
- `SPACEKIT_FAUCET_MAX_REQUESTS`
- `SPACEKIT_API_KEY` (optional)

---

## 4) Storage Node Config (env)

- `SPACEKIT_STORAGE_PORT`
- `SPACEKIT_STORAGE_API_KEY`
- `SPACEKIT_STORAGE_COLLECTION=spacekitvm`
- `SPACEKIT_STORAGE_DID`

---

## 5) TURN / STUN Config

- `TURN_REALM=spacekit.xyz`
- `TURN_USER`
- `TURN_PASS`
- `TURN_PORT=3478`
- TLS certs for TURN (optional but recommended)

---

## 6) Snapshot Hosting

- Upload `snapshot.json` and `chunks/*` to S3
- Ensure `application/json` and `application/octet-stream` MIME types
- Enable cache for chunks; short cache for `snapshot.json`

---

## 7) Website Config (env or build-time)

- `VITE_RPC_URL`
- `VITE_STORAGE_URL`
- `VITE_P2P_BOOTSTRAP_URL`
- `VITE_P2P_SIGNAL_URL`
- `VITE_TURN_URL`
- `VITE_TURN_USER`
- `VITE_TURN_CRED`
- `VITE_SNAPSHOT_URL`

---

## 8) Validation Checklist

- `/rpc` reachable (POST)
- `/faucet` reachable (POST)
- Storage `/api/documents` reachable with DID auth
- P2P bootstrap + signaling WS reachable
- TURN connectivity works from browser
- Snapshot downloads verify hashes

---

## 9) Rollout Steps

- `terraform init && terraform apply`
- Deploy compute-node + storage-node containers
- Deploy P2P WS services
- Deploy TURN
- Upload snapshots
- Smoke test in Playground

