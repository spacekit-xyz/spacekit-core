# Quick Start & Configuration Guide

## Installation

### Basic Setup

```bash
# Add to Cargo.toml
cargo add memsearcher tokio --features tokio/full
```

### With Web Integration

```bash
cargo add memsearcher axum tokio --features tokio/full
```

## Configuration Options

### Memory Configuration

```rust
use memsearcher::{MemSearchAgent, ClaudeClient};

// Conservative (low cost, less context)
let agent = MemSearchAgent::new(
    1000,  // max_memory_tokens - very compact
    3000,  // budget_per_turn - smaller responses
    llm_client,
);

// Balanced (recommended for most use cases)
let agent = MemSearchAgent::new(
    2000,  // max_memory_tokens - good balance
    5000,  // budget_per_turn - standard responses
    llm_client,
);

// Generous (higher quality, more expensive)
let agent = MemSearchAgent::new(
    4000,  // max_memory_tokens - rich context
    8000,  // budget_per_turn - detailed responses
    llm_client,
);
```

### Token Budget Planning

Calculate your budget based on usage:

```rust
use memsearcher::TokenBudgetTracker;

// Example: 10,000 users, 10 turns each, 5000 tokens per turn
// Total: 10,000 * 10 * 5,000 = 500M tokens
// At $3/M tokens (Claude): $1,500

let per_user_budget = 10 * 5000; // 10 turns, 5000 each
let tracker = TokenBudgetTracker::new(5000, per_user_budget);
```

### Cost Estimation

| Approach | Tokens/Turn (avg) | 100 Turns | Cost (Claude @$3/M) |
|----------|-------------------|-----------|---------------------|
| Traditional | 3000 → 50,000 | ~2.5M | $7.50 |
| MemSearcher | 2000 (constant) | 200K | $0.60 |
| **Savings** | **92%** | **~12x** | **$6.90** |

## LLM Provider Configuration

### Claude (Anthropic)

```rust
use memsearcher::ClaudeClient;

let api_key = std::env::var("ANTHROPIC_API_KEY")?;
let llm_client = Box::new(
    ClaudeClient::new(api_key)
        .with_model("claude-sonnet-4-20250514".to_string())
);
```

### OpenAI

```rust
use memsearcher::OpenAIClient;

let api_key = std::env::var("OPENAI_API_KEY")?;
let llm_client = Box::new(
    OpenAIClient::new(api_key, "gpt-4".to_string())
);
```

### Custom Provider (e.g., Ollama, local models)

```rust
use async_trait::async_trait;
use memsearcher::{LLMClient, AgentError};

struct OllamaClient {
    base_url: String,
    model: String,
}

#[async_trait]
impl LLMClient for OllamaClient {
    async fn generate(&self, prompt: &str) -> Result<String, AgentError> {
        let client = reqwest::Client::new();
        let response = client
            .post(format!("{}/api/generate", self.base_url))
            .json(&serde_json::json!({
                "model": self.model,
                "prompt": prompt,
            }))
            .send()
            .await
            .map_err(|e| AgentError::LLMError(e.to_string()))?;
            
        // Parse response...
        Ok("Response".to_string())
    }
}
```

## Environment Variables

Create a `.env` file:

```bash
# Required
ANTHROPIC_API_KEY=sk-ant-...

# Optional
MAX_MEMORY_TOKENS=2000
BUDGET_PER_TURN=5000
SESSION_TIMEOUT_SECS=3600
```

Load with `dotenv`:

```rust
use dotenv::dotenv;

#[tokio::main]
async fn main() {
    dotenv().ok();
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .expect("ANTHROPIC_API_KEY not set");
    // ...
}
```

## Performance Tuning

### For High Volume

```rust
// Use smaller memory budget
let agent = MemSearchAgent::new(
    1000,  // Tighter memory
    3000,  // Smaller responses
    llm_client,
);

// Aggressive session cleanup
let config = AgentConfig {
    session_timeout_secs: 600,  // 10 min instead of 1 hour
    ..Default::default()
};
```

### For Quality

