# SpaceKit Simulator Integration Guide

This guide explains how the AWS Secrets Manager integration affects projects using `spacekit-storage-node`, specifically the `spacekit-simulator`.

## Overview

There are **two types of encryption** in the system:

1. **Database Encryption** (Storage Node Internal)
   - Encrypts the storage node's database files (metadata, indexes, etc.)
   - Uses quantum KEM keys stored in AWS Secrets Manager or local files
   - Managed automatically by the storage node
   - Users don't interact with these keys directly

2. **User Data Encryption** (Application Level)
   - Users encrypt their own data with their wallet keys
   - Uses quantum KEM keys from user wallets (Kyber768, Kyber1024, etc.)
   - Users control their own encryption/decryption
   - Data is stored encrypted in the storage node

## Setup Steps

### Step 1: Create Database Encryption Keys

The storage node needs encryption keys for its internal database. You have two options:

#### Option A: Local File Storage (Development)

**No action needed!** The storage node will automatically:
- Generate quantum-resistant keys on first run
- Store them in a local `.key` file
- Use them for database encryption

```bash
# Just run your simulator - keys are created automatically
cargo run --example ai_companion_demo
```

Keys will be stored at: `./your_storage_path/db.key`

#### Option B: AWS Secrets Manager (Production)

For production deployments, configure AWS Secrets Manager:

1. **Set environment variables:**
```bash
export DATABASE_KEM_SECRET_NAME="spacekit/simulator-database-keys"
export AWS_DEFAULT_REGION="us-east-1"

# If not using IAM roles:
export AWS_ACCESS_KEY_ID="your-access-key"
export AWS_SECRET_ACCESS_KEY="your-secret-key"
```

2. **Build with AWS Secrets feature:**
```bash
cd spacekit-storage-node
cargo build --features aws-secrets --release

cd ../spacekit-simulator
cargo build --release
```

3. **Run the simulator:**
```bash
# Keys will be automatically retrieved from AWS Secrets Manager
cargo run --example ai_companion_demo
```

**First Run Behavior:**
- If keys don't exist in AWS Secrets Manager, they will be automatically generated
- New keys will be stored in AWS Secrets Manager
- Subsequent runs will use the stored keys

### Step 2: Run SpaceKit Simulator

The simulator works the same way regardless of key storage method:

```bash
# Example: AI Companion Demo
cargo run --example ai_companion_demo

# Example: User Encryption Demo
cargo run --example user_encryption_demo

# Example: Password Recovery Demo
cargo run --example password_recovery_demo
```

The storage node will automatically:
- ✅ Detect if AWS Secrets Manager is configured
- ✅ Load keys from AWS (if configured) or local files (if not)
- ✅ Encrypt/decrypt database files transparently
- ✅ Continue working normally

### Step 3: User Encryption/Decryption Flow

Users encrypt and decrypt their **own data** using their wallet keys. This is separate from database encryption.

#### Creating User Keys (Wallet)

```rust
use spacekit_simulator::wallet_manager::WalletManager;
use spacekit_primitives::v1::crypto::EncryptionAlgorithm;

let wallet_manager = WalletManager::new();
let wallet_result = wallet_manager.create_wallet(
    "alice",
    "my_secure_password",
    EncryptionAlgorithm::Kyber768,  // User's quantum key algorithm
).await?;

let wallet = &wallet_result.wallet;
// wallet.public_key - for encryption
// wallet.private_key - for decryption (encrypted with password)
```

#### Encrypting User Data

```rust
use spacekit_storage_node::{StorageNode, StorageNodeConfig};

// 1. Initialize storage node (database encryption happens automatically)
let storage_node = Arc::new(StorageNode::new(storage_config).await?);
storage_node.start().await?;

// 2. User encrypts their data with their public key
let user_data = b"My secret data";
let encrypted_data = encrypt_with_user_key(user_data, &wallet.public_key)?;

// 3. Store encrypted data in storage node
storage_node.store_encrypted_data(
    "user:alice:private_data",
    &encrypted_data,
    wallet.did.as_str(),  // Owner DID for access control
).await?;
```

#### Decrypting User Data

```rust
// 1. Retrieve encrypted data (access control enforced by DID)
let encrypted_data = storage_node.retrieve_key_value(
    "user:alice:private_data",
    wallet.did.as_str(),  // Must match owner DID
).await?;

// 2. User decrypts with their private key
let decrypted_data = decrypt_with_user_key(&encrypted_data, &wallet.private_key)?;
```

## Complete Example

Here's a complete example showing both encryption layers:

