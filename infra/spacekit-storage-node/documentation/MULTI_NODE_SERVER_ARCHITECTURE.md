# Multi-Node Server Architecture - Production Design

## 🎯 Overview

Production-ready architecture where:
- Each SpaceKit OS desktop instance is a **node** on the network
- Users can **create their own server** (node) or **join other servers**
- Global discovery of servers, users, and groups
- Cross-server communication and synchronization
- Deployed on AWS and Google Cloud

## 🏗️ Architecture

### Node Types

```
┌─────────────────────────────────────────────────────────────┐
│                    Node Architecture                        │
└─────────────────────────────────────────────────────────────┘

1. Storage Node (Global Registry)
   ├─ Stores: Users, Servers, Groups, Memberships
   ├─ Provides: Discovery API, Query Interface
   └─ Deployment: AWS RDS / Google Cloud SQL

2. Compute Node (Smart Contracts)
   ├─ Group Registry Contract
   ├─ Server Registry Contract
   ├─ Membership Contracts
   └─ Deployment: AWS ECS / Google Cloud Run

3. Messaging Node (Per Server)
   ├─ Each server runs its own Messaging Node
   ├─ Gossipsub for cross-server communication
   └─ P2P network for message routing

4. User Nodes (SpaceKit OS Desktop)
   ├─ Each desktop = one node
   ├─ Can create server or join existing
   └─ Connects to Storage/Compute nodes
```

### Server Model

```
Server (Node)
├─ Server ID: Unique identifier
├─ Server Name: Display name
├─ Owner DID: Creator's DID
├─ Server Type: Public, Private, Invite-Only
├─ Endpoint: P2P address (multiaddr)
├─ Messaging Node: Running on this server
├─ Groups: Groups hosted on this server
└─ Members: Users who joined this server
```

## 📊 Data Models

### Server Registry

```rust
// In Storage Node database
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Server {
    pub id: String,                    // Unique server ID
    pub name: String,                  // Server display name
    pub description: Option<String>,   // Server description
    pub owner_did: String,             // Creator's DID
    pub server_type: ServerType,       // Public, Private, Invite-Only
    pub endpoint: String,              // P2P multiaddr
    pub messaging_port: u16,           // Messaging node port
    pub created_at: DateTime<Utc>,
    pub member_count: u32,
    pub group_count: u32,
    pub is_active: bool,
    pub region: Option<String>,        // AWS/GCP region
    pub tags: Vec<String>,             // Categories/tags
    pub max_members: Option<u32>,      // Member limit
    pub min_reputation: Option<i64>,   // Reputation requirement
}
```

### Global User (Extended)

```rust
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GlobalUser {
    pub did: String,
    pub username: String,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub public_key: Vec<u8>,
    pub encryption_algorithm: String,
    pub created_at: DateTime<Utc>,
    pub last_seen: Option<DateTime<Utc>>,
    pub is_online: bool,
    pub reputation_score: Option<i64>,
    pub home_server_id: Option<String>,  // User's primary server
    pub joined_servers: Vec<String>,     // Server IDs user joined
}
```

### Global Group (Extended)

```rust
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GlobalGroup {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub creator_did: String,
    pub server_id: String,              // Server hosting this group
    pub group_type: GroupType,         // Public, Private, Gated
    pub category: Option<String>,
    pub created_at: DateTime<Utc>,
    pub member_count: u32,
    pub is_active: bool,
    pub min_reputation: Option<i64>,
    pub subscription_price: Option<u64>,
}
```

### Server Membership

```rust
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServerMembership {
    pub server_id: String,
    pub user_did: String,
    pub role: ServerRole,              // Owner, Admin, Moderator, Member
    pub joined_at: DateTime<Utc>,
    pub invited_by: Option<String>,    // DID of inviter
}
```

## 🔄 User Flows

### Creating a Server

```
1. User clicks "Create Server" in SpaceKit OS

2. SpaceKit OS:
   ├─ Generate server ID
   ├─ Start Messaging Node on local port
   ├─ Get P2P multiaddr
   └─ Register server in Storage Node

3. Storage Node:
   ├─ Store server metadata
   ├─ Create ServerMembership (user as Owner)
   └─ Return server_id

4. Compute Node:
   ├─ Deploy Server Registry contract
   ├─ Register server on blockchain
   └─ Emit ServerCreated event

5. Messaging Node:
   ├─ Start Gossipsub
   ├─ Subscribe to server topics
   └─ Begin accepting connections

6. User's desktop becomes a server node
```

### Joining a Server

```
1. User browses public servers or receives invite

2. User clicks "Join Server"

3. Storage Node:
   ├─ Verify server exists and is joinable
   ├─ Check access (public/private/invite-only)
   ├─ Create ServerMembership
   └─ Return server endpoint

4. Compute Node:
   ├─ Verify membership on-chain (if gated)
   ├─ Process payment (if paid server)
   └─ Grant access

5. SpaceKit OS:
   ├─ Connect to server's Messaging Node
   ├─ Subscribe to server's Gossipsub topics
   └─ Sync groups and members

6. User can now:
   ├─ See server's groups
   ├─ Join groups on server
   ├─ Message other members
   └─ Receive notifications
```

