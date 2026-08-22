# Public vs Private Messaging Nodes - Complete Guide

## 🌐 Overview

SWTCHX messaging nodes can operate in **three distinct modes**, each with different access control, discovery, and security models.

---

## 📊 Comparison Table

| Feature | Public Node | Private Node | Invite-Only |
|---------|-------------|--------------|-------------|
| **Access** | Anyone (with reputation) | Whitelist only | Invitation required |
| **Discovery** | Global (DHT + mDNS) | None (hidden) | Limited (invites) |
| **Moderation** | Community + Auto | Admin-controlled | Admin-controlled |
| **Reputation** | Required | Optional | Optional |
| **Rate Limiting** | Strict | Moderate | Lenient |
| **Cost** | Free or stake-based | Free (private) | Free or paid |
| **Use Case** | Communities, forums | Companies, teams | Events, temp groups |

---

## 🌍 PUBLIC MESSAGING NODES

### What is a Public Node?

A messaging node accessible to anyone on the SWTCHX network, similar to a public Discord or Telegram server.

### Architecture

```
                Internet / Global DHT
                         │
        ┌────────────────┼────────────────┐
        │                │                │
    User A           User B           User C
    (Anyone)         (Anyone)         (Anyone)
        │                │                │
        └────────────────┼────────────────┘
                         │
                   Public Node
              (Community Chat Server)
              
Access: Anyone with DID
Discovery: Global (DHT)
Moderation: Reputation-based
```

### Configuration

```rust
use swtchx_messaging_node::{MessagingNode, MessagingConfig, NodeType};

let config = MessagingConfig {
    node_did: "did:swtch:public:community-chat".to_string(),
    listen_addr: "0.0.0.0:7000".parse().unwrap(),
    enable_peer_discovery: true,  // Global discovery
    bootstrap_peers: vec![
        "/dnsaddr/bootstrap.swtchx.network/tcp/7000".to_string(),
    ],
    // ... rest of config
};

// Create as public node
let node = MessagingNode::new_with_type(config, NodeType::Public).await?;

// Configure access policies
node.access_control().update_policies(AccessPolicies {
    min_reputation: 0,           // Allow new users
    rate_limit_per_minute: 30,   // Anti-spam
    auto_ban_threshold: 5,       // Auto-moderate
    allow_anonymous: false,      // Require DID
    require_stake: false,        // Free to use
    ..Default::default()
}).await?;

node.start().await?;
```

### Features

**Open Access:**
```rust
// Anyone can join
assert!(node.access_control().has_access("did:swtch:user:newcomer").await?);
```

**Reputation Gating:**
```rust
// But reputation matters
let rep = node.access_control().get_reputation(user_did).await?;
if rep.score < -50 {
    // Restricted or banned
}
```

**Community Moderation:**
```rust
// Users can report spam
node.access_control().report_spam(
    "did:swtch:user:spammer",
    "did:swtch:user:reporter"
).await?;

// Moderators can ban
node.access_control().ban_user(
    "did:swtch:user:bad-actor".to_string(),
    "Harassment".to_string()
).await?;
```

### Use Cases

1. **Global Community Chat**
```
Example: crypto.swtchx.community
- Open to all crypto enthusiasts
- Reputation-based moderation
- 1000+ concurrent users
- Topic-based channels
```

2. **Support Forum**
```
Example: support.myproject.io
- Public help desk
- Anyone can ask questions
- Trusted users answer
- Moderators oversee
```

3. **Open Source Project**
```
Example: dev.myproject.chat
- Developer discussions
- Code collaboration
- Public but moderated
- Contributors get higher reputation
```

---

## 🏢 PRIVATE MESSAGING NODES

### What is a Private Node?

A messaging node restricted to whitelisted DIDs only, similar to a company's internal Slack.

### Architecture

```
            Company Firewall / VPN
                      │
        ┌─────────────┼─────────────┐
        │             │             │
    Employee A    Employee B    Employee C
    (Whitelisted) (Whitelisted) (Whitelisted)
        │             │             │
        └─────────────┼─────────────┘
                      │
                Private Node
            (Company Internal Only)
            
Access: Whitelist ONLY
Discovery: None (internal only)
Moderation: Admin-controlled
```

