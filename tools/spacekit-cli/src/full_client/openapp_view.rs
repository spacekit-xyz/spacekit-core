//! View-layer emitter — `widgets`/`views`/`flows`/`text`/`tokens` -> React.
//!
//! Two framework realizations share the same widget/token/text output:
//!   * `framework: next` (default) — an app-router tree of async server
//!     components. `data` bindings resolve on the server (capabilities through
//!     their runners, entities through `db`), view `policy` is checked against
//!     the caller, `states` become `loading.tsx` / `error.tsx` / an empty branch,
//!     taps navigate with `next/link`, and view `actions` are server-action forms.
//!   * `framework: react` — a Vite + react-router SPA whose pages fetch
//!     capability data through the generated client SDK.
//!
//! Each realization writes `web/routes.json`, a manifest of the behavior it
//! actually realized (routes, guards, bindings, navigation, actions, states),
//! which conformance compares across profiles.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Map, Value};

use super::{
    camel, is_builtin_policy, is_next, kebab, named_policy, parse_navigation, pascal, pluralize,
    policy_include, snake, strip_ref, view_resource_entity, AppModel, IoParam, Profile, View,
    ViewData, Widget,
};

pub(super) fn emit_view(m: &AppModel, p: &Profile, client_pkg: &str) -> Vec<(String, String)> {
    let framework = p.view.get("framework", "next");
    let mut out = if is_next(framework) {
        emit_next(m, p)
    } else {
        emit_react_spa(m, p, client_pkg)
    };
    out.push(("web/lib/text.ts".to_string(), text_ts(m, p)));
    out.push((
        "web/routes.json".to_string(),
        serde_json::to_string_pretty(&routes_manifest(m, is_next(framework))).unwrap_or_default() + "\n",
    ));
    out
}

// ---------------------------------------------------------------------------
// Shared: text, tokens, widgets, expressions
// ---------------------------------------------------------------------------

fn text_ts(m: &AppModel, p: &Profile) -> String {
    let locale = p.view.get("locale", "en");
    let mut s = String::from(
        "// Generated i18n registry (Spec §11). The active locale is a profile/runtime choice.\n",
    );
    s.push_str(&format!(
        "export const DEFAULT_LOCALE = \"{locale}\";\n\nexport const TEXT: Record<string, Record<string, string>> = {{\n"
    ));
    for (key, locales) in &m.text {
        let parts: Vec<String> = locales
            .iter()
            .map(|(loc, v)| format!("{}: {}", js_key(loc), serde_json::to_string(v).unwrap_or_default()))
            .collect();
        s.push_str(&format!("  {}: {{ {} }},\n", js_key(key), parts.join(", ")));
    }
    s.push_str(
        "};\n\n\
         /** Look up a string; falls back to the default locale, then English, then the key. */\n\
         export function t(key: string, locale: string = DEFAULT_LOCALE): string {\n  \
         const entry = TEXT[key];\n  \
         if (!entry) return key;\n  \
         return entry[locale] ?? entry[DEFAULT_LOCALE] ?? entry.en ?? Object.values(entry)[0] ?? key;\n}\n",
    );
    s
}

fn js_key(k: &str) -> String {
    if k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && !k.starts_with(|c: char| c.is_ascii_digit()) {
        k.to_string()
    } else {
        serde_json::to_string(k).unwrap_or_default()
    }
}

fn tokens_css(tokens: &Value) -> String {
    let mut vars: Vec<String> = Vec::new();
    flatten_tokens("", tokens, &mut vars);
    let mut s = String::from("/* Design tokens (spec `tokens` -> CSS custom properties). */\n:root {\n");
    for v in vars {
        s.push_str(&format!("  {v}\n"));
    }
    s.push_str("}\n");
    s
}

fn flatten_tokens(prefix: &str, node: &Value, out: &mut Vec<String>) {
    match node {
        Value::Object(map) => {
            for (k, v) in map {
                let key = if prefix.is_empty() {
                    kebab(k)
                } else {
                    format!("{prefix}-{}", kebab(k))
                };
                flatten_tokens(&key, v, out);
            }
        }
        Value::Number(n) => {
            let unit = if prefix.starts_with("space") || prefix.starts_with("breakpoints") || prefix.ends_with("-size") {
                "px"
            } else {
                ""
            };
            out.push(format!("--{prefix}: {n}{unit};"));
        }
        Value::String(s) => out.push(format!("--{prefix}: {s};")),
        _ => {}
    }
}

fn widget_tsx(m: &AppModel, w: &Widget) -> String {
    let mut imports: BTreeSet<String> = BTreeSet::new();
    let mut props_lines = String::new();
    for p in &w.props {
        let ty = prop_ts(m, p, &mut imports);
        let opt = if p.required { "" } else { "?" };
        props_lines.push_str(&format!("  {}{}: {};\n", camel(&p.name), opt, ty));
    }
    for slot in &w.slots {
        props_lines.push_str(&format!("  {}?: React.ReactNode;\n", camel(slot)));
    }
    if props_lines.is_empty() {
        props_lines.push_str("  className?: string;\n");
    }

    let mut s = String::from(
        "// Generated widget: a pure, prop-driven component. Restyle freely; keep the props.\n\
         import React from \"react\";\n",
    );
    for imp in &imports {
        s.push_str(imp);
    }
    s.push('\n');
    s.push_str(&format!("export interface {}Props {{\n{}}}\n\n", w.name, props_lines));
    s.push_str(&format!(
        "export function {name}(props: {name}Props) {{\n  return (\n    <div className=\"widget-{cls}\">\n",
        name = w.name,
        cls = kebab(&w.name),
    ));
    for p in &w.props {
        let c = camel(&p.name);
        if p.ty.starts_with('@') {
            // Show the entity's first text-ish field so the widget renders something useful.
            let ent = m.entity(strip_ref(&p.ty));
            let label = ent.and_then(|e| {
                e.fields
                    .iter()
                    .find(|f| matches!(f.name.as_str(), "title" | "name" | "displayName" | "label"))
                    .or_else(|| e.fields.iter().find(|f| f.ty == "text" && f.name != e.identity))
            });
            if let Some(f) = label {
                s.push_str(&format!(
                    "      <span data-prop=\"{n}\">{{String(props.{c}?.{f} ?? \"\")}}</span>\n",
                    n = p.name,
                    f = camel(&f.name)
                ));
            }
        } else {
            s.push_str(&format!(
                "      <span data-prop=\"{n}\">{{String(props.{c} ?? \"\")}}</span>\n",
                n = p.name,
            ));
        }
    }
    for slot in &w.slots {
        s.push_str(&format!("      <div data-slot=\"{slot}\">{{props.{c}}}</div>\n", c = camel(slot)));
    }
    s.push_str("    </div>\n  );\n}\n");
    s
}

