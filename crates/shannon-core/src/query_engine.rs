//! # Query Engine
//!
//! Main orchestrator for streaming query processing with tool orchestration.

use crate::api::{LlmClient, ContentBlock, ContentDelta, Message, MessageContent, StreamEvent, ToolResultContent};
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
    client: LlmClient,
    tools: Arc<ToolRegistry>,
    permissions: Arc<RwLock<PermissionManager>>,
    state: Arc<StateManager>,
    config: QueryEngineConfig,
    conversation: ConversationState,
    cost_tracker: Arc<RwLock<CostTracker>>,
}

impl QueryEngine {
    /// Create a new query engine
    pub fn new(
        client: LlmClient,
        tools: ToolRegistry,
        permissions: PermissionManager,
        state: StateManager,
        config: QueryEngineConfig,
    ) -> Self {
        let model = client.model().to_string();
        Self {
            client,
            tools: Arc::new(tools),
            permissions: Arc::new(RwLock::new(permissions)),
            state: Arc::new(state),
            config,
            conversation: ConversationState::default(),
            cost_tracker: Arc::new(RwLock::new(CostTracker::new(model))),
        }
    }

    /// Create with default configuration
    pub fn with_defaults(
        client: LlmClient,
        tools: ToolRegistry,
        permissions: PermissionManager,
        state: StateManager,
    ) -> Self {
        let model = client.model().to_string();
        Self {
            client,
            tools: Arc::new(tools),
            permissions: Arc::new(RwLock::new(permissions)),
            state: Arc::new(state),
            config: QueryEngineConfig::default(),
            conversation: ConversationState::default(),
            cost_tracker: Arc::new(RwLock::new(CostTracker::new(model))),
        }
    }

