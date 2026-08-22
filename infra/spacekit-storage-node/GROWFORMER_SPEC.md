# Growformer Library-Based Loader Specification

**Status:** Pre-implementation specification
**Version:** 1.0
**Owner:** SWTCH Labs
**Date:** 2026
**Audience:** Growformer team, SpaceKit CLI engineering, smart contract integration
**Supersedes:** [GROWFORMER_PUB.md](GROWFORMER_PUB.md) (retired; renamed to [CONTENT_PUBLISHING.md](CONTENT_PUBLISHING.md) for general content)

This document specifies the library-based distribution model for growformer. Growformer is refactored from a standalone binary into a Rust library that is statically compiled into the `spacekit` CLI. Users who download the CLI get growformer as part of it; entitlement checks at function entry gate whether they can invoke training, inference, or merge operations.

This approach provides the strongest IP protection available for client-side software because growformer never exists as a separate distributable binary. The growformer code is interleaved with thousands of other functions in the `spacekit` CLI binary, making extraction substantially harder than downloading a standalone executable.

Other content types (videos, datasets, third-party agents, general software) use the encrypted-envelope-with-time-bound-decryption approach in [CONTENT_PUBLISHING.md](CONTENT_PUBLISHING.md). Growformer is the specific exception that warrants this stronger protection model.

## 1. Goals and scope

### 1.1 Goals

**Strongest IP protection for growformer.** The growformer code never exists as a separable binary. Extraction requires reverse-engineering the `spacekit` CLI binary, which is a significantly higher bar than downloading a standalone executable.

**Cleaner UX for users.** Users don't need separate `spacekit content download --name growformer` step. They have growformer because they have the CLI. The entitlement step grants permission to use it; nothing additional needs to be downloaded.

**Independent versioning.** Growformer can be updated via CLI releases. New growformer versions ship by releasing a new CLI version, not by uploading new envelope files to the storage operator.

**Cryptographically enforced entitlements.** Even though growformer code is present on every CLI installation, the entitlement system controls whether users can invoke it. Without valid entitlement, growformer function calls return errors.

### 1.2 Out of scope

- General content distribution (videos, datasets, third-party software) — those use the encrypted envelope model
- Cross-platform considerations beyond what the CLI itself handles
- Third-party agents that are not SWTCH Labs IP

## 2. The architectural model

```
┌────────────────────────────────────────────────────────────────┐
│ spacekit CLI binary (single distributed binary per platform)   │
│                                                                │
│  ┌──────────────────────────────────────────────────────┐     │
│  │ growformer library (compiled in, statically linked)  │     │
│  │                                                       │     │
│  │  - encoder, decoder                                   │     │
│  │  - router, classifier                                 │     │
│  │  - training engine                                    │     │
│  │  - inference engine                                   │     │
│  │  - brain merge logic                                  │     │
│  │  - all training data, weights, model code            │     │
│  └──────────────────────────────────────────────────────┘     │
│                                                                │
│  ┌──────────────────────────────────────────────────────┐     │
│  │ Entitlement check layer                               │     │
│  │  - reads local grants database                        │     │
│  │  - queries on-chain entitlement-ledger                │     │
│  │  - validates expiration                               │     │
│  │  - gates growformer function entry                    │     │
│  └──────────────────────────────────────────────────────┘     │
│                                                                │
│  ┌──────────────────────────────────────────────────────┐     │
│  │ Standard CLI commands (everything else)               │     │
│  │  - storage, network, contract, repo, workspace, etc.  │     │
│  └──────────────────────────────────────────────────────┘     │
└────────────────────────────────────────────────────────────────┘
```

The growformer library and the entitlement check layer are both part of the same `spacekit` CLI binary. Growformer functions cannot be invoked without passing through the entitlement check.

## 3. Refactoring growformer from binary to library

The current growformer binary needs to become a library crate. The refactoring:

### 3.1 What stays the same

- Internal algorithms (encoder, router, training, inference)
- Data structures (Brain, training samples, model state)
- File formats (`.gf.toml` project files, `.bin` brain files)
- Behavior at the algorithm level

### 3.2 What changes

