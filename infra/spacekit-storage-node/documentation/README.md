# SpaceKit Storage Node Documentation

Welcome to the SpaceKit Storage Node documentation. This directory contains all public-facing documentation organized by category.

## 📚 Documentation Structure

### Guides
- **[Quick Start](guides/quick-start.md)** - Get started with the storage node
- **[Deployment](guides/deployment.md)** - Deploy the storage node as a standalone service
- **[Build and Deployment](guides/build-and-deployment.md)** - Build binaries for AWS EC2 and Google Cloud
- **[Migrations](guides/migrations.md)** - Database migration guide
- **[NFT Collections](guides/nft-collections.md)** - NFT storage and collection management
- **[PostgreSQL Comparison](guides/postgresql-comparison.md)** - Comparison with PostgreSQL
- **[Simulator Integration](guides/simulator-integration.md)** - Integration with SpaceKit Network Simulator
- **[SpaceKit repository hosting](guides/spacekit-repository-hosting.md)** - CAS blobs, commit facts, refs, and `spacekit repo` CLI
- **[Federation handoff](guides/federation-workspace-handoff.md)** - Workspace export/import between operators
- **[DID-signed migration](guides/did-signed-migration.md)** - SPHINCS+ migration manifests (v2)
- **[Operator discovery](guides/operator-discovery.md)** - `spacekit:operator:v1` manifests

### Competitive Analysis
- **[Competitor Comparison](COMPETITOR_COMPARISON.md)** - Comprehensive comparison matrix with major competitors

### Use Cases
- **[Content Publishing & Subscriptions](CONTENT_PUBLISHING_AND_SUBSCRIPTIONS.md)** - Channel-based content publishing with subscriptions and pay-per-view

### API Documentation
- **[API index](api/README.md)** — canonical route pointer (`src/api/mod.rs`) + focused guides
- **[SQL Query API](api/sql-query-api.md)** — structured JSON queries (`POST /query/*`; not SQL wire protocol)
- **[SQL Support Clarification](SQL_SUPPORT_CLARIFICATION.md)** — ⚠️ Important: what "SQL-like" wording actually guarantees

### Security
- **[Security Architecture](security/security-architecture.md)** - Overall security architecture
- **[Security Quick Reference](security/security-quick-reference.md)** - Quick security reference
- **[Encryption and Security](ENCRYPTION_AND_SECURITY.md)** - Comprehensive encryption and security guide
- **[DDoS Analysis](security/ddos-analysis.md)** - DDoS protection analysis and recommendations

### Whitepaper
- **[Whitepaper](whitepaper/whitepaper.md)** - Main SpaceKit Storage Node whitepaper
- **[Tokenomics](whitepaper/tokenomics.md)** - Storage node earning guide (see also [`spacekit-tokenomics`](../../spacekit-tokenomics/))

## 🚀 Quick Links

- **Main README**: [../README.md](../README.md)
- **Changelog (phases + history)**: [../CHANGELOG.md](../CHANGELOG.md)
- **Getting Started**: [guides/quick-start.md](guides/quick-start.md)
- **Security**: [ENCRYPTION_AND_SECURITY.md](ENCRYPTION_AND_SECURITY.md)
- **API docs**: [api/README.md](api/README.md)

## 📋 Internal Documentation

The following documents are for internal reference:
- [DOCUMENTATION_AUDIT.md](DOCUMENTATION_AUDIT.md) - Documentation audit and cleanup guide
- [PUBLIC_RELEASE_CHECKLIST.md](PUBLIC_RELEASE_CHECKLIST.md) - Pre-release checklist

## 🔍 Finding Documentation

### By Topic

**Getting Started:**
- New to SpaceKit? Start with [Quick Start](guides/quick-start.md)
- Want to deploy? See [Deployment Guide](guides/deployment.md)

**Security:**
- Understanding encryption? See [Encryption and Security](ENCRYPTION_AND_SECURITY.md)
- Security architecture? See [Security Architecture](security/security-architecture.md)
- DDoS protection? See [DDoS Analysis](security/ddos-analysis.md)

**Development:**
- API usage? Start at **[API index](api/README.md)**, see **[SQL Query API](api/sql-query-api.md)**
- **Repository / CAS / facts?** See [SpaceKit repository hosting](guides/spacekit-repository-hosting.md)
- Database migrations? See [Migrations Guide](guides/migrations.md)
- NFT features? See [NFT Collections Guide](guides/nft-collections.md)

**Business:**
- Product overview? See [Whitepaper](whitepaper/whitepaper.md)
- Tokenomics? Canonical: [`spacekit-tokenomics`](../../spacekit-tokenomics/). Storage rates: [Tokenomics](whitepaper/tokenomics.md)

## 📝 Contributing

When adding new documentation:
1. Place user-facing docs in appropriate subdirectories
2. Keep internal/development notes separate
3. Update this README with new documentation
4. Follow the existing documentation structure

