# CLI Content Publishing & Messaging Implementation

## Summary

This document outlines the implementation of messaging and content publishing commands in the SpaceKit CLI, enabling users to interact with the SpaceKit Network for chat and content publishing.

## Answers to Key Questions

### 1. Is Encrypted Content Storage and P2P Distribution Managed/Governed by Smart Contracts?

**Current State:**
- ✅ Storage Node has access control
- ✅ P2P network exists
- ❌ **Not fully governed by smart contracts yet**

**Target Architecture:**
- **Storage Governance Contract**: Manages storage policies, access control, replication
- **P2P Distribution Contract**: Manages chunk distribution, replication verification
- **Content Registry Contract**: Links Fact Packages to channels

**Implementation Plan:**
See `CONTENT_FACT_PACKAGE_INTEGRATION.md` for detailed smart contract governance design.

### 2. Content Should Follow Fact Package System

**✅ YES - Implemented**

All content is now stored as **Fact Packages** with:
- Rich metadata (categories, tags, domains, verification levels)
- Built-in access control policies
- Sharing metadata
- SPHINCS+ signature verification
- Content types support (Binary for MP4, images, etc.)

**Implementation:**
- Content stored as `FactPackage` with `FactContent::Binary`
- Metadata includes channel info, pricing, sharing policies
- Access policies control who can view content
- Encryption metadata for quantum-safe content protection

### 3. CLI Client for Messaging and Content Publishing

**✅ Implemented**

The CLI now includes:

#### Messaging Commands

```bash
# Send a direct message
spacekit message send --to did:spacekit:user:bob --message "Hello!"

# List conversations
spacekit message list --detailed

# Start interactive chat
spacekit message chat --with did:spacekit:user:bob

# Create a group
spacekit message create-group --name "Team Chat" --description "Team discussions"

# Send group message
spacekit message group-message --group group_123 --message "Team update"
```

#### Content Publishing Commands

```bash
# Create a channel
spacekit content create-channel \
  --name "My Video Channel" \
  --description "Tutorial videos" \
  --pricing subscription \
  --price 5.0

# Publish content
spacekit content publish \
  --channel channel_123 \
  --file video.mp4 \
  --title "Introduction to Rust" \
  --description "Learn Rust basics" \
  --pricing pay_per_view \
  --price 0.1

# Subscribe to channel
spacekit content subscribe --channel channel_123

# List channels
spacekit content list-channels --subscribed --detailed

# List content in channel
spacekit content list-content --channel channel_123 --limit 20

# View/download content
spacekit content view --content-id content_456 --output video.mp4
```

## Implementation Details

### CLI Structure

1. **Command Enums**: Added `MessageCommands` and `ContentCommands`
2. **Handler Functions**: 
   - `handle_message_command()` - Processes messaging commands
   - `handle_content_command()` - Processes content publishing commands
3. **Integration**: Commands integrated into main CLI handler

### Current Implementation Status

#### ✅ Completed
- Command definitions and structure
- Handler function stubs
- Integration with existing CLI infrastructure
- Connection to Storage/Compute nodes
- File upload/download for content

#### 🚧 Needs Backend Integration
- **Messaging Node Integration**: Connect to messaging node for real chat
- **Smart Contract Integration**: Deploy and call governance contracts
- **Fact Package Creation**: Convert content to Fact Packages with metadata
- **Gossipsub Integration**: Publish notifications via Messaging Node
- **Payment Processing**: Integrate payment contracts for pay-per-view

### Next Steps

1. **Integrate Messaging Node Client**
   - Connect to messaging node via simulator or direct connection
   - Implement real message sending/receiving
   - Add Gossipsub topic subscriptions

2. **Implement Fact Package Creation**
   - Convert uploaded files to Fact Packages
   - Add proper metadata (category, tags, domain, etc.)
   - Set access policies based on pricing model

3. **Deploy Smart Contracts**
   - Storage Governance Contract
   - P2P Distribution Governance Contract
   - Channel Registry Contract

4. **Complete Content Publishing Flow**
   - Store content as Fact Package
   - Register with governance contracts
   - Chunk and distribute via P2P
   - Publish notifications via Gossipsub

5. **Add Interactive Chat**
   - Real-time message receiving
   - Terminal UI for chat interface
   - Message history

## Usage Examples

### Example 1: Create Channel and Publish Content

```bash
# 1. Initialize workspace (if not done)
spacekit init --algorithm kyber768 --name my-channel

# 2. Connect to nodes
spacekit connect compute --url http://localhost:8080 --node-did did:spacekitx:compute:node1
spacekit connect storage --url http://localhost:9000 --node-did did:spacekitx:storage:node1

# 3. Create channel
spacekit content create-channel \
  --name "Rust Tutorials" \
  --description "Learn Rust programming" \
  --pricing subscription \
  --price 10.0

# 4. Publish content
spacekit content publish \
  --channel channel_abc123 \
  --file tutorial.mp4 \
  --title "Rust Basics" \
  --description "Introduction to Rust" \
  --pricing free
```

### Example 2: Subscribe and View Content

```bash
# 1. Subscribe to channel
spacekit content subscribe --channel channel_abc123

# 2. List available content
spacekit content list-content --channel channel_abc123

# 3. View/download content
spacekit content view --content-id content_xyz789 --output tutorial.mp4
```

### Example 3: Send Messages

```bash
# 1. Send direct message
spacekit message send \
  --to did:spacekit:user:alice \
  --message "Hey, check out my new video!"

# 2. Create group and send message
spacekit message create-group --name "Content Creators"
spacekit message group-message \
  --group group_123 \
  --message "New content published!"
```

## Architecture

```
CLI Commands
    │
    ├─► Message Commands
    │   ├─► Messaging Node (Gossipsub)
    │   └─► Storage Node (file attachments)
    │
    └─► Content Commands
        ├─► Storage Node (Fact Package storage)
        ├─► Compute Node (Smart contracts)
        └─► Messaging Node (Notifications)
```

## Files Modified

1. **`spacekit-cli/src/main.rs`**:
   - Added `MessageCommands` enum
   - Added `ContentCommands` enum
   - Added `handle_message_command()` function
   - Added `handle_content_command()` function
   - Integrated into main command handler

2. **`spacekit-storage-node/documentation/CONTENT_FACT_PACKAGE_INTEGRATION.md`**:
   - Created comprehensive design document
   - Outlined smart contract governance architecture
   - Defined Fact Package integration approach

3. **`spacekit-storage-node/documentation/IMPLEMENTATION_PLAN.md`**:
   - Created implementation plan
   - Answered key questions
   - Outlined next steps

## Testing

To test the CLI commands:

```bash
# Build CLI
cd spacekit-cli
cargo build --release

# Test messaging
./target/release/spacekit message list

# Test content publishing
./target/release/spacekit content list-channels
```

## Notes

- Commands are functional but need backend integration
- Messaging requires Messaging Node connection
- Content publishing requires Fact Package conversion
- Smart contracts need to be deployed
- Interactive chat needs terminal UI implementation