**Current structure:** `growformer/src/main.rs` parses CLI arguments and dispatches to internal functions. The binary is a self-contained executable.

**New structure:** `growformer/src/lib.rs` exposes a public API. The internal modules remain largely unchanged; only the entry point and the argument parsing change.

```rust
// growformer/Cargo.toml — becomes a library
[package]
name = "growformer"
version = "1.0.0"

[lib]
name = "growformer"
path = "src/lib.rs"

# Keep the binary too for development/testing inside the growformer team
[[bin]]
name = "growformer-cli"
path = "src/bin/growformer_cli.rs"
required-features = ["standalone_cli"]
```

```rust
// growformer/src/lib.rs — the new library entry point
//
// Public API exposed to consumers like the spacekit CLI

pub use crate::training::train_brain;
pub use crate::inference::infer_brain;
pub use crate::merge::merge_brains;
pub use crate::brain::Brain;
pub use crate::project::Project;
pub use crate::error::GrowformerError;

mod training;
mod inference;
mod merge;
mod brain;
mod project;
mod encoder;
mod router;
mod classifier;
mod codec;
mod error;
```

The public functions take a `EntitlementContext` parameter that the spacekit CLI provides:

```rust
pub fn train_brain(
    args: TrainBrainArgs,
    entitlement: &EntitlementContext,
) -> Result<Brain, GrowformerError> {
    // Entitlement check at function entry
    if !entitlement.has_active_entitlement_for("growformer.train") {
        return Err(GrowformerError::NoEntitlement {
            capability: "train",
            tier: entitlement.tier_name.clone(),
        });
    }
    
    // Apply tier-specific quota if applicable
    entitlement.consume_quota("train")?;
    
    // Run the actual training (unchanged algorithm)
    train_brain_internal(args)
}

pub fn infer_brain(
    args: InferArgs,
    entitlement: &EntitlementContext,
) -> Result<InferResult, GrowformerError> {
    if !entitlement.has_active_entitlement_for("growformer.infer") {
        return Err(GrowformerError::NoEntitlement {
            capability: "infer",
            tier: entitlement.tier_name.clone(),
        });
    }
    
    infer_brain_internal(args)
}

pub fn merge_brains(
    args: MergeArgs,
    entitlement: &EntitlementContext,
) -> Result<Brain, GrowformerError> {
    if !entitlement.has_active_entitlement_for("growformer.merge") {
        return Err(GrowformerError::NoEntitlement {
            capability: "merge",
            tier: entitlement.tier_name.clone(),
        });
    }
    
    merge_brains_internal(args)
}
```

### 3.3 The EntitlementContext

The `EntitlementContext` is a trait or struct provided by the SpaceKit CLI:

```rust
// Defined in the growformer crate so it's part of the library API contract
pub struct EntitlementContext {
    pub user_did: DID,
    pub tier_name: String,
    pub active_capabilities: Vec<String>,
    pub expires_at: u64,
    pub quota_remaining: Option<u64>,
    pub on_chain_verified: bool,
}

impl EntitlementContext {
    pub fn has_active_entitlement_for(&self, capability: &str) -> bool {
        // Check tier permits the capability
        if !self.active_capabilities.iter().any(|c| c == capability) {
            return false;
        }
        
        // Check expiration
        let now = current_timestamp();
        if self.expires_at > 0 && self.expires_at < now {
            return false;
        }
        
        true
    }
    
    pub fn consume_quota(&self, operation: &str) -> Result<(), GrowformerError> {
        // Check quota; if exhausted, return error
        if let Some(remaining) = self.quota_remaining {
            if remaining == 0 {
                return Err(GrowformerError::QuotaExhausted);
            }
            // Operator-side persistence updates the quota count
        }
        Ok(())
    }
}
```

The CLI is responsible for constructing the `EntitlementContext` correctly before each function invocation. The growformer library trusts the context it's given but validates the checks it actually controls (expiration, capability membership).

### 3.4 Removed: standalone CLI in growformer

The current `growformer/src/main.rs` is removed. Growformer no longer has a standalone CLI that users invoke. The CLI surface for training, inference, and merge moves entirely to the spacekit CLI.

