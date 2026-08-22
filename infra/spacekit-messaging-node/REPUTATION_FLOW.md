# Reputation System - Complete Flow Documentation

## 📍 Where Reputation is Updated

Reputation is updated **automatically throughout the messaging flow** in the `MessagingNode`. Here's the complete flow:

---

## 🔄 Complete Message Flow with Reputation

### 1. User Registration

```
User calls: register_user(did, username, public_key, algorithm)
                │
                ▼
┌───────────────────────────────────────┐
│ MessagingNode::register_user()        │
├───────────────────────────────────────┤
│ 1. Check blacklist                    │ ← access_control.is_blacklisted()
│    └─ Banned? → DENY ❌               │
│                                       │
│ 2. Check node access                  │ ← access_control.has_access()
│    └─ Private node + not whitelisted? │
│        → DENY ❌                       │
│                                       │
│ 3. Register user in message_handler   │
│                                       │
│ 4. Grant default permissions          │ ← access_control.grant_permissions()
│    └─ Role: Member                    │    (UserRole::Member)
│                                       │
│ 5. Initialize reputation              │ ← access_control.get_reputation()
│    └─ Creates ReputationScore         │    (score: 0, new user)
│        with score = 0                 │
│                                       │
│ 6. Update node stats                  │
│                                       │
│ 7. Return User object ✅              │
└───────────────────────────────────────┘
```

**Location**: `swtchx-messaging-node/src/lib.rs:213-250`

---

### 2. Sending a Message

```
User calls: send_direct_message(sender_id, recipient_did, content)
                │
                ▼
┌───────────────────────────────────────┐
│ MessagingNode::send_direct_message()  │
├───────────────────────────────────────┤
│ 1. Get sender's DID                   │
│                                       │
│ 2. Check access control               │ ← access_control.has_access()
│    └─ Banned/No permission? → DENY ❌ │
│                                       │
│ 3. Check send permission              │ ← access_control.can_perform()
│    └─ Can send messages? → Yes/No     │    (Action::SendMessage)
│                                       │
│ 4. Send message via message_handler   │
│                                       │
│ 5. Update node stats                  │
│                                       │
│ 6. Update reputation ✨               │ ← access_control.record_message_sent()
│    └─ Increments total_messages       │    +1 reputation per 100 messages
│        Checks for reputation boost    │
│                                       │
│ 7. Broadcast events                   │
│                                       │
│ 8. Return events ✅                   │
└───────────────────────────────────────┘
```

**Location**: `swtchx-messaging-node/src/lib.rs:306-347`

---

### 3. Reporting Spam

```
User calls: report_spam(offender_did, reporter_did)
                │
                ▼
┌───────────────────────────────────────┐
│ AccessControlManager::report_spam()   │
├───────────────────────────────────────┤
│ 1. Create violation record            │
│    └─ Type: Spam                      │
│    └─ Severity: Medium                │
│    └─ Reported by: reporter_did       │
│                                       │
│ 2. Record violation                   │ ← record_violation()
│    └─ Add to violations list          │
│    └─ Apply penalty: -50 points       │
│                                       │
│ 3. Increment spam_reports counter     │
│    └─ spam_reports += 1               │
│                                       │
│ 4. Check auto-ban threshold           │
│    └─ If violations >= 5 → BAN        │
│                                       │
│ 5. Update last_updated timestamp      │
│                                       │
│ 6. Return success ✅                  │
└───────────────────────────────────────┘
```

**Location**: `swtchx-messaging-node/src/access_control.rs:270-290`

---

### 4. Manual Reputation Update (Admin)

```
Admin calls: update_reputation(did, delta, reason)
                │
                ▼
┌───────────────────────────────────────┐
│AccessControlManager::update_reputation│
├───────────────────────────────────────┤
│ 1. Get user's reputation score        │
│                                       │
│ 2. Apply delta                        │
│    └─ score += delta                  │
│    └─ Can be positive or negative     │
│                                       │
│ 3. Update timestamp                   │
│    └─ last_updated = now()            │
│                                       │
│ 4. Log change                         │
│    └─ Print: "Updated reputation..."  │
│                                       │
│ 5. Check for auto-ban                 │
│    └─ If score < -100 → BAN           │
│                                       │
│ 6. Return success ✅                  │
└───────────────────────────────────────┘
```

