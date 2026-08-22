---
title: "Content Plan — Fundraise Window"
subtitle: "Cross-blog navigation, Welcome to Web4 edit, and upcoming post calendar"
author: "Internal · SpaceKit by swtch labs · 2026"
geometry: margin=0.6in
fontsize: 9.5pt
colorlinks: true
linkcolor: NavyBlue
urlcolor: NavyBlue
---

This document captures the three remaining content tasks from the revised recommendation list — items 2, 3, and 4. The first task (Standard Library post for swtch.ai) is shipped separately. This document is internal — it's the plan, not the published content.

## Task 2 — Cross-blog navigation between swtch.ai and spacekit.xyz

**The problem.** You currently have two blogs. swtch.ai/blog (Signal Log) is the founder-voice technical blog. spacekit.xyz/blog is the product/team-voice blog. Each blog is good for its audience, but neither one tells the reader the other exists. An investor landing on either blog has no idea there's a parallel content stream they should also be reading.

**The fix.** Add a navigation block to each blog pointing to the other. Two implementations, pick whichever fits the existing site structure better.

### Option A: Inline cross-link at the top of each blog index

On `swtch.ai/blog` index page, immediately below the page title/intro:

> *Looking for SpaceKit product updates and creator-focused news? See the [SpaceKit blog](https://spacekit.xyz/blog).*

On `spacekit.xyz/blog` index page, immediately below the page title/intro:

> *Looking for engineering, research, and architecture posts from the founder? See the [Signal Log at swtch labs](https://swtch.ai/blog).*

This is the lowest-friction implementation. One line per index page. Reader sees it immediately on landing.

### Option B: Sidebar block on every blog post

A small sidebar or footer block on every individual post page. Wider footprint but seen on every page rather than only on the index.

**Recommendation: ship Option A first.** It's a 5-minute change per site. Option B can follow if you want it later but isn't critical.

### Voice difference is intentional

When implementing, do not make the two blogs sound the same. The voice difference is a feature, not a bug:

- **swtch.ai/blog** is founder-voice, technical, named author byline (Astor Rivera, Founder & CEO), longer-form, denser. Reader expects engineering thinking.
- **spacekit.xyz/blog** is team-voice ("SpaceKit Team"), product-focused, marketing-friendly, more accessible. Reader expects what the platform does.

Keep both voices distinct. Cross-link them, but don't merge them.

\newpage

## Task 3 — "Welcome to Web4" light edit on spacekit.xyz

**The problem.** The "Welcome to Web4" post on spacekit.xyz was published February 4, 2026, under the original product framing. The v18 deck now leads with "AI-native Layer 1" — a more specific category claim than Web4. Investors landing on the deck and then on the blog see two framings without an explicit relationship between them. The Web4 framing is good (Web4 is the long-term thesis), but the L1 framing needs to be visible alongside it.

**The fix.** Add one paragraph to the existing "Welcome to Web4" post — near the top, after the opening hook but before the main argument. Do not rewrite the post. The Web4 framing stays. The new paragraph clarifies the L1 as the immediate building block.

### Draft paragraph to insert

Insert this paragraph after the post's existing opening but before the section that develops the Web4 argument:

> *Editor's note (May 2026): When this post was first published in February, the framing emphasized Web4 as the destination — a network where decentralized apps run end-to-end without depending on centralized infrastructure. That destination is still where we're going. The current building block is the **SpaceKit AI-native Layer 1**: a blockchain where smart contracts call AI inference natively, post-quantum cryptography is the default, and the standard library treats DeFi and AI primitives as equal-citizen imports. Web4 is what becomes possible when this L1 is at scale. For the technical depth on how the L1 works, see [Introducing RouteKit](https://swtch.ai/blog/introducing-routekit) and [The SpaceKit Standard Library](https://swtch.ai/blog/spacekit-standard-library) on the Signal Log.*

That paragraph does three things:

1. Acknowledges the original framing without rewriting it
2. Names the L1 explicitly with the "equal-citizen imports" phrase that anchors the v18 deck
3. Links forward to the founder-voice technical posts that develop the L1 thesis

### Bylines and dates

Do not change the original post date (February 4, 2026). The editor's note marker (May 2026) shows the update is recent. Do not change the byline ("SpaceKit Team" stays). The paragraph itself is implicitly editorial commentary, not new authorial content.

### What not to do

Do not write a "Welcome to Web4 v2" post. Do not retire the Web4 post. Do not remove the Web4 category. The existing post is part of the project's content history and removing or replacing it looks like revisionism. The minimal edit above is the right intervention.

\newpage

## Task 4 — Upcoming post calendar (fundraise window)

**The plan.** Three additional posts during the 90-day fundraise window, all on swtch.ai (the founder-voice technical blog). One per ~3 weeks. Each post serves dual purpose: standalone technical content for developer audiences, and citable evidence for investor DD.

The Standard Library post (already shipped) is week 0 of this calendar. The three upcoming posts below are sequenced for maximum DD leverage.

### Post 1 — RouteKit Walkthrough (target: week 3)

**Working title:** *"Reading RouteKit: How One Contract Composes AI, Storage, Messaging, and Payments"*

**Category:** Architecture

**Byline:** Astor Rivera, Founder & CEO

**Length target:** 1500-2000 words

**Structure:**
- Opening: why a reference contract matters for a new platform
- The wire format (opcodes 0x01 through 0x20)
- Walk through each handler — `handle_complete`, `handle_pipeline`, `handle_search_v1`, `handle_converse`, `handle_frontier`, `handle_configure`, `handle_brain_info`, `handle_ping`
- The cost ladder (COST_LOCAL=100, COST_SEARCH=200, COST_PIPE=300, COST_FRONTIER=5000) and why each tier is priced where it is
- The macro imports that make the composition possible
- What developers can build by extending RouteKit
- Close: link to GitHub source, link to deployed testnet contract

**Why this post matters for DD:** RouteKit is cited prominently in v18 slide 6 ("reference contract showing the composition pattern"). Right now an investor can read the contract on GitHub but there's no narrative explanation. This post is the bridge between the deck claim and the open-source artifact.

**Prerequisites for publishing:** RouteKit must be open-sourced on GitHub and deployed to testnet. Both are on your pending action list.

### Post 2 — SKCL Macro System (target: week 6)

**Working title:** *"SKCL: A Macro System for Safe Composition in WASM Smart Contracts"*

**Category:** Architecture or Research

**Byline:** Astor Rivera, Founder & CEO

**Length target:** 1500-2500 words

**Structure:**
- Opening: why we needed a contract language rather than writing raw Rust against the SpaceKit ABI
- The host ABI — what primitives SpaceKit exposes at the runtime layer
- The macro system — how SKCL wraps the ABI in a safer, more composable programming model
- The `spacekit_contract!` macro and what it does
- A worked example: writing a minimal contract from scratch in SKCL
- Comparison: the same contract written without SKCL (longer, more error-prone)
- What developers gain from the macro system (type safety, error handling, lifecycle hooks)
- Close: pointer to SKCL crate, pointer to Standard Library contracts that use SKCL idiomatically

**Why this post matters for DD:** SKCL is mentioned in the deck (slide 4, the L1 quadrant grid) but isn't deeply explained anywhere public. Technical investors will want to see the language details. This post provides the substance and signals that SpaceKit treats developer ergonomics seriously.

**Prerequisites for publishing:** None specific. SKCL repo should be public (github.com/spacekit-xyz/spacekit-contract-lang); confirm before publishing.

### Post 3 — Agentic Workspaces (target: week 9)

**Working title:** *"Agentic Workspaces: TTL Sandboxes, CAS-Backed Repos, and Multi-Model ACID Transactions"*

**Category:** Architecture

**Byline:** Astor Rivera, Founder & CEO

**Length target:** 2000-2500 words

**Structure:**
- Opening: what an agentic workspace is and why it's part of an L1 rather than a separate service
- The five capabilities: sandboxes, CAS-backed repos, PQ envelopes, DID-native ACL, SSE change feeds
- TTL sandboxes — the lifecycle, the dry-run primitive, the crash-safe reconciliation pass
- CAS-backed repos — Git-style workflow on content-addressed storage, the FactPackage commit schema
- Multi-model ACID transactions — Serializable isolation across relational, document, vector, and FTS subsystems
- The PQ envelope flow — Kyber-1024 encryption, entitlement-gated rewrap
- The DID-native ACL pattern across operations
- SSE change feeds for cross-agent automation
- Why these five together — same primitive serves every persona (creators drafting, agents speculating, developers version-controlling, buyers receiving)
- Close: pointer to storage node repo, pointer to the customer-facing capabilities one-pager

**Why this post matters for DD:** Agentic workspaces are referenced in the read-ahead deck appendix but aren't deeply explained anywhere. This is the post that demonstrates SpaceKit's depth on the *collaboration* axis, which most L1 projects don't think about. A technical DD partner reading this comes away with the conclusion: *"This isn't just an L1, it's a development platform."*

**Prerequisites for publishing:** Storage node should be public on GitHub. Confirm before publishing.

\newpage

## Post calendar summary

| Week | Post | Status | Critical path |
|------|------|--------|---------------|
| 0 (now) | The SpaceKit Standard Library | Drafted, ready to publish | Standard Library repo public |
| 3 | RouteKit Walkthrough | Outlined | RouteKit repo public + testnet deployed |
| 6 | SKCL Macro System | Outlined | SKCL repo public |
| 9 | Agentic Workspaces | Outlined | Storage node repo public |

This cadence gives the fundraise window four technical posts on swtch.ai, totaling 7,000-10,000 words of dated, founder-voice technical content. Combined with the existing six posts (six prior to this calendar), the Signal Log enters the fundraise with **ten technical posts across four months** — a strong DD signal for any investor who looks at it.

## What I'd suggest *not* doing during fundraise

A few content moves that look attractive but I'd push back on:

**Don't add a separate "Investor Updates" blog category.** Some companies do quarterly investor letters as public posts. Doing this during a fundraise creates the wrong signal — it makes the company look like it's already raising publicly, which contradicts the private SAFT structure. Investor-focused content lives in the data room, not the blog.

**Don't ship multiple posts in the same week.** The pattern that works for DD signals is *consistent cadence*. One thoughtful post every 2-3 weeks reads as "actively building"; three posts in a single week reads as "trying to look active." The trailing window is what matters, not the burst.

**Don't write thinkpiece-style posts about AI/crypto convergence.** This is what generic crypto media looks like, and investors don't read it. Stick to architecture and research posts where you describe specific technical work. The Signal Log's existing voice (RouteKit, vault charges, Split MNIST, ERC-4337, x402, Tokenization Then Intelligence) is the right voice — specific, technical, dated to real work.

## Cross-references between posts

When the four posts above are all published, link them to each other:

- The Standard Library post (already drafted) mentions RouteKit; should link to the RouteKit walkthrough when it ships
- The RouteKit walkthrough should link to the Standard Library post and the SKCL post (since RouteKit is written in SKCL)
- The SKCL post should link to the Standard Library post and use Standard Library contracts as examples
- The Agentic Workspaces post should link to the storage node capabilities one-pager (already in data room)

Internal linking across posts is what turns a blog into a content corpus. Each post becomes findable from multiple paths, and a reader who lands on one post can follow connections to the rest.

---

*Internal content plan · SpaceKit by swtch labs · For Astor's execution · 2026*