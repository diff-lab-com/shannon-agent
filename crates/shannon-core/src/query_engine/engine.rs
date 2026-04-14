//! Main QueryEngine struct and orchestration logic.
//!
//! # Session Persistence
//!
//! The QueryEngine supports automatic conversation persistence to disk:
//!
//! - **Auto-save**: After each successful query, the conversation is automatically
//!   saved to `~/.shannon/sessions/{session_id}.json`
//! - **Auto-restore**: Use `QueryEngine::with_session_id()` to create an engine
//!   with a specific session ID, then call `restore_session()` to load previous
//!   conversations
//! - **Title generation**: The first user message (truncated to 50 chars) is used
//!   as the session title
//!
//! ## Example: Resume a previous session
//!
//! ```ignore
//! use shannon_core::query_engine::QueryEngine;
//! use uuid::Uuid;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Create engine with specific session ID
//! let mut engine = QueryEngine::with_session_id(
//!     client,
//!     tools,
//!     permissions,
//!     state,
//!     config,
//!     session_id, // Uuid from previous session
//! );
//!
//! // Restore conversation history
//! if engine.restore_session(session_id)? {
//!     println!("Session restored successfully");
//! } else {
//!     println!("No previous session found");
//! }
//! # Ok(())
//! # }
//! ```

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
use crate::tools::ToolRegistry;
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
    /// Session ID for conversation persistence
    pub(crate) session_id: Uuid,
    /// Rule-based permission classifier for pre-checking tool invocations.
    /// Stored for access via `crate::permission_classifier` module path in permissions flow
    #[allow(dead_code)]
    pub(crate) permission_classifier: crate::permission_classifier::PermissionClassifier,
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
        let session_id = Uuid::new_v4();
        Self {
            client,
            tools: Arc::new(tools),
            permissions: Arc::new(RwLock::new(permissions)),
            state: Arc::new(state),
            config,
            conversation: ConversationState::default(),
            cost_tracker: Arc::new(RwLock::new(CostTracker::new(model))),
            memory: None,
            session_id,
            permission_classifier: crate::permission_classifier::PermissionClassifier::new(),
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
        let session_id = Uuid::new_v4();
        Self {
            client,
            tools: Arc::new(tools),
            permissions: Arc::new(RwLock::new(permissions)),
            state: Arc::new(state),
            config: QueryEngineConfig::default(),
            conversation: ConversationState::default(),
            cost_tracker: Arc::new(RwLock::new(CostTracker::new(model))),
            memory: None,
            session_id,
            permission_classifier: crate::permission_classifier::PermissionClassifier::new(),
        }
    }

    /// Create a new query engine with a specific session ID for resuming
    ///
    /// This allows creating a QueryEngine that can restore a previous session.
    /// Use `restore_session()` after creation to load the conversation history.
    pub fn with_session_id(
        client: LlmClient,
        tools: ToolRegistry,
        permissions: PermissionManager,
        state: StateManager,
        config: QueryEngineConfig,
        session_id: Uuid,
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
            session_id,
            permission_classifier: crate::permission_classifier::PermissionClassifier::new(),
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

    /// Get the current session ID
    pub fn session_id(&self) -> Uuid {
        self.session_id
    }

    /// Restore conversation from a previously saved session
    ///
    /// Attempts to load session data from disk. Returns Ok(false) if no
    /// persisted session exists for the given session_id.
    pub fn restore_session(&mut self, session_id: Uuid) -> Result<bool, QueryError> {
        match self.state.load_session(&session_id) {
            Ok(Some(session_data)) => {
                // Restore conversation messages
                self.conversation.messages = session_data.messages;
                self.conversation.turn_count = session_data.metadata.turn_count;
                self.conversation.total_tokens = session_data.metadata.total_input_tokens
                    + session_data.metadata.total_output_tokens;
                // Cost is not tracked in persisted metadata, so we keep current value
                self.session_id = session_id;
                Ok(true)
            }
            Ok(None) => Ok(false),
            Err(e) => Err(QueryError::StateError(e.to_string())),
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
        let client_provider = self.client.provider().clone();
        let user_message = context.user_message.clone();
        let state_for_save = self.state.clone();
        let session_id_for_save = self.session_id;
        let cost_tracker = self.cost_tracker.clone();

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
            // Prevent OS sleep during long-running queries (drops on exit)
            let _sleep_guard = crate::prevent_sleep::PreventSleepGuard::new();

            // Create a new client for this task, preserving provider from original config
            let client_config = crate::api::LlmClientConfig {
                api_key: client_api_key,
                base_url: client_base_url,
                model: client_model.clone(),
                max_tokens: client_max_tokens,
                provider: client_provider,
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

                    // Auto-save conversation after completion
                    let final_messages = conversation.messages.clone();
                    let _ = save_conversation_to_disk(
                        &state_for_save,
                        session_id_for_save,
                        &final_messages,
                        &client_model,
                    );

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

                // Auto-compress conversation if it exceeds the threshold
                {
                    // Rough token estimate: ~4 chars per token
                    let total_chars: usize = messages.iter().map(|m| {
                        match &m.content {
                            MessageContent::Text(t) => t.len(),
                            MessageContent::Blocks(blocks) => blocks.iter().map(|b| match b {
                                ContentBlock::Text { text } => text.len(),
                                ContentBlock::ToolUse { name, input, .. } => {
                                    name.len() + input.to_string().len()
                                }
                                ContentBlock::ToolResult { content, .. } => {
                                    content.as_ref().map(|c| match c {
                                        ToolResultContent::Single(s) => s.len(),
                                        ToolResultContent::Multiple(v) => v.iter().map(|b| match b {
                                            ContentBlock::Text { text } => text.len(),
                                            _ => 50,
                                        }).sum::<usize>(),
                                    }).unwrap_or(50)
                                }
                                ContentBlock::Image { source } => source.data.len(),
                            }).sum::<usize>()
                        }
                    }).sum();
                    let estimated_tokens = total_chars / 4;
                    let max_context = config.max_context_tokens.unwrap_or(200_000);
                    if estimated_tokens as f32 / max_context as f32 > 0.8 {
                        match crate::compact::CompactEngine::with_defaults() {
                            Ok(mut compact_engine) => {
                                match compact_engine.compact(&mut messages) {
                                    Ok(result) => {
                                        let _ = tx.send(Ok(QueryEvent::Progress {
                                            query_id,
                                            message: format!(
                                                "Context compressed: {} -> {} tokens",
                                                result.original_tokens, result.compacted_tokens
                                            ),
                                        }));
                                    }
                                    Err(e) => {
                                        tracing::warn!("Compression failed: {}, truncating instead", e);
                                        let keep = 20;
                                        if messages.len() > keep {
                                            messages = messages.split_off(messages.len() - keep);
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::warn!("CompactEngine init failed ({}), truncating old messages", e);
                                let keep = 20;
                                if messages.len() > keep {
                                    messages = messages.split_off(messages.len() - keep);
                                }
                            }
                        }
                    }
                }

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
                                            if let ContentBlock::ToolUse {
                                                    id,
                                                    name,
                                                    input,
                                                } = &content_block {
                                                current_tool_use =
                                                    Some((id.clone(), name.clone()));
                                                let _ = tx.send(Ok(QueryEvent::ToolUseRequest {
                                                    query_id,
                                                    tool_use_id: id.clone(),
                                                    tool_name: name.clone(),
                                                    tool_input: input.clone(),
                                                }));
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

                                            // Update shared cost tracker
                                            if let Ok(mut tracker) = cost_tracker.write() {
                                                tracker.record_usage(&client_model, input_tokens, output_tokens);
                                            }

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
                                                            "Executing tool: {tool_name}"
                                                        ),
                                                    }));

                                                    // Pre-check with classifier and permission system
                                                    let permission_result = {
                                                        let guard = permissions.read().expect("permissions rwlock poisoned");
                                                        guard.classify_and_check(
                                                            session_id_for_permissions,
                                                            &tool_name,
                                                            &tool_input,
                                                        )
                                                    };

                                                    match permission_result {
                                                        Err(_) => {
                                                            // Denied by classifier
                                                            let error_msg = format!(
                                                                "Tool denied by classifier: {tool_name}"
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
                                                        Ok(None) => {
                                                            // Auto-allowed (low risk or always-allowed)
                                                            // Fall through to execute
                                                        }
                                                        Ok(Some(prompt)) => {
                                                            // Check if already denied
                                                            if prompt.risk_level
                                                                == crate::permissions::RiskLevel::Critical
                                                            {
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

                                                                // Wait for user response
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
                                                                            .expect("permissions rwlock poisoned")
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
                                                            // If no permission channel, assume auto-allow
                                                        }
                                                    }

                                                    // Spawn a timer that emits periodic
                                                    // progress events for long-running
                                                    // tools (>2s).
                                                    let progress_tx = tx.clone();
                                                    let tool_name_clone = tool_name.clone();
                                                    let tool_id_clone = tool_id.clone();
                                                    let progress_handle = tokio::spawn(async move {
                                                        let mut elapsed = 0u64;
                                                        loop {
                                                            tokio::time::sleep(
                                                                tokio::time::Duration::from_secs(2),
                                                            )
                                                            .await;
                                                            elapsed += 2;
                                                            let progress =
                                                                (elapsed as f32 / 30.0).min(0.95);
                                                            let _ = progress_tx.send(Ok(
                                                                QueryEvent::ToolProgress {
                                                                    query_id,
                                                                    tool_use_id: tool_id_clone
                                                                        .clone(),
                                                                    tool_name: tool_name_clone
                                                                        .clone(),
                                                                    progress,
                                                                    message: format!(
                                                                        "Running for {elapsed}s..."
                                                                    ),
                                                                },
                                                            ));
                                                        }
                                                    });

                                                    let result = tools
                                                        .execute(&tool_name, tool_input.clone())
                                                        .await;

                                                    // Abort the progress timer now that
                                                    // the tool has finished.
                                                    progress_handle.abort();

                                                    match result {
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
                                                                format!("Tool error: {e}");
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

                                                // Auto-save conversation after completion
                                                let final_messages = conversation.messages.clone();
                                                let _ = save_conversation_to_disk(
                                                    &state_for_save,
                                                    session_id_for_save,
                                                    &final_messages,
                                                    &client_model,
                                                );

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

                            // Auto-save conversation after completion
                            let final_messages = conversation.messages.clone();
                            let _ = save_conversation_to_disk(
                                &state_for_save,
                                session_id_for_save,
                                &final_messages,
                                &client_model,
                            );

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
            receiver.recv().await.map(|event| (event, receiver))
        });

        Box::pin(stream)
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

    /// Get the current cost tracker summary string.
    ///
    /// Returns a formatted summary of accumulated API costs including
    /// input/output tokens and total USD cost.
    pub fn cost_summary(&self) -> String {
        self.cost_tracker.read().expect("cost_tracker rwlock poisoned").summary()
    }
}

/// Helper function to save conversation to disk
///
/// This is called from the background task after a query completes successfully.
fn save_conversation_to_disk(
    state: &Arc<StateManager>,
    session_id: Uuid,
    messages: &[Message],
    model: &str,
) -> Result<(), String> {
    use crate::state::SessionPersistMetadata;
    use crate::api::{ContentBlock, MessageContent};

    // Generate title from first user message
    let title = messages
        .iter()
        .find(|m| m.role == "user")
        .and_then(|m| match &m.content {
            MessageContent::Text(text) => {
                let preview = if text.len() > 50 {
                    format!("{}...", &text[..47])
                } else {
                    text.clone()
                };
                Some(preview)
            }
            MessageContent::Blocks(blocks) => {
                blocks.iter().find_map(|b| match b {
                    ContentBlock::Text { text } => {
                        let preview = if text.len() > 50 {
                            format!("{}...", &text[..47])
                        } else {
                            text.clone()
                        };
                        Some(preview)
                    }
                    _ => None,
                })
            }
        });

    // Build metadata
    let metadata = SessionPersistMetadata {
        model: model.to_string(),
        title,
        ..Default::default()
    };

    state
        .save_session(&session_id, messages, &metadata)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{LlmClient, LlmClientConfig, MessageContent};
    use crate::permissions::PermissionManager;
    use crate::tools::ToolRegistry;
    use std::env;
    use std::fs;
    use uuid::Uuid;

    fn create_test_client() -> LlmClient {
        let config = LlmClientConfig {
            api_key: "test-key".to_string(),
            base_url: "http://localhost:11434".to_string(),
            model: "test-model".to_string(),
            ..Default::default()
        };
        LlmClient::new(config)
    }

    #[test]
    fn test_query_engine_session_id() {
        let client = create_test_client();
        let tools = ToolRegistry::new();
        let permissions = PermissionManager::new();
        let state = StateManager::new();
        let config = QueryEngineConfig::default();

        let engine = QueryEngine::new(client, tools, permissions, state, config);
        let session_id = engine.session_id();

        // Should generate a valid UUID
        assert_ne!(session_id, Uuid::nil());
    }

    #[test]
    fn test_query_engine_with_session_id() {
        let client = create_test_client();
        let tools = ToolRegistry::new();
        let permissions = PermissionManager::new();
        let state = StateManager::new();
        let config = QueryEngineConfig::default();

        let specific_id = Uuid::new_v4();
        let engine = QueryEngine::with_session_id(
            client,
            tools,
            permissions,
            state,
            config,
            specific_id,
        );

        assert_eq!(engine.session_id(), specific_id);
    }

    #[test]
    fn test_save_and_restore_session() {
        // Create a temp directory for this test
        let temp_dir = env::temp_dir()
            .join("shannon-session-test")
            .join(Uuid::new_v4().to_string());
        fs::create_dir_all(&temp_dir).unwrap();

        let state = Arc::new(StateManager::with_sessions_dir(temp_dir.clone()).unwrap());
        let session_id = Uuid::new_v4();

        // Create some test messages
        let messages = vec![
            crate::api::Message {
                role: "user".to_string(),
                content: MessageContent::Text("Hello, how are you?".to_string()),
            },
            crate::api::Message {
                role: "assistant".to_string(),
                content: MessageContent::Text("I'm doing well, thanks!".to_string()),
            },
        ];

        // Save session
        let result = save_conversation_to_disk(
            &state,
            session_id,
            &messages,
            "test-model",
        );
        assert!(result.is_ok(), "Failed to save session: {:?}", result.err());

        // Verify file was created
        let session_file = temp_dir.join(format!("{session_id}.json"));
        assert!(session_file.exists(), "Session file not created");

        // Load and verify
        let loaded = state.load_session(&session_id).unwrap();
        assert!(loaded.is_some(), "Failed to load session");

        let session_data = loaded.unwrap();
        assert_eq!(session_data.session_id, session_id);
        assert_eq!(session_data.messages.len(), 2);
        assert_eq!(session_data.metadata.model, "test-model");

        // Verify title was generated from first user message
        assert_eq!(
            session_data.metadata.title,
            Some("Hello, how are you?".to_string())
        );

        // Cleanup
        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn test_save_conversation_long_title_truncated() {
        let temp_dir = env::temp_dir()
            .join("shannon-title-test")
            .join(Uuid::new_v4().to_string());
        fs::create_dir_all(&temp_dir).unwrap();

        let state = Arc::new(StateManager::with_sessions_dir(temp_dir.clone()).unwrap());
        let session_id = Uuid::new_v4();

        // Create a message with long text (100 chars)
        let long_text = "A".repeat(100);
        let messages = vec![crate::api::Message {
            role: "user".to_string(),
            content: MessageContent::Text(long_text),
        }];

        // Save
        save_conversation_to_disk(&state, session_id, &messages, "test-model").unwrap();

        // Load and check title is truncated to 47 chars + "..." = 50 total
        let loaded = state.load_session(&session_id).unwrap().unwrap();
        let expected_title = "A".repeat(47) + "...";
        assert_eq!(loaded.metadata.title, Some(expected_title));

        // Cleanup
        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn test_restore_session_nonexistent() {
        let client = create_test_client();
        let tools = ToolRegistry::new();
        let permissions = PermissionManager::new();
        let state = StateManager::new();
        let config = QueryEngineConfig::default();

        let mut engine = QueryEngine::new(client, tools, permissions, state, config);
        let nonexistent_id = Uuid::new_v4();

        // Should return Ok(false) for nonexistent session
        let result = engine.restore_session(nonexistent_id);
        assert!(result.is_ok());
        assert!(!result.unwrap());
    }
}
