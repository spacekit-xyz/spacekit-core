/// Token counting utilities for accurate memory management
use std::collections::HashMap;

/// Simple token counter based on character/word heuristics
/// For production, use tiktoken or similar
pub struct TokenCounter {
    chars_per_token: f32,
}

impl TokenCounter {
    pub fn new() -> Self {
        Self {
            chars_per_token: 4.0, // Average for English text
        }
    }

    pub fn count(&self, text: &str) -> usize {
        (text.len() as f32 / self.chars_per_token).ceil() as usize
    }

    pub fn count_messages(&self, messages: &[String]) -> usize {
        messages.iter().map(|m| self.count(m)).sum()
    }
}

impl Default for TokenCounter {
    fn default() -> Self {
        Self::new()
    }
}

/// More sophisticated token counter using word-based estimation
pub struct WordBasedTokenCounter {
    word_to_token_ratio: f32,
}

impl WordBasedTokenCounter {
    pub fn new() -> Self {
        Self {
            word_to_token_ratio: 1.3, // Words are often split into multiple tokens
        }
    }

    pub fn count(&self, text: &str) -> usize {
        let word_count = text.split_whitespace().count();
        (word_count as f32 * self.word_to_token_ratio).ceil() as usize
    }
}

impl Default for WordBasedTokenCounter {
    fn default() -> Self {
        Self::new()
    }
}

/// Token budget tracker for monitoring usage across turns
pub struct TokenBudgetTracker {
    budget_per_turn: usize,
    total_budget: usize,
    used_tokens: Vec<usize>,
    counter: TokenCounter,
}

impl TokenBudgetTracker {
    pub fn new(budget_per_turn: usize, total_budget: usize) -> Self {
        Self {
            budget_per_turn,
            total_budget,
            used_tokens: Vec::new(),
            counter: TokenCounter::new(),
        }
    }

    pub fn record_turn(&mut self, tokens: usize) {
        self.used_tokens.push(tokens);
    }

    pub fn total_used(&self) -> usize {
        self.used_tokens.iter().sum()
    }

    pub fn average_per_turn(&self) -> f32 {
        if self.used_tokens.is_empty() {
            0.0
        } else {
            self.total_used() as f32 / self.used_tokens.len() as f32
        }
    }

    pub fn remaining_budget(&self) -> usize {
        self.total_budget.saturating_sub(self.total_used())
    }

    pub fn is_within_budget(&self, turn_tokens: usize) -> bool {
        turn_tokens <= self.budget_per_turn 
            && self.total_used() + turn_tokens <= self.total_budget
    }

    pub fn get_stats(&self) -> BudgetStats {
        BudgetStats {
            total_turns: self.used_tokens.len(),
            total_tokens_used: self.total_used(),
            average_per_turn: self.average_per_turn(),
            remaining: self.remaining_budget(),
            budget_per_turn: self.budget_per_turn,
            total_budget: self.total_budget,
        }
    }
}

#[derive(Debug, Clone)]
pub struct BudgetStats {
    pub total_turns: usize,
    pub total_tokens_used: usize,
    pub average_per_turn: f32,
    pub remaining: usize,
    pub budget_per_turn: usize,
    pub total_budget: usize,
}

impl std::fmt::Display for BudgetStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Turns: {} | Tokens Used: {}/{} | Avg/Turn: {:.1} | Budget/Turn: {}",
            self.total_turns,
            self.total_tokens_used,
            self.total_budget,
            self.average_per_turn,
            self.budget_per_turn
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_counter() {
        let counter = TokenCounter::new();
        let text = "Hello, world!"; // ~13 chars, ~3-4 tokens
        let count = counter.count(text);
        assert!(count >= 3 && count <= 4);
    }

    #[test]
    fn test_budget_tracker() {
        let mut tracker = TokenBudgetTracker::new(1000, 10000);
        
        tracker.record_turn(500);
        tracker.record_turn(600);
        tracker.record_turn(400);
        
        assert_eq!(tracker.total_used(), 1500);
        assert_eq!(tracker.average_per_turn(), 500.0);
        assert_eq!(tracker.remaining_budget(), 8500);
    }

    #[test]
    fn test_within_budget() {
        let tracker = TokenBudgetTracker::new(1000, 5000);
        
        assert!(tracker.is_within_budget(800));
        assert!(!tracker.is_within_budget(1200));
    }
}
