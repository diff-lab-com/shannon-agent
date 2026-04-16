//! Types and data structures for the query engine.

use crate::permissions::{PermissionChoice, PermissionPrompt};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use uuid::Uuid;

/// Pre-query cost estimate
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostEstimate {
    /// Estimated input tokens
    pub estimated_input_tokens: u64,
    /// Estimated output tokens (based on max_tokens)
    pub estimated_output_tokens: u64,
    /// Estimated cost for this query in USD
    pub estimated_cost_usd: f64,
    /// Session total before this query
    pub session_total_usd: f64,
    /// Projected session total after this query
    pub projected_total_usd: f64,
}

impl std::fmt::Display for CostEstimate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Est: ~{}in+{}out tokens, ~${:.4} (session total: ${:.4}→${:.4})",
            self.estimated_input_tokens,
            self.estimated_output_tokens,
            self.estimated_cost_usd,
            self.session_total_usd,
            self.projected_total_usd
        )
    }
}

/// Cost record for a single turn
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnCost {
    /// Turn number (1-based)
    pub turn: usize,
    /// Model used for this turn
    pub model: String,
    /// Input tokens consumed
    pub input_tokens: u64,
    /// Output tokens generated
    pub output_tokens: u64,
    /// Cost in USD for this turn
    pub cost_usd: f64,
}

/// Accumulated cost for a specific model
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModelCostBreakdown {
    /// Total input tokens for this model
    pub input_tokens: u64,
    /// Total output tokens for this model
    pub output_tokens: u64,
    /// Total cost in USD for this model
    pub cost_usd: f64,
    /// Number of turns using this model
    pub turn_count: usize,
}

/// Cost tracker for API usage with turn-by-turn and per-model breakdown
#[derive(Debug, Clone)]
pub struct CostTracker {
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cost_usd: f64,
    pub model_name: String,
    /// Per-turn cost records
    turn_costs: Vec<TurnCost>,
    /// Per-model accumulated costs
    model_breakdowns: std::collections::HashMap<String, ModelCostBreakdown>,
    /// Optional budget limit in USD
    pub budget_limit_usd: Option<f64>,
}

