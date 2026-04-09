//! Types and data structures for the query engine.

use crate::permissions::{PermissionChoice, PermissionPrompt};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use uuid::Uuid;

/// Cost tracker for API usage
#[derive(Debug, Clone)]
pub struct CostTracker {
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cost_usd: f64,
    pub model_name: String,
}

impl CostTracker {
    /// Create a new cost tracker for a specific model
    pub fn new(model: String) -> Self {
        Self {
            total_input_tokens: 0,
            total_output_tokens: 0,
            total_cost_usd: 0.0,
            model_name: model,
        }
    }

    /// Calculate cost based on model pricing (in USD)
    /// Prices per million tokens as of 2025
    pub fn calculate_cost(model: &str, input_tokens: u64, output_tokens: u64) -> f64 {
        let (input_price_per_mtok, output_price_per_mtok) = match model {
            // Anthropic Claude series
            m if m.contains("claude-3-5-sonnet") || m.contains("claude-sonnet-4") => (3.0, 15.0),
            m if m.contains("claude-3-5-haiku") => (0.80, 4.0),
            m if m.contains("claude-3-opus") => (15.0, 75.0),
            // OpenAI GPT series
            m if m.contains("gpt-4o") => (2.5, 10.0),
            m if m.contains("gpt-4-turbo") => (10.0, 30.0),
            m if m.contains("gpt-3.5-turbo") => (0.5, 1.5),
            // Ollama local models (free)
            m if m.contains("llama") || m.contains("mistral") || m.contains("qwen") => (0.0, 0.0),
            // Default fallback
            _ => (3.0, 15.0),
        };

        let input_cost = (input_tokens as f64 / 1_000_000.0) * input_price_per_mtok;
        let output_cost = (output_tokens as f64 / 1_000_000.0) * output_price_per_mtok;
        input_cost + output_cost
    }

    /// Record usage and update totals
    pub fn record_usage(&mut self, model: &str, input_tokens: u64, output_tokens: u64) {
        self.total_input_tokens += input_tokens;
        self.total_output_tokens += output_tokens;
        self.total_cost_usd += Self::calculate_cost(model, input_tokens, output_tokens);
    }

    /// Get the total cost in USD
    pub fn total_cost(&self) -> f64 {
        self.total_cost_usd
    }

    /// Get a formatted summary of costs
    pub fn summary(&self) -> String {
        format!(
            "Model: {} | Input tokens: {} | Output tokens: {} | Total cost: ${:.6}",
            self.model_name, self.total_input_tokens, self.total_output_tokens, self.total_cost_usd
        )
    }
}

impl Default for CostTracker {
    fn default() -> Self {
        Self::new("claude-sonnet-4-20250514".to_string())
    }
}

/// Permission request for user approval
#[derive(Debug, Clone)]
pub struct PermissionRequest {
    pub prompt: PermissionPrompt,
    pub response_tx: mpsc::UnboundedSender<PermissionChoice>,
}

/// Errors that can occur during query processing
#[derive(thiserror::Error, Debug)]
pub enum QueryError {
    #[error("API error: {0}")]
    ApiError(String),

    #[error("Tool execution error: {0}")]
    ToolError(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("State error: {0}")]
    StateError(String),

    #[error("Invalid query: {0}")]
    InvalidQuery(String),

    #[error("Rate limit exceeded")]
    RateLimitExceeded,

    #[error("Query timeout")]
    Timeout,

    #[error("Configuration error: {0}")]
    ConfigurationError(String),
}

/// Context information for a query
#[derive(Debug, Clone)]
pub struct QueryContext {
    pub query_id: Uuid,
    pub session_id: Uuid,
    pub user_message: String,
    pub metadata: QueryMetadata,
}

/// Additional metadata about the query
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryMetadata {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub tools_allowed: bool,
    pub max_tokens: Option<u32>,
    pub model: String,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
}

/// Configuration for the query engine
#[derive(Debug, Clone)]
pub struct QueryEngineConfig {
    pub max_turns: usize,
    pub max_budget_usd: Option<f64>,
    pub timeout_seconds: u64,
    pub verbose: bool,
    pub enable_thinking: bool,
    /// Maximum context tokens before compression (default: 100K)
    pub max_context_tokens: Option<usize>,
    /// Percentage threshold to trigger compression (0.0-1.0, default: 0.8)
    pub compression_threshold: f32,
    /// Number of recent messages to keep in full during compression
    pub keep_recent_messages: usize,
}

impl Default for QueryEngineConfig {
    fn default() -> Self {
        Self {
            max_turns: 20,
            max_budget_usd: None,
            timeout_seconds: 300,
            verbose: false,
            enable_thinking: false,
            max_context_tokens: Some(100_000),
            compression_threshold: 0.8,
            keep_recent_messages: 10,
        }
    }
}

/// Events emitted during query processing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QueryEvent {
    /// Query started processing
    Started { query_id: Uuid },

    /// Text content from Claude
    Text { query_id: Uuid, content: String },

    /// Tool use requested by Claude
    ToolUseRequest {
        query_id: Uuid,
        tool_use_id: String,
        tool_name: String,
        tool_input: serde_json::Value,
    },

    /// Tool execution completed
    ToolUseResult {
        query_id: Uuid,
        tool_use_id: String,
        tool_name: String,
        result: String,
        is_error: bool,
    },

    /// Turn completed
    TurnCompleted {
        query_id: Uuid,
        turn_number: usize,
        tokens_used: u64,
    },

    /// Query completed successfully
    Completed { query_id: Uuid },

    /// Query failed with error
    Failed { query_id: Uuid, error: String },

    /// Progress update
    Progress { query_id: Uuid, message: String },

    /// Usage statistics
    Usage {
        query_id: Uuid,
        input_tokens: u64,
        output_tokens: u64,
        cost_usd: f64,
    },

    /// Cost summary event
    Cost {
        query_id: Uuid,
        total_cost_usd: f64,
        input_tokens: u64,
        output_tokens: u64,
    },
}

/// Streaming query result
pub type QueryStream =
    std::pin::Pin<Box<dyn futures::stream::Stream<Item = Result<QueryEvent, QueryError>> + Send>>;

/// Statistics about the current conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationStats {
    pub message_count: usize,
    pub turn_count: usize,
    pub total_tokens: u64,
    pub total_cost: f64,
}