### Cross-Server Communication

```
User on Server A → Message → User on Server B

1. User A sends message
   ├─ Local Messaging Node (Server A)
   ├─ Check if recipient on same server
   └─ If not, route via P2P

2. P2P Routing:
   ├─ Query Storage Node for recipient's server
   ├─ Get server endpoint
   ├─ Connect to Server B's Messaging Node
   └─ Deliver message

3. Server B's Messaging Node:
   ├─ Receive message
   ├─ Route to recipient
   └─ Deliver notification
```

## 🌐 Production Deployment

### AWS Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    AWS Deployment                        │
└─────────────────────────────────────────────────────────┘

Storage Node:
├─ RDS PostgreSQL (Multi-AZ)
├─ ElastiCache (Redis) for caching
├─ S3 for file storage
└─ CloudFront for CDN

Compute Node:
├─ ECS Fargate (WASM execution)
├─ Application Load Balancer
└─ CloudWatch for monitoring

Messaging Node (Per Server):
├─ EC2 instances (user servers)
├─ ECS tasks (managed servers)
└─ Auto Scaling Groups

Discovery:
├─ Route 53 for DNS
├─ Service Discovery (ECS)
└─ Cloud Map for service registry
```

### Google Cloud Architecture

```
┌─────────────────────────────────────────────────────────┐
│                 Google Cloud Deployment                 │
└─────────────────────────────────────────────────────────┘

Storage Node:
├─ Cloud SQL (PostgreSQL, High Availability)
├─ Memorystore (Redis) for caching
├─ Cloud Storage for files
└─ Cloud CDN for content delivery

Compute Node:
├─ Cloud Run (WASM execution)
├─ Cloud Load Balancer
└─ Cloud Monitoring

Messaging Node (Per Server):
├─ Compute Engine VMs (user servers)
├─ Cloud Run (managed servers)
└─ Managed Instance Groups

Discovery:
├─ Cloud DNS
├─ Service Directory
└─ Cloud Endpoints
```

## 🔐 Security & Access Control

### Server Access

**Public Server**:
- ✅ Discoverable in public directory
- ✅ Anyone can join
- ✅ No invitation required

**Private Server**:
- ❌ Not discoverable publicly
- ✅ Invitation-only
- ✅ Owner/Admin can invite

**Gated Server**:
- ✅ Discoverable but requires:
   - Minimum reputation
   - Payment (one-time or subscription)
   - Admin approval

### Group Access

Groups inherit server access but can have additional restrictions:
- Server members can see public groups
- Private groups require invitation
- Gated groups require payment/reputation

## 📡 API Endpoints

### Server Management

```rust
// Server Discovery
GET  /api/servers                    // List public servers
GET  /api/servers/{id}               // Get server details
GET  /api/servers/search?q={query}   // Search servers

// Server Creation
POST /api/servers                    // Create new server
PUT  /api/servers/{id}               // Update server settings
DELETE /api/servers/{id}             // Delete server

// Server Membership
POST /api/servers/{id}/join          // Join server
DELETE /api/servers/{id}/leave       // Leave server
GET  /api/servers/{id}/members       // List members
POST /api/servers/{id}/invite        // Invite user

// Server Groups
GET  /api/servers/{id}/groups        // List groups on server
POST /api/servers/{id}/groups        // Create group on server
```

### User Management

```rust
// User Registration
POST /api/users/register             // Register global user
GET  /api/users/{did}                // Get user profile
PUT  /api/users/{did}                // Update profile
PUT  /api/users/{did}/presence       // Update presence

// User Servers
GET  /api/users/{did}/servers        // User's servers
GET  /api/users/{did}/home-server    // User's home server
```

### Group Management

```rust
// Group Discovery
GET  /api/groups                     // Discover groups (all servers)
GET  /api/groups?server_id={id}      // Groups on specific server
GET  /api/groups/{id}                 // Get group details

// Group Operations
POST /api/groups                      // Create group
POST /api/groups/{id}/join            // Join group
GET  /api/groups/{id}/members         // List members
```

## 🔄 Synchronization

### Cross-Server Sync

**Storage Node Sync**:
- All servers query same Storage Node
- Storage Node is source of truth
- Servers cache locally for performance

**Messaging Node Sync**:
- Gossipsub topics per server: `server:{server_id}`
- Gossipsub topics per group: `group:{group_id}`
- Messages propagate via P2P network

**Compute Node Sync**:
- Blockchain is source of truth for memberships
- All servers query same blockchain
- Smart contracts ensure consistency

### Server Discovery

```
1. Server starts up
   ├─ Register in Storage Node
   ├─ Register in Compute Node (blockchain)
   └─ Announce via Gossipsub topic: "servers:announce"

