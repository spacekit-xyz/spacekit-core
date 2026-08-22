//! Spec-driven SDK generation (the "Stainless model" for the Growformer architecture).
//!
//! Pipeline:  OpenAPI spec  ->  typed `SpecModel`  ->  language emitter.
//!
//! Supported emitters: **Python**, **TypeScript**, **Rust** (same IR).
//!
//! Each SDK includes runtime primitives: auth, retries/backoff, timeouts, typed
//! errors, auto-pagination, SSE streaming, webhook verification, multipart uploads.
//!
//! Schema IR supports enums, nullable, oneOf/anyOf unions, and allOf composition.
//!
//! Incremental regeneration tracks generated files in `.sdkgen-manifest.json`
//! (SHA-256 per file) with hand-edit protection (`--plan`, `--prune`, `--force`).
//!
//! Documentation: `spacekit-cli/documentation/AGENT_SDK_GENERATION.md`
//!
//! The neural layer (Growformer) is intentionally *not* on the critical path
//! here — generation is reproducible from the spec. Growformer hooks in later
//! for naming/pagination heuristics where the spec is ambiguous.

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

// ---------------------------------------------------------------------------
// CLI args
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct SdkArgs {
    pub spec: PathBuf,
    pub out: Option<PathBuf>,
    pub package: Option<String>,
    pub lang: String,
    pub check: bool,
    pub plan: bool,
    pub prune: bool,
    pub force: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Lang {
    Python,
    TypeScript,
    Rust,
}

// ---------------------------------------------------------------------------
// Typed spec model (the IR every emitter consumes)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum TypeRef {
    Str,
    Int,
    Float,
    Bool,
    Any,
    Null,
    Ref(String),
    Array(Box<TypeRef>),
    Map(Box<TypeRef>),
    Enum(Vec<String>),   // string enum
    Union(Vec<TypeRef>), // oneOf/anyOf or nullable
}

#[derive(Debug, Clone)]
struct Field {
    name: String,
    ty: TypeRef,
    required: bool,
}

#[derive(Debug, Clone)]
struct Schema {
    name: String,
    fields: Vec<Field>,
}

#[derive(Debug, Clone, PartialEq)]
enum Loc {
    Path,
    Query,
    Header,
}

#[derive(Debug, Clone)]
struct Param {
    name: String,
    loc: Loc,
    ty: TypeRef,
    required: bool,
}

#[derive(Debug, Clone)]
enum PageKind {
    Cursor,
    Offset,
}

#[derive(Debug, Clone)]
struct Pagination {
    kind: PageKind,
    data_field: String,
    next_field: Option<String>, // cursor pagination
    cursor_param: Option<String>,
    offset_param: Option<String>,
}

#[derive(Debug, Clone)]
struct FormField {
    name: String,
    is_file: bool,
    required: bool,
    ty: TypeRef,
}

#[derive(Debug, Clone)]
struct Method {
    name: String,
    http: String,
    path: String,
    summary: Option<String>,
    params: Vec<Param>,
    body: Option<(TypeRef, bool)>, // (type, required) — JSON body
    response: Option<TypeRef>,
    pagination: Option<Pagination>,
    streaming: bool,                   // response is text/event-stream (SSE)
    multipart: Option<Vec<FormField>>, // multipart/form-data request body
}

#[derive(Debug, Clone)]
struct Resource {
    attr: String,  // snake_case attribute on the client
    class: String, // PascalCase class name
    methods: Vec<Method>,
}

#[derive(Debug, Clone)]
enum Auth {
    None,
    Bearer,
    ApiKeyHeader(String),
}

#[derive(Debug, Clone)]
struct SpecModel {
    title: String,
    version: String,
    base_url: String,
    auth: Auth,
    schemas: Vec<Schema>,
    aliases: Vec<(String, TypeRef)>, // named enums / unions / scalar types
    resources: Vec<Resource>,
    has_webhooks: bool,
}

enum SchemaDef {
    Object(Schema),
    Alias(String, TypeRef),
}

#[path = "sdkgen_rust.rs"]
mod sdkgen_rust;

