# Global User & Group Registry - Discord-like Architecture

## 🎯 Overview

This document describes the architecture for a **global user pool** and **discoverable public/private groups** that work across multiple nodes, similar to Discord but decentralized.

## Current Problem

- ❌ Each messaging node has its own user registry (not shared)
- ❌ Groups are only local to one node (not discoverable)
- ❌ No way to browse public groups across the network
- ❌ No feed subscription mechanism
- ❌ Users on different nodes can't see each other

## Proposed Solution

### Architecture: Storage Node as Global Registry

```
┌─────────────────────────────────────────────────────────────┐
│                    Global Registry Architecture              │
└─────────────────────────────────────────────────────────────┘

Storage Node (Global Registry)
├─ Users: Global user profiles (DID → UserProfile)
├─ Groups: Public/private groups (GroupID → GroupMetadata)
├─ Memberships: Group memberships (GroupID → Vec<DID>)
└─ Subscriptions: Feed subscriptions (DID → Vec<GroupID>)

Compute Node (Smart Contracts)
├─ Group Registry Contract: On-chain group creation/discovery
├─ Membership Contract: Access control, invitations
└─ Subscription Contract: Feed subscriptions, payments

Messaging Node (Real-time Notifications)
├─ Gossipsub Topics: One per group (group:{group_id})
├─ User Presence: Presence announcements
└─ Feed Updates: New content notifications
```

## 🏗️ Component Design

### 1. Global User Registry (Storage Node)

**Storage**: All users stored in Storage Node's database

```rust
// In Storage Node database
pub struct GlobalUser {
    pub did: String,
    pub username: String,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub public_key: Vec<u8>,
    pub encryption_algorithm: String,
    pub created_at: DateTime<Utc>,
    pub last_seen: Option<DateTime<Utc>>,
    pub reputation_score: Option<i64>,
    pub is_online: bool,
}
```

**API Endpoints**:
- `GET /api/users?did={did}` - Get user by DID
- `GET /api/users/search?q={query}` - Search users
- `POST /api/users/register` - Register new user
- `PUT /api/users/{did}/presence` - Update presence

**Query Interface**:
```rust
// Query all users
let users = storage_node.query()
    .select("users")
    .where_("is_online", FilterOp::Equals, true)
    .execute()
    .await?;
```

### 2. Global Group Registry (Storage Node + Compute Node)

**Storage Node**: Stores group metadata and memberships

```rust
pub struct GlobalGroup {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub creator_did: String,
    pub group_type: GroupType, // Public, Private, Gated
    pub category: Option<String>, // Technology, Gaming, etc.
    pub created_at: DateTime<Utc>,
    pub member_count: u32,
    pub is_active: bool,
    pub min_reputation: Option<i64>, // For gated groups
    pub subscription_price: Option<u64>, // For paid groups
}
```

**Compute Node**: Smart contract for group creation and discovery

```rust
// Group Registry Contract
pub fn create_group(
    name: String,
    description: String,
    group_type: GroupType,
    category: Option<String>,
) -> Result<GroupID> {
    // Store on blockchain
    // Emit GroupCreated event
    // Return group_id
}

pub fn discover_groups(
    category: Option<String>,
    group_type: Option<GroupType>,
    min_members: Option<u32>,
) -> Vec<GroupMetadata> {
    // Query blockchain for matching groups
    // Return sorted by member_count, activity
}
```

**API Endpoints**:
- `GET /api/groups?category={cat}&type={type}` - Discover groups
- `GET /api/groups/{id}` - Get group details
- `POST /api/groups` - Create group (via smart contract)
- `GET /api/groups/{id}/members` - List members

### 3. Feed Subscriptions (Storage Node + Messaging Node)

**Storage Node**: Stores subscription records

```rust
pub struct FeedSubscription {
    pub subscriber_did: String,
    pub group_id: String,
    pub subscribed_at: DateTime<Utc>,
    pub notification_preferences: NotificationPreferences,
    pub last_read_at: Option<DateTime<Utc>>,
}
```

**Messaging Node**: Gossipsub topics for notifications

```
Topic Format: "group:{group_id}"
- All group members subscribe to this topic
- When new content is published, message is broadcast
- Subscribers automatically receive notification
```

**Subscription Flow**:
```
1. User subscribes to group
   ├─ Storage Node: Create subscription record
   ├─ Compute Node: Process payment (if paid group)
   └─ Messaging Node: Subscribe to Gossipsub topic

2. New content published
   ├─ Publisher: Publish to Gossipsub topic
   ├─ Messaging Node: Broadcast to all subscribers
   └─ Subscribers: Receive notification automatically
```

### 4. Real-time Notifications (Messaging Node)

**Gossipsub Topics**:
- `group:{group_id}` - Group feed updates
- `user:{did}` - Direct user notifications
- `presence:global` - User presence updates

**Notification Types**:
```rust
pub enum FeedNotification {
    NewContent {
        group_id: String,
        content_id: String,
        content_type: ContentType,
        publisher_did: String,
        timestamp: u64,
    },
    GroupUpdate {
        group_id: String,
        update_type: GroupUpdateType,
        data: serde_json::Value,
    },
    MemberJoined {
        group_id: String,
        member_did: String,
    },
}
```

## 🔄 User Flow

