# SpaceKit Real-World Assets (RWA) Contracts

Reference implementations and production-grade smart contracts for representing real-world assets on the SpaceKit network. This directory contains two complementary contracts plus the documentation needed to deploy them as part of legally operative asset records.

**These contracts are real products designed for production use, not just reference implementations.** They include the legal framework guidance, deed addendum templates, and integration patterns needed to make on-chain asset records legally operative through appropriate off-chain anchoring.

## What's in this directory

**Contracts:**

- **`PropertyToken.rs`** — Whole-property real estate token. One token per property, single or joint ownership, comprehensive metadata, encumbrance tracking, document references. Designed to become legally operative when incorporated by reference into a deed addendum.

- **`AssetToken.rs`** — Generic asset token for any property type. Vehicles, art, equipment, intellectual property, livestock, collectibles, digital assets. Extensible schema with type-specific attributes. Less detailed than PropertyToken but more flexible.

**Documentation:**

- **`README.md`** (this file) — Overview, scope, and how the two contracts relate.

- **`PropertyToken-design.md`** — Usage patterns for PropertyToken, including the deed addendum framework, title company integration, and SpaceKit Pay integration for property purchases.

- **`AssetToken-design.md`** — Usage patterns for AssetToken, including asset-type-specific attribute schemas, evolution paths to PropertyToken, and authentication/provenance patterns.

- **`example-deed-addendum.md`** — Actual deed addendum template that incorporates PropertyToken by reference. Suitable for use as a starting point by real estate attorneys. **Not a substitute for attorney review.**

- **`regulatory-considerations.md`** — Calibrated regulatory framing for both contracts: jurisdiction concerns, federal law considerations, recording requirements, securities-law implications, and what these contracts are NOT (they are not investment instruments).

## The relationship between the two contracts

PropertyToken and AssetToken serve overlapping but distinct purposes:

**PropertyToken** is the real-estate-specific contract with comprehensive metadata (location with parcel ID, legal description, lot/building sizes), encumbrance tracking (mortgages, liens, easements with their full lifecycle), and the deed addendum anchoring pattern that makes it legally operative for real property transfers.

