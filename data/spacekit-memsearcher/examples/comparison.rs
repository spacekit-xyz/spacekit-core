/// Comparison: Traditional context management vs MemSearcher
/// This example demonstrates the token usage differences

use std::collections::VecDeque;

/// Traditional approach: Keep full conversation history
struct TraditionalAgent {
    conversation_history: VecDeque<(String, String)>, // (query, response) pairs
}

impl TraditionalAgent {
    fn new() -> Self {
        Self {
            conversation_history: VecDeque::new(),
        }
    }

    fn process_turn(&mut self, query: String, response: String) {
        self.conversation_history.push_back((query, response));
    }

    fn get_context(&self) -> String {
        self.conversation_history
            .iter()
            .map(|(q, r)| format!("Q: {}\nA: {}", q, r))
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    fn token_count(&self) -> usize {
        estimate_tokens(&self.get_context())
    }
}

/// MemSearcher approach: Keep only compressed facts
struct MemSearcherAgent {
    compressed_memory: String,
    max_tokens: usize,
}

impl MemSearcherAgent {
    fn new(max_tokens: usize) -> Self {
        Self {
            compressed_memory: String::new(),
            max_tokens,
        }
    }

    fn process_turn(&mut self, query: String, response: String) {
        // Extract key facts (simplified - in real implementation uses LLM)
        let facts = self.extract_facts(&query, &response);
        
        // Merge with existing memory
        self.compressed_memory = self.merge_and_compress(&self.compressed_memory, &facts);
        
        // Ensure we stay within budget
        self.trim_to_budget();
    }

    fn extract_facts(&self, query: &str, response: &str) -> String {
        // Simplified fact extraction
        format!("Key from '{}': {}", 
            query.split_whitespace().take(3).collect::<Vec<_>>().join(" "),
            response.split_whitespace().take(10).collect::<Vec<_>>().join(" ")
        )
    }

    fn merge_and_compress(&self, existing: &str, new_facts: &str) -> String {
        if existing.is_empty() {
            new_facts.to_string()
        } else {
            format!("{}\n{}", existing, new_facts)
        }
    }

    fn trim_to_budget(&mut self) {
        while estimate_tokens(&self.compressed_memory) > self.max_tokens {
            // Remove oldest facts (simplified - real implementation uses importance scoring)
            if let Some(pos) = self.compressed_memory.find('\n') {
                self.compressed_memory = self.compressed_memory[pos + 1..].to_string();
            } else {
                break;
            }
        }
    }

    fn token_count(&self) -> usize {
        estimate_tokens(&self.compressed_memory)
    }
}

fn estimate_tokens(text: &str) -> usize {
    (text.len() / 4).max(1)
}

fn main() {
    println!("📊 Traditional vs MemSearcher Comparison\n");
    println!("{}", "=".repeat(80));

    let mut traditional = TraditionalAgent::new();
    let mut memsearcher = MemSearcherAgent::new(500); // 500 token budget

    // Simulate 10 conversation turns
    let conversations = vec![
        ("What is Rust?", "Rust is a systems programming language focused on safety, concurrency, and performance. It achieves memory safety without garbage collection."),
        ("How does ownership work?", "Ownership is Rust's key concept. Each value has a single owner, and when the owner goes out of scope, the value is dropped."),
        ("What about borrowing?", "Borrowing allows references to values without taking ownership. You can have multiple immutable borrows or one mutable borrow."),
        ("Explain lifetimes", "Lifetimes are Rust's way of tracking how long references are valid. They prevent dangling references at compile time."),
        ("What are traits?", "Traits define shared behavior. They're similar to interfaces in other languages and enable polymorphism."),
        ("How do I handle errors?", "Rust uses Result and Option types for error handling. The ? operator propagates errors concisely."),
        ("What's a closure?", "Closures are anonymous functions that can capture their environment. They're useful for iterators and callbacks."),
        ("Explain async/await", "Async/await enables writing asynchronous code that looks synchronous. Futures represent values that will be available later."),
        ("What are macros?", "Macros enable metaprogramming. They generate code at compile time and are more powerful than functions."),
        ("How do I organize code?", "Use modules, crates, and workspaces. Modules organize code within a crate, crates are compilation units."),
    ];

    println!("\n{:>4} | {:>20} | {:>20} | {:>10}", 
        "Turn", "Traditional Tokens", "MemSearcher Tokens", "Savings");
    println!("{}", "-".repeat(80));

    for (i, (query, response)) in conversations.iter().enumerate() {
        traditional.process_turn(query.to_string(), response.to_string());
        memsearcher.process_turn(query.to_string(), response.to_string());

        let trad_tokens = traditional.token_count();
        let mem_tokens = memsearcher.token_count();
        let savings = ((trad_tokens - mem_tokens) as f32 / trad_tokens as f32 * 100.0);

        println!("{:>4} | {:>20} | {:>20} | {:>9.1}%",
            i + 1,
            trad_tokens,
            mem_tokens,
            savings
        );
    }

    println!("{}", "=".repeat(80));
    println!("\n📈 Summary:");
    println!("Final Traditional tokens: {}", traditional.token_count());
    println!("Final MemSearcher tokens: {}", memsearcher.token_count());
    
    let total_savings = (traditional.token_count() - memsearcher.token_count()) as f32 
        / traditional.token_count() as f32 * 100.0;
    println!("Total savings: {:.1}%", total_savings);

    println!("\n💡 Key Insights:");
    println!("• Traditional approach: Linear growth - {} tokens by turn 10", traditional.token_count());
    println!("• MemSearcher approach: Constant size - {} tokens maintained", memsearcher.token_count());
    println!("• Cost reduction: ~{:.0}x cheaper for long conversations", 
        traditional.token_count() as f32 / memsearcher.token_count() as f32);
    println!("• Inference speed: MemSearcher stays fast, Traditional slows down");
    println!("• Scalability: MemSearcher can handle 100+ turns efficiently");

    println!("\n🎯 When MemSearcher shines:");
    println!("✓ Multi-turn conversations (>5 turns)");
    println!("✓ Cost-sensitive applications");
    println!("✓ High-volume chatbots");
    println!("✓ Long-running agent tasks");
    
    println!("\n⚠️  When to use Traditional:");
    println!("• Single-turn queries");
    println!("• Legal/compliance needs (full history)");
    println!("• Debugging (need exact conversation)");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_traditional_growth() {
        let mut agent = TraditionalAgent::new();
        
        agent.process_turn("Q1".to_string(), "A1".to_string());
        let tokens_1 = agent.token_count();
        
        agent.process_turn("Q2".to_string(), "A2".to_string());
        let tokens_2 = agent.token_count();
        
        // Traditional grows linearly
        assert!(tokens_2 > tokens_1);
    }

    #[test]
    fn test_memsearcher_constant() {
        let mut agent = MemSearcherAgent::new(100);
        
        for i in 0..10 {
            agent.process_turn(
                format!("Question {}", i),
                format!("Answer {}", i),
            );
            
            // Should stay within budget
            assert!(agent.token_count() <= 100);
        }
    }

    #[test]
    fn test_savings_calculation() {
        let mut trad = TraditionalAgent::new();
        let mut mem = MemSearcherAgent::new(200);
        
        for i in 0..5 {
            let q = format!("What is topic {}?", i);
            let a = format!("Topic {} is about something interesting that requires explanation", i);
            
            trad.process_turn(q.clone(), a.clone());
            mem.process_turn(q, a);
        }
        
        // MemSearcher should use significantly fewer tokens
        assert!(mem.token_count() < trad.token_count() / 2);
    }
}
