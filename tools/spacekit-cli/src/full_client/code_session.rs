//! `spacekit agent code` — hybrid "expert Python dev" session.
//!
//! Architecture (plan "Python Dev Session"): Growformer is a router, not a code
//! synthesizer. Growformer + the topic graph ROUTE a prompt to a template id; a
//! deterministic TEMPLATE LIBRARY renders structurally-valid, runnable Python;
//! python3 verifies; spacekit-diff tracks multi-turn changesets; spacekit-repo
//! can commit.
//!
//! Phases implemented here:
//!   1. construct-new + run
//!   2. param extraction (fn/class name) + multi-template compose
//!   3. modify-existing: structural Python resolver (top-level + class method)
//!   4. session memory (SessionVfs + spacekit-diff) + multi-turn REPL
//!   5. execution fix-loop + /commit (spacekit-repo CommitContent)

use std::collections::BTreeMap;
use std::error::Error;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use colored::Colorize;
use serde::Deserialize;

use spacekit_diff::{diff_blobs, diff_trees, DiffHunk, Hash, TreeChange, TreeSnapshot};

// ── Template library (source of truth for construction) ─────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct Param {
    pub name: String,
    #[serde(default)]
    pub default: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Template {
    pub id: String,
    pub kind: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub imports: Vec<String>,
    #[serde(default)]
    pub params: Vec<Param>,
    pub body: String,
    #[serde(default)]
    pub test: String,
}

impl Template {
    fn param(&self, name: &str) -> bool {
        self.params.iter().any(|p| p.name == name)
    }
}

#[derive(Debug, Deserialize)]
struct TemplateFile {
    #[serde(default)]
    template: Vec<Template>,
}

pub struct TemplateLibrary {
    pub templates: Vec<Template>,
}

impl TemplateLibrary {
    pub fn load(dir: &Path) -> Result<Self, Box<dyn Error>> {
        if !dir.is_dir() {
            return Err(format!(
                "templates dir not found: {} (pass --templates DIR)",
                dir.display()
            )
            .into());
        }
        let mut templates = Vec::new();
        let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().map(|e| e == "toml").unwrap_or(false))
            .collect();
        entries.sort();
        for path in entries {
            let raw = std::fs::read_to_string(&path)?;
            let parsed: TemplateFile =
                toml::from_str(&raw).map_err(|e| format!("parse {}: {}", path.display(), e))?;
            templates.extend(parsed.template);
        }
        if templates.is_empty() {
            return Err(format!("no [[template]] entries under {}", dir.display()).into());
        }
        Ok(Self { templates })
    }

    pub fn by_id(&self, id: &str) -> Option<&Template> {
        self.templates.iter().find(|t| t.id == id)
    }

    /// Score templates by keyword hits: (hit_count, longest_matched_keyword_len).
    fn ranked(&self, prompt: &str) -> Vec<(&Template, usize, usize)> {
        let p = prompt.to_ascii_lowercase();
        let mut scored: Vec<(&Template, usize, usize)> = Vec::new();
        for t in &self.templates {
            let mut hits = 0usize;
            let mut longest = 0usize;
            for kw in &t.keywords {
                let k = kw.to_ascii_lowercase();
                if word_match(&p, &k) {
                    hits += 1;
                    longest = longest.max(k.len());
                }
            }
            if hits > 0 {
                scored.push((t, hits, longest));
            }
        }
        scored.sort_by(|a, b| (b.1, b.2).cmp(&(a.1, a.2)));
        scored
    }

    /// Ranked routing: a unified score combining Growformer's topic router
    /// (authoritative, score 1000) with keyword hits. Returns descending order
    /// so callers can surface top-k candidates instead of a silent single pick.
    pub fn route_ranked<'a>(&'a self, graph_path: &Path, prompt: &str) -> Vec<(&'a Template, i64)> {
        if graph_path.is_file() {
            if let Some(s) = graph_path.to_str() {
                let _ = growformer::growformer_lang::init_topic_graph(s);
            }
        }
        let mut out: Vec<(&Template, i64)> = Vec::new();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut pinned_topic: Option<String> = None;
        if let Some(topic) = growformer::growformer_lang::infer_operation_topic(prompt) {
            if let Some(t) = self.by_id(&topic) {
                out.push((t, 1000));
                seen.insert(t.id.clone());
                pinned_topic = Some(t.id.clone());
            }
        }
        for (t, hits, longest) in self.ranked(prompt) {
            if seen.insert(t.id.clone()) {
                out.push((t, (hits as i64) * 10 + longest as i64));
            }
        }
        // Precomputed route table: surface related candidates the live keyword
        // pass missed (capped below the Growformer pin). Consult the pinned
        // topic plus the current top candidate's topic.
        if let Some(table) = load_route_table(graph_path) {
            let mut consult: Vec<String> = Vec::new();
            if let Some(tp) = &pinned_topic {
                consult.push(tp.clone());
            }
            if let Some((t, _)) = out.first() {
                if !consult.contains(&t.id) {
                    consult.push(t.id.clone());
                }
            }
            for topic in consult {
                let Some(entry) = table.routes.iter().find(|e| e.topic == topic) else {
                    continue;
                };
                for hop in &entry.hops {
                    if hop.conditions & COND_PRIMARY != 0 {
                        continue; // primary already present
                    }
                    let Some(t) = self.by_id(&hop.template) else {
                        continue;
                    };
                    let boost = (hop.weight.max(0) as i64).min(200);
                    if let Some(existing) = out.iter_mut().find(|(c, _)| c.id == t.id) {
                        existing.1 += boost / 4; // mild reinforcement of a live hit
                    } else if seen.insert(t.id.clone()) {
                        out.push((t, boost)); // a precomputed route the live pass missed
                    }
                }
            }
        }
        // Learning loop: fold accepted prompt→template phrasings back into routing.
        // Memory lives beside the knowledge graph so it persists across sessions.
        let mem = RoutingMemory::load(&memory_path_for(graph_path));
        if !mem.entries.is_empty() {
            let bonus = mem.bonus(prompt);
            if !bonus.is_empty() {
                for (t, score) in out.iter_mut() {
                    if let Some(b) = bonus.get(&t.id) {
                        // Cap below the topic-graph pin (1000) but above keyword hits.
                        *score += (b * 30.0).round() as i64;
                    }
                }
                out.sort_by(|a, b| b.1.cmp(&a.1));
            }
        }
        // Final ordering (the table-consult step may have appended candidates).
        out.sort_by(|a, b| b.1.cmp(&a.1));
        out
    }

    /// Route a prompt to a single template (top of the ranked list).
    pub fn route<'a>(&'a self, graph_path: &Path, prompt: &str) -> Option<&'a Template> {
        self.route_ranked(graph_path, prompt)
            .first()
            .map(|(t, _)| *t)
    }

    /// Longest keyword of `t` that word-matches the prompt (lowercased).
    fn best_kw(t: &Template, p_lower: &str) -> Option<String> {
        t.keywords
            .iter()
            .map(|k| k.to_ascii_lowercase())
            .filter(|k| word_match(p_lower, k))
            .max_by_key(|k| k.len())
    }

    /// Phase 2 compose: the routed primary plus any additional strongly-matched
    /// templates (multi-word keyword present in the prompt), bounded to 3.
    pub fn route_multi<'a>(&'a self, graph_path: &Path, prompt: &str) -> Vec<&'a Template> {
        let mut chosen: Vec<&Template> = Vec::new();
        if let Some(primary) = self.route(graph_path, prompt) {
            chosen.push(primary);
        }
        let p = prompt.to_ascii_lowercase();
        // The phrase that earned each already-chosen template its spot, so we can
        // reject secondaries whose match overlaps (e.g. "binary search" ⊂
        // "binary search tree" should not compose both).
        let mut chosen_kw: Vec<String> =
            chosen.iter().filter_map(|t| Self::best_kw(t, &p)).collect();
        for (t, _, _) in self.ranked(prompt) {
            if chosen.len() >= 3 {
                break;
            }
            if chosen.iter().any(|c| c.id == t.id) {
                continue;
            }
            // Only compose a secondary when the prompt explicitly names it via a
            // multi-word keyword (reduces false positives).
            let sk = t
                .keywords
                .iter()
                .map(|k| k.to_ascii_lowercase())
                .filter(|k| k.split_whitespace().count() >= 2 && word_match(&p, k))
                .max_by_key(|k| k.len());
            let Some(sk) = sk else { continue };
            // Skip when this match overlaps an already-chosen template's match
            // (one phrase contains the other) — keep the more specific route.
            if chosen_kw
                .iter()
                .any(|ck| ck.contains(&sk) || sk.contains(ck))
            {
                continue;
            }
            chosen.push(t);
            chosen_kw.push(sk);
        }
        chosen
    }
}

// ── Learning loop: routing memory + event log ───────────────────────────────
//
// Every accepted construct/modify reinforces a (distinctive prompt tokens →
// template) association; every /undo penalizes the last one. `route_ranked`
// reads this back as a scoring bonus, so phrasings the user actually accepts
// win next time without a retrain. The raw event stream (events.jsonl) is the
// durable dataset for periodic offline retraining of the brain.

use serde::Serialize;

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct MemoryEntry {
    tokens: Vec<String>,
    template: String,
    weight: f64,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct RoutingMemory {
    #[serde(default)]
    entries: Vec<MemoryEntry>,
}

const MEMORY_STOPWORDS: &[&str] = &[
    "the",
    "and",
    "for",
    "with",
    "that",
    "this",
    "from",
    "into",
    "use",
    "using",
    "make",
    "create",
    "build",
    "add",
    "write",
    "give",
    "want",
    "need",
    "please",
    "code",
    "python",
    "function",
    "class",
    "method",
    "implement",
    "generate",
    "new",
    "file",
    "able",
    "should",
    "your",
    "you",
];

/// Distinctive lowercase tokens for memory keying: drop stopwords and short
/// tokens, dedup, keep order, cap to 6.
fn distinctive_tokens(prompt: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in prompt
        .to_ascii_lowercase()
        .split(|c: char| !c.is_alphanumeric())
    {
        if raw.len() < 3 || MEMORY_STOPWORDS.contains(&raw) {
            continue;
        }
        let tok = raw.to_string();
        if !out.contains(&tok) {
            out.push(tok);
        }
        if out.len() >= 6 {
            break;
        }
    }
    out
}

fn memory_path_for(graph_path: &Path) -> PathBuf {
    graph_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .join("routing_memory.json")
}

impl RoutingMemory {
    fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    fn save(&self, path: &Path) -> std::io::Result<()> {
        let s = serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".into());
        std::fs::write(path, s)
    }

    /// Per-template bonus: an entry contributes its weight when all of its
    /// tokens are present (word-matched) in the prompt.
    fn bonus(&self, prompt: &str) -> BTreeMap<String, f64> {
        let p = prompt.to_ascii_lowercase();
        let mut out: BTreeMap<String, f64> = BTreeMap::new();
        for e in &self.entries {
            if e.weight <= 0.0 || e.tokens.is_empty() {
                continue;
            }
            if e.tokens.iter().all(|t| word_match(&p, t)) {
                *out.entry(e.template.clone()).or_insert(0.0) += e.weight;
            }
        }
        out
    }

    /// Strengthen (delta > 0) or weaken (delta < 0) the association between this
    /// prompt's distinctive tokens and a template. Weights clamp to [0, 10];
    /// entries that hit zero are dropped.
    fn reinforce(&mut self, prompt: &str, template: &str, delta: f64) {
        let tokens = distinctive_tokens(prompt);
        if tokens.is_empty() {
            return;
        }
        if let Some(e) = self
            .entries
            .iter_mut()
            .find(|e| e.template == template && e.tokens == tokens)
        {
            e.weight = (e.weight + delta).clamp(0.0, 10.0);
        } else if delta > 0.0 {
            self.entries.push(MemoryEntry {
                tokens,
                template: template.to_string(),
                weight: delta.clamp(0.0, 10.0),
            });
        }
        self.entries.retain(|e| e.weight > 0.0);
    }
}

/// Reinforce/penalize a routing decision and persist it.
fn record_route(graph_path: &Path, prompt: &str, template: &str, delta: f64) {
    let path = memory_path_for(graph_path);
    let mut mem = RoutingMemory::load(&path);
    mem.reinforce(prompt, template, delta);
    let _ = mem.save(&path);
}

/// Append a durable training/telemetry event for offline retraining.
fn log_event(workdir: &Path, kind: &str, prompt: &str, template: &str, target: &str, ok: bool) {
    let dir = workdir.join(".spacekit-code-session");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let rec = serde_json::json!({
        "ts": ts,
        "kind": kind,
        "prompt": prompt,
        "template": template,
        "target": target,
        "ok": ok,
    });
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("events.jsonl"))
    {
        let _ = writeln!(f, "{}", rec);
    }
}

// ── Precomputed route table (compiled "route options up front") ──────────────
//
// A portable, versioned, integrity-checked descriptor that pre-ranks, for each
// topic, the candidate templates (multi-hop) with weights + condition flags +
// an optional fallback. The schema — not the encoding — is what expands route
// options; we serialize with bincode (compact varints) behind a self-describing
// header and emit a hex sidecar for channels that only carry text.
//
//   wire layout: MAGIC(u32 be) VERSION(u16 be) LEN(u32 be) payload[LEN] SUM[4]
//   SUM = first 4 bytes of blake3(MAGIC..=payload)
//
// `route_ranked` consults the table to surface precomputed related routes the
// live keyword pass might miss, capped below the Growformer pin.

const ROUTE_MAGIC: u32 = 0x534B_5254; // "SKRT"
const ROUTE_VERSION: u16 = 1;

// Documented condition bitflags (extensible; unknown bits ignored by old readers).
const COND_PRIMARY: u32 = 1 << 0; // the topic's own template
const COND_IS_ALGORITHM: u32 = 1 << 1;
const COND_IS_PATTERN: u32 = 1 << 2;
const COND_VERIFIABLE: u32 = 1 << 3; // has an embedded test
const COND_HAS_PARAMS: u32 = 1 << 4;

#[derive(Debug, Clone, bincode::Encode, bincode::Decode)]
struct Hop {
    template: String,
    weight: i32,
    conditions: u32,
}

#[derive(Debug, Clone, bincode::Encode, bincode::Decode)]
struct RouteEntry {
    topic: String,
    hops: Vec<Hop>,
    fallback: Option<String>,
}

#[derive(Debug, Clone, bincode::Encode, bincode::Decode)]
struct RouteTable {
    version: u16,
    routes: Vec<RouteEntry>,
}

fn route_table_path_for(graph_path: &Path) -> PathBuf {
    graph_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .join("route_table.hex")
}

/// Serialize a route table into the self-describing, checksummed wire blob.
fn route_blob_encode(table: &RouteTable) -> Result<Vec<u8>, Box<dyn Error>> {
    let payload = bincode::encode_to_vec(table, bincode::config::standard())?;
    let mut out = Vec::with_capacity(payload.len() + 14);
    out.extend_from_slice(&ROUTE_MAGIC.to_be_bytes());
    out.extend_from_slice(&ROUTE_VERSION.to_be_bytes());
    out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    out.extend_from_slice(&payload);
    let sum = blake3::hash(&out);
    out.extend_from_slice(&sum.as_bytes()[..4]);
    Ok(out)
}

/// Parse + validate a wire blob (magic, version, checksum) into a route table.
fn route_blob_decode(bytes: &[u8]) -> Result<RouteTable, Box<dyn Error>> {
    if bytes.len() < 14 {
        return Err("route table: too short".into());
    }
    let magic = u32::from_be_bytes(bytes[0..4].try_into().unwrap());
    if magic != ROUTE_MAGIC {
        return Err("route table: bad magic".into());
    }
    let version = u16::from_be_bytes(bytes[4..6].try_into().unwrap());
    if version != ROUTE_VERSION {
        return Err(format!("route table: unsupported version {}", version).into());
    }
    let len = u32::from_be_bytes(bytes[6..10].try_into().unwrap()) as usize;
    let end = 10 + len;
    if bytes.len() < end + 4 {
        return Err("route table: truncated".into());
    }
    let sum = blake3::hash(&bytes[..end]);
    if sum.as_bytes()[..4] != bytes[end..end + 4] {
        return Err("route table: checksum mismatch".into());
    }
    let (table, _) = bincode::decode_from_slice(&bytes[10..end], bincode::config::standard())?;
    Ok(table)
}

/// Load + decode the compiled route table sitting beside the knowledge graph.
fn load_route_table(graph_path: &Path) -> Option<RouteTable> {
    let s = std::fs::read_to_string(route_table_path_for(graph_path)).ok()?;
    let bytes = hex::decode(s.trim()).ok()?;
    route_blob_decode(&bytes).ok()
}

fn template_conditions(t: &Template) -> u32 {
    let mut c = 0;
    if t.kind == "algorithm" {
        c |= COND_IS_ALGORITHM;
    }
    if t.kind == "pattern" {
        c |= COND_IS_PATTERN;
    }
    if !t.test.is_empty() {
        c |= COND_VERIFIABLE;
    }
    if !t.params.is_empty() {
        c |= COND_HAS_PARAMS;
    }
    c
}

/// Distinctive keyword *tokens* for a template (words ≥3 chars across all of
/// its keyword phrases), used to relate templates that share vocabulary.
fn keyword_tokens(t: &Template) -> std::collections::HashSet<String> {
    let mut set = std::collections::HashSet::new();
    for kw in &t.keywords {
        for tok in kw
            .to_ascii_lowercase()
            .split(|c: char| !c.is_alphanumeric())
        {
            if tok.len() >= 3 {
                set.insert(tok.to_string());
            }
        }
    }
    set
}

