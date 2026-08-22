# Regulatory Considerations — PropertyToken and AssetToken

**Status:** Calibrated regulatory framing
**Version:** 1.0
**Owner:** SWTCH Labs

This document provides regulatory framing for the PropertyToken and AssetToken contracts. It is intended to help users understand what these contracts are, what they are not, and what legal infrastructure is required to make them legally operative.

**This document is NOT legal advice.** It is a calibrated overview of regulatory considerations that arise in real-world use. Implementations require legal counsel familiar with the specific use case, jurisdiction, and asset type.

## 1. What these contracts are NOT

Starting with explicit negatives because confusion about these is the most common source of regulatory exposure:

**These contracts are NOT securities or investment instruments.**

A PropertyToken or AssetToken representing a single property or asset is not a security. It is a digital record of property ownership, not an investment vehicle. The token doesn't pay dividends, doesn't trade in fractional units to multiple holders, doesn't represent a share of a larger enterprise.

If your use case involves issuing many tokens representing fractional ownership of a property with the expectation of profits, you should use a different contract (FractionalProperty, which is not yet released and will have its own regulatory framework requiring Reg D 506(c) exemption or equivalent).

**These contracts are NOT money transmission.**

PropertyToken and AssetToken don't transmit money. SpaceKit Pay (a separate contract) handles payment routing. PropertyToken and AssetToken handle property records. The separation is intentional and important regulatorily.

When PropertyToken integrates with SpaceKit Pay for purchase payments, the money transmission analysis is for SpaceKit Pay (which is non-custodial per FinCEN guidance), not for PropertyToken.

**These contracts are NOT a substitute for legal documentation.**

A PropertyToken does not transfer real estate ownership. A deed (or equivalent legal instrument) recorded with the relevant authority is what transfers ownership. The PropertyToken is supplementary record.

An AssetToken does not transfer vehicle ownership. The state-issued vehicle title is what transfers ownership. The AssetToken is supplementary record.

This is the most important regulatory framing: the contracts are records, not legal authority. The legal authority remains with traditional instruments (deeds, titles, certificates) recorded with traditional authorities (recorders, registries, etc.).

**These contracts do NOT create new legal frameworks.**

Real estate law continues to apply to PropertyToken-associated properties. Vehicle title law continues to apply to AssetToken-tracked vehicles. The contracts work within existing legal frameworks, not around them.

## 2. What these contracts ARE

Now the positives:

**These contracts ARE records of ownership and history.**

They maintain a cryptographically verifiable record of who owns what, when transfers occurred, and what documents are associated. This is useful for:

- Audit trails for property records
- Provenance tracking for art and collectibles
- Asset management for businesses
- Continuous documentation of property history

**These contracts ARE supplementary to legal instruments.**

When properly anchored (via deed addendum for real estate, or via appropriate documentation for other assets), they become part of the official record. They don't replace the official record; they supplement it.

**These contracts ARE cryptographically authentic.**

Every operation is signed by the appropriate DIDs using post-quantum cryptography (SPHINCS+). Forgery is computationally infeasible. The cryptographic authenticity is real, even though the legal force depends on the off-chain anchoring.

**These contracts ARE persistent.**

As long as SpaceKit operators maintain the storage, the records persist. The contracts are designed for decades of operation. Documents stored as CAS blobs are durable.

## 3. Federal (US) considerations

Several federal regulatory frameworks may apply depending on use case.

### 3.1 Securities law

**For PropertyToken (whole property):** Generally not a security under Howey. The token represents ownership of a specific real property (not an investment contract), is not sold to investors as a class, doesn't have an expectation of profits from the efforts of others, doesn't have a common enterprise structure.

**For AssetToken (general assets):** Generally not a security, same reasoning. The token represents ownership of a specific asset.

**Caveat:** If a single token is used as part of a structured arrangement that does have securities characteristics (e.g., multiple tokens being sold to multiple parties with a common business expectation), the entire arrangement might be a security regardless of the underlying contract. The contract design alone doesn't determine securities status; the business structure does.

**Recommendation:** Single-owner or joint-owner use cases of single properties or assets are not securities. Multi-investor schemes built on top of these contracts may be. Consult counsel for specific structures.

### 3.2 Money transmission (FinCEN)

**FinCEN FIN-2019-G001** provides guidance on non-custodial software in cryptocurrency transactions. The reasoning extends to property transactions where appropriate.

**For PropertyToken/AssetToken alone:** The contracts don't transmit money. They record property states. No money transmission occurs.

**For PropertyToken/AssetToken integrated with SpaceKit Pay:** SpaceKit Pay's non-custodial design (per FIN-2019-G001) means it's not money transmission. The combined system inherits this property.

**For traditional escrow integration:** Title companies, escrow agents, and similar parties may be money services businesses under their own regulatory framework, but their handling of payments related to PropertyToken-associated properties doesn't change their regulatory status.

### 3.3 Anti-money laundering (AML)