fn prop_ts(m: &AppModel, p: &IoParam, imports: &mut BTreeSet<String>) -> String {
    let base = if let Some(ent) = p.ty.strip_prefix('@') {
        if m.entity(ent).is_some() {
            imports.insert(format!("import type {{ {ent} }} from \"../../server/types\";\n"));
        }
        // Read shapes may arrive partially loaded or serialized.
        format!("Partial<{ent}>")
    } else if p.ty == "enum" && !p.values.is_empty() {
        p.values.iter().map(|v| format!("\"{v}\"")).collect::<Vec<_>>().join(" | ")
    } else {
        match p.ty.as_str() {
            "integer" | "decimal" | "money" => "number | string",
            "boolean" => "boolean",
            _ => "string",
        }
        .to_string()
    };
    if p.list {
        format!("Array<{base}>")
    } else {
        base
    }
}

/// Names in scope while rendering a view.
#[derive(Clone)]
struct Scope {
    locals: BTreeSet<String>,
    /// Declared view params.
    params: BTreeSet<String>,
}

impl Scope {
    fn for_view(v: &View) -> Scope {
        let mut locals = BTreeSet::new();
        for d in &v.data {
            locals.insert(camel(&d.name));
        }
        let params: BTreeSet<String> = v.params.iter().map(|p| p.name.clone()).collect();
        Scope { locals, params }
    }
    fn with_local(&self, name: &str) -> Scope {
        let mut s = self.clone();
        s.locals.insert(name.to_string());
        s
    }
}

/// Render a binding expression to TypeScript. `@text.k` -> `t("k")`,
/// `params.x` -> the param local (or the query string), known locals and dotted
/// paths stay expressions, anything else is a string literal.
fn render_expr(s: &str, scope: &Scope) -> String {
    let t = s.trim();
    if let Some(key) = t.strip_prefix("@text.") {
        return format!("t({})", serde_json::to_string(key).unwrap_or_default());
    }
    if let Some(rest) = t.strip_prefix("params.") {
        let name = rest.split('.').next().unwrap_or(rest);
        return if scope.params.contains(name) {
            camel(name)
        } else {
            format!("query.{}", camel(name))
        };
    }
    let looks_ident = !t.is_empty()
        && t.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
        && !t.starts_with(|c: char| c.is_ascii_digit());
    let head = t.split('.').next().unwrap_or(t);
    if looks_ident && (scope.locals.contains(head) || scope.params.contains(head) || head == "result") {
        if scope.params.contains(head) && !scope.locals.contains(head) {
            return camel(t);
        }
        return t.to_string();
    }
    if t.parse::<f64>().is_ok() || t == "true" || t == "false" {
        return t.to_string();
    }
    serde_json::to_string(t).unwrap_or_default()
}

fn render_value(v: &Value, scope: &Scope) -> String {
    match v {
        Value::String(s) => render_expr(s, scope),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }
}

/// A spec boolean expression (`books.length == 0`) as TypeScript.
fn render_condition(expr: &str) -> String {
    expr.replace("!=", "!==").replace("==", "===").replace("!===", "!==").replace("====", "===")
}

/// The URL for `-> @View(args)` as a template literal.
fn nav_href(m: &AppModel, nav: &str, scope: &Scope) -> Option<String> {
    let (name, args) = parse_navigation(nav)?;
    let target = m.views.iter().find(|v| v.name == name)?;
    let route = view_route(target);
    let mut used = BTreeSet::new();
    let mut path = String::new();
    for seg in route.split('/').filter(|s| !s.is_empty()) {
        path.push('/');
        if let Some(param) = seg.strip_prefix(':') {
            let val = args
                .iter()
                .find(|(k, _)| k == param)
                .map(|(_, v)| render_expr(v, scope))
                .unwrap_or_else(|| "\"\"".into());
            used.insert(param.to_string());
            path.push_str(&format!("${{encodeURIComponent(String({val}))}}"));
        } else {
            path.push_str(seg);
        }
    }
    if path.is_empty() {
        path.push('/');
    }
    let query: Vec<String> = args
        .iter()
        .filter(|(k, _)| !used.contains(k))
        .map(|(k, v)| format!("{k}=${{encodeURIComponent(String({}))}}", render_expr(v, scope)))
        .collect();
    if !query.is_empty() {
        path.push('?');
        path.push_str(&query.join("&"));
    }
    Some(format!("`{path}`"))
}

/// A view's route (`/books/:id`), defaulting to `/<kebab-name>`.
fn view_route(v: &View) -> String {
    match &v.route {
        Some(r) if !r.trim_matches('/').is_empty() => format!("/{}", r.trim_matches('/')),
        Some(_) => "/".to_string(),
        None => format!("/{}", kebab(&v.name)),
    }
}

fn collect_layout_widgets(node: &Value, out: &mut BTreeSet<String>) {
    match node {
        Value::Array(items) => items.iter().for_each(|i| collect_layout_widgets(i, out)),
        Value::Object(map) => {
            if let Some(w) = map.get("widget").and_then(|x| x.as_str()) {
                out.insert(strip_ref(w).to_string());
            }
            for (_, v) in map {
                if v.is_array() || v.is_object() {
                    collect_layout_widgets(v, out);
                }
            }
        }
        _ => {}
    }
}

fn layout_uses_text(node: &Value) -> bool {
    match node {
        Value::String(s) => s.trim().starts_with("@text."),
        Value::Array(a) => a.iter().any(layout_uses_text),
        Value::Object(m) => m.values().any(layout_uses_text),
        _ => false,
    }
}

fn layout_navigates(node: &Value) -> bool {
    match node {
        Value::Array(a) => a.iter().any(layout_navigates),
        Value::Object(m) => {
            m.get("on").and_then(|o| o.as_object()).map(|o| o.values().any(|v| v.as_str().map(|s| s.trim().starts_with("->")).unwrap_or(false))).unwrap_or(false)
                || m.values().any(|v| (v.is_array() || v.is_object()) && layout_navigates(v))
        }
        _ => false,
    }
}