#[path = "openapp.rs"]
pub mod openapp;

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub fn handle_sdk(args: &SdkArgs) -> Result<(), Box<dyn Error>> {
    let lang = match args.lang.to_lowercase().as_str() {
        "python" | "py" => Lang::Python,
        "typescript" | "ts" => Lang::TypeScript,
        "rust" | "rs" => Lang::Rust,
        other => {
            return Err(
                format!("language '{other}' not supported (use: python, typescript, rust)").into(),
            )
        }
    };

    let raw = fs::read_to_string(&args.spec)
        .map_err(|e| format!("cannot read spec {}: {e}", args.spec.display()))?;
    let is_json = args
        .spec
        .extension()
        .map(|e| e.eq_ignore_ascii_case("json"))
        .unwrap_or(false);
    let doc: Value = if is_json {
        serde_json::from_str(&raw).map_err(|e| format!("invalid JSON spec: {e}"))?
    } else {
        // serde_yaml deserializes YAML directly into a serde_json::Value.
        serde_yaml::from_str(&raw).map_err(|e| format!("invalid YAML spec: {e}"))?
    };

    let model = parse_openapi(&doc)?;

    let package = args
        .package
        .clone()
        .unwrap_or_else(|| sanitize_pkg(&model.title));
    let out_root = args
        .out
        .clone()
        .unwrap_or_else(|| PathBuf::from(format!("./{package}_sdk")));
    let pkg_dir = out_root.join(&package);

    let files = match lang {
        Lang::Python => emit_python(&model, &package),
        Lang::TypeScript => emit_typescript(&model, &package),
        Lang::Rust => sdkgen_rust::emit_rust(&model, &package),
    };

    // Assemble the full emitted set, keyed by path relative to `out_root`.
    let mut emitted: BTreeMap<String, String> = BTreeMap::new();
    for (rel, contents) in &files {
        emitted.insert(format!("{package}/{rel}"), contents.clone());
    }
    emitted.insert(
        "README.md".to_string(),
        render_readme(&model, &package, lang),
    );

    let n_models = model.schemas.len();
    let n_methods: usize = model.resources.iter().map(|r| r.methods.len()).sum();
    let n_paginated: usize = model
        .resources
        .iter()
        .flat_map(|r| &r.methods)
        .filter(|m| m.pagination.is_some())
        .count();

    let verb = if args.plan { "Planned" } else { "Generated" };
    println!("{verb} {} SDK -> {}", model.title, pkg_dir.display());
    println!(
        "  {} resources · {} methods ({} paginated) · {} models · auth: {}",
        model.resources.len(),
        n_methods,
        n_paginated,
        n_models,
        match &model.auth {
            Auth::None => "none".to_string(),
            Auth::Bearer => "bearer".to_string(),
            Auth::ApiKeyHeader(h) => format!("api-key ({h})"),
        }
    );

    // Incremental regeneration: diff against the previous manifest + disk state,
    // write only what changed (protecting hand-edited files), then report.
    let plan = plan_regen(&out_root, &emitted, args.force)?;
    let outcome = apply_regen(&out_root, &emitted, &plan, RegenOpts::from(args))?;

    if args.plan {
        return Ok(());
    }
    if outcome.conflicts > 0 {
        eprintln!(
            "  {} file(s) were hand-edited since last generation and left untouched; \
             re-run with --force to overwrite.",
            outcome.conflicts
        );
    }

    if args.check {
        match lang {
            Lang::Python => match import_test(&out_root, &package) {
                Ok(()) => println!("  check: OK (python3 imported `{package}`)"),
                Err(e) => {
                    eprintln!("  check: FAILED\n{e}");
                    return Err("generated package failed python import".into());
                }
            },
            Lang::TypeScript => {
                match ts_typecheck(&pkg_dir) {
                    Ok(true) => println!("  check: OK (tsc --noEmit passed)"),
                    Ok(false) => {
                        println!("  check: SKIPPED (tsc not found on PATH; emitted code not type-checked)")
                    }
                    Err(e) => {
                        eprintln!("  check: FAILED (tsc reported errors)\n{e}");
                        return Err("generated package failed tsc typecheck".into());
                    }
                }
            }
            Lang::Rust => match sdkgen_rust::rs_check(&pkg_dir) {
                Ok(true) => println!("  check: OK (cargo check passed)"),
                Ok(false) => {
                    println!("  check: SKIPPED (cargo not found on PATH; emitted crate not type-checked)")
                }
                Err(e) => {
                    eprintln!("  check: FAILED (cargo reported errors)\n{e}");
                    return Err("generated crate failed cargo check".into());
                }
            },
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Incremental regeneration (manifest-tracked, hand-edit aware)
// ---------------------------------------------------------------------------

const MANIFEST_NAME: &str = ".sdkgen-manifest.json";

#[derive(Debug, Clone, Copy, PartialEq)]
enum FileStatus {
    Added,     // not on disk -> write
    Modified,  // tracked & regenerated since last gen -> overwrite
    Unchanged, // identical to emitted -> skip
    Conflict,  // hand-edited since last gen -> skip unless --force
    Removed,   // tracked previously, no longer emitted -> orphan
}

#[derive(Debug)]
struct PlanEntry {
    rel: String,
    status: FileStatus,
    /// For `Removed`, whether the on-disk file still matches what we generated
    /// (safe to prune) vs. was hand-edited (prune would lose work).
    prunable: bool,
}

struct RegenOutcome {
    conflicts: usize,
}

/// Realization-neutral knobs that drive the incremental writer. Both the SDK
/// command (`agent sdk`) and the webapp command (`agent app`) feed this in.
// NOTE: `force` is consumed directly by `plan_regen` (it decides whether
// hand-edited files become conflicts), so it is intentionally not stored here.
#[derive(Debug, Clone, Copy)]
struct RegenOpts {
    plan: bool,
    prune: bool,
}

impl From<&SdkArgs> for RegenOpts {
    fn from(a: &SdkArgs) -> Self {
        RegenOpts {
            plan: a.plan,
            prune: a.prune,
        }
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(bytes);
    let digest = h.finalize();
    let mut s = String::with_capacity(64);
    for b in digest {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

fn load_manifest(out_root: &PathBuf) -> (bool, BTreeMap<String, String>) {
    let path = out_root.join(MANIFEST_NAME);
    let raw = match fs::read_to_string(&path) {
        Ok(r) => r,
        Err(_) => return (false, BTreeMap::new()),
    };
    let doc: Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(_) => return (false, BTreeMap::new()),
    };
    let mut map = BTreeMap::new();
    if let Some(files) = doc.get("files").and_then(|v| v.as_object()) {
        for (k, v) in files {
            if let Some(h) = v.as_str() {
                map.insert(k.clone(), h.to_string());
            }
        }
    }
    (true, map)
}

/// Compute the regeneration plan by diffing emitted content against the prior
/// manifest and the current on-disk state.
fn plan_regen(
    out_root: &PathBuf,
    emitted: &BTreeMap<String, String>,
    force: bool,
) -> Result<Vec<PlanEntry>, Box<dyn Error>> {
    let (had_manifest, prev) = load_manifest(out_root);
    let mut entries = Vec::new();

    for (rel, contents) in emitted {
        let new_hash = sha256_hex(contents.as_bytes());
        let abs = out_root.join(rel);
        let disk = fs::read(&abs).ok();
        let status = match disk {
            None => FileStatus::Added,
            Some(bytes) => {
                let disk_hash = sha256_hex(&bytes);
                if disk_hash == new_hash {
                    FileStatus::Unchanged
                } else {
                    match prev.get(rel) {
                        // Tracked and untouched since last gen -> safe to overwrite.
                        Some(ph) if *ph == disk_hash => FileStatus::Modified,
                        // Tracked but hand-edited -> conflict (unless forced).
                        Some(_) => {
                            if force {
                                FileStatus::Modified
                            } else {
                                FileStatus::Conflict
                            }
                        }
                        // Untracked existing file: overwrite on a fresh (manifest-less)
                        // tree; treat as a conflict once we are tracking state.
                        None => {
                            if had_manifest && !force {
                                FileStatus::Conflict
                            } else {
                                FileStatus::Modified
                            }
                        }
                    }
                }
            }
        };
        entries.push(PlanEntry {
            rel: rel.clone(),
            status,
            prunable: true,
        });
    }

    // Orphans: previously-tracked files no longer emitted.
    for (rel, ph) in &prev {
        if emitted.contains_key(rel) {
            continue;
        }
        let abs = out_root.join(rel);
        match fs::read(&abs) {
            Ok(bytes) => {
                let prunable = sha256_hex(&bytes) == *ph;
                entries.push(PlanEntry {
                    rel: rel.clone(),
                    status: FileStatus::Removed,
                    prunable,
                });
            }
            Err(_) => {} // already gone from disk
        }
    }

    entries.sort_by(|a, b| a.rel.cmp(&b.rel));
    Ok(entries)
}

/// Execute the plan (unless `--plan`), persist the manifest, and print a summary.
fn apply_regen(
    out_root: &PathBuf,
    emitted: &BTreeMap<String, String>,
    plan: &[PlanEntry],
    opts: RegenOpts,
) -> Result<RegenOutcome, Box<dyn Error>> {
    let dry = opts.plan;
    let mut counts = [0usize; 5]; // Added, Modified, Unchanged, Conflict, Removed

    for e in plan {
        let (sym, idx) = match e.status {
            FileStatus::Added => ("+", 0),
            FileStatus::Modified => ("~", 1),
            FileStatus::Unchanged => ("=", 2),
            FileStatus::Conflict => ("!", 3),
            FileStatus::Removed => ("-", 4),
        };
        counts[idx] += 1;

        // Only surface noise-free lines: skip Unchanged unless planning.
        if matches!(e.status, FileStatus::Unchanged) && !dry {
            continue;
        }

        let note = match e.status {
            FileStatus::Conflict => "  (hand-edited; kept — use --force to overwrite)",
            FileStatus::Removed if e.prunable && opts.prune => "  (pruned)",
            FileStatus::Removed if e.prunable => "  (orphan; use --prune to delete)",
            FileStatus::Removed => "  (orphan, hand-edited; kept)",
            _ => "",
        };
        println!("    {sym} {}{note}", e.rel);

        if dry {
            continue;
        }
        match e.status {
            FileStatus::Added | FileStatus::Modified => {
                let abs = out_root.join(&e.rel);
                if let Some(parent) = abs.parent() {
                    fs::create_dir_all(parent)?;
                }
                if let Some(contents) = emitted.get(&e.rel) {
                    fs::write(&abs, contents)?;
                }
            }
            FileStatus::Removed if e.prunable && opts.prune => {
                let abs = out_root.join(&e.rel);
                let _ = fs::remove_file(&abs);
                prune_empty_dirs(out_root, abs.parent());
            }
            _ => {}
        }
    }

    println!(
        "  {} added · {} modified · {} unchanged · {} conflicts · {} orphaned",
        counts[0], counts[1], counts[2], counts[3], counts[4]
    );

    if !dry {
        write_manifest(out_root, emitted)?;
    }

    Ok(RegenOutcome {
        conflicts: counts[3],
    })
}

/// Remove now-empty directories left by pruning, walking up to (but not past) `out_root`.
fn prune_empty_dirs(out_root: &PathBuf, mut dir: Option<&std::path::Path>) {
    while let Some(d) = dir {
        if d == out_root || !d.starts_with(out_root) {
            break;
        }
        let is_empty = match fs::read_dir(d) {
            Ok(mut entries) => entries.next().is_none(),
            Err(_) => false,
        };
        if !is_empty || fs::remove_dir(d).is_err() {
            break;
        }
        dir = d.parent();
    }
}

fn write_manifest(
    out_root: &PathBuf,
    emitted: &BTreeMap<String, String>,
) -> Result<(), Box<dyn Error>> {
    let mut files = serde_json::Map::new();
    for (rel, contents) in emitted {
        files.insert(rel.clone(), Value::String(sha256_hex(contents.as_bytes())));
    }
    let doc = serde_json::json!({
        "version": 1,
        "generator": "spacekit agent sdk",
        "files": Value::Object(files),
    });
    fs::create_dir_all(out_root)?;
    fs::write(
        out_root.join(MANIFEST_NAME),
        serde_json::to_string_pretty(&doc)?,
    )?;
    Ok(())
}

/// Best-effort TypeScript typecheck. `Ok(true)` = passed, `Ok(false)` = no tsc
/// available (skipped), `Err` = tsc ran and reported errors.
fn ts_typecheck(pkg_dir: &PathBuf) -> Result<bool, String> {
    match Command::new("tsc")
        .args(["--noEmit", "-p", "tsconfig.json"])
        .current_dir(pkg_dir)
        .output()
    {
        Ok(out) if out.status.success() => Ok(true),
        Ok(out) => Err(format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )),
        Err(_) => Ok(false), // tsc not installed
    }
}

fn import_test(out_root: &PathBuf, package: &str) -> Result<(), String> {
    let output = Command::new("python3")
        .arg("-c")
        .arg(format!("import {package}"))
        .current_dir(out_root)
        .output()
        .map_err(|e| format!("failed to run python3: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

// ---------------------------------------------------------------------------
// Front-end: OpenAPI 3.x -> SpecModel
// ---------------------------------------------------------------------------

fn parse_openapi(doc: &Value) -> Result<SpecModel, Box<dyn Error>> {
    let info = doc.get("info");
    let title = info
        .and_then(|i| i.get("title"))
        .and_then(|v| v.as_str())
        .unwrap_or("api")
        .to_string();
    let version = info
        .and_then(|i| i.get("version"))
        .and_then(|v| v.as_str())
        .unwrap_or("0.0.0")
        .to_string();

    let base_url = doc
        .get("servers")
        .and_then(|s| s.as_array())
        .and_then(|a| a.first())
        .and_then(|s| s.get("url"))
        .and_then(|v| v.as_str())
        .unwrap_or("https://api.example.com")
        .trim_end_matches('/')
        .to_string();

    let components = doc.get("components").cloned().unwrap_or(Value::Null);
    let auth = parse_auth(&components);

    // Component schemas -> objects (dataclasses/interfaces) or aliases (enum/union/scalar).
    let mut schemas = Vec::new();
    let mut aliases: Vec<(String, TypeRef)> = Vec::new();
    let mut class_names = std::collections::HashSet::new();
    if let Some(map) = components.get("schemas").and_then(|v| v.as_object()) {
        for name in map.keys() {
            class_names.insert(pascal(name));
        }
        for (name, schema) in map {
            match classify_schema(name, schema, &class_names, &components) {
                SchemaDef::Object(s) => schemas.push(s),
                SchemaDef::Alias(n, ty) => aliases.push((n, ty)),
            }
        }
    }
    schemas.sort_by(|a, b| a.name.cmp(&b.name));
    aliases.sort_by(|a, b| a.0.cmp(&b.0));

    // Paths -> resources/methods.
    let mut resources: BTreeMap<String, Resource> = BTreeMap::new();
    if let Some(paths) = doc.get("paths").and_then(|v| v.as_object()) {
        let mut path_keys: Vec<&String> = paths.keys().collect();
        path_keys.sort();
        for path in path_keys {
            let item = &paths[path];
            let path_params = item.get("parameters");
            for http in ["get", "post", "put", "patch", "delete"] {
                let op = match item.get(http) {
                    Some(o) if o.is_object() => o,
                    _ => continue,
                };
                let method =
                    parse_operation(http, path, op, path_params, &components, &class_names);
                let (attr, class) = resource_of(op, path);
                let entry = resources.entry(attr.clone()).or_insert_with(|| Resource {
                    attr: attr.clone(),
                    class: class.clone(),
                    methods: Vec::new(),
                });
                entry.methods.push(method);
            }
        }
    }

    let resources: Vec<Resource> = resources.into_values().collect();
    if resources.is_empty() {
        return Err("spec has no operations under `paths`".into());
    }

    let has_webhooks = doc
        .get("webhooks")
        .and_then(|v| v.as_object())
        .map(|m| !m.is_empty())
        .unwrap_or(false);

    Ok(SpecModel {
        title,
        version,
        base_url,
        auth,
        schemas,
        aliases,
        resources,
        has_webhooks,
    })
}

/// Decide whether a component schema becomes an object type or a named alias.
fn classify_schema(
    name: &str,
    schema: &Value,
    classes: &std::collections::HashSet<String>,
    components: &Value,
) -> SchemaDef {
    if let Some(values) = string_enum_values(schema) {
        return SchemaDef::Alias(pascal(name), TypeRef::Enum(values));
    }
    for key in ["oneOf", "anyOf"] {
        if let Some(arr) = schema.get(key).and_then(|v| v.as_array()) {
            let variants: Vec<TypeRef> = arr.iter().map(|v| resolve_type(v, classes)).collect();
            return SchemaDef::Alias(pascal(name), normalize_union(variants));
        }
    }
    if let Some(arr) = schema.get("allOf").and_then(|v| v.as_array()) {
        if let Some(s) = merge_all_of(name, arr, classes, components) {
            return SchemaDef::Object(s);
        }
    }
    let is_object = schema.get("properties").is_some()
        || schema.get("type").and_then(|v| v.as_str()) == Some("object");
    if is_object {
        return SchemaDef::Object(
            parse_object_schema(name, schema, classes).unwrap_or(Schema {
                name: pascal(name),
                fields: Vec::new(),
            }),
        );
    }
    if schema.get("type").is_some() {
        return SchemaDef::Alias(pascal(name), resolve_type(schema, classes));
    }
    SchemaDef::Alias(pascal(name), TypeRef::Any)
}

/// Merge an `allOf` list into a single object (composition / inheritance).
fn merge_all_of(
    name: &str,
    arr: &[Value],
    classes: &std::collections::HashSet<String>,
    components: &Value,
) -> Option<Schema> {
    let mut fields: Vec<Field> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for sub in arr {
        let resolved = deref(sub, components).unwrap_or_else(|| sub.clone());
        let props = match resolved.get("properties").and_then(|v| v.as_object()) {
            Some(p) => p.clone(),
            None => continue,
        };
        let required: std::collections::HashSet<String> = resolved
            .get("required")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        for (pname, pschema) in &props {
            if seen.insert(pname.clone()) {
                fields.push(Field {
                    name: pname.clone(),
                    ty: resolve_type(pschema, classes),
                    required: required.contains(pname),
                });
            }
        }
    }
    if fields.is_empty() {
        return None;
    }
    fields.sort_by_key(|f| !f.required);
    Some(Schema {
        name: pascal(name),
        fields,
    })
}

fn parse_auth(components: &Value) -> Auth {
    let schemes = match components
        .get("securitySchemes")
        .and_then(|v| v.as_object())
    {
        Some(s) => s,
        None => return Auth::None,
    };
    for scheme in schemes.values() {
        let ty = scheme.get("type").and_then(|v| v.as_str()).unwrap_or("");
        match ty {
            "http" => {
                let s = scheme.get("scheme").and_then(|v| v.as_str()).unwrap_or("");
                if s.eq_ignore_ascii_case("bearer") {
                    return Auth::Bearer;
                }
            }
            "apiKey" => {
                if scheme.get("in").and_then(|v| v.as_str()) == Some("header") {
                    let name = scheme
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Authorization")
                        .to_string();
                    return Auth::ApiKeyHeader(name);
                }
            }
            "oauth2" => return Auth::Bearer,
            _ => {}
        }
    }
    Auth::None
}

fn parse_object_schema(
    name: &str,
    schema: &Value,
    classes: &std::collections::HashSet<String>,
) -> Option<Schema> {
    let props = schema.get("properties").and_then(|v| v.as_object())?;
    let required: std::collections::HashSet<String> = schema
        .get("required")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let mut fields = Vec::new();
    for (pname, pschema) in props {
        let ty = resolve_type(pschema, classes);
        fields.push(Field {
            name: pname.clone(),
            ty,
            required: required.contains(pname),
        });
    }
    // dataclass: required (no default) must precede optional.
    fields.sort_by_key(|f| !f.required);
    Some(Schema {
        name: pascal(name),
        fields,
    })
}

fn parse_operation(
    http: &str,
    path: &str,
    op: &Value,
    path_level_params: Option<&Value>,
    components: &Value,
    classes: &std::collections::HashSet<String>,
) -> Method {
    let name = op
        .get("operationId")
        .and_then(|v| v.as_str())
        .map(snake)
        .unwrap_or_else(|| derive_method_name(http, path));

    let summary = op
        .get("summary")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let mut params = Vec::new();
    let mut push_params = |arr: &Value| {
        if let Some(list) = arr.as_array() {
            for p in list {
                if let Some(param) = parse_param(p, classes) {
                    params.push(param);
                }
            }
        }
    };
    if let Some(plp) = path_level_params {
        push_params(plp);
    }
    if let Some(op_params) = op.get("parameters") {
        push_params(op_params);
    }

    let multipart = op
        .get("requestBody")
        .and_then(|rb| rb.get("content"))
        .and_then(|c| c.get("multipart/form-data"))
        .and_then(|m| m.get("schema"))
        .and_then(|s| deref(s, components))
        .and_then(|schema| parse_form_fields(&schema, classes));

    let body = if multipart.is_some() {
        None
    } else {
        op.get("requestBody").map(|rb| {
            let required = rb
                .get("required")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let ty = rb
                .get("content")
                .and_then(json_schema_of_content)
                .map(|s| resolve_type(&s, classes))
                .unwrap_or(TypeRef::Any);
            (ty, required)
        })
    };

    let response_schema = response_schema_of(op);
    let response = response_schema.as_ref().map(|s| resolve_type(s, classes));

    let streaming = response_has_sse(op);
    let pagination = if streaming {
        None
    } else {
        detect_pagination(http, &name, &params, response_schema.as_ref(), components)
    };

    Method {
        name,
        http: http.to_uppercase(),
        path: path.to_string(),
        summary,
        params,
        body,
        response,
        pagination,
        streaming,
        multipart,
    }
}

/// Build the form-field list for a `multipart/form-data` object schema.
/// `format: binary` (or `byte`) string properties become file fields.
fn parse_form_fields(
    schema: &Value,
    classes: &std::collections::HashSet<String>,
) -> Option<Vec<FormField>> {
    let props = schema.get("properties").and_then(|v| v.as_object())?;
    let required: std::collections::HashSet<String> = schema
        .get("required")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let mut fields = Vec::new();
    for (name, pschema) in props {
        let format = pschema.get("format").and_then(|v| v.as_str()).unwrap_or("");
        let is_string = pschema.get("type").and_then(|v| v.as_str()) == Some("string");
        let is_file = is_string && (format == "binary" || format == "byte");
        fields.push(FormField {
            name: name.clone(),
            is_file,
            required: required.contains(name),
            ty: resolve_type(pschema, classes),
        });
    }
    // required (no default) first for nicer signatures
    fields.sort_by_key(|f| !f.required);
    Some(fields)
}

/// True if any success response declares a `text/event-stream` body (SSE).
fn response_has_sse(op: &Value) -> bool {
    let responses = match op.get("responses").and_then(|v| v.as_object()) {
        Some(r) => r,
        None => return false,
    };
    for r in responses.values() {
        if let Some(content) = r.get("content").and_then(|v| v.as_object()) {
            if content.contains_key("text/event-stream") {
                return true;
            }
        }
    }
    false
}

fn parse_param(p: &Value, classes: &std::collections::HashSet<String>) -> Option<Param> {
    let name = p.get("name").and_then(|v| v.as_str())?.to_string();
    let loc = match p.get("in").and_then(|v| v.as_str()).unwrap_or("query") {
        "path" => Loc::Path,
        "header" => Loc::Header,
        _ => Loc::Query,
    };
    let required = p
        .get("required")
        .and_then(|v| v.as_bool())
        .unwrap_or(loc == Loc::Path);
    let ty = p
        .get("schema")
        .map(|s| resolve_type(s, classes))
        .unwrap_or(TypeRef::Str);
    Some(Param {
        name,
        loc,
        ty,
        required,
    })
}

fn json_schema_of_content(content: &Value) -> Option<Value> {
    content
        .get("application/json")
        .and_then(|j| j.get("schema"))
        .cloned()
}

fn response_schema_of(op: &Value) -> Option<Value> {
    let responses = op.get("responses")?.as_object()?;
    for key in ["200", "201", "202", "2XX", "default"] {
        if let Some(r) = responses.get(key) {
            if let Some(s) = r.get("content").and_then(json_schema_of_content) {
                return Some(s);
            }
        }
    }
    None
}

fn resolve_type(schema: &Value, classes: &std::collections::HashSet<String>) -> TypeRef {
    if let Some(r) = schema.get("$ref").and_then(|v| v.as_str()) {
        let name = pascal(r.rsplit('/').next().unwrap_or(r));
        return if classes.contains(&name) {
            TypeRef::Ref(name)
        } else {
            TypeRef::Any
        };
    }

    // oneOf / anyOf -> union.
    for key in ["oneOf", "anyOf"] {
        if let Some(arr) = schema.get(key).and_then(|v| v.as_array()) {
            let variants: Vec<TypeRef> = arr.iter().map(|v| resolve_type(v, classes)).collect();
            return normalize_union(variants);
        }
    }
    // allOf at field level: resolve the first $ref/object, else Any.
    if let Some(arr) = schema.get("allOf").and_then(|v| v.as_array()) {
        if let Some(first) = arr.iter().find(|v| v.get("$ref").is_some()) {
            return resolve_type(first, classes);
        }
        return TypeRef::Any;
    }

    // String enums.
    if let Some(values) = string_enum_values(schema) {
        return TypeRef::Enum(values);
    }

    // `type` may be a string or an array (OpenAPI 3.1, e.g. ["string","null"]).
    let mut nullable = schema
        .get("nullable")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let base = match schema.get("type") {
        Some(Value::Array(types)) => {
            let mut non_null: Vec<TypeRef> = Vec::new();
            for t in types {
                match t.as_str() {
                    Some("null") => nullable = true,
                    Some(other) => non_null.push(scalar_type(other, schema, classes)),
                    None => {}
                }
            }
            normalize_union(non_null)
        }
        Some(Value::String(t)) => scalar_type(t, schema, classes),
        _ => TypeRef::Any,
    };

    if nullable {
        normalize_union(vec![base, TypeRef::Null])
    } else {
        base
    }
}

fn scalar_type(t: &str, schema: &Value, classes: &std::collections::HashSet<String>) -> TypeRef {
    match t {
        "string" => TypeRef::Str,
        "integer" => TypeRef::Int,
        "number" => TypeRef::Float,
        "boolean" => TypeRef::Bool,
        "array" => {
            let inner = schema
                .get("items")
                .map(|i| resolve_type(i, classes))
                .unwrap_or(TypeRef::Any);
            TypeRef::Array(Box::new(inner))
        }
        "object" => {
            if let Some(ap) = schema.get("additionalProperties") {
                if ap.is_object() {
                    return TypeRef::Map(Box::new(resolve_type(ap, classes)));
                }
            }
            TypeRef::Any
        }
        _ => TypeRef::Any,
    }
}

fn string_enum_values(schema: &Value) -> Option<Vec<String>> {
    let arr = schema.get("enum")?.as_array()?;
    if arr.is_empty() {
        return None;
    }
    let mut vals = Vec::new();
    for v in arr {
        match v.as_str() {
            Some(s) => vals.push(s.to_string()),
            None => return None, // non-string enum: fall back to base type
        }
    }
    Some(vals)
}

/// Flatten nested unions, drop `Any`-dominated unions, dedup `Null`, collapse singletons.
fn normalize_union(variants: Vec<TypeRef>) -> TypeRef {
    let mut flat: Vec<TypeRef> = Vec::new();
    let mut has_null = false;
    for v in variants {
        match v {
            TypeRef::Union(inner) => {
                for iv in inner {
                    if matches!(iv, TypeRef::Null) {
                        has_null = true;
                    } else {
                        flat.push(iv);
                    }
                }
            }
            TypeRef::Null => has_null = true,
            other => flat.push(other),
        }
    }
    if flat.iter().any(|t| matches!(t, TypeRef::Any)) {
        return TypeRef::Any;
    }
    if has_null {
        flat.push(TypeRef::Null);
    }
    match flat.len() {
        0 => TypeRef::Any,
        1 => flat.into_iter().next().unwrap(),
        _ => TypeRef::Union(flat),
    }
}

/// Resolve a possibly-$ref schema to the underlying object schema's properties.
fn deref<'a>(schema: &'a Value, components: &'a Value) -> Option<Value> {
    if let Some(r) = schema.get("$ref").and_then(|v| v.as_str()) {
        let name = r.rsplit('/').next().unwrap_or(r);
        return components.get("schemas").and_then(|s| s.get(name)).cloned();
    }
    Some(schema.clone())
}

fn detect_pagination(
    http: &str,
    name: &str,
    params: &[Param],
    response_schema: Option<&Value>,
    components: &Value,
) -> Option<Pagination> {
    if http != "get" {
        return None;
    }
    let resolved = response_schema.and_then(|s| deref(s, components))?;
    let props = resolved.get("properties").and_then(|v| v.as_object())?;

    // data field = first array-typed property (prefer common names).
    let mut data_field: Option<String> = None;
    for pref in ["data", "items", "results"] {
        if props.get(pref).map(is_array_schema).unwrap_or(false) {
            data_field = Some(pref.to_string());
            break;
        }
    }
    if data_field.is_none() {
        data_field = props
            .iter()
            .find(|(_, v)| is_array_schema(v))
            .map(|(k, _)| k.clone());
    }
    let data_field = data_field?;

    let has = |n: &str| params.iter().any(|p| p.name == n && p.loc == Loc::Query);
    let next_field = props
        .keys()
        .find(|k| k.contains("next") || k.as_str() == "cursor")
        .cloned();

    let is_listish = name.starts_with("list") || has("cursor") || has("offset") || has("page");
    if !is_listish {
        return None;
    }

    if has("cursor") || next_field.is_some() {
        Some(Pagination {
            kind: PageKind::Cursor,
            data_field,
            next_field,
            cursor_param: if has("cursor") {
                Some("cursor".to_string())
            } else {
                None
            },
            offset_param: None,
        })
    } else if has("offset") || has("page") {
        let offset_param = if has("offset") { "offset" } else { "page" };
        Some(Pagination {
            kind: PageKind::Offset,
            data_field,
            next_field: None,
            cursor_param: None,
            offset_param: Some(offset_param.to_string()),
        })
    } else {
        None
    }
}

fn is_array_schema(v: &Value) -> bool {
    v.get("type").and_then(|t| t.as_str()) == Some("array")
}

fn resource_of(op: &Value, path: &str) -> (String, String) {
    if let Some(tag) = op
        .get("tags")
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
        .and_then(|v| v.as_str())
    {
        return (snake(tag), pascal(tag));
    }
    // Fall back to the first non-parameter path segment.
    let seg = path
        .split('/')
        .find(|s| !s.is_empty() && !s.starts_with('{'))
        .unwrap_or("api");
    (snake(seg), pascal(seg))
}

// ---------------------------------------------------------------------------
// Python emitter
// ---------------------------------------------------------------------------

fn emit_python(model: &SpecModel, package: &str) -> Vec<(String, String)> {
    vec![
        ("models.py".to_string(), emit_models(model)),
        ("_client.py".to_string(), emit_client(model)),
        ("resources.py".to_string(), emit_resources(model)),
        ("__init__.py".to_string(), emit_init(model, package)),
    ]
}

fn py_type(ty: &TypeRef) -> String {
    match ty {
        TypeRef::Str => "str".to_string(),
        TypeRef::Int => "int".to_string(),
        TypeRef::Float => "float".to_string(),
        TypeRef::Bool => "bool".to_string(),
        TypeRef::Any => "Any".to_string(),
        TypeRef::Null => "None".to_string(),
        TypeRef::Ref(n) => format!("\"{n}\""),
        TypeRef::Array(inner) => format!("List[{}]", py_type(inner)),
        TypeRef::Map(inner) => format!("Dict[str, {}]", py_type(inner)),
        TypeRef::Enum(vals) => format!(
            "Literal[{}]",
            vals.iter()
                .map(|v| format!("{v:?}"))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        TypeRef::Union(items) => {
            if items.len() == 2 && items.iter().any(|t| matches!(t, TypeRef::Null)) {
                let other = items.iter().find(|t| !matches!(t, TypeRef::Null)).unwrap();
                format!("Optional[{}]", py_type(other))
            } else {
                format!(
                    "Union[{}]",
                    items.iter().map(py_type).collect::<Vec<_>>().join(", ")
                )
            }
        }
    }
}

fn emit_models(model: &SpecModel) -> String {
    let mut out = String::new();
    out.push_str("from __future__ import annotations\n");
    out.push_str("from dataclasses import dataclass, asdict\n");
    out.push_str("from typing import Any, Dict, List, Literal, Optional, Union\n\n\n");

    if model.schemas.is_empty() && model.aliases.is_empty() {
        out.push_str("# (spec declared no component schemas)\n");
        return out;
    }

    // Objects that carry a from_dict() parser (alias refs must not call from_dict).
    let objects: std::collections::HashSet<String> =
        model.schemas.iter().map(|s| s.name.clone()).collect();

    // Named type aliases (enums / unions / scalar types) first.
    for (name, ty) in &model.aliases {
        out.push_str(&format!("{name} = {}\n", py_type(ty)));
    }
    if !model.aliases.is_empty() {
        out.push_str("\n\n");
    }

    for s in &model.schemas {
        out.push_str("@dataclass\n");
        out.push_str(&format!("class {}:\n", s.name));
        if s.fields.is_empty() {
            out.push_str("    pass\n\n");
            continue;
        }
        for f in &s.fields {
            let ann = py_type(&f.ty);
            if f.required {
                out.push_str(&format!("    {}: {}\n", ident(&f.name), ann));
            } else {
                out.push_str(&format!(
                    "    {}: {} = None\n",
                    ident(&f.name),
                    py_optional(&f.ty)
                ));
            }
        }
        out.push('\n');
        // from_dict
        out.push_str("    @classmethod\n");
        out.push_str("    def from_dict(cls, d: Optional[Dict[str, Any]]) -> Optional[\"");
        out.push_str(&s.name);
        out.push_str("\"]:\n");
        out.push_str("        if d is None:\n            return None\n");
        out.push_str("        return cls(\n");
        for f in &s.fields {
            out.push_str(&format!(
                "            {}={},\n",
                ident(&f.name),
                from_dict_expr(&f.name, &f.ty, &objects)
            ));
        }
        out.push_str("        )\n\n");
        // to_dict
        out.push_str("    def to_dict(self) -> Dict[str, Any]:\n");
        out.push_str(
            "        return {k: v for k, v in asdict(self).items() if v is not None}\n\n\n",
        );
    }
    out
}

fn from_dict_expr(key: &str, ty: &TypeRef, objects: &std::collections::HashSet<String>) -> String {
    let get = format!("d.get(\"{key}\")");
    match ty {
        TypeRef::Ref(n) if objects.contains(n) => format!("{n}.from_dict({get})"),
        TypeRef::Array(inner) => match inner.as_ref() {
            TypeRef::Ref(n) if objects.contains(n) => {
                format!("[{n}.from_dict(_x) for _x in ({get} or [])]")
            }
            _ => get,
        },
        _ => get,
    }
}

const CLIENT_TEMPLATE: &str = r#"from __future__ import annotations
import json as _json
import os as _os
import time as _time
import urllib.error as _urlerror
import urllib.parse as _urlparse
import urllib.request as _urlrequest
from typing import Any, Dict, Iterator, Optional, Tuple

from .resources import __RESOURCE_IMPORTS__

DEFAULT_BASE_URL = "__BASE_URL__"
_RETRY_STATUS = {429, 500, 502, 503, 504}


class APIError(Exception):
    """Raised for non-2xx responses (carries status, message, request id)."""

    def __init__(self, status: int, message: str, request_id: Optional[str] = None, body: Any = None):
        super().__init__(f"[{status}] {message}")
        self.status = status
        self.message = message
        self.request_id = request_id
        self.body = body


class Client:
    def __init__(
        self,
        api_key: Optional[str] = None,
        base_url: str = DEFAULT_BASE_URL,
        timeout: float = 30.0,
        max_retries: int = 2,
        webhook_secret: Optional[str] = None,
    ):
        self.api_key = api_key
        self.base_url = base_url.rstrip("/")
        self.timeout = timeout
        self.max_retries = max_retries
        self.webhook_secret = webhook_secret
__RESOURCE_INIT__

    def _headers(self) -> Dict[str, str]:
        headers = {"Accept": "application/json", "Content-Type": "application/json"}
__AUTH_CODE__
        return headers

    def _build_url(self, path: str, query: Optional[Dict[str, Any]] = None) -> str:
        url = self.base_url + path
        if query:
            clean = {k: v for k, v in query.items() if v is not None}
            if clean:
                url += "?" + _urlparse.urlencode(clean, doseq=True)
        return url

    def _encode_body(self, body: Any) -> Optional[bytes]:
        if body is None:
            return None
        payload = body.to_dict() if hasattr(body, "to_dict") else body
        return _json.dumps(payload).encode("utf-8")

    def _request(
        self,
        method: str,
        path: str,
        query: Optional[Dict[str, Any]] = None,
        body: Any = None,
    ) -> Any:
        url = self._build_url(path, query)
        data = self._encode_body(body)
        attempt = 0
        while True:
            req = _urlrequest.Request(url, data=data, method=method, headers=self._headers())
            try:
                with _urlrequest.urlopen(req, timeout=self.timeout) as resp:
                    raw = resp.read().decode("utf-8")
                    return _json.loads(raw) if raw else None
            except _urlerror.HTTPError as exc:
                request_id = exc.headers.get("x-request-id") if exc.headers else None
                if exc.code in _RETRY_STATUS and attempt < self.max_retries:
                    _time.sleep(min(2 ** attempt * 0.5, 8.0))
                    attempt += 1
                    continue
                try:
                    parsed = _json.loads(exc.read().decode("utf-8"))
                    message = parsed.get("message") or parsed.get("error") or str(exc)
                except Exception:
                    parsed = None
                    message = str(exc)
                raise APIError(exc.code, message, request_id, parsed)
            except _urlerror.URLError as exc:
                if attempt < self.max_retries:
                    _time.sleep(min(2 ** attempt * 0.5, 8.0))
                    attempt += 1
                    continue
                raise APIError(0, str(exc))

    def _stream(
        self,
        method: str,
        path: str,
        query: Optional[Dict[str, Any]] = None,
        body: Any = None,
    ) -> Iterator[Any]:
        """Server-Sent Events: yields parsed JSON from each `data:` frame."""
        url = self._build_url(path, query)
        data = self._encode_body(body)
        headers = dict(self._headers())
        headers["Accept"] = "text/event-stream"
        req = _urlrequest.Request(url, data=data, method=method, headers=headers)
        with _urlrequest.urlopen(req, timeout=self.timeout) as resp:
            for raw in resp:
                line = raw.decode("utf-8").strip()
                if not line or not line.startswith("data:"):
                    continue
                chunk = line[5:].strip()
                if chunk == "[DONE]":
                    break
                try:
                    yield _json.loads(chunk)
                except Exception:
                    yield chunk

    def _multipart(
        self,
        method: str,
        path: str,
        fields: Dict[str, Any],
        files: Dict[str, Tuple[str, Any]],
        query: Optional[Dict[str, Any]] = None,
    ) -> Any:
        """Send a multipart/form-data request (encoded with the stdlib)."""
        url = self._build_url(path, query)
        boundary = "----spacekit" + _os.urandom(16).hex()
        buf = bytearray()
        for name, value in fields.items():
            if value is None:
                continue
            buf += f"--{boundary}\r\n".encode("utf-8")
            buf += f'Content-Disposition: form-data; name="{name}"\r\n\r\n'.encode("utf-8")
            buf += str(value).encode("utf-8") + b"\r\n"
        for name, (filename, content) in files.items():
            if content is None:
                continue
            blob = content if isinstance(content, (bytes, bytearray)) else str(content).encode("utf-8")
            buf += f"--{boundary}\r\n".encode("utf-8")
            buf += (
                f'Content-Disposition: form-data; name="{name}"; filename="{filename}"\r\n'
            ).encode("utf-8")
            buf += b"Content-Type: application/octet-stream\r\n\r\n"
            buf += bytes(blob) + b"\r\n"
        buf += f"--{boundary}--\r\n".encode("utf-8")

        headers = dict(self._headers())
        headers["Content-Type"] = f"multipart/form-data; boundary={boundary}"
        req = _urlrequest.Request(url, data=bytes(buf), method=method, headers=headers)
        try:
            with _urlrequest.urlopen(req, timeout=self.timeout) as resp:
                raw = resp.read().decode("utf-8")
                return _json.loads(raw) if raw else None
        except _urlerror.HTTPError as exc:
            request_id = exc.headers.get("x-request-id") if exc.headers else None
            try:
                parsed = _json.loads(exc.read().decode("utf-8"))
                message = parsed.get("message") or parsed.get("error") or str(exc)
            except Exception:
                parsed = None
                message = str(exc)
            raise APIError(exc.code, message, request_id, parsed)
        except _urlerror.URLError as exc:
            raise APIError(0, str(exc))
"#;

fn emit_client(model: &SpecModel) -> String {
    let mut names: Vec<String> = model.resources.iter().map(|r| r.class.clone()).collect();
    if model.has_webhooks {
        names.push("Webhooks".to_string());
    }
    let resource_imports = if names.is_empty() {
        "()".to_string()
    } else {
        format!("({},)", names.join(", "))
    };
    let mut init_lines: Vec<String> = model
        .resources
        .iter()
        .map(|r| format!("        self.{} = {}(self)", r.attr, r.class))
        .collect();
    if model.has_webhooks {
        init_lines.push("        self.webhooks = Webhooks(self)".to_string());
    }
    let resource_init = if init_lines.is_empty() {
        "        pass".to_string()
    } else {
        init_lines.join("\n")
    };
    let auth_code = match &model.auth {
        Auth::None => "        # no auth scheme declared in spec".to_string(),
        Auth::Bearer => {
            "        if self.api_key:\n            headers[\"Authorization\"] = f\"Bearer {self.api_key}\"".to_string()
        }
        Auth::ApiKeyHeader(name) => format!(
            "        if self.api_key:\n            headers[\"{name}\"] = self.api_key"
        ),
    };

    CLIENT_TEMPLATE
        .replace("__RESOURCE_IMPORTS__", &resource_imports)
        .replace("__BASE_URL__", &model.base_url)
        .replace("__RESOURCE_INIT__", &resource_init)
        .replace("__AUTH_CODE__", &auth_code)
}

fn emit_resources(model: &SpecModel) -> String {
    let mut out = String::new();
    out.push_str("from __future__ import annotations\n");
    out.push_str(
        "from typing import Any, Dict, Iterator, List, Literal, Optional, Union, TYPE_CHECKING\n",
    );
    if model.has_webhooks {
        out.push_str("import base64 as _base64\n");
        out.push_str("import hashlib as _hashlib\n");
        out.push_str("import hmac as _hmac\n");
        out.push_str("import json as _json\n");
    }
    out.push('\n');
    out.push_str("from .models import *  # noqa: F401,F403\n\n");
    out.push_str("if TYPE_CHECKING:\n    from ._client import Client\n\n\n");

    let objects: std::collections::HashSet<String> =
        model.schemas.iter().map(|s| s.name.clone()).collect();

    for r in &model.resources {
        out.push_str(&format!("class {}:\n", r.class));
        out.push_str("    def __init__(self, client: \"Client\"):\n");
        out.push_str("        self._client = client\n\n");
        for m in &r.methods {
            out.push_str(&emit_method(m, &objects));
            out.push('\n');
        }
        out.push('\n');
    }

    if model.has_webhooks {
        out.push_str(WEBHOOKS_PY);
    }
    out
}

const WEBHOOKS_PY: &str = r#"class Webhooks:
    """Verify and parse incoming webhooks (Standard Webhooks HMAC-SHA256)."""

    def __init__(self, client: "Client"):
        self._client = client

    def unwrap(self, payload: Any, headers: Dict[str, str], secret: Optional[str] = None) -> Any:
        key = secret or self._client.webhook_secret
        body = payload.decode("utf-8") if isinstance(payload, (bytes, bytearray)) else payload
        if key:
            self.verify(body, headers, key)
        return _json.loads(body)

    @staticmethod
    def verify(payload: str, headers: Dict[str, str], secret: str) -> None:
        def header(name: str) -> Optional[str]:
            return headers.get(name) or headers.get(name.title()) or headers.get(name.upper())

        msg_id = header("webhook-id")
        timestamp = header("webhook-timestamp")
        signature = header("webhook-signature")
        if not (msg_id and timestamp and signature):
            raise ValueError("missing webhook signature headers")
        signed = f"{msg_id}.{timestamp}.{payload}"
        # Standard Webhooks: `whsec_<base64>` secrets are base64-decoded; a plain
        # secret is used as raw UTF-8 bytes (kept identical across SDK languages).
        if secret.startswith("whsec_"):
            key_bytes = _base64.b64decode(secret[6:])
        else:
            key_bytes = secret.encode("utf-8")
        digest = _hmac.new(key_bytes, signed.encode("utf-8"), _hashlib.sha256).digest()
        expected = _base64.b64encode(digest).decode("utf-8")
        provided = [p.split(",", 1)[1] if "," in p else p for p in signature.split(" ")]
        if not any(_hmac.compare_digest(expected, p) for p in provided):
            raise ValueError("webhook signature mismatch")
"#;

fn emit_method(m: &Method, objects: &std::collections::HashSet<String>) -> String {
    if m.streaming {
        return emit_streaming_method(m);
    }
    if let Some(fields) = &m.multipart {
        return emit_multipart_method(m, fields, objects);
    }
    if let Some(pg) = &m.pagination {
        return emit_paginated_method(m, pg);
    }
    let mut out = String::new();

    // Signature: required params first, optional (=None) last.
    let mut required: Vec<String> = Vec::new();
    let mut optional: Vec<String> = Vec::new();
    for p in &m.params {
        if p.loc == Loc::Header {
            continue;
        }
        let ann = py_type(&p.ty);
        if p.required {
            required.push(format!("{}: {}", ident(&p.name), ann));
        } else {
            optional.push(format!("{}: {} = None", ident(&p.name), py_optional(&p.ty)));
        }
    }
    if let Some((bty, breq)) = &m.body {
        let ann = py_type(bty);
        if *breq {
            required.push(format!("body: {}", ann));
        } else {
            optional.push(format!("body: Optional[{}] = None", ann));
        }
    }
    let mut sig = vec!["self".to_string()];
    sig.extend(required);
    sig.extend(optional);
    let ret = m
        .response
        .as_ref()
        .map(return_annotation)
        .unwrap_or_else(|| "Any".to_string());

    out.push_str(&format!(
        "    def {}({}) -> {}:\n",
        m.name,
        sig.join(", "),
        ret
    ));
    if let Some(s) = &m.summary {
        out.push_str(&format!("        \"\"\"{}\"\"\"\n", s.replace('"', "'")));
    }
    out.push_str(&format!("        path = {}\n", path_expr(&m.path)));

    let query: Vec<&Param> = m.params.iter().filter(|p| p.loc == Loc::Query).collect();
    if query.is_empty() {
        out.push_str(&format!(
            "        resp = self._client._request(\"{}\", path{})\n",
            m.http,
            if m.body.is_some() { ", body=body" } else { "" }
        ));
    } else {
        out.push_str("        query = {\n");
        for q in &query {
            out.push_str(&format!(
                "            \"{}\": {},\n",
                q.name,
                ident(&q.name)
            ));
        }
        out.push_str("        }\n");
        out.push_str(&format!(
            "        resp = self._client._request(\"{}\", path, query=query{})\n",
            m.http,
            if m.body.is_some() { ", body=body" } else { "" }
        ));
    }
    out.push_str(&format!(
        "        return {}\n",
        wrap_response("resp", &m.response, objects)
    ));
    out
}

fn emit_paginated_method(m: &Method, pg: &Pagination) -> String {
    let mut out = String::new();
    // Items are yielded as raw dicts in v0 (element typing is a follow-up).
    let item_ann = "Any".to_string();

    let mut required: Vec<String> = Vec::new();
    let mut optional: Vec<String> = Vec::new();
    for p in &m.params {
        if p.loc != Loc::Query {
            // path params remain required positional
            if p.loc == Loc::Path {
                required.push(format!("{}: {}", ident(&p.name), py_type(&p.ty)));
            }
            continue;
        }
        // pagination control params are managed internally
        if Some(&p.name) == pg.cursor_param.as_ref() || Some(&p.name) == pg.offset_param.as_ref() {
            continue;
        }
        optional.push(format!(
            "{}: Optional[{}] = None",
            ident(&p.name),
            py_type(&p.ty)
        ));
    }
    let mut sig = vec!["self".to_string()];
    sig.extend(required);
    sig.extend(optional);

    out.push_str(&format!(
        "    def {}({}) -> Iterator[{}]:\n",
        m.name,
        sig.join(", "),
        item_ann
    ));
    let doc = m
        .summary
        .clone()
        .unwrap_or_else(|| "Auto-paginating iterator.".to_string());
    out.push_str(&format!(
        "        \"\"\"{} (auto-paginated)\"\"\"\n",
        doc.replace('"', "'")
    ));
    out.push_str(&format!("        path = {}\n", path_expr(&m.path)));

    // Build the static query (non-pagination params).
    out.push_str("        base_query: Dict[str, Any] = {\n");
    for p in &m.params {
        if p.loc != Loc::Query {
            continue;
        }
        if Some(&p.name) == pg.cursor_param.as_ref() || Some(&p.name) == pg.offset_param.as_ref() {
            continue;
        }
        out.push_str(&format!(
            "            \"{}\": {},\n",
            p.name,
            ident(&p.name)
        ));
    }
    out.push_str("        }\n");

    let data_get = format!("(page.get(\"{}\") or [])", pg.data_field);
    match pg.kind {
        PageKind::Cursor => {
            let cursor_param = pg
                .cursor_param
                .clone()
                .unwrap_or_else(|| "cursor".to_string());
            out.push_str("        _cursor = None\n");
            out.push_str("        while True:\n");
            out.push_str("            query = dict(base_query)\n");
            out.push_str(&format!(
                "            if _cursor is not None:\n                query[\"{cursor_param}\"] = _cursor\n"
            ));
            out.push_str(&format!(
                "            page = self._client._request(\"{}\", path, query=query) or {{}}\n",
                m.http
            ));
            out.push_str(&format!("            for _item in {data_get}:\n"));
            out.push_str("                yield _item\n");
            if let Some(nf) = &pg.next_field {
                out.push_str(&format!("            _cursor = page.get(\"{nf}\")\n"));
            } else {
                out.push_str("            _cursor = page.get(\"next_cursor\")\n");
            }
            out.push_str("            if not _cursor:\n                break\n");
        }
        PageKind::Offset => {
            let offset_param = pg
                .offset_param
                .clone()
                .unwrap_or_else(|| "offset".to_string());
            out.push_str("        _offset = 0\n");
            out.push_str("        while True:\n");
            out.push_str("            query = dict(base_query)\n");
            out.push_str(&format!(
                "            query[\"{offset_param}\"] = _offset\n"
            ));
            out.push_str(&format!(
                "            page = self._client._request(\"{}\", path, query=query) or {{}}\n",
                m.http
            ));
            out.push_str(&format!("            _items = {data_get}\n"));
            out.push_str("            for _item in _items:\n                yield _item\n");
            out.push_str("            if not _items:\n                break\n");
            out.push_str("            _offset += len(_items)\n");
        }
    }
    out
}

fn emit_multipart_method(
    m: &Method,
    fields: &[FormField],
    objects: &std::collections::HashSet<String>,
) -> String {
    let mut required: Vec<String> = Vec::new();
    let mut optional: Vec<String> = Vec::new();
    for p in &m.params {
        if p.loc == Loc::Header {
            continue;
        }
        let ann = py_type(&p.ty);
        if p.required {
            required.push(format!("{}: {}", ident(&p.name), ann));
        } else {
            optional.push(format!("{}: {} = None", ident(&p.name), py_optional(&p.ty)));
        }
    }
    for f in fields {
        let ann = if f.is_file {
            "bytes".to_string()
        } else {
            py_type(&f.ty)
        };
        if f.required {
            required.push(format!("{}: {}", ident(&f.name), ann));
        } else {
            optional.push(format!("{}: Optional[{}] = None", ident(&f.name), ann));
        }
    }
    let mut sig = vec!["self".to_string()];
    sig.extend(required);
    sig.extend(optional);
    let ret = m
        .response
        .as_ref()
        .map(return_annotation)
        .unwrap_or_else(|| "Any".to_string());

    let mut out = String::new();
    if let Some(s) = &m.summary {
        out.push_str(&format!("    \"\"\"{}\"\"\"\n", s.replace('"', "'")));
    }
    out.push_str(&format!(
        "    def {}({}) -> {}:\n",
        m.name,
        sig.join(", "),
        ret
    ));
    out.push_str(&format!("        path = {}\n", path_expr(&m.path)));
    out.push_str("        fields: Dict[str, Any] = {\n");
    for f in fields.iter().filter(|f| !f.is_file) {
        out.push_str(&format!(
            "            \"{}\": {},\n",
            f.name,
            ident(&f.name)
        ));
    }
    out.push_str("        }\n");
    out.push_str("        files: Dict[str, Any] = {\n");
    for f in fields.iter().filter(|f| f.is_file) {
        out.push_str(&format!(
            "            \"{0}\": (\"{0}\", {1}),\n",
            f.name,
            ident(&f.name)
        ));
    }
    out.push_str("        }\n");
    let query: Vec<&Param> = m.params.iter().filter(|p| p.loc == Loc::Query).collect();
    let query_arg = if query.is_empty() {
        String::new()
    } else {
        let mut q = String::from(", query={");
        for p in &query {
            q.push_str(&format!("\"{}\": {}, ", p.name, ident(&p.name)));
        }
        q.push('}');
        q
    };
    out.push_str(&format!(
        "        resp = self._client._multipart(\"{}\", path, fields, files{})\n",
        m.http, query_arg
    ));
    out.push_str(&format!(
        "        return {}\n",
        wrap_response("resp", &m.response, objects)
    ));
    out
}

fn emit_streaming_method(m: &Method) -> String {
    let mut required: Vec<String> = Vec::new();
    let mut optional: Vec<String> = Vec::new();
    for p in &m.params {
        if p.loc == Loc::Header {
            continue;
        }
        let ann = py_type(&p.ty);
        if p.required {
            required.push(format!("{}: {}", ident(&p.name), ann));
        } else {
            optional.push(format!("{}: {} = None", ident(&p.name), py_optional(&p.ty)));
        }
    }
    if let Some((bty, breq)) = &m.body {
        let ann = py_type(bty);
        if *breq {
            required.push(format!("body: {}", ann));
        } else {
            optional.push(format!("body: Optional[{}] = None", ann));
        }
    }
    let mut sig = vec!["self".to_string()];
    sig.extend(required);
    sig.extend(optional);

    let mut out = String::new();
    out.push_str(&format!(
        "    def {}({}) -> Iterator[Any]:\n",
        m.name,
        sig.join(", ")
    ));
    let doc = m
        .summary
        .clone()
        .unwrap_or_else(|| "Stream server-sent events.".to_string());
    out.push_str(&format!(
        "        \"\"\"{} (server-sent events)\"\"\"\n",
        doc.replace('"', "'")
    ));
    out.push_str(&format!("        path = {}\n", path_expr(&m.path)));
    let body_arg = if m.body.is_some() { ", body=body" } else { "" };
    let query: Vec<&Param> = m.params.iter().filter(|p| p.loc == Loc::Query).collect();
    if query.is_empty() {
        out.push_str(&format!(
            "        yield from self._client._stream(\"{}\", path{})\n",
            m.http, body_arg
        ));
    } else {
        out.push_str("        query = {\n");
        for q in &query {
            out.push_str(&format!(
                "            \"{}\": {},\n",
                q.name,
                ident(&q.name)
            ));
        }
        out.push_str("        }\n");
        out.push_str(&format!(
            "        yield from self._client._stream(\"{}\", path, query=query{})\n",
            m.http, body_arg
        ));
    }
    out
}

/// Wrap a type in `Optional[...]` unless it is already nullable (avoids `Optional[Optional[..]]`).
fn py_optional(ty: &TypeRef) -> String {
    let already_nullable =
        matches!(ty, TypeRef::Union(items) if items.iter().any(|t| matches!(t, TypeRef::Null)));
    if already_nullable {
        py_type(ty)
    } else {
        format!("Optional[{}]", py_type(ty))
    }
}

fn return_annotation(ty: &TypeRef) -> String {
    match ty {
        TypeRef::Ref(n) => format!("Optional[\"{n}\"]"),
        TypeRef::Array(inner) => format!("List[{}]", py_type(inner)),
        other => py_type(other),
    }
}

fn wrap_response(
    var: &str,
    ty: &Option<TypeRef>,
    objects: &std::collections::HashSet<String>,
) -> String {
    match ty {
        Some(TypeRef::Ref(n)) if objects.contains(n) => format!("{n}.from_dict({var})"),
        Some(TypeRef::Array(inner)) => match inner.as_ref() {
            TypeRef::Ref(n) if objects.contains(n) => {
                format!("[{n}.from_dict(_x) for _x in ({var} or [])]")
            }
            _ => var.to_string(),
        },
        _ => var.to_string(),
    }
}

fn path_expr(path: &str) -> String {
    // Convert OpenAPI `/users/{id}` into a Python f-string with sanitized idents.
    if !path.contains('{') {
        return format!("\"{path}\"");
    }
    let mut result = String::from("f\"");
    let mut chars = path.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '{' {
            let mut name = String::new();
            for nc in chars.by_ref() {
                if nc == '}' {
                    break;
                }
                name.push(nc);
            }
            result.push('{');
            result.push_str(&ident(&name));
            result.push('}');
        } else {
            result.push(c);
        }
    }
    result.push('"');
    result
}

fn emit_init(model: &SpecModel, _package: &str) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "\"\"\"{} v{} — generated SDK.\"\"\"\n",
        model.title, model.version
    ));
    out.push_str("from ._client import Client, APIError\n");
    out.push_str("from .models import *  # noqa: F401,F403\n\n");
    out.push_str("__all__ = [\"Client\", \"APIError\"]\n");
    out
}

fn render_readme(model: &SpecModel, package: &str, lang: Lang) -> String {
    let first = model
        .resources
        .first()
        .and_then(|r| r.methods.first().map(|m| (r.attr.clone(), m.name.clone())));
    let n_aliases = model.aliases.len();
    let (lang_name, layout, runtime_note, fence, usage, check_cmd) = match lang {
        Lang::Python => {
            let usage = match &first {
                Some((attr, method)) => format!(
                    "from {package} import Client\n\nclient = Client(api_key=\"...\")\nresult = client.{attr}.{method}(...)\n"
                ),
                None => format!("from {package} import Client\n\nclient = Client(api_key=\"...\")\n"),
            };
            (
                "Python",
                format!(
                    "- `{pkg}/models.py` — dataclasses ({nmod} objects{aliases})\n\
                     - `{pkg}/_client.py` — HTTP client\n\
                     - `{pkg}/resources.py` — resource methods\n\
                     - `{pkg}/__init__.py` — exports",
                    pkg = package,
                    nmod = model.schemas.len(),
                    aliases = if n_aliases > 0 {
                        format!(", {n_aliases} type aliases")
                    } else {
                        String::new()
                    },
                ),
                "Dependency-free (Python stdlib only).",
                "python",
                usage,
                format!("python3 -c \"import {package}\""),
            )
        }
        Lang::TypeScript => {
            let usage = match &first {
                Some((attr, method)) => format!(
                    "import {{ Client }} from \"{package}\";\n\nconst client = new Client({{ apiKey: \"...\" }});\nconst result = await client.{attr}.{method}(...);\n"
                ),
                None => format!(
                    "import {{ Client }} from \"{package}\";\n\nconst client = new Client({{ apiKey: \"...\" }});\n"
                ),
            };
            (
                "TypeScript",
                format!(
                    "- `{pkg}/src/models.ts` — interfaces ({nmod} objects{aliases})\n\
                     - `{pkg}/src/client.ts` — HTTP client\n\
                     - `{pkg}/src/resources.ts` — resource methods\n\
                     - `{pkg}/src/index.ts` — exports\n\
                     - `{pkg}/package.json`, `{pkg}/tsconfig.json`",
                    pkg = package,
                    nmod = model.schemas.len(),
                    aliases = if n_aliases > 0 {
                        format!(", {n_aliases} type aliases")
                    } else {
                        String::new()
                    },
                ),
                "Uses the built-in `fetch` API (Node 18+ / browsers).",
                "typescript",
                usage,
                "tsc --noEmit -p tsconfig.json".to_string(),
            )
        }
        Lang::Rust => {
            let usage = match &first {
                Some((attr, method)) => format!(
                    "use {package}::{{Client, ClientOptions}};\n\n#[tokio::main]\nasync fn main() -> Result<(), {package}::APIError> {{\n    let client = Client::new(ClientOptions {{\n        api_key: Some(\"...\".into()),\n        ..Default::default()\n    }})?;\n    let result = client.{attr}.{method}(...).await?;\n    Ok(())\n}}\n"
                ),
                None => format!(
                    "use {package}::{{Client, ClientOptions}};\n\nlet client = Client::new(ClientOptions::default())?;\n"
                ),
            };
            (
                "Rust",
                format!(
                    "- `{pkg}/src/models.rs` — serde structs ({nmod} objects{aliases})\n\
                     - `{pkg}/src/client.rs` — async reqwest client\n\
                     - `{pkg}/src/resources.rs` — resource methods\n\
                     - `{pkg}/src/lib.rs` — crate root\n\
                     - `{pkg}/Cargo.toml`",
                    pkg = package,
                    nmod = model.schemas.len(),
                    aliases = if n_aliases > 0 {
                        format!(", {n_aliases} type aliases")
                    } else {
                        String::new()
                    },
                ),
                "Async (`tokio` + `reqwest` with rustls).",
                "rust",
                usage,
                "cargo check".to_string(),
            )
        }
    };
    format!(
        "# {title} SDK ({lang_name})\n\n\
Generated from an OpenAPI spec (spec → typed model → emit).\n\n\
- Base URL: `{base}`\n\
- Auth: {auth}\n\
- Resources: {nres} · Methods: {nm} · Models: {nmod}\n\n\
## Package layout\n\n{layout}\n\n\
## Runtime\n\n{runtime_note} Includes:\n\n\
- Authentication (from spec security schemes)\n\
- Retries with exponential backoff (429 / 5xx)\n\
- Configurable timeouts\n\
- Typed errors with request IDs\n\
- Auto-pagination for list endpoints\n\
- SSE streaming (`text/event-stream`)\n\
- Webhook signature verification (Standard Webhooks)\n\
- Multipart / file uploads\n\n\
Schema support: string enums, nullable fields, `oneOf`/`anyOf` unions, `allOf` composition.\n\n\
## Usage\n\n```{fence}\n{usage}```\n\n\
## Regeneration\n\n\
Re-run `spacekit agent sdk` against an updated spec to incrementally update this package.\n\
A `.sdkgen-manifest.json` in the output directory tracks generated files; hand-edited\n\
files are preserved unless you pass `--force`. Preview changes with `--plan`.\n\n\
Type-check / import test: `{check_cmd}`\n\n\
See `spacekit-cli/documentation/AGENT_SDK_GENERATION.md` for the full reference.\n",
        title = model.title,
        base = model.base_url,
        auth = match &model.auth {
            Auth::None => "none".to_string(),
            Auth::Bearer => "bearer token".to_string(),
            Auth::ApiKeyHeader(h) => format!("api key (header `{h}`)"),
        },
        nres = model.resources.len(),
        nm = model
            .resources
            .iter()
            .map(|r| r.methods.len())
            .sum::<usize>(),
        nmod = model.schemas.len(),
    )
}

// ---------------------------------------------------------------------------
// TypeScript emitter (proves the SpecModel IR is language-agnostic)
// ---------------------------------------------------------------------------

fn emit_typescript(model: &SpecModel, package: &str) -> Vec<(String, String)> {
    vec![
        ("package.json".to_string(), ts_package_json(model, package)),
        ("tsconfig.json".to_string(), ts_tsconfig()),
        ("src/models.ts".to_string(), ts_models(model)),
        ("src/client.ts".to_string(), ts_client(model)),
        ("src/resources.ts".to_string(), ts_resources(model)),
        ("src/index.ts".to_string(), ts_index()),
    ]
}

fn ts_type(ty: &TypeRef) -> String {
    match ty {
        TypeRef::Str => "string".to_string(),
        TypeRef::Int | TypeRef::Float => "number".to_string(),
        TypeRef::Bool => "boolean".to_string(),
        TypeRef::Any => "any".to_string(),
        TypeRef::Null => "null".to_string(),
        TypeRef::Ref(n) => n.clone(),
        TypeRef::Array(inner) => {
            // Parenthesize unions inside arrays: (A | B)[]
            if matches!(inner.as_ref(), TypeRef::Union(_) | TypeRef::Enum(_)) {
                format!("({})[]", ts_type(inner))
            } else {
                format!("{}[]", ts_type(inner))
            }
        }
        TypeRef::Map(inner) => format!("Record<string, {}>", ts_type(inner)),
        TypeRef::Enum(vals) => vals
            .iter()
            .map(|v| format!("{v:?}"))
            .collect::<Vec<_>>()
            .join(" | "),
        TypeRef::Union(items) => items.iter().map(ts_type).collect::<Vec<_>>().join(" | "),
    }
}

fn ts_return(ty: &Option<TypeRef>) -> String {
    match ty {
        Some(t) => ts_type(t),
        None => "any".to_string(),
    }
}

fn ts_models(model: &SpecModel) -> String {
    let mut out = String::from("// Generated models.\n\n");
    if model.schemas.is_empty() && model.aliases.is_empty() {
        out.push_str("export {};\n");
        return out;
    }
    for (name, ty) in &model.aliases {
        out.push_str(&format!("export type {name} = {};\n", ts_type(ty)));
    }
    if !model.aliases.is_empty() {
        out.push('\n');
    }
    for s in &model.schemas {
        out.push_str(&format!("export interface {} {{\n", s.name));
        for f in &s.fields {
            let opt = if f.required { "" } else { "?" };
            out.push_str(&format!(
                "  {}{}: {};\n",
                ts_prop_name(&f.name),
                opt,
                ts_type(&f.ty)
            ));
        }
        out.push_str("}\n\n");
    }
    out
}

const TS_CLIENT_TEMPLATE: &str = r#"// Generated client.
import { __RESOURCE_LIST__ } from "./resources";

export const DEFAULT_BASE_URL = "__BASE_URL__";
const RETRY_STATUS = new Set([429, 500, 502, 503, 504]);

export interface ClientOptions {
  apiKey?: string;
  baseURL?: string;
  timeoutMs?: number;
  maxRetries?: number;
  webhookSecret?: string;
}

export interface RequestOptions {
  query?: Record<string, unknown>;
  body?: unknown;
}

export class APIError extends Error {
  readonly status: number;
  readonly requestId?: string;
  readonly body?: unknown;
  constructor(status: number, message: string, requestId?: string, body?: unknown) {
    super(`[${status}] ${message}`);
    this.name = "APIError";
    this.status = status;
    this.requestId = requestId;
    this.body = body;
  }
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

export class Client {
  readonly apiKey?: string;
  readonly baseURL: string;
  readonly timeoutMs: number;
  readonly maxRetries: number;
  readonly webhookSecret?: string;
__RESOURCE_FIELDS__

  constructor(options: ClientOptions = {}) {
    this.apiKey = options.apiKey;
    this.baseURL = (options.baseURL ?? DEFAULT_BASE_URL).replace(/\/+$/, "");
    this.timeoutMs = options.timeoutMs ?? 30000;
    this.maxRetries = options.maxRetries ?? 2;
    this.webhookSecret = options.webhookSecret;
__RESOURCE_INIT__
  }

  private buildHeaders(): Record<string, string> {
    const headers: Record<string, string> = {
      Accept: "application/json",
      "Content-Type": "application/json",
    };
__AUTH_CODE__
    return headers;
  }

  private buildURL(path: string, query?: Record<string, unknown>): string {
    let url = this.baseURL + path;
    if (query) {
      const params = new URLSearchParams();
      for (const [key, value] of Object.entries(query)) {
        if (value !== undefined && value !== null) params.append(key, String(value));
      }
      const qs = params.toString();
      if (qs) url += "?" + qs;
    }
    return url;
  }

  async *stream<T>(method: string, path: string, options: RequestOptions = {}): AsyncGenerator<T, void, unknown> {
    const url = this.buildURL(path, options.query);
    const payload = options.body !== undefined ? JSON.stringify(options.body) : undefined;
    const headers = { ...this.buildHeaders(), Accept: "text/event-stream" };
    const response = await fetch(url, { method, headers, body: payload });
    if (!response.ok || !response.body) {
      const requestId = response.headers.get("x-request-id") ?? undefined;
      throw new APIError(response.status, response.statusText, requestId);
    }
    const reader = response.body.getReader();
    const decoder = new TextDecoder();
    let buffer = "";
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      buffer += decoder.decode(value, { stream: true });
      let newline: number;
      while ((newline = buffer.indexOf("\n")) >= 0) {
        const line = buffer.slice(0, newline).trimEnd();
        buffer = buffer.slice(newline + 1);
        if (!line.startsWith("data:")) continue;
        const data = line.slice(5).trim();
        if (data === "[DONE]") return;
        try {
          yield JSON.parse(data) as T;
        } catch {
          /* skip non-JSON frame */
        }
      }
    }
  }

  async request<T>(method: string, path: string, options: RequestOptions = {}): Promise<T> {
    const url = this.buildURL(path, options.query);
    const payload = options.body !== undefined ? JSON.stringify(options.body) : undefined;
    let attempt = 0;
    for (;;) {
      const controller = new AbortController();
      const timer = setTimeout(() => controller.abort(), this.timeoutMs);
      try {
        const response = await fetch(url, {
          method,
          headers: this.buildHeaders(),
          body: payload,
          signal: controller.signal,
        });
        clearTimeout(timer);
        if (!response.ok) {
          const requestId = response.headers.get("x-request-id") ?? undefined;
          if (RETRY_STATUS.has(response.status) && attempt < this.maxRetries) {
            await sleep(Math.min(2 ** attempt * 500, 8000));
            attempt += 1;
            continue;
          }
          let body: unknown;
          let message = response.statusText;
          try {
            body = await response.json();
            const obj = body as { message?: string; error?: string };
            message = obj.message ?? obj.error ?? message;
          } catch {
            /* non-JSON error body */
          }
          throw new APIError(response.status, message, requestId, body);
        }
        const text = await response.text();
        return (text ? JSON.parse(text) : undefined) as T;
      } catch (error) {
        clearTimeout(timer);
        if (error instanceof APIError) throw error;
        if (attempt < this.maxRetries) {
          await sleep(Math.min(2 ** attempt * 500, 8000));
          attempt += 1;
          continue;
        }
        throw new APIError(0, error instanceof Error ? error.message : String(error));
      }
    }
  }

  async requestMultipart<T>(
    method: string,
    path: string,
    form: FormData,
    query?: Record<string, unknown>,
  ): Promise<T> {
    const url = this.buildURL(path, query);
    const headers = this.buildHeaders();
    delete headers["Content-Type"]; // let fetch set the multipart boundary
    let attempt = 0;
    for (;;) {
      const controller = new AbortController();
      const timer = setTimeout(() => controller.abort(), this.timeoutMs);
      try {
        const response = await fetch(url, {
          method,
          headers,
          body: form,
          signal: controller.signal,
        });
        clearTimeout(timer);
        if (!response.ok) {
          const requestId = response.headers.get("x-request-id") ?? undefined;
          if (RETRY_STATUS.has(response.status) && attempt < this.maxRetries) {
            await sleep(Math.min(2 ** attempt * 500, 8000));
            attempt += 1;
            continue;
          }
          let body: unknown;
          let message = response.statusText;
          try {
            body = await response.json();
            const obj = body as { message?: string; error?: string };
            message = obj.message ?? obj.error ?? message;
          } catch {
            /* non-JSON error body */
          }
          throw new APIError(response.status, message, requestId, body);
        }
        const text = await response.text();
        return (text ? JSON.parse(text) : undefined) as T;
      } catch (error) {
        clearTimeout(timer);
        if (error instanceof APIError) throw error;
        if (attempt < this.maxRetries) {
          await sleep(Math.min(2 ** attempt * 500, 8000));
          attempt += 1;
          continue;
        }
        throw new APIError(0, error instanceof Error ? error.message : String(error));
      }
    }
  }
}
"#;

fn ts_client(model: &SpecModel) -> String {
    let mut classes: Vec<String> = model.resources.iter().map(|r| r.class.clone()).collect();
    let mut fields: Vec<(String, String)> = model
        .resources
        .iter()
        .map(|r| (camel(&r.attr), r.class.clone()))
        .collect();
    if model.has_webhooks {
        classes.push("Webhooks".to_string());
        fields.push(("webhooks".to_string(), "Webhooks".to_string()));
    }
    let resource_list = classes.join(", ");
    let resource_fields = fields
        .iter()
        .map(|(attr, class)| format!("  readonly {attr}: {class};"))
        .collect::<Vec<_>>()
        .join("\n");
    let resource_init = fields
        .iter()
        .map(|(attr, class)| format!("    this.{attr} = new {class}(this);"))
        .collect::<Vec<_>>()
        .join("\n");
    let auth_code = match &model.auth {
        Auth::None => "    // no auth scheme declared in spec".to_string(),
        Auth::Bearer => {
            "    if (this.apiKey) headers[\"Authorization\"] = `Bearer ${this.apiKey}`;".to_string()
        }
        Auth::ApiKeyHeader(name) => {
            format!("    if (this.apiKey) headers[\"{name}\"] = this.apiKey;")
        }
    };

    TS_CLIENT_TEMPLATE
        .replace("__RESOURCE_LIST__", &resource_list)
        .replace("__BASE_URL__", &model.base_url)
        .replace("__RESOURCE_FIELDS__", &resource_fields)
        .replace("__RESOURCE_INIT__", &resource_init)
        .replace("__AUTH_CODE__", &auth_code)
}

fn ts_resources(model: &SpecModel) -> String {
    let mut out = String::from("// Generated resources.\n");
    out.push_str("import type { Client } from \"./client\";\n");
    if model.has_webhooks {
        out.push_str("import { APIError } from \"./client\";\n");
    }
    if !model.schemas.is_empty() || !model.aliases.is_empty() {
        let names = model
            .schemas
            .iter()
            .map(|s| s.name.clone())
            .chain(model.aliases.iter().map(|(n, _)| n.clone()))
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!("import type {{ {names} }} from \"./models\";\n"));
    }
    out.push('\n');

    for r in &model.resources {
        out.push_str(&format!("export class {} {{\n", r.class));
        out.push_str("  constructor(private readonly client: Client) {}\n\n");
        for m in &r.methods {
            out.push_str(&ts_method(m));
            out.push('\n');
        }
        out.push_str("}\n\n");
    }

    if model.has_webhooks {
        out.push_str(WEBHOOKS_TS);
    }
    out
}

const WEBHOOKS_TS: &str = r#"export class Webhooks {
  constructor(private readonly client: Client) {}

  /** Verify (Standard Webhooks HMAC-SHA256) and parse an incoming webhook. */
  async unwrap(payload: string, headers: Record<string, string>, secret?: string): Promise<any> {
    const key = secret ?? this.client.webhookSecret;
    if (key) await verifyWebhook(payload, headers, key);
    return JSON.parse(payload);
  }
}

function pickHeader(headers: Record<string, string>, name: string): string | undefined {
  return headers[name] ?? headers[name.toLowerCase()] ?? headers[name.toUpperCase()];
}

function base64ToBytes(b64: string): Uint8Array {
  const binary = atob(b64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) bytes[i] = binary.charCodeAt(i);
  return bytes;
}

function bytesToBase64(bytes: Uint8Array): string {
  let binary = "";
  for (let i = 0; i < bytes.length; i += 1) binary += String.fromCharCode(bytes[i]);
  return btoa(binary);
}

async function verifyWebhook(
  payload: string,
  headers: Record<string, string>,
  secret: string,
): Promise<void> {
  const id = pickHeader(headers, "webhook-id");
  const timestamp = pickHeader(headers, "webhook-timestamp");
  const signature = pickHeader(headers, "webhook-signature");
  if (!id || !timestamp || !signature) {
    throw new APIError(400, "missing webhook signature headers");
  }
  const signed = `${id}.${timestamp}.${payload}`;
  // Standard Webhooks: `whsec_<base64>` secrets are base64-decoded; a plain
  // secret is used as raw UTF-8 bytes (kept identical across SDK languages).
  const keyBytes = secret.startsWith("whsec_")
    ? base64ToBytes(secret.slice(6))
    : new TextEncoder().encode(secret);
  const cryptoKey = await crypto.subtle.importKey(
    "raw",
    keyBytes as unknown as BufferSource,
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"],
  );
  const message = new TextEncoder().encode(signed) as unknown as BufferSource;
  const digest = await crypto.subtle.sign("HMAC", cryptoKey, message);
  const expected = bytesToBase64(new Uint8Array(digest));
  const provided = signature.split(" ").map((part) => (part.includes(",") ? part.split(",")[1] : part));
  if (!provided.some((candidate) => candidate === expected)) {
    throw new APIError(400, "webhook signature mismatch");
  }
}
"#;

fn ts_method(m: &Method) -> String {
    if m.streaming {
        return ts_streaming_method(m);
    }
    if let Some(fields) = &m.multipart {
        return ts_multipart_method(m, fields);
    }
    if let Some(pg) = &m.pagination {
        return ts_paginated_method(m, pg);
    }
    let mut required: Vec<String> = Vec::new();
    let mut optional: Vec<String> = Vec::new();
    for p in &m.params {
        if p.loc == Loc::Header {
            continue;
        }
        if p.required {
            required.push(format!("{}: {}", camel(&p.name), ts_type(&p.ty)));
        } else {
            optional.push(format!("{}?: {}", camel(&p.name), ts_type(&p.ty)));
        }
    }
    if let Some((bty, breq)) = &m.body {
        if *breq {
            required.push(format!("body: {}", ts_type(bty)));
        } else {
            optional.push(format!("body?: {}", ts_type(bty)));
        }
    }
    let mut sig = required;
    sig.extend(optional);
    let ret = ts_return(&m.response);

    let mut out = String::new();
    if let Some(s) = &m.summary {
        out.push_str(&format!("  /** {} */\n", s.replace("*/", "* /")));
    }
    out.push_str(&format!(
        "  async {}({}): Promise<{}> {{\n",
        camel(&m.name),
        sig.join(", "),
        ret
    ));
    out.push_str(&format!("    const path = {};\n", ts_path_expr(&m.path)));

    let query: Vec<&Param> = m.params.iter().filter(|p| p.loc == Loc::Query).collect();
    let has_query = !query.is_empty();
    if has_query {
        out.push_str("    const query: Record<string, unknown> = {\n");
        for q in &query {
            out.push_str(&format!("      {:?}: {},\n", q.name, camel(&q.name)));
        }
        out.push_str("    };\n");
    }
    let opts = match (has_query, m.body.is_some()) {
        (true, true) => ", { query, body }",
        (true, false) => ", { query }",
        (false, true) => ", { body }",
        (false, false) => "",
    };
    out.push_str(&format!(
        "    return await this.client.request<{}>(\"{}\", path{});\n",
        ret, m.http, opts
    ));
    out.push_str("  }\n");
    out
}

fn ts_paginated_method(m: &Method, pg: &Pagination) -> String {
    let mut params: Vec<String> = Vec::new();
    for p in &m.params {
        if p.loc == Loc::Path {
            params.push(format!("{}: {}", camel(&p.name), ts_type(&p.ty)));
        }
    }
    for p in &m.params {
        if p.loc != Loc::Query {
            continue;
        }
        if Some(&p.name) == pg.cursor_param.as_ref() || Some(&p.name) == pg.offset_param.as_ref() {
            continue;
        }
        params.push(format!("{}?: {}", camel(&p.name), ts_type(&p.ty)));
    }

    let mut out = String::new();
    let doc = m
        .summary
        .clone()
        .unwrap_or_else(|| "Auto-paginating iterator.".to_string());
    out.push_str(&format!(
        "  /** {} (auto-paginated) */\n",
        doc.replace("*/", "* /")
    ));
    out.push_str(&format!(
        "  async *{}({}): AsyncGenerator<any, void, unknown> {{\n",
        camel(&m.name),
        params.join(", ")
    ));
    out.push_str(&format!("    const path = {};\n", ts_path_expr(&m.path)));
    out.push_str("    const baseQuery: Record<string, unknown> = {\n");
    for p in &m.params {
        if p.loc != Loc::Query {
            continue;
        }
        if Some(&p.name) == pg.cursor_param.as_ref() || Some(&p.name) == pg.offset_param.as_ref() {
            continue;
        }
        out.push_str(&format!("      {:?}: {},\n", p.name, camel(&p.name)));
    }
    out.push_str("    };\n");

    let data = &pg.data_field;
    match pg.kind {
        PageKind::Cursor => {
            let cursor = pg
                .cursor_param
                .clone()
                .unwrap_or_else(|| "cursor".to_string());
            let next = pg
                .next_field
                .clone()
                .unwrap_or_else(|| "next_cursor".to_string());
            out.push_str("    let cursor: string | undefined = undefined;\n");
            out.push_str("    for (;;) {\n");
            out.push_str("      const query: Record<string, unknown> = { ...baseQuery };\n");
            out.push_str(&format!(
                "      if (cursor !== undefined) query[{cursor:?}] = cursor;\n"
            ));
            out.push_str(&format!(
                "      const page = (await this.client.request<any>(\"{}\", path, {{ query }})) ?? {{}};\n",
                m.http
            ));
            out.push_str(&format!(
                "      for (const item of (page[{data:?}] ?? [])) yield item;\n"
            ));
            out.push_str(&format!("      cursor = page[{next:?}];\n"));
            out.push_str("      if (!cursor) break;\n");
            out.push_str("    }\n");
        }
        PageKind::Offset => {
            let offset = pg
                .offset_param
                .clone()
                .unwrap_or_else(|| "offset".to_string());
            out.push_str("    let offset = 0;\n");
            out.push_str("    for (;;) {\n");
            out.push_str("      const query: Record<string, unknown> = { ...baseQuery };\n");
            out.push_str(&format!("      query[{offset:?}] = offset;\n"));
            out.push_str(&format!(
                "      const page = (await this.client.request<any>(\"{}\", path, {{ query }})) ?? {{}};\n",
                m.http
            ));
            out.push_str(&format!("      const items = page[{data:?}] ?? [];\n"));
            out.push_str("      for (const item of items) yield item;\n");
            out.push_str("      if (items.length === 0) break;\n");
            out.push_str("      offset += items.length;\n");
            out.push_str("    }\n");
        }
    }
    out.push_str("  }\n");
    out
}

fn ts_multipart_method(m: &Method, fields: &[FormField]) -> String {
    let mut required: Vec<String> = Vec::new();
    let mut optional: Vec<String> = Vec::new();
    for p in &m.params {
        if p.loc == Loc::Header {
            continue;
        }
        if p.required {
            required.push(format!("{}: {}", camel(&p.name), ts_type(&p.ty)));
        } else {
            optional.push(format!("{}?: {}", camel(&p.name), ts_type(&p.ty)));
        }
    }
    for f in fields {
        let ann = if f.is_file {
            "Blob".to_string()
        } else {
            ts_type(&f.ty)
        };
        if f.required {
            required.push(format!("{}: {}", camel(&f.name), ann));
        } else {
            optional.push(format!("{}?: {}", camel(&f.name), ann));
        }
    }
    let mut sig = required;
    sig.extend(optional);
    let ret = ts_return(&m.response);

    let mut out = String::new();
    if let Some(s) = &m.summary {
        out.push_str(&format!("  /** {} */\n", s.replace("*/", "* /")));
    }
    out.push_str(&format!(
        "  async {}({}): Promise<{}> {{\n",
        camel(&m.name),
        sig.join(", "),
        ret
    ));
    out.push_str(&format!("    const path = {};\n", ts_path_expr(&m.path)));
    out.push_str("    const form = new FormData();\n");
    for f in fields {
        let local = camel(&f.name);
        let value = if f.is_file {
            local.clone()
        } else {
            format!("String({local})")
        };
        if f.required {
            out.push_str(&format!("    form.append({:?}, {});\n", f.name, value));
        } else {
            out.push_str(&format!(
                "    if ({local} !== undefined) form.append({:?}, {});\n",
                f.name, value
            ));
        }
    }
    let query: Vec<&Param> = m.params.iter().filter(|p| p.loc == Loc::Query).collect();
    let query_arg = if query.is_empty() {
        String::new()
    } else {
        out.push_str("    const query: Record<string, unknown> = {\n");
        for q in &query {
            out.push_str(&format!("      {:?}: {},\n", q.name, camel(&q.name)));
        }
        out.push_str("    };\n");
        ", query".to_string()
    };
    out.push_str(&format!(
        "    return await this.client.requestMultipart<{}>(\"{}\", path, form{});\n",
        ret, m.http, query_arg
    ));
    out.push_str("  }\n");
    out
}

fn ts_streaming_method(m: &Method) -> String {
    let mut required: Vec<String> = Vec::new();
    let mut optional: Vec<String> = Vec::new();
    for p in &m.params {
        if p.loc == Loc::Header {
            continue;
        }
        if p.required {
            required.push(format!("{}: {}", camel(&p.name), ts_type(&p.ty)));
        } else {
            optional.push(format!("{}?: {}", camel(&p.name), ts_type(&p.ty)));
        }
    }
    if let Some((bty, breq)) = &m.body {
        if *breq {
            required.push(format!("body: {}", ts_type(bty)));
        } else {
            optional.push(format!("body?: {}", ts_type(bty)));
        }
    }
    let mut sig = required;
    sig.extend(optional);

    let mut out = String::new();
    let doc = m
        .summary
        .clone()
        .unwrap_or_else(|| "Stream server-sent events.".to_string());
    out.push_str(&format!(
        "  /** {} (server-sent events) */\n",
        doc.replace("*/", "* /")
    ));
    out.push_str(&format!(
        "  async *{}({}): AsyncGenerator<any, void, unknown> {{\n",
        camel(&m.name),
        sig.join(", ")
    ));
    out.push_str(&format!("    const path = {};\n", ts_path_expr(&m.path)));

    let query: Vec<&Param> = m.params.iter().filter(|p| p.loc == Loc::Query).collect();
    let has_query = !query.is_empty();
    if has_query {
        out.push_str("    const query: Record<string, unknown> = {\n");
        for q in &query {
            out.push_str(&format!("      {:?}: {},\n", q.name, camel(&q.name)));
        }
        out.push_str("    };\n");
    }
    let opts = match (has_query, m.body.is_some()) {
        (true, true) => ", { query, body }",
        (true, false) => ", { query }",
        (false, true) => ", { body }",
        (false, false) => "",
    };
    out.push_str(&format!(
        "    yield* this.client.stream<any>(\"{}\", path{});\n",
        m.http, opts
    ));
    out.push_str("  }\n");
    out
}

fn ts_path_expr(path: &str) -> String {
    let mut result = String::from("`");
    let mut chars = path.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '{' {
            let mut name = String::new();
            for nc in chars.by_ref() {
                if nc == '}' {
                    break;
                }
                name.push(nc);
            }
            result.push_str("${");
            result.push_str(&camel(&name));
            result.push('}');
        } else if c == '`' {
            result.push_str("\\`");
        } else {
            result.push(c);
        }
    }
    result.push('`');
    result
}

