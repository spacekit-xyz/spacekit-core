//! OpenApp business-layer emitter — `capabilities`/`events` -> TypeScript.
//!
//! Honors the profile's `transport` (server-actions / rest / rpc — all surface
//! as async TS functions here) and `errors` realization (problem-json throws an
//! `ApiError`; result-type returns a discriminated union). `binding: compile`
//! only (runtime binding is a future fork).

use super::{camel, AppModel, Capability, Entity, FieldDef, IoParam, Profile};

pub(super) fn emit_business(m: &AppModel, p: &Profile, client_pkg: &str) -> Vec<(String, String)> {
    let _ = client_pkg;
    // NOTE: `server/db.ts` (the data-access handle) is owned by the data-layer
    // emitter, since its realization depends on the chosen `store`.
    let mut out = Vec::new();
    out.push(("server/types.ts".to_string(), types_ts(m)));
    out.push(("server/errors.ts".to_string(), errors_ts(m, p)));

    let mut index = String::from("// Generated server actions (one per capability).\n");
    for c in &m.capabilities {
        let file = format!("server/actions/{}.ts", camel(&c.name));
        out.push((file, action_ts(m, c, p)));
        index.push_str(&format!("export * from \"./{}\";\n", camel(&c.name)));
    }
    out.push(("server/actions/index.ts".to_string(), index));
    out.push(("server/package.json".to_string(), package_json(m)));
    out
}

/// OpenApp scalar -> TypeScript type.
fn ts_scalar(ty: &str) -> &'static str {
    match ty {
        "integer" | "decimal" | "money" => "number",
        "boolean" => "boolean",
        "json" => "unknown",
        _ => "string",
    }
}

fn ts_type_of(m: &AppModel, ty: &str, values: &[String], list: bool) -> String {
    let base = if let Some(stripped) = ty.strip_prefix('@') {
        stripped.to_string()
    } else if ty == "enum" && !values.is_empty() {
        values
            .iter()
            .map(|v| format!("\"{v}\""))
            .collect::<Vec<_>>()
            .join(" | ")
    } else {
        ts_scalar(ty).to_string()
    };
    let _ = m;
    if list {
        format!("{base}[]")
    } else {
        base
    }
}

fn entity_ts(m: &AppModel, e: &Entity) -> String {
    let mut s = format!("export interface {} {{\n", e.name);
    for f in &e.fields {
        let opt = if f.required { "" } else { "?" };
        let ty = field_ts(m, f);
        s.push_str(&format!("  {}{}: {};\n", camel(&f.name), opt, ty));
    }
    for r in &e.relations {
        let target = &r.target;
        let (decl, ty) = match r.kind {
            super::RelKind::HasMany | super::RelKind::ManyToMany => ("?", format!("{target}[]")),
            _ => ("?", target.clone()),
        };
        s.push_str(&format!("  {}{}: {};\n", camel(&r.name), decl, ty));
    }
    s.push_str("}\n");
    s
}

fn field_ts(m: &AppModel, f: &FieldDef) -> String {
    ts_type_of(m, &f.ty, &f.values, false)
}

fn io_interface(m: &AppModel, name: &str, io: &[IoParam]) -> String {
    if io.is_empty() {
        return format!("export type {name} = Record<string, never>;\n");
    }
    let mut s = format!("export interface {name} {{\n");
    for p in io {
        let opt = if p.required { "" } else { "?" };
        let ty = ts_type_of(m, &p.ty, &p.values, p.list);
        s.push_str(&format!("  {}{}: {};\n", camel(&p.name), opt, ty));
    }
    s.push_str("}\n");
    s
}

fn types_ts(m: &AppModel) -> String {
    let mut s =
        String::from("// Generated domain + capability types (OpenApp business layer).\n\n");
    for e in &m.entities {
        s.push_str(&entity_ts(m, e));
        s.push('\n');
    }
    for c in &m.capabilities {
        s.push_str(&io_interface(m, &format!("{}Input", c.name), &c.input));
        s.push_str(&io_interface(m, &format!("{}Output", c.name), &c.output));
        s.push('\n');
    }
    for ev in &m.events {
        s.push_str(&io_interface(m, &format!("{}Event", ev.name), &ev.payload));
        s.push('\n');
    }
    s
}

