# Global User & Group Registry - Implementation Roadmap

## 🎯 Goal

Transform SpaceKit from per-node user/group registries to a **Discord-like global system** where:
- ✅ All users visible across all nodes
- ✅ Public groups discoverable by anyone
- ✅ Feed subscriptions with real-time notifications
- ✅ Cross-node synchronization

## 📋 Current State

### ✅ What Exists
- **Group Registry Contract**: `spacekit-compute-node/contracts/group_registry.rs` (on-chain group creation)
- **Storage Node API**: HTTP API server with query interface
- **Messaging Node**: Gossipsub pub/sub for notifications
- **P2P Network**: libp2p with mDNS discovery

### ❌ What's Missing
- Global user registry in Storage Node
- Group metadata storage in Storage Node
- Feed subscription system
- Group discovery API
- Cross-node user/group sync

## 🏗️ Architecture Decision

### Option 1: Storage Node as Global Registry (Recommended)
**Pros**:
- ✅ Single source of truth
- ✅ Easy to query and search
- ✅ Already has API server
- ✅ Can use existing query interface

**Cons**:
- ⚠️ Storage Node becomes critical dependency
- ⚠️ Needs high availability

**Implementation**:
- Store users and groups in Storage Node database
- All nodes query same Storage Node
- Messaging Node registers users in Storage Node (not local)

### Option 2: Blockchain-Only Registry
**Pros**:
- ✅ Fully decentralized
- ✅ No single point of failure

**Cons**:
- ❌ Slower queries
- ❌ Higher gas costs
- ❌ Complex search/filtering

### Option 3: Hybrid (Storage + Blockchain)
**Pros**:
- ✅ Fast queries from Storage Node
- ✅ Blockchain for trust/verification
- ✅ Best of both worlds

**Cons**:
- ⚠️ More complex to maintain
- ⚠️ Sync between systems

## 🚀 Recommended Implementation: Option 1 (Storage Node as Registry)

### Phase 1: Global User Registry (Week 1)

#### Step 1.1: Extend User Model in Storage Node
```rust
// In spacekit-storage-node/src/database/mod.rs
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GlobalUser {
    pub did: String,                    // Primary key
    pub username: String,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub public_key: Vec<u8>,
    pub encryption_algorithm: String,
    pub created_at: DateTime<Utc>,
    pub last_seen: Option<DateTime<Utc>>,
    pub is_online: bool,
    pub reputation_score: Option<i64>,
}
```

#### Step 1.2: Add User API Endpoints
```rust
// In spacekit-storage-node/src/api/mod.rs
POST /api/users/register
GET  /api/users/{did}
GET  /api/users/search?q={query}
PUT  /api/users/{did}/presence
GET  /api/users/online
```

#### Step 1.3: Update Messaging Node
```rust
// In spacekit-messaging-node/src/lib.rs
// Instead of local registry:
// self.message_handler.register_user(...)

// Register in Storage Node:
storage_node.register_global_user(user).await?;
```

### Phase 2: Group Discovery (Week 2)

#### Step 2.1: Add Group Model to Storage Node
```rust
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GlobalGroup {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub creator_did: String,
    pub group_type: GroupType, // Public, Private, Gated
    pub category: Option<String>,
    pub created_at: DateTime<Utc>,
    pub member_count: u32,
    pub is_active: bool,
    pub min_reputation: Option<i64>,
    pub subscription_price: Option<u64>,
}
```

#### Step 2.2: Add Group API Endpoints
```rust
POST /api/groups              // Create group
GET  /api/groups               // Discover groups (with filters)
GET  /api/groups/{id}          // Get group details
GET  /api/groups/{id}/members  // List members
POST /api/groups/{id}/join     // Join group
```

#### Step 2.3: Integrate with Group Registry Contract
```rust
// When creating group:
1. Create in Storage Node (fast queries)
2. Register on blockchain (trust/verification)
3. Create Gossipsub topic: "group:{id}"
```

### Phase 3: Feed Subscriptions (Week 3)

#### Step 3.1: Add Subscription Model
```rust
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FeedSubscription {
    pub subscriber_did: String,
    pub group_id: String,
    pub subscribed_at: DateTime<Utc>,
    pub notification_preferences: NotificationPreferences,
    pub last_read_at: Option<DateTime<Utc>>,
}
```

#### Step 3.2: Implement Subscription API
```rust
POST /api/groups/{id}/subscribe    // Subscribe to group feed
DELETE /api/groups/{id}/subscribe  // Unsubscribe
GET  /api/groups/{id}/feed         // Get feed updates
GET  /api/users/{did}/subscriptions // User's subscriptions
```

#### Step 3.3: Gossipsub Integration
```rust
// When user subscribes:
messaging_node.subscribe_to_topic(&format!("group:{}", group_id)).await?;

// When new content published:
messaging_node.publish_to_topic(
    &format!("group:{}", group_id),
    FeedNotification::NewContent { ... }
).await?;
```

### Phase 4: UI Integration (Week 4)

#### Step 4.1: Group Discovery UI
- Browse public groups
- Search by category
- Filter by type (public/private)
- Sort by member count

#### Step 4.2: Subscription Management
- Subscribe/unsubscribe buttons
- Feed view with notifications
- Notification preferences

#### Step 4.3: Real-time Updates
- Show online users
- Live feed updates
- Notification badges

## 🔄 Migration Strategy

### For Existing Users
1. Export users from Messaging Node
2. Register in Storage Node
3. Update Messaging Node to query Storage Node

### For Existing Groups
1. Export groups from Messaging Node
2. Register in Storage Node
3. Create Gossipsub topics
4. Notify existing members

## 📊 Data Flow Examples

### User Registration
```
User → Messaging Node → Storage Node (register)
                      → Compute Node (optional: on-chain registration)
```

### Group Discovery
```
User → Storage Node API → Query groups
                      → Filter by category/type
                      → Return sorted list
```

### Feed Subscription
```
User → Storage Node (create subscription)
    → Messaging Node (subscribe to Gossipsub topic)
    → Compute Node (process payment if paid group)
```

### New Content Notification
```
Publisher → Storage Node (store content)
         → Messaging Node (publish to Gossipsub topic)
         → All subscribers (receive notification automatically)
```

## 🎯 Success Metrics

- ✅ Users visible across all nodes
- ✅ Public groups discoverable
- ✅ Feed subscriptions working
- ✅ Real-time notifications delivered
- ✅ Cross-node messaging functional

## 🚧 Next Steps

1. **Start with Phase 1**: Implement global user registry
2. **Test with 2 nodes**: Verify users visible on both
3. **Add Phase 2**: Implement group discovery
4. **Add Phase 3**: Feed subscriptions
5. **Add Phase 4**: UI integration

## 💡 Alternative: Quick Win Approach

If you want to see results faster:

1. **Use Fact Storage for Groups**: Store groups as Fact Packages
2. **Query via existing API**: Use `/query/facts` endpoint
3. **Gossipsub for notifications**: Already implemented
4. **Add UI**: Browse and subscribe

This leverages existing infrastructure and can be done in 1-2 days instead of 4 weeks.