**Exception:** During development by the growformer team, a `growformer-cli` binary remains available (gated behind the `standalone_cli` Cargo feature). This is for internal team development only and not distributed to users.

### 3.5 Estimated refactoring work

- Convert `growformer` crate from binary to library structure: ~3 days
- Define the public API (`train_brain`, `infer_brain`, `merge_brains`): ~2 days
- Define and implement the `EntitlementContext`: ~2 days
- Integrate entitlement checks at function entry: ~2 days
- Move CLI argument parsing from growformer to spacekit-cli: ~3 days
- Tests for the library API: ~3 days
- Documentation: ~2 days

Total: about 2-3 weeks of focused work in the growformer codebase.

## 4. CLI integration

The spacekit CLI integrates the growformer library:

### 4.1 Dependency declaration

```toml
# spacekit-cli/Cargo.toml
[dependencies]
growformer = { path = "../growformer", version = "1.0.0" }
```

The growformer crate becomes a workspace dependency. Released spacekit CLI binaries include the compiled growformer code.

### 4.2 Command handlers

The existing `spacekit agent` commands call into growformer's library API:

```rust
// In spacekit-cli/src/full_client.rs or similar
async fn handle_agent_train(args: AgentTrainArgs, ctx: &mut Context) -> Result<()> {
    // Build entitlement context from local grants + on-chain state
    let entitlement = build_entitlement_context(ctx, "growformer").await?;
    
    if !entitlement.has_active_entitlement_for("growformer.train") {
        return Err(SpaceKitError::EntitlementRequired {
            feature: "growformer.train",
            renewal_command: "spacekit content access --name growformer",
        }.into());
    }
    
    // Convert spacekit args to growformer args
    let train_args = growformer::TrainBrainArgs {
        project_path: args.project,
        output_path: args.brain_output,
        // ... etc
    };
    
    // Call growformer library
    let brain = growformer::train_brain(train_args, &entitlement)?;
    
    // Save the result
    brain.save(&args.brain_output)?;
    
    Ok(())
}

// Similar pattern for infer and merge
```

### 4.3 The EntitlementContext builder

The CLI constructs the entitlement context by combining local grant state with on-chain verification:

```rust
async fn build_entitlement_context(
    ctx: &Context,
    feature: &str,
) -> Result<EntitlementContext> {
    let user_did = ctx.user_did();
    
    // Check local grants database
    let local_grant = ctx.grants_db.find_grant(&user_did, feature)?
        .ok_or(SpaceKitError::NoGrant)?;
    
    // Optionally verify on-chain (per user's verify_on_chain config)
    let on_chain_verified = if should_verify_on_chain(ctx) {
        let on_chain_state = ctx.entitlement_ledger
            .verify_access(&user_did, &local_grant.feature)
            .await?;
        
        match on_chain_state {
            AccessStatus::Granted { .. } => true,
            _ => return Err(SpaceKitError::OnChainEntitlementInvalid),
        }
    } else {
        false
    };
    
    // Build context
    Ok(EntitlementContext {
        user_did,
        tier_name: local_grant.tier_name,
        active_capabilities: local_grant.capabilities,
        expires_at: local_grant.expires_at,
        quota_remaining: local_grant.quota_remaining,
        on_chain_verified,
    })
}
```

### 4.4 Estimated CLI integration work

- Add growformer dependency in Cargo.toml: ~1 hour
- Refactor `handle_agent_*` commands to use library API: ~1 week
- Build entitlement context construction: ~3 days
- Tests for the integration: ~3 days
- Cross-platform build verification (the new CLI binary now includes growformer): ~2 days

Total: about 2 weeks of CLI integration work.

## 5. The user experience

After implementation:

```bash
# User downloads and installs spacekit CLI (one binary per platform)
# brew install spacekit  # or download from spacekit.xyz

# User initializes identity
spacekit init

# User obtains growformer entitlement (no download required!)
spacekit content access --name growformer
# Returns: "Growformer entitlement granted. Tier: free. Valid until: <timestamp>."

# User uses growformer — the binary is already there, just gated
spacekit agent train --project my-agent.gf.toml
# CLI checks entitlement, invokes growformer.train_brain() internally
# No file downloads, no envelope decryption, no temp files

spacekit agent infer --brain my-agent.bin --prompt "summarize this"
# Same pattern — direct library call gated by entitlement
```