/// Render a layout subtree to JSX. `link` names the navigation component
/// (`Link` with `href` for Next, `Link` with `to` for react-router).
fn render_layout(m: &AppModel, node: &Value, scope: &Scope, indent: usize, link_attr: &str) -> String {
    let pad = "  ".repeat(indent);
    match node {
        Value::Array(items) => items
            .iter()
            .map(|i| render_layout(m, i, scope, indent, link_attr))
            .collect::<String>(),
        Value::Object(map) => {
            if let Some(rep) = map.get("repeat").and_then(|x| x.as_str()) {
                // "book in books"
                let mut parts = rep.split_whitespace();
                let var = parts.next().unwrap_or("item").to_string();
                let _in = parts.next();
                let coll = parts.next().unwrap_or("items").to_string();
                let inner = scope.with_local(&var);
                let mut child = map.clone();
                child.remove("repeat");
                let child_jsx = render_layout(m, &Value::Object(child), &inner, indent + 2, link_attr);
                let coll_expr = render_expr(&coll, scope);
                let coll_expr = if coll_expr.starts_with('"') {
                    // Not a binding: nothing to repeat over (e.g. a client-side queue).
                    "([] as any[])".to_string()
                } else {
                    format!("({coll_expr} ?? [])")
                };
                return format!(
                    "{pad}{{{coll_expr}.map(({var}: any, i: number) => (\n{pad}  <React.Fragment key={{String({var}?.id ?? i)}}>\n{child_jsx}{pad}  </React.Fragment>\n{pad}))}}\n",
                );
            }
            let mut element = String::new();
            if let Some(w) = map.get("widget").and_then(|x| x.as_str()) {
                let name = strip_ref(w);
                let mut attrs = String::new();
                if let Some(props) = map.get("props").and_then(|x| x.as_object()) {
                    for (k, val) in props {
                        attrs.push_str(&format!(" {}={{{}}}", camel(k), render_value(val, scope)));
                    }
                }
                let mut slot_attrs = String::new();
                if let Some(slots) = map.get("slots").and_then(|x| x.as_object()) {
                    for (slot, content) in slots {
                        let inner = render_layout(m, content, scope, indent + 2, link_attr);
                        slot_attrs.push_str(&format!(
                            "{pad}  {slot}={{\n{pad}    <>\n{inner}{pad}    </>\n{pad}  }}\n",
                            slot = camel(slot)
                        ));
                    }
                }
                element = if slot_attrs.is_empty() {
                    format!("{pad}<{name}{attrs} />\n")
                } else {
                    format!("{pad}<{name}{attrs}\n{slot_attrs}{pad}/>\n")
                };
            } else if let Some(vn) = map.get("view").and_then(|x| x.as_str()) {
                element = format!("{pad}{{/* embedded view {} */}}\n", strip_ref(vn));
            }
            // Taps navigate.
            if let Some(nav) = map
                .get("on")
                .and_then(|o| o.get("tap"))
                .and_then(|x| x.as_str())
            {
                if let Some(href) = nav_href(m, nav, scope) {
                    return format!("{pad}<Link {link_attr}={{{href}}}>\n{}{pad}</Link>\n", indent_by(&element, 1));
                }
            }
            element
        }
        _ => String::new(),
    }
}

fn indent_by(s: &str, levels: usize) -> String {
    let pad = "  ".repeat(levels);
    s.lines().map(|l| format!("{pad}{l}\n")).collect()
}

/// How a data binding resolves: the output field it reads from a capability
/// (bindings named like an output field, or the only output, are unwrapped).
fn capability_binding_field(m: &AppModel, d: &ViewData) -> Option<String> {
    let cap = m.capability(strip_ref(&d.from))?;
    if let Some(o) = cap.output.iter().find(|o| o.name == d.name) {
        return Some(camel(&o.name));
    }
    if cap.output.len() == 1 {
        return Some(camel(&cap.output[0].name));
    }
    None
}

fn humanize(name: &str) -> String {
    let words = snake(name).replace('_', " ");
    let mut c = words.chars();
    match c.next() {
        Some(f) => f.to_ascii_uppercase().to_string() + c.as_str(),
        None => String::new(),
    }
}

fn state<'a>(v: &'a View, name: &str) -> Option<&'a super::ViewState> {
    v.states.iter().find(|s| s.name == name)
}

/// `<Widget prop={…} />` for a state, filling a `message` prop from `fallback`.
fn state_element(st: &super::ViewState, scope: &Scope, message_fallback: Option<&str>, m: &AppModel) -> Option<String> {
    let name = strip_ref(st.show.as_deref()?).to_string();
    let widget = m.widgets.iter().find(|w| w.name == name)?;
    let mut attrs = String::new();
    for (k, v) in &st.props {
        attrs.push_str(&format!(" {}={{{}}}", camel(k), render_value(v, scope)));
    }
    if let Some(fb) = message_fallback {
        if widget.props.iter().any(|p| p.name == "message") && !st.props.contains_key("message") {
            attrs.push_str(&format!(" message={{{fb}}}"));
        }
    }
    Some(format!("<{name}{attrs} />"))
}

fn state_props_use_text(st: &super::ViewState) -> bool {
    st.props.values().any(layout_uses_text)
}

// ---------------------------------------------------------------------------
// Next.js (app-router / RSC / server actions)
// ---------------------------------------------------------------------------

fn emit_next(m: &AppModel, p: &Profile) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if let Some(tokens) = &m.tokens {
        out.push(("web/app/tokens.css".to_string(), tokens_css(tokens)));
    }
    for w in &m.widgets {
        out.push((format!("web/components/{}.tsx", w.name), widget_tsx(m, w)));
    }
    out.push(("web/app/layout.tsx".to_string(), root_layout(m, p)));

    for v in &m.views {
        let dir = route_to_dir(v);
        let prefix = if dir.is_empty() {
            "web/app".to_string()
        } else {
            format!("web/app/{dir}")
        };
        let depth = if dir.is_empty() { 0 } else { dir.split('/').count() };
        let up = "../".repeat(depth + 2); // <dir>/page.tsx -> app -> web -> root
        out.push((format!("{prefix}/page.tsx"), view_page(m, p, v, &up)));
        if let Some(st) = state(v, "loading") {
            if let Some(file) = state_file(m, st, &up, false) {
                out.push((format!("{prefix}/loading.tsx"), file));
            }
        }
        if let Some(st) = state(v, "error") {
            if let Some(file) = state_file(m, st, &up, true) {
                out.push((format!("{prefix}/error.tsx"), file));
            }
        }
    }
    if !m.flows.is_empty() {
        out.push(("web/flows.md".to_string(), flows_md(m)));
    }
    out.push(("web/package.json".to_string(), package_json(m, p)));
    out.push(("web/tsconfig.json".to_string(), NEXT_TSCONFIG.to_string()));
    out.push(("web/next-env.d.ts".to_string(), "/// <reference types=\"next\" />\n/// <reference types=\"next/image-types/global\" />\n".to_string()));
    out.push(("web/next.config.mjs".to_string(), NEXT_CONFIG.to_string()));
    out
}

