//! Query engine and orchestration

use shannon_core_api::{LlmClient, MessageResponse, Message, StreamEvent, ContentBlock, Usage};
use shannon_core_tools::{ToolExecutionService, ToolExecutionError};
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;
use futures_util::StreamExt;

/// Tool use output
#[derive(Debug, Clone)]
pub struct ToolUseOutput {
    pub tool_use_id: String,
    pub content: String,
    pub is_error: bool,
}

/// Query context
#[derive(Debug, Clone)]
pub struct QueryContext {
    pub session_id: Uuid,
    pub messages: Vec<Message>,
    pub tools: Vec<String>,
    pub max_tokens: Option<usize>,
    pub temperature: Option<f32>,
}

impl QueryContext {
    pub fn new(session_id: Uuid) -> Self {
        Self {
            session_id,
            messages: Vec::new(),
            tools: Vec::new(),
            max_tokens: None,
            temperature: None,
        }
    }

    pub fn with_messages(mut self, messages: Vec<Message>) -> Self {
        self.messages = messages;
        self
    }

    pub fn with_tools(mut self, tools: Vec<String>) -> Self {
        self.tools = tools;
        self
    }
}

/// Query options
#[derive(Debug, Clone)]
pub struct QueryOptions {
    pub stream: bool,
    pub compact_messages: bool,
    pub max_retries: usize,
    pub timeout: std::time::Duration,
}

impl Default for QueryOptions {
    fn default() -> Self {
        Self {
            stream: true,
            compact_messages: true,
            max_retries: 3,
            timeout: std::time::Duration::from_secs(120),
        }
    }
}

/// Query response
#[derive(Debug, Clone)]
pub struct QueryResponse {
    pub message: MessageResponse,
    pub tokens_used: usize,
    pub compacted: bool,
}

/// Query engine errors
#[derive(Debug, thiserror::Error)]
pub enum QueryError {
    #[error("LLM API error: {0}")]
    LlmError(String),

    #[error("Tool execution error: {0}")]
    ToolError(#[from] ToolExecutionError),

    #[error("Compaction error: {0}")]
    CompactionError(String),

    #[error("Timeout: query exceeded {0:?}")]
    Timeout(std::time::Duration),

    #[error("No response from LLM")]
    NoResponse,
}

/// Query engine - main orchestrator
pub struct QueryEngine {
    llm_client: Arc<LlmClient>,
    tool_executor: Arc<ToolExecutionService>,
    compact_engine: Arc<crate::CompactEngine>,
    default_options: QueryOptions,
}

impl QueryEngine {
    pub fn new(
        llm_client: Arc<LlmClient>,
        tool_executor: Arc<ToolExecutionService>,
        compact_engine: Arc<crate::CompactEngine>,
    ) -> Self {
        Self {
            llm_client,
            tool_executor,
            compact_engine,
            default_options: QueryOptions::default(),
        }
    }

    pub fn with_options(mut self, options: QueryOptions) -> Self {
        self.default_options = options;
        self
    }

    /// Execute a query with the given context
    pub async fn query(&self, ctx: QueryContext) -> Result<QueryResponse, QueryError> {
        self.query_with_options(ctx, self.default_options.clone()).await
    }

    /// Execute a query with custom options
    pub async fn query_with_options(
        &self,
        ctx: QueryContext,
        options: QueryOptions,
    ) -> Result<QueryResponse, QueryError> {
        let mut messages = ctx.messages;

        // Apply compaction if enabled
        let compacted = if options.compact_messages {
            // Convert Message to shannon_types::Message for compaction
            let compat_messages: Vec<shannon_types::Message> = messages.iter().map(|m| {
                let content_str = match &m.content {
                    shannon_core_api::MessageContent::Text(s) => s.clone(),
                    shannon_core_api::MessageContent::Blocks(blocks) => {
                        blocks.iter().map(|b| match b {
                            ContentBlock::Text { text } => text.clone(),
                            _ => String::new(),
                        }).collect::<Vec<_>>().join(" ")
                    }
                };
                shannon_types::Message {
                    id: uuid::Uuid::new_v4().to_string(),
                    role: m.role.clone(),
                    content: content_str,
                    timestamp: chrono::Utc::now(),
                    metadata: serde_json::Value::Null,
                }
            }).collect();

            let compacted = self.compact_engine.compact(compat_messages)
                .map_err(|e| QueryError::CompactionError(e.to_string()))?;

            // Convert back to Message
            messages = compacted.into_iter().map(|m| {
                Message {
                    role: m.role,
                    content: shannon_core_api::MessageContent::Text(m.content),
                }
            }).collect();
            true
        } else {
            false
        };

        // Execute query with timeout
        let response = tokio::time::timeout(
            options.timeout,
            self.execute_llm_query(&messages, options.stream)
        )
        .await
        .map_err(|_| QueryError::Timeout(options.timeout))??;

        let tokens_used = response.usage.input_tokens as usize + response.usage.output_tokens as usize;

        Ok(QueryResponse {
            message: response,
            tokens_used,
            compacted,
        })
    }