fn errors_ts(m: &AppModel, p: &Profile) -> String {
    let mut codes: Vec<String> = m
        .capabilities
        .iter()
        .flat_map(|c| c.errors.clone())
        .collect();
    codes.sort();
    codes.dedup();
    let union = if codes.is_empty() {
        "string".to_string()
    } else {
        codes
            .iter()
            .map(|c| format!("\"{c}\""))
            .collect::<Vec<_>>()
            .join(" | ")
    };
    let style = p.business.get("errors", "problem-json");
    let mut s = format!("// Capability error codes (errors realization: {style}).\nexport type ErrorCode = {union};\n\n");
    s.push_str(
        "export class ApiError extends Error {\n  \
         constructor(public code: ErrorCode, message?: string) {\n    \
         super(message ?? code);\n    this.name = \"ApiError\";\n  }\n}\n\n",
    );
    s.push_str(
        "export type Result<T> = { ok: true; value: T } | { ok: false; error: ErrorCode };\n",
    );
    s
}

fn action_ts(m: &AppModel, c: &Capability, p: &Profile) -> String {
    let _ = m;
    let fn_name = camel(&c.name);
    let input_ty = format!("{}Input", c.name);
    let output_ty = format!("{}Output", c.name);
    let result_type = p.business.get("errors", "problem-json") == "result-type";
    let transport = p.business.effective(&c.name, "transport", "rest");
    let server_action = transport == "server-actions";

    let ret = if result_type {
        format!("Result<{output_ty}>")
    } else {
        format!("{output_ty}")
    };

    let mut s =
        String::from("// Generated capability handler. Fill in the body; the contract is fixed.\n");
    if server_action {
        s.push_str("\"use server\";\n");
    }
    s.push_str("import { db } from \"../db\";\n");
    s.push_str(&format!(
        "import type {{ {input_ty}, {output_ty} }} from \"../types\";\n"
    ));
    if result_type {
        s.push_str("import type { Result } from \"../errors\";\n");
    } else {
        s.push_str("import { ApiError } from \"../errors\";\n");
    }
    s.push('\n');

    if let Some(sum) = &c.summary {
        s.push_str(&format!("/** {sum} */\n"));
    }
    // Contract annotations (the auditable side-effect surface from the spec).
    s.push_str(&format!(
        "// policy:  {}\n",
        c.policy.clone().unwrap_or_else(|| "public".into())
    ));
    if !c.reads.is_empty() {
        s.push_str(&format!("// reads:   {}\n", c.reads.join(", ")));
    }
    if !c.writes.is_empty() {
        let ws: Vec<String> = c
            .writes
            .iter()
            .map(|w| format!("{} {}", w.effect, w.entity))
            .collect();
        s.push_str(&format!("// writes:  {}\n", ws.join(", ")));
    }
    if !c.emits.is_empty() {
        s.push_str(&format!("// emits:   {}\n", c.emits.join(", ")));
    }

    s.push_str(&format!(
        "export async function {fn_name}(input: {input_ty}): Promise<{ret}> {{\n"
    ));
    s.push_str("  void input;\n  void db;\n");
    if result_type {
        s.push_str(&format!(
            "  // TODO: implement {}. Return {{ ok: true, value }} or {{ ok: false, error }}.\n",
            c.name
        ));
        s.push_str(&format!(
            "  throw new Error(\"Not implemented: {}\");\n",
            c.name
        ));
    } else {
        s.push_str(&format!(
            "  // TODO: implement {}. Throw `new ApiError(code)` for declared failures.\n",
            c.name
        ));
        s.push_str("  void ApiError;\n");
        s.push_str(&format!(
            "  throw new Error(\"Not implemented: {}\");\n",
            c.name
        ));
    }
    s.push_str("}\n");
    s
}

fn package_json(m: &AppModel) -> String {
    format!(
        "{{\n  \"name\": \"{name}-server\",\n  \"private\": true,\n  \"type\": \"module\",\n  \
         \"dependencies\": {{\n    \"@prisma/client\": \"^5\"\n  }},\n  \
         \"devDependencies\": {{\n    \"prisma\": \"^5\",\n    \"typescript\": \"^5\"\n  }}\n}}\n",
        name = super::kebab(&m.app.name),
    )
}