/// Compile a route table: one topic per template, whose hops are the template
/// itself (primary, weight 1000) plus up to 4 related siblings. Relatedness =
/// shared keyword tokens (scored shared*10 + longest token) with a small
/// same-category bonus, so a topic precomputes sensible alternative routes.
fn build_route_table(lib: &TemplateLibrary) -> RouteTable {
    let toks: Vec<std::collections::HashSet<String>> =
        lib.templates.iter().map(keyword_tokens).collect();
    let mut routes = Vec::with_capacity(lib.templates.len());
    for (i, t) in lib.templates.iter().enumerate() {
        let mut hops = vec![Hop {
            template: t.id.clone(),
            weight: 1000,
            conditions: template_conditions(t) | COND_PRIMARY,
        }];
        let mut sec: Vec<(usize, i32)> = Vec::new();
        for (j, ts) in toks.iter().enumerate() {
            if j == i {
                continue;
            }
            let shared: Vec<&String> = ts.intersection(&toks[i]).collect();
            let longest = shared.iter().map(|k| k.len()).max().unwrap_or(0);
            let same_cat = !t.category.is_empty() && t.category == lib.templates[j].category;
            let score = (shared.len() * 10 + longest) as i32 + if same_cat { 5 } else { 0 };
            if score > 0 && (!shared.is_empty() || same_cat) {
                sec.push((j, score));
            }
        }
        sec.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        for (j, w) in sec.into_iter().take(4) {
            let ot = &lib.templates[j];
            hops.push(Hop {
                template: ot.id.clone(),
                weight: w,
                conditions: template_conditions(ot),
            });
        }
        routes.push(RouteEntry {
            topic: t.id.clone(),
            hops,
            fallback: None,
        });
    }
    RouteTable {
        version: ROUTE_VERSION,
        routes,
    }
}

/// Human-readable condition flags for a hop.
fn cond_flags_str(c: u32) -> String {
    let mut v = Vec::new();
    if c & COND_PRIMARY != 0 {
        v.push("primary");
    }
    if c & COND_IS_ALGORITHM != 0 {
        v.push("algo");
    }
    if c & COND_IS_PATTERN != 0 {
        v.push("pattern");
    }
    if c & COND_VERIFIABLE != 0 {
        v.push("test");
    }
    if c & COND_HAS_PARAMS != 0 {
        v.push("params");
    }
    v.join("|")
}

/// Pretty-print a decoded route table (for `route-compile --verify`).
fn print_route_table(table: &RouteTable) {
    for entry in &table.routes {
        println!("{}", entry.topic.cyan().bold());
        for hop in &entry.hops {
            println!(
                "  {:>5}  {:<28} {}",
                hop.weight,
                hop.template,
                cond_flags_str(hop.conditions).dimmed()
            );
        }
        if let Some(fb) = &entry.fallback {
            println!("  {} {}", "fallback ->".dimmed(), fb);
        }
    }
}

/// Write the compiled route table beside the graph if it's missing, so the
/// first `agent code` invocation gets the benefit without a manual step.
fn ensure_route_table(lib: &TemplateLibrary, graph: &Path) {
    let p = route_table_path_for(graph);
    if p.exists() {
        return;
    }
    if let Ok(bytes) = route_blob_encode(&build_route_table(lib)) {
        if let Some(parent) = p.parent() {
            if !parent.as_os_str().is_empty() {
                let _ = std::fs::create_dir_all(parent);
            }
        }
        if std::fs::write(&p, hex::encode(&bytes)).is_ok() {
            eprintln!("{} route table -> {}", "bootstrapped".dimmed(), p.display());
        }
    }
}

/// Lint gate: render every template with its defaults and verify it (embedded
/// test + static checks). Returns the count of templates with failing tests so
/// callers can treat it as a CI gate. Reports per-template issues to stdout.
fn lint_templates(lib: &TemplateLibrary, workdir: &Path) -> usize {
    let mut failed = 0usize;
    let mut linty = 0usize;
    let mut unverifiable = 0usize;
    for t in &lib.templates {
        let values = default_values(t);
        let code = instantiate(t, &values);
        let test = render(&t.test, &values);
        let rep = verify_and_repair(workdir, &t.id, &code, &test);
        let mut notes: Vec<String> = Vec::new();
        if !rep.verifiable {
            unverifiable += 1;
            notes.push("python3 unavailable".to_string());
        } else if !rep.test_ok {
            failed += 1;
            let line = rep
                .stderr
                .lines()
                .rev()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("");
            notes.push(format!("TEST FAILED: {}", line.trim()));
        }
        if rep.lint_issues > 0 {
            linty += 1;
            notes.push(format!("{} lint issue(s)", rep.lint_issues));
        }
        if !notes.is_empty() {
            let tag = if rep.verifiable && !rep.test_ok {
                "FAIL".red().bold()
            } else {
                "warn".yellow().bold()
            };
            println!("  {} {:<32} {}", tag, t.id, notes.join("; ").dimmed());
        }
    }
    println!(
        "{} {} templates | {} test-fail, {} lint-warn, {} unverifiable",
        "template-lint ->".green().bold(),
        lib.templates.len(),
        failed,
        linty,
        unverifiable
    );
    failed
}

pub struct RouteCompileArgs {
    pub templates: PathBuf,
    pub graph: PathBuf,
    pub out: Option<PathBuf>,
    pub verify: bool,
    pub lint: bool,
}

pub fn handle_route_compile(args: &RouteCompileArgs) -> Result<(), Box<dyn Error>> {
    let lib = TemplateLibrary::load(&args.templates)?;

    if args.lint {
        let workdir = std::env::temp_dir().join("sk_template_lint");
        let failed = lint_templates(&lib, &workdir);
        if failed > 0 {
            return Err(format!("{} template(s) failed their embedded test", failed).into());
        }
    }

    let table = build_route_table(&lib);
    let bytes = route_blob_encode(&table)?;
    let hexs = hex::encode(&bytes);
    let out = args
        .out
        .clone()
        .unwrap_or_else(|| route_table_path_for(&args.graph));
    if let Some(parent) = out.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    std::fs::write(&out, &hexs)?;
    let hops: usize = table.routes.iter().map(|r| r.hops.len()).sum();
    // Round-trip to prove the artifact is readable.
    let decoded = route_blob_decode(&bytes)?;
    println!(
        "{} {} topics, {} hops, v{} | {} bytes ({} hex chars) -> {}",
        "route-compile ->".green().bold(),
        table.routes.len(),
        hops,
        ROUTE_VERSION,
        bytes.len(),
        hexs.len(),
        out.display()
    );
    if args.verify {
        println!("{}", "--- route table ---".dimmed());
        print_route_table(&decoded);
    }
    Ok(())
}

// ── Rendering + param extraction ────────────────────────────────────────────

/// Substitute `${name}` placeholders (string.Template style; never collides with
/// Python's own `{}`/f-strings).
pub fn render(text: &str, values: &BTreeMap<String, String>) -> String {
    let mut out = text.to_string();
    for (k, v) in values {
        out = out.replace(&format!("${{{}}}", k), v);
    }
    out
}

pub fn default_values(t: &Template) -> BTreeMap<String, String> {
    t.params
        .iter()
        .map(|p| (p.name.clone(), p.default.clone()))
        .collect()
}

fn sanitize_ident(s: &str) -> Option<String> {
    let s = s.trim_matches(|c: char| !c.is_alphanumeric() && c != '_');
    if s.is_empty() {
        return None;
    }
    let mut chars = s.chars();
    let first = chars.next().unwrap();
    if !(first.is_alphabetic() || first == '_') {
        return None;
    }
    if !s.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return None;
    }
    // avoid python keywords as names
    const KW: &[&str] = &[
        "class", "def", "for", "if", "in", "is", "and", "or", "not", "the", "a",
    ];
    if KW.contains(&s) {
        return None;
    }
    Some(s.to_string())
}

/// Extract an identifier following any cue word (e.g. "called foo", "named bar").
fn ident_after(prompt: &str, cues: &[&str]) -> Option<String> {
    let tokens: Vec<&str> = prompt.split_whitespace().collect();
    for (i, tok) in tokens.iter().enumerate() {
        let lower = tok.to_ascii_lowercase();
        if cues.contains(&lower.as_str()) {
            if let Some(next) = tokens.get(i + 1) {
                if let Some(id) = sanitize_ident(next) {
                    return Some(id);
                }
            }
        }
    }
    None
}

/// Phase 2: override fn_name / class_name from the prompt when present.
pub fn extract_values(t: &Template, prompt: &str) -> BTreeMap<String, String> {
    let mut values = default_values(t);
    if let Some(name) = ident_after(prompt, &["called", "named"]) {
        if t.param("fn_name") {
            values.insert("fn_name".into(), name.clone());
        }
        if t.param("class_name") {
            values.insert("class_name".into(), name);
        }
    }
    if t.param("fn_name") {
        if let Some(name) = ident_after(prompt, &["function", "func", "def"]) {
            values.insert("fn_name".into(), name);
        }
    }
    if t.param("class_name") {
        if let Some(name) = ident_after(prompt, &["class"]) {
            values.insert("class_name".into(), name);
        }
    }
    values
}

fn body_only(t: &Template, values: &BTreeMap<String, String>) -> String {
    render(t.body.trim_matches('\n'), values)
        .trim_end()
        .to_string()
}

/// Render imports + body into a standalone, importable module (no test).
pub fn instantiate(t: &Template, values: &BTreeMap<String, String>) -> String {
    let mut out = String::new();
    if !t.imports.is_empty() {
        out.push_str(&t.imports.join("\n"));
        out.push_str("\n\n\n");
    }
    out.push_str(&body_only(t, values));
    out.push('\n');
    out
}

/// Compose multiple templates into one module: merged (deduped) imports + bodies.
pub fn compose(parts: &[(&Template, BTreeMap<String, String>)]) -> String {
    let mut imports: Vec<String> = Vec::new();
    for (t, _) in parts {
        for imp in &t.imports {
            if !imports.contains(imp) {
                imports.push(imp.clone());
            }
        }
    }
    let mut out = String::new();
    if !imports.is_empty() {
        out.push_str(&imports.join("\n"));
        out.push_str("\n\n\n");
    }
    let bodies: Vec<String> = parts.iter().map(|(t, v)| body_only(t, v)).collect();
    out.push_str(&bodies.join("\n\n\n"));
    out.push('\n');
    out
}

fn artifact_name(id: &str) -> String {
    let stem = id
        .strip_prefix("algo_")
        .or_else(|| id.strip_prefix("pattern_"))
        .unwrap_or(id);
    format!("{}.py", stem)
}

// ── Structural Python resolver (modify-existing) ────────────────────────────
//
// A focused "module/class-graph sub-variant" (the user-sanctioned alternative to
// project-graph, which cannot resolve Python methods). Indentation-based, which
// is correct for well-formatted Python.

fn line_indent(line: &str) -> usize {
    line.chars().take_while(|c| *c == ' ').count()
}

/// Substring match constrained to word boundaries: `"search"` matches in
/// `"binary search"` but not in `"research"`. `_` counts as a word char.
fn word_match(hay: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    let hb = hay.as_bytes();
    let mut start = 0;
    while let Some(pos) = hay[start..].find(needle) {
        let i = start + pos;
        let before_ok = i == 0 || {
            let b = hb[i - 1];
            !(b.is_ascii_alphanumeric() || b == b'_')
        };
        let end = i + needle.len();
        let after_ok = end >= hb.len() || {
            let b = hb[end];
            !(b.is_ascii_alphanumeric() || b == b'_')
        };
        if before_ok && after_ok {
            return true;
        }
        start = i + 1;
        if start > hay.len() {
            break;
        }
    }
    false
}

/// Merge new import lines into a module after the existing top-level import block.
fn merge_imports(source: &str, imports: &[String]) -> String {
    if imports.is_empty() {
        return source.to_string();
    }
    let lines: Vec<&str> = source.lines().collect();
    let existing: std::collections::HashSet<&str> = lines
        .iter()
        .copied()
        .filter(|l| l.starts_with("import ") || l.starts_with("from "))
        .collect();
    let to_add: Vec<String> = imports
        .iter()
        .filter(|i| !existing.contains(i.as_str()))
        .cloned()
        .collect();
    if to_add.is_empty() {
        return source.to_string();
    }
    // Insert after the last top-level import in the first contiguous header block.
    let mut insert_at = 0usize;
    for (i, l) in lines.iter().enumerate() {
        if l.starts_with("import ") || l.starts_with("from ") {
            insert_at = i + 1;
        } else if l.trim().is_empty() || l.starts_with('#') {
            if insert_at == 0 {
                continue;
            }
        } else if insert_at > 0 {
            break;
        } else {
            break;
        }
    }
    let mut out: Vec<String> = Vec::new();
    out.extend(lines[..insert_at].iter().map(|s| s.to_string()));
    out.extend(to_add);
    out.extend(lines[insert_at..].iter().map(|s| s.to_string()));
    out.join("\n") + "\n"
}

/// Append a top-level definition to the end of a module.
fn insert_top_level(source: &str, code_body: &str) -> String {
    let trimmed = source.trim_end_matches('\n');
    let sep = if trimmed.is_empty() { "" } else { "\n\n\n" };
    format!("{}{}{}\n", trimmed, sep, code_body.trim_end())
}

/// Detect a top-level `def name`/`class name` already present.
fn has_top_level_def(source: &str, name: &str) -> bool {
    source.lines().any(|l| {
        line_indent(l) == 0
            && (l.starts_with(&format!("def {}(", name))
                || l.starts_with(&format!("class {}(", name))
                || l.starts_with(&format!("class {}:", name)))
    })
}

/// Adapt a rendered top-level function into a class method: ensure `self` is the
/// first parameter and indent the whole def by `indent` spaces.
fn function_to_method(func_code: &str, indent: usize) -> Result<String, Box<dyn Error>> {
    let lines: Vec<&str> = func_code.lines().collect();
    let def_idx = lines
        .iter()
        .position(|l| l.trim_start().starts_with("def "))
        .ok_or("template body is not a function; cannot convert to a method")?;
    let mut adapted: Vec<String> = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        let mut line = l.to_string();
        if i == def_idx {
            // Insert self as the first parameter.
            if let Some(open) = line.find('(') {
                let after = &line[open + 1..];
                let inner = after.trim_start();
                if inner.starts_with("self") {
                    // already a method
                } else if inner.starts_with(')') {
                    line = format!("{}(self{}", &line[..open], &line[open + 1..]);
                } else {
                    line = format!("{}(self, {}", &line[..open], after);
                }
            }
        }
        let pad = " ".repeat(indent);
        if line.trim().is_empty() {
            adapted.push(String::new());
        } else {
            adapted.push(format!("{}{}", pad, line));
        }
    }
    Ok(adapted.join("\n"))
}

/// Insert a method into `class_name`, at the end of its body (after the last
/// member). Returns the new source or an error if the class is not found.
fn insert_method(
    source: &str,
    class_name: &str,
    func_code: &str,
) -> Result<String, Box<dyn Error>> {
    let lines: Vec<String> = source.lines().map(|s| s.to_string()).collect();
    let header_idx = lines
        .iter()
        .position(|l| {
            line_indent(l) == 0
                && (l.starts_with(&format!("class {}(", class_name))
                    || l.starts_with(&format!("class {}:", class_name)))
        })
        .ok_or_else(|| format!("class {} not found in target file", class_name))?;
    let class_indent = line_indent(&lines[header_idx]);

    // Body indent: first non-blank line after the header that is more indented.
    let mut body_indent = class_indent + 4;
    for l in &lines[header_idx + 1..] {
        if l.trim().is_empty() {
            continue;
        }
        if line_indent(l) > class_indent {
            body_indent = line_indent(l);
        }
        break;
    }

    // Class block end: first subsequent non-blank, non-comment line at indent
    // <= class_indent; else EOF.
    let mut end_idx = lines.len();
    for (off, l) in lines[header_idx + 1..].iter().enumerate() {
        if l.trim().is_empty() || l.trim_start().starts_with('#') {
            continue;
        }
        if line_indent(l) <= class_indent {
            end_idx = header_idx + 1 + off;
            break;
        }
    }
    // Trim trailing blank lines that belong inside the class block.
    let mut insert_at = end_idx;
    while insert_at > header_idx + 1 && lines[insert_at - 1].trim().is_empty() {
        insert_at -= 1;
    }

    let method = function_to_method(func_code, body_indent)?;
    let mut out: Vec<String> = Vec::new();
    out.extend(lines[..insert_at].iter().cloned());
    out.push(String::new()); // blank line before the new method
    out.extend(method.lines().map(|s| s.to_string()));
    out.extend(lines[insert_at..].iter().cloned());
    Ok(out.join("\n").trim_end_matches('\n').to_string() + "\n")
}

// ── Edit-operation IR + structural application ──────────────────────────────
//
// Generalization: modify-existing is no longer "splice a template body". A
// prompt is lowered to a typed list of `EditOp`s, which a structural backend
// (Python `ast` locator → line-anchored splice, formatting preserved) applies.
// If python3/ast is unavailable, a zero-dependency textual fallback applies the
// same ops. New edit kinds (params, decorators, statements) slot in here without
// touching routing or the REPL.

#[derive(Debug, Clone)]
enum EditOp {
    /// Ensure `import module` or `from module import symbols…` is present
    /// (symbol-level merge into an existing `from` import when possible).
    AddImport {
        module: String,
        symbols: Vec<String>,
        raw: String,
    },
    /// Append a top-level function/class definition to the module.
    InsertDef { name: String, code: String },
    /// Insert a method (function adapted with `self`) into a class body.
    InsertMethod {
        class: String,
        name: String,
        code: String,
    },
    /// Add a decorator above an existing function/method (idempotent).
    AddDecorator {
        class: Option<String>,
        target: String,
        decorator: String,
    },
    /// Add a parameter to an existing function/method signature (idempotent).
    AddParam {
        class: Option<String>,
        target: String,
        param: String,
    },
    /// Append statement(s) to the end of an existing function/method body.
    AppendToBody {
        class: Option<String>,
        target: String,
        code: String,
    },
}

/// Parse a template import line into an `AddImport` op.
fn parse_import_line(line: &str) -> Option<EditOp> {
    let l = line.trim();
    if let Some(rest) = l.strip_prefix("from ") {
        let (m, syms) = rest.split_once(" import ")?;
        let symbols: Vec<String> = syms
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        Some(EditOp::AddImport {
            module: m.trim().to_string(),
            symbols,
            raw: l.to_string(),
        })
    } else if let Some(rest) = l.strip_prefix("import ") {
        Some(EditOp::AddImport {
            module: rest.trim().to_string(),
            symbols: Vec::new(),
            raw: l.to_string(),
        })
    } else {
        None
    }
}