const NEXT_TSCONFIG: &str = r#"{
  "compilerOptions": {
    "target": "ES2022",
    "lib": ["dom", "dom.iterable", "es2022"],
    "allowJs": false,
    "skipLibCheck": true,
    "strict": true,
    "noEmit": true,
    "esModuleInterop": true,
    "module": "esnext",
    "moduleResolution": "bundler",
    "resolveJsonModule": true,
    "isolatedModules": true,
    "jsx": "preserve",
    "incremental": true,
    "plugins": [{ "name": "next" }]
  },
  "include": ["next-env.d.ts", "**/*.ts", "**/*.tsx", "../server/**/*.ts"],
  "exclude": ["node_modules"]
}
"#;

const NEXT_CONFIG: &str = r#"// The server layer lives beside the web app (../server); let Next compile it.
/** @type {import('next').NextConfig} */
const nextConfig = {
  experimental: { externalDir: true },
};
export default nextConfig;
"#;

/// `loading.tsx` / `error.tsx` for a declared state.
fn state_file(m: &AppModel, st: &super::ViewState, up: &str, is_error: bool) -> Option<String> {
    let name = strip_ref(st.show.as_deref()?).to_string();
    m.widgets.iter().find(|w| w.name == name)?;
    let scope = Scope { locals: BTreeSet::new(), params: BTreeSet::new() };
    let el = state_element(st, &scope, is_error.then_some("error.message"), m)?;
    let text_import = if state_props_use_text(st) {
        format!("import {{ t }} from \"{up}web/lib/text\";\n")
    } else {
        String::new()
    };
    Some(if is_error {
        format!(
            "\"use client\";\n// Generated error state (Spec §10).\n\
             import React from \"react\";\n\
             import {{ {name} }} from \"{up}web/components/{name}\";\n{text_import}\n\
             export default function ErrorState({{ error }}: {{ error: Error & {{ digest?: string }} }}) {{\n  \
             return {el};\n}}\n"
        )
    } else {
        format!(
            "// Generated loading state (Spec §10).\n\
             import React from \"react\";\n\
             import {{ {name} }} from \"{up}web/components/{name}\";\n{text_import}\n\
             export default function Loading() {{\n  return {el};\n}}\n"
        )
    })
}