**AssetToken** is the generic contract for any other property type. It has a flexible attribute schema that adapts to different asset categories (a vehicle's VIN and year, an artwork's medium and artist, equipment's serial number and manufacturer). It's not designed for the deed-addendum pattern because most non-real-estate assets don't have an equivalent recording requirement.

**Evolution path:** An AssetToken representing a vacant lot can be evolved to a PropertyToken when a deed addendum is created. The AssetToken's `LINK_TO_PROPERTY` operation creates the cross-reference. The two tokens then coexist, with the PropertyToken being the legally operative record once the deed addendum is filed.

**Use one or both:** Most projects use one of the two. Use PropertyToken if you're working with real estate specifically. Use AssetToken if you're working with other assets. Use both if you need to handle both categories (e.g., a real estate investment fund that also holds vehicles or equipment).

## A note on the third contract: FractionalProperty

A third RWA contract is planned but not yet shipped: **FractionalProperty**. This would be the RealT-style contract where a property is owned by an LLC or trust that issues N tokens, with each token representing fractional ownership and entitling holders to pro-rata rental income.

FractionalProperty is not in this directory yet because it has significant additional complexity:

- Regulatory: fractional property tokens are almost certainly securities under US law, requiring Reg D 506(c) exemption or equivalent
- Compliance: accredited investor verification, KYC requirements, transfer restrictions
- Economic: dividend distribution mechanics, voting rights, secondary market support
- Operational: holder registries, notifications, tax reporting

When FractionalProperty ships, it will be in this directory alongside PropertyToken and AssetToken with its own design document and regulatory framing.

## Important: these contracts are not legally binding on their own

This is the most important thing to understand about both PropertyToken and AssetToken. They are records and references, not legal authority.

**A PropertyToken does not by itself transfer real estate ownership.** The legal transfer of real property requires a deed (or equivalent jurisdictional instrument) recorded with the county recorder of deeds (or equivalent authority). PropertyToken becomes legally operative when a deed addendum incorporates the smart contract by reference and is recorded along with the deed.

**An AssetToken does not by itself transfer ownership of vehicles, art, equipment, or other assets.** The legal frameworks for these vary by asset type and jurisdiction. A vehicle title needs to be reissued through the relevant DMV. Art authentication is governed by provenance, auction-house records, and expert verification. Each asset category has its own legal infrastructure.

What these contracts DO provide:

- A cryptographically signed, content-addressed record of ownership and transfer history
- A verifiable audit trail of all changes to the asset record
- An integration point for off-chain documents (deeds, titles, certificates) via CAS references
- A foundation for the legal infrastructure (deed addendums, title company integrations, etc.) that makes the records legally operative

The contract is one piece of a larger legal arrangement. The arrangement requires legal counsel.

## Recommended use patterns

### Pattern 1: Single property, single owner (residential real estate)

Most common use case. An owner uses PropertyToken to create a digital record of their property, files a deed addendum incorporating the token by reference, and uses the contract for ownership history, document attachments, and eventually property transfer.

```
1. Owner mints PropertyToken for their property
2. Owner attaches deed (CAS-stored) as DOC_DEED
3. Owner has attorney draft deed addendum incorporating PropertyToken
4. Attorney files deed addendum with county recorder
5. PropertyToken now has legal anchor; ongoing operations are legally meaningful
```

### Pattern 2: Property purchase with SpaceKit Pay

Buyer and seller use PropertyToken and SpaceKit Pay together for an integrated purchase.

```
1. Seller has existing PropertyToken with deed addendum on file
2. Buyer pays via SpaceKit Pay to escrow account (typically title company)
3. Title company verifies clear title, releases payment to seller
4. Title company signs the PropertyToken transfer as escrow agent
5. Seller signs transfer; buyer signs receipt
6. New deed and updated deed addendum filed with county recorder
7. PropertyToken now shows buyer as owner; legally operative
```

### Pattern 3: Vehicle ownership records

Owner uses AssetToken to maintain comprehensive records of their vehicle (or fleet).

```
1. Owner mints AssetToken with type=VEHICLE, includes VIN as unique identifier
2. Owner attaches title (CAS-stored) as DOC_TITLE
3. Owner attaches photos, insurance docs, maintenance records over time
4. When selling, transfer is recorded on AssetToken; title is reissued via DMV
5. AssetToken provides comprehensive provenance even though legal authority is the DMV title
```

### Pattern 4: Art authentication and provenance

Art owner uses AssetToken to establish and maintain provenance.

```
1. Owner mints AssetToken with type=ART, includes artist, medium, year as attributes
2. Owner attaches certificate of authenticity as DOC_CERTIFICATE_OF_AUTHENTICITY
3. Owner attaches expert appraisals as DOC_APPRAISAL
4. Owner attaches photos for visual reference
5. On sale, provenance chain is preserved cryptographically in the transfer history
```

### Pattern 5: Equipment inventory for businesses

A business uses AssetToken to track major equipment.

```
1. Business (organizational DID) mints AssetToken for each piece of equipment
2. Type=EQUIPMENT, attributes include serial number, manufacturer, purchase date
3. Maintenance records attached as documents over time
4. Insurance policies attached
5. When equipment is sold or disposed, transfer recorded
6. Useful for tax depreciation tracking, insurance audits, asset valuation
```

## Integration with other SpaceKit primitives

These contracts integrate naturally with the broader SpaceKit ecosystem:

**FactPackage primitive.** PropertyToken and AssetToken records are FactPackages with appropriate schemas (`spacekit:property:v1`, `spacekit:asset:v1`). They benefit from the same signature verification, access policies, and citation graph capabilities as all SpaceKit storage.

**DID identity.** Owners, holders, escrow agents, title companies — all parties are identified by their DIDs. SPHINCS+ signatures on all operations provide post-quantum cryptographic guarantees.

**Storage Node CAS.** Documents (deeds, titles, certificates, photos) are stored as CAS blobs in the storage node. The contracts reference them by content hash. Documents are deduplicated automatically; tampering is detectable.

**SpaceKit Pay.** Property and asset purchases can route through SpaceKit Pay for atomic payment-and-transfer. The 95/5 split applies (95% to seller, 5% to SpaceKit treasury) or alternative routing if escrow/title company is involved.

**SpaceKit Workspaces.** Property and asset records can live in workspaces where the relevant parties (owner, attorney, title company, insurance agent) collaborate on the property's documentation.

**ASTRA emission.** Storage node operators serving property/asset records earn ASTRA via the storage service category in the emission schedule. Property records are durable on-chain content.

## Read order for new users

If you're new to these contracts, read in this order:

1. **README.md** (this file) — orientation
2. **regulatory-considerations.md** — what these contracts are and aren't, legally
3. **PropertyToken-design.md** OR **AssetToken-design.md** depending on your use case
4. **example-deed-addendum.md** if working with real estate
5. **The contract source files** (`PropertyToken.rs`, `AssetToken.rs`) for the technical details

## Important: legal counsel

Using these contracts in a legally operative manner requires legal counsel familiar with:

- Real property law in the relevant jurisdiction (for PropertyToken)
- Specific asset-type legal frameworks (for AssetToken, varies by type)
- Smart contract integration with traditional legal instruments
- Recording requirements with the relevant authorities
- Tax implications of asset tokenization

The documentation in this directory provides templates, patterns, and considerations — but it does not substitute for attorney review of your specific situation.

SWTCH Labs has engaged Withers Worldwide for legal review of the contracts and the deed addendum framework. Withers's review covers general design and US federal/state considerations. Implementations in specific jurisdictions or for specific use cases require additional review by counsel familiar with the local legal environment.

## Versioning

These contracts and their documentation are versioned:

- Contract version: v1.0 (initial production release)
- Schema version: `spacekit:property:v1`, `spacekit:asset:v1`
- Documentation version: 1.0

Future versions will be released as `v2`, etc., with clear migration paths. The schema versioning ensures that records created under v1 schemas remain readable as the ecosystem evolves.

## Contact

For questions on these contracts:

Astor Rivera
Founder & CTO, SWTCH Labs
astor@swtch.ai

For questions about real estate or asset tokenization specifically (use cases, integrations, partnerships), please indicate this in your inquiry so it can be routed appropriately.