/// First `def`/`class` name declared in a rendered snippet.
fn def_name(code: &str) -> String {
    for l in code.lines() {
        let t = l.trim_start();
        let t = t.strip_prefix("async ").unwrap_or(t);
        for kw in ["def ", "class "] {
            if let Some(rest) = t.strip_prefix(kw) {
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                if !name.is_empty() {
                    return name;
                }
            }
        }
    }
    "definition".to_string()
}

/// Lower a routed template + values into edit operations.
fn plan_edits(
    t: &Template,
    values: &BTreeMap<String, String>,
    target_class: Option<String>,
) -> Vec<EditOp> {
    let mut ops: Vec<EditOp> = Vec::new();
    for imp in &t.imports {
        if let Some(op) = parse_import_line(imp) {
            ops.push(op);
        }
    }
    let code = body_only(t, values);
    let name = def_name(&code);
    match target_class {
        Some(class) => ops.push(EditOp::InsertMethod { class, name, code }),
        None => ops.push(EditOp::InsertDef { name, code }),
    }
    ops
}

// Structural model produced by the embedded `ast` locator (line-anchored).

#[derive(Debug, Deserialize)]
struct FromImportInfo {
    line: usize,
    end_line: usize,
    symbols: Vec<String>,
}

/// Per-function/method structural detail used by the micro-edit ops
/// (decorator/param/body insertion). All line numbers are 1-based.
#[derive(Debug, Default, Clone, Deserialize)]
struct DefInfo {
    def_line: usize,
    decorator_line: usize,
    #[allow(dead_code)]
    body_start_line: usize,
    body_end_line: usize,
    body_indent: String,
    args: Vec<String>,
    decorators: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ClassInfo {
    #[allow(dead_code)]
    header_line: usize,
    body_indent: String,
    last_member_end_line: usize,
    methods: Vec<String>,
    #[serde(default)]
    methods_info: BTreeMap<String, DefInfo>,
}

#[derive(Debug, Deserialize)]
struct StructModel {
    #[serde(default)]
    ok: bool,
    #[serde(default)]
    import_insert_line: usize,
    #[serde(default)]
    module_body_end_line: usize,
    #[serde(default)]
    functions: Vec<String>,
    #[serde(default)]
    plain_imports: Vec<String>,
    #[serde(default)]
    from_imports: BTreeMap<String, FromImportInfo>,
    #[serde(default)]
    classes: BTreeMap<String, ClassInfo>,
    #[serde(default)]
    functions_info: BTreeMap<String, DefInfo>,
}

impl StructModel {
    /// Locate a function or method by name. With an explicit `class`, only that
    /// class is searched; otherwise top-level functions first, then any class
    /// method with the name. Returns (DefInfo, owning class name if a method).
    fn resolve_def(&self, class: Option<&str>, target: &str) -> Option<(&DefInfo, Option<String>)> {
        if let Some(c) = class {
            return self
                .classes
                .get(c)
                .and_then(|ci| ci.methods_info.get(target))
                .map(|d| (d, Some(c.to_string())));
        }
        if let Some(d) = self.functions_info.get(target) {
            return Some((d, None));
        }
        for (cname, ci) in &self.classes {
            if let Some(d) = ci.methods_info.get(target) {
                return Some((d, Some(cname.clone())));
            }
        }
        None
    }
}

const AST_LOCATOR_PY: &str = r#"
import sys, json, ast

def definfo(node):
    decos = []
    for d in node.decorator_list:
        try:
            decos.append(ast.unparse(d))
        except Exception:
            decos.append(getattr(d, "id", ""))
    deco_line = node.decorator_list[0].lineno if node.decorator_list else node.lineno
    a = node.args
    args = [x.arg for x in (list(getattr(a, "posonlyargs", [])) + list(a.args))]
    if a.vararg: args.append(a.vararg.arg)
    args += [x.arg for x in a.kwonlyargs]
    if a.kwarg: args.append(a.kwarg.arg)
    body = node.body
    body_start = body[0].lineno if body else node.lineno
    body_end = max(getattr(s, "end_lineno", s.lineno) for s in body) if body else node.lineno
    indent = " " * (body[0].col_offset if body else node.col_offset + 4)
    return {
        "def_line": node.lineno,
        "decorator_line": deco_line,
        "body_start_line": body_start,
        "body_end_line": body_end,
        "body_indent": indent,
        "args": args,
        "decorators": decos,
    }

def main():
    data = json.loads(sys.stdin.read())
    src = data["source"]
    try:
        tree = ast.parse(src)
    except SyntaxError as e:
        print(json.dumps({"ok": False, "error": "syntax: %s" % e}))
        return
    lines = src.splitlines()
    out = {
        "ok": True, "error": "",
        "import_insert_line": 0,
        "module_body_end_line": len(lines),
        "functions": [], "plain_imports": [],
        "from_imports": {}, "classes": {},
        "functions_info": {},
    }
    body = tree.body
    if body:
        out["module_body_end_line"] = max(getattr(s, "end_lineno", s.lineno) for s in body)
    last_import_end = 0
    for node in body:
        if isinstance(node, ast.Import):
            last_import_end = max(last_import_end, node.end_lineno)
            for a in node.names:
                out["plain_imports"].append(a.name)
        elif isinstance(node, ast.ImportFrom):
            last_import_end = max(last_import_end, node.end_lineno)
            mod = ("." * (node.level or 0)) + (node.module or "")
            out["from_imports"][mod] = {
                "line": node.lineno,
                "end_line": node.end_lineno,
                "symbols": [a.name for a in node.names],
            }
        elif isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
            out["functions"].append(node.name)
            out["functions_info"][node.name] = definfo(node)
        elif isinstance(node, ast.ClassDef):
            members = node.body
            indent = "    "
            if members:
                indent = " " * members[0].col_offset
            last_member_end = node.lineno
            methods = []
            methods_info = {}
            if members:
                last_member_end = max(getattr(m, "end_lineno", m.lineno) for m in members)
                for m in members:
                    if isinstance(m, (ast.FunctionDef, ast.AsyncFunctionDef)):
                        methods.append(m.name)
                        methods_info[m.name] = definfo(m)
            out["classes"][node.name] = {
                "header_line": node.lineno,
                "body_indent": indent,
                "last_member_end_line": last_member_end,
                "methods": methods,
                "methods_info": methods_info,
            }
    if last_import_end == 0 and body:
        first = body[0]
        if isinstance(first, ast.Expr) and isinstance(getattr(first, "value", None), ast.Constant) and isinstance(first.value.value, str):
            out["import_insert_line"] = first.end_lineno
    else:
        out["import_insert_line"] = last_import_end
    print(json.dumps(out))

main()
"#;

/// Build a structural model of `source` using the embedded `ast` locator.
/// Returns `None` if python3/ast is unavailable or the file fails to parse
/// (callers then fall back to the textual resolver).
fn analyze(source: &str) -> Option<StructModel> {
    let helper = std::env::temp_dir().join(format!(".sk_ast_locator_{}.py", std::process::id()));
    std::fs::write(&helper, AST_LOCATOR_PY).ok()?;
    let input = serde_json::json!({ "source": source }).to_string();
    let mut child = std::process::Command::new("python3")
        .arg(&helper)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .ok()?;
    {
        use std::io::Write as _;
        child.stdin.take()?.write_all(input.as_bytes()).ok()?;
    }
    let output = child.wait_with_output().ok()?;
    let _ = std::fs::remove_file(&helper);
    if !output.status.success() {
        return None;
    }
    let model: StructModel = serde_json::from_slice(&output.stdout).ok()?;
    if !model.ok {
        return None;
    }
    Some(model)
}

/// Apply edit ops, preferring the structural (ast-anchored) backend.
fn apply_edits(source: &str, ops: &[EditOp]) -> Result<String, Box<dyn Error>> {
    match analyze(source) {
        Some(model) => apply_structural(source, &model, ops),
        None => apply_textual(source, ops),
    }
}

/// Structural application: every op is resolved to line-anchored inserts/replaces
/// against the *original* line numbers, then reconstructed in one pass so anchors
/// never need recomputation. The untouched parts of the file are preserved byte
/// for byte.
fn apply_structural(
    source: &str,
    model: &StructModel,
    ops: &[EditOp],
) -> Result<String, Box<dyn Error>> {
    let lines: Vec<String> = source.lines().map(|s| s.to_string()).collect();
    // anchor (1-based line to insert AFTER; 0 == top) -> payload lines
    let mut inserts: BTreeMap<usize, Vec<String>> = BTreeMap::new();
    let mut replaces: BTreeMap<usize, String> = BTreeMap::new();

    for op in ops {
        match op {
            EditOp::AddImport {
                module,
                symbols,
                raw,
            } => {
                if symbols.is_empty() {
                    if model.plain_imports.iter().any(|m| m == module) {
                        continue;
                    }
                    inserts
                        .entry(model.import_insert_line)
                        .or_default()
                        .push(format!("import {}", module));
                } else if let Some(fi) = model.from_imports.get(module) {
                    let has_missing = symbols.iter().any(|s| !fi.symbols.contains(s));
                    if !has_missing {
                        continue;
                    }
                    if fi.line == fi.end_line {
                        // Symbol-level merge into the existing single-line import.
                        let mut all = fi.symbols.clone();
                        for s in symbols {
                            if !all.contains(s) {
                                all.push(s.clone());
                            }
                        }
                        replaces.insert(
                            fi.line,
                            format!("from {} import {}", module, all.join(", ")),
                        );
                    } else {
                        inserts
                            .entry(model.import_insert_line)
                            .or_default()
                            .push(raw.clone());
                    }
                } else {
                    inserts
                        .entry(model.import_insert_line)
                        .or_default()
                        .push(raw.clone());
                }
            }
            EditOp::InsertDef { name, code } => {
                if model.functions.iter().any(|f| f == name) || model.classes.contains_key(name) {
                    return Err(format!(
                        "`{}` already exists at top level; pass a different name (e.g. \"... called my_{}\")",
                        name, name
                    )
                    .into());
                }
                let mut payload = vec![String::new(), String::new()];
                payload.extend(code.lines().map(|s| s.to_string()));
                inserts
                    .entry(model.module_body_end_line)
                    .or_default()
                    .extend(payload);
            }
            EditOp::InsertMethod { class, name, code } => {
                let ci = model
                    .classes
                    .get(class)
                    .ok_or_else(|| format!("class {} not found in target file", class))?;
                if ci.methods.iter().any(|m| m == name) {
                    return Err(format!(
                        "method `{}` already exists on class {}; pass a different name",
                        name, class
                    )
                    .into());
                }
                let method = function_to_method(code, ci.body_indent.chars().count())?;
                let mut payload = vec![String::new()];
                payload.extend(method.lines().map(|s| s.to_string()));
                inserts
                    .entry(ci.last_member_end_line)
                    .or_default()
                    .extend(payload);
            }
            EditOp::AddDecorator {
                class,
                target,
                decorator,
            } => {
                let (di, owner) = model
                    .resolve_def(class.as_deref(), target)
                    .ok_or_else(|| not_found_msg("function/method", target, class))?;
                let deco = normalize_decorator(decorator);
                let key = decorator_key(&deco);
                if di.decorators.iter().any(|d| decorator_key(d) == key) {
                    continue; // already decorated
                }
                let indent: String = lines
                    .get(di.def_line.saturating_sub(1))
                    .map(|l| l.chars().take_while(|c| *c == ' ').collect())
                    .unwrap_or_default();
                let _ = owner;
                inserts
                    .entry(di.decorator_line.saturating_sub(1))
                    .or_default()
                    .push(format!("{}{}", indent, deco));
            }
            EditOp::AddParam {
                class,
                target,
                param,
            } => {
                let (di, _owner) = model
                    .resolve_def(class.as_deref(), target)
                    .ok_or_else(|| not_found_msg("function/method", target, class))?;
                let pname = param_ident(param);
                if di.args.iter().any(|a| *a == pname) {
                    continue; // already a parameter
                }
                let (idx0, col) = find_signature_close(&lines, di.def_line).ok_or_else(|| {
                    format!(
                        "could not locate the signature of `{}` to add a parameter",
                        target
                    )
                })?;
                let line = &lines[idx0];
                let (before, after) = line.split_at(col);
                let sep = if di.args.is_empty() { "" } else { ", " };
                replaces.insert(
                    idx0 + 1,
                    format!("{}{}{}{}", before, sep, param.trim(), after),
                );
            }
            EditOp::AppendToBody {
                class,
                target,
                code,
            } => {
                let (di, _owner) = model
                    .resolve_def(class.as_deref(), target)
                    .ok_or_else(|| not_found_msg("function/method", target, class))?;
                let payload = reindent(code, &di.body_indent);
                if payload.is_empty() {
                    continue;
                }
                inserts.entry(di.body_end_line).or_default().extend(payload);
            }
        }
    }

    // Replaces only ever target single-line import statements.
    let mut patched = lines;
    for (ln, text) in &replaces {
        if *ln >= 1 && *ln <= patched.len() {
            patched[ln - 1] = text.clone();
        }
    }

    let mut out: Vec<String> = Vec::new();
    if let Some(v) = inserts.get(&0) {
        out.extend(v.clone());
    }
    for (i, line) in patched.iter().enumerate() {
        out.push(line.clone());
        if let Some(v) = inserts.get(&(i + 1)) {
            out.extend(v.clone());
        }
    }
    Ok(out.join("\n").trim_end_matches('\n').to_string() + "\n")
}

/// Zero-dependency fallback used when the ast locator is unavailable.
fn apply_textual(source: &str, ops: &[EditOp]) -> Result<String, Box<dyn Error>> {
    let import_lines: Vec<String> = ops
        .iter()
        .filter_map(|op| match op {
            EditOp::AddImport { raw, .. } => Some(raw.clone()),
            _ => None,
        })
        .collect();
    let mut out = merge_imports(source, &import_lines);
    for op in ops {
        match op {
            EditOp::AddImport { .. } => {}
            EditOp::InsertDef { name, code } => {
                if has_top_level_def(&out, name) {
                    return Err(format!(
                        "`{}` already exists at top level; pass a different name",
                        name
                    )
                    .into());
                }
                out = insert_top_level(&out, code);
            }
            EditOp::InsertMethod { class, code, .. } => {
                out = insert_method(&out, class, code)?;
            }
            // Micro-edits need precise line/signature anchoring; without the ast
            // locator we can't place them safely, so fail loudly instead of
            // guessing.
            EditOp::AddDecorator { .. } | EditOp::AddParam { .. } | EditOp::AppendToBody { .. } => {
                return Err(
                    "structural micro-edits (decorator/param/body) require python3 with the ast module"
                        .into(),
                );
            }
        }
    }
    Ok(out)
}

// ── Micro-edit helpers (decorator / param / body) ───────────────────────────

fn not_found_msg(kind: &str, target: &str, class: &Option<String>) -> String {
    match class {
        Some(c) => format!("{} `{}` not found on class `{}`", kind, target, c),
        None => format!("{} `{}` not found in target file", kind, target),
    }
}

/// Ensure a decorator string is written as `@...`.
fn normalize_decorator(decorator: &str) -> String {
    let d = decorator.trim();
    if d.starts_with('@') {
        d.to_string()
    } else {
        format!("@{}", d)
    }
}

/// Dotted name of a decorator, ignoring `@` and any call args, for idempotency
/// comparisons (`@app.route("/x")` and `@app.route` share the key `app.route`).
fn decorator_key(decorator: &str) -> String {
    decorator
        .trim()
        .trim_start_matches('@')
        .split('(')
        .next()
        .unwrap_or("")
        .trim()
        .to_string()
}

/// Leading identifier of a parameter spec (`timeout: int = 5` → `timeout`).
fn param_ident(param: &str) -> String {
    param
        .trim()
        .trim_start_matches('*')
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect()
}

/// Locate the `)` that closes a `def`'s argument list, returning its
/// (0-based line index, byte column within that line). Tracks paren depth from
/// the first `(` after the `def` keyword.
fn find_signature_close(lines: &[String], def_line: usize) -> Option<(usize, usize)> {
    let mut depth = 0i32;
    let mut started = false;
    for (i, line) in lines.iter().enumerate().skip(def_line.saturating_sub(1)) {
        for (col, ch) in line.char_indices() {
            match ch {
                '(' => {
                    depth += 1;
                    started = true;
                }
                ')' => {
                    depth -= 1;
                    if started && depth == 0 {
                        return Some((i, col));
                    }
                }
                _ => {}
            }
        }
    }
    None
}

/// Re-indent a code block to `indent`: strip the block's own common leading
/// whitespace, then prefix each non-blank line with `indent`.
fn reindent(code: &str, indent: &str) -> Vec<String> {
    let raw: Vec<&str> = code.lines().collect();
    let common = raw
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.chars().take_while(|c| *c == ' ').count())
        .min()
        .unwrap_or(0);
    raw.iter()
        .map(|l| {
            if l.trim().is_empty() {
                String::new()
            } else {
                let stripped: String = l.chars().skip(common).collect();
                format!("{}{}", indent, stripped)
            }
        })
        .collect()
}

// ── Micro-edit prompt planner ───────────────────────────────────────────────
//
// Some prompts aren't "insert an algorithm" — they tweak an existing def
// ("decorate fetch with lru_cache", "add a timeout parameter to fetch",
// "append `return x` to compute"). These lower directly to micro-edit ops
// against a resolved target, bypassing template routing. Every grammar is
// gated on the named function/method actually existing in the model, so an
// ordinary "add a method" prompt never gets hijacked.

fn substr_after_ci(prompt: &str, marker: &str) -> Option<String> {
    let lp = prompt.to_ascii_lowercase();
    let idx = lp.find(marker)?;
    let tail = prompt[idx + marker.len()..].trim();
    if tail.is_empty() {
        None
    } else {
        Some(tail.to_string())
    }
}