### Creating a Public Group

```
1. User calls: POST /api/groups
   ├─ Name: "SpaceKit Developers"
   ├─ Type: Public
   ├─ Category: Technology
   └─ Description: "Discussion about SpaceKit development"

2. Storage Node:
   ├─ Store group metadata
   ├─ Create group record
   └─ Return group_id

3. Compute Node:
   ├─ Deploy group registry contract
   ├─ Register group on blockchain
   └─ Emit GroupCreated event

4. Messaging Node:
   ├─ Create Gossipsub topic: "group:{group_id}"
   └─ Creator automatically subscribed
```

### Discovering Public Groups

```
1. User calls: GET /api/groups?category=Technology&type=Public

2. Storage Node:
   ├─ Query groups by category and type
   ├─ Filter by active status
   └─ Return sorted by member_count

3. Compute Node (optional):
   ├─ Query blockchain for additional metadata
   └─ Verify group authenticity

4. UI displays:
   ├─ List of public groups
   ├─ Member counts
   ├─ Categories
   └─ Join buttons
```

### Subscribing to a Group Feed

```
1. User clicks "Subscribe" on a group

2. Storage Node:
   ├─ Create FeedSubscription record
   └─ Store subscription preferences

3. Compute Node:
   ├─ Verify access (public/private/gated)
   ├─ Process payment (if paid group)
   └─ Grant access on-chain

4. Messaging Node:
   ├─ Subscribe to Gossipsub topic: "group:{group_id}"
   └─ Start receiving notifications

5. User receives:
   ├─ Confirmation notification
   └─ Access to group feed
```

### Receiving Feed Updates

```
1. Publisher posts new content to group

2. Storage Node:
   ├─ Store content metadata
   └─ Update group activity timestamp

3. Messaging Node:
   ├─ Publish FeedNotification to topic "group:{group_id}"
   └─ Gossipsub broadcasts to all subscribers

4. Subscribers:
   ├─ Receive notification automatically
   ├─ Display in feed UI
   └─ Mark as unread if not viewed
```

## 🔐 Access Control

### Public Groups
- ✅ Anyone can discover and join
- ✅ No payment required
- ✅ No reputation requirement

### Private Groups
- ✅ Invitation-only
- ✅ Not discoverable in public search
- ✅ Access controlled by group admins

### Gated Groups
- ✅ Discoverable but require:
   - Minimum reputation score
   - Payment (one-time or subscription)
   - Admin approval

## 📊 Data Synchronization

### Cross-Node Sync

**Storage Node Sync**:
- All nodes query the same Storage Node for users/groups
- Storage Node is the source of truth
- Nodes cache locally for performance

**Messaging Node Sync**:
- Gossipsub automatically syncs across connected nodes
- Messages propagate via P2P network
- No central server needed

**Compute Node Sync**:
- Blockchain is source of truth for memberships
- All nodes query same blockchain
- Smart contracts ensure consistency

## 🎯 Implementation Plan

### Phase 1: Global User Registry
1. ✅ Add `GlobalUser` struct to Storage Node
2. ✅ Create API endpoints for user registration/query
3. ✅ Update Messaging Node to register users in Storage Node
4. ✅ Implement user presence updates

### Phase 2: Group Discovery
1. ✅ Add `GlobalGroup` struct to Storage Node
2. ✅ Create Group Registry smart contract
3. ✅ Implement group creation API
4. ✅ Implement group discovery API
5. ✅ Add group categories and filtering

### Phase 3: Feed Subscriptions
1. ✅ Add `FeedSubscription` struct to Storage Node
2. ✅ Implement subscription API
3. ✅ Create Gossipsub topic per group
4. ✅ Implement notification broadcasting

### Phase 4: UI Integration
1. ✅ Add group discovery UI
2. ✅ Add subscription management UI
3. ✅ Add feed view with notifications
4. ✅ Add group creation UI

## 🔄 Migration from Current System

### Users
- Current: Each Messaging Node has its own user registry
- New: All users registered in Storage Node
- Migration: Register existing users in Storage Node

### Groups
- Current: Groups only exist in Messaging Node memory
- New: Groups stored in Storage Node + blockchain
- Migration: Export groups and register in new system

## 📝 API Examples

### Register User
```bash
POST /api/users/register
{
  "did": "did:spacekit:user:alice",
  "username": "alice",
  "display_name": "Alice Developer",
  "public_key": "...",
  "encryption_algorithm": "Kyber768"
}
```

### Discover Groups
```bash
GET /api/groups?category=Technology&type=Public&min_members=10
```

### Subscribe to Group
```bash
POST /api/groups/{group_id}/subscribe
{
  "subscriber_did": "did:spacekit:user:alice",
  "notification_preferences": {
    "notify_on_content": true,
    "notify_on_member_join": false
  }
}
```

### Get Feed Updates
```bash
GET /api/groups/{group_id}/feed?since={timestamp}
```

## 🎉 Benefits

1. **Global Visibility**: All users visible across all nodes
2. **Discoverable Groups**: Public groups can be found by anyone
3. **Real-time Notifications**: Instant updates via Gossipsub
4. **Scalable**: P2P distribution handles thousands of subscribers
5. **Decentralized**: No central server required
6. **Trustless**: Smart contracts ensure access control

