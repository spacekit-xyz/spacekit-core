# Security Quick Reference - Protecting Against Instance Compromise

## 🚨 Critical: Even if hacker gets EC2/GCP access, they CANNOT decrypt your data!

## Why It's Secure

### ✅ Multi-Layer Protection

1. **Database Keys** → Stored in AWS Secrets Manager (encrypted with KMS)
   - Not stored on instance
   - Requires IAM role access
   - IP-restricted access
   - All access logged

2. **User Data** → Encrypted with user's wallet keys (separate from database keys)
   - Even with database keys, attacker cannot decrypt user data
   - User private keys encrypted with passwords
   - Access controlled by DID

## Quick Setup (5 Minutes)

### Step 1: Create KMS Key
```bash
aws kms create-key \
    --description "SpaceKit Storage Encryption" \
    --key-usage ENCRYPT_DECRYPT
# Save the Key ID
```

### Step 2: Create Secret with KMS
```bash
export AWS_KMS_KEY_ID="arn:aws:kms:us-east-1:123456789012:key/abcd1234-..."
aws secretsmanager create-secret \
    --name spacekit/storage-node-database-keys \
    --kms-key-id $AWS_KMS_KEY_ID
```

### Step 3: Create Restrictive IAM Policy
```json
{
  "Version": "2012-10-17",
  "Statement": [{
    "Effect": "Allow",
    "Action": ["secretsmanager:GetSecretValue"],
    "Resource": "arn:aws:secretsmanager:*:*:secret:spacekit/storage-node-database-keys-*",
    "Condition": {
      "IpAddress": {"aws:SourceIp": ["10.0.0.0/8"]}
    }
  }]
}
```

### Step 4: Attach IAM Role to EC2
```bash
# Use AWS Console or CLI to attach IAM role to EC2 instance
# Instance will automatically use role credentials
```

### Step 5: Set Environment Variables
```bash
export DATABASE_KEM_SECRET_NAME="spacekit/storage-node-database-keys"
export AWS_KMS_KEY_ID="arn:aws:kms:us-east-1:123456789012:key/abcd1234-..."
export AWS_DEFAULT_REGION="us-east-1"
```

## Security Checklist

### ✅ Must Have (Production)
- [ ] AWS KMS key for Secrets Manager encryption
- [ ] IAM role with least-privilege policy
- [ ] IP restrictions in IAM policy
- [ ] CloudTrail logging enabled
- [ ] Instance in private subnet (no public IP)

### ✅ Should Have (Enhanced Security)
- [ ] VPC endpoint for Secrets Manager
- [ ] CloudWatch alarms for unauthorized access
- [ ] Key rotation policy (90 days)
- [ ] Time-based access restrictions
- [ ] MFA for key rotation
- [ ] Reverse proxy/WAF with connection limits and distributed rate limiting
- [ ] SpaceKit distributed rate limiting (`SPACEKIT_RATE_LIMIT_URL` + `rate-limit-spacekit` feature, enable service with `SPACEKIT_RATE_LIMIT_ENABLE_SERVICE=1`)

### ✅ Nice to Have (Maximum Security)
- [ ] Hardware Security Module (HSM)
- [ ] Multi-region key replication
- [ ] Regular penetration testing
- [ ] Security audit logs review

## What Attacker CANNOT Do

Even with full EC2/GCP instance access:

❌ **Cannot access encryption keys**
- Keys in Secrets Manager (not on instance)
- Requires IAM role (not accessible from instance)
- IP-restricted (only from VPC)

❌ **Cannot decrypt database files**
- Files encrypted with keys not on instance
- Keys require Secrets Manager access
- Access is logged and monitored

❌ **Cannot decrypt user data**
- User data encrypted with separate wallet keys
- Database keys cannot decrypt user data
- User private keys encrypted with passwords

## Testing Security

```bash
# Simulate attacker
ssh attacker@ec2-instance

# Try to get keys (should fail)
aws secretsmanager get-secret-value --secret-id spacekit/storage-node-database-keys
# Error: AccessDenied

# Try to read database (encrypted, unreadable)
cat /var/lib/spacekit/database.json
# Encrypted gibberish

# Try to decrypt user data (impossible without user keys)
# No way to decrypt - user keys separate from database keys
```

## Emergency Response

If instance is compromised:

1. **Immediately rotate keys:**
```bash
aws secretsmanager rotate-secret --secret-id spacekit/storage-node-database-keys
```

2. **Revoke IAM access:**
```bash
aws iam detach-role-policy --role-name SpaceKitStorageNodeRole --policy-arn arn:aws:iam::...
```

3. **Review CloudTrail logs:**
```bash
aws cloudtrail lookup-events --lookup-attributes AttributeKey=ResourceName,AttributeValue=spacekit/storage-node-database-keys
```

4. **Notify security team**

## Key Points

✅ **Database keys** = Encrypt database structure (metadata, indexes)  
✅ **User wallet keys** = Encrypt user data (separate, user-controlled)  
✅ **Even with database keys** = Cannot decrypt user data  
✅ **Even with instance access** = Cannot get database keys (IAM protected)  
✅ **All access logged** = CloudTrail tracks everything  

## Full Documentation

See:
- `security/security-architecture.md`
- `security/ddos-analysis.md`
- `ENCRYPTION_AND_SECURITY.md`