/// First decorator-looking token of a tail: a single word, or a `name(...)`
/// call (kept whole so args survive).
fn first_decorator_token(tail: &str) -> Option<String> {
    let tail = tail.trim().trim_end_matches(['.', ',']);
    if tail.is_empty() {
        return None;
    }
    if let Some(p) = tail.find('(') {
        if !tail[..p].contains(' ') {
            return Some(tail.to_string());
        }
    }
    tail.split_whitespace().next().map(|s| s.to_string())
}

fn word_before(prompt: &str, word: &str) -> Option<String> {
    let tokens: Vec<&str> = prompt.split_whitespace().collect();
    for (i, t) in tokens.iter().enumerate() {
        if t.to_ascii_lowercase().trim_end_matches([',', '.']) == word && i > 0 {
            return sanitize_ident(tokens[i - 1]);
        }
    }
    None
}

/// Parameter spec near a cue word, handling both orders ("add parameter
/// timeout …" and "add a timeout parameter …"). Skips articles and rejects
/// prepositions; preserves any `=default` / `: type`.
fn param_spec_after(prompt: &str, cues: &[&str]) -> Option<String> {
    const PREPS: &[&str] = &[
        "to", "of", "on", "for", "into", "from", "with", "in", "at", "by", "as",
    ];
    const ARTS: &[&str] = &["a", "an", "the", "named", "called", "new"];
    let tokens: Vec<&str> = prompt.split_whitespace().collect();
    let clean = |s: &str| s.trim_end_matches([',', '.', ';']).to_string();
    let usable = |s: &str| -> bool {
        !param_ident(s).is_empty() && !PREPS.contains(&s.to_ascii_lowercase().as_str())
    };
    for (i, t) in tokens.iter().enumerate() {
        if !cues.contains(&t.to_ascii_lowercase().as_str()) {
            continue;
        }
        // Look after the cue (skipping articles): "add parameter timeout …".
        let mut j = i + 1;
        while tokens
            .get(j)
            .map_or(false, |n| ARTS.contains(&n.to_ascii_lowercase().as_str()))
        {
            j += 1;
        }
        if let Some(n) = tokens.get(j) {
            let spec = clean(n);
            if usable(&spec) {
                return Some(spec);
            }
        }
        // Look before the cue (skipping articles): "add a timeout parameter …".
        let mut k = i as isize - 1;
        while k >= 0 && ARTS.contains(&tokens[k as usize].to_ascii_lowercase().as_str()) {
            k -= 1;
        }
        if k >= 0 {
            let spec = clean(tokens[k as usize]);
            if usable(&spec) {
                return Some(spec);
            }
        }
    }
    None
}

fn between_backticks(prompt: &str) -> Option<String> {
    let start = prompt.find('`')?;
    let rest = &prompt[start + 1..];
    let end = rest.find('`')?;
    let code = rest[..end].trim();
    if code.is_empty() {
        None
    } else {
        Some(code.to_string())
    }
}

/// Auto-import for well-known bare decorators.
fn decorator_import(key: &str) -> Option<EditOp> {
    let (module, sym) = match key {
        "lru_cache" | "cache" | "wraps" | "reduce" | "partial" | "cached_property" => {
            ("functools", key)
        }
        "dataclass" | "field" => ("dataclasses", key),
        "contextmanager" => ("contextlib", key),
        _ => return None,
    };
    Some(EditOp::AddImport {
        module: module.to_string(),
        symbols: vec![sym.to_string()],
        raw: format!("from {} import {}", module, sym),
    })
}

/// Lower a prompt to micro-edit ops if it matches a tweak grammar and names a
/// def that exists in `model`; otherwise `None` (caller falls back to routing).
fn plan_micro_edits(prompt: &str, model: &StructModel) -> Option<Vec<EditOp>> {
    let lp = prompt.to_ascii_lowercase();
    let hint = class_name_from_prompt(prompt);
    let resolve = |target: &str| -> Option<Option<String>> {
        model
            .resolve_def(hint.as_deref(), target)
            .or_else(|| model.resolve_def(None, target))
            .map(|(_, owner)| owner)
    };

    // 1. Decorator: "decorate fetch with lru_cache" / "add staticmethod decorator to f".
    if lp.contains("decorat") {
        let target = ident_after(prompt, &["decorate", "decorating"])
            .or_else(|| ident_after(prompt, &["function", "method", "def"]))
            .or_else(|| ident_after(prompt, &["to", "on", "for"]));
        let deco = substr_after_ci(prompt, " with ")
            .and_then(|t| first_decorator_token(&t))
            .or_else(|| word_before(prompt, "decorator"));
        if let (Some(target), Some(deco)) = (target, deco) {
            if let Some(owner) = resolve(&target) {
                let mut ops = Vec::new();
                let key = decorator_key(&deco);
                if !key.contains('.') {
                    if let Some(imp) = decorator_import(&key) {
                        ops.push(imp);
                    }
                }
                ops.push(EditOp::AddDecorator {
                    class: owner,
                    target,
                    decorator: deco,
                });
                return Some(ops);
            }
        }
    }

    // 2. Parameter: "add a timeout parameter to fetch".
    if ["parameter", "param", "argument", "arg"]
        .iter()
        .any(|w| word_match(&lp, w))
    {
        let param = param_spec_after(prompt, &["parameter", "param", "argument", "arg"]);
        let target = ident_after(prompt, &["to", "of", "on", "for", "into"]);
        if let (Some(param), Some(target)) = (param, target) {
            if let Some(owner) = resolve(&target) {
                return Some(vec![EditOp::AddParam {
                    class: owner,
                    target,
                    param,
                }]);
            }
        }
    }

    // 3. Append to body: "append `return total` to compute" (code in backticks).
    if lp.contains("append") || word_match(&lp, "statement") || word_match(&lp, "line") {
        if let Some(code) = between_backticks(prompt) {
            if let Some(target) = ident_after(prompt, &["to", "of", "in", "into"]) {
                if let Some(owner) = resolve(&target) {
                    return Some(vec![EditOp::AppendToBody {
                        class: owner,
                        target,
                        code,
                    }]);
                }
            }
        }
    }
    None
}

/// One-line human summary of a micro-edit op (for the changeset header / log).
fn describe_op(op: &EditOp) -> String {
    let qual = |class: &Option<String>, target: &str| match class {
        Some(c) => format!("{}.{}", c, target),
        None => target.to_string(),
    };
    match op {
        EditOp::AddImport { raw, .. } => format!("ensure `{}`", raw),
        EditOp::AddDecorator {
            class,
            target,
            decorator,
        } => {
            format!("decorate {} with {}", qual(class, target), decorator)
        }
        EditOp::AddParam {
            class,
            target,
            param,
        } => {
            format!("add param `{}` to {}", param, qual(class, target))
        }
        EditOp::AppendToBody { class, target, .. } => {
            format!("append to {} body", qual(class, target))
        }
        EditOp::InsertDef { name, .. } => format!("insert def {}", name),
        EditOp::InsertMethod { class, name, .. } => format!("insert method {}.{}", class, name),
    }
}

/// Resolve the target class for a method insertion. Prefers the structural
/// model's known classes; falls back to a textual `class X` scan.
fn detect_target_class(prompt: &str, source: &str, model: Option<&StructModel>) -> Option<String> {
    let class_exists = |name: &str| -> bool {
        if let Some(m) = model {
            if m.classes.contains_key(name) {
                return true;
            }
        }
        source.lines().any(|l| {
            l.starts_with(&format!("class {}(", name)) || l.starts_with(&format!("class {}:", name))
        })
    };
    if let Some(name) = ident_after(prompt, &["class"]) {
        if class_exists(&name) {
            return Some(name);
        }
    }
    if let Some(name) = ident_after(prompt, &["to", "into", "onto"]) {
        if class_exists(&name) {
            return Some(name);
        }
    }
    None
}

/// Surface ranked routing candidates so an ambiguous prompt isn't a silent pick.
fn print_candidates(candidates: &[(&Template, i64)]) {
    if candidates.len() < 2 {
        return;
    }
    let top: Vec<String> = candidates
        .iter()
        .take(3)
        .map(|(t, s)| format!("{} ({})", t.id, s))
        .collect();
    println!("{} {}", "candidates:".dimmed(), top.join(", ").dimmed());
}

// ── Session VFS + changesets (spacekit-diff) ────────────────────────────────

fn hash_bytes(bytes: &[u8]) -> Hash {
    *blake3::hash(bytes).as_bytes()
}

/// Canonical VFS key: drop a leading `./` so `generated/x.py` and
/// `./generated/x.py` resolve to the same file.
fn norm_path(p: &str) -> String {
    p.strip_prefix("./").unwrap_or(p).to_string()
}

pub struct SessionVfs {
    files: BTreeMap<String, String>,
    baseline: BTreeMap<String, String>,
    undo_stack: Vec<BTreeMap<String, String>>,
    /// (prompt, template id) of the most recent accepted routing decision, so
    /// `/undo` can penalize that association in the routing memory.
    last_action: Option<(String, String)>,
}

impl SessionVfs {
    fn new() -> Self {
        Self {
            files: BTreeMap::new(),
            baseline: BTreeMap::new(),
            undo_stack: Vec::new(),
            last_action: None,
        }
    }

    fn checkpoint(&mut self) {
        self.undo_stack.push(self.files.clone());
    }

    fn undo(&mut self) -> bool {
        if let Some(prev) = self.undo_stack.pop() {
            self.files = prev;
            true
        } else {
            false
        }
    }

    fn get(&self, path: &str) -> Option<&String> {
        self.files.get(&norm_path(path))
    }

    /// Load a file into the VFS from disk if not already present; track its
    /// original content as the baseline so diffs are meaningful.
    fn ensure_loaded(&mut self, path: &str) -> Result<(), Box<dyn Error>> {
        let key = norm_path(path);
        if self.files.contains_key(&key) {
            return Ok(());
        }
        if let Ok(content) = std::fs::read_to_string(path) {
            self.baseline.insert(key.clone(), content.clone());
            self.files.insert(key, content);
        }
        Ok(())
    }

    fn set(&mut self, path: &str, content: String) {
        // No baseline entry here: files absent from `baseline` show up as
        // "Added" in the changeset; files loaded via `ensure_loaded` keep their
        // on-disk baseline and show up as "Modified".
        self.files.insert(norm_path(path), content);
    }

    fn snapshot(map: &BTreeMap<String, String>) -> TreeSnapshot {
        let mut entries: BTreeMap<String, Hash> = BTreeMap::new();
        for (p, c) in map {
            entries.insert(p.clone(), hash_bytes(c.as_bytes()));
        }
        TreeSnapshot { entries }
    }

    fn changeset(&self) -> Vec<TreeChange> {
        diff_trees(
            &Self::snapshot(&self.baseline),
            &Self::snapshot(&self.files),
        )
    }
}

// ── Execution ───────────────────────────────────────────────────────────────

/// Wall-clock cap for executing generated code (prevents hangs / runaway loops).
const VERIFY_TIMEOUT_SECS: u64 = 10;

/// Python modules that are safe to auto-import when an unqualified name resolves
/// to one of them (used by the repair loop).
const KNOWN_STD_MODULES: &[&str] = &[
    "math",
    "heapq",
    "functools",
    "itertools",
    "collections",
    "random",
    "re",
    "os",
    "sys",
    "json",
    "bisect",
    "string",
    "statistics",
    "datetime",
    "decimal",
    "fractions",
];

pub struct RunResult {
    pub ok: bool,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
}

/// Run a python file with a wall-clock timeout, draining stdout/stderr on
/// dedicated threads (so large output can't deadlock the pipe). Executes in
/// `cwd` with bytecode writing disabled and stdin closed. (Network isolation is
/// not enforced here — see the timeout as the primary safety bound.)
fn run_python_file(file: &Path, cwd: &Path) -> Result<RunResult, Box<dyn Error>> {
    use std::io::Read as _;
    use std::time::{Duration, Instant};

    let mut child = std::process::Command::new("python3")
        .arg(file)
        .current_dir(cwd)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to launch python3: {} (is it installed?)", e))?;

    let mut so = child.stdout.take();
    let mut se = child.stderr.take();
    let out_h = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(ref mut r) = so {
            let _ = r.read_to_string(&mut s);
        }
        s
    });
    let err_h = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(ref mut r) = se {
            let _ = r.read_to_string(&mut s);
        }
        s
    });

    let start = Instant::now();
    let timeout = Duration::from_secs(VERIFY_TIMEOUT_SECS);
    let (timed_out, status) = loop {
        match child.try_wait()? {
            Some(st) => break (false, Some(st)),
            None => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    break (true, None);
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    };

    let stdout = out_h.join().unwrap_or_default();
    let mut stderr = err_h.join().unwrap_or_default();
    let ok = !timed_out && status.map(|s| s.success()).unwrap_or(false);
    if timed_out {
        stderr.push_str(&format!(
            "\n[killed: exceeded {}s timeout]",
            VERIFY_TIMEOUT_SECS
        ));
    }
    Ok(RunResult {
        ok,
        stdout,
        stderr,
        timed_out,
    })
}

fn python_run_source(workdir: &Path, label: &str, full: &str) -> Result<RunResult, Box<dyn Error>> {
    std::fs::create_dir_all(workdir)?;
    let run_path = workdir.join(format!(".verify_{}.py", label));
    std::fs::write(&run_path, full)?;
    let res = run_python_file(&run_path, workdir);
    let _ = std::fs::remove_file(&run_path);
    res
}

/// Run code (+ optional test) with python3 (sandboxed by timeout).
pub fn python_verify(
    workdir: &Path,
    label: &str,
    code: &str,
    test: &str,
) -> Result<RunResult, Box<dyn Error>> {
    let mut full = code.trim_end().to_string();
    if !test.trim().is_empty() {
        full.push_str("\n\n");
        full.push_str(test.trim());
        full.push('\n');
    }
    python_run_source(workdir, label, &full)
}

