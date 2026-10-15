//! Business-layer emitter — `capabilities`, `events`, `policies` -> TypeScript.
//!
//! Every capability becomes three files:
//!
//! * `server/capabilities/<cap>.ts` (generated) — the contract: input
//!   validation, the capability's policy, its closed error set, and the events
//!   it may emit, all enforced around the handler by `runCapability`.
//! * `server/actions/<cap>.ts` (generated) — the public entry point (a Next.js
//!   server action when `transport: server-actions`), which resolves the caller
//!   with `currentActor()` and calls the capability.
//! * `server/handlers/<cap>.ts` (scaffold, written once) — the business logic,
//!   which the developer owns. It receives a typed `ctx` to authorize the
//!   loaded resource, emit declared events, and fail with declared errors.
//!
//! The profile's `errors` choice (`problem-json` throws `ApiError`;
//! `result-type` returns a discriminated union) only changes the public entry
//! point; the enforced contract is identical (checked by conformance).

use std::collections::BTreeMap;

use serde_json::{json, Map, Value};

use super::{
    assist::Assist, camel, is_builtin_policy, named_policy, policy, policy_include, strip_ref,
    AppModel, Capability, Entity, FieldDef, IoParam, Profile,
};

pub(super) struct BusinessOut {
    pub files: Vec<(String, String)>,
    pub scaffold: Vec<(String, String)>,
}

pub(super) fn emit_business(m: &AppModel, p: &Profile, assist: Option<&Assist>) -> BusinessOut {
    let mut files = Vec::new();
    let mut scaffold = Vec::new();
    files.push(("server/types.ts".to_string(), types_ts(m)));
    files.push(("server/errors.ts".to_string(), errors_ts(m, p)));
    files.push(("server/policies.ts".to_string(), policies_ts(m)));
    files.push(("server/validation.ts".to_string(), validation_ts(m)));
    files.push(("server/events.ts".to_string(), events_ts(m)));
    files.push(("server/context.ts".to_string(), CONTEXT_TS.to_string()));

    let mut index = String::from("// Generated public entry points (one per capability).\n");
    let mut caps_index = String::from(
        "// Generated capability runners: contract enforcement around each handler.\n\
         // Server-internal (workflows call these with the system actor); not client-callable.\n",
    );
    for c in &m.capabilities {
        let base = camel(&c.name);
        files.push((format!("server/capabilities/{base}.ts"), capability_ts(m, c)));
        files.push((format!("server/actions/{base}.ts"), action_ts(c, p)));
        scaffold.push((format!("server/handlers/{base}.ts"), handler_ts(m, c, assist)));
        index.push_str(&format!("export * from \"./{base}\";\n"));
        caps_index.push_str(&format!("export * from \"./{base}\";\n"));
    }
    if m.capabilities.is_empty() {
        index.push_str("export {};\n");
        caps_index.push_str("export {};\n");
    }
    files.push(("server/actions/index.ts".to_string(), index));
    files.push(("server/capabilities/index.ts".to_string(), caps_index));
    files.push(("server/package.json".to_string(), package_json(m, p)));
    files.push(("server/tsconfig.json".to_string(), SERVER_TSCONFIG.to_string()));
    files.push((
        "server/contract.json".to_string(),
        serde_json::to_string_pretty(&contract_manifest(m, p)).unwrap_or_default() + "\n",
    ));
    if needs_http_server(p) {
        files.push(("server/http.ts".to_string(), http_ts(m, p)));
    }
    scaffold.push(("server/auth.ts".to_string(), AUTH_TS.to_string()));
    BusinessOut { files, scaffold }
}

/// REST/RPC transports (and the React SPA, which talks to the API over HTTP)
/// get a small Node HTTP server; server actions do not need one.
fn needs_http_server(p: &Profile) -> bool {
    p.business.get("transport", "server-actions") != "server-actions"
        || !super::is_next(p.view.get("framework", "next"))
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Spec scalar -> TypeScript type.
pub(super) fn ts_scalar(ty: &str) -> &'static str {
    match ty {
        "integer" => "number",
        "decimal" | "money" => "Money",
        "boolean" => "boolean",
        "json" => "unknown",
        "timestamp" | "date" => "string | Date",
        _ => "string",
    }
}

pub(super) fn ts_type_of(ty: &str, values: &[String], list: bool) -> String {
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
    if list {
        if base.contains(' ') {
            format!("({base})[]")
        } else {
            format!("{base}[]")
        }
    } else {
        base
    }
}

fn entity_ts(e: &Entity) -> String {
    let mut s = format!("export interface {} {{\n", e.name);
    // Read shape: what the data layer returns. Identity, generated and defaulted
    // fields are always present; other optional fields may be null.
    for f in &e.fields {
        let present = f.required || f.generated || f.default.is_some() || f.name == e.identity;
        let ty = field_ts(f);
        if present {
            s.push_str(&format!("  {}: {};\n", camel(&f.name), ty));
        } else {
            s.push_str(&format!("  {}?: {} | null;\n", camel(&f.name), ty));
        }
    }
    for r in &e.relations {
        let target = &r.target;
        let ty = match r.kind {
            super::RelKind::HasMany | super::RelKind::ManyToMany => format!("{target}[]"),
            _ => format!("{target} | null"),
        };
        s.push_str(&format!("  {}?: {};\n", camel(&r.name), ty));
        if matches!(r.kind, super::RelKind::BelongsTo) {
            s.push_str(&format!("  {}Id?: string | null;\n", camel(&r.name)));
        }
    }
    s.push_str("}\n");
    s
}

fn field_ts(f: &FieldDef) -> String {
    ts_type_of(&f.ty, &f.values, false)
}

fn io_interface(name: &str, io: &[IoParam]) -> String {
    if io.is_empty() {
        return format!("export type {name} = Record<string, never>;\n");
    }
    let mut s = format!("export interface {name} {{\n");
    for p in io {
        let opt = if p.required { "" } else { "?" };
        let ty = ts_type_of(&p.ty, &p.values, p.list);
        s.push_str(&format!("  {}{}: {};\n", camel(&p.name), opt, ty));
    }
    s.push_str("}\n");
    s
}

fn types_ts(m: &AppModel) -> String {
    let mut s = String::from("// Generated domain, capability and event types (business layer).\n\n");
    s.push_str(
        "/** The caller. `null` means signed out. Shape is yours to extend in `auth.ts`. */\n\
         export interface Actor {\n  id: string;\n  role?: string;\n  wallet?: string | null;\n  \
         /** Set on the actor workflows run as. */\n  system?: boolean;\n  [key: string]: unknown;\n}\n\n\
         /** Decimal values: plain numbers, numeric strings, or decimal objects (e.g. Prisma.Decimal). */\n\
         export type Money = number | string | { toNumber(): number; toString(): string };\n\n",
    );
    for e in &m.entities {
        s.push_str(&entity_ts(e));
        s.push('\n');
    }
    for c in &m.capabilities {
        s.push_str(&io_interface(&format!("{}Input", c.name), &c.input));
        s.push_str(&io_interface(&format!("{}Output", c.name), &c.output));
        s.push('\n');
    }
    for ev in &m.events {
        s.push_str(&io_interface(&format!("{}Event", ev.name), &ev.payload));
        s.push('\n');
    }
    s
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

const BUILTIN_ERRORS: &[&str] = &[
    "Unauthenticated",
    "Forbidden",
    "ValidationFailed",
    "NotFound",
    "NotImplemented",
];

fn errors_ts(m: &AppModel, p: &Profile) -> String {
    let mut codes: Vec<String> = m.capabilities.iter().flat_map(|c| c.errors.clone()).collect();
    codes.sort();
    codes.dedup();
    let union = if codes.is_empty() {
        "never".to_string()
    } else {
        codes.iter().map(|c| format!("\"{c}\"")).collect::<Vec<_>>().join(" | ")
    };
    let builtin = BUILTIN_ERRORS
        .iter()
        .map(|c| format!("\"{c}\""))
        .collect::<Vec<_>>()
        .join(" | ");
    let style = p.business.get("errors", "problem-json");
    format!(
        "// Capability error codes (errors realization: {style}).\n\
         // Each capability may only fail with the errors it declares (Spec §6) plus these built-ins.\n\
         export type DeclaredError = {union};\n\
         export type BuiltinError = {builtin};\n\
         export type ErrorCode = DeclaredError | BuiltinError;\n\
         export const BUILTIN_ERRORS: readonly BuiltinError[] = [{builtin_list}];\n\n\
         export class ApiError<C extends string = ErrorCode> extends Error {{\n  \
         constructor(public code: C, message?: string, public details?: unknown) {{\n    \
         super(message ?? code);\n    this.name = \"ApiError\";\n  }}\n}}\n\n\
         export function isApiError(err: unknown): err is ApiError<string> {{\n  \
         return err instanceof ApiError;\n}}\n\n\
         export type Result<T, E extends string = ErrorCode> =\n  \
         | {{ ok: true; value: T }}\n  | {{ ok: false; error: E; message?: string }};\n",
        builtin_list = BUILTIN_ERRORS
            .iter()
            .map(|c| format!("\"{c}\""))
            .collect::<Vec<_>>()
            .join(", "),
    )
}

// ---------------------------------------------------------------------------
// Policies
// ---------------------------------------------------------------------------

fn policies_ts(m: &AppModel) -> String {
    let mut s = String::from(
        "// Generated access policies (Spec §9). Each is a pure function of the caller\n\
         // (`actor`, null when signed out) and the resource being acted on.\n\
         import type { Actor } from \"./types\";\n\n\
         /**\n * Read a field path. `x.id` falls back to the foreign key `xId` when the\n \
         * relation itself is not loaded. Missing values read as undefined.\n */\n\
         export function read(obj: unknown, path: readonly string[]): unknown {\n  \
         let cur: any = obj;\n  \
         for (let i = 0; i < path.length; i++) {\n    \
         if (cur == null) return undefined;\n    \
         const seg = path[i];\n    \
         if (i === path.length - 2 && path[i + 1] === \"id\" && cur[seg] == null && cur[`${seg}Id`] != null) {\n      \
         return cur[`${seg}Id`];\n    }\n    \
         cur = cur[seg];\n  }\n  \
         return cur;\n}\n\n\
         /** Equality that never matches two missing values. */\n\
         export function same(a: unknown, b: unknown): boolean {\n  \
         return a != null && b != null && a === b;\n}\n\n",
    );
    s.push_str("export const POLICIES = {\n");
    for pol in &m.policies {
        let body = match &pol.expr {
            Ok(e) => policy::to_ts(e),
            Err(_) => "false".to_string(),
        };
        s.push_str(&format!("  /** {} */\n", pol.source.replace("*/", "* /")));
        s.push_str(&format!(
            "  {}: (actor: Actor | null, resource: unknown): boolean => {{\n    void actor;\n    void resource;\n    return Boolean({});\n  }},\n",
            pol.name, body
        ));
    }
    s.push_str("} as const;\n\n");
    s.push_str(
        "export type PolicyName = keyof typeof POLICIES;\n\
         export type PolicyRef = PolicyName | \"public\" | \"authenticated\";\n\n\
         /** Evaluate a policy. Any exception counts as a denial. */\n\
         export function evaluate(policy: PolicyRef, actor: Actor | null, resource: unknown): boolean {\n  \
         if (policy === \"public\") return true;\n  \
         if (policy === \"authenticated\") return actor != null;\n  \
         try {\n    return POLICIES[policy](actor, resource);\n  } catch {\n    return false;\n  }\n}\n\n",
    );
    // Relation includes per (policy, entity) actually used.
    let mut includes: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    let mut add = |pol: &super::Policy, ent: &str| {
        if let Some(inc) = policy_include(m, pol, ent).to_prisma() {
            includes.entry(pol.name.clone()).or_default().insert(ent.to_string(), inc);
        }
    };
    for c in &m.capabilities {
        if let (Some(pref), Some(ent)) = (&c.policy, c.primary_entity()) {
            if let Some(pol) = named_policy(m, pref) {
                add(pol, &ent);
            }
        }
    }
    for v in &m.views {
        if let (Some(pref), Some(ent)) = (&v.policy, super::view_resource_entity(m, v)) {
            if let Some(pol) = named_policy(m, pref) {
                add(pol, &ent.name);
            }
        }
    }
    s.push_str(
        "/** Relations each policy walks, as data-layer `include` objects, so a loaded\n \
         * resource carries what the policy reads. */\n\
         export const POLICY_INCLUDE = {\n",
    );
    for (pol, ents) in &includes {
        s.push_str(&format!("  {pol}: {{\n"));
        for (ent, inc) in ents {
            s.push_str(&format!("    {ent}: {inc},\n"));
        }
        s.push_str("  },\n");
    }
    s.push_str("} as const;\n");
    s
}

// ---------------------------------------------------------------------------
// Validation (field rules + capability inputs)
// ---------------------------------------------------------------------------

fn field_spec(ty: &str, required: bool, values: &[String], list: bool, rules: Option<&super::Rules>) -> String {
    let mut parts = vec![format!("type: \"{}\"", ty.trim_start_matches('@'))];
    if ty.starts_with('@') {
        parts.push("ref: true".into());
    }
    if required {
        parts.push("required: true".into());
    }
    if list {
        parts.push("list: true".into());
    }
    if !values.is_empty() {
        parts.push(format!(
            "values: [{}]",
            values.iter().map(|v| format!("\"{v}\"")).collect::<Vec<_>>().join(", ")
        ));
    }
    if let Some(r) = rules {
        if let Some(v) = r.min {
            parts.push(format!("min: {v}"));
        }
        if let Some(v) = r.max {
            parts.push(format!("max: {v}"));
        }
        if let Some(v) = r.min_length {
            parts.push(format!("minLength: {v}"));
        }
        if let Some(v) = r.max_length {
            parts.push(format!("maxLength: {v}"));
        }
        if let Some(v) = &r.pattern {
            parts.push(format!("pattern: {}", serde_json::to_string(v).unwrap_or_default()));
        }
        if !r.one_of.is_empty() {
            parts.push(format!("oneOf: {}", serde_json::to_string(&r.one_of).unwrap_or_default()));
        }
    }
    format!("{{ {} }}", parts.join(", "))
}

fn validation_ts(m: &AppModel) -> String {
    let mut s = String::from(VALIDATION_HEAD);
    s.push_str("/** Field rules per entity (Spec §5), enforced by the data layer on create/update. */\n");
    s.push_str("export const ENTITY_RULES: Record<string, Record<string, FieldSpec>> = {\n");
    for e in &m.entities {
        s.push_str(&format!("  {}: {{\n", e.name));
        for f in &e.fields {
            if f.name == e.identity {
                continue;
            }
            let required = f.required && !f.generated && f.default.is_none();
            s.push_str(&format!(
                "    {}: {},\n",
                camel(&f.name),
                field_spec(&f.ty, required, &f.values, false, Some(&f.rules))
            ));
        }
        s.push_str("  },\n");
    }
    s.push_str("};\n\n");
    s.push_str("/** Capability inputs: types, required fields, enum values. */\n");
    s.push_str("export const INPUT_RULES: Record<string, Record<string, FieldSpec>> = {\n");
    for c in &m.capabilities {
        s.push_str(&format!("  {}: {{\n", c.name));
        for i in &c.input {
            s.push_str(&format!(
                "    {}: {},\n",
                camel(&i.name),
                field_spec(&i.ty, i.required, &i.values, i.list, None)
            ));
        }
        s.push_str("  },\n");
    }
    s.push_str("};\n\n");
    s.push_str(
        "/** Validate entity data. `partial` (updates) skips required checks. Prisma model\n \
         * names and entity names are the same. */\n\
         export function validateEntity(entity: string, data: unknown, opts: { partial?: boolean } = {}): Issue[] {\n  \
         const spec = ENTITY_RULES[entity];\n  \
         if (!spec || data == null || typeof data !== \"object\") return [];\n  \
         return check(spec, data as Record<string, unknown>, { ...opts, allowUnknown: true });\n}\n\n\
         export function validateInput(capability: string, input: unknown): Issue[] {\n  \
         const spec = INPUT_RULES[capability] ?? {};\n  \
         if (input == null || typeof input !== \"object\") return [{ path: \"\", message: \"input must be an object\" }];\n  \
         return check(spec, input as Record<string, unknown>, { allowUnknown: false });\n}\n",
    );
    s
}

const VALIDATION_HEAD: &str = r#"// Generated validation (Spec §5 rules + capability input contracts).
import { ApiError } from "./errors";

export interface Issue {
  path: string;
  message: string;
}

export interface FieldSpec {
  type: string;
  ref?: boolean;
  required?: boolean;
  list?: boolean;
  values?: readonly string[];
  min?: number;
  max?: number;
  minLength?: number;
  maxLength?: number;
  pattern?: string;
  oneOf?: readonly unknown[];
}

const EMAIL = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;

function typeIssue(f: FieldSpec, v: unknown): string | null {
  if (f.ref) return v !== null && typeof v === "object" ? null : "must be an object";
  switch (f.type) {
    case "integer":
      return typeof v === "number" && Number.isInteger(v) ? null : "must be an integer";
    case "decimal":
    case "money":
      if (typeof v === "number" && Number.isFinite(v)) return null;
      if (typeof v === "string" && v.trim() !== "" && !Number.isNaN(Number(v))) return null;
      if (v !== null && typeof v === "object" && typeof (v as { toNumber?: unknown }).toNumber === "function") return null;
      return "must be a number";
    case "boolean":
      return typeof v === "boolean" ? null : "must be true or false";
    case "timestamp":
    case "date":
      if (v instanceof Date) return Number.isNaN(v.getTime()) ? "must be a valid date" : null;
      return typeof v === "string" && !Number.isNaN(Date.parse(v)) ? null : "must be a date";
    case "enum":
      return typeof v === "string" && (f.values ?? []).includes(v) ? null : `must be one of ${(f.values ?? []).join(", ")}`;
    case "email":
      return typeof v === "string" && EMAIL.test(v) ? null : "must be an email address";
    case "url":
      try {
        new URL(String(v));
        return null;
      } catch {
        return "must be a URL";
      }
    case "json":
      return null;
    case "id":
      return typeof v === "string" || typeof v === "number" ? null : "must be an id";
    default:
      return typeof v === "string" ? null : "must be text";
  }
}

function numeric(v: unknown): number | null {
  if (typeof v === "number") return v;
  if (typeof v === "string" && v.trim() !== "" && !Number.isNaN(Number(v))) return Number(v);
  if (v !== null && typeof v === "object" && typeof (v as { toNumber?: () => number }).toNumber === "function") {
    return (v as { toNumber: () => number }).toNumber();
  }
  return null;
}

function checkValue(path: string, f: FieldSpec, v: unknown, out: Issue[]): void {
  const t = typeIssue(f, v);
  if (t) {
    out.push({ path, message: t });
    return;
  }
  const n = numeric(v);
  if (f.min !== undefined && n !== null && n < f.min) out.push({ path, message: `must be at least ${f.min}` });
  if (f.max !== undefined && n !== null && n > f.max) out.push({ path, message: `must be at most ${f.max}` });
  if (typeof v === "string") {
    if (f.minLength !== undefined && v.length < f.minLength) out.push({ path, message: `must be at least ${f.minLength} characters` });
    if (f.maxLength !== undefined && v.length > f.maxLength) out.push({ path, message: `must be at most ${f.maxLength} characters` });
    if (f.pattern !== undefined && !new RegExp(f.pattern).test(v)) out.push({ path, message: "has the wrong format" });
  }
  if (f.oneOf !== undefined && !f.oneOf.includes(v)) out.push({ path, message: `must be one of ${f.oneOf.join(", ")}` });
}

export function check(
  spec: Record<string, FieldSpec>,
  data: Record<string, unknown>,
  opts: { partial?: boolean; allowUnknown?: boolean } = {},
): Issue[] {
  const out: Issue[] = [];
  for (const [key, f] of Object.entries(spec)) {
    const v = data[key];
    if (v === undefined || v === null) {
      if (f.required && !opts.partial) out.push({ path: key, message: "is required" });
      continue;
    }
    if (f.list) {
      if (!Array.isArray(v)) {
        out.push({ path: key, message: "must be a list" });
        continue;
      }
      v.forEach((item, i) => checkValue(`${key}[${i}]`, f, item, out));
      continue;
    }
    checkValue(key, f, v, out);
  }
  if (!opts.allowUnknown) {
    for (const key of Object.keys(data)) {
      if (!(key in spec)) out.push({ path: key, message: "is not an input of this capability" });
    }
  }
  return out;
}

export function assertValid(issues: Issue[]): void {
  if (issues.length > 0) {
    throw new ApiError(
      "ValidationFailed",
      issues.map((i) => (i.path ? `${i.path} ${i.message}` : i.message)).join("; "),
      issues,
    );
  }
}

"#;

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

fn events_ts(m: &AppModel) -> String {
    let mut s = String::from(
        "// Generated domain-event bus (in-process). Capabilities queue events through\n\
         // `ctx.emit`; they are delivered here after the handler succeeds.\n",
    );
    if !m.events.is_empty() {
        let names: Vec<String> = m.events.iter().map(|e| format!("{}Event", e.name)).collect();
        s.push_str(&format!("import type {{ {} }} from \"./types\";\n", names.join(", ")));
    }
    s.push_str("\nexport interface EventPayloads {\n");
    for e in &m.events {
        s.push_str(&format!("  {}: {}Event;\n", e.name, e.name));
    }
    s.push_str("}\n\n");
    s.push_str(
        "export type EventName = keyof EventPayloads;\n\
         type Handler<K extends EventName> = (payload: EventPayloads[K]) => Promise<void> | void;\n\n\
         const subscribers = new Map<EventName, Array<Handler<never>>>();\n\n\
         export function on<K extends EventName>(event: K, handler: Handler<K>): void {\n  \
         const list = subscribers.get(event) ?? [];\n  \
         list.push(handler as Handler<never>);\n  \
         subscribers.set(event, list);\n}\n\n\
         let workflowsLoaded: Promise<unknown> | null = null;\n\n\
         /** Deliver an event to its subscribers (workflows register on first use). */\n\
         export async function emit<K extends EventName>(event: K, payload: EventPayloads[K]): Promise<void> {\n  \
         workflowsLoaded ??= import(\"./workflows/index\");\n  \
         await workflowsLoaded;\n  \
         for (const handler of subscribers.get(event) ?? []) {\n    \
         try {\n      await (handler as Handler<K>)(payload);\n    } catch (err) {\n      \
         console.error(`[events] subscriber of ${String(event)} failed:`, err);\n    }\n  }\n}\n",
    );
    s
}

// ---------------------------------------------------------------------------
// Capability contract + entry point + handler scaffold
// ---------------------------------------------------------------------------

const CONTEXT_TS: &str = r#"// Generated capability runtime: enforces each capability's contract around its handler.
import { ApiError, BUILTIN_ERRORS, isApiError } from "./errors";
import { emit, type EventName, type EventPayloads } from "./events";
import { evaluate, type PolicyRef } from "./policies";
import type { Actor } from "./types";
import { assertValid, validateInput } from "./validation";

/** The actor workflows run as. It passes every policy: the triggering
 *  capability already authorized the original caller. */
export const SYSTEM_ACTOR: Actor = { id: "system", role: "system", system: true };

export interface CapabilitySpec {
  readonly name: string;
  readonly policy: PolicyRef;
  /** The policy reads `resource`, so the handler must call `ctx.authorize`. */
  readonly requiresResource: boolean;
  readonly errors: readonly string[];
  readonly emits: readonly EventName[];
}

export interface CapabilityContext<E extends string, V extends EventName> {
  /** The caller (null when signed out). */
  readonly actor: Actor | null;
  /** Check the capability's policy against the resource you loaded. Throws `Forbidden`. */
  authorize(resource: unknown): void;
  /** Queue one of the capability's declared events; delivered after success. */
  emit<K extends V>(event: K, payload: EventPayloads[K]): void;
  /** Fail with one of the capability's declared errors. */
  fail(code: E, message?: string): never;
}

export async function runCapability<I, O, E extends string, V extends EventName>(
  spec: CapabilitySpec,
  actor: Actor | null,
  input: I,
  handler: (input: I, ctx: CapabilityContext<E, V>) => Promise<O>,
): Promise<O> {
  const system = actor?.system === true;
  if (!system && spec.policy === "authenticated" && actor == null) {
    throw new ApiError("Unauthenticated");
  }
  assertValid(validateInput(spec.name, input));
  if (!system && spec.policy !== "public" && spec.policy !== "authenticated" && !spec.requiresResource) {
    if (!evaluate(spec.policy, actor, undefined)) throw new ApiError("Forbidden");
  }

  let authorized = system || !spec.requiresResource;
  const queued: Array<[EventName, unknown]> = [];
  const ctx: CapabilityContext<E, V> = {
    actor,
    authorize(resource: unknown) {
      if (!system && !evaluate(spec.policy, actor, resource)) throw new ApiError("Forbidden");
      authorized = true;
    },
    emit(event, payload) {
      if (!spec.emits.includes(event)) {
        throw new Error(`${spec.name} may not emit ${String(event)}: it is not in the capability's emits list`);
      }
      queued.push([event, payload]);
    },
    fail(code, message) {
      throw new ApiError(code, message);
    },
  };

  let out: O;
  try {
    out = await handler(input, ctx);
  } catch (err) {
    if (isApiError(err) && !spec.errors.includes(err.code) && !(BUILTIN_ERRORS as readonly string[]).includes(err.code)) {
      throw new Error(`${spec.name} failed with undeclared error ${err.code}; add it to the capability's errors in the spec`);
    }
    throw err;
  }
  if (!authorized) {
    throw new Error(
      `${spec.name} returned without calling ctx.authorize(resource); policy ${spec.policy} must be checked against the loaded resource`,
    );
  }
  for (const [event, payload] of queued) {
    await emit(event, payload as EventPayloads[typeof event]);
  }
  return out;
}

/** Turn a thrown `ApiError` into a result value (the `result-type` errors realization). */
export async function asResult<T>(run: () => Promise<T>): Promise<{ ok: true; value: T } | { ok: false; error: string; message?: string }> {
  try {
    return { ok: true, value: await run() };
  } catch (err) {
    if (isApiError(err)) return { ok: false, error: err.code, message: err.message };
    throw err;
  }
}
"#;

/// What a capability's policy means for code generation.
pub(super) struct PolicyPlan {
    /// `public`, `authenticated`, or the policy name.
    pub reference: String,
    pub requires_resource: bool,
    pub source: Option<String>,
}

pub(super) fn policy_plan(m: &AppModel, c: &Capability) -> PolicyPlan {
    let reference = c
        .policy
        .as_deref()
        .map(strip_ref)
        .unwrap_or("public")
        .to_string();
    if is_builtin_policy(&reference) {
        return PolicyPlan { reference, requires_resource: false, source: None };
    }
    let pol = named_policy(m, &reference);
    PolicyPlan {
        requires_resource: pol
            .and_then(|p| p.expr.as_ref().ok())
            .map(policy::uses_resource)
            .unwrap_or(false),
        source: pol.map(|p| p.source.clone()),
        reference,
    }
}

fn str_array(items: &[String]) -> String {
    format!(
        "[{}]",
        items.iter().map(|s| format!("\"{}\"", strip_ref(s))).collect::<Vec<_>>().join(", ")
    )
}

fn spec_const(c: &Capability) -> String {
    super::snake(&c.name).to_uppercase()
}

fn capability_ts(m: &AppModel, c: &Capability) -> String {
    let base = camel(&c.name);
    let konst = spec_const(c);
    let plan = policy_plan(m, c);
    let mut s = format!(
        "// Generated contract for capability {name}. Do not edit; edit the spec and regenerate.\n\
         import {{ runCapability, type CapabilityContext }} from \"../context\";\n\
         import {{ handle{name} }} from \"../handlers/{base}\";\n\
         import type {{ Actor, {name}Input, {name}Output }} from \"../types\";\n\n",
        name = c.name,
    );
    s.push_str(&format!(
        "export const {konst} = {{\n  name: \"{name}\",\n  policy: \"{pol}\",\n  requiresResource: {rr},\n  \
         errors: {errs},\n  emits: {emits},\n}} as const;\n\n",
        name = c.name,
        pol = plan.reference,
        rr = plan.requires_resource,
        errs = str_array(&c.errors),
        emits = str_array(&c.emits),
    ));
    s.push_str(&format!(
        "export type {name}Error = (typeof {konst}.errors)[number];\n\
         export type {name}EventName = (typeof {konst}.emits)[number];\n\
         export type {name}Context = CapabilityContext<{name}Error, {name}EventName>;\n\n",
        name = c.name
    ));
    if let Some(sum) = &c.summary {
        s.push_str(&format!("/** {sum} */\n"));
    }
    s.push_str(&format!(
        "export function run{name}(input: {name}Input, actor: Actor | null): Promise<{name}Output> {{\n  \
         return runCapability({konst}, actor, input, handle{name});\n}}\n",
        name = c.name
    ));
    s
}

fn action_ts(c: &Capability, p: &Profile) -> String {
    let fn_name = camel(&c.name);
    let base = camel(&c.name);
    let result_type = p.business.get("errors", "problem-json") == "result-type";
    let transport = p.business.effective(&c.name, "transport", "server-actions");
    let mut s = String::from("// Generated public entry point. Do not edit.\n");
    if transport == "server-actions" {
        s.push_str("\"use server\";\n");
    }
    s.push_str("import { currentActor } from \"../auth\";\n");
    if result_type {
        s.push_str("import { asResult } from \"../context\";\n");
    }
    s.push_str(&format!(
        "import {{ run{name} }} from \"../capabilities/{base}\";\n",
        name = c.name
    ));
    if result_type {
        s.push_str(&format!(
            "import type {{ {name}Error }} from \"../capabilities/{base}\";\n\
             import type {{ BuiltinError, Result }} from \"../errors\";\n",
            name = c.name
        ));
    }
    s.push_str(&format!(
        "import type {{ {name}Input, {name}Output }} from \"../types\";\n\n",
        name = c.name
    ));
    if let Some(sum) = &c.summary {
        s.push_str(&format!("/** {sum} */\n"));
    }
    if result_type {
        s.push_str(&format!(
            "export async function {fn_name}(input: {name}Input): Promise<Result<{name}Output, {name}Error | BuiltinError>> {{\n  \
             const actor = await currentActor();\n  \
             return (await asResult(() => run{name}(input, actor))) as Result<{name}Output, {name}Error | BuiltinError>;\n}}\n",
            name = c.name
        ));
    } else {
        s.push_str(&format!(
            "export async function {fn_name}(input: {name}Input): Promise<{name}Output> {{\n  \
             return run{name}(input, await currentActor());\n}}\n",
            name = c.name
        ));
    }
    s
}

fn handler_ts(m: &AppModel, c: &Capability, assist: Option<&Assist>) -> String {
    let base = camel(&c.name);
    let plan = policy_plan(m, c);
    let mut s = format!(
        "// Business logic for {name}. Generated once by `spacekit spec generate`; this file is yours\n\
         // and is never overwritten. The contract around it (input validation, policy, declared\n\
         // errors and events) is enforced by ../capabilities/{base}.ts.\n//\n",
        name = c.name
    );
    if let Some(sum) = &c.summary {
        s.push_str(&format!("//   {sum}\n"));
    }
    match (&plan.source, plan.requires_resource) {
        (Some(src), true) => s.push_str(&format!(
            "//   policy:  {} ({src}) -> load the resource, then call ctx.authorize(resource)\n",
            plan.reference
        )),
        (Some(src), false) => s.push_str(&format!(
            "//   policy:  {} ({src}) -> checked before this handler runs\n",
            plan.reference
        )),
        (None, _) => s.push_str(&format!("//   policy:  {}\n", plan.reference)),
    }
    if !c.reads.is_empty() {
        s.push_str(&format!("//   reads:   {}\n", c.reads.join(", ")));
    }
    if !c.writes.is_empty() {
        let ws: Vec<String> = c.writes.iter().map(|w| format!("{} {}", w.effect, w.entity)).collect();
        s.push_str(&format!("//   writes:  {}\n", ws.join(", ")));
    }
    if !c.emits.is_empty() {
        s.push_str(&format!(
            "//   emits:   {} -> ctx.emit(\"{}\", {{ … }})\n",
            c.emits.join(", "),
            strip_ref(&c.emits[0])
        ));
    }
    if !c.errors.is_empty() {
        s.push_str(&format!(
            "//   errors:  {} -> ctx.fail(\"{}\")\n",
            c.errors.join(", "),
            c.errors[0]
        ));
    }
    if let Some(a) = assist {
        s.push_str(&a.suggestion_comment(c));
    }
    s.push_str(&format!(
        "\nimport {{ ApiError }} from \"../errors\";\n\
         import type {{ {name}Context }} from \"../capabilities/{base}\";\n\
         import type {{ {name}Input, {name}Output }} from \"../types\";\n",
        name = c.name
    ));
    // A worked example of the resource check, using the policy's include tree.
    let example = if plan.requires_resource {
        c.primary_entity().and_then(|ent| {
            let id_input = c.input.iter().find(|i| i.ty == "id" || i.name.ends_with("Id") || i.name == "id")?;
            let inc = named_policy(m, &plan.reference)
                .and_then(|pol| policy_include(m, pol, &ent).to_prisma())
                .map(|_| format!(", include: POLICY_INCLUDE.{}.{ent}", plan.reference))
                .unwrap_or_default();
            Some(format!(
                "  // Example:\n  \
                 //   const {var} = await db.{var}.findUnique({{ where: {{ id: input.{id} }}{inc} }});\n  \
                 //   if (!{var}) throw new ApiError(\"NotFound\");\n  \
                 //   ctx.authorize({var});\n",
                var = camel(&ent),
                id = camel(&id_input.name),
            ))
        })
    } else {
        None
    };
    if example.is_some() {
        s.push_str("// import { db } from \"../db\";\n// import { POLICY_INCLUDE } from \"../policies\";\n");
    } else {
        s.push_str("// import { db } from \"../db\";\n");
    }
    s.push_str(&format!(
        "\nexport async function handle{name}(input: {name}Input, ctx: {name}Context): Promise<{name}Output> {{\n",
        name = c.name
    ));
    if let Some(ex) = example {
        s.push_str(&ex);
    }
    s.push_str(&format!(
        "  void input;\n  void ctx;\n  throw new ApiError(\"NotImplemented\", \"{name} is not implemented yet (server/handlers/{base}.ts)\");\n}}\n",
        name = c.name
    ));
    s
}

const AUTH_TS: &str = r#"// Who is calling. Generated once; this file is yours.
//
// Return the signed-in actor, or null when signed out (then only `public`
// capabilities and views work). Wire this to your sign-in: a SpaceKit
// identity (DID), a session cookie, or a bearer token.
import type { Actor } from "./types";

/** During development you can act as someone by setting SPACEKIT_DEV_ACTOR,
 *  e.g. SPACEKIT_DEV_ACTOR='{"id":"u1","role":"admin"}'. Ignored in production. */
function devActor(): Actor | null {
  const raw = process.env.SPACEKIT_DEV_ACTOR;
  if (!raw || process.env.NODE_ENV === "production") return null;
  try {
    return JSON.parse(raw) as Actor;
  } catch {
    return null;
  }
}

/** The caller of a server action or server component. */
export async function currentActor(): Promise<Actor | null> {
  return devActor();
}

/** The caller of an HTTP request (REST/RPC transports). */
export async function actorFromRequest(headers: Record<string, string | string[] | undefined>): Promise<Actor | null> {
  void headers;
  return devActor();
}
"#;

// ---------------------------------------------------------------------------
// HTTP server (REST/RPC transports, React SPA)
// ---------------------------------------------------------------------------

fn http_ts(m: &AppModel, p: &Profile) -> String {
    let openapi = super::synthesize_openapi(m, p);
    let mut routes: Vec<(String, String, String, bool)> = Vec::new(); // method, path, cap, query
    for c in &m.capabilities {
        let transport = p.business.effective(&c.name, "transport", "rest");
        let (http, path, _, _, _) = super::project_capability(m, c, &transport);
        routes.push((http.to_uppercase(), path, c.name.clone(), c.is_query() && transport != "rpc"));
    }
    let _ = openapi;
    let mut s = String::from(
        "// Generated HTTP server for the REST/RPC transports (Node, no framework).\n\
         // Run with: npx tsx server/http.ts   (PORT defaults to 8080; serves under /api)\n\
         import { createServer, type IncomingMessage, type ServerResponse } from \"node:http\";\n\
         import { actorFromRequest } from \"./auth\";\n\
         import { isApiError } from \"./errors\";\n",
    );
    if !m.capabilities.is_empty() {
        let runs: Vec<String> = m.capabilities.iter().map(|c| format!("run{}", c.name)).collect();
        s.push_str(&format!("import {{ {} }} from \"./capabilities/index\";\n", runs.join(", ")));
    }
    s.push_str("import { INPUT_RULES } from \"./validation\";\n\n");
    s.push_str(
        "type Run = (input: any, actor: any) => Promise<unknown>;\n\
         const ROUTES: Array<{ method: string; path: string; capability: string; query: boolean; run: Run }> = [\n",
    );
    for (method, path, cap, query) in &routes {
        s.push_str(&format!(
            "  {{ method: \"{method}\", path: \"/api{path}\", capability: \"{cap}\", query: {query}, run: run{cap} }},\n"
        ));
    }
    s.push_str(
        "];\n\n\
         const STATUS: Record<string, number> = { Unauthenticated: 401, Forbidden: 403, NotFound: 404, ValidationFailed: 422, NotImplemented: 501 };\n\n\
         /** Query strings are text; coerce to the declared input types. */\n\
         function fromQuery(capability: string, params: URLSearchParams): Record<string, unknown> {\n  \
         const spec = INPUT_RULES[capability] ?? {};\n  \
         const out: Record<string, unknown> = {};\n  \
         for (const [key, value] of params) {\n    \
         if (key === \"cursor\" || key === \"limit\") continue;\n    \
         const t = spec[key]?.type;\n    \
         out[key] = t === \"integer\" || t === \"decimal\" || t === \"money\" ? Number(value) : t === \"boolean\" ? value === \"true\" : value;\n  }\n  \
         return out;\n}\n\n\
         async function body(req: IncomingMessage): Promise<unknown> {\n  \
         const chunks: Buffer[] = [];\n  \
         for await (const c of req) chunks.push(c as Buffer);\n  \
         const raw = Buffer.concat(chunks).toString(\"utf8\");\n  \
         return raw ? JSON.parse(raw) : {};\n}\n\n\
         function send(res: ServerResponse, status: number, value: unknown): void {\n  \
         res.writeHead(status, { \"Content-Type\": \"application/json\", \"Access-Control-Allow-Origin\": process.env.CORS_ORIGIN ?? \"*\" });\n  \
         res.end(JSON.stringify(value));\n}\n\n\
         export const server = createServer(async (req, res) => {\n  \
         if (req.method === \"OPTIONS\") {\n    \
         res.writeHead(204, { \"Access-Control-Allow-Origin\": process.env.CORS_ORIGIN ?? \"*\", \"Access-Control-Allow-Headers\": \"content-type, authorization\", \"Access-Control-Allow-Methods\": \"GET, POST\" });\n    \
         return res.end();\n  }\n  \
         const url = new URL(req.url ?? \"/\", \"http://localhost\");\n  \
         const route = ROUTES.find((r) => r.method === req.method && r.path === url.pathname);\n  \
         if (!route) return send(res, 404, { code: \"NotFound\", message: \"no such route\" });\n  \
         try {\n    \
         const input = route.query ? fromQuery(route.capability, url.searchParams) : await body(req);\n    \
         const out = await route.run(input, await actorFromRequest(req.headers));\n    \
         // A single list output is wrapped as { data } to match the client SDK's pagination shape.\n    \
         const values = out && typeof out === \"object\" ? Object.values(out as object) : [];\n    \
         if (route.query && values.length === 1 && Array.isArray(values[0])) return send(res, 200, { data: values[0], next_cursor: null });\n    \
         return send(res, 200, out);\n  } catch (err) {\n    \
         if (isApiError(err)) return send(res, STATUS[err.code] ?? 400, { code: err.code, message: err.message });\n    \
         console.error(err);\n    \
         return send(res, 500, { code: \"Internal\", message: \"internal error\" });\n  }\n});\n\n\
         if (process.argv[1] && /http\\.(ts|js)$/.test(process.argv[1])) {\n  \
         const port = Number(process.env.PORT ?? 8080);\n  \
         server.listen(port, () => console.log(`API on http://localhost:${port}/api`));\n}\n",
    );
    s
}

// ---------------------------------------------------------------------------
// Realized-behavior manifest (conformance input)
// ---------------------------------------------------------------------------

fn io_manifest(io: &[IoParam]) -> Value {
    let mut o = Map::new();
    for p in io {
        o.insert(
            camel(&p.name),
            json!({ "type": ts_type_of(&p.ty, &p.values, p.list), "required": p.required }),
        );
    }
    Value::Object(o)
}

/// What the generated business layer enforces, written next to it as
/// `server/contract.json`. Conformance compares this file across profiles.
fn contract_manifest(m: &AppModel, p: &Profile) -> Value {
    let mut caps = Map::new();
    for c in &m.capabilities {
        let plan = policy_plan(m, c);
        let mut errors: Vec<String> = c.errors.clone();
        errors.sort();
        let mut emits: Vec<String> = c.emits.iter().map(|e| strip_ref(e).to_string()).collect();
        emits.sort();
        caps.insert(
            c.name.clone(),
            json!({
                "entry": camel(&c.name),
                "input": io_manifest(&c.input),
                "output": io_manifest(&c.output),
                "policy": plan.reference,
                "resourceCheck": plan.requires_resource,
                "errors": errors,
                "emits": emits,
            }),
        );
    }
    let policies: Map<String, Value> = m
        .policies
        .iter()
        .map(|pol| {
            (
                pol.name.clone(),
                json!(pol.expr.as_ref().map(policy::to_ts).unwrap_or_else(|_| "false".into())),
            )
        })
        .collect();
    let rules: Map<String, Value> = m
        .entities
        .iter()
        .map(|e| {
            let fields: Map<String, Value> = e
                .fields
                .iter()
                .filter(|f| f.name != e.identity)
                .map(|f| {
                    let required = f.required && !f.generated && f.default.is_none();
                    (camel(&f.name), json!(field_spec(&f.ty, required, &f.values, false, Some(&f.rules))))
                })
                .collect();
            (e.name.clone(), Value::Object(fields))
        })
        .collect();
    json!({
        "capabilities": caps,
        "policies": policies,
        "entityRules": rules,
        "events": m.events.iter().map(|e| (e.name.clone(), io_manifest(&e.payload))).collect::<Map<String, Value>>(),
        "workflows": super::workflow::manifest(m, p),
    })
}

fn package_json(m: &AppModel, p: &Profile) -> String {
    let storage = super::is_storage_node(p.data.get("store", "postgres"));
    let deps = if storage {
        String::new()
    } else {
        "    \"@prisma/client\": \"^5.22.0\"\n".to_string()
    };
    let mut dev = vec![
        "    \"typescript\": \"^5.6.0\"".to_string(),
        "    \"@types/node\": \"^20.0.0\"".to_string(),
        "    \"tsx\": \"^4.19.0\"".to_string(),
    ];
    if !storage {
        dev.push("    \"prisma\": \"^5.22.0\"".to_string());
    }
    let mut scripts = vec!["    \"typecheck\": \"tsc -p tsconfig.json --noEmit\"".to_string()];
    if needs_http_server(p) {
        scripts.push("    \"start\": \"tsx http.ts\"".to_string());
    }
    format!(
        "{{\n  \"name\": \"{name}-server\",\n  \"private\": true,\n  \"type\": \"module\",\n  \
         \"scripts\": {{\n{scripts}\n  }},\n  \
         \"dependencies\": {{\n{deps}  }},\n  \
         \"devDependencies\": {{\n{dev}\n  }}\n}}\n",
        name = super::kebab(&m.app.name),
        scripts = scripts.join(",\n"),
        dev = dev.join(",\n"),
    )
}

const SERVER_TSCONFIG: &str = r#"{
  "compilerOptions": {
    "target": "ES2022",
    "module": "ESNext",
    "moduleResolution": "Bundler",
    "strict": true,
    "esModuleInterop": true,
    "skipLibCheck": true,
    "noEmit": true,
    "types": ["node"]
  },
  "include": ["./**/*.ts"],
  "exclude": ["node_modules"]
}
"#;
