//! `spacekit agent map` (v2) + `spacekit agent pack` — multi-language project
//! graph and LLM context packer. Self-contained: tree-sitter analyzers for
//! C#/TS/Rust/Bicep, a regex SQL pass, cross-file resolution, and a context
//! packer. Python is delegated to the existing `code_session` analyzer via
//! `super::code_session::python_lang_files` so prior behavior is preserved.
//!
//! Integration notes:
//!   * declared as `mod repo_lang;` in `full_client.rs`
//!   * `AgentCommands::Map` -> `repo_lang::handle_map(root, out)`
//!   * add `AgentCommands::Pack {..}` -> `repo_lang::handle_pack(..)`

use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::error::Error;
use std::path::{Path, PathBuf};
use tree_sitter::Node;

// Python delegated to the existing analyzer (preserves prior behavior).
use super::code_session::python_lang_files;

// Language-neutral structural analysis for the repo map.
// Each analyzer parses one file with tree-sitter and returns a `LangFile`:
// declared modules/namespaces, symbols (with container nesting), imports
// (module refs + path refs), extends/implements, best-effort calls, and
// test markers. The repo-map builder turns these into nodes/edges and does
// cross-file resolution (namespace/symbol -> file/id).


#[derive(Debug, Serialize, Default)]
pub struct Sym {
    pub kind: String,          // class|interface|struct|enum|record|method|function|property|field|sql_object|bicep_resource|bicep_module|pipeline_stage|pipeline_job|test|namespace|module
    pub name: String,
    pub line: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container: Option<String>, // dotted container path within the file, e.g. "Acme.Services.GreeterService"
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub bases: Vec<String>,        // base classes / interfaces / traits
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub is_test: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub calls: Vec<String>,       // best-effort callee simple names
}

#[derive(Debug, Serialize, Default)]
pub struct Imp {
    pub target: String,           // namespace/module string OR path
    pub kind: String,             // "module" (external/namespaced) | "path" (relative file ref) | "namespace"
}

#[derive(Debug, Serialize, Default)]
pub struct LangFile {
    pub lang: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub namespaces: Vec<String>,  // namespaces/modules this file *declares*
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub symbols: Vec<Sym>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub imports: Vec<Imp>,
}

fn txt<'a>(n: Node, src: &'a str) -> &'a str {
    n.utf8_text(src.as_bytes()).unwrap_or("")
}
fn line_of(n: Node) -> usize { n.start_position().row + 1 }

fn field_text<'a>(n: Node, field: &str, src: &'a str) -> Option<&'a str> {
    n.child_by_field_name(field).map(|c| txt(c, src))
}

// ─────────────────────────────── C# ───────────────────────────────

pub fn analyze_csharp(src: &str) -> LangFile {
    let mut out = LangFile { lang: "csharp".into(), ..Default::default() };
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&tree_sitter_c_sharp::LANGUAGE.into()).unwrap();
    let tree = match parser.parse(src, None) { Some(t) => t, None => return out };
    let root = tree.root_node();
    walk_csharp(root, src, None, &mut out);
    out
}

fn cs_name(n: Node, src: &str) -> String {
    // name field is usually an identifier or qualified_name
    field_text(n, "name", src).unwrap_or("").to_string()
}

fn walk_csharp(node: Node, src: &str, container: Option<String>, out: &mut LangFile) {
    let mut cur = node.walk();
    for child in node.children(&mut cur) {
        match child.kind() {
            "using_directive" => {
                // grab the last identifier/qualified_name child = the namespace
                let mut c2 = child.walk();
                for gc in child.children(&mut c2) {
                    if matches!(gc.kind(), "identifier" | "qualified_name" | "name_equals") { }
                }
                // simpler: text minus "using"/";"/"static"
                let raw = txt(child, src).trim_start_matches("using").trim();
                let ns = raw.trim_end_matches(';').replace("static ", "").trim().to_string();
                if !ns.is_empty() {
                    out.imports.push(Imp { target: ns, kind: "namespace".into() });
                }
            }
            "namespace_declaration" | "file_scoped_namespace_declaration" => {
                let name = cs_name(child, src);
                if !name.is_empty() { out.namespaces.push(name.clone()); }
                let newc = Some(join(container.as_deref(), &name));
                walk_csharp(child_body(child), src, newc, out);
            }
            "class_declaration" | "interface_declaration" | "struct_declaration"
            | "record_declaration" | "record_struct_declaration" | "enum_declaration" => {
                let kind = match child.kind() {
                    "class_declaration" => "class",
                    "interface_declaration" => "interface",
                    "struct_declaration" => "struct",
                    "enum_declaration" => "enum",
                    _ => "record",
                };
                let name = cs_name(child, src);
                let bases = cs_bases(child, src);
                let full = join(container.as_deref(), &name);
                out.symbols.push(Sym {
                    kind: kind.into(), name: name.clone(), line: line_of(child),
                    container: container.clone(), bases, ..Default::default()
                });
                walk_csharp(child_body(child), src, Some(full), out);
            }
            "method_declaration" | "constructor_declaration" | "local_function_statement" => {
                let name = if child.kind() == "constructor_declaration" {
                    cs_name(child, src)
                } else { cs_name(child, src) };
                let is_test = has_test_attr(child, src);
                let calls = collect_calls_cs(child_body(child), src);
                out.symbols.push(Sym {
                    kind: "method".into(), name, line: line_of(child),
                    container: container.clone(), is_test, calls, ..Default::default()
                });
            }
            "property_declaration" => {
                out.symbols.push(Sym {
                    kind: "property".into(), name: cs_name(child, src), line: line_of(child),
                    container: container.clone(), ..Default::default()
                });
            }
            "field_declaration" => {
                // variable_declaration -> variable_declarator name
                if let Some(vd) = child.child_by_field_name("declaration").or_else(|| first_kind(child, "variable_declaration")) {
                    if let Some(decl) = first_kind(vd, "variable_declarator") {
                        out.symbols.push(Sym {
                            kind: "field".into(), name: field_text(decl, "name", src).unwrap_or("").into(),
                            line: line_of(child), container: container.clone(), ..Default::default()
                        });
                    }
                }
            }
            "declaration_list" | "compilation_unit" => {
                walk_csharp(child, src, container.clone(), out);
            }
            _ => {
                // recurse to catch nested namespaces/classes
                if child.child_count() > 0 { walk_csharp(child, src, container.clone(), out); }
            }
        }
    }
}

fn child_body(n: Node) -> Node { n.child_by_field_name("body").unwrap_or(n) }

fn first_kind<'a>(n: Node<'a>, kind: &str) -> Option<Node<'a>> {
    let mut c = n.walk();
    for ch in n.children(&mut c) { if ch.kind() == kind { return Some(ch); } }
    None
}

fn cs_bases(n: Node, src: &str) -> Vec<String> {
    let mut v = Vec::new();
    if let Some(bl) = first_kind(n, "base_list") {
        let mut c = bl.walk();
        for ch in bl.children(&mut c) {
            if matches!(ch.kind(), "identifier" | "qualified_name" | "generic_name") {
                v.push(txt(ch, src).to_string());
            }
        }
    }
    v
}

fn has_test_attr(n: Node, src: &str) -> bool {
    // look for attribute_list preceding: [Fact],[Theory],[Test],[TestMethod]
    let mut c = n.walk();
    for ch in n.children(&mut c) {
        if ch.kind() == "attribute_list" {
            let t = txt(ch, src);
            if t.contains("Fact") || t.contains("Theory") || t.contains("Test") { return true; }
        }
    }
    // also check text prefix
    false
}

fn collect_calls_cs(n: Node, src: &str) -> Vec<String> {
    let mut v = Vec::new();
    collect_calls_cs_rec(n, src, &mut v);
    v.sort(); v.dedup(); v
}
fn collect_calls_cs_rec(n: Node, src: &str, v: &mut Vec<String>) {
    let mut c = n.walk();
    for ch in n.children(&mut c) {
        if ch.kind() == "invocation_expression" {
            if let Some(f) = ch.child_by_field_name("function") {
                let name = match f.kind() {
                    "member_access_expression" => field_text(f, "name", src).unwrap_or("").to_string(),
                    "identifier" => txt(f, src).to_string(),
                    _ => String::new(),
                };
                if !name.is_empty() { v.push(name); }
            }
        }
        if ch.child_count() > 0 { collect_calls_cs_rec(ch, src, v); }
    }
}

fn join(a: Option<&str>, b: &str) -> String {
    match a { Some(a) if !a.is_empty() => format!("{}.{}", a, b), _ => b.to_string() }
}

// ─────────────────────────── shared call collector ───────────────────────────
fn collect_calls_generic(n: Node, src: &str, v: &mut Vec<String>) {
    let mut c = n.walk();
    for ch in n.children(&mut c) {
        if matches!(ch.kind(), "call_expression" | "invocation_expression") {
            if let Some(f) = ch.child_by_field_name("function") {
                let name = match f.kind() {
                    "member_expression" | "member_access_expression" =>
                        field_text(f, "property", src).or_else(|| field_text(f, "name", src)).unwrap_or("").to_string(),
                    "identifier" => txt(f, src).to_string(),
                    _ => String::new(),
                };
                if !name.is_empty() { v.push(name); }
            }
        }
        if ch.child_count() > 0 { collect_calls_generic(ch, src, v); }
    }
}
fn dedup(mut v: Vec<String>) -> Vec<String> { v.sort(); v.dedup(); v }

// ─────────────────────────────── TypeScript ───────────────────────────────
pub fn analyze_ts(src: &str, tsx: bool) -> LangFile {
    let mut out = LangFile { lang: if tsx {"tsx"} else {"typescript"}.into(), ..Default::default() };
    let mut parser = tree_sitter::Parser::new();
    let lang = if tsx { tree_sitter_typescript::LANGUAGE_TSX } else { tree_sitter_typescript::LANGUAGE_TYPESCRIPT };
    parser.set_language(&lang.into()).unwrap();
    let tree = match parser.parse(src, None) { Some(t) => t, None => return out };
    walk_ts(tree.root_node(), src, None, &mut out);
    out
}
fn ts_str_source(imp: Node, src: &str) -> Option<String> {
    let s = imp.child_by_field_name("source")?;
    let raw = txt(s, src);
    Some(raw.trim_matches(|c| c=='"' || c=='\'' || c=='`').to_string())
}
fn walk_ts(node: Node, src: &str, container: Option<String>, out: &mut LangFile) {
    let mut cur = node.walk();
    for child in node.children(&mut cur) {
        match child.kind() {
            "import_statement" => {
                if let Some(p) = ts_str_source(child, src) {
                    let kind = if p.starts_with('.') { "path" } else { "module" };
                    out.imports.push(Imp { target: p, kind: kind.into() });
                }
            }
            "export_statement" => {
                if let Some(decl) = child.child_by_field_name("declaration") {
                    walk_ts_decl(decl, src, container.clone(), out);
                } else { walk_ts(child, src, container.clone(), out); }
            }
            "class_declaration" | "abstract_class_declaration" | "function_declaration"
            | "interface_declaration" | "lexical_declaration" | "enum_declaration"
            | "type_alias_declaration" | "internal_module" | "module" => {
                walk_ts_decl(child, src, container.clone(), out);
            }
            _ => { if child.child_count() > 0 { walk_ts(child, src, container.clone(), out); } }
        }
    }
}
fn ts_type_name(n: Node, src: &str) -> String { field_text(n, "name", src).unwrap_or("").to_string() }
fn walk_ts_decl(child: Node, src: &str, container: Option<String>, out: &mut LangFile) {
    match child.kind() {
        "class_declaration" | "abstract_class_declaration" => {
            let name = ts_type_name(child, src);
            let mut bases = Vec::new();
            if let Some(h) = first_kind(child, "class_heritage") {
                let mut c = h.walk();
                for ec in h.children(&mut c) {
                    let mut c2 = ec.walk();
                    for id in ec.children(&mut c2) {
                        if matches!(id.kind(), "identifier" | "type_identifier" | "generic_type") { bases.push(txt(id, src).to_string()); }
                    }
                }
            }
            let full = join(container.as_deref(), &name);
            out.symbols.push(Sym { kind: "class".into(), name, line: line_of(child), container: container.clone(), bases, ..Default::default() });
            if let Some(body) = child.child_by_field_name("body") { walk_ts_members(body, src, Some(full), out); }
        }
        "interface_declaration" => {
            out.symbols.push(Sym { kind: "interface".into(), name: ts_type_name(child, src), line: line_of(child), container: container.clone(), ..Default::default() });
        }
        "function_declaration" | "generator_function_declaration" => {
            let calls = { let mut v=Vec::new(); if let Some(b)=child.child_by_field_name("body"){collect_calls_generic(b,src,&mut v);} dedup(v) };
            out.symbols.push(Sym { kind: "function".into(), name: field_text(child,"name",src).unwrap_or("").into(), line: line_of(child), container: container.clone(), calls, ..Default::default() });
        }
        "enum_declaration" => out.symbols.push(Sym { kind:"enum".into(), name: ts_type_name(child,src), line: line_of(child), container: container.clone(), ..Default::default() }),
        "type_alias_declaration" => out.symbols.push(Sym { kind:"type".into(), name: ts_type_name(child,src), line: line_of(child), container: container.clone(), ..Default::default() }),
        "lexical_declaration" | "variable_declaration" => {
            let mut c = child.walk();
            for d in child.children(&mut c) {
                if d.kind() == "variable_declarator" {
                    let val = d.child_by_field_name("value");
                    let is_fn = val.map(|v| matches!(v.kind(), "arrow_function" | "function" | "function_expression")).unwrap_or(false);
                    if is_fn {
                        let calls = { let mut v=Vec::new(); if let Some(b)=val { collect_calls_generic(b,src,&mut v);} dedup(v) };
                        out.symbols.push(Sym { kind:"function".into(), name: field_text(d,"name",src).unwrap_or("").into(), line: line_of(d), container: container.clone(), calls, ..Default::default() });
                    }
                }
            }
        }
        _ => {}
    }
}
fn walk_ts_members(body: Node, src: &str, container: Option<String>, out: &mut LangFile) {
    let mut c = body.walk();
    for m in body.children(&mut c) {
        match m.kind() {
            "method_definition" => {
                let calls = { let mut v=Vec::new(); if let Some(b)=m.child_by_field_name("body"){collect_calls_generic(b,src,&mut v);} dedup(v) };
                out.symbols.push(Sym { kind:"method".into(), name: field_text(m,"name",src).unwrap_or("").into(), line: line_of(m), container: container.clone(), calls, ..Default::default() });
            }
            "public_field_definition" | "field_definition" =>
                out.symbols.push(Sym { kind:"property".into(), name: field_text(m,"name",src).unwrap_or("").into(), line: line_of(m), container: container.clone(), ..Default::default() }),
            _ => {}
        }
    }
}

// ─────────────────────────────── Rust ───────────────────────────────
pub fn analyze_rust(src: &str) -> LangFile {
    let mut out = LangFile { lang: "rust".into(), ..Default::default() };
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&tree_sitter_rust::LANGUAGE.into()).unwrap();
    let tree = match parser.parse(src, None) { Some(t) => t, None => return out };
    walk_rust(tree.root_node(), src, None, false, &mut out);
    out
}
fn walk_rust(node: Node, src: &str, container: Option<String>, in_test: bool, out: &mut LangFile) {
    let mut cur = node.walk();
    let mut pending_test = false; // set by #[test]/#[cfg(test)] attribute_item
    for child in node.children(&mut cur) {
        match child.kind() {
            "attribute_item" => {
                let t = txt(child, src);
                if t.contains("test") || t.contains("cfg(test") { pending_test = true; }
                continue;
            }
            "use_declaration" => {
                if let Some(a) = child.child_by_field_name("argument") {
                    let raw = txt(a, src).to_string();
                    let kind = if raw.starts_with("crate")||raw.starts_with("self")||raw.starts_with("super") {"path"} else {"module"};
                    out.imports.push(Imp { target: raw, kind: kind.into() });
                }
            }
            "mod_item" => {
                let name = field_text(child, "name", src).unwrap_or("").to_string();
                let is_test = in_test || pending_test || name == "tests" || name == "test";
                let full = join(container.as_deref(), &name);
                out.symbols.push(Sym { kind:"module".into(), name, line: line_of(child), container: container.clone(), is_test, ..Default::default() });
                if let Some(b) = child.child_by_field_name("body") { walk_rust(b, src, Some(full), is_test, out); }
            }
            "struct_item" | "enum_item" | "union_item" => {
                let kind = if child.kind()=="enum_item" {"enum"} else {"struct"};
                out.symbols.push(Sym { kind:kind.into(), name: field_text(child,"name",src).unwrap_or("").into(), line: line_of(child), container: container.clone(), ..Default::default() });
            }
            "trait_item" => out.symbols.push(Sym { kind:"trait".into(), name: field_text(child,"name",src).unwrap_or("").into(), line: line_of(child), container: container.clone(), ..Default::default() }),
            "impl_item" => {
                let ty = field_text(child, "type", src).unwrap_or("").to_string();
                let tr = field_text(child, "trait", src);
                let label = match tr { Some(t) => format!("{} for {}", t, ty), None => ty.clone() };
                let full = join(container.as_deref(), &ty);
                // emit impl methods as symbols under the type
                if let Some(b) = child.child_by_field_name("body") {
                    let mut c2 = b.walk();
                    for f in b.children(&mut c2) {
                        if f.kind() == "function_item" {
                            let calls = { let mut v=Vec::new(); if let Some(bd)=f.child_by_field_name("body"){collect_calls_generic(bd,src,&mut v);} dedup(v) };
                            out.symbols.push(Sym { kind:"method".into(), name: field_text(f,"name",src).unwrap_or("").into(), line: line_of(f), container: Some(full.clone()), is_test: in_test, calls, ..Default::default() });
                        }
                    }
                }
                let _ = label;
            }
            "function_item" => {
                let is_test = in_test || pending_test;
                let calls = { let mut v=Vec::new(); if let Some(bd)=child.child_by_field_name("body"){collect_calls_generic(bd,src,&mut v);} dedup(v) };
                out.symbols.push(Sym { kind:"function".into(), name: field_text(child,"name",src).unwrap_or("").into(), line: line_of(child), container: container.clone(), is_test, calls, ..Default::default() });
            }
            _ => { if child.child_count() > 0 { walk_rust(child, src, container.clone(), in_test, out); } }
        }
        pending_test = false;
    }
}

// ─────────────────────────────── Bicep ───────────────────────────────
pub fn analyze_bicep(src: &str) -> LangFile {
    let mut out = LangFile { lang: "bicep".into(), ..Default::default() };
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&tree_sitter_bicep::LANGUAGE.into()).unwrap();
    let tree = match parser.parse(src, None) { Some(t) => t, None => return out };
    let root = tree.root_node();
    let mut cur = root.walk();
    for child in root.children(&mut cur) {
        match child.kind() {
            "module_declaration" => {
                // (identifier) (string (string_content))  -> name + path
                let name = first_kind(child, "identifier").map(|n| txt(n,src).to_string()).unwrap_or_default();
                let path = first_kind(child, "string").map(|s| txt(s,src).trim_matches(|c| c=='\'').to_string());
                out.symbols.push(Sym { kind:"bicep_module".into(), name, line: line_of(child), ..Default::default() });
                if let Some(p) = path { out.imports.push(Imp { target: p, kind: "path".into() }); }
            }
            "resource_declaration" => {
                let name = first_kind(child, "identifier").map(|n| txt(n,src).to_string()).unwrap_or_default();
                out.symbols.push(Sym { kind:"bicep_resource".into(), name, line: line_of(child), ..Default::default() });
            }
            "parameter_declaration" => out.symbols.push(Sym { kind:"param".into(), name: first_kind(child,"identifier").map(|n| txt(n,src).to_string()).unwrap_or_default(), line: line_of(child), ..Default::default() }),
            "output_declaration" => out.symbols.push(Sym { kind:"output".into(), name: first_kind(child,"identifier").map(|n| txt(n,src).to_string()).unwrap_or_default(), line: line_of(child), ..Default::default() }),
            _ => {}
        }
    }
    out
}

// ─────────────────────────────── SQL (regex, dialect-agnostic) ───────────────────────────────
pub fn analyze_sql(src: &str) -> LangFile {
    use regex::Regex;
    let mut out = LangFile { lang: "sql".into(), ..Default::default() };
    let create = Regex::new(r#"(?im)^\s*CREATE\s+(?:OR\s+ALTER\s+|OR\s+REPLACE\s+)?(TABLE|VIEW|PROC(?:EDURE)?|FUNCTION|TRIGGER|INDEX|TYPE)\s+(?:IF\s+NOT\s+EXISTS\s+)?([#A-Za-z0-9_.\[\]"`]+)"#).unwrap();
    for cap in create.captures_iter(src) {
        let raw_kind = cap[1].to_uppercase();
        let kind = match raw_kind.as_str() {
            "TABLE" => "sql_table", "VIEW" => "sql_view",
            k if k.starts_with("PROC") => "sql_proc",
            "FUNCTION" => "sql_function", "TRIGGER" => "sql_trigger",
            "INDEX" => "sql_index", "TYPE" => "sql_type", _ => "sql_object",
        };
        let name = clean_sql_name(&cap[2]);
        let line = src[..cap.get(0).unwrap().start()].bytes().filter(|&b| b==b'\n').count() + 1;
        out.symbols.push(Sym { kind: kind.into(), name, line, ..Default::default() });
    }
    // referenced tables: FROM/JOIN/INTO/UPDATE <name>
    let refs = Regex::new(r#"(?i)\b(?:FROM|JOIN|INTO|UPDATE)\s+([#A-Za-z0-9_.\[\]"`]+)"#).unwrap();
    let mut seen = std::collections::HashSet::new();
    for cap in refs.captures_iter(src) {
        let name = clean_sql_name(&cap[1]);
        if !name.is_empty() && seen.insert(name.clone()) {
            out.imports.push(Imp { target: name, kind: "sql_ref".into() });
        }
    }
    out
}
fn clean_sql_name(s: &str) -> String {
    s.trim_matches(|c| c=='['||c==']'||c=='"'||c=='`').split('.').last().unwrap_or(s)
        .trim_matches(|c| c=='['||c==']'||c=='"'||c=='`').to_string()
}
// Repo-level graph builder. Mirrors the CLI's existing RepoNode/RepoEdge shape
// (schema bumped to v2) so it drops into `code_session.rs`. Walks the tree,
// dispatches each file to a language analyzer, emits nodes + contains edges,
// then resolves cross-file edges: imports(path->file), references(namespace->file),
// extends/implements(base->symbol), calls(callee->symbol), tests(test->target).


#[derive(Debug, Default, Serialize)]
pub struct RepoNode {
    pub id: String,
    pub kind: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")] pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub container: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub lang: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")] pub size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")] pub lines: Option<usize>,
    #[serde(skip_serializing_if = "std::ops::Not::not")] pub test: bool,
}
#[derive(Debug, Serialize)]
pub struct RepoEdge {
    #[serde(rename = "type")] pub etype: String,
    pub from: String,
    pub to: String,
    #[serde(skip_serializing_if = "Option::is_none")] pub rel: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub external: Option<bool>,
}
#[derive(Debug, Serialize)]
pub struct RepoMap {
    pub schema: String,
    pub root: String,
    pub stats: BTreeMap<String, usize>,
    pub languages: BTreeMap<String, usize>,
    pub nodes: Vec<RepoNode>,
    pub edges: Vec<RepoEdge>,
}

const REPO_IGNORE: &[&str] = &[".git","__pycache__","node_modules","target",".venv","venv",
    ".mypy_cache",".pytest_cache",".idea",".vscode",".spacekit-code-session","dist","build",".ruff_cache","bin","obj"];

fn lang_for(path: &str) -> &'static str {
    match Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or("") {
        "cs" => "csharp", "ts"|"mts"|"cts" => "typescript", "tsx" => "tsx",
        "js"|"jsx"|"mjs"|"cjs" => "javascript", "rs" => "rust",
        "sql" => "sql", "bicep" => "bicep",
        "yml"|"yaml" => "yaml", "csproj"|"props"|"targets" => "msbuild",
        "py" => "python", "json" => "json", "md" => "markdown", "toml" => "toml",
        _ => "other",
    }
}
fn is_test_path(rel: &str) -> bool {
    let l = rel.to_lowercase();
    l.contains(".test.") || l.contains(".spec.") || l.contains("test/") || l.contains("tests/")
        || l.contains(".tests") || l.ends_with("_test.rs") || l.contains("__tests__")
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<(String,bool,PathBuf)>) {
    let mut entries: Vec<_> = match std::fs::read_dir(dir) { Ok(r)=>r.filter_map(|e| e.ok()).collect(), Err(_)=>return };
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || REPO_IGNORE.contains(&name.as_str()) { continue; }
        let abs = e.path();
        let is_dir = abs.is_dir();
        let rel = abs.strip_prefix(root).unwrap_or(&abs).to_string_lossy().replace('\\',"/");
        out.push((rel, is_dir, abs.clone()));
        if is_dir { collect(root, &abs, out); }
    }
}
fn parent_dir_id(rel: &str) -> String {
    match Path::new(rel).parent().map(|p| p.to_string_lossy().replace('\\',"/")) {
        Some(p) if !p.is_empty() => format!("dir:{}", p),
        _ => "dir:.".into(),
    }
}
fn sym_id(rel: &str, container: &Option<String>, name: &str) -> String {
    match container { Some(c) if !c.is_empty() => format!("sym:{}::{}.{}", rel, c, name), _ => format!("sym:{}::{}", rel, name) }
}

pub fn build(root: &Path) -> RepoMap {
    let root_abs = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    let mut languages: BTreeMap<String, usize> = BTreeMap::new();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    nodes.push(RepoNode{ id:"dir:.".into(), kind:"dir".into(), name: root_abs.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or(".".into()), path: Some(".".into()), ..Default::default() });

    let mut entries = Vec::new();
    collect(&root_abs, &root_abs, &mut entries);

    // repo-wide indices for resolution
    let mut file_set: HashSet<String> = HashSet::new();
    let mut ns_to_files: HashMap<String, Vec<String>> = HashMap::new();  // C# namespace -> files
    let mut name_to_ids: HashMap<String, Vec<String>> = HashMap::new();  // symbol simple name -> ids
    let mut analyzed: Vec<(String, String, LangFile)> = Vec::new();      // (rel, lang, analysis)
    let mut py_files: Vec<(String, String)> = Vec::new();

    for (rel, is_dir, abs) in &entries {
        if *is_dir {
            *counts.entry("dirs".into()).or_default() += 1;
            nodes.push(RepoNode{ id: format!("dir:{}", rel), kind:"dir".into(), name: Path::new(rel).file_name().unwrap().to_string_lossy().into(), path: Some(rel.clone()), ..Default::default() });
            edges.push(RepoEdge{ etype:"contains".into(), from: parent_dir_id(rel), to: format!("dir:{}", rel), rel:None, external:None });
            continue;
        }
        *counts.entry("files".into()).or_default() += 1;
        file_set.insert(rel.clone());
        let lang = lang_for(rel);
        *languages.entry(lang.into()).or_default() += 1;
        let bytes = std::fs::read(abs).unwrap_or_default();
        let text = String::from_utf8(bytes.clone()).ok();
        nodes.push(RepoNode{ id: format!("file:{}", rel), kind:"file".into(), name: Path::new(rel).file_name().unwrap().to_string_lossy().into(), path: Some(rel.clone()), lang: Some(lang.into()), size: Some(bytes.len() as u64), lines: text.as_ref().map(|t| t.lines().count()), test: is_test_path(rel), ..Default::default() });
        edges.push(RepoEdge{ etype:"contains".into(), from: parent_dir_id(rel), to: format!("file:{}", rel), rel:None, external:None });

        let Some(src) = text else { continue };
        if bytes.len() > 2_000_000 { continue; }
        let lf = match lang {
            "csharp" => analyze_csharp(&src),
            "typescript" => analyze_ts(&src, false),
            "tsx" => analyze_ts(&src, true),
            "rust" => analyze_rust(&src),
            "bicep" => analyze_bicep(&src),
            "sql" => analyze_sql(&src),
            "python" => { py_files.push((rel.clone(), src)); continue; }
            _ => continue,
        };
        for ns in &lf.namespaces { ns_to_files.entry(ns.clone()).or_default().push(rel.clone()); }
        analyzed.push((rel.clone(), lang.to_string(), lf));
    }

    // Python is delegated to the existing analyzer (preserves prior behavior).
    for (rel, lf) in python_lang_files(&py_files, &file_set) {
        for ns in &lf.namespaces { ns_to_files.entry(ns.clone()).or_default().push(rel.clone()); }
        analyzed.push((rel, "python".to_string(), lf));
    }

    // pass 1: emit symbol nodes + contains edges, build name index
    let file_test: HashSet<String> = entries.iter().filter(|(r,d,_)| !d && is_test_path(r)).map(|(r,_,_)| r.clone()).collect();
    let mut sym_count = 0usize;
    for (rel, _lang, lf) in &analyzed {
        let file_id = format!("file:{}", rel);
        for s in &lf.symbols {
            let id = sym_id(rel, &s.container, &s.name);
            let container_id = match &s.container {
                Some(c) if !c.is_empty() => {
                    // container path's last segment is the parent symbol; link to it if present else to file
                    format!("sym:{}::{}", rel, c)
                }
                _ => file_id.clone(),
            };
            nodes.push(RepoNode{ id: id.clone(), kind: s.kind.clone(), name: s.name.clone(), file: Some(rel.clone()), container: s.container.clone(), line: Some(s.line), test: s.is_test || file_test.contains(rel), ..Default::default() });
            edges.push(RepoEdge{ etype:"contains".into(), from: container_id, to: id.clone(), rel:None, external:None });
            name_to_ids.entry(s.name.clone()).or_default().push(id.clone());
            sym_count += 1;
        }
    }
    counts.insert("symbols".into(), sym_count);

    // pass 2: resolve edges
    for (rel, lang, lf) in &analyzed {
        let file_id = format!("file:{}", rel);
        // imports
        for imp in &lf.imports {
            match imp.kind.as_str() {
                "path" => {
                    if let Some(t) = resolve_path_import(rel, &imp.target, &file_set) {
                        edges.push(RepoEdge{ etype:"imports".into(), from: file_id.clone(), to: format!("file:{}", t), rel: Some("path".into()), external: Some(false) });
                    } else {
                        edges.push(RepoEdge{ etype:"imports".into(), from: file_id.clone(), to: format!("module:{}", imp.target), rel: Some("path".into()), external: Some(true) });
                    }
                }
                "namespace" => {
                    // C# using -> files declaring that namespace (references)
                    if let Some(files) = ns_to_files.get(&imp.target) {
                        for f in files { if f != rel { edges.push(RepoEdge{ etype:"references".into(), from: file_id.clone(), to: format!("file:{}", f), rel: Some(format!("using {}", imp.target)), external: Some(false) }); } }
                    } else {
                        edges.push(RepoEdge{ etype:"references".into(), from: file_id.clone(), to: format!("module:{}", imp.target), rel: Some("using".into()), external: Some(true) });
                    }
                }
                "sql_ref" => {
                    // link to a table/view symbol of that name if unique
                    let cands: Vec<&String> = name_to_ids.get(&imp.target).map(|v| v.iter().filter(|id| id.contains("::")).collect()).unwrap_or_default();
                    if let Some(id) = cands.first() { edges.push(RepoEdge{ etype:"references".into(), from: file_id.clone(), to: (*id).clone(), rel: Some("sql_ref".into()), external: Some(false) }); }
                }
                _ => {}
            }
        }
        // extends/implements + calls (resolved-only, unique simple name)
        for s in &lf.symbols {
            let sid = sym_id(rel, &s.container, &s.name);
            for base in &s.bases {
                let simple = base.split('.').last().unwrap_or(base).split('<').next().unwrap_or(base).to_string();
                if let Some(ids) = name_to_ids.get(&simple) { if ids.len()==1 && ids[0]!=sid { edges.push(RepoEdge{ etype:"extends".into(), from: sid.clone(), to: ids[0].clone(), rel:None, external:None }); } }
            }
            for callee in &s.calls {
                if let Some(ids) = name_to_ids.get(callee) { if ids.len()==1 && ids[0]!=sid { edges.push(RepoEdge{ etype:"calls".into(), from: sid.clone(), to: ids[0].clone(), rel:None, external:None }); } }
            }
        }
        let _ = lang;
    }

    // pass 3: tests -> target by file-name heuristic (Foo.Tests/Foo.test.ts/foo_test.rs -> Foo)
    for (rel, _l, _lf) in &analyzed {
        if !is_test_path(rel) { continue; }
        let stem = Path::new(rel).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let base = stem.replace(".Tests","").replace(".Test","").replace(".test","").replace(".spec","").replace("Tests","").trim_end_matches("_test").to_string();
        if base.len() < 2 { continue; }
        // find a non-test file whose stem matches base
        if let Some((trel,_,_)) = entries.iter().find(|(r,d,_)| !d && !is_test_path(r) && Path::new(r).file_stem().map(|s| s.to_string_lossy()==base).unwrap_or(false)) {
            edges.push(RepoEdge{ etype:"tests".into(), from: format!("file:{}", rel), to: format!("file:{}", trel), rel: Some("by-name".into()), external: Some(false) });
        }
    }

    counts.insert("edges".into(), edges.len());
    RepoMap{ schema:"spacekit:repo-map:v2".into(), root: root_abs.to_string_lossy().into(), stats: counts, languages, nodes, edges }
}

fn resolve_path_import(from_rel: &str, target: &str, files: &HashSet<String>) -> Option<String> {
    let base = Path::new(from_rel).parent().unwrap_or(Path::new("")).to_path_buf();
    let joined = normalize(&base.join(target));
    // try direct, then common extensions / index files
    let cands = [
        joined.clone(),
        format!("{}.ts", joined), format!("{}.tsx", joined), format!("{}.d.ts", joined),
        format!("{}.js", joined), format!("{}.bicep", joined),
        format!("{}/index.ts", joined), format!("{}/index.tsx", joined),
    ];
    cands.into_iter().find(|c| files.contains(c))
}
fn normalize(p: &Path) -> String {
    let mut parts: Vec<String> = Vec::new();
    for comp in p.components() {
        use std::path::Component::*;
        match comp { ParentDir => { parts.pop(); }, CurDir => {}, Normal(s) => parts.push(s.to_string_lossy().into()), _ => {} }
    }
    parts.join("/")
}
// Context pack: given a repo map + a task query and/or seed files, expand the
// graph to the relevant neighborhood, select files under a token budget, and
// emit a model-ready bundle (structure outline + full contents of selected files).


pub struct PackArgs {
    pub query: Option<String>,
    pub seeds: Vec<String>,   // file paths (rel or containing)
    pub hops: usize,
    pub budget_tokens: usize, // approx; bytes ~= tokens*4
}

fn approx_tokens(bytes: usize) -> usize { bytes / 4 + 1 }

pub fn pack(root: &Path, map: &RepoMap, args: &PackArgs) -> String {
    // index nodes
    let node_by_id: HashMap<&str, &RepoNode> = map.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    // file adjacency (undirected) over meaningful edges; symbols collapse to their file
    let sym_file: HashMap<&str, String> = map.nodes.iter()
        .filter(|n| n.kind!="dir" && n.kind!="file")
        .filter_map(|n| n.file.as_ref().map(|f| (n.id.as_str(), format!("file:{}", f)))).collect();
    let to_file = |id: &str| -> Option<String> {
        if id.starts_with("file:") { Some(id.to_string()) }
        else if id.starts_with("sym:") { sym_file.get(id).cloned() }
        else { None } // module: external -> ignore
    };
    let mut adj: HashMap<String, HashSet<String>> = HashMap::new();
    for e in &map.edges {
        if e.etype == "contains" { continue; }
        if let (Some(a), Some(b)) = (to_file(&e.from), to_file(&e.to)) {
            if a!=b { adj.entry(a.clone()).or_default().insert(b.clone()); adj.entry(b).or_default().insert(a); }
        }
    }

    // seeds
    let mut seed_files: HashSet<String> = HashSet::new();
    for s in &args.seeds {
        for n in &map.nodes { if n.kind=="file" { if let Some(p)=&n.path { if p==s || p.ends_with(s) || p.contains(s) { seed_files.insert(n.id.clone()); } } } }
    }
    let mut query_hits: HashMap<String, usize> = HashMap::new();
    if let Some(q) = &args.query {
        let toks: Vec<String> = q.to_lowercase().split(|c:char| !c.is_alphanumeric()).filter(|t| t.len()>=3).map(|s| s.to_string()).collect();
        for n in &map.nodes {
            if n.kind=="dir" { continue; }
            let hay = format!("{} {} {}", n.name, n.path.clone().unwrap_or_default(), n.container.clone().unwrap_or_default()).to_lowercase();
            let hits = toks.iter().filter(|t| hay.contains(*t)).count();
            if hits>0 { if let Some(f)=to_file(&n.id) { *query_hits.entry(f.clone()).or_default() += hits; seed_files.insert(f); } }
        }
    }
    if seed_files.is_empty() {
        // no seeds/query: seed with all files (importance-ranked later)
        for n in &map.nodes { if n.kind=="file" { seed_files.insert(n.id.clone()); } }
    }

    // BFS hop distances
    let mut dist: HashMap<String, usize> = HashMap::new();
    let mut q: VecDeque<String> = VecDeque::new();
    for s in &seed_files { dist.insert(s.clone(), 0); q.push_back(s.clone()); }
    while let Some(cur) = q.pop_front() {
        let d = dist[&cur];
        if d >= args.hops { continue; }
        if let Some(ns) = adj.get(&cur) { for nb in ns { if !dist.contains_key(nb) { dist.insert(nb.clone(), d+1); q.push_back(nb.clone()); } } }
    }

    // symbols per file (for outline)
    let mut file_syms: BTreeMap<String, Vec<&RepoNode>> = BTreeMap::new();
    for n in &map.nodes { if n.kind!="dir" && n.kind!="file" { if let Some(f)=&n.file { file_syms.entry(format!("file:{}", f)).or_default().push(n); } } }

    // rank candidate files
    let mut cands: Vec<(String, usize, usize, u64)> = dist.iter().map(|(id,&d)| {
        let qh = *query_hits.get(id).unwrap_or(&0);
        let size = node_by_id.get(id.as_str()).and_then(|n| n.size).unwrap_or(0);
        (id.clone(), d, qh, size)
    }).collect();
    // sort: closer first, then more query hits, then smaller
    cands.sort_by(|a,b| a.1.cmp(&b.1).then(b.2.cmp(&a.2)).then(a.3.cmp(&b.3)));

    // select under budget
    let mut selected: Vec<String> = Vec::new();
    let mut omitted: Vec<String> = Vec::new();
    let mut used = 0usize;
    for (id,_,_,size) in &cands {
        let t = approx_tokens(*size as usize);
        if used + t <= args.budget_tokens { selected.push(id.clone()); used += t; }
        else { omitted.push(id.clone()); }
    }

    render(root, map, &node_by_id, &file_syms, &selected, &omitted, &dist, &query_hits, args, used)
}

fn lang_fence(lang: &str) -> &str {
    match lang { "csharp"=>"csharp","typescript"|"tsx"=>"typescript","rust"=>"rust","sql"=>"sql","bicep"=>"bicep","python"=>"python","yaml"=>"yaml", _=>"" }
}

#[allow(clippy::too_many_arguments)]
fn render(root: &Path, map: &RepoMap, node_by_id: &HashMap<&str,&RepoNode>, file_syms: &BTreeMap<String,Vec<&RepoNode>>,
          selected: &[String], omitted: &[String], dist: &HashMap<String,usize>, qhits: &HashMap<String,usize>, args: &PackArgs, used: usize) -> String {
    let mut o = String::new();
    o.push_str(&format!("# Context pack — {}\n\n", map.root));
    if let Some(q)=&args.query { o.push_str(&format!("**Task:** {}\n\n", q)); }
    o.push_str(&format!("**Stats:** {} files, {} symbols, {} edges. Selected {} files (~{} tokens, budget {}).\n\n",
        map.stats.get("files").unwrap_or(&0), map.stats.get("symbols").unwrap_or(&0), map.stats.get("edges").unwrap_or(&0), selected.len(), used, args.budget_tokens));

    // structure outline (all files, compact)
    o.push_str("## Project structure\n\n```\n");
    let mut files: Vec<&&RepoNode> = node_by_id.values().filter(|n| n.kind=="file").collect();
    files.sort_by(|a,b| a.path.cmp(&b.path));
    for n in files {
        let id = &n.id;
        let mark = if selected.contains(id) {"*"} else if dist.contains_key(id) {"+"} else {" "};
        let syms = file_syms.get(id).map(|v| {
            let mut names: Vec<String> = v.iter().filter(|s| matches!(s.kind.as_str(),"class"|"interface"|"struct"|"enum"|"trait"|"function"|"record"|"sql_table"|"sql_proc"|"sql_view"|"bicep_resource"|"bicep_module"|"module")).map(|s| format!("{} {}", s.kind, s.name)).collect();
            names.truncate(8); names.join(", ")
        }).unwrap_or_default();
        o.push_str(&format!("{} {:<40} {}\n", mark, n.path.clone().unwrap_or_default(), syms));
    }
    o.push_str("```\n(* = included below, + = related/in-scope)\n\n");

    // relationships summary (non-contains edges among selected/in-scope)
    o.push_str("## Key relationships\n\n");
    let mut rels = 0;
    for e in &map.edges {
        if matches!(e.etype.as_str(), "imports"|"references"|"calls"|"extends"|"tests") && e.external!=Some(true) {
            o.push_str(&format!("- {} `{}` → `{}`{}\n", e.etype, e.from.trim_start_matches("file:").trim_start_matches("sym:"), e.to.trim_start_matches("file:").trim_start_matches("sym:"), e.rel.as_ref().map(|r| format!(" ({})", r)).unwrap_or_default()));
            rels+=1; if rels>=60 { o.push_str("- …\n"); break; }
        }
    }
    o.push('\n');

    // file contents
    o.push_str("## Files\n\n");
    for id in selected {
        let n = match node_by_id.get(id.as_str()) { Some(n)=>n, None=>continue };
        let rel = n.path.clone().unwrap_or_default();
        let abs = root.join(&rel);
        let body = std::fs::read_to_string(&abs).unwrap_or_default();
        let why = if qhits.get(id).copied().unwrap_or(0)>0 {"query match"} else if dist.get(id)==Some(&0) {"seed"} else {"related"};
        o.push_str(&format!("### {}  ({}, {})\n\n```{}\n{}\n```\n\n", rel, n.lang.clone().unwrap_or_default(), why, lang_fence(n.lang.as_deref().unwrap_or("")), body.trim_end()));
    }
    if !omitted.is_empty() {
        o.push_str("## Related but omitted (over budget)\n\n");
        for id in omitted { if let Some(n)=node_by_id.get(id.as_str()) { o.push_str(&format!("- {}\n", n.path.clone().unwrap_or_default())); } }
    }
    o
}

// ─────────────────────────── CLI entry points ───────────────────────────
pub fn handle_map(root: &Path, out: &Option<PathBuf>) -> Result<(), Box<dyn Error>> {
    if !root.is_dir() { return Err(format!("not a directory: {}", root.display()).into()); }
    let map = build(root);
    let json = serde_json::to_string_pretty(&map)?;
    let out = out.clone().unwrap_or_else(|| {
        let name = root.canonicalize().ok().and_then(|p| p.file_name().map(|s| s.to_string_lossy().to_string())).unwrap_or_else(|| "repo".into());
        PathBuf::from(format!("{}.repo.json", name))
    });
    if let Some(parent) = out.parent() { if !parent.as_os_str().is_empty() { std::fs::create_dir_all(parent)?; } }
    std::fs::write(&out, &json)?;
    println!("repo-map v2 -> {} files, {} symbols, {} edges -> {}",
        map.stats.get("files").unwrap_or(&0), map.stats.get("symbols").unwrap_or(&0), map.stats.get("edges").unwrap_or(&0), out.display());
    let langs: Vec<String> = map.languages.iter().map(|(k,v)| format!("{}:{}", k, v)).collect();
    println!("languages: {}", langs.join(", "));
    Ok(())
}

pub fn handle_pack(root: &Path, query: Option<String>, seeds: Vec<String>, hops: usize, budget: usize, out: &Option<PathBuf>) -> Result<(), Box<dyn Error>> {
    if !root.is_dir() { return Err(format!("not a directory: {}", root.display()).into()); }
    let map = build(root);
    let args = PackArgs { query, seeds, hops, budget_tokens: budget };
    let bundle = pack(root, &map, &args);
    match out {
        Some(p) => { std::fs::write(p, &bundle)?; println!("context-pack -> {}", p.display()); }
        None => println!("{}", bundle),
    }
    Ok(())
}