The flow is simpler than the envelope-based approach. No separate `spacekit content download --name growformer` step. No platform-specific binary management. The user has growformer because they have the CLI; the entitlement controls whether they can use it.

## 6. Entitlement model

Growformer entitlements use the standard SpaceKit entitlement system but at the feature-capability level rather than the file level:

### 6.1 The FactPackage schema

A new schema specifically for library-embedded features:

```rust
// Schema: spacekit:licensed_feature:v1
struct LicensedFeature {
    schema: String,                          // "spacekit:licensed_feature:v1"
    feature_name: String,                    // "growformer"
    feature_version: String,                 // "1.0.0"
    minimum_cli_version: String,             // CLI version that contains this feature
    
    // Description
    title: String,
    description: String,
    publisher_name: String,
    
    // No envelope_file_id — there's no separate file
    // No platform_binaries — the feature is in the CLI
    
    // Capabilities the feature exposes
    capabilities: Vec<FeatureCapability>,
    
    // Tier definitions
    tiers: Vec<FeatureTier>,
    
    // On-chain references
    app_store_app_id: [u8; 32],
    license_nft_contract: Address,
    entitlement_ledger_address: Address,
    
    // Publishing metadata
    publisher_did: DID,
    storage_operator_did: DID,             // for entitlement queries
    published_at: u64,
    
    // Signature
    signature: SPHINCSSignature,
}

struct FeatureCapability {
    name: String,                          // "growformer.train", "growformer.infer", "growformer.merge"
    description: String,                   // human-readable
}

struct FeatureTier {
    name: String,                          // "free", "personal", "commercial", etc.
    license_type: LicenseType,
    price: Price,
    entitlement_duration_seconds: Option<u64>,
    grant_type: GrantType,
    eligibility: Eligibility,
    capabilities_included: Vec<String>,    // which capabilities this tier unlocks
    quota: Option<Quota>,
}
```

This schema is parallel to `spacekit:licensed_content:v1` (for downloadable content) but specifically for library-embedded features. Storage operators support both schemas.

### 6.2 Tier examples for growformer

```json
{
  "tiers": [
    {
      "name": "free",
      "license_type": "Personal",
      "price": { "amount_wei": "0", "currency": "ASTRA", "on_network": "spacekit" },
      "entitlement_duration_seconds": 2592000,
      "grant_type": "Free",
      "eligibility": "OpenToAll",
      "capabilities_included": ["growformer.train", "growformer.infer", "growformer.merge"],
      "quota": { "operations": 1000, "period_seconds": 2592000 }
    },
    {
      "name": "personal",
      "license_type": "Personal",
      "price": { "amount_wei": "20000000000", "currency": "ASTRA", "on_network": "spacekit" },
      "entitlement_duration_seconds": 2592000,
      "grant_type": "Subscription",
      "eligibility": "PaymentRequired",
      "capabilities_included": ["growformer.train", "growformer.infer", "growformer.merge"],
      "quota": null
    },
    {
      "name": "commercial",
      "license_type": "Commercial",
      "price": { "amount_wei": "200000000000", "currency": "ASTRA", "on_network": "spacekit" },
      "entitlement_duration_seconds": 2592000,
      "grant_type": "Subscription",
      "eligibility": "PaymentRequired",
      "capabilities_included": ["growformer.train", "growformer.infer", "growformer.merge"],
      "quota": null
    }
  ]
}
```

### 6.3 How entitlement enforcement works

When a user invokes `spacekit agent train`:

1. CLI queries the local grants database for an active grant for "growformer"
2. CLI constructs an `EntitlementContext` from the grant
3. CLI calls `growformer::train_brain(args, &entitlement_context)`
4. The library function checks `entitlement.has_active_entitlement_for("growformer.train")`
5. If false, returns `GrowformerError::NoEntitlement`
6. If true, checks quota (if applicable), runs training

