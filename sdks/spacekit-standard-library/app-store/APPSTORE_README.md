# SWTCHX AppStore Smart Contracts

Decentralized app marketplace with NFT-based licensing and quantum-secure distribution.

## Overview

The AppStore system consists of two main smart contracts:

1. **AppStore Contract** (`appstore.rs`) - Main marketplace for publishing, purchasing, and discovering apps
2. **App License NFT Contract** (`app_license_nft.rs`) - NFT contract for each app (one instance per app)

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                  AppStore Contract                      │
│  - App Registry & Discovery                            │
│  - Purchase Flow (Payment + NFT Minting)               │
│  - Revenue Distribution (99% creator, 1% platform)     │
│  - Featured Apps & Categories                          │
└─────────────────────────────────────────────────────────┘
                        │
                        │ Mints NFT for each purchase
                        ▼
┌─────────────────────────────────────────────────────────┐
│            App License NFT Contract (per app)           │
│  - NFT Ownership = License to Use App                  │
│  - Transferable Licenses (for resale)                  │
│  - License Metadata (price, version, timestamp)        │
└─────────────────────────────────────────────────────────┘
```

## How It Works

### Publishing an App

1. Developer uploads WASM binary to **Fact Package** system
2. Developer calls `publish_app()` with:
   - App metadata (name, description, category)
   - Price in SWTCHX tokens
   - Fact package ID (reference to uploaded WASM)
   - WASM hash (for verification)
   - License type (Personal, Commercial, Enterprise)
3. Contract verifies:
   - Publisher has sufficient reputation (min 100)
   - Fact package exists and hash matches
4. Contract deploys new NFT contract for this app
5. App is indexed by category and author

### Purchasing an App

1. User calls `purchase_app(app_id)`
2. If paid app:
   - Transfer SWTCHX tokens from buyer
   - 99% goes to app creator
   - 1% goes to platform treasury
3. Mint NFT license to buyer's wallet
4. NFT ownership = proof of license
5. User can now download app from P2P network using NFT as proof

### Downloading an App

1. User requests download from P2P network
2. P2P nodes verify user owns NFT for this app
3. Nodes deliver Fact Package containing WASM binary
4. Client verifies WASM hash matches blockchain record
5. App is installed locally

### App Updates

- When developer publishes new version, existing NFT holders automatically get access
- No additional payment required
- Users can choose to update or stay on current version

## Contract Functions

### AppStore Contract

#### Publishing & Management

- `publish_app()` - Publish new app to marketplace
- `update_app()` - Publish new version of existing app
- `set_featured()` - Mark app as featured (admin only)

#### Purchasing

- `purchase_app()` - Buy app (paid or free)
- `has_license()` - Check if user owns license
- `transfer_license()` - Transfer NFT to another user

#### Discovery

- `get_featured_apps()` - Get curated featured apps
- `get_popular_apps()` - Get top apps by downloads
- `get_apps_by_category()` - Browse apps in a category
- `get_apps_by_author()` - Get all apps by a developer
- `search_apps()` - Search by name/description

#### Analytics

- `get_download_count()` - Get total downloads for an app
- `get_app_revenue()` - Get total revenue for an app

### App License NFT Contract

#### Core NFT Operations

- `initialize()` - Set up NFT contract for an app
- `mint()` - Mint new license (called by AppStore)
- `transfer()` - Transfer license to another user
- `burn()` - Revoke license

#### Querying

- `owner_of()` - Get owner of a token
- `has_license()` - Check if DID owns any license
- `tokens_of()` - Get all tokens owned by a DID
- `get_metadata()` - Get license metadata
- `total_supply()` - Get total licenses minted

## Revenue Model

### Purchase Flow

```
User pays 0.1 SWTCHX for app
    │
    ├─► 0.099 SWTCHX (99%) → App Creator
    │
    └─► 0.001 SWTCHX (1%) → Platform Treasury
