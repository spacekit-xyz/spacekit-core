# Deed Addendum Template — PropertyToken Incorporation

**Status:** Template document
**Version:** 1.0
**Owner:** SWTCH Labs

This is a template deed addendum that incorporates a SpaceKit PropertyToken into the official record of a real property. It is provided as a starting point for use by real estate attorneys.

## CRITICAL DISCLAIMERS

**This template is NOT legal advice.** It is a starting point for attorneys to customize for specific transactions, jurisdictions, and properties.

**This template MUST be reviewed by a real estate attorney** licensed in the jurisdiction where the property is located before use. Real estate law varies significantly by state and country; this template addresses common US patterns but is not suitable for use without jurisdictional adaptation.

**Do not use this template without:**
1. Review by a licensed real estate attorney in the relevant jurisdiction
2. Customization to the specific property, parties, and transaction
3. Confirmation that the recording authority will accept the addendum
4. Proper notarization or witnessing per jurisdictional requirements
5. Coordination with title insurance underwriter (if title insurance is being issued)

The template below is suitable for residential real estate transactions in US states with electronic recording (eRecording) and progressive real estate technology adoption. Other contexts may require substantial modification.

---

## Template Begins Below

---

# DEED ADDENDUM

## Incorporation of SpaceKit PropertyToken Digital Property Record

**[PROPERTY ADDRESS]**
**[PARCEL ID / LEGAL DESCRIPTION]**

---

### Parties

**GRANTOR(S):**
[Full legal name(s) of grantor(s)]
[Mailing address]
SpaceKit DID: [did:spacekit:...]

**GRANTEE(S):**
[Full legal name(s) of grantee(s)]
[Mailing address]
SpaceKit DID: [did:spacekit:...]

---

### Property

The property subject to this Addendum is more particularly described as:

**Street Address:** [Address]

**County:** [County], **State:** [State]

**Parcel ID:** [Parcel Identification Number]

**Legal Description:**

[Insert full legal description as it appears in the official deed, including all metes and bounds, lot/block references, subdivision references, or other identifying information required by the recording authority]

---

### Recital

WHEREAS, the Grantor(s) named above is/are the lawful owner(s) of the Property described above, holding fee simple title (or such lesser interest as may be indicated below);

WHEREAS, the Grantor(s) seek to incorporate, by reference into the official record of the Property, certain digital records maintained on the SpaceKit blockchain network, which records shall serve as a supplementary record of the Property's ownership, encumbrances, and related documents;

WHEREAS, this incorporation is intended to complement, not replace, the traditional legal instruments governing the Property (including but not limited to deeds, title insurance, and the records of the County Recorder of Deeds);

NOW, THEREFORE, the Grantor(s) hereby incorporate the following digital record into the official record of the Property:

---

### Article I — Incorporation of SpaceKit PropertyToken

**Section 1.1 — Incorporation by Reference.**

The Grantor(s) hereby incorporate by reference into the official record of the Property the digital property record maintained on the SpaceKit blockchain network at:

**SpaceKit Contract Address:** [Insert SpaceKit Network address of PropertyToken contract]

**Property Token ID:** [Insert 64-character hexadecimal token ID]

**Contract Schema:** spacekit:property:v1

**SpaceKit Network:** [Mainnet / Testnet — must be Mainnet for legally operative use]

The digital record at this Contract Address and Token ID shall be referred to herein as the "Digital Property Record."

**Section 1.2 — Authoritative Records.**

The official record of the Property continues to be maintained by the [County Name] County Recorder of Deeds. This Addendum supplements but does not replace that official record. In the event of any conflict between the Digital Property Record and the official record, the official record shall prevail.

**Section 1.3 — Verification.**

The Digital Property Record can be verified by any party with access to the SpaceKit blockchain network using standard SpaceKit tooling. Verification confirms:

(a) The current owner(s) of the Property as recorded in the Digital Property Record;

(b) The historical ownership transfers recorded in the Digital Property Record;

(c) The encumbrances recorded against the Digital Property Record;

(d) The documents referenced in the Digital Property Record by their content hashes;

(e) The cryptographic signatures verifying each operation recorded in the Digital Property Record.

---

### Article II — Recognized Operations

**Section 2.1 — Operations Recognized by This Addendum.**

The following operations performed on the Digital Property Record shall be considered supplementary records of the Property when accompanied by appropriate traditional legal instruments and signatures:

(a) Encumbrance Addition: When a lien, mortgage, easement, or similar encumbrance is added to the Digital Property Record by the appropriate party (the encumbrance holder, the property owner with the consent of the encumbrance holder, or both as required by law), the Digital Property Record's encumbrance entry shall serve as a supplementary record of such encumbrance.

(b) Encumbrance Release: When an encumbrance is marked as released in the Digital Property Record by the appropriate party (typically the encumbrance holder), the Digital Property Record's release entry shall serve as a supplementary record of such release.

(c) Document Reference: When a document is referenced in the Digital Property Record by its content hash, and the document itself is available through the SpaceKit storage system, such reference shall serve as a supplementary record of the document.

