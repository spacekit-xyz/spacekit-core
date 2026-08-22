//! `spacekit repo` — CAS blobs + fact commits + document refs (`/blobs`, `/facts`, `/api/documents/...`).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use colored::Colorize;
use serde::{Deserialize, Serialize};
use spacekit_did::sphincs::SphincsPlus;
use spacekit_primitives::v1::crypto::quantum::SPHINCSSignature;
use spacekit_primitives::v1::fact::FactPackage;
use spacekit_repo::{
    build_commit_fact_package, detect_exact_renames, diff_trees, merge_blobs, merge_trees,
    parse_commit_from_fact_package, recompute_commit_fact_id, unified_diff, CommitContent,
    MergeConflict, RepoRefJson, TreeChange, TreeSnapshot, DEFAULT_FILE_MODE, EXEC_FILE_MODE,
};

use percent_encoding::{utf8_percent_encode, AsciiSet, CONTROLS};

use super::{
    identity_cmd, load_cli_config, resolve_remote_storage_base_url, CLIConfig, CliContext,
    RepoCommands,
};

/// SPHINCS+ parameter set used for commit signatures (matches `did_wallet.json`).
const COMMIT_SIG_ALGORITHM: &str = "SPHINCS+-SHAKE-256-128s-simple";

/// Encode `/` in document collection/id so warp's two-segment route matches.
const DOCUMENT_PATH_ENCODE_SET: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'<')
    .add(b'>')
    .add(b'`')
    .add(b'#')
    .add(b'?')
    .add(b'/');

fn encode_document_path_segment(segment: &str) -> String {
    utf8_percent_encode(segment, DOCUMENT_PATH_ENCODE_SET).to_string()
}

fn document_api_url(base: &str, collection: &str, doc_id: &str) -> String {
    format!(
        "{}/api/documents/{}/{}",
        base.trim_end_matches('/'),
        encode_document_path_segment(collection),
        encode_document_path_segment(doc_id),
    )
}

async fn repo_effective_did() -> Result<String, Box<dyn std::error::Error>> {
    Ok(CliContext::load_sync()?.did)
}

const REPO_DIR: &str = ".spacekit/repo";
const SKIP_DIR_NAMES: &[&str] = &[".spacekit", ".git", "target", "node_modules"];

fn repo_root() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn repo_base() -> PathBuf {
    repo_root().join(REPO_DIR)
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, Box<dyn std::error::Error>> {
    let s = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&s)?)
}

fn write_json<T: Serialize>(path: &Path, v: &T) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(v)?)?;
    Ok(())
}

fn read_head_branch() -> Result<String, Box<dyn std::error::Error>> {
    let p = repo_base().join("HEAD");
    let s = std::fs::read_to_string(&p)?;
    Ok(s.trim().to_string())
}

fn branch_to_ref_file(branch: &str) -> PathBuf {
    let name = branch.strip_prefix("refs/heads/").unwrap_or(branch);
    repo_base().join("refs/heads").join(name)
}

fn validate_branch_short(name: &str) -> Result<(), Box<dyn std::error::Error>> {
    if name.is_empty() {
        return Err("branch name is empty".into());
    }
    if name.contains('/') || name.contains('\\') {
        return Err("branch name must be a single path segment (no `/` or `\\`)".into());
    }
    if name.contains("..") || name == "." {
        return Err("invalid branch name".into());
    }
    Ok(())
}

fn refs_heads_symref(short: &str) -> Result<String, Box<dyn std::error::Error>> {
    validate_branch_short(short)?;
    Ok(format!("refs/heads/{}", short))
}

async fn materialize_commit_tree(
    root: &Path,
    commit: &CommitContent,
    client: &reqwest::Client,
    base: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    for (rel, h) in &commit.tree {
        let abs = root.join(rel);
        let mode = commit.mode_for(rel);
        if abs.is_file() && blake3_file(&abs)? == *h {
            apply_file_mode(&abs, mode);
            continue;
        }
        let bytes = fetch_blob_cached(client, base, h).await?;
        if let Some(par) = abs.parent() {
            std::fs::create_dir_all(par)?;
        }
        std::fs::write(&abs, &bytes)?;
        apply_file_mode(&abs, mode);
    }
    Ok(())
}

fn read_tip_hex() -> Result<Option<String>, Box<dyn std::error::Error>> {
    let branch = read_head_branch()?;
    read_tip_for_symref(&branch)
}

fn read_tip_for_symref(branch_symref: &str) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let rf = branch_to_ref_file(branch_symref);
    if !rf.exists() {
        return Ok(None);
    }
    let s = std::fs::read_to_string(&rf)?;
    let t = s.trim();
    if t.is_empty() {
        Ok(None)
    } else {
        Ok(Some(t.to_string()))
    }
}

fn write_tip_hex(tip: &str) -> Result<(), Box<dyn std::error::Error>> {
    let branch = read_head_branch()?;
    write_tip_for_symref(&branch, tip)
}

fn write_tip_for_symref(branch_symref: &str, tip: &str) -> Result<(), Box<dyn std::error::Error>> {
    let rf = branch_to_ref_file(branch_symref);
    std::fs::create_dir_all(rf.parent().unwrap())?;
    std::fs::write(&rf, tip.trim())?;
    Ok(())
}

fn resolve_repo_branch_symref(
    branch_arg: Option<&str>,
) -> Result<String, Box<dyn std::error::Error>> {
    match branch_arg {
        Some(short) => refs_heads_symref(short),
        None => read_head_branch(),
    }
}

fn index_path() -> PathBuf {
    repo_base().join("index.json")
}

fn load_index() -> Result<BTreeMap<String, String>, Box<dyn std::error::Error>> {
    let p = index_path();
    if !p.exists() {
        return Ok(BTreeMap::new());
    }
    read_json(&p)
}

fn save_index(m: &BTreeMap<String, String>) -> Result<(), Box<dyn std::error::Error>> {
    write_json(&index_path(), m)
}

fn commit_object_path(hex: &str) -> PathBuf {
    let pre = &hex[..2.min(hex.len())];
    repo_base()
        .join("objects/commits")
        .join(pre)
        .join(format!("{}.json", hex))
}

fn save_commit_local(pkg: &FactPackage) -> Result<(), Box<dyn std::error::Error>> {
    let id = hex::encode(pkg.fact_id);
    let path = commit_object_path(&id);
    std::fs::create_dir_all(path.parent().unwrap())?;
    write_json(&path, pkg)?;
    Ok(())
}

fn load_commit_local(hex: &str) -> Result<FactPackage, Box<dyn std::error::Error>> {
    let path = commit_object_path(hex);
    read_json(&path)
}

fn should_skip_dir(name: &str) -> bool {
    SKIP_DIR_NAMES.iter().any(|s| *s == name)
}

// ===========================================================================
// Local object store (blobs) + integrity verification
// ===========================================================================

fn blob_object_path(hex: &str) -> PathBuf {
    let pre = &hex[..2.min(hex.len())];
    repo_base().join("objects/blobs").join(pre).join(hex)
}

fn have_blob_local(hex: &str) -> bool {
    blob_object_path(hex).is_file()
}

/// Store bytes in the local blob store, returning the BLAKE3 hex key.
fn store_blob_local_bytes(bytes: &[u8]) -> Result<String, Box<dyn std::error::Error>> {
    let hex = hex::encode(blake3::hash(bytes).as_bytes());
    let p = blob_object_path(&hex);
    if !p.exists() {
        if let Some(par) = p.parent() {
            std::fs::create_dir_all(par)?;
        }
        std::fs::write(&p, bytes)?;
    }
    Ok(hex)
}

/// Store network-fetched bytes under an *expected* hash, verifying integrity.
fn store_blob_local_verified(hex: &str, bytes: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    let actual = hex::encode(blake3::hash(bytes).as_bytes());
    if actual != hex {
        return Err(format!(
            "blob integrity error: content hashes to {actual} but was requested as {hex}"
        )
        .into());
    }
    let p = blob_object_path(hex);
    if !p.exists() {
        if let Some(par) = p.parent() {
            std::fs::create_dir_all(par)?;
        }
        std::fs::write(&p, bytes)?;
    }
    Ok(())
}

fn load_blob_local(hex: &str) -> Option<Vec<u8>> {
    std::fs::read(blob_object_path(hex)).ok()
}

/// Fetch a blob by hash, preferring the local store and verifying remote bytes.
async fn fetch_blob_cached(
    client: &reqwest::Client,
    base: &str,
    hex: &str,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    if let Some(b) = load_blob_local(hex) {
        return Ok(b);
    }
    let url = format!("{}/blobs/{}", base.trim_end_matches('/'), hex);
    let bytes = client
        .get(&url)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    store_blob_local_verified(hex, &bytes)?;
    Ok(bytes.to_vec())
}

/// Resolve the bytes for a content hash from any local source (blob store,
/// then a path hint, then the working tree). Used when pushing.
fn local_bytes_for_hash(root: &Path, hex: &str, hint: Option<&str>) -> Option<Vec<u8>> {
    if let Some(b) = load_blob_local(hex) {
        return Some(b);
    }
    if let Some(rel) = hint {
        if let Ok(b) = std::fs::read(root.join(rel)) {
            if hex::encode(blake3::hash(&b).as_bytes()) == hex {
                return Some(b);
            }
        }
    }
    None
}

// ===========================================================================
// File modes (executable bit)
// ===========================================================================

#[cfg(unix)]
fn path_mode_on_disk(abs: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    match std::fs::metadata(abs) {
        Ok(md) => {
            if md.permissions().mode() & 0o111 != 0 {
                EXEC_FILE_MODE
            } else {
                DEFAULT_FILE_MODE
            }
        }
        Err(_) => DEFAULT_FILE_MODE,
    }
}

#[cfg(not(unix))]
fn path_mode_on_disk(_abs: &Path) -> u32 {
    DEFAULT_FILE_MODE
}

#[cfg(unix)]
fn apply_file_mode(abs: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let want = if mode == EXEC_FILE_MODE { 0o755 } else { 0o644 };
    if let Ok(md) = std::fs::metadata(abs) {
        let mut perms = md.permissions();
        if perms.mode() & 0o7777 != want {
            perms.set_mode(want);
            let _ = std::fs::set_permissions(abs, perms);
        }
    }
}