2. Other servers/nodes:
   ├─ Subscribe to "servers:announce"
   ├─ Receive server announcements
   ├─ Query Storage Node for details
   └─ Cache server list locally

3. Users browsing servers:
   ├─ Query Storage Node API
   ├─ Filter by type/category
   └─ Display in UI
```

## 🚀 Implementation Phases

### Phase 1: Server Infrastructure (Week 1-2)
- ✅ Add Server model to Storage Node
- ✅ Create Server Registry smart contract
- ✅ Implement server creation API
- ✅ Implement server discovery API
- ✅ Update SpaceKit OS to create/join servers

### Phase 2: Cross-Server Communication (Week 3)
- ✅ P2P routing between servers
- ✅ Cross-server message delivery
- ✅ Server membership sync
- ✅ Presence updates across servers

### Phase 3: Group System (Week 4)
- ✅ Groups per server
- ✅ Cross-server group discovery
- ✅ Group membership sync
- ✅ Feed subscriptions

### Phase 4: Production Deployment (Week 5-6)
- ✅ AWS deployment configuration
- ✅ Google Cloud deployment configuration
- ✅ High availability setup
- ✅ Monitoring and logging
- ✅ Load testing

## 📊 Database Schema

### Storage Node Tables

```sql
-- Servers
CREATE TABLE servers (
    id VARCHAR(255) PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    description TEXT,
    owner_did VARCHAR(255) NOT NULL,
    server_type VARCHAR(50) NOT NULL,
    endpoint VARCHAR(255) NOT NULL,
    messaging_port INTEGER NOT NULL,
    created_at TIMESTAMP NOT NULL,
    member_count INTEGER DEFAULT 0,
    group_count INTEGER DEFAULT 0,
    is_active BOOLEAN DEFAULT true,
    region VARCHAR(50),
    tags TEXT[], -- Array of tags
    max_members INTEGER,
    min_reputation BIGINT
);

-- Server Memberships
CREATE TABLE server_memberships (
    server_id VARCHAR(255) NOT NULL,
    user_did VARCHAR(255) NOT NULL,
    role VARCHAR(50) NOT NULL,
    joined_at TIMESTAMP NOT NULL,
    invited_by VARCHAR(255),
    PRIMARY KEY (server_id, user_did)
);

-- Global Users
CREATE TABLE global_users (
    did VARCHAR(255) PRIMARY KEY,
    username VARCHAR(255) NOT NULL,
    display_name VARCHAR(255),
    avatar_url VARCHAR(500),
    public_key BYTEA NOT NULL,
    encryption_algorithm VARCHAR(50) NOT NULL,
    created_at TIMESTAMP NOT NULL,
    last_seen TIMESTAMP,
    is_online BOOLEAN DEFAULT false,
    reputation_score BIGINT,
    home_server_id VARCHAR(255),
    joined_servers TEXT[] -- Array of server IDs
);

-- Global Groups
CREATE TABLE global_groups (
    id VARCHAR(255) PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    description TEXT,
    creator_did VARCHAR(255) NOT NULL,
    server_id VARCHAR(255) NOT NULL,
    group_type VARCHAR(50) NOT NULL,
    category VARCHAR(100),
    created_at TIMESTAMP NOT NULL,
    member_count INTEGER DEFAULT 0,
    is_active BOOLEAN DEFAULT true,
    min_reputation BIGINT,
    subscription_price BIGINT
);

-- Indexes
CREATE INDEX idx_servers_type ON servers(server_type);
CREATE INDEX idx_servers_owner ON servers(owner_did);
CREATE INDEX idx_servers_active ON servers(is_active);
CREATE INDEX idx_server_memberships_user ON server_memberships(user_did);
CREATE INDEX idx_global_groups_server ON global_groups(server_id);
CREATE INDEX idx_global_groups_type ON global_groups(group_type);
```

## 🎯 Production Considerations

### Scalability
- **Storage Node**: Use read replicas for query load
- **Messaging Nodes**: Horizontal scaling per server
- **Compute Node**: Auto-scaling based on contract execution

### High Availability
- **Multi-AZ deployment** for Storage Node
- **Load balancers** for API endpoints
- **Health checks** and auto-recovery

### Monitoring
- **CloudWatch / Cloud Monitoring** for metrics
- **Log aggregation** for debugging
- **Alerting** for critical issues

### Security
- **TLS/SSL** for all API endpoints
- **Rate limiting** to prevent abuse
- **DDoS protection** via AWS Shield / Cloud Armor
- **Encryption at rest** for database

## 🔄 Migration Path

### From Current System

1. **Export existing data**:
   - Users from Messaging Node
   - Groups from Messaging Node

2. **Register in Storage Node**:
   - Create GlobalUser records
   - Create GlobalGroup records
   - Create default server for existing users

3. **Update clients**:
   - SpaceKit OS: Connect to Storage Node
   - CLI: Connect to Storage Node
   - Migrate local data to global registry

