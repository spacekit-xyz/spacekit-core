//! OpenApp view-layer emitter — `widgets`/`views`/`flows`/`tokens` -> React.
//!
//! Two framework realizations share the same widget/token/flow output:
//!   * `framework: next` (default) — an `app-router` tree of (async) server
//!     components that resolve `data` bindings against the server `db` and call
//!     server actions; widgets are pure prop-driven components.
//!   * `framework: react` — a Vite + react-router SPA whose pages are client
//!     components that fetch capability data through the generated client SDK.

use std::collections::BTreeSet;

use serde_json::Value;

use super::{
    camel, is_next, kebab, pascal, pluralize, snake, AppModel, IoParam, Profile, View, Widget,
};

pub(super) fn emit_view(m: &AppModel, p: &Profile, client_pkg: &str) -> Vec<(String, String)> {
    let framework = p.view.get("framework", "next");
    if is_next(framework) {
        emit_next(m, p)
    } else {
        emit_react_spa(m, p, client_pkg)
    }
}

// ---------------------------------------------------------------------------
// Next.js (app-router / RSC / server actions)
// ---------------------------------------------------------------------------

fn emit_next(m: &AppModel, p: &Profile) -> Vec<(String, String)> {
    let mut out = Vec::new();

    // Design tokens.
    if let Some(tokens) = &m.tokens {
        out.push(("web/app/tokens.css".to_string(), tokens_css(tokens)));
    }

    // Widgets -> components.
    for w in &m.widgets {
        out.push((format!("web/components/{}.tsx", w.name), widget_tsx(m, w)));
    }

    // Root layout.
    out.push(("web/app/layout.tsx".to_string(), root_layout(m, p)));

    // Views -> app-router pages.
    for v in &m.views {
        let dir = route_to_dir(v);
        let path = if dir.is_empty() {
            "web/app/page.tsx".to_string()
        } else {
            format!("web/app/{dir}/page.tsx")
        };
        out.push((path, view_page(m, v, &dir)));
    }

    // Flows -> documentation of the intended journeys.
    if !m.flows.is_empty() {
        out.push(("web/flows.md".to_string(), flows_md(m)));
    }

    out.push(("web/package.json".to_string(), package_json(m, p)));
    out
}

// ---------------------------------------------------------------------------
// React SPA (Vite + react-router + generated client SDK)
// ---------------------------------------------------------------------------

fn emit_react_spa(m: &AppModel, p: &Profile, client_pkg: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();

    // Shared design tokens (imported from the SPA entrypoint).
    if let Some(tokens) = &m.tokens {
        out.push(("web/src/tokens.css".to_string(), tokens_css(tokens)));
    }

    // Shared widgets (same prop-driven components as the Next target).
    for w in &m.widgets {
        out.push((format!("web/components/{}.tsx", w.name), widget_tsx(m, w)));
    }

    // Client SDK wiring.
    out.push(("web/src/api.ts".to_string(), spa_api_ts(client_pkg)));

    // Pages (client components).
    for v in &m.views {
        out.push((
            format!("web/src/pages/{}.tsx", pascal(&v.name)),
            spa_page(m, v),
        ));
    }

    // Router + entrypoint.
    out.push(("web/src/App.tsx".to_string(), spa_app(m)));
    out.push(("web/src/main.tsx".to_string(), spa_main(m)));
    out.push(("web/index.html".to_string(), spa_index_html(m)));
    out.push(("web/vite.config.ts".to_string(), spa_vite_config()));

    if !m.flows.is_empty() {
        out.push(("web/flows.md".to_string(), flows_md(m)));
    }
    out.push((
        "web/package.json".to_string(),
        spa_package_json(m, p, client_pkg),
    ));
    out
}

/// react-router path for a view: `/` stays `/`, `:id` segments are preserved.
fn spa_route(v: &View) -> String {
    match &v.route {
        Some(r) if !r.trim_matches('/').is_empty() => r.clone(),
        Some(_) => "/".to_string(),
        None => format!("/{}", kebab(&v.name)),
    }
}