#[cfg(not(unix))]
fn apply_file_mode(_abs: &Path, _mode: u32) {}

/// Compute the modes map (only entries differing from the default) for a tree.
fn modes_for_tree(root: &Path, tree: &BTreeMap<String, String>) -> BTreeMap<String, u32> {
    let mut m = BTreeMap::new();
    for rel in tree.keys() {
        let mode = path_mode_on_disk(&root.join(rel));
        if mode != DEFAULT_FILE_MODE {
            m.insert(rel.clone(), mode);
        }
    }
    m
}

// ===========================================================================
// .gitignore / .spacekitignore matching
// ===========================================================================

/// A compiled ignore pattern (a small subset of gitignore semantics:
/// blank/`#` lines, `!` negation, leading `/` anchoring, trailing `/`
/// dir-only, and `*` / `?` / `**` globs).
#[derive(Clone)]
struct IgnorePattern {
    glob: String,
    negated: bool,
    dir_only: bool,
    anchored: bool,
}

#[derive(Clone, Default)]
struct IgnoreMatcher {
    patterns: Vec<IgnorePattern>,
}

impl IgnoreMatcher {
    fn load(root: &Path) -> Self {
        let mut patterns = Vec::new();
        for name in [".spacekitignore", ".gitignore"] {
            let p = root.join(name);
            if let Ok(s) = std::fs::read_to_string(&p) {
                for raw in s.lines() {
                    let line = raw.trim_end();
                    if line.is_empty() || line.starts_with('#') {
                        continue;
                    }
                    let mut pat = line.to_string();
                    let negated = pat.starts_with('!');
                    if negated {
                        pat = pat[1..].to_string();
                    }
                    let dir_only = pat.ends_with('/');
                    if dir_only {
                        pat.pop();
                    }
                    let anchored = pat.starts_with('/');
                    if anchored {
                        pat = pat[1..].to_string();
                    }
                    if pat.is_empty() {
                        continue;
                    }
                    patterns.push(IgnorePattern {
                        glob: pat,
                        negated,
                        dir_only,
                        anchored,
                    });
                }
            }
        }
        Self { patterns }
    }

    /// True if `rel` (POSIX, relative to repo root) is ignored. `is_dir`
    /// indicates whether the path is a directory.
    fn is_ignored(&self, rel: &str, is_dir: bool) -> bool {
        let mut ignored = false;
        for p in &self.patterns {
            if p.dir_only && !is_dir {
                continue;
            }
            let hit = if p.anchored || p.glob.contains('/') {
                glob_match(&p.glob, rel)
            } else {
                // Unanchored: match against the basename or any path suffix.
                rel.split('/').any(|seg| glob_match(&p.glob, seg)) || glob_match(&p.glob, rel)
            };
            if hit {
                ignored = !p.negated;
            }
        }
        ignored
    }
}

/// Minimal glob matcher supporting `*` (not `/`), `**` (any, incl. `/`), and `?`.
fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    glob_rec(&p, 0, &t, 0)
}

fn glob_rec(p: &[char], mut pi: usize, t: &[char], mut ti: usize) -> bool {
    while pi < p.len() {
        match p[pi] {
            '*' => {
                let double = pi + 1 < p.len() && p[pi + 1] == '*';
                if double {
                    // `**` matches across `/`.
                    let next = pi + 2;
                    if next >= p.len() {
                        return true;
                    }
                    for k in ti..=t.len() {
                        if glob_rec(p, next, t, k) {
                            return true;
                        }
                    }
                    return false;
                } else {
                    // `*` matches any run that does not include `/`.
                    let next = pi + 1;
                    for k in ti..=t.len() {
                        if glob_rec(p, next, t, k) {
                            return true;
                        }
                        if k < t.len() && t[k] == '/' {
                            break;
                        }
                    }
                    return false;
                }
            }
            '?' => {
                if ti >= t.len() || t[ti] == '/' {
                    return false;
                }
                pi += 1;
                ti += 1;
            }
            c => {
                if ti >= t.len() || t[ti] != c {
                    return false;
                }
                pi += 1;
                ti += 1;
            }
        }
    }
    ti == t.len()
}

// ===========================================================================
// Commit signing + verification (SPHINCS+, keyed by `~/.spacekit/did_wallet.json`)
// ===========================================================================

/// Load the SPHINCS+ `(public_key, secret_key)` from the DID wallet, if present.
fn load_repo_signing_key() -> Option<(Vec<u8>, Vec<u8>)> {
    let wallet = dirs::home_dir()?.join(".spacekit").join("did_wallet.json");
    let raw = std::fs::read_to_string(wallet).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let pk = hex::decode(v.get("sphincs_pk_hex")?.as_str()?).ok()?;
    let sk = hex::decode(v.get("sphincs_sk_hex")?.as_str()?).ok()?;
    Some((pk, sk))
}

/// Sign a freshly built commit package in place (fills `signature`). Returns
/// `true` if a signature was attached, `false` if no signing key was available.
fn sign_commit_pkg(pkg: &mut FactPackage) -> bool {
    let Some((pk, sk)) = load_repo_signing_key() else {
        return false;
    };
    match SphincsPlus::sign(&sk, &pkg.fact_id) {
        Ok(sig_bytes) => {
            pkg.signature = SPHINCSSignature::new(sig_bytes, COMMIT_SIG_ALGORITHM.to_string(), pk);
            true
        }
        Err(_) => false,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VerifyStatus {
    /// Signature present, valid, and the signing key matches the author DID.
    GoodTrusted,
    /// Signature valid but the signing key doesn't match the author DID address.
    GoodUntrusted,
    /// No signature attached.
    Unsigned,
    /// Signature present but invalid, or fact-id integrity failed.
    Bad,
}

/// Derive the testnet/mainnet DID address from a SPHINCS+ public key:
/// `hex(SHA-256(pk)[0..20])`.
fn did_address_from_pk(pk: &[u8]) -> String {
    use sha2::Digest;
    let hash = sha2::Sha256::digest(pk);
    hex::encode(&hash[..20])
}

/// Verify a commit package: fact-id integrity + signature + author binding.
fn verify_commit_pkg(pkg: &FactPackage) -> VerifyStatus {
    // 1. Integrity: the stored fact_id must match the recomputed one.
    match recompute_commit_fact_id(pkg) {
        Ok(id) if id == pkg.fact_id => {}
        _ => return VerifyStatus::Bad,
    }
    let sig = &pkg.signature;
    if sig.signature_bytes.is_empty() || sig.public_key.is_empty() {
        return VerifyStatus::Unsigned;
    }
    // 2. Signature over the fact_id.
    if !SphincsPlus::verify(&sig.public_key, &pkg.fact_id, &sig.signature_bytes) {
        return VerifyStatus::Bad;
    }
    // 3. Author binding: the signing key's DID address must appear in the
    //    author DID string (which embeds `...:address`).
    let addr = did_address_from_pk(&sig.public_key);
    if let Ok(content) = parse_commit_from_fact_package(pkg) {
        if content.author_name.contains(&addr) {
            return VerifyStatus::GoodTrusted;
        }
    }
    VerifyStatus::GoodUntrusted
}

// ===========================================================================
// Reflog
// ===========================================================================

fn reflog_path() -> PathBuf {
    repo_base().join("logs/HEAD")
}

/// Append a reflog entry: `<old> <new> <iso8601> <message>`.
fn append_reflog(old: &str, new: &str, message: &str) {
    let line = format!(
        "{} {} {} {}\n",
        if old.is_empty() {
            "0".repeat(64)
        } else {
            old.to_string()
        },
        if new.is_empty() {
            "0".repeat(64)
        } else {
            new.to_string()
        },
        chrono::Utc::now().to_rfc3339(),
        message,
    );
    let p = reflog_path();
    if let Some(par) = p.parent() {
        let _ = std::fs::create_dir_all(par);
    }
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&p)
    {
        let _ = f.write_all(line.as_bytes());
    }
}

// ===========================================================================
// Ancestry / merge-base over the local commit graph
// ===========================================================================

fn commit_parents_local(id: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let pkg = load_commit_local(id)?;
    Ok(pkg.dependencies.iter().map(hex::encode).collect())
}

/// All ancestors of `tip` (inclusive) that exist in the local object store.
fn ancestors_local(tip: &str) -> BTreeSet<String> {
    let mut seen = BTreeSet::new();
    let mut stack = vec![tip.to_string()];
    while let Some(id) = stack.pop() {
        if id.is_empty() || !seen.insert(id.clone()) {
            continue;
        }
        if let Ok(parents) = commit_parents_local(&id) {
            for p in parents {
                if !seen.contains(&p) {
                    stack.push(p);
                }
            }
        }
    }
    seen
}

fn is_ancestor_local(maybe_ancestor: &str, tip: &str) -> bool {
    if maybe_ancestor.is_empty() {
        return true;
    }
    ancestors_local(tip).contains(maybe_ancestor)
}

/// Best common ancestor of two commits (first ancestor of `a` also reachable
/// from `b`, walking `a` breadth-first). Returns `None` if histories disjoint.
fn merge_base_local(a: &str, b: &str) -> Option<String> {
    let b_anc = ancestors_local(b);
    let mut seen = BTreeSet::new();
    let mut queue = std::collections::VecDeque::new();
    queue.push_back(a.to_string());
    while let Some(id) = queue.pop_front() {
        if id.is_empty() || !seen.insert(id.clone()) {
            continue;
        }
        if b_anc.contains(&id) {
            return Some(id);
        }
        if let Ok(parents) = commit_parents_local(&id) {
            for p in parents {
                queue.push_back(p);
            }
        }
    }
    None
}

fn list_tracked_files(root: &Path) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    let ignore = IgnoreMatcher::load(root);
    let mut out = Vec::new();
    walk_files(root, root, &ignore, &mut out)?;
    out.sort();
    Ok(out)
}

