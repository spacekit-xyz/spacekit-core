# Public Release Checklist

## 📚 Documentation Cleanup

### Files to DELETE (Internal/Outdated)
- ⚠️ This repo’s docs have already been reorganized under `documentation/`.
- ❌ Consider moving historical status docs (e.g. `*_COMPLETE.md`, `ALL_PHASES_COMPLETE.md`) into an internal-only folder or removing them from the public release.

### Files to KEEP (Public Documentation)
- ✅ `documentation/README.md` - Documentation index
- ✅ `documentation/guides/quick-start.md` - User guide
- ✅ `documentation/guides/deployment.md` - Deployment guide
- ✅ `documentation/guides/build-and-deployment.md` - Build + release artifacts
- ✅ `documentation/api/sql-query-api.md` - API documentation
- ✅ `documentation/guides/nft-collections.md` - Feature docs
- ✅ `documentation/guides/postgresql-comparison.md` - Competitive positioning
- ✅ `documentation/guides/migrations.md` - Migration guide
- ✅ `documentation/guides/simulator-integration.md` - Integration docs
- ✅ `documentation/security/security-architecture.md` - Security docs
- ✅ `documentation/security/security-quick-reference.md` - Security reference
- ✅ `documentation/security/ddos-analysis.md` - DDoS analysis + mitigations
- ✅ `documentation/whitepaper/whitepaper.md` - Whitepaper
- ✅ `documentation/whitepaper/tokenomics.md` - Tokenomics

### Files to REVIEW & CONSOLIDATE
- ⚠️ Encryption-related docs (merge into `documentation/security/security-architecture.md` and `documentation/ENCRYPTION_AND_SECURITY.md`)
- ⚠️ `ENTERPRISE_GRADE_ROADMAP.md` - Keep private or create public version

**See `DOCUMENTATION_AUDIT.md` for detailed analysis.**

---

## 🔒 Security & DDoS Protection Status

### Current Protection: ⚠️ **~70% - nearing production-ready**

#### ✅ What's Working
- IP-based rate limiting applied broadly across HTTP API routes
- Request body size limits (JSON + uploads)
- Per-instance concurrency limits on file operations (API semaphore)
- DID-based authentication on protected endpoints
- DID-bound SPHINCS+ signature verification for fact packages (global DID registry)
- Debug/sensitive endpoints gated behind DID auth and an explicit env flag
- Row-level access checks exist in multiple handlers (still needs systematic review)

#### ❌ Critical Gaps
1. **No shared/global connection limits** - use a reverse proxy/WAF for fleet-wide controls
2. **Request timeouts are partial** - file upload/content have timeouts, but proxy-level timeouts still recommended
3. **In-memory rate limiter** - lost on restart; enable Redis-backed rate limiting for multi-node

### Required Fixes Before Production

1. **Deploy behind a reverse proxy/WAF** (Cloudflare/AWS WAF + nginx/ALB) with connection limiting
2. **Add request timeouts** (or enforce via proxy) for upload-heavy routes
3. **Persist/distribute rate limiting** (Redis or other shared store) for multi-node deployments
4. **Implement DID resolution** and bind signatures to DID-owned keys

**See `documentation/security/ddos-analysis.md` for detailed recommendations.**

### Recommended Production Setup
- Use **Cloudflare** or **AWS WAF** for DDoS protection
- Use **nginx** reverse proxy with rate limiting
- Use **Redis** for distributed rate limiting
- Monitor and alert on suspicious patterns

---

## 🚀 Build Scripts for Cloud Deployment

### AWS EC2 Build
**Script**: `build-docker-aws.sh`
- ✅ Fixed macOS/Linux compatibility
- ✅ Added production features flag
- ✅ Output: `dist/spacekit-storage-node`

**Usage**:
```bash
./build-docker-aws.sh
```

### Google Cloud Platform Build
**Script**: `build-docker-gcp.sh`
- ✅ Created new script for GCP
- ✅ Optimized for GCP deployment
- ✅ Output: `dist-gcp/spacekit-storage-node`

**Usage**:
```bash
./build-docker-gcp.sh
```

**See `BUILD_AND_DEPLOYMENT.md` for deployment instructions.**

---

## ✅ Action Items Before Public Release

### Immediate (Critical)
- [x] Implement IP-based rate limiting
- [x] Add rate limiting broadly across endpoints
- [x] Add request size limits
- [ ] Add request timeout enforcement (or document proxy requirement)
- [ ] Ensure any internal-only docs are removed or clearly marked internal
- [ ] Test AWS and GCP builds

### High Priority
- [ ] Set up Cloudflare/AWS WAF
- [ ] Create production deployment guide
- [ ] Add monitoring and alerting
- [ ] Consolidate encryption documentation

### Medium Priority
- [ ] Create public roadmap (sanitized version)
- [ ] Set up CI/CD for automated builds
- [ ] Create Docker images for containerized deployment

---

## 🧪 CI/Release Checklist (Minimum)

Run these commands in CI and before a release:

```bash
cargo test
cargo clippy --all-targets --all-features
cargo audit
```

---

## 📊 Summary

| Category | Status | Action Required |
|----------|--------|-----------------|
| Documentation | ⚠️ Needs cleanup | Mark/remove internal-only docs |
| Security | 🟠 Needs finishing | Add DID resolution + timeouts/proxy requirements |
| DDoS Protection | 🟠 Needs finishing | Add connection limits + timeouts/proxy guidance |
| Build Scripts | ✅ Complete | AWS & GCP builds ready |
| Deployment | ⚠️ Needs testing | Test on actual cloud instances |

**Recommendation**: Address security gaps before public release. The storage node is functional but needs hardening for production use.

