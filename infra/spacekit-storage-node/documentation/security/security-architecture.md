# Security Architecture - Protection Against Instance Compromise

## Executive Summary

**Question:** Can a hacker who gains access to your EC2/GCP instance read your encrypted data?

**Answer:** **NO** - Multiple security layers prevent this, even with full instance access.

## Security Layers

### Layer 1: Database Encryption Keys (Storage Node Internal)

**What it protects:** Database files (metadata, indexes, file locations)

**How it works:**
```
Database Files → Encrypted with Database Keys → Stored on Disk
Database Keys → Stored in AWS Secrets Manager → Encrypted with KMS
```

**Protection mechanisms:**
1. ✅ Keys **NOT stored on instance** - Only in AWS Secrets Manager
2. ✅ Keys encrypted with **AWS KMS** (additional encryption layer)
3. ✅ Access via **IAM roles** (not stored credentials)
4. ✅ **IP restrictions** - Only from authorized VPCs
5. ✅ **All access logged** - CloudTrail tracks every access
6. ✅ **Time-based restrictions** - Optional time windows

**Even if attacker gets instance access:**
- ❌ Cannot access Secrets Manager (requires IAM role)
- ❌ Cannot read keys from disk (keys not on instance)
- ❌ Cannot decrypt database files (keys not accessible)

### Layer 2: User Data Encryption (Application Level)

**What it protects:** User's actual data (conversations, files, etc.)

**How it works:**
```
User Data → Encrypted with User's Wallet Public Key → Stored in Database
User Private Key → Encrypted with User's Password → Stored in Wallet
```

**Protection mechanisms:**
1. ✅ User data encrypted with **separate keys** (not database keys)
2. ✅ User private keys encrypted with **user passwords**
3. ✅ Access controlled by **DID** (Decentralized Identifier)
4. ✅ **Zero-knowledge architecture** - Server never sees plaintext

**Even if attacker gets database keys:**
- ❌ Cannot decrypt user data (different encryption keys)
- ❌ Cannot access user private keys (encrypted with passwords)
- ❌ Cannot impersonate users (DID-based access control)

## Attack Scenarios

### Scenario 1: Attacker Gains Root Access to EC2

**What attacker can do:**
- ✅ Read files on disk
- ✅ Access running processes
- ✅ View environment variables
- ✅ Check network connections

**What attacker CANNOT do:**
- ❌ Access AWS Secrets Manager (requires IAM role, not on instance)
- ❌ Decrypt database files (keys not accessible)
- ❌ Decrypt user data (separate encryption keys)
- ❌ Access user passwords (not stored on server)

**Result:** Attacker sees encrypted data but cannot decrypt it.

## Operational Hardening (Public Internet)

Even with strong encryption, production deployments should add network-level controls:

1. **Reverse proxy / WAF** (Cloudflare, AWS WAF, nginx/ALB) for connection limits and L7 protection
2. **Distributed rate limiting** (Redis or gateway-level) to avoid per-node bypass
3. **Request timeouts** at the proxy to mitigate Slowloris-style attacks
4. **Centralized logging/metrics** for anomaly detection

### Scenario 2: Attacker Gets IAM Credentials

**What attacker can do:**
- ✅ Access AWS Secrets Manager (if credentials valid)
- ✅ Retrieve database encryption keys

**What attacker CANNOT do:**
- ❌ Decrypt user data (different encryption keys)
- ❌ Access user private keys (encrypted with passwords)
- ❌ Bypass IP restrictions (if configured)
- ❌ Hide access (CloudTrail logs everything)

**Result:** Attacker can decrypt database structure but not user data.

### Scenario 3: Attacker Gets Database Files

**What attacker can do:**
- ✅ Read encrypted database files
- ✅ Analyze file structure

**What attacker CANNOT do:**
- ❌ Decrypt database files (need keys from Secrets Manager)
- ❌ Decrypt user data (even with database keys, user data uses different keys)
- ❌ Access user data without user's private key and password

**Result:** Attacker has encrypted files but cannot decrypt them.

## Security Architecture Diagram

```
┌─────────────────────────────────────────────────────────────┐
│                    EC2/GCP Instance                         │
│  ┌──────────────────────────────────────────────────────┐  │
│  │         SpaceKit Storage Node                        │  │
│  │                                                      │  │
│  │  ┌──────────────────────────────────────────────┐  │  │
│  │  │  Database Files (Encrypted)                  │  │  │
│  │  │  - Metadata                                   │  │  │
│  │  │  - Indexes                                    │  │  │
│  │  │  - User Data (Encrypted with User Keys)      │  │  │
│  │  └──────────────────────────────────────────────┘  │  │
│  │           │                                         │  │
│  │           │ Needs Database Keys                    │  │
│  │           ▼                                         │  │
│  │  ┌──────────────────────────────────────────────┐  │  │
│  │  │  AWS Secrets Manager (via IAM Role)          │  │  │
│  │  │  - Database Keys (Encrypted with KMS)        │  │  │
│  │  │  - IP Restricted                             │  │  │
│  │  │  - Access Logged                             │  │  │
│  │  └──────────────────────────────────────────────┘  │  │
│  └──────────────────────────────────────────────────────┘  │
│                                                             │
│  ⚠️  Attacker with instance access:                        │
│     ❌ Cannot access Secrets Manager (IAM required)        │
│     ❌ Cannot decrypt database (keys not accessible)       │
│     ❌ Cannot decrypt user data (different keys)            │
└─────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│                    User Data Flow                           │
│                                                             │
│  User Data → Encrypt with User's Public Key                │
│           → Store in Database (Encrypted)                  │
│           → Database Encrypted with Database Keys          │
│                                                             │
│  To Decrypt:                                                │
│    1. Get Database Keys (from Secrets Manager)            │
│    2. Decrypt Database                                     │
│    3. Get User's Encrypted Data                            │
│    4. Decrypt with User's Private Key (needs password)     │
│                                                             │
│  ⚠️  Attacker needs:                                        │
│     - Database Keys (from Secrets Manager)                 │
│     - User's Private Key (encrypted with password)          │
│     - User's Password                                       │
│     → All three required = Impossible                      │
└─────────────────────────────────────────────────────────────┘
```

