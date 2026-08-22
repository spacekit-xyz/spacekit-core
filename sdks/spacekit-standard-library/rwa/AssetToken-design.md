# AssetToken Design Document

**Status:** Production specification
**Version:** 1.0
**Owner:** SWTCH Labs

This document specifies the design, usage patterns, and asset-type-specific schemas for AssetToken — the SpaceKit generic asset token contract. AssetToken is companion to PropertyToken (for real estate) but covers any other property type.

This document is technical specification combined with operational guidance. It is not legal advice. Implementations require attorney review appropriate to the specific asset type and jurisdiction.

## 1. Purpose and scope

AssetToken represents any non-real-estate property as a unique on-chain token. The contract maintains the asset's metadata, ownership history, and document references. The flexible attribute schema adapts to different asset categories.

**In scope:**
- Vehicles (cars, motorcycles, boats, aircraft)
- Art and collectibles
- Equipment (industrial, professional, scientific)
- Intellectual property (patents, trademarks, copyrights)
- Livestock and agricultural assets
- Precious metals and commodities
- Digital assets
- Inventory and business assets
- Custom asset types via extensible type system

**Out of scope:**
- Real estate (use PropertyToken instead)
- Fractional ownership (use FractionalProperty when available)
- Securities and investment instruments (different legal framework entirely)
- Currency and cash (not a property record use case)

## 2. The asset type system

AssetToken's flexibility comes from its asset type field plus its flexible attributes JSON.

### 2.1 Built-in asset types

The contract defines built-in types for common categories:

- `1` Vehicle (car, motorcycle, boat, aircraft, etc.)
- `2` Art (painting, sculpture, photograph, mixed media)
- `3` Equipment (industrial, professional, scientific, agricultural)
- `4` Intellectual property (patent, trademark, copyright, trade secret)
- `5` Livestock (cattle, horses, exotic animals, etc.)
- `6` Collectible (rare books, sports memorabilia, coins, stamps)
- `7` Precious metal (gold, silver, platinum bars/coins)
- `8` Digital asset (domains, NFT collections, gaming items)
- `9` Inventory (business inventory tracking)
- `255` Other (anything not in the above categories)

Custom types start at `0x80` (128) — operators can define their own type IDs for novel asset categories.

### 2.2 Type-specific attributes

The `attributes` field is a JSON object whose schema depends on the asset type. Examples below.

**Vehicle attributes:**
```json
{
  "make": "Toyota",
  "model": "Camry",
  "year": 2024,
  "vin": "1HGBH41JXMN109186",
  "color": "Silver",
  "mileage": 15000,
  "engine_type": "hybrid",
  "fuel_type": "gasoline_hybrid",
  "transmission": "automatic",
  "license_plate": "ABC-1234",
  "state_of_registration": "TX"
}
```

**Art attributes:**
```json
{
  "artist_name": "Vincent Van Gogh",
  "artist_did": "did:spacekit:...",
  "title": "Starry Night Over the Rhone",
  "year_created": 1888,
  "medium": "oil on canvas",
  "dimensions": "72.5 x 92 cm",
  "style": "post-impressionism",
  "provenance_count": 3,
  "condition_grade": "excellent",
  "last_appraisal_date": 1740009600,
  "last_appraisal_value_usd": 5000000
}
```

**Equipment attributes:**
```json
{
  "manufacturer": "John Deere",
  "model": "9620R",
  "serial_number": "1RW9620RPLW000123",
  "year_manufactured": 2023,
  "category": "agricultural_tractor",
  "horsepower": 620,
  "operating_hours": 1250,
  "maintenance_records_count": 12,
  "warranty_expiration": 1772092800,
  "primary_location": "Farm A, 100 Acre Field"
}
```

**Intellectual property attributes:**
```json
{
  "ip_type": "patent",
  "registration_number": "US12345678",
  "title": "Method and Apparatus for Distributed Consensus",
  "filing_date": 1735689600,
  "grant_date": 1772092800,
  "expiration_date": 2362262400,
  "inventors": ["did:spacekit:inv1", "did:spacekit:inv2"],
  "jurisdiction": "US",
  "patent_office": "USPTO",
  "current_status": "granted"
}
```

**Livestock attributes:**
```json
{
  "species": "bovine",
  "breed": "Angus",
  "registration_id": "AAA12345",
  "date_of_birth": 1735689600,
  "sex": "female",
  "registered_organization": "American Angus Association",
  "tag_number": "001234",
  "current_location": "Pasture 3",
  "vaccination_records_count": 8,
  "purpose": "breeding"
}
```

**Collectible attributes:**
```json
{
  "category": "sports_memorabilia",
  "subcategory": "baseball_card",
  "name": "Mickey Mantle 1952 Topps",
  "grading_service": "PSA",
  "grade": "9",
  "certification_number": "12345678",
  "year_produced": 1952,
  "condition_notes": "Excellent corners, sharp colors",
  "last_known_sale_value_usd": 5200000,
  "last_known_sale_date": 1735689600
}
```

**Precious metal attributes:**
```json
{
  "metal": "gold",
  "form": "bullion_bar",
  "weight_oz_troy": 100,
  "purity": "999.9",
  "refinery": "PAMP Suisse",
  "serial_number": "PAMP000123456",
  "storage_location": "Brink's Vault, Salt Lake City",
  "storage_account_id": "VAULT-789",
  "current_spot_value_usd": 200000
}
```

**Digital asset attributes:**
```json
{
  "asset_type": "domain_name",
  "name": "spacekit.xyz",
  "registrar": "Namecheap",
  "registration_date": 1735689600,
  "expiration_date": 1772092800,
  "name_servers": ["ns1.spacekit.xyz", "ns2.spacekit.xyz"],
  "dnssec_enabled": true
}
```

### 2.3 Custom asset types

For novel asset categories, operators use type IDs in the `0x80` (128) to `0xFE` (254) range:

```json
{
  "asset_type": 128,
  "custom_type_name": "Carbon Credit",
  "attributes": {
    "credit_type": "verified_emissions_reduction",
    "issuing_organization": "Verra",
    "registration_id": "VCS-12345",
    "project_id": "12345",
    "vintage_year": 2024,
    "tonnes_co2_equivalent": 1000,
    "verification_standard": "VCS v4.0"
  }
}
```

When a custom type is used, the `custom_type_name` field documents what the type represents. Multiple operators using the same custom type ID should coordinate on a common attribute schema, though this isn't enforced by the contract.

## 3. Ownership models

AssetToken supports three ownership models:

### 3.1 Sole ownership

Single owner DID. Most common for personal assets (your car, your art collection).

### 3.2 Joint ownership

Multiple owner DIDs with equal rights. Used for assets co-owned by partners, spouses, business partners.

The asset can be sold/transferred by any owner (with appropriate signatures). Disagreements between joint owners require off-chain resolution; the contract doesn't enforce joint decision-making.

### 3.3 Organizational ownership

Asset owned by an organizational DID (LLC, trust, corporation, partnership). The organization's DID is the owner of record; individuals associated with the organization (members, officers, beneficiaries) sign on behalf of the organization.

This is common for:

- Business equipment (owned by the company DID)
- Investment portfolios (owned by an investment fund DID)
- Collectibles held by museums (owned by the institution's DID)
- IP assets (owned by a corporation that licenses to others)

## 4. Document references

AssetToken references off-chain documents stored as CAS blobs. Document types vary by asset category.

### 4.1 Common document types

- `1` Title (vehicle title, etc.)
- `2` Certificate of authenticity (art, collectibles)
- `3` Registration (registration with relevant authority)
- `4` Appraisal (professional valuation)
- `5` Insurance (policy documents)
- `6` Provenance (chain of custody documentation)
- `7` Purchase receipt
- `8` Photo (visual documentation)
- `9` Inspection (mechanical, condition assessment)
- `255` Other

### 4.2 Asset-type-specific document patterns

Different asset types use different document patterns:

**Vehicle documents typically include:**
- Title (DOC_TITLE) — the state-issued vehicle title
- Registration (DOC_REGISTRATION) — current registration
- Insurance (DOC_INSURANCE) — current insurance policy
- Inspection (DOC_INSPECTION) — most recent inspection
- Purchase receipt (DOC_PURCHASE_RECEIPT)
- Photos (DOC_PHOTO) — multiple, showing condition

**Art documents typically include:**
- Certificate of authenticity (DOC_CERTIFICATE_OF_AUTHENTICITY)
- Provenance (DOC_PROVENANCE) — chain of ownership history
- Appraisal (DOC_APPRAISAL) — professional valuations
- Insurance (DOC_INSURANCE) — fine art insurance policy
- Photos (DOC_PHOTO) — high-resolution images
- Exhibition history (DOC_OTHER) — where the piece has been shown

**Equipment documents typically include:**
- Purchase receipt (DOC_PURCHASE_RECEIPT)
- Manufacturer warranty (DOC_OTHER)
- Operator's manual (DOC_OTHER)
- Maintenance records (DOC_INSPECTION, multiple)
- Insurance (DOC_INSURANCE)
- Photos (DOC_PHOTO)

**IP documents typically include:**
- Registration certificate (DOC_REGISTRATION)
- Original filing documents (DOC_OTHER)
- Examination correspondence (DOC_OTHER)
- License agreements (DOC_OTHER)

## 5. Evolution to PropertyToken

A special operation `LINK_TO_PROPERTY` allows an AssetToken to be linked to a PropertyToken. This supports the use case where an asset (e.g., a vacant lot) is initially tracked as a generic asset and later evolves into a fully-anchored real estate property when a deed addendum is filed.

### 5.1 When to use evolution

Use cases for AssetToken → PropertyToken evolution:

- **Land assemblies.** A developer assembles multiple vacant lots as AssetTokens (light-weight tracking). When ready to develop and formalize, each becomes a PropertyToken with deed addendum.
- **Inherited property pre-registration.** An estate inherits property; family wants to track it digitally before deciding on formalization. Initially an AssetToken; becomes PropertyToken when sold to a new owner with proper legal anchoring.
- **Pre-construction tracking.** A developer tracks a property through the development phase as an AssetToken (with construction-related metadata); converts to PropertyToken at occupancy.

### 5.2 How evolution works

1. Original AssetToken exists with property-like metadata
2. PropertyToken is minted for the same property (separate operation)
3. AssetToken's owner calls `LINK_TO_PROPERTY` with the PropertyToken's token_id
4. The AssetToken record now references the PropertyToken
5. The PropertyToken becomes the legally operative record once a deed addendum is filed
6. The AssetToken remains for historical reference but the PropertyToken is the authoritative record

The two tokens coexist; they're cross-referenced.

## 6. Authentication and provenance patterns

A key use case for AssetToken is establishing and maintaining authentication and provenance. This is especially important for art, collectibles, and high-value items.

### 6.1 Provenance chain

For art and collectibles, the provenance chain (who has owned this piece) is part of its value. AssetToken supports this through:

- **Ownership history.** Every transfer is recorded, creating an immutable chain
- **Document attachments.** Certificates of authenticity, exhibition records, expert opinions
- **Long-term durability.** As long as SpaceKit operators maintain the storage, the provenance is preserved

### 6.2 Authentication services

Authentication services (like PSA for sports cards, GIA for diamonds, Sotheby's for art) can have their own DIDs and attest to assets:

1. Owner creates AssetToken for the item
2. Authentication service examines the item
3. Authentication service issues a signed attestation (their DID signs a fact)
4. The attestation is attached to the AssetToken as a document
5. Buyers verify both the asset's history and the authentication service's signature

This creates a verifiable authentication chain that doesn't require trusting any single party — the chain itself is cryptographically verifiable.

### 6.3 Counter-forgery considerations

AssetToken doesn't prevent forgery of physical items. If someone creates a fake Van Gogh and mints an AssetToken claiming it's authentic, the AssetToken doesn't make the fake real.

What AssetToken does provide:

- **Verifiable history.** A buyer can verify that the AssetToken was minted by someone, that documents were attached at specific times, that ownership has transferred through specific parties.
- **Tamper-resistance.** The token's record can't be retroactively modified.
- **Cryptographic accountability.** If a forgery is later discovered, the chain of attestations shows who made claims and when.

But the physical authenticity still depends on traditional expert authentication. AssetToken is a record-keeping system, not a forgery detector.

## 7. Operational patterns by asset type

### 7.1 Vehicles

**Typical workflow:**

1. Owner mints AssetToken with type=VEHICLE on purchase, attaches title (DOC_TITLE)
2. Owner attaches registration each year (DOC_REGISTRATION)
3. Owner attaches inspections (DOC_INSPECTION) periodically
4. On sale, transfer recorded on AssetToken; title reissued via DMV
5. Buyer continues ownership documentation

**What makes this useful:** Comprehensive vehicle history (more than Carfax provides) including all maintenance, insurance, and inspection records. Helpful for fleet management, classic car valuation, and dispute resolution.

**Legal anchor:** The state-issued vehicle title remains the legal authority. AssetToken is the supplementary record.

### 7.2 Art and collectibles

**Typical workflow:**

1. Artist or initial owner mints AssetToken on creation/acquisition
2. Provenance documentation attached
3. Each subsequent owner records transfer
4. Authentication services issue attestations as needed
5. Insurance, exhibitions, restorations all documented

**What makes this useful:** Continuous provenance from creation. Eliminates many forgery vectors because the chain of custody is cryptographically verifiable.

**Legal anchor:** Traditional art world conventions and expert authentication.

### 7.3 Equipment

**Typical workflow:**

1. Business mints AssetToken on equipment purchase (or imports existing assets)
2. Equipment specifications recorded
3. Maintenance records attached as performed
4. Insurance policies attached
5. End-of-life (sale, scrapping) recorded as transfer or burn

**What makes this useful:** Tax depreciation tracking, insurance audits, maintenance compliance, asset retirement records.

**Legal anchor:** Equipment ownership doesn't have a centralized recording authority; the purchase receipt and associated documents are the legal evidence.

### 7.4 Intellectual property

**Typical workflow:**

1. IP owner mints AssetToken on grant of IP
2. Registration certificate attached
3. License agreements recorded as transfers (with appropriate terms in transfer metadata)
4. Maintenance fees, renewals documented
5. Litigation events documented

**What makes this useful:** Centralized IP portfolio management with cryptographic verification of all licensing and assignments.

**Legal anchor:** The patent office, trademark office, or copyright office's records remain the legal authority. AssetToken is the supplementary record.

### 7.5 Livestock

**Typical workflow:**

1. Owner mints AssetToken for each animal (or batch for commodity livestock)
2. Registration with breed organization (if applicable) documented
3. Vaccination records, health certificates attached
4. Breeding/calving records documented
5. Sale or disposal recorded as transfer

**What makes this useful:** Comprehensive animal records for breeding programs, veterinary care, livestock auctions, and traceability for food safety.

**Legal anchor:** Brand registration with state agriculture authorities, breed association registrations.

## 8. Bulk operations and inventory management

For businesses tracking large numbers of assets, AssetToken supports patterns for efficient bulk operations:

### 8.1 Batch minting

Multiple AssetTokens can be minted in close succession by a single business DID, all sharing some attributes (e.g., a batch of identical equipment from the same manufacturer) while having different unique identifiers (serial numbers).

The contract doesn't have a special batch operation; each mint is a separate transaction. But the gas costs are bounded and the operations can be parallelized.

### 8.2 Asset categories

Use the `attributes.category` field to group similar assets. Query by owner and category to get inventory views:

```
LIST_ASSETS_BY_OWNER(business_did)
→ filter results client-side by attributes.category
```

### 8.3 Sub-asset relationships

Some assets contain other assets (a tractor with multiple attachments, a building with appliances). Pattern: the parent asset's attributes reference the sub-asset token IDs:

```json
{
  "attributes": {
    "type": "facility",
    "sub_assets": [
      "asset_token_id_1",
      "asset_token_id_2",
      "asset_token_id_3"
    ]
  }
}
```

This is a client-side convention, not contract-enforced.

## 9. Integration with SpaceKit Pay

AssetToken integrates with SpaceKit Pay for asset purchase payments similarly to PropertyToken:

1. Buyer authorizes payment via SpaceKit Pay
2. Payment routes 95% to seller, 5% to SpaceKit treasury
3. Asset transfer recorded on AssetToken with payment confirmation hash
4. Cryptographic audit trail links payment to asset transfer

For high-value assets (art, collectibles), escrow patterns similar to PropertyToken's apply.

For lower-value assets (everyday vehicles, equipment), direct buyer-seller payments without escrow are often sufficient.

## 10. Open questions and limitations

### 10.1 Cross-jurisdiction asset ownership

Vehicle titles, IP registrations, and many other asset records are jurisdictional. An AssetToken can document an asset whose legal authority is in one jurisdiction even if the owner is in another, but the AssetToken doesn't change the underlying legal framework.

### 10.2 Asset type proliferation

The custom type range (0x80-0xFE) allows operators to define their own types. Without coordination, different operators might define different IDs for the same type. The community may eventually need a coordination mechanism (registry of common custom types) to prevent fragmentation.

### 10.3 Authentication service trust

If a "fake" authentication service issues attestations, the contract has no way to know the attestations are not legitimate. Trust in authentication services is established socially (reputation, regulatory recognition, industry consensus). AssetToken provides the cryptographic infrastructure but not the trust framework.

### 10.4 Privacy considerations

Some assets are intentionally private (gold storage, art collections, IP portfolios). AssetToken records can be encrypted with envelope encryption for privacy, but the existence of the AssetToken itself is public. Patterns for fully-private asset tracking are an open question for future work.

### 10.5 Schema evolution

The current schema is v1. Future versions may add:

- Geographic location attributes with proper geo data structures
- Time-series valuation history
- Standardized attestation formats
- Integration with specific industry registries

Schema versioning ensures v1 records remain readable.

## 11. Sign-off

This design document is the canonical reference for AssetToken usage in production. Implementations should review the asset-type-specific patterns and consult appropriate counsel for the specific use case.

Astor Rivera
Founder & CTO, SWTCH Labs
astor@swtch.ai