fn spa_api_ts(client_pkg: &str) -> String {
    format!(
        "// Generated client-SDK wiring for the SPA.\n\
         import {{ Client }} from \"../../client/{pkg}/src/index\";\n\n\
         export const api = new Client({{\n  \
         baseURL: import.meta.env.VITE_API_BASE_URL ?? \"http://localhost:8080\",\n  \
         apiKey: import.meta.env.VITE_API_KEY,\n}});\n\n\
         /// Collect a client-SDK result whether it is an array, a single value,\n\
         /// or a paginated async iterator, into a plain array.\n\
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

fn spa_page(m: &AppModel, v: &View) -> String {
    let mut locals: BTreeSet<String> = BTreeSet::new();
    for d in &v.data {
        locals.insert(d.name.clone());
    }
    for pm in &v.params {
        locals.insert(pm.name.clone());
    }

    // Widget imports.
    let mut widget_imports: BTreeSet<String> = BTreeSet::new();
    collect_layout_widgets(&v.layout, &mut widget_imports);

    let mut imports = String::from("import React, { useEffect, useState } from \"react\";\n");
    if !v.params.is_empty() {
        imports.push_str("import { useParams } from \"react-router-dom\";\n");
    }
    // Only import the client when there is capability-backed data to fetch.
    let has_capability_data = v
        .data
        .iter()
        .any(|d| m.capability(d.from.trim_start_matches('@')).is_some());
    if has_capability_data {
        imports.push_str("import { api, collect } from \"../api\";\n");
    }
    for w in &widget_imports {
        imports.push_str(&format!(
            "import {{ {w} }} from \"../../components/{w}\";\n"
        ));
    }

    // State hooks for each data binding.
    let mut hooks = String::new();
    let mut effects = String::new();
    for d in &v.data {
        let from = d.from.trim_start_matches('@');
        hooks.push_str(&format!(
            "  const [{}, set{}] = useState<any[]>([]);\n",
            camel(&d.name),
            pascal(&d.name)
        ));
        if let Some(cap) = m.capability(from) {
            let res_attr = snake(
                &cap.primary_entity()
                    .map(|e| pluralize(&e))
                    .unwrap_or_else(|| "Resources".to_string()),
            );
            let method = camel(from);
            let args = if d.with.is_empty() {
                String::new()
            } else {
                let parts: Vec<String> = d
                    .with
                    .iter()
                    .map(|(k, val)| format!("{}: {}", camel(k), render_expr(val, &locals)))
                    .collect();
                format!("{{ {} }}", parts.join(", "))
            };
            effects.push_str(&format!(
                "    collect(api.{res}.{method}({args})).then(set{set});\n",
                res = res_attr,
                method = method,
                args = args,
                set = pascal(&d.name),
            ));
        } else {
            effects.push_str(&format!(
                "    // TODO: `{name}` binds @{from} directly; expose a capability to read it in SPA mode.\n",
                name = d.name,
                from = from,
            ));
        }
    }

    let layout_jsx = render_layout(&v.layout, &locals, 3);

    let mut s = String::new();
    s.push_str("// Generated view (OpenApp view layer, React SPA client component).\n");
    s.push_str(&imports);
    s.push('\n');
    if let Some(sum) = &v.summary {
        s.push_str(&format!("/** {sum} */\n"));
    }
    s.push_str(&format!(
        "export default function {}Page() {{\n",
        pascal(&v.name)
    ));
    if !v.params.is_empty() {
        s.push_str("  const params = useParams();\n");
        for p in &v.params {
            s.push_str(&format!(
                "  const {} = params.{} ?? \"\";\n",
                p.name, p.name
            ));
        }
    }
    s.push_str(&hooks);
    if !effects.is_empty() {
        s.push_str("  useEffect(() => {\n");
        s.push_str(&effects);
        s.push_str("  }, []);\n");
    }
    s.push_str("  return (\n");
    s.push_str(&format!(
        "    <main className=\"view-{}\">\n",
        kebab(&v.name)
    ));
    s.push_str(&layout_jsx);
    s.push_str("    </main>\n  );\n}\n");
    s
}

fn spa_app(m: &AppModel) -> String {
    let mut imports = String::from(
        "import React from \"react\";\n\
         import { createBrowserRouter, RouterProvider } from \"react-router-dom\";\n",
    );
    for v in &m.views {
        imports.push_str(&format!(
            "import {name}Page from \"./pages/{name}\";\n",
            name = pascal(&v.name)
        ));
    }
    let mut routes = String::new();
    for v in &m.views {
        routes.push_str(&format!(
            "  {{ path: \"{path}\", element: <{name}Page /> }},\n",
            path = spa_route(v),
            name = pascal(&v.name),
        ));
    }
    format!(
        "{imports}\nconst router = createBrowserRouter([\n{routes}]);\n\n\
         export default function App() {{\n  return <RouterProvider router={{router}} />;\n}}\n",
    )
}

fn spa_main(m: &AppModel) -> String {
    let tokens_import = if m.tokens.is_some() {
        "import \"./tokens.css\";\n"
    } else {
        ""
    };
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

fn spa_package_json(m: &AppModel, p: &Profile, client_pkg: &str) -> String {
    let tailwind_dev = if p.view.get("styling", "tailwind") == "tailwind" {
        ",\n    \"tailwindcss\": \"^3\",\n    \"postcss\": \"^8\",\n    \"autoprefixer\": \"^10\""
    } else {
        ""
    };
    let _ = client_pkg;
    format!(
        "{{\n  \"name\": \"{name}-web\",\n  \"private\": true,\n  \"type\": \"module\",\n  \
         \"scripts\": {{\n    \"dev\": \"vite\",\n    \"build\": \"vite build\",\n    \"preview\": \"vite preview\"\n  }},\n  \
         \"dependencies\": {{\n    \"react\": \"^18\",\n    \"react-dom\": \"^18\",\n    \"react-router-dom\": \"^6\"\n  }},\n  \
         \"devDependencies\": {{\n    \"typescript\": \"^5\",\n    \"@types/react\": \"^18\",\n    \"@types/react-dom\": \"^18\",\n    \"vite\": \"^5\",\n    \"@vitejs/plugin-react\": \"^4\"{tailwind_dev}\n  }}\n}}\n",
        name = kebab(&m.app.name),
    )
}

// ---------------------------------------------------------------------------
// Tokens
// ---------------------------------------------------------------------------

fn tokens_css(tokens: &Value) -> String {
    let mut vars: Vec<String> = Vec::new();
    flatten_tokens("", tokens, &mut vars);
    let mut s =
        String::from("/* Design tokens (OpenApp tokens -> CSS custom properties). */\n:root {\n");
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
            // Spacing-like scales get px; everything else stays raw.
            let unit = if prefix.starts_with("space") {
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

// ---------------------------------------------------------------------------
// Widgets
// ---------------------------------------------------------------------------

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

    let mut s = String::from("import React from \"react\";\n");
    for imp in &imports {
        s.push_str(imp);
    }
    s.push('\n');
    s.push_str(&format!(
        "export interface {}Props {{\n{}}}\n\n",
        w.name, props_lines
    ));
    s.push_str(&format!(
        "export function {name}(props: {name}Props) {{\n  return (\n    <div className=\"{cls}\">\n",
        name = w.name,
        cls = format!("widget-{}", kebab(&w.name)),
    ));
    // Render scalar props as text; slots as children.
    for p in &w.props {
        if !p.ty.starts_with('@') {
            s.push_str(&format!(
                "      <span data-prop=\"{n}\">{{String(props.{c} ?? \"\")}}</span>\n",
                n = p.name,
                c = camel(&p.name)
            ));
        }
    }
    for slot in &w.slots {
        s.push_str(&format!(
            "      <div data-slot=\"{slot}\">{{props.{c}}}</div>\n",
            c = camel(slot)
        ));
    }
    s.push_str("    </div>\n  );\n}\n");
    s
}

fn prop_ts(m: &AppModel, p: &IoParam, imports: &mut BTreeSet<String>) -> String {
    let base = if let Some(ent) = p.ty.strip_prefix('@') {
        if m.entity(ent).is_some() {
            imports.insert(format!(
                "import type {{ {ent} }} from \"../../server/types\";\n"
            ));
        }
        ent.to_string()
    } else if p.ty == "enum" && !p.values.is_empty() {
        p.values
            .iter()
            .map(|v| format!("\"{v}\""))
            .collect::<Vec<_>>()
            .join(" | ")
    } else {
        match p.ty.as_str() {
            "integer" | "decimal" | "money" => "number",
            "boolean" => "boolean",
            _ => "string",
        }
        .to_string()
    };
    if p.list {
        format!("{base}[]")
    } else {
        base
    }
}

// ---------------------------------------------------------------------------
// Views
// ---------------------------------------------------------------------------

/// `/books/:id` -> `books/[id]`, `/` -> ``.
fn route_to_dir(v: &View) -> String {
    let route = match &v.route {
        Some(r) => r.clone(),
        None => format!("/{}", kebab(&v.name)),
    };
    let trimmed = route.trim_matches('/');
    if trimmed.is_empty() {
        return String::new();
    }
    trimmed
        .split('/')
        .map(|seg| {
            if let Some(param) = seg.strip_prefix(':') {
                format!("[{param}]")
            } else {
                seg.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn view_page(m: &AppModel, v: &View, dir: &str) -> String {
    // Relative depth from web/app/<dir>/page.tsx back to the output root.
    let depth = if dir.is_empty() {
        0
    } else {
        dir.split('/').count()
    };
    let up = "../".repeat(depth + 2); // page.tsx -> app -> web -> root

    // Local binding scope: data names, params, and repeat variables.
    let mut locals: BTreeSet<String> = BTreeSet::new();
    for d in &v.data {
        locals.insert(d.name.clone());
    }
    for pm in &v.params {
        locals.insert(pm.name.clone());
    }

    let mut imports = String::from("import React from \"react\";\n");
    let mut widget_imports: BTreeSet<String> = BTreeSet::new();
    collect_layout_widgets(&v.layout, &mut widget_imports);
    for w in &widget_imports {
        imports.push_str(&format!(
            "import {{ {w} }} from \"{up}web/components/{w}\";\n"
        ));
    }

    // Capability data bindings import server actions.
    let mut action_imports: BTreeSet<String> = BTreeSet::new();
    for d in &v.data {
        let from = d.from.trim_start_matches('@');
        if m.capability(from).is_some() {
            action_imports.insert(camel(from));
        }
    }
    for a in &v.actions {
        let from = a.invokes.trim_start_matches('@');
        if m.capability(from).is_some() {
            action_imports.insert(camel(from));
        }
    }
    if !action_imports.is_empty() {
        let names: Vec<String> = action_imports.iter().cloned().collect();
        imports.push_str(&format!(
            "import {{ {} }} from \"{up}server/actions\";\n",
            names.join(", ")
        ));
    }

    let needs_db = v
        .data
        .iter()
        .any(|d| m.entity(d.from.trim_start_matches('@')).is_some());
    if needs_db {
        imports.push_str(&format!("import {{ db }} from \"{up}server/db\";\n"));
    }

    // Props (route params) for the page.
    let params_arg = if v.params.is_empty() {
        String::new()
    } else {
        let fields: Vec<String> = v
            .params
            .iter()
            .map(|p| format!("{}: string", p.name))
            .collect();
        format!("{{ params }}: {{ params: {{ {} }} }}", fields.join("; "))
    };

    let mut body = String::new();
    // Resolve data bindings.
    for d in &v.data {
        let from = d.from.trim_start_matches('@');
        if let Some(cap) = m.capability(from) {
            let args = if d.with.is_empty() {
                "{}".to_string()
            } else {
                let parts: Vec<String> = d
                    .with
                    .iter()
                    .map(|(k, val)| format!("{}: {}", camel(k), render_expr(val, &locals)))
                    .collect();
                format!("{{ {} }}", parts.join(", "))
            };
            let _ = cap;
            body.push_str(&format!(
                "  const {} = await {}({});\n",
                camel(&d.name),
                camel(from),
                args
            ));
        } else if let Some(ent) = m.entity(from) {
            // Entity binding -> Prisma query (findFirst when filtered, else findMany).
            if let Some(w) = &d.where_expr {
                body.push_str(&format!(
                    "  const {} = await db.{}.findFirst({{ where: {} }});\n",
                    camel(&d.name),
                    camel(&ent.name),
                    where_to_prisma(w, &locals)
                ));
            } else {
                body.push_str(&format!(
                    "  const {} = await db.{}.findMany();\n",
                    camel(&d.name),
                    camel(&ent.name)
                ));
            }
        }
    }

    let layout_jsx = render_layout(&v.layout, &locals, 3);

    let mut s = String::new();
    s.push_str("// Generated view (OpenApp view layer, React app-router server component).\n");
    s.push_str(&imports);
    s.push('\n');
    if let Some(sum) = &v.summary {
        s.push_str(&format!("/** {sum} */\n"));
    }
    s.push_str(&format!(
        "export default async function {}Page({}) {{\n",
        pascal(&v.name),
        params_arg
    ));
    // Destructure params for local use.
    for p in &v.params {
        body.insert_str(0, &format!("  const {} = params.{};\n", p.name, p.name));
    }
    s.push_str(&body);
    s.push_str("  return (\n");
    s.push_str(&format!(
        "    <main className=\"view-{}\">\n",
        kebab(&v.name)
    ));
    s.push_str(&layout_jsx);
    s.push_str("    </main>\n  );\n}\n");
    s
}

fn collect_layout_widgets(node: &Value, out: &mut BTreeSet<String>) {
    match node {
        Value::Array(items) => items.iter().for_each(|i| collect_layout_widgets(i, out)),
        Value::Object(map) => {
            if let Some(w) = map.get("widget").and_then(|x| x.as_str()) {
                out.insert(w.trim_start_matches('@').to_string());
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

/// Render a layout subtree to JSX at the given indent (in 2-space units).
fn render_layout(node: &Value, locals: &BTreeSet<String>, indent: usize) -> String {
    let pad = "  ".repeat(indent);
    match node {
        Value::Array(items) => items
            .iter()
            .map(|i| render_layout(i, locals, indent))
            .collect::<String>(),
        Value::Object(map) => {
            // Repeat wrapper.
            if let Some(rep) = map.get("repeat").and_then(|x| x.as_str()) {
                // "book in books"
                let mut parts = rep.split_whitespace();
                let var = parts.next().unwrap_or("item").to_string();
                let _in = parts.next();
                let coll = parts.next().unwrap_or("items").to_string();
                let mut inner_locals = locals.clone();
                inner_locals.insert(var.clone());
                // Render the element without the repeat key.
                let mut child = map.clone();
                child.remove("repeat");
                let child_jsx = render_layout(&Value::Object(child), &inner_locals, indent + 2);
                let coll_expr = render_expr(&coll, locals);
                return format!(
                    "{pad}{{{coll}.map(({var}: any) => (\n{child}{pad}))}}\n",
                    coll = coll_expr,
                    var = var,
                    child = child_jsx,
                );
            }
            if let Some(w) = map.get("widget").and_then(|x| x.as_str()) {
                let name = w.trim_start_matches('@');
                let mut attrs = String::new();
                if let Some(props) = map.get("props").and_then(|x| x.as_object()) {
                    for (k, val) in props {
                        attrs.push_str(&format!(
                            " {}={{{}}}",
                            camel(k),
                            render_value_expr(val, locals)
                        ));
                    }
                }
                // Slots become render-prop attributes.
                let mut slot_attrs = String::new();
                if let Some(slots) = map.get("slots").and_then(|x| x.as_object()) {
                    for (slot, content) in slots {
                        let inner = render_layout(content, locals, indent + 2);
                        slot_attrs.push_str(&format!(
                            "{pad}  {slot}={{<>\n{inner}{pad}  </>}}\n",
                            slot = camel(slot)
                        ));
                    }
                }
                if slot_attrs.is_empty() {
                    return format!("{pad}<{name}{attrs} />\n");
                }
                return format!("{pad}<{name}{attrs}\n{slot_attrs}{pad}/>\n");
            }
            if let Some(vn) = map.get("view").and_then(|x| x.as_str()) {
                let name = pascal(vn.trim_start_matches('@'));
                return format!("{pad}<{name}Page />\n");
            }
            String::new()
        }
        _ => String::new(),
    }
}

/// Render a props value (string binding/literal, or other JSON) to a JS expr.
fn render_value_expr(v: &Value, locals: &BTreeSet<String>) -> String {
    match v {
        Value::String(s) => render_expr(s, locals),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }
}

/// A bare string is an expression if its head identifier is a known local
/// binding (or it's a dotted/`params.`/navigation form); otherwise it's a string
/// literal.
fn render_expr(s: &str, locals: &BTreeSet<String>) -> String {
    let t = s.trim();
    // Navigation arrow: `-> @View(id: x)` -> a router push expression placeholder.
    if let Some(rest) = t.strip_prefix("->") {
        let target = rest.trim();
        return format!("() => {{/* navigate: {target} */}}");
    }
    let head = t.split(['.', '(']).next().unwrap_or(t);
    let is_binding = locals.contains(head) || head == "params" || head == "result";
    let looks_ident = !t.is_empty()
        && t.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.');
    if is_binding && looks_ident {
        t.to_string()
    } else if looks_ident && t.contains('.') {
        // dotted access on an unknown head — still treat as expression
        t.to_string()
    } else {
        format!("\"{}\"", t.replace('"', "\\\""))
    }
}

fn where_to_prisma(expr: &str, locals: &BTreeSet<String>) -> String {
    // Handle the common `field == value` form; fall back to a passthrough comment.
    if let Some((lhs, rhs)) = expr.split_once("==") {
        let field = lhs.trim();
        let val = render_expr(rhs.trim(), locals);
        return format!("{{ {field}: {val} }}");
    }
    format!("{{ /* {expr} */ }}")
}

fn root_layout(m: &AppModel, p: &Profile) -> String {
    let tokens_import = if m.tokens.is_some() {
        "import \"./tokens.css\";\n"
    } else {
        ""
    };
    let tailwind = p.view.get("styling", "tailwind") == "tailwind";
    let globals = if tailwind {
        "// Tailwind: add your globals.css with @tailwind directives.\n"
    } else {
        ""
    };
    format!(
        "import React from \"react\";\n{tokens_import}{globals}\n\
         export const metadata = {{ title: \"{title}\" }};\n\n\
         export default function RootLayout({{ children }}: {{ children: React.ReactNode }}) {{\n  \
         return (\n    <html lang=\"en\">\n      <body>{{children}}</body>\n    </html>\n  );\n}}\n",
        title = m.app.name,
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
                s.push_str(&format!("1. `{}`", at.trim_start_matches('@')));
                if let Some(on) = step.get("on") {
                    s.push_str(&format!("  — on {}", on));
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
        ",\n    \"tailwindcss\": \"^3\",\n    \"postcss\": \"^8\",\n    \"autoprefixer\": \"^10\""
    } else {
        ""
    };
    format!(
        "{{\n  \"name\": \"{name}-web\",\n  \"private\": true,\n  \
         \"dependencies\": {{\n    \"next\": \"^14\",\n    \"react\": \"^18\",\n    \"react-dom\": \"^18\"\n  }},\n  \
         \"devDependencies\": {{\n    \"typescript\": \"^5\",\n    \"@types/react\": \"^18\"{tailwind_dev}\n  }}\n}}\n",
        name = kebab(&m.app.name),
    )
}