    async fn execute_llm_query(
        &self,
        messages: &[Message],
        stream: bool,
    ) -> Result<MessageResponse, QueryError> {
        if stream {
            // For streaming, collect the stream into a single response
            let mut stream = self.llm_client.send_message_stream(messages.to_vec(), None).await
                .map_err(|e| QueryError::LlmError(e.to_string()))?;

            let mut final_content = String::new();
            let mut final_response: Option<MessageResponse> = None;

            while let Some(event_result) = stream.next().await {
                let event = event_result.map_err(|e| QueryError::LlmError(e.to_string()))?;
                match event {
                    StreamEvent::MessageStart { message } => {
                        final_response = Some(message);
                    }
                    StreamEvent::ContentBlockDelta { delta, .. } => {
                        if let shannon_core_api::ContentDelta::TextDelta { text } = delta {
                            final_content.push_str(&text);
                        }
                    }
                    StreamEvent::MessageStop => {
                        break;
                    }
                    _ => {}
                }
            }

            // Build final response from collected data
            let mut response = final_response.unwrap_or_else(|| MessageResponse {
                id: uuid::Uuid::new_v4().to_string(),
                role: "assistant".to_string(),
                content: vec![],
                model: self.llm_client.model().to_string(),
                stop_reason: Some("end_turn".to_string()),
                usage: Usage {
                    input_tokens: 0,
                    output_tokens: 0,
                },
            });

            response.content = vec![ContentBlock::Text { text: final_content }];
            Ok(response)
        } else {
            let content = self.llm_client.send_message(messages.to_vec(), None).await
                .map_err(|e| QueryError::LlmError(e.to_string()))?;

            Ok(MessageResponse {
                id: uuid::Uuid::new_v4().to_string(),
                role: "assistant".to_string(),
                content,
                model: self.llm_client.model().to_string(),
                stop_reason: Some("end_turn".to_string()),
                usage: Usage {
                    input_tokens: 0,
                    output_tokens: 0,
                },
            })
        }
    }

    /// Execute a tool use
    pub async fn execute_tool(
        &self,
        session_id: Uuid,
        tool_name: &str,
        tool_input: serde_json::Value,
    ) -> Result<ToolUseOutput, ToolExecutionError> {
        let result = self.tool_executor.run_tool_use(
            session_id,
            tool_name,
            tool_input.clone(),
        ).await?;

        Ok(ToolUseOutput {
            tool_use_id: uuid::Uuid::new_v4().to_string(),
            content: format!("{:?}", result),
            is_error: false,
        })
    }
}

/// Query state manager
pub struct QueryStateManager {
    active_queries: Arc<RwLock<std::collections::HashMap<Uuid, QueryContext>>>,
}

impl QueryStateManager {
    pub fn new() -> Self {
        Self {
            active_queries: Arc::new(RwLock::new(std::collections::HashMap::new())),
        }
    }

    pub async fn register_query(&self, id: Uuid, ctx: QueryContext) {
        self.active_queries.write().await.insert(id, ctx);
    }

    pub async fn get_query(&self, id: &Uuid) -> Option<QueryContext> {
        self.active_queries.read().await.get(id).cloned()
    }

    pub async fn remove_query(&self, id: &Uuid) {
        self.active_queries.write().await.remove(id);
    }

    pub async fn active_count(&self) -> usize {
        self.active_queries.read().await.len()
    }
}
