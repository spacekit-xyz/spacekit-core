# DID-Signed Migration Design Specification

**Status:** Implementation specification (review-ready)
**Version:** 1.0
**Phase:** 3 (federation foundation)
**Owner:** SWTCH Labs
**Audience:** SpaceKit storage node engineers; federation participants; security audit teams

This document specifies the upgrade of SpaceKit's workspace migration system from HMAC-only authentication to layered authentication with DID signatures. The existing HMAC migration continues to function; DID signatures add a cryptographic verification layer that creates long-lived audit trails suitable for cross-operator settlement, dispute resolution, and audit-firm review.

This is an implementation spec, not a design exploration. The team has agreed on the architecture; this document specifies it in enough detail to be implemented and tested.

## 1. Why this upgrade

The existing migration system uses HMAC for inter-operator authentication. HMAC works well for pairwise operator relationships where both operators have exchanged a shared secret in advance. It provides:

- Authentication: the requesting operator can prove possession of a shared secret
- Integrity: the request payload cannot be tampered with in transit
- Speed: HMAC verification is fast (microseconds)

What HMAC does not provide:

- **Identity binding.** HMAC proves possession of a shared secret, not which entity possesses it. If multiple operators use the same shared secret (unlikely but possible) or if a secret is leaked, HMAC cannot distinguish between legitimate use and unauthorized use.
- **Long-term auditability.** HMAC keys rotate. Once a key is rotated, past HMAC-authenticated requests cannot be re-verified by anyone without access to the old key. For audit purposes spanning years, this is a problem.
- **Non-repudiation.** An operator can deny having sent an HMAC-authenticated request because they can claim the shared secret was compromised. There's no cryptographic proof tying a specific identity to the request.
- **Settlement-grade audit trails.** When operators settle accounts based on workspace migration history, they need to point at signed records and say "operator X signed this migration on this date." HMAC doesn't provide that.

DID-signed migration adds these properties on top of HMAC. The HMAC layer continues to provide fast authentication for the day-to-day request flow; the DID signature layer creates verifiable long-lived records that survive key rotations and changes in operator infrastructure.

## 2. Layered authentication model

The complete authentication model after this upgrade has three layers:

### Layer 1: HMAC (transport authentication)

Inter-operator HTTP requests carry HMAC headers as they do today. The HMAC verification happens at request time and proves the requester possesses a shared secret with the recipient. This is the fast path.

### Layer 2: DID signature (identity-bound authentication)

Critical operations within an inter-operator request — specifically migration manifests and other settlement-relevant operations — carry DID signatures using SPHINCS+. The DID signature binds the operation to a specific operator identity, independent of the HMAC layer.

### Layer 3: Long-term verification (audit-grade)

DID-signed operations can be re-verified at any future point by anyone with access to:
- The DID's public key (registered in the operator manifest, which is itself a fact stored in the storage node)
- The signed payload (preserved in the migration record)
- The signature (preserved in the migration record)

This means an audit firm in 2030 can verify a migration that happened in 2026, even if the operators involved have rotated their HMAC keys, changed their infrastructure, or merged with other organizations.

## 3. The migration manifest schema

The existing migration manifest already exists with HMAC authentication. The upgrade adds new fields for DID signatures while preserving all existing fields.

### 3.1 Existing fields (preserved)

```json
{
  "schema": "spacekit:migration:v1",
  "migration_id": "...",
  "source_operator_url": "https://operator-a.example/api",
  "destination_operator_url": "https://operator-b.example/api",
  "workspace_id": "...",
  "workspace_did": "did:spacekit:owner",
  "manifest_hash": "blake3:...",
  "blob_count": 1234,
  "fact_count": 567,
  "initiated_at": 1763000000,
  "expires_at": 1763086400,
  "hmac_key_id": "operator-a-key-1",
  "hmac_signature": "..."
}
```

### 3.2 New fields (added for DID signatures)

