//! # Query Engine
//!
//! Main orchestrator for streaming query processing with tool orchestration.

use crate::api::{ClaudeClient, ContentBlock, ContentDelta, Message, MessageContent, StreamEvent, ToolResultContent};
use crate::permissions::{PermissionChoice, PermissionPrompt, PermissionManager};
use crate::state::StateManager;
use crate::tools::{ToolOutput, ToolRegistry};
use futures::stream::{self, Stream, StreamExt};
use serde::{Deserialize, Serialize};
use std::pin::Pin;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::mpsc;
use std::sync::RwLock;
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
            // Claude 3.5 Sonnet
            m if m.contains("claude-3-5-sonnet") || m.contains("claude-sonnet-4-") => (3.0, 15.0),
            // Claude 3.5 Haiku
            m if m.contains("claude-3-5-haiku") => (0.80, 4.0),
            // Claude 3 Opus
            m if m.contains("claude-3-opus") => (15.0, 75.0),
            // Claude 4.x Sonnet (same pricing as 3.5 Sonnet)
            m if m.contains("claude-sonnet-4") => (3.0, 15.0),
            // Default fallback (similar to 3.5 Sonnet)
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
        Self::new("claude-3-5-sonnet-20241022".to_string())
    }
}

/// Permission request for user approval
#[derive(Debug, Clone)]
pub struct PermissionRequest {
    pub prompt: PermissionPrompt,
    pub response_tx: mpsc::UnboundedSender<PermissionChoice>,
}

/// Errors that can occur during query processing
#[derive(Error, Debug)]
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
pub type QueryStream = Pin<Box<dyn Stream<Item = Result<QueryEvent, QueryError>> + Send>>;

/// Conversation state for tracking messages
#[derive(Debug, Clone)]
struct ConversationState {
    messages: Vec<Message>,
    turn_count: usize,
    total_tokens: u64,
    total_cost: f64,
}

impl Default for ConversationState {
    fn default() -> Self {
        Self {
            messages: Vec::new(),
            turn_count: 0,
            total_tokens: 0,
            total_cost: 0.0,
        }
    }
}

