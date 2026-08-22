# SWTCHX Messaging Node - Access Control & Permissions Guide

## Overview

The SWTCHX messaging node implements a comprehensive **DID-based access control system** with reputation scoring, blacklists, whitelists, and role-based permissions.

---

## 🏛️ Node Types

### 1. Public Messaging Nodes

**Characteristics:**
- Anyone can join (subject to reputation)
- Open discovery via mDNS/DHT
- Reputation-based access control
- Rate limiting enforced
- Community moderation

**Use Cases:**
- Public communities
- Open-source projects
- Public forums
- General chat servers

**Configuration:**
```rust
let access_control = AccessControlManager::new(NodeType::Public);

// Set policies
access_control.update_policies(AccessPolicies {
    min_reputation: 0,           // Allow new users
    rate_limit_per_minute: 60,   // Anti-spam
    auto_ban_threshold: 5,       // Ban after 5 violations
    allow_anonymous: false,      // Require DID
    require_stake: false,        // No staking required
    ..Default::default()
}).await?;
```

**Example: Public Community Node**
```
Node: public.swtchx.community
Type: Public
Access: Anyone with DID
Moderation: Reputation-based
Discovery: Global (DHT)
```

---

### 2. Private Messaging Nodes

**Characteristics:**
- Whitelist-only access
- Invitation required
- No public discovery
- Full control by owner
- Private conversations

**Use Cases:**
- Company internal messaging
- Private teams
- Family groups
- Exclusive communities

**Configuration:**
```rust
let access_control = AccessControlManager::new(NodeType::Private);

// Add allowed users
access_control.add_to_whitelist("did:swtch:user:alice".to_string()).await?;
access_control.add_to_whitelist("did:swtch:user:bob".to_string()).await?;

// Set strict policies
access_control.update_policies(AccessPolicies {
    require_invitation: true,
    allow_anonymous: false,
    min_reputation: 100,  // Only trusted users
    ..Default::default()
}).await?;
```

**Example: Company Node**
```
Node: acme-corp-internal
Type: Private
Access: Employees only (whitelisted DIDs)
Moderation: Admin-controlled
Discovery: Invite-only
```

---

### 3. Invite-Only Nodes

**Characteristics:**
- Must receive invitation
- Granular permissions per user
- Temporary access grants
- Expiring permissions

**Use Cases:**
- Event-based chat
- Temporary collaborations
- Guest access scenarios

**Configuration:**
```rust
let access_control = AccessControlManager::new(NodeType::InviteOnly);

// Grant temporary access
access_control.grant_permissions(
    "did:swtch:user:guest".to_string(),
    UserRole::Member,
    Some("did:swtch:user:admin".to_string()),
).await?;
```

---

## 🔐 Access Control Flow

### Joining a Node

```
User attempts to connect
        │
        ▼
┌───────────────────┐
│ Check Blacklist   │ ─── Blacklisted? ──► DENY ❌
└─────────┬─────────┘
          │ Not blacklisted
          ▼
┌───────────────────┐
│ Check Node Type   │
└─────────┬─────────┘
          │
    ┌─────┴─────────────┬─────────────┐
    │                   │             │
    ▼                   ▼             ▼
┌─────────┐      ┌──────────┐  ┌────────────┐
│ Public  │      │ Private  │  │Invite-Only │
└────┬────┘      └─────┬────┘  └──────┬─────┘
     │                 │              │
     │                 ▼              ▼
     │          ┌─────────────┐  ┌──────────────┐
     │          │ Whitelisted?│  │Has Permission?│
     │          └──────┬──────┘  └───────┬──────┘
     │                 │                 │
     ▼                 ▼                 ▼
┌─────────────┐  ┌─────────┐      ┌─────────┐
│Check        │  │ Yes/No  │      │ Yes/No  │
│Reputation   │  └─────────┘      └─────────┘
└──────┬──────┘
       │
       ▼
┌─────────────┐
│ ALLOW ✅    │
└─────────────┘
```

---

## 👥 User Roles & Permissions

### Role Hierarchy

```
Owner (6)
  ├── Full control
  ├── Can promote/demote anyone
  └── Can change node type

Admin (5)
  ├── Grant permissions
  ├── Ban/unban users
  ├── Moderate all content
  └── Manage groups

Moderator (4)
  ├── Moderate messages
  ├── Warn users
  ├── Temporary bans
  └── Access reports

Trusted (3)
  ├── Invite new users
  ├── Create groups
  ├── Send messages
  └── Vote on moderation

Member (2)
  ├── Send messages
  ├── Create groups
  └── Join groups

Guest (1)
  ├── Send messages (limited)
  └── Read-only groups

Banned (0)
  └── No access
```

