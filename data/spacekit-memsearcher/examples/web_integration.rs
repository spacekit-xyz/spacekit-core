/// Example: Integrating MemSearcher with a web application
/// This shows how to build a stateful conversational API

use memsearcher::{MemSearchAgent, ClaudeClient, TokenBudgetTracker};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Deserialize, Serialize};

/// Configuration for the agent service
#[derive(Clone, Debug)]
pub struct AgentConfig {
    pub max_memory_tokens: usize,
    pub budget_per_turn: usize,
    pub total_budget: usize,
    pub session_timeout_secs: u64,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_memory_tokens: 2000,
            budget_per_turn: 5000,
            total_budget: 100_000,
            session_timeout_secs: 3600, // 1 hour
        }
    }
}

/// Represents a user session with its own agent and budget
pub struct Session {
    agent: MemSearchAgent,
    budget_tracker: TokenBudgetTracker,
    last_active: std::time::Instant,
    user_id: String,
}

impl Session {
    pub fn new(user_id: String, config: &AgentConfig, llm_client: Box<dyn memsearcher::LLMClient>) -> Self {
        Self {
            agent: MemSearchAgent::new(
                config.max_memory_tokens,
                config.budget_per_turn,
                llm_client,
            ),
            budget_tracker: TokenBudgetTracker::new(
                config.budget_per_turn,
                config.total_budget,
            ),
            last_active: std::time::Instant::now(),
            user_id,
        }
    }

    pub fn is_expired(&self, timeout_secs: u64) -> bool {
        self.last_active.elapsed().as_secs() > timeout_secs
    }

    pub fn update_activity(&mut self) {
        self.last_active = std::time::Instant::now();
    }
}

/// Multi-user session manager
pub struct SessionManager {
    sessions: Arc<RwLock<HashMap<String, Session>>>,
    config: AgentConfig,
    api_key: String,
}

