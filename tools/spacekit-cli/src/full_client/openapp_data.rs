//! OpenApp data-layer emitter — `entities` -> Prisma schema (+ migration scaffold).
//!
//! `relations: referenced` (the default) maps `hasMany`/`belongsTo` onto foreign
//! keys; `embedded` is reserved for document stores (not yet emitted). Missing
//! back-relations are synthesized so the schema is self-consistent.

use std::collections::BTreeMap;

use super::{
    camel, is_storage_node, kebab, pluralize, AppModel, Entity, FieldDef, Profile, RelKind,
    RelationDef,
};

pub(super) fn emit_data(m: &AppModel, p: &Profile) -> Vec<(String, String)> {
    let store = p.data.get("store", "postgres").to_string();
    if is_storage_node(&store) {
        return emit_storage_node(m, p);
    }
    emit_prisma(m, p)
}

// ---------------------------------------------------------------------------
// Prisma (relational) backend
// ---------------------------------------------------------------------------

fn emit_prisma(m: &AppModel, p: &Profile) -> Vec<(String, String)> {
    let orm = p.data.get("orm", "prisma").to_string();
    let mut out = Vec::new();
    // The data-access handle (shared by business + view layers).
    out.push(("server/db.ts".to_string(), prisma_db_ts()));
    if orm == "none" {
        return out;
    }
    out.push(("prisma/schema.prisma".to_string(), prisma_schema(m, p)));
    out.push((".env.example".to_string(), env_example(p)));
    if p.data.get("migrations", "true") == "true" {
        out.push(("prisma/README.md".to_string(), migrations_readme(p)));
    }
    out
}

fn prisma_db_ts() -> String {
    "// Prisma client singleton (data-layer access for server actions).\n\
     import { PrismaClient } from \"@prisma/client\";\n\n\
     declare global {\n  // eslint-disable-next-line no-var\n  var prisma: PrismaClient | undefined;\n}\n\n\
     export const db = global.prisma ?? new PrismaClient();\n\
     if (process.env.NODE_ENV !== \"production\") global.prisma = db;\n"
        .to_string()
}

// ---------------------------------------------------------------------------
// spacekit-storage-node (DID-scoped document store) backend
// ---------------------------------------------------------------------------

/// Collection name for an entity in the storage node (kebab-plural).
fn collection_name(entity: &str) -> String {
    kebab(&pluralize(entity))
}

fn emit_storage_node(m: &AppModel, p: &Profile) -> Vec<(String, String)> {
    let _ = p;
    let mut out = Vec::new();
    out.push(("server/storage-client.ts".to_string(), storage_client_ts()));
    out.push(("server/db.ts".to_string(), storage_db_ts(m)));
    out.push((".env.example".to_string(), storage_env_example()));
    out.push(("server/collections.md".to_string(), collections_md(m)));
    out
}

/// Low-level HTTP client for the storage node document API
/// (`/api/documents/{collection}/{id}`, `POST /query/documents/{collection}`).
fn storage_client_ts() -> String {
    r#"// Generated low-level client for spacekit-storage-node (document API).
// Docs: PUT/GET/DELETE /api/documents/{collection}/{id}, POST /query/documents/{collection}.

const BASE_URL = process.env.SPACEKIT_STORAGE_URL ?? "http://127.0.0.1:3030";
const OWNER_DID = process.env.SPACEKIT_DID ?? "did:spacekit:user:local";

function enc(seg: string): string {
  return encodeURIComponent(seg);
}

async function req(method: string, path: string, body?: unknown): Promise<Response> {
  const res = await fetch(`${BASE_URL}${path}`, {
    method,
    headers: {
      Authorization: `DID ${OWNER_DID}`,
      "Content-Type": "application/json",
    },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (!res.ok && res.status !== 404) {
    throw new Error(`storage-node ${method} ${path} -> ${res.status}`);
  }
  return res;
}

export type Filter = { path: string; op: "Equals" | "Contains" | "In" | "GreaterThanOrEqual" | "LessThan"; value: unknown };
export type Query = { filters?: Filter[]; limit?: number; offset?: number; sort_by?: { field: string; order: "Asc" | "Desc" } };

export async function getDoc<T>(collection: string, id: string): Promise<T | null> {
  const res = await req("GET", `/api/documents/${enc(collection)}/${enc(id)}`);
  if (res.status === 404) return null;
  const json = await res.json();
  return (json?.document?.data ?? null) as T | null;
}

export async function putDoc<T>(collection: string, id: string, data: T): Promise<T> {
  const res = await req("PUT", `/api/documents/${enc(collection)}/${enc(id)}`, data);
  const json = await res.json();
  return (json?.document?.data ?? data) as T;
}

export async function listDocs<T>(collection: string): Promise<T[]> {
  const res = await req("GET", `/api/documents/${enc(collection)}`);
  const json = await res.json();
  return ((json?.documents ?? []) as Array<{ data: T | null }>)
    .map((d) => d.data)
    .filter((d): d is T => d !== null);
}

export async function deleteDoc(collection: string, id: string): Promise<void> {
  await req("DELETE", `/api/documents/${enc(collection)}/${enc(id)}`);
}

export async function queryDocs<T>(collection: string, query: Query): Promise<T[]> {
  const res = await req("POST", `/query/documents/${enc(collection)}`, query);
  const json = await res.json();
  return ((json?.documents ?? []) as Array<{ data: T | null }>)
    .map((d) => d.data)
    .filter((d): d is T => d !== null);
}
"#
    .to_string()
}

/// A Prisma-shaped `db` over the storage node, so business + view code is
/// identical regardless of the chosen store (keeps conformance intact).
fn storage_db_ts(m: &AppModel) -> String {
    let mut s = String::from(
        "// Data-access handle backed by spacekit-storage-node.\n\
         // Exposes a Prisma-like surface so generated business/view code is store-agnostic.\n\
         import { randomUUID } from \"crypto\";\n\
         import { getDoc, putDoc, listDocs, deleteDoc, queryDocs } from \"./storage-client\";\n",
    );
    // Import entity types.
    if !m.entities.is_empty() {
        let names: Vec<String> = m.entities.iter().map(|e| e.name.clone()).collect();
        s.push_str(&format!(
            "import type {{ {} }} from \"./types\";\n\n",
            names.join(", ")
        ));
    } else {
        s.push('\n');
    }

    s.push_str(
        "type WhereInput = Record<string, unknown>;\n\n\
         function toFilters(where: WhereInput) {\n  \
         return Object.entries(where).map(([path, value]) => ({ path, op: \"Equals\" as const, value }));\n}\n\n\
         function repo<T extends { id?: string }>(collection: string) {\n  \
         return {\n    \
         async findUnique(args: { where: { id: string } }): Promise<T | null> {\n      \
         return getDoc<T>(collection, String(args.where.id));\n    },\n    \
         async findFirst(args?: { where?: WhereInput }): Promise<T | null> {\n      \
         const where = args?.where ?? {};\n      \
         if (where.id) return getDoc<T>(collection, String(where.id));\n      \
         const rows = await queryDocs<T>(collection, { filters: toFilters(where), limit: 1 });\n      \
         return rows[0] ?? null;\n    },\n    \
         async findMany(args?: { where?: WhereInput }): Promise<T[]> {\n      \
         const where = args?.where ?? {};\n      \
         if (Object.keys(where).length === 0) return listDocs<T>(collection);\n      \
         return queryDocs<T>(collection, { filters: toFilters(where) });\n    },\n    \
         async create(args: { data: T }): Promise<T> {\n      \
         const id = (args.data.id as string | undefined) ?? randomUUID();\n      \
         return putDoc<T>(collection, id, { ...args.data, id });\n    },\n    \
         async update(args: { where: { id: string }; data: Partial<T> }): Promise<T> {\n      \
         const current = (await getDoc<T>(collection, args.where.id)) ?? ({} as T);\n      \
         return putDoc<T>(collection, args.where.id, { ...current, ...args.data, id: args.where.id });\n    },\n    \
         async delete(args: { where: { id: string } }): Promise<void> {\n      \
         await deleteDoc(collection, args.where.id);\n    },\n  };\n}\n\n",
    );

    s.push_str("export const db = {\n");
    for e in &m.entities {
        s.push_str(&format!(
            "  {}: repo<{}>(\"{}\"),\n",
            camel(&e.name),
            e.name,
            collection_name(&e.name),
        ));
    }
    s.push_str("};\n");
    s
}

fn storage_env_example() -> String {
    "# spacekit-storage-node data backend\n\
     SPACEKIT_STORAGE_URL=\"http://127.0.0.1:3030\"\n\
     # Owner DID — every document is scoped to this identity.\n\
     SPACEKIT_DID=\"did:spacekit:user:local\"\n"
        .to_string()
}

fn collections_md(m: &AppModel) -> String {
    let mut s = String::from(
        "# Storage-node collections\n\n\
         Each entity maps to a DID-scoped collection in spacekit-storage-node. \
         Documents are addressed as `/api/documents/{collection}/{id}` and scoped to \
         `SPACEKIT_DID`. Storage is schemaless; relations are stored as id fields and \
         resolved with `POST /query/documents/{collection}`.\n\n\
         | Entity | Collection | Identity |\n|---|---|---|\n",
    );
    for e in &m.entities {
        s.push_str(&format!(
            "| `{}` | `{}` | `{}` |\n",
            e.name,
            collection_name(&e.name),
            e.identity
        ));
    }
    s.push_str(
        "\n> No relational migrations: the storage node creates collections on first write. \
         Filtered reads use the document query DSL (`Equals`, `Contains`, `In`, \
         `GreaterThanOrEqual`, `LessThan`).\n",
    );
    s
}

/// The Prisma scalar + id default for the profile's `identity` choice.
fn id_realization(identity: &str) -> (&'static str, &'static str) {
    match identity {
        "serial" => ("Int", "@default(autoincrement())"),
        "cuid" => ("String", "@default(cuid())"),
        "objectid" => ("String", "@default(auto()) @map(\"_id\") @db.ObjectId"),
        _ => ("String", "@default(uuid())"), // uuid
    }
}

fn prisma_scalar(ty: &str) -> &'static str {
    match ty {
        "id" | "text" | "longtext" | "email" | "url" | "phone" => "String",
        "integer" => "Int",
        "decimal" | "money" => "Decimal",
        "boolean" => "Boolean",
        "timestamp" | "date" | "time" => "DateTime",
        "json" => "Json",
        "file" | "image" => "String",
        _ => "String",
    }
}

/// A synthesized or declared back-relation to add to a target entity.
#[derive(Clone)]
struct BackRel {
    field: String,
    target: String,
    list: bool,
    fk: Option<String>, // present when this side carries the foreign key
}

fn prisma_schema(m: &AppModel, p: &Profile) -> String {
    let store = p.data.get("store", "postgres");
    let provider = match store {
        "mysql" => "mysql",
        "sqlite" => "sqlite",
        _ => "postgresql",
    };
    let identity = p.data.get("identity", "uuid").to_string();
    let (id_scalar, id_default) = id_realization(&identity);
    let snake = p.data.get("naming", "camelCase") == "snake_case";

    // First pass: figure out synthesized back-relations per entity.
    let mut extra: BTreeMap<String, Vec<BackRel>> = BTreeMap::new();
    for e in &m.entities {
        for r in &e.relations {
            let target_has_back = m
                .entity(&r.target)
                .map(|t| t.relations.iter().any(|tr| tr.target == e.name))
                .unwrap_or(false);
            if target_has_back {
                continue; // both sides declared
            }
            match r.kind {
                RelKind::BelongsTo | RelKind::HasOne => {
                    // The target gains a list back-reference.
                    extra.entry(r.target.clone()).or_default().push(BackRel {
                        field: camel(&format!("{}s", e.name)),
                        target: e.name.clone(),
                        list: true,
                        fk: None,
                    });
                }
                RelKind::HasMany => {
                    // The target gains a singular back-reference holding the FK.
                    extra.entry(r.target.clone()).or_default().push(BackRel {
                        field: camel(&e.name),
                        target: e.name.clone(),
                        list: false,
                        fk: Some(camel(&format!("{}Id", e.name))),
                    });
                }
                RelKind::ManyToMany => {
                    extra.entry(r.target.clone()).or_default().push(BackRel {
                        field: camel(&format!("{}s", e.name)),
                        target: e.name.clone(),
                        list: true,
                        fk: None,
                    });
                }
            }
        }
    }

    let mut s = String::new();
    s.push_str("// Generated by `spacekit agent app` — OpenApp data layer.\n");
    s.push_str("// Do not edit by hand; re-run generation (hand-edits are detected).\n\n");
    s.push_str(&format!(
        "datasource db {{\n  provider = \"{provider}\"\n  url      = env(\"DATABASE_URL\")\n}}\n\n"
    ));
    s.push_str("generator client {\n  provider = \"prisma-client-js\"\n}\n\n");

    // Enums (collected from enum fields).
    for e in &m.entities {
        for f in &e.fields {
            if f.ty == "enum" && !f.values.is_empty() {
                s.push_str(&format!("enum {} {{\n", enum_name(&e.name, &f.name)));
                for v in &f.values {
                    s.push_str(&format!("  {}\n", v));
                }
                s.push_str("}\n\n");
            }
        }
    }

    for e in &m.entities {
        s.push_str(&format!("model {} {{\n", e.name));
        let mut index_lines: Vec<String> = Vec::new();

        for f in &e.fields {
            s.push_str(&render_field(e, f, &identity, id_scalar, id_default, snake));
            if f.indexed && f.name != e.identity {
                index_lines.push(format!("  @@index([{}])", camel(&f.name)));
            }
        }

        // Declared relations.
        for r in &e.relations {
            s.push_str(&render_relation(e, r, id_scalar));
        }
        // Synthesized back-relations.
        if let Some(backs) = extra.get(&e.name) {
            for b in backs {
                s.push_str(&render_backrel(b, id_scalar));
            }
        }

        if snake {
            s.push_str(&format!("\n  @@map(\"{}\")\n", to_snake(&e.name)));
        }
        for il in index_lines {
            s.push_str(&il);
            s.push('\n');
        }
        s.push_str("}\n\n");
    }

    s
}

fn render_field(
    e: &Entity,
    f: &FieldDef,
    identity: &str,
    id_scalar: &str,
    id_default: &str,
    snake: bool,
) -> String {
    let mut line = format!("  {}", camel(&f.name));
    if f.name == e.identity {
        let _ = identity;
        line.push_str(&format!(" {id_scalar} @id {id_default}"));
        if snake {
            line.push_str(&format!(" @map(\"{}\")", to_snake(&f.name)));
        }
        line.push('\n');
        return line;
    }

    let ty = if f.ty == "enum" && !f.values.is_empty() {
        enum_name(&e.name, &f.name)
    } else {
        prisma_scalar(&f.ty).to_string()
    };
    let optional = if f.required { "" } else { "?" };
    line.push_str(&format!(" {ty}{optional}"));

    if f.unique {
        line.push_str(" @unique");
    }
    if let Some(def) = prisma_default(f) {
        line.push_str(&format!(" {def}"));
    }
    if snake {
        line.push_str(&format!(" @map(\"{}\")", to_snake(&f.name)));
    }
    line.push('\n');
    line
}

fn prisma_default(f: &FieldDef) -> Option<String> {
    if f.generated {
        return match f.ty.as_str() {
            "timestamp" | "date" | "time" => Some("@default(now())".to_string()),
            _ => None,
        };
    }
    let d = f.default.as_ref()?;
    let rendered = match d {
        serde_json::Value::String(s) => {
            if f.ty == "enum" {
                s.clone()
            } else {
                format!("\"{}\"", s)
            }
        }
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        _ => return None,
    };
    Some(format!("@default({rendered})"))
}

fn render_relation(e: &Entity, r: &RelationDef, id_scalar: &str) -> String {
    let _ = e;
    let field = camel(&r.name);
    match r.kind {
        RelKind::BelongsTo | RelKind::HasOne => {
            let fk = camel(&format!("{}Id", r.name));
            format!(
                "  {field} {target}? @relation(fields: [{fk}], references: [id])\n  {fk} {id_scalar}?\n",
                target = r.target,
            )
        }
        RelKind::HasMany | RelKind::ManyToMany => {
            format!("  {field} {target}[]\n", target = r.target)
        }
    }
}

fn render_backrel(b: &BackRel, id_scalar: &str) -> String {
    if b.list {
        format!("  {} {}[]\n", b.field, b.target)
    } else if let Some(fk) = &b.fk {
        format!(
            "  {field} {target}? @relation(fields: [{fk}], references: [id])\n  {fk} {id_scalar}?\n",
            field = b.field,
            target = b.target,
        )
    } else {
        format!("  {} {}?\n", b.field, b.target)
    }
}

fn enum_name(entity: &str, field: &str) -> String {
    format!("{}{}", entity, super::pascal(field))
}

fn to_snake(s: &str) -> String {
    super::snake(s)
}

fn env_example(p: &Profile) -> String {
    let url = match p.data.get("store", "postgres") {
        "mysql" => "mysql://user:pass@localhost:3306/app",
        "sqlite" => "file:./dev.db",
        _ => "postgresql://user:pass@localhost:5432/app?schema=public",
    };
    format!("# Data layer connection (Prisma reads DATABASE_URL)\nDATABASE_URL=\"{url}\"\n")
}

fn migrations_readme(p: &Profile) -> String {
    format!(
        "# Data layer\n\n\
        Schema: `schema.prisma` (store: **{store}**, identity: **{identity}**).\n\n\
        ```bash\n\
        npm i -D prisma && npm i @prisma/client\n\
        npx prisma migrate dev --name init   # create + apply migration\n\
        npx prisma generate                  # regenerate the client\n\
        ```\n\n\
        The schema is regenerated from the OpenApp `entities`. Re-running \
        `spacekit agent app` detects hand-edits and preserves them unless `--force`.\n",
        store = p.data.get("store", "postgres"),
        identity = p.data.get("identity", "uuid"),
    )
}