/// `/books/:id` -> `books/[id]`, `/` -> ``.
fn route_to_dir(v: &View) -> String {
    let route = view_route(v);
    let trimmed = route.trim_matches('/');
    if trimmed.is_empty() {
        return String::new();
    }
    trimmed
        .split('/')
        .map(|seg| match seg.strip_prefix(':') {
            Some(param) => format!("[{param}]"),
            None => seg.to_string(),
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn route_params(v: &View) -> Vec<String> {
    view_route(v)
        .split('/')
        .filter_map(|s| s.strip_prefix(':').map(|x| x.to_string()))
        .collect()
}

fn view_page(m: &AppModel, p: &Profile, v: &View, up: &str) -> String {
    let scope = Scope::for_view(v);
    let result_type = p.business.get("errors", "problem-json") == "result-type";
    let route_ps = route_params(v);

    let mut widget_imports: BTreeSet<String> = BTreeSet::new();
    collect_layout_widgets(&v.layout, &mut widget_imports);
    for st in &v.states {
        if st.name == "empty" {
            if let Some(w) = &st.show {
                widget_imports.insert(strip_ref(w).to_string());
            }
        }
    }
    widget_imports.retain(|w| m.widgets.iter().any(|x| &x.name == w));

    let mut runner_imports: BTreeSet<String> = BTreeSet::new();
    let mut action_imports: BTreeSet<String> = BTreeSet::new();
    let mut type_imports: BTreeSet<String> = BTreeSet::new();
    for d in &v.data {
        if m.capability(strip_ref(&d.from)).is_some() {
            runner_imports.insert(format!("run{}", strip_ref(&d.from)));
        }
    }
    for a in &v.actions {
        let cap = strip_ref(&a.invokes);
        if m.capability(cap).is_some() {
            action_imports.insert(camel(cap));
            type_imports.insert(format!("{cap}Input"));
        }
    }
    let needs_db = v.data.iter().any(|d| m.entity(strip_ref(&d.from)).is_some());
    let view_policy = v.policy.as_deref().map(strip_ref).filter(|p| *p != "public");
    let needs_actor = view_policy.is_some() || !runner_imports.is_empty();
    let uses_text = layout_uses_text(&v.layout) || v.states.iter().any(state_props_use_text);
    let navigates = layout_navigates(&v.layout);
    let has_actions = v.actions.iter().any(|a| m.capability(strip_ref(&a.invokes)).is_some());
    let uses_not_found = view_policy.is_some() || v.data.iter().any(|d| m.entity(strip_ref(&d.from)).is_some() && d.where_expr.is_some());

    let mut s = String::from(
        "// Generated view (React server component). Do not edit; edit the spec and regenerate.\n\
         import React from \"react\";\n",
    );
    if navigates {
        s.push_str("import Link from \"next/link\";\n");
    }
    let mut nav_fns = Vec::new();
    if uses_not_found {
        nav_fns.push("notFound");
    }
    if has_actions {
        nav_fns.push("redirect");
    }
    if !nav_fns.is_empty() {
        s.push_str(&format!("import {{ {} }} from \"next/navigation\";\n", nav_fns.join(", ")));
    }
    for w in &widget_imports {
        s.push_str(&format!("import {{ {w} }} from \"{up}web/components/{w}\";\n"));
    }
    if uses_text {
        s.push_str(&format!("import {{ t }} from \"{up}web/lib/text\";\n"));
    }
    if !runner_imports.is_empty() {
        s.push_str(&format!(
            "import {{ {} }} from \"{up}server/capabilities/index\";\n",
            runner_imports.iter().cloned().collect::<Vec<_>>().join(", ")
        ));
    }
    if !action_imports.is_empty() {
        s.push_str(&format!(
            "import {{ {} }} from \"{up}server/actions/index\";\n",
            action_imports.iter().cloned().collect::<Vec<_>>().join(", ")
        ));
        if !result_type {
            s.push_str(&format!("import {{ isApiError }} from \"{up}server/errors\";\n"));
        }
    }
    if needs_actor {
        s.push_str(&format!("import {{ currentActor }} from \"{up}server/auth\";\n"));
    }
    if view_policy.is_some() {
        s.push_str(&format!("import {{ evaluate }} from \"{up}server/policies\";\n"));
    }
    let resource = view_resource_entity(m, v);
    let include = view_policy
        .and_then(|vp| named_policy(m, vp))
        .zip(resource)
        .and_then(|(pol, ent)| policy_include(m, pol, &ent.name).to_prisma());
    if include.is_some() {
        s.push_str(&format!("import {{ POLICY_INCLUDE }} from \"{up}server/policies\";\n"));
    }
    if needs_db {
        s.push_str(&format!("import {{ db }} from \"{up}server/db\";\n"));
    }
    if !type_imports.is_empty() {
        s.push_str(&format!(
            "import type {{ {} }} from \"{up}server/types\";\n",
            type_imports.iter().cloned().collect::<Vec<_>>().join(", ")
        ));
    }
    s.push('\n');
    if let Some(sum) = &v.summary {
        s.push_str(&format!("/** {sum} */\n"));
    }

    // Props: route params and the query string.
    let route_fields: Vec<String> = route_ps.iter().map(|p| format!("{p}: string")).collect();
    s.push_str(&format!(
        "export default async function {}Page({{ params, searchParams }}: {{\n  params: {{ {} }};\n  searchParams: Record<string, string | string[] | undefined>;\n}}) {{\n",
        pascal(&v.name),
        route_fields.join("; ")
    ));
    s.push_str("  void params;\n");
    s.push_str(
        "  const query = Object.fromEntries(\n    Object.entries(searchParams).map(([k, v]) => [k, Array.isArray(v) ? v[0] : v]),\n  ) as Record<string, string | undefined>;\n",
    );
    for prm in &v.params {
        if route_ps.contains(&prm.name) {
            s.push_str(&format!("  const {} = decodeURIComponent(params.{});\n", camel(&prm.name), prm.name));
        } else {
            s.push_str(&format!("  const {} = query.{};\n", camel(&prm.name), prm.name));
        }
    }
    if needs_actor {
        s.push_str("  const actor = await currentActor();\n");
    }
    if let Some(vp) = view_policy {
        if is_builtin_policy(vp) {
            s.push_str(&format!("  if (!evaluate(\"{vp}\", actor, undefined)) notFound();\n"));
        } else if resource.is_none() {
            s.push_str(&format!(
                "  // policy {vp}: no entity is bound here, so it is enforced by the capability that loads the data.\n"
            ));
        }
    }

    // Data bindings.
    let mut resource_checked = false;
    for d in &v.data {
        let local = camel(&d.name);
        let from = strip_ref(&d.from);
        if let Some(cap) = m.capability(from) {
            let args: Vec<String> = d
                .with
                .iter()
                .map(|(k, val)| format!("{}: {}", camel(k), render_expr(val, &scope)))
                .collect();
            let call = format!("run{}({{ {} }}, actor)", cap.name, args.join(", "));
            match capability_binding_field(m, d) {
                Some(field) => s.push_str(&format!("  const {local} = (await {call}).{field};\n")),
                None => s.push_str(&format!("  const {local} = await {call};\n")),
            }
        } else if let Some(ent) = m.entity(from) {
            let inc = if resource.map(|r| r.name == ent.name).unwrap_or(false) && !resource_checked {
                include
                    .as_ref()
                    .and_then(|_| view_policy.map(|vp| format!(", include: POLICY_INCLUDE.{vp}.{}", ent.name)))
                    .unwrap_or_default()
            } else {
                String::new()
            };
            match &d.where_expr {
                Some(w) => {
                    s.push_str(&format!(
                        "  const {local} = await db.{}.findFirst({{ where: {}{inc} }});\n",
                        camel(&ent.name),
                        where_to_prisma(w, &scope)
                    ));
                    s.push_str(&format!("  if (!{local}) notFound();\n"));
                }
                None => s.push_str(&format!(
                    "  const {local} = await db.{}.findMany({{{}}});\n",
                    camel(&ent.name),
                    inc.trim_start_matches(',')
                )),
            }
            if let (Some(vp), Some(r)) = (view_policy, resource) {
                if !is_builtin_policy(vp) && r.name == ent.name && !resource_checked && d.where_expr.is_some() {
                    s.push_str(&format!("  if (!evaluate(\"{vp}\", actor, {local})) notFound();\n"));
                    resource_checked = true;
                }
            }
        }
    }

    // Empty state.
    if let Some(st) = state(v, "empty") {
        if let (Some(when), Some(el)) = (&st.when, state_element(st, &scope, None, m)) {
            s.push_str(&format!(
                "  if ({}) {{\n    return (\n      <main className=\"view-{} view-empty\">\n        {el}\n      </main>\n    );\n  }}\n",
                render_condition(when),
                kebab(&v.name)
            ));
        }
    }

    // View actions as server-action forms.
    let mut forms = String::new();
    for a in &v.actions {
        let cap = strip_ref(&a.invokes);
        if m.capability(cap).is_none() {
            continue;
        }
        let input_local = format!("{}Input", camel(&a.name));
        let args: Vec<String> = a
            .with
            .iter()
            .map(|(k, val)| format!("{}: {}", camel(k), render_expr(val, &scope)))
            .collect();
        s.push_str(&format!("  const {input_local}: {cap}Input = {{ {} }};\n", args.join(", ")));
        let here = nav_self(v, &scope);
        let success = a
            .on_success
            .as_deref()
            .and_then(|nav| nav_href(m, nav, &scope.with_local("result")))
            .unwrap_or_else(|| here.clone());
        let error_nav = match a.on_error.as_deref().map(str::trim) {
            Some(nav) if nav.starts_with("->") => nav_href(m, nav, &scope).unwrap_or_else(|| here.clone()),
            _ => here.clone(),
        };
        let body = if result_type {
            format!(
                "        \"use server\";\n        \
                 const r = await {fname}({input_local});\n        \
                 if (!r.ok) {{\n          const back = {err};\n          redirect(`${{back}}${{back.includes(\"?\") ? \"&\" : \"?\"}}error=${{r.error}}`);\n        }}\n        \
                 const result = r.value;\n        void result;\n        \
                 redirect({success});\n",
                fname = camel(cap),
                err = error_nav,
            )
        } else {
            format!(
                "        \"use server\";\n        \
                 let result;\n        \
                 try {{\n          result = await {fname}({input_local});\n        }} catch (err) {{\n          \
                 if (!isApiError(err)) throw err;\n          \
                 const back = {err};\n          \
                 redirect(`${{back}}${{back.includes(\"?\") ? \"&\" : \"?\"}}error=${{err.code}}`);\n        }}\n        \
                 void result;\n        \
                 redirect({success});\n",
                fname = camel(cap),
                err = error_nav,
            )
        };
        forms.push_str(&format!(
            "      <form\n        action={{async () => {{\n{body}        }}}}\n      >\n        \
             <button type=\"submit\">{label}</button>\n      </form>\n",
            label = humanize(&a.name)
        ));
    }

    let layout_jsx = render_layout(m, &v.layout, &scope, 3, "href");
    s.push_str("  return (\n");
    s.push_str(&format!("    <main className=\"view-{}\">\n", kebab(&v.name)));
    if has_actions {
        s.push_str("      {query.error ? <p role=\"alert\">{query.error}</p> : null}\n");
    }
    s.push_str(&layout_jsx);
    s.push_str(&forms);
    s.push_str("    </main>\n  );\n}\n");
    s
}

/// The current view's own URL, for `onError: stay`.
fn nav_self(v: &View, scope: &Scope) -> String {
    let mut path = String::new();
    for seg in view_route(v).split('/').filter(|s| !s.is_empty()) {
        path.push('/');
        match seg.strip_prefix(':') {
            Some(p) => path.push_str(&format!("${{encodeURIComponent({})}}", camel(p))),
            None => path.push_str(seg),
        }
    }
    let _ = scope;
    if path.is_empty() {
        path.push('/');
    }
    format!("`{path}`")
}

fn where_to_prisma(expr: &str, scope: &Scope) -> String {
    // `field == value [&& field == value]`; anything else is passed through as a comment.
    let mut parts = Vec::new();
    for clause in expr.split("&&") {
        match clause.split_once("==") {
            Some((lhs, rhs)) => {
                parts.push(format!("{}: {}", camel(lhs.trim()), render_expr(rhs.trim(), scope)));
            }
            None => return format!("{{ /* unsupported filter: {expr} */ }}"),
        }
    }
    format!("{{ {} }}", parts.join(", "))
}

fn root_layout(m: &AppModel, p: &Profile) -> String {
    let tokens_import = if m.tokens.is_some() { "import \"./tokens.css\";\n" } else { "" };
    let tailwind = p.view.get("styling", "tailwind") == "tailwind";
    let globals = if tailwind {
        "// Tailwind: add app/globals.css with the @tailwind directives and import it here.\n"
    } else {
        ""
    };
    format!(
        "import React from \"react\";\n{tokens_import}{globals}\n\
         export const metadata = {{ title: {title} }};\n\n\
         export default function RootLayout({{ children }}: {{ children: React.ReactNode }}) {{\n  \
         return (\n    <html lang=\"{lang}\">\n      <body>{{children}}</body>\n    </html>\n  );\n}}\n",
        title = serde_json::to_string(&m.app.name).unwrap_or_default(),
        lang = p.view.get("locale", "en"),
    )
}

fn flows_md(m: &AppModel) -> String {
    let mut s = String::from("# Flows\n\nHigher-level journeys across views.\n\n");
    for f in &m.flows {
        s.push_str(&format!("## {}\n\n", f.name));
        if let Some(sum) = &f.summary {
            s.push_str(&format!("{sum}\n\n"));
        }
        for step in &f.steps {
            if let Some(at) = step.get("at").and_then(|x| x.as_str()) {
                s.push_str(&format!("1. `{}`", strip_ref(at)));
                if let Some(on) = step.get("on") {
                    s.push_str(&format!(" — on {}", on));
                }
                s.push('\n');
            }
        }
        s.push('\n');
    }
    s
}

fn package_json(m: &AppModel, p: &Profile) -> String {
    let tailwind_dev = if p.view.get("styling", "tailwind") == "tailwind" {
        ",\n    \"tailwindcss\": \"^3.4.0\",\n    \"postcss\": \"^8.4.0\",\n    \"autoprefixer\": \"^10.4.0\""
    } else {
        ""
    };
    let prisma = if super::is_storage_node(p.data.get("store", "postgres")) {
        ""
    } else {
        ",\n    \"@prisma/client\": \"^5.22.0\""
    };
    format!(
        "{{\n  \"name\": \"{name}-web\",\n  \"private\": true,\n  \
         \"scripts\": {{\n    \"dev\": \"next dev\",\n    \"build\": \"next build\",\n    \"start\": \"next start\",\n    \"typecheck\": \"tsc --noEmit\"\n  }},\n  \
         \"dependencies\": {{\n    \"next\": \"^14.2.0\",\n    \"react\": \"^18.3.0\",\n    \"react-dom\": \"^18.3.0\"{prisma}\n  }},\n  \
         \"devDependencies\": {{\n    \"typescript\": \"^5.6.0\",\n    \"@types/node\": \"^20.0.0\",\n    \"@types/react\": \"^18.3.0\",\n    \"@types/react-dom\": \"^18.3.0\"{tailwind_dev}\n  }}\n}}\n",
        name = kebab(&m.app.name),
    )
}

// ---------------------------------------------------------------------------
// React SPA (Vite + react-router + generated client SDK)
// ---------------------------------------------------------------------------

fn emit_react_spa(m: &AppModel, p: &Profile, client_pkg: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if let Some(tokens) = &m.tokens {
        out.push(("web/src/tokens.css".to_string(), tokens_css(tokens)));
    }
    for w in &m.widgets {
        out.push((format!("web/components/{}.tsx", w.name), widget_tsx(m, w)));
    }
    out.push(("web/src/api.ts".to_string(), spa_api_ts(client_pkg)));
    for v in &m.views {
        out.push((format!("web/src/pages/{}.tsx", pascal(&v.name)), spa_page(m, v)));
    }
    out.push(("web/src/App.tsx".to_string(), spa_app(m)));
    out.push(("web/src/main.tsx".to_string(), spa_main(m)));
    out.push(("web/index.html".to_string(), spa_index_html(m)));
    out.push(("web/vite.config.ts".to_string(), spa_vite_config()));
    if !m.flows.is_empty() {
        out.push(("web/flows.md".to_string(), flows_md(m)));
    }
    out.push(("web/package.json".to_string(), spa_package_json(m, p)));
    out
}

fn spa_api_ts(client_pkg: &str) -> String {
    format!(
        "// Generated client-SDK wiring for the SPA.\n\
         import {{ Client }} from \"../../client/{pkg}/src/index\";\n\n\
         export const api = new Client({{\n  \
         baseURL: import.meta.env.VITE_API_BASE_URL ?? \"http://localhost:8080/api\",\n  \
         apiKey: import.meta.env.VITE_API_KEY,\n}});\n\n\
         /** Collect a client-SDK result (array, value, or paginated async iterator) into an array. */\n\
         export async function collect<T>(result: any): Promise<T[]> {{\n  \
         if (result && typeof result[Symbol.asyncIterator] === \"function\") {{\n    \
         const acc: T[] = [];\n    \
         for await (const item of result) acc.push(item as T);\n    \
         return acc;\n  }}\n  \
         if (Array.isArray(result)) return result as T[];\n  \
         return result == null ? [] : [result as T];\n}}\n",
        pkg = client_pkg,
    )
}

fn spa_resource(m: &AppModel, cap: &str) -> (String, String) {
    let c = m.capability(cap);
    let res = snake(
        &c.and_then(|c| c.primary_entity())
            .map(|e| pluralize(&e))
            .unwrap_or_else(|| "Resources".to_string()),
    );
    (res, camel(cap))
}

fn spa_page(m: &AppModel, v: &View) -> String {
    let scope = Scope::for_view(v);
    let route_ps = route_params(v);
    let mut widget_imports: BTreeSet<String> = BTreeSet::new();
    collect_layout_widgets(&v.layout, &mut widget_imports);
    for st in &v.states {
        if let Some(w) = &st.show {
            widget_imports.insert(strip_ref(w).to_string());
        }
    }
    widget_imports.retain(|w| m.widgets.iter().any(|x| &x.name == w));
    let has_cap_data = v.data.iter().any(|d| m.capability(strip_ref(&d.from)).is_some());
    let has_actions = v.actions.iter().any(|a| m.capability(strip_ref(&a.invokes)).is_some());
    let navigates = layout_navigates(&v.layout);
    let uses_text = layout_uses_text(&v.layout) || v.states.iter().any(state_props_use_text);

    let mut router = vec!["useSearchParams"];
    if !route_ps.is_empty() {
        router.push("useParams");
    }
    if navigates {
        router.push("Link");
    }
    if has_actions {
        router.push("useNavigate");
    }
    let mut s = String::from("// Generated view (React SPA client component). Do not edit; edit the spec and regenerate.\n");
    s.push_str("import React, { useEffect, useState } from \"react\";\n");
    s.push_str(&format!("import {{ {} }} from \"react-router-dom\";\n", router.join(", ")));
    if has_cap_data || has_actions {
        s.push_str("import { api, collect } from \"../api\";\n");
    }
    for w in &widget_imports {
        s.push_str(&format!("import {{ {w} }} from \"../../components/{w}\";\n"));
    }
    if uses_text {
        s.push_str("import { t } from \"../../lib/text\";\n");
    }
    s.push('\n');
    if let Some(sum) = &v.summary {
        s.push_str(&format!("/** {sum} */\n"));
    }
    s.push_str(&format!("export default function {}Page() {{\n", pascal(&v.name)));
    if !route_ps.is_empty() {
        s.push_str("  const params = useParams();\n");
    }
    s.push_str("  const [search] = useSearchParams();\n  const query = Object.fromEntries(search.entries()) as Record<string, string | undefined>;\n");
    for prm in &v.params {
        if route_ps.contains(&prm.name) {
            s.push_str(&format!("  const {} = params.{} ?? \"\";\n", camel(&prm.name), prm.name));
        } else {
            s.push_str(&format!("  const {} = query.{};\n", camel(&prm.name), prm.name));
        }
    }
    if has_actions {
        s.push_str("  const navigate = useNavigate();\n");
    }
    s.push_str("  const [loading, setLoading] = useState(true);\n  const [error, setError] = useState<string | null>(null);\n");
    let mut loads = String::new();
    for d in &v.data {
        let local = camel(&d.name);
        let from = strip_ref(&d.from);
        s.push_str(&format!("  const [{local}, set{}] = useState<any>(null);\n", pascal(&d.name)));
        if let Some(cap) = m.capability(from) {
            let (res, method) = spa_resource(m, from);
            let args: Vec<String> = d
                .with
                .iter()
                .map(|(k, val)| format!("{}: {}", camel(k), render_expr(val, &scope)))
                .collect();
            let call = format!("api.{res}.{method}({})", if args.is_empty() { String::new() } else { format!("{{ {} }}", args.join(", ")) });
            let is_list = cap.output.len() == 1 && cap.output[0].list;
            if is_list {
                loads.push_str(&format!("      set{}(await collect({call}));\n", pascal(&d.name)));
            } else {
                match capability_binding_field(m, d) {
                    Some(f) => loads.push_str(&format!("      set{}(((await {call}) as any).{f});\n", pascal(&d.name))),
                    None => loads.push_str(&format!("      set{}(await {call});\n", pascal(&d.name))),
                }
            }
        } else {
            loads.push_str(&format!(
                "      // `{}` reads @{from} directly, which the SPA cannot do: expose a read capability for it.\n",
                d.name
            ));
        }
    }
    s.push_str("  useEffect(() => {\n    let live = true;\n    (async () => {\n      try {\n");
    s.push_str(&loads.replace("      set", "        if (live) set").replace("      //", "        //"));
    s.push_str("      } catch (err) {\n        if (live) setError(err instanceof Error ? err.message : String(err));\n      } finally {\n        if (live) setLoading(false);\n      }\n    })();\n    return () => {\n      live = false;\n    };\n    // eslint-disable-next-line react-hooks/exhaustive-deps\n  }, [search]);\n");

    // States.
    let st_loading = state(v, "loading").and_then(|st| state_element(st, &scope, None, m)).unwrap_or_else(|| "<p>Loading…</p>".into());
    s.push_str(&format!("  if (loading) return {st_loading};\n"));
    let st_error = state(v, "error").and_then(|st| state_element(st, &scope, Some("error"), m)).unwrap_or_else(|| "<p role=\"alert\">{error}</p>".into());
    s.push_str(&format!("  if (error) return {st_error};\n"));
    if let Some(st) = state(v, "empty") {
        if let (Some(when), Some(el)) = (&st.when, state_element(st, &scope, None, m)) {
            s.push_str(&format!("  if ({}) return {el};\n", render_condition(&when.replace(".length", "?.length"))));
        }
    }

    // Actions.
    let mut buttons = String::new();
    for a in &v.actions {
        let cap = strip_ref(&a.invokes);
        if m.capability(cap).is_none() {
            continue;
        }
        let (res, method) = spa_resource(m, cap);
        let args: Vec<String> = a
            .with
            .iter()
            .map(|(k, val)| format!("{}: {}", camel(k), render_expr(val, &scope)))
            .collect();
        let success = a
            .on_success
            .as_deref()
            .and_then(|nav| nav_href(m, nav, &scope.with_local("result")))
            .map(|h| format!("navigate({h});"))
            .unwrap_or_default();
        buttons.push_str(&format!(
            "      <button\n        type=\"button\"\n        onClick={{async () => {{\n          try {{\n            \
             const result: any = await api.{res}.{method}({{ {args} }});\n            void result;\n            {success}\n          }} catch (err) {{\n            \
             setError(err instanceof Error ? err.message : String(err));\n          }}\n        }}}}\n      >\n        {label}\n      </button>\n",
            args = args.join(", "),
            label = humanize(&a.name),
        ));
    }
    let layout_jsx = render_layout(m, &v.layout, &scope, 3, "to");
    s.push_str("  return (\n");
    s.push_str(&format!("    <main className=\"view-{}\">\n", kebab(&v.name)));
    s.push_str(&layout_jsx);
    s.push_str(&buttons);
    s.push_str("    </main>\n  );\n}\n");
    s
}

fn spa_app(m: &AppModel) -> String {
    let mut imports = String::from(
        "import React from \"react\";\nimport { createBrowserRouter, RouterProvider } from \"react-router-dom\";\n",
    );
    for v in &m.views {
        imports.push_str(&format!("import {name}Page from \"./pages/{name}\";\n", name = pascal(&v.name)));
    }
    let mut routes = String::new();
    for v in &m.views {
        routes.push_str(&format!(
            "  {{ path: \"{path}\", element: <{name}Page /> }},\n",
            path = view_route(v),
            name = pascal(&v.name),
        ));
    }
    format!(
        "{imports}\nconst router = createBrowserRouter([\n{routes}]);\n\n\
         export default function App() {{\n  return <RouterProvider router={{router}} />;\n}}\n",
    )
}

fn spa_main(m: &AppModel) -> String {
    let tokens_import = if m.tokens.is_some() { "import \"./tokens.css\";\n" } else { "" };
    format!(
        "import React from \"react\";\n\
         import {{ createRoot }} from \"react-dom/client\";\n\
         import App from \"./App\";\n{tokens_import}\n\
         createRoot(document.getElementById(\"root\")!).render(<App />);\n",
    )
}

fn spa_index_html(m: &AppModel) -> String {
    format!(
        "<!doctype html>\n<html lang=\"en\">\n  <head>\n    <meta charset=\"UTF-8\" />\n    \
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\" />\n    \
         <title>{title}</title>\n  </head>\n  <body>\n    <div id=\"root\"></div>\n    \
         <script type=\"module\" src=\"/src/main.tsx\"></script>\n  </body>\n</html>\n",
        title = m.app.name,
    )
}

fn spa_vite_config() -> String {
    "import { defineConfig } from \"vite\";\n\
     import react from \"@vitejs/plugin-react\";\n\n\
     export default defineConfig({ plugins: [react()] });\n"
        .to_string()
}

fn spa_package_json(m: &AppModel, p: &Profile) -> String {
    let tailwind_dev = if p.view.get("styling", "tailwind") == "tailwind" {
        ",\n    \"tailwindcss\": \"^3.4.0\",\n    \"postcss\": \"^8.4.0\",\n    \"autoprefixer\": \"^10.4.0\""
    } else {
        ""
    };
    format!(
        "{{\n  \"name\": \"{name}-web\",\n  \"private\": true,\n  \"type\": \"module\",\n  \
         \"scripts\": {{\n    \"dev\": \"vite\",\n    \"build\": \"vite build\",\n    \"preview\": \"vite preview\"\n  }},\n  \
         \"dependencies\": {{\n    \"react\": \"^18.3.0\",\n    \"react-dom\": \"^18.3.0\",\n    \"react-router-dom\": \"^6.26.0\"\n  }},\n  \
         \"devDependencies\": {{\n    \"typescript\": \"^5.6.0\",\n    \"@types/react\": \"^18.3.0\",\n    \"@types/react-dom\": \"^18.3.0\",\n    \"vite\": \"^5.4.0\",\n    \"@vitejs/plugin-react\": \"^4.3.0\"{tailwind_dev}\n  }}\n}}\n",
        name = kebab(&m.app.name),
    )
}

// ---------------------------------------------------------------------------
// Realized-behavior manifest (conformance input)
// ---------------------------------------------------------------------------

fn layout_navigation(node: &Value, out: &mut Vec<String>) {
    match node {
        Value::Array(a) => a.iter().for_each(|x| layout_navigation(x, out)),
        Value::Object(map) => {
            if let Some(on) = map.get("on").and_then(|x| x.as_object()) {
                for (evt, nav) in on {
                    if let Some((target, args)) = nav.as_str().and_then(parse_navigation) {
                        let args: Vec<String> = args.iter().map(|(k, v)| format!("{k}={v}")).collect();
                        out.push(format!("{evt} -> {target}({})", args.join(", ")));
                    }
                }
            }
            for v in map.values() {
                if v.is_array() || v.is_object() {
                    layout_navigation(v, out);
                }
            }
        }
        _ => {}
    }
}

/// What each view realization does, written as `web/routes.json`.
fn routes_manifest(m: &AppModel, next: bool) -> Value {
    let mut views = Map::new();
    for v in &m.views {
        let mut data = Map::new();
        for d in &v.data {
            let from = strip_ref(&d.from);
            let realized = if m.capability(from).is_some() {
                json!({ "from": from, "via": "capability", "field": capability_binding_field(m, d) })
            } else if next {
                json!({ "from": from, "via": "entity", "where": d.where_expr })
            } else {
                json!({ "from": from, "via": "unrealized" })
            };
            data.insert(d.name.clone(), realized);
        }
        let mut actions = Map::new();
        for a in &v.actions {
            actions.insert(
                a.name.clone(),
                json!({
                    "invokes": strip_ref(&a.invokes),
                    "with": a.with,
                    "onSuccess": a.on_success.as_deref().and_then(parse_navigation).map(|(t, _)| t),
                    "onError": a.on_error.as_deref().map(|e| if e.trim() == "stay" { "stay".to_string() } else { parse_navigation(e).map(|(t, _)| t).unwrap_or_default() }),
                }),
            );
        }
        let mut nav = Vec::new();
        layout_navigation(&v.layout, &mut nav);
        nav.sort();
        let guard = match v.policy.as_deref().map(strip_ref) {
            None | Some("public") => json!("public"),
            Some(p) if next => json!(p),
            // The SPA cannot evaluate server policies; capabilities still enforce theirs.
            Some(p) => json!(format!("server-only:{p}")),
        };
        let realized_states: Vec<String> = v
            .states
            .iter()
            .filter(|s| matches!(s.name.as_str(), "loading" | "empty" | "error"))
            .map(|s| s.name.clone())
            .collect();
        let mut transitions: Vec<String> = v.transitions.iter().map(|t| strip_ref(&t.to).to_string()).collect();
        transitions.sort();
        views.insert(
            v.name.clone(),
            json!({
                "route": view_route(v),
                "guard": guard,
                "data": data,
                "actions": actions,
                "navigation": nav,
                "transitions": transitions,
                "states": realized_states,
            }),
        );
    }
    let _ = BTreeMap::<String, String>::new();
    json!({ "views": views })
}