(d) Property Transfer: When the ownership recorded in the Digital Property Record is updated to reflect a transfer, and such transfer is accompanied by a properly executed and recorded deed (or equivalent legal instrument), the updated Digital Property Record shall serve as a supplementary record of the transfer.

**Section 2.2 — Operations NOT Constituting Legal Authority.**

For clarity, no operation on the Digital Property Record shall, on its own, transfer title, create or release encumbrances, or otherwise affect the legal status of the Property without the corresponding traditional legal instrument also being properly executed and recorded.

A transfer recorded only in the Digital Property Record, without a properly executed and recorded deed, shall not transfer title.

An encumbrance recorded only in the Digital Property Record, without the legal documentation traditionally required for such encumbrance, shall not create a valid lien.

---

### Article III — Maintenance and Updates

**Section 3.1 — Duration.**

This Addendum shall remain in effect for the entire duration of the Property's existence as a separate legal parcel, subject only to release as provided in Section 3.3.

**Section 3.2 — Successor Owners.**

This Addendum binds and benefits not only the original Grantor(s) and Grantee(s), but also any subsequent owners of the Property. A successor owner taking title to the Property takes title subject to this Addendum and may use the same Digital Property Record (with updated ownership entries) without requiring a new addendum, provided the Digital Property Record continues to reference the same Contract Address and Token ID.

**Section 3.3 — Release of Addendum.**

This Addendum may be released by:

(a) Agreement of all then-current owners of the Property, executed and recorded with the same formality as this Addendum;

(b) An order of a court of competent jurisdiction;

(c) Operation of law (e.g., extinguishment of the Property as a separate legal parcel).

Upon release, the Digital Property Record shall no longer be considered incorporated into the Property's official record, though the Digital Property Record itself may continue to exist on the SpaceKit blockchain network.

---

### Article IV — Authentication and Authority

**Section 4.1 — Authority of Smart Contract Operations.**

Operations performed on the SpaceKit PropertyToken contract are recorded with cryptographic signatures (SPHINCS+ post-quantum signatures) issued by Decentralized Identifiers (DIDs) representing the parties performing the operations.

A signature on a Digital Property Record operation by a DID shall be considered an act of the DID's controlling party for purposes of this Addendum, provided:

(a) The DID is properly registered on the SpaceKit network;

(b) The signature can be cryptographically verified against the DID's registered public key;

(c) The operation was timestamped appropriately and not replayed.

**Section 4.2 — Multi-Party Operations.**

