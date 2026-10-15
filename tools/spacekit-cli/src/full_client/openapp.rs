//! SpaceKit App generation — deterministic web-application generation
//! (app spec + profile -> data, business and view layers).
//!
//! Pipeline:
//! ```text
//!   app.yaml (spacekit: "0.2") ─┐
//!                               ├─► AppModel IR ─► validate(spec) ─► validate(spec × profile)
//!   profile.yaml (+ --env) ─────┘                                          │
//!                          ┌──────────────┬──────────────────┬─────────────┴────┐
//!                          │ data emitter │ business emitter  │ view emitter     │
//!                          │ (Prisma or   │ (TS capabilities, │ (Next.js or      │
//!                          │ storage node)│ policies, events, │  React SPA)      │
//!                          │              │ workflows)        │                  │
//!                          └──────────────┴──────────────────┴──────────────────┘
//!                                         │
//!                          synthesize OpenAPI ─► (reuse) parse_openapi + emit_* ─► client SDK
//! ```
//!
//! The transport-neutral `capabilities` are projected to an OpenAPI document,
//! which the existing Python/TypeScript/Rust emitters turn into a typed client.
//! The profile chooses the stack and patterns but, by rule, never the *meaning*
//! of the app (Profile §1). Documents with the legacy `openapp: "0.1"` root key
//! are still accepted.
//!
//! Spec:    `sdks/spacekit-apps/SPACEKITAPP-SPEC.md` (v0.2)
//! Profile: `sdks/spacekit-apps/SPACEKITAPP-PROFILE.md` (v0.2)

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
#[path = "openapp_policy.rs"]
pub(crate) mod policy;
#[path = "openapp_workflow.rs"]
mod workflow;
#[path = "openapp_diff.rs"]
pub(crate) mod diff;
#[path = "openapp_assist.rs"]
pub(crate) mod assist;
#[cfg(test)]
#[path = "openapp_tests.rs"]
mod tests;

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
    /// Profile environment layer to apply (`environments.<env>`), e.g. `dev`.
    pub env: Option<String>,
    /// Regenerate even when the spec changed incompatibly without a major
    /// `app.version` bump (Spec §14).
    pub allow_breaking: bool,
    /// Brain project (`*.gf.toml`) whose certified routing suggests a design
    /// pattern for each capability handler stub.
    pub assist: Option<PathBuf>,
}

impl AppArgs {
    pub fn new(spec: PathBuf) -> Self {
        AppArgs {
            spec,
            profile: None,
            out: None,
            sdk_lang: None,
            check: false,
            plan: false,
            prune: false,
            force: false,
            conformance: None,
            env: None,
            allow_breaking: false,
            assist: None,
        }
    }
}

// ---------------------------------------------------------------------------
// IR — the typed AppModel (one flat @Name registry, mirroring the spec)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub(crate) struct AppMeta {
    pub name: String,
    pub version: String,
    /// Spec format version from the root key (`spacekit: "0.2"`, or legacy `openapp`).
    pub spec_version: String,
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
    /// Validation rules (Spec §5): enforced at the data boundary and on forms.
    pub rules: Rules,
    /// For `file`/`image` fields: the logical storage bucket the bytes live in.
    pub bucket: Option<String>,
}

/// Field validation rules (Spec v0.2 §5). Properties, not patterns.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Rules {
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub min_length: Option<u64>,
    pub max_length: Option<u64>,
    pub pattern: Option<String>,
    pub one_of: Vec<Value>,
}

impl Rules {
    pub fn is_empty(&self) -> bool {
        *self == Rules::default()
    }
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
    /// Optional lifecycle hint (`read`, `upload`, `offchain`, …). Informational.
    pub kind: Option<String>,
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
    /// Outcome keys other than `onSuccess`/`onError` (not realized; reported).
    pub other_outcomes: Vec<String>,
}

/// A declared view state (Spec v0.2 §10): `loading`, `empty`, or `error`.
#[derive(Debug, Clone)]
pub(crate) struct ViewState {
    pub name: String,
    /// Boolean expression (only meaningful for `empty`).
    pub when: Option<String>,
    /// The widget shown (`@Spinner`).
    pub show: Option<String>,
    pub props: BTreeMap<String, Value>,
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
    pub states: Vec<ViewState>,
    pub layout: Value, // kept raw; the view emitter walks it
}

#[derive(Debug, Clone)]
pub(crate) struct Flow {
    pub name: String,
    pub summary: Option<String>,
    pub steps: Vec<Value>,
}