For quota-tracked tiers, after the operation completes, the CLI updates the local grant's quota count and optionally syncs to on-chain.

## 7. What this prevents

The library-based approach defeats most extraction attempts:

**Prevented: Direct binary extraction.** There's no growformer binary to extract. The growformer code is part of the spacekit CLI binary. A user trying to extract just growformer would need to:

1. Identify which sections of the spacekit binary contain growformer code (large reverse-engineering task)
2. Extract those sections
3. Reconstruct the dependencies (every internal function growformer uses also lives in the spacekit binary)
4. Repackage as a standalone binary

This is a multi-week reverse engineering task, not a "download and copy" task. The cost-to-bypass exceeds the value of free use for the vast majority of users.

**Prevented: Republication as content.** A user can't `spacekit content publish` growformer because there's no growformer binary file to publish. Even if they tried to publish a copy of the spacekit CLI binary, the storage operator could detect this via hash comparison with the published canonical CLI binary.

**Prevented: Distribution by other operators.** Even rogue storage operators can't distribute growformer separately. They'd have to distribute the entire spacekit CLI, which is the legitimate product (and the hash check would detect this).

**Prevented: Modification of growformer logic.** Modifications to growformer in a user's copy would require modifying the spacekit binary. Modified spacekit binaries fail integrity checks against the canonical version.

## 8. What it doesn't prevent

Honest about the limits:

**Reverse engineering during a valid entitlement.** A user with active entitlement can run growformer, then dump the process memory or use debugging tools to extract function-level state. Same as any client-side software. Mitigation: terms of service, usage monitoring.

**Inspection of the spacekit binary.** A user can disassemble the spacekit CLI binary to look at growformer logic. This is harder than examining a standalone growformer binary (because the code is interleaved with other CLI functionality) but not impossible. Mitigation: code obfuscation if needed (Rust strip, etc.), licensing terms.

**Modified-CLI attacks.** A determined attacker could modify the spacekit CLI source code to remove entitlement checks, recompile, and run growformer without entitlement. Detection: legitimate users use signed binary distributions. The modified CLI's binary signature wouldn't match.

**Determined commercial reverse engineering.** A competitor with substantial resources could fully reverse engineer growformer from the spacekit binary. Mitigation: this requires significant engineering investment; competitors typically prefer to build their own from scratch.

## 9. Comparison with the encrypted envelope approach

For clarity, why growformer specifically uses library-embedded rather than encrypted envelope:

| Property | Library-embedded (growformer) | Encrypted envelope (everything else) |
|---|---|---|
| Storage location | In spacekit CLI binary | Storage operator (storage.spacekit.xyz) |
| Distribution | CLI binary distribution | `spacekit content download` |
| Entitlement check | At function entry | At decryption time |
| Extraction difficulty | High (multi-week RE) | Medium (decrypted briefly in memory) |
| Update path | New CLI release | New envelope upload |
| Coupling | Tight (CLI release coupled to growformer release) | Loose (independent update) |
| Best for | IP-critical content (growformer) | Standard content (videos, datasets, third-party software) |

Growformer's extreme IP value justifies the tight coupling and stronger protection. Other content's standard IP value works fine with the looser envelope approach.

## 10. Integration with other content models

The two distribution models coexist cleanly:

**Users of the spacekit CLI:**
- Have growformer functionality embedded (library)
- Need to download and decrypt other content (envelope)
- The `spacekit content access` command handles both: granting entitlement for growformer (no download), or granting entitlement plus downloading envelope for other content

**Operators:**
- Don't host growformer (it's in the CLI, not in storage)
- Host other content as encrypted envelopes
- Manage entitlements for both types via the same entitlement-ledger contract

**The entitlement-ledger:**
- Stores entitlements for both content (file-based) and features (library-embedded) uniformly
- The same `OP_VERIFY` works for both
- Querying patterns are the same

## 11. CLI release implications

A few things to consider about CLI releases:

**Each CLI release includes growformer.** This means CLI binaries are larger (growformer adds the size of its compiled code, probably 10-15 MB).