### Configuration

```rust
let config = MessagingConfig {
    node_did: "did:swtch:corp:acme-internal".to_string(),
    listen_addr: "10.0.0.50:7000".parse().unwrap(), // Internal IP
    enable_peer_discovery: false,  // NO public discovery
    bootstrap_peers: vec![],       // No public peers
    // ... rest
};

// Create as private node
let node = MessagingNode::new_with_type(config, NodeType::Private).await?;

// Add employees to whitelist
let employees = load_employee_dids().await?;
for did in employees {
    node.access_control().add_to_whitelist(did).await?;
}

// Grant admin role to IT team
node.access_control().grant_permissions(
    "did:swtch:corp:it-admin".to_string(),
    UserRole::Admin,
    Some("did:swtch:corp:ceo".to_string()),
).await?;

node.start().await?;
```

### Features

**Whitelist-Only Access:**
```rust
// Only whitelisted DIDs allowed
let has_access = node.access_control().has_access(user_did).await?;
// Returns false unless DID in whitelist
```

**No Public Discovery:**
```
- Not advertised via mDNS
- Not in global DHT
- No public endpoints
- VPN/internal network only
```

**Admin-Controlled:**
```rust
// Admins manage whitelist
node.access_control().add_to_whitelist(new_employee_did).await?;
node.access_control().remove_from_whitelist(departed_employee_did).await?;

// Admins set policies
node.access_control().update_policies(strict_policies).await?;
```

### Use Cases

1. **Corporate Internal Chat**
```
Example: acme-corp.internal
- Employees only
- Behind company VPN
- No external access
- Compliance-ready
```

2. **Healthcare System**
```
Example: hospital-secure.messaging
- HIPAA compliant
- Medical staff only
- Audit trail required
- No public exposure
```

3. **Government/Military**
```
Example: classified.messaging
- Clearance-verified DIDs
- Classified network only
- Quantum-secure
- No internet connection
```

---

## 🎫 INVITE-ONLY NODES

### What is an Invite-Only Node?

A messaging node where users must receive explicit invitations with permissions.

### Architecture

```
        Organizers send invitations
                    │
        ┌───────────┼───────────┐
        │           │           │
  Attendee A    Attendee B   Attendee C
  (Invited)     (Invited)    (Invited)
        │           │           │
        └───────────┼───────────┘
                    │
             Invite-Only Node
          (Conference/Event Chat)
          
Access: Invitation required
Discovery: Limited (invite links)
Moderation: Organizer-controlled
```

### Configuration

```rust
let config = MessagingConfig {
    node_did: "did:swtch:event:devcon2025".to_string(),
    listen_addr: "0.0.0.0:7000".parse().unwrap(),
    enable_peer_discovery: true,  // Limited to invited
    // ...
};

let node = MessagingNode::new_with_type(config, NodeType::InviteOnly).await?;

// Grant temporary access
node.access_control().grant_permissions(
    "did:swtch:user:attendee1".to_string(),
    UserRole::Member,
    Some("did:swtch:event:organizer".to_string()),
).await?;

// Access expires after event
// (set expires_at when granting permissions)
```

### Features

**Invitation System:**
```rust
// Organizer invites user
let invitation = create_invitation(
    event_node_did,
    invitee_did,
    UserRole::Member,
    expires_at: event_end_date,
).await?;

// User accepts invitation
accept_invitation(invitation).await?;
// → Automatically grants permissions
```

**Temporary Access:**
```rust
// Permissions can expire
UserPermissions {
    expires_at: Some(DateTime::parse("2025-12-31T23:59:59Z")),
    // ... rest
}

// Auto-revoked after expiration
```

### Use Cases

1. **Conference/Event Chat**
```
Example: devcon2025.chat
- Attendees only
- Ticket verification
- Expires after event
- Session-based rooms
```

