pub mod llm_client;
pub mod token_counter;
pub mod advanced_memory;

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

pub use llm_client::{ClaudeClient, OpenAIClient};
pub use token_counter::{TokenCounter, TokenBudgetTracker};
pub use advanced_memory::{SemanticMemoryManager, ImportanceScorer, RewardBasedMemoryOptimizer};

/// Represents a single fact or piece of information in memory
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub content: String,
    pub importance: f32,
    pub timestamp: u64,
    pub token_count: usize,
}

/// Compact memory that maintains only essential facts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactMemory {
    entries: VecDeque<MemoryEntry>,
    max_tokens: usize,
    current_tokens: usize,
}

impl CompactMemory {
    pub fn new(max_tokens: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            max_tokens,
            current_tokens: 0,
        }
    }

    /// Add a new memory entry, automatically pruning if needed
    pub fn add(&mut self, entry: MemoryEntry) {
        self.current_tokens += entry.token_count;
        self.entries.push_back(entry);
        self.prune_if_needed();
    }

    /// Remove low-importance entries to stay within token budget
    fn prune_if_needed(&mut self) {
        while self.current_tokens > self.max_tokens && !self.entries.is_empty() {
            // Sort by importance and remove least important
            let mut sorted: Vec<_> = self.entries.iter().enumerate().collect();
            sorted.sort_by(|a, b| a.1.importance.partial_cmp(&b.1.importance).unwrap());
            
            if let Some((idx, _)) = sorted.first() {
                if let Some(removed) = self.entries.remove(*idx) {
                    self.current_tokens -= removed.token_count;
                }
            }
        }
    }

    /// Get all memory as a formatted string for LLM context
    pub fn to_context(&self) -> String {
        self.entries
            .iter()
            .map(|e| e.content.clone())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Rewrite memory based on new information (key MemSearcher feature)
    pub fn rewrite_from_summary(&mut self, summary: String, importance: f32, token_count: usize) {
        // Clear existing entries and replace with compressed summary
        self.entries.clear();
        self.current_tokens = 0;
        
        self.add(MemoryEntry {
            content: summary,
            importance,
            timestamp: self.get_timestamp(),
            token_count,
        });
    }

    /// Merge new facts while maintaining compactness
    pub fn merge_facts(&mut self, facts: Vec<String>, token_estimator: &dyn Fn(&str) -> usize) {
        for fact in facts {
            let tokens = token_estimator(&fact);
            self.add(MemoryEntry {
                content: fact,
                importance: 0.7, // Default importance, could be learned
                timestamp: self.get_timestamp(),
                token_count: tokens,
            });
        }
    }

    fn get_timestamp(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    pub fn token_count(&self) -> usize {
        self.current_tokens
    }
}

/// Agent actions: Search or Answer
#[derive(Debug, Clone)]
pub enum AgentAction {
    Search(String),  // Search query
    Answer(String),  // Direct answer
}

/// Result after agent processes a turn
#[derive(Debug, Clone)]
pub struct TurnResult {
    pub action: AgentAction,
    pub new_memory_summary: Option<String>,
    pub tokens_used: usize,
}

/// The main MemSearcher agent
pub struct MemSearchAgent {
    memory: CompactMemory,
    token_budget_per_turn: usize,
    llm_client: Box<dyn LLMClient>,
}

impl MemSearchAgent {
    pub fn new(
        max_memory_tokens: usize,
        token_budget_per_turn: usize,
        llm_client: Box<dyn LLMClient>,
    ) -> Self {
        Self {
            memory: CompactMemory::new(max_memory_tokens),
            token_budget_per_turn,
            llm_client,
        }
    }

    /// Process a user query using memory + search/answer decision
    pub async fn process_query(&mut self, query: &str) -> Result<String, AgentError> {
        // Step 1: Build context from memory
        let memory_context = self.memory.to_context();
        
        // Step 2: Decide whether to search or answer
        let decision_prompt = self.build_decision_prompt(query, &memory_context);
        let decision = self.llm_client.generate(&decision_prompt).await?;
        
        let action = self.parse_action(&decision)?;
        
        // Step 3: Execute action
        let result = match &action {
            AgentAction::Search(search_query) => {
                let search_results = self.execute_search(search_query).await?;
                
                // Generate answer from search results
                let answer_prompt = self.build_answer_prompt(query, &search_results, &memory_context);
                self.llm_client.generate(&answer_prompt).await?
            }
            AgentAction::Answer(answer) => answer.clone(),
        };
        
        // Step 4: Rewrite memory to compress information
        self.update_memory(query, &result).await?;
        
        Ok(result)
    }

    fn build_decision_prompt(&self, query: &str, memory: &str) -> String {
        format!(
            r#"You are a search agent. Given the query and your memory, decide whether to SEARCH for more information or ANSWER directly.

Memory:
{memory}

Query: {query}

Respond with either:
SEARCH: <search query>
or
ANSWER: <direct answer>
"#
        )
    }

    fn build_answer_prompt(&self, query: &str, search_results: &str, memory: &str) -> String {
        format!(
            r#"Answer the query using the search results and memory.

Memory:
{memory}

Search Results:
{search_results}

Query: {query}

Answer:"#
        )
    }

    async fn update_memory(&mut self, query: &str, answer: &str) -> Result<(), AgentError> {
        let current_memory = self.memory.to_context();
        
        let compress_prompt = format!(
            r#"Given the conversation turn, extract only the essential facts to remember.
Be extremely concise - keep only information needed for future queries.

Previous Memory:
{current_memory}

New Query: {query}
New Answer: {answer}

Output only the essential facts to remember (bullet points):"#
        );
        
        let compressed = self.llm_client.generate(&compress_prompt).await?;
        let token_count = self.estimate_tokens(&compressed);
        
        // Rewrite memory with compressed version
        self.memory.rewrite_from_summary(compressed, 1.0, token_count);
        
        Ok(())
    }

    async fn execute_search(&self, query: &str) -> Result<String, AgentError> {
        // Placeholder - integrate with your actual search implementation
        Ok(format!("Search results for: {}", query))
    }

    fn parse_action(&self, response: &str) -> Result<AgentAction, AgentError> {
        if response.contains("SEARCH:") {
            let query = response
                .split("SEARCH:")
                .nth(1)
                .ok_or(AgentError::ParseError)?
                .trim()
                .to_string();
            Ok(AgentAction::Search(query))
        } else if response.contains("ANSWER:") {
            let answer = response
                .split("ANSWER:")
                .nth(1)
                .ok_or(AgentError::ParseError)?
                .trim()
                .to_string();
            Ok(AgentAction::Answer(answer))
        } else {
            Err(AgentError::ParseError)
        }
    }

    fn estimate_tokens(&self, text: &str) -> usize {
        // Simple estimation: ~4 chars per token
        // Use tiktoken or similar for accurate counting
        text.len() / 4
    }

    pub fn get_memory_stats(&self) -> MemoryStats {
        MemoryStats {
            current_tokens: self.memory.token_count(),
            max_tokens: self.memory.max_tokens,
            entry_count: self.memory.entries.len(),
        }
    }
}

#[derive(Debug)]
pub struct MemoryStats {
    pub current_tokens: usize,
    pub max_tokens: usize,
    pub entry_count: usize,
}

/// Trait for LLM client integration
#[async_trait::async_trait]
pub trait LLMClient: Send + Sync {
    async fn generate(&self, prompt: &str) -> Result<String, AgentError>;
}

#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("Failed to parse LLM response")]
    ParseError,
    #[error("LLM error: {0}")]
    LLMError(String),
    #[error("Search error: {0}")]
    SearchError(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_pruning() {
        let mut memory = CompactMemory::new(100);
        
        memory.add(MemoryEntry {
            content: "Important fact".to_string(),
            importance: 0.9,
            timestamp: 0,
            token_count: 30,
        });
        
        memory.add(MemoryEntry {
            content: "Less important fact".to_string(),
            importance: 0.3,
            timestamp: 0,
            token_count: 40,
        });
        
        memory.add(MemoryEntry {
            content: "Very important fact".to_string(),
            importance: 0.95,
            timestamp: 0,
            token_count: 50,
        });
        
        // Should prune least important entry
        assert!(memory.token_count() <= 100);
        assert!(memory.to_context().contains("Very important"));
    }
}