**Growformer version is locked to CLI version.** Users running CLI v1.0 have growformer v1.0; users running CLI v1.1 have growformer v1.1. This is intentional — keeping them in sync simplifies the system.

**Major growformer changes require new CLI releases.** Not a problem because both are SWTCH Labs products with synchronized release cycles.

**Bug fixes in growformer require new CLI releases.** Same as above — bug fixes go through the standard CLI release process.

This is a tradeoff. The library approach gives stronger IP protection at the cost of looser independent versioning. For growformer specifically, it's the right tradeoff because IP protection is paramount.

## 12. Implementation phases

**Phase 1: Growformer library refactor (2-3 weeks)**

- Convert `growformer` crate to library structure
- Define public API
- Implement `EntitlementContext` and check at function entry
- Tests of the library API in isolation

**Phase 2: CLI integration (2 weeks)**

- Add growformer dependency to spacekit-cli
- Refactor `handle_agent_*` commands to use library API
- Build entitlement context construction
- Integration tests with the full flow

**Phase 3: FactPackage schema and entitlement flow (1-1.5 weeks)**

- Implement `spacekit:licensed_feature:v1` schema
- Update storage operator to support both content and feature entitlements
- CLI commands for feature entitlements (e.g., `spacekit content access --feature growformer`)

**Phase 4: Migration and rollout (1-2 weeks)**

- Migration plan for existing growformer users
- CLI release with growformer included
- Deprecate the old growformer binary distribution path
- Documentation updates

Total: 6-8 weeks for the full library-based growformer distribution.

## 13. Migration considerations

Currently growformer can be installed locally via the content system (per the existing content_installs logs). Migrating to library-based:

**Existing installs:** Users with currently-installed growformer binaries continue to work via the old mechanism. The CLI supports both paths during the migration period.

**New installs:** New users get growformer via the library path. No separate download needed.

**Cutover:** After ~3 months, the old mechanism is deprecated. Users on legacy versions are encouraged to upgrade to the latest CLI.

The migration is graceful — no forced upgrades, no breaking changes for existing users.

## 14. Decision items for the team

A few items requiring decisions:

**Decision 1: Growformer library license.** The growformer code embedded in the spacekit CLI needs a license. Options:

- Proprietary: spacekit CLI is closed-source for growformer parts (most protective)
- Source-available: code is visible but not modifiable (compromise)
- Open-source with usage license: code is open but usage is controlled by entitlement (transparent but harder to enforce)

Recommendation: source-available for v1 with terms restricting commercial use to entitled users. Withers Worldwide review.

**Decision 2: Embedding growformer in all CLI builds vs. optional builds.**

Options: every CLI binary includes growformer (larger but consistent), or growformer is a separate build target (smaller default CLI, but two distribution paths).

Recommendation: always embed for v1. Optimize size later if needed.

**Decision 3: Forward compatibility of brain.bin files.**

Brains trained with growformer v1.0 should work with later versions, but what about brains trained with later versions on earlier CLI?

Recommendation: brain.bin files include version metadata. Newer brains can run on older CLI (within compatibility window). Older brains always work on newer CLI.

**Decision 4: Growformer team's development workflow.**

The growformer team needs to continue developing growformer outside the spacekit CLI. The `growformer-cli` binary (gated behind `standalone_cli` feature) supports this. Recommendation: keep this binary available for the growformer team but not distributed to users.

**Decision 5: Public API stability.**

The public API of growformer is now consumed by the spacekit CLI. API changes break CLI integration. Recommendation: semver versioning, deprecation periods for breaking changes, written API contract.

## 15. Sign-off

This specification covers the library-based distribution model for growformer. Growformer is refactored into a Rust library, statically compiled into the spacekit CLI, and gated by entitlement checks at function entry.

This is the strongest IP protection available for client-side software. Other content (videos, datasets, third-party software) uses the encrypted-envelope-with-time-bound-decryption approach specified separately. The two models coexist; the right model depends on the content's IP value.

Implementation scope: 6-8 weeks across growformer team and CLI team working in parallel.

Astor Rivera
Founder & CTO, SWTCH Labs
astor@swtch.ai