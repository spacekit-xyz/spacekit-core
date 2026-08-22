# PropertyToken Design Document

**Status:** Production specification
**Version:** 1.0
**Owner:** SWTCH Labs

This document specifies the design, usage patterns, and operational requirements for PropertyToken — the SpaceKit contract for whole-property real estate records. It is the companion document to `PropertyToken.rs` and provides the framework for using the contract in legally operative real estate scenarios.

This document is technical specification combined with operational guidance. It is not legal advice. Implementations require attorney review.

## 1. Purpose and scope

PropertyToken represents a single real estate property as a unique on-chain token. The contract maintains the property's metadata, ownership history, encumbrances, and document references. Combined with a deed addendum (described below), it becomes legally operative as part of the official property record.

**In scope:**
- Single real estate properties (residential, commercial, industrial, agricultural, vacant land, mixed-use)
- Single owner or multiple owners with documented ownership structure
- Encumbrance tracking (mortgages, liens, easements, restrictions, tax liens, HOA)
- Document references (deeds, addendums, title insurance, inspections, surveys, tax assessments, insurance)
- Property transfers with multi-party verification
- Integration with SpaceKit Pay for purchase payments

**Out of scope:**
- Fractional ownership (use FractionalProperty when available)
- Non-real-estate assets (use AssetToken)
- Direct legal authority over property transfer (deed addendum provides legal anchor)
- Title insurance issuance (the contract references title insurance documents but doesn't issue them)
- Title search and clearance (still requires traditional title company services)

## 2. The legal anchor: deed addendum framework

This is the most important section of this document. PropertyToken is not by itself legally operative. It becomes operative through a deed addendum that incorporates it by reference.

### 2.1 What the deed addendum does

A deed addendum is a legal document, signed by the property owner(s) and recorded with the county recorder of deeds (or equivalent authority), that incorporates the smart contract by reference into the official property record.

The addendum says, in essence:

> The undersigned property owner(s) incorporate by reference the digital property record maintained at SpaceKit network contract address [X], property token ID [Y], as part of the official record of this property. Operations performed on the digital record by appropriate parties shall be considered operations on the property to the extent permitted by applicable law and the terms of this addendum.

The actual addendum text is more detailed — see `example-deed-addendum.md` for a template. The template is a starting point; specific jurisdictions and properties require customization by counsel.

### 2.2 What makes the addendum legally operative

For the deed addendum to be legally meaningful:

1. **It must be properly drafted by counsel.** Real estate law is jurisdictional; what works in one state may not work in another.
2. **It must be properly executed** (signed, notarized as required by jurisdiction).
3. **It must be properly recorded** with the appropriate authority (county recorder of deeds typically).
4. **It must include all required content** (legal description of the property, parties, governing law, dispute resolution mechanism).

The smart contract is the easy part. The deed addendum is the substantive legal work.

### 2.3 Operations the addendum makes meaningful

Once the deed addendum is recorded, operations on the PropertyToken can be referenced in subsequent legal proceedings as part of the property record:

- **Encumbrance additions and releases** can be referenced as evidence of liens, mortgages, etc.
- **Document attachments** can be referenced as part of the property's documentation
- **Transfer operations**, when accompanied by appropriate traditional legal instruments (new deeds, new title insurance, etc.), can serve as a cryptographically verifiable record of the transfer

The contract is a record-keeping system that has been incorporated by reference into the official record. It's analogous to how a property's official record might reference an HOA's bylaws maintained at a different address — the bylaws aren't part of the deed but they're incorporated.

### 2.4 What the addendum does NOT do

The deed addendum does NOT:

- Make the smart contract a substitute for proper legal instruments. Deeds still need to be created, signed, notarized, and recorded for transfers.
- Override jurisdictional recording requirements. Deeds must still be filed with the county recorder.
- Eliminate the need for title insurance. Title insurance protects against title defects; the smart contract doesn't.
- Eliminate the need for traditional escrow. Escrow services protect both parties during a transfer; smart contract verification doesn't substitute for fiduciary escrow.

The smart contract complements the existing legal infrastructure; it doesn't replace it.

## 3. Property metadata schema

The PropertyToken stores comprehensive property metadata. Understanding the schema is important for correct usage.

### 3.1 Property identification

- **token_id**: 32-byte unique identifier assigned at mint time
- **property_type**: One of:
  - `1` Residential (single-family home, condo, townhouse)
  - `2` Commercial (office, retail, hospitality)
  - `3` Industrial (warehouse, manufacturing, distribution)
  - `4` Agricultural (farmland, ranch, orchard)
  - `5` Vacant land (undeveloped, unimproved)
  - `6` Mixed-use (multi-purpose property)

### 3.2 Location data

- **country**: ISO 3166-1 alpha-2 country code (e.g., "US")
- **state_province**: State/province within country (e.g., "TX")
- **county**: County or equivalent administrative subdivision
- **city**: City or municipality
- **street_address**: Street address (e.g., "123 Main St")
- **postal_code**: Postal/ZIP code
- **parcel_id**: Tax assessor's parcel ID (essential for property identification)
- **legal_description**: Metes and bounds, or lot/block legal description (essential for legal precision)

The parcel ID and legal description are the most legally important fields. The street address may change (renumbering, renaming) but the parcel ID and legal description are the canonical identifiers used in property records.

### 3.3 Attributes

- **lot_size_sqft**: Lot size in square feet (numeric)
- **building_size_sqft**: Total building size in square feet (0 for vacant land)
- **year_built**: Year of construction (0 for vacant land)

Additional attributes can be added via UPDATE_METADATA. The base schema covers the most common fields; specific properties may have additional attributes (zoning, utilities, school district, etc.) added to the metadata JSON.

### 3.4 Ownership structure

- **ownership_type**: One of:
  - `1` Sole ownership (single owner)
  - `2` Tenants in common (multiple owners with specified percentages)
  - `3` Joint tenants (multiple owners with right of survivorship)
  - `4` Community property (spousal ownership in community property states)
  - `5` Trust (property held by a trust)
  - `6` LLC (property held by a limited liability company)

- **owner_dids**: List of owner DIDs
- **ownership_percentages**: For tenants in common, basis-point percentages summing to 10000 (i.e., 5000 = 50%)

For ownership types other than tenants in common, percentages are equal across owners.

## 4. Encumbrance tracking

Encumbrances are claims on the property that affect its title. PropertyToken tracks them as separate signed records.

### 4.1 Encumbrance types

- `1` Mortgage (loan secured by the property)
- `2` Lien (general claim, e.g., judgment lien)
- `3` Easement (right of access for utility, neighbor, etc.)
- `4` Restriction (HOA covenant, deed restriction)
- `5` Tax lien (unpaid property taxes)
- `6` HOA lien (homeowners association dues)

### 4.2 Encumbrance lifecycle

Each encumbrance has:

- **encumbrance_id**: Unique 32-byte identifier
- **type**: One of the types above
- **holder_did**: The DID of the party holding the encumbrance (bank for mortgage, government for tax lien, etc.)
- **amount_or_terms**: For monetary encumbrances, the amount; for non-monetary, the terms in plain text
- **document_hash**: CAS hash of the supporting document (mortgage agreement, easement document, etc.)
- **expires_at**: Expiration timestamp (0 for indefinite)
- **active**: Boolean indicating whether the encumbrance is currently active

Encumbrances can be added by property owners (recording a mortgage they've taken) OR by the holder (a tax authority adding a tax lien). The contract verifies the caller has appropriate standing.

### 4.3 Releasing encumbrances

When a mortgage is paid off, when an easement expires, when a lien is satisfied — the encumbrance is released via RELEASE_ENCUMBRANCE.

The release is typically performed by the encumbrance holder (e.g., the bank releasing a mortgage after payoff). The contract records the release with timestamp and the releasing party's DID.

Released encumbrances remain in the record (for history) but are marked as inactive. This preserves the audit trail.

## 5. Document references

PropertyToken references off-chain documents stored as CAS blobs in the storage node. The contract maintains a list of document references with metadata.

### 5.1 Document types

- `1` Deed (the deed itself)
- `2` Deed addendum (the addendum incorporating PropertyToken)
- `3` Title insurance (policy document)
- `4` Inspection (home inspection report)
- `5` Survey (property survey)
- `6` Tax assessment (current tax assessment)
- `7` Insurance policy (homeowners insurance)
- `255` Other (anything else)

### 5.2 Document metadata

Each document reference includes:

- **doc_id**: Unique 32-byte identifier
- **type**: Document type
- **hash**: CAS hash of the document blob (32 bytes)
- **title**: Human-readable title (e.g., "Original deed dated 2020-03-15")
- **description**: Optional longer description
- **added_by**: DID that added the reference
- **added_at**: Timestamp

The document itself is stored as a CAS blob; the contract only stores the reference. This keeps the contract storage manageable while preserving full document provenance.

### 5.3 Document privacy

Documents are stored in the SpaceKit storage node according to their access policy:

- **Public documents** (e.g., recorded deeds that are part of the public record) can be stored with public access policy
- **Private documents** (e.g., insurance details, financial details of mortgages) can be encrypted with envelope encryption so only authorized parties can decrypt

The PropertyToken just stores the reference; the storage node handles the access control.

## 6. Property transfer flow

The TRANSFER operation is the most complex. Here's how it works in practice.

### 6.1 Pre-transfer preparation

Before initiating a transfer:

1. **Title search.** Traditional title search by a title company to identify any encumbrances or title defects. The title company can review the PropertyToken's encumbrance list as part of this.

2. **Title insurance.** The buyer typically obtains title insurance to protect against title defects. The policy is attached as a document to the PropertyToken.

3. **Purchase agreement.** Buyer and seller sign a purchase agreement. This agreement may be attached to the PropertyToken as a document, but the contract doesn't require it.

4. **Escrow setup.** If escrow is being used, the title company or escrow agent is configured. The escrow agent will sign the transfer.

5. **New deed preparation.** Counsel prepares the new deed and the updated deed addendum (which references the same PropertyToken but with the new owner).

### 6.2 Transfer execution

The TRANSFER operation accepts:

- **token_id**: The property being transferred
- **new_ownership_type**: Sole, tenants in common, etc.
- **new_owner_count**: Number of new owners
- **new_owner_dids**: List of new owner DIDs
- **new_percentages**: For tenants in common, ownership percentages
- **deed_addendum_hash**: CAS hash of the new deed addendum
- **payment_confirmation**: CAS hash of payment transaction (e.g., SpaceKit Pay receipt; all zeros if no payment via SpaceKit)

The caller must be a current owner. The contract:

1. Verifies caller is in the current owner list
2. Updates the owners record to the new owner list
3. Updates the reverse indices (removes old owners' association with this token, adds new owners')
4. Appends a transfer entry to the history with all transfer metadata
5. Emits the `property.transferred` event

After the on-chain transfer:

1. **New deed is filed** with the county recorder
2. **New deed addendum is filed** with the county recorder
3. **Title insurance policy is updated** to reflect new owner
4. **Property tax records** are updated by the tax assessor

The on-chain transfer and the off-chain filings happen as part of one coordinated transaction (typically managed by the title company or closing attorney).

### 6.3 SpaceKit Pay integration

When the property purchase is paid via SpaceKit Pay:

1. Buyer authorizes payment via SpaceKit Pay
2. Payment routes to escrow agent (typically title company)
3. Title company holds funds until closing conditions are met
4. At closing, title company releases funds to seller (or to seller's designated recipient)
5. SpaceKit Pay receipt is generated; receipt hash is included in the PropertyToken TRANSFER operation as `payment_confirmation`

This creates a verifiable cryptographic link between the payment and the transfer. Auditors can trace from the PropertyToken to the SpaceKit Pay payment to the stablecoin transfers.

### 6.4 Multi-party signature requirements

Different transfer scenarios may require different signature sets:

- **Direct owner-to-owner transfer** (no escrow): Only the seller's signature required
- **Transfer through escrow agent**: Both seller's signature AND escrow agent's signature (the escrow agent verifies payment is complete)
- **Transfer with title company**: Both seller's signature AND title company's signature (the title company confirms title is clear)

The contract verifies the caller is a current owner. Additional signatures (escrow, title company) are tracked via separate signed facts referenced by the transfer's metadata. Specific signature policies can be configured per-property in the metadata.

## 7. SpaceKit Pay integration

PropertyToken is designed to integrate with SpaceKit Pay for property purchase payments.

### 7.1 The integration pattern

When a property is being sold:

1. Buyer connects wallet, has SpaceKit Workspaces account
2. Seller has PropertyToken with active deed addendum
3. Title company (or escrow agent) is configured as a party
4. Buyer initiates payment via SpaceKit Pay to escrow address
5. SpaceKit Pay routes payment: 95% to seller (deferred via escrow), 5% to SpaceKit treasury
6. Title company verifies clear title, releases escrow to seller
7. PropertyToken TRANSFER records the buyer as new owner, with SpaceKit Pay receipt hash

### 7.2 Why this is valuable

The integration provides:

- **Atomic payment-and-record-update.** Payment and the on-chain record update happen in coordinated transaction. No "paid but record not updated" or "record updated but payment didn't go through" failure modes.

- **Cryptographic audit trail.** Every payment can be traced to its property record; every property record can be traced to its payment.

- **Reduced settlement friction.** SpaceKit Pay routing is faster than traditional wire transfers; settlement happens in minutes rather than hours.

- **Lower transaction costs.** SpaceKit Pay's 5% treasury fee is comparable to or lower than traditional escrow fees for many transactions.

### 7.3 What still requires traditional infrastructure

Even with SpaceKit Pay integration:

- **Title insurance** is still needed and still issued by traditional title companies
- **Recording fees** to the county recorder are still required
- **Property taxes** still flow through traditional tax authorities
- **Mortgage payoffs** to existing lenders may use traditional banking rails

SpaceKit Pay handles the buyer-seller payment; everything else uses existing infrastructure.

## 8. Operational integration with title companies

For PropertyToken to be useful at scale, title companies need to integrate with it. Here's the integration pattern.

### 8.1 What title companies do

Traditional title companies:

- Conduct title searches
- Issue title insurance
- Manage escrow
- Record deeds with county recorders
- Manage closing logistics

### 8.2 What changes with PropertyToken

Title companies can offer PropertyToken integration as a service. Their integration includes:

- **DID registration.** Title company has a DID for the PropertyToken transfer signatures
- **Title search includes on-chain encumbrance review.** In addition to traditional title search, the title company queries the PropertyToken's encumbrances
- **Deed addendum drafting.** Title company drafts the deed addendum (in coordination with attorneys) that incorporates the PropertyToken
- **Closing includes PropertyToken TRANSFER operation.** At closing, the title company executes the TRANSFER in addition to traditional closing steps
- **Recording includes deed addendum.** Title company files the deed addendum along with the new deed

### 8.3 Title company integration roadmap

Phase 1 (initial): SWTCH Labs identifies one or two title company partners willing to pilot PropertyToken integration. Pilot in a single jurisdiction (likely Texas or another state with progressive real estate technology adoption).

Phase 2 (expansion): Successful pilots expand to additional title companies and jurisdictions. SWTCH Labs publishes implementation guides for title companies.

Phase 3 (standardization): PropertyToken becomes a recognized option in title company workflows. Standard integration libraries, training materials, certification programs.

This is multi-year work. The contract is the foundation; the title company integration is the product that makes it broadly usable.

## 9. Open questions and limitations

A few honest acknowledgments:

### 9.1 Jurisdiction variability

US real estate law varies state by state. The deed addendum pattern is generally workable but specifics differ. Some jurisdictions:

- Have electronic recording (eRecording) that works well with this pattern
- Have paper-only recording that adds friction
- Have specific statutory requirements for deeds that may need adjustment
- Have community property laws that affect ownership types

PropertyToken is designed to be jurisdiction-agnostic, but production use requires jurisdiction-specific counsel.

### 9.2 Title insurance gaps

Title insurance protects against title defects that arise from issues NOT discovered during title search. PropertyToken doesn't change this; if a title defect exists (forged signature on a prior deed, undisclosed heir, boundary dispute), it exists regardless of what the PropertyToken says.

Users should NOT rely on PropertyToken as a substitute for title insurance.

### 9.3 Quiet title and adverse possession

Some legal property concepts (quiet title actions, adverse possession claims) involve property records being legally challenged. PropertyToken doesn't make properties immune to these challenges; they would be decided through traditional courts.

### 9.4 International considerations

This document focuses on US real estate. International real estate has very different legal frameworks. PropertyToken can be used internationally but requires substantial jurisdiction-specific legal work to make it operative.

### 9.5 Future schema evolution

The current schema is v1. Future versions may add:

- More detailed location data (lat/lng, geocoded boundaries)
- Time-series property characteristics (assessed value history, occupancy history)
- Multi-language support for legal descriptions
- Photo and visual reference attachments built into the schema

Schema versioning ensures v1 records remain readable as the ecosystem evolves.

## 10. Sign-off

This design document is the canonical reference for PropertyToken usage in production. Implementations require legal counsel and may require additional engineering work for specific use cases.

Astor Rivera
Founder & CTO, SWTCH Labs
astor@swtch.ai
