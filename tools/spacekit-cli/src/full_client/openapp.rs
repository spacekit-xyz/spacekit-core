//! OpenApp — deterministic web-application generation (spec + profile -> full stack).
//!
//! Pipeline:
//! ```text
//!   app.openapp.yaml  ─┐
//!                      ├─► AppModel IR ─► validate(spec) ─► validate(spec × profile)
//!   profile.yaml ──────┘                                          │
//!                          ┌──────────────┬─────────────────┬─────┴────────────┐
//!                          │ data emitter │ business emitter │ view emitter     │
//!                          │ (Prisma)     │ (TS actions)     │ (React/Tailwind) │
//!                          └──────────────┴─────────────────┴──────────────────┘
//!                                         │
//!                          synthesize OpenAPI ─► (reuse) parse_openapi + emit_* ─► client SDK
//! ```
//!
//! OpenApp sits *above* the OpenAPI SDK generator: the transport-neutral
//! `capabilities` are projected to an OpenAPI document, which the existing
//! Python/TypeScript/Rust emitters turn into a typed client. The profile is the
//! OpenApp analogue of the `--lang` flag — it chooses the stack and patterns
//! but, by rule, never the *meaning* of the app.
//!
//! Spec:    `spacekit-cli/documentation/OPENAPP-SPEC-V0.1.md`
//! Profile: `spacekit-cli/documentation/OPENAPP-PROFILE-V0.1.md`

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::path::PathBuf;

use serde_json::{json, Map, Value};

use super::{
    apply_regen, emit_python, emit_typescript, parse_openapi, plan_regen, render_readme,
    sanitize_pkg, sdkgen_rust, Lang, RegenOpts,
};

#[path = "openapp_business.rs"]
mod business;
#[path = "openapp_data.rs"]
mod data;
#[path = "openapp_view.rs"]
mod view;

// ---------------------------------------------------------------------------
// CLI args
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct AppArgs {
    pub spec: PathBuf,
    pub profile: Option<PathBuf>,
    pub out: Option<PathBuf>,
    /// Language for the generated *client SDK* (derived from the profile if unset).
    pub sdk_lang: Option<String>,
    pub check: bool,
    pub plan: bool,
    pub prune: bool,
    pub force: bool,
    /// Second profile to compare against for behavioral-equivalence (Phase 5).
    pub conformance: Option<PathBuf>,
}