fn ts_index() -> String {
    let mut out = String::from("// Generated entrypoint.\n");
    out.push_str("export { Client, APIError } from \"./client\";\n");
    out.push_str("export type { ClientOptions, RequestOptions } from \"./client\";\n");
    out.push_str("export * from \"./models\";\n");
    out
}

fn ts_package_json(model: &SpecModel, package: &str) -> String {
    format!(
        "{{\n  \"name\": {pkg:?},\n  \"version\": {ver:?},\n  \"description\": {desc:?},\n  \"type\": \"module\",\n  \"main\": \"src/index.ts\",\n  \"types\": \"src/index.ts\",\n  \"files\": [\"src\"],\n  \"scripts\": {{\n    \"typecheck\": \"tsc --noEmit -p tsconfig.json\"\n  }}\n}}\n",
        pkg = package,
        ver = model.version,
        desc = format!("{} SDK (generated)", model.title),
    )
}

fn ts_tsconfig() -> String {
    String::from(
        "{\n  \"compilerOptions\": {\n    \"target\": \"ES2020\",\n    \"module\": \"ESNext\",\n    \"moduleResolution\": \"node\",\n    \"lib\": [\"ES2020\", \"DOM\"],\n    \"strict\": true,\n    \"skipLibCheck\": true,\n    \"noEmit\": true,\n    \"esModuleInterop\": true,\n    \"forceConsistentCasingInFileNames\": true\n  },\n  \"include\": [\"src\"]\n}\n",
    )
}