```rust
// Use larger memory budget
let agent = MemSearchAgent::new(
    4000,  // More context
    8000,  // Detailed responses
    llm_client,
);

// Use advanced memory features
use memsearcher::advanced_memory::SemanticMemoryManager;
let memory_manager = SemanticMemoryManager::new(4000, llm_client);
```

### For Speed

```rust
// Use faster model
let llm_client = Box::new(
    ClaudeClient::new(api_key)
        .with_model("claude-haiku-3-20250529".to_string()) // Faster model
);

// Smaller token budgets
let agent = MemSearchAgent::new(1500, 3000, llm_client);
```

## Monitoring & Debugging

### Enable Logging

```rust
// Add to Cargo.toml
// env_logger = "0.11"

use env_logger;

#[tokio::main]
async fn main() {
    env_logger::init();
    // Your code
}
```

### Track Metrics

```rust
use memsearcher::TokenBudgetTracker;

let mut tracker = TokenBudgetTracker::new(5000, 100000);

// After each turn
let stats = tracker.get_stats();
println!("Stats: {}", stats);

// Log to your monitoring system
log::info!("tokens_used={} average={}", 
    stats.total_tokens_used,
    stats.average_per_turn
);
```

### Common Issues

#### High Token Usage

```rust
// Check memory stats
let stats = agent.get_memory_stats();
if stats.current_tokens > stats.max_tokens * 0.8 {
    println!("Warning: Memory nearly full");
    // Consider consolidating
}
```

#### Slow Responses

```rust
// Switch to faster model
let llm_client = Box::new(
    ClaudeClient::new(api_key)
        .with_model("claude-haiku-3-20250529".to_string())
);

// Reduce token budget
let agent = MemSearchAgent::new(1000, 3000, llm_client);
```

#### Out of Budget

```rust
let budget_stats = tracker.get_stats();
if budget_stats.remaining < 10000 {
    println!("Warning: Low budget remaining");
    // Notify user or upgrade plan
}
```

## Production Checklist

- [ ] Set appropriate memory limits based on use case
- [ ] Configure session timeouts
- [ ] Set up monitoring and alerts
- [ ] Implement rate limiting
- [ ] Add error handling and retries
- [ ] Configure logging
- [ ] Test with realistic conversation lengths
- [ ] Measure actual token usage
- [ ] Set up cost tracking
- [ ] Implement session persistence (if needed)
- [ ] Add health checks
- [ ] Configure auto-scaling (for web apps)

## Example: Complete Production Setup

```rust
use memsearcher::{MemSearchAgent, ClaudeClient, TokenBudgetTracker};
use std::sync::Arc;
use tokio::sync::RwLock;

struct ProductionAgent {
    agent: MemSearchAgent,
    tracker: TokenBudgetTracker,
    metrics: Arc<RwLock<Metrics>>,
}

struct Metrics {
    total_queries: u64,
    total_tokens: u64,
    errors: u64,
}

impl ProductionAgent {
    async fn process_with_monitoring(&mut self, query: &str) 
        -> Result<String, Box<dyn std::error::Error>> 
    {
        let start = std::time::Instant::now();
        
        // Process query
        let result = self.agent.process_query(query).await;
        
        // Track metrics
        let mut metrics = self.metrics.write().await;
        metrics.total_queries += 1;
        
        match result {
            Ok(response) => {
                let tokens = estimate_tokens(&response);
                metrics.total_tokens += tokens as u64;
                self.tracker.record_turn(tokens);
                
                log::info!(
                    "query_processed duration_ms={} tokens={}",
                    start.elapsed().as_millis(),
                    tokens
                );
                
                Ok(response)
            }
            Err(e) => {
                metrics.errors += 1;
                log::error!("query_failed error={:?}", e);
                Err(Box::new(e))
            }
        }
    }
}

fn estimate_tokens(text: &str) -> usize {
    text.len() / 4
}
```

## Next Steps

1. Run the examples: `cargo run --example demo`
2. Try the comparison: `cargo run --example comparison`
3. Read the full API documentation: `cargo doc --open`
4. Check out the advanced memory features in `src/advanced_memory.rs`
5. Join the community for questions and updates