```

### Free Apps

- Free apps (price = 0) still mint NFT licenses
- NFT proves ownership without payment
- Useful for access control and analytics

## License Types

### Personal License (Non-Transferable)
- Single user only
- Cannot be resold or transferred
- Cheapest option for users

### Commercial License (Transferable)
- Can be resold to others
- Commercial use allowed
- Creates secondary market

### Enterprise License (Multi-Seat)
- Transferable
- Designed for business use
- Can be shared within organization

## Security Features

### Quantum-Resistant

- **Kyber768** - Fact package hash verification
- **Dilithium** - Smart contract signatures
- **SPHINCS+** - Long-term NFT ownership proofs

### Code Safety

- **WASM Sandboxing** - Apps run in isolated environment
- **Fact Verification** - Downloaded code must match blockchain hash
- **Capability-Based** - Apps must request permissions

### Economic Security

- **Reputation Gating** - Minimum 100 reputation to publish
- **Escrow System** - Atomic payment + NFT minting
- **Fraud Protection** - Malicious apps can be reported and suspended

## Integration with UI

The React UI (`MarketPlacePage.tsx`) connects to these contracts:

```typescript
// Publishing an app
const publishApp = async () => {
    const appId = await invoke("publish_app", {
        name: "My App",
        description: "Cool app",
        category: "productivity",
        price: 100000000, // 0.1 SWTCHX in smallest units
        factPackageId: "fact_pkg_myapp_v1",
        wasmHash: "0x123...",
    });
};

// Purchasing an app
const purchaseApp = async (appId) => {
    const tokenId = await invoke("purchase_app", { appId });
    // NFT minted to user's wallet
    // Can now download from P2P network
};

// Checking license ownership
const hasLicense = await invoke("has_license", {
    appId,
    userDid: currentUser.did,
});
```

## Example App Metadata

```json
{
  "app_id": "1234567890",
  "name": "SWTCHX Messenger",
  "description": "Quantum-secure P2P messaging",
  "category": "social",
  "version": "1.0.0",
  "price": 0,
  "author_did": "did:swtch:team:core",
  "fact_package_id": "fact_pkg_messenger_v1",
  "wasm_hash": "0xabc123...",
  "nft_contract_id": "nft_app_1234567890",
  "is_featured": true,
  "feature_banner": "Editor's Choice",
  "downloads": 1250,
  "rating": 4.8,
  "created_at": 1698768000,
  "updated_at": 1698768000
}
```

## Building the Contracts

```bash
# Build AppStore contract
cargo build --target wasm32-unknown-unknown --release --manifest-path appstore.rs

# Build App License NFT contract
cargo build --target wasm32-unknown-unknown --release --manifest-path app_license_nft.rs
```

## Deployment

1. Deploy AppStore contract to blockchain
2. For each app published, a new NFT contract instance is deployed
3. NFT contract address is stored in app metadata

## Events Emitted

### AppStore Events

- `AppPublished(app_id, author_did, name, price)`
- `AppUpdated(app_id, new_version)`
- `AppPurchased(app_id, buyer_did, token_id, price)`
- `RevenueDistributed(app_id, author_amount, platform_amount)`
- `LicenseTransferred(app_id, token_id, from_did, to_did)`

### NFT Events

- `Transfer(from_did, to_did, token_id)`
- `Burn(token_id)`

## Gas Optimization

- **Batch Operations**: Purchase multiple apps in one transaction
- **Lazy NFT Deployment**: NFT contract only deployed on first purchase
- **Efficient Indexing**: Apps indexed by category, author, featured status
- **Minimal Storage**: Only essential data on-chain, rest in Fact Packages

## Reputation Integration

Publishers must have:
- **Application Reputation** ≥ 100 to publish apps
- **Overall Reputation** ≥ 0 to purchase apps

Higher reputation publishers may get:
- Featured placement priority
- Lower platform fees
- Verified badge

## Future Enhancements

1. **App Bundles** - Buy multiple apps at discount
2. **Subscriptions** - Monthly/yearly licenses
3. **Trial Periods** - 7-day free trial
4. **Referral Program** - Earn rewards for referrals
5. **DAO Governance** - Community votes on featured apps
6. **Cross-Chain** - Apps work across multiple blockchains
7. **Update Notifications** - Push notifications for app updates

## Testing

```bash
# Unit tests (when implemented)
cargo test

# Integration tests with simulator
./test-appstore.sh
```

## Status

- [x] AppStore contract core functions
- [x] App License NFT contract
- [x] Revenue distribution (99/1 split)
- [x] Reputation gating
- [x] Fact package integration
- [x] React UI with featured/popular sections
- [ ] Full metadata serialization
- [ ] Complete indexing system
- [ ] Rating & review system
- [ ] Search implementation
- [ ] Deployment scripts

---

**Version**: 1.0  
**Last Updated**: November 1, 2025  
**Status**: Development / Ready for Testing