impl ConversationState {
    /// Estimate the token count of the current conversation
    /// This is a rough approximation based on character count
    pub fn estimate_tokens(&self) -> usize {
        let mut total_chars = 0;
        for msg in &self.messages {
            // Rough approximation: ~4 chars per token for text
            total_chars += match &msg.content {
                crate::api::MessageContent::Text(text) => text.len(),
                crate::api::MessageContent::Blocks(blocks) => {
                    let mut block_chars = 0;
                    for block in blocks {
                        match block {
                            crate::api::ContentBlock::Text { text } => block_chars += text.len(),
                            crate::api::ContentBlock::ToolUse { name, input, .. } => {
                                block_chars += name.len() + serde_json::to_string(input).map_or(0, |s| s.len())
                            }
                            crate::api::ContentBlock::ToolResult { content, .. } => {
                                if let Some(c) = content {
                                    match c {
                                        crate::api::ToolResultContent::Single(s) => block_chars += s.len(),
                                        crate::api::ToolResultContent::Multiple(blocks) => {
                                            block_chars += blocks.iter().map(|b| match b {
                                                crate::api::ContentBlock::Text { text } => text.len(),
                                                crate::api::ContentBlock::ToolUse { name, input, .. } => {
                                                    name.len() + serde_json::to_string(input).map_or(0, |s| s.len())
                                                }
                                                crate::api::ContentBlock::ToolResult { .. } => 0,
                                                _ => 0,
                                            }).sum::<usize>();
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    block_chars
                }
            };
        }
        // Rough approximation: ~4 characters per token
        total_chars / 4
    }

    /// Check if the conversation needs compression based on config
    pub fn needs_compression(&self, config: &QueryEngineConfig) -> bool {
        if let Some(max_tokens) = config.max_context_tokens {
            let threshold = (max_tokens as f32 * config.compression_threshold) as usize;
            self.estimate_tokens() > threshold
        } else {
            false
        }
    }

    /// Compress the conversation by summarizing older messages
    /// Keeps the most recent messages in full and summarizes older ones
    pub fn compress(&mut self, config: &QueryEngineConfig) {
        if self.messages.len() <= config.keep_recent_messages + 1 {
            return; // Not enough messages to compress
        }

        let keep_count = config.keep_recent_messages;
        let split_point = self.messages.len().saturating_sub(keep_count);

        let old_messages: Vec<Message> = self.messages.drain(..split_point).collect();
        let summary = Self::summarize_messages(&old_messages);

        // Create a summary message as a system message
        let summary_msg = crate::api::Message {
            role: "system".to_string(),
            content: crate::api::MessageContent::Text(
                format!("[Previous conversation summary]\n\n{}", summary)
            ),
        };

        // Insert summary at the beginning
        self.messages.insert(0, summary_msg);
    }

    /// Generate a summary of messages
    fn summarize_messages(messages: &[Message]) -> String {
        let mut summary_parts = Vec::new();
        let mut turn_count = 0;

        for msg in messages {
            match &msg.content {
                crate::api::MessageContent::Text(text) => {
                    let role = if msg.role == "user" { "User" } else { "Assistant" };
                    // Take first 100 chars of each message for the summary
                    let preview = if text.len() > 100 {
                        format!("{}...", &text[..97])
                    } else {
                        text.clone()
                    };
                    summary_parts.push(format!("{}: {}", role, preview));
                    turn_count += 1;
                }
                crate::api::MessageContent::Blocks(blocks) => {
                    let mut tool_uses = Vec::new();
                    for block in blocks {
                        if let crate::api::ContentBlock::ToolUse { name, .. } = block {
                            tool_uses.push(name.clone());
                        } else if let crate::api::ContentBlock::ToolResult { content, .. } = block {
                            if let Some(crate::api::ToolResultContent::Single(result)) = content {
                                summary_parts.push(format!("Tool result: {}",
                                    if result.len() > 80 { format!("{}...", &result[..77]) } else { result.clone() }
                                ));
                            } else if let Some(crate::api::ToolResultContent::Multiple(results)) = content {
                                summary_parts.push(format!("Tool results: {} items", results.len()));
                            }
                        }
                    }
                    if !tool_uses.is_empty() {
                        summary_parts.push(format!("Tools used: {}", tool_uses.join(", ")));
                    }
                }
            }
        }

        format!(
            "Summary of {} turns:\n{}",
            turn_count,
            summary_parts.join("\n")
        )
    }
}

/// Main query engine orchestrator
pub struct QueryEngine {
    client: ClaudeClient,
    tools: Arc<ToolRegistry>,
    permissions: Arc<RwLock<PermissionManager>>,
    state: Arc<StateManager>,
    config: QueryEngineConfig,
    event_tx: mpsc::UnboundedSender<QueryEvent>,
    conversation: ConversationState,
    cost_tracker: Arc<RwLock<CostTracker>>,
}

impl QueryEngine {
    /// Create a new query engine
    pub fn new(
        client: ClaudeClient,
        tools: ToolRegistry,
        permissions: PermissionManager,
        state: StateManager,
        config: QueryEngineConfig,
    ) -> Self {
        let (event_tx, _) = mpsc::unbounded_channel();
        let model = client.model().to_string();
        Self {
            client,
            tools: Arc::new(tools),
            permissions: Arc::new(RwLock::new(permissions)),
            state: Arc::new(state),
            config,
            event_tx,
            conversation: ConversationState::default(),
            cost_tracker: Arc::new(RwLock::new(CostTracker::new(model))),
        }
    }

    /// Create with default configuration
    pub fn with_defaults(
        client: ClaudeClient,
        tools: ToolRegistry,
        permissions: PermissionManager,
        state: StateManager,
    ) -> Self {
        let (event_tx, _) = mpsc::unbounded_channel();
        let model = client.model().to_string();
        Self {
            client,
            tools: Arc::new(tools),
            permissions: Arc::new(RwLock::new(permissions)),
            state: Arc::new(state),
            config: QueryEngineConfig::default(),
            event_tx,
            conversation: ConversationState::default(),
            cost_tracker: Arc::new(RwLock::new(CostTracker::new(model))),
        }
    }

    /// Subscribe to query events
    pub fn subscribe(&self) -> mpsc::UnboundedReceiver<QueryEvent> {
        let (_, event_rx) = mpsc::unbounded_channel();
        event_rx
    }

    /// Emit an event to all subscribers
    fn emit_event(&self, event: QueryEvent) {
        let _ = self.event_tx.send(event);
    }

    /// Add a user message to the conversation
    pub fn add_user_message(&mut self, content: String) {
        use crate::api::MessageContent;
        self.conversation.messages.push(crate::api::Message {
            role: "user".to_string(),
            content: MessageContent::Text(content),
        });
    }

    /// Add an assistant message to the conversation
    pub fn add_assistant_message(&mut self, content: Vec<crate::api::ContentBlock>) {
        use crate::api::{ContentBlock, Message, MessageContent};
        let blocks: Vec<ContentBlock> = content;
        self.conversation.messages.push(Message {
            role: "assistant".to_string(),
            content: MessageContent::Blocks(blocks),
        });
    }

    /// Get the current conversation history
    pub fn conversation_history(&self) -> Vec<Message> {
        self.conversation.messages.clone()
    }

    /// Clear the conversation history
    pub fn clear_conversation(&mut self) {
        self.conversation = ConversationState::default();
    }

    /// Process a query with streaming events
    pub async fn process_query(&self, context: QueryContext, permission_request_tx: Option<mpsc::UnboundedSender<PermissionRequest>>) -> QueryStream {
        let query_id = context.query_id;
        let config = self.config.clone();
        let session_id_for_permissions = context.session_id;

        // Create receiver for events
        let (tx, rx) = mpsc::unbounded_channel();

        // Get necessary state for the spawned task
        let tools = self.tools.clone();
        let permissions = self.permissions.clone();
        let client_api_key = self.client.api_key().to_string();
        let client_model = self.client.model().to_string();
        let client_base_url = self.client.base_url().to_string();
        let client_max_tokens = self.client.max_tokens();
        let user_message = context.user_message.clone();

        // Spawn background task to handle query processing
        tokio::spawn(async move {
            // Create a new client for this task
            let client_config = crate::api::ClaudeClientConfig {
                api_key: client_api_key,
                base_url: client_base_url,
                model: client_model.clone(),
                max_tokens: client_max_tokens,
                ..Default::default()
            };
            let client = ClaudeClient::new(client_config);

            let mut conversation = ConversationState::default();
            conversation.messages.push(Message {
                role: "user".to_string(),
                content: MessageContent::Text(user_message),
            });

            let mut turn = 0;
            let mut tool_results: Vec<(String, String)> = Vec::new();
            let mut permission_requests: Vec<(mpsc::UnboundedReceiver<PermissionChoice>, String, serde_json::Value)> = Vec::new();
            let mut total_input_tokens: u64 = 0;
            let mut total_output_tokens: u64 = 0;

            loop {
                if turn >= config.max_turns {
                    let total_cost = CostTracker::calculate_cost(&client_model, total_input_tokens, total_output_tokens);
                    let _ = tx.send(Ok(QueryEvent::Cost {
                        query_id,
                        total_cost_usd: total_cost,
                        input_tokens: total_input_tokens,
                        output_tokens: total_output_tokens,
                    }));
                    let _ = tx.send(Ok(QueryEvent::Completed { query_id }));
                    break;
                }

                // Build messages for API call
                let mut messages = conversation.messages.clone();

                // Add pending tool results from previous turn
                for (tool_use_id, result_content) in tool_results.drain(..) {
                    messages.push(Message {
                        role: "user".to_string(),
                        content: MessageContent::Blocks(vec![
                            ContentBlock::ToolResult {
                                tool_use_id,
                                content: Some(ToolResultContent::Single(result_content)),
                                is_error: Some(false),
                            }
                        ]),
                    });
                }

                // Get tools schema
                let tools_schema = Some(tools.to_tool_definitions());

                // Call the API
                match client.send_message_stream(messages, tools_schema).await {
                    Ok(mut stream) => {
                        let mut current_tool_use: Option<(String, String)> = None;
                        let mut accumulated_tool_input = String::new();
                        let mut tool_inputs: Vec<(String, String, serde_json::Value)> = Vec::new();
                        let mut has_content = false;

                        // Process streaming events
                        while let Some(event_result) = stream.next().await {
                            match event_result {
                                Ok(stream_event) => {
                                    match stream_event {
                                        StreamEvent::MessageStart { .. } => {}
                                        StreamEvent::ContentBlockStart { content_block, .. } => {
                                            match &content_block {
                                                ContentBlock::ToolUse { id, name, input } => {
                                                    current_tool_use = Some((id.clone(), name.clone()));
                                                    let _ = tx.send(Ok(QueryEvent::ToolUseRequest {
                                                        query_id,
                                                        tool_use_id: id.clone(),
                                                        tool_name: name.clone(),
                                                        tool_input: input.clone(),
                                                    }));
                                                }
                                                _ => {}
                                            }
                                        }
                                        StreamEvent::ContentBlockDelta { delta, .. } => {
                                            match delta {
                                                ContentDelta::TextDelta { text } => {
                                                    has_content = true;
                                                    let _ = tx.send(Ok(QueryEvent::Text {
                                                        query_id,
                                                        content: text,
                                                    }));
                                                }
                                                ContentDelta::InputJsonDelta { partial_json } => {
                                                    accumulated_tool_input.push_str(&partial_json);
                                                }
                                            }
                                        }
                                        StreamEvent::ContentBlockStop { .. } => {
                                            if let Some((id, name)) = current_tool_use.take() {
                                                if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(&accumulated_tool_input) {
                                                    tool_inputs.push((id, name, json_val));
                                                }
                                                accumulated_tool_input.clear();
                                            }
                                        }
                                        StreamEvent::MessageDelta { usage, .. } => {
                                            let input_tokens = usage.input_tokens as u64;
                                            let output_tokens = usage.output_tokens as u64;
                                            let cost_usd = CostTracker::calculate_cost(&client_model, input_tokens, output_tokens);
                                            
                                            total_input_tokens += input_tokens;
                                            total_output_tokens += output_tokens;
                                            
                                            let _ = tx.send(Ok(QueryEvent::Usage {
                                                query_id,
                                                input_tokens,
                                                output_tokens,
                                                cost_usd,
                                            }));

                                            if !tool_inputs.is_empty() {
                                                // Execute tools
                                                for (tool_id, tool_name, tool_input) in tool_inputs.drain(..) {
                                                    let _ = tx.send(Ok(QueryEvent::Progress {
                                                        query_id,
                                                        message: format!("Executing tool: {}", tool_name),
                                                    }));

                                                    // Check if permission is needed
                                                    // Create a scope to ensure the RwLockReadGuard is dropped before await
                                                    let permission_needed = {
                                                        let guard = permissions.read().unwrap();
                                                        guard.create_permission_prompt(&tool_name, &tool_input, session_id_for_permissions)
                                                    };

                                                    if let Some(prompt) = permission_needed {
                                                        // Check if already denied
                                                        if prompt.risk_level == crate::permissions::RiskLevel::Critical {
                                                            // Already denied - skip execution
                                                            let error_msg = format!("Tool denied: {}", prompt.description);
                                                            let _ = tx.send(Ok(QueryEvent::ToolUseResult {
                                                                query_id,
                                                                tool_use_id: tool_id.clone(),
                                                                tool_name,
                                                                result: error_msg.clone(),
                                                                is_error: true,
                                                            }));
                                                            tool_results.push((tool_id, error_msg));
                                                            continue;
                                                        }

                                                        // Send permission request if a channel is provided
                                                        if let Some(ref req_tx) = permission_request_tx {
                                                            let (response_tx, mut response_rx) = mpsc::unbounded_channel();
                                                            let _ = req_tx.send(PermissionRequest {
                                                                prompt: prompt.clone(),
                                                                response_tx,
                                                            });

                                                            // Wait for user response (guard is now dropped, safe to await)
                                                            match response_rx.recv().await {
                                                                Some(crate::permissions::PermissionChoice::Deny) => {
                                                                    let denied_msg = format!("Permission denied: {}", prompt.description);
                                                                    let _ = tx.send(Ok(QueryEvent::ToolUseResult {
                                                                        query_id,
                                                                        tool_use_id: tool_id.clone(),
                                                                        tool_name,
                                                                        result: denied_msg.clone(),
                                                                        is_error: true,
                                                                    }));
                                                                    tool_results.push((tool_id, denied_msg));
                                                                    continue;
                                                                }
                                                                Some(crate::permissions::PermissionChoice::AllowOnce) => {
                                                                    // Execute once - no memory change needed
                                                                }
                                                                Some(crate::permissions::PermissionChoice::AlwaysAllow) => {
                                                                    // Remember for future - update permission manager
                                                                    let _ = permissions.write().unwrap().process_permission_choice(session_id_for_permissions, &prompt, crate::permissions::PermissionChoice::AlwaysAllow);
                                                                }
                                                                None => {
                                                                    let error_msg = "Permission channel closed".to_string();
                                                                    let _ = tx.send(Ok(QueryEvent::ToolUseResult {
                                                                        query_id,
                                                                        tool_use_id: tool_id.clone(),
                                                                        tool_name,
                                                                        result: error_msg.clone(),
                                                                        is_error: true,
                                                                    }));
                                                                    tool_results.push((tool_id, error_msg));
                                                                    continue;
                                                                }
                                                            }
                                                        }
                                                        // If no permission channel, assume auto-allow (for non-interactive contexts)
                                                    }

                                                    match tools.execute(&tool_name, tool_input.clone()).await {
                                                        Ok(output) => {
                                                            let _ = tx.send(Ok(QueryEvent::ToolUseResult {
                                                                query_id,
                                                                tool_use_id: tool_id.clone(),
                                                                tool_name,
                                                                result: output.content.clone(),
                                                                is_error: false,
                                                            }));
                                                            tool_results.push((tool_id, output.content.clone()));
                                                        }
                                                        Err(e) => {
                                                            let error_msg = format!("Tool error: {}", e);
                                                            let _ = tx.send(Ok(QueryEvent::ToolUseResult {
                                                                query_id,
                                                                tool_use_id: tool_id.clone(),
                                                                tool_name,
                                                                result: error_msg.clone(),
                                                                is_error: true,
                                                            }));
                                                            tool_results.push((tool_id, error_msg));
                                                        }
                                                    }
                                                }

                                                turn += 1;
                                                let _ = tx.send(Ok(QueryEvent::TurnCompleted {
                                                    query_id,
                                                    turn_number: turn,
                                                    tokens_used: (usage.input_tokens + usage.output_tokens) as u64,
                                                }));
                                            } else {
                                                let total_cost = CostTracker::calculate_cost(&client_model, total_input_tokens, total_output_tokens);
                                                let _ = tx.send(Ok(QueryEvent::Cost {
                                                    query_id,
                                                    total_cost_usd: total_cost,
                                                    input_tokens: total_input_tokens,
                                                    output_tokens: total_output_tokens,
                                                }));
                                                let _ = tx.send(Ok(QueryEvent::Completed { query_id }));
                                                return;
                                            }
                                        }
                                        StreamEvent::MessageStop => {}
                                        StreamEvent::Ping => {}
                                    }
                                }
                                Err(e) => {
                                    let _ = tx.send(Ok(QueryEvent::Failed {
                                        query_id,
                                        error: e.to_string(),
                                    }));
                                    return;
                                }
                            }
                        }

                        if !has_content && tool_inputs.is_empty() {
                            let total_cost = CostTracker::calculate_cost(&client_model, total_input_tokens, total_output_tokens);
                            let _ = tx.send(Ok(QueryEvent::Cost {
                                query_id,
                                total_cost_usd: total_cost,
                                input_tokens: total_input_tokens,
                                output_tokens: total_output_tokens,
                            }));
                            let _ = tx.send(Ok(QueryEvent::Completed { query_id }));
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(Ok(QueryEvent::Failed {
                            query_id,
                            error: e.to_string(),
                        }));
                    }
                }
            }
        });

        // Convert channel receiver to stream
        let stream = stream::unfold(rx, move |mut receiver| async move {
            match receiver.recv().await {
                Some(event) => Some((event, receiver)),
                None => None,
            }
        });

        Box::pin(stream)
    }

    /// Execute a tool call
    async fn execute_tool(
        &self,
        tool_name: &str,
        tool_input: serde_json::Value,
        context: &QueryContext,
    ) -> Result<ToolOutput, QueryError> {
        // Check permissions first (unwrap is safe - we own the lock)
        if let Err(e) = self
            .permissions
            .read()
            .unwrap()
            .check_tool_permission(context.session_id, tool_name)
        {
            return Err(QueryError::PermissionDenied(e.to_string()));
        }

        // Execute the tool
        self.tools
            .execute(tool_name, tool_input)
            .await
            .map_err(|e| QueryError::ToolError(e.to_string()))
    }

    /// Process a single turn of the conversation
    async fn process_turn(
        &self,
        query_id: Uuid,
        session_id: Uuid,
        turn_number: usize,
    ) -> Result<Vec<QueryEvent>, QueryError> {
        let mut events = Vec::new();

        // Build messages for API call
        let messages = self.conversation.messages.clone();

        // Get tools schema if enabled
        let tools_schema = if self.conversation.messages.len() > 0 {
            Some(self.tools.to_tool_definitions())
        } else {
            None
        };

        // Call the API (stub - would use actual streaming)
        match self.client.send_message_stream(messages, tools_schema).await {
            Ok(mut stream) => {
                // Process streaming events
                while let Some(event_result) = stream.next().await {
                    match event_result {
                        Ok(stream_event) => {
                            match stream_event {
                                StreamEvent::ContentBlockDelta { delta, .. } => {
                                    match delta {
                                        ContentDelta::TextDelta { text } => {
                                            events.push(QueryEvent::Text {
                                                query_id,
                                                content: text,
                                            });
                                        }
                                        ContentDelta::InputJsonDelta { partial_json } => {
                                            // Handle tool input streaming - emit as text for now
                                            events.push(QueryEvent::Text {
                                                query_id,
                                                content: format!("[Tool Input: {}]", partial_json),
                                            });
                                        }
                                    }
                                }
                                StreamEvent::MessageStop => {
                                    events.push(QueryEvent::TurnCompleted {
                                        query_id,
                                        turn_number,
                                        tokens_used: 0,
                                    });
                                }
                                _ => {}
                            }
                        }
                        Err(e) => {
                            events.push(QueryEvent::Failed {
                                query_id,
                                error: e.to_string(),
                            });
                            return Ok(events);
                        }
                    }
                }
            }
            Err(e) => {
                events.push(QueryEvent::Failed {
                    query_id,
                    error: e.to_string(),
                });
            }
        }

        Ok(events)
    }

    /// Validate a query before processing
    fn validate_query(&self, context: &QueryContext) -> Result<(), QueryError> {
        if context.user_message.trim().is_empty() {
            return Err(QueryError::InvalidQuery("Empty message".to_string()));
        }

        if context.metadata.max_tokens == Some(0) {
            return Err(QueryError::InvalidQuery(
                "Invalid max_tokens value".to_string(),
            ));
        }

        Ok(())
    }

    /// Get current conversation statistics
    pub fn conversation_stats(&self) -> ConversationStats {
        ConversationStats {
            message_count: self.conversation.messages.len(),
            turn_count: self.conversation.turn_count,
            total_tokens: self.conversation.total_tokens,
            total_cost: self.conversation.total_cost,
        }
    }
}

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
    use crate::tools::{Tool, ToolOutput};
    use async_trait::async_trait;
    use std::collections::HashMap;

    struct TestTool {
        name: String,
    }

    #[async_trait]
    impl Tool for TestTool {
        fn name(&self) -> &str {
            &self.name
        }

        fn description(&self) -> &str {
            "A test tool"
        }

        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "input": {"type": "string"}
                }
            })
        }

        async fn execute(&self, _input: serde_json::Value) -> Result<ToolOutput, crate::tools::ToolError> {
            Ok(ToolOutput {
                content: "Test executed".to_string(),
                is_error: false,
                metadata: HashMap::new(),
            })
        }
    }

    #[tokio::test]
    async fn test_query_context_creation() {
        let context = QueryContext {
            query_id: Uuid::new_v4(),
            session_id: Uuid::new_v4(),
            user_message: "Hello".to_string(),
            metadata: QueryMetadata {
                timestamp: chrono::Utc::now(),
                tools_allowed: true,
                max_tokens: Some(4096),
                model: "claude-3-5-sonnet-20241022".to_string(),
                temperature: Some(0.7),
                top_p: None,
            },
        };
        assert_eq!(context.user_message, "Hello");
        assert!(context.metadata.tools_allowed);
    }

    #[test]
    fn test_conversation_stats() {
        let stats = ConversationStats {
            message_count: 5,
            turn_count: 2,
            total_tokens: 1000,
            total_cost: 0.01,
        };
        assert_eq!(stats.message_count, 5);
        assert_eq!(stats.turn_count, 2);
    }

    #[test]
    fn test_query_engine_config_default() {
        let config = QueryEngineConfig::default();
        assert_eq!(config.max_turns, 20);
        assert_eq!(config.timeout_seconds, 300);
        assert!(!config.verbose);
        assert_eq!(config.max_context_tokens, Some(100_000));
        assert_eq!(config.compression_threshold, 0.8);
        assert_eq!(config.keep_recent_messages, 10);
    }

    #[test]
    fn test_conversation_token_estimation() {
        let mut conv = ConversationState::default();
        conv.messages.push(crate::api::Message {
            role: "user".to_string(),
            content: crate::api::MessageContent::Text("Hello world".to_string()),
        });
        conv.messages.push(crate::api::Message {
            role: "assistant".to_string(),
            content: crate::api::MessageContent::Text("Hi there!".to_string()),
        });

        let tokens = conv.estimate_tokens();
        // "Hello world" (11) + "Hi there!" (10) = 21 chars / 4 ≈ 5 tokens
        assert!(tokens >= 4 && tokens <= 7);
    }

    #[test]
    fn test_conversation_compression_needed() {
        let config = QueryEngineConfig {
            max_context_tokens: Some(100),
            compression_threshold: 0.8,
            keep_recent_messages: 2,
            ..Default::default()
        };

        let mut conv = ConversationState::default();
        // Add small messages - under threshold
        for _ in 0..5 {
            conv.messages.push(crate::api::Message {
                role: "user".to_string(),
                content: crate::api::MessageContent::Text("Hi".to_string()),
            });
        }

        assert!(!conv.needs_compression(&config));

        // Add many messages - over threshold
        for _ in 0..50 {
            conv.messages.push(crate::api::Message {
                role: "user".to_string(),
                content: crate::api::MessageContent::Text("This is a longer message to increase token count".to_string()),
            });
        }

        assert!(conv.needs_compression(&config));
    }

    #[test]
    fn test_conversation_compress() {
        let config = QueryEngineConfig {
            keep_recent_messages: 2,
            ..Default::default()
        };

        let mut conv = ConversationState::default();
        for i in 0..5 {
            conv.messages.push(crate::api::Message {
                role: "user".to_string(),
                content: crate::api::MessageContent::Text(format!("Message {}", i)),
            });
        }

        let original_count = conv.messages.len();
        conv.compress(&config);

        // Should have summary + 2 recent messages = 3 messages total
        assert_eq!(conv.messages.len(), 3);
        assert!(original_count > conv.messages.len());

        // First message should be a summary
        match &conv.messages[0].content {
            crate::api::MessageContent::Text(text) => {
                assert!(text.contains("[Previous conversation summary]"));
                assert!(text.contains("Summary of"));
            }
            _ => panic!("First message should be a text summary"),
        }
    }

    // CostTracker tests

    #[test]
    fn test_cost_tracker_calculate_cost_sonnet() {
        // Claude 3.5 Sonnet: $3/MTok input, $15/MTok output
        let model = "claude-3-5-sonnet-20241022";
        
        // 1M input tokens = $3.00
        let cost = CostTracker::calculate_cost(model, 1_000_000, 0);
        assert!((cost - 3.0).abs() < 0.001);
        
        // 1M output tokens = $15.00
        let cost = CostTracker::calculate_cost(model, 0, 1_000_000);
        assert!((cost - 15.0).abs() < 0.001);
        
        // 1M input + 1M output = $18.00
        let cost = CostTracker::calculate_cost(model, 1_000_000, 1_000_000);
        assert!((cost - 18.0).abs() < 0.001);
    }

    #[test]
    fn test_cost_tracker_calculate_cost_haiku() {
        // Claude 3.5 Haiku: $0.80/MTok input, $4/MTok output
        let model = "claude-3-5-haiku-20241022";
        
        // 1M input tokens = $0.80
        let cost = CostTracker::calculate_cost(model, 1_000_000, 0);
        assert!((cost - 0.80).abs() < 0.001);
        
        // 1M output tokens = $4.00
        let cost = CostTracker::calculate_cost(model, 0, 1_000_000);
        assert!((cost - 4.0).abs() < 0.001);
        
        // 1M input + 1M output = $4.80
        let cost = CostTracker::calculate_cost(model, 1_000_000, 1_000_000);
        assert!((cost - 4.80).abs() < 0.001);
    }

    #[test]
    fn test_cost_tracker_calculate_cost_opus() {
        // Claude 3 Opus: $15/MTok input, $75/MTok output
        let model = "claude-3-opus-20240229";
        
        // 1M input tokens = $15.00
        let cost = CostTracker::calculate_cost(model, 1_000_000, 0);
        assert!((cost - 15.0).abs() < 0.001);
        
        // 1M output tokens = $75.00
        let cost = CostTracker::calculate_cost(model, 0, 1_000_000);
        assert!((cost - 75.0).abs() < 0.001);
        
        // 1M input + 1M output = $90.00
        let cost = CostTracker::calculate_cost(model, 1_000_000, 1_000_000);
        assert!((cost - 90.0).abs() < 0.001);
    }

    #[test]
    fn test_cost_tracker_calculate_cost_sonnet4() {
        // Claude Sonnet 4: same pricing as 3.5 Sonnet
        let model = "claude-sonnet-4-20250514";
        
        // 1M input + 1M output = $18.00
        let cost = CostTracker::calculate_cost(model, 1_000_000, 1_000_000);
        assert!((cost - 18.0).abs() < 0.001);
    }

    #[test]
    fn test_cost_tracker_calculate_cost_default() {
        // Unknown model should fallback to 3.5 Sonnet pricing
        let model = "unknown-model";
        
        // 1M input + 1M output = $18.00 (default pricing)
        let cost = CostTracker::calculate_cost(model, 1_000_000, 1_000_000);
        assert!((cost - 18.0).abs() < 0.001);
    }

    #[test]
    fn test_cost_tracker_calculate_cost_small_tokens() {
        // Test with small token counts (less than 1M)
        let model = "claude-3-5-sonnet-20241022";
        
        // 1000 input + 500 output
        // Input: 1000/1M * $3 = $0.000003
        // Output: 500/1M * $15 = $0.0000075
        // Total: $0.0000105
        let cost = CostTracker::calculate_cost(model, 1000, 500);
        let expected = (1000.0 / 1_000_000.0) * 3.0 + (500.0 / 1_000_000.0) * 15.0;
        assert!((cost - expected).abs() < 0.000001);
    }

    #[test]
    fn test_cost_tracker_record_usage() {
        let mut tracker = CostTracker::new("claude-3-5-sonnet-20241022".to_string());
        
        // Record first usage
        tracker.record_usage("claude-3-5-sonnet-20241022", 100_000, 50_000);
        assert_eq!(tracker.total_input_tokens, 100_000);
        assert_eq!(tracker.total_output_tokens, 50_000);
        assert!(tracker.total_cost_usd > 0.0);
        
        // Record second usage - should accumulate
        tracker.record_usage("claude-3-5-sonnet-20241022", 200_000, 100_000);
        assert_eq!(tracker.total_input_tokens, 300_000);
        assert_eq!(tracker.total_output_tokens, 150_000);
        assert!(tracker.total_cost_usd > 0.001);
    }

    #[test]
    fn test_cost_tracker_summary() {
        let tracker = CostTracker::new("claude-3-5-haiku".to_string());
        let summary = tracker.summary();
        
        assert!(summary.contains("claude-3-5-haiku"));
        assert!(summary.contains("Input tokens:"));
        assert!(summary.contains("Output tokens:"));
        assert!(summary.contains("Total cost:"));
    }

    #[test]
    fn test_cost_tracker_total_cost() {
        let mut tracker = CostTracker::new("claude-3-opus".to_string());
        
        assert!((tracker.total_cost() - 0.0).abs() < 0.001);
        
        tracker.record_usage("claude-3-opus", 1_000_000, 1_000_000);
        // Opus: $15 input + $75 output = $90 total
        assert!((tracker.total_cost() - 90.0).abs() < 0.001);
    }
}