2. **Workshop Collaboration**
```
Example: workshop-q42025.collab
- Participants only
- Duration: 3 days
- Auto-cleanup after
- Facilitator-controlled
```

---

## 🔐 Access Control Mechanisms

### 1. Blacklist (Deny List)

**Purpose**: Permanently ban malicious actors

```rust
// Ban a user (applies to ALL node types)
node.access_control().ban_user(
    "did:swtch:user:badactor".to_string(),
    "Harassment and spam".to_string()
).await?;

// Blacklist is checked FIRST
// Banned users cannot access ANY groups/DMs on this node
```

**Persistence:**
- Stored in node database
- Survives restarts
- Can be exported/synced
- Shareable between trusted nodes

**Viewing:**
```rust
let banned = node.access_control().get_blacklist().await;
println!("Banned users: {:#?}", banned);
```

---

### 2. Whitelist (Allow List)

**Purpose**: Explicitly allow specific DIDs (private nodes)

```rust
// For private nodes
if node_type == NodeType::Private {
    // Only whitelisted users can access
    node.access_control().add_to_whitelist(employee_did).await?;
}

// Whitelist checked AFTER blacklist
// User must be whitelisted AND not blacklisted
```

**Management:**
```rust
// Add user
access_control.add_to_whitelist("did:swtch:corp:alice").await?;

// Remove user (e.g., employee leaves)
access_control.remove_from_whitelist("did:swtch:corp:alice").await?;

// Bulk import
for did in load_from_csv("employees.csv")? {
    access_control.add_to_whitelist(did).await?;
}
```

---

### 3. Reputation System

**Purpose**: Merit-based access and privileges

```rust
pub struct ReputationScore {
    score: i64,                    // -1000 to +1000
    total_messages: u64,
    spam_reports: u32,
    helpful_votes: u32,
    violations: Vec<Violation>,
    behavioral_score: Option<f64>, // Integration with behavioral crypto
}
```

**Reputation Progression:**
```
-1000 ──┬── Auto-banned
  -100  ├── Restricted access
     0  ├── New user (neutral)
  +100  ├── Trusted user
  +500  ├── Highly reputed
 +1000 ─┴── Maximum reputation
```

**How to Earn:**
```
+ Send messages:         +1 per 100 messages
+ Helpful votes:         +10 per vote
+ Moderate successfully: +25 points
+ Invite active users:   +15 points
```

**How to Lose:**
```
- Spam reports:          -20 per report
- Violations (low):      -10 points
- Violations (medium):   -50 points
- Violations (high):     -200 points
- Violations (critical): -1000 points (ban)
- Inactivity:            -1 per day
```

---

### 4. Role-Based Permissions

**Purpose**: Granular control over what users can do

```rust
pub enum UserRole {
    Banned = 0,      // No access
    Guest = 1,       // Read-mostly
    Member = 2,      // Standard user
    Trusted = 3,     // Can invite
    Moderator = 4,   // Can moderate
    Admin = 5,       // Full control
    Owner = 6,       // Node owner
}
```

**Grant Permissions:**
```rust
// Promote user to moderator
node.access_control().grant_permissions(
    "did:swtch:user:alice".to_string(),
    UserRole::Moderator,
    Some("did:swtch:admin:bob".to_string()), // Granted by Bob
).await?;
```

**Check Permissions:**
```rust
// Before allowing action
let can_moderate = node.access_control().can_perform(
    user_did,
    Action::Moderate
).await?;

if !can_moderate {
    return Err("Insufficient permissions");
}
```

---

## 🛡️ Security Architecture

### Public Node Security

```
User Connection Attempt
        │
        ▼
┌─────────────────┐
│ DID Verification│ ─── Invalid DID ──► DENY
└────────┬────────┘
         ▼
┌─────────────────┐
│ Blacklist Check │ ─── Banned ──► DENY
└────────┬────────┘
         ▼
┌─────────────────┐
│Reputation Check │ ─── Score < threshold ──► DENY
└────────┬────────┘
         ▼
┌─────────────────┐
│ Rate Limit Check│ ─── Exceeded ──► THROTTLE
└────────┬────────┘
         ▼
      ALLOW ✅
```

