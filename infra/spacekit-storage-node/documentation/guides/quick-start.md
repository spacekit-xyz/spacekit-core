# Quick Start: SpaceKit Simulator with Database Encryption

## TL;DR - Three Steps

### 1. Create Keys (Automatic - No Action Needed!)

**Development (Local):**
```bash
# Just run - keys created automatically
cargo run --example ai_companion_demo
```

**Production (AWS Secrets Manager):**
```bash
export DATABASE_KEM_SECRET_NAME="spacekit/simulator-database-keys"
export AWS_DEFAULT_REGION="us-east-1"
cargo run --example ai_companion_demo
```

### 2. Run SpaceKit Simulator

```bash
# Any example works - encryption is automatic
cargo run --example ai_companion_demo
cargo run --example user_encryption_demo
cargo run --example password_recovery_demo
```

### 3. Users Encrypt/Decrypt Their Data

```rust
// User creates wallet (has their own keys)
let wallet = wallet_manager.create_wallet("alice", "password", Kyber768).await?;

// User encrypts data with their public key
let encrypted = encrypt_data(data, &wallet.public_key)?;

// Store in storage node (already encrypted)
storage_node.store_encrypted_data("key", &encrypted, wallet.did.as_str()).await?;

// Retrieve and decrypt with private key
let retrieved = storage_node.retrieve_key_value("key", wallet.did.as_str()).await?;
let decrypted = decrypt_data(&retrieved, &wallet.private_key)?;
```

## Two Types of Encryption

| Type | What It Encrypts | Keys Stored Where | Who Controls |
|------|------------------|-------------------|--------------|
| **Database Encryption** | Storage node's database files | AWS Secrets Manager or local `.key` file | Storage Node (automatic) |
| **User Data Encryption** | User's actual data | User's wallet file | User (full control) |

## Key Points

✅ **Database encryption is automatic** - you don't need to do anything  
✅ **Keys are created automatically** on first run if they don't exist  
✅ **Users have their own keys** for encrypting/decrypting their data  
✅ **Access control by DID** - only owner can retrieve their data  

## Environment Variables (Optional)

Only needed for AWS Secrets Manager (production):

```bash
export DATABASE_KEM_SECRET_NAME="spacekit/simulator-database-keys"
export AWS_DEFAULT_REGION="us-east-1"
```

## Examples

See `examples/storage/user_encryption_demo.rs` for complete user encryption flow.

## Full Documentation

- `guides/simulator-integration.md` - Complete integration guide
- `ENCRYPTION_AND_SECURITY.md` - Database encryption + key management details
- `setup_simulator_keys.sh` - Setup script

