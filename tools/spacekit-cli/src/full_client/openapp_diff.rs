//! Spec compatibility (SpaceKit App Spec v0.2 §14).
//!
//! Compares two behavioral fingerprints (see `behavioral_fingerprint`) and
//! classifies every change as *compatible* (minor bump) or *breaking* (major
//! bump). Where a change cannot be proven safe (an edited policy expression,
//! say) it is treated as breaking.

use serde_json::{Map, Value};

#[derive(Debug, Default)]
pub(crate) struct Report {
    pub breaking: Vec<String>,
    pub compatible: Vec<String>,
}

impl Report {
    fn brk(&mut self, s: String) {
        self.breaking.push(s);
    }
    fn ok(&mut self, s: String) {
        self.compatible.push(s);
    }
}

fn obj(v: &Value) -> Map<String, Value> {
    v.as_object().cloned().unwrap_or_default()
}

fn s_of(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => "none".into(),
        other => other.to_string(),
    }
}

fn str_list(v: Option<&Value>) -> Vec<String> {
    v.and_then(|x| x.as_array())
        .map(|a| a.iter().map(s_of).collect())
        .unwrap_or_default()
}

fn flag(v: &Value, key: &str) -> bool {
    v.get(key).and_then(|x| x.as_bool()).unwrap_or(false)
}

/// Classify every change between two fingerprints.
pub(crate) fn compare(old: &Value, new: &Value) -> Report {
    let mut r = Report::default();
    entities(&mut r, old.get("entities"), new.get("entities"));
    capabilities(&mut r, old.get("capabilities"), new.get("capabilities"));
    named(&mut r, "event", old.get("events"), new.get("events"), |r, name, a, b| {
        params(r, &format!("event `{name}` payload"), a.get("payload"), b.get("payload"), Direction::Output);
    });
    named(&mut r, "workflow", old.get("workflows"), new.get("workflows"), workflow);
    named(&mut r, "policy", old.get("policies"), new.get("policies"), |r, name, a, b| {
        if a != b {
            r.brk(format!(
                "policy `{name}` changed ({} -> {}); treated as breaking because a rewritten rule may be tighter",
                s_of(a),
                s_of(b)
            ));
        }
    });
    named(&mut r, "widget", old.get("widgets"), new.get("widgets"), |r, name, a, b| {
        params(r, &format!("widget `{name}` props"), a.get("props"), b.get("props"), Direction::Input);
        set_change(r, &format!("widget `{name}` slots"), a.get("slots"), b.get("slots"));
    });
    named(&mut r, "view", old.get("views"), new.get("views"), view);
    set_change(&mut r, "text keys", old.get("text"), new.get("text"));
    named(&mut r, "storage bucket", old.get("storage"), new.get("storage"), |r, name, a, b| {
        if a != b {
            r.brk(format!("storage bucket `{name}` access changed ({} -> {})", s_of(a), s_of(b)));
        }
    });
    r
}

/// Generic added/removed handling for a registry section.
fn named(
    r: &mut Report,
    what: &str,
    old: Option<&Value>,
    new: Option<&Value>,
    on_both: impl Fn(&mut Report, &str, &Value, &Value),
) {
    let (a, b) = (obj(old.unwrap_or(&Value::Null)), obj(new.unwrap_or(&Value::Null)));
    for (k, va) in &a {
        match b.get(k) {
            None => r.brk(format!("{what} `{k}` removed or renamed")),
            Some(vb) => on_both(r, k, va, vb),
        }
    }
    for k in b.keys() {
        if !a.contains_key(k) {
            r.ok(format!("{what} `{k}` added"));
        }
    }
}