fn ts_prop_name(name: &str) -> String {
    let valid = !name.is_empty()
        && name
            .chars()
            .next()
            .map(|c| c.is_ascii_alphabetic() || c == '_' || c == '$')
            .unwrap_or(false)
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$');
    if valid {
        name.to_string()
    } else {
        format!("{name:?}") // JSON-quoted property key
    }
}

const TS_KEYWORDS: &[&str] = &[
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "default",
    "delete",
    "do",
    "else",
    "enum",
    "export",
    "extends",
    "false",
    "finally",
    "for",
    "function",
    "if",
    "import",
    "in",
    "instanceof",
    "new",
    "null",
    "return",
    "super",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "typeof",
    "var",
    "void",
    "while",
    "with",
];

fn camel(s: &str) -> String {
    let sn = snake(s);
    let mut out = String::new();
    for (i, part) in sn.split('_').filter(|p| !p.is_empty()).enumerate() {
        if i == 0 {
            out.push_str(part);
        } else {
            let mut chars = part.chars();
            if let Some(f) = chars.next() {
                out.push(f.to_ascii_uppercase());
                out.push_str(chars.as_str());
            }
        }
    }
    if out.is_empty() {
        out.push_str("field");
    }
    if TS_KEYWORDS.contains(&out.as_str()) {
        out.push('_');
    }
    out
}