/// Is an external tool runnable? (probes `--version`).
fn has_tool(name: &str) -> bool {
    std::process::Command::new(name)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn has_pyflakes() -> bool {
    std::process::Command::new("python3")
        .args(["-c", "import pyflakes"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Static analysis of a snippet (undefined names, unused imports, …). Returns
/// `None` when no linter is available, otherwise the list of issues found
/// (`Some(vec![])` == clean). Prefers `ruff`, falls back to `pyflakes`.
fn static_check(workdir: &Path, code: &str) -> Option<Vec<String>> {
    let _ = std::fs::create_dir_all(workdir);
    let path = workdir.join(format!(".lint_{}.py", std::process::id()));
    if std::fs::write(&path, code).is_err() {
        return None;
    }
    let result = if has_tool("ruff") {
        std::process::Command::new("ruff")
            .args(["check", "--quiet", "--no-cache"])
            .arg(&path)
            .output()
            .ok()
            .map(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .chain(String::from_utf8_lossy(&o.stderr).lines())
                    .filter(|l| !l.trim().is_empty())
                    .map(|l| l.to_string())
                    .collect::<Vec<_>>()
            })
    } else if has_pyflakes() {
        std::process::Command::new("python3")
            .args(["-m", "pyflakes"])
            .arg(&path)
            .output()
            .ok()
            .map(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .filter(|l| !l.trim().is_empty())
                    .map(|l| l.to_string())
                    .collect::<Vec<_>>()
            })
    } else {
        None
    };
    let _ = std::fs::remove_file(&path);
    result
}

/// Apply `ruff format` / `black` if available; returns the original on failure.
fn format_code(code: &str) -> String {
    use std::io::Write as _;
    let try_fmt = |cmd: &str, args: &[&str]| -> Option<String> {
        let mut child = std::process::Command::new(cmd)
            .args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .ok()?;
        child.stdin.take()?.write_all(code.as_bytes()).ok()?;
        let out = child.wait_with_output().ok()?;
        if out.status.success() && !out.stdout.is_empty() {
            Some(String::from_utf8_lossy(&out.stdout).to_string())
        } else {
            None
        }
    };
    if has_tool("ruff") {
        if let Some(f) = try_fmt("ruff", &["format", "-"]) {
            return f;
        }
    }
    if has_tool("black") {
        if let Some(f) = try_fmt("black", &["-q", "-"]) {
            return f;
        }
    }
    code.to_string()
}

/// Diagnose a traceback and propose a repaired version of `code`.
/// Returns `(description, new_code)`. Conservative: only fixes it can make
/// safely (missing imports today; extend with more mappings over time).
fn diagnose_fix(code: &str, stderr: &str) -> Option<(String, String)> {
    // ModuleNotFoundError: No module named 'X'
    let m = "ModuleNotFoundError: No module named '";
    if let Some(idx) = stderr.find(m) {
        let rest = &stderr[idx + m.len()..];
        if let Some(end) = rest.find('\'') {
            let module = &rest[..end];
            let imp = format!("import {}", module);
            if !code.contains(&imp) {
                return Some((format!("add `{}`", imp), format!("{}\n{}", imp, code)));
            }
        }
    }
    // NameError: name 'X' is not defined  → import X if X is a known module
    let n = "NameError: name '";
    if let Some(idx) = stderr.find(n) {
        let rest = &stderr[idx + n.len()..];
        if let Some(end) = rest.find('\'') {
            let name = &rest[..end];
            if KNOWN_STD_MODULES.contains(&name) {
                let imp = format!("import {}", name);
                if !code.contains(&imp) {
                    return Some((format!("add `{}`", imp), format!("{}\n{}", imp, code)));
                }
            }
        }
    }
    None
}

/// Quality report for a constructed snippet.
struct VerifyReport {
    code: String,
    test_ok: bool,
    lint_issues: usize,
    verifiable: bool,
    fixes: Vec<String>,
    stderr: String,
}

/// Run a snippet's embedded test under the sandbox, applying the bounded repair
/// loop (run → diagnose → patch → run, up to 3 rounds), then static-check the
/// final code.
fn verify_and_repair(workdir: &Path, label: &str, code: &str, test: &str) -> VerifyReport {
    let mut current = code.to_string();
    let mut fixes: Vec<String> = Vec::new();
    let mut last_stderr = String::new();
    let mut test_ok = false;
    let mut verifiable = false;

    for _ in 0..3 {
        match python_verify(workdir, label, &current, test) {
            Ok(res) => {
                verifiable = true;
                last_stderr = res.stderr.clone();
                if res.ok {
                    test_ok = true;
                    break;
                }
                match diagnose_fix(&current, &res.stderr) {
                    Some((desc, fixed)) => {
                        fixes.push(desc);
                        current = fixed;
                    }
                    None => break,
                }
            }
            Err(_) => {
                // python3 unavailable: can't verify.
                verifiable = false;
                break;
            }
        }
    }

    let lint_issues = static_check(workdir, &current)
        .map(|v| v.len())
        .unwrap_or(0);
    VerifyReport {
        code: current,
        test_ok,
        lint_issues,
        verifiable,
        fixes,
        stderr: last_stderr,
    }
}

// ── CLI entry ───────────────────────────────────────────────────────────────

pub struct CodeArgs {
    pub prompt: Option<String>,
    pub templates: PathBuf,
    pub graph: PathBuf,
    pub out: Option<PathBuf>,
    pub workdir: PathBuf,
    pub run: bool,
    pub file: Option<PathBuf>,
    pub session: bool,
}

pub fn handle_code(args: &CodeArgs) -> Result<(), Box<dyn Error>> {
    let lib = TemplateLibrary::load(&args.templates)?;
    eprintln!(
        "{} {} templates from {}",
        "loaded".dimmed(),
        lib.templates.len(),
        args.templates.display()
    );

    ensure_route_table(&lib, &args.graph);

    if args.session {
        return repl(&lib, args);
    }

    let prompt = args
        .prompt
        .as_deref()
        .ok_or("`agent code` requires --prompt \"...\" (or --session for a REPL)")?;

    let mut vfs = SessionVfs::new();
    let requested = args.file.as_ref().map(|f| f.to_string_lossy().to_string());
    match resolve_edit_target(args, &vfs, requested.as_deref(), prompt)? {
        Resolution::Edit(target, _class) => {
            if requested.as_deref() != Some(target.as_str()) {
                println!("{} {}", "resolved ->".green().bold(), target);
            }
            modify_existing(&lib, args, &mut vfs, &target, prompt)?;
            if let Some(content) = vfs.get(&target) {
                std::fs::write(&target, content)?;
                println!("{} {}", "updated".green().bold(), target);
            }
        }
        Resolution::Construct => construct_new(&lib, args, &mut vfs, prompt)?,
        Resolution::Ambiguous(class, files) => return Err(ambiguous_msg(&class, &files).into()),
    }
    Ok(())
}

// ── Construct (Phase 1 + 2) ─────────────────────────────────────────────────

/// Select code for a prompt. When the prompt explicitly composes templates,
/// merges them; otherwise routes to candidates and (when ambiguous) runs
/// best-of-k verification to pick the candidate whose embedded test passes.
/// Returns (label, code, test, primary_id).
/// Lowercase alphanumeric tokens (≥3 chars) of a free-text prompt.
fn prompt_tokens(s: &str) -> std::collections::HashSet<String> {
    s.to_ascii_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() >= 3)
        .map(|t| t.to_string())
        .collect()
}

/// Searchable vocabulary of a template: keyword tokens + id parts + summary words.
fn template_vocab(t: &Template) -> std::collections::HashSet<String> {
    let mut set = keyword_tokens(t);
    for part in t.id.split('_') {
        if part.len() >= 3 {
            set.insert(part.to_ascii_lowercase());
        }
    }
    for w in t
        .summary
        .to_ascii_lowercase()
        .split(|c: char| !c.is_alphanumeric())
    {
        if w.len() >= 3 {
            set.insert(w.to_string());
        }
    }
    set
}

/// Fuzzy "did you mean" fallback: templates whose vocabulary overlaps the prompt
/// the most, even when no keyword phrase matched exactly.
fn nearest_templates<'a>(lib: &'a TemplateLibrary, prompt: &str, n: usize) -> Vec<&'a Template> {
    let p = prompt_tokens(prompt);
    if p.is_empty() {
        return Vec::new();
    }
    let mut scored: Vec<(&Template, usize)> = lib
        .templates
        .iter()
        .map(|t| (t, template_vocab(t).intersection(&p).count()))
        .filter(|(_, c)| *c > 0)
        .collect();
    scored.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.id.cmp(&b.0.id)));
    scored.into_iter().take(n).map(|(t, _)| t).collect()
}

/// Build a helpful "no exact match" error that suggests the closest catalog
/// entries instead of dead-ending the user.
fn no_match_error(lib: &TemplateLibrary, prompt: &str) -> Box<dyn Error> {
    let near = nearest_templates(lib, prompt, 3);
    if near.is_empty() {
        return format!(
            "no template matched {:?}.\nName an algorithm or pattern (e.g. \"binary search\", \"strategy pattern\"). Run `agent route-compile --verify` to list the catalog.",
            prompt
        )
        .into();
    }
    let mut msg = format!("no exact match for {:?}. Did you mean:\n", prompt);
    for t in near {
        msg.push_str(&format!("  - {} — {}\n", t.id, t.summary));
    }
    msg.push_str(
        "Re-run naming one of these (e.g. its keywords), or pick a different algorithm/pattern.",
    );
    msg.into()
}

/// Selection result: (label, code, test, primary_id, verified).
/// `verified` = the returned code ran and its test passed (drives the learning
/// loop's event `ok` flag and routing-memory reinforcement).
type Selection = (String, String, String, String, bool);

fn select_and_render(
    lib: &TemplateLibrary,
    args: &CodeArgs,
    prompt: &str,
) -> Result<Selection, Box<dyn Error>> {
    let parts_t = lib.route_multi(&args.graph, prompt);
    if parts_t.len() > 1 {
        let parts: Vec<(&Template, BTreeMap<String, String>)> = parts_t
            .iter()
            .map(|t| (*t, extract_values(t, prompt)))
            .collect();
        let code = compose(&parts);
        let test = parts
            .iter()
            .map(|(t, v)| render(&t.test, v))
            .collect::<Vec<_>>()
            .join("\n");
        let ids: Vec<&str> = parts_t.iter().map(|t| t.id.as_str()).collect();
        let primary = parts_t[0];
        // Verify the composition. Composed units can collide (name clashes, merged
        // imports), so this is a real gate, not a formality.
        let rep = verify_and_repair(&args.workdir, &primary.id, &code, &test);
        if rep.verifiable && rep.test_ok {
            println!(
                "{} {} {} [{}]",
                "route ->".green().bold(),
                ids.join(" + ").cyan().bold(),
                "(composed)".dimmed(),
                "verified".green()
            );
            return Ok((primary.id.clone(), rep.code, test, primary.id.clone(), true));
        }
        // Verify-guided diffusion: a bad composition shouldn't ship. Fall through
        // to single-template construction and let the ranked path pick a unit that
        // actually runs.
        println!(
            "{} {} {}",
            "compose unverified ->".yellow().bold(),
            ids.join(" + ").dimmed(),
            "diffusing to single-template construction".dimmed()
        );
    }

    let candidates = lib.route_ranked(&args.graph, prompt);
    if candidates.is_empty() {
        return Err(no_match_error(lib, prompt));
    }
    print_candidates(&candidates);

    let top_rank = candidates[0].1;

    let render_verdict = |args: &CodeArgs, t: &Template| -> (String, String, &'static str) {
        let values = extract_values(t, prompt);
        let code = instantiate(t, &values);
        let test = render(&t.test, &values);
        let rep = verify_and_repair(&args.workdir, &t.id, &code, &test);
        let verdict = if !rep.verifiable {
            "unverified"
        } else if rep.test_ok && rep.lint_issues == 0 {
            "verified"
        } else if rep.test_ok {
            "verified*" // passes but has lint nits
        } else {
            "best-effort" // semantically correct, test still red
        };
        (rep.code, test, verdict)
    };

    // Confident semantic pin (Growformer) OR a single candidate: routing is
    // authoritative. Verify/repair is a *quality* pass — we never swap to a
    // semantically weaker template just because its test happens to pass.
    if candidates.len() == 1 || top_rank >= 1000 {
        let t = candidates[0].0;
        let (code, test, verdict) = render_verdict(args, t);
        println!(
            "{} {} {} [{}]",
            "route ->".green().bold(),
            t.id.cyan().bold(),
            format!("({} / {})", t.kind, t.category).dimmed(),
            verdict.color(if verdict.starts_with("verified") {
                "green"
            } else {
                "yellow"
            })
        );
        return Ok((
            t.id.clone(),
            code,
            test,
            t.id.clone(),
            verdict.starts_with("verified"),
        ));
    }

    // Ambiguous (keyword-only) route: best-of-k, but only among candidates that
    // are *semantically competitive* with the top (within 2x rank), and fold the
    // route rank into the score so a much weaker match can't win on test alone.
    let mut best: Option<(&Template, String, String)> = None;
    let mut best_score = i64::MIN;
    for (t, rank) in candidates.iter().take(4) {
        if rank * 2 < top_rank {
            continue; // not competitive enough to consider
        }
        let values = extract_values(t, prompt);
        let code = instantiate(t, &values);
        let test = render(&t.test, &values);
        let rep = verify_and_repair(&args.workdir, &t.id, &code, &test);
        let score = if rep.verifiable {
            (rep.test_ok as i64) * 1000 - (rep.lint_issues as i64) + rank
        } else {
            *rank // unverifiable: fall back to route rank
        };
        if score > best_score {
            best_score = score;
            best = Some((*t, rep.code.clone(), test));
        }
        if rep.verifiable && rep.test_ok && rep.lint_issues == 0 {
            break; // clean pass — stop early
        }
    }
    let (t, code, test) = best.expect("at least one competitive candidate");
    let verified = best_score >= 1000;
    let verdict = if verified {
        "verified".green()
    } else {
        "best-effort".yellow()
    };
    println!(
        "{} {} {} [{}]",
        "route ->".green().bold(),
        t.id.cyan().bold(),
        format!("({} / {})", t.kind, t.category).dimmed(),
        verdict
    );
    Ok((t.id.clone(), code, test, t.id.clone(), verified))
}

fn construct_new(
    lib: &TemplateLibrary,
    args: &CodeArgs,
    vfs: &mut SessionVfs,
    prompt: &str,
) -> Result<(), Box<dyn Error>> {
    let (label, code, test, primary_id, verified) = select_and_render(lib, args, prompt)?;
    let code = format_code(&code);

    let out_path = args.out.clone().unwrap_or_else(|| {
        args.workdir
            .join("generated")
            .join(artifact_name(&primary_id))
    });
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&out_path, &code)?;
    vfs.checkpoint();
    vfs.set(&out_path.to_string_lossy(), code.clone());
    println!("{} {}", "wrote".green().bold(), out_path.display());
    println!("\n{}\n{}", "--- code ---".dimmed(), code);

    if args.run {
        run_with_fixloop(args, vfs, &out_path.to_string_lossy(), &label, &code, &test)?;
    }

    // Learning loop: reinforce a verified pick, but record a confident-but-failed
    // route as a soft negative AND an honest `ok:false` event so the offline
    // hard-negative miner (corpus_diffuse.py) sees real failures to train against.
    record_route(
        &args.graph,
        prompt,
        &primary_id,
        if verified { 1.0 } else { -0.25 },
    );
    vfs.last_action = Some((prompt.to_string(), primary_id.clone()));
    log_event(
        &args.workdir,
        "construct",
        prompt,
        &primary_id,
        &out_path.to_string_lossy(),
        verified,
    );
    Ok(())
}

// ── Cross-file targeting (uses the repo symbol index) ───────────────────────

struct SymbolHit {
    file: String,
    kind: String,
    #[allow(dead_code)]
    line: usize,
}

/// Index top-level classes & functions across the python files under `root`.
fn index_python_symbols(root: &Path) -> BTreeMap<String, Vec<SymbolHit>> {
    let root_abs = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let mut entries: Vec<(String, bool, PathBuf)> = Vec::new();
    collect_entries(&root_abs, &root_abs, &mut entries);
    let mut py: Vec<(String, String)> = Vec::new();
    for (rel, is_dir, abs) in &entries {
        if *is_dir || lang_for(rel) != "python" {
            continue;
        }
        if let Ok(t) = std::fs::read_to_string(abs) {
            py.push((rel.clone(), t));
        }
    }
    let mut index: BTreeMap<String, Vec<SymbolHit>> = BTreeMap::new();
    if let Some(an) = analyze_python_files(&py) {
        for (rel, pf) in &an.files {
            for c in &pf.classes {
                index.entry(c.name.clone()).or_default().push(SymbolHit {
                    file: rel.clone(),
                    kind: "class".to_string(),
                    line: c.line,
                });
            }
            for f in &pf.functions {
                index.entry(f.name.clone()).or_default().push(SymbolHit {
                    file: rel.clone(),
                    kind: "function".to_string(),
                    line: f.line,
                });
            }
        }
    }
    index
}

fn read_target(vfs: &SessionVfs, path: &str) -> Option<String> {
    if let Some(s) = vfs.get(path) {
        return Some(s.clone());
    }
    std::fs::read_to_string(path).ok()
}

fn class_in_source(src: &str, name: &str) -> bool {
    src.lines().any(|l| {
        l.starts_with(&format!("class {}(", name)) || l.starts_with(&format!("class {}:", name))
    })
}

/// A class name referenced by the prompt ("… to class App", "… into App").
/// The explicit `class X` cue is case-agnostic; prepositional cues only count
/// when the target looks like a class (CapWords) to avoid false scans on
/// ordinary prompts like "function to reverse a string".
fn class_name_from_prompt(prompt: &str) -> Option<String> {
    if let Some(name) = ident_after(prompt, &["class"]) {
        return Some(name);
    }
    if let Some(name) = ident_after(prompt, &["to", "into", "onto"]) {
        if name
            .chars()
            .next()
            .map(|c| c.is_uppercase())
            .unwrap_or(false)
        {
            return Some(name);
        }
    }
    None
}

/// All files under `root` that define class `name` (absolute paths).
fn class_file_hits(root: &Path, name: &str) -> Vec<String> {
    let index = index_python_symbols(root);
    let root_abs = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    match index.get(name) {
        Some(hits) => hits
            .iter()
            .filter(|h| h.kind == "class")
            .map(|h| root_abs.join(&h.file).to_string_lossy().to_string())
            .collect(),
        None => Vec::new(),
    }
}

/// Outcome of deciding where an "add"/"edit" should land.
enum Resolution {
    /// Edit an existing file. (path, class_name)
    Edit(String, Option<String>),
    /// Nothing to edit — construct a fresh module instead.
    Construct,
    /// A named class lives in more than one file; the caller must disambiguate.
    Ambiguous(String, Vec<String>),
}

/// Decide which file an "add" should edit. Redirects across files when the
/// prompt names a class that lives elsewhere; resolves a file purely from the
/// class when none was supplied. Ambiguity (a class defined in several files)
/// is surfaced explicitly rather than silently falling back.
fn resolve_edit_target(
    args: &CodeArgs,
    vfs: &SessionVfs,
    requested: Option<&str>,
    prompt: &str,
) -> Result<Resolution, Box<dyn Error>> {
    let class = class_name_from_prompt(prompt);

    if let Some(req) = requested {
        let src = read_target(vfs, req);
        if let Some(s) = &src {
            // Class present in the requested file (or no class → top-level insert here).
            let here = class
                .as_ref()
                .map(|c| class_in_source(s, c))
                .unwrap_or(true);
            if here {
                return Ok(Resolution::Edit(req.to_string(), class));
            }
        }
        // Requested file doesn't host the class — try to redirect to where it lives.
        if let Some(c) = &class {
            let hits = class_file_hits(&args.workdir, c);
            match hits.len() {
                1 => return Ok(Resolution::Edit(hits.into_iter().next().unwrap(), class)),
                n if n > 1 => return Ok(Resolution::Ambiguous(c.clone(), hits)),
                _ => {}
            }
        }
        // Fall back to the requested file (top-level insert) if it exists.
        if src.is_some() {
            return Ok(Resolution::Edit(req.to_string(), class));
        }
        return Err(format!("target file not found: {}", req).into());
    }

    // No file supplied: resolve purely from a class reference.
    if let Some(c) = &class {
        let hits = class_file_hits(&args.workdir, c);
        match hits.len() {
            1 => return Ok(Resolution::Edit(hits.into_iter().next().unwrap(), class)),
            n if n > 1 => return Ok(Resolution::Ambiguous(c.clone(), hits)),
            // Named a class we can't find — construct rather than error out.
            _ => return Ok(Resolution::Construct),
        }
    }
    Ok(Resolution::Construct)
}

/// Human-readable error for an ambiguous class reference.
fn ambiguous_msg(class: &str, files: &[String]) -> String {
    format!(
        "class `{}` is defined in {} files:\n  {}\nPass --file <path> to choose one.",
        class,
        files.len(),
        files.join("\n  ")
    )
}

// ── Modify-existing (Phase 3) ───────────────────────────────────────────────