**Location**: `swtchx-messaging-node/src/access_control.rs:245-268`

---

### 5. Banning a User

```
Admin/System calls: ban_user(did, reason)
                │
                ▼
┌───────────────────────────────────────┐
│ AccessControlManager::ban_user()      │
├───────────────────────────────────────┤
│ 1. Add to blacklist                   │
│    └─ blacklist.insert(did)           │
│                                       │
│ 2. Record critical violation          │ ← record_violation()
│    └─ Type: PolicyViolation           │
│    └─ Severity: Critical              │
│    └─ Penalty: -1000 points           │
│                                       │
│ 3. Update reputation                  │
│    └─ score -= 1000                   │
│                                       │
│ 4. Log ban                            │
│    └─ Print: "Banned user..."         │
│                                       │
│ 5. Return success ✅                  │
└───────────────────────────────────────┘
```

**Location**: `swtchx-messaging-node/src/access_control.rs:175-195`

---

## 🎯 Reputation Update Points in Code

### In MessagingNode (`src/lib.rs`)

**Line 220-222**: Registration - Check blacklist/whitelist
```rust
if self.access_control.is_blacklisted(&did).await {
    return Err(anyhow::anyhow!("Cannot register: User is banned").into());
}
```

**Line 233-237**: Registration - Grant default permissions
```rust
let _ = self.access_control.grant_permissions(
    did.clone(),
    UserRole::Member,
    None,
).await;
```

**Line 240**: Registration - Initialize reputation (score = 0)
```rust
let _ = self.access_control.get_reputation(&did).await;
```

**Line 275-282**: Message Send - Check access control
```rust
if !self.access_control.has_access(&sender_did).await? {
    return Err(...);
}
if !self.access_control.can_perform(&sender_did, Action::SendMessage).await? {
    return Err(...);
}
```

**Line 295**: Message Send - Update reputation (+1 per 100 messages)
```rust
let _ = self.access_control.record_message_sent(&sender_did).await;
```

**Line 339**: Direct Message - Update reputation
```rust
let _ = self.access_control.record_message_sent(&sender_did).await;
```

### In AccessControlManager (`src/access_control.rs`)

**Line 175-195**: Ban user (reputation -= 1000)
**Line 245-268**: Manual reputation update
**Line 270-290**: Report spam (reputation -= 50)
**Line 293-309**: Record message sent (reputation +1 per 100)

---

## 📊 Reputation Update Triggers

### Automatic (Positive) ✅

1. **Message Sent Successfully**
```
Location: MessagingNode::send_text_message() (line 295)
          MessagingNode::send_direct_message() (line 339)
Trigger: Every successful message
Update: +1 point per 100 messages
Code: access_control.record_message_sent(did)
```

2. **User Registration**
```
Location: MessagingNode::register_user() (line 240)
Trigger: New user joins
Update: Initialize at score = 0
Code: access_control.get_reputation(did)
```

### Automatic (Negative) ⚠️

1. **Spam Report**
```
Location: AccessControlManager::report_spam() (line 270)
Trigger: User reports another user for spam
Update: -50 points + violation record
Code: access_control.report_spam(offender_did, reporter_did)
```

2. **Violation Record**
```
Location: AccessControlManager::record_violation() (line 311)
Trigger: Rule violation detected
Update: -10 to -1000 based on severity
Code: access_control.record_violation(did, violation)
```

3. **Auto-Ban**
```
Location: Multiple places (triggered by low score or violation threshold)
Trigger: Score < -100 OR violations >= 5
Update: -1000 points + blacklist
Code: Automatic in update_reputation() and record_violation()
```

### Manual (Admin) 🛠️

1. **Direct Reputation Adjustment**
```
Location: AccessControlManager::update_reputation() (line 245)
Trigger: Admin command
Update: Any amount (+/- specified delta)
Code: access_control.update_reputation(did, delta, reason)
```