## Implementation Checklist

### Required for Production

- [ ] **AWS KMS Key** - Encrypt Secrets Manager secrets
- [ ] **IAM Role** - Attach to EC2 instance (not access keys)
- [ ] **IAM Policy** - Least privilege, IP restrictions
- [ ] **CloudTrail** - Log all Secrets Manager access
- [ ] **Private Subnet** - No public IP for instance
- [ ] **Security Groups** - Restrict network access

### Recommended for Enhanced Security

- [ ] **VPC Endpoint** - Secrets Manager access without internet
- [ ] **CloudWatch Alarms** - Alert on unauthorized access
- [ ] **Key Rotation** - Automatic rotation every 90 days
- [ ] **Time Restrictions** - Only allow access during business hours
- [ ] **MFA** - Require MFA for key rotation operations

### Optional for Maximum Security

- [ ] **Hardware Security Module (HSM)** - CloudHSM for key storage
- [ ] **Multi-Region Replication** - High availability
- [ ] **Penetration Testing** - Regular security audits
- [ ] **Key Escrow** - Disaster recovery (separate from production)

## Quick Setup Commands

### 1. Create KMS Key
```bash
aws kms create-key --description "SpaceKit Storage Encryption"
# Save Key ID: arn:aws:kms:us-east-1:123456789012:key/abcd1234-...
```

### 2. Create Secret with KMS
```bash
export AWS_KMS_KEY_ID="arn:aws:kms:us-east-1:123456789012:key/abcd1234-..."
aws secretsmanager create-secret \
    --name spacekit/storage-node-database-keys \
    --kms-key-id $AWS_KMS_KEY_ID
```

### 3. Create IAM Role and Policy
```bash
# Create role
aws iam create-role --role-name SpaceKitStorageNodeRole \
    --assume-role-policy-document file://trust-policy.json

# Attach policy (see `security/security-quick-reference.md` for a minimal production checklist)
aws iam put-role-policy --role-name SpaceKitStorageNodeRole \
    --policy-name StorageNodeSecretsPolicy \
    --policy-document file://secrets-policy.json

# Create instance profile
aws iam create-instance-profile --instance-profile-name SpaceKitStorageNodeProfile
aws iam add-role-to-instance-profile \
    --instance-profile-name SpaceKitStorageNodeProfile \
    --role-name SpaceKitStorageNodeRole
```

### 4. Attach to EC2 Instance
```bash
aws ec2 associate-iam-instance-profile \
    --instance-id i-1234567890abcdef0 \
    --iam-instance-profile Name=SpaceKitStorageNodeProfile
```

### 5. Set Environment Variables
```bash
export DATABASE_KEM_SECRET_NAME="spacekit/storage-node-database-keys"
export AWS_KMS_KEY_ID="arn:aws:kms:us-east-1:123456789012:key/abcd1234-..."
export AWS_DEFAULT_REGION="us-east-1"
```

## Monitoring and Alerts

### CloudWatch Alarm for Unauthorized Access
```bash
aws cloudwatch put-metric-alarm \
    --alarm-name UnauthorizedSecretsAccess \
    --alarm-description "Alert on unauthorized Secrets Manager access" \
    --metric-name GetSecretValue \
    --namespace AWS/SecretsManager \
    --statistic Sum \
    --period 300 \
    --evaluation-periods 1 \
    --threshold 10 \
    --comparison-operator GreaterThanThreshold \
    --alarm-actions arn:aws:sns:us-east-1:123456789012:security-alerts
```

### CloudTrail Query for Key Access
```bash
aws cloudtrail lookup-events \
    --lookup-attributes AttributeKey=ResourceName,AttributeValue=spacekit/storage-node-database-keys \
    --max-results 50
```

## Incident Response

If instance is compromised:

1. **Immediate Actions:**
   - Rotate encryption keys
   - Revoke IAM role access
   - Review CloudTrail logs
   - Notify security team

2. **Investigation:**
   - Check CloudTrail for key access
   - Review CloudWatch alarms
   - Analyze access patterns
   - Document incident

3. **Recovery:**
   - Generate new keys
   - Re-encrypt database with new keys
   - Update IAM policies
   - Test security controls

## Summary

✅ **Database keys** protected by:
- AWS Secrets Manager (not on instance)
- AWS KMS encryption
- IAM role-based access
- IP restrictions
- Access logging

✅ **User data** protected by:
- Separate encryption keys (user wallet keys)
- Password-encrypted private keys
- DID-based access control
- Zero-knowledge architecture

✅ **Even with instance access:**
- Cannot access database keys (IAM protected)
- Cannot decrypt database files (keys not accessible)
- Cannot decrypt user data (different keys)

**Result:** Multi-layer security ensures data remains protected even if instance is compromised.