/// One step of a workflow (Spec v0.2 §7).
#[derive(Debug, Clone)]
pub(crate) struct WorkflowStep {
    pub capability: String,
    pub with: BTreeMap<String, String>,
    pub compensate: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct Workflow {
    pub name: String,
    pub summary: Option<String>,
    /// Triggering event.
    pub on: String,
    pub steps: Vec<WorkflowStep>,
    pub on_complete: Option<String>,
    pub on_failure: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct Policy {
    pub name: String,
    pub source: String,
    /// Parsed expression, or the parse error (reported by validation).
    pub expr: Result<policy::Expr, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Kind {
    Entity,
    Capability,
    Event,
    Workflow,
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
    pub workflows: Vec<Workflow>,
    pub policies: Vec<Policy>,
    pub widgets: Vec<Widget>,
    pub views: Vec<View>,
    pub flows: Vec<Flow>,
    pub tokens: Option<Value>,
    /// i18n registry: key -> locale -> string.
    pub text: BTreeMap<String, BTreeMap<String, String>>,
    /// Logical storage buckets: name -> access (`private` | `public`).
    pub storage: BTreeMap<String, String>,
    /// Non-fatal notes about constructs this generator reads but does not realize.
    pub notes: Vec<String>,
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
    pub fn event(&self, name: &str) -> Option<&Event> {
        self.events.iter().find(|e| e.name == name)
    }
    pub fn policy(&self, name: &str) -> Option<&Policy> {
        self.policies.iter().find(|p| p.name == name)
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
    /// Other realization blocks (`storage`, `media`, …) kept for the emitters
    /// that understand them; never meaning-bearing.
    pub extra: BTreeMap<String, LayerCfg>,
    /// The environment layer applied (`--env`), if any.
    pub env: Option<String>,
    /// The `spec:` path the profile names, resolved against the profile file.
    pub spec: Option<PathBuf>,
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
            extra: BTreeMap::new(),
            env: None,
            spec: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Files produced by one generation, in memory.
pub(crate) struct Generated {
    /// Generator-owned files (tracked; regenerated; hand-edits detected).
    pub files: BTreeMap<String, String>,
    /// Scaffolds written once and then owned by the developer (handlers, auth).
    pub scaffold: BTreeMap<String, String>,
    pub lang: Lang,
    pub client_pkg: String,
}

/// Load + validate the spec and profile. Prints validation failures and
/// realization notes; returns an error when either document is invalid.
pub(crate) fn prepare(
    spec: &PathBuf,
    profile: Option<&PathBuf>,
    env: Option<&str>,
) -> Result<(AppModel, Profile), Box<dyn Error>> {
    let model = load_model(spec)?;

    let issues = validate_spec(&model);
    if !issues.is_empty() {
        eprintln!(
            "✗ {} is not a valid SpaceKit app spec ({} issue(s)):",
            spec.display(),
            issues.len()
        );
        for i in &issues {
            eprintln!("    - {i}");
        }
        return Err("invalid app spec".into());
    }

    let profile = match profile {
        Some(p) => load_profile(p, &model, env)?,
        None => {
            if env.is_some() {
                return Err("--env needs a --profile that defines `environments`".into());
            }
            Profile::default_for(&model)
        }
    };

    // Profile must realize, never re-mean (rule §1).
    let pissues = validate_profile(&model, &profile);
    if !pissues.is_empty() {
        eprintln!("✗ profile `{}` rejected ({} issue(s)):", profile.name, pissues.len());
        for i in &pissues {
            eprintln!("    - {i}");
        }
        return Err("invalid profile".into());
    }
    Ok((model, profile))
}

fn print_notes(model: &AppModel, profile: &Profile) {
    let mut notes = realization_notes(model);
    notes.extend(profile_notes(profile));
    if !notes.is_empty() {
        println!("  notes:");
        for n in notes {
            println!("    · {n}");
        }
    }
}

/// Realization choices the profile makes that this generator only partly honors.
fn profile_notes(p: &Profile) -> Vec<String> {
    let mut notes = Vec::new();
    let engine = workflow_engine(p);
    if engine != "in-process" {
        notes.push(format!(
            "business.workflows.engine `{engine}` is not generated yet; workflows use the \
             in-process saga runner (same steps and compensation)"
        ));
    }
    for (block, cfg) in &p.extra {
        if !cfg.values.is_empty() {
            notes.push(format!("profile block `{block}` is recorded but not realized yet"));
        }
    }
    let tokens = p.view.get("tokens", "css-variables");
    if tokens != "css-variables" {
        notes.push(format!("view.tokens `{tokens}` is not generated yet; tokens are emitted as CSS variables"));
    }
    notes
}

pub(crate) fn workflow_engine(p: &Profile) -> String {
    p.business.get("workflows.engine", "in-process").to_string()
}

/// Run every emitter in memory.
pub(crate) fn generate(
    model: &AppModel,
    profile: &Profile,
    sdk_lang: Option<&str>,
    assist: Option<&assist::Assist>,
) -> Result<Generated, Box<dyn Error>> {
    let pkg = sanitize_pkg(&model.app.name);
    let sdk_lang = sdk_lang
        .map(|s| s.to_string())
        .unwrap_or_else(|| profile.business.get("language", "typescript").to_string());
    let lang = match sdk_lang.to_lowercase().as_str() {
        "python" | "py" => Lang::Python,
        "typescript" | "ts" | "js" | "javascript" => Lang::TypeScript,
        "rust" | "rs" => Lang::Rust,
        other => return Err(format!("unknown --sdk-lang `{other}` (python, typescript, rust)").into()),
    };

    let mut files: BTreeMap<String, String> = BTreeMap::new();
    let mut scaffold: BTreeMap<String, String> = BTreeMap::new();

    // Phase 1: capabilities -> OpenAPI -> existing SDK generator.
    let openapi = synthesize_openapi(model, profile);
    if profile.business.get("emit_openapi", "true") == "true" {
        files.insert("openapi.json".to_string(), serde_json::to_string_pretty(&openapi)? + "\n");
    }
    let spec_model = parse_openapi(&openapi)?;
    let client_pkg = format!("{pkg}_client");
    let client_files = match lang {
        Lang::Python => emit_python(&spec_model, &client_pkg),
        Lang::TypeScript => emit_typescript(&spec_model, &client_pkg),
        Lang::Rust => sdkgen_rust::emit_rust(&spec_model, &client_pkg),
    };
    for (rel, contents) in &client_files {
        files.insert(format!("client/{client_pkg}/{rel}"), contents.clone());
    }
    files.insert(
        format!("client/{client_pkg}/README.md"),
        render_readme(&spec_model, &client_pkg, lang),
    );

    // Phase 2: entities -> data layer.
    for (rel, contents) in data::emit_data(model, profile) {
        files.insert(rel, contents);
    }

    // Phase 3: capabilities/events/policies/workflows -> business layer.
    let business = business::emit_business(model, profile, assist);
    files.extend(business.files);
    scaffold.extend(business.scaffold);
    for (rel, contents) in workflow::emit_workflows(model, profile) {
        files.insert(rel, contents);
    }

    // Phase 4: widgets/views/flows/text/tokens -> view layer.
    for (rel, contents) in view::emit_view(model, profile, &client_pkg) {
        files.insert(rel, contents);
    }

    // Phase 5 artifact: the behavioral fingerprint (meaning, profile-independent).
    let fp = behavioral_fingerprint(model);
    let fp_hash = super::sha256_hex(canonical_json(&fp).as_bytes());
    files.insert(
        FINGERPRINT_FILE.to_string(),
        serde_json::to_string_pretty(&json!({
            "version": 2,
            "app": model.app.name,
            "appVersion": model.app.version,
            "hash": fp_hash,
            "behavior": fp,
        }))? + "\n",
    );

    files.insert(
        "README.md".to_string(),
        render_app_readme(model, profile, &pkg, lang),
    );
    files.insert(".gitignore".to_string(), "node_modules/\n.next/\ndist/\n.env\n*.db\n".to_string());

    Ok(Generated {
        files,
        scaffold,
        lang,
        client_pkg,
    })
}

/// Where the fingerprint of the last generation lives in the output tree.
pub(crate) const FINGERPRINT_FILE: &str = ".spacekit-app.fingerprint.json";

pub fn handle_app(args: &AppArgs) -> Result<(), Box<dyn Error>> {
    let (model, profile) = prepare(&args.spec, args.profile.as_ref(), args.env.as_deref())?;

    // Conformance (Profile §8): compare the realized behavior of two profiles.
    if let Some(other_path) = &args.conformance {
        let other = load_profile(other_path, &model, args.env.as_deref())?;
        let opissues = validate_profile(&model, &other);
        if !opissues.is_empty() {
            eprintln!("✗ comparison profile `{}` rejected:", other.name);
            for i in &opissues {
                eprintln!("    - {i}");
            }
            return Err("invalid comparison profile".into());
        }
        return run_conformance(&model, &profile, &other);
    }

    let pkg = sanitize_pkg(&model.app.name);
    let out_root = args
        .out
        .clone()
        .or_else(|| profile.output.values.get("dir").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from(format!("./{pkg}_app")));

    let assist = match &args.assist {
        Some(project) => {
            let mut a = assist::Assist::load(project)?;
            a.route_all(&model)?;
            Some(a)
        }
        None => None,
    };
    let generated = generate(&model, &profile, args.sdk_lang.as_deref(), assist.as_ref())?;

    // Spec §14: refuse to regenerate across an unflagged breaking change.
    if let Some(prev) = read_previous_fingerprint(&out_root) {
        let new_fp = behavioral_fingerprint(&model);
        let report = diff::compare(&prev.behavior, &new_fp);
        if !report.breaking.is_empty() {
            let bumped = diff::major(&model.app.version) > diff::major(&prev.app_version);
            if !bumped && !args.allow_breaking {
                eprintln!(
                    "✗ the spec changed incompatibly since the last generation ({} breaking change(s)),",
                    report.breaking.len()
                );
                eprintln!(
                    "  but app.version went {} -> {} without a major bump (Spec §14):",
                    prev.app_version, model.app.version
                );
                for b in report.breaking.iter().take(20) {
                    eprintln!("    - {b}");
                }
                eprintln!("  Bump the major version, or re-run with --allow-breaking.");
                return Err("breaking spec change without a major version bump".into());
            }
        }
    }

    // ----- Report + incremental write (reusing the SDK regen engine) -----
    let verb = if args.plan { "Planned" } else { "Generated" };
    println!("{verb} {} -> {}", model.app.name, out_root.display());
    println!(
        "  {} entities · {} capabilities · {} events · {} workflows · {} views · profile: {}{}",
        model.entities.len(),
        model.capabilities.len(),
        model.events.len(),
        model.workflows.len(),
        model.views.len(),
        profile.name,
        profile.env.as_deref().map(|e| format!(" (env: {e})")).unwrap_or_default(),
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
    print_notes(&model, &profile);

    let opts = RegenOpts {
        plan: args.plan,
        prune: args.prune,
    };
    let plan = plan_regen(&out_root, &generated.files, args.force)?;
    let outcome = apply_regen(&out_root, &generated.files, &plan, opts)?;

    // Scaffolds: written once, then the developer's.
    let mut scaffolded = 0;
    for (rel, contents) in &generated.scaffold {
        let abs = out_root.join(rel);
        if abs.exists() {
            continue;
        }
        println!("    + {rel}  (scaffold: yours to edit, never overwritten)");
        scaffolded += 1;
        if !args.plan {
            if let Some(parent) = abs.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&abs, contents)?;
        }
    }
    if scaffolded > 0 {
        println!("  {scaffolded} scaffold file(s) created");
    }
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
        run_checks(&out_root, &generated.client_pkg, generated.lang)?;
    }

    Ok(())
}

struct PreviousFingerprint {
    app_version: String,
    behavior: Value,
}

fn read_previous_fingerprint(out_root: &PathBuf) -> Option<PreviousFingerprint> {
    let raw = fs::read_to_string(out_root.join(FINGERPRINT_FILE)).ok()?;
    let doc: Value = serde_json::from_str(&raw).ok()?;
    Some(PreviousFingerprint {
        app_version: str_field(&doc, "appVersion").unwrap_or_else(|| "0.0.0".into()),
        behavior: doc.get("behavior")?.clone(),
    })
}

/// The realized-behavior manifests each emitter writes: what the generated code
/// actually enforces, as opposed to what the spec says.
const CONFORMANCE_FILES: &[&str] = &["server/contract.json", "web/routes.json"];

/// Generate both profiles in memory and compare what their code realizes.
pub(crate) fn conformance_report(
    model: &AppModel,
    a: &Profile,
    b: &Profile,
) -> Result<Vec<String>, Box<dyn Error>> {
    let ga = generate(model, a, None, None)?;
    let gb = generate(model, b, None, None)?;
    let mut diffs = Vec::new();
    for f in CONFORMANCE_FILES {
        let va: Value = ga
            .files
            .get(*f)
            .map(|s| serde_json::from_str(s))
            .transpose()?
            .unwrap_or(Value::Null);
        let vb: Value = gb
            .files
            .get(*f)
            .map(|s| serde_json::from_str(s))
            .transpose()?
            .unwrap_or(Value::Null);
        diff::json_differences(f, &va, &vb, &mut diffs);
    }
    Ok(diffs)
}

fn run_conformance(model: &AppModel, a: &Profile, b: &Profile) -> Result<(), Box<dyn Error>> {
    println!("Conformance: `{}` vs `{}` on {}", a.name, b.name, model.app.name);
    let diffs = conformance_report(model, a, b)?;
    let checked = format!(
        "{} capabilities, {} policies, {} workflows, {} views",
        model.capabilities.len(),
        model.policies.len(),
        model.workflows.len(),
        model.views.len()
    );
    if diffs.is_empty() {
        println!("  ✓ PASS — both realizations enforce the same behavior ({checked})");
        Ok(())
    } else {
        eprintln!("  ✗ FAIL — the generated code differs in behavior:");
        for d in &diffs {
            eprintln!("    - {d}");
        }
        Err("conformance failed".into())
    }
}

/// The `spec:` path a profile names (resolved against the profile's folder).
pub fn profile_spec_path(profile: &PathBuf) -> Result<Option<PathBuf>, Box<dyn Error>> {
    let doc = read_doc(profile)?;
    let base = profile.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    Ok(str_field(&doc, "spec").map(|s| base.join(s)))
}

/// `spacekit spec check`: validate only.
pub fn handle_check(
    spec: &PathBuf,
    profile: Option<&PathBuf>,
    env: Option<&str>,
) -> Result<(), Box<dyn Error>> {
    let (model, prof) = prepare(spec, profile, env)?;
    println!(
        "✓ {} is a valid SpaceKit app spec (v{}, app {} {})",
        spec.display(),
        model.app.spec_version,
        model.app.name,
        model.app.version
    );
    println!(
        "  {} entities · {} capabilities · {} events · {} workflows · {} policies · {} views",
        model.entities.len(),
        model.capabilities.len(),
        model.events.len(),
        model.workflows.len(),
        model.policies.len(),
        model.views.len()
    );
    if profile.is_some() {
        println!("✓ profile `{}` realizes it without changing meaning", prof.name);
    }
    print_notes(&model, &prof);
    Ok(())
}

/// `spacekit spec conform`: behavioral equivalence of two profiles.
pub fn handle_conform(
    spec: &PathBuf,
    a: &PathBuf,
    b: &PathBuf,
    env: Option<&str>,
) -> Result<(), Box<dyn Error>> {
    let (model, pa) = prepare(spec, Some(a), env)?;
    let pb = load_profile(b, &model, env)?;
    let issues = validate_profile(&model, &pb);
    if !issues.is_empty() {
        eprintln!("✗ profile `{}` rejected:", pb.name);
        for i in &issues {
            eprintln!("    - {i}");
        }
        return Err("invalid profile".into());
    }
    run_conformance(&model, &pa, &pb)
}

/// `spacekit spec diff`: compatibility of two spec versions (Spec §14).
pub fn handle_diff(old: &PathBuf, new: &PathBuf) -> Result<(), Box<dyn Error>> {
    let a = load_model(old)?;
    let b = load_model(new)?;
    let report = diff::compare(&behavioral_fingerprint(&a), &behavioral_fingerprint(&b));
    println!(
        "{} {} -> {} {}",
        a.app.name, a.app.version, b.app.name, b.app.version
    );
    if report.breaking.is_empty() && report.compatible.is_empty() {
        println!("  no behavioral changes");
        return Ok(());
    }
    for c in &report.compatible {
        println!("  + compatible: {c}");
    }
    for c in &report.breaking {
        println!("  ! breaking:   {c}");
    }
    let (ma, mb) = (diff::major(&a.app.version), diff::major(&b.app.version));
    let (na, nb) = (diff::minor(&a.app.version), diff::minor(&b.app.version));
    if !report.breaking.is_empty() {
        if mb > ma {
            println!("  ✓ major version bumped ({} -> {})", a.app.version, b.app.version);
        } else {
            eprintln!(
                "  ✗ {} breaking change(s) need a major version bump (app.version {} -> {})",
                report.breaking.len(),
                a.app.version,
                b.app.version
            );
            return Err("breaking changes without a major version bump".into());
        }
    } else if mb == ma && nb <= na {
        println!(
            "  · compatible changes usually take a minor bump (app.version {} -> {})",
            a.app.version, b.app.version
        );
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
        serde_json::from_str(&raw)
            .map_err(|e| format!("{}: invalid JSON: {e}", path.display()))?
    } else {
        serde_yaml::from_str(&raw).map_err(|e| {
            let msg = e.to_string();
            let hint = if msg.contains("cannot start any token") {
                "\n  hint: YAML reserves `@` at the start of a value. Quote references and \
                 navigation, e.g. `policy: \"@OwnsOrder\"`, `tap: \"-> @Catalog\"` (Spec §3)."
            } else if msg.contains("mapping values are not allowed") {
                "\n  hint: put a space after every `key:` (e.g. `uploadedAt: { … }`), and quote \
                 values that contain `: `."
            } else {
                ""
            };
            format!("{}: invalid YAML: {msg}{hint}", path.display())
        })?
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
    let root = as_map(doc).ok_or("a SpaceKit app document must be a YAML/JSON map")?;

    let spec_version = match (root.get("spacekit"), root.get("openapp")) {
        (Some(v), _) | (None, Some(v)) => scalar_to_string(v),
        (None, None) => {
            return Err(
                "missing the `spacekit: \"0.2\"` version key (is this a SpaceKit app spec?)".into(),
            )
        }
    };
    if !matches!(spec_version.as_str(), "0.1" | "0.2") {
        return Err(format!(
            "unsupported spec version `{spec_version}` (this generator reads 0.1 and 0.2)"
        )
        .into());
    }
    if root.contains_key("dapp") {
        return Err(
            "this is a dApp document (`dapp:` key). dApp generation (contracts, indexer, wallet, \
             on-chain transactions) is not in this release; see SPACEKIT-DAPP-SPEC.md for the plan"
                .into(),
        );
    }

    let app_v = root.get("app").ok_or("missing required `app` block")?;
    let app = AppMeta {
        name: str_field(app_v, "name").unwrap_or_else(|| "App".to_string()),
        version: app_v
            .get("version")
            .map(scalar_to_string)
            .unwrap_or_else(|| "0.1.0".to_string()),
        spec_version,
        description: str_field(app_v, "description"),
    };
    let mut notes: Vec<String> = Vec::new();

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

    let mut workflows = Vec::new();
    if let Some(map) = root.get("workflows").and_then(as_map) {
        for (name, body) in map {
            workflows.push(parse_workflow(name, body, &mut notes));
        }
    }

    let mut policies = Vec::new();
    if let Some(map) = root.get("policies").and_then(as_map) {
        for (name, body) in map {
            let source = body.as_str().unwrap_or("").to_string();
            let expr = policy::parse(&source);
            policies.push(Policy {
                name: name.clone(),
                source,
                expr,
            });
        }
    }

    let mut text = BTreeMap::new();
    if let Some(map) = root.get("text").and_then(as_map) {
        for (key, body) in map {
            let mut locales = BTreeMap::new();
            match body {
                Value::Object(m) => {
                    for (loc, v) in m {
                        locales.insert(loc.clone(), scalar_to_string(v));
                    }
                }
                other => {
                    locales.insert("en".to_string(), scalar_to_string(other));
                }
            }
            text.insert(key.clone(), locales);
        }
    }

    let mut storage = BTreeMap::new();
    if let Some(map) = root.get("storage").and_then(as_map) {
        for (bucket, body) in map {
            let access = str_field(body, "access").unwrap_or_else(|| "private".to_string());
            storage.insert(bucket.clone(), access);
        }
    }

    for key in root.keys() {
        if !matches!(
            key.as_str(),
            "spacekit"
                | "openapp"
                | "app"
                | "entities"
                | "capabilities"
                | "events"
                | "workflows"
                | "policies"
                | "widgets"
                | "views"
                | "flows"
                | "text"
                | "tokens"
                | "storage"
        ) {
            notes.push(format!("top-level `{key}` is not part of the spec and was ignored"));
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
    for w in &workflows {
        registry.insert(w.name.clone(), Kind::Workflow);
    }
    for p in &policies {
        registry.insert(p.name.clone(), Kind::Policy);
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
    workflows.sort_by(|a, b| a.name.cmp(&b.name));
    policies.sort_by(|a, b| a.name.cmp(&b.name));
    widgets.sort_by(|a, b| a.name.cmp(&b.name));
    views.sort_by(|a, b| a.name.cmp(&b.name));
    flows.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(AppModel {
        app,
        entities,
        capabilities,
        events,
        workflows,
        policies,
        widgets,
        views,
        flows,
        tokens,
        text,
        storage,
        notes,
        registry,
    })
}

fn parse_workflow(name: &str, body: &Value, notes: &mut Vec<String>) -> Workflow {
    let on = match body.get("on") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Object(m)) => m
            .get("event")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
        _ => String::new(),
    };
    let mut steps = Vec::new();
    if let Some(arr) = body.get("steps").and_then(|x| x.as_array()) {
        for st in arr {
            if st.get("bundle").is_some() {
                notes.push(format!(
                    "workflow `{name}` uses an on-chain `bundle` step (dApp spec); it is not generated"
                ));
                continue;
            }
            let mut with = BTreeMap::new();
            if let Some(wm) = st.get("with").and_then(as_map) {
                for (k, v) in wm {
                    with.insert(k.clone(), value_to_expr(v));
                }
            }
            steps.push(WorkflowStep {
                capability: str_field(st, "do").unwrap_or_default(),
                with,
                compensate: str_field(st, "compensate"),
            });
        }
    }
    let outcome = |key: &str| -> Option<String> {
        body.get(key).and_then(|o| match o {
            Value::String(s) => Some(s.clone()),
            other => str_field(other, "emit"),
        })
    };
    Workflow {
        name: name.to_string(),
        summary: str_field(body, "summary"),
        on,
        steps,
        on_complete: outcome("onComplete"),
        on_failure: outcome("onFailure"),
    }
}

fn parse_rules(v: Option<&Value>) -> Rules {
    let Some(v) = v else {
        return Rules::default();
    };
    let num = |k: &str| v.get(k).and_then(|x| x.as_f64());
    let int = |k: &str| v.get(k).and_then(|x| x.as_u64());
    Rules {
        min: num("min"),
        max: num("max"),
        min_length: int("minLength"),
        max_length: int("maxLength"),
        pattern: str_field(v, "pattern"),
        one_of: v
            .get("oneOf")
            .and_then(|x| x.as_array())
            .cloned()
            .unwrap_or_default(),
    }
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
                rules: parse_rules(fv.get("rules")),
                bucket: str_field(fv, "bucket"),
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
        kind: str_field(body, "kind"),
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
            let other_outcomes = av
                .as_object()
                .map(|m| {
                    m.keys()
                        .filter(|k| k.starts_with("on") && *k != "onSuccess" && *k != "onError")
                        .cloned()
                        .collect()
                })
                .unwrap_or_default();
            actions.push(ViewAction {
                name: aname.clone(),
                invokes: str_field(av, "invokes").unwrap_or_default(),
                with,
                on_success: str_field(av, "onSuccess"),
                on_error: str_field(av, "onError"),
                other_outcomes,
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

    let mut states = Vec::new();
    if let Some(map) = body.get("states").and_then(as_map) {
        for (sname, sv) in map {
            let (show, when, props) = match sv {
                Value::String(s) => (Some(s.clone()), None, BTreeMap::new()),
                other => (
                    str_field(other, "show"),
                    str_field(other, "when"),
                    other
                        .get("props")
                        .and_then(as_map)
                        .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                        .unwrap_or_default(),
                ),
            };
            states.push(ViewState {
                name: sname.clone(),
                when,
                show,
                props,
            });
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
        // `requires:` is accepted as a synonym for a view `policy:`.
        policy: str_field(body, "policy").or_else(|| str_field(body, "requires")),
        states,
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

fn load_profile(
    path: &PathBuf,
    model: &AppModel,
    env: Option<&str>,
) -> Result<Profile, Box<dyn Error>> {
    let mut prof = Profile::default_for(model);
    let mut chain = Vec::new();
    apply_profile_file(path, &mut prof, env, &mut chain)?;
    if let Some(e) = env {
        prof.env = Some(e.to_string());
    }
    Ok(prof)
}

/// Load a profile document onto `prof`: its `extends` base first, then its own
/// blocks, then the requested environment layer (Profile §2, §7).
fn apply_profile_file(
    path: &PathBuf,
    prof: &mut Profile,
    env: Option<&str>,
    chain: &mut Vec<PathBuf>,
) -> Result<(), Box<dyn Error>> {
    let canon = fs::canonicalize(path).unwrap_or_else(|_| path.clone());
    if chain.contains(&canon) {
        return Err(format!("profile `extends` cycle through {}", path.display()).into());
    }
    chain.push(canon);
    let doc = read_doc(path)?;
    let root = as_map(&doc).ok_or("profile must be a map")?;
    let base_dir = path.parent().map(|p| p.to_path_buf()).unwrap_or_default();

    if let Some(ext) = str_field(&doc, "extends") {
        apply_profile_file(&base_dir.join(ext), prof, env, chain)?;
    }
    if let Some(name) = str_field(&doc, "name") {
        prof.name = name;
    } else if prof.name == "default" {
        prof.name = "custom".to_string();
    }
    if let Some(spec) = str_field(&doc, "spec") {
        prof.spec = Some(base_dir.join(spec));
    }
    apply_layers(prof, root)?;

    if let Some(e) = env {
        let envs = root.get("environments").and_then(as_map);
        match envs.and_then(|m| m.get(e)).and_then(as_map) {
            Some(layer) => apply_layers(prof, layer)?,
            None => {
                // Only an error when this profile defines environments at all and
                // no ancestor supplied the requested one either.
                if envs.is_some() && prof.env.as_deref() != Some(e) {
                    let names: Vec<&String> = envs.map(|m| m.keys().collect()).unwrap_or_default();
                    return Err(format!(
                        "profile {} has no environment `{e}` (defined: {})",
                        path.display(),
                        names.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
                    )
                    .into());
                }
            }
        }
        if envs.map(|m| m.contains_key(e)).unwrap_or(false) {
            prof.env = Some(e.to_string());
        }
    }
    Ok(())
}

/// Profile keys that are document metadata rather than realization layers.
const PROFILE_META_KEYS: &[&str] = &[
    "spacekit-profile",
    "openapp-profile",
    "name",
    "spec",
    "extends",
    "environments",
];

fn apply_layers(prof: &mut Profile, root: &Map<String, Value>) -> Result<(), Box<dyn Error>> {
    for (key, block) in root {
        if PROFILE_META_KEYS.contains(&key.as_str()) {
            continue;
        }
        let Some(block) = block.as_object() else {
            return Err(format!("profile `{key}` must be a map of realization choices").into());
        };
        let cfg = parse_layer(block);
        match key.as_str() {
            "data" => merge_layer(&mut prof.data, cfg),
            "business" => merge_layer(&mut prof.business, cfg),
            "view" => merge_layer(&mut prof.view, cfg),
            "output" => merge_layer(&mut prof.output, cfg),
            other => merge_layer(prof.extra.entry(other.to_string()).or_default(), cfg),
        }
    }
    Ok(())
}

/// Flatten a layer block: nested maps become dotted keys
/// (`workflows: { engine: inngest }` -> `workflows.engine = inngest`).
fn parse_layer(block: &Map<String, Value>) -> LayerCfg {
    let mut cfg = LayerCfg::default();
    for (k, v) in block {
        if k == "overrides" {
            if let Some(om) = v.as_object() {
                for (name, ov) in om {
                    if let Some(ovm) = ov.as_object() {
                        let mut entry = BTreeMap::new();
                        flatten_into(&mut entry, "", ovm);
                        cfg.overrides.insert(name.clone(), entry);
                    }
                }
            }
            continue;
        }
        match v.as_object() {
            Some(inner) => flatten_into(&mut cfg.values, k, inner),
            None => {
                cfg.values.insert(k.clone(), scalar_to_string(v));
            }
        }
    }
    cfg
}

fn flatten_into(out: &mut BTreeMap<String, String>, prefix: &str, map: &Map<String, Value>) {
    for (k, v) in map {
        let key = if prefix.is_empty() {
            k.clone()
        } else {
            format!("{prefix}.{k}")
        };
        match v.as_object() {
            Some(inner) => flatten_into(out, &key, inner),
            None => {
                out.insert(key, scalar_to_string(v));
            }
        }
    }
}

fn merge_layer(into: &mut LayerCfg, from: LayerCfg) {
    for (k, v) in from.values {
        into.values.insert(k, v);
    }
    for (k, v) in from.overrides {
        into.overrides.entry(k).or_default().extend(v);
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
    m.workflows.iter().for_each(|w| bump(&w.name));
    m.policies.iter().for_each(|p| bump(&p.name));
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
            check_rules(e, f, &mut issues);
            if let Some(b) = &f.bucket {
                if !m.storage.contains_key(b) {
                    issues.push(format!(
                        "entity `{}` field `{}` uses bucket `{b}`, which `storage` does not declare",
                        e.name, f.name
                    ));
                }
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
            if let (Some(pol), Some(ent)) = (named_policy(m, p), c.primary_entity()) {
                check_policy_paths(m, pol, &ent, &format!("capability `{}`", c.name), &mut issues);
            }
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

    // Policies must parse (Spec §9 grammar).
    for p in &m.policies {
        if let Err(e) = &p.expr {
            issues.push(format!("policy `{}` is not a valid expression: {e}", p.name));
        }
    }

    // Workflows (Spec §7).
    for w in &m.workflows {
        let trigger = m.event(strip_ref(&w.on));
        if w.on.is_empty() {
            issues.push(format!("workflow `{}` has no `on` event", w.name));
        } else if trigger.is_none() {
            issues.push(format!("workflow `{}` is triggered by unknown event `{}`", w.name, w.on));
        }
        if w.steps.is_empty() {
            issues.push(format!("workflow `{}` has no steps", w.name));
        }
        for (i, st) in w.steps.iter().enumerate() {
            let ctx = format!("workflow `{}` step {}", w.name, i + 1);
            match m.capability(strip_ref(&st.capability)) {
                None => issues.push(format!("{ctx} does unknown capability `{}`", st.capability)),
                Some(cap) => {
                    for k in st.with.keys() {
                        if !cap.input.iter().any(|inp| &inp.name == k) {
                            issues.push(format!("{ctx} passes `{k}` not in capability `{}` input", cap.name));
                        }
                    }
                    for inp in &cap.input {
                        if inp.required && !st.with.contains_key(&inp.name) {
                            issues.push(format!("{ctx} omits required input `{}` of `{}`", inp.name, cap.name));
                        }
                    }
                }
            }
            for expr in st.with.values() {
                if let Some(rest) = expr.strip_prefix("event.") {
                    let head = rest.split('.').next().unwrap_or("");
                    if let Some(ev) = trigger {
                        if !ev.payload.iter().any(|p| p.name == head) {
                            issues.push(format!(
                                "{ctx} reads `event.{head}`, which event `{}` does not carry",
                                ev.name
                            ));
                        }
                    }
                } else if !is_literal_expr(expr) {
                    issues.push(format!(
                        "{ctx} maps `{expr}`; workflow inputs may only read `event.…` or literals"
                    ));
                }
            }
            if let Some(comp) = &st.compensate {
                match m.capability(strip_ref(comp)) {
                    None => issues.push(format!("{ctx} compensates with unknown capability `{comp}`")),
                    Some(cc) => {
                        // Compensations receive the step's own inputs.
                        for inp in &cc.input {
                            if inp.required && !st.with.contains_key(&inp.name) {
                                issues.push(format!(
                                    "{ctx}: compensation `{}` needs `{}`, which the step does not pass",
                                    cc.name, inp.name
                                ));
                            }
                        }
                    }
                }
            }
        }
        for (key, ev) in [("onComplete", &w.on_complete), ("onFailure", &w.on_failure)] {
            if let Some(ev) = ev {
                match m.event(strip_ref(ev)) {
                    None => issues.push(format!("workflow `{}` {key} emits unknown event `{ev}`", w.name)),
                    Some(out_ev) => {
                        for p in &out_ev.payload {
                            let from_trigger = trigger
                                .map(|t| t.payload.iter().any(|tp| tp.name == p.name))
                                .unwrap_or(false);
                            let is_reason = p.name == "reason" && key == "onFailure";
                            if p.required && !from_trigger && !is_reason {
                                issues.push(format!(
                                    "workflow `{}` {key} emits `{}`, but its required `{}` is not in the \
                                     triggering event's payload",
                                    w.name, out_ev.name, p.name
                                ));
                            }
                        }
                    }
                }
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
            if let (Some(pol), Some(ent)) = (named_policy(m, p), view_resource_entity(m, v)) {
                check_policy_paths(m, pol, &ent.name, &format!("view `{}`", v.name), &mut issues);
            }
        }
        // States (Spec §10).
        for st in &v.states {
            if let Some(w) = &st.show {
                let name = strip_ref(w);
                if !m.widgets.iter().any(|x| x.name == name) {
                    issues.push(format!(
                        "view `{}` state `{}` shows unknown widget `{w}`",
                        v.name, st.name
                    ));
                }
            }
            if st.name == "empty" && st.when.is_none() {
                issues.push(format!("view `{}` state `empty` needs a `when` expression", v.name));
            }
        }
        // Layout widget/view refs.
        check_layout_refs(m, &v.layout, &v.name, &mut issues);
        // Text references (Spec §11) anywhere in the view.
        let mut texts = Vec::new();
        collect_text_refs(&v.layout, &mut texts);
        for st in &v.states {
            for val in st.props.values() {
                collect_text_refs(val, &mut texts);
            }
        }
        for key in texts {
            if !m.text.contains_key(&key) {
                issues.push(format!("view `{}` uses `@text.{key}`, which `text` does not define", v.name));
            }
        }
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
    if is_builtin_policy(p) {
        return;
    }
    let name = strip_ref(p);
    if m.policy(name).is_none() {
        issues.push(format!("{ctx} references unknown policy `{p}`"));
    }
}

/// `public` and `authenticated` are keywords, not named policies (Spec §9).
pub(crate) fn is_builtin_policy(p: &str) -> bool {
    matches!(strip_ref(p), "public" | "authenticated")
}

/// The named policy a `policy:` reference points at (None for keywords/unknown).
pub(crate) fn named_policy<'a>(m: &'a AppModel, p: &str) -> Option<&'a Policy> {
    if is_builtin_policy(p) {
        return None;
    }
    m.policy(strip_ref(p))
}

/// The entity a view's policy is evaluated against: its first entity-bound `data`.
pub(crate) fn view_resource_entity<'a>(m: &'a AppModel, v: &View) -> Option<&'a Entity> {
    v.data.iter().find_map(|d| m.entity(strip_ref(&d.from)))
}

/// Every `resource.a.b` path must walk declared relations and end on a field.
fn check_policy_paths(m: &AppModel, pol: &Policy, entity: &str, ctx: &str, issues: &mut Vec<String>) {
    let Ok(expr) = &pol.expr else { return };
    let mut paths = Vec::new();
    policy::resource_paths(expr, &mut paths);
    for path in paths {
        if let Err(e) = walk_resource_path(m, entity, &path) {
            issues.push(format!(
                "{ctx} uses policy `{}` on `{entity}`, but `resource.{}` {e}",
                pol.name,
                path.join(".")
            ));
        }
    }
}

/// Resolve `resource.<path>` from `entity`; returns the relation names walked.
pub(crate) fn walk_resource_path(m: &AppModel, entity: &str, path: &[String]) -> Result<Vec<String>, String> {
    let mut cur = m.entity(entity).ok_or_else(|| format!("(unknown entity `{entity}`)"))?;
    let mut rels = Vec::new();
    for (i, seg) in path.iter().enumerate() {
        if let Some(r) = cur.relations.iter().find(|r| &r.name == seg) {
            rels.push(seg.clone());
            match m.entity(&r.target) {
                Some(t) => cur = t,
                None => return Err(format!("walks to unknown entity `{}`", r.target)),
            }
            if i == path.len() - 1 {
                return Err(format!("ends on relation `{seg}`; compare a field such as `{seg}.id`"));
            }
            continue;
        }
        if cur.field(seg).is_some() {
            if i != path.len() - 1 {
                return Err(format!("reads into `{seg}`, which is a field of `{}`, not a relation", cur.name));
            }
            return Ok(rels);
        }
        return Err(format!("is not a field or relation of `{}` (at `{seg}`)", cur.name));
    }
    Err("must name a field".into())
}

/// Prisma `include` tree for the relations a policy walks on `entity`.
pub(crate) fn policy_include(m: &AppModel, pol: &Policy, entity: &str) -> policy::IncludeTree {
    let mut tree = policy::IncludeTree::default();
    if let Ok(expr) = &pol.expr {
        let mut paths = Vec::new();
        policy::resource_paths(expr, &mut paths);
        for path in paths {
            if let Ok(rels) = walk_resource_path(m, entity, &path) {
                let mut node = &mut tree;
                for r in rels {
                    node = node.0.entry(r).or_default();
                }
            }
        }
    }
    tree
}

fn check_rules(e: &Entity, f: &FieldDef, issues: &mut Vec<String>) {
    let r = &f.rules;
    if r.is_empty() {
        return;
    }
    let numeric = matches!(f.ty.as_str(), "integer" | "decimal" | "money");
    let textual = matches!(f.ty.as_str(), "text" | "longtext" | "email" | "url" | "phone");
    let ctx = format!("entity `{}` field `{}`", e.name, f.name);
    if (r.min.is_some() || r.max.is_some()) && !numeric {
        issues.push(format!("{ctx}: `min`/`max` apply to number fields, not `{}`", f.ty));
    }
    if (r.min_length.is_some() || r.max_length.is_some() || r.pattern.is_some()) && !textual {
        issues.push(format!("{ctx}: `minLength`/`maxLength`/`pattern` apply to text fields, not `{}`", f.ty));
    }
    if let (Some(a), Some(b)) = (r.min, r.max) {
        if a > b {
            issues.push(format!("{ctx}: `min` ({a}) is greater than `max` ({b})"));
        }
    }
    if let (Some(a), Some(b)) = (r.min_length, r.max_length) {
        if a > b {
            issues.push(format!("{ctx}: `minLength` ({a}) is greater than `maxLength` ({b})"));
        }
    }
    if let Some(p) = &r.pattern {
        if regex::Regex::new(p).is_err() {
            issues.push(format!("{ctx}: `pattern` is not a valid regular expression"));
        }
    }
}

/// A workflow `with` value that is a literal (number, bool, quoted string).
fn is_literal_expr(s: &str) -> bool {
    let t = s.trim();
    t.parse::<f64>().is_ok()
        || t == "true"
        || t == "false"
        || (t.starts_with('\'') && t.ends_with('\''))
        || (t.starts_with('"') && t.ends_with('"'))
}

fn collect_text_refs(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::String(s) => {
            if let Some(key) = s.trim().strip_prefix("@text.") {
                out.push(key.to_string());
            }
        }
        Value::Array(a) => a.iter().for_each(|x| collect_text_refs(x, out)),
        Value::Object(m) => m.values().for_each(|x| collect_text_refs(x, out)),
        _ => {}
    }
}

/// Constructs the generator reads but does not (yet) realize. Printed as notes,
/// never silently dropped.
pub(crate) fn realization_notes(m: &AppModel) -> Vec<String> {
    let mut notes = m.notes.clone();
    for v in &m.views {
        for st in &v.states {
            if !matches!(st.name.as_str(), "loading" | "empty" | "error") {
                notes.push(format!(
                    "view `{}` state `{}` is not realized (supported states: loading, empty, error)",
                    v.name, st.name
                ));
            }
        }
        for a in &v.actions {
            for o in &a.other_outcomes {
                notes.push(format!(
                    "view `{}` action `{}` outcome `{o}` is not realized (supported: onSuccess, onError)",
                    v.name, a.name
                ));
            }
        }
    }
    if !m.storage.is_empty() {
        notes.push(
            "`storage` buckets are validated and recorded; file fields are stored as object keys. \
             Upload/presign adapters are not generated yet"
                .to_string(),
        );
    }
    notes
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
    let Some((name, args)) = parse_navigation(nav) else {
        issues.push(format!("{ctx}: `{nav}` is not `stay` or `-> @View(arg: value)`"));
        return;
    };
    let Some(target) = m.views.iter().find(|v| v.name == name) else {
        issues.push(format!("{ctx} navigates to unknown view `{name}`"));
        return;
    };
    for (k, _) in &args {
        if !target.params.iter().any(|p| &p.name == k) {
            issues.push(format!("{ctx} passes `{k}`, which view `{name}` has no param for"));
        }
    }
    for p in &target.params {
        let in_route = target
            .route
            .as_deref()
            .map(|r| r.split('/').any(|seg| seg == format!(":{}", p.name)))
            .unwrap_or(false);
        if (p.required || in_route) && !args.iter().any(|(k, _)| k == &p.name) {
            issues.push(format!("{ctx} navigates to `{name}` without its `{}` param", p.name));
        }
    }
}

/// `-> @View(a: x, b: y)` -> ("View", [("a","x"), ("b","y")]).
pub(crate) fn parse_navigation(nav: &str) -> Option<(String, Vec<(String, String)>)> {
    let t = nav.trim().strip_prefix("->")?.trim();
    let (head, rest) = match t.find('(') {
        Some(i) => (&t[..i], Some(&t[i + 1..])),
        None => (t, None),
    };
    let name = strip_ref(head.trim()).to_string();
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return None;
    }
    let mut args = Vec::new();
    if let Some(rest) = rest {
        let inner = rest.strip_suffix(')')?;
        for part in inner.split(',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            let (k, v) = part.split_once(':')?;
            args.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    Some((name, args))
}

fn check_layout_refs(m: &AppModel, node: &Value, view: &str, issues: &mut Vec<String>) {
    match node {
        Value::Array(items) => {
            for it in items {
                check_layout_refs(m, it, view, issues);
            }
        }
        Value::Object(map) => {
            if let Some(on) = map.get("on").and_then(|x| x.as_object()) {
                for (evt, nav) in on {
                    if let Some(nav) = nav.as_str() {
                        check_navigation(
                            m,
                            Some(nav),
                            &format!("view `{view}` layout `on.{evt}`"),
                            issues,
                        );
                    }
                }
            }
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
        "steps",
        "compensate",
        "onComplete",
        "onFailure",
    ];
    let mut layers: Vec<(&str, &LayerCfg)> = vec![
        ("data", &p.data),
        ("business", &p.business),
        ("view", &p.view),
        ("output", &p.output),
    ];
    for (k, v) in &p.extra {
        layers.push((k.as_str(), v));
    }
    for (layer_name, layer) in layers {
        for k in layer.values.keys() {
            if k.split('.').any(|seg| FORBIDDEN.contains(&seg)) {
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
        if orm == "prisma" && !matches!(store, "postgres" | "postgresql" | "mysql" | "sqlite") {
            issues.push(format!(
                "data.store `{store}` not yet implemented (supported: postgres, mysql, sqlite with prisma, \
                 or spacekit-storage-node)"
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
                            "rules": rules_fp(&f.rules),
                            "bucket": f.bucket,
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
            let mut writes: Vec<Value> = c
                .writes
                .iter()
                .map(|w| json!({ "effect": w.effect, "entity": w.entity }))
                .collect();
            writes.sort_by_key(|v| v.to_string());
            let mut errors = c.errors.clone();
            errors.sort();
            let mut emits = c.emits.clone();
            emits.sort();
            let mut reads = c.reads.clone();
            reads.sort();
            (
                c.name.clone(),
                json!({
                    "input": io_fp(&c.input),
                    "output": io_fp(&c.output),
                    "reads": reads,
                    "writes": writes,
                    "emits": emits,
                    "policy": c.policy.as_deref().map(strip_ref).unwrap_or("public"),
                    "idempotent": c.idempotent,
                    "errors": errors,
                }),
            )
        })
        .collect();

    let events: Map<String, Value> = m
        .events
        .iter()
        .map(|e| (e.name.clone(), json!({ "payload": io_fp(&e.payload) })))
        .collect();

    let workflows: Map<String, Value> = m
        .workflows
        .iter()
        .map(|w| {
            (
                w.name.clone(),
                json!({
                    "on": strip_ref(&w.on),
                    "steps": w.steps.iter().map(|s| json!({
                        "do": strip_ref(&s.capability),
                        "with": s.with,
                        "compensate": s.compensate.as_deref().map(strip_ref),
                    })).collect::<Vec<_>>(),
                    "onComplete": w.on_complete.as_deref().map(strip_ref),
                    "onFailure": w.on_failure.as_deref().map(strip_ref),
                }),
            )
        })
        .collect();

    let policies: Map<String, Value> = m
        .policies
        .iter()
        .map(|p| {
            let canon = match &p.expr {
                Ok(e) => policy::canonical(e),
                Err(_) => p.source.trim().to_string(),
            };
            (p.name.clone(), json!(canon))
        })
        .collect();

    let widgets: Map<String, Value> = m
        .widgets
        .iter()
        .map(|w| (w.name.clone(), json!({ "props": io_fp(&w.props), "slots": w.slots })))
        .collect();

    let views: Map<String, Value> = m
        .views
        .iter()
        .map(|v| {
            (
                v.name.clone(),
                json!({
                    "route": v.route,
                    "params": io_fp(&v.params),
                    "policy": v.policy.as_deref().map(strip_ref),
                    "data": v.data.iter().map(|d| json!({ "name": d.name, "from": strip_ref(&d.from), "where": d.where_expr, "with": d.with })).collect::<Vec<_>>(),
                    "actions": v.actions.iter().map(|a| json!({ "name": a.name, "invokes": strip_ref(&a.invokes), "with": a.with, "onSuccess": a.on_success, "onError": a.on_error })).collect::<Vec<_>>(),
                    "transitions": v.transitions.iter().map(|t| json!({ "to": strip_ref(&t.to), "when": t.when })).collect::<Vec<_>>(),
                    "states": v.states.iter().map(|s| json!({ "name": s.name, "when": s.when, "show": s.show.as_deref().map(strip_ref) })).collect::<Vec<_>>(),
                }),
            )
        })
        .collect();

    json!({
        "app": { "name": m.app.name },
        "entities": entities,
        "capabilities": capabilities,
        "events": events,
        "workflows": workflows,
        "policies": policies,
        "widgets": widgets,
        "views": views,
        "text": m.text.keys().collect::<Vec<_>>(),
        "storage": m.storage,
    })
}

fn rules_fp(r: &Rules) -> Value {
    let mut o = Map::new();
    if let Some(v) = r.min {
        o.insert("min".into(), json!(v));
    }
    if let Some(v) = r.max {
        o.insert("max".into(), json!(v));
    }
    if let Some(v) = r.min_length {
        o.insert("minLength".into(), json!(v));
    }
    if let Some(v) = r.max_length {
        o.insert("maxLength".into(), json!(v));
    }
    if let Some(v) = &r.pattern {
        o.insert("pattern".into(), json!(v));
    }
    if !r.one_of.is_empty() {
        o.insert("oneOf".into(), json!(r.one_of));
    }
    Value::Object(o)
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