### Permission Matrix

| Action | Guest | Member | Trusted | Moderator | Admin | Owner |
|--------|-------|--------|---------|-----------|-------|-------|
| Send Messages | ✅ (limited) | ✅ | ✅ | ✅ | ✅ | ✅ |
| Create Groups | ❌ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Invite Users | ❌ | ❌ | ✅ | ✅ | ✅ | ✅ |
| Moderate Content | ❌ | ❌ | ❌ | ✅ | ✅ | ✅ |
| Ban Users | ❌ | ❌ | ❌ | ⏱️ (temp) | ✅ | ✅ |
| Grant Permissions | ❌ | ❌ | ❌ | ❌ | ✅ | ✅ |
| Change Node Type | ❌ | ❌ | ❌ | ❌ | ❌ | ✅ |

---

## 📊 Reputation System

### Reputation Score

**Range**: -1000 to +1000
- **-1000**: Permanent ban territory
- **-100**: Auto-ban threshold  
- **0**: Neutral (new users)
- **+100**: Trusted user
- **+500**: Highly reputed
- **+1000**: Maximum reputation

### Earning Reputation

**Positive Actions** (+points):
```rust
// Send messages (small boost)
+1 point per 100 messages

// Receive helpful votes
+10 points per helpful vote

// Successful moderation
+25 points (moderators)

// Invite active users
+15 points per invited user who stays active
```

**Negative Actions** (-points):
```rust
// Violations
Low severity:      -10 points
Medium severity:   -50 points
High severity:    -200 points
Critical:       -1000 points (instant ban)

// Spam reports
-20 points per confirmed spam report

// Rate limit violations
-5 points per violation

// Inactivity (decay)
-1 point per day of inactivity
```

### Auto-Moderation

```rust
// Automatic actions based on reputation

if reputation.score < -100 {
    → Auto-ban user
}

if reputation.spam_reports > 10 {
    → Temporary suspension
}

if reputation.violations.len() >= 5 {
    → Auto-ban
}

if reputation.score > 500 {
    → Promote to Trusted
}
```

---

## 🚫 Blacklist & Whitelist

### Blacklist (Ban List)

**Purpose**: Permanently deny access to specific DIDs

**Operations:**
```rust
// Ban a user
access_control.ban_user(
    "did:swtch:user:spammer".to_string(),
    "Repeated spam violations".to_string()
).await?;

// Check if banned
let is_banned = access_control.is_blacklisted("did:swtch:user:spammer").await;

// Unban (if needed)
access_control.unban_user("did:swtch:user:spammer").await?;

// Get all banned users
let banned = access_control.get_blacklist().await;
```

**Blacklist Persistence:**
- Stored in node's database
- Synced across node restarts
- Can be exported/imported
- Shareable between trusted nodes

### Whitelist (Allow List)

**Purpose**: Explicitly allow specific DIDs (for private nodes)

**Operations:**
```rust
// Add to whitelist
access_control.add_to_whitelist("did:swtch:user:alice".to_string()).await?;

// Remove from whitelist
access_control.remove_from_whitelist("did:swtch:user:alice").await?;

// Check whitelist
let is_allowed = access_control.is_whitelisted("did:swtch:user:alice").await;

// Get all whitelisted users
let allowed = access_control.get_whitelist().await;
```

**Whitelist Management:**
- Owner/Admin can modify
- Bulk import/export
- Invitation system integration
- DID verification required

---

## ⚡ Rate Limiting

### Purpose
Prevent spam and abuse by limiting message frequency.

### Configuration

```rust
let policies = AccessPolicies {
    rate_limit_per_minute: 60,  // Max 60 messages/minute
    ..Default::default()
};

access_control.update_policies(policies).await?;
```

### Implementation

```rust
// Check before allowing message
let can_send = access_control.check_rate_limit(
    user_did,
    messages_sent_last_minute,
).await?;

if !can_send {
    return Err("Rate limit exceeded");
}
```

### Limits by Role

| Role | Messages/Minute | Groups/Hour | Invites/Day |
|------|----------------|-------------|-------------|
| Guest | 10 | 0 | 0 |
| Member | 60 | 5 | 0 |
| Trusted | 100 | 10 | 5 |
| Moderator | 200 | Unlimited | 20 |
| Admin | Unlimited | Unlimited | Unlimited |
| Owner | Unlimited | Unlimited | Unlimited |

---

## 🎯 Usage Examples

