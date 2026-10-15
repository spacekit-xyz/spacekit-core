//! Brain assist: certified design-pattern routing for capability handlers.
//!
//! `--assist <project.gf.toml>` loads a brain project's `[certified_routing]`
//! (for example the Python Coder brain in `spacekit-projects/coding/python`)
//! and asks it which design pattern or algorithm fits each capability. The
//! answer is written as a comment in the handler scaffold. It is a suggestion:
//! the brain is certified for routing (picking the right pattern), not for
//! writing the handler, and it abstains when it is not confident.
//!
//! Certified routing encodes text with sentence-transformers
//! (`all-mpnet-base-v2`) through a small Python bridge, so it needs Python 3
//! with `sentence-transformers` installed. The same loader is used by
//! `spacekit agent code` / `agent app` (see `code_session.rs`).

use std::collections::BTreeMap;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Command;

use growformer::semantic_router::{CertifiedRoutingPaths, SemanticRouter};

use super::{AppModel, Capability};

/// The encoder bridge, embedded so installed binaries do not depend on a
/// source checkout of growformer.
const ENCODER_PY: &str = include_str!("../../scripts/mpnet_encode_stdin.py");

pub(crate) struct CertifiedProject {
    pub name: String,
    pub dir: PathBuf,
    pub paths: CertifiedRoutingPaths,
    pub verdict: Option<PathBuf>,
}

/// Read `[certified_routing]` from a brain project's `.gf.toml`.
pub(crate) fn certified_project(gf_toml: &Path) -> Result<CertifiedProject, Box<dyn Error>> {
    let raw = std::fs::read_to_string(gf_toml)
        .map_err(|e| format!("cannot read brain project {}: {e}", gf_toml.display()))?;
    let doc: toml::Value = toml::from_str(&raw).map_err(|e| format!("{}: {e}", gf_toml.display()))?;
    let dir = gf_toml.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let name = doc
        .get("project")
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
        .unwrap_or("brain")
        .to_string();
    let cr = doc.get("certified_routing").ok_or_else(|| {
        format!(
            "{} has no [certified_routing] section; only certified brains can assist",
            gf_toml.display()
        )
    })?;
    let get = |k: &str| cr.get(k).and_then(|v| v.as_str()).map(|s| dir.join(s));
    let enabled = cr.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false);
    if !enabled {
        return Err(format!("{}: [certified_routing] is not enabled", gf_toml.display()).into());
    }
    let semantic_graph = get("semantic_graph").ok_or("[certified_routing] needs semantic_graph")?;
    let thresholds = get("thresholds").unwrap_or_else(|| dir.join("agent/routing_thresholds.json"));
    Ok(CertifiedProject {
        name,
        paths: CertifiedRoutingPaths {
            enabled,
            semantic_graph,
            thresholds,
            snippet_catalog: get("snippet_catalog"),
            code_generation: cr.get("code_generation").and_then(|v| v.as_bool()).unwrap_or(false),
        },
        verdict: get("verdict"),
        dir,
    })
}

fn python_bin() -> String {
    std::env::var("GROWFORMER_PYTHON").unwrap_or_else(|_| "python3".into())
}

/// Make sure the mpnet encoder can run: Python with sentence-transformers, and
/// the bridge script on disk (`GROWFORMER_MPNET_ENCODE_SCRIPT`).
pub(crate) fn ensure_encoder() -> Result<(), String> {
    let probe = Command::new(python_bin())
        .args(["-c", "import sentence_transformers"])
        .output()
        .map_err(|e| format!("{} not found ({e}); set GROWFORMER_PYTHON", python_bin()))?;
    if !probe.status.success() {
        return Err(format!(
            "certified routing needs sentence-transformers: `{} -m pip install sentence-transformers` \
             (the all-mpnet-base-v2 model downloads once on first use)",
            python_bin()
        ));
    }
    let usable = std::env::var("GROWFORMER_MPNET_ENCODE_SCRIPT")
        .map(|p| Path::new(&p).is_file())
        .unwrap_or(false);
    if !usable {
        let dir = std::env::temp_dir().join("spacekit-growformer");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let script = dir.join("mpnet_encode_stdin.py");
        std::fs::write(&script, ENCODER_PY).map_err(|e| e.to_string())?;
        std::env::set_var("GROWFORMER_MPNET_ENCODE_SCRIPT", &script);
    }
    Ok(())
}