/// Removed members are breaking; added members are compatible.
fn set_change(r: &mut Report, what: &str, old: Option<&Value>, new: Option<&Value>) {
    let (a, b) = (str_list(old), str_list(new));
    for x in &a {
        if !b.contains(x) {
            r.brk(format!("{what}: `{x}` removed"));
        }
    }
    for x in &b {
        if !a.contains(x) {
            r.ok(format!("{what}: `{x}` added"));
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Direction {
    /// Values flow in (capability input, widget props, view params):
    /// adding a required one breaks callers.
    Input,
    /// Values flow out (capability output, event payload): removing one breaks readers.
    Output,
}

fn params(r: &mut Report, what: &str, old: Option<&Value>, new: Option<&Value>, dir: Direction) {
    let (a, b) = (obj(old.unwrap_or(&Value::Null)), obj(new.unwrap_or(&Value::Null)));
    for (k, pa) in &a {
        let Some(pb) = b.get(k) else {
            r.brk(format!("{what}: `{k}` removed"));
            continue;
        };
        if pa.get("type") != pb.get("type") || pa.get("list") != pb.get("list") {
            r.brk(format!(
                "{what}: `{k}` type changed ({} -> {})",
                s_of(pa.get("type").unwrap_or(&Value::Null)),
                s_of(pb.get("type").unwrap_or(&Value::Null))
            ));
        }
        if dir == Direction::Input && !flag(pa, "required") && flag(pb, "required") {
            r.brk(format!("{what}: `{k}` became required"));
        }
        if dir == Direction::Input && flag(pa, "required") && !flag(pb, "required") {
            r.ok(format!("{what}: `{k}` became optional"));
        }
        enum_values(r, &format!("{what}: `{k}`"), pa, pb);
    }
    for (k, pb) in &b {
        if a.contains_key(k) {
            continue;
        }
        if dir == Direction::Input && flag(pb, "required") {
            r.brk(format!("{what}: required `{k}` added"));
        } else {
            r.ok(format!("{what}: `{k}` added"));
        }
    }
}

fn enum_values(r: &mut Report, what: &str, a: &Value, b: &Value) {
    let (va, vb) = (str_list(a.get("values")), str_list(b.get("values")));
    if va.is_empty() && vb.is_empty() {
        return;
    }
    let removed: Vec<&String> = va.iter().filter(|x| !vb.contains(x)).collect();
    let added: Vec<&String> = vb.iter().filter(|x| !va.contains(x)).collect();
    if !removed.is_empty() {
        r.brk(format!(
            "{what} enum narrowed (removed {})",
            removed.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
        ));
    }
    if !added.is_empty() {
        r.ok(format!(
            "{what} enum widened (added {})",
            added.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
        ));
    }
}

fn entities(r: &mut Report, old: Option<&Value>, new: Option<&Value>) {
    named(r, "entity", old, new, |r, name, a, b| {
        if a.get("identity") != b.get("identity") {
            r.brk(format!("entity `{name}` identity changed"));
        }
        let (fa, fb) = (obj(a.get("fields").unwrap_or(&Value::Null)), obj(b.get("fields").unwrap_or(&Value::Null)));
        for (k, x) in &fa {
            let what = format!("entity `{name}` field `{k}`");
            let Some(y) = fb.get(k) else {
                r.brk(format!("{what} removed or renamed"));
                continue;
            };
            if x.get("type") != y.get("type") {
                r.brk(format!(
                    "{what} type changed ({} -> {})",
                    s_of(x.get("type").unwrap_or(&Value::Null)),
                    s_of(y.get("type").unwrap_or(&Value::Null))
                ));
            }
            if !flag(x, "required") && flag(y, "required") {
                r.brk(format!("{what} became required"));
            }
            if flag(x, "required") && !flag(y, "required") {
                r.ok(format!("{what} became optional"));
            }
            if !flag(x, "unique") && flag(y, "unique") {
                r.brk(format!("{what} became unique"));
            }
            if x.get("bucket") != y.get("bucket") {
                r.brk(format!("{what} moved to a different storage bucket"));
            }
            enum_values(r, &what, x, y);
            rules(r, &what, x.get("rules"), y.get("rules"));
        }
        for (k, y) in &fb {
            if fa.contains_key(k) {
                continue;
            }
            if flag(y, "required") && !flag(y, "generated") {
                r.brk(format!("entity `{name}` gained required field `{k}`"));
            } else {
                r.ok(format!("entity `{name}` gained optional field `{k}`"));
            }
        }
        named(r, &format!("entity `{name}` relation"), a.get("relations"), b.get("relations"), |r, rel, x, y| {
            if x != y {
                r.brk(format!("entity `{name}` relation `{rel}` changed"));
            }
        });
    });
}

/// Rules: relaxing is compatible, tightening (or adding a rule) is breaking.
fn rules(r: &mut Report, what: &str, old: Option<&Value>, new: Option<&Value>) {
    let (a, b) = (obj(old.unwrap_or(&Value::Null)), obj(new.unwrap_or(&Value::Null)));
    let num = |m: &Map<String, Value>, k: &str| m.get(k).and_then(|x| x.as_f64());
    // (key, larger-is-tighter)
    for (key, larger_tighter) in [("min", true), ("minLength", true), ("max", false), ("maxLength", false)] {
        match (num(&a, key), num(&b, key)) {
            (None, Some(v)) => r.brk(format!("{what} gained rule `{key}: {v}`")),
            (Some(v), None) => r.ok(format!("{what} dropped rule `{key}: {v}`")),
            (Some(x), Some(y)) if x != y => {
                let tighter = if larger_tighter { y > x } else { y < x };
                let msg = format!("{what} rule `{key}` {x} -> {y}");
                if tighter {
                    r.brk(format!("{msg} (tightened)"));
                } else {
                    r.ok(format!("{msg} (relaxed)"));
                }
            }
            _ => {}
        }
    }
    match (a.get("pattern"), b.get("pattern")) {
        (None, Some(_)) => r.brk(format!("{what} gained a `pattern` rule")),
        (Some(_), None) => r.ok(format!("{what} dropped its `pattern` rule")),
        (Some(x), Some(y)) if x != y => r.brk(format!("{what} `pattern` changed")),
        _ => {}
    }
    let (oa, ob) = (str_list(a.get("oneOf")), str_list(b.get("oneOf")));
    if oa != ob {
        if ob.is_empty() {
            r.ok(format!("{what} dropped its `oneOf` rule"));
        } else if !oa.is_empty() && oa.iter().all(|x| ob.contains(x)) {
            r.ok(format!("{what} `oneOf` widened"));
        } else {
            r.brk(format!("{what} `oneOf` narrowed or added"));
        }
    }
}

fn capabilities(r: &mut Report, old: Option<&Value>, new: Option<&Value>) {
    named(r, "capability", old, new, |r, name, a, b| {
        let what = format!("capability `{name}`");
        params(r, &format!("{what} input"), a.get("input"), b.get("input"), Direction::Input);
        params(r, &format!("{what} output"), a.get("output"), b.get("output"), Direction::Output);
        if a.get("writes") != b.get("writes") {
            r.brk(format!("{what} `writes` effects changed"));
        }
        if a.get("errors") != b.get("errors") {
            r.brk(format!(
                "{what} error set changed ([{}] -> [{}])",
                str_list(a.get("errors")).join(", "),
                str_list(b.get("errors")).join(", ")
            ));
        }
        set_change(r, &format!("{what} emits"), a.get("emits"), b.get("emits"));
        if a.get("policy") != b.get("policy") {
            let (pa, pb) = (s_of(a.get("policy").unwrap_or(&Value::Null)), s_of(b.get("policy").unwrap_or(&Value::Null)));
            if pb == "public" {
                r.ok(format!("{what} policy relaxed ({pa} -> public)"));
            } else {
                r.brk(format!("{what} policy changed ({pa} -> {pb})"));
            }
        }
        if a.get("idempotent") != b.get("idempotent") {
            r.brk(format!("{what} idempotency changed"));
        }
        if a.get("reads") != b.get("reads") {
            r.ok(format!("{what} `reads` changed"));
        }
    });
}

fn workflow(r: &mut Report, name: &str, a: &Value, b: &Value) {
    let what = format!("workflow `{name}`");
    if a.get("on") != b.get("on") {
        r.brk(format!("{what} trigger changed"));
    }
    let empty = Vec::new();
    let sa = a.get("steps").and_then(|x| x.as_array()).unwrap_or(&empty);
    let sb = b.get("steps").and_then(|x| x.as_array()).unwrap_or(&empty);
    let ida: Vec<String> = sa.iter().map(|s| s_of(s.get("do").unwrap_or(&Value::Null))).collect();
    let idb: Vec<String> = sb.iter().map(|s| s_of(s.get("do").unwrap_or(&Value::Null))).collect();
    if ida != idb {
        r.brk(format!("{what} steps changed ([{}] -> [{}])", ida.join(", "), idb.join(", ")));
    } else {
        for (i, (x, y)) in sa.iter().zip(sb.iter()).enumerate() {
            match (x.get("compensate"), y.get("compensate")) {
                (Some(Value::String(c)), Some(Value::Null)) | (Some(Value::String(c)), None) => {
                    r.brk(format!("{what} step {} lost its compensation `{c}`", i + 1))
                }
                (Some(Value::Null), Some(Value::String(c))) | (None, Some(Value::String(c))) => {
                    r.ok(format!("{what} step {} gained compensation `{c}`", i + 1))
                }
                (Some(p), Some(q)) if p != q => r.brk(format!("{what} step {} compensation changed", i + 1)),
                _ => {}
            }
            if x.get("with") != y.get("with") {
                r.brk(format!("{what} step {} inputs changed", i + 1));
            }
        }
    }
    for key in ["onComplete", "onFailure"] {
        if a.get(key) != b.get(key) {
            r.brk(format!("{what} {key} outcome changed"));
        }
    }
}

fn view(r: &mut Report, name: &str, a: &Value, b: &Value) {
    let what = format!("view `{name}`");
    if a.get("route") != b.get("route") {
        r.brk(format!(
            "{what} route changed ({} -> {})",
            s_of(a.get("route").unwrap_or(&Value::Null)),
            s_of(b.get("route").unwrap_or(&Value::Null))
        ));
    }
    params(r, &format!("{what} params"), a.get("params"), b.get("params"), Direction::Input);
    if a.get("policy") != b.get("policy") {
        r.brk(format!("{what} policy changed"));
    }
    let to = |v: &Value| -> Vec<String> {
        v.get("transitions")
            .and_then(|x| x.as_array())
            .map(|a| a.iter().map(|t| s_of(t.get("to").unwrap_or(&Value::Null))).collect())
            .unwrap_or_default()
    };
    let (ta, tb) = (to(a), to(b));
    for t in &ta {
        if !tb.contains(t) {
            r.brk(format!("{what} lost its transition to `{t}`"));
        }
    }
    for t in &tb {
        if !ta.contains(t) {
            r.ok(format!("{what} gained a transition to `{t}`"));
        }
    }
    for key in ["data", "actions", "states"] {
        if a.get(key) != b.get(key) {
            r.ok(format!("{what} {key} changed"));
        }
    }
}

/// `1.2.3` -> 1 (missing or malformed parts read as 0).
pub(crate) fn major(v: &str) -> u64 {
    v.trim().trim_start_matches('v').split('.').next().and_then(|x| x.parse().ok()).unwrap_or(0)
}

pub(crate) fn minor(v: &str) -> u64 {
    v.trim().trim_start_matches('v').split('.').nth(1).and_then(|x| x.parse().ok()).unwrap_or(0)
}

/// Every path at which two JSON documents differ, for conformance reports.
pub(crate) fn json_differences(prefix: &str, a: &Value, b: &Value, out: &mut Vec<String>) {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            for (k, va) in x {
                let p = format!("{prefix}.{k}");
                match y.get(k) {
                    Some(vb) => json_differences(&p, va, vb, out),
                    None => out.push(format!("{p}: only in the first profile")),
                }
            }
            for k in y.keys() {
                if !x.contains_key(k) {
                    out.push(format!("{prefix}.{k}: only in the second profile"));
                }
            }
        }
        (Value::Array(x), Value::Array(y)) if x.len() == y.len() => {
            for (i, (va, vb)) in x.iter().zip(y.iter()).enumerate() {
                json_differences(&format!("{prefix}[{i}]"), va, vb, out);
            }
        }
        _ if a != b => out.push(format!("{prefix}: {} vs {}", a, b)),
        _ => {}
    }
}