fn modify_existing(
    lib: &TemplateLibrary,
    args: &CodeArgs,
    vfs: &mut SessionVfs,
    path: &str,
    prompt: &str,
) -> Result<(), Box<dyn Error>> {
    vfs.ensure_loaded(path)?;
    let source = vfs
        .get(path)
        .cloned()
        .ok_or_else(|| format!("target file not found: {}", path))?;

    // One structural model drives both class detection and the edit application.
    let model = analyze(&source);

    // Micro-edit fast path: tweak an existing def (decorator/param/body) rather
    // than splice a template. Only fires when the named def exists in the model.
    if let Some(m) = &model {
        if let Some(ops) = plan_micro_edits(prompt, m) {
            let new_source = apply_edits(&source, &ops)?;
            let summary: Vec<String> = ops.iter().map(describe_op).collect();
            println!("{} {}", "edit ->".green().bold(), summary.join("; ").cyan());
            if new_source == source {
                println!("{}", "no change (already applied)".dimmed());
                return Ok(());
            }
            vfs.checkpoint();
            vfs.set(path, new_source.clone());
            println!("\n{}", "--- changeset ---".dimmed());
            let base = vfs
                .baseline
                .get(&norm_path(path))
                .map(|s| s.as_str())
                .unwrap_or("");
            print_blob_diff(base, &new_source);
            let kind = ops.last().map(describe_op).unwrap_or_default();
            log_event(&args.workdir, "micro-edit", prompt, &kind, path, true);
            return Ok(());
        }
    }

    let candidates = lib.route_ranked(&args.graph, prompt);
    if candidates.is_empty() {
        return Err(no_match_error(lib, prompt));
    }
    let t = candidates[0].0;
    print_candidates(&candidates);
    let values = extract_values(t, prompt);

    let target_class = detect_target_class(prompt, &source, model.as_ref());

    if let Some(class) = &target_class {
        let code = body_only(t, &values);
        let c = code.trim_start();
        if !(c.starts_with("def ") || c.starts_with("async def ")) {
            return Err(format!(
                "template {} is not a function; cannot insert it as a method of {}",
                t.id, class
            )
            .into());
        }
    }

    println!(
        "{} {} into {}{}",
        "add ->".green().bold(),
        t.id.cyan().bold(),
        path,
        target_class
            .as_ref()
            .map(|c| format!(" (method of {})", c))
            .unwrap_or_default()
    );

    let ops = plan_edits(t, &values, target_class);
    let new_source = apply_edits(&source, &ops)?;

    vfs.checkpoint();
    vfs.set(path, new_source.clone());
    println!("\n{}", "--- changeset ---".dimmed());
    let base = vfs
        .baseline
        .get(&norm_path(path))
        .map(|s| s.as_str())
        .unwrap_or("");
    print_blob_diff(base, &new_source);

    // Learning loop: the splice applied cleanly — reinforce + log.
    record_route(&args.graph, prompt, &t.id, 1.0);
    vfs.last_action = Some((prompt.to_string(), t.id.clone()));
    log_event(&args.workdir, "modify", prompt, &t.id, path, true);
    Ok(())
}

// ── Run with fix-loop (Phase 5) ─────────────────────────────────────────────

fn run_with_fixloop(
    args: &CodeArgs,
    vfs: &mut SessionVfs,
    path: &str,
    label: &str,
    code: &str,
    test: &str,
) -> Result<(), Box<dyn Error>> {
    let report = verify_and_repair(&args.workdir, label, code, test);
    for fix in &report.fixes {
        println!("{} {} and re-running", "fix ->".yellow().bold(), fix);
    }
    // Persist any repaired code back to disk + session.
    if report.code != code {
        std::fs::write(path, &report.code)?;
        vfs.set(path, report.code.clone());
    }
    if report.test_ok {
        println!("{} python3 verification passed", "run ->".green().bold());
    } else if !report.verifiable {
        println!(
            "{} python3 unavailable; skipped verification",
            "run ->".yellow().bold()
        );
    } else {
        println!("{} python3 verification FAILED", "run ->".red().bold());
    }
    if report.lint_issues > 0 {
        println!("{} {} lint issue(s)", "lint:".yellow(), report.lint_issues);
    }
    if !report.stderr.trim().is_empty() && !report.test_ok {
        println!("{}\n{}", "stderr:".dimmed(), report.stderr.trim_end());
    }
    if report.verifiable && !report.test_ok {
        return Err("generated code failed python3 verification".into());
    }
    Ok(())
}

// ── Diff rendering ──────────────────────────────────────────────────────────

fn print_blob_diff(old: &str, new: &str) {
    let trim = |s: &str| s.trim_end_matches('\n').to_string();
    let hunks = diff_blobs(old.as_bytes(), new.as_bytes());
    for hunk in hunks {
        match hunk {
            DiffHunk::Equal { .. } => {}
            DiffHunk::Insert { lines, .. } => {
                for l in lines {
                    println!("{}", format!("+ {}", trim(&l)).green());
                }
            }
            DiffHunk::Delete { lines, .. } => {
                for l in lines {
                    println!("{}", format!("- {}", trim(&l)).red());
                }
            }
            DiffHunk::Replace {
                old_lines,
                new_lines,
                ..
            } => {
                for l in old_lines {
                    println!("{}", format!("- {}", trim(&l)).red());
                }
                for l in new_lines {
                    println!("{}", format!("+ {}", trim(&l)).green());
                }
            }
        }
    }
}

// ── Multi-turn REPL (Phase 4 + 5) ───────────────────────────────────────────

fn repl(lib: &TemplateLibrary, args: &CodeArgs) -> Result<(), Box<dyn Error>> {
    let mut vfs = SessionVfs::new();
    println!(
        "{}",
        "Python dev session. Commands: /new <prompt>, /add <file> <prompt>, /show, /diff, /run [file], /undo, /write [dir], /commit <msg>, /help, /quit"
            .dimmed()
    );
    let stdin = std::io::stdin();
    loop {
        print!("{} ", "code>".cyan().bold());
        std::io::stdout().flush().ok();
        let mut line = String::new();
        if stdin.lock().read_line(&mut line)? == 0 {
            break; // EOF
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let (cmd, rest) = match line.split_once(char::is_whitespace) {
            Some((c, r)) => (c, r.trim()),
            None => (line, ""),
        };
        let result: Result<(), Box<dyn Error>> = match cmd {
            "/quit" | "/q" | "/exit" => break,
            "/help" | "/h" => {
                println!("/new <prompt> | /add <file> <prompt> | /edit <prompt> | /show | /diff | /run [file] | /undo | /write [dir] | /map [root] [out] | /commit <msg> | /quit");
                Ok(())
            }
            "/new" => construct_new(lib, args, &mut vfs, rest),
            "/add" => match rest.split_once(char::is_whitespace) {
                Some((file, prompt)) => {
                    do_modify(lib, args, &mut vfs, Some(file.trim()), prompt.trim())
                }
                None => Err("usage: /add <file> <prompt>".into()),
            },
            "/edit" => {
                if rest.is_empty() {
                    Err("usage: /edit <prompt with a class name, e.g. 'add a cache method to class App'>".into())
                } else {
                    do_modify(lib, args, &mut vfs, None, rest)
                }
            }
            "/show" => {
                cmd_show(&vfs);
                Ok(())
            }
            "/diff" => {
                cmd_diff(&vfs);
                Ok(())
            }
            "/run" => cmd_run(args, &mut vfs, rest),
            "/undo" => {
                if vfs.undo() {
                    println!("{}", "reverted last change".yellow());
                    // Learning loop: the user rejected the last pick — weaken it.
                    if let Some((p, t)) = vfs.last_action.take() {
                        record_route(&args.graph, &p, &t, -1.0);
                    }
                } else {
                    println!("{}", "nothing to undo".dimmed());
                }
                Ok(())
            }
            "/write" => cmd_write(&vfs, args, rest),
            "/map" => {
                let mut parts = rest.split_whitespace();
                let root = parts
                    .next()
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from("."));
                let out = parts.next().map(PathBuf::from);
                handle_map(&RepoMapArgs { root, out })
            }
            "/commit" => cmd_commit(&vfs, args, rest),
            other if other.starts_with('/') => Err(format!("unknown command: {}", other).into()),
            // Bare text == /new
            _ => construct_new(lib, args, &mut vfs, line),
        };
        if let Err(e) = result {
            println!("{} {}", "error:".red().bold(), e);
        }
    }
    Ok(())
}

fn cmd_show(vfs: &SessionVfs) {
    if vfs.files.is_empty() {
        println!("{}", "(session is empty)".dimmed());
        return;
    }
    println!("{}", "session files:".dimmed());
    for (p, c) in &vfs.files {
        println!(
            "  {} {}",
            p.cyan(),
            format!("({} lines)", c.lines().count()).dimmed()
        );
    }
}

fn cmd_diff(vfs: &SessionVfs) {
    let changes = vfs.changeset();
    if changes.is_empty() {
        println!("{}", "no changes".dimmed());
        return;
    }
    for ch in &changes {
        match ch {
            TreeChange::Added { path, .. } => {
                println!("{} {}", "A".green().bold(), path);
                print_blob_diff(
                    vfs.baseline.get(path).map(|s| s.as_str()).unwrap_or(""),
                    &vfs.files[path],
                );
            }
            TreeChange::Modified { path, .. } => {
                println!("{} {}", "M".yellow().bold(), path);
                print_blob_diff(
                    vfs.baseline.get(path).map(|s| s.as_str()).unwrap_or(""),
                    &vfs.files[path],
                );
            }
            TreeChange::Removed { path, .. } => {
                println!("{} {}", "D".red().bold(), path);
            }
        }
    }
}

/// Shared REPL modify path: resolve the target file (with cross-file redirect)
/// then apply the edit to the session VFS.
fn do_modify(
    lib: &TemplateLibrary,
    args: &CodeArgs,
    vfs: &mut SessionVfs,
    requested: Option<&str>,
    prompt: &str,
) -> Result<(), Box<dyn Error>> {
    match resolve_edit_target(args, vfs, requested, prompt)? {
        Resolution::Edit(target, _class) => {
            if requested != Some(target.as_str()) {
                println!("{} {}", "resolved ->".green().bold(), target);
            }
            modify_existing(lib, args, vfs, &target, prompt)
        }
        Resolution::Ambiguous(class, files) => Err(ambiguous_msg(&class, &files).into()),
        // /add with a file should have resolved to that file; /edit needs a class.
        Resolution::Construct => Err(
            "could not resolve a target class from the prompt; name a class (e.g. \"… to class App\") or use /new to construct a module"
                .into(),
        ),
    }
}

fn cmd_run(args: &CodeArgs, vfs: &mut SessionVfs, rest: &str) -> Result<(), Box<dyn Error>> {
    let path = if rest.is_empty() {
        vfs.files
            .keys()
            .next()
            .cloned()
            .ok_or("session is empty; nothing to run")?
    } else {
        let want = norm_path(rest);
        vfs.files
            .keys()
            .find(|k| **k == want || k.ends_with(&format!("/{}", want)) || k.ends_with(&want))
            .cloned()
            .ok_or_else(|| format!("{} is not in the session (use /show)", rest))?
    };
    let code = vfs
        .files
        .get(&path)
        .cloned()
        .ok_or_else(|| format!("{} is not in the session (use /show)", path))?;
    let label = Path::new(&path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("run")
        .to_string();
    run_with_fixloop(args, vfs, &path, &label, &code, "")
}

fn cmd_write(vfs: &SessionVfs, args: &CodeArgs, rest: &str) -> Result<(), Box<dyn Error>> {
    let dir = if rest.is_empty() {
        args.workdir.clone()
    } else {
        PathBuf::from(rest)
    };
    let dot = Path::new(".");
    for (p, c) in &vfs.files {
        let dest = if Path::new(p).is_absolute() {
            PathBuf::from(p)
        } else if dir == dot {
            PathBuf::from(p)
        } else {
            dir.join(p)
        };
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&dest, c)?;
        println!("{} {}", "wrote".green(), dest.display());
    }
    Ok(())
}

fn cmd_commit(vfs: &SessionVfs, args: &CodeArgs, msg: &str) -> Result<(), Box<dyn Error>> {
    if msg.trim().is_empty() {
        return Err("usage: /commit <message>".into());
    }
    if vfs.files.is_empty() {
        return Err("nothing to commit".into());
    }
    // Materialize files, then build a spacekit-repo CommitContent (tree of
    // path -> blake3 hex). Full push to a storage node is out of scope here.
    cmd_write(vfs, args, "")?;
    let mut tree: BTreeMap<String, String> = BTreeMap::new();
    for (p, c) in &vfs.files {
        tree.insert(p.clone(), hex::encode(hash_bytes(c.as_bytes())));
    }
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let commit =
        spacekit_repo::CommitContent::new(tree, msg.to_string(), "spacekit-code".to_string(), ts);
    let json = serde_json::to_string_pretty(&commit)?;
    let commit_path = args
        .workdir
        .join(".spacekit-code-session")
        .join("commit.json");
    if let Some(parent) = commit_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&commit_path, &json)?;
    println!(
        "{} {} files, recorded commit -> {}",
        "commit ->".green().bold(),
        commit.tree.len(),
        commit_path.display()
    );
    Ok(())
}

// ── Repo map (project graph for ML / cross-file targeting) ───────────────────
//
// Emits a content-addressed node/edge graph of the repository:
//   nodes: dir, file, class, method, function
//   edges: contains (dir→file, file→symbol, class→method),
//          imports (file→file internal / file→module external),
//          calls   (function→function/class, best-effort, resolved-only)
//
// Python files are parsed with the same embedded `ast` backend used by the
// editor; other files become file nodes tagged by language. The schema is
// deliberately a flat node/edge list so it drops straight into graph-ML
// pipelines (and later lets modify-existing target symbols across files).

#[derive(Debug, Default, serde::Serialize)]
struct RepoNode {
    id: String,
    kind: String,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    class: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    lang: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    lines: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    hash: Option<String>,
}

#[derive(Debug, serde::Serialize)]
struct RepoEdge {
    #[serde(rename = "type")]
    etype: String,
    from: String,
    to: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    module: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    external: Option<bool>,
}

#[derive(Debug, serde::Serialize)]
struct RepoStats {
    dirs: usize,
    files: usize,
    py_files: usize,
    symbols: usize,
    edges: usize,
    languages: BTreeMap<String, usize>,
}

#[derive(Debug, serde::Serialize)]
struct RepoMap {
    schema: String,
    root: String,
    generated_at: u64,
    stats: RepoStats,
    nodes: Vec<RepoNode>,
    edges: Vec<RepoEdge>,
}

// Python analysis result (one entry per .py file).

#[derive(Debug, Deserialize)]
struct PyImport {
    module: String,
    #[serde(default)]
    level: usize,
    #[serde(default)]
    line: usize,
}

#[derive(Debug, Deserialize)]
struct PyMethod {
    name: String,
    #[serde(default)]
    line: usize,
}

#[derive(Debug, Deserialize)]
struct PyClass {
    name: String,
    #[serde(default)]
    line: usize,
    #[serde(default)]
    methods: Vec<PyMethod>,
}

#[derive(Debug, Deserialize)]
struct PyFunc {
    name: String,
    #[serde(default)]
    line: usize,
    #[serde(default)]
    calls: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct PyFile {
    #[serde(default)]
    functions: Vec<PyFunc>,
    #[serde(default)]
    classes: Vec<PyClass>,
    #[serde(default)]
    imports: Vec<PyImport>,
}

#[derive(Debug, Deserialize)]
struct PyRepoAnalysis {
    #[serde(default)]
    files: BTreeMap<String, PyFile>,
}

const REPO_MAP_PY: &str = r#"
import sys, json, ast

def calls_in(node):
    names = set()
    for n in ast.walk(node):
        if isinstance(n, ast.Call):
            f = n.func
            if isinstance(f, ast.Name):
                names.add(f.id)
            elif isinstance(f, ast.Attribute):
                names.add(f.attr)
    return sorted(names)

def analyze(src):
    tree = ast.parse(src)
    res = {"functions": [], "classes": [], "imports": []}
    for node in tree.body:
        if isinstance(node, ast.Import):
            for a in node.names:
                res["imports"].append({"module": a.name, "level": 0, "line": node.lineno})
        elif isinstance(node, ast.ImportFrom):
            res["imports"].append({"module": node.module or "", "level": node.level or 0, "line": node.lineno})
        elif isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
            res["functions"].append({"name": node.name, "line": node.lineno, "calls": calls_in(node)})
        elif isinstance(node, ast.ClassDef):
            methods = [{"name": m.name, "line": m.lineno}
                       for m in node.body if isinstance(m, (ast.FunctionDef, ast.AsyncFunctionDef))]
            res["classes"].append({"name": node.name, "line": node.lineno, "methods": methods})
    return res

def main():
    data = json.loads(sys.stdin.read())
    out = {"files": {}}
    for item in data["files"]:
        try:
            out["files"][item["path"]] = analyze(item["source"])
        except Exception:
            out["files"][item["path"]] = {"functions": [], "classes": [], "imports": []}
    print(json.dumps(out))

main()
"#;

fn analyze_python_files(files: &[(String, String)]) -> Option<PyRepoAnalysis> {
    if files.is_empty() {
        return Some(PyRepoAnalysis {
            files: BTreeMap::new(),
        });
    }
    let helper = std::env::temp_dir().join(format!(".sk_repo_map_{}.py", std::process::id()));
    std::fs::write(&helper, REPO_MAP_PY).ok()?;
    let payload = serde_json::json!({
        "files": files.iter().map(|(p, s)| serde_json::json!({"path": p, "source": s})).collect::<Vec<_>>()
    })
    .to_string();
    let mut child = std::process::Command::new("python3")
        .arg(&helper)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .ok()?;
    {
        use std::io::Write as _;
        child.stdin.take()?.write_all(payload.as_bytes()).ok()?;
    }
    let output = child.wait_with_output().ok()?;
    let _ = std::fs::remove_file(&helper);
    if !output.status.success() {
        return None;
    }
    serde_json::from_slice(&output.stdout).ok()
}

fn lang_for(path: &str) -> &'static str {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    match ext {
        "py" => "python",
        "rs" => "rust",
        "ts" | "tsx" => "typescript",
        "js" | "jsx" | "mjs" | "cjs" => "javascript",
        "go" => "go",
        "java" => "java",
        "c" | "h" => "c",
        "cpp" | "cc" | "hpp" => "cpp",
        "toml" => "toml",
        "json" => "json",
        "yaml" | "yml" => "yaml",
        "md" => "markdown",
        "txt" => "text",
        "sh" => "shell",
        _ => "other",
    }
}

const REPO_IGNORE: &[&str] = &[
    ".git",
    "__pycache__",
    "node_modules",
    "target",
    ".venv",
    "venv",
    ".mypy_cache",
    ".pytest_cache",
    ".idea",
    ".vscode",
    ".spacekit-code-session",
    "dist",
    "build",
    ".ruff_cache",
];