    /// Get a reference to the tool registry
    pub fn tools(&self) -> &ToolRegistry {
        &self.tools
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
            let client_config = crate::api::LlmClientConfig {
                api_key: client_api_key,
                base_url: client_base_url,
                model: client_model.clone(),
                max_tokens: client_max_tokens,
                ..Default::default()
            };
            let client = LlmClient::new(client_config);

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
                            return;
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

    // OpenAI model cost tests

    #[test]
    fn test_cost_tracker_calculate_cost_gpt4o() {
        // GPT-4o: $2.5/MTok input, $10/MTok output
        let model = "gpt-4o";

        let cost = CostTracker::calculate_cost(model, 1_000_000, 0);
        assert!((cost - 2.5).abs() < 0.001);

        let cost = CostTracker::calculate_cost(model, 0, 1_000_000);
        assert!((cost - 10.0).abs() < 0.001);

        let cost = CostTracker::calculate_cost(model, 1_000_000, 1_000_000);
        assert!((cost - 12.5).abs() < 0.001);
    }

    #[test]
    fn test_cost_tracker_calculate_cost_gpt4turbo() {
        // GPT-4 Turbo: $10/MTok input, $30/MTok output
        let model = "gpt-4-turbo";

        let cost = CostTracker::calculate_cost(model, 1_000_000, 1_000_000);
        assert!((cost - 40.0).abs() < 0.001);
    }

    #[test]
    fn test_cost_tracker_calculate_cost_gpt35turbo() {
        // GPT-3.5 Turbo: $0.5/MTok input, $1.5/MTok output
        let model = "gpt-3.5-turbo";

        let cost = CostTracker::calculate_cost(model, 1_000_000, 1_000_000);
        assert!((cost - 2.0).abs() < 0.001);
    }

    // Ollama local model cost tests (free)

    #[test]
    fn test_cost_tracker_calculate_cost_ollama_llama() {
        // Ollama local models: $0
        let model = "llama3";

        let cost = CostTracker::calculate_cost(model, 1_000_000, 1_000_000);
        assert!((cost - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_cost_tracker_calculate_cost_ollama_mistral() {
        let model = "mistral:7b";

        let cost = CostTracker::calculate_cost(model, 1_000_000, 1_000_000);
        assert!((cost - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_cost_tracker_calculate_cost_ollama_qwen() {
        let model = "qwen:72b";

        let cost = CostTracker::calculate_cost(model, 1_000_000, 1_000_000);
        assert!((cost - 0.0).abs() < 0.001);
    }

    // Mixed model cost tracking

    #[test]
    fn test_cost_tracker_mixed_models() {
        let mut tracker = CostTracker::new("claude-sonnet-4".to_string());

        // Claude Sonnet 4: $3 + $15 = $18
        tracker.record_usage("claude-sonnet-4-20250514", 1_000_000, 1_000_000);

        // GPT-4o: $2.5 + $10 = $12.50
        tracker.record_usage("gpt-4o", 1_000_000, 1_000_000);

        // Ollama: $0
        tracker.record_usage("llama3:70b", 1_000_000, 1_000_000);

        // Total: $18 + $12.50 + $0 = $30.50
        assert!((tracker.total_cost() - 30.5).abs() < 0.001);
        assert_eq!(tracker.total_input_tokens, 3_000_000);
        assert_eq!(tracker.total_output_tokens, 3_000_000);
    }

    #[test]
    fn test_cost_tracker_zero_tokens() {
        let model = "claude-sonnet-4";
        let cost = CostTracker::calculate_cost(model, 0, 0);
        assert!((cost - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_cost_tracker_summary_includes_model_name() {
        let mut tracker = CostTracker::new("gpt-4o".to_string());
        tracker.record_usage("gpt-4o", 500_000, 250_000);

        let summary = tracker.summary();
        assert!(summary.contains("gpt-4o"));
        assert!(summary.contains("500000"));
        assert!(summary.contains("250000"));
    }

    // ── QueryError display tests ────────────────────────────────────────

    #[test]
    fn test_query_error_display_messages() {
        let err = QueryError::ApiError("rate limited".to_string());
        assert!(err.to_string().contains("API error"));
        assert!(err.to_string().contains("rate limited"));

        let err = QueryError::ToolError("bash failed".to_string());
        assert!(err.to_string().contains("Tool execution error"));

        let err = QueryError::PermissionDenied("read blocked".to_string());
        assert!(err.to_string().contains("Permission denied"));

        let err = QueryError::StateError("session lost".to_string());
        assert!(err.to_string().contains("State error"));

        let err = QueryError::InvalidQuery("empty".to_string());
        assert!(err.to_string().contains("Invalid query"));

        let err = QueryError::RateLimitExceeded;
        assert!(err.to_string().contains("Rate limit"));

        let err = QueryError::Timeout;
        assert!(err.to_string().contains("timeout"));

        let err = QueryError::ConfigurationError("bad key".to_string());
        assert!(err.to_string().contains("Configuration error"));
    }

    // ── QueryEvent variant construction tests ────────────────────────────

    #[test]
    fn test_query_event_started() {
        let id = Uuid::new_v4();
        let event = QueryEvent::Started { query_id: id };
        match event {
            QueryEvent::Started { query_id } => assert_eq!(query_id, id),
            _ => panic!("Expected Started variant"),
        }
    }

    #[test]
    fn test_query_event_text() {
        let id = Uuid::new_v4();
        let event = QueryEvent::Text { query_id: id, content: "Hello world".to_string() };
        match event {
            QueryEvent::Text { content, .. } => assert_eq!(content, "Hello world"),
            _ => panic!("Expected Text variant"),
        }
    }

    #[test]
    fn test_query_event_tool_use_request() {
        let id = Uuid::new_v4();
        let event = QueryEvent::ToolUseRequest {
            query_id: id,
            tool_use_id: "tool_123".to_string(),
            tool_name: "bash".to_string(),
            tool_input: serde_json::json!({"command": "ls"}),
        };
        match event {
            QueryEvent::ToolUseRequest { tool_name, tool_input, .. } => {
                assert_eq!(tool_name, "bash");
                assert_eq!(tool_input["command"], "ls");
            }
            _ => panic!("Expected ToolUseRequest variant"),
        }
    }

    #[test]
    fn test_query_event_tool_use_result() {
        let id = Uuid::new_v4();
        let event = QueryEvent::ToolUseResult {
            query_id: id,
            tool_use_id: "tool_456".to_string(),
            tool_name: "read".to_string(),
            result: "file contents".to_string(),
            is_error: false,
        };
        match event {
            QueryEvent::ToolUseResult { result, is_error, .. } => {
                assert_eq!(result, "file contents");
                assert!(!is_error);
            }
            _ => panic!("Expected ToolUseResult variant"),
        }
    }

    #[test]
    fn test_query_event_tool_use_result_error() {
        let event = QueryEvent::ToolUseResult {
            query_id: Uuid::new_v4(),
            tool_use_id: "t1".to_string(),
            tool_name: "bash".to_string(),
            result: "permission denied".to_string(),
            is_error: true,
        };
        match event {
            QueryEvent::ToolUseResult { is_error, .. } => assert!(is_error),
            _ => panic!("Expected ToolUseResult variant"),
        }
    }

    #[test]
    fn test_query_event_turn_completed() {
        let id = Uuid::new_v4();
        let event = QueryEvent::TurnCompleted {
            query_id: id,
            turn_number: 3,
            tokens_used: 1500,
        };
        match event {
            QueryEvent::TurnCompleted { turn_number, tokens_used, .. } => {
                assert_eq!(turn_number, 3);
                assert_eq!(tokens_used, 1500);
            }
            _ => panic!("Expected TurnCompleted variant"),
        }
    }

    #[test]
    fn test_query_event_completed() {
        let id = Uuid::new_v4();
        let event = QueryEvent::Completed { query_id: id };
        assert!(matches!(event, QueryEvent::Completed { query_id: _ }));
    }

    #[test]
    fn test_query_event_failed() {
        let id = Uuid::new_v4();
        let event = QueryEvent::Failed { query_id: id, error: "timeout".to_string() };
        match event {
            QueryEvent::Failed { error, .. } => assert_eq!(error, "timeout"),
            _ => panic!("Expected Failed variant"),
        }
    }

    #[test]
    fn test_query_event_progress() {
        let id = Uuid::new_v4();
        let event = QueryEvent::Progress { query_id: id, message: "Processing...".to_string() };
        match event {
            QueryEvent::Progress { message, .. } => assert_eq!(message, "Processing..."),
            _ => panic!("Expected Progress variant"),
        }
    }

    #[test]
    fn test_query_event_usage() {
        let id = Uuid::new_v4();
        let event = QueryEvent::Usage {
            query_id: id,
            input_tokens: 1000,
            output_tokens: 500,
            cost_usd: 0.015,
        };
        match event {
            QueryEvent::Usage { input_tokens, output_tokens, cost_usd, .. } => {
                assert_eq!(input_tokens, 1000);
                assert_eq!(output_tokens, 500);
                assert!((cost_usd - 0.015).abs() < 0.0001);
            }
            _ => panic!("Expected Usage variant"),
        }
    }

    #[test]
    fn test_query_event_cost() {
        let id = Uuid::new_v4();
        let event = QueryEvent::Cost {
            query_id: id,
            total_cost_usd: 1.23,
            input_tokens: 50000,
            output_tokens: 25000,
        };
        match event {
            QueryEvent::Cost { total_cost_usd, input_tokens, output_tokens, .. } => {
                assert!((total_cost_usd - 1.23).abs() < 0.001);
                assert_eq!(input_tokens, 50000);
                assert_eq!(output_tokens, 25000);
            }
            _ => panic!("Expected Cost variant"),
        }
    }

    // ── QueryMetadata serialization tests ────────────────────────────────

    #[test]
    fn test_query_metadata_serialization_roundtrip() {
        let metadata = QueryMetadata {
            timestamp: chrono::Utc::now(),
            tools_allowed: true,
            max_tokens: Some(8192),
            model: "claude-sonnet-4-20250514".to_string(),
            temperature: Some(0.7),
            top_p: Some(0.95),
        };
        let json = serde_json::to_string(&metadata).unwrap();
        let deserialized: QueryMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.tools_allowed, true);
        assert_eq!(deserialized.max_tokens, Some(8192));
        assert_eq!(deserialized.model, "claude-sonnet-4-20250514");
        assert_eq!(deserialized.temperature, Some(0.7));
        assert_eq!(deserialized.top_p, Some(0.95));
    }

    #[test]
    fn test_query_metadata_serialization_none_fields() {
        let metadata = QueryMetadata {
            timestamp: chrono::Utc::now(),
            tools_allowed: false,
            max_tokens: None,
            model: "gpt-4o".to_string(),
            temperature: None,
            top_p: None,
        };
        let json = serde_json::to_string(&metadata).unwrap();
        let deserialized: QueryMetadata = serde_json::from_str(&json).unwrap();
        assert!(!deserialized.tools_allowed);
        assert!(deserialized.max_tokens.is_none());
        assert!(deserialized.temperature.is_none());
        assert!(deserialized.top_p.is_none());
    }

    // ── ConversationState edge case tests ───────────────────────────────

    #[test]
    fn test_conversation_state_default() {
        let conv = ConversationState::default();
        assert!(conv.messages.is_empty());
        assert_eq!(conv.turn_count, 0);
        assert_eq!(conv.total_tokens, 0);
        assert!((conv.total_cost - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_conversation_estimate_tokens_empty() {
        let conv = ConversationState::default();
        assert_eq!(conv.estimate_tokens(), 0);
    }

    #[test]
    fn test_conversation_estimate_tokens_blocks_content() {
        use crate::api::{ContentBlock, MessageContent, ToolResultContent};
        let mut conv = ConversationState::default();

        // Message with Blocks content containing Text block
        conv.messages.push(crate::api::Message {
            role: "assistant".to_string(),
            content: MessageContent::Blocks(vec![
                ContentBlock::Text { text: "Hello from block".to_string() },
            ]),
        });

        let tokens = conv.estimate_tokens();
        // "Hello from block" = 16 chars / 4 = 4 tokens
        assert!(tokens >= 3 && tokens <= 6);
    }

    #[test]
    fn test_conversation_estimate_tokens_tool_use_block() {
        use crate::api::{ContentBlock, MessageContent};
        let mut conv = ConversationState::default();

        conv.messages.push(crate::api::Message {
            role: "assistant".to_string(),
            content: MessageContent::Blocks(vec![
                ContentBlock::ToolUse {
                    id: "tu_1".to_string(),
                    name: "bash".to_string(),
                    input: serde_json::json!({"command": "ls -la"}),
                },
            ]),
        });

        let tokens = conv.estimate_tokens();
        // "bash" (4) + JSON serialization of input (~20 chars) / 4 ≈ 6 tokens
        assert!(tokens > 0);
    }

    #[test]
    fn test_conversation_compress_empty_does_nothing() {
        let config = QueryEngineConfig::default();
        let mut conv = ConversationState::default();
        conv.compress(&config);
        assert!(conv.messages.is_empty());
    }

    #[test]
    fn test_conversation_compress_few_messages_no_change() {
        let config = QueryEngineConfig {
            keep_recent_messages: 5,
            ..Default::default()
        };
        let mut conv = ConversationState::default();
        // Only 4 messages, but keep_recent_messages = 5, so no compression
        for i in 0..4 {
            conv.messages.push(crate::api::Message {
                role: "user".to_string(),
                content: crate::api::MessageContent::Text(format!("Msg {}", i)),
            });
        }
        conv.compress(&config);
        // 4 messages <= keep_recent_messages(5) + 1 = 6, so no compression
        assert_eq!(conv.messages.len(), 4);
    }

    #[test]
    fn test_conversation_compress_exactly_threshold() {
        let config = QueryEngineConfig {
            keep_recent_messages: 2,
            ..Default::default()
        };
        let mut conv = ConversationState::default();
        // keep_recent + 1 = 3, so 4 messages should compress
        for i in 0..4 {
            conv.messages.push(crate::api::Message {
                role: "user".to_string(),
                content: crate::api::MessageContent::Text(format!("Message {}", i)),
            });
        }
        conv.compress(&config);
        // summary + 2 kept = 3
        assert_eq!(conv.messages.len(), 3);
        // First message is the summary
        match &conv.messages[0].content {
            crate::api::MessageContent::Text(text) => {
                assert!(text.contains("[Previous conversation summary]"));
            }
            _ => panic!("Expected text content"),
        }
        // Last 2 messages are the kept ones
        match &conv.messages[2].content {
            crate::api::MessageContent::Text(text) => {
                assert_eq!(text, "Message 3");
            }
            _ => panic!("Expected text content"),
        }
    }

    #[test]
    fn test_conversation_needs_compression_no_limit() {
        let config = QueryEngineConfig {
            max_context_tokens: None,
            ..Default::default()
        };
        let mut conv = ConversationState::default();
        for _ in 0..1000 {
            conv.messages.push(crate::api::Message {
                role: "user".to_string(),
                content: crate::api::MessageContent::Text("A very long message".repeat(100)),
            });
        }
        // No max_context_tokens set → never needs compression
        assert!(!conv.needs_compression(&config));
    }

    #[test]
    fn test_conversation_needs_compression_under_threshold() {
        let config = QueryEngineConfig {
            max_context_tokens: Some(10000),
            compression_threshold: 0.8,
            ..Default::default()
        };
        let mut conv = ConversationState::default();
        // A few short messages → well under 8000 token threshold
        conv.messages.push(crate::api::Message {
            role: "user".to_string(),
            content: crate::api::MessageContent::Text("Hi".to_string()),
        });
        assert!(!conv.needs_compression(&config));
    }

    // ── CostTracker edge cases ──────────────────────────────────────────

    #[test]
    fn test_cost_tracker_new_initializes_zero() {
        let tracker = CostTracker::new("claude-3-5-sonnet".to_string());
        assert_eq!(tracker.total_input_tokens, 0);
        assert_eq!(tracker.total_output_tokens, 0);
        assert!((tracker.total_cost_usd - 0.0).abs() < 0.001);
        assert_eq!(tracker.model_name, "claude-3-5-sonnet");
    }

    #[test]
    fn test_cost_tracker_default_is_sonnet() {
        let tracker = CostTracker::default();
        assert_eq!(tracker.model_name, "claude-sonnet-4-20250514");
    }

    #[test]
    fn test_cost_tracker_accumulates_correctly() {
        let mut tracker = CostTracker::new("gpt-4o".to_string());

        // Three separate recordings
        tracker.record_usage("gpt-4o", 100_000, 50_000);
        tracker.record_usage("gpt-4o", 200_000, 100_000);
        tracker.record_usage("gpt-4o", 300_000, 150_000);

        assert_eq!(tracker.total_input_tokens, 600_000);
        assert_eq!(tracker.total_output_tokens, 300_000);

        // GPT-4o: input $2.5/MTok, output $10/MTok
        // (600K/1M * $2.5) + (300K/1M * $10) = $1.50 + $3.00 = $4.50
        let expected = (600_000.0 / 1_000_000.0) * 2.5 + (300_000.0 / 1_000_000.0) * 10.0;
        assert!((tracker.total_cost() - expected).abs() < 0.001);
    }

    #[test]
    fn test_calculate_cost_case_sensitivity() {
        // Model matching uses .contains() which is case-sensitive
        let cost_lower = CostTracker::calculate_cost("claude-3-5-sonnet", 1_000_000, 0);
        let cost_mixed = CostTracker::calculate_cost("Claude-3-5-Sonnet", 1_000_000, 0);
        // Mixed case won't match → falls back to default pricing ($3/MTok)
        // which happens to be the same as sonnet pricing
        assert!((cost_lower - 3.0).abs() < 0.001);
        assert!((cost_mixed - 3.0).abs() < 0.001);
    }

    #[test]
    fn test_calculate_cost_model_with_prefix() {
        // "gpt-4o-mini" contains "gpt-4o" → should match GPT-4o pricing
        let cost = CostTracker::calculate_cost("gpt-4o-mini", 1_000_000, 1_000_000);
        assert!((cost - 12.5).abs() < 0.001);
    }

    // ── QueryEngineConfig edge cases ────────────────────────────────────

    #[test]
    fn test_query_engine_config_custom() {
        let config = QueryEngineConfig {
            max_turns: 5,
            max_budget_usd: Some(1.0),
            timeout_seconds: 60,
            verbose: true,
            enable_thinking: false,
            max_context_tokens: Some(50_000),
            compression_threshold: 0.6,
            keep_recent_messages: 5,
        };
        assert_eq!(config.max_turns, 5);
        assert_eq!(config.max_budget_usd, Some(1.0));
        assert_eq!(config.timeout_seconds, 60);
        assert!(config.verbose);
        assert!(!config.enable_thinking);
        assert_eq!(config.max_context_tokens, Some(50_000));
        assert!((config.compression_threshold - 0.6).abs() < 0.001);
    }

    // ── ConversationStats tests ─────────────────────────────────────────

    #[test]
    fn test_conversation_stats_debug() {
        let stats = ConversationStats {
            message_count: 10,
            turn_count: 5,
            total_tokens: 5000,
            total_cost: 0.25,
        };
        let debug_str = format!("{:?}", stats);
        assert!(debug_str.contains("message_count"));
        assert!(debug_str.contains("turn_count"));
    }

    #[test]
    fn test_conversation_stats_clone() {
        let stats = ConversationStats {
            message_count: 3,
            turn_count: 1,
            total_tokens: 500,
            total_cost: 0.01,
        };
        let cloned = stats.clone();
        assert_eq!(cloned.message_count, stats.message_count);
        assert_eq!(cloned.turn_count, stats.turn_count);
        assert_eq!(cloned.total_tokens, stats.total_tokens);
    }

    // ── QueryContext tests ──────────────────────────────────────────────

    #[test]
    fn test_query_context_debug() {
        let ctx = QueryContext {
            query_id: Uuid::new_v4(),
            session_id: Uuid::new_v4(),
            user_message: "test query".to_string(),
            metadata: QueryMetadata {
                timestamp: chrono::Utc::now(),
                tools_allowed: false,
                max_tokens: None,
                model: "test-model".to_string(),
                temperature: None,
                top_p: None,
            },
        };
        let debug_str = format!("{:?}", ctx);
        assert!(debug_str.contains("test query"));
    }

    // ── QueryStream type alias test ─────────────────────────────────────

    #[test]
    fn test_query_stream_is_send() {
        // Verify the type alias compiles and is Send
        fn assert_send<T: Send>() {}
        assert_send::<QueryStream>();
    }

    // ── ConversationState compress edge cases ──────────────────────────

    #[test]
    fn test_conversation_compress_minimum_messages() {
        let mut state = ConversationState::default();
        let config = QueryEngineConfig {
            keep_recent_messages: 3,
            ..QueryEngineConfig::default()
        };

        // Add exactly keep_recent_messages + 2 messages (5 total, above threshold of 4)
        for i in 0..5 {
            state.messages.push(Message {
                role: "user".to_string(),
                content: MessageContent::Blocks(vec![ContentBlock::Text {
                    text: format!("Message number {}", i),
                }]),
            });
        }

        let before = state.messages.len();
        state.compress(&config);
        assert!(state.messages.len() < before);
    }

    #[test]
    fn test_conversation_compress_preserves_recent_order() {
        let mut state = ConversationState::default();
        let config = QueryEngineConfig {
            keep_recent_messages: 2,
            ..QueryEngineConfig::default()
        };

        for i in 0..4 {
            state.messages.push(Message {
                role: "user".to_string(),
                content: MessageContent::Blocks(vec![ContentBlock::Text {
                    text: format!("Msg {}", i),
                }]),
            });
        }

        state.compress(&config);

        let len = state.messages.len();
        // Last 2 messages should be "Msg 2" and "Msg 3" (indices 2 and 3 from 0..4)
        if let MessageContent::Blocks(blocks) = &state.messages[len - 2].content {
            if let ContentBlock::Text { text: t1 } = &blocks[0] {
                assert!(t1.contains("Msg 2"));
            }
        }
        if let MessageContent::Blocks(blocks) = &state.messages[len - 1].content {
            if let ContentBlock::Text { text: t2 } = &blocks[0] {
                assert!(t2.contains("Msg 3"));
            }
        }
    }

    #[test]
    fn test_conversation_state_estimate_tokens_with_tool_use() {
        let mut state = ConversationState::default();
        state.messages.push(Message {
            role: "assistant".to_string(),
            content: MessageContent::Blocks(vec![
                ContentBlock::Text {
                    text: "Running bash command".to_string(),
                },
                ContentBlock::ToolUse {
                    id: "tu_1".to_string(),
                    name: "bash".to_string(),
                    input: serde_json::json!({"command": "ls -la"}),
                },
            ]),
        });
        let tokens = state.estimate_tokens();
        assert!(tokens > 0);
    }

    // ── Serialization edge cases ──────────────────────────────────────

    #[test]
    fn test_query_metadata_minimal_serialization() {
        let metadata = QueryMetadata {
            timestamp: chrono::Utc::now(),
            tools_allowed: false,
            max_tokens: None,
            model: "test-model".to_string(),
            temperature: None,
            top_p: None,
        };
        let json = serde_json::to_string(&metadata).unwrap();
        let deserialized: QueryMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.tools_allowed, false);
        assert!(deserialized.max_tokens.is_none());
        assert!(deserialized.temperature.is_none());
        assert!(deserialized.top_p.is_none());
    }

    #[test]
    fn test_conversation_stats_serialization() {
        let stats = ConversationStats {
            message_count: 42,
            turn_count: 10,
            total_tokens: 50000,
            total_cost: 1.234,
        };
        let json = serde_json::to_string(&stats).unwrap();
        let deserialized: ConversationStats = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.message_count, 42);
        assert_eq!(deserialized.turn_count, 10);
        assert_eq!(deserialized.total_tokens, 50000);
        assert!((deserialized.total_cost - 1.234).abs() < 0.001);
    }

    #[test]
    fn test_conversation_stats_zero_values() {
        let stats = ConversationStats {
            message_count: 0,
            turn_count: 0,
            total_tokens: 0,
            total_cost: 0.0,
        };
        let json = serde_json::to_string(&stats).unwrap();
        let deserialized: ConversationStats = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.message_count, 0);
        assert_eq!(deserialized.total_cost, 0.0);
    }

    // ── CostTracker model name matching ──────────────────────────────────

    #[test]
    fn test_cost_tracker_model_name_variants() {
        // "claude-3-5-sonnet" should match Sonnet pricing: 3.0 input + 15.0 output per M tok
        let cost = CostTracker::calculate_cost("claude-3-5-sonnet", 1_000_000, 1_000_000);
        assert!((cost - 18.0).abs() < 0.001);

        // "claude-3-5-haiku" should match Haiku pricing
        let cost = CostTracker::calculate_cost("claude-3-5-haiku", 1_000_000, 1_000_000);
        // Haiku: 0.80 input + 4.0 output = 4.80 total
        assert!((cost - 4.80).abs() < 0.001);

        // "claude-3-opus" should match Opus pricing: 15.0 input + 75.0 output = 90.0
        let cost = CostTracker::calculate_cost("claude-3-opus", 1_000_000, 1_000_000);
        assert!((cost - 90.0).abs() < 0.001);

        // Unknown model → default pricing
        let cost = CostTracker::calculate_cost("unknown-model", 1_000_000, 1_000_000);
        assert!(cost > 0.0);
    }

    #[test]
    fn test_cost_tracker_accumulate_multiple_models() {
        let mut tracker = CostTracker::new("claude-sonnet-4".to_string());

        // Record usage from multiple models
        tracker.record_usage("claude-sonnet-4", 100_000, 50_000);
        tracker.record_usage("gpt-4o", 200_000, 100_000);
        tracker.record_usage("llama3", 50_000, 25_000);

        // Verify totals are accumulated
        assert_eq!(tracker.total_input_tokens, 350_000);
        assert_eq!(tracker.total_output_tokens, 175_000);
        assert!(tracker.total_cost() > 0.0);
    }

    #[test]
    fn test_cost_tracker_accumulate_across_recordings() {
        let mut tracker = CostTracker::new("claude-sonnet-4".to_string());

        // First recording
        tracker.record_usage("claude-sonnet-4", 1_000_000, 500_000);
        let cost1 = tracker.total_cost();

        // Second recording should add to cost
        tracker.record_usage("claude-sonnet-4", 1_000_000, 500_000);
        let cost2 = tracker.total_cost();

        assert!((cost2 - 2.0 * cost1).abs() < 0.001);
    }

    #[test]
    fn test_query_engine_config_builder_chained() {
        let config = QueryEngineConfig {
            max_turns: 1,
            max_budget_usd: Some(0.01),
            timeout_seconds: 10,
            verbose: false,
            enable_thinking: false,
            max_context_tokens: Some(1000),
            compression_threshold: 0.9,
            keep_recent_messages: 1,
        };
        assert_eq!(config.max_turns, 1);
        assert_eq!(config.max_budget_usd, Some(0.01));
        assert_eq!(config.timeout_seconds, 10);
        assert!(!config.verbose);
        assert!(!config.enable_thinking);
        assert_eq!(config.max_context_tokens, Some(1000));
        assert!((config.compression_threshold - 0.9).abs() < 0.001);
        assert_eq!(config.keep_recent_messages, 1);
    }
}