Real estate is a known vector for money laundering (high-value purchases, opaque ownership structures, etc.). FinCEN has expanded reporting requirements for cash purchases of high-value real estate in certain markets.

**PropertyToken implications:** The contracts make ownership history more transparent than traditional opaque title arrangements. This is generally helpful for AML compliance, not harmful.

**However:** The contracts don't perform KYC/AML themselves. They record what DIDs are associated with properties; the identity verification (linking DIDs to actual people or entities) happens through other means.

For high-value transactions, traditional KYC/AML requirements continue to apply through the title company, attorney, or other intermediary handling the transaction.

### 3.4 IRS and tax considerations

Property records affect taxes (capital gains on sales, depreciation on equipment, gift tax on transfers without consideration, etc.).

**Recommendation:** PropertyToken and AssetToken records can be used to support tax filings (providing evidence of acquisition dates, costs, improvements, etc.), but they don't change the underlying tax treatment.

Cryptocurrency transactions involving SpaceKit Pay receive their own tax treatment (capital gains on stablecoin transactions, etc.). Consult tax counsel.

### 3.5 Treasury sanctions (OFAC)

Property transactions with sanctioned parties are prohibited. Title companies and other intermediaries typically screen for sanctions compliance.

**Recommendation:** PropertyToken transactions involving SpaceKit Pay should include appropriate sanctions screening at the intermediary level (title company, attorney). The contracts don't perform sanctions screening themselves.

## 4. State-level considerations (US)

US real estate law varies significantly by state. Several considerations apply broadly:

### 4.1 Recording requirements

Most states require deeds to be recorded with the county recorder of deeds to be effective against subsequent purchasers. The deed addendum framework requires the same recording — file the addendum with the deed.

Some states have specific recording formats, margin requirements, font requirements, etc. The deed addendum template needs adaptation to these specifics.

### 4.2 Electronic recording (eRecording)

Most US states now allow electronic recording of real estate documents. This is helpful for PropertyToken integration:

- The deed addendum can be filed electronically alongside the deed
- The integration is more efficient than paper-based recording
- Records are more easily accessed and verified

States with full eRecording: most states by population. States with partial or no eRecording: typically more rural states.

### 4.3 Property tax considerations

Property taxes are assessed by local authorities (county assessor typically). PropertyToken doesn't change tax obligations; it can provide better evidence for assessments and transfers.

### 4.4 Community property states

Nine states have community property (CA, AZ, ID, LA, NV, NM, TX, WA, WI). In these states, property acquired during marriage is generally community property regardless of how title is held.

**PropertyToken implications:** When recording ownership in community property states, ensure both spouses are properly reflected (typically as joint owners or with community property designation). Single-spouse PropertyToken records of community property might create complications.

### 4.5 Homestead laws

Many states have homestead protections that affect how primary residences can be sold or encumbered. These laws typically require spousal consent.

**PropertyToken implications:** Transfers of homesteaded properties require all required signatures, which may include a spouse not listed as an owner.

## 5. Jurisdiction-specific considerations

### 5.1 Texas

Texas has been progressive in real estate technology adoption. Electronic recording is widespread. The deed addendum framework should work well here.

Texas also has community property; spouse signatures may be required.

### 5.2 California

California has electronic recording in most counties. Real estate law is similar to other states but with specific transfer tax (documentary transfer tax) considerations.

### 5.3 Florida

Florida has electronic recording. Homestead exemption is constitutional and affects how primary residences can be sold.

### 5.4 New York

New York has electronic recording but significant variation by county. Specific recording requirements may apply (typewritten requirements, specific paper sizes, etc.).

### 5.5 Other US states

Each state has its own specifics. The deed addendum template is a starting point; local attorney review is essential.

### 5.6 International

International real estate has very different legal frameworks:

- **UK:** Land Registry has been digitizing for years; there's an existing electronic record system. PropertyToken would supplement but not replace the Land Registry.
- **EU:** Varies by country. Some countries have well-developed land registries; others have less centralized systems.
- **Mexico:** Real estate ownership is governed by state laws; certain foreign ownership restrictions apply.
- **Other countries:** Highly variable. Some countries have no centralized land registry at all.

International use of PropertyToken requires substantial jurisdiction-specific legal work.

## 6. Asset-specific regulatory considerations

For AssetToken, regulatory frameworks vary widely by asset type.

### 6.1 Vehicles

**US:** State DMV systems are the authoritative recordkeepers for vehicles. AssetToken supplements but doesn't replace DMV records. Title transfers must go through DMV.

**International:** Each country has its own vehicle registration system.

### 6.2 Art

**Authentication:** Art authentication is largely a private market matter, not heavily regulated except for very high-value items where money laundering concerns apply (FATF guidance).

**Cultural property:** Some art has cultural property restrictions (export limits, indigenous artifacts, looted art concerns). AssetToken doesn't address these; due diligence is required.

### 6.3 Equipment