```json
{
  // ... all existing fields preserved ...
  
  "schema_version": "spacekit:migration:v2",
  "did_signatures": [
    {
      "signer_role": "source_operator",
      "signer_did": "did:spacekit:operator-a-identity",
      "signature_algorithm": "sphincs-shake-256-128s",
      "signed_payload_hash": "blake3:...",
      "signature": "...",
      "signed_at": 1763000000
    },
    {
      "signer_role": "destination_operator",
      "signer_did": "did:spacekit:operator-b-identity",
      "signature_algorithm": "sphincs-shake-256-128s",
      "signed_payload_hash": "blake3:...",
      "signature": "...",
      "signed_at": 1763000005
    },
    {
      "signer_role": "workspace_owner",
      "signer_did": "did:spacekit:user-owner",
      "signature_algorithm": "sphincs-shake-256-128s",
      "signed_payload_hash": "blake3:...",
      "signature": "...",
      "signed_at": 1763000002
    }
  ]
}
```

### 3.3 Schema version coordination

The `schema_version` field signals whether DID signatures are present:

- `spacekit:migration:v1` — HMAC only, no DID signatures (legacy)
- `spacekit:migration:v2` — HMAC plus one or more DID signatures (current)

Nodes supporting v2 must also support v1 for backward compatibility during the transition period.

### 3.4 Signer roles

The `signer_role` field identifies what authority the signer is exercising:

- **`source_operator`** — the operator initiating the migration (current host of the workspace)
- **`destination_operator`** — the operator receiving the workspace
- **`workspace_owner`** — the user (or one of the multi-wallet co-owners) who owns the workspace

Different migration scenarios require different signer combinations:

| Scenario | Required signers |
|----------|------------------|
| Operator-initiated migration (operator decommissioning) | source_operator |
| User-initiated migration (user moves workspace) | workspace_owner |
| Bilateral migration (negotiated between operators with owner consent) | source_operator + destination_operator + workspace_owner |
| Fork migration (workspace copied to multiple destinations) | source_operator + workspace_owner; each destination signs separately |

The required signer set is determined by the migration scenario, not by protocol rules. Nodes verifying migrations check that all required signers for the migration type are present and valid.

### 3.5 Signed payload definition

The DID signatures sign over a canonical representation of the migration manifest, excluding the signatures themselves (to avoid recursive dependencies).

The signed payload is:

```
schema || schema_version || migration_id || source_operator_url ||
destination_operator_url || workspace_id || workspace_did ||
manifest_hash || blob_count || fact_count || initiated_at || expires_at
```

All fields concatenated in this exact order, each field length-prefixed with a 4-byte big-endian length. This produces a deterministic byte sequence that all parties agree on.

The signed payload is hashed with BLAKE3 to produce `signed_payload_hash`. The actual SPHINCS+ signature signs the BLAKE3 hash, not the full payload. This optimization is acceptable because BLAKE3 is collision-resistant; finding two different payloads with the same BLAKE3 hash is computationally infeasible.

## 4. The signing flow

### 4.1 Source operator initiation

When operator A initiates a migration of workspace W to operator B:

1. Operator A constructs the migration manifest with all v1 fields populated
2. Operator A sets `schema_version` to `spacekit:migration:v2`
3. Operator A computes the signed payload hash from the v1 fields
4. Operator A signs the hash with the operator's identity DID's SPHINCS+ private key
5. Operator A appends a signature entry to `did_signatures` with role `source_operator`
6. Operator A sends the manifest to operator B via the HMAC-authenticated handoff endpoint

### 4.2 Workspace owner counter-signing (if required)

For migrations requiring workspace owner consent:

1. Operator A's request to operator B includes a request for owner counter-signature
2. Operator B presents the migration request to the workspace owner (via the workspace UI)
3. The workspace owner reviews the migration details and confirms
4. The owner's frontend signs the same canonical payload hash with the owner's DID
5. The owner's signature is appended to `did_signatures` with role `workspace_owner`
6. The fully signed manifest proceeds with the migration

### 4.3 Destination operator counter-signing

Operator B counter-signs to acknowledge acceptance of the migration:

1. Operator B verifies the source operator's signature and the workspace owner's signature (if present)
2. Operator B confirms it has the capacity to accept the migration
3. Operator B signs the canonical payload hash with operator B's identity DID
4. Operator B's signature is appended to `did_signatures` with role `destination_operator`
5. The fully signed manifest is the authoritative record of the migration

### 4.4 Migration execution

After all required signatures are in place:

1. The source operator transfers blob content, fact content, and workspace metadata to the destination operator
2. Each blob and fact is verified by content hash on arrival
3. The destination operator stores the fully signed manifest as a fact (this becomes the permanent migration record)
4. The source operator records the same manifest as a migration-out record
5. The workspace document is updated to reflect its new operator
6. Both operators emit migration completion events