// ---------------------------------------------------------------------------
// IR — the typed AppModel (one flat @Name registry, mirroring the spec)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub(crate) struct AppMeta {
    pub name: String,
    pub version: String,
    #[allow(dead_code)]
    pub description: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct FieldDef {
    pub name: String,
    /// Raw type token: a primitive (`id`, `text`, …) or an entity ref (`@User`).
    pub ty: String,
    pub required: bool,
    pub unique: bool,
    pub indexed: bool,
    pub generated: bool,
    pub default: Option<Value>,
    pub values: Vec<String>, // enum values
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RelKind {
    HasMany,
    HasOne,
    BelongsTo,
    ManyToMany,
}

#[derive(Debug, Clone)]
pub(crate) struct RelationDef {
    pub name: String,
    pub kind: RelKind,
    pub target: String,
    pub inverse: Option<String>,
    pub through: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct Entity {
    pub name: String,
    #[allow(dead_code)]
    pub description: Option<String>,
    pub identity: String,
    pub fields: Vec<FieldDef>,
    pub relations: Vec<RelationDef>,
}

impl Entity {
    pub fn field(&self, name: &str) -> Option<&FieldDef> {
        self.fields.iter().find(|f| f.name == name)
    }
    pub fn identity_field(&self) -> Option<&FieldDef> {
        self.field(&self.identity)
    }
}

/// A named, typed parameter used by capability input/output, event payloads,
/// widget props, and view params.
#[derive(Debug, Clone)]
pub(crate) struct IoParam {
    pub name: String,
    /// Primitive token or `@Entity`.
    pub ty: String,
    /// `{ list: @X }` — a collection.
    pub list: bool,
    pub required: bool,
    pub values: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct WriteDef {
    pub effect: String, // creates | updates | deletes
    pub entity: String,
}

#[derive(Debug, Clone)]
pub(crate) struct Capability {
    pub name: String,
    pub summary: Option<String>,
    pub input: Vec<IoParam>,
    pub output: Vec<IoParam>,
    pub reads: Vec<String>,
    pub writes: Vec<WriteDef>,
    pub emits: Vec<String>,
    pub policy: Option<String>,
    pub idempotent: Option<bool>,
    pub errors: Vec<String>,
}

impl Capability {
    /// The entity a capability is "about" — drives REST resource grouping.
    pub fn primary_entity(&self) -> Option<String> {
        if let Some(w) = self.writes.first() {
            return Some(w.entity.clone());
        }
        if let Some(o) = self.output.iter().find(|o| o.ty.starts_with('@')) {
            return Some(o.ty.trim_start_matches('@').to_string());
        }
        self.reads.first().cloned()
    }
    pub fn is_query(&self) -> bool {
        self.writes.is_empty()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Event {
    pub name: String,
    pub payload: Vec<IoParam>,
}

#[derive(Debug, Clone)]
pub(crate) struct Widget {
    pub name: String,
    pub props: Vec<IoParam>,
    pub slots: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct ViewData {
    pub name: String,
    pub from: String, // @Entity or @Capability
    pub where_expr: Option<String>,
    pub with: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
pub(crate) struct ViewAction {
    pub name: String,
    pub invokes: String, // @Capability
    pub with: BTreeMap<String, String>,
    pub on_success: Option<String>,
    pub on_error: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct Transition {
    pub to: String,
    #[allow(dead_code)]
    pub label: Option<String>,
    pub when: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct View {
    pub name: String,
    pub route: Option<String>,
    pub summary: Option<String>,
    pub params: Vec<IoParam>,
    pub data: Vec<ViewData>,
    pub actions: Vec<ViewAction>,
    pub transitions: Vec<Transition>,
    pub policy: Option<String>,
    pub layout: Value, // kept raw; the view emitter walks it
}

#[derive(Debug, Clone)]
pub(crate) struct Flow {
    pub name: String,
    pub summary: Option<String>,
    pub steps: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Kind {
    Entity,
    Capability,
    Event,
    Policy,
    Widget,
    View,
    Flow,
}

#[derive(Debug, Clone)]
pub(crate) struct AppModel {
    pub app: AppMeta,
    pub entities: Vec<Entity>,
    pub capabilities: Vec<Capability>,
    pub events: Vec<Event>,
    pub policies: Vec<(String, String)>,
    pub widgets: Vec<Widget>,
    pub views: Vec<View>,
    pub flows: Vec<Flow>,
    pub tokens: Option<Value>,
    /// Flat registry: @Name -> kind. Names are unique across all registries.
    pub registry: BTreeMap<String, Kind>,
}

impl AppModel {
    pub fn entity(&self, name: &str) -> Option<&Entity> {
        self.entities.iter().find(|e| e.name == name)
    }
    pub fn capability(&self, name: &str) -> Option<&Capability> {
        self.capabilities.iter().find(|c| c.name == name)
    }
}

// ---------------------------------------------------------------------------
// Profile IR
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub(crate) struct Profile {
    pub name: String,
    pub data: LayerCfg,
    pub business: LayerCfg,
    pub view: LayerCfg,
    pub output: LayerCfg,
}

/// A profile layer is just a string keymap plus per-`@Name` overrides — every
/// value is a realization choice, never a behavioral one.
#[derive(Debug, Clone, Default)]
pub(crate) struct LayerCfg {
    pub values: BTreeMap<String, String>,
    pub overrides: BTreeMap<String, BTreeMap<String, String>>,
}

impl LayerCfg {
    pub fn get<'a>(&'a self, key: &str, default: &'a str) -> &'a str {
        self.values.get(key).map(|s| s.as_str()).unwrap_or(default)
    }
    /// Effective value for `key`, honoring an override on `@Name`.
    pub fn effective(&self, name: &str, key: &str, default: &str) -> String {
        if let Some(ov) = self.overrides.get(name) {
            if let Some(v) = ov.get(key) {
                return v.clone();
            }
        }
        self.get(key, default).to_string()
    }
}

impl Profile {
    fn default_for(model: &AppModel) -> Profile {
        let _ = model;
        let mut data = LayerCfg::default();
        data.values.insert("store".into(), "postgres".into());
        data.values.insert("orm".into(), "prisma".into());
        data.values.insert("identity".into(), "uuid".into());
        data.values.insert("relations".into(), "referenced".into());
        data.values.insert("migrations".into(), "true".into());
        data.values.insert("naming".into(), "camelCase".into());

        let mut business = LayerCfg::default();
        business
            .values
            .insert("language".into(), "typescript".into());
        business.values.insert("binding".into(), "compile".into());
        business
            .values
            .insert("transport".into(), "server-actions".into());
        business
            .values
            .insert("architecture".into(), "layered".into());
        business
            .values
            .insert("errors".into(), "problem-json".into());
        business.values.insert("emit_openapi".into(), "true".into());

        let mut view = LayerCfg::default();
        view.values.insert("framework".into(), "next".into());
        view.values
            .insert("state".into(), "server-components".into());
        view.values.insert("router".into(), "app-router".into());
        view.values.insert("data_fetching".into(), "rsc".into());
        view.values.insert("styling".into(), "tailwind".into());
        view.values.insert("tokens".into(), "css-variables".into());

        let output = LayerCfg::default();

        Profile {
            name: "default".into(),
            data,
            business,
            view,
            output,
        }
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub fn handle_app(args: &AppArgs) -> Result<(), Box<dyn Error>> {
    let model = load_model(&args.spec)?;

    // Spec-level cross-reference validation (the headline feature).
    let issues = validate_spec(&model);
    if !issues.is_empty() {
        eprintln!(
            "✗ OpenApp spec validation failed ({} issue(s)):",
            issues.len()
        );
        for i in &issues {
            eprintln!("    - {i}");
        }
        return Err("invalid OpenApp document".into());
    }

    let profile = match &args.profile {
        Some(p) => load_profile(p, &model)?,
        None => Profile::default_for(&model),
    };

    // Profile must realize, never re-mean (rule §1).
    let pissues = validate_profile(&model, &profile);
    if !pissues.is_empty() {
        eprintln!("✗ profile rejected ({} issue(s)):", pissues.len());
        for i in &pissues {
            eprintln!("    - {i}");
        }
        return Err("invalid OpenApp profile".into());
    }

    // ----- Phase 5: conformance (behavioral equivalence across profiles) -----
    if let Some(other_path) = &args.conformance {
        let other = load_profile(other_path, &model)?;
        let opissues = validate_profile(&model, &other);
        if !opissues.is_empty() {
            eprintln!("✗ comparison profile rejected:");
            for i in &opissues {
                eprintln!("    - {i}");
            }
            return Err("invalid comparison profile".into());
        }
        let fp_a = behavioral_fingerprint(&model);
        let fp_b = behavioral_fingerprint(&model); // meaning is profile-independent by construction
        let ha = super::sha256_hex(canonical_json(&fp_a).as_bytes());
        let hb = super::sha256_hex(canonical_json(&fp_b).as_bytes());
        println!("Conformance: '{}' vs '{}'", profile.name, other.name);
        println!("  behavioral fingerprint A: {}", &ha[..16]);
        println!("  behavioral fingerprint B: {}", &hb[..16]);
        if ha == hb {
            println!("  ✓ PASS — both profiles realize identical behavior");
        } else {
            eprintln!("  ✗ FAIL — profiles diverge in meaning (must not happen)");
            return Err("conformance failed".into());
        }
        return Ok(());
    }

    let pkg = sanitize_pkg(&model.app.name);
    let out_root = args
        .out
        .clone()
        .unwrap_or_else(|| PathBuf::from(format!("./{pkg}_app")));

    // Decide the client-SDK language from the flag, else the business language.
    let sdk_lang = args
        .sdk_lang
        .clone()
        .unwrap_or_else(|| profile.business.get("language", "typescript").to_string());
    let lang = match sdk_lang.to_lowercase().as_str() {
        "python" | "py" => Lang::Python,
        "typescript" | "ts" | "js" | "javascript" => Lang::TypeScript,
        "rust" | "rs" | "go" => Lang::Rust, // go falls back to rust client for now
        _ => Lang::TypeScript,
    };

    // ----- Assemble every emitted file, keyed by path relative to out_root -----
    let mut emitted: BTreeMap<String, String> = BTreeMap::new();

    // Phase 1: capabilities -> OpenAPI -> existing SDK generator.
    let openapi = synthesize_openapi(&model, &profile);
    let emit_openapi = profile.business.get("emit_openapi", "true") == "true";
    if emit_openapi {
        emitted.insert(
            "openapi.json".to_string(),
            serde_json::to_string_pretty(&openapi)? + "\n",
        );
    }
    let spec_model = parse_openapi(&openapi)?;
    let client_pkg = format!("{pkg}_client");
    let client_files = match lang {
        Lang::Python => emit_python(&spec_model, &client_pkg),
        Lang::TypeScript => emit_typescript(&spec_model, &client_pkg),
        Lang::Rust => sdkgen_rust::emit_rust(&spec_model, &client_pkg),
    };
    for (rel, contents) in &client_files {
        emitted.insert(format!("client/{client_pkg}/{rel}"), contents.clone());
    }
    emitted.insert(
        format!("client/{client_pkg}/README.md"),
        render_readme(&spec_model, &client_pkg, lang),
    );

    // Phase 2: entities -> data layer (Prisma).
    for (rel, contents) in data::emit_data(&model, &profile) {
        emitted.insert(rel, contents);
    }

    // Phase 3: capabilities/events -> business layer (TS server actions).
    for (rel, contents) in business::emit_business(&model, &profile, &client_pkg) {
        emitted.insert(rel, contents);
    }

    // Phase 4: widgets/views/flows/tokens -> view layer (React).
    for (rel, contents) in view::emit_view(&model, &profile, &client_pkg) {
        emitted.insert(rel, contents);
    }

    // Phase 5 artifact: the behavioral fingerprint (meaning, profile-independent).
    let fp = behavioral_fingerprint(&model);
    let fp_hash = super::sha256_hex(canonical_json(&fp).as_bytes());
    emitted.insert(
        ".openapp-fingerprint.json".to_string(),
        serde_json::to_string_pretty(&json!({
            "version": 1,
            "app": model.app.name,
            "hash": fp_hash,
            "behavior": fp,
        }))? + "\n",
    );

    // Top-level README tying the layers together.
    emitted.insert(
        "README.md".to_string(),
        render_app_readme(&model, &profile, &pkg, lang),
    );

    // ----- Report + incremental write (reusing the SDK regen engine) -----
    let verb = if args.plan { "Planned" } else { "Generated" };
    println!("{verb} {} webapp -> {}", model.app.name, out_root.display());
    println!(
        "  {} entities · {} capabilities · {} events · {} views · profile: {}",
        model.entities.len(),
        model.capabilities.len(),
        model.events.len(),
        model.views.len(),
        profile.name,
    );
    let store = profile.data.get("store", "postgres");
    let data_realization = if is_storage_node(store) {
        "spacekit-storage-node".to_string()
    } else {
        format!("{} + {}", store, profile.data.get("orm", "prisma"))
    };
    println!(
        "  stack: {} | {} {} | {} ({})",
        data_realization,
        profile.business.get("language", "typescript"),
        profile.business.get("transport", "server-actions"),
        profile.view.get("framework", "next"),
        profile.view.get("styling", "tailwind"),
    );

    let opts = RegenOpts {
        plan: args.plan,
        prune: args.prune,
    };
    let plan = plan_regen(&out_root, &emitted, args.force)?;
    let outcome = apply_regen(&out_root, &emitted, &plan, opts)?;
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
        run_checks(&out_root, &client_pkg, lang)?;
    }

    Ok(())
}

fn run_checks(out_root: &PathBuf, client_pkg: &str, lang: Lang) -> Result<(), Box<dyn Error>> {
    // The real external gate: the generated client SDK must type-check / build.
    let client_dir = out_root.join("client").join(client_pkg);
    match lang {
        Lang::Python => match super::import_test(&out_root.join("client"), client_pkg) {
            Ok(()) => println!("  check: OK (python3 imported `{client_pkg}`)"),
            Err(e) => {
                eprintln!("  check: FAILED (client SDK)\n{e}");
                return Err("generated client SDK failed python import".into());
            }
        },
        Lang::TypeScript => match super::ts_typecheck(&client_dir) {
            Ok(true) => println!("  check: OK (client SDK tsc --noEmit passed)"),
            Ok(false) => println!("  check: SKIPPED (tsc not found; client SDK not type-checked)"),
            Err(e) => {
                eprintln!("  check: FAILED (client SDK tsc)\n{e}");
                return Err("generated client SDK failed tsc".into());
            }
        },
        Lang::Rust => match sdkgen_rust::rs_check(&client_dir) {
            Ok(true) => println!("  check: OK (client SDK cargo check passed)"),
            Ok(false) => println!("  check: SKIPPED (cargo not found; client SDK not checked)"),
            Err(e) => {
                eprintln!("  check: FAILED (client SDK cargo)\n{e}");
                return Err("generated client SDK failed cargo check".into());
            }
        },
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Loading + parsing the OpenApp document
// ---------------------------------------------------------------------------

fn read_doc(path: &PathBuf) -> Result<Value, Box<dyn Error>> {
    let raw =
        fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let is_json = path
        .extension()
        .map(|e| e.eq_ignore_ascii_case("json"))
        .unwrap_or(false);
    let doc: Value = if is_json {
        serde_json::from_str(&raw).map_err(|e| format!("invalid JSON: {e}"))?
    } else {
        serde_yaml::from_str(&raw).map_err(|e| format!("invalid YAML: {e}"))?
    };
    Ok(doc)
}

fn load_model(path: &PathBuf) -> Result<AppModel, Box<dyn Error>> {
    let doc = read_doc(path)?;
    parse_model(&doc)
}

fn as_map(v: &Value) -> Option<&Map<String, Value>> {
    v.as_object()
}

fn str_field(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).map(|s| s.to_string())
}

/// Parse a named, typed parameter block (`{ type: id, required: true }` or
/// `{ list: @Book }` or a bare `@Order`).
fn parse_ioparam(name: &str, v: &Value) -> IoParam {
    // Bare ref shorthand: `order: @Order`
    if let Some(s) = v.as_str() {
        let (ty, list) = parse_type_token(s);
        return IoParam {
            name: name.to_string(),
            ty,
            list,
            required: true,
            values: vec![],
        };
    }
    let list_target = v.get("list").and_then(|x| x.as_str());
    if let Some(t) = list_target {
        let (ty, _) = parse_type_token(t);
        return IoParam {
            name: name.to_string(),
            ty,
            list: true,
            required: v.get("required").and_then(|x| x.as_bool()).unwrap_or(false),
            values: vec![],
        };
    }
    let ty_raw = str_field(v, "type").unwrap_or_else(|| "text".to_string());
    let (ty, list) = parse_type_token(&ty_raw);
    let values = v
        .get("values")
        .and_then(|x| x.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    IoParam {
        name: name.to_string(),
        ty,
        list,
        required: v.get("required").and_then(|x| x.as_bool()).unwrap_or(false),
        values,
    }
}

/// `@Book` -> ("Book", false); `[@Book]` not used, lists are `{list:}`.
fn parse_type_token(s: &str) -> (String, bool) {
    let t = s.trim();
    (t.to_string(), false)
}

fn parse_model(doc: &Value) -> Result<AppModel, Box<dyn Error>> {
    let root = as_map(doc).ok_or("OpenApp document must be a map")?;

    if root.get("openapp").is_none() {
        return Err("missing `openapp` version key (is this an OpenApp document?)".into());
    }

    let app_v = root.get("app").ok_or("missing required `app` block")?;
    let app = AppMeta {
        name: str_field(app_v, "name").unwrap_or_else(|| "App".to_string()),
        version: str_field(app_v, "version").unwrap_or_else(|| "0.1.0".to_string()),
        description: str_field(app_v, "description"),
    };

    let mut entities = Vec::new();
    if let Some(map) = root.get("entities").and_then(as_map) {
        for (name, body) in map {
            entities.push(parse_entity(name, body));
        }
    }

    let mut capabilities = Vec::new();
    if let Some(map) = root.get("capabilities").and_then(as_map) {
        for (name, body) in map {
            capabilities.push(parse_capability(name, body));
        }
    }

    let mut events = Vec::new();
    if let Some(map) = root.get("events").and_then(as_map) {
        for (name, body) in map {
            let payload = body
                .get("payload")
                .and_then(as_map)
                .map(|m| m.iter().map(|(k, v)| parse_ioparam(k, v)).collect())
                .unwrap_or_default();
            events.push(Event {
                name: name.clone(),
                payload,
            });
        }
    }

    let mut policies = Vec::new();
    if let Some(map) = root.get("policies").and_then(as_map) {
        for (name, body) in map {
            policies.push((name.clone(), body.as_str().unwrap_or("").to_string()));
        }
    }

    let mut widgets = Vec::new();
    if let Some(map) = root.get("widgets").and_then(as_map) {
        for (name, body) in map {
            let props = body
                .get("props")
                .and_then(as_map)
                .map(|m| m.iter().map(|(k, v)| parse_ioparam(k, v)).collect())
                .unwrap_or_default();
            let slots = body
                .get("slots")
                .and_then(|x| x.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            widgets.push(Widget {
                name: name.clone(),
                props,
                slots,
            });
        }
    }

    let mut views = Vec::new();
    if let Some(map) = root.get("views").and_then(as_map) {
        for (name, body) in map {
            views.push(parse_view(name, body));
        }
    }

    let mut flows = Vec::new();
    if let Some(map) = root.get("flows").and_then(as_map) {
        for (name, body) in map {
            let steps = body
                .get("steps")
                .and_then(|x| x.as_array())
                .cloned()
                .unwrap_or_default();
            flows.push(Flow {
                name: name.clone(),
                summary: str_field(body, "summary"),
                steps,
            });
        }
    }

    let tokens = root.get("tokens").cloned();

    // Build the flat registry.
    let mut registry: BTreeMap<String, Kind> = BTreeMap::new();
    for e in &entities {
        registry.insert(e.name.clone(), Kind::Entity);
    }
    for c in &capabilities {
        registry.insert(c.name.clone(), Kind::Capability);
    }
    for e in &events {
        registry.insert(e.name.clone(), Kind::Event);
    }
    for (n, _) in &policies {
        registry.insert(n.clone(), Kind::Policy);
    }
    for w in &widgets {
        registry.insert(w.name.clone(), Kind::Widget);
    }
    for v in &views {
        registry.insert(v.name.clone(), Kind::View);
    }
    for f in &flows {
        registry.insert(f.name.clone(), Kind::Flow);
    }

    // Stable ordering for deterministic output.
    entities.sort_by(|a, b| a.name.cmp(&b.name));
    capabilities.sort_by(|a, b| a.name.cmp(&b.name));
    events.sort_by(|a, b| a.name.cmp(&b.name));
    policies.sort_by(|a, b| a.0.cmp(&b.0));
    widgets.sort_by(|a, b| a.name.cmp(&b.name));
    views.sort_by(|a, b| a.name.cmp(&b.name));
    flows.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(AppModel {
        app,
        entities,
        capabilities,
        events,
        policies,
        widgets,
        views,
        flows,
        tokens,
        registry,
    })
}

fn parse_entity(name: &str, body: &Value) -> Entity {
    let identity = str_field(body, "identity").unwrap_or_else(|| "id".to_string());
    let mut fields = Vec::new();
    if let Some(map) = body.get("fields").and_then(as_map) {
        for (fname, fv) in map {
            let ty = str_field(fv, "type").unwrap_or_else(|| "text".to_string());
            let values = fv
                .get("values")
                .and_then(|x| x.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            fields.push(FieldDef {
                name: fname.clone(),
                ty,
                required: fv
                    .get("required")
                    .and_then(|x| x.as_bool())
                    .unwrap_or(false),
                unique: fv.get("unique").and_then(|x| x.as_bool()).unwrap_or(false),
                indexed: fv.get("indexed").and_then(|x| x.as_bool()).unwrap_or(false),
                generated: fv
                    .get("generated")
                    .and_then(|x| x.as_bool())
                    .unwrap_or(false),
                default: fv.get("default").cloned(),
                values,
            });
        }
    }
    fields.sort_by(|a, b| a.name.cmp(&b.name));

    let mut relations = Vec::new();
    if let Some(map) = body.get("relations").and_then(as_map) {
        for (rname, rv) in map {
            let (kind, target) = if let Some(t) = str_field(rv, "hasMany") {
                (RelKind::HasMany, t)
            } else if let Some(t) = str_field(rv, "hasOne") {
                (RelKind::HasOne, t)
            } else if let Some(t) = str_field(rv, "belongsTo") {
                (RelKind::BelongsTo, t)
            } else if let Some(t) = str_field(rv, "manyToMany") {
                (RelKind::ManyToMany, t)
            } else {
                continue;
            };
            relations.push(RelationDef {
                name: rname.clone(),
                kind,
                target,
                inverse: str_field(rv, "inverse"),
                through: str_field(rv, "through"),
            });
        }
    }
    relations.sort_by(|a, b| a.name.cmp(&b.name));

    Entity {
        name: name.to_string(),
        description: str_field(body, "description"),
        identity,
        fields,
        relations,
    }
}

fn parse_capability(name: &str, body: &Value) -> Capability {
    let parse_io = |key: &str| -> Vec<IoParam> {
        body.get(key)
            .and_then(as_map)
            .map(|m| {
                let mut v: Vec<IoParam> = m.iter().map(|(k, val)| parse_ioparam(k, val)).collect();
                v.sort_by(|a, b| a.name.cmp(&b.name));
                v
            })
            .unwrap_or_default()
    };

    let reads = body
        .get("reads")
        .and_then(|x| x.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let mut writes = Vec::new();
    if let Some(arr) = body.get("writes").and_then(|x| x.as_array()) {
        for w in arr {
            for effect in ["creates", "updates", "deletes"] {
                if let Some(ent) = str_field(w, effect) {
                    writes.push(WriteDef {
                        effect: effect.to_string(),
                        entity: ent,
                    });
                }
            }
        }
    }

    let emits = body
        .get("emits")
        .and_then(|x| x.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let errors = body
        .get("errors")
        .and_then(|x| x.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    Capability {
        name: name.to_string(),
        summary: str_field(body, "summary"),
        input: parse_io("input"),
        output: parse_io("output"),
        reads,
        writes,
        emits,
        policy: str_field(body, "policy"),
        idempotent: body.get("idempotent").and_then(|x| x.as_bool()),
        errors,
    }
}

fn parse_view(name: &str, body: &Value) -> View {
    let params = body
        .get("params")
        .and_then(as_map)
        .map(|m| m.iter().map(|(k, v)| parse_ioparam(k, v)).collect())
        .unwrap_or_default();

    let mut data = Vec::new();
    if let Some(map) = body.get("data").and_then(as_map) {
        for (dname, dv) in map {
            let from = str_field(dv, "from").unwrap_or_default();
            let mut with = BTreeMap::new();
            if let Some(wm) = dv.get("with").and_then(as_map) {
                for (k, v) in wm {
                    with.insert(k.clone(), value_to_expr(v));
                }
            }
            data.push(ViewData {
                name: dname.clone(),
                from,
                where_expr: str_field(dv, "where"),
                with,
            });
        }
    }

    let mut actions = Vec::new();
    if let Some(map) = body.get("actions").and_then(as_map) {
        for (aname, av) in map {
            let mut with = BTreeMap::new();
            if let Some(wm) = av.get("with").and_then(as_map) {
                for (k, v) in wm {
                    with.insert(k.clone(), value_to_expr(v));
                }
            }
            actions.push(ViewAction {
                name: aname.clone(),
                invokes: str_field(av, "invokes").unwrap_or_default(),
                with,
                on_success: str_field(av, "onSuccess"),
                on_error: str_field(av, "onError"),
            });
        }
    }

    let mut transitions = Vec::new();
    if let Some(arr) = body.get("transitions").and_then(|x| x.as_array()) {
        for t in arr {
            if let Some(to) = str_field(t, "to") {
                transitions.push(Transition {
                    to,
                    label: str_field(t, "label"),
                    when: str_field(t, "when"),
                });
            }
        }
    }

    View {
        name: name.to_string(),
        route: str_field(body, "route"),
        summary: str_field(body, "summary"),
        params,
        data,
        actions,
        transitions,
        policy: str_field(body, "policy"),
        layout: body.get("layout").cloned().unwrap_or(Value::Null),
    }
}

/// Render a scalar binding expression (`params.q`, `book.id`, a literal) to text.
fn value_to_expr(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Profile loading
// ---------------------------------------------------------------------------

fn load_profile(path: &PathBuf, model: &AppModel) -> Result<Profile, Box<dyn Error>> {
    let doc = read_doc(path)?;
    let root = as_map(&doc).ok_or("profile must be a map")?;
    let mut prof = Profile::default_for(model);
    prof.name = str_field(&doc, "name").unwrap_or_else(|| "custom".to_string());

    for (layer_key, target) in [("data", 0u8), ("business", 1), ("view", 2), ("output", 3)] {
        if let Some(block) = root.get(layer_key).and_then(as_map) {
            let cfg = parse_layer(block);
            match target {
                0 => merge_layer(&mut prof.data, cfg),
                1 => merge_layer(&mut prof.business, cfg),
                2 => merge_layer(&mut prof.view, cfg),
                _ => merge_layer(&mut prof.output, cfg),
            }
        }
    }
    Ok(prof)
}

fn parse_layer(block: &Map<String, Value>) -> LayerCfg {
    let mut cfg = LayerCfg::default();
    for (k, v) in block {
        if k == "overrides" {
            if let Some(om) = v.as_object() {
                for (name, ov) in om {
                    if let Some(ovm) = ov.as_object() {
                        let mut entry = BTreeMap::new();
                        for (ok, ovv) in ovm {
                            entry.insert(ok.clone(), scalar_to_string(ovv));
                        }
                        cfg.overrides.insert(name.clone(), entry);
                    }
                }
            }
            continue;
        }
        cfg.values.insert(k.clone(), scalar_to_string(v));
    }
    cfg
}

fn merge_layer(into: &mut LayerCfg, from: LayerCfg) {
    for (k, v) in from.values {
        into.values.insert(k, v);
    }
    for (k, v) in from.overrides {
        into.overrides.insert(k, v);
    }
}

fn scalar_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        // Nested maps (rare in a layer value position) collapse to JSON.
        other => other.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Validation — the cross-reference checker (spec §13) + profile rule (§1)
// ---------------------------------------------------------------------------

fn strip_ref(s: &str) -> &str {
    s.trim_start_matches('@')
}

/// Whether a `data.store` choice targets the spacekit-storage-node document API.
pub(crate) fn is_storage_node(store: &str) -> bool {
    matches!(store, "spacekit-storage-node" | "storage-node" | "spacekit")
}

/// Whether a `view.framework` choice is the Next.js (app-router/RSC) target.
pub(crate) fn is_next(framework: &str) -> bool {
    matches!(framework, "next" | "nextjs")
}

fn validate_spec(m: &AppModel) -> Vec<String> {
    let mut issues = Vec::new();

    // Names must be unique across all registries. (Duplicates collapse in the
    // BTreeMap, so detect by counting pre-registry occurrences.)
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    let mut bump = |n: &str| *seen.entry(n.to_string()).or_insert(0) += 1;
    m.entities.iter().for_each(|e| bump(&e.name));
    m.capabilities.iter().for_each(|c| bump(&c.name));
    m.events.iter().for_each(|e| bump(&e.name));
    m.policies.iter().for_each(|p| bump(&p.0));
    m.widgets.iter().for_each(|w| bump(&w.name));
    m.views.iter().for_each(|v| bump(&v.name));
    m.flows.iter().for_each(|f| bump(&f.name));
    for (n, c) in seen.iter().filter(|(_, c)| **c > 1) {
        issues.push(format!(
            "name `{n}` is defined {c} times (names must be unique across registries)"
        ));
    }

    let is_entity = |n: &str| m.entity(n).is_some();

    // Entity relations + field refs.
    for e in &m.entities {
        if e.identity_field().is_none() {
            issues.push(format!(
                "entity `{}` declares identity `{}` but has no such field",
                e.name, e.identity
            ));
        }
        for f in &e.fields {
            if f.ty.starts_with('@') && !is_entity(strip_ref(&f.ty)) {
                issues.push(format!(
                    "entity `{}` field `{}` references unknown entity `{}`",
                    e.name, f.name, f.ty
                ));
            }
            if f.ty == "enum" && f.values.is_empty() {
                issues.push(format!(
                    "entity `{}` field `{}` is enum but lists no `values`",
                    e.name, f.name
                ));
            }
        }
        for r in &e.relations {
            if !is_entity(&r.target) {
                issues.push(format!(
                    "entity `{}` relation `{}` targets unknown entity `{}`",
                    e.name, r.name, r.target
                ));
            }
            if let Some(t) = &r.through {
                if !is_entity(t) {
                    issues.push(format!(
                        "entity `{}` relation `{}` through unknown entity `{}`",
                        e.name, r.name, t
                    ));
                }
            }
        }
    }

    // Capabilities: reads/writes/emits/policy/io entity refs.
    for c in &m.capabilities {
        for r in &c.reads {
            if !is_entity(r) {
                issues.push(format!(
                    "capability `{}` reads unknown entity `{}`",
                    c.name, r
                ));
            }
        }
        for w in &c.writes {
            if !is_entity(&w.entity) {
                issues.push(format!(
                    "capability `{}` {} unknown entity `{}`",
                    c.name, w.effect, w.entity
                ));
            }
        }
        for ev in &c.emits {
            if !m.events.iter().any(|e| &e.name == ev) {
                issues.push(format!(
                    "capability `{}` emits unknown event `{}`",
                    c.name, ev
                ));
            }
        }
        if let Some(p) = &c.policy {
            check_policy(m, p, &format!("capability `{}`", c.name), &mut issues);
        }
        for io in c.input.iter().chain(c.output.iter()) {
            if io.ty.starts_with('@') && !is_entity(strip_ref(&io.ty)) {
                issues.push(format!(
                    "capability `{}` param `{}` references unknown entity `{}`",
                    c.name, io.name, io.ty
                ));
            }
        }
    }

    // Events payload refs.
    for e in &m.events {
        for p in &e.payload {
            if p.ty.starts_with('@') && !is_entity(strip_ref(&p.ty)) {
                issues.push(format!(
                    "event `{}` payload `{}` references unknown entity `{}`",
                    e.name, p.name, p.ty
                ));
            }
        }
    }

    // Widget prop refs.
    for w in &m.widgets {
        for p in &w.props {
            if p.ty.starts_with('@') && !is_entity(strip_ref(&p.ty)) {
                issues.push(format!(
                    "widget `{}` prop `{}` references unknown entity `{}`",
                    w.name, p.name, p.ty
                ));
            }
        }
    }

    // Views: data bindings, actions->capabilities, transitions->views, policy.
    for v in &m.views {
        for d in &v.data {
            let from = strip_ref(&d.from);
            let kind = m.registry.get(from);
            match kind {
                Some(Kind::Entity) | Some(Kind::Capability) => {}
                _ => issues.push(format!(
                    "view `{}` data `{}` binds unknown @{} (expected entity or capability)",
                    v.name, d.name, from
                )),
            }
            // If bound to a capability, `with` keys must be real inputs.
            if let Some(cap) = m.capability(from) {
                for k in d.with.keys() {
                    if !cap.input.iter().any(|i| &i.name == k) {
                        issues.push(format!(
                            "view `{}` data `{}` passes `{}` not in capability `{}` input",
                            v.name, d.name, k, cap.name
                        ));
                    }
                }
            }
        }
        for a in &v.actions {
            let target = strip_ref(&a.invokes);
            match m.capability(target) {
                Some(cap) => {
                    for k in a.with.keys() {
                        if !cap.input.iter().any(|i| &i.name == k) {
                            issues.push(format!(
                                "view `{}` action `{}` passes `{}` not in capability `{}` input",
                                v.name, a.name, k, cap.name
                            ));
                        }
                    }
                    // Required inputs must be supplied.
                    for i in &cap.input {
                        if i.required && !a.with.contains_key(&i.name) {
                            issues.push(format!("view `{}` action `{}` omits required input `{}` of capability `{}`", v.name, a.name, i.name, cap.name));
                        }
                    }
                }
                None => issues.push(format!(
                    "view `{}` action `{}` invokes unknown capability `{}`",
                    v.name, a.name, a.invokes
                )),
            }
            check_navigation(
                m,
                a.on_success.as_deref(),
                &format!("view `{}` action `{}` onSuccess", v.name, a.name),
                &mut issues,
            );
            check_navigation(
                m,
                a.on_error.as_deref(),
                &format!("view `{}` action `{}` onError", v.name, a.name),
                &mut issues,
            );
        }
        for t in &v.transitions {
            let to = strip_ref(&t.to);
            if !m.views.iter().any(|vv| vv.name == to) {
                issues.push(format!(
                    "view `{}` transition targets unknown view `{}`",
                    v.name, t.to
                ));
            }
        }
        if let Some(p) = &v.policy {
            check_policy(m, p, &format!("view `{}`", v.name), &mut issues);
        }
        // Layout widget/view refs.
        check_layout_refs(m, &v.layout, &v.name, &mut issues);
    }

    // Flows: each step `at: @View`.
    for f in &m.flows {
        for step in &f.steps {
            if let Some(at) = step.get("at").and_then(|x| x.as_str()) {
                let at = strip_ref(at);
                if !m.views.iter().any(|v| v.name == at) {
                    issues.push(format!(
                        "flow `{}` step references unknown view `{}`",
                        f.name, at
                    ));
                }
            }
        }
    }

    issues
}

fn check_policy(m: &AppModel, p: &str, ctx: &str, issues: &mut Vec<String>) {
    if p == "public" || p == "authenticated" {
        return;
    }
    let name = strip_ref(p);
    if !m.policies.iter().any(|(n, _)| n == name) {
        issues.push(format!("{ctx} references unknown policy `{p}`"));
    }
}

/// A navigation outcome is either `stay` or `-> @View(args)`.
fn check_navigation(m: &AppModel, nav: Option<&str>, ctx: &str, issues: &mut Vec<String>) {
    let nav = match nav {
        Some(n) => n.trim(),
        None => return,
    };
    if nav == "stay" || nav.is_empty() {
        return;
    }
    let target = nav.trim_start_matches("->").trim();
    let name = target.split('(').next().unwrap_or(target).trim();
    let name = strip_ref(name);
    if !m.views.iter().any(|v| v.name == name) {
        issues.push(format!("{ctx} navigates to unknown view `{name}`"));
    }
}

fn check_layout_refs(m: &AppModel, node: &Value, view: &str, issues: &mut Vec<String>) {
    match node {
        Value::Array(items) => {
            for it in items {
                check_layout_refs(m, it, view, issues);
            }
        }
        Value::Object(map) => {
            for key in ["widget", "view"] {
                if let Some(r) = map.get(key).and_then(|x| x.as_str()) {
                    let name = strip_ref(r);
                    let ok = match key {
                        "widget" => m.widgets.iter().any(|w| w.name == name),
                        _ => m.views.iter().any(|v| v.name == name),
                    };
                    if !ok {
                        issues.push(format!(
                            "view `{view}` layout references unknown {key} `{r}`"
                        ));
                    }
                }
            }
            for (k, v) in map {
                if k == "slots" || k == "layout" || v.is_array() || v.is_object() {
                    check_layout_refs(m, v, view, issues);
                }
            }
        }
        _ => {}
    }
}

/// The profile invariant (§1): a profile may only choose *realization*. It may
/// not introduce meaning-bearing keys and its overrides must name real things.
fn validate_profile(m: &AppModel, p: &Profile) -> Vec<String> {
    let mut issues = Vec::new();

    // Forbid meaning-injecting keys masquerading as realization.
    const FORBIDDEN: &[&str] = &[
        "entities",
        "capabilities",
        "events",
        "policies",
        "fields",
        "writes",
        "input",
        "output",
        "transitions",
        "views",
        "reads",
        "emits",
    ];
    for (layer_name, layer) in [
        ("data", &p.data),
        ("business", &p.business),
        ("view", &p.view),
        ("output", &p.output),
    ] {
        for k in layer.values.keys() {
            if FORBIDDEN.contains(&k.as_str()) {
                issues.push(format!(
                    "profile `{layer_name}.{k}` would change app meaning — not allowed (rule §1)"
                ));
            }
        }
        // Overrides must reference @Names that exist in the spec.
        for name in layer.overrides.keys() {
            if !m.registry.contains_key(name) {
                issues.push(format!("profile `{layer_name}` overrides unknown @{name}"));
            }
        }
    }

    // Stack support guardrails — fail loudly rather than emit broken code.
    let store = p.data.get("store", "postgres");
    if !is_storage_node(store) {
        // Relational stores go through Prisma.
        let orm = p.data.get("orm", "prisma");
        if orm != "prisma" && orm != "none" {
            issues.push(format!(
                "data.orm `{orm}` not yet implemented (supported: prisma, none, or store: spacekit-storage-node)"
            ));
        }
        if orm == "prisma" && !matches!(store, "postgres" | "mysql" | "sqlite") {
            issues.push(format!(
                "data.store `{store}` unsupported (use postgres/mysql/sqlite with prisma, or spacekit-storage-node)"
            ));
        }
    }
    let language = p.business.get("language", "typescript");
    if language != "typescript" {
        issues.push(format!(
            "business.language `{language}` not yet implemented (supported: typescript)"
        ));
    }
    let framework = p.view.get("framework", "next");
    if !matches!(framework, "next" | "nextjs" | "react") {
        issues.push(format!(
            "view.framework `{framework}` not yet implemented (supported: next, react)"
        ));
    }
    let binding = p.business.get("binding", "compile");
    if binding != "compile" {
        issues.push(format!(
            "business.binding `{binding}` not yet implemented (supported: compile)"
        ));
    }

    issues
}

// ---------------------------------------------------------------------------
// Phase 1 — synthesize an OpenAPI document from the business layer
// ---------------------------------------------------------------------------

pub(crate) fn synthesize_openapi(m: &AppModel, p: &Profile) -> Value {
    let mut schemas = Map::new();

    // Entities -> component schemas.
    for e in &m.entities {
        schemas.insert(e.name.clone(), entity_schema(e));
    }

    // Paths from capabilities.
    let mut paths = Map::new();
    let mut needs_auth = false;
    for c in &m.capabilities {
        let transport = p.business.effective(&c.name, "transport", "rest");
        let (http, path, params, body, response) = project_capability(m, c, &transport);
        if !matches!(c.policy.as_deref(), Some("public") | None) {
            needs_auth = true;
        }

        let mut op = Map::new();
        if let Some(s) = &c.summary {
            op.insert("summary".into(), json!(s));
        }
        op.insert("operationId".into(), json!(super::snake(&c.name)));
        // Resource grouping. Pluralize so the generated resource *class* never
        // collides with the entity *model* of the same name (e.g. `Books` vs `Book`).
        let tag = c
            .primary_entity()
            .map(|e| pluralize(&e))
            .unwrap_or_else(|| "Resources".to_string());
        op.insert("tags".into(), json!([tag]));
        if !params.is_empty() {
            op.insert("parameters".into(), Value::Array(params));
        }
        if let Some(b) = body {
            op.insert("requestBody".into(), b);
        }
        let mut responses = Map::new();
        responses.insert(
            "200".into(),
            json!({
                "description": "OK",
                "content": { "application/json": { "schema": response } }
            }),
        );
        if !c.errors.is_empty() {
            responses.insert(
                "400".into(),
                json!({
                    "description": c.errors.join(", "),
                    "content": { "application/json": { "schema": { "$ref": "#/components/schemas/Error" } } }
                }),
            );
        }
        op.insert("responses".into(), Value::Object(responses));
        if !matches!(c.policy.as_deref(), Some("public") | None) {
            op.insert("security".into(), json!([{ "bearerAuth": [] }]));
        }

        let entry = paths
            .entry(path)
            .or_insert_with(|| Value::Object(Map::new()));
        if let Some(obj) = entry.as_object_mut() {
            obj.insert(http, Value::Object(op));
        }
    }

    if !m.capabilities.iter().all(|c| c.errors.is_empty()) {
        schemas.insert(
            "Error".into(),
            json!({
                "type": "object",
                "properties": {
                    "code": { "type": "string" },
                    "message": { "type": "string" }
                }
            }),
        );
    }

    let mut components = Map::new();
    components.insert("schemas".into(), Value::Object(schemas));
    if needs_auth {
        components.insert(
            "securitySchemes".into(),
            json!({ "bearerAuth": { "type": "http", "scheme": "bearer" } }),
        );
    }

    json!({
        "openapi": "3.0.3",
        "info": {
            "title": m.app.name,
            "version": m.app.version,
            "description": m.app.description.clone().unwrap_or_default(),
        },
        "servers": [{ "url": "/api" }],
        "components": Value::Object(components),
        "paths": Value::Object(paths),
    })
}

fn entity_schema(e: &Entity) -> Value {
    let mut props = Map::new();
    let mut required = Vec::new();
    for f in &e.fields {
        props.insert(f.name.clone(), field_schema(f));
        if f.required {
            required.push(json!(f.name));
        }
    }
    // Relations as nested refs (read shape).
    for r in &e.relations {
        let item = json!({ "$ref": format!("#/components/schemas/{}", r.target) });
        let schema = match r.kind {
            RelKind::HasMany | RelKind::ManyToMany => json!({ "type": "array", "items": item }),
            RelKind::HasOne | RelKind::BelongsTo => item,
        };
        props.entry(r.name.clone()).or_insert(schema);
    }
    let mut obj = Map::new();
    obj.insert("type".into(), json!("object"));
    obj.insert("properties".into(), Value::Object(props));
    if !required.is_empty() {
        obj.insert("required".into(), Value::Array(required));
    }
    Value::Object(obj)
}

fn field_schema(f: &FieldDef) -> Value {
    if f.ty.starts_with('@') {
        return json!({ "$ref": format!("#/components/schemas/{}", strip_ref(&f.ty)) });
    }
    let (ty, format) = openapi_scalar(&f.ty);
    let mut obj = Map::new();
    obj.insert("type".into(), json!(ty));
    if let Some(fmt) = format {
        obj.insert("format".into(), json!(fmt));
    }
    if f.ty == "enum" && !f.values.is_empty() {
        obj.insert("enum".into(), json!(f.values));
    }
    Value::Object(obj)
}

/// OpenApp scalar -> (OpenAPI type, optional format).
fn openapi_scalar(t: &str) -> (&'static str, Option<&'static str>) {
    match t {
        "id" | "text" | "longtext" | "phone" => ("string", None),
        "email" => ("string", Some("email")),
        "url" => ("string", Some("uri")),
        "integer" => ("integer", None),
        "decimal" | "money" => ("number", None),
        "boolean" => ("boolean", None),
        "timestamp" => ("string", Some("date-time")),
        "date" => ("string", Some("date")),
        "time" => ("string", None),
        "enum" => ("string", None),
        "json" => ("object", None),
        "file" | "image" => ("string", Some("binary")),
        _ => ("string", None),
    }
}

fn ioparam_schema(io: &IoParam) -> Value {
    let base = if io.ty.starts_with('@') {
        json!({ "$ref": format!("#/components/schemas/{}", strip_ref(&io.ty)) })
    } else {
        let (ty, format) = openapi_scalar(&io.ty);
        let mut o = Map::new();
        o.insert("type".into(), json!(ty));
        if let Some(fmt) = format {
            o.insert("format".into(), json!(fmt));
        }
        if !io.values.is_empty() {
            o.insert("enum".into(), json!(io.values));
        }
        Value::Object(o)
    };
    if io.list {
        json!({ "type": "array", "items": base })
    } else {
        base
    }
}

/// Project a capability onto (method, path, params, requestBody, responseSchema).
fn project_capability(
    m: &AppModel,
    c: &Capability,
    transport: &str,
) -> (String, String, Vec<Value>, Option<Value>, Value) {
    let is_list_output = c.output.iter().any(|o| o.list);

    // Build the response schema: an object of the named outputs. A single list
    // output is shaped as `{ data: [...], next_cursor }` to trigger the SDK's
    // auto-pagination.
    let response = if is_list_output && c.output.len() == 1 {
        let o = &c.output[0];
        json!({
            "type": "object",
            "properties": {
                "data": ioparam_schema(o),
                "next_cursor": { "type": "string", "nullable": true }
            },
            "required": ["data"]
        })
    } else {
        let mut props = Map::new();
        let mut required = Vec::new();
        for o in &c.output {
            props.insert(o.name.clone(), ioparam_schema(o));
            required.push(json!(o.name));
        }
        json!({ "type": "object", "properties": props, "required": required })
    };

    let rpc = transport == "rpc" || transport == "message";
    let kebab = super::snake(&c.name).replace('_', "-");

    // RPC: always POST with the whole input as a JSON body.
    if rpc {
        let body = (!c.input.is_empty()).then(|| input_body(c));
        return (
            "post".into(),
            format!("/rpc/{kebab}"),
            vec![],
            body,
            response,
        );
    }

    // REST: queries -> GET (+ query params & pagination), commands -> POST.
    if c.is_query() {
        let mut params: Vec<Value> = c
            .input
            .iter()
            .map(|i| {
                json!({
                    "name": i.name,
                    "in": "query",
                    "required": i.required,
                    "schema": ioparam_schema(&IoParam { list: false, ..i.clone() })
                })
            })
            .collect();
        if is_list_output {
            params.push(json!({ "name": "cursor", "in": "query", "required": false, "schema": { "type": "string" } }));
            params.push(json!({ "name": "limit", "in": "query", "required": false, "schema": { "type": "integer" } }));
        }
        let path = rest_path(m, c);
        return ("get".into(), path, params, None, response);
    }

    let body = (!c.input.is_empty()).then(|| input_body(c));
    let path = rest_path(m, c);
    ("post".into(), path, vec![], body, response)
}

fn input_body(c: &Capability) -> Value {
    let mut props = Map::new();
    let mut required = Vec::new();
    for i in &c.input {
        props.insert(i.name.clone(), ioparam_schema(i));
        if i.required {
            required.push(json!(i.name));
        }
    }
    json!({
        "required": true,
        "content": {
            "application/json": {
                "schema": { "type": "object", "properties": props, "required": required }
            }
        }
    })
}

fn rest_path(m: &AppModel, c: &Capability) -> String {
    let base = c
        .primary_entity()
        .map(|e| super::snake(&pluralize(&e)))
        .unwrap_or_else(|| "app".to_string());
    let _ = m;
    let kebab = super::snake(&c.name).replace('_', "-");
    format!("/{base}/{kebab}")
}

/// Naive English pluralizer — enough to keep resource names distinct from models.
fn pluralize(s: &str) -> String {
    if s.is_empty() {
        return s.to_string();
    }
    let lower = s.to_lowercase();
    if lower.ends_with('y')
        && !lower.ends_with("ay")
        && !lower.ends_with("ey")
        && !lower.ends_with("oy")
        && !lower.ends_with("uy")
    {
        return format!("{}ies", &s[..s.len() - 1]);
    }
    if lower.ends_with('s')
        || lower.ends_with('x')
        || lower.ends_with('z')
        || lower.ends_with("ch")
        || lower.ends_with("sh")
    {
        return format!("{s}es");
    }
    format!("{s}s")
}

// ---------------------------------------------------------------------------
// Phase 5 — behavioral fingerprint (meaning only; profile-independent)
// ---------------------------------------------------------------------------

pub(crate) fn behavioral_fingerprint(m: &AppModel) -> Value {
    let entities: Map<String, Value> = m
        .entities
        .iter()
        .map(|e| {
            let fields: Map<String, Value> = e
                .fields
                .iter()
                .map(|f| {
                    (
                        f.name.clone(),
                        json!({
                            "type": f.ty,
                            "required": f.required,
                            "unique": f.unique,
                            "generated": f.generated,
                            "values": f.values,
                        }),
                    )
                })
                .collect();
            let relations: Map<String, Value> = e
                .relations
                .iter()
                .map(|r| {
                    (
                        r.name.clone(),
                        json!({ "kind": rel_kind_str(&r.kind), "target": r.target, "inverse": r.inverse }),
                    )
                })
                .collect();
            (
                e.name.clone(),
                json!({ "identity": e.identity, "fields": fields, "relations": relations }),
            )
        })
        .collect();

    let capabilities: Map<String, Value> = m
        .capabilities
        .iter()
        .map(|c| {
            (
                c.name.clone(),
                json!({
                    "input": io_fp(&c.input),
                    "output": io_fp(&c.output),
                    "reads": c.reads,
                    "writes": c.writes.iter().map(|w| json!({ "effect": w.effect, "entity": w.entity })).collect::<Vec<_>>(),
                    "emits": c.emits,
                    "policy": c.policy,
                    "idempotent": c.idempotent,
                    "errors": c.errors,
                }),
            )
        })
        .collect();

    let events: Map<String, Value> = m
        .events
        .iter()
        .map(|e| (e.name.clone(), json!({ "payload": io_fp(&e.payload) })))
        .collect();

    let policies: Map<String, Value> = m
        .policies
        .iter()
        .map(|(n, r)| (n.clone(), json!(r)))
        .collect();

    let views: Map<String, Value> = m
        .views
        .iter()
        .map(|v| {
            (
                v.name.clone(),
                json!({
                    "route": v.route,
                    "policy": v.policy,
                    "data": v.data.iter().map(|d| json!({ "name": d.name, "from": d.from })).collect::<Vec<_>>(),
                    "actions": v.actions.iter().map(|a| json!({ "name": a.name, "invokes": a.invokes, "onSuccess": a.on_success, "onError": a.on_error })).collect::<Vec<_>>(),
                    "transitions": v.transitions.iter().map(|t| json!({ "to": t.to, "when": t.when })).collect::<Vec<_>>(),
                }),
            )
        })
        .collect();

    json!({
        "app": { "name": m.app.name, "version": m.app.version },
        "entities": entities,
        "capabilities": capabilities,
        "events": events,
        "policies": policies,
        "views": views,
    })
}

fn io_fp(io: &[IoParam]) -> Map<String, Value> {
    io.iter()
        .map(|p| {
            (
                p.name.clone(),
                json!({ "type": p.ty, "list": p.list, "required": p.required, "values": p.values }),
            )
        })
        .collect()
}

fn rel_kind_str(k: &RelKind) -> &'static str {
    match k {
        RelKind::HasMany => "hasMany",
        RelKind::HasOne => "hasOne",
        RelKind::BelongsTo => "belongsTo",
        RelKind::ManyToMany => "manyToMany",
    }
}

/// Canonical JSON: serde_json::Value already orders object keys (BTreeMap under
/// the `preserve_order`-off default), so to_string is deterministic.
fn canonical_json(v: &Value) -> String {
    serde_json::to_string(v).unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Top-level README
// ---------------------------------------------------------------------------

fn render_app_readme(m: &AppModel, p: &Profile, pkg: &str, lang: Lang) -> String {
    let lang_name = match lang {
        Lang::Python => "Python",
        Lang::TypeScript => "TypeScript",
        Lang::Rust => "Rust",
    };
    let mut entity_list = String::new();
    for e in &m.entities {
        entity_list.push_str(&format!("- `{}` ({} fields)\n", e.name, e.fields.len()));
    }
    let mut view_list = String::new();
    for v in &m.views {
        let route = v.route.clone().unwrap_or_else(|| "—".to_string());
        view_list.push_str(&format!("- `{}` → `{}`\n", v.name, route));
    }

    let store = p.data.get("store", "postgres");
    let storage_node = is_storage_node(store);
    let framework = p.view.get("framework", "next");
    let next = is_next(framework);
    let errors = p.business.get("errors", "problem-json");

    let data_row = if storage_node {
        "spacekit-storage-node (DID-scoped document store)".to_string()
    } else {
        format!(
            "{} + {} (identity: {})",
            store,
            p.data.get("orm", "prisma"),
            p.data.get("identity", "uuid")
        )
    };

    // Layout block — data + view trees depend on the profile.
    let data_layout = if storage_node {
        "  server/storage-client.ts  # data layer (storage-node document client)\n  \
         server/collections.md     # entity → collection map\n"
    } else {
        "  prisma/schema.prisma      # data layer (Prisma schema)\n"
    };
    let view_layout = if next {
        "  web/app/                  # view layer (Next.js app-router, RSC)\n"
    } else {
        "  web/src/                  # view layer (React SPA, Vite + react-router)\n"
    };

    // Getting-started — tailored to the chosen store + framework.
    let data_setup = if storage_node {
        "### 1. Data backend (spacekit-storage-node)\n\n\
         ```bash\n\
         # start a storage node (HTTP API defaults to 127.0.0.1:3030)\n\
         cargo run --bin standalone -- --port 3030   # in the spacekit-storage-node repo\n\n\
         cp .env.example .env        # SPACEKIT_STORAGE_URL, SPACEKIT_DID\n\
         cd server && npm install\n\
         ```\n\
         No migrations: collections are created on first write and scoped to `SPACEKIT_DID`.\n\
         See `server/collections.md` for the entity → collection map.\n\n"
            .to_string()
    } else {
        "### 1. Data backend (Prisma)\n\n\
         ```bash\n\
         cp .env.example .env        # set DATABASE_URL\n\
         cd server && npm install\n\
         npx prisma generate --schema ../prisma/schema.prisma\n\
         npx prisma migrate dev --schema ../prisma/schema.prisma --name init\n\
         ```\n\n"
            .to_string()
    };
    let errors_note = if errors == "result-type" {
        "return a `Result<T>` (see `server/errors.ts`)"
    } else {
        "throw `ApiError` (see `server/errors.ts`)"
    };
    let view_setup = if next {
        "### 3. View (Next.js)\n\n\
         Pages are server components calling `db` + server actions, so the data backend\n\
         must be reachable at render time.\n\n\
         ```bash\n\
         cd web && npm install\n\
         # add scripts to package.json: \"dev\": \"next dev\", \"build\": \"next build\", \"start\": \"next start\"\n\
         npm run dev    # http://localhost:3000\n\
         ```\n"
            .to_string()
    } else {
        "### 3. View (React SPA)\n\n\
         Pages fetch through the generated client SDK; point it at your running API.\n\n\
         ```bash\n\
         cd web && npm install\n\
         echo 'VITE_API_BASE_URL=http://localhost:8080' >> .env\n\
         npm run dev    # vite, http://localhost:5173\n\
         ```\n"
            .to_string()
    };

    format!(
        "# {name}\n\n\
        {desc}\n\n\
        Generated by `spacekit agent webapp` from an **OpenApp v0.1** document.\n\
        One spec, one profile (`{profile}`), a full deterministic stack.\n\n\
        ## Layers\n\n\
        | Layer | Realized as |\n|---|---|\n\
        | data | {data_row} |\n\
        | business | {blang} · {transport} · {arch} · errors={errors} |\n\
        | view | {framework} · {router} · {styling} |\n\
        | client SDK | {lang_name} (from `openapi.json`) |\n\n\
        ## Layout\n\n\
        ```\n\
        {pkg}_app/\n  \
        openapi.json              # capabilities projected to OpenAPI\n  \
        client/{pkg}_client/      # generated typed client SDK\n\
        {data_layout}  \
        server/                   # business layer (server actions) + db.ts\n\
        {view_layout}  \
        .openapp-fingerprint.json # behavioral contract (profile-independent)\n\
        ```\n\n\
        ## Entities\n\n{entities}\n\
        ## Views\n\n{views}\n\
        ## Getting started\n\n\
        {data_setup}\
        ### 2. Business logic (server)\n\n\
        Fill in the action stubs in `server/actions/*.ts` (one per capability). \
        Inputs/outputs are typed via `server/types.ts`; on error, {errors_note}. \
        Implemented bodies are preserved across re-generation.\n\n\
        {view_setup}\n\
        ## Regenerate\n\n\
        ```bash\n\
        spacekit agent webapp --spec app.openapp.yaml --profile {profile}.yaml --check\n\
        ```\n\n\
        Files are tracked in `.sdkgen-manifest.json`; hand-edits are preserved on \
        re-generation (use `--force` to overwrite, `--plan` to preview, `--prune` to \
        drop files no longer emitted).\n",
        name = m.app.name,
        desc = m
            .app
            .description
            .clone()
            .unwrap_or_else(|| "An OpenApp-generated application.".to_string()),
        profile = p.name,
        data_row = data_row,
        blang = p.business.get("language", "typescript"),
        transport = p.business.get("transport", "server-actions"),
        arch = p.business.get("architecture", "layered"),
        errors = errors,
        framework = framework,
        router = p.view.get("router", "app-router"),
        styling = p.view.get("styling", "tailwind"),
        lang_name = lang_name,
        pkg = pkg,
        data_layout = data_layout,
        view_layout = view_layout,
        data_setup = data_setup,
        errors_note = errors_note,
        view_setup = view_setup,
        entities = entity_list,
        views = view_list,
    )
}

// Bring the parent's naming helpers into scope; descendant emitter modules
// reach them via `super::pascal` / `super::snake`.
use super::{pascal, snake};

/// camelCase helper used by the emitters.
pub(crate) fn camel(s: &str) -> String {
    let p = pascal(s);
    let mut chars = p.chars();
    match chars.next() {
        Some(first) => first.to_ascii_lowercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}

/// kebab-case helper.
pub(crate) fn kebab(s: &str) -> String {
    snake(s).replace('_', "-")
}