/// Whether the project's certification verdict says it passed.
pub(crate) fn verdict_passed(p: &CertifiedProject) -> Option<bool> {
    let raw = std::fs::read_to_string(p.verdict.as_ref()?).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    v.get("passed")
        .and_then(|x| x.as_bool())
        .or_else(|| v.get("status").and_then(|s| s.as_str()).map(|s| s.eq_ignore_ascii_case("passed")))
}

/// One template from the project's `templates/*.toml` (id -> summary).
fn template_summaries(dir: &Path) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let Ok(rd) = std::fs::read_dir(dir.join("templates")) else {
        return out;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }
        let Ok(raw) = std::fs::read_to_string(&path) else { continue };
        let Ok(doc) = toml::from_str::<toml::Value>(&raw) else { continue };
        for t in doc.get("template").and_then(|t| t.as_array()).into_iter().flatten() {
            if let (Some(id), Some(sum)) = (
                t.get("id").and_then(|x| x.as_str()),
                t.get("summary").and_then(|x| x.as_str()),
            ) {
                out.insert(id.to_string(), sum.to_string());
            }
        }
    }
    out
}

#[derive(Debug, Clone)]
pub(crate) struct Suggestion {
    pub topic: Option<String>,
    pub summary: Option<String>,
    pub score: f32,
    pub margin: f32,
}

pub(crate) struct Assist {
    brain: String,
    router: Option<SemanticRouter>,
    dir: PathBuf,
    by_capability: BTreeMap<String, Suggestion>,
}

/// The text the brain routes for a capability.
pub(crate) fn capability_prompt(c: &Capability) -> String {
    let words = super::snake(&c.name).replace('_', " ");
    let mut s = words;
    if let Some(sum) = &c.summary {
        s.push_str(": ");
        s.push_str(sum);
    }
    for w in &c.writes {
        s.push_str(&format!(". {} {}", w.effect, w.entity));
    }
    if !c.reads.is_empty() {
        s.push_str(&format!(". reads {}", c.reads.join(", ")));
    }
    s
}

impl Assist {
    pub fn load(gf_toml: &PathBuf) -> Result<Assist, Box<dyn Error>> {
        let project = certified_project(gf_toml)?;
        if verdict_passed(&project) == Some(false) {
            return Err(format!("{}: the routing certification verdict did not pass", project.name).into());
        }
        ensure_encoder()?;
        let router = SemanticRouter::load(&project.paths)?;
        Ok(Assist {
            brain: project.name,
            router: Some(router),
            dir: project.dir,
            by_capability: BTreeMap::new(),
        })
    }

    /// Route every capability in one encoder call.
    pub fn route_all(&mut self, m: &AppModel) -> Result<(), Box<dyn Error>> {
        let Some(router) = &self.router else { return Ok(()) };
        let prompts: Vec<String> = m.capabilities.iter().map(capability_prompt).collect();
        let summaries = template_summaries(&self.dir);
        let decisions = router.infer_batch(&prompts)?;
        for (c, d) in m.capabilities.iter().zip(decisions) {
            self.by_capability.insert(
                c.name.clone(),
                Suggestion {
                    summary: d.topic.as_ref().and_then(|t| summaries.get(t).cloned()),
                    topic: d.topic,
                    score: d.top1,
                    margin: d.margin,
                },
            );
        }
        Ok(())
    }

    #[cfg(test)]
    pub fn from_suggestions(brain: &str, s: BTreeMap<String, Suggestion>) -> Assist {
        Assist { brain: brain.to_string(), router: None, dir: PathBuf::new(), by_capability: s }
    }

    pub fn suggestion(&self, capability: &str) -> Option<&Suggestion> {
        self.by_capability.get(capability)
    }

    /// The comment block written into a handler scaffold.
    pub fn suggestion_comment(&self, c: &Capability) -> String {
        match self.by_capability.get(&c.name) {
            Some(Suggestion { topic: Some(t), summary, score, margin }) => format!(
                "//\n//   suggested by the {} brain (certified routing, score {:.2}, margin {:.2}):\n//     {} — {}\n\
                 //     a suggestion, not generated code; see `spacekit agent code --prompt \"{}\"`\n",
                self.brain,
                score,
                margin,
                t,
                summary.clone().unwrap_or_default(),
                t.replace('_', " ")
            ),
            Some(s) => format!(
                "//\n//   the {} brain abstained for this capability (best score {:.2}): no confident pattern match\n",
                self.brain, s.score
            ),
            None => String::new(),
        }
    }
}