2. **Ban/Unban**
```
Location: AccessControlManager::ban_user() (line 175)
Trigger: Admin bans user
Update: -1000 points + blacklist
Code: access_control.ban_user(did, reason)
```

---

## 🔍 Example: Complete User Journey

### New User Joins

```
1. User calls register_user()
   ├─ Location: lib.rs:213
   ├─ Check: Blacklist (line 221)
   ├─ Check: Whitelist (line 226)
   ├─ Grant: Member role (line 233)
   └─ Create: Reputation score = 0 (line 240)

   Result: User registered with neutral reputation
```

### User Sends 100 Messages

```
2. User sends messages 1-99
   ├─ Location: lib.rs:306 (per message)
   ├─ Check: Access (line 320)
   ├─ Check: Permission (line 324)
   ├─ Send: Message (line 329)
   └─ Update: total_messages++ (line 339)
   
   Result: Messages sent, reputation still 0

3. User sends message #100
   ├─ Same flow as above
   ├─ Update: total_messages = 100
   └─ Boost: score += 1 (line 305 in access_control.rs)
   
   Result: Reputation now = 1! Small reward for activity
```

### User Gets Reported for Spam

```
4. Another user reports spam
   ├─ Location: access_control.rs:270
   ├─ Create: Violation record (Spam, Medium)
   ├─ Penalty: score -= 50
   ├─ Increment: spam_reports++
   └─ Check: Auto-ban threshold
   
   Result: Reputation = 1 - 50 = -49
```

### User Continues Good Behavior

```
5. User sends 400 more legitimate messages
   ├─ Messages 101-500 sent
   ├─ Every 100: +1 reputation
   └─ Total boost: +4 points
   
   Result: Reputation = -49 + 4 = -45 (recovering!)
```

### User Gets Banned

```
6. User violates policy again (5th violation)
   ├─ Location: access_control.rs:311 (record_violation)
   ├─ Check: violations.len() >= 5 (line 348)
   ├─ Trigger: Auto-ban (line 350)
   ├─ Add: To blacklist
   └─ Penalty: -1000 points
   
   Result: User banned, reputation = -1045
```

---

## 🎮 UI Integration Points

### Where UI Calls Reputation Updates

**From SWTCHX OS (`src-tauri/src/lib.rs`):**

**Line 610-632**: Report Spam Command
```rust
#[tauri::command]
async fn report_spam(offender_did) {
    messaging_node.access_control().report_spam(...).await?;
    // ↑ Updates reputation here
}
```

**Line 589-608**: Get Reputation Command
```rust
#[tauri::command]
async fn get_user_reputation(did) {
    let rep = messaging_node.access_control().get_reputation(&did).await?;
    // ↑ Reads current reputation
}
```

**Line 568-587**: Ban User Command
```rust
#[tauri::command]
async fn ban_user(did, reason) {
    messaging_node.access_control().ban_user(did, reason).await?;
    // ↑ Updates reputation to -1000 + blacklists
}
```

### From Messaging Node (`src/lib.rs`)

**Line 295**: Group message sent
```rust
let _ = self.access_control.record_message_sent(&sender_did).await;
// ↑ Updates reputation here (automatic)
```

**Line 339**: Direct message sent
```rust
let _ = self.access_control.record_message_sent(&sender_did).await;
// ↑ Updates reputation here (automatic)
```

**Line 240**: User registration
```rust
let _ = self.access_control.get_reputation(&did).await;
// ↑ Initializes reputation here
```

---

## 📈 Reputation Calculation Logic

### In `access_control.rs`

**Lines 293-309**: `record_message_sent()`
```rust
pub async fn record_message_sent(&self, did: &str) -> Result<()> {
    let mut reputation = self.reputation.write().await;
    
    if let Some(score) = reputation.get_mut(did) {
        score.total_messages += 1;  // Increment message count
        
        // Small reputation boost for activity (up to limit)
        if score.total_messages % 100 == 0 && score.score < 1000 {
            score.score += 1;  // ← REPUTATION UPDATED HERE
        }
    }
    
    Ok(())
}
```

