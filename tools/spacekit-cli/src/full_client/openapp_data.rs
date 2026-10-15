//! Data-layer emitter — `entities` -> Prisma schema (+ migration scaffold), or a
//! spacekit-storage-node document backend. Both enforce the spec's field rules
//! on every write.
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
    r#"// Prisma client (data-layer access) with the spec's field rules enforced on every write.
import { PrismaClient } from "@prisma/client";
import { assertValid, validateEntity } from "./validation";

function withRules(client: PrismaClient) {
  return client.$extends({
    query: {
      $allModels: {
        async create({ model, args, query }) {
          assertValid(validateEntity(model, args.data));
          return query(args);
        },
        async createMany({ model, args, query }) {
          const rows = Array.isArray(args.data) ? args.data : [args.data];
          for (const row of rows) assertValid(validateEntity(model, row));
          return query(args);
        },
        async update({ model, args, query }) {
          assertValid(validateEntity(model, args.data, { partial: true }));
          return query(args);
        },
        async updateMany({ model, args, query }) {
          assertValid(validateEntity(model, args.data, { partial: true }));
          return query(args);
        },
        async upsert({ model, args, query }) {
          assertValid(validateEntity(model, args.create));
          assertValid(validateEntity(model, args.update, { partial: true }));
          return query(args);
        },
      },
    },
  });
}

type Db = ReturnType<typeof withRules>;
const globalForDb = globalThis as unknown as { __spacekitDb?: Db };

export const db: Db = globalForDb.__spacekitDb ?? withRules(new PrismaClient());
if (process.env.NODE_ENV !== "production") globalForDb.__spacekitDb = db;
"#
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

    s.push_str("import { assertValid, validateEntity } from \"./validation\";\n\n");
    // Relation map: which FK field and collection each to-one relation resolves through.
    s.push_str(
        "/** To-one relations stored as `<name>Id`, resolved for `include`. */\n\
         const RELATIONS: Record<string, Record<string, { entity: string; collection: string }>> = {\n",
    );
    for e in &m.entities {
        s.push_str(&format!("  {}: {{\n", e.name));
        for r in &e.relations {
            if matches!(r.kind, RelKind::BelongsTo | RelKind::HasOne) {
                s.push_str(&format!(
                    "    {}: {{ entity: \"{}\", collection: \"{}\" }},\n",
                    camel(&r.name),
                    r.target,
                    collection_name(&r.target)
                ));
            }
        }
        s.push_str("  },\n");
    }
    s.push_str("};\n\n");
    s.push_str("/** Defaults and generated values filled in on create (the storage node is schemaless). */\n");
    s.push_str("const DEFAULTS: Record<string, () => Record<string, unknown>> = {\n");
    for e in &m.entities {
        let mut parts = Vec::new();
        for f in &e.fields {
            if f.name == e.identity {
                continue;
            }
            if f.generated && matches!(f.ty.as_str(), "timestamp" | "date" | "time") {
                parts.push(format!("{}: new Date().toISOString()", camel(&f.name)));
            } else if let Some(d) = &f.default {
                parts.push(format!("{}: {}", camel(&f.name), d));
            }
        }
        s.push_str(&format!("  {}: () => ({{ {} }}),\n", e.name, parts.join(", ")));
    }
    s.push_str("};\n\n");
    s.push_str(STORAGE_REPO_TS);

    s.push_str("export const db = {\n");
    for e in &m.entities {
        s.push_str(&format!(
            "  {}: repo<{}>(\"{}\", \"{}\"),\n",
            camel(&e.name),
            e.name,
            e.name,
            collection_name(&e.name),
        ));
    }
    s.push_str("};\n");
    s
}

const STORAGE_REPO_TS: &str = r#"type WhereInput = Record<string, unknown>;
type Include = { [relation: string]: true | { include: Include } };

function toFilters(where: WhereInput) {
  return Object.entries(where).map(([path, value]) => ({ path, op: "Equals" as const, value }));
}

/** Resolve `include` for to-one relations (stored as `<relation>Id`). */
async function resolve<T>(entity: string, row: T | null, include?: Include): Promise<T | null> {
  if (!row || !include) return row;
  const out: Record<string, unknown> = { ...(row as Record<string, unknown>) };
  for (const [rel, spec] of Object.entries(include)) {
    const target = RELATIONS[entity]?.[rel];
    const fk = out[`${rel}Id`];
    if (!target || fk == null) continue;
    const child = await getDoc<Record<string, unknown>>(target.collection, String(fk));
    out[rel] = spec === true ? child : await resolve(target.entity, child, spec.include);
  }
  return out as T;
}

function repo<T extends { id?: string }>(entity: string, collection: string) {
  return {
    async findUnique(args: { where: { id: string }; include?: Include }): Promise<T | null> {
      return resolve(entity, await getDoc<T>(collection, String(args.where.id)), args.include);
    },
    async findFirst(args?: { where?: WhereInput; include?: Include }): Promise<T | null> {
      const where = args?.where ?? {};
      const row = where.id
        ? await getDoc<T>(collection, String(where.id))
        : (await queryDocs<T>(collection, { filters: toFilters(where), limit: 1 }))[0] ?? null;
      return resolve(entity, row, args?.include);
    },
    async findMany(args?: { where?: WhereInput; include?: Include }): Promise<T[]> {
      const where = args?.where ?? {};
      const rows = Object.keys(where).length === 0 ? await listDocs<T>(collection) : await queryDocs<T>(collection, { filters: toFilters(where) });
      if (!args?.include) return rows;
      return (await Promise.all(rows.map((r) => resolve(entity, r, args.include)))).filter((r): r is Awaited<T> => r !== null) as T[];
    },
    async create(args: { data: Partial<T> }): Promise<T> {
      assertValid(validateEntity(entity, args.data));
      const id = (args.data.id as string | undefined) ?? randomUUID();
      return putDoc<T>(collection, id, { ...DEFAULTS[entity]?.(), ...(args.data as T), id } as T);
    },
    async update(args: { where: { id: string }; data: Partial<T> }): Promise<T> {
      assertValid(validateEntity(entity, args.data, { partial: true }));
      const current = (await getDoc<T>(collection, args.where.id)) ?? ({} as T);
      return putDoc<T>(collection, args.where.id, { ...current, ...args.data, id: args.where.id });
    },
    async delete(args: { where: { id: string } }): Promise<void> {
      await deleteDoc(collection, args.where.id);
    },
  };
}

"#;

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

    let plan = plan_relations(m);
    let sqlite = store == "sqlite";

    let mut s = String::new();
    s.push_str("// Generated by `spacekit spec generate` — SpaceKit app data layer.\n");
    s.push_str("// Do not edit by hand; re-run generation (hand-edits are detected).\n\n");
    s.push_str(&format!(
        "datasource db {{\n  provider = \"{provider}\"\n  url      = env(\"DATABASE_URL\")\n}}\n\n"
    ));
    s.push_str("generator client {\n  provider = \"prisma-client-js\"\n}\n\n");

    // Enums (collected from enum fields). SQLite has no enums: they become strings
    // and the allowed values are enforced by the validation layer.
    for e in m.entities.iter().filter(|_| !sqlite) {
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
            s.push_str(&render_field(e, f, &identity, id_scalar, id_default, snake, sqlite));
            if f.indexed && f.name != e.identity {
                index_lines.push(format!("  @@index([{}])", camel(&f.name)));
            }
        }

        // Relation fields (declared and synthesized back sides).
        for line in plan.fields.get(&e.name).into_iter().flatten() {
            s.push_str(&line.render(id_scalar));
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
    sqlite: bool,
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

    let ty = if f.ty == "enum" && !f.values.is_empty() && !sqlite {
        enum_name(&e.name, &f.name)
    } else {
        prisma_scalar(&f.ty).to_string()
    };
    let optional = if f.required { "" } else { "?" };
    line.push_str(&format!(" {ty}{optional}"));

    if f.unique {
        line.push_str(" @unique");
    }
    if let Some(def) = prisma_default(f, sqlite) {
        line.push_str(&format!(" {def}"));
    }
    if snake {
        line.push_str(&format!(" @map(\"{}\")", to_snake(&f.name)));
    }
    line.push('\n');
    line
}

fn prisma_default(f: &FieldDef, sqlite: bool) -> Option<String> {
    if f.generated {
        return match f.ty.as_str() {
            "timestamp" | "date" | "time" => Some("@default(now())".to_string()),
            _ => None,
        };
    }
    let d = f.default.as_ref()?;
    let rendered = match d {
        serde_json::Value::String(s) => {
            if f.ty == "enum" && !sqlite {
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

/// One relation field to render on a model.
#[derive(Debug, Clone)]
struct RelField {
    field: String,
    target: String,
    list: bool,
    /// Foreign-key field this side carries (to-one owning side).
    fk: Option<String>,
    /// `@relation("name")` when two entities are linked more than once.
    name: Option<String>,
    /// The FK must be unique (one-to-one).
    unique_fk: bool,
}

impl RelField {
    fn render(&self, id_scalar: &str) -> String {
        let named = self
            .name
            .as_ref()
            .map(|n| format!("\"{n}\""))
            .unwrap_or_default();
        if self.list {
            let attr = if named.is_empty() { String::new() } else { format!(" @relation({named})") };
            return format!("  {} {}[]{attr}\n", self.field, self.target);
        }
        match &self.fk {
            Some(fk) => {
                let name_part = if named.is_empty() { String::new() } else { format!("{named}, ") };
                let unique = if self.unique_fk { " @unique" } else { "" };
                format!(
                    "  {field} {target}? @relation({name_part}fields: [{fk}], references: [id])\n  {fk} {id_scalar}?{unique}\n",
                    field = self.field,
                    target = self.target,
                )
            }
            None => {
                let attr = if named.is_empty() { String::new() } else { format!(" @relation({named})") };
                format!("  {} {}?{attr}\n", self.field, self.target)
            }
        }
    }
}

#[derive(Default)]
struct RelationPlan {
    fields: BTreeMap<String, Vec<RelField>>,
}

/// Pair every declared relation with its other side (by `inverse`, else by a
/// complementary relation on the target), synthesize missing back sides, and
/// name relations when two entities are linked more than once — so the
/// Prisma schema is always complete and unambiguous.
fn plan_relations(m: &AppModel) -> RelationPlan {
    #[derive(Clone)]
    struct Decl<'a> {
        owner: &'a str,
        r: &'a RelationDef,
    }
    let decls: Vec<Decl> = m
        .entities
        .iter()
        .flat_map(|e| e.relations.iter().map(move |r| Decl { owner: &e.name, r }))
        .filter(|d| m.entity(&d.r.target).is_some())
        .collect();
    let complementary = |a: &RelKind, b: &RelKind| {
        matches!(
            (a, b),
            (RelKind::BelongsTo, RelKind::HasMany)
                | (RelKind::HasMany, RelKind::BelongsTo)
                | (RelKind::BelongsTo, RelKind::HasOne)
                | (RelKind::HasOne, RelKind::BelongsTo)
                | (RelKind::ManyToMany, RelKind::ManyToMany)
        )
    };
    let mut paired: Vec<Option<usize>> = vec![None; decls.len()];
    // 1. Explicit inverses.
    for i in 0..decls.len() {
        if paired[i].is_some() {
            continue;
        }
        let Some(inv) = &decls[i].r.inverse else { continue };
        if let Some(j) = decls.iter().position(|d| {
            d.owner == decls[i].r.target && &d.r.name == inv && d.r.target == decls[i].owner
        }) {
            if paired[j].is_none() && i != j {
                paired[i] = Some(j);
                paired[j] = Some(i);
            }
        }
    }
    // 2. Unique complementary matches (prefer belongsTo <-> hasMany).
    for pass in 0..2 {
        for i in 0..decls.len() {
            if paired[i].is_some() {
                continue;
            }
            let candidates: Vec<usize> = (0..decls.len())
                .filter(|&j| {
                    j != i
                        && paired[j].is_none()
                        && decls[j].owner == decls[i].r.target
                        && decls[j].r.target == decls[i].owner
                        && complementary(&decls[i].r.kind, &decls[j].r.kind)
                        && (pass == 1
                            || matches!(
                                (&decls[i].r.kind, &decls[j].r.kind),
                                (RelKind::BelongsTo, RelKind::HasMany) | (RelKind::HasMany, RelKind::BelongsTo) | (RelKind::ManyToMany, RelKind::ManyToMany)
                            ))
                })
                .collect();
            if let Some(&j) = candidates.first() {
                paired[i] = Some(j);
                paired[j] = Some(i);
            }
        }
    }

    // How many links exist between each unordered entity pair (for naming).
    let mut links: BTreeMap<(String, String), usize> = BTreeMap::new();
    for (i, d) in decls.iter().enumerate() {
        if let Some(j) = paired[i] {
            if j < i {
                continue; // count each pair once
            }
        }
        let key = if d.owner <= d.r.target.as_str() {
            (d.owner.to_string(), d.r.target.clone())
        } else {
            (d.r.target.clone(), d.owner.to_string())
        };
        *links.entry(key).or_default() += 1;
    }
    let needs_name = |a: &str, b: &str| {
        let key = if a <= b { (a.to_string(), b.to_string()) } else { (b.to_string(), a.to_string()) };
        links.get(&key).copied().unwrap_or(0) > 1
    };

    let mut plan = RelationPlan::default();
    let mut taken: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for e in &m.entities {
        let mut names: Vec<String> = e.fields.iter().map(|f| camel(&f.name)).collect();
        names.extend(e.relations.iter().map(|r| camel(&r.name)));
        names.extend(
            e.relations
                .iter()
                .filter(|r| matches!(r.kind, RelKind::BelongsTo | RelKind::HasOne))
                .map(|r| camel(&format!("{}Id", r.name))),
        );
        taken.insert(e.name.clone(), names);
    }
    let fresh = |taken: &mut BTreeMap<String, Vec<String>>, entity: &str, want: String| -> String {
        let list = taken.entry(entity.to_string()).or_default();
        let mut name = want.clone();
        let mut n = 2;
        while list.contains(&name) {
            name = format!("{want}{n}");
            n += 1;
        }
        list.push(name.clone());
        name
    };

    let mut done = vec![false; decls.len()];
    for i in 0..decls.len() {
        if done[i] {
            continue;
        }
        done[i] = true;
        let d = &decls[i];
        let rel_name = needs_name(d.owner, &d.r.target).then(|| format!("{}_{}", d.owner, camel(&d.r.name)));
        match paired[i] {
            Some(j) => {
                done[j] = true;
                let o = &decls[j];
                // Which side owns the FK: belongsTo, else (hasOne/hasOne) the first.
                let (a, b) = match (&d.r.kind, &o.r.kind) {
                    (RelKind::BelongsTo, _) => (d, o),
                    (_, RelKind::BelongsTo) => (o, d),
                    _ => (d, o),
                };
                let one_to_one = matches!(b.r.kind, RelKind::HasOne);
                if matches!(a.r.kind, RelKind::ManyToMany) {
                    for x in [a, b] {
                        plan.fields.entry(x.owner.to_string()).or_default().push(RelField {
                            field: camel(&x.r.name),
                            target: x.r.target.clone(),
                            list: true,
                            fk: None,
                            name: rel_name.clone(),
                            unique_fk: false,
                        });
                    }
                    continue;
                }
                plan.fields.entry(a.owner.to_string()).or_default().push(RelField {
                    field: camel(&a.r.name),
                    target: a.r.target.clone(),
                    list: false,
                    fk: Some(camel(&format!("{}Id", a.r.name))),
                    name: rel_name.clone(),
                    unique_fk: one_to_one,
                });
                plan.fields.entry(b.owner.to_string()).or_default().push(RelField {
                    field: camel(&b.r.name),
                    target: b.r.target.clone(),
                    list: !one_to_one,
                    fk: None,
                    name: rel_name,
                    unique_fk: false,
                });
            }
            None => {
                // Unpaired: render this side, synthesize the other.
                match d.r.kind {
                    RelKind::BelongsTo | RelKind::HasOne => {
                        plan.fields.entry(d.owner.to_string()).or_default().push(RelField {
                            field: camel(&d.r.name),
                            target: d.r.target.clone(),
                            list: false,
                            fk: Some(camel(&format!("{}Id", d.r.name))),
                            name: rel_name.clone(),
                            unique_fk: false,
                        });
                        let back = if rel_name.is_some() {
                            format!("{}As{}", camel(&pluralize(d.owner)), super::pascal(&d.r.name))
                        } else {
                            camel(&pluralize(d.owner))
                        };
                        let back = fresh(&mut taken, &d.r.target, back);
                        plan.fields.entry(d.r.target.clone()).or_default().push(RelField {
                            field: back,
                            target: d.owner.to_string(),
                            list: true,
                            fk: None,
                            name: rel_name,
                            unique_fk: false,
                        });
                    }
                    RelKind::HasMany => {
                        plan.fields.entry(d.owner.to_string()).or_default().push(RelField {
                            field: camel(&d.r.name),
                            target: d.r.target.clone(),
                            list: true,
                            fk: None,
                            name: rel_name.clone(),
                            unique_fk: false,
                        });
                        let back = fresh(&mut taken, &d.r.target, camel(d.owner));
                        let fk = fresh(&mut taken, &d.r.target, format!("{back}Id"));
                        plan.fields.entry(d.r.target.clone()).or_default().push(RelField {
                            field: back,
                            target: d.owner.to_string(),
                            list: false,
                            fk: Some(fk),
                            name: rel_name,
                            unique_fk: false,
                        });
                    }
                    RelKind::ManyToMany => {
                        plan.fields.entry(d.owner.to_string()).or_default().push(RelField {
                            field: camel(&d.r.name),
                            target: d.r.target.clone(),
                            list: true,
                            fk: None,
                            name: rel_name.clone(),
                            unique_fk: false,
                        });
                        let back = fresh(&mut taken, &d.r.target, camel(&pluralize(d.owner)));
                        plan.fields.entry(d.r.target.clone()).or_default().push(RelField {
                            field: back,
                            target: d.owner.to_string(),
                            list: true,
                            fk: None,
                            name: rel_name,
                            unique_fk: false,
                        });
                    }
                }
            }
        }
    }
    plan
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
        The schema is regenerated from the spec's `entities`. Re-running \
        `spacekit spec generate` detects hand-edits and preserves them unless `--force`.\n",
        store = p.data.get("store", "postgres"),
        identity = p.data.get("identity", "uuid"),
    )
}
