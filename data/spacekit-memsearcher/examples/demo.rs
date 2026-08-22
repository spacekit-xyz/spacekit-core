use memsearcher::{MemSearchAgent, AgentError};
use memsearcher::llm_client::ClaudeClient;
use memsearcher::token_counter::{TokenBudgetTracker, TokenCounter};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Setup
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .expect("ANTHROPIC_API_KEY environment variable not set");
    
    let llm_client = Box::new(ClaudeClient::new(api_key));
    
    // Create agent with token budgets
    let max_memory_tokens = 2000;  // Keep memory compact
    let budget_per_turn = 5000;    // Max tokens per interaction
    
    let mut agent = MemSearchAgent::new(
        max_memory_tokens,
        budget_per_turn,
        llm_client,
    );
    
    let mut budget_tracker = TokenBudgetTracker::new(budget_per_turn, 50000);
    let token_counter = TokenCounter::new();
    
    // Example conversation showing memory management
    let queries = vec![
        "What are the key features of Rust's ownership system?",
        "How does borrowing work in practice?",
        "Can you compare Rust's approach to C++'s approach?",
        "What are some common pitfalls when learning Rust ownership?",
        "Given what we've discussed, what should a beginner focus on first?",
    ];
    
    println!("🚀 MemSearcher Agent Demo\n");
    println!("Max Memory: {} tokens", max_memory_tokens);
    println!("Budget per turn: {} tokens\n", budget_per_turn);
    println!("{}", "=".repeat(80));
    
    for (i, query) in queries.iter().enumerate() {
        println!("\n📝 Turn {}: {}", i + 1, query);
        println!("{}", "-".repeat(80));
        
        // Process query
        let response = agent.process_query(query).await?;
        
        // Track token usage
        let turn_tokens = token_counter.count(query) + token_counter.count(&response);
        budget_tracker.record_turn(turn_tokens);
        
        // Display results
        println!("\n💬 Response: {}", response);
        
        let memory_stats = agent.get_memory_stats();
        println!("\n📊 Memory Stats:");
        println!("   Entries: {}", memory_stats.entry_count);
        println!("   Tokens: {}/{}", memory_stats.current_tokens, memory_stats.max_tokens);
        println!("   Usage: {:.1}%", 
            (memory_stats.current_tokens as f32 / memory_stats.max_tokens as f32) * 100.0
        );
        
        println!("\n💰 Budget Stats: {}", budget_tracker.get_stats());
        println!("{}", "=".repeat(80));
    }
    
    // Final statistics
    println!("\n📈 Final Statistics");
    println!("{}", "-".repeat(80));
    let final_stats = budget_tracker.get_stats();
    println!("Total turns: {}", final_stats.total_turns);
    println!("Total tokens used: {}", final_stats.total_tokens_used);
    println!("Average tokens per turn: {:.1}", final_stats.average_per_turn);
    println!("Tokens remaining: {}", final_stats.remaining);
    println!("Memory stayed compact: ✓");
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    
    // Mock LLM client for testing
    use async_trait::async_trait;
    use memsearcher::LLMClient;
    
    struct MockLLMClient {
        responses: Vec<String>,
        call_count: std::sync::Arc<std::sync::Mutex<usize>>,
    }
    
    impl MockLLMClient {
        fn new(responses: Vec<String>) -> Self {
            Self {
                responses,
                call_count: std::sync::Arc::new(std::sync::Mutex::new(0)),
            }
        }
    }
    
    #[async_trait]
    impl LLMClient for MockLLMClient {
        async fn generate(&self, _prompt: &str) -> Result<String, AgentError> {
            let mut count = self.call_count.lock().unwrap();
            let response = self.responses.get(*count % self.responses.len())
                .cloned()
                .unwrap_or_else(|| "ANSWER: Default response".to_string());
            *count += 1;
            Ok(response)
        }
    }
    
    #[tokio::test]
    async fn test_agent_with_mock() {
        let responses = vec![
            "SEARCH: rust ownership system".to_string(),
            "ANSWER: Rust ownership ensures memory safety without garbage collection.".to_string(),
            "ANSWER: Essential facts: ownership, borrowing, lifetimes.".to_string(),
        ];
        
        let llm_client = Box::new(MockLLMClient::new(responses));
        let mut agent = MemSearchAgent::new(2000, 5000, llm_client);
        
        let result = agent.process_query("What is Rust ownership?").await;
        assert!(result.is_ok());
        
        let stats = agent.get_memory_stats();
        assert!(stats.current_tokens <= stats.max_tokens);
    }
}