**Lines 245-268**: `update_reputation()`
```rust
pub async fn update_reputation(&self, did: &str, delta: i64, reason: &str) -> Result<()> {
    let mut reputation = self.reputation.write().await;
    
    if let Some(score) = reputation.get_mut(did) {
        score.score += delta;  // ← REPUTATION UPDATED HERE
        score.last_updated = Utc::now();
        
        println!("📊 Updated reputation for {}: {} ({:+} - {})", 
                 did, score.score, delta, reason);
        
        // Auto-ban if score too low
        if score.score < -100 {
            self.ban_user(did.to_string(), "Low reputation score".to_string()).await?;
        }
    }
    
    Ok(())
}
```

**Lines 311-352**: `record_violation()`
```rust
pub async fn record_violation(&self, did: &str, violation: Violation) -> Result<()> {
    let mut reputation = self.reputation.write().await;
    
    if let Some(score) = reputation.get_mut(did) {
        // Apply reputation penalty based on severity
        let penalty = match violation.severity {
            ViolationSeverity::Low => -10,
            ViolationSeverity::Medium => -50,
            ViolationSeverity::High => -200,
            ViolationSeverity::Critical => -1000,
        };
        
        score.score += penalty;  // ← REPUTATION UPDATED HERE (negative)
        score.violations.push(violation.clone());
        score.last_updated = Utc::now();
        
        // Check auto-ban threshold
        if violation.severity == ViolationSeverity::Critical 
           || score.violations.len() >= auto_ban_threshold {
            self.ban_user(did, "Auto-ban: threshold exceeded").await?;
        }
    }
    
    Ok(())
}
```

---

## 🔄 Data Flow Diagram

```
┌─────────────────────────────────────────────────────────┐
│                    SWTCHX OS (UI)                       │
├─────────────────────────────────────────────────────────┤
│                                                         │
│  User Action:                                           │
│  ├─ Send Message                                        │
│  ├─ Report Spam                                         │
│  └─ (Admin) Ban User                                    │
│                                                         │
└────────────┬────────────────────────────────────────────┘
             │ invoke(command)
             ▼
┌─────────────────────────────────────────────────────────┐
│              Tauri Backend (lib.rs)                     │
├─────────────────────────────────────────────────────────┤
│                                                         │
│  send_direct_message()  ────────┐                       │
│  send_group_message()   ────────┤                       │
│  report_spam()          ────────┼─────► Calls           │
│  ban_user()             ────────┘      MessagingNode    │
│                                                         │
└────────────┬────────────────────────────────────────────┘
             │
             ▼
┌─────────────────────────────────────────────────────────┐
│           MessagingNode (messaging-node/src/lib.rs)     │
├─────────────────────────────────────────────────────────┤
│                                                         │
│  send_direct_message() {                                │
│    ├─ Check access (line 320)                           │
│    ├─ Send message (line 329)                           │
│    └─ record_message_sent() ────┐                       │
│  }                               │                      │
│                                  │                      │
│  register_user() {               │                      │
│    ├─ Check blacklist (line 221)│                      │
│    ├─ Grant permissions (line 233)                     │
│    └─ Initialize reputation ─────┤                      │
│  }                               │                      │
│                                  │                      │
└──────────────────────────────────┼──────────────────────┘
                                   │
                                   ▼
┌─────────────────────────────────────────────────────────┐
│    AccessControlManager (access_control.rs)             │
├─────────────────────────────────────────────────────────┤
│                                                         │
│  reputation: HashMap<String, ReputationScore>           │
│  ├─ did:swtch:user:alice → { score: 150, msgs: 15000 } │
│  ├─ did:swtch:user:bob   → { score: -20, msgs: 500 }   │
│  └─ did:swtch:user:eve   → { score: -1000, BANNED }    │
│                                                         │
│  record_message_sent(did) {                             │
│    reputation[did].total_messages++; ← UPDATE HERE     │
│    if (total_messages % 100 == 0) {                     │
│      reputation[did].score += 1;    ← UPDATE HERE     │
│    }                                                    │
│  }                                                      │
│                                                         │
│  report_spam(offender, reporter) {                      │
│    reputation[offender].score -= 50; ← UPDATE HERE     │
│    reputation[offender].spam_reports++; ← UPDATE HERE  │
│  }                                                      │
│                                                         │
│  record_violation(did, violation) {                     │
│    penalty = match severity {                           │
│      Low => -10, Medium => -50,                         │
│      High => -200, Critical => -1000                    │
│    };                                                   │
│    reputation[did].score += penalty; ← UPDATE HERE      │
│    reputation[did].violations.push(violation);          │
│  }                                                      │
│                                                         │
└─────────────────────────────────────────────────────────┘
```

