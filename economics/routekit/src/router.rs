//! Task types aligned with spacekit-agent-microgpt and chat UI routing.
//!
//! MicroGPT vocab: 0=search, 1=summarize, 2=classify, 3=code_review, 4=arg_start, 5=arg_end, 6=sep, 7=eos, 8=pad.
//! Kit UI maps microgpt output token to: chat(0,6,7), analyze(1), summarize(2), code_review(3), classify(4), status(5).
//! RouteKit uses the same task labels so clients that use microgpt-router can send `task` and get the right model.

use serde::Deserialize;
use std::str::FromStr;

/// Task type for routing. Matches microgpt tool-call DSL and Kit agent operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskType {
    /// General chat / open-ended (microgpt tokens 0, 6, 7)
    Chat,
    /// Search (microgpt 0) — often routed to fast/retrieval models
    Search,
    /// Summarize (microgpt 1)
    Summarize,
    /// Classify (microgpt 2)
    Classify,
    /// Code review (microgpt 3)
    CodeReview,
    /// Analyze / safety-sentiment (Kit ANALYZE, microgpt 1 in some mappings)
    Analyze,
    /// Status / no-op (microgpt 5)
    Status,
}

impl TaskType {
    /// Prefer cheaper/faster models for simple tasks.
    pub fn prefers_fast(&self) -> bool {
        matches!(
            self,
            TaskType::Classify | TaskType::Status | TaskType::Search
        )
    }
}

impl FromStr for TaskType {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.to_lowercase();
        let s = s.trim();
        Ok(match s {
            "chat" => TaskType::Chat,
            "search" => TaskType::Search,
            "summarize" | "summary" => TaskType::Summarize,
            "classify" | "classification" => TaskType::Classify,
            "code_review" | "code review" | "codereview" => TaskType::CodeReview,
            "analyze" | "analysis" => TaskType::Analyze,
            "status" => TaskType::Status,
            _ => return Err(()),
        })
    }
}

impl<'de> Deserialize<'de> for TaskType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        TaskType::from_str(&s).map_err(|_| serde::de::Error::custom("unknown task type"))
    }
}

/// Result of routing: which provider and model to use.
#[derive(Debug, Clone)]
pub struct RouteDecision {
    pub provider: ProviderKind,
    pub model: String,
    pub task: TaskType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    OpenAI,
    Anthropic,
    Mistral,
}

/// Select provider and model from config + optional task. Uses first available provider;
/// when multiple providers are configured, uses prices to prefer cheaper model for fast tasks and costlier/best for quality tasks.
/// Return provider/model candidates in routing preference order for bounded failover.
pub fn route_candidates(
    providers: &crate::config::ProvidersConfig,
    prices: &crate::prices::ModelPrices,
    task: TaskType,
    preferred_model: Option<&str>,
) -> Vec<RouteDecision> {
    // If client requested a specific model, try to satisfy it if we have that provider configured.
    if let Some(m) = preferred_model {
        if let Some(ref o) = providers.openai {
            if o.models.iter().any(|x| x == m || m.contains(x.as_str())) {
                return vec![RouteDecision {
                    provider: ProviderKind::OpenAI,
                    model: m.to_string(),
                    task,
                }];
            }
        }
        if let Some(ref a) = providers.anthropic {
            if a.models.iter().any(|x| x == m || m.contains(x.as_str())) {
                return vec![RouteDecision {
                    provider: ProviderKind::Anthropic,
                    model: m.to_string(),
                    task,
                }];
            }
        }
        if let Some(ref mst) = providers.mistral {
            if mst.models.iter().any(|x| x == m || m.contains(x.as_str())) {
                return vec![RouteDecision {
                    provider: ProviderKind::Mistral,
                    model: m.to_string(),
                    task,
                }];
            }
        }
        return Vec::new();
    }

    // Build all (provider, model) candidates: one model per configured provider (fast = first, quality = last in list).
    let prefer_fast = task.prefers_fast();
    let pick = |list: &[String], prefer_first: bool| {
        if list.is_empty() {
            None
        } else if prefer_first {
            Some(list[0].clone())
        } else {
            Some(list.last().cloned().unwrap_or_else(|| list[0].clone()))
        }
    };

    let mut candidates: Vec<(ProviderKind, String)> = Vec::new();
    if let Some(ref o) = providers.openai {
        if let Some(model) = pick(&o.models, prefer_fast) {
            candidates.push((ProviderKind::OpenAI, model));
        }
    }
    if let Some(ref a) = providers.anthropic {
        if let Some(model) = pick(&a.models, prefer_fast) {
            candidates.push((ProviderKind::Anthropic, model));
        }
    }
    if let Some(ref m) = providers.mistral {
        if let Some(model) = pick(&m.models, prefer_fast) {
            candidates.push((ProviderKind::Mistral, model));
        }
    }

    if candidates.is_empty() {
        return Vec::new();
    }

    // When we have multiple candidates, sort by nominal cost (100 in, 500 out); prefer cheap for fast, expensive for quality.
    const NOMINAL_IN: u64 = 100;
    const NOMINAL_OUT: u64 = 500;
    if candidates.len() > 1 && !prices.is_empty() {
        candidates.sort_by(|a, b| {
            let cost_a = prices
                .estimate_cost_usd(&a.1, NOMINAL_IN, NOMINAL_OUT)
                .unwrap_or(f64::MAX);
            let cost_b = prices
                .estimate_cost_usd(&b.1, NOMINAL_IN, NOMINAL_OUT)
                .unwrap_or(f64::MAX);
            if prefer_fast {
                cost_a
                    .partial_cmp(&cost_b)
                    .unwrap_or(std::cmp::Ordering::Equal)
            } else {
                cost_b
                    .partial_cmp(&cost_a)
                    .unwrap_or(std::cmp::Ordering::Equal) // quality: prefer higher cost
            }
        });
    }

    candidates
        .into_iter()
        .map(|(provider, model)| RouteDecision {
            provider,
            model,
            task,
        })
        .collect()
}