// ---------------------------------------------------------------------------
// Identifier helpers
// ---------------------------------------------------------------------------

const PY_KEYWORDS: &[&str] = &[
    "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class", "continue",
    "def", "del", "elif", "else", "except", "finally", "for", "from", "global", "if", "import",
    "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return", "try", "while",
    "with", "yield",
];

/// Sanitize an arbitrary string into a valid Python identifier.
fn ident(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    if out.is_empty()
        || out
            .chars()
            .next()
            .map(|c| c.is_ascii_digit())
            .unwrap_or(false)
    {
        out.insert(0, '_');
    }
    if PY_KEYWORDS.contains(&out.as_str()) {
        out.push('_');
    }
    out
}

fn snake(s: &str) -> String {
    let mut out = String::new();
    let mut prev_lower = false;
    for c in s.chars() {
        if c.is_ascii_uppercase() {
            if prev_lower {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
            prev_lower = false;
        } else if c.is_ascii_alphanumeric() {
            out.push(c);
            prev_lower = c.is_ascii_lowercase() || c.is_ascii_digit();
        } else {
            if !out.ends_with('_') {
                out.push('_');
            }
            prev_lower = false;
        }
    }
    let out = out.trim_matches('_').to_string();
    let out = if out.is_empty() {
        "field".to_string()
    } else {
        out
    };
    if PY_KEYWORDS.contains(&out.as_str()) {
        format!("{out}_")
    } else {
        out
    }
}

fn pascal(s: &str) -> String {
    snake(s)
        .split('_')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let mut chars = p.chars();
            match chars.next() {
                Some(f) => f.to_ascii_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

fn sanitize_pkg(title: &str) -> String {
    let s = snake(title);
    if s.is_empty() {
        "api_client".to_string()
    } else {
        s
    }
}

fn derive_method_name(http: &str, path: &str) -> String {
    let mut parts = vec![http.to_string()];
    for seg in path.split('/') {
        if seg.is_empty() {
            continue;
        }
        if seg.starts_with('{') {
            let inner = seg.trim_matches(|c| c == '{' || c == '}');
            parts.push(format!("by_{}", snake(inner)));
        } else {
            parts.push(snake(seg));
        }
    }
    let joined = parts.join("_");
    if PY_KEYWORDS.contains(&joined.as_str()) {
        format!("{joined}_")
    } else {
        joined
    }
}