fn walk_files(
    root: &Path,
    cur: &Path,
    ignore: &IgnoreMatcher,
    acc: &mut Vec<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    for e in std::fs::read_dir(cur)? {
        let e = e?;
        let p = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        let rel = p
            .strip_prefix(root)
            .unwrap_or(&p)
            .to_string_lossy()
            .replace('\\', "/");
        if p.is_dir() {
            if should_skip_dir(&name) || ignore.is_ignored(&rel, true) {
                continue;
            }
            walk_files(root, &p, ignore, acc)?;
        } else if p.is_file() {
            if ignore.is_ignored(&rel, false) {
                continue;
            }
            acc.push(PathBuf::from(rel));
        }
    }
    Ok(())
}

fn blake3_file(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let data = std::fs::read(path)?;
    Ok(hex::encode(blake3::hash(&data).as_bytes()))
}

fn working_tree_hashes(
    root: &Path,
) -> Result<BTreeMap<String, String>, Box<dyn std::error::Error>> {
    let paths = list_tracked_files(root)?;
    let mut m = BTreeMap::new();
    for rel in paths {
        let abs = root.join(&rel);
        let h = blake3_file(&abs)?;
        m.insert(rel.to_string_lossy().to_string(), h);
    }
    Ok(m)
}

fn snapshot_from_hex_tree(
    m: &BTreeMap<String, String>,
) -> Result<TreeSnapshot, Box<dyn std::error::Error>> {
    let mut s = TreeSnapshot::new();
    for (p, h) in m {
        let b = hex::decode(h)?;
        if b.len() != 32 {
            return Err(format!("bad hash for {}", p).into());
        }
        let mut a = [0u8; 32];
        a.copy_from_slice(&b);
        s.insert(p.clone(), a);
    }
    Ok(s)
}

fn spacekit_username_from_did(did: &str) -> Option<String> {
    let name = did.strip_prefix("did:spacekit:user:")?;
    if name.len() < 3 {
        return None;
    }
    Some(name.to_lowercase())
}

fn resolve_repo_storage_name(raw: &str, did: &str) -> String {
    if raw.contains('/') {
        return raw.to_string();
    }
    if let Some(user) = spacekit_username_from_did(did) {
        return format!("{}/{}", user, raw);
    }
    raw.to_string()
}

fn parse_repo_owner_type(storage_name: &str, did: &str) -> (String, String, String) {
    if let Some((owner, repo)) = storage_name.rsplit_once('/') {
        return (owner.to_string(), repo.to_string(), "user".to_string());
    }
    let owner = spacekit_username_from_did(did).unwrap_or_else(|| "unknown".to_string());
    (owner.clone(), storage_name.to_string(), "user".to_string())
}

fn resolve_website_api_url() -> String {
    std::env::var("SPACEKIT_WEBSITE_API_URL")
        .or_else(|_| std::env::var("VITE_API_URL"))
        .unwrap_or_else(|_| "http://127.0.0.1:3001".to_string())
        .trim_end_matches('/')
        .to_string()
}

async fn authorize_repo_push_via_website_api(
    client: &reqwest::Client,
    did: &str,
    storage_name: &str,
    session_token: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let api = resolve_website_api_url();
    let url = format!("{}/api/repos/authorize-push", api);
    let body = serde_json::json!({
        "storage_name": storage_name,
        "owner_did": did,
    });
    let mut req = client
        .post(&url)
        .header("owner-did", did)
        .header("content-type", "application/json")
        .json(&body);
    if let Some(tok) = session_token {
        req = req.header("Authorization", format!("Bearer {tok}"));
    }
    let resp = req.send().await?;
    if resp.status().is_success() {
        return Ok(());
    }
    let status = resp.status();
    let detail = resp.text().await.unwrap_or_default();
    if status == reqwest::StatusCode::NOT_FOUND || status == reqwest::StatusCode::BAD_GATEWAY {
        eprintln!(
            "{}",
            "⚠️  website-api authorize-push unavailable; proceeding without ACL check".yellow()
        );
        return Ok(());
    }
    if status == reqwest::StatusCode::UNAUTHORIZED && session_token.is_none() {
        return Err(
            "push requires website sign-in — run `spacekit login` then `spacekit identity link`"
                .into(),
        );
    }
    Err(format!(
        "push not authorized ({}): {}",
        status,
        detail.chars().take(200).collect::<String>()
    )
    .into())
}

fn doc_collection(repo_name: &str) -> String {
    format!("repos/{}/refs", repo_name)
}

fn ref_doc_id(branch: &str) -> String {
    let name = branch.strip_prefix("refs/heads/").unwrap_or(branch);
    format!("heads/{}", name)
}

async fn put_blob(
    client: &reqwest::Client,
    base: &str,
    hash_hex: &str,
    bytes: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    let url = format!("{}/blobs/{}", base.trim_end_matches('/'), hash_hex);
    let r = client.put(&url).body(bytes.to_vec()).send().await?;
    let status = r.status();
    if !status.is_success() && status != reqwest::StatusCode::CREATED {
        let t = r.text().await.unwrap_or_default();
        return Err(format!(
            "PUT {} -> {} {}",
            url,
            status,
            t.chars().take(200).collect::<String>()
        )
        .into());
    }
    Ok(())
}

async fn post_fact(
    client: &reqwest::Client,
    base: &str,
    pkg: &FactPackage,
) -> Result<(), Box<dyn std::error::Error>> {
    let url = format!("{}/facts", base.trim_end_matches('/'));
    let r = client.post(&url).json(pkg).send().await?;
    let status = r.status();
    if !status.is_success() && status != reqwest::StatusCode::CREATED {
        let t = r.text().await.unwrap_or_default();
        return Err(format!(
            "POST facts -> {} {}",
            status,
            t.chars().take(400).collect::<String>()
        )
        .into());
    }
    Ok(())
}

async fn fetch_fact_remote(
    client: &reqwest::Client,
    base: &str,
    id_hex: &str,
) -> Result<FactPackage, Box<dyn std::error::Error>> {
    let url = format!("{}/facts/{}", base.trim_end_matches('/'), id_hex);
    let r = client.get(&url).send().await?;
    if !r.status().is_success() {
        return Err(format!("GET {} -> {}", url, r.status()).into());
    }
    Ok(r.json().await?)
}

fn parent_chain_local(tip: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let mut chain = Vec::new();
    let mut cur = tip.to_string();
    let mut guard = 0u32;
    while !cur.is_empty() && guard < 10_000 {
        guard += 1;
        chain.push(cur.clone());
        let pkg: FactPackage = load_commit_local(&cur)?;
        if pkg.dependencies.is_empty() {
            break;
        }
        if pkg.dependencies.len() != 1 {
            break;
        }
        cur = hex::encode(pkg.dependencies[0]);
    }
    Ok(chain)
}

fn hash_to_path_from_chain(
    chain: &[String],
) -> Result<HashMap<String, String>, Box<dyn std::error::Error>> {
    let mut m: HashMap<String, String> = HashMap::new();
    for id in chain.iter().rev() {
        let pkg = load_commit_local(id)?;
        let c = parse_commit_from_fact_package(&pkg)?;
        for (p, h) in c.tree {
            m.insert(h, p);
        }
    }
    Ok(m)
}

fn format_change(c: &TreeChange) -> String {
    match c {
        TreeChange::Added { path, .. } => format!("added:   {}", path),
        TreeChange::Removed { path, .. } => format!("removed: {}", path),
        TreeChange::Modified { path, .. } => format!("changed: {}", path),
    }
}

/// Stage working-tree changes into the index before commit (paths added/modified/removed vs index).
fn auto_stage_working_tree(
    index: &mut BTreeMap<String, String>,
    root: &Path,
) -> Result<usize, Box<dyn std::error::Error>> {
    let work = working_tree_hashes(root)?;
    let idx_snap = snapshot_from_hex_tree(index)?;
    let work_snap = snapshot_from_hex_tree(&work)?;
    let changes = diff_trees(&idx_snap, &work_snap);
    if changes.is_empty() {
        return Ok(0);
    }
    for ch in &changes {
        match ch {
            TreeChange::Added { path, hash }
            | TreeChange::Modified {
                path,
                new_hash: hash,
                ..
            } => {
                index.insert(path.clone(), hex::encode(hash));
            }
            TreeChange::Removed { path, .. } => {
                index.remove(path);
            }
        }
    }
    Ok(changes.len())
}

async fn clone_remote(
    remote: String,
    repo_name: String,
    dir: Option<PathBuf>,
    depth: Option<usize>,
) -> Result<(), Box<dyn std::error::Error>> {
    let dest = dir.unwrap_or_else(|| PathBuf::from(&repo_name));
    std::fs::create_dir_all(&dest)?;
    std::env::set_current_dir(&dest)?;
    run_init(Some(repo_name), Some(remote.clone())).await?;
    run_pull(Some(remote), None, depth).await?;
    Ok(())
}

async fn run_init(
    name: Option<String>,
    remote: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let base = repo_base();
    std::fs::create_dir_all(base.join("refs/heads"))?;
    std::fs::create_dir_all(base.join("objects/commits"))?;

    let cfg = serde_json::json!({
        "name": name.clone().unwrap_or_else(|| {
            repo_root()
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("repo")
                .to_string()
        }),
        "description": "",
        "default_branch": "main",
        "remote_url": remote.clone().unwrap_or_default(),
    });
    write_json(&base.join("config.json"), &cfg)?;

    std::fs::write(base.join("HEAD"), "refs/heads/main")?;
    let rf = branch_to_ref_file("main");
    std::fs::create_dir_all(rf.parent().unwrap())?;
    std::fs::write(&rf, "")?;
    save_index(&BTreeMap::new())?;

    println!(
        "{}",
        format!("✅ Repo initialized under {}", base.display()).green()
    );
    Ok(())
}

/// Read the remote tip for a branch ref document.
async fn remote_tip(
    client: &reqwest::Client,
    base: &str,
    repo_name: &str,
    branch_sym: &str,
    did: &str,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let collection = doc_collection(repo_name);
    let doc_id = ref_doc_id(branch_sym);
    let url = document_api_url(base, &collection, &doc_id);
    let r = client
        .get(&url)
        .header("Authorization", format!("DID {}", did))
        .send()
        .await?;
    if r.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !r.status().is_success() {
        return Err(format!("GET ref {} -> {}", url, r.status()).into());
    }
    let v: serde_json::Value = r.json().await?;
    Ok(v["document"]["data"]["tip"].as_str().map(|s| s.to_string()))
}