```rust
use spacekit_storage_node::{StorageNode, StorageNodeConfig, NetworkConfig};
use spacekit_simulator::wallet_manager::WalletManager;
use spacekit_primitives::v1::crypto::EncryptionAlgorithm;
use std::path::PathBuf;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    // ========================================================================
    // PART 1: Storage Node Setup (Database Encryption)
    // ========================================================================
    // Database encryption keys are managed automatically:
    // - If DATABASE_KEM_SECRET_NAME is set → uses AWS Secrets Manager
    // - Otherwise → uses local file storage
    
    let storage_config = StorageNodeConfig {
        max_storage_bytes: 10 * 1024 * 1024 * 1024,
        data_dir: PathBuf::from("./my_storage"),
        database_path: Some(PathBuf::from("./my_storage/db")),
        node_did: "did:spacekit:storage:demo".to_string(),
        preferred_algorithm: "kyber1024".to_string(),  // For database encryption
        network_config: NetworkConfig::default(),
        ..Default::default()
    };
    
    // Database encryption happens automatically here
    let storage_node = Arc::new(StorageNode::new(storage_config).await?);
    storage_node.start().await?;
    
    println!("✅ Storage node initialized");
    println!("   Database encryption: Automatic (AWS Secrets or local file)");
    
    // ========================================================================
    // PART 2: User Wallet Setup (User Data Encryption)
    // ========================================================================
    let wallet_manager = WalletManager::new();
    let wallet_result = wallet_manager.create_wallet(
        "alice",
        "my_password",
        EncryptionAlgorithm::Kyber768,  // User's encryption algorithm
    ).await?;
    
    let wallet = &wallet_result.wallet;
    println!("✅ User wallet created");
    println!("   DID: {}", wallet.did.as_str());
    println!("   Encryption: Kyber768");
    
    // ========================================================================
    // PART 3: User Encrypts and Stores Data
    // ========================================================================
    let user_data = b"My secret message";
    
    // User encrypts with their public key (simplified - use proper KEM in production)
    let encrypted_user_data = encrypt_user_data(user_data, &wallet.public_key)?;
    
    // Store in storage node (already encrypted by user)
    storage_node.store_encrypted_data(
        "user:alice:message",
        &encrypted_user_data,
        wallet.did.as_str(),
    ).await?;
    
    println!("✅ User data encrypted and stored");
    
    // ========================================================================
    // PART 4: User Retrieves and Decrypts Data
    // ========================================================================
    let retrieved = storage_node.retrieve_key_value(
        "user:alice:message",
        wallet.did.as_str(),
    ).await?;
    
    if let Some(encrypted) = retrieved {
        // User decrypts with their private key
        let decrypted = decrypt_user_data(&encrypted, &wallet.private_key)?;
        println!("✅ Data decrypted: {:?}", decrypted);
    }
    
    Ok(())
}
```

## Key Management Summary

| Key Type | Purpose | Storage | Managed By | User Access |
|----------|---------|---------|------------|-------------|
| **Database Keys** | Encrypt storage node database files | AWS Secrets Manager or local `.key` file | Storage Node | No direct access |
| **User Wallet Keys** | Encrypt/decrypt user's data | User's wallet file (encrypted with password) | User | Full control |

## Environment Variables Reference

### For Database Encryption (Storage Node)

```bash
# AWS Secrets Manager (Production)
export DATABASE_KEM_SECRET_NAME="spacekit/simulator-database-keys"
export AWS_DEFAULT_REGION="us-east-1"
export AWS_ACCESS_KEY_ID="your-key"  # Optional if using IAM roles
export AWS_SECRET_ACCESS_KEY="your-secret"  # Optional if using IAM roles

# Alternative secret name
export QUANTUM_KEYPAIR_SECRET_NAME="spacekit/quantum-keypair"
```

### For User Data Encryption (Application)

Users manage their own keys through the wallet system - no environment variables needed.

## Troubleshooting

### Issue: "Failed to load keys from AWS Secrets Manager"

**Solution:**
1. Check AWS credentials are configured
2. Verify IAM permissions for Secrets Manager
3. Check secret name matches `DATABASE_KEM_SECRET_NAME`
4. Fallback: Remove environment variable to use local file storage

### Issue: "Access denied" when retrieving user data

**Solution:**
- Ensure you're using the correct DID (owner DID)
- Verify the wallet DID matches the owner DID used when storing
- Check access control permissions

### Issue: Keys not found on first run

**Solution:**
- This is normal! Keys will be automatically generated
- For AWS: Keys will be stored in Secrets Manager
- For local: Keys will be stored in `.key` file

## Migration Guide

### From Local to AWS Secrets Manager

1. **Backup existing keys:**
```bash
# Find your key file
find . -name "*.key" -type f

# Backup it
cp ./my_storage/db.key ./my_storage/db.key.backup
```

2. **Set up AWS Secrets Manager:**
```bash
export DATABASE_KEM_SECRET_NAME="spacekit/simulator-database-keys"
export AWS_DEFAULT_REGION="us-east-1"
```

3. **Run simulator:**
```bash
# First run will migrate keys to AWS
cargo run --example ai_companion_demo
```

4. **Verify:**
```bash
# Check AWS Secrets Manager has the keys
aws secretsmanager get-secret-value --secret-id spacekit/simulator-database-keys
```

### From AWS to Local

1. **Remove AWS environment variables:**
```bash
unset DATABASE_KEM_SECRET_NAME
unset QUANTUM_KEYPAIR_SECRET_NAME
```

2. **Run simulator:**
```bash
# Will automatically fall back to local file storage
cargo run --example ai_companion_demo
```

## Security Best Practices

1. **Development:**
   - Use local file storage (default)
   - Keep `.key` files secure (don't commit to git)
   - Use strong passwords for user wallets

2. **Production:**
   - Use AWS Secrets Manager for database keys
   - Configure IAM roles with least privilege
   - Enable AWS CloudTrail for audit logging
   - Use strong quantum algorithms (Kyber1024)
   - Rotate keys periodically

3. **User Data:**
   - Users should use strong passwords for wallets
   - Store wallet files securely
   - Use Kyber768 or Kyber1024 for user encryption
   - Never share private keys

## Next Steps

- See `examples/storage/user_encryption_demo.rs` for complete user encryption example
- See `examples/storage/password_recovery_demo.rs` for password recovery
- See `ENCRYPTION_AND_SECURITY.md` for detailed database encryption documentation