### Private Node Security

```
User Connection Attempt
        │
        ▼
┌─────────────────┐
│ DID Verification│ ─── Invalid DID ──► DENY
└────────┬────────┘
         ▼
┌─────────────────┐
│ Blacklist Check │ ─── Banned ──► DENY
└────────┬────────┘
         ▼
┌─────────────────┐
│ Whitelist Check │ ─── Not whitelisted ──► DENY
└────────┬────────┘
         ▼
┌─────────────────┐
│Permission Check │ ─── No permission ──► DENY
└────────┬────────┘
         ▼
      ALLOW ✅
```

---

## 📖 Real-World Scenarios

### Scenario 1: Open Source Project

**Setup: Public Node**
```rust
Node: did:swtch:oss:my-project
Type: Public
Access: Open to all developers
Moderation: Community-driven

// Contributors get Trusted role
if user.github_contributions > 10 {
    grant_permissions(user_did, UserRole::Trusted, ...).await?;
}

// Core team gets Moderator
for maintainer in core_team {
    grant_permissions(maintainer, UserRole::Moderator, ...).await?;
}
```

**Benefits:**
- Open collaboration
- Merit-based privileges
- Self-moderating community
- Global accessibility

---

### Scenario 2: Enterprise Internal

**Setup: Private Node**
```rust
Node: did:swtch:corp:acme
Type: Private
Access: Employees only
Discovery: None (internal network)

// Load from HR system
let employees = hr_api.get_active_employees().await?;
for employee in employees {
    add_to_whitelist(employee.did).await?;
}

// Departments get specific permissions
for (dept, members) in departments {
    let role = match dept {
        "IT" => UserRole::Admin,
        "Management" => UserRole::Moderator,
        _ => UserRole::Member,
    };
    
    for member in members {
        grant_permissions(member, role, ...).await?;
    }
}
```

**Benefits:**
- Complete control
- Compliance-ready (GDPR, HIPAA)
- Audit trail
- No data leakage

---

### Scenario 3: Paid Community

**Setup: Public Node with Stake Requirement**
```rust
Node: did:swtch:premium:exclusive-alpha
Type: Public (with stake)
Access: Anyone who stakes 100 SWTCHX tokens

let policies = AccessPolicies {
    require_stake: true,
    minimum_stake_amount: 100_000_000_000_000_000_000, // 100 SWTCHX
    min_reputation: 0,
    ..Default::default()
};

// Integration with swtchx-staking
async fn check_stake(did: &str) -> Result<bool> {
    let staking_contract = get_staking_contract().await?;
    let stake = staking_contract.get_stake(did).await?;
    Ok(stake >= minimum_stake_amount)
}
```

**Revenue Model:**
- Stake required to access
- Stakes earn rewards
- Bad actors lose stake
- Sustainable community funding

---

## 🔄 Node Type Migration

### Converting Node Types

```rust
// Start as public
let node = MessagingNode::new_with_type(config, NodeType::Public).await?;

// Later, convert to private
// (requires rebuilding with new type)
let node = MessagingNode::new_with_type(config, NodeType::Private).await?;

// Import existing user list as whitelist
for user in existing_users {
    node.access_control().add_to_whitelist(user.did).await?;
}
```

**Best Practices:**
- Announce migration to users
- Export data before conversion
- Test in staging environment
- Provide migration period

---

## 🎮 Admin Operations

### Managing Public Node

```rust
// View statistics
let stats = access_control.get_stats().await;
println!("Total users: {}", stats.total_users);
println!("Avg reputation: {}", stats.average_reputation);

// Review low reputation users
for (did, rep) in get_all_reputations().await? {
    if rep.score < -50 {
        println!("Warning: {} has low reputation", did);
    }
}

// Ban bad actors
ban_user("did:swtch:user:spammer", "Spam").await?;

// Promote good users
grant_permissions("did:swtch:user:helper", UserRole::Trusted, ...).await?;
```

### Managing Private Node