fn collect_entries(root: &Path, dir: &Path, out: &mut Vec<(String, bool, PathBuf)>) {
    let mut entries: Vec<std::fs::DirEntry> = match std::fs::read_dir(dir) {
        Ok(rd) => rd.filter_map(|e| e.ok()).collect(),
        Err(_) => return,
    };
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || REPO_IGNORE.contains(&name.as_str()) {
            continue;
        }
        let abs = e.path();
        let is_dir = abs.is_dir();
        let rel = abs
            .strip_prefix(root)
            .unwrap_or(&abs)
            .to_string_lossy()
            .replace('\\', "/");
        out.push((rel, is_dir, abs.clone()));
        if is_dir {
            collect_entries(root, &abs, out);
        }
    }
}

fn parent_dir_id(rel: &str) -> String {
    match rel.rfind('/') {
        Some(i) => format!("dir:{}", &rel[..i]),
        None => "dir:.".to_string(),
    }
}

fn dir_components(rel: &str) -> Vec<String> {
    match rel.rfind('/') {
        Some(i) => rel[..i].split('/').map(|s| s.to_string()).collect(),
        None => Vec::new(),
    }
}

/// Resolve a Python import to a repo-relative file path, or `None` if external.
fn resolve_py_import(
    importer_rel: &str,
    module: &str,
    level: usize,
    repo_files: &std::collections::HashSet<String>,
) -> Option<String> {
    let parts: Vec<String> = module
        .split('.')
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();
    let mut bases: Vec<Vec<String>> = Vec::new();
    if level == 0 {
        bases.push(Vec::new()); // repo root (treated as a sys.path entry)
        bases.push(dir_components(importer_rel)); // sibling of importer
    } else {
        let mut comps = dir_components(importer_rel);
        for _ in 0..level.saturating_sub(1) {
            comps.pop();
        }
        bases.push(comps);
    }
    for base in bases {
        let mut path = base;
        path.extend(parts.iter().cloned());
        let joined = path.join("/");
        let joined = joined.trim_start_matches('/').to_string();
        for cand in [format!("{}.py", joined), format!("{}/__init__.py", joined)] {
            let cand = cand.trim_start_matches('/').to_string();
            if repo_files.contains(&cand) {
                return Some(cand);
            }
        }
    }
    None
}

fn build_repo_map(root: &Path) -> Result<RepoMap, Box<dyn Error>> {
    let root_abs = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let root_name = root_abs
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| ".".to_string());

    let mut nodes: Vec<RepoNode> = Vec::new();
    let mut edges: Vec<RepoEdge> = Vec::new();
    let mut languages: BTreeMap<String, usize> = BTreeMap::new();
    let mut dir_count = 0usize;
    let mut file_count = 0usize;

    nodes.push(RepoNode {
        id: "dir:.".to_string(),
        kind: "dir".to_string(),
        name: root_name,
        path: Some(".".to_string()),
        ..Default::default()
    });

    let mut entries: Vec<(String, bool, PathBuf)> = Vec::new();
    collect_entries(&root_abs, &root_abs, &mut entries);

    let mut py_files: Vec<(String, String)> = Vec::new();
    let mut repo_file_set: std::collections::HashSet<String> = std::collections::HashSet::new();

    for (rel, is_dir, abs) in &entries {
        if *is_dir {
            dir_count += 1;
            nodes.push(RepoNode {
                id: format!("dir:{}", rel),
                kind: "dir".to_string(),
                name: Path::new(rel)
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| rel.clone()),
                path: Some(rel.clone()),
                ..Default::default()
            });
            edges.push(RepoEdge {
                etype: "contains".to_string(),
                from: parent_dir_id(rel),
                to: format!("dir:{}", rel),
                module: None,
                external: None,
            });
            continue;
        }
        file_count += 1;
        repo_file_set.insert(rel.clone());
        let lang = lang_for(rel);
        *languages.entry(lang.to_string()).or_insert(0) += 1;
        let bytes = std::fs::read(abs).unwrap_or_default();
        let size = bytes.len() as u64;
        let hash = hex::encode(hash_bytes(&bytes));
        let text = String::from_utf8(bytes).ok();
        let line_count = text.as_ref().map(|t| t.lines().count());
        nodes.push(RepoNode {
            id: format!("file:{}", rel),
            kind: "file".to_string(),
            name: Path::new(rel)
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| rel.clone()),
            path: Some(rel.clone()),
            lang: Some(lang.to_string()),
            size: Some(size),
            lines: line_count,
            hash: Some(hash),
            ..Default::default()
        });
        edges.push(RepoEdge {
            etype: "contains".to_string(),
            from: parent_dir_id(rel),
            to: format!("file:{}", rel),
            module: None,
            external: None,
        });
        if lang == "python" && size < 2_000_000 {
            if let Some(t) = text {
                py_files.push((rel.clone(), t));
            }
        }
    }

    let py_count = py_files.len();
    let mut symbol_count = 0usize;

    if let Some(analysis) = analyze_python_files(&py_files) {
        // First pass: emit symbol nodes + contains edges; build a global
        // simple-name -> symbol-id index for call resolution.
        let mut name_index: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut func_calls: Vec<(String, Vec<String>)> = Vec::new();

        for (rel, pf) in &analysis.files {
            let file_id = format!("file:{}", rel);
            for func in &pf.functions {
                let fid = format!("sym:{}::{}", rel, func.name);
                nodes.push(RepoNode {
                    id: fid.clone(),
                    kind: "function".to_string(),
                    name: func.name.clone(),
                    file: Some(rel.clone()),
                    line: Some(func.line),
                    ..Default::default()
                });
                edges.push(RepoEdge {
                    etype: "contains".to_string(),
                    from: file_id.clone(),
                    to: fid.clone(),
                    module: None,
                    external: None,
                });
                name_index
                    .entry(func.name.clone())
                    .or_default()
                    .push(fid.clone());
                func_calls.push((fid, func.calls.clone()));
                symbol_count += 1;
            }
            for class in &pf.classes {
                let cid = format!("sym:{}::{}", rel, class.name);
                nodes.push(RepoNode {
                    id: cid.clone(),
                    kind: "class".to_string(),
                    name: class.name.clone(),
                    file: Some(rel.clone()),
                    line: Some(class.line),
                    ..Default::default()
                });
                edges.push(RepoEdge {
                    etype: "contains".to_string(),
                    from: file_id.clone(),
                    to: cid.clone(),
                    module: None,
                    external: None,
                });
                name_index
                    .entry(class.name.clone())
                    .or_default()
                    .push(cid.clone());
                symbol_count += 1;
                for m in &class.methods {
                    let mid = format!("sym:{}::{}.{}", rel, class.name, m.name);
                    nodes.push(RepoNode {
                        id: mid.clone(),
                        kind: "method".to_string(),
                        name: m.name.clone(),
                        file: Some(rel.clone()),
                        class: Some(class.name.clone()),
                        line: Some(m.line),
                        ..Default::default()
                    });
                    edges.push(RepoEdge {
                        etype: "contains".to_string(),
                        from: cid.clone(),
                        to: mid,
                        module: None,
                        external: None,
                    });
                    symbol_count += 1;
                }
            }
            // imports
            for imp in &pf.imports {
                let display = format!("{}{}", ".".repeat(imp.level), imp.module);
                match resolve_py_import(rel, &imp.module, imp.level, &repo_file_set) {
                    Some(target) => edges.push(RepoEdge {
                        etype: "imports".to_string(),
                        from: file_id.clone(),
                        to: format!("file:{}", target),
                        module: Some(display),
                        external: Some(false),
                    }),
                    None => edges.push(RepoEdge {
                        etype: "imports".to_string(),
                        from: file_id.clone(),
                        to: format!("module:{}", display),
                        module: Some(display),
                        external: Some(true),
                    }),
                }
            }
        }

        // Second pass: resolved-only call edges (unique simple-name match).
        for (caller, calls) in &func_calls {
            for callee in calls {
                if let Some(ids) = name_index.get(callee) {
                    if ids.len() == 1 && &ids[0] != caller {
                        edges.push(RepoEdge {
                            etype: "calls".to_string(),
                            from: caller.clone(),
                            to: ids[0].clone(),
                            module: None,
                            external: None,
                        });
                    }
                }
            }
        }
    }

    let edge_count = edges.len();
    let generated_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    Ok(RepoMap {
        schema: "spacekit:repo-map:v1".to_string(),
        root: root_abs.to_string_lossy().to_string(),
        generated_at,
        stats: RepoStats {
            dirs: dir_count,
            files: file_count,
            py_files: py_count,
            symbols: symbol_count,
            edges: edge_count,
            languages,
        },
        nodes,
        edges,
    })
}

pub struct RepoMapArgs {
    pub root: PathBuf,
    pub out: Option<PathBuf>,
}

pub fn handle_map(args: &RepoMapArgs) -> Result<(), Box<dyn Error>> {
    if !args.root.is_dir() {
        return Err(format!("not a directory: {}", args.root.display()).into());
    }
    let map = build_repo_map(&args.root)?;
    let json = serde_json::to_string_pretty(&map)?;
    let out = args.out.clone().unwrap_or_else(|| {
        let name = args
            .root
            .canonicalize()
            .ok()
            .and_then(|p| p.file_name().map(|s| s.to_string_lossy().to_string()))
            .unwrap_or_else(|| "repo".to_string());
        PathBuf::from(format!("{}.repo.json", name))
    });
    if let Some(parent) = out.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    std::fs::write(&out, &json)?;
    let langs: Vec<String> = map
        .stats
        .languages
        .iter()
        .map(|(k, v)| format!("{}:{}", k, v))
        .collect();
    println!(
        "{} {} dirs, {} files ({} py), {} symbols, {} edges -> {}",
        "repo-map ->".green().bold(),
        map.stats.dirs,
        map.stats.files,
        map.stats.py_files,
        map.stats.symbols,
        map.stats.edges,
        out.display()
    );
    println!("{} {}", "languages:".dimmed(), langs.join(", ").dimmed());
    Ok(())
}

// ── Application graph (`spacekit agent app`) ─────────────────────────────────
//
// A higher level above single-template construction: an *app recipe* composes
// catalog primitives (design patterns + algorithms) into a runnable multi-file
// scaffold. The recipe graph encodes the mapping
//     app goal  ->  ordered components (role -> template)  ->  wiring (app.py)
// so "I need an app that does X" routes to a recipe, instantiates each component
// from the SAME templates `agent code` uses, and emits a runnable artifact plus
// `app.json` (the application graph: nodes = components, edges = contains /
// wires_to) for inspection and ML.

#[derive(Debug, Clone, Deserialize)]
struct AppStage {
    /// Module/file name for the component (components/<role>.py).
    role: String,
    /// Catalog template id to instantiate for this component.
    template: String,
    #[serde(default)]
    class_name: String,
    #[serde(default)]
    fn_name: String,
    #[serde(default)]
    purpose: String,
}

#[derive(Debug, Clone, Deserialize)]
struct AppRecipe {
    id: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    keywords: Vec<String>,
    #[serde(default)]
    stages: Vec<AppStage>,
    /// The app.py that imports + wires the component classes.
    #[serde(default)]
    entrypoint: String,
}

#[derive(Debug, Deserialize)]
struct AppRecipeFile {
    #[serde(default)]
    app: Vec<AppRecipe>,
}

struct AppLibrary {
    recipes: Vec<AppRecipe>,
}

impl AppLibrary {
    fn load(dir: &Path) -> Result<Self, Box<dyn Error>> {
        if !dir.is_dir() {
            return Err(format!(
                "recipes dir not found: {} (pass --recipes DIR)",
                dir.display()
            )
            .into());
        }
        let mut recipes = Vec::new();
        let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().map(|e| e == "toml").unwrap_or(false))
            .collect();
        entries.sort();
        for path in entries {
            let raw = std::fs::read_to_string(&path)?;
            let parsed: AppRecipeFile =
                toml::from_str(&raw).map_err(|e| format!("parse {}: {}", path.display(), e))?;
            recipes.extend(parsed.app);
        }
        if recipes.is_empty() {
            return Err(format!("no [[app]] recipes under {}", dir.display()).into());
        }
        Ok(Self { recipes })
    }

    /// Rank recipes by keyword overlap with the goal prompt (longest phrase wins).
    fn route(&self, prompt: &str) -> Vec<(&AppRecipe, i64)> {
        let p = prompt.to_ascii_lowercase();
        let mut scored: Vec<(&AppRecipe, i64)> = self
            .recipes
            .iter()
            .map(|r| {
                let score: i64 = r
                    .keywords
                    .iter()
                    .filter(|k| word_match(&p, &k.to_ascii_lowercase()))
                    .map(|k| 10 + k.len() as i64)
                    .sum();
                (r, score)
            })
            .filter(|(_, s)| *s > 0)
            .collect();
        scored.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.id.cmp(&b.0.id)));
        scored
    }
}

/// Searchable vocabulary of a recipe: keyword tokens + id parts + summary words.
fn recipe_vocab(r: &AppRecipe) -> std::collections::HashSet<String> {
    let mut set = std::collections::HashSet::new();
    for k in &r.keywords {
        for w in k.to_ascii_lowercase().split(|c: char| !c.is_alphanumeric()) {
            if w.len() >= 3 {
                set.insert(w.to_string());
            }
        }
    }
    for part in r.id.split('_') {
        if part.len() >= 3 {
            set.insert(part.to_ascii_lowercase());
        }
    }
    for w in r
        .summary
        .to_ascii_lowercase()
        .split(|c: char| !c.is_alphanumeric())
    {
        if w.len() >= 3 {
            set.insert(w.to_string());
        }
    }
    set
}

fn app_no_match_error(apps: &AppLibrary, prompt: &str) -> Box<dyn Error> {
    let p = prompt_tokens(prompt);
    let mut scored: Vec<(&AppRecipe, usize)> = apps
        .recipes
        .iter()
        .map(|r| (r, recipe_vocab(r).intersection(&p).count()))
        .filter(|(_, c)| *c > 0)
        .collect();
    scored.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.id.cmp(&b.0.id)));
    if scored.is_empty() {
        let all: Vec<String> = apps.recipes.iter().map(|r| r.id.clone()).collect();
        return format!(
            "no app recipe matched {:?}.\nAvailable recipes: {}",
            prompt,
            all.join(", ")
        )
        .into();
    }
    let mut msg = format!("no exact app recipe for {:?}. Did you mean:\n", prompt);
    for (r, _) in scored.into_iter().take(3) {
        msg.push_str(&format!("  - {} — {}\n", r.id, r.summary));
    }
    msg.into()
}

fn render_app_readme(recipe: &AppRecipe, lib: &TemplateLibrary) -> String {
    let mut s = format!(
        "# {}\n\n{}\n\n## Architecture\n\n",
        recipe.id, recipe.summary
    );
    s.push_str(
        "Composed from catalog primitives (design patterns + algorithms) — the same \
templates the SpaceKit Python dev session constructs:\n\n",
    );
    s.push_str("| Component | Template | Kind | Purpose |\n");
    s.push_str("| --- | --- | --- | --- |\n");
    for stage in &recipe.stages {
        let kind = lib
            .by_id(&stage.template)
            .map(|t| t.kind.clone())
            .unwrap_or_default();
        s.push_str(&format!(
            "| `components/{}.py` | `{}` | {} | {} |\n",
            stage.role, stage.template, kind, stage.purpose
        ));
    }
    s.push_str("\n## Run\n\n```sh\npython3 app.py\n```\n\n");
    s.push_str("`app.json` holds the application graph (components + wiring) for tooling.\n\n");
    s.push_str("Generated by `spacekit agent app`.\n");
    s
}

fn render_app_graph(recipe: &AppRecipe, lib: &TemplateLibrary) -> serde_json::Value {
    use serde_json::json;
    let app_id = format!("app:{}", recipe.id);
    let mut nodes = vec![json!({
        "id": app_id,
        "kind": "app",
        "label": recipe.id,
        "summary": recipe.summary,
    })];
    let mut edges = Vec::new();
    let mut files = vec!["app.py".to_string()];
    for stage in &recipe.stages {
        let comp_id = format!("component:{}", stage.role);
        let t = lib.by_id(&stage.template);
        let symbol = if !stage.class_name.is_empty() {
            stage.class_name.clone()
        } else {
            stage.fn_name.clone()
        };
        nodes.push(json!({
            "id": comp_id,
            "kind": "component",
            "role": stage.role,
            "template": stage.template,
            "category": t.map(|t| t.category.clone()).unwrap_or_default(),
            "template_kind": t.map(|t| t.kind.clone()).unwrap_or_default(),
            "symbol": symbol,
            "purpose": stage.purpose,
        }));
        edges.push(json!({ "from": app_id, "to": comp_id, "rel": "contains" }));
        files.push(format!("components/{}.py", stage.role));
    }
    // Sequential data-flow wiring between components, in declared order.
    for pair in recipe.stages.windows(2) {
        edges.push(json!({
            "from": format!("component:{}", pair[0].role),
            "to": format!("component:{}", pair[1].role),
            "rel": "wires_to",
        }));
    }
    json!({
        "schema": "spacekit.app/1",
        "app": { "id": recipe.id, "summary": recipe.summary },
        "nodes": nodes,
        "edges": edges,
        "files": files,
    })
}

pub struct AppArgs {
    pub prompt: Option<String>,
    pub recipes: PathBuf,
    pub templates: PathBuf,
    pub out: Option<PathBuf>,
    pub run: bool,
}