/// Download the commit chain for `tip` into the local object store (commits +
/// blobs), stopping at `local_tip`, an already-present commit, or after `depth`
/// commits. Verifies fact-id integrity of every fetched commit.
async fn fetch_chain_into_store(
    client: &reqwest::Client,
    base: &str,
    tip: &str,
    local_tip: &str,
    depth: Option<usize>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut cur = tip.to_string();
    let mut to_fetch: Vec<String> = Vec::new();
    let limit = depth.unwrap_or(usize::MAX);
    loop {
        if to_fetch.len() >= limit {
            break;
        }
        if cur.is_empty() || cur == local_tip || load_commit_local(&cur).is_ok() {
            break;
        }
        let pkg = fetch_fact_remote(client, base, &cur).await?;
        if recompute_commit_fact_id(&pkg)? != pkg.fact_id {
            return Err(format!("commit {cur} failed integrity check (fact-id mismatch)").into());
        }
        to_fetch.push(cur.clone());
        match pkg.dependencies.len() {
            0 => break,
            1 => cur = hex::encode(pkg.dependencies[0]),
            _ => {
                // Merge commit: fetch the first-parent line here; other parents
                // are fetched on demand by the merge/log walkers.
                cur = hex::encode(pkg.dependencies[0]);
            }
        }
    }

    // Persist oldest-first so parents land before children.
    for id in to_fetch.iter().rev() {
        let pkg = fetch_fact_remote(client, base, id).await?;
        save_commit_local(&pkg)?;
        let commit = parse_commit_from_fact_package(&pkg)?;
        for h in commit.tree.values() {
            if !have_blob_local(h) {
                let _ = fetch_blob_cached(client, base, h).await?;
            }
        }
    }
    Ok(())
}

async fn run_pull(
    storage_url: Option<String>,
    branch_override: Option<String>,
    depth: Option<usize>,
) -> Result<(), Box<dyn std::error::Error>> {
    let _cfg: CLIConfig = load_cli_config().await?;
    let (base, _) = resolve_remote_storage_base_url(storage_url.as_deref())?;
    let repo_cfg: serde_json::Value = read_json(&repo_base().join("config.json"))?;
    let repo_name = repo_cfg["name"]
        .as_str()
        .ok_or("repo config missing name")?
        .to_string();
    let branch_sym = resolve_repo_branch_symref(branch_override.as_deref())?;
    let did = repo_effective_did().await?;
    let client = reqwest::Client::new();
    let tip = remote_tip(&client, &base, &repo_name, &branch_sym, &did)
        .await?
        .ok_or("remote ref missing tip")?;

    let head_sym = read_head_branch()?;
    let local_tip = read_tip_for_symref(&branch_sym)?.unwrap_or_default();
    let branch_short = branch_sym
        .strip_prefix("refs/heads/")
        .unwrap_or(&branch_sym);
    if local_tip == tip {
        println!("Already up to date ({}).", branch_short.bright_white());
        return Ok(());
    }

    fetch_chain_into_store(&client, &base, &tip, &local_tip, depth).await?;

    // Reject a non-fast-forward pull (diverged) unless the local branch is empty
    // or strictly behind. (Merging requires `spacekit repo merge`.)
    if !local_tip.is_empty() && !is_ancestor_local(&local_tip, &tip) {
        return Err(format!(
            "branch '{branch_short}' has diverged from the remote — run `spacekit repo merge` after fetch"
        )
        .into());
    }

    let head_pkg = load_commit_local(&tip)?;
    let commit = parse_commit_from_fact_package(&head_pkg)?;
    write_tip_for_symref(&branch_sym, &tip)?;
    if branch_sym == head_sym {
        append_reflog(&local_tip, &tip, &format!("pull: {branch_short}"));
        let root = repo_root();
        materialize_commit_tree(&root, &commit, &client, &base).await?;
        save_index(&commit.tree)?;
        println!("{}", format!("✅ Pulled {}", branch_short).green());
    } else {
        println!(
            "{} {}",
            format!("✅ Updated ref {}", branch_short).green(),
            "(not checked out; run `spacekit repo checkout` to sync working tree)".dimmed()
        );
    }
    Ok(())
}

fn run_branch_list() -> Result<(), Box<dyn std::error::Error>> {
    let heads_dir = repo_base().join("refs/heads");
    if !heads_dir.exists() {
        println!("{}", "(no refs/heads)".dimmed());
        return Ok(());
    }
    let current = read_head_branch()?;
    let mut names = Vec::new();
    for e in std::fs::read_dir(&heads_dir)? {
        let e = e?;
        if e.path().is_file() {
            names.push(e.file_name().to_string_lossy().into_owned());
        }
    }
    names.sort();
    for n in names {
        let sym = format!("refs/heads/{}", n);
        let marker = if sym == current { "*" } else { " " };
        println!("{} {}", marker, n.green());
    }
    Ok(())
}

fn run_branch_create(short: &str) -> Result<(), Box<dyn std::error::Error>> {
    refs_heads_symref(short)?;
    let rf = branch_to_ref_file(short);
    if rf.exists() {
        return Err(format!("branch '{}' already exists", short).into());
    }
    let tip_line = match read_tip_hex()? {
        Some(t) if !t.is_empty() => format!("{}\n", t),
        _ => String::new(),
    };
    std::fs::create_dir_all(rf.parent().unwrap())?;
    std::fs::write(&rf, tip_line)?;
    println!("{}", format!("✅ Branch {}", short).green());
    Ok(())
}

fn run_branch_delete(short: &str) -> Result<(), Box<dyn std::error::Error>> {
    refs_heads_symref(short)?;
    let current = read_head_branch()?;
    if current == format!("refs/heads/{}", short) {
        return Err("cannot delete the branch you are on".into());
    }
    let rf = branch_to_ref_file(short);
    if !rf.exists() {
        return Err(format!("branch '{}' does not exist", short).into());
    }
    std::fs::remove_file(&rf)?;
    println!("{}", format!("✅ Deleted branch {}", short).green());
    Ok(())
}

async fn run_checkout(
    branch: String,
    storage_url: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    refs_heads_symref(&branch)?;
    let sym = format!("refs/heads/{}", branch);
    let rf = branch_to_ref_file(&branch);
    if !rf.exists() {
        return Err(format!("unknown local branch '{}'", branch).into());
    }
    std::fs::write(repo_base().join("HEAD"), format!("{}\n", sym))?;

    let tip = std::fs::read_to_string(&rf)?;
    let tip = tip.trim();
    if tip.is_empty() {
        save_index(&BTreeMap::new())?;
        println!(
            "{}",
            format!("✅ Switched to {} (no commits yet)", branch).green()
        );
        return Ok(());
    }

    let pkg = match load_commit_local(tip) {
        Ok(p) => p,
        Err(_) => {
            let (base, _) = resolve_remote_storage_base_url(storage_url.as_deref())?;
            let client = reqwest::Client::new();
            fetch_fact_remote(&client, &base, tip).await?
        }
    };
    save_commit_local(&pkg)?;
    let commit = parse_commit_from_fact_package(&pkg)?;

    let (base, _) = resolve_remote_storage_base_url(storage_url.as_deref())?;
    let client = reqwest::Client::new();
    let root = repo_root();
    materialize_commit_tree(&root, &commit, &client, &base).await?;
    save_index(&commit.tree)?;
    println!("{}", format!("✅ Switched to {}", branch).green());
    Ok(())
}

async fn run_list(storage_url: Option<String>) -> Result<(), Box<dyn std::error::Error>> {
    let (base, _) = resolve_remote_storage_base_url(storage_url.as_deref())?;
    let did = repo_effective_did().await?;
    let client = reqwest::Client::new();

    let url = format!("{}/api/documents/repo_registry", base.trim_end_matches('/'));
    let resp = client
        .get(&url)
        .header("Authorization", format!("DID {}", did))
        .send()
        .await?;

    if !resp.status().is_success() {
        println!(
            "{}",
            "No repos found (or storage node unreachable)".dimmed()
        );
        return Ok(());
    }

    let body: serde_json::Value = resp.json().await?;
    let docs = body
        .get("documents")
        .and_then(|d| d.as_array())
        .cloned()
        .unwrap_or_default();

    if docs.is_empty() {
        println!("{}", "No repos found.".dimmed());
        println!(
            "{}",
            "   💡 Push a repo: spacekit repo init && spacekit repo add && spacekit repo commit -m \"init\" && spacekit repo push"
                .dimmed()
        );
        return Ok(());
    }

    println!(
        "{}",
        format!("📦 {} repo(s) on {}", docs.len(), base).bright_white()
    );
    println!();
    for doc in &docs {
        let data = doc.get("data").cloned().unwrap_or_default();
        let name = data
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("(unnamed)");
        let owner = data
            .get("owner_did")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let branch = data
            .get("default_branch")
            .and_then(|v| v.as_str())
            .unwrap_or("main");
        let visibility = data
            .get("visibility")
            .and_then(|v| v.as_str())
            .unwrap_or("public");
        let updated = data
            .get("updated_at")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let vis_display = match visibility {
            "private" => "🔒 private".yellow().to_string(),
            _ => "🌐 public".green().to_string(),
        };
        println!("   {} {} ({})", name.cyan(), vis_display, branch);
        if !owner.is_empty() && owner != "unknown" {
            println!("      Owner: {}", owner.dimmed());
        }
        if !updated.is_empty() {
            println!("      Updated: {}", updated.dimmed());
        }
        println!();
    }
    Ok(())
}

// ===========================================================================
// Small shared helpers for the new subcommands
// ===========================================================================

fn decode_id(hex_str: &str) -> Result<[u8; 32], Box<dyn std::error::Error>> {
    let v = hex::decode(hex_str.trim())?;
    if v.len() != 32 {
        return Err("invalid commit id (expected 64 hex chars)".into());
    }
    let mut b = [0u8; 32];
    b.copy_from_slice(&v);
    Ok(b)
}