**Tax considerations:** Equipment depreciation has specific tax rules. AssetToken can provide evidence for tax filings.

**Regulatory equipment:** Some equipment is regulated (medical devices, aircraft, watercraft, etc.). AssetToken doesn't substitute for the relevant regulatory registration.

### 6.4 Intellectual property

**Authoritative registries:** IP registries (USPTO, EUIPO, etc.) are the authoritative authorities. AssetToken supplements but doesn't replace.

**Licensing:** IP licensing has specific contractual requirements; AssetToken can record license assignments but doesn't substitute for proper licensing agreements.

### 6.5 Livestock

**Animal welfare regulations:** Various federal and state regulations govern livestock. AssetToken can provide records useful for compliance.

**Brand registration:** State brand registration is the authority for livestock ownership in ranching states.

### 6.6 Precious metals

**Vault custody:** When precious metals are stored in commercial vaults, the vault is typically the legal custodian. AssetToken can record ownership but doesn't substitute for the vault's records.

### 6.7 Digital assets

**Domain names:** Authoritative registrar records are the legal authority.

**NFTs:** Already on-chain; AssetToken would be redundant for most NFT use cases.

## 7. Operational compliance

Several operational considerations apply:

### 7.1 Privacy and data protection

Property records may contain personal information. Privacy laws may apply:

- **GDPR (EU):** Personal data in PropertyToken records of EU properties may be subject to GDPR. Right to deletion creates complications since the records are immutable.
- **CCPA (California):** Similar considerations apply for California residents.
- **HIPAA:** Health information should not be stored in property records.

Encrypted documents (using envelope encryption) can mitigate some privacy concerns by making personal information accessible only to authorized parties.

### 7.2 Record retention

Many jurisdictions require property records to be retained for specific periods (often 7+ years for tax purposes). PropertyToken records persist indefinitely, satisfying retention requirements.

### 7.3 Audit and discovery

Property records may be subject to subpoena or audit. PropertyToken records are cryptographically verifiable and can support audit requirements, but parties should understand that records are not anonymous — the chain of custody is visible.

## 8. Withers Worldwide engagement

SWTCH Labs has engaged Withers Worldwide for legal review of these contracts. The engagement covers:

- General contract design review for US federal and state considerations
- The deed addendum framework
- The relationship between PropertyToken/AssetToken and SpaceKit Pay
- The relationship between PropertyToken/AssetToken and ASTRA

Withers's review does NOT cover:

- Jurisdiction-specific advice for all US states or countries (would require local counsel)
- Specific transaction advice (would require counsel for that transaction)
- Asset-type-specific advice for all asset categories (would require counsel for that asset type)

For specific use cases, additional legal counsel is required beyond Withers's general review.

## 9. Operational recommendations

For users considering PropertyToken or AssetToken in production:

### 9.1 Start with a legal review

Before any production use, engage a real estate attorney (for PropertyToken) or asset-specific attorney (for AssetToken) familiar with the relevant jurisdiction and asset type.

### 9.2 Use the templates as starting points only

The example deed addendum template is a starting point. Real use requires customization by counsel.

### 9.3 Coordinate with title companies (for real estate)

Title companies are critical partners for real estate transactions. Engaging with one or more title companies early in the implementation helps identify operational issues and integration challenges.

### 9.4 Maintain traditional records

Even with PropertyToken/AssetToken, maintain traditional records (deeds, titles, certificates, insurance policies, etc.). The on-chain records are supplementary, not replacement.

### 9.5 Train relevant parties

If operating at scale, train relevant parties (employees, attorneys, title companies, real estate agents, asset specialists) on the contracts and their proper use. The technology is useful only if used correctly.

### 9.6 Plan for record migration

If switching from another system to PropertyToken/AssetToken, plan the migration carefully. Historical records may need to be imported as documents; current ownership and encumbrances need to be reflected accurately.

## 10. Honest limitations

Several things these contracts don't solve:

### 10.1 Legal anchor adoption

The deed addendum framework requires title company and attorney adoption. Without partners willing to use it, PropertyToken's legal operativeness is limited.

### 10.2 Cross-jurisdiction harmonization

There's no universal legal framework for real estate or assets across jurisdictions. PropertyToken works within existing frameworks; it doesn't create new harmonized frameworks.

### 10.3 Title insurance evolution

Title insurance products haven't evolved to specifically address blockchain-anchored records. Underwriters may have specific concerns or requirements that need to be worked out.

### 10.4 Recording authority adoption

County recorders of deeds vary in their technology adoption. Some are ready for digital integration; others are not. The deed addendum framework requires acceptance by the recording authority.

### 10.5 User education

Most users are unfamiliar with blockchain-based property records. Adoption requires significant user education and trust building.

## 11. Sign-off

This regulatory considerations document provides calibrated framing for PropertyToken and AssetToken use. It is not legal advice. Specific use cases require legal counsel.

Astor Rivera
Founder & CTO, SWTCH Labs
astor@swtch.ai