pub fn handle_app(args: &AppArgs) -> Result<(), Box<dyn Error>> {
    let lib = TemplateLibrary::load(&args.templates)?;
    let apps = AppLibrary::load(&args.recipes)?;
    let prompt = args
        .prompt
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .ok_or("pass --prompt \"...\" describing the app to build")?;

    let ranked = apps.route(prompt);
    if ranked.is_empty() {
        return Err(app_no_match_error(&apps, prompt));
    }
    println!("{} {:?}", "app goal:".dimmed(), prompt);
    for (r, score) in ranked.iter().take(3) {
        println!("  {:>4}  {} — {}", score, r.id.cyan(), r.summary.dimmed());
    }
    let recipe = ranked[0].0;
    println!("{} {}", "recipe ->".green().bold(), recipe.id.cyan().bold());

    let out = args
        .out
        .clone()
        .unwrap_or_else(|| PathBuf::from(&recipe.id));
    let comp_dir = out.join("components");
    std::fs::create_dir_all(&comp_dir)?;
    std::fs::write(comp_dir.join("__init__.py"), "")?;

    for stage in &recipe.stages {
        let t = lib.by_id(&stage.template).ok_or_else(|| {
            format!(
                "recipe '{}' stage '{}' references unknown template '{}'",
                recipe.id, stage.role, stage.template
            )
        })?;
        let mut values = default_values(t);
        if t.param("class_name") && !stage.class_name.is_empty() {
            values.insert("class_name".into(), stage.class_name.clone());
        }
        if t.param("fn_name") && !stage.fn_name.is_empty() {
            values.insert("fn_name".into(), stage.fn_name.clone());
        }
        let module = format_code(&instantiate(t, &values));
        let path = comp_dir.join(format!("{}.py", stage.role));
        std::fs::write(&path, &module)?;
        println!(
            "  {} components/{}.py  ({})",
            "+".green(),
            stage.role,
            stage.template.dimmed()
        );
    }

    let entry = format!("{}\n", recipe.entrypoint.trim_matches('\n'));
    let app_py = out.join("app.py");
    std::fs::write(&app_py, format_code(&entry))?;
    std::fs::write(out.join("README.md"), render_app_readme(recipe, &lib))?;
    let graph = render_app_graph(recipe, &lib);
    std::fs::write(out.join("app.json"), serde_json::to_string_pretty(&graph)?)?;

    println!(
        "{} {} ({} components) -> {}",
        "app ->".green().bold(),
        recipe.id,
        recipe.stages.len(),
        out.display()
    );

    if args.run {
        println!("{}", "running app.py ...".dimmed());
        let res = run_python_file(&app_py, &out)?;
        if !res.stdout.trim().is_empty() {
            println!("{}", res.stdout.trim_end());
        }
        if res.ok {
            println!("{}", "app ran successfully".green().bold());
        } else {
            if !res.stderr.trim().is_empty() {
                eprintln!("{}", res.stderr.trim_end().red());
            }
            return Err("app failed to run".into());
        }
    } else {
        println!(
            "{} cd {} && python3 app.py",
            "next:".dimmed(),
            out.display()
        );
    }
    Ok(())
}

// ── Knowledge base + feature decomposition planner (`spacekit agent plan`) ────
//
// The reasoning layer above app recipes. A recipe is a *fixed* archetype; the
// planner is *open*: it takes an arbitrary feature request ("we need a module
// that does X"), DECOMPOSES it into known sub-problems via a knowledge base
// (need-phrase -> algorithm/pattern building block, with rationale + Big-O), and
// emits a module FILE GRAPH (`plan.json`: files, the symbols/functions inside
// them, and edges) that --scaffold can materialize. Premise: most application
// sub-problems are already solved by a known algorithm or design pattern.

#[derive(Debug, Clone, Deserialize)]
struct Capability {
    id: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    need: Vec<String>,
    template: String,
    #[serde(default)]
    role: String,
    #[serde(default)]
    rationale: String,
    #[serde(default)]
    complexity: String,
}

#[derive(Debug, Deserialize)]
struct KnowledgeBaseFile {
    #[serde(default)]
    capability: Vec<Capability>,
}

struct KnowledgeBase {
    caps: Vec<Capability>,
}

/// Light stem: lowercase a token and strip a trailing plural "s" (keeping "ss"),
/// so "caches"->"cache", "tasks"->"task", "validates"->"validate". Deliberately
/// minimal — plurals are the dominant morphological gap in feature phrasing.
fn stem(word: &str) -> String {
    let w = word.to_ascii_lowercase();
    if w.len() > 3 && w.ends_with('s') && !w.ends_with("ss") {
        w[..w.len() - 1].to_string()
    } else {
        w
    }
}

fn prompt_stem_set(prompt: &str) -> std::collections::HashSet<String> {
    prompt
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() >= 3)
        .map(stem)
        .collect()
}

const NEED_STOPWORDS: &[&str] = &["the", "and", "for", "with", "that", "into", "through"];

/// A need phrase matches when every content token (stemmed) is present in the
/// prompt's stem set.
fn need_matches(prompt_stems: &std::collections::HashSet<String>, need: &str) -> bool {
    let tokens: Vec<String> = need
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() >= 3 && !NEED_STOPWORDS.contains(t))
        .map(stem)
        .collect();
    !tokens.is_empty() && tokens.iter().all(|t| prompt_stems.contains(t))
}

impl KnowledgeBase {
    fn load(path: &Path) -> Result<Self, Box<dyn Error>> {
        if !path.is_file() {
            return Err(format!(
                "knowledge base not found: {} (pass --kb FILE)",
                path.display()
            )
            .into());
        }
        let raw = std::fs::read_to_string(path)?;
        let parsed: KnowledgeBaseFile =
            toml::from_str(&raw).map_err(|e| format!("parse {}: {}", path.display(), e))?;
        if parsed.capability.is_empty() {
            return Err(format!("no [[capability]] entries in {}", path.display()).into());
        }
        Ok(Self {
            caps: parsed.capability,
        })
    }

    /// Capabilities whose need-phrases appear in the prompt, longest-match first.
    /// Matching is stem-aware (plurals/verb forms) so "caches results" hits the
    /// "cache" need and "validates input" hits "validate".
    fn decompose(&self, prompt: &str) -> Vec<(&Capability, usize)> {
        let stems = prompt_stem_set(prompt);
        let mut hits: Vec<(&Capability, usize)> = Vec::new();
        for cap in &self.caps {
            let best = cap
                .need
                .iter()
                .filter(|n| need_matches(&stems, n))
                .map(|n| n.len())
                .max();
            if let Some(len) = best {
                hits.push((cap, len));
            }
        }
        hits.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.id.cmp(&b.0.id)));
        hits
    }

    fn vocab(cap: &Capability) -> std::collections::HashSet<String> {
        let mut set = std::collections::HashSet::new();
        for n in &cap.need {
            for w in n.to_ascii_lowercase().split(|c: char| !c.is_alphanumeric()) {
                if w.len() >= 3 {
                    set.insert(w.to_string());
                }
            }
        }
        for w in cap
            .title
            .to_ascii_lowercase()
            .split(|c: char| !c.is_alphanumeric())
        {
            if w.len() >= 3 {
                set.insert(w.to_string());
            }
        }
        set
    }
}

/// The default public symbol a template exposes, and whether it is a class/function.
fn symbol_of(t: &Template) -> (String, &'static str) {
    for p in &t.params {
        if p.name == "class_name" {
            return (p.default.clone(), "class");
        }
        if p.name == "fn_name" {
            return (p.default.clone(), "function");
        }
    }
    (
        t.id.strip_prefix("algo_")
            .or_else(|| t.id.strip_prefix("pattern_"))
            .unwrap_or(&t.id)
            .to_string(),
        "module",
    )
}

/// One resolved building block in the plan.
struct PlanItem<'a> {
    role: String,
    template: &'a Template,
    title: String,
    rationale: String,
    complexity: String,
    symbol: String,
    symbol_kind: &'static str,
    source: &'static str,
}

fn slugify_module(prompt: &str, explicit: Option<&str>) -> String {
    if let Some(m) = explicit {
        return m.to_string();
    }
    const STOP: &[&str] = &[
        "the", "a", "an", "we", "need", "want", "that", "does", "for", "with", "and", "build",
        "make", "create", "module", "feature", "python", "app", "system", "service",
    ];
    let words: Vec<String> = prompt
        .to_ascii_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 3 && !STOP.contains(w))
        .take(3)
        .map(|w| w.to_string())
        .collect();
    if words.is_empty() {
        "feature_module".to_string()
    } else {
        format!("{}_module", words.join("_"))
    }
}

fn resolve_plan<'a>(
    kb: &'a KnowledgeBase,
    lib: &'a TemplateLibrary,
    args: &CodeArgs,
    prompt: &str,
) -> Vec<PlanItem<'a>> {
    let mut items: Vec<PlanItem<'a>> = Vec::new();
    let mut used_templates: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut used_roles: std::collections::HashSet<String> = std::collections::HashSet::new();

    let mut push_item = |role: String,
                         t: &'a Template,
                         title: String,
                         rationale: String,
                         complexity: String,
                         source: &'static str,
                         items: &mut Vec<PlanItem<'a>>| {
        if !used_templates.insert(t.id.clone()) {
            return;
        }
        let mut role = role;
        while !used_roles.insert(role.clone()) {
            role.push('_');
        }
        let (symbol, symbol_kind) = symbol_of(t);
        items.push(PlanItem {
            role,
            template: t,
            title,
            rationale,
            complexity,
            symbol,
            symbol_kind,
            source,
        });
    };

    for (cap, _) in kb.decompose(prompt) {
        if let Some(t) = lib.by_id(&cap.template) {
            let role = if cap.role.is_empty() {
                symbol_of(t).0.to_ascii_lowercase()
            } else {
                cap.role.clone()
            };
            push_item(
                role,
                t,
                cap.title.clone(),
                cap.rationale.clone(),
                cap.complexity.clone(),
                "kb",
                &mut items,
            );
        }
    }

    // If the KB matched nothing, fall back to direct template routing so a bare
    // "give me dijkstra" still produces a (single-block) plan.
    if items.is_empty() {
        for (t, rank) in lib.route_ranked(&args.graph, prompt).into_iter().take(3) {
            if rank < 10 {
                continue;
            }
            let role = symbol_of(t).0.to_ascii_lowercase();
            push_item(
                role,
                t,
                t.summary.clone(),
                String::new(),
                String::new(),
                "route",
                &mut items,
            );
        }
    }
    items
}

fn plan_no_match_error(kb: &KnowledgeBase, prompt: &str) -> Box<dyn Error> {
    let p = prompt_tokens(prompt);
    let mut scored: Vec<(&Capability, usize)> = kb
        .caps
        .iter()
        .map(|c| (c, KnowledgeBase::vocab(c).intersection(&p).count()))
        .filter(|(_, n)| *n > 0)
        .collect();
    scored.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.id.cmp(&b.0.id)));
    if scored.is_empty() {
        return format!(
            "couldn't decompose {:?} into known capabilities.\nDescribe the feature's needs (e.g. \"cache results\", \"schedule tasks by priority\", \"validate input\"). See data/knowledge_base.toml for the vocabulary.",
            prompt
        )
        .into();
    }
    let mut msg = format!(
        "no capability directly matched {:?}. Closest capabilities:\n",
        prompt
    );
    for (c, _) in scored.into_iter().take(4) {
        msg.push_str(&format!(
            "  - {} ({}) — {}\n",
            c.title, c.template, c.rationale
        ));
    }
    msg.into()
}

fn render_plan_graph(module: &str, prompt: &str, items: &[PlanItem]) -> serde_json::Value {
    use serde_json::json;
    let mod_id = format!("module:{}", module);
    let facade_id = format!("facade:{}", module);
    let mut nodes = vec![
        json!({ "id": mod_id, "kind": "module", "label": module, "feature": prompt }),
        json!({ "id": facade_id, "kind": "facade", "path": format!("{}/__init__.py", module) }),
    ];
    let mut edges = vec![json!({ "from": mod_id, "to": facade_id, "rel": "contains" })];
    let mut files = vec![format!("{}/__init__.py", module)];
    for it in items {
        let file_id = format!("file:{}", it.role);
        let sym_id = format!("symbol:{}.{}", it.role, it.symbol);
        nodes.push(json!({
            "id": file_id,
            "kind": "file",
            "path": format!("{}/{}.py", module, it.role),
            "capability": it.title,
            "template": it.template.id,
            "rationale": it.rationale,
            "complexity": it.complexity,
            "source": it.source,
        }));
        nodes.push(json!({
            "id": sym_id,
            "kind": it.symbol_kind,
            "name": it.symbol,
            "file": format!("{}/{}.py", module, it.role),
        }));
        edges.push(json!({ "from": mod_id, "to": file_id, "rel": "contains" }));
        edges.push(json!({ "from": file_id, "to": sym_id, "rel": "defines" }));
        edges.push(json!({ "from": facade_id, "to": sym_id, "rel": "exports" }));
        files.push(format!("{}/{}.py", module, it.role));
    }
    json!({
        "schema": "spacekit.plan/1",
        "module": module,
        "feature": prompt,
        "capabilities": items.iter().map(|it| json!({
            "role": it.role,
            "title": it.title,
            "template": it.template.id,
            "symbol": it.symbol,
            "rationale": it.rationale,
            "complexity": it.complexity,
            "source": it.source,
        })).collect::<Vec<_>>(),
        "nodes": nodes,
        "edges": edges,
        "files": files,
    })
}

fn render_plan_readme(module: &str, prompt: &str, items: &[PlanItem]) -> String {
    let mut s = format!(
        "# {}\n\nFeature: {}\n\n## Decomposition\n\n",
        module, prompt
    );
    s.push_str("Each sub-problem maps to a known algorithm/design pattern:\n\n");
    s.push_str("| File | Symbol | Building block | Why | Complexity |\n");
    s.push_str("| --- | --- | --- | --- | --- |\n");
    for it in items {
        s.push_str(&format!(
            "| `{}/{}.py` | `{}` | `{}` | {} | {} |\n",
            module, it.role, it.symbol, it.template.id, it.rationale, it.complexity
        ));
    }
    s.push_str("\n## Use\n\n```python\n");
    for it in items {
        s.push_str(&format!("from {} import {}\n", module, it.symbol));
    }
    s.push_str(
        "```\n\nGenerated by `spacekit agent plan`. `plan.json` holds the module file graph.\n",
    );
    s
}

pub struct PlanArgs {
    pub prompt: Option<String>,
    pub kb: PathBuf,
    pub templates: PathBuf,
    pub graph: PathBuf,
    pub module: Option<String>,
    pub out: Option<PathBuf>,
    pub scaffold: bool,
}

pub fn handle_plan(args: &PlanArgs) -> Result<(), Box<dyn Error>> {
    let lib = TemplateLibrary::load(&args.templates)?;
    let kb = KnowledgeBase::load(&args.kb)?;
    // Reuse CodeArgs only for the graph path inside resolve_plan's fallback route.
    let code_args = CodeArgs {
        prompt: args.prompt.clone(),
        templates: args.templates.clone(),
        graph: args.graph.clone(),
        out: None,
        workdir: PathBuf::from("."),
        run: false,
        file: None,
        session: false,
    };
    let prompt = args
        .prompt
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .ok_or("pass --prompt \"we need a module that ...\"")?;

    let items = resolve_plan(&kb, &lib, &code_args, prompt);
    if items.is_empty() {
        return Err(plan_no_match_error(&kb, prompt));
    }
    let module = slugify_module(prompt, args.module.as_deref());

    println!("{} {:?}", "feature:".dimmed(), prompt);
    println!(
        "{} {} ({} sub-problems)",
        "module ->".green().bold(),
        module.cyan().bold(),
        items.len()
    );
    for it in &items {
        println!(
            "  {} {:<12} {} {}  [{}]",
            "·".dimmed(),
            it.role.cyan(),
            it.template.id.dimmed(),
            it.complexity.dimmed(),
            it.title,
        );
        if !it.rationale.is_empty() {
            println!("      {}", it.rationale.dimmed());
        }
    }

    let graph = render_plan_graph(&module, prompt, &items);
    let plan_json = serde_json::to_string_pretty(&graph)?;

    if !args.scaffold {
        let out = args
            .out
            .clone()
            .unwrap_or_else(|| PathBuf::from(format!("{}.plan.json", module)));
        if let Some(parent) = out.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        std::fs::write(&out, &plan_json)?;
        println!("{} {}", "plan ->".green().bold(), out.display());
        println!(
            "{} re-run with --scaffold to materialize the module",
            "next:".dimmed()
        );
        return Ok(());
    }

    // Scaffold the module file graph.
    let base = args.out.clone().unwrap_or_else(|| PathBuf::from("."));
    let mod_dir = base.join(&module);
    std::fs::create_dir_all(&mod_dir)?;
    for it in &items {
        let values = default_values(it.template);
        let code = format_code(&instantiate(it.template, &values));
        std::fs::write(mod_dir.join(format!("{}.py", it.role)), &code)?;
        println!("  {} {}/{}.py", "+".green(), module, it.role);
    }
    // Facade: re-export every building block as the module's public surface.
    let mut facade = String::from("\"\"\"Auto-generated feature module facade.\"\"\"\n\n");
    for it in &items {
        facade.push_str(&format!("from .{} import {}\n", it.role, it.symbol));
    }
    facade.push_str(&format!(
        "\n__all__ = [{}]\n",
        items
            .iter()
            .map(|it| format!("\"{}\"", it.symbol))
            .collect::<Vec<_>>()
            .join(", ")
    ));
    std::fs::write(mod_dir.join("__init__.py"), facade)?;
    std::fs::write(mod_dir.join("plan.json"), &plan_json)?;
    std::fs::write(
        mod_dir.join("README.md"),
        render_plan_readme(&module, prompt, &items),
    )?;

    // Import smoke test: prove the assembled module loads.
    let smoke = base.join(format!(".plan_smoke_{}.py", module));
    std::fs::write(
        &smoke,
        format!(
            "import {}\nprint('module ok:', {}.__all__)\n",
            module, module
        ),
    )?;
    let res = run_python_file(&smoke, &base)?;
    let _ = std::fs::remove_file(&smoke);
    println!(
        "{} {} ({} files) -> {}",
        "module ->".green().bold(),
        module,
        items.len() + 1,
        mod_dir.display()
    );
    if res.ok {
        println!("{} {}", "import ok:".green().bold(), res.stdout.trim());
    } else {
        eprintln!("{}\n{}", "module import failed".red(), res.stderr.trim());
        return Err("scaffolded module failed to import".into());
    }
    Ok(())
}