impl SessionManager {
    pub fn new(api_key: String, config: AgentConfig) -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            config,
            api_key,
        }
    }

    /// Get or create a session for a user
    pub async fn get_or_create_session(&self, user_id: &str) -> Arc<RwLock<Session>> {
        let mut sessions = self.sessions.write().await;
        
        // Clean up expired sessions
        sessions.retain(|_, session| !session.is_expired(self.config.session_timeout_secs));

        // Get or create session
        if !sessions.contains_key(user_id) {
            let llm_client = Box::new(ClaudeClient::new(self.api_key.clone()));
            let session = Session::new(user_id.to_string(), &self.config, llm_client);
            sessions.insert(user_id.to_string(), session);
        }

        // Clone Arc for the session
        let session = sessions.get(user_id).unwrap();
        Arc::new(RwLock::new(Session {
            agent: MemSearchAgent::new(
                self.config.max_memory_tokens,
                self.config.budget_per_turn,
                Box::new(ClaudeClient::new(self.api_key.clone())),
            ),
            budget_tracker: session.budget_tracker.clone(),
            last_active: session.last_active,
            user_id: session.user_id.clone(),
        }))
    }

    /// Process a query for a specific user
    pub async fn process_query(
        &self,
        user_id: &str,
        query: &str,
    ) -> Result<QueryResponse, String> {
        let mut sessions = self.sessions.write().await;
        
        let session = sessions
            .get_mut(user_id)
            .ok_or_else(|| "Session not found".to_string())?;

        session.update_activity();

        // Process query
        let response = session
            .agent
            .process_query(query)
            .await
            .map_err(|e| format!("Agent error: {:?}", e))?;

        // Track tokens
        let token_count = estimate_tokens(query) + estimate_tokens(&response);
        session.budget_tracker.record_turn(token_count);

        // Get stats
        let memory_stats = session.agent.get_memory_stats();
        let budget_stats = session.budget_tracker.get_stats();

        Ok(QueryResponse {
            response,
            memory_tokens: memory_stats.current_tokens,
            max_memory_tokens: memory_stats.max_tokens,
            turn_tokens: token_count,
            total_tokens_used: budget_stats.total_tokens_used,
            remaining_budget: budget_stats.remaining,
        })
    }

    /// Get statistics for a user's session
    pub async fn get_session_stats(&self, user_id: &str) -> Option<SessionStats> {
        let sessions = self.sessions.read().await;
        
        sessions.get(user_id).map(|session| {
            let memory_stats = session.agent.get_memory_stats();
            let budget_stats = session.budget_tracker.get_stats();

            SessionStats {
                user_id: user_id.to_string(),
                memory_tokens: memory_stats.current_tokens,
                max_memory_tokens: memory_stats.max_tokens,
                total_turns: budget_stats.total_turns,
                total_tokens_used: budget_stats.total_tokens_used,
                average_tokens_per_turn: budget_stats.average_per_turn,
                remaining_budget: budget_stats.remaining,
            }
        })
    }

    /// Clear a user's session (reset memory)
    pub async fn clear_session(&self, user_id: &str) -> bool {
        let mut sessions = self.sessions.write().await;
        sessions.remove(user_id).is_some()
    }

    /// Get all active sessions count
    pub async fn active_sessions_count(&self) -> usize {
        let sessions = self.sessions.read().await;
        sessions.len()
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct QueryResponse {
    pub response: String,
    pub memory_tokens: usize,
    pub max_memory_tokens: usize,
    pub turn_tokens: usize,
    pub total_tokens_used: usize,
    pub remaining_budget: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SessionStats {
    pub user_id: String,
    pub memory_tokens: usize,
    pub max_memory_tokens: usize,
    pub total_turns: usize,
    pub total_tokens_used: usize,
    pub average_tokens_per_turn: f32,
    pub remaining_budget: usize,
}

fn estimate_tokens(text: &str) -> usize {
    (text.len() / 4).max(1)
}

// Example usage with Axum web framework
#[cfg(feature = "axum-example")]
mod axum_example {
    use super::*;
    use axum::{
        extract::{Path, State},
        http::StatusCode,
        response::Json,
        routing::{get, post},
        Router,
    };

    #[derive(Deserialize)]
    struct ChatRequest {
        query: String,
    }

    pub fn create_router(session_manager: Arc<SessionManager>) -> Router {
        Router::new()
            .route("/chat/:user_id", post(chat_handler))
            .route("/stats/:user_id", get(stats_handler))
            .route("/clear/:user_id", post(clear_handler))
            .route("/health", get(health_handler))
            .with_state(session_manager)
    }

    async fn chat_handler(
        State(manager): State<Arc<SessionManager>>,
        Path(user_id): Path<String>,
        Json(request): Json<ChatRequest>,
    ) -> Result<Json<QueryResponse>, StatusCode> {
        manager
            .process_query(&user_id, &request.query)
            .await
            .map(Json)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
    }

    async fn stats_handler(
        State(manager): State<Arc<SessionManager>>,
        Path(user_id): Path<String>,
    ) -> Result<Json<SessionStats>, StatusCode> {
        manager
            .get_session_stats(&user_id)
            .await
            .map(Json)
            .ok_or(StatusCode::NOT_FOUND)
    }

    async fn clear_handler(
        State(manager): State<Arc<SessionManager>>,
        Path(user_id): Path<String>,
    ) -> StatusCode {
        if manager.clear_session(&user_id).await {
            StatusCode::OK
        } else {
            StatusCode::NOT_FOUND
        }
    }

    async fn health_handler(State(manager): State<Arc<SessionManager>>) -> Json<serde_json::Value> {
        let active_sessions = manager.active_sessions_count().await;
        Json(serde_json::json!({
            "status": "healthy",
            "active_sessions": active_sessions
        }))
    }

    #[tokio::main]
    async fn main() -> Result<(), Box<dyn std::error::Error>> {
        let api_key = std::env::var("ANTHROPIC_API_KEY")?;
        let config = AgentConfig::default();
        let manager = Arc::new(SessionManager::new(api_key, config));

        let app = create_router(manager);

        println!("🚀 Server running on http://localhost:3000");
        
        let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
        axum::serve(listener, app).await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_session_manager() {
        let config = AgentConfig {
            max_memory_tokens: 1000,
            budget_per_turn: 2000,
            total_budget: 10000,
            session_timeout_secs: 60,
        };

        let manager = SessionManager::new("test_key".to_string(), config);

        // Create session
        let session = manager.get_or_create_session("user123").await;
        assert_eq!(session.read().await.user_id, "user123");

        // Check active sessions
        let count = manager.active_sessions_count().await;
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn test_session_cleanup() {
        let config = AgentConfig {
            max_memory_tokens: 1000,
            budget_per_turn: 2000,
            total_budget: 10000,
            session_timeout_secs: 1, // 1 second timeout
        };

        let manager = SessionManager::new("test_key".to_string(), config);

        manager.get_or_create_session("user1").await;
        assert_eq!(manager.active_sessions_count().await, 1);

        // Wait for timeout
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        // Trigger cleanup by creating new session
        manager.get_or_create_session("user2").await;
        
        // user1 should be cleaned up
        assert_eq!(manager.active_sessions_count().await, 1);
    }
}
