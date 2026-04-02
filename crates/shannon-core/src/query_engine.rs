//! # Query Engine
//!
//! Main orchestrator for streaming query processing with tool orchestration.

use crate::api::{ClaudeClient, Message, MessageContent, StreamEvent};
use crate::permissions::PermissionManager;
use crate::state::StateManager;
use crate::tools::{ToolOutput, ToolRegistry};
use futures::stream::{self, Stream, StreamExt};
use serde::{Deserialize, Serialize};
use std::pin::Pin;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::mpsc;
use uuid::Uuid;

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
}

impl Default for QueryEngineConfig {
    fn default() -> Self {
        Self {
            max_turns: 20,
            max_budget_usd: None,
            timeout_seconds: 300,
            verbose: false,
            enable_thinking: false,
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

/// Main query engine orchestrator
pub struct QueryEngine {
    client: ClaudeClient,
    tools: Arc<ToolRegistry>,
    permissions: Arc<PermissionManager>,
    state: Arc<StateManager>,
    config: QueryEngineConfig,
    event_tx: mpsc::UnboundedSender<QueryEvent>,
    conversation: ConversationState,
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
        Self {
            client,
            tools: Arc::new(tools),
            permissions: Arc::new(permissions),
            state: Arc::new(state),
            config,
            event_tx,
            conversation: ConversationState::default(),
        }
    }

    /// Create with default configuration
    pub fn with_defaults(
        client: ClaudeClient,
        tools: ToolRegistry,
        permissions: PermissionManager,
        state: StateManager,
    ) -> Self {
        Self::new(client, tools, permissions, state, QueryEngineConfig::default())
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
        self.conversation.messages.push(Message {
            role: "user".to_string(),
            content: vec![MessageContent::Text { text: content }],
        });
    }

    /// Add an assistant message to the conversation
    pub fn add_assistant_message(&mut self, content: Vec<MessageContent>) {
        self.conversation.messages.push(Message {
            role: "assistant".to_string(),
            content,
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
    pub async fn process_query(&self, context: QueryContext) -> QueryStream {
        let query_id = context.query_id;
        let tools = self.tools.clone();
        let permissions = self.permissions.clone();
        let session_id = context.session_id;
        let config = self.config.clone();
        let event_tx = self.event_tx.clone();

        // Emit start event
        let _ = event_tx.send(QueryEvent::Started { query_id });

        // Create the processing stream
        let stream = stream::unfold(
            (query_id, tools, permissions, session_id, config, 0, false),
            move |(qid, tools, perms, sid, cfg, turn, complete)| async move {
                if complete {
                    return None;
                }

                // Simulate processing - in real implementation, this would:
                // 1. Build request with conversation history
                // 2. Call Claude API
                // 3. Stream events back
                // 4. Handle tool use requests
                // 5. Continue until completion or max turns

                // For now, emit a completion event
                Some((
                    Ok(QueryEvent::Completed { query_id: qid }),
                    (qid, tools, perms, sid, cfg, turn, true),
                ))
            },
        );

        Box::pin(stream)
    }

    /// Execute a tool call
    async fn execute_tool(
        &self,
        tool_name: &str,
        tool_input: serde_json::Value,
        context: &QueryContext,
    ) -> Result<ToolOutput, QueryError> {
        // Check permissions first
        if let Err(e) = self
            .permissions
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
            Some(self.tools.to_json_schema())
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
                                StreamEvent::ContentBlockDelta { index, delta } => {
                                    if let Some(text) = delta.get("text").and_then(|v| v.as_str()) {
                                        events.push(QueryEvent::Text {
                                            query_id,
                                            content: text.to_string(),
                                        });
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
    use crate::permissions::{Permission, PermissionLevel};
    use crate::tools::ToolOutput;
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
    }
}