fn require_blob(h: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    match load_blob_local(h) {
        Some(b) => Ok(b),
        None => Err(format!("missing blob {h} locally (run `spacekit repo fetch`)").into()),
    }
}

fn commit_content(id: &str) -> Result<CommitContent, Box<dyn std::error::Error>> {
    let pkg = load_commit_local(id)?;
    Ok(parse_commit_from_fact_package(&pkg)?)
}

fn commit_tree(id: &str) -> Result<BTreeMap<String, String>, Box<dyn std::error::Error>> {
    Ok(commit_content(id)?.tree)
}

fn head_tip_tree() -> Result<BTreeMap<String, String>, Box<dyn std::error::Error>> {
    match read_tip_hex()? {
        Some(t) if !t.is_empty() => commit_tree(&t),
        _ => Ok(BTreeMap::new()),
    }
}

// ---- merge state ----------------------------------------------------------

fn merge_head_path() -> PathBuf {
    repo_base().join("MERGE_HEAD")
}
fn merge_msg_path() -> PathBuf {
    repo_base().join("MERGE_MSG")
}
fn read_merge_head() -> Option<String> {
    std::fs::read_to_string(merge_head_path())
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}
fn merge_state_active() -> bool {
    read_merge_head().is_some()
}
fn clear_merge_state() {
    let _ = std::fs::remove_file(merge_head_path());
    let _ = std::fs::remove_file(merge_msg_path());
}

// ---- hooks ----------------------------------------------------------------