### Example 1: Public Community Node

```rust
// Create public node
let node = MessagingNode::new(config).await?;
let access_control = AccessControlManager::new(NodeType::Public);

// Set welcoming policies
access_control.update_policies(AccessPolicies {
    min_reputation: -50,          // Allow most users
    rate_limit_per_minute: 60,
    auto_ban_threshold: 10,       // Lenient
    allow_anonymous: false,
    require_stake: false,
    ..Default::default()
}).await?;

// Anyone with a DID can join!
// Reputation system handles moderation
```

### Example 2: Company Private Node

```rust
// Create private node
let node = MessagingNode::new(config).await?;
let access_control = AccessControlManager::new(NodeType::Private);

// Add company employees
for employee_did in company_employee_dids {
    access_control.add_to_whitelist(employee_did).await?;
}

// Grant admin to IT team
access_control.grant_permissions(
    "did:swtch:corp:it-admin".to_string(),
    UserRole::Admin,
    Some("did:swtch:corp:ceo".to_string()),
).await?;

// Only whitelisted employees can access
```

### Example 3: Conference Event Node

```rust
// Create invite-only node
let node = MessagingNode::new(config).await?;
let access_control = AccessControlManager::new(NodeType::InviteOnly);

// Grant temporary access to attendees
for attendee in conference_attendees {
    let mut perms = UserPermissions::default();
    perms.did = attendee.did.clone();
    perms.role = UserRole::Member;
    perms.expires_at = Some(conference_end_date); // Auto-revoke after event
    
    access_control.grant_permissions(
        attendee.did,
        UserRole::Member,
        Some("did:swtch:conf:organizer".to_string()),
    ).await?;
}

// Access automatically expires after conference
```

### Example 4: Moderation Workflow

```rust
// User reports spam
access_control.report_spam(
    "did:swtch:user:spammer",
    "did:swtch:user:reporter",
).await?;

// Check reputation
let rep = access_control.get_reputation("did:swtch:user:spammer").await?;
println!("Reputation: {}, Spam reports: {}", rep.score, rep.spam_reports);

// If multiple reports, ban
if rep.spam_reports > 5 {
    access_control.ban_user(
        "did:swtch:user:spammer".to_string(),
        "Multiple spam reports".to_string(),
    ).await?;
}
```

---

## 🔧 Integration with MessagingNode

### Checking Access Before Actions

```rust
impl MessagingNode {
    pub async fn send_message(&self, sender_did: &str, content: String) -> Result<()> {
        // 1. Check blacklist
        if self.access_control.is_blacklisted(sender_did).await {
            return Err(anyhow!("User is banned"));
        }
        
        // 2. Check permissions
        if !self.access_control.can_perform(sender_did, Action::SendMessage).await? {
            return Err(anyhow!("No permission to send messages"));
        }
        
        // 3. Check rate limit
        let message_count = self.get_message_count_last_minute(sender_did).await?;
        if !self.access_control.check_rate_limit(sender_did, message_count).await? {
            return Err(anyhow!("Rate limit exceeded"));
        }
        
        // 4. Send message
        // ... send logic ...
        
        // 5. Update reputation (positive)
        self.access_control.record_message_sent(sender_did).await?;
        
        Ok(())
    }
}
```

---

## 🎮 Tauri Commands (For SWTCHX OS)

### Access Management Commands

```rust
#[tauri::command]
async fn ban_user(did: String, reason: String) -> Result<String, String>;

#[tauri::command]
async fn unban_user(did: String) -> Result<String, String>;

#[tauri::command]
async fn add_to_whitelist(did: String) -> Result<String, String>;

#[tauri::command]
async fn remove_from_whitelist(did: String) -> Result<String, String>;

#[tauri::command]
async fn grant_role(did: String, role: String) -> Result<String, String>;

#[tauri::command]
async fn get_user_reputation(did: String) -> Result<serde_json::Value, String>;

#[tauri::command]
async fn report_spam(offender_did: String) -> Result<String, String>;

#[tauri::command]
async fn get_access_stats() -> Result<serde_json::Value, String>;
```

### Usage in UI

```typescript
// Ban a user
await invoke("ban_user", {
    did: "did:swtch:user:spammer",
    reason: "Repeated spam"
});

// Check reputation
const rep = await invoke("get_user_reputation", {
    did: "did:swtch:user:alice"
});
console.log(`Reputation: ${rep.score}`);

// Report spam
await invoke("report_spam", {
    offenderDid: "did:swtch:user:spammer"
});
```

---

## 🏗️ Public vs Private Architecture

### Public Node Network

```
                    DHT (Global Discovery)
                            │
        ┌───────────────────┼───────────────────┐
        │                   │                   │
    Node A              Node B              Node C
  (Public)            (Public)            (Public)
        │                   │                   │
        ├───────────────────┼───────────────────┤
        │         Gossipsub Mesh                │
        └───────────────────────────────────────┘
                            │
                    Anyone can join
                  (subject to reputation)
```

**Characteristics:**
- Discoverable via DHT
- Open mDNS broadcasting
- Public peer list
- Community moderation
- Reputation-based filtering

---

### Private Node Network

```
        Company Firewall
                │
    ┌───────────┼───────────┐
    │           │           │
Node A      Node B      Node C
(Private)   (Private)   (Private)
    │           │           │
    └───────────┼───────────┘
          Employees Only
        (Whitelisted DIDs)
```

**Characteristics:**
- No public discovery
- Whitelist-only access
- mDNS limited to local network
- Admin-controlled permissions
- No reputation system (trusted network)

---

### Hybrid Public-Private

```
                Public Relay
                     │
        ┌────────────┼────────────┐
        │            │            │
  Public Zone    Gateway    Private Zone
        │            │            │
    Anyone       Verified     Employees
                  Users
```

**Example**: Company with public support channel
- Public node for customer support
- Private node for internal  
- Gateway for verified customers to internal support

---

## 🔒 Security Best Practices

### For Public Nodes

1. **Enable Reputation System**
```rust
access_control.update_policies(AccessPolicies {
    min_reputation: 0,
    auto_ban_threshold: 5,
    ..Default::default()
}).await?;
```

2. **Strict Rate Limiting**
```rust
rate_limit_per_minute: 30,  // Prevent spam floods
```

3. **Community Moderation**
```rust
// Empower trusted users
grant_permissions(user_did, UserRole::Moderator, ...).await?;
```

4. **Monitor Violations**
```rust
// Regular audit of reputation scores
let stats = access_control.get_stats().await;
```

### For Private Nodes

1. **Strict Whitelist**
```rust
// Only add verified employees/members
add_to_whitelist(verified_employee_did).await?;
```

2. **Regular Access Reviews**
```rust
// Audit whitelist monthly
// Remove inactive users
// Verify DID ownership
```

3. **No Anonymous Access**
```rust
allow_anonymous: false,
require_invitation: true,
```

4. **Logging & Audit Trail**
```rust
// Log all access attempts
// Monitor for unauthorized access
// Alert on suspicious activity
```

---

## 🌐 Public Messaging Node Deployment

### Scenario: Community Chat Server

```yaml
Node Configuration:
  Type: Public
  DID: did:swtch:community:global-chat
  Discovery: DHT + mDNS
  Port: 7000 (public)
  
Access Policies:
  Min Reputation: 0
  Rate Limit: 60/min
  Auto-ban: 5 violations
  Stake Required: No
  
Moderation:
  Admins: 3
  Moderators: 10
  Automated: Reputation-based
```

**Deployment:**
```bash
# On VPS/Cloud server
git clone https://github.com/swtch-network/swtchx-messaging-node
cd swtchx-messaging-node

# Configure as public
cat > config.json <<EOF
{
  "node_type": "Public",
  "node_did": "did:swtch:community:global-chat",
  "listen_addr": "0.0.0.0:7000",
  "enable_peer_discovery": true,
  "bootstrap_peers": [
    "/dnsaddr/bootstrap.swtchx.network"
  ],
  "access_policies": {
    "min_reputation": 0,
    "rate_limit_per_minute": 60,
    "auto_ban_threshold": 5
  }
}
EOF

# Run as service
cargo build --release --bin standalone
./target/release/swtchx-messaging-node --config config.json
```

**DNS Setup:**
```
chat.swtchx.community → Your server IP
```

**Users Connect:**
```rust
// In SWTCHX OS
let messaging_config = MessagingConfig {
    bootstrap_peers: vec![
        "/dnsaddr/chat.swtchx.community/tcp/7000".to_string()
    ],
    // ... rest
};
```

---

## 🏢 Private Node Deployment

### Scenario: Company Internal Messaging

```yaml
Node Configuration:
  Type: Private
  DID: did:swtch:corp:acmeinc-internal
  Discovery: None (invite-only)
  Port: 7000 (internal network)
  
Access Control:
  Whitelist: employees.json
  No Public Discovery: true
  Require VPN: true
  
Moderation:
  Auto: None (trusted network)
  Manual: IT admins
```

