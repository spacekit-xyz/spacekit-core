use crate::{CompactMemory, MemoryEntry, AgentError, LLMClient};
use std::collections::HashMap;

/// Advanced memory manager with semantic importance scoring
pub struct SemanticMemoryManager {
    memory: CompactMemory,
    importance_scorer: ImportanceScorer,
    llm_client: Box<dyn LLMClient>,
}

impl SemanticMemoryManager {
    pub fn new(max_tokens: usize, llm_client: Box<dyn LLMClient>) -> Self {
        Self {
            memory: CompactMemory::new(max_tokens),
            importance_scorer: ImportanceScorer::new(),
            llm_client,
        }
    }

    /// Extract and score facts from conversation turn
    pub async fn extract_and_add_facts(
        &mut self,
        query: &str,
        response: &str,
    ) -> Result<(), AgentError> {
        // Use LLM to extract key facts
        let extraction_prompt = format!(
            r#"Extract key facts from this conversation turn. Output each fact on a new line.
Only include information that would be useful for future queries.

Query: {query}
Response: {response}

Facts (one per line):"#
        );

        let facts_text = self.llm_client.generate(&extraction_prompt).await?;
        let facts: Vec<String> = facts_text
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| line.trim().to_string())
            .collect();

        // Score each fact's importance
        for fact in facts {
            let importance = self.score_importance(&fact, query).await?;
            let token_count = estimate_tokens(&fact);

            self.memory.add(MemoryEntry {
                content: fact,
                importance,
                timestamp: get_timestamp(),
                token_count,
            });
        }

        Ok(())
    }

    /// Score fact importance using semantic relevance
    async fn score_importance(&mut self, fact: &str, context: &str) -> Result<f32, AgentError> {
        // Simple heuristic scoring (could be replaced with embeddings)
        let score = self.importance_scorer.score(fact, context);
        Ok(score)
    }

    /// Consolidate similar facts to reduce redundancy
    pub async fn consolidate_facts(&mut self) -> Result<(), AgentError> {
        let facts = self.memory.to_context();
        
        if facts.is_empty() {
            return Ok(());
        }

        let consolidation_prompt = format!(
            r#"Consolidate these facts by removing redundancy while preserving all unique information.
Output consolidated facts, one per line.

Facts:
{facts}

Consolidated facts:"#
        );

        let consolidated = self.llm_client.generate(&consolidation_prompt).await?;
        let token_count = estimate_tokens(&consolidated);

        self.memory.rewrite_from_summary(consolidated, 1.0, token_count);
        Ok(())
    }
}

/// Scores fact importance based on multiple heuristics
pub struct ImportanceScorer {
    keyword_weights: HashMap<String, f32>,
}

impl ImportanceScorer {
    pub fn new() -> Self {
        Self {
            keyword_weights: HashMap::new(),
        }
    }

    pub fn score(&self, fact: &str, context: &str) -> f32 {
        let mut score = 0.5; // Base score

        // Recency bias: more recent facts slightly more important
        score += 0.1;

        // Length heuristic: very short facts might be less informative
        if fact.len() < 20 {
            score -= 0.1;
        }

        // Keyword matching: facts related to context are more important
        let context_words: Vec<&str> = context.split_whitespace().collect();
        let fact_words: Vec<&str> = fact.split_whitespace().collect();
        
        let overlap = context_words
            .iter()
            .filter(|w| fact_words.contains(w))
            .count();
        
        score += (overlap as f32 / context_words.len().max(1) as f32) * 0.3;

        // Clamp between 0 and 1
        score.max(0.0).min(1.0)
    }

    pub fn add_keyword_weight(&mut self, keyword: String, weight: f32) {
        self.keyword_weights.insert(keyword, weight);
    }
}

impl Default for ImportanceScorer {
    fn default() -> Self {
        Self::new()
    }
}

/// Reward-based memory optimization (simplified RL concept)
pub struct RewardBasedMemoryOptimizer {
    memory_states: Vec<MemoryState>,
    cumulative_reward: f32,
}

#[derive(Clone)]
struct MemoryState {
    entries: Vec<MemoryEntry>,
    action: MemoryAction,
    reward: f32,
}

#[derive(Clone, Debug)]
enum MemoryAction {
    Keep,
    Prune,
    Consolidate,
}

impl RewardBasedMemoryOptimizer {
    pub fn new() -> Self {
        Self {
            memory_states: Vec::new(),
            cumulative_reward: 0.0,
        }
    }

    /// Record memory state and associated reward (success of answer)
    pub fn record_turn(&mut self, entries: Vec<MemoryEntry>, action: MemoryAction, reward: f32) {
        self.memory_states.push(MemoryState {
            entries,
            action,
            reward,
        });
        self.cumulative_reward += reward;
    }

    /// Get average reward to assess memory strategy effectiveness
    pub fn get_average_reward(&self) -> f32 {
        if self.memory_states.is_empty() {
            0.0
        } else {
            self.cumulative_reward / self.memory_states.len() as f32
        }
    }

    /// Suggest optimal memory action based on past rewards
    pub fn suggest_action(&self) -> MemoryAction {
        // Count successful outcomes for each action type
        let mut action_rewards: HashMap<String, Vec<f32>> = HashMap::new();

        for state in &self.memory_states {
            let key = format!("{:?}", state.action);
            action_rewards
                .entry(key)
                .or_insert_with(Vec::new)
                .push(state.reward);
        }

        // Find action with highest average reward
        let best_action = action_rewards
            .iter()
            .max_by(|a, b| {
                let avg_a: f32 = a.1.iter().sum::<f32>() / a.1.len() as f32;
                let avg_b: f32 = b.1.iter().sum::<f32>() / b.1.len() as f32;
                avg_a.partial_cmp(&avg_b).unwrap()
            });

        match best_action {
            Some((action_name, _)) if action_name.contains("Prune") => MemoryAction::Prune,
            Some((action_name, _)) if action_name.contains("Consolidate") => {
                MemoryAction::Consolidate
            }
            _ => MemoryAction::Keep,
        }
    }
}

impl Default for RewardBasedMemoryOptimizer {
    fn default() -> Self {
        Self::new()
    }
}

fn estimate_tokens(text: &str) -> usize {
    (text.len() / 4).max(1)
}

fn get_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_importance_scorer() {
        let scorer = ImportanceScorer::new();
        
        let context = "What is Rust ownership?";
        let fact1 = "Rust uses ownership to manage memory";
        let fact2 = "The sky is blue";

        let score1 = scorer.score(fact1, context);
        let score2 = scorer.score(fact2, context);

        assert!(score1 > score2, "Relevant fact should score higher");
    }

    #[test]
    fn test_reward_optimizer() {
        let mut optimizer = RewardBasedMemoryOptimizer::new();
        
        optimizer.record_turn(vec![], MemoryAction::Keep, 0.8);
        optimizer.record_turn(vec![], MemoryAction::Prune, 0.4);
        optimizer.record_turn(vec![], MemoryAction::Keep, 0.9);

        let avg_reward = optimizer.get_average_reward();
        assert!(avg_reward > 0.6);
        
        let suggested = optimizer.suggest_action();
        // Keep should be suggested as it has higher average reward
        assert!(matches!(suggested, MemoryAction::Keep));
    }
}
