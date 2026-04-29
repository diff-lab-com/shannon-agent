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
    ContentBlock, ContentDelta, LlmClient, LlmProvider, Message, MessageContent, StreamEvent, SystemContentBlock,
    ToolResultContent,
};
use crate::memory::AutoDreamService;
use crate::memory::MemoryStore;
use crate::permissions::PermissionManager;
use crate::query_engine::context_injector::ContextInjector;
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
    /// Hook manager for lifecycle events (pre/post tool use, session start/end)
    pub(crate) hook_manager: Arc<tokio::sync::RwLock<crate::hooks::HookManager>>,
    /// Context injector for project instructions and preference memory.
    pub(crate) context_injector: Option<Arc<ContextInjector>>,
}

/// Helper to create a loaded HookManager
fn hook_mgr() -> crate::hooks::HookManager {
    let mut mgr = crate::hooks::HookManager::new();
    let _ = mgr.load();
    mgr
}

/// Generate a unified diff preview for a file edit operation.
fn generate_diff_preview(path: &str, old: &str, new: &str) -> String {
    let mut diff = format!("--- {path} (current)\n+++ {path} (proposed)\n");
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();

    // Simple line-by-line diff: show removed (-) and added (+) lines
    let max_lines = old_lines.len().max(new_lines.len());
    let mut changes = 0u32;
    let max_changes = 30; // Limit diff output size

    for i in 0..max_lines {
        if changes >= max_changes {
            diff.push_str(&format!("... ({} more lines)\n", max_lines - i));
            break;
        }
        let old_line = old_lines.get(i).copied();
        let new_line = new_lines.get(i).copied();

        match (old_line, new_line) {
            (Some(o), Some(n)) if o == n => {}
            (Some(_), None) => {
                diff.push_str(&format!("-{}\n", old_lines[i]));
                changes += 1;
            }
            (None, Some(_)) => {
                diff.push_str(&format!("+{}\n", new_lines[i]));
                changes += 1;
            }
            (Some(_), Some(_)) => {
                diff.push_str(&format!("-{}\n", old_lines[i]));
                diff.push_str(&format!("+{}\n", new_lines[i]));
                changes += 2;
            }
            (None, None) => unreachable!(),
        }
    }

    if changes == 0 && old_lines.len() != new_lines.len() {
        // Length changed but no line-level diff caught
        diff.push_str(&format!(
            "@@ file size changed: {} -> {} lines @@\n",
            old_lines.len(),
            new_lines.len()
        ));
    }

    diff
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
            hook_manager: Arc::new(tokio::sync::RwLock::new(hook_mgr())),
            context_injector: None,
        }
    }

    /// Create with default configuration
    pub fn with_defaults(
        client: LlmClient,
        tools: ToolRegistry,
        permissions: PermissionManager,
        state: StateManager,
    ) -> Self {
        Self::with_defaults_arc(client, Arc::new(tools), permissions, state)
    }

    /// Create with default configuration and a pre-wrapped `Arc<ToolRegistry>`.
    ///
    /// Use this when you need to share the registry with async callbacks
    /// (e.g. MCP `on_tools_changed` for dynamic tool re-registration).
    pub fn with_defaults_arc(
        client: LlmClient,
        tools: Arc<ToolRegistry>,
        permissions: PermissionManager,
        state: StateManager,
    ) -> Self {
        let model = client.model().to_string();
        let session_id = Uuid::new_v4();
        Self {
            client,
            tools,
            permissions: Arc::new(RwLock::new(permissions)),
            state: Arc::new(state),
            config: QueryEngineConfig::default(),
            conversation: ConversationState::default(),
            cost_tracker: Arc::new(RwLock::new(CostTracker::new(model))),
            memory: None,
            session_id,
            hook_manager: Arc::new(tokio::sync::RwLock::new(hook_mgr())),
            context_injector: None,
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
            hook_manager: Arc::new(tokio::sync::RwLock::new(hook_mgr())),
            context_injector: None,
        }
    }

    /// Set a custom system prompt, replacing the default.
    pub fn with_system_prompt(mut self, prompt: String) -> Self {
        self.config.system_prompt = Some(prompt);
        self
    }

    /// Append content to the existing system prompt.
    pub fn append_system_prompt(&mut self, content: &str) {
        let current = self.config.system_prompt.take().unwrap_or_default();
        self.config.system_prompt = Some(format!("{current}\n\n{content}"));
    }

    /// Get the current system prompt, if set.
    pub fn system_prompt(&self) -> Option<String> {
        self.config.system_prompt.clone()
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

    /// Access the memory store, if configured.
    pub fn memory(&self) -> Option<&Arc<std::sync::RwLock<MemoryStore>>> {
        self.memory.as_ref()
    }

    /// Attach a context injector for project instructions and preference memory.
    ///
    /// When set, the injector provides project instructions and user preferences
    /// that are injected into the system prompt and re-injected after compaction.
    pub fn with_context_injector(mut self, injector: ContextInjector) -> Self {
        self.context_injector = Some(Arc::new(injector));
        self
    }

    /// Access the context injector, if configured.
    pub fn context_injector(&self) -> Option<&Arc<ContextInjector>> {
        self.context_injector.as_ref()
    }

    /// Set the maximum number of turns for a conversation
    pub fn set_max_turns(&mut self, turns: usize) {
        self.config.max_turns = turns;
    }

    /// Get the current session ID
    pub fn session_id(&self) -> Uuid {
        self.session_id
    }

    /// Access the hook manager for firing lifecycle events (SessionStart, SessionEnd, etc.)
    pub fn hook_manager(&self) -> Arc<tokio::sync::RwLock<crate::hooks::HookManager>> {
        self.hook_manager.clone()
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

    /// Add a user message with content blocks (e.g., text + image)
    pub fn add_user_message_blocks(&mut self, blocks: Vec<crate::api::ContentBlock>) {
        use crate::api::MessageContent;
        self.conversation.messages.push(crate::api::Message {
            role: "user".to_string(),
            content: MessageContent::Blocks(blocks),
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

    /// Rewind the conversation by removing the last `n` user turns.
    ///
    /// A turn starts with a user message and includes all subsequent non-user
    /// messages until the next user message. Returns the number of messages removed.
    /// Decrements `turn_count` by the number of turns rewound.
    pub fn rewind_conversation(&mut self, turns: usize) -> usize {
        if turns == 0 || self.conversation.messages.is_empty() {
            return 0;
        }

        let mut turns_found = 0;
        let mut cutoff = self.conversation.messages.len();

        for i in (0..self.conversation.messages.len()).rev() {
            if self.conversation.messages[i].role == "user" {
                turns_found += 1;
                cutoff = i;
                if turns_found >= turns {
                    break;
                }
            }
        }

        if turns_found == 0 {
            return 0;
        }

        let removed = self.conversation.messages.len() - cutoff;
        self.conversation.messages.truncate(cutoff);
        self.conversation.turn_count = self.conversation.turn_count.saturating_sub(turns_found);

        removed
    }

    /// Clear the conversation history
    pub fn clear_conversation(&mut self) {
        self.conversation = ConversationState::default();
    }

    /// Restore a previously saved conversation (for session resume)
    pub fn restore_messages(&mut self, messages: Vec<crate::api::Message>) {
        self.conversation.messages = messages;
    }

    /// Get the current conversation messages (for session persistence).
    pub fn conversation_messages(&self) -> &[crate::api::Message] {
        &self.conversation.messages
    }

    /// Get a reference to the underlying LLM client.
    pub fn client(&self) -> &LlmClient {
        &self.client
    }

    /// Update the model used for API calls.
    pub fn set_model(&mut self, model: String) {
        self.client.set_model(model);
    }

    /// Update the model AND switch provider (including base_url).
    pub fn set_model_for_provider(&mut self, model: String, provider: LlmProvider) {
        self.client.set_model_for_provider(model, provider);
    }

    /// Replace the conversation history with new messages (e.g., after compaction)
    pub fn replace_conversation(&mut self, messages: Vec<Message>) {
        let turn_count = messages.iter().filter(|m| m.role == "user").count();
        self.conversation.messages = messages;
        self.conversation.turn_count = turn_count;
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
        let hook_manager = self.hook_manager.clone();
        let _context_injector = self.context_injector.clone();

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

        // Build structured system prompt with cache breakpoints.
        // Layout: [base prompt] → [memory (cached)] → [smart context (cached)] → [project instructions (cached)]
        let mut system_blocks: Vec<SystemContentBlock> = Vec::new();
        let use_cache = matches!(client_provider, crate::api::LlmProvider::Anthropic | crate::api::LlmProvider::Bedrock | crate::api::LlmProvider::Custom);

        // Base system prompt
        if let Some(ref base) = config.system_prompt {
            system_blocks.push(SystemContentBlock::text(base.clone()));
        }

        // Memory entries
        if !memory_entries.is_empty() {
            let mut mem_text = String::from("## Relevant Memories\n");
            for entry in &memory_entries {
                mem_text.push_str(&format!(
                    "- [{}] (confidence: {:.2}) {}\n",
                    entry.category, entry.confidence, entry.content
                ));
            }
            let block = if use_cache {
                SystemContentBlock::cached(mem_text)
            } else {
                SystemContentBlock::text(mem_text)
            };
            system_blocks.push(block);
        }

        // Smart context: auto-include relevant files based on query
        let smart_context = {
            let working_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
            crate::smart_context::find_relevant_context(&user_message, &working_dir)
        };
        if let Some(ctx) = crate::smart_context::format_context_for_prompt(&smart_context) {
            let block = if use_cache {
                SystemContentBlock::cached(ctx)
            } else {
                SystemContentBlock::text(ctx)
            };
            system_blocks.push(block);
        }

        // Inject CLAUDE.md / AGENTS.md / GEMINI.md project instructions
        {
            let working_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
            if let Some(ctx) = crate::project_instructions::load_full_context(&working_dir) {
                let block = if use_cache {
                    SystemContentBlock::cached(ctx.content)
                } else {
                    SystemContentBlock::text(ctx.content)
                };
                system_blocks.push(block);
            }
        }

        // Inject context from ContextInjector (preference memory + hot-reloaded instructions)
        if let Some(ref injector) = self.context_injector {
            let extra_blocks = injector.build_system_blocks(use_cache);
            system_blocks.extend(extra_blocks);
        }

        // Decide whether to use structured blocks or fallback to plain string.
        // Use structured blocks only when we have content (avoids empty system arrays).
        let system_blocks_opt = if system_blocks.is_empty() {
            None
        } else {
            Some(system_blocks)
        };
        let system_prompt = config.system_prompt.clone();

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

            // Fire UserPromptSubmit hook
            {
                let prompt_event = crate::hooks::HookEvent::UserPromptSubmit {
                    prompt: user_message.clone(),
                };
                let hm = hook_manager.read().await;
                let _ = hm.run_hooks(&prompt_event).await;
            }

            // Create a new client for this task, preserving provider from original config
            let client_config = {
                let mut cfg = crate::api::LlmClientConfig {
                    api_key: client_api_key,
                    base_url: client_base_url,
                    model: client_model.clone(),
                    max_tokens: client_max_tokens,
                    provider: client_provider,
                    ..Default::default()
                };
                // Enable extended thinking with a budget if configured
                if config.enable_thinking {
                    cfg.budget_tokens = Some(10000);
                }
                cfg
            };
            let client = LlmClient::new(client_config);

            let mut turn = 0;
            let mut tool_results: Vec<(String, String, bool)> = Vec::new(); // (tool_use_id, content, is_error)
            let mut total_input_tokens: u64 = 0;
            let mut total_output_tokens: u64 = 0;
            let mut file_edits_made = false;
            let mut compaction_failures: u32 = 0;
            const MAX_COMPACTION_FAILURES: u32 = 2;

            // Denial circuit breaker: track consecutive permission denials.
            // After MAX_CONSECUTIVE_DENIALS the model is told to stop retrying;
            // if it still retries HARD_LIMIT more times, the loop aborts.
            let mut consecutive_denials: u32 = 0;
            const DENIAL_SOFT_LIMIT: u32 = 3;  // inject warning to LLM
            const DENIAL_HARD_LIMIT: u32 = 5;  // abort the agent loop

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
                for (tool_use_id, result_content, is_error) in tool_results.drain(..) {
                    messages.push(Message {
                        role: "user".to_string(),
                        content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                            tool_use_id,
                            content: Some(ToolResultContent::Single(result_content)),
                            is_error: Some(is_error),
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
                                ContentBlock::Thinking { thinking } => thinking.len(),
                            }).sum::<usize>()
                        }
                    }).sum();
                    let estimated_tokens = total_chars / 4;
                    let max_context = config.max_context_tokens.unwrap_or(200_000);
                    if estimated_tokens as f32 / max_context as f32 > 0.8 {
                        // Circuit breaker: if compaction has failed repeatedly, skip it and just truncate
                        if compaction_failures >= MAX_COMPACTION_FAILURES {
                            let keep = 20;
                            if messages.len() > keep {
                                messages = messages.split_off(messages.len() - keep);
                            }
                            let _ = tx.send(Ok(QueryEvent::Progress {
                                query_id,
                                message: "Compaction skipped (too many failures), truncating old messages".to_string(),
                            }));
                        } else {
                            match crate::compact::CompactEngine::with_defaults() {
                                Ok(mut compact_engine) => {
                                    // Build re-injection context from ContextInjector if available,
                                    // otherwise fall back to the system prompt (truncated).
                                    match compact_engine.compact(&mut messages) {
                                        Ok(result) => {
                                            compaction_failures = 0; // reset on success
                                            let _ = tx.send(Ok(QueryEvent::Progress {
                                                query_id,
                                                message: format!(
                                                    "Context compressed (3-tier): {} -> {} tokens ({:.0}% reduction, {} messages compacted)",
                                                    result.original_tokens,
                                                    result.compacted_tokens,
                                                    result.reduction_ratio * 100.0,
                                                    result.messages_compacted,
                                                ),
                                            }));
                                            let _ = tx.send(Ok(QueryEvent::Info {
                                                query_id,
                                                message: format!(
                                                    "compaction: {} -> {} tokens ({:.0}% reduction, {} removed, {} compacted, {:?})",
                                                    result.original_tokens,
                                                    result.compacted_tokens,
                                                    result.reduction_ratio * 100.0,
                                                    result.messages_removed,
                                                    result.messages_compacted,
                                                    result.strategy,
                                                ),
                                            }));
                                        }
                                        Err(e) => {
                                            compaction_failures += 1;
                                            tracing::warn!("Compression failed: {}, truncating instead", e);
                                            let keep = 20;
                                            if messages.len() > keep {
                                                messages = messages.split_off(messages.len() - keep);
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    compaction_failures += 1;
                                    tracing::warn!("CompactEngine init failed ({}), truncating old messages", e);
                                    let keep = 20;
                                    if messages.len() > keep {
                                        messages = messages.split_off(messages.len() - keep);
                                    }
                                }
                            }
                        }
                    }
                }

                // Call the API — use structured system blocks when available for prompt caching
                let stream_result = if let Some(ref blocks) = system_blocks_opt {
                    client.send_message_stream_structured(messages.clone(), tools_schema.clone(), blocks.clone()).await
                } else {
                    client.send_message_stream(messages.clone(), tools_schema.clone(), system_prompt.clone()).await
                };
                match stream_result {
                    Ok(mut stream) => {
                        let mut current_tool_use: Option<(String, String)> = None;
                        let mut accumulated_tool_input = String::new();
                        let mut tool_inputs: Vec<(String, String, serde_json::Value)> = Vec::new();
                        let mut has_content = false;
                        // Accumulate the full assistant response for conversation tracking
                        let mut assistant_text = String::new();
                        let mut assistant_tool_uses: Vec<ContentBlock> = Vec::new();

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
                                                ContentBlock::Thinking { .. } => {
                                                    // Thinking block started — deltas will arrive via ThinkingDelta
                                                }
                                                _ => {}
                                            }
                                        }
                                        StreamEvent::ContentBlockDelta { delta, .. } => {
                                            match delta {
                                                ContentDelta::TextDelta { text } => {
                                                    has_content = true;
                                                    assistant_text.push_str(&text);
                                                    let _ = tx.send(Ok(QueryEvent::Text {
                                                        query_id,
                                                        content: text,
                                                    }));
                                                }
                                                ContentDelta::InputJsonDelta { partial_json } => {
                                                    accumulated_tool_input.push_str(&partial_json);
                                                }
                                                ContentDelta::ThinkingDelta { thinking } => {
                                                    let _ = tx.send(Ok(QueryEvent::Thinking {
                                                        query_id,
                                                        content: thinking,
                                                    }));
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
                                                    tool_inputs.push((id.clone(), name.clone(), json_val.clone()));
                                                    assistant_tool_uses.push(ContentBlock::ToolUse {
                                                        id,
                                                        name,
                                                        input: json_val,
                                                    });
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

                                                // Budget enforcement: check if limit exceeded
                                                if tracker.is_budget_exceeded() {
                                                    let limit = tracker.budget_limit_usd.unwrap_or(0.0);
                                                    let total = tracker.total_cost();
                                                    let _ = tx.send(Ok(QueryEvent::Progress {
                                                        query_id,
                                                        message: format!(
                                                            "Budget limit reached (${limit:.2}). Stopping. (spent: ${total:.4})"
                                                        ),
                                                    }));
                                                    // Break out of the loop by setting turn to max
                                                    turn = config.max_turns;
                                                    break;
                                                }

                                                // Budget warning at 80% usage
                                                if let Some(ratio) = tracker.budget_usage_ratio() {
                                                    if (0.8..0.81).contains(&ratio) {
                                                        let limit = tracker.budget_limit_usd.unwrap_or(0.0);
                                                        let total = tracker.total_cost();
                                                        let _ = tx.send(Ok(QueryEvent::Progress {
                                                            query_id,
                                                            message: format!(
                                                                "Budget warning: ${total:.4} / ${limit:.2} ({:.0}%)",
                                                                ratio * 100.0
                                                            ),
                                                        }));
                                                    }
                                                }
                                            }

                                            let _ = tx.send(Ok(QueryEvent::Usage {
                                                query_id,
                                                input_tokens,
                                                output_tokens,
                                                cost_usd,
                                            }));

                                            if !tool_inputs.is_empty() {
                                                // Phase 1: Check permissions and hooks (sequential — may need user input)
                                                let mut approved_tools: Vec<(String, String, serde_json::Value)> = Vec::new();

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
                                                            consecutive_denials += 1;
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
                                                            tool_results.push((tool_id, error_msg, true));
                                                            continue;
                                                        }
                                                        Ok(None) => {
                                                            // Auto-allowed (low risk or always-allowed)
                                                            // Fall through to check hooks
                                                        }
                                                        Ok(Some(mut prompt)) => {
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
                                                                tool_results.push((tool_id, error_msg, true));
                                                                continue;
                                                            }

                                                            // Send permission request if a channel is provided
                                                            if let Some(ref req_tx) =
                                                                permission_request_tx
                                                            {
                                                                // Generate diff preview for file edit/write tools
                                                                if matches!(tool_name.as_str(), "edit" | "write" | "EditTool" | "WriteTool") {
                                                                    if let Some(path) = tool_input.get("file_path").and_then(|v| v.as_str()) {
                                                                        let path_buf = std::path::PathBuf::from(path);
                                                                        if path_buf.exists() {
                                                                            if let Ok(old_content) = std::fs::read_to_string(&path_buf) {
                                                                                let new_content = tool_input.get("content")
                                                                                    .or_else(|| tool_input.get("new_string"))
                                                                                    .and_then(|v| v.as_str())
                                                                                    .unwrap_or("");
                                                                                let diff = generate_diff_preview(path, &old_content, new_content);
                                                                                prompt.diff_preview = Some(diff);
                                                                            }
                                                                        } else if tool_name == "write" || tool_name == "WriteTool" {
                                                                            // New file — show that it's being created
                                                                            if let Some(content) = tool_input.get("content").and_then(|v| v.as_str()) {
                                                                                let preview = if content.len() > 500 {
                                                                                    format!("+ Creating new file ({} bytes)\n{}\n... (truncated)", content.len(), &content[..500])
                                                                                } else {
                                                                                    format!("+ Creating new file\n{content}")
                                                                                };
                                                                                prompt.diff_preview = Some(preview);
                                                                            }
                                                                        }
                                                                    }
                                                                }
                                                                let (response_tx, mut response_rx) =
                                                                    mpsc::unbounded_channel();
                                                                // Clone prompt for the request; keep a reference for deny message
                                                                let prompt_desc = prompt.description.clone();
                                                                let prompt_for_choice = prompt.clone();
                                                                let _ = req_tx.send(
                                                                    super::types::PermissionRequest {
                                                                        prompt,
                                                                        response_tx,
                                                                    },
                                                                );

                                                                // Wait for user response
                                                                match response_rx.recv().await {
                                                                    Some(
                                                                        crate::permissions::PermissionChoice::Deny,
                                                                    ) => {
                                                                        consecutive_denials += 1;
                                                                        let denied_msg = format!(
                                                                            "Permission denied: {prompt_desc}"
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
                                                                            .push((tool_id, denied_msg, true));
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
                                                                                &prompt_for_choice,
                                                                                crate::permissions::PermissionChoice::AlwaysAllow,
                                                                            );
                                                                    }
                                                                    Some(
                                                                        crate::permissions::PermissionChoice::EditAndRun,
                                                                    ) => {
                                                                        // User edited the command; treat as allow-once
                                                                        let _ = permissions
                                                                            .write()
                                                                            .expect("permissions rwlock poisoned")
                                                                            .process_permission_choice(
                                                                                session_id_for_permissions,
                                                                                &prompt_for_choice,
                                                                                crate::permissions::PermissionChoice::EditAndRun,
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
                                                                            .push((tool_id, error_msg, true));
                                                                        continue;
                                                                    }
                                                                }
                                                            }
                                                            // If no permission channel, assume auto-allow
                                                        }
                                                    }

                                                    // Run PreToolUse hooks
                                                    let hook_event = crate::hooks::HookEvent::PreToolUse {
                                                        tool_name: tool_name.clone(),
                                                        input: tool_input.clone(),
                                                    };
                                                    let pre_hook_decision = {
                                                        let hm = hook_manager.read().await;
                                                        match hm.run_hooks(&hook_event).await {
                                                            Ok(results) => crate::hooks::HookManager::resolve_results(&results),
                                                            Err(e) => {
                                                                tracing::warn!("PreToolUse hook error: {e}");
                                                                crate::hooks::HookDecision::Allow
                                                            }
                                                        }
                                                    };

                                                    let effective_input = match &pre_hook_decision {
                                                        crate::hooks::HookDecision::Deny { reason } => {
                                                            let error_msg = format!("Hook denied: {reason}");
                                                            let _ = tx.send(Ok(QueryEvent::ToolUseResult {
                                                                query_id,
                                                                tool_use_id: tool_id.clone(),
                                                                tool_name,
                                                                result: error_msg.clone(),
                                                                is_error: true,
                                                            }));
                                                            tool_results.push((tool_id, error_msg, true));
                                                            continue;
                                                        }
                                                        crate::hooks::HookDecision::Modify { modified_input, .. } => {
                                                            modified_input.clone().unwrap_or(tool_input.clone())
                                                        }
                                                        crate::hooks::HookDecision::Allow => tool_input.clone(),
                                                    };

                                                    approved_tools.push((tool_id, tool_name, effective_input));
                                                }

                                                // Circuit breaker: check consecutive denials before executing tools.
                                                if consecutive_denials >= DENIAL_HARD_LIMIT {
                                                    let _ = tx.send(Ok(QueryEvent::ToolUseResult {
                                                        query_id,
                                                        tool_use_id: "circuit-breaker".to_string(),
                                                        tool_name: "system".to_string(),
                                                        result: "Too many consecutive permission denials. Stopping.".to_string(),
                                                        is_error: true,
                                                    }));
                                                    break; // exit the agent loop
                                                }

                                                // Phase 2: Execute approved tools using read/write-aware batch scheduler.
                                                //
                                                // Read-only tools are grouped into parallel batches (max 10).
                                                // Write tools execute one at a time to avoid race conditions.
                                                {
                                                    let batches = tools.partition_tool_calls(approved_tools, 10);

                                                    for batch in batches {
                                                        match batch {
                                                            crate::tools::ToolBatch::Parallel(tool_calls) => {
                                                                // Execute read-only tools concurrently
                                                                let mut exec_handles = Vec::new();
                                                                for (tool_id, tool_name, effective_input) in tool_calls {
                                                                    // Emit progress: tool started
                                                                    let _ = tx.send(Ok(QueryEvent::ToolProgress {
                                                                        query_id,
                                                                        tool_use_id: tool_id.clone(),
                                                                        tool_name: tool_name.clone(),
                                                                        progress: 0.0,
                                                                        message: format!("{tool_name} started"),
                                                                    }));
                                                                    let tools_exec = tools.clone();
                                                                    let exec_name = tool_name.clone();
                                                                    let exec_input = effective_input.clone();
                                                                    let handle = tokio::spawn(async move {
                                                                        (tool_id, tool_name, effective_input, tools_exec.execute(&exec_name, exec_input).await)
                                                                    });
                                                                    exec_handles.push(handle);
                                                                }

                                                                for handle in exec_handles {
                                                                    match handle.await {
                                                                        Ok((tool_id, tool_name, effective_input, result)) => {
                                                                            // Run PostToolUse hooks
                                                                            {
                                                                                let output_val = match &result {
                                                                                    Ok(o) => serde_json::Value::String(o.content.clone()),
                                                                                    Err(e) => serde_json::Value::String(format!("Error: {e}")),
                                                                                };
                                                                                let post_event = crate::hooks::HookEvent::PostToolUse {
                                                                                    tool_name: tool_name.clone(),
                                                                                    input: effective_input,
                                                                                    output: output_val,
                                                                                };
                                                                                let hm = hook_manager.read().await;
                                                                                let _ = hm.run_hooks(&post_event).await;
                                                                            }

                                                                            // Emit progress: tool completed
                                                                            let _ = tx.send(Ok(QueryEvent::ToolProgress {
                                                                                query_id,
                                                                                tool_use_id: tool_id.clone(),
                                                                                tool_name: tool_name.clone(),
                                                                                progress: 1.0,
                                                                                message: format!("{tool_name} completed"),
                                                                            }));
                                                                            match result {
                                                                                Ok(output) => {
                                                                                    let is_err = output.is_error;
                                                                                    consecutive_denials = 0; // reset on success
                                                                                    let _ = tx.send(Ok(QueryEvent::ToolUseResult {
                                                                                        query_id,
                                                                                        tool_use_id: tool_id.clone(),
                                                                                        tool_name: tool_name.clone(),
                                                                                        result: output.content.clone(),
                                                                                        is_error: is_err,
                                                                                    }));
                                                                                    tool_results.push((tool_id, output.content.clone(), is_err));
                                                                                }
                                                                                Err(e) => {
                                                                                    let error_msg = format!("Tool error: {e}");
                                                                                    let _ = tx.send(Ok(QueryEvent::ToolUseResult {
                                                                                        query_id,
                                                                                        tool_use_id: tool_id.clone(),
                                                                                        tool_name,
                                                                                        result: error_msg.clone(),
                                                                                        is_error: true,
                                                                                    }));
                                                                                    tool_results.push((tool_id, error_msg, true));
                                                                                }
                                                                            }
                                                                        }
                                                                        Err(e) => {
                                                                            let error_msg = format!("Task join error: {e}");
                                                                            let _ = tx.send(Ok(QueryEvent::ToolUseResult {
                                                                                query_id,
                                                                                tool_use_id: String::new(),
                                                                                tool_name: String::new(),
                                                                                result: error_msg.clone(),
                                                                                is_error: true,
                                                                            }));
                                                                        }
                                                                    }
                                                                }
                                                            }
                                                            crate::tools::ToolBatch::Serial((tool_id, tool_name, effective_input)) => {
                                                                // Execute write tools sequentially (one at a time)
                                                                // Emit progress: tool started
                                                                let _ = tx.send(Ok(QueryEvent::ToolProgress {
                                                                    query_id,
                                                                    tool_use_id: tool_id.clone(),
                                                                    tool_name: tool_name.clone(),
                                                                    progress: 0.0,
                                                                    message: format!("{tool_name} started"),
                                                                }));
                                                                let result = tools
                                                                    .execute(&tool_name, effective_input.clone())
                                                                    .await;

                                                                // Run PostToolUse hooks
                                                                {
                                                                    let output_val = match &result {
                                                                        Ok(o) => serde_json::Value::String(o.content.clone()),
                                                                        Err(e) => serde_json::Value::String(format!("Error: {e}")),
                                                                    };
                                                                    let post_event = crate::hooks::HookEvent::PostToolUse {
                                                                        tool_name: tool_name.clone(),
                                                                        input: effective_input,
                                                                        output: output_val,
                                                                    };
                                                                    let hm = hook_manager.read().await;
                                                                    let _ = hm.run_hooks(&post_event).await;
                                                                }

                                                                // Emit progress: tool completed
                                                                let _ = tx.send(Ok(QueryEvent::ToolProgress {
                                                                    query_id,
                                                                    tool_use_id: tool_id.clone(),
                                                                    tool_name: tool_name.clone(),
                                                                    progress: 1.0,
                                                                    message: format!("{tool_name} completed"),
                                                                }));
                                                                match result {
                                                                    Ok(output) => {
                                                                        let is_err = output.is_error;
                                                                        consecutive_denials = 0; // reset on success
                                                                        let _ = tx.send(Ok(QueryEvent::ToolUseResult {
                                                                            query_id,
                                                                            tool_use_id: tool_id.clone(),
                                                                            tool_name: tool_name.clone(),
                                                                            result: output.content.clone(),
                                                                            is_error: is_err,
                                                                        }));
                                                                        tool_results.push((tool_id, output.content.clone(), is_err));
                                                                        if matches!(tool_name.as_str(), "Edit" | "Write") {
                                                                            file_edits_made = true;
                                                                        }
                                                                    }
                                                                    Err(e) => {
                                                                        let error_msg = format!("Tool error: {e}");
                                                                        let _ = tx.send(Ok(QueryEvent::ToolUseResult {
                                                                            query_id,
                                                                            tool_use_id: tool_id.clone(),
                                                                            tool_name,
                                                                            result: error_msg.clone(),
                                                                            is_error: true,
                                                                        }));
                                                                        tool_results.push((tool_id, error_msg, true));
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                }

                                                // Soft-limit warning: inject a message telling the model to stop retrying
                                                if (DENIAL_SOFT_LIMIT..DENIAL_HARD_LIMIT).contains(&consecutive_denials) {
                                                    let warning = format!(
                                                        "The user has denied {consecutive_denials} consecutive tool calls. \
                                                         Stop retrying the same or similar operations. \
                                                         Ask the user for clarification or try a completely different approach."
                                                    );
                                                    tool_results.push(("denial-warning".to_string(), warning, false));
                                                }

                                                turn += 1;

                                                // Save assistant response to conversation for multi-turn context.
                                                // The API requires: assistant(tool_use) → user(tool_result).
                                                // Without the assistant message, the next API call has no
                                                // context for which tools were requested.
                                                {
                                                    let mut assistant_blocks: Vec<ContentBlock> = Vec::new();
                                                    if !assistant_text.is_empty() {
                                                        assistant_blocks.push(ContentBlock::Text {
                                                            text: assistant_text.clone(),
                                                        });
                                                    }
                                                    assistant_blocks.append(&mut assistant_tool_uses);
                                                    if !assistant_blocks.is_empty() {
                                                        conversation.messages.push(Message {
                                                            role: "assistant".to_string(),
                                                            content: MessageContent::Blocks(assistant_blocks),
                                                        });
                                                    }
                                                }

                                                // Auto-commit: if enabled and file-write tools were used,
                                                // stage changes and commit automatically.
                                                if config.auto_commit && file_edits_made {
                                                    file_edits_made = false; // reset for next turn
                                                    let _ = async {
                                                        let add_output = tokio::process::Command::new("git")
                                                            .args(["add", "-A"])
                                                            .output()
                                                            .await;
                                                        if let Ok(out) = add_output {
                                                            if out.status.success() {
                                                                // Generate commit message from diff stat
                                                                let stat_output = tokio::process::Command::new("git")
                                                                    .args(["diff", "--stat", "--cached"])
                                                                    .output()
                                                                    .await;
                                                                let msg = match stat_output {
                                                                    Ok(s) if s.status.success() => {
                                                                        let stat = String::from_utf8_lossy(&s.stdout);
                                                                        let file_count = stat.lines().filter(|l| !l.trim().is_empty()).count().saturating_sub(1);
                                                                        if file_count == 0 {
                                                                            "chore: update files".to_string()
                                                                        } else {
                                                                            format!("chore: auto-commit ({file_count} files)")
                                                                        }
                                                                    }
                                                                    _ => "chore: auto-commit".to_string(),
                                                                };
                                                                let commit_output = tokio::process::Command::new("git")
                                                                    .args(["commit", "-m", &msg])
                                                                    .output()
                                                                    .await;
                                                                if let Ok(co) = commit_output {
                                                                    if co.status.success() {
                                                                        let hash = String::from_utf8_lossy(&co.stdout)
                                                                            .lines()
                                                                            .find(|l| l.starts_with('['))
                                                                            .unwrap_or("committed")
                                                                            .to_string();
                                                                        let _ = tx.send(Ok(QueryEvent::ToolUseResult {
                                                                            query_id,
                                                                            tool_use_id: String::new(),
                                                                            tool_name: "auto_commit".to_string(),
                                                                            result: format!("Auto-committed: {hash}"),
                                                                            is_error: false,
                                                                        }));
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }.await;
                                                }

                                                let _ = tx.send(Ok(QueryEvent::TurnCompleted {
                                                    query_id,
                                                    turn_number: turn,
                                                    tokens_used: (usage.input_tokens
                                                        + usage.output_tokens)
                                                        as u64,
                                                }));
                                            } else {
                                                // No tool uses — save assistant text to conversation
                                                if !assistant_text.is_empty() {
                                                    conversation.messages.push(Message {
                                                        role: "assistant".to_string(),
                                                        content: MessageContent::Text(assistant_text.clone()),
                                                    });
                                                }
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

    /// Get current conversation statistics.
    ///
    /// Token counts and cost are sourced from the cost tracker, which accumulates
    /// actual API-reported usage (not estimates).
    pub fn conversation_stats(&self) -> ConversationStats {
        let tracker = self.cost_tracker.read().expect("cost_tracker rwlock poisoned");
        ConversationStats {
            message_count: self.conversation.messages.len(),
            turn_count: self.conversation.turn_count,
            total_tokens: tracker.total_input_tokens + tracker.total_output_tokens,
            total_cost: tracker.total_cost_usd,
        }
    }

    /// Get the current cost tracker summary string.
    ///
    /// Returns a formatted summary of accumulated API costs including
    /// input/output tokens and total USD cost.
    pub fn cost_summary(&self) -> String {
        self.cost_tracker.read().expect("cost_tracker rwlock poisoned").summary()
    }

    /// Get a reference to the cost tracker for reading cost details.
    pub fn cost_tracker(&self) -> &Arc<RwLock<CostTracker>> {
        &self.cost_tracker
    }

    /// Get a reference to the permission manager for reading/adjusting permissions.
    pub fn permissions(&self) -> &Arc<RwLock<PermissionManager>> {
        &self.permissions
    }

    /// Update conversation state with actual API-reported token usage.
    ///
    /// Called after each streaming response to keep `conversation.total_tokens`
    /// and `conversation.total_cost` in sync with the cost tracker.
    pub fn update_usage(&mut self, input_tokens: u64, output_tokens: u64, cost_usd: f64) {
        self.conversation.total_tokens = input_tokens + output_tokens;
        self.conversation.total_cost = cost_usd;
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

    // ── Rewind Conversation Tests ────────────────────────────────────

    #[test]
    fn test_rewind_conversation_single_turn() {
        let mut engine = create_test_engine();
        engine.add_user_message("Hello".to_string());
        engine.add_assistant_message(vec![crate::api::ContentBlock::Text {
            text: "Hi there".to_string(),
        }]);
        engine.add_user_message("How are you?".to_string());
        engine.add_assistant_message(vec![crate::api::ContentBlock::Text {
            text: "Fine".to_string(),
        }]);
        assert_eq!(engine.conversation.messages.len(), 4);
        assert_eq!(engine.conversation.turn_count, 0); // turn_count not auto-incremented in test

        let removed = engine.rewind_conversation(1);
        assert_eq!(removed, 2);
        assert_eq!(engine.conversation.messages.len(), 2);
        assert_eq!(engine.conversation.messages[0].role, "user");
    }

    #[test]
    fn test_rewind_conversation_multiple_turns() {
        let mut engine = create_test_engine();
        engine.add_user_message("Q1".to_string());
        engine.add_assistant_message(vec![crate::api::ContentBlock::Text {
            text: "A1".to_string(),
        }]);
        engine.add_user_message("Q2".to_string());
        engine.add_assistant_message(vec![crate::api::ContentBlock::Text {
            text: "A2".to_string(),
        }]);
        engine.add_user_message("Q3".to_string());
        engine.add_assistant_message(vec![crate::api::ContentBlock::Text {
            text: "A3".to_string(),
        }]);
        assert_eq!(engine.conversation.messages.len(), 6);

        let removed = engine.rewind_conversation(2);
        assert_eq!(removed, 4);
        assert_eq!(engine.conversation.messages.len(), 2);
    }

    #[test]
    fn test_rewind_conversation_all() {
        let mut engine = create_test_engine();
        engine.add_user_message("Q1".to_string());
        engine.add_assistant_message(vec![crate::api::ContentBlock::Text {
            text: "A1".to_string(),
        }]);

        let removed = engine.rewind_conversation(5);
        assert_eq!(removed, 2);
        assert!(engine.conversation.messages.is_empty());
    }

    #[test]
    fn test_rewind_conversation_empty() {
        let mut engine = create_test_engine();
        let removed = engine.rewind_conversation(1);
        assert_eq!(removed, 0);
        assert!(engine.conversation.messages.is_empty());
    }

    #[test]
    fn test_rewind_conversation_zero() {
        let mut engine = create_test_engine();
        engine.add_user_message("Q1".to_string());
        let removed = engine.rewind_conversation(0);
        assert_eq!(removed, 0);
        assert_eq!(engine.conversation.messages.len(), 1);
    }

    #[test]
    fn test_rewind_conversation_with_tool_messages() {
        let mut engine = create_test_engine();
        engine.add_user_message("Run tests".to_string());
        // Simulate tool result as assistant messages
        engine.add_assistant_message(vec![crate::api::ContentBlock::Text {
            text: "I'll run the tests".to_string(),
        }]);
        engine.add_assistant_message(vec![crate::api::ContentBlock::Text {
            text: "All tests passed".to_string(),
        }]);
        engine.add_user_message("Now commit".to_string());
        engine.add_assistant_message(vec![crate::api::ContentBlock::Text {
            text: "Committed".to_string(),
        }]);
        // Total: 5 messages (1 user + 2 asst + 1 user + 1 asst)
        assert_eq!(engine.conversation.messages.len(), 5);

        // Rewind 1 turn removes "Now commit" + "Committed" = 2 messages
        let removed = engine.rewind_conversation(1);
        assert_eq!(removed, 2);
        assert_eq!(engine.conversation.messages.len(), 3);

        // Rewind 1 more turn removes "Run tests" + 2 assistant messages = 3
        let removed = engine.rewind_conversation(1);
        assert_eq!(removed, 3);
        assert!(engine.conversation.messages.is_empty());
    }

    #[test]
    fn test_rewind_conversation_no_user_messages() {
        let mut engine = create_test_engine();
        // Only assistant messages, no user message to anchor a turn
        engine.add_assistant_message(vec![crate::api::ContentBlock::Text {
            text: "Hello".to_string(),
        }]);
        engine.add_assistant_message(vec![crate::api::ContentBlock::Text {
            text: "World".to_string(),
        }]);

        let removed = engine.rewind_conversation(1);
        assert_eq!(removed, 0);
        assert_eq!(engine.conversation.messages.len(), 2);
    }

    fn create_test_engine() -> QueryEngine {
        let client = create_test_client();
        let tools = ToolRegistry::new();
        let permissions = PermissionManager::new();
        let state = StateManager::new();
        let config = QueryEngineConfig::default();
        QueryEngine::new(client, tools, permissions, state, config)
    }

    // ── ContextInjector Integration Tests ──────────────────────────────

    fn temp_dir_for_test(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir()
            .join("shannon-engine-test")
            .join(name)
            .join(Uuid::new_v4().to_string());
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn test_engine_with_context_injector() {
        let project_dir = temp_dir_for_test("injector_project");
        let storage_dir = temp_dir_for_test("injector_storage");

        std::fs::write(
            project_dir.join("CLAUDE.md"),
            "# Test Project\nAlways write tests.",
        )
        .unwrap();

        let injector =
            crate::query_engine::ContextInjector::new(project_dir.clone(), storage_dir.clone());
        let engine = create_test_engine().with_context_injector(injector);

        // Should have a context injector
        assert!(engine.context_injector().is_some());

        // The injector should find project instructions
        let injector = engine.context_injector().unwrap();
        let instructions = injector.project_instructions_text();
        assert!(instructions.is_some());
        assert!(instructions.unwrap().contains("Test Project"));

        // Cleanup
        let _ = std::fs::remove_dir_all(project_dir);
        let _ = std::fs::remove_dir_all(storage_dir);
    }

    #[test]
    fn test_engine_without_context_injector() {
        let engine = create_test_engine();
        assert!(engine.context_injector().is_none());
    }

    #[test]
    fn test_engine_context_injector_preference_memory() {
        let project_dir = temp_dir_for_test("pref_project");
        let storage_dir = temp_dir_for_test("pref_storage");

        let injector =
            crate::query_engine::ContextInjector::new(project_dir.clone(), storage_dir.clone());
        let engine = create_test_engine().with_context_injector(injector);

        let injector = engine.context_injector().unwrap();
        // No preferences → empty string
        assert!(injector.preference_memory_text().is_empty());

        // Cleanup
        let _ = std::fs::remove_dir_all(project_dir);
        let _ = std::fs::remove_dir_all(storage_dir);
    }

    #[test]
    fn test_engine_context_injector_reinjection_context() {
        let project_dir = temp_dir_for_test("reinject_project");
        let storage_dir = temp_dir_for_test("reinject_storage");

        std::fs::write(
            project_dir.join("CLAUDE.md"),
            "# Reinjection Test\nUse Rust.",
        )
        .unwrap();

        let injector =
            crate::query_engine::ContextInjector::new(project_dir.clone(), storage_dir.clone());
        let engine = create_test_engine().with_context_injector(injector);

        let injector = engine.context_injector().unwrap();
        let reinjection = injector.reinjection_context();
        assert!(reinjection.contains("Reinjection Test"));
        assert!(reinjection.contains("Use Rust"));

        // Cleanup
        let _ = std::fs::remove_dir_all(project_dir);
        let _ = std::fs::remove_dir_all(storage_dir);
    }

    #[test]
    fn test_engine_context_injector_system_blocks() {
        let project_dir = temp_dir_for_test("blocks_project");
        let storage_dir = temp_dir_for_test("blocks_storage");

        std::fs::write(
            project_dir.join("CLAUDE.md"),
            "# Blocks Test\nBe concise.",
        )
        .unwrap();

        let injector =
            crate::query_engine::ContextInjector::new(project_dir.clone(), storage_dir.clone());
        let engine = create_test_engine().with_context_injector(injector);

        let injector = engine.context_injector().unwrap();
        let blocks = injector.build_system_blocks(true);
        assert!(!blocks.is_empty());
        // Should have cache_control set
        assert!(blocks[0].cache_control.is_some());

        // Cleanup
        let _ = std::fs::remove_dir_all(project_dir);
        let _ = std::fs::remove_dir_all(storage_dir);
    }
}