**Deployment:**
```bash
# On company internal server
cd swtchx-messaging-node

# Load employee DIDs
cat employees.json
[
  "did:swtch:corp:alice",
  "did:swtch:corp:bob",
  ...
]

# Configure as private
cat > config.json <<EOF
{
  "node_type": "Private",
  "node_did": "did:swtch:corp:acmeinc-internal",
  "listen_addr": "10.0.0.50:7000",
  "enable_peer_discovery": false,
  "whitelist_file": "employees.json",
  "require_vpn": true
}
EOF

# Run on internal network only
./target/release/swtchx-messaging-node --config config.json --internal-only
```

**Access:**
- Only from company VPN
- Whitelist verified against HR database
- No external access possible

---

## 📝 Configuration Files

### Public Node Config

```json
{
  "node_type": "Public",
  "node_did": "did:swtch:community:mychat",
  "listen_addr": "0.0.0.0:7000",
  "enable_peer_discovery": true,
  "enable_dht": true,
  "bootstrap_peers": [
    "/dnsaddr/bootstrap.swtchx.network/tcp/7000"
  ],
  "access_policies": {
    "min_reputation": 0,
    "rate_limit_per_minute": 60,
    "require_invitation": false,
    "auto_ban_threshold": 5,
    "allow_anonymous": false,
    "require_stake": false
  },
  "moderation": {
    "enable_auto_mod": true,
    "spam_detection": true,
    "content_filtering": true
  }
}
```

### Private Node Config

```json
{
  "node_type": "Private",
  "node_did": "did:swtch:private:myteam",
  "listen_addr": "0.0.0.0:7000",
  "enable_peer_discovery": false,
  "whitelist_file": "./whitelist.json",
  "access_policies": {
    "min_reputation": 100,
    "rate_limit_per_minute": 200,
    "require_invitation": true,
    "allow_anonymous": false,
    "require_stake": false
  },
  "moderation": {
    "enable_auto_mod": false,
    "admin_dids": [
      "did:swtch:private:admin1",
      "did:swtch:private:admin2"
    ]
  }
}
```

---

## 🔐 DID-Based Access

### How DIDs Provide Access

```
User's DID: did:swtch:user:alice-12345
                │
                ▼
        ┌───────────────┐
        │ DID Document  │
        │ - Public Key  │
        │ - Services    │
        │ - Auth        │
        └───────┬───────┘
                │
                ▼
    ┌─────────────────────┐
    │ Access Check        │
    │ 1. Verify signature │
    │ 2. Check blacklist  │
    │ 3. Check whitelist  │
    │ 4. Check reputation │
    │ 5. Check permissions│
    └─────────┬───────────┘
              │
              ▼
        ALLOW / DENY
```

### DID Verification Process

1. **User Presents DID** - `did:swtch:user:alice`
2. **Resolve DID Document** - Get public key
3. **Verify Signature** - Prove ownership
4. **Check Access Control** - Blacklist, whitelist, reputation
5. **Grant Access** - Issue session token (if allowed)

---

## 📊 Statistics & Monitoring

### Access Statistics

```rust
let stats = access_control.get_stats().await;

println!("Node Type: {:?}", stats.node_type);
println!("Total Users: {}", stats.total_users);
println!("Whitelisted: {}", stats.whitelisted_users);
println!("Blacklisted: {}", stats.blacklisted_users);
println!("Avg Reputation: {:.2}", stats.average_reputation);
```

### Monitoring Endpoints

```bash
# Get node access info
curl http://localhost:7000/api/access/stats

# Get reputation for DID
curl http://localhost:7000/api/access/reputation/did:swtch:user:alice

# Get blacklist (admin only)
curl -H "Authorization: Bearer <admin-token>" \
     http://localhost:7000/api/access/blacklist
```

---

## 🚀 Next Steps

### Implementation Checklist

- [x] Access control module created
- [x] Reputation system implemented
- [x] Blacklist/whitelist support
- [x] Role-based permissions
- [ ] Integrate with MessagingNode
- [ ] Add Tauri commands
- [ ] Create admin UI
- [ ] Add stake requirement support
- [ ] Behavioral crypto integration

### Future Enhancements

1. **Stake-Based Access** (swtchx-staking integration)
2. **Behavioral Reputation** (swtchx-behavioral integration)
3. **Decentralized Moderation** (DAO governance)
4. **Cross-Node Reputation** (shared blacklists)
5. **AI-Powered Moderation** (content filtering)

---

**Status**: Access control system implemented ✅  
**Next**: Integration with MessagingNode  
**Documentation**: Complete