## 5. The verification flow

### 5.1 Real-time verification (during migration)

When operator B receives a migration request from operator A:

1. **HMAC verification.** Operator B verifies the HMAC layer using the shared secret. If HMAC fails, the request is rejected as malformed.
2. **Schema version check.** Operator B reads the `schema_version` field. If v2, proceeds to DID signature verification. If v1, proceeds without DID verification (legacy compatibility).
3. **Source operator DID verification.** Operator B looks up operator A's identity DID's public key (from operator A's published operator manifest, which is a fact in operator B's storage or can be fetched from operator A's `/api/operators/self` endpoint).
4. **Signature verification.** Operator B verifies the SPHINCS+ signature using the public key. If verification fails, the request is rejected.
5. **Required signer check.** Operator B confirms all required signers for the migration scenario are present in `did_signatures`.
6. **Counter-sign.** If all checks pass, operator B counter-signs and proceeds.

### 5.2 Post-migration verification (audit)

At any future point, an auditor verifying a past migration:

1. Retrieves the migration manifest fact from either operator's storage
2. Reads the `schema_version` to determine if DID signatures are present
3. For each signature in `did_signatures`:
   - Looks up the signer's identity DID's public key (from the operator manifest fact, or from any node that has a copy of it)
   - Reconstructs the canonical signed payload from the v1 fields
   - Computes the BLAKE3 hash of the canonical payload
   - Verifies the signature using SPHINCS+
4. If all signatures verify and all required signers are present, the migration is cryptographically valid
5. If any signature fails verification, the migration record is corrupted or the signer's identity has been spoofed (the latter is computationally infeasible but the former could happen via storage corruption)

This verification can happen years after the migration. The verification doesn't require the operators involved to still exist or to still have their HMAC keys. It requires only the manifest and the public keys.

### 5.3 Verification of historical operator identity

To verify a signature, the verifier needs the signer's DID's public key. For operator DIDs, this comes from the operator manifest fact (the `spacekit:operator:v1` schema), which contains the operator's SPHINCS+ public key.

The operator manifest fact is itself signed by the operator's DID (operator manifests are typically self-published). This creates a chicken-and-egg problem for very long-term verification: how do we verify the operator manifest's signature when we need the public key to verify, and the public key comes from the manifest?

Resolution: when operator manifests are first published, the publishing operator submits the manifest to a federation discovery index. The discovery index (a future Stream E item) records the timestamp of first publication. Subsequent verification of historical operator identity uses this first-published timestamp as the trust anchor. Until the discovery index ships, manifests are stored in the storage node and treated as trusted-on-first-receipt.

## 6. Backward compatibility

### 6.1 During the transition period

Nodes supporting DID-signed migration must also support HMAC-only migration. This means:

1. v1 manifests received from operators that don't support v2 are accepted with HMAC verification only
2. v2 manifests sent to operators that don't support v2 must include the HMAC layer (already does) and the v2 nodes should not refuse to send v2 manifests
3. Operators advertise their migration version support in their operator manifest

### 6.2 Operator manifest extensions

The operator manifest fact gains a new field:

```json
{
  "schema": "spacekit:operator:v1",
  "display_name": "Dev",
  "policy_uri": "https://...",
  "blob_fact_auth_mode": "hybrid",
  "supported_migration_versions": ["v1", "v2"],
  "did_signature_capable": true,
  "sphincs_public_key": "...",
  // ... existing fields ...
}
```

Operators that support DID-signed migration set `did_signature_capable: true` and include `v2` in `supported_migration_versions`. Operators that haven't upgraded yet set `did_signature_capable: false` or omit the field.

### 6.3 Migration version negotiation

When operator A initiates a migration to operator B:

1. Operator A fetches operator B's manifest from `/api/operators/self`
2. Operator A reads `supported_migration_versions` from the manifest
3. Operator A uses the highest version supported by both:
   - If both support v2, use v2 (DID-signed)
   - If either only supports v1, use v1 (HMAC-only)
4. Operator A's manifest reflects the chosen version

This negotiation happens automatically. Operators don't need to coordinate manually about which version to use.

### 6.4 Upgrade path for existing operators

Operators upgrading from v1-only to v2 follow this path:

1. Upgrade the SpaceKit binary to the version supporting v2
2. Restart the storage node
3. Run `spacekit operator publish` with `--sign` flag to update the operator manifest with the DID signature capability
4. The new manifest is republished; other operators discovering this operator see v2 support

No coordination required. The upgrade is asynchronous across the federation.

## 7. Threat model and security analysis

### 7.1 What this defends against

**Identity spoofing.** Without DID signatures, an attacker who obtains a shared HMAC secret could spoof migrations claiming to be from a legitimate operator. With DID signatures, the attacker also needs the SPHINCS+ private key, which is computationally infeasible to forge.

**HMAC key compromise.** If an HMAC shared secret is compromised, the operator can rotate it without invalidating past DID-signed migrations. The DID signature remains valid as long as the SPHINCS+ key remains valid. This separates the consequences of HMAC key compromise from the consequences of identity-level compromise.

**Long-term audit fraud.** Without DID signatures, past migration records cannot be re-verified if HMAC keys have rotated. With DID signatures, any party with access to public keys can verify past migrations. This prevents an operator from claiming "that migration didn't happen" or "I didn't authorize that migration" years after the fact.

**Settlement disputes.** When operators settle accounts based on migration history, the DID signatures provide unambiguous evidence of who authorized what. Settlement disputes can be resolved by cryptographic evidence rather than HMAC-secret-based claims.

### 7.2 What this does not defend against

**SPHINCS+ private key compromise.** If an operator's SPHINCS+ private key is leaked, an attacker can sign arbitrary migrations as that operator. Mitigation: operators store their SPHINCS+ private keys in hardware security modules (HSMs) or use multi-signature schemes where the operator's identity requires multiple parties to sign.

**Replay attacks.** A migration manifest could in principle be replayed. The `migration_id` and `expires_at` fields are intended to prevent this. Operators must reject migrations with `migration_id` values they've previously processed. The expires_at provides a time-bounded validity window.

**Collusion between operators.** Two cooperating operators can produce migration records that don't reflect what actually happened. DID signatures don't prevent collusion; they prove that specific operators participated. If both operators are colluding to defraud a third party, the cryptography doesn't help.

**Workspace owner compromise.** If a workspace owner's wallet is compromised, the attacker can authorize migrations on the user's behalf. Multi-wallet workspace ownership (a feature we've already specified) provides defense-in-depth here.

**Operator infrastructure compromise.** If an operator's signing infrastructure is compromised, the attacker can produce signed migrations indistinguishable from legitimate ones. Mitigation: operational security around signing infrastructure, plus monitoring for unusual migration patterns.

### 7.3 Quantum resistance

The cryptographic choices are explicitly quantum-resistant:

- **SPHINCS+ for signatures.** NIST FIPS 205. Hash-based, stateless, resistant to attacks by both classical and quantum computers.
- **BLAKE3 for hashing.** Not technically a NIST standard but built on Blake2 (which is well-studied). Collision-resistant against classical attacks; quantum attacks on hash functions have only Grover speedup (square root), not exponential speedup.

The migration records will remain verifiable even if practical quantum computers become available. Past migration records signed with SPHINCS+ today will be re-verifiable in 2050.

## 8. Operational considerations

### 8.1 Signing performance

SPHINCS+ signing takes approximately 200-400ms per signature on modern hardware. A typical migration involves three signatures (source, destination, owner), so signing overhead is approximately 1 second of cumulative compute time across all parties.

This is acceptable for migrations because:
- Migrations are not high-frequency operations (a workspace might migrate once a year)
- Migrations are not user-facing latency-sensitive operations
- The signing happens once per migration, not per blob or per fact

### 8.2 Storage overhead

DID signatures add storage overhead to migration manifests:
- SPHINCS+-SHAKE-256-128s signatures are approximately 8KB each
- Three signatures = 24KB per migration manifest
- Plus the metadata (signer DID, role, timestamp): about 200 bytes per signature

Total overhead per migration: approximately 25KB. For a federation with hundreds of migrations per day, this adds gigabytes per year of additional storage but well within reasonable bounds.

### 8.3 Operator key management

Each operator must manage their SPHINCS+ private key responsibly:

- **Generation.** Keys are generated during initial operator setup (`spacekit init` or equivalent)
- **Storage.** Private keys stored encrypted at rest, ideally in HSMs or hardware-backed storage
- **Backup.** Keys backed up securely; loss of the key means the operator cannot sign future migrations under that identity
- **Rotation.** Key rotation is supported via a key-rotation attestation that signs a new key with the old key, creating a verifiable chain of operator identities

### 8.4 Workspace owner key reuse

The workspace owner signs migrations using the same SPHINCS+ keypair derived from their wallet (as specified in the SpaceKit Workspaces frontend identity and crypto handling document). No new key generation is needed; existing wallet-derived keys work.

### 8.5 Monitoring and alerting

Storage nodes should monitor migration patterns and alert on anomalies:
- Migrations from operators whose manifests don't match the signing key used
- Migrations with stale `initiated_at` timestamps (potential replay)
- Migrations from operators with reputation indicators that haven't been built yet but will be

## 9. Test plan

### 9.1 Unit tests

The implementation needs unit tests for:

- Canonical payload construction is deterministic across implementations
- BLAKE3 hashing produces consistent results
- SPHINCS+ signing and verification roundtrip
- Schema version negotiation logic
- Required signer enforcement per migration scenario
- Operator manifest field parsing for migration version support

### 9.2 Integration tests

Multi-node integration tests covering:

- v2-to-v2 migration: full DID-signed flow
- v1-to-v1 migration: legacy HMAC-only flow
- v1-to-v2 migration: v1 sender, v2 receiver — receiver accepts v1 manifest
- v2-to-v1 migration: v2 sender, v1 receiver — sender sends v1 manifest
- Mixed federation: three operators with various version support

### 9.3 Soak tests

Extended soak tests covering:

- Sustained migration load (multiple migrations per minute)
- Long-running migrations (large workspaces, large blob counts)
- Concurrent migrations involving the same operator
- Migration with simulated network failures and retries

### 9.4 Audit tests

Tests specifically for the audit case (verification long after migration):

- Verify a migration after operator HMAC keys have rotated
- Verify a migration when one of the operators is no longer active
- Verify a migration using only the public keys (no HMAC verification possible)
- Detect tampered migration records

### 9.5 Failure tests

Tests for failure modes:

- Invalid signatures rejected
- Missing required signatures rejected
- Stale `initiated_at` rejected
- Replayed `migration_id` rejected
- Manifest tampering (any field modification) rejected

## 10. Migration policy considerations

A few policy questions that emerge from this design:

### 10.1 Who can initiate migrations?

Source operator initiation is straightforward. Owner-initiated migrations need a flow for users to discover migration is possible and request it. The UX for this is in the SpaceKit Workspaces frontend specification.

### 10.2 What if signatures don't match?

If a migration arrives with invalid signatures, the destination operator rejects it. The source operator (if legitimately attempting the migration) should detect the rejection and investigate. If the signature mismatch is due to operator infrastructure error, the operator can correct and retry. If it's due to attempted spoofing, the operator can alert security.

### 10.3 Workspace owner unavailability

Some migrations may be initiated by source operators when workspace owners are unavailable (e.g., operator decommissioning). The policy framework (Stream D) governs what's acceptable:
- Operator decommissioning with notice: source_operator signature alone may be sufficient if the operator's policy permits
- Operator emergency: governance may permit migrations without owner counter-signature in specific scenarios
- Standard migration: workspace_owner counter-signature is typically required

These policy decisions are out of scope for this technical spec but should be addressed in operator policy documents.

## 11. Implementation phases

The implementation can proceed in phases:

**Phase 1: Schema support.** Update the migration manifest schema to v2. Existing migration code continues to use v1 fields. Implementation is mostly type definitions and serialization code.

**Phase 2: Signing infrastructure.** Implement the SPHINCS+ signing code. Implement canonical payload construction. Verify signing produces deterministic output across instances.

**Phase 3: Verification infrastructure.** Implement verification at request time and at storage time. Verify roundtrip correctness through tests.

**Phase 4: CLI integration.** Update CLI commands to handle the new flow. Specifically:
- `spacekit operator publish` produces v2 manifests with capability indication
- New `spacekit migration sign` command for explicit signing of migration manifests
- New `spacekit migration verify` command for explicit verification

**Phase 5: Operator manifest extension.** Update the operator manifest schema to include migration version support fields. Update `spacekit operator publish` and `spacekit operator show` to handle these.

**Phase 6: Integration testing.** Two-node and three-node integration tests. Confirm backward compatibility with v1-only nodes.

**Phase 7: Federation testing matrix.** Add DID-signed migration scenarios to the federation testing matrix.

**Phase 8: Documentation.** Update `did-signed-migration.md`, `federation-design.md`, and `federation-testing.md` to reflect implemented state.

Each phase is independently testable. Total implementation effort: approximately 3-4 engineer-weeks for one focused engineer.

## 12. Open questions

A few items deferred to implementation:

**Question 1: HSM integration.** Should the implementation support hardware security module (HSM) integration for operator signing keys? Strongly recommended for production operators but not required for self-hosted single-tenant deployments. May be added in a follow-up spec.

**Question 2: Multi-signature operator identities.** Should an operator's identity support multi-signature schemes (e.g., 2-of-3 signatures required for migration approval)? Useful for high-stakes operators but adds complexity. Defer to a follow-up spec.

**Question 3: Cross-network migration.** Can workspaces migrate between operators on different SpaceKit networks (testnet to mainnet, for example)? Probably yes but with explicit operator-level acknowledgment. Defer specification to network-specific migration spec.

**Question 4: Migration receipts.** Should successful migrations produce explicit receipts (a fact or attestation) that all parties sign? Currently the migration manifest itself serves as the receipt. Could be enhanced with separate completion receipts for clearer post-completion verification.

## 13. Relationship to other federation work

This spec coordinates with several other federation specifications:

**Federation design (federation-design.md).** This spec is the DID-signed migration component of the broader federation design. The federation design covers operator discovery, cross-operator collaboration, and other federation primitives; this spec covers the migration security model specifically.

**Operator policy framework (Stream D).** Operator policies govern who can migrate, under what conditions. This spec provides the cryptographic mechanism; the policy framework provides the operational rules. Specifically:
- Source operator policy may govern when migrations are initiated
- Destination operator policy may govern which workspaces are accepted
- The policy URI in the operator manifest enables clients to evaluate operator policies

**Discovery index (Stream E).** When the federation discovery index ships, operator manifests will be discoverable. This enables broader federation by removing the need for pairwise operator coordination. DID-signed migration works with discoverable operators (verification works whether the manifest is fetched from discovery index or directly from the operator).

**Settlement framework (future Stream E).** Cross-operator economic settlement uses migration history as evidence. DID-signed migrations provide the audit-grade records that settlement requires.

## 14. Backward compatibility detailed

A more precise specification of backward compatibility:

| Sender | Receiver | Migration version | DID signatures present |
|--------|----------|--------------------|------------------------|
| v1-only | v1-only | v1 | No |
| v1-only | v2-supporting | v1 (negotiated to lowest) | No |
| v2-supporting | v1-only | v1 (negotiated to lowest) | No |
| v2-supporting | v2-supporting | v2 | Yes |

Operators advertise their version support in their manifest. Negotiation happens automatically.

For migrations being conducted today (before this upgrade), the legacy v1 flow continues to work indefinitely. There is no forced upgrade. Operators upgrade at their own pace.

For long-term audit verification, only v2 migrations have audit-grade records. v1 migrations remain verifiable only via HMAC, which becomes less useful as HMAC keys rotate.

The federation can have a mix of v1 and v2 operators indefinitely. Over time, the proportion of v2 operators increases as operators upgrade. The audit-grade properties become more universal as adoption grows.

## 15. Implementation completion criteria

The implementation is considered complete when:

- All seven implementation phases (Section 11) are shipped
- Integration tests pass for all combinations in Section 14's compatibility matrix
- Soak tests pass for sustained DID-signed migration load
- Federation testing matrix is updated and all scenarios pass
- Documentation is updated to reflect implemented state
- A two-operator demonstration shows a real workspace migration with DID signatures from initiation through completion
- The audit verification path is tested with a migration record at least 30 days old (proving verification works after HMAC keys would conceptually have rotated)

## 16. Phase 3 launch readiness

Phase 3 (open federation marketplace) requires several pieces beyond DID-signed migration:

- Policy enforcement (Stream D)
- Discovery index 
- Reputation system
- Settlement framework

DID-signed migration is foundational for several of these. Specifically:
- Policy enforcement uses migration history to track operator behavior
- Reputation uses migration patterns as one signal
- Settlement uses migration records as evidence

When DID-signed migration ships, it doesn't immediately enable open federation marketplace, but it unblocks the work that does.

## 17. Sign-off

This specification is ready for implementation. The team has reviewed and committed to the design. Implementation can proceed.

Astor Rivera
Founder & CTO, SWTCH Labs
astor@swtch.ai