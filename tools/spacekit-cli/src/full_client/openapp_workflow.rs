//! Workflow emitter — `workflows` (Spec v0.2 §7) -> TypeScript sagas.
//!
//! Each workflow subscribes to its triggering event and runs its steps in
//! order as the system actor. When a step fails, the `compensate` capabilities
//! of the steps that already succeeded run in reverse order, then `onFailure`
//! is emitted; otherwise `onComplete` is emitted. Declared business errors are
//! not retried; other failures are retried up to `business.workflows.retries`.
//!
//! Only the in-process runner is generated so far. Durable engines named in the
//! profile (temporal, inngest, durable-queue) fall back to it with a note; the
//! saga semantics are the same either way.

use std::collections::BTreeSet;

use serde_json::{json, Map, Value};

use super::{camel, strip_ref, AppModel, Profile, Workflow};

pub(super) fn emit_workflows(m: &AppModel, p: &Profile) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut index = String::from(
        "// Generated workflow registry: importing this module subscribes every workflow\n\
         // to its triggering event. `events.ts` loads it on first emit.\n",
    );
    for w in &m.workflows {
        let base = camel(&w.name);
        out.push((format!("server/workflows/{base}.ts"), workflow_ts(m, p, w)));
        index.push_str(&format!("import \"./{base}\";\n"));
    }
    index.push_str("export {};\n");
    out.push(("server/workflows/index.ts".to_string(), index));
    out
}

fn retries(p: &Profile) -> u32 {
    p.business
        .get("workflows.retries", "0")
        .parse::<u32>()
        .unwrap_or(0)
}

/// Render a step's `with` value: `event.order.id` stays an expression; quoted
/// strings, numbers and booleans are literals.
fn render_with(expr: &str) -> String {
    let t = expr.trim();
    if t.starts_with("event.") || t.parse::<f64>().is_ok() || t == "true" || t == "false" {
        return t.to_string();
    }
    let inner = t.trim_matches(|c| c == '\'' || c == '"');
    format!("\"{}\"", inner.replace('"', "\\\""))
}

fn with_object(with: &std::collections::BTreeMap<String, String>) -> String {
    if with.is_empty() {
        return "{}".into();
    }
    let parts: Vec<String> = with
        .iter()
        .map(|(k, v)| format!("{}: {}", camel(k), render_with(v)))
        .collect();
    format!("{{ {} }}", parts.join(", "))
}

/// Payload for an outcome event: fields named like the trigger's payload are
/// carried over; a failure `reason` gets the error.
fn outcome_payload(m: &AppModel, w: &Workflow, event: &str, failure: bool) -> String {
    let Some(ev) = m.event(strip_ref(event)) else {
        return "{}".into();
    };
    let trigger = m.event(strip_ref(&w.on));
    let mut parts = Vec::new();
    for p in &ev.payload {
        let carried = trigger
            .map(|t| t.payload.iter().any(|tp| tp.name == p.name))
            .unwrap_or(false);
        if carried {
            parts.push(format!("{}: event.{}", camel(&p.name), camel(&p.name)));
        } else if failure && p.name == "reason" {
            parts.push(format!("{}: reason", camel(&p.name)));
        }
    }
    format!("{{ {} }}", parts.join(", "))
}

fn workflow_ts(m: &AppModel, p: &Profile, w: &Workflow) -> String {
    let fn_name = camel(&w.name);
    let trigger = strip_ref(&w.on).to_string();
    let mut runs: BTreeSet<String> = BTreeSet::new();
    for st in &w.steps {
        runs.insert(format!("run{}", strip_ref(&st.capability)));
        if let Some(c) = &st.compensate {
            runs.insert(format!("run{}", strip_ref(c)));
        }
    }
    let mut s = format!(
        "// Generated workflow {name} (Spec §7): saga with compensation, in-process runner.\n\
         // Do not edit; edit the spec and regenerate.\n\
         import {{ SYSTEM_ACTOR }} from \"../context\";\n\
         import {{ isApiError }} from \"../errors\";\n\
         import {{ emit, on }} from \"../events\";\n",
        name = w.name
    );
    if !runs.is_empty() {
        s.push_str(&format!(
            "import {{ {} }} from \"../capabilities/index\";\n",
            runs.iter().cloned().collect::<Vec<_>>().join(", ")
        ));
    }
    s.push_str(&format!("import type {{ {trigger}Event }} from \"../types\";\n\n"));

    // Declared errors per step capability: these are business outcomes, not retried.
    s.push_str(&format!(
        "/** Attempts per step for unexpected failures (business.workflows.retries). */\n\
         const RETRIES = {};\n\n",
        retries(p)
    ));
    s.push_str("const DECLARED_ERRORS: Record<string, readonly string[]> = {\n");
    let mut seen = BTreeSet::new();
    for st in &w.steps {
        for cap_name in std::iter::once(&st.capability).chain(st.compensate.iter()) {
            let name = strip_ref(cap_name).to_string();
            if !seen.insert(name.clone()) {
                continue;
            }
            let errs = m
                .capability(&name)
                .map(|c| c.errors.iter().map(|e| format!("\"{e}\"")).collect::<Vec<_>>().join(", "))
                .unwrap_or_default();
            s.push_str(&format!("  {name}: [{errs}],\n"));
        }
    }
    s.push_str("};\n\n");
    s.push_str(
        "async function attempt<T>(step: string, run: () => Promise<T>): Promise<T> {\n  \
         for (let i = 0; ; i++) {\n    \
         try {\n      return await run();\n    } catch (err) {\n      \
         const declared = isApiError(err) && (DECLARED_ERRORS[step] ?? []).includes(err.code);\n      \
         if (declared || i >= RETRIES) throw err;\n    }\n  }\n}\n\n",
    );

    if let Some(sum) = &w.summary {
        s.push_str(&format!("/** {sum} */\n"));
    }
    s.push_str(&format!(
        "export async function {fn_name}(event: {trigger}Event): Promise<\"completed\" | \"failed\"> {{\n  \
         const undo: Array<{{ step: string; run: () => Promise<unknown> }}> = [];\n  \
         try {{\n"
    ));
    for st in &w.steps {
        let cap = strip_ref(&st.capability);
        let args = with_object(&st.with);
        s.push_str(&format!(
            "    await attempt(\"{cap}\", () => run{cap}({args}, SYSTEM_ACTOR));\n"
        ));
        if let Some(comp) = &st.compensate {
            let comp = strip_ref(comp);
            s.push_str(&format!(
                "    undo.push({{ step: \"{comp}\", run: () => run{comp}({args}, SYSTEM_ACTOR) }});\n"
            ));
        }
    }
    s.push_str("  } catch (err) {\n");
    s.push_str(
        "    // Compensate the steps that succeeded, most recent first.\n    \
         for (const u of undo.reverse()) {\n      \
         try {\n        await attempt(u.step, u.run);\n      } catch (cerr) {\n        \
         console.error(`[workflow] compensation ${u.step} failed:`, cerr);\n      }\n    }\n",
    );
    match &w.on_failure {
        Some(ev) => {
            let payload = outcome_payload(m, w, ev, true);
            if payload.contains("reason") {
                s.push_str("    const reason = isApiError(err) ? err.code : String(err);\n");
            }
            s.push_str(&format!(
                "    await emit(\"{}\", {payload});\n",
                strip_ref(ev)
            ));
            if !payload.contains("reason") {
                s.push_str("    void err;\n");
            }
        }
        None => s.push_str("    console.error(`[workflow] failed:`, err);\n"),
    }
    s.push_str("    return \"failed\";\n  }\n");
    if let Some(ev) = &w.on_complete {
        s.push_str(&format!(
            "  await emit(\"{}\", {});\n",
            strip_ref(ev),
            outcome_payload(m, w, ev, false)
        ));
    }
    s.push_str("  return \"completed\";\n}\n\n");
    s.push_str(&format!(
        "on(\"{trigger}\", async (event) => {{\n  await {fn_name}(event);\n}});\n"
    ));
    s
}

/// The saga graph as realized, for `server/contract.json`.
pub(super) fn manifest(m: &AppModel, _p: &Profile) -> Value {
    let mut out = Map::new();
    for w in &m.workflows {
        out.insert(
            w.name.clone(),
            json!({
                "on": strip_ref(&w.on),
                "steps": w.steps.iter().map(|st| json!({
                    "do": strip_ref(&st.capability),
                    "with": with_object(&st.with),
                    "compensate": st.compensate.as_deref().map(strip_ref),
                })).collect::<Vec<_>>(),
                "compensationOrder": "reverse",
                "onComplete": w.on_complete.as_deref().map(strip_ref),
                "onFailure": w.on_failure.as_deref().map(strip_ref),
                "runsAs": "system",
            }),
        );
    }
    Value::Object(out)
}