```rust
// Add new employee
add_to_whitelist("did:swtch:corp:newhire").await?;
grant_permissions("did:swtch:corp:newhire", UserRole::Member, ...).await?;

// Remove departed employee
remove_from_whitelist("did:swtch:corp:departed").await?;
revoke_permissions("did:swtch:corp:departed").await?;

// Promote to admin
grant_permissions("did:swtch:corp:senior", UserRole::Admin, ...).await?;

// Audit access
let whitelist = get_whitelist().await;
for did in whitelist {
    verify_still_employed(did).await?;
}
```

---

## 🌉 Hybrid Architectures

### Public + Private Hybrid

```
        Public Zone              Private Zone
             │                        │
    ┌────────┼────────┐      ┌────────┼────────┐
    │        │        │      │        │        │
Customer  Customer  │      Employee Employee  │
    │        │      │          │        │      │
    └────────┼──────┘          └────────┼──────┘
             │                          │
       Public Node                 Private Node
    (Support/Sales)               (Internal)
             │                          │
             └──────────┬───────────────┘
                        │
                   Gateway Node
              (Verified customers → internal)
```

**Use Case**: Company with customer support
- Public node for customer queries
- Private node for internal coordination
- Gateway for escalations to internal team

---

## 📚 API Reference

### Access Control API

```rust
// Create manager
let acl = AccessControlManager::new(NodeType::Public);

// Blacklist operations
acl.ban_user(did, reason).await?;
acl.unban_user(did).await?;
acl.is_blacklisted(did).await;
acl.get_blacklist().await;

// Whitelist operations (private nodes)
acl.add_to_whitelist(did).await?;
acl.remove_from_whitelist(did).await?;
acl.is_whitelisted(did).await;
acl.get_whitelist().await;

// Permissions
acl.grant_permissions(did, role, granted_by).await?;
acl.revoke_permissions(did).await?;
acl.get_permissions(did).await;
acl.can_perform(did, action).await?;

// Reputation
acl.get_reputation(did).await?;
acl.update_reputation(did, delta, reason).await?;
acl.record_violation(did, violation).await?;
acl.report_spam(offender_did, reporter_did).await?;
acl.record_message_sent(did).await?;

// Policies
acl.update_policies(policies).await?;
acl.get_policies().await;

// Stats
acl.get_stats().await;
```

---

## 🎯 Recommendations

### Choose Public Node When:
- ✅ Building open community
- ✅ Want maximum reach
- ✅ Trust reputation system
- ✅ Can handle moderation

### Choose Private Node When:
- ✅ Internal organization use
- ✅ Need complete control
- ✅ Compliance requirements
- ✅ Sensitive information

### Choose Invite-Only When:
- ✅ Temporary events
- ✅ Controlled growth
- ✅ Vetting process needed
- ✅ Limited duration

---

## 🔮 Future Enhancements

### Planned Features

1. **Stake-Based Access** (Q1 2026)
   - Integrate swtchx-staking
   - Proof-of-stake for entry
   - Slashing for violations

2. **Behavioral Reputation** (Q1 2026)
   - swtchx-behavioral integration
   - AI-powered scoring
   - Predictive moderation

3. **DAO Governance** (Q2 2026)
   - Community voting on bans
   - Decentralized moderation
   - Token-weighted decisions

4. **Cross-Node Reputation** (Q2 2026)
   - Shared blacklists
   - Reputation portability
   - Trusted node networks

---

## 📝 Summary

| Aspect | Public | Private | Invite-Only |
|--------|--------|---------|-------------|
| **Who can join?** | Anyone | Whitelist | Invitations |
| **Discovery?** | Global | None | Limited |
| **Moderation?** | Community | Admin | Organizer |
| **Cost?** | Free/Stake | Free | Free/Paid |
| **Best for?** | Communities | Companies | Events |

**Key Takeaway**: Choose node type based on your access control needs!

---

**Status**: Access control system implemented ✅  
**Integration**: Ready for MessagingNode  
**Documentation**: Complete  
**Next**: Add Tauri commands for UI management