/// Run a repo hook (`.spacekit/repo/hooks/<name>`) if present + executable.
/// Returns `true` to proceed (hook absent, not executable, or exited 0).
fn run_hook(name: &str) -> bool {
    let p = repo_base().join("hooks").join(name);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(md) = std::fs::metadata(&p) {
            if md.permissions().mode() & 0o111 != 0 {
                match std::process::Command::new(&p)
                    .current_dir(repo_root())
                    .status()
                {
                    Ok(st) => return st.success(),
                    Err(_) => return true,
                }
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = &p;
    }
    true
}

// ---- remote-tracking refs (for `fetch`) -----------------------------------

fn remote_tracking_path(branch_short: &str) -> PathBuf {
    repo_base().join("refs/remotes/origin").join(branch_short)
}
fn read_remote_tracking(branch_short: &str) -> String {
    std::fs::read_to_string(remote_tracking_path(branch_short))
        .ok()
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}
fn write_remote_tracking(branch_short: &str, tip: &str) -> Result<(), Box<dyn std::error::Error>> {
    let p = remote_tracking_path(branch_short);
    std::fs::create_dir_all(p.parent().unwrap())?;
    std::fs::write(&p, tip.trim())?;
    Ok(())
}

// ---- topological ordering of a commit's reachable ancestors ---------------

fn topo_order_ancestors(tip: &str) -> Vec<String> {
    fn dfs(id: &str, visited: &mut BTreeSet<String>, order: &mut Vec<String>) {
        if id.is_empty() || !visited.insert(id.to_string()) {
            return;
        }
        if let Ok(parents) = commit_parents_local(id) {
            for p in parents {
                dfs(&p, visited, order);
            }
        }
        order.push(id.to_string());
    }
    let mut visited = BTreeSet::new();
    let mut order = Vec::new();
    dfs(tip, &mut visited, &mut order);
    order
}

/// Write every path in `tree` to the working dir from the local blob store,
/// remove tracked files no longer present, and apply file modes.
fn write_tree_to_working(
    root: &Path,
    tree: &BTreeMap<String, String>,
    modes: &BTreeMap<String, u32>,
) -> Result<(), Box<dyn std::error::Error>> {
    let current = working_tree_hashes(root)?;
    for rel in current.keys() {
        if !tree.contains_key(rel) {
            let _ = std::fs::remove_file(root.join(rel));
        }
    }
    for (rel, h) in tree {
        let bytes = require_blob(h)?;
        let abs = root.join(rel);
        if let Some(par) = abs.parent() {
            std::fs::create_dir_all(par)?;
        }
        std::fs::write(&abs, &bytes)?;
        apply_file_mode(&abs, modes.get(rel).copied().unwrap_or(DEFAULT_FILE_MODE));
    }
    Ok(())
}

// ===========================================================================
// Command implementations
// ===========================================================================

fn run_add(paths: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let root = repo_root();
    let mut index = load_index()?;
    if paths.is_empty() {
        index = working_tree_hashes(&root)?;
    } else {
        for p in paths {
            let pb = PathBuf::from(p);
            let abs = if pb.is_absolute() { pb } else { root.join(&pb) };
            let rel = abs
                .strip_prefix(&root)?
                .to_string_lossy()
                .replace('\\', "/");
            if abs.is_file() {
                index.insert(rel, blake3_file(&abs)?);
            }
        }
    }
    // Cache staged blobs locally so diff/commit/push work offline.
    for (rel, h) in &index {
        if !have_blob_local(h) {
            if let Ok(bytes) = std::fs::read(root.join(rel)) {
                let _ = store_blob_local_verified(h, &bytes);
            }
        }
    }
    save_index(&index)?;
    println!("{}", "✅ Staged".green());
    Ok(())
}

fn run_status() -> Result<(), Box<dyn std::error::Error>> {
    let root = repo_root();
    let head_ref = read_head_branch()?;
    let short = head_ref.strip_prefix("refs/heads/").unwrap_or(&head_ref);
    println!("{}", format!("On branch {}", short).bright_white());
    if merge_state_active() {
        println!(
            "{}",
            "You have unmerged paths (resolve, then `spacekit repo merge --continue`, or `--abort`)"
                .yellow()
        );
    }
    let index = load_index()?;
    let head_tree = head_tip_tree()?;
    let staged = diff_trees(
        &snapshot_from_hex_tree(&head_tree)?,
        &snapshot_from_hex_tree(&index)?,
    );
    let work = working_tree_hashes(&root)?;
    let unstaged = diff_trees(
        &snapshot_from_hex_tree(&index)?,
        &snapshot_from_hex_tree(&work)?,
    );
    if !staged.is_empty() {
        println!("{}", "Changes to be committed:".green());
        for c in &staged {
            println!("  {}", format_change(c).green());
        }
    }
    if !unstaged.is_empty() {
        println!("{}", "Changes not staged for commit:".yellow());
        for c in &unstaged {
            println!("  {}", format_change(c).yellow());
        }
    }
    if staged.is_empty() && unstaged.is_empty() {
        println!("{}", "Nothing to commit, working tree clean".green());
    }
    Ok(())
}

async fn run_commit(message: &str, amend: bool) -> Result<(), Box<dyn std::error::Error>> {
    let _cfg: CLIConfig = load_cli_config().await?;
    let root = repo_root();
    let mut index = load_index()?;
    let staged = auto_stage_working_tree(&mut index, &root)?;
    if staged > 0 {
        save_index(&index)?;
        println!(
            "{}",
            format!("Auto-staged {} file(s) from working tree", staged).yellow()
        );
    }
    if index.is_empty() {
        return Err("Nothing to commit (add files with `spacekit repo add`)".into());
    }
    if !run_hook("pre-commit") {
        return Err("pre-commit hook rejected the commit".into());
    }

    let tip = read_tip_hex()?.filter(|t| !t.is_empty());
    let merge_second = read_merge_head();

    let parents: Vec<[u8; 32]> = if amend {
        let cur = tip
            .clone()
            .ok_or("nothing to amend (no commits on this branch)")?;
        load_commit_local(&cur)?.dependencies
    } else {
        let mut ps = Vec::new();
        if let Some(t) = &tip {
            ps.push(decode_id(t)?);
        }
        if let Some(m) = &merge_second {
            ps.push(decode_id(m)?);
        }
        ps
    };

    let did = repo_effective_did().await?;
    let mut commit = CommitContent::new(
        index.clone(),
        message.to_string(),
        did.clone(),
        chrono::Utc::now().timestamp() as u64,
    );
    commit.modes = modes_for_tree(&root, &index);
    let mut pkg = build_commit_fact_package(&did, parents, commit.clone())?;
    let signed = sign_commit_pkg(&mut pkg);
    let id = hex::encode(pkg.fact_id);

    if merge_second.is_none() && !amend {
        if let Some(t) = &tip {
            if let Ok(parent_c) = commit_content(t) {
                if parent_c.tree == commit.tree {
                    println!(
                        "{}",
                        "⚠️  Commit tree is identical to parent — no file content changed".yellow()
                    );
                }
            }
        }
    }

    save_commit_local(&pkg)?;
    for (rel, h) in &index {
        if !have_blob_local(h) {
            if let Ok(b) = std::fs::read(root.join(rel)) {
                let _ = store_blob_local_verified(h, &b);
            }
        }
    }
    let old_tip = tip.clone().unwrap_or_default();
    write_tip_hex(&id)?;
    save_index(&index)?;
    clear_merge_state();
    append_reflog(
        &old_tip,
        &id,
        &format!("commit{}: {}", if amend { " (amend)" } else { "" }, message),
    );
    run_hook("post-commit");
    let sig_note = if signed {
        "signed".green()
    } else {
        "unsigned (no ~/.spacekit/did_wallet.json)".yellow()
    };
    println!("{} {} [{}]", "✅ Commit".green(), id.green(), sig_note);
    Ok(())
}

async fn run_push(
    storage_url: Option<String>,
    branch_opt: Option<String>,
    force: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let (base, _) = resolve_remote_storage_base_url(storage_url.as_deref())?;
    let repo_cfg: serde_json::Value = read_json(&repo_base().join("config.json"))?;
    let repo_name = repo_cfg["name"]
        .as_str()
        .ok_or("repo config missing name")?
        .to_string();
    let branch_sym = resolve_repo_branch_symref(branch_opt.as_deref())?;
    let branch_short = branch_sym
        .strip_prefix("refs/heads/")
        .unwrap_or(&branch_sym);
    let rf = branch_to_ref_file(&branch_sym);
    if !rf.exists() {
        return Err(format!(
            "branch '{}' does not exist locally (create with `spacekit repo branch {}`)",
            branch_short, branch_short
        )
        .into());
    }
    let tip = read_tip_for_symref(&branch_sym)?
        .filter(|t| !t.trim().is_empty())
        .ok_or_else(|| format!("no commits to push on branch '{}'", branch_short))?;

    let client = reqwest::Client::new();
    let did = repo_effective_did().await?;
    let storage_name = resolve_repo_storage_name(&repo_name, &did);

    // Non-fast-forward guard: refuse to overwrite remote work we don't contain.
    if let Some(rt) = remote_tip(&client, &base, &storage_name, &branch_sym, &did).await? {
        if !rt.is_empty() && rt != tip {
            if load_commit_local(&rt).is_err() {
                let _ = fetch_chain_into_store(&client, &base, &rt, &tip, None).await;
            }
            if !is_ancestor_local(&rt, &tip) && !force {
                return Err(format!(
                    "non-fast-forward: remote '{branch_short}' has commits you don't have — \
                     `spacekit repo pull`/`merge` first, or pass --force"
                )
                .into());
            }
        }
    }

    if !run_hook("pre-push") {
        return Err("pre-push hook rejected the push".into());
    }

    // Push the full reachable history (handles merge commits), oldest-first.
    let chain = topo_order_ancestors(&tip);
    let mut needed_hashes: HashSet<String> = HashSet::new();
    for id in &chain {
        let c = commit_content(id)?;
        for h in c.tree.into_values() {
            needed_hashes.insert(h);
        }
    }

    let hashes: Vec<String> = needed_hashes.into_iter().collect();
    let exists_url = format!("{}/blobs/exists", base.trim_end_matches('/'));
    let body = serde_json::json!({ "hashes": hashes });
    let resp: serde_json::Value = client
        .post(&exists_url)
        .json(&body)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let missing: Vec<String> = resp["missing"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();

    let root = repo_root();
    let hmap = hash_to_path_from_chain(&chain)?;
    for h in &missing {
        let bytes =
            local_bytes_for_hash(&root, h, hmap.get(h).map(|s| s.as_str())).ok_or_else(|| {
                format!(
                    "blob {} not found locally (restore files or re-checkout)",
                    h
                )
            })?;
        put_blob(&client, &base, h, &bytes).await?;
    }

    for id in chain.iter() {
        let pkg = load_commit_local(id)?;
        post_fact(&client, &base, &pkg).await?;
    }

    let session = load_cli_config()
        .await
        .ok()
        .and_then(|c| identity_cmd::website_session_token(&c));
    authorize_repo_push_via_website_api(&client, &did, &storage_name, session.as_deref()).await?;
    let (owner_slug, repo_slug, owner_type) = parse_repo_owner_type(&storage_name, &did);
    let collection = doc_collection(&storage_name);
    let doc_id = ref_doc_id(&branch_sym);
    let url = document_api_url(&base, &collection, &doc_id);
    let doc_body = RepoRefJson { tip: tip.clone() };
    let r = client
        .put(&url)
        .header("Authorization", format!("DID {}", did))
        .header("content-type", "application/json")
        .json(&doc_body)
        .send()
        .await?;
    if !r.status().is_success() {
        let t = r.text().await.unwrap_or_default();
        return Err(format!(
            "PUT ref {} -> {}",
            url,
            t.chars().take(300).collect::<String>()
        )
        .into());
    }

    const WEBSITE_ADMIN_DID: &str = "did:spacekit:admin:website-api";
    let _ = client
        .put(&url)
        .header("Authorization", format!("DID {}", WEBSITE_ADMIN_DID))
        .header("content-type", "application/json")
        .json(&doc_body)
        .send()
        .await;

    let registry_url = document_api_url(&base, "repo_registry", &storage_name);
    let registry_body = serde_json::json!({
        "name": storage_name,
        "storage_name": storage_name,
        "full_name": storage_name,
        "owner_slug": owner_slug,
        "repo_slug": repo_slug,
        "owner_type": owner_type,
        "owner_did": did,
        "default_branch": branch_short,
        "updated_at": chrono::Utc::now().to_rfc3339(),
        "visibility": "public",
    });
    for reg_did in [did.as_str(), WEBSITE_ADMIN_DID] {
        let _ = client
            .put(&registry_url)
            .header("Authorization", format!("DID {}", reg_did))
            .header("content-type", "application/json")
            .json(&registry_body)
            .send()
            .await;
    }

    println!("{}", format!("✅ Pushed {}", branch_short).green());
    Ok(())
}

async fn run_fetch(
    storage_url: Option<String>,
    branch_opt: Option<String>,
    depth: Option<usize>,
) -> Result<(), Box<dyn std::error::Error>> {
    let (base, _) = resolve_remote_storage_base_url(storage_url.as_deref())?;
    let repo_cfg: serde_json::Value = read_json(&repo_base().join("config.json"))?;
    let repo_name = repo_cfg["name"]
        .as_str()
        .ok_or("repo config missing name")?
        .to_string();
    let branch_sym = resolve_repo_branch_symref(branch_opt.as_deref())?;
    let branch_short = branch_sym
        .strip_prefix("refs/heads/")
        .unwrap_or(&branch_sym);
    let did = repo_effective_did().await?;
    let client = reqwest::Client::new();
    let tip = remote_tip(&client, &base, &repo_name, &branch_sym, &did)
        .await?
        .ok_or("remote ref missing tip")?;
    let local_tracking = read_remote_tracking(branch_short);
    fetch_chain_into_store(&client, &base, &tip, &local_tracking, depth).await?;
    write_remote_tracking(branch_short, &tip)?;
    println!(
        "{} origin/{} -> {}",
        "✅ Fetched".green(),
        branch_short,
        &tip[..tip.len().min(12)]
    );
    Ok(())
}

/// Resolve, fetch, and apply a 3-way merge of `branch` into HEAD.
async fn run_merge(
    branch: Option<String>,
    do_continue: bool,
    abort: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let root = repo_root();

    if abort {
        if !merge_state_active() {
            return Err("no merge in progress".into());
        }
        let head_tip = read_tip_hex()?.unwrap_or_default();
        if !head_tip.is_empty() {
            let c = commit_content(&head_tip)?;
            write_tree_to_working(&root, &c.tree, &c.modes)?;
            save_index(&c.tree)?;
        }
        clear_merge_state();
        println!("{}", "✅ Merge aborted".green());
        return Ok(());
    }

    if do_continue {
        if !merge_state_active() {
            return Err("no merge in progress".into());
        }
        let msg = std::fs::read_to_string(merge_msg_path()).unwrap_or_else(|_| "Merge".to_string());
        // Verify there are no remaining conflict markers in tracked files.
        let work = working_tree_hashes(&root)?;
        for rel in work.keys() {
            if let Ok(bytes) = std::fs::read(root.join(rel)) {
                if bytes.windows(7).any(|w| w == b"<<<<<<<") {
                    return Err(format!("unresolved conflict markers remain in {rel}").into());
                }
            }
        }
        // Stage resolved tree, then commit (run_commit picks up MERGE_HEAD).
        let mut index = load_index()?;
        auto_stage_working_tree(&mut index, &root)?;
        save_index(&index)?;
        return run_commit(&msg, false).await;
    }

    let theirs_branch = branch.ok_or("specify a branch to merge")?;
    validate_branch_short(&theirs_branch)?;
    if merge_state_active() {
        return Err("a merge is already in progress (use --continue or --abort)".into());
    }
    let ours = read_tip_hex()?
        .filter(|t| !t.is_empty())
        .ok_or("HEAD has no commits")?;
    let theirs = read_tip_for_symref(&refs_heads_symref(&theirs_branch)?)?
        .filter(|t| !t.is_empty())
        .ok_or_else(|| format!("branch '{theirs_branch}' has no commits"))?;

    if is_ancestor_local(&theirs, &ours) {
        println!("{}", "Already up to date.".green());
        return Ok(());
    }
    if is_ancestor_local(&ours, &theirs) {
        // Fast-forward.
        let c = commit_content(&theirs)?;
        write_tree_to_working(&root, &c.tree, &c.modes)?;
        save_index(&c.tree)?;
        write_tip_hex(&theirs)?;
        append_reflog(
            &ours,
            &theirs,
            &format!("merge {theirs_branch}: fast-forward"),
        );
        println!(
            "{}",
            format!("✅ Fast-forwarded to {}", &theirs[..12]).green()
        );
        return Ok(());
    }

    let base_id = merge_base_local(&ours, &theirs);
    let base_tree = match &base_id {
        Some(b) => commit_tree(b)?,
        None => BTreeMap::new(),
    };
    let our_tree = commit_tree(&ours)?;
    let their_tree = commit_tree(&theirs)?;

    let result = merge_trees(
        &snapshot_from_hex_tree(&base_tree)?,
        &snapshot_from_hex_tree(&our_tree)?,
        &snapshot_from_hex_tree(&their_tree)?,
    );

    // Start from the auto-merged tree (hex), then resolve each conflict.
    let mut merged: BTreeMap<String, String> = result
        .tree
        .entries
        .iter()
        .map(|(p, h)| (p.clone(), hex::encode(h)))
        .collect();
    let mut conflicted: Vec<String> = Vec::new();

    for conflict in &result.conflicts {
        match conflict {
            MergeConflict::Content {
                path,
                base_hash,
                our_hash,
                their_hash,
            } => {
                let b = require_blob(&hex::encode(base_hash))?;
                let o = require_blob(&hex::encode(our_hash))?;
                let t = require_blob(&hex::encode(their_hash))?;
                let m = merge_blobs(&b, &o, &t);
                let h = store_blob_local_bytes(&m.content)?;
                merged.insert(path.clone(), h);
                if m.has_conflicts {
                    conflicted.push(path.clone());
                }
            }
            MergeConflict::AddAdd {
                path,
                our_hash,
                their_hash,
            } => {
                let o = require_blob(&hex::encode(our_hash))?;
                let t = require_blob(&hex::encode(their_hash))?;
                let m = merge_blobs(&[], &o, &t);
                let h = store_blob_local_bytes(&m.content)?;
                merged.insert(path.clone(), h);
                conflicted.push(path.clone());
            }
            MergeConflict::ModifyDelete {
                path,
                modified_hash,
                ..
            } => {
                // Keep the modified version; flag for the user to decide.
                merged.insert(path.clone(), hex::encode(modified_hash));
                conflicted.push(path.clone());
            }
        }
    }

    // Materialize merged tree (with markers in conflicted files) + stage it.
    // Preserve the executable bit from whichever side recorded it.
    let our_modes = commit_content(&ours)?.modes;
    let their_modes = commit_content(&theirs)?.modes;
    let mut modes: BTreeMap<String, u32> = BTreeMap::new();
    for rel in merged.keys() {
        if let Some(m) = our_modes.get(rel).or_else(|| their_modes.get(rel)) {
            modes.insert(rel.clone(), *m);
        }
    }
    write_tree_to_working(&root, &merged, &modes)?;
    save_index(&merged)?;

    std::fs::write(merge_head_path(), &theirs)?;
    std::fs::write(merge_msg_path(), format!("Merge branch '{theirs_branch}'"))?;

    if conflicted.is_empty() {
        let msg = format!("Merge branch '{theirs_branch}'");
        return run_commit(&msg, false).await;
    }

    println!(
        "{}",
        "⚠️  Automatic merge failed; fix conflicts then run `spacekit repo merge --continue`"
            .yellow()
    );
    for p in &conflicted {
        println!("  {} {}", "both modified:".red(), p);
    }
    Ok(())
}

fn print_tree_diff(
    old_tree: &BTreeMap<String, String>,
    new_tree: &BTreeMap<String, String>,
    name_only: bool,
    content: bool,
    old_commit: Option<&str>,
    new_commit: Option<&str>,
    root: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let sa = snapshot_from_hex_tree(old_tree)?;
    let sb = snapshot_from_hex_tree(new_tree)?;
    let changes = diff_trees(&sa, &sb);
    let (renames, rest) = detect_exact_renames(&changes);
    for r in &renames {
        if name_only {
            println!("{} -> {}", r.from, r.to);
        } else {
            println!("renamed: {} -> {}", r.from, r.to);
        }
    }
    for ch in &rest {
        if name_only {
            println!("{}", ch.path());
            continue;
        }
        println!("{}", format_change(ch));
        if content {
            let path = ch.path();
            let bytes_for = |commit: Option<&str>, hash: Option<&str>| -> Option<Vec<u8>> {
                match (commit, hash) {
                    (Some(_), Some(h)) => load_blob_local(h),
                    (None, _) => std::fs::read(root.join(path)).ok(),
                    _ => None,
                }
            };
            let (old_bytes, new_bytes) = match ch {
                TreeChange::Added { hash, .. } => (
                    Vec::new(),
                    bytes_for(new_commit, Some(&hex::encode(hash))).unwrap_or_default(),
                ),
                TreeChange::Removed { hash, .. } => (
                    bytes_for(old_commit, Some(&hex::encode(hash))).unwrap_or_default(),
                    Vec::new(),
                ),
                TreeChange::Modified {
                    old_hash, new_hash, ..
                } => (
                    bytes_for(old_commit, Some(&hex::encode(old_hash))).unwrap_or_default(),
                    bytes_for(new_commit, Some(&hex::encode(new_hash))).unwrap_or_default(),
                ),
            };
            let ud = unified_diff(
                &old_bytes,
                &new_bytes,
                &format!("a/{path}"),
                &format!("b/{path}"),
                3,
            );
            for line in ud.lines() {
                let colored = if line.starts_with('+') {
                    line.green()
                } else if line.starts_with('-') {
                    line.red()
                } else if line.starts_with("@@") {
                    line.cyan()
                } else {
                    line.normal()
                };
                println!("{}", colored);
            }
        }
    }
    if renames.is_empty() && rest.is_empty() {
        println!("{}", "(no changes)".dimmed());
    }
    Ok(())
}

fn run_show(commit: Option<String>) -> Result<(), Box<dyn std::error::Error>> {
    let id = match commit {
        Some(c) => c,
        None => read_tip_hex()?.ok_or("no commits")?,
    };
    let pkg = load_commit_local(&id)?;
    let c = parse_commit_from_fact_package(&pkg)?;
    let status = verify_commit_pkg(&pkg);
    println!("{} {}", "commit".yellow(), id.yellow());
    println!("Author: {}", c.author_name);
    println!("Date:   {}", c.timestamp);
    println!("Signature: {}", verify_status_label(status));
    if !pkg.dependencies.is_empty() {
        let parents: Vec<String> = pkg
            .dependencies
            .iter()
            .map(|p| hex::encode(p)[..12].to_string())
            .collect();
        println!("Parents: {}", parents.join(" "));
    }
    println!("\n    {}\n", c.message);
    let parent_tree = match pkg.dependencies.first() {
        Some(p) => commit_tree(&hex::encode(p))?,
        None => BTreeMap::new(),
    };
    let parent_id = pkg.dependencies.first().map(hex::encode);
    let root = repo_root();
    print_tree_diff(
        &parent_tree,
        &c.tree,
        false,
        true,
        parent_id.as_deref(),
        Some(&id),
        &root,
    )
}

fn run_tag(
    name: Option<String>,
    commit: Option<String>,
    delete: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let tags_dir = repo_base().join("refs/tags");
    if let Some(d) = delete {
        let p = tags_dir.join(&d);
        if !p.exists() {
            return Err(format!("tag '{d}' does not exist").into());
        }
        std::fs::remove_file(&p)?;
        println!("{}", format!("✅ Deleted tag {d}").green());
        return Ok(());
    }
    let Some(name) = name else {
        if tags_dir.exists() {
            let mut names: Vec<String> = std::fs::read_dir(&tags_dir)?
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            for n in names {
                let tip = std::fs::read_to_string(tags_dir.join(&n)).unwrap_or_default();
                println!("{}  {}", n.green(), &tip.trim()[..tip.trim().len().min(12)]);
            }
        }
        return Ok(());
    };
    validate_branch_short(&name)?;
    let target = match commit {
        Some(c) => c,
        None => read_tip_hex()?.ok_or("no commits to tag")?,
    };
    if load_commit_local(&target).is_err() {
        return Err(format!("commit {target} not found locally").into());
    }
    std::fs::create_dir_all(&tags_dir)?;
    std::fs::write(tags_dir.join(&name), target.trim())?;
    println!(
        "{}",
        format!("✅ Tagged {name} -> {}", &target[..target.len().min(12)]).green()
    );
    Ok(())
}

fn run_reset(
    commit: Option<String>,
    soft: bool,
    hard: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let root = repo_root();
    let target = match commit {
        Some(c) => c,
        None => read_tip_hex()?.ok_or("no commits")?,
    };
    if load_commit_local(&target).is_err() {
        return Err(format!("commit {target} not found locally").into());
    }
    let old_tip = read_tip_hex()?.unwrap_or_default();
    let c = commit_content(&target)?;
    write_tip_hex(&target)?;
    append_reflog(&old_tip, &target, "reset");
    if !soft {
        save_index(&c.tree)?;
    }
    if hard {
        write_tree_to_working(&root, &c.tree, &c.modes)?;
    }
    let mode = if soft {
        "soft"
    } else if hard {
        "hard"
    } else {
        "mixed"
    };
    println!(
        "{}",
        format!("✅ Reset ({mode}) to {}", &target[..target.len().min(12)]).green()
    );
    Ok(())
}

fn run_restore(
    paths: &[String],
    staged: bool,
    source: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let root = repo_root();
    let src = match source {
        Some(s) => s,
        None => read_tip_hex()?.ok_or("no commits")?,
    };
    let tree = commit_tree(&src)?;
    let mut index = load_index()?;
    for raw in paths {
        let rel = raw.replace('\\', "/");
        match tree.get(&rel) {
            Some(h) => {
                if staged {
                    index.insert(rel.clone(), h.clone());
                } else {
                    let bytes = require_blob(h)?;
                    let abs = root.join(&rel);
                    if let Some(par) = abs.parent() {
                        std::fs::create_dir_all(par)?;
                    }
                    std::fs::write(&abs, &bytes)?;
                }
            }
            None => {
                if staged {
                    index.remove(&rel);
                } else {
                    let _ = std::fs::remove_file(root.join(&rel));
                }
            }
        }
        println!("{} {}", "restored".green(), rel);
    }
    if staged {
        save_index(&index)?;
    }
    Ok(())
}

/// Apply the change between `commit` and its first parent, in either the forward
/// (cherry-pick) or reverse (revert) direction, onto the current HEAD tree.
async fn run_apply_commit(commit: &str, reverse: bool) -> Result<(), Box<dyn std::error::Error>> {
    let root = repo_root();
    if load_commit_local(commit).is_err() {
        return Err(format!("commit {commit} not found locally").into());
    }
    let c = commit_content(commit)?;
    let parent_tree = match load_commit_local(commit)?.dependencies.first() {
        Some(p) => commit_tree(&hex::encode(p))?,
        None => BTreeMap::new(),
    };
    let (from, to) = if reverse {
        (&c.tree, &parent_tree) // undo: move content back toward parent
    } else {
        (&parent_tree, &c.tree) // apply: parent -> commit
    };
    let changes = diff_trees(&snapshot_from_hex_tree(from)?, &snapshot_from_hex_tree(to)?);

    let mut tree = head_tip_tree()?;
    for ch in &changes {
        match ch {
            TreeChange::Added { path, hash } => {
                tree.insert(path.clone(), hex::encode(hash));
            }
            TreeChange::Removed { path, .. } => {
                tree.remove(path);
            }
            TreeChange::Modified { path, new_hash, .. } => {
                tree.insert(path.clone(), hex::encode(new_hash));
            }
        }
    }
    // Ensure all referenced blobs are present locally.
    for h in tree.values() {
        require_blob(h)?;
    }
    let modes = {
        let mut m = BTreeMap::new();
        for (rel, _) in tree.iter() {
            if let Some(md) = c.modes.get(rel) {
                m.insert(rel.clone(), *md);
            }
        }
        m
    };
    write_tree_to_working(&root, &tree, &modes)?;
    save_index(&tree)?;
    let verb = if reverse { "Revert" } else { "Cherry-pick" };
    let msg = if reverse {
        format!("Revert \"{}\"", c.message)
    } else {
        c.message.clone()
    };
    let _ = verb;
    run_commit(&msg, false).await
}

fn run_reflog() -> Result<(), Box<dyn std::error::Error>> {
    let p = reflog_path();
    if !p.exists() {
        println!("{}", "(no reflog)".dimmed());
        return Ok(());
    }
    let s = std::fs::read_to_string(&p)?;
    for line in s.lines().rev() {
        let mut it = line.splitn(4, ' ');
        let _old = it.next().unwrap_or("");
        let new = it.next().unwrap_or("");
        let _ts = it.next().unwrap_or("");
        let msg = it.next().unwrap_or("");
        println!("{} {}", new.get(..12).unwrap_or(new).cyan(), msg);
    }
    Ok(())
}

fn verify_status_label(s: VerifyStatus) -> String {
    match s {
        VerifyStatus::GoodTrusted => "good (trusted: key matches author DID)".green().to_string(),
        VerifyStatus::GoodUntrusted => "valid (untrusted: key ≠ author DID)".yellow().to_string(),
        VerifyStatus::Unsigned => "unsigned".yellow().to_string(),
        VerifyStatus::Bad => "BAD (integrity/signature failure)".red().to_string(),
    }
}

fn run_verify(commit: Option<String>, all: bool) -> Result<(), Box<dyn std::error::Error>> {
    let start = match commit {
        Some(c) => c,
        None => read_tip_hex()?.ok_or("no commits")?,
    };
    let ids = if all {
        topo_order_ancestors(&start)
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
    } else {
        vec![start]
    };
    let mut bad = 0usize;
    for id in &ids {
        let pkg = load_commit_local(id)?;
        let status = verify_commit_pkg(&pkg);
        if status == VerifyStatus::Bad {
            bad += 1;
        }
        println!(
            "{} {}",
            &id[..id.len().min(12)].cyan(),
            verify_status_label(status)
        );
    }
    if bad > 0 {
        return Err(format!("{bad} commit(s) failed verification").into());
    }
    Ok(())
}

fn run_gc() -> Result<(), Box<dyn std::error::Error>> {
    // Reachable commits = ancestors of every head + tag (+ HEAD).
    let mut reachable_commits: BTreeSet<String> = BTreeSet::new();
    let mut tips: Vec<String> = Vec::new();
    let heads_dir = repo_base().join("refs/heads");
    if heads_dir.exists() {
        for e in std::fs::read_dir(&heads_dir)? {
            let t = std::fs::read_to_string(e?.path()).unwrap_or_default();
            let t = t.trim().to_string();
            if !t.is_empty() {
                tips.push(t);
            }
        }
    }
    let tags_dir = repo_base().join("refs/tags");
    if tags_dir.exists() {
        for e in std::fs::read_dir(&tags_dir)? {
            let t = std::fs::read_to_string(e?.path()).unwrap_or_default();
            let t = t.trim().to_string();
            if !t.is_empty() {
                tips.push(t);
            }
        }
    }
    for t in &tips {
        for a in ancestors_local(t) {
            reachable_commits.insert(a);
        }
    }
    let mut reachable_blobs: BTreeSet<String> = BTreeSet::new();
    for id in &reachable_commits {
        if let Ok(c) = commit_content(id) {
            for h in c.tree.into_values() {
                reachable_blobs.insert(h);
            }
        }
    }

    let mut pruned_commits = 0usize;
    let commits_dir = repo_base().join("objects/commits");
    if commits_dir.exists() {
        for pre in std::fs::read_dir(&commits_dir)? {
            let pre = pre?;
            if !pre.path().is_dir() {
                continue;
            }
            for f in std::fs::read_dir(pre.path())? {
                let f = f?;
                let name = f.file_name().to_string_lossy().into_owned();
                let id = name.trim_end_matches(".json");
                if !reachable_commits.contains(id) {
                    let _ = std::fs::remove_file(f.path());
                    pruned_commits += 1;
                }
            }
        }
    }
    let mut pruned_blobs = 0usize;
    let blobs_dir = repo_base().join("objects/blobs");
    if blobs_dir.exists() {
        for pre in std::fs::read_dir(&blobs_dir)? {
            let pre = pre?;
            if !pre.path().is_dir() {
                continue;
            }
            for f in std::fs::read_dir(pre.path())? {
                let f = f?;
                let name = f.file_name().to_string_lossy().into_owned();
                if !reachable_blobs.contains(&name) {
                    let _ = std::fs::remove_file(f.path());
                    pruned_blobs += 1;
                }
            }
        }
    }
    println!(
        "{}",
        format!("✅ gc: pruned {pruned_commits} commit(s), {pruned_blobs} blob(s)").green()
    );
    Ok(())
}

async fn run_diff(
    a: Option<String>,
    b: Option<String>,
    content: bool,
    name_only: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let root = repo_root();
    match (a, b) {
        (None, None) => {
            // Working tree vs HEAD tip (like `git diff HEAD`).
            let head_tree = head_tip_tree()?;
            let work = working_tree_hashes(&root)?;
            print_tree_diff(
                &head_tree,
                &work,
                name_only,
                content,
                read_tip_hex()?.as_deref(),
                None,
                &root,
            )
        }
        (a, b) => {
            let tip = read_tip_hex()?.ok_or("no commits")?;
            let chain = parent_chain_local(&tip)?;
            let id_b = b.unwrap_or_else(|| chain.first().cloned().unwrap_or_default());
            let id_a = a.or_else(|| chain.get(1).cloned()).unwrap_or_default();
            if id_a.is_empty() || id_b.is_empty() {
                return Err("need two commits (make a second commit or pass --a/--b)".into());
            }
            let ta = commit_tree(&id_a)?;
            let tb = commit_tree(&id_b)?;
            print_tree_diff(
                &ta,
                &tb,
                name_only,
                content,
                Some(&id_a),
                Some(&id_b),
                &root,
            )
        }
    }
}

fn run_log(limit: usize, graph: bool) -> Result<(), Box<dyn std::error::Error>> {
    let tip = read_tip_hex()?.ok_or("no commits")?;
    // BFS over the full DAG, most-recent-first by timestamp.
    let mut seen = BTreeSet::new();
    let mut frontier: Vec<(u64, String)> = Vec::new();
    if let Ok(c) = commit_content(&tip) {
        frontier.push((c.timestamp, tip.clone()));
    }
    let mut count = 0usize;
    while let Some(pos) = frontier
        .iter()
        .enumerate()
        .max_by_key(|(_, (ts, _))| *ts)
        .map(|(i, _)| i)
    {
        let (_, id) = frontier.remove(pos);
        if !seen.insert(id.clone()) {
            continue;
        }
        let pkg = match load_commit_local(&id) {
            Ok(p) => p,
            Err(_) => continue,
        };
        let c = parse_commit_from_fact_package(&pkg)?;
        let is_merge = pkg.dependencies.len() > 1;
        let marker = if graph {
            if is_merge {
                "*─┐ "
            } else {
                "*   "
            }
        } else {
            ""
        };
        println!("{}{} {}", marker, id[..id.len().min(12)].cyan(), c.message);
        count += 1;
        if count >= limit {
            break;
        }
        for p in &pkg.dependencies {
            let pid = hex::encode(p);
            if !seen.contains(&pid) {
                let ts = commit_content(&pid).map(|c| c.timestamp).unwrap_or(0);
                frontier.push((ts, pid));
            }
        }
    }
    Ok(())
}

pub(super) async fn handle_repo_command(
    cmd: &RepoCommands,
) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        RepoCommands::Clone {
            remote,
            repo_name,
            dir,
            depth,
        } => clone_remote(remote.clone(), repo_name.clone(), dir.clone(), *depth).await,
        RepoCommands::Init { name, remote } => run_init(name.clone(), remote.clone()).await,
        RepoCommands::Status => run_status(),
        RepoCommands::Add { paths } => run_add(paths),
        RepoCommands::Commit { message, amend } => run_commit(message, *amend).await,
        RepoCommands::Push {
            storage_url,
            branch: branch_opt,
            force,
        } => run_push(storage_url.clone(), branch_opt.clone(), *force).await,
        RepoCommands::Pull {
            storage_url,
            branch: branch_opt,
            depth,
        } => run_pull(storage_url.clone(), branch_opt.clone(), *depth).await,
        RepoCommands::Fetch {
            storage_url,
            branch: branch_opt,
            depth,
        } => run_fetch(storage_url.clone(), branch_opt.clone(), *depth).await,
        RepoCommands::Merge {
            branch,
            r#continue,
            abort,
        } => run_merge(branch.clone(), *r#continue, *abort).await,
        RepoCommands::Log { limit, graph } => run_log(*limit, *graph),
        RepoCommands::Show { commit } => run_show(commit.clone()),
        RepoCommands::Diff {
            a,
            b,
            content,
            name_only,
        } => run_diff(a.clone(), b.clone(), *content, *name_only).await,
        RepoCommands::Tag {
            name,
            commit,
            delete,
        } => run_tag(name.clone(), commit.clone(), delete.clone()),
        RepoCommands::Reset {
            commit,
            soft,
            mixed: _,
            hard,
        } => run_reset(commit.clone(), *soft, *hard),
        RepoCommands::Restore {
            paths,
            staged,
            source,
        } => run_restore(paths, *staged, source.clone()),
        RepoCommands::Revert { commit } => run_apply_commit(commit, true).await,
        RepoCommands::CherryPick { commit } => run_apply_commit(commit, false).await,
        RepoCommands::Reflog => run_reflog(),
        RepoCommands::Verify { commit, all } => run_verify(commit.clone(), *all),
        RepoCommands::Gc => run_gc(),
        RepoCommands::Branch { name, delete } => {
            if let Some(d) = delete {
                run_branch_delete(&d)
            } else if let Some(n) = name {
                run_branch_create(&n)
            } else {
                run_branch_list()
            }
        }
        RepoCommands::Checkout {
            branch,
            storage_url,
        } => run_checkout(branch.clone(), storage_url.clone()).await,
        RepoCommands::List { storage_url } => run_list(storage_url.clone()).await,
    }
}
