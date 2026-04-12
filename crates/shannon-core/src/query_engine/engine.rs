//! Main QueryEngine struct and orchestration logic.

use crate::api::{
    ContentBlock, ContentDelta, LlmClient, Message, MessageContent, StreamEvent, ToolResultContent,
};
use crate::memory::AutoDreamService;
use crate::memory::MemoryStore;
use crate::permissions::PermissionManager;
use crate::query_engine::streaming::ConversationState;
use crate::query_engine::types::{
    ConversationStats, CostTracker, QueryContext, QueryEngineConfig, QueryError, QueryEvent,
    QueryStream,
};
use crate::state::StateManager;
use crate::tools::{ToolOutput, ToolRegistry};
use futures::stream::{self, StreamExt};
use std::sync::{Arc, RwLock};
use tokio::sync::mpsc;
use uuid::Uuid;

/// Main query engine orchestrator
pub struct QueryEngine {
    pub(crate) client: LlmClient,
    pub(crate) tools: Arc<ToolRegistry>,
    pub(crate) permissions: Arc<RwLock<PermissionManager>>,
    pub(crate) state: Arc<StateManager>,
    pub(crate) config: QueryEngineConfig,
    pub(crate) conversation: ConversationState,
    pub(crate) cost_tracker: Arc<RwLock<CostTracker>>,
    /// Optional memory store for persisting and retrieving conversation memories.
    pub(crate) memory: Option<Arc<std::sync::RwLock<MemoryStore>>>,
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
            memory: None,
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
            memory: None,
        }
    }

    /// Attach a memory store to this query engine.
    ///
    /// Enables memory-augmented queries (relevant memories injected into the
    /// system prompt) and automatic memory extraction after each conversation
    /// turn via [`AutoDreamService`].
    pub fn with_memory(mut self, store: MemoryStore) -> Self {
        self.memory = Some(Arc::new(std::sync::RwLock::new(store)));
        self
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
    pub async fn process_query(
        &self,
        context: QueryContext,
        permission_request_tx: Option<mpsc::UnboundedSender<super::types::PermissionRequest>>,
    ) -> QueryStream {
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

        // Search for relevant memories to augment the system prompt
        let memory_entries = if let Some(ref mem_store) = self.memory {
            match mem_store.read() {
                Ok(store) => {
                    let results = store.search(&user_message, None);
                    results.into_iter().take(5).collect::<Vec<_>>()
                }
                Err(_) => Vec::new(),
            }
        } else {
            Vec::new()
        };

        let system_prompt = if memory_entries.is_empty() {
            config.system_prompt.clone()
        } else {
            let mut prompt = config.system_prompt.clone().unwrap_or_default();
            prompt.push_str("\n\n## Relevant Memories\n");
            for entry in &memory_entries {
                prompt.push_str(&format!(
                    "- [{}] (confidence: {:.2}) {}\n",
                    entry.category, entry.confidence, entry.content
                ));
            }
            Some(prompt)
        };

        // Clone existing conversation to preserve multi-turn context
        let mut conversation = self.conversation.clone();
        conversation.messages.push(Message {
            role: "user".to_string(),
            content: MessageContent::Text(user_message.clone()),
        });

        // Clone memory store for post-query extraction (fire-and-forget)
        let memory_for_extraction = self.memory.clone();

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

            let mut turn = 0;
            let mut tool_results: Vec<(String, String)> = Vec::new();
            let mut total_input_tokens: u64 = 0;
            let mut total_output_tokens: u64 = 0;

            loop {
                if turn >= config.max_turns {
                    let total_cost =
                        CostTracker::calculate_cost(&client_model, total_input_tokens, total_output_tokens);
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

                // Auto-compress conversation when approaching token limits
                if conversation.needs_compression(&config) {
                    let _ = tx.send(Ok(QueryEvent::Progress {
                        query_id,
                        message: "Compressing conversation context...".to_string(),
                    }));
                    conversation.compress(&config);
                    messages = conversation.messages.clone();
                }

                // Add pending tool results from previous turn
                for (tool_use_id, result_content) in tool_results.drain(..) {
                    messages.push(Message {
                        role: "user".to_string(),
                        content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                            tool_use_id,
                            content: Some(ToolResultContent::Single(result_content)),
                            is_error: Some(false),
                        }]),
                    });
                }

                // Get tools schema
                let tools_schema = Some(tools.to_tool_definitions());

                // Call the API
                match client.send_message_stream(messages, tools_schema, system_prompt.clone()).await {
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
                                        StreamEvent::ContentBlockStart {
                                            content_block, ..
                                        } => {
                                            match &content_block {
                                                ContentBlock::ToolUse {
                                                    id,
                                                    name,
                                                    input,
                                                } => {
                                                    current_tool_use =
                                                        Some((id.clone(), name.clone()));
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
                                                if let Ok(json_val) =
                                                    serde_json::from_str::<serde_json::Value>(
                                                        &accumulated_tool_input,
                                                    )
                                                {
                                                    tool_inputs.push((id, name, json_val));
                                                }
                                                accumulated_tool_input.clear();
                                            }
                                        }
                                        StreamEvent::MessageDelta { usage, .. } => {
                                            let input_tokens = usage.input_tokens as u64;
                                            let output_tokens = usage.output_tokens as u64;
                                            let cost_usd = CostTracker::calculate_cost(
                                                &client_model,
                                                input_tokens,
                                                output_tokens,
                                            );

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
                                                for (tool_id, tool_name, tool_input) in
                                                    tool_inputs.drain(..)
                                                {
                                                    let _ = tx.send(Ok(QueryEvent::Progress {
                                                        query_id,
                                                        message: format!(
                                                            "Executing tool: {}",
                                                            tool_name
                                                        ),
                                                    }));

                                                    // Check if permission is needed
                                                    // Create a scope to ensure the RwLockReadGuard is dropped before await
                                                    let permission_needed = {
                                                        let guard = permissions.read().unwrap();
                                                        guard.create_permission_prompt(
                                                            &tool_name,
                                                            &tool_input,
                                                            session_id_for_permissions,
                                                        )
                                                    };

                                                    if let Some(prompt) = permission_needed {
                                                        // Check if already denied
                                                        if prompt.risk_level
                                                            == crate::permissions::RiskLevel::Critical
                                                        {
                                                            // Already denied - skip execution
                                                            let error_msg = format!(
                                                                "Tool denied: {}",
                                                                prompt.description
                                                            );
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
                                                        if let Some(ref req_tx) =
                                                            permission_request_tx
                                                        {
                                                            let (response_tx, mut response_rx) =
                                                                mpsc::unbounded_channel();
                                                            let _ = req_tx.send(
                                                                super::types::PermissionRequest {
                                                                    prompt: prompt.clone(),
                                                                    response_tx,
                                                                },
                                                            );

                                                            // Wait for user response (guard is now dropped, safe to await)
                                                            match response_rx.recv().await {
                                                                Some(
                                                                    crate::permissions::PermissionChoice::Deny,
                                                                ) => {
                                                                    let denied_msg = format!(
                                                                        "Permission denied: {}",
                                                                        prompt.description
                                                                    );
                                                                    let _ = tx.send(Ok(QueryEvent::ToolUseResult {
                                                                        query_id,
                                                                        tool_use_id: tool_id
                                                                            .clone(),
                                                                        tool_name,
                                                                        result: denied_msg
                                                                            .clone(),
                                                                        is_error: true,
                                                                    }));
                                                                    tool_results
                                                                        .push((tool_id, denied_msg));
                                                                    continue;
                                                                }
                                                                Some(
                                                                    crate::permissions::PermissionChoice::AllowOnce,
                                                                ) => {}
                                                                Some(
                                                                    crate::permissions::PermissionChoice::AlwaysAllow,
                                                                ) => {
                                                                    let _ = permissions
                                                                        .write()
                                                                        .unwrap()
                                                                        .process_permission_choice(
                                                                            session_id_for_permissions,
                                                                            &prompt,
                                                                            crate::permissions::PermissionChoice::AlwaysAllow,
                                                                        );
                                                                }
                                                                None => {
                                                                    let error_msg =
                                                                        "Permission channel closed"
                                                                            .to_string();
                                                                    let _ = tx.send(Ok(QueryEvent::ToolUseResult {
                                                                        query_id,
                                                                        tool_use_id: tool_id
                                                                            .clone(),
                                                                        tool_name,
                                                                        result: error_msg
                                                                            .clone(),
                                                                        is_error: true,
                                                                    }));
                                                                    tool_results
                                                                        .push((tool_id, error_msg));
                                                                    continue;
                                                                }
                                                            }
                                                        }
                                                        // If no permission channel, assume auto-allow (for non-interactive contexts)
                                                    }

                                                    match tools
                                                        .execute(&tool_name, tool_input.clone())
                                                        .await
                                                    {
                                                        Ok(output) => {
                                                            let _ = tx.send(Ok(QueryEvent::ToolUseResult {
                                                                query_id,
                                                                tool_use_id: tool_id.clone(),
                                                                tool_name,
                                                                result: output.content.clone(),
                                                                is_error: false,
                                                            }));
                                                            tool_results
                                                                .push((tool_id, output.content.clone()));
                                                        }
                                                        Err(e) => {
                                                            let error_msg =
                                                                format!("Tool error: {}", e);
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
                                                    tokens_used: (usage.input_tokens
                                                        + usage.output_tokens)
                                                        as u64,
                                                }));
                                            } else {
                                                let total_cost = CostTracker::calculate_cost(
                                                    &client_model,
                                                    total_input_tokens,
                                                    total_output_tokens,
                                                );
                                                let _ = tx.send(Ok(QueryEvent::Cost {
                                                    query_id,
                                                    total_cost_usd: total_cost,
                                                    input_tokens: total_input_tokens,
                                                    output_tokens: total_output_tokens,
                                                }));
                                                let _ =
                                                    tx.send(Ok(QueryEvent::Completed { query_id }));
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
                            let total_cost = CostTracker::calculate_cost(
                                &client_model,
                                total_input_tokens,
                                total_output_tokens,
                            );
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

            // Post-query: fire-and-forget memory extraction via AutoDreamService
            if let Some(ref mem_store) = memory_for_extraction {
                let store_arc = mem_store.clone();
                let msgs = conversation.messages.clone();
                tokio::spawn(async move {
                    let dream = AutoDreamService::new(store_arc);
                    let project = std::env::current_dir()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|_| "default".to_string());
                    let _ = dream.process_conversation(&msgs, &project);
                });
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
    pub(crate) async fn execute_tool(
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
    pub(crate) async fn process_turn(
        &self,
        query_id: Uuid,
        _session_id: Uuid,
        turn_number: usize,
    ) -> Result<Vec<QueryEvent>, QueryError> {
        let mut events = Vec::new();

        // Build messages for API call
        let messages = self.conversation.messages.clone();

        // Get tools schema if enabled
        let tools_schema = if !self.conversation.messages.is_empty() {
            Some(self.tools.to_tool_definitions())
        } else {
            None
        };

        // Call the API (stub - would use actual streaming)
        match self.client.send_message_stream(messages, tools_schema, self.config.system_prompt.clone()).await {
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
    pub(crate) fn validate_query(&self, context: &QueryContext) -> Result<(), QueryError> {
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