impl CostTracker {
    /// Create a new cost tracker for a specific model
    pub fn new(model: String) -> Self {
        Self {
            total_input_tokens: 0,
            total_output_tokens: 0,
            total_cost_usd: 0.0,
            model_name: model,
            turn_costs: Vec::new(),
            model_breakdowns: std::collections::HashMap::new(),
            budget_limit_usd: None,
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

    /// Record usage and update totals with turn tracking
    pub fn record_usage(&mut self, model: &str, input_tokens: u64, output_tokens: u64) {
        let cost = Self::calculate_cost(model, input_tokens, output_tokens);
        self.total_input_tokens += input_tokens;
        self.total_output_tokens += output_tokens;
        self.total_cost_usd += cost;

        // Update per-model breakdown
        let entry = self.model_breakdowns.entry(model.to_string())
            .or_default();
        entry.input_tokens += input_tokens;
        entry.output_tokens += output_tokens;
        entry.cost_usd += cost;
        entry.turn_count += 1;
    }

    /// Record usage for a specific turn number
    pub fn record_turn(&mut self, turn: usize, model: &str, input_tokens: u64, output_tokens: u64) {
        let cost = Self::calculate_cost(model, input_tokens, output_tokens);
        self.record_usage(model, input_tokens, output_tokens);
        self.turn_costs.push(TurnCost {
            turn,
            model: model.to_string(),
            input_tokens,
            output_tokens,
            cost_usd: cost,
        });
    }

    /// Get the total cost in USD
    pub fn total_cost(&self) -> f64 {
        self.total_cost_usd
    }

    /// Set a budget limit in USD
    pub fn set_budget(&mut self, limit: f64) {
        self.budget_limit_usd = Some(limit);
    }

    /// Check if the budget has been exceeded
    pub fn is_budget_exceeded(&self) -> bool {
        self.budget_limit_usd
            .map(|limit| self.total_cost_usd >= limit)
            .unwrap_or(false)
    }

    /// Check if the budget usage is above a threshold (0.0-1.0)
    pub fn budget_usage_ratio(&self) -> Option<f64> {
        self.budget_limit_usd
            .map(|limit| self.total_cost_usd / limit)
    }

    /// Get per-turn cost records
    pub fn turn_costs(&self) -> &[TurnCost] {
        &self.turn_costs
    }

    /// Get per-model cost breakdowns
    pub fn model_breakdowns(&self) -> &std::collections::HashMap<String, ModelCostBreakdown> {
        &self.model_breakdowns
    }

    /// Get a formatted summary of costs
    pub fn summary(&self) -> String {
        format!(
            "Model: {} | Input tokens: {} | Output tokens: {} | Total cost: ${:.6}",
            self.model_name, self.total_input_tokens, self.total_output_tokens, self.total_cost_usd
        )
    }

    /// Estimate the cost of a query before sending it.
    ///
    /// Uses a rough token estimation (~4 chars per token) on the conversation
    /// history plus the new message, and assumes the model will use `max_tokens`
    /// output tokens.
    pub fn estimate_query_cost(
        &self,
        model: &str,
        history_chars: usize,
        new_message_chars: usize,
        max_output_tokens: u64,
    ) -> CostEstimate {
        let estimated_input_tokens = ((history_chars + new_message_chars) as f64 / 4.0).ceil() as u64;
        let estimated_cost = Self::calculate_cost(model, estimated_input_tokens, max_output_tokens);
        CostEstimate {
            estimated_input_tokens,
            estimated_output_tokens: max_output_tokens,
            estimated_cost_usd: estimated_cost,
            session_total_usd: self.total_cost_usd,
            projected_total_usd: self.total_cost_usd + estimated_cost,
        }
    }

    /// Get a detailed cost report including per-model breakdown and budget status
    pub fn detailed_report(&self) -> String {
        let mut report = String::new();

        // Header
        report.push_str(&format!(
            "Cost Summary:\n  Total: ${:.4} ({} input + {} output tokens)\n",
            self.total_cost_usd, self.total_input_tokens, self.total_output_tokens,
        ));

        // Budget status
        if let Some(limit) = self.budget_limit_usd {
            let ratio = self.total_cost_usd / limit;
            let status = if ratio >= 1.0 { "EXCEEDED" } else if ratio >= 0.8 { "WARNING" } else { "OK" };
            report.push_str(&format!(
                "  Budget: ${:.4} / ${:.2} ({:.0}% — {status})\n",
                self.total_cost_usd, limit, ratio * 100.0,
            ));
        }

        // Per-model breakdown
        if self.model_breakdowns.len() > 1 {
            report.push_str("  Per-model breakdown:\n");
            let mut models: Vec<_> = self.model_breakdowns.iter().collect();
            models.sort_by(|a, b| b.1.cost_usd.partial_cmp(&a.1.cost_usd).unwrap_or(std::cmp::Ordering::Equal));
            for (model, breakdown) in &models {
                report.push_str(&format!(
                    "    {}: ${:.4} ({} turns, {} in + {} out)\n",
                    model, breakdown.cost_usd, breakdown.turn_count,
                    breakdown.input_tokens, breakdown.output_tokens,
                ));
            }
        }

        // Recent turns (last 5)
        if !self.turn_costs.is_empty() {
            let show = self.turn_costs.len().min(5);
            let start = self.turn_costs.len() - show;
            report.push_str(&format!("  Recent turns (last {show}):\n"));
            for tc in &self.turn_costs[start..] {
                report.push_str(&format!(
                    "    Turn {}: ${:.4} ({} in + {} out, {})\n",
                    tc.turn, tc.cost_usd, tc.input_tokens, tc.output_tokens, tc.model,
                ));
            }
        }

        report
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

/// Strategy for compressing conversation history when approaching token limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompressionStrategy {
    /// Remove oldest messages beyond `keep_recent_messages`, replacing them with
    /// a short summary. This is the default and most conservative strategy.
    #[default]
    SummarizeOld,
    /// Simply drop oldest messages beyond `keep_recent_messages` without
    /// generating a summary. Useful when context window is very small or
    /// when summary overhead is undesirable.
    TruncateOldest,
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
    /// Strategy to use when compressing conversation history
    pub compression_strategy: CompressionStrategy,
    /// System prompt for the LLM (default: coding assistant)
    pub system_prompt: Option<String>,
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
            compression_strategy: CompressionStrategy::default(),
            system_prompt: Some(
                "You are Shannon, an expert coding assistant. You help users with software engineering tasks \
                 including writing code, debugging, refactoring, and explaining code. \
                 Be concise, accurate, and follow best practices. When using tools, prefer the most \
                 direct approach. Always respond in the same language the user uses."
                    .to_string(),
            ),
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

    /// Tool execution progress update
    ToolProgress {
        query_id: Uuid,
        tool_use_id: String,
        tool_name: String,
        progress: f32,
        message: String,
    },

    /// Thinking content from extended thinking mode
    Thinking { query_id: Uuid, content: String },

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cost_tracker_new() {
        let tracker = CostTracker::new("claude-sonnet-4".to_string());
        assert_eq!(tracker.model_name, "claude-sonnet-4");
        assert_eq!(tracker.total_input_tokens, 0);
        assert_eq!(tracker.total_output_tokens, 0);
        assert_eq!(tracker.total_cost_usd, 0.0);
        assert!(tracker.turn_costs().is_empty());
        assert!(tracker.model_breakdowns().is_empty());
        assert!(tracker.budget_limit_usd.is_none());
    }

    #[test]
    fn test_cost_tracker_record_usage() {
        let mut tracker = CostTracker::new("claude-sonnet-4".to_string());
        tracker.record_usage("claude-sonnet-4", 1000, 500);
        assert_eq!(tracker.total_input_tokens, 1000);
        assert_eq!(tracker.total_output_tokens, 500);
        assert!(tracker.total_cost_usd > 0.0);

        // Per-model breakdown should be updated
        let breakdown = tracker.model_breakdowns().get("claude-sonnet-4").unwrap();
        assert_eq!(breakdown.input_tokens, 1000);
        assert_eq!(breakdown.output_tokens, 500);
        assert_eq!(breakdown.turn_count, 1);
    }

    #[test]
    fn test_cost_tracker_record_turn() {
        let mut tracker = CostTracker::new("claude-sonnet-4".to_string());
        tracker.record_turn(1, "claude-sonnet-4", 1000, 500);
        tracker.record_turn(2, "claude-sonnet-4", 2000, 1000);

        assert_eq!(tracker.turn_costs().len(), 2);
        assert_eq!(tracker.turn_costs()[0].turn, 1);
        assert_eq!(tracker.turn_costs()[1].turn, 2);
        assert_eq!(tracker.total_input_tokens, 3000);
        assert_eq!(tracker.total_output_tokens, 1500);
    }

    #[test]
    fn test_cost_tracker_multiple_models() {
        let mut tracker = CostTracker::new("claude-sonnet-4".to_string());
        tracker.record_turn(1, "claude-sonnet-4", 1000, 500);
        tracker.record_turn(2, "gpt-4o", 2000, 1000);
        tracker.record_turn(3, "claude-sonnet-4", 500, 250);

        assert_eq!(tracker.model_breakdowns().len(), 2);

        let sonnet = tracker.model_breakdowns().get("claude-sonnet-4").unwrap();
        assert_eq!(sonnet.turn_count, 2);
        assert_eq!(sonnet.input_tokens, 1500);

        let gpt = tracker.model_breakdowns().get("gpt-4o").unwrap();
        assert_eq!(gpt.turn_count, 1);
        assert_eq!(gpt.input_tokens, 2000);
    }

    #[test]
    fn test_cost_tracker_budget() {
        let mut tracker = CostTracker::new("claude-sonnet-4".to_string());
        assert!(!tracker.is_budget_exceeded());
        assert!(tracker.budget_usage_ratio().is_none());

        tracker.set_budget(1.0);
        assert_eq!(tracker.budget_limit_usd, Some(1.0));
        assert!(!tracker.is_budget_exceeded());
        assert_eq!(tracker.budget_usage_ratio(), Some(0.0));

        // Record enough usage to exceed budget
        tracker.record_usage("claude-sonnet-4", 1_000_000, 500_000);
        // Cost = (1M * 3.0 + 500K * 15.0) / 1M = 3.0 + 7.5 = 10.5
        assert!(tracker.total_cost_usd > 1.0);
        assert!(tracker.is_budget_exceeded());
        assert!(tracker.budget_usage_ratio().unwrap() > 1.0);
    }

    #[test]
    fn test_cost_tracker_detailed_report_single_model() {
        let mut tracker = CostTracker::new("claude-sonnet-4".to_string());
        tracker.record_turn(1, "claude-sonnet-4", 1000, 500);
        let report = tracker.detailed_report();
        assert!(report.contains("Cost Summary"));
        assert!(report.contains("$"));
        // Should not show per-model breakdown with single model
        assert!(!report.contains("Per-model breakdown"));
    }

    #[test]
    fn test_cost_tracker_detailed_report_multi_model() {
        let mut tracker = CostTracker::new("claude-sonnet-4".to_string());
        tracker.record_turn(1, "claude-sonnet-4", 1000, 500);
        tracker.record_turn(2, "gpt-4o", 2000, 1000);
        let report = tracker.detailed_report();
        assert!(report.contains("Per-model breakdown"));
        assert!(report.contains("claude-sonnet-4"));
        assert!(report.contains("gpt-4o"));
    }

    #[test]
    fn test_cost_tracker_detailed_report_with_budget() {
        let mut tracker = CostTracker::new("claude-sonnet-4".to_string());
        tracker.set_budget(10.0);
        tracker.record_turn(1, "claude-sonnet-4", 1000, 500);
        let report = tracker.detailed_report();
        assert!(report.contains("Budget"));
    }

    #[test]
    fn test_cost_tracker_detailed_report_recent_turns() {
        let mut tracker = CostTracker::new("claude-sonnet-4".to_string());
        for i in 1..=7 {
            tracker.record_turn(i, "claude-sonnet-4", 100, 50);
        }
        let report = tracker.detailed_report();
        assert!(report.contains("Recent turns (last 5)"));
        // Should show turns 3-7
        assert!(report.contains("Turn 7"));
        assert!(report.contains("Turn 3"));
    }

    #[test]
    fn test_calculate_cost_claude_sonnet() {
        let cost = CostTracker::calculate_cost("claude-3-5-sonnet-20241022", 1_000_000, 0);
        assert!((cost - 3.0).abs() < 0.001);

        let cost_out = CostTracker::calculate_cost("claude-3-5-sonnet-20241022", 0, 1_000_000);
        assert!((cost_out - 15.0).abs() < 0.001);
    }

    #[test]
    fn test_calculate_cost_gpt4o() {
        let cost = CostTracker::calculate_cost("gpt-4o", 1_000_000, 0);
        assert!((cost - 2.5).abs() < 0.001);

        let cost_out = CostTracker::calculate_cost("gpt-4o", 0, 1_000_000);
        assert!((cost_out - 10.0).abs() < 0.001);
    }

    #[test]
    fn test_calculate_cost_ollama_free() {
        let cost = CostTracker::calculate_cost("llama3", 1_000_000, 1_000_000);
        assert_eq!(cost, 0.0);
    }

    #[test]
    fn test_calculate_cost_unknown_model() {
        let cost = CostTracker::calculate_cost("unknown-model", 1_000_000, 0);
        // Uses default fallback pricing
        assert!((cost - 3.0).abs() < 0.001);
    }

    #[test]
    fn test_cost_tracker_default() {
        let tracker = CostTracker::default();
        assert_eq!(tracker.model_name, "claude-sonnet-4-20250514");
    }

    #[test]
    fn test_cost_tracker_summary() {
        let mut tracker = CostTracker::new("test-model".to_string());
        tracker.record_usage("test-model", 5000, 2000);
        let summary = tracker.summary();
        assert!(summary.contains("test-model"));
        assert!(summary.contains("5000"));
        assert!(summary.contains("2000"));
        assert!(summary.contains("$"));
    }

    #[test]
    fn test_cost_tracker_total_cost() {
        let mut tracker = CostTracker::new("claude-sonnet-4".to_string());
        tracker.record_usage("claude-sonnet-4", 100_000, 50_000);
        let total = tracker.total_cost();
        // 100K * 3.0/1M + 50K * 15.0/1M = 0.3 + 0.75 = 1.05
        assert!((total - 1.05).abs() < 0.01);
    }

    #[test]
    fn test_model_cost_breakdown_default() {
        let breakdown = ModelCostBreakdown::default();
        assert_eq!(breakdown.input_tokens, 0);
        assert_eq!(breakdown.output_tokens, 0);
        assert_eq!(breakdown.cost_usd, 0.0);
        assert_eq!(breakdown.turn_count, 0);
    }

    #[test]
    fn test_turn_cost_serialization() {
        let tc = TurnCost {
            turn: 1,
            model: "claude-sonnet-4".to_string(),
            input_tokens: 1000,
            output_tokens: 500,
            cost_usd: 0.015,
        };
        let json = serde_json::to_string(&tc).unwrap();
        let back: TurnCost = serde_json::from_str(&json).unwrap();
        assert_eq!(back.turn, 1);
        assert_eq!(back.model, "claude-sonnet-4");
        assert_eq!(back.input_tokens, 1000);
    }

    // -- CostEstimate tests --

    #[test]
    fn test_cost_estimate_basic() {
        let tracker = CostTracker::new("claude-3-5-sonnet".to_string());
        let est = tracker.estimate_query_cost("claude-3-5-sonnet", 4000, 100, 4096);
        // 4100 chars / 4 = ~1025 input tokens
        assert_eq!(est.estimated_input_tokens, 1025);
        assert_eq!(est.estimated_output_tokens, 4096);
        assert!(est.estimated_cost_usd > 0.0);
        assert_eq!(est.session_total_usd, 0.0);
        assert_eq!(est.projected_total_usd, est.estimated_cost_usd);
    }

    #[test]
    fn test_cost_estimate_with_existing_session() {
        let mut tracker = CostTracker::new("claude-3-5-sonnet".to_string());
        tracker.record_usage("claude-3-5-sonnet", 10000, 5000);
        let est = tracker.estimate_query_cost("claude-3-5-sonnet", 2000, 200, 2048);
        assert!(est.session_total_usd > 0.0);
        assert!(est.projected_total_usd > est.session_total_usd);
    }

    #[test]
    fn test_cost_estimate_display() {
        let tracker = CostTracker::new("gpt-4o".to_string());
        let est = tracker.estimate_query_cost("gpt-4o", 1000, 100, 2048);
        let display = format!("{est}");
        assert!(display.contains("tokens"));
        assert!(display.contains('$'));
    }

    #[test]
    fn test_cost_estimate_free_model() {
        let tracker = CostTracker::new("llama3".to_string());
        let est = tracker.estimate_query_cost("llama3", 1000, 100, 2048);
        assert_eq!(est.estimated_cost_usd, 0.0);
    }
}