---

## 💾 Persistence

### Where Reputation is Stored

**In Memory:**
```rust
// AccessControlManager holds HashMap
Arc<RwLock<HashMap<String, ReputationScore>>>

// Lives in MessagingNode
messaging_node.access_control.reputation
```

**On Disk** (Future):
```rust
// Will be persisted to:
./data/messaging/reputation.db

// Using sqlite or similar
// Loaded on node start
// Saved periodically
```

---

## 🎯 Answer to Your Question

### "Where is reputation updated?"

**Answer**: Reputation is updated in **multiple places**:

1. **MessagingNode** (`src/lib.rs`):
   - Line 295: After sending group message
   - Line 339: After sending direct message
   - Line 240: When user registers (initialization)

2. **AccessControlManager** (`src/access_control.rs`):
   - Line 305: In `record_message_sent()` (automatic)
   - Line 256: In `update_reputation()` (manual/admin)
   - Line 330: In `record_violation()` (penalties)
   - Line 283: In `report_spam()` (spam reports)
   - Line 185: In `ban_user()` (ban = -1000)

3. **Tauri Commands** (`src-tauri/src/lib.rs`):
   - Line 610: `report_spam` command (UI → Backend)
   - Line 589: `get_user_reputation` command (read-only)
   - Line 568: `ban_user` command (admin action)

### "Is it in the simulator?"

**Answer**: No! Reputation is managed in the **MessagingNode** itself, which is embedded in the simulator but is a separate component.

**Flow**:
```
Simulator
  └─ Contains: MessagingNode
       └─ Contains: AccessControlManager
            └─ Manages: Reputation HashMap

When you call messaging functions, they automatically update reputation.
The simulator doesn't touch reputation - it's all in MessagingNode!
```

---

## 🧪 Testing Reputation

```typescript
// 1. Register user (reputation = 0)
await invoke("register_messaging_user", { username: "alice" });

// 2. Check reputation
let rep = await invoke("get_user_reputation", { 
    did: "did:swtch:user:alice" 
});
console.log(rep.score); // 0

// 3. Send 100 messages
for (let i = 0; i < 100; i++) {
    await invoke("send_direct_message", { ...});
}

// 4. Check reputation again
rep = await invoke("get_user_reputation", { 
    did: "did:swtch:user:alice" 
});
console.log(rep.score); // 1 (+1 for 100 messages)

// 5. Get reported for spam
await invoke("report_spam", { 
    offenderDid: "did:swtch:user:alice" 
});

// 6. Check reputation
rep = await invoke("get_user_reputation", { 
    did: "did:swtch:user:alice" 
});
console.log(rep.score); // -49 (1 - 50)
```

---

## 📝 Summary

**Reputation updates happen automatically**:
- ✅ Every message sent: Small boost
- ✅ Every spam report: Large penalty
- ✅ Every violation: Scaled penalty
- ✅ Auto-ban at -100 score

**Location**:
- Primary: `MessagingNode` (checks + updates)
- Storage: `AccessControlManager` (HashMap in memory)
- Future: Persisted to database

**Not in simulator**: The simulator just contains the MessagingNode, but doesn't manage reputation itself.

---

**Status**: ✅ Fully integrated  
**Updates**: Automatic on every message  
**Checks**: Before every action  
**Storage**: In-memory (future: database)