Operations requiring multi-party authentication (such as property transfers requiring both grantor and grantee signatures, or escrow-mediated transfers requiring an escrow agent's signature) shall be considered complete only when all required signatures are verified.

**Section 4.3 — Identity Verification.**

The relationship between SpaceKit DIDs and individual or organizational identities shall be established through standard identity verification means (driver's licenses, articles of incorporation, etc.). This Addendum does not create or modify identity verification requirements; it relies on identity verification performed through other means.

---

### Article V — Disputes and Governing Law

**Section 5.1 — Governing Law.**

This Addendum shall be governed by the laws of the State of [State Name], without regard to its conflict of laws provisions.

**Section 5.2 — Jurisdiction.**

Any dispute arising from this Addendum or its interpretation shall be brought in the [County Name] County [Court Name] of the State of [State Name].

**Section 5.3 — Resolution of Conflicts.**

In the event of any apparent conflict between the Digital Property Record and the official record maintained by the County Recorder of Deeds, the official record shall be considered authoritative for legal purposes. However, the Digital Property Record may be admissible as evidence of facts (such as the dates of certain actions, the identities of parties signing certain operations, etc.) to the extent permitted by applicable rules of evidence.

**Section 5.4 — Effect of SpaceKit Network Disruption.**

In the event that the SpaceKit blockchain network ceases to operate, becomes inaccessible, or otherwise fails, this Addendum shall not be affected as to the Property's continued status. The traditional legal instruments governing the Property remain in full force regardless of the operational status of the SpaceKit network.

---

### Article VI — Acknowledgment of Limitations

**Section 6.1 — Title Insurance.**

The parties acknowledge that this Addendum does not constitute or substitute for title insurance. The parties are advised to maintain appropriate title insurance coverage for the Property.

**Section 6.2 — Escrow Services.**

The parties acknowledge that this Addendum does not constitute or substitute for traditional escrow services. Transactions involving the Property should continue to use appropriate escrow arrangements when warranted.

**Section 6.3 — Title Search.**

The parties acknowledge that this Addendum does not constitute or substitute for traditional title search. Buyers and lenders should continue to conduct appropriate title searches before transactions involving the Property.

**Section 6.4 — Property Recording Authority.**

The parties acknowledge that the [County Name] County Recorder of Deeds remains the authoritative recording authority for the Property. This Addendum, once recorded, becomes part of the official record but does not change the recording authority.

---

### Article VII — Signatures and Execution

The undersigned have executed this Addendum as of the date(s) set forth below.

---

**GRANTOR:**

_________________________________  
[Printed name]  
Date: ______________________  
SpaceKit DID: [did:spacekit:...]

**STATE OF [STATE]**

**COUNTY OF [COUNTY]**

On this _____ day of _____________, 20___, before me personally appeared [Grantor name(s)], known to me (or satisfactorily proven) to be the person(s) whose name(s) is/are subscribed to the foregoing instrument, and acknowledged that he/she/they executed the same for the purposes therein contained.

In witness whereof, I hereunto set my hand and official seal.

_________________________________  
Notary Public  
My Commission Expires: ___________  
[Notary Seal]

---

**GRANTEE:**

_________________________________  
[Printed name]  
Date: ______________________  
SpaceKit DID: [did:spacekit:...]

**STATE OF [STATE]**

**COUNTY OF [COUNTY]**

On this _____ day of _____________, 20___, before me personally appeared [Grantee name(s)], known to me (or satisfactorily proven) to be the person(s) whose name(s) is/are subscribed to the foregoing instrument, and acknowledged that he/she/they executed the same for the purposes therein contained.

In witness whereof, I hereunto set my hand and official seal.

_________________________________  
Notary Public  
My Commission Expires: ___________  
[Notary Seal]

---

## Template Ends Here

---

## Notes for Attorneys Using This Template

### Required customizations

The template above is a starting point. Required customizations include:

1. **Property identification.** Insert the actual property address, parcel ID, and full legal description in the appropriate places.

2. **Party identification.** Insert the grantor and grantee names, addresses, and DIDs.

3. **SpaceKit references.** Insert the actual contract address and token ID being incorporated.

4. **Jurisdiction.** Update Section 5.1 governing law, Section 5.2 jurisdiction, and the notary acknowledgments per the actual jurisdiction.

5. **Recording requirements.** Customize for the specific recording authority's requirements (margin sizes, paper format, specific recital language, etc.).

### Common adjustments

Depending on jurisdiction and transaction, you may need to adjust:

1. **Witness requirements.** Some jurisdictions require witnesses in addition to (or instead of) notarization. The template uses notarization; adapt as needed.

2. **Marriage and homestead.** In community property states, spouse signatures may be required. Add spouse signature blocks if applicable.

3. **Entity grantor/grantee.** If a party is an LLC, corporation, trust, or other entity rather than an individual, the signature block needs to reflect the entity's signing requirements (officer signatures with corporate seals, trustee signatures with trust acknowledgments, etc.).

4. **Multiple owners.** If there are multiple grantors or grantees, add signature blocks for each. For tenants in common, document the ownership percentages.

5. **Mortgage transactions.** If the property has an existing mortgage, the mortgage holder may need to consent to the addendum or be notified. Add provisions as required.

6. **Title insurance underwriter requirements.** If title insurance is being issued for the property, the underwriter may have specific requirements for what the addendum should and shouldn't say. Coordinate with the underwriter.

### What attorneys should verify

Before signing off on this addendum for a specific transaction:

1. **The PropertyToken contract is correctly deployed.** Verify the contract address and that the token ID exists and refers to the right property.

2. **The grantor's DID controls the property.** Verify that the grantor's claimed DID actually has ownership recorded in the PropertyToken.

3. **The recording authority will accept the addendum.** Confirm with the county recorder's office that the addendum format is acceptable.

4. **Title insurance compatibility.** If title insurance is involved, confirm the underwriter is comfortable with the addendum.

5. **No outstanding liens or encumbrances unaccounted for.** Run a traditional title search and reconcile with the PropertyToken's encumbrance list.

### Recording the addendum

After execution:

1. The addendum is filed with the same recording authority that handles deeds (typically the County Recorder of Deeds).

2. The addendum is recorded in connection with the underlying deed transaction.

3. The recorded addendum becomes part of the public record of the property.

4. Future transactions involving the property should reference both the underlying deed and this addendum.

### Subsequent transfers

When the property is later transferred:

1. The current owner uses the PropertyToken's TRANSFER operation, signing with their DID.

2. The new deed is recorded with the recording authority.

3. A new deed addendum is filed (similar to this template) confirming the new owner's incorporation of the same PropertyToken with their DID.

The PropertyToken persists across ownership changes; the deed addendums are updated to reflect new owners.

### Title company integration

Title companies wanting to integrate with PropertyToken in their workflows should:

1. **Register the title company's DID** on the SpaceKit network.

2. **Update closing checklists** to include PropertyToken-related steps (verify ownership, check encumbrances, execute TRANSFER, file new addendum).

3. **Train closing officers** on the technical and legal aspects of PropertyToken transactions.

4. **Coordinate with their title insurance underwriter** on how PropertyToken-related closings should be handled.

5. **Establish technical infrastructure** for connecting their systems to the SpaceKit network.

This is real product integration work that takes weeks to months for a title company to set up properly. The benefit is faster, cheaper, more secure real estate transactions with full audit trails.

---

## Document version

Version 1.0 (initial release)

Future versions of this template will be released as the deed addendum framework matures based on real-world usage and feedback from real estate attorneys.

## Contact

For questions about this template or PropertyToken usage:

Astor Rivera
Founder & CTO, SWTCH Labs
astor@swtch.ai
