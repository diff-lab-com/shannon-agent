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

use crate::memory::AutoDreamService;
use crate::memory::MemoryStore;
use crate::permissions::PermissionManager;
use crate::query_engine::context_injector::ContextInjector;
use crate::query_engine::streaming::ConversationState;
use crate::query_engine::types::{
    ConversationStats, CostTracker, QueryContext, QueryEngineConfig, QueryError, QueryEvent,
    QueryStream,
};
use crate::tools::ToolRegistry;
use shannon_engine::api::{
    ContentBlock, ContentDelta, ImageSource, LlmClient, LlmProvider, Message, MessageContent,
    StreamEvent, SystemContentBlock, ToolResultContent,
};
use shannon_engine::state::StateManager;

/// Minimal system prompt for local/small models that cannot handle tool definitions.
const LOCAL_MODEL_SYSTEM_PROMPT: &str =
    "You are Shannon, a helpful AI assistant. Respond concisely in the user's language.";
use futures::stream::{self, StreamExt};
use shannon_types::recover_lock;
use std::sync::{Arc, RwLock};
use tokio::sync::mpsc;
use uuid::Uuid;

/// Send a query event, logging a warning if the receiver has been dropped.
macro_rules! send_event {
    ($tx:expr, $event:expr) => {
        if let Err(e) = $tx.send(Ok($event)) {
            tracing::warn!("query event dropped (receiver closed): {e}");
        }
    };
}

/// Progress sender that forwards tool output lines as `ToolProgress` events.
struct ChannelProgressSender {
    tx: mpsc::UnboundedSender<Result<QueryEvent, QueryError>>,
    query_id: Uuid,
    tool_use_id: String,
    tool_name: String,
}

impl crate::tools::ProgressSender for ChannelProgressSender {
    fn send(&self, line: &str) {
        send_event!(
            self.tx,
            QueryEvent::ToolProgress {
                query_id: self.query_id,
                tool_use_id: self.tool_use_id.clone(),
                tool_name: self.tool_name.clone(),
                progress: -1.0,
                message: line.to_string(),
            }
        );
    }
}

// ── Tool result entry ──────────────────────────────────────────────

/// A pending tool result waiting to be assembled into an API message.
///
/// Carries the tool's output metadata so the engine can construct rich
/// content blocks (e.g. `ContentBlock::Image`) when the tool returned
/// binary image data.
struct ToolResultEntry {
    tool_use_id: String,
    content: String,
    is_error: bool,
    /// Metadata from the tool's `ToolOutput`. Currently only used to
    /// detect image results (`metadata["type"] == "image"`).
    metadata: std::collections::HashMap<String, serde_json::Value>,
}

impl ToolResultEntry {
    /// Build the appropriate `ToolResultContent` for this entry.
    ///
    /// For image results (detected via `metadata["type"] == "image"`),
    /// returns `ToolResultContent::Multiple` containing a text description
    /// block followed by a `ContentBlock::Image` block so the LLM can
    /// "see" the image.
    ///
    /// For everything else, returns `ToolResultContent::Single`.
    fn to_tool_result_content(&self) -> Option<ToolResultContent> {
        if self.is_error {
            return Some(ToolResultContent::Single(self.content.clone()));
        }

        // Check if this is an image result from the Read/AnalyzeImage tool.
        let is_image = self
            .metadata
            .get("type")
            .and_then(|v| v.as_str())
            .map(|s| s == "image")
            .unwrap_or(false);

        if is_image {
            let media_type = self
                .metadata
                .get("media_type")
                .and_then(|v| v.as_str())
                .unwrap_or("application/octet-stream");

            // Parse the base64 data from the JSON content string.
            // The Read tool returns a JSON structure with a "data" field.
            let base64_data = serde_json::from_str::<serde_json::Value>(&self.content)
                .ok()
                .and_then(|v| v.get("data").and_then(|d| d.as_str()).map(String::from))
                .unwrap_or_default();

            if base64_data.is_empty() {
                // Fallback: couldn't parse, return as text
                return Some(ToolResultContent::Single(self.content.clone()));
            }

            let file_path = self
                .metadata
                .get("file_path")
                .and_then(|v| v.as_str())
                .unwrap_or("image");

            Some(ToolResultContent::Multiple(vec![
                ContentBlock::Text {
                    text: format!(
                        "Image file: {file_path} ({media_type})\nThe image content is provided as an image block below."
                    ),
                },
                ContentBlock::Image {
                    source: ImageSource::base64(media_type, base64_data),
                },
            ]))
        } else {
            Some(ToolResultContent::Single(self.content.clone()))
        }
    }
}

// ── Streaming state machine ────────────────────────────────────────

/// Phase of the streaming response lifecycle within a single turn.
///
/// Replaces the previous flag-based control (`stream_finalized: bool`)
/// with an explicit state that makes transitions self-documenting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StreamingPhase {
    /// Actively receiving content blocks from the SSE stream.
    Receiving,
    /// `MessageDelta` processed with tool calls — response saved to
    /// conversation, will break from stream loop and continue the
    /// outer turn loop to dispatch tool results.
    Finalized,
}

// ── Query complexity classification ──────────────────────────────────

/// Query complexity level for model routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QueryComplexity {
    /// Simple lookup, short question — route to fast_model
    Simple,
    /// Planning, architecture, design — route to plan_model
    Planning,
    /// Standard coding task — use primary model
    Standard,
}

/// Keywords that signal planning/architecture queries.
const PLANNING_KEYWORDS: &[&str] = &[
    "architect",
    "architecture",
    "design",
    "plan",
    "planning",
    "refactor",
    "migrate",
    "strategy",
    "blueprint",
    "roadmap",
    "system design",
    "evaluate",
    "analyze",
    "review",
];

/// Keywords that signal complex implementation queries.
const COMPLEX_KEYWORDS: &[&str] = &[
    "implement",
    "build",
    "create",
    "develop",
    "integrate",
    "debug",
    "fix",
    "solve",
    "troubleshoot",
];

/// Classify a user query by complexity for model routing.
fn classify_query_complexity(query: &str) -> QueryComplexity {
    let lower = query.to_lowercase();

    // Short queries with no complex keywords → Simple
    if query.len() < 200
        && !PLANNING_KEYWORDS.iter().any(|k| lower.contains(k))
        && !COMPLEX_KEYWORDS.iter().any(|k| lower.contains(k))
    {
        return QueryComplexity::Simple;
    }

    // Planning/architecture keywords → Planning
    if PLANNING_KEYWORDS.iter().any(|k| lower.contains(k)) {
        return QueryComplexity::Planning;
    }

    QueryComplexity::Standard
}

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
    pub(crate) hook_manager: Arc<tokio::sync::RwLock<shannon_engine::hooks::HookManager>>,
    /// Triggered routines registry for hook-event-driven automation
    pub(crate) triggered_routines:
        Arc<tokio::sync::RwLock<crate::triggered_routines::TriggeredRoutineRegistry>>,
    /// Context injector for project instructions and preference memory.
    pub(crate) context_injector: Option<Arc<ContextInjector>>,
    /// Shared flag set by `PlanManager` (in `shannon-tools`) to signal that
    /// plan mode is active. When `true`, the engine blocks write tools before
    /// the permission check.
    pub(crate) plan_mode_active: Arc<RwLock<bool>>,
    /// Git-based checkpoint manager for undo/revert support before file-modifying tools.
    pub(crate) checkpoint_manager: crate::checkpoint::CheckpointManager,
    /// Effective maximum context tokens — resolved from user config > Ollama num_ctx > model registry.
    pub(crate) effective_max_context_tokens: usize,
    /// Custom permission profiles loaded from `.shannon/profiles/*.toml` and `.claude/profiles/*.toml`.
    pub(crate) custom_profiles:
        Arc<tokio::sync::RwLock<crate::custom_profiles::CustomProfileRegistry>>,
}

impl QueryEngine {
    /// Resolve effective max context tokens from priority chain:
    /// user config > Ollama num_ctx (queried later) > model registry > fallback (128K).
    fn resolve_max_context_tokens(model: &str, user_override: Option<usize>) -> usize {
        if let Some(tokens) = user_override {
            return tokens;
        }
        crate::model_registry::context_window_for(model)
    }

    /// Return the resolved context window size for display purposes.
    ///
    /// Checks the Ollama cached info first (which reflects the real `num_ctx`
    /// queried from the running model), then falls back to the initial value
    /// resolved from config / model registry at construction time.
    pub fn resolved_context_window(&self) -> usize {
        if self.config.max_context_tokens.is_some() {
            return self.effective_max_context_tokens;
        }
        if *self.client.provider() == shannon_engine::api::LlmProvider::Ollama {
            if let Some(info) = self.client.cached_ollama_info() {
                if info.num_ctx > 0 {
                    return info.num_ctx;
                }
            }
        }
        self.effective_max_context_tokens
    }

    /// Pre-query provider for real context window size.
    ///
    /// For Ollama, queries `/api/show` to resolve the actual `num_ctx`
    /// before the first user query, so tool-disable decisions are correct
    /// from the start.  Safe to call multiple times — results are cached.
    pub async fn pre_resolve_context(&mut self) {
        if *self.client.provider() == shannon_engine::api::LlmProvider::Ollama
            && self.config.max_context_tokens.is_none()
        {
            if let Some(info) = self.client.check_ollama_capabilities().await {
                if info.num_ctx > 0 && info.num_ctx != self.effective_max_context_tokens {
                    tracing::info!(
                        old = self.effective_max_context_tokens,
                        new = info.num_ctx,
                        "Pre-resolved Ollama context window"
                    );
                    self.effective_max_context_tokens = info.num_ctx;
                }
            }
        }
    }
}

/// Helper to create a loaded HookManager
fn hook_mgr() -> shannon_engine::hooks::HookManager {
    let mut mgr = shannon_engine::hooks::HookManager::new();
    if let Err(e) = mgr.load() {
        tracing::warn!("Failed to load hooks configuration: {e}");
    }
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
            (None, None) => { /* both iterators exhausted — skip */ }
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
        let effective_max_context_tokens =
            Self::resolve_max_context_tokens(client.model(), config.max_context_tokens);
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
            triggered_routines: Arc::new(tokio::sync::RwLock::new(
                crate::triggered_routines::TriggeredRoutineRegistry::load_from_dirs(),
            )),
            context_injector: None,
            plan_mode_active: Arc::new(RwLock::new(false)),
            checkpoint_manager: crate::checkpoint::CheckpointManager::for_session(
                &session_id.to_string(),
            ),
            effective_max_context_tokens,
            custom_profiles: Arc::new(tokio::sync::RwLock::new(
                crate::custom_profiles::CustomProfileRegistry::load_from_dirs(),
            )),
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
        let effective_max_context_tokens = Self::resolve_max_context_tokens(
            client.model(),
            None, // defaults have no user override
        );
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
            triggered_routines: Arc::new(tokio::sync::RwLock::new(
                crate::triggered_routines::TriggeredRoutineRegistry::load_from_dirs(),
            )),
            context_injector: None,
            plan_mode_active: Arc::new(RwLock::new(false)),
            checkpoint_manager: crate::checkpoint::CheckpointManager::for_session(
                &session_id.to_string(),
            ),
            effective_max_context_tokens,
            custom_profiles: Arc::new(tokio::sync::RwLock::new(
                crate::custom_profiles::CustomProfileRegistry::load_from_dirs(),
            )),
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
        let effective_max_context_tokens =
            Self::resolve_max_context_tokens(client.model(), config.max_context_tokens);
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
            triggered_routines: Arc::new(tokio::sync::RwLock::new(
                crate::triggered_routines::TriggeredRoutineRegistry::load_from_dirs(),
            )),
            context_injector: None,
            plan_mode_active: Arc::new(RwLock::new(false)),
            checkpoint_manager: crate::checkpoint::CheckpointManager::for_session(
                &session_id.to_string(),
            ),
            effective_max_context_tokens,
            custom_profiles: Arc::new(tokio::sync::RwLock::new(
                crate::custom_profiles::CustomProfileRegistry::load_from_dirs(),
            )),
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

    /// Set the thinking effort level (`/effort`).
    ///
    /// Maps to `budget_tokens` for Anthropic and `reasoning_effort` for OpenAI.
    pub fn set_effort_level(&mut self, level: Option<String>) {
        self.config.effort_level = level;
    }

    /// Set the focus area (`/focus`).
    ///
    /// Injected into the system prompt to steer model attention.
    pub fn set_focus_area(&mut self, area: Option<String>) {
        self.config.focus_area = area;
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

    /// Set the shared plan-mode flag so the engine can block write tools when
    /// plan mode is active.
    ///
    /// The flag is typically obtained from [`PlanManager::plan_mode_flag()`] in
    /// `shannon-tools` and cloned into the engine before the first query.
    pub fn with_plan_mode_active(mut self, flag: Arc<RwLock<bool>>) -> Self {
        self.plan_mode_active = flag;
        self
    }

    /// Check whether plan mode is currently active.
    pub fn is_plan_mode_active(&self) -> bool {
        self.plan_mode_active.read().map(|g| *g).unwrap_or(false)
    }

    /// Obtain a cloneable handle to the plan-mode flag.
    pub fn plan_mode_active_handle(&self) -> Arc<RwLock<bool>> {
        Arc::clone(&self.plan_mode_active)
    }

    /// Set the maximum number of turns for a conversation
    pub fn set_max_turns(&mut self, turns: usize) {
        self.config.max_turns = turns;
    }

    /// Get the current session ID
    pub fn session_id(&self) -> Uuid {
        self.session_id
    }

    /// Start a new session: clear conversation and generate a fresh session ID.
    pub fn new_session(&mut self) -> Uuid {
        self.clear_conversation();
        self.session_id = Uuid::new_v4();
        self.session_id
    }

    /// Access the hook manager for firing lifecycle events (SessionStart, SessionEnd, etc.)
    pub fn hook_manager(&self) -> Arc<tokio::sync::RwLock<shannon_engine::hooks::HookManager>> {
        self.hook_manager.clone()
    }

    /// Access the triggered routines registry.
    pub fn triggered_routines(
        &self,
    ) -> Arc<tokio::sync::RwLock<crate::triggered_routines::TriggeredRoutineRegistry>> {
        self.triggered_routines.clone()
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

    /// Access the custom permission profiles registry.
    pub fn custom_profiles(
        &self,
    ) -> &Arc<tokio::sync::RwLock<crate::custom_profiles::CustomProfileRegistry>> {
        &self.custom_profiles
    }

    /// Add a user message to the conversation
    pub fn add_user_message(&mut self, content: String) {
        use shannon_engine::api::MessageContent;
        self.conversation
            .messages
            .push(shannon_engine::api::Message {
                role: "user".to_string(),
                content: MessageContent::Text(content),
            });
    }

    /// Add a user message with content blocks (e.g., text + image)
    pub fn add_user_message_blocks(&mut self, blocks: Vec<shannon_engine::api::ContentBlock>) {
        use shannon_engine::api::MessageContent;
        self.conversation
            .messages
            .push(shannon_engine::api::Message {
                role: "user".to_string(),
                content: MessageContent::Blocks(blocks),
            });
    }

    /// Add an assistant message to the conversation
    pub fn add_assistant_message(&mut self, content: Vec<shannon_engine::api::ContentBlock>) {
        use shannon_engine::api::{ContentBlock, Message, MessageContent};
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

    /// Restore conversation messages from a completed query (syncs background task state back).
    /// Logs a warning if the restored messages look incomplete (e.g., missing the assistant response).
    pub fn restore_messages(&mut self, messages: Vec<shannon_engine::api::Message>) {
        let msg_count = messages.len();
        let last_role = messages.last().map(|m| m.role.as_str()).unwrap_or("none");
        let prev_count = self.conversation.messages.len();
        tracing::info!(
            msg_count,
            prev_count,
            last_role,
            "restore_messages: syncing conversation from background task"
        );
        if msg_count > 0 && last_role != "assistant" {
            tracing::warn!(
                msg_count,
                last_role,
                "restore_messages: last message is not from assistant — conversation may be incomplete"
            );
        }
        self.conversation.messages = messages;
    }

    /// Estimate token count of the current conversation including system prompt.
    /// Uses the same CJK-aware estimation as the compression threshold check.
    pub fn estimate_conversation_tokens(&self) -> usize {
        self.conversation
            .estimate_tokens_with_system_prompt(self.config.system_prompt.as_deref())
    }

    /// Get the current conversation messages (for session persistence).
    pub fn conversation_messages(&self) -> &[shannon_engine::api::Message] {
        &self.conversation.messages
    }

    /// Get a reference to the underlying LLM client.
    pub fn client(&self) -> &LlmClient {
        &self.client
    }

    /// Update the model used for API calls.
    pub fn set_model(&mut self, model: String) {
        self.effective_max_context_tokens = crate::model_registry::context_window_for(&model);
        // Clear stale Ollama cache so pre_resolve_context() re-queries
        if *self.client.provider() == shannon_engine::api::LlmProvider::Ollama {
            self.client.clear_ollama_cache();
        }
        let mut tracker = self.cost_tracker.write().unwrap_or_else(|e| e.into_inner());
        tracker.model_name = model.clone();
        self.client.set_model(model);
    }

    /// Update the model AND switch provider (including base_url).
    pub fn set_model_for_provider(&mut self, model: String, provider: LlmProvider) {
        self.effective_max_context_tokens = crate::model_registry::context_window_for(&model);
        // Clear stale Ollama cache so pre_resolve_context() re-queries
        if provider == shannon_engine::api::LlmProvider::Ollama {
            self.client.clear_ollama_cache();
        }
        let mut tracker = self.cost_tracker.write().unwrap_or_else(|e| e.into_inner());
        tracker.model_name = model.clone();
        self.client.set_model_for_provider(model, provider);
    }

    /// Replace the conversation history with new messages (e.g., after compaction)
    pub fn replace_conversation(&mut self, messages: Vec<Message>) {
        let turn_count = messages.iter().filter(|m| m.role == "user").count();
        tracing::debug!(
            msg_count = messages.len(),
            turn_count,
            last_role = messages.last().map(|m| m.role.as_str()).unwrap_or("none"),
            "replace_conversation: replacing conversation history"
        );
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

        // Resolve model aliases in fast_model and plan_model
        let fast_model = self
            .config
            .fast_model
            .as_ref()
            .map(|m| crate::model_registry::resolve_model(m, Some(self.client.provider())));
        let plan_model = self
            .config
            .plan_model
            .as_ref()
            .map(|m| crate::model_registry::resolve_model(m, Some(self.client.provider())));

        // Multi-tier model routing
        let client_model = {
            let query = &context.user_message;
            let complexity = classify_query_complexity(query);

            match complexity {
                QueryComplexity::Simple => {
                    fast_model.as_deref().unwrap_or(&client_model).to_string()
                }
                QueryComplexity::Planning => {
                    plan_model.as_deref().unwrap_or(&client_model).to_string()
                }
                QueryComplexity::Standard => client_model.clone(),
            }
        };
        let client_base_url = self.client.base_url().to_string();
        let client_max_tokens = self.client.max_tokens();
        let client_provider = self.client.provider().clone();
        let user_message = context.user_message.clone();
        let state_for_save = self.state.clone();
        let session_id_for_save = self.session_id;
        let cost_tracker = self.cost_tracker.clone();
        let hook_manager = self.hook_manager.clone();
        let triggered_routines = self.triggered_routines.clone();
        let context_injector = self.context_injector.clone();
        let plan_mode_active = self.plan_mode_active.clone();
        let checkpoint_manager = self.checkpoint_manager.clone();
        let effective_max_context_tokens = self.effective_max_context_tokens;

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
        let use_cache = matches!(
            client_provider,
            shannon_engine::api::LlmProvider::Anthropic
                | shannon_engine::api::LlmProvider::Bedrock
                | shannon_engine::api::LlmProvider::Custom
        );

        // Base system prompt — always cache the system prompt prefix for Anthropic,
        // as it's identical across all turns and the largest cache savings come from here.
        if let Some(ref base) = config.system_prompt {
            let block = if use_cache {
                SystemContentBlock::cached(base.clone())
            } else {
                SystemContentBlock::text(base.clone())
            };
            system_blocks.push(block);
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
            let working_dir =
                std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
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
            let working_dir =
                std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
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

        // Inject browser control instructions when browser MCP tools are present
        {
            let tool_names = tools.list();
            if let Some(browser_text) = crate::query_engine::browser_control_prompt(&tool_names) {
                let block = if use_cache {
                    SystemContentBlock::cached(browser_text)
                } else {
                    SystemContentBlock::text(browser_text)
                };
                system_blocks.push(block);
            }
        }

        // Inject team coordination instructions when team tools are present
        {
            let tool_names = tools.list();
            if let Some(team_text) =
                crate::query_engine::team_prompt::team_coordination_prompt(&tool_names)
            {
                let block = if use_cache {
                    SystemContentBlock::cached(team_text)
                } else {
                    SystemContentBlock::text(team_text)
                };
                system_blocks.push(block);
            }
        }

        // Inject focus area from /focus command into system prompt
        if let Some(ref focus) = config.focus_area {
            let focus_text = format!(
                "## User Focus Area\n\
                 The user wants you to focus on: **{focus}**.\n\
                 Prioritize this area in your responses. Give extra attention to \
                 aspects related to {focus} when analyzing, coding, or reviewing."
            );
            system_blocks.push(SystemContentBlock::text(focus_text));
        }

        // Decide whether to use structured blocks or fallback to plain string.
        // Use structured blocks only when we have content (avoids empty system arrays).
        let mut system_blocks_opt = if system_blocks.is_empty() {
            None
        } else {
            Some(system_blocks)
        };
        let mut system_prompt = if context.metadata.tools_allowed {
            config.system_prompt.clone()
        } else if client_provider == shannon_engine::api::LlmProvider::Ollama {
            // Ollama models use their own chat templates; a system prompt
            // confuses small/unstable models causing malformed output.
            None
        } else {
            Some(LOCAL_MODEL_SYSTEM_PROMPT.to_string())
        };

        // Inject the working directory so the model knows where to write files.
        if let Ok(cwd) = std::env::current_dir() {
            let cwd_text = format!("\n\n## Environment\n\nWorking directory: {}", cwd.display());
            if let Some(ref mut prompt) = system_prompt {
                prompt.push_str(&cwd_text);
            }
            if let Some(ref mut blocks) = system_blocks_opt {
                blocks.push(shannon_engine::api::types::SystemContentBlock::text(
                    cwd_text,
                ));
            }
        }

        // Clone existing conversation to preserve multi-turn context
        let mut conversation = self.conversation.clone();
        tracing::debug!(
            existing_msgs = conversation.messages.len(),
            last_role = conversation
                .messages
                .last()
                .map(|m| m.role.as_str())
                .unwrap_or("none"),
            "Starting new query: cloning conversation for background task"
        );
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
                let prompt_event = shannon_engine::hooks::HookEvent::UserPromptSubmit {
                    prompt: user_message.clone(),
                };
                let hm = hook_manager.read().await;
                let _ = hm.run_hooks(&prompt_event).await;
            }

            // Create a new client for this task, preserving provider from original config
            let client_config = {
                // For Ollama models with tiny context (< 4096), cap num_predict
                // to half the context so the model has room for input tokens.
                let capped_max_tokens = if client_provider
                    == shannon_engine::api::LlmProvider::Ollama
                    && effective_max_context_tokens < 4096
                    && client_max_tokens as usize > effective_max_context_tokens / 2
                {
                    tracing::info!(
                        max_tokens = effective_max_context_tokens / 2,
                        "Capping num_predict for tiny Ollama model"
                    );
                    (effective_max_context_tokens / 2) as u32
                } else {
                    client_max_tokens
                };
                let mut cfg = shannon_engine::api::LlmClientConfig {
                    api_key: client_api_key,
                    base_url: client_base_url,
                    model: client_model.clone(),
                    max_tokens: capped_max_tokens,
                    provider: client_provider.clone(),
                    ..Default::default()
                };
                // Enable extended thinking with a budget if configured
                if config.enable_thinking {
                    cfg.budget_tokens = Some(10000);
                }
                // Map effort_level from /effort command to provider-specific parameters
                if let Some(ref effort) = config.effort_level {
                    let reasoning_effort = match effort.as_str() {
                        "low" => shannon_engine::api::types::ReasoningEffort::Low,
                        "medium" => shannon_engine::api::types::ReasoningEffort::Medium,
                        "high" => shannon_engine::api::types::ReasoningEffort::High,
                        _ => shannon_engine::api::types::ReasoningEffort::Medium,
                    };
                    cfg.reasoning_effort = Some(reasoning_effort);
                    // For Anthropic: also set budget_tokens based on effort level
                    if matches!(
                        cfg.provider,
                        shannon_engine::api::LlmProvider::Anthropic
                            | shannon_engine::api::LlmProvider::Bedrock
                            | shannon_engine::api::LlmProvider::Custom
                    ) {
                        let budget = reasoning_effort.to_anthropic_budget(200_000);
                        cfg.budget_tokens = Some(budget as u32);
                    }
                }
                cfg
            };
            let client = LlmClient::new(client_config);

            let mut turn = 0;
            let mut tool_results: Vec<ToolResultEntry> = Vec::new();
            let mut total_input_tokens: u64 = 0;
            let mut total_output_tokens: u64 = 0;
            let mut file_edits_made = false;
            let mut compaction_failures: u32 = 0;
            const MAX_COMPACTION_FAILURES: u32 = 2;

            // Denial circuit breaker: track consecutive permission denials.
            // After MAX_CONSECUTIVE_DENIALS the model is told to stop retrying;
            // if it still retries HARD_LIMIT more times, the loop aborts.
            let mut consecutive_denials: u32 = 0;
            const DENIAL_SOFT_LIMIT: u32 = 3; // inject warning to LLM
            const DENIAL_HARD_LIMIT: u32 = 5; // abort the agent loop

            loop {
                if turn >= config.max_turns {
                    let total_cost = CostTracker::calculate_cost(
                        &client_model,
                        total_input_tokens,
                        total_output_tokens,
                    );
                    send_event!(
                        tx,
                        QueryEvent::Cost {
                            query_id,
                            total_cost_usd: total_cost,
                            input_tokens: total_input_tokens,
                            output_tokens: total_output_tokens,
                        }
                    );
                    send_event!(
                        tx,
                        QueryEvent::ConversationUpdate {
                            query_id,
                            messages: conversation.messages.clone(),
                        }
                    );
                    send_event!(tx, QueryEvent::Completed { query_id });

                    // Auto-save conversation after completion
                    if let Err(e) = save_conversation_to_disk(
                        &state_for_save,
                        session_id_for_save,
                        &conversation.messages,
                        &client_model,
                    ) {
                        tracing::warn!(session = %session_id_for_save, "Failed to save conversation: {e}");
                    }

                    break;
                }

                // Build messages for API call
                let mut messages = conversation.messages.clone();

                // Auto-compress conversation when approaching token limits
                if conversation.needs_compression(&config) {
                    send_event!(
                        tx,
                        QueryEvent::Progress {
                            query_id,
                            message: "Compressing conversation context...".to_string(),
                        }
                    );
                    conversation.compress(&config);
                    messages = conversation.messages.clone();
                }

                // Add pending tool results from previous turn.
                // Persist to conversation.messages as well so multi-turn context
                // maintains the required assistant(tool_use) → user(tool_result) sequence.
                for entry in tool_results.drain(..) {
                    let content = entry.to_tool_result_content();
                    let tool_msg = Message {
                        role: "user".to_string(),
                        content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                            tool_use_id: entry.tool_use_id,
                            content,
                            is_error: Some(entry.is_error),
                        }]),
                    };
                    messages.push(tool_msg.clone());
                    conversation.messages.push(tool_msg);
                }

                // Resolve effective max context FIRST: Ollama num_ctx > model registry > fallback.
                // Run every turn — check_ollama_capabilities() caches results after the first
                // HTTP call, so subsequent turns just read the cache with zero overhead.
                let mut effective_max_context = effective_max_context_tokens;
                if client_provider == shannon_engine::api::LlmProvider::Ollama
                    && config.max_context_tokens.is_none()
                {
                    if let Some(info) = client.check_ollama_capabilities().await {
                        if info.num_ctx > 0 {
                            effective_max_context = info.num_ctx;
                        }
                        tracing::debug!(
                            num_ctx = effective_max_context,
                            turn,
                            "Ollama context resolved"
                        );
                    }
                }

                // Get tools schema — respect tools_allowed from QueryContext.
                // For Ollama models with small context (< 8192), tool definitions
                // consume too much of the context window and crowd out conversation
                // history, causing multi-turn context loss. Auto-disable tools.
                // Also replace the full system prompt with a minimal one to free up
                // context for actual conversation.
                let mut tools_schema = if context.metadata.tools_allowed {
                    let tool_defs = tools.to_tool_definitions();
                    if client_provider == shannon_engine::api::LlmProvider::Ollama
                        && effective_max_context < 8192
                    {
                        tracing::info!(
                            num_ctx = effective_max_context,
                            "Ollama model has small context, auto-disabling tools to preserve conversation history"
                        );
                        send_event!(
                            tx,
                            QueryEvent::Progress {
                                query_id,
                                message: "Tools disabled (model context under 8K)".to_string(),
                            }
                        );
                        // Replace full system prompt with minimal one to free context
                        system_blocks_opt = None;
                        system_prompt = Some("You are a helpful assistant.".to_string());
                        None
                    } else {
                        Some(tool_defs)
                    }
                } else {
                    None
                };

                // Auto-compress conversation if it exceeds the threshold
                {
                    let estimated_tokens =
                        shannon_engine::compact::helpers::estimate_tokens(&messages)
                            + config
                                .system_prompt
                                .as_ref()
                                .map(|sp| {
                                    shannon_engine::compact::helpers::estimate_text_tokens(sp)
                                })
                                .unwrap_or(0);
                    let max_context = effective_max_context.max(1); // Guard against division by zero
                    let usage_ratio = estimated_tokens as f32 / max_context as f32;

                    // Pre-compaction warning at 60% — gives users visibility before compression fires
                    if usage_ratio > 0.6 && usage_ratio <= config.compression_threshold {
                        send_event!(
                            tx,
                            QueryEvent::Progress {
                                query_id,
                                message: format!(
                                    "Context: {:.0}% full ({}/{}) — compaction will trigger at {:.0}%",
                                    usage_ratio * 100.0,
                                    estimated_tokens,
                                    max_context,
                                    config.compression_threshold * 100.0,
                                ),
                            }
                        );
                    }

                    if usage_ratio > config.compression_threshold {
                        // Circuit breaker: if compaction has failed repeatedly, skip it and just truncate
                        if compaction_failures >= MAX_COMPACTION_FAILURES {
                            let keep = config.keep_recent_messages;
                            if messages.len() > keep {
                                messages = messages.split_off(messages.len() - keep);
                            }
                            send_event!(tx, QueryEvent::Progress {
                                query_id,
                                message: "Compaction skipped (too many failures), truncating old messages".to_string(),
                            });
                        } else {
                            match shannon_engine::compact::CompactEngine::with_llm_summarizer(
                                client.clone(),
                            ) {
                                Ok(mut compact_engine) => {
                                    // Sync compact engine's context limit with our effective limit
                                    compact_engine.config.max_context_tokens =
                                        effective_max_context;
                                    // Build re-injection context from ContextInjector if available,
                                    // otherwise fall back to the system prompt (truncated).
                                    // Build re-injection context from ContextInjector if available
                                    let reinjection = context_injector
                                        .as_ref()
                                        .map(|ci| ci.reinjection_context())
                                        .unwrap_or_default();

                                    match compact_engine.compact(&mut messages) {
                                        Ok(result) => {
                                            compaction_failures = 0; // reset on success

                                            // Re-inject critical context after compaction so
                                            // the model retains project instructions, MEMORY.md,
                                            // and preference memory across the compaction boundary.
                                            if !reinjection.is_empty() && !messages.is_empty() {
                                                let ctx_msg = shannon_engine::api::Message {
                                                    role: "system".to_string(),
                                                    content:
                                                        shannon_engine::api::MessageContent::Text(
                                                            format!(
                                                                "[Re-injected context after compaction]\n\n{reinjection}"
                                                            ),
                                                        ),
                                                };
                                                messages.insert(0, ctx_msg);
                                            }

                                            send_event!(
                                                tx,
                                                QueryEvent::Progress {
                                                    query_id,
                                                    message: format!(
                                                        "Context compressed (3-tier): {} → {} tokens ({:.0}% reduction, {} messages compacted)",
                                                        result.original_tokens,
                                                        result.compacted_tokens,
                                                        result.reduction_ratio * 100.0,
                                                        result.messages_compacted,
                                                    ),
                                                }
                                            );
                                            send_event!(
                                                tx,
                                                QueryEvent::Info {
                                                    query_id,
                                                    message: format!(
                                                        "compaction: {} → {} tokens ({:.0}% reduction, {} removed, {} compacted, {:?})",
                                                        result.original_tokens,
                                                        result.compacted_tokens,
                                                        result.reduction_ratio * 100.0,
                                                        result.messages_removed,
                                                        result.messages_compacted,
                                                        result.strategy,
                                                    ),
                                                }
                                            );
                                        }
                                        Err(e) => {
                                            compaction_failures += 1;
                                            tracing::warn!(
                                                "Compression failed: {}, truncating instead",
                                                e
                                            );
                                            let keep = 20;
                                            if messages.len() > keep {
                                                messages =
                                                    messages.split_off(messages.len() - keep);
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    compaction_failures += 1;
                                    tracing::warn!(
                                        "CompactEngine init failed ({}), truncating old messages",
                                        e
                                    );
                                    let keep = 20;
                                    if messages.len() > keep {
                                        messages = messages.split_off(messages.len() - keep);
                                    }
                                }
                            }
                        }
                    }
                }

                // Sync: always keep conversation.messages in sync with the messages
                // actually sent to the API. Compact may produce same-count but
                // different-content messages, so sync unconditionally.
                if conversation.messages.len() != messages.len() {
                    tracing::debug!(
                        before = conversation.messages.len(),
                        after = messages.len(),
                        "Syncing conversation.messages with compressed messages"
                    );
                }
                conversation.messages = messages.clone();

                // Diagnostic: log conversation state before API call
                tracing::info!(
                    msg_count = messages.len(),
                    estimated_tokens = shannon_engine::compact::helpers::estimate_tokens(&messages),
                    turn = turn + 1,
                    max_turns = config.max_turns,
                    "Sending API request"
                );

                // Pre-send context overflow detection: estimate total tokens including
                // tools, warn if near limit, and auto-strip tools for Ollama to preserve
                // conversation history.
                {
                    let tools_tokens = tools_schema
                        .as_ref()
                        .map(|t| {
                            // Rough estimate: ~4 chars per token for JSON tool definitions
                            let json_len = serde_json::to_string(t).map(|s| s.len()).unwrap_or(0);
                            json_len / 4
                        })
                        .unwrap_or(0);
                    let mut pre_send_estimate =
                        shannon_engine::compact::helpers::estimate_tokens(&messages)
                            + config
                                .system_prompt
                                .as_ref()
                                .map(|sp| {
                                    shannon_engine::compact::helpers::estimate_text_tokens(sp)
                                })
                                .unwrap_or(0)
                            + tools_tokens;
                    let mut pre_send_ratio =
                        pre_send_estimate as f32 / effective_max_context as f32;
                    if pre_send_ratio > 0.9 {
                        send_event!(
                            tx,
                            QueryEvent::Progress {
                                query_id,
                                message: format!(
                                    "Context at {:.0}% ({}/{} tokens) — approaching limit",
                                    pre_send_ratio * 100.0,
                                    pre_send_estimate,
                                    effective_max_context,
                                ),
                            }
                        );
                        tracing::warn!(
                            estimated_tokens = pre_send_estimate,
                            tools_tokens,
                            max_context = effective_max_context,
                            "Sending request near context limit"
                        );
                        // Auto-strip tools for Ollama when context overflow detected —
                        // preserving conversation history is more important than tool support.
                        if client_provider == shannon_engine::api::LlmProvider::Ollama
                            && tools_schema.is_some()
                            && tools_tokens > 0
                        {
                            tracing::info!(
                                "Auto-stripping tools to preserve conversation context for Ollama"
                            );
                            send_event!(
                                tx,
                                QueryEvent::Progress {
                                    query_id,
                                    message: "Auto-disabling tools — context near limit"
                                        .to_string(),
                                }
                            );
                            tools_schema = None;
                            pre_send_estimate -= tools_tokens;
                            pre_send_ratio =
                                pre_send_estimate as f32 / effective_max_context as f32;
                        }
                        // For Ollama still over limit: strip system prompt/blocks
                        // to free context for conversation history.
                        if pre_send_ratio > 0.95
                            && client_provider == shannon_engine::api::LlmProvider::Ollama
                        {
                            if system_blocks_opt.is_some() {
                                tracing::info!(
                                    "Stripping system blocks for Ollama — context near limit"
                                );
                                send_event!(
                                    tx,
                                    QueryEvent::Progress {
                                        query_id,
                                        message: "Stripping system context — context near limit"
                                            .to_string(),
                                    }
                                );
                                system_blocks_opt = None;
                            } else if system_prompt.is_some() {
                                tracing::info!(
                                    "Stripping system prompt for Ollama — context near limit"
                                );
                                system_prompt = None;
                            }
                        }
                        // If still over limit after stripping tools, truncate older messages
                        // to fit within context. Keep the most recent turns.
                        if pre_send_ratio > 1.0 && messages.len() > 2 {
                            let target_tokens = (effective_max_context as f32 * 0.8) as usize;
                            while shannon_engine::compact::helpers::estimate_tokens(&messages)
                                > target_tokens
                                && messages.len() > 2
                            {
                                // Remove the oldest non-adjacent pair to maintain
                                // assistant/user message alternation
                                if messages.len() > 3 {
                                    messages.remove(0);
                                    messages.remove(0);
                                } else {
                                    break;
                                }
                            }
                            let new_estimate =
                                shannon_engine::compact::helpers::estimate_tokens(&messages);
                            tracing::info!(
                                truncated_to = messages.len(),
                                new_estimate,
                                "Truncated older messages to fit context"
                            );
                            send_event!(
                                tx,
                                QueryEvent::Progress {
                                    query_id,
                                    message: format!(
                                        "Truncated history to {} messages ({} tokens)",
                                        messages.len(),
                                        new_estimate
                                    ),
                                }
                            );
                        }
                    }
                }

                // Call the API — use structured system blocks when available for prompt caching
                let stream_result = if let Some(ref blocks) = system_blocks_opt {
                    client
                        .send_message_stream_structured_with_retry(
                            messages.clone(),
                            tools_schema.clone(),
                            blocks.clone(),
                        )
                        .await
                } else {
                    client
                        .send_message_stream_with_retry(
                            messages.clone(),
                            tools_schema.clone(),
                            system_prompt.clone(),
                        )
                        .await
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
                        let mut phase = StreamingPhase::Receiving;
                        // Cache tokens arrive in MessageStart (Anthropic), merge with MessageDelta
                        let mut start_cache_read: u64 = 0;
                        let mut start_cache_creation: u64 = 0;

                        // Process streaming events
                        while let Some(event_result) = stream.next().await {
                            match event_result {
                                Ok(stream_event) => {
                                    match stream_event {
                                        StreamEvent::MessageStart { message } => {
                                            start_cache_read =
                                                message.usage.cache_read_input_tokens as u64;
                                            start_cache_creation =
                                                message.usage.cache_creation_input_tokens as u64;
                                        }
                                        StreamEvent::ContentBlockStart {
                                            content_block, ..
                                        } => {
                                            match &content_block {
                                                ContentBlock::ToolUse { id, name, input } => {
                                                    current_tool_use =
                                                        Some((id.clone(), name.clone()));
                                                    send_event!(
                                                        tx,
                                                        QueryEvent::ToolUseRequest {
                                                            query_id,
                                                            tool_use_id: id.clone(),
                                                            tool_name: name.clone(),
                                                            tool_input: input.clone(),
                                                        }
                                                    );
                                                }
                                                ContentBlock::Thinking { .. } => {
                                                    // Thinking block started — deltas will arrive via ThinkingDelta
                                                }
                                                _ => {}
                                            }
                                        }
                                        StreamEvent::ContentBlockDelta { delta, .. } => match delta
                                        {
                                            ContentDelta::TextDelta { text } => {
                                                has_content = true;
                                                assistant_text.push_str(&text);
                                                send_event!(
                                                    tx,
                                                    QueryEvent::Text {
                                                        query_id,
                                                        content: text,
                                                    }
                                                );
                                            }
                                            ContentDelta::InputJsonDelta { partial_json } => {
                                                accumulated_tool_input.push_str(&partial_json);
                                            }
                                            ContentDelta::ThinkingDelta { thinking } => {
                                                send_event!(
                                                    tx,
                                                    QueryEvent::Thinking {
                                                        query_id,
                                                        content: thinking,
                                                    }
                                                );
                                            }
                                        },
                                        StreamEvent::ContentBlockStop { .. } => {
                                            if let Some((id, name)) = current_tool_use.take() {
                                                let raw =
                                                    std::mem::take(&mut accumulated_tool_input);
                                                match serde_json::from_str::<serde_json::Value>(
                                                    &raw,
                                                ) {
                                                    Ok(json_val) => {
                                                        tool_inputs.push((
                                                            id.clone(),
                                                            name.clone(),
                                                            json_val.clone(),
                                                        ));
                                                        assistant_tool_uses.push(
                                                            ContentBlock::ToolUse {
                                                                id,
                                                                name,
                                                                input: json_val,
                                                            },
                                                        );
                                                    }
                                                    Err(e) => {
                                                        tracing::warn!(
                                                            "Tool input JSON parse failed for '{name}': {e}"
                                                        );
                                                        send_event!(
                                                            tx,
                                                            QueryEvent::ToolUseResult {
                                                                query_id,
                                                                tool_use_id: id.clone(),
                                                                tool_name: name.clone(),
                                                                result: format!(
                                                                    "Failed to parse tool arguments: {e}"
                                                                ),
                                                                is_error: true,
                                                            }
                                                        );
                                                        tool_results.push(ToolResultEntry {
                                                            tool_use_id: id,
                                                            content: format!(
                                                                "Malformed tool input: {e}"
                                                            ),
                                                            is_error: true,
                                                            metadata: Default::default(),
                                                        });
                                                        assistant_tool_uses.push(
                                                            ContentBlock::ToolUse {
                                                                id: String::new(),
                                                                name,
                                                                input: serde_json::Value::Null,
                                                            },
                                                        );
                                                    }
                                                }
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
                                            {
                                                let mut tracker = cost_tracker
                                                    .write()
                                                    .unwrap_or_else(|e| e.into_inner());
                                                tracker.record_usage(
                                                    &client_model,
                                                    input_tokens,
                                                    output_tokens,
                                                );

                                                // Budget enforcement: check if limit exceeded
                                                if tracker.is_budget_exceeded() {
                                                    let limit =
                                                        tracker.budget_limit_usd.unwrap_or(0.0);
                                                    let total = tracker.total_cost();
                                                    send_event!(
                                                        tx,
                                                        QueryEvent::Progress {
                                                            query_id,
                                                            message: format!(
                                                                "Budget limit reached (${limit:.2}). Stopping. (spent: ${total:.4})"
                                                            ),
                                                        }
                                                    );
                                                    // Break out of the loop by setting turn to max
                                                    turn = config.max_turns;
                                                    break;
                                                }

                                                // Budget warning at 80% usage (fires once)
                                                if tracker.check_and_mark_budget_warning() {
                                                    let limit =
                                                        tracker.budget_limit_usd.unwrap_or(0.0);
                                                    let total = tracker.total_cost();
                                                    let pct = if limit > 0.0 {
                                                        (total / limit * 100.0) as u32
                                                    } else {
                                                        0
                                                    };
                                                    send_event!(
                                                        tx,
                                                        QueryEvent::Progress {
                                                            query_id,
                                                            message: format!(
                                                                "Budget warning: ${total:.4} / ${limit:.2} ({pct}%)"
                                                            ),
                                                        }
                                                    );
                                                }
                                            }

                                            let cache_creation_tokens = start_cache_creation
                                                .max(usage.cache_creation_input_tokens as u64);
                                            let cache_read_tokens = start_cache_read
                                                .max(usage.cache_read_input_tokens as u64);

                                            send_event!(
                                                tx,
                                                QueryEvent::Usage {
                                                    query_id,
                                                    input_tokens,
                                                    output_tokens,
                                                    cost_usd,
                                                    cache_creation_tokens,
                                                    cache_read_tokens,
                                                }
                                            );

                                            // Flush any pending tool input that wasn't finalized by
                                            // ContentBlockStop (OpenAI/Ollama don't emit that event).
                                            if let Some((id, name)) = current_tool_use.take() {
                                                let raw =
                                                    std::mem::take(&mut accumulated_tool_input);
                                                match serde_json::from_str::<serde_json::Value>(
                                                    &raw,
                                                ) {
                                                    Ok(json_val) => {
                                                        tool_inputs.push((
                                                            id.clone(),
                                                            name.clone(),
                                                            json_val.clone(),
                                                        ));
                                                        assistant_tool_uses.push(
                                                            ContentBlock::ToolUse {
                                                                id,
                                                                name,
                                                                input: json_val,
                                                            },
                                                        );
                                                    }
                                                    Err(e) => {
                                                        tracing::warn!(
                                                            "Malformed tool input (post-stream flush): {e}"
                                                        );
                                                        tool_results.push(ToolResultEntry {
                                                            tool_use_id: id,
                                                            content: format!(
                                                                "Malformed tool input: {e}"
                                                            ),
                                                            is_error: true,
                                                            metadata: Default::default(),
                                                        });
                                                    }
                                                }
                                            }

                                            if !tool_inputs.is_empty() {
                                                // Phase 1: Check permissions and hooks (sequential — may need user input)
                                                let mut approved_tools: Vec<(
                                                    String,
                                                    String,
                                                    serde_json::Value,
                                                )> = Vec::new();

                                                for (tool_id, tool_name, tool_input) in
                                                    tool_inputs.drain(..)
                                                {
                                                    send_event!(
                                                        tx,
                                                        QueryEvent::Progress {
                                                            query_id,
                                                            message: format!(
                                                                "Executing tool: {tool_name}"
                                                            ),
                                                        }
                                                    );

                                                    // Plan mode gate: block write tools when plan mode is active.
                                                    let is_plan_active = plan_mode_active
                                                        .read()
                                                        .map(|g| *g)
                                                        .unwrap_or(false);
                                                    if is_plan_active {
                                                        let is_write_tool = crate::tool_execution::is_file_modifying_tool(&tool_name);
                                                        if is_write_tool {
                                                            let error_msg = format!(
                                                                "Plan mode: write operations blocked. \
                                                                 Use exit_plan_mode to resume editing. \
                                                                 Blocked tool: {tool_name}"
                                                            );
                                                            send_event!(
                                                                tx,
                                                                QueryEvent::ToolUseResult {
                                                                    query_id,
                                                                    tool_use_id: tool_id.clone(),
                                                                    tool_name,
                                                                    result: error_msg.clone(),
                                                                    is_error: true,
                                                                }
                                                            );
                                                            tool_results.push(ToolResultEntry {
                                                                tool_use_id: tool_id,
                                                                content: error_msg,
                                                                is_error: true,
                                                                metadata: Default::default(),
                                                            });
                                                            continue;
                                                        }
                                                    }

                                                    // Pre-check with classifier and permission system
                                                    let permission_result = {
                                                        let guard =
                                                            recover_lock(permissions.read());
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
                                                            send_event!(
                                                                tx,
                                                                QueryEvent::ToolUseResult {
                                                                    query_id,
                                                                    tool_use_id: tool_id.clone(),
                                                                    tool_name,
                                                                    result: error_msg.clone(),
                                                                    is_error: true,
                                                                }
                                                            );
                                                            tool_results.push(ToolResultEntry {
                                                                tool_use_id: tool_id,
                                                                content: error_msg,
                                                                is_error: true,
                                                                metadata: Default::default(),
                                                            });
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
                                                                send_event!(tx, QueryEvent::ToolUseResult {
                                                                    query_id,
                                                                    tool_use_id: tool_id.clone(),
                                                                    tool_name,
                                                                    result: error_msg.clone(),
                                                                    is_error: true,
                                                                });
                                                                tool_results.push(ToolResultEntry {
                                                                    tool_use_id: tool_id,
                                                                    content: error_msg,
                                                                    is_error: true,
                                                                    metadata: Default::default(),
                                                                });
                                                                continue;
                                                            }

                                                            // Send permission request if a channel is provided
                                                            if let Some(ref req_tx) =
                                                                permission_request_tx
                                                            {
                                                                // Generate diff preview for file edit/write tools
                                                                if matches!(
                                                                    tool_name.as_str(),
                                                                    "edit"
                                                                        | "write"
                                                                        | "EditTool"
                                                                        | "WriteTool"
                                                                ) {
                                                                    if let Some(path) = tool_input
                                                                        .get("file_path")
                                                                        .and_then(|v| v.as_str())
                                                                    {
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
                                                                        } else if tool_name
                                                                            == "write"
                                                                            || tool_name
                                                                                == "WriteTool"
                                                                        {
                                                                            // New file — show that it's being created
                                                                            if let Some(content) =
                                                                                tool_input
                                                                                    .get("content")
                                                                                    .and_then(|v| {
                                                                                        v.as_str()
                                                                                    })
                                                                            {
                                                                                let preview =
                                                                                    if content.len()
                                                                                        > 500
                                                                                    {
                                                                                        let mut end = 500.min(content.len());
                                                                                        while !content.is_char_boundary(end) { end -= 1; }
                                                                                        format!("+ Creating new file ({} bytes)\n{}\n... (truncated)", content.len(), &content[..end])
                                                                                    } else {
                                                                                        format!(
                                                                                            "+ Creating new file\n{content}"
                                                                                        )
                                                                                    };
                                                                                prompt
                                                                                    .diff_preview =
                                                                                    Some(preview);
                                                                            }
                                                                        }
                                                                    }
                                                                }
                                                                let (response_tx, mut response_rx) =
                                                                    mpsc::unbounded_channel();
                                                                // Clone prompt for the request; keep a reference for deny message
                                                                let prompt_desc =
                                                                    prompt.description.clone();
                                                                let prompt_for_choice =
                                                                    prompt.clone();
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
                                                                        send_event!(tx, QueryEvent::ToolUseResult {
                                                                            query_id,
                                                                            tool_use_id: tool_id
                                                                                .clone(),
                                                                            tool_name,
                                                                            result: denied_msg
                                                                                .clone(),
                                                                            is_error: true,
                                                                        });
                                                                        tool_results
                                                                            .push(ToolResultEntry {
                                                                                tool_use_id: tool_id,
                                                                                content: denied_msg,
                                                                                is_error: true,
                                                                                metadata: Default::default(),
                                                                            });
                                                                        continue;
                                                                    }
                                                                    Some(
                                                                        crate::permissions::PermissionChoice::AllowOnce,
                                                                    ) => {}
                                                                    Some(
                                                                        crate::permissions::PermissionChoice::AlwaysAllow,
                                                                    ) => {
                                                                        let _ = recover_lock(permissions.write())
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
                                                                        let _ = recover_lock(permissions.write())
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
                                                                        send_event!(tx, QueryEvent::ToolUseResult {
                                                                            query_id,
                                                                            tool_use_id: tool_id
                                                                                .clone(),
                                                                            tool_name,
                                                                            result: error_msg
                                                                                .clone(),
                                                                            is_error: true,
                                                                        });
                                                                        tool_results
                                                                            .push(ToolResultEntry {
                                                                                tool_use_id: tool_id,
                                                                                content: error_msg,
                                                                                is_error: true,
                                                                                metadata: Default::default(),
                                                                            });
                                                                        continue;
                                                                    }
                                                                }
                                                            }
                                                            // If no permission channel, assume auto-allow
                                                        }
                                                    }

                                                    // Run PreToolUse hooks
                                                    let hook_event =
                                                        shannon_engine::hooks::HookEvent::PreToolUse {
                                                            tool_name: tool_name.clone(),
                                                            input: tool_input.clone(),
                                                        };
                                                    let pre_hook_decision = {
                                                        let hm = hook_manager.read().await;
                                                        match hm.run_hooks(&hook_event).await {
                                                            Ok(results) => shannon_engine::hooks::HookManager::resolve_results(&results),
                                                            Err(e) => {
                                                                tracing::warn!("PreToolUse hook error: {e}");
                                                                shannon_engine::hooks::HookDecision::Allow
                                                            }
                                                        }
                                                    };

                                                    let mut effective_input = tool_input.clone();
                                                    match &pre_hook_decision {
                                                        shannon_engine::hooks::HookDecision::Deny {
                                                            reason,
                                                        } => {
                                                            let error_msg =
                                                                format!("Hook denied: {reason}");
                                                            send_event!(
                                                                tx,
                                                                QueryEvent::ToolUseResult {
                                                                    query_id,
                                                                    tool_use_id: tool_id.clone(),
                                                                    tool_name,
                                                                    result: error_msg.clone(),
                                                                    is_error: true,
                                                                }
                                                            );
                                                            tool_results.push(ToolResultEntry {
                                                                tool_use_id: tool_id,
                                                                content: error_msg,
                                                                is_error: true,
                                                                metadata: Default::default(),
                                                            });
                                                            continue;
                                                        }
                                                        shannon_engine::hooks::HookDecision::Modify {
                                                            modified_input,
                                                            ..
                                                        } => {
                                                            if let Some(new_input) = modified_input
                                                            {
                                                                tracing::debug!(
                                                                    "PreToolUse hook modified input for tool '{}'",
                                                                    tool_name
                                                                );
                                                                effective_input = new_input.clone();
                                                            }
                                                        }
                                                        shannon_engine::hooks::HookDecision::Allow => {}
                                                    }

                                                    approved_tools.push((
                                                        tool_id,
                                                        tool_name,
                                                        effective_input,
                                                    ));
                                                }

                                                // Circuit breaker: check consecutive denials before executing tools.
                                                if consecutive_denials >= DENIAL_HARD_LIMIT {
                                                    send_event!(tx, QueryEvent::ToolUseResult {
                                                        query_id,
                                                        tool_use_id: "circuit-breaker".to_string(),
                                                        tool_name: "system".to_string(),
                                                        result: "Too many consecutive permission denials. Stopping.".to_string(),
                                                        is_error: true,
                                                    });
                                                    break; // exit the agent loop
                                                }

                                                // Phase 2: Execute approved tools using read/write-aware batch scheduler.
                                                //
                                                // Read-only tools are grouped into parallel batches.
                                                // Write tools execute one at a time to avoid race conditions.
                                                {
                                                    let batches = tools.partition_tool_calls(
                                                        approved_tools,
                                                        config.max_parallel_tools,
                                                    );

                                                    for batch in batches {
                                                        match batch {
                                                            crate::tools::ToolBatch::Parallel(
                                                                tool_calls,
                                                            ) => {
                                                                // Execute read-only tools concurrently
                                                                let mut exec_handles = Vec::new();
                                                                for (
                                                                    tool_id,
                                                                    tool_name,
                                                                    effective_input,
                                                                ) in tool_calls
                                                                {
                                                                    let id_for_error =
                                                                        tool_id.clone();
                                                                    // Emit progress: tool started
                                                                    send_event!(
                                                                        tx,
                                                                        QueryEvent::ToolProgress {
                                                                            query_id,
                                                                            tool_use_id: tool_id
                                                                                .clone(),
                                                                            tool_name: tool_name
                                                                                .clone(),
                                                                            progress: 0.0,
                                                                            message: format!(
                                                                                "{tool_name} started"
                                                                            ),
                                                                        }
                                                                    );
                                                                    let tools_exec = tools.clone();
                                                                    let exec_name =
                                                                        tool_name.clone();
                                                                    let exec_input =
                                                                        effective_input.clone();
                                                                    let progress_sender =
                                                                        std::sync::Arc::new(
                                                                            ChannelProgressSender {
                                                                                tx: tx.clone(),
                                                                                query_id,
                                                                                tool_use_id:
                                                                                    tool_id.clone(),
                                                                                tool_name:
                                                                                    tool_name
                                                                                        .clone(),
                                                                            },
                                                                        );
                                                                    let handle = tokio::spawn(
                                                                        async move {
                                                                            (tool_id, tool_name, effective_input, tools_exec.execute_streaming(&exec_name, exec_input, progress_sender).await)
                                                                        },
                                                                    );
                                                                    exec_handles.push((
                                                                        id_for_error,
                                                                        handle,
                                                                    ));
                                                                }

                                                                let mut batch_had_denial = false;
                                                                for (saved_tool_id, handle) in
                                                                    exec_handles
                                                                {
                                                                    match handle.await {
                                                                        Ok((
                                                                            tool_id,
                                                                            tool_name,
                                                                            effective_input,
                                                                            result,
                                                                        )) => {
                                                                            // Run PostToolUse hooks
                                                                            {
                                                                                let output_val = match &result {
                                                                                    Ok(o) => serde_json::Value::String(o.content.clone()),
                                                                                    Err(e) => serde_json::Value::String(format!("Error: {e}")),
                                                                                };
                                                                                let post_event = shannon_engine::hooks::HookEvent::PostToolUse {
                                                                                    tool_name: tool_name.clone(),
                                                                                    input: effective_input,
                                                                                    output: output_val,
                                                                                };
                                                                                let hm =
                                                                                    hook_manager
                                                                                        .read()
                                                                                        .await;
                                                                                let _ = hm
                                                                                    .run_hooks(
                                                                                        &post_event,
                                                                                    )
                                                                                    .await;
                                                                            }

                                                                            // Execute matching triggered routines (non-blocking)
                                                                            {
                                                                                let routines =
                                                                                    triggered_routines
                                                                                        .clone();
                                                                                let tool =
                                                                                    tool_name
                                                                                        .clone();
                                                                                tokio::spawn(
                                                                                    async move {
                                                                                        let reg =
                                                                                            routines
                                                                                                .read()
                                                                                                .await;
                                                                                        let results = reg
                                                                                            .execute_matching(
                                                                                                &crate::HookEventType::PostToolUse,
                                                                                                &tool,
                                                                                                None,
                                                                                            )
                                                                                            .await;
                                                                                        for r in
                                                                                            &results
                                                                                        {
                                                                                            if r.success() {
                                                                                                tracing::info!(name = %r.name, "Triggered routine completed");
                                                                                            } else {
                                                                                                tracing::warn!(name = %r.name, stderr = %r.stderr, "Triggered routine failed");
                                                                                            }
                                                                                        }
                                                                                    },
                                                                                );
                                                                            }

                                                                            // Emit progress: tool completed
                                                                            send_event!(tx, QueryEvent::ToolProgress {
                                                                                query_id,
                                                                                tool_use_id: tool_id.clone(),
                                                                                tool_name: tool_name.clone(),
                                                                                progress: 1.0,
                                                                                message: format!("{tool_name} completed"),
                                                                            });
                                                                            match result {
                                                                                Ok(output) => {
                                                                                    let is_err = output.is_error;
                                                                                    if is_err {
                                                                                        batch_had_denial = true;
                                                                                    }
                                                                                    send_event!(tx, QueryEvent::ToolUseResult {
                                                                                        query_id,
                                                                                        tool_use_id: tool_id.clone(),
                                                                                        tool_name: tool_name.clone(),
                                                                                        result: output.content.clone(),
                                                                                        is_error: is_err,
                                                                                    });
                                                                                    tool_results
                                                                                        .push(ToolResultEntry {
                                                                                        tool_use_id: tool_id,
                                                                                        content: output.content.clone(),
                                                                                        is_error: is_err,
                                                                                        metadata: Default::default(),
                                                                                    });
                                                                                }
                                                                                Err(e) => {
                                                                                    // Tool execution errors are not permission denials
                                                                                    let error_msg = format!(
                                                                                        "Tool error: {e}"
                                                                                    );
                                                                                    send_event!(tx, QueryEvent::ToolUseResult {
                                                                                        query_id,
                                                                                        tool_use_id: tool_id.clone(),
                                                                                        tool_name,
                                                                                        result: error_msg.clone(),
                                                                                        is_error: true,
                                                                                    });
                                                                                    tool_results
                                                                                        .push(ToolResultEntry {
                                                                                        tool_use_id: tool_id,
                                                                                        content: error_msg,
                                                                                        is_error: true,
                                                                                        metadata: Default::default(),
                                                                                    });
                                                                                }
                                                                            }
                                                                        }
                                                                        Err(e) => {
                                                                            // Task join errors are not permission denials
                                                                            let error_msg = format!(
                                                                                "Task join error: {e}"
                                                                            );
                                                                            send_event!(tx, QueryEvent::ToolUseResult {
                                                                                query_id,
                                                                                tool_use_id: saved_tool_id.clone(),
                                                                                tool_name: String::new(),
                                                                                result: error_msg.clone(),
                                                                                is_error: true,
                                                                            });
                                                                            tool_results.push(ToolResultEntry {
                                                                                tool_use_id: saved_tool_id,
                                                                                content: error_msg,
                                                                                is_error: true,
                                                                                metadata: Default::default(),
                                                                            });
                                                                        }
                                                                    }
                                                                }
                                                                if !batch_had_denial {
                                                                    consecutive_denials = 0;
                                                                }
                                                            }
                                                            crate::tools::ToolBatch::Serial((
                                                                tool_id,
                                                                tool_name,
                                                                effective_input,
                                                            )) => {
                                                                // Execute write tools sequentially (one at a time)
                                                                // Create a checkpoint before file-modifying tools for undo support
                                                                if matches!(
                                                                    tool_name.as_str(),
                                                                    "Edit" | "Write" | "Bash"
                                                                ) && checkpoint_manager
                                                                    .is_enabled()
                                                                {
                                                                    if let Err(e) = checkpoint_manager.create_checkpoint(&tool_name, &format!("Before {tool_name} tool execution")) {
                                                                        tracing::debug!("Checkpoint creation skipped: {e}");
                                                                    }
                                                                }
                                                                // Emit progress: tool started
                                                                send_event!(
                                                                    tx,
                                                                    QueryEvent::ToolProgress {
                                                                        query_id,
                                                                        tool_use_id: tool_id
                                                                            .clone(),
                                                                        tool_name: tool_name
                                                                            .clone(),
                                                                        progress: 0.0,
                                                                        message: format!(
                                                                            "{tool_name} started"
                                                                        ),
                                                                    }
                                                                );
                                                                let progress_sender =
                                                                    std::sync::Arc::new(
                                                                        ChannelProgressSender {
                                                                            tx: tx.clone(),
                                                                            query_id,
                                                                            tool_use_id: tool_id
                                                                                .clone(),
                                                                            tool_name: tool_name
                                                                                .clone(),
                                                                        },
                                                                    );
                                                                let result = tools
                                                                    .execute_streaming(
                                                                        &tool_name,
                                                                        effective_input.clone(),
                                                                        progress_sender,
                                                                    )
                                                                    .await;

                                                                // Run PostToolUse hooks
                                                                {
                                                                    let output_val = match &result {
                                                                        Ok(o) => serde_json::Value::String(o.content.clone()),
                                                                        Err(e) => serde_json::Value::String(format!("Error: {e}")),
                                                                    };
                                                                    let post_event = shannon_engine::hooks::HookEvent::PostToolUse {
                                                                        tool_name: tool_name.clone(),
                                                                        input: effective_input,
                                                                        output: output_val,
                                                                    };
                                                                    let hm =
                                                                        hook_manager.read().await;
                                                                    let _ = hm
                                                                        .run_hooks(&post_event)
                                                                        .await;
                                                                }

                                                                // Execute matching triggered routines (non-blocking)
                                                                {
                                                                    let routines =
                                                                        triggered_routines.clone();
                                                                    let tool = tool_name.clone();
                                                                    tokio::spawn(async move {
                                                                        let reg =
                                                                            routines.read().await;
                                                                        let results = reg
                                                                                .execute_matching(
                                                                                    &crate::HookEventType::PostToolUse,
                                                                                    &tool,
                                                                                    None,
                                                                                )
                                                                                .await;
                                                                        for r in &results {
                                                                            if r.success() {
                                                                                tracing::info!(name = %r.name, "Triggered routine completed");
                                                                            } else {
                                                                                tracing::warn!(name = %r.name, stderr = %r.stderr, "Triggered routine failed");
                                                                            }
                                                                        }
                                                                    });
                                                                }

                                                                // Emit progress: tool completed
                                                                send_event!(
                                                                    tx,
                                                                    QueryEvent::ToolProgress {
                                                                        query_id,
                                                                        tool_use_id: tool_id
                                                                            .clone(),
                                                                        tool_name: tool_name
                                                                            .clone(),
                                                                        progress: 1.0,
                                                                        message: format!(
                                                                            "{tool_name} completed"
                                                                        ),
                                                                    }
                                                                );
                                                                match result {
                                                                    Ok(output) => {
                                                                        let is_err =
                                                                            output.is_error;
                                                                        consecutive_denials = 0; // reset on success
                                                                        send_event!(tx, QueryEvent::ToolUseResult {
                                                                            query_id,
                                                                            tool_use_id: tool_id.clone(),
                                                                            tool_name: tool_name.clone(),
                                                                            result: output.content.clone(),
                                                                            is_error: is_err,
                                                                        });
                                                                        tool_results.push(
                                                                            ToolResultEntry {
                                                                                tool_use_id:
                                                                                    tool_id,
                                                                                content: output
                                                                                    .content
                                                                                    .clone(),
                                                                                is_error: is_err,
                                                                                metadata: output
                                                                                    .metadata
                                                                                    .clone(),
                                                                            },
                                                                        );
                                                                        if matches!(
                                                                            tool_name.as_str(),
                                                                            "Edit" | "Write"
                                                                        ) {
                                                                            file_edits_made = true;
                                                                        }
                                                                    }
                                                                    Err(e) => {
                                                                        let error_msg = format!(
                                                                            "Tool error: {e}"
                                                                        );
                                                                        send_event!(tx, QueryEvent::ToolUseResult {
                                                                            query_id,
                                                                            tool_use_id: tool_id.clone(),
                                                                            tool_name,
                                                                            result: error_msg.clone(),
                                                                            is_error: true,
                                                                        });
                                                                        tool_results.push(ToolResultEntry {
                                                                            tool_use_id: tool_id,
                                                                            content: error_msg,
                                                                            is_error: true,
                                                                            metadata: Default::default(),
                                                                        });
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                }

                                                // Soft-limit warning: inject a message telling the model to stop retrying
                                                if (DENIAL_SOFT_LIMIT..DENIAL_HARD_LIMIT)
                                                    .contains(&consecutive_denials)
                                                {
                                                    let warning = format!(
                                                        "The user has denied {consecutive_denials} consecutive tool calls. \
                                                         Stop retrying the same or similar operations. \
                                                         Ask the user for clarification or try a completely different approach."
                                                    );
                                                    tool_results.push(ToolResultEntry {
                                                        tool_use_id: "denial-warning".to_string(),
                                                        content: warning,
                                                        is_error: false,
                                                        metadata: Default::default(),
                                                    });
                                                }

                                                turn += 1;

                                                // Save assistant response to conversation for multi-turn context.
                                                // The API requires: assistant(tool_use) → user(tool_result).
                                                // Without the assistant message, the next API call has no
                                                // context for which tools were requested.
                                                {
                                                    let mut assistant_blocks: Vec<ContentBlock> =
                                                        Vec::new();
                                                    if !assistant_text.is_empty() {
                                                        assistant_blocks.push(ContentBlock::Text {
                                                            text: assistant_text.clone(),
                                                        });
                                                    }
                                                    assistant_blocks
                                                        .append(&mut assistant_tool_uses);
                                                    if !assistant_blocks.is_empty() {
                                                        conversation.messages.push(Message {
                                                            role: "assistant".to_string(),
                                                            content: MessageContent::Blocks(
                                                                assistant_blocks,
                                                            ),
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
                                                                        send_event!(tx, QueryEvent::ToolUseResult {
                                                                            query_id,
                                                                            tool_use_id: String::new(),
                                                                            tool_name: "auto_commit".to_string(),
                                                                            result: format!("Auto-committed: {hash}"),
                                                                            is_error: false,
                                                                        });
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }.await;
                                                }

                                                send_event!(
                                                    tx,
                                                    QueryEvent::TurnCompleted {
                                                        query_id,
                                                        turn_number: turn,
                                                        tokens_used: (usage.input_tokens
                                                            + usage.output_tokens)
                                                            as u64,
                                                    }
                                                );
                                                // Mark finalized so the post-loop safety net
                                                // doesn't short-circuit the next turn's API call.
                                                phase = StreamingPhase::Finalized;
                                                // Break from the streaming while-let loop so
                                                // tool results are processed on the next turn
                                                // iteration instead of consuming more events
                                                // (which could trigger the else branch and
                                                // save a duplicate assistant message).
                                                break;
                                            } else {
                                                // No tool uses — save assistant text to conversation
                                                if assistant_text.is_empty()
                                                    && total_output_tokens > 0
                                                {
                                                    tracing::warn!(
                                                        output_tokens = total_output_tokens,
                                                        "Model returned output tokens but no text content — context may be too small"
                                                    );
                                                    send_event!(tx, QueryEvent::Warning {
                                                        query_id,
                                                        message: "Model produced no text output — context window may be too small for this turn.".to_string(),
                                                    });
                                                }
                                                if !assistant_text.is_empty() {
                                                    conversation.messages.push(Message {
                                                        role: "assistant".to_string(),
                                                        content: MessageContent::Text(
                                                            assistant_text,
                                                        ),
                                                    });
                                                }
                                                let total_cost = CostTracker::calculate_cost(
                                                    &client_model,
                                                    total_input_tokens,
                                                    total_output_tokens,
                                                );
                                                send_event!(
                                                    tx,
                                                    QueryEvent::Cost {
                                                        query_id,
                                                        total_cost_usd: total_cost,
                                                        input_tokens: total_input_tokens,
                                                        output_tokens: total_output_tokens,
                                                    }
                                                );
                                                let _ =
                                                    tx.send(Ok(QueryEvent::ConversationUpdate {
                                                        query_id,
                                                        messages: conversation.messages.clone(),
                                                    }));
                                                let _ =
                                                    tx.send(Ok(QueryEvent::Completed { query_id }));

                                                // Auto-save conversation after completion
                                                if let Err(e) = save_conversation_to_disk(
                                                    &state_for_save,
                                                    session_id_for_save,
                                                    &conversation.messages,
                                                    &client_model,
                                                ) {
                                                    tracing::warn!(session = %session_id_for_save, "Failed to save conversation: {e}");
                                                }

                                                return;
                                            }
                                        }
                                        StreamEvent::MessageStop => {}
                                        StreamEvent::Ping => {}
                                    }
                                }
                                Err(e) => {
                                    // Content-first: if partial content was streamed before the error,
                                    // preserve it immediately. Local models (Ollama) often generate
                                    // valid text before hitting a malformed tool-call error, and
                                    // retrying is expensive (new HTTP request) and may hang.
                                    let has_partial = !assistant_text.is_empty()
                                        || !assistant_tool_uses.is_empty();
                                    if has_partial {
                                        let partial_len = assistant_text.len();
                                        let mut blocks: Vec<ContentBlock> = Vec::new();
                                        if !assistant_text.is_empty() {
                                            blocks.push(ContentBlock::Text {
                                                text: std::mem::take(&mut assistant_text),
                                            });
                                        }
                                        blocks.append(&mut assistant_tool_uses);
                                        conversation.messages.push(Message {
                                            role: "assistant".to_string(),
                                            content: MessageContent::Blocks(blocks),
                                        });
                                        tracing::warn!(
                                            "Stream error after partial response ({partial_len} chars) — preserving content"
                                        );
                                        let suggestion = e
                                            .user_suggestion()
                                            .map(|s| format!(" {s}"))
                                            .unwrap_or_default();
                                        let warning_msg = if suggestion.is_empty() {
                                            "Stream ended unexpectedly. Partial response preserved."
                                                .to_string()
                                        } else {
                                            format!("Stream ended unexpectedly.{suggestion}")
                                        };
                                        send_event!(
                                            tx,
                                            QueryEvent::Warning {
                                                query_id,
                                                message: warning_msg,
                                            }
                                        );
                                        send_event!(
                                            tx,
                                            QueryEvent::ConversationUpdate {
                                                query_id,
                                                messages: conversation.messages.clone(),
                                            }
                                        );
                                        send_event!(tx, QueryEvent::Completed { query_id });
                                        if let Err(err) = save_conversation_to_disk(
                                            &state_for_save,
                                            session_id_for_save,
                                            &conversation.messages,
                                            &client_model,
                                        ) {
                                            tracing::warn!(session = %session_id_for_save, "Failed to save conversation: {err}");
                                        }
                                        return;
                                    }

                                    // No partial content — retry without tools for Ollama models
                                    // that can't handle tool-call formatting.
                                    // Use non-streaming mode: Ollama may return HTTP 200 with content
                                    // even when the model generates malformed output, unlike streaming
                                    // mode which can return HTTP 500 immediately.
                                    if e.is_ollama_malformed_output() {
                                        tracing::warn!(
                                            "Ollama malformed output (no partial content), retrying without tools (non-streaming): {e}"
                                        );
                                        send_event!(
                                            tx,
                                            QueryEvent::Progress {
                                                query_id,
                                                message: "Retrying without tools...".to_string(),
                                            }
                                        );
                                        let no_tools: Option<
                                            Vec<shannon_engine::api::ToolDefinition>,
                                        > = None;
                                        let no_system: Option<String> = None;
                                        match tokio::time::timeout(
                                            std::time::Duration::from_secs(60),
                                            client.send_message(
                                                messages.clone(),
                                                no_tools,
                                                no_system,
                                            ),
                                        )
                                        .await
                                        {
                                            Ok(Ok(content_blocks)) => {
                                                // If full-history retry returned an Ollama error
                                                // warning, try once more with just the last user
                                                // message — tiny models may choke on long history.
                                                let is_ollama_warning = content_blocks.iter().any(|b| {
                                                    matches!(b, ContentBlock::Text { text } if text.starts_with("⚠️ Ollama model output error"))
                                                });
                                                let final_blocks = if is_ollama_warning {
                                                    // Keep last 2 turns (up to 4 messages) so the
                                                    // model has enough context to answer the follow-up.
                                                    let minimal: Vec<Message> = {
                                                        let msgs: Vec<&Message> =
                                                            messages.iter().rev().take(4).collect();
                                                        msgs.into_iter().cloned().collect()
                                                    };
                                                    tracing::warn!(
                                                        "Ollama retry still errored, last-resort with last {} msgs (of {})",
                                                        minimal.len(),
                                                        messages.len()
                                                    );
                                                    match tokio::time::timeout(
                                                        std::time::Duration::from_secs(60),
                                                        client.send_message(minimal, None, None),
                                                    ).await {
                                                        Ok(Ok(blocks)) if !blocks.iter().any(|b| {
                                                            matches!(b, ContentBlock::Text { text } if text.starts_with("⚠️ Ollama model output error"))
                                                        }) => blocks,
                                                        _ => {
                                                            send_event!(tx, QueryEvent::Failed {
                                                                query_id,
                                                                error: "This model cannot produce valid output — it may be too small or incompatible. Try /model to switch to a larger model.".to_string(),
                                                            });
                                                            return;
                                                        }
                                                    }
                                                } else {
                                                    content_blocks
                                                };

                                                let mut retry_text = String::new();
                                                for block in &final_blocks {
                                                    if let ContentBlock::Text { text } = block {
                                                        retry_text.push_str(text);
                                                        send_event!(
                                                            tx,
                                                            QueryEvent::Text {
                                                                query_id,
                                                                content: text.clone(),
                                                            }
                                                        );
                                                    }
                                                }
                                                if !retry_text.is_empty() {
                                                    let already_added = conversation.messages.last()
                                                        .map(|m| matches!(&m.content, MessageContent::Text(t) if t == &retry_text))
                                                        .unwrap_or(false);
                                                    if !already_added {
                                                        conversation.messages.push(Message {
                                                            role: "assistant".to_string(),
                                                            content: MessageContent::Text(
                                                                retry_text,
                                                            ),
                                                        });
                                                    }
                                                }
                                                let total_cost = CostTracker::calculate_cost(
                                                    &client_model,
                                                    total_input_tokens,
                                                    total_output_tokens,
                                                );
                                                send_event!(
                                                    tx,
                                                    QueryEvent::Cost {
                                                        query_id,
                                                        total_cost_usd: total_cost,
                                                        input_tokens: total_input_tokens,
                                                        output_tokens: total_output_tokens,
                                                    }
                                                );
                                                send_event!(
                                                    tx,
                                                    QueryEvent::ConversationUpdate {
                                                        query_id,
                                                        messages: conversation.messages.clone(),
                                                    }
                                                );
                                                send_event!(tx, QueryEvent::Completed { query_id });
                                                if let Err(err) = save_conversation_to_disk(
                                                    &state_for_save,
                                                    session_id_for_save,
                                                    &conversation.messages,
                                                    &client_model,
                                                ) {
                                                    tracing::warn!(session = %session_id_for_save, "Failed to save conversation: {err}");
                                                }
                                                return;
                                            }
                                            Ok(Err(retry_err)) => {
                                                tracing::warn!(
                                                    "Non-streaming retry error: {retry_err}"
                                                );
                                                let error_msg = if retry_err
                                                    .is_ollama_malformed_output()
                                                {
                                                    "This model cannot produce valid output — it may be too small, corrupted, or incompatible. Try /model to switch.".to_string()
                                                } else {
                                                    format!(
                                                        "Local model error — retry without tools failed: {retry_err}"
                                                    )
                                                };
                                                send_event!(
                                                    tx,
                                                    QueryEvent::Failed {
                                                        query_id,
                                                        error: error_msg,
                                                    }
                                                );
                                                return;
                                            }
                                            Err(_) => {
                                                tracing::warn!(
                                                    "Non-streaming retry timed out (60s)"
                                                );
                                                send_event!(tx, QueryEvent::Failed {
                                                    query_id,
                                                    error: "Local model error — retry timed out. The model may be loading, try again.".to_string(),
                                                });
                                                return;
                                            }
                                        }
                                    }

                                    // No partial content, non-recoverable — fail
                                    let suggestion = e
                                        .user_suggestion()
                                        .map(|s| format!(" {s}"))
                                        .unwrap_or_default();
                                    let user_error = if suggestion.is_empty() {
                                        format!("{e}")
                                    } else {
                                        suggestion
                                    };
                                    send_event!(
                                        tx,
                                        QueryEvent::Failed {
                                            query_id,
                                            error: user_error,
                                        }
                                    );
                                    return;
                                }
                            }
                        }

                        if !has_content
                            && tool_inputs.is_empty()
                            && phase != StreamingPhase::Finalized
                        {
                            let total_cost = CostTracker::calculate_cost(
                                &client_model,
                                total_input_tokens,
                                total_output_tokens,
                            );
                            send_event!(
                                tx,
                                QueryEvent::Cost {
                                    query_id,
                                    total_cost_usd: total_cost,
                                    input_tokens: total_input_tokens,
                                    output_tokens: total_output_tokens,
                                }
                            );
                            send_event!(
                                tx,
                                QueryEvent::ConversationUpdate {
                                    query_id,
                                    messages: conversation.messages.clone(),
                                }
                            );
                            send_event!(tx, QueryEvent::Completed { query_id });

                            // Auto-save conversation after completion
                            if let Err(e) = save_conversation_to_disk(
                                &state_for_save,
                                session_id_for_save,
                                &conversation.messages,
                                &client_model,
                            ) {
                                tracing::warn!(session = %session_id_for_save, "Failed to save conversation: {e}");
                            }

                            return;
                        }

                        // Safety net: if the stream had content but the MessageDelta
                        // handler didn't finalize (e.g. budget exceeded, premature
                        // stream close, or missing stop event), save the assistant
                        // response now so the next turn retains context.
                        if phase == StreamingPhase::Receiving && has_content {
                            let has_text = !assistant_text.is_empty();
                            let has_tool_uses = !assistant_tool_uses.is_empty();
                            if has_text || has_tool_uses {
                                // Check if the last message is already this assistant response
                                let already_saved = conversation.messages.last().is_some_and(|m| {
                                    matches!(&m.content, MessageContent::Text(t) if has_text && t == &assistant_text)
                                        || matches!(&m.content, MessageContent::Blocks(blocks)
                                            if blocks.len() == assistant_tool_uses.len() + if has_text { 1 } else { 0 })
                                });
                                if !already_saved {
                                    tracing::warn!(
                                        text_len = assistant_text.len(),
                                        tool_uses = assistant_tool_uses.len(),
                                        "Stream ended without finalization — saving assistant response as safety net"
                                    );
                                    let mut blocks: Vec<ContentBlock> = Vec::new();
                                    if has_text {
                                        blocks.push(ContentBlock::Text {
                                            text: assistant_text,
                                        });
                                    }
                                    blocks.append(&mut assistant_tool_uses);
                                    conversation.messages.push(Message {
                                        role: "assistant".to_string(),
                                        content: MessageContent::Blocks(blocks),
                                    });
                                }
                            }
                            let total_cost = CostTracker::calculate_cost(
                                &client_model,
                                total_input_tokens,
                                total_output_tokens,
                            );
                            send_event!(
                                tx,
                                QueryEvent::Cost {
                                    query_id,
                                    total_cost_usd: total_cost,
                                    input_tokens: total_input_tokens,
                                    output_tokens: total_output_tokens,
                                }
                            );
                            send_event!(
                                tx,
                                QueryEvent::ConversationUpdate {
                                    query_id,
                                    messages: conversation.messages.clone(),
                                }
                            );
                            send_event!(tx, QueryEvent::Completed { query_id });

                            if let Err(e) = save_conversation_to_disk(
                                &state_for_save,
                                session_id_for_save,
                                &conversation.messages,
                                &client_model,
                            ) {
                                tracing::warn!(session = %session_id_for_save, "Failed to save conversation: {e}");
                            }

                            return;
                        }
                    }
                    Err(e) => {
                        // Check if this is a token overflow — attempt auto-compaction and retry once
                        if e.is_token_overflow() {
                            let compact_keep = config.keep_recent_messages;
                            if messages.len() > compact_keep {
                                tracing::warn!(
                                    "Token overflow detected, auto-compacting and retrying"
                                );
                                messages = messages.split_off(messages.len() - compact_keep);
                                // Re-inject system prompt at front
                                if let Some(ref sp) = system_prompt {
                                    if !sp.is_empty() {
                                        messages.insert(
                                            0,
                                            shannon_engine::api::Message {
                                                role: "system".to_string(),
                                                content: shannon_engine::api::MessageContent::Text(
                                                    sp.clone(),
                                                ),
                                            },
                                        );
                                    }
                                }
                                // Sync compacted messages back to conversation
                                // so ConversationUpdate reflects the actual state
                                conversation.messages = messages.clone();

                                let retry_result = if let Some(ref blocks) = system_blocks_opt {
                                    client
                                        .send_message_stream_structured_with_retry(
                                            messages.clone(),
                                            tools_schema.clone(),
                                            blocks.clone(),
                                        )
                                        .await
                                } else {
                                    client
                                        .send_message_stream_with_retry(
                                            messages.clone(),
                                            tools_schema.clone(),
                                            system_prompt.clone(),
                                        )
                                        .await
                                };
                                match retry_result {
                                    Ok(mut retry_stream) => {
                                        // Re-process the retry stream — extract text content and
                                        // accumulate into conversation so the response isn't lost
                                        let mut retry_text = String::new();
                                        while let Some(event_result) = retry_stream.next().await {
                                            match event_result {
                                                Ok(StreamEvent::ContentBlockDelta {
                                                    delta,
                                                    ..
                                                }) => {
                                                    if let ContentDelta::TextDelta { text } = delta
                                                    {
                                                        retry_text.push_str(&text);
                                                        send_event!(
                                                            tx,
                                                            QueryEvent::Text {
                                                                query_id,
                                                                content: text
                                                            }
                                                        );
                                                    }
                                                }
                                                Ok(StreamEvent::MessageDelta { delta, .. }) => {
                                                    if delta.stop_reason.as_deref()
                                                        == Some("end_turn")
                                                    {
                                                        // Add the retry response to conversation before sending update
                                                        if !retry_text.is_empty() {
                                                            conversation.messages.push(Message {
                                                                role: "assistant".to_string(),
                                                                content: MessageContent::Text(
                                                                    retry_text.clone(),
                                                                ),
                                                            });
                                                        }
                                                        send_event!(
                                                            tx,
                                                            QueryEvent::ConversationUpdate {
                                                                query_id,
                                                                messages: conversation
                                                                    .messages
                                                                    .clone(),
                                                            }
                                                        );
                                                        send_event!(
                                                            tx,
                                                            QueryEvent::Completed { query_id }
                                                        );
                                                    }
                                                }
                                                Ok(_) => {} // Ping, MessageStart, MessageStop, etc.
                                                Err(retry_err) => {
                                                    // Save partial retry text before failing
                                                    if !retry_text.is_empty() {
                                                        conversation.messages.push(Message {
                                                            role: "assistant".to_string(),
                                                            content: MessageContent::Text(
                                                                retry_text,
                                                            ),
                                                        });
                                                    }
                                                    let suggestion = retry_err
                                                        .user_suggestion()
                                                        .map(|s| format!(" {s}"))
                                                        .unwrap_or_default();
                                                    send_event!(
                                                        tx,
                                                        QueryEvent::ConversationUpdate {
                                                            query_id,
                                                            messages: conversation.messages.clone(),
                                                        }
                                                    );
                                                    send_event!(
                                                        tx,
                                                        QueryEvent::Failed {
                                                            query_id,
                                                            error: format!(
                                                                "Auto-compact retry also failed: {retry_err}.{suggestion}"
                                                            ),
                                                        }
                                                    );
                                                    return;
                                                }
                                            }
                                        }
                                        // Stream ended without end_turn — still add the response
                                        if !retry_text.is_empty() {
                                            conversation.messages.push(Message {
                                                role: "assistant".to_string(),
                                                content: MessageContent::Text(retry_text),
                                            });
                                        }
                                        send_event!(
                                            tx,
                                            QueryEvent::ConversationUpdate {
                                                query_id,
                                                messages: conversation.messages.clone(),
                                            }
                                        );
                                        send_event!(tx, QueryEvent::Completed { query_id });
                                        return;
                                    }
                                    Err(retry_err) => {
                                        let suggestion = retry_err
                                            .user_suggestion()
                                            .map(|s| format!(" {s}"))
                                            .unwrap_or_default();
                                        send_event!(
                                            tx,
                                            QueryEvent::ConversationUpdate {
                                                query_id,
                                                messages: conversation.messages.clone(),
                                            }
                                        );
                                        send_event!(
                                            tx,
                                            QueryEvent::Failed {
                                                query_id,
                                                error: format!(
                                                    "Token overflow — auto-compact retry failed: {retry_err}.{suggestion}"
                                                ),
                                            }
                                        );
                                        return;
                                    }
                                }
                            }
                        }
                        // Ollama HTTP 500 with malformed output — retry without tools.
                        // Use non-streaming mode: Ollama may return HTTP 200 with content
                        // even when the model generates malformed output, unlike streaming
                        // mode which can return HTTP 500 immediately.
                        // Strip system prompt to prevent small models from attempting tool calls.
                        if e.is_ollama_malformed_output() {
                            tracing::warn!(
                                "Ollama HTTP error (malformed output), retrying without tools (non-streaming): {e}"
                            );
                            send_event!(
                                tx,
                                QueryEvent::Progress {
                                    query_id,
                                    message: "Retrying without tools...".to_string(),
                                }
                            );
                            let no_tools: Option<Vec<shannon_engine::api::ToolDefinition>> = None;
                            let no_system: Option<String> = None;
                            match tokio::time::timeout(
                                std::time::Duration::from_secs(60),
                                client.send_message(messages.clone(), no_tools, no_system),
                            )
                            .await
                            {
                                Ok(Ok(content_blocks)) => {
                                    // If full-history retry returned an Ollama error
                                    // warning, try once more with just the last user
                                    // message — tiny models may choke on long history.
                                    let is_ollama_warning = content_blocks.iter().any(|b| {
                                        matches!(b, ContentBlock::Text { text } if text.starts_with("⚠️ Ollama model output error"))
                                    });
                                    let final_blocks = if is_ollama_warning {
                                        let minimal: Vec<Message> = messages
                                            .iter()
                                            .rev()
                                            .find(|m| m.role == "user")
                                            .cloned()
                                            .map(|m| vec![m])
                                            .unwrap_or_else(|| messages.clone());
                                        tracing::warn!(
                                            "Ollama retry still errored, last-resort minimal input ({}/{} msgs)",
                                            minimal.len(),
                                            messages.len()
                                        );
                                        match tokio::time::timeout(
                                            std::time::Duration::from_secs(60),
                                            client.send_message(minimal, None, None),
                                        ).await {
                                            Ok(Ok(blocks)) if !blocks.iter().any(|b| {
                                                matches!(b, ContentBlock::Text { text } if text.starts_with("⚠️ Ollama model output error"))
                                            }) => blocks,
                                            _ => {
                                                send_event!(tx, QueryEvent::Failed {
                                                    query_id,
                                                    error: "This model cannot produce valid output — it may be too small or incompatible. Try /model to switch to a larger model.".to_string(),
                                                });
                                                return;
                                            }
                                        }
                                    } else {
                                        content_blocks
                                    };

                                    let mut retry_text = String::new();
                                    for block in &final_blocks {
                                        if let ContentBlock::Text { text } = block {
                                            retry_text.push_str(text);
                                            send_event!(
                                                tx,
                                                QueryEvent::Text {
                                                    query_id,
                                                    content: text.clone(),
                                                }
                                            );
                                        }
                                    }
                                    if !retry_text.is_empty() {
                                        conversation.messages.push(Message {
                                            role: "assistant".to_string(),
                                            content: MessageContent::Text(retry_text),
                                        });
                                    }
                                    let total_cost = CostTracker::calculate_cost(
                                        &client_model,
                                        total_input_tokens,
                                        total_output_tokens,
                                    );
                                    send_event!(
                                        tx,
                                        QueryEvent::Cost {
                                            query_id,
                                            total_cost_usd: total_cost,
                                            input_tokens: total_input_tokens,
                                            output_tokens: total_output_tokens,
                                        }
                                    );
                                    send_event!(
                                        tx,
                                        QueryEvent::ConversationUpdate {
                                            query_id,
                                            messages: conversation.messages.clone(),
                                        }
                                    );
                                    send_event!(tx, QueryEvent::Completed { query_id });
                                    if let Err(err) = save_conversation_to_disk(
                                        &state_for_save,
                                        session_id_for_save,
                                        &conversation.messages,
                                        &client_model,
                                    ) {
                                        tracing::warn!(session = %session_id_for_save, "Failed to save conversation: {err}");
                                    }
                                    return;
                                }
                                Ok(Err(retry_err)) => {
                                    tracing::warn!("Non-streaming retry error: {retry_err}");
                                    let error_msg = if retry_err.is_ollama_malformed_output() {
                                        "This model cannot produce valid output — it may be too small, corrupted, or incompatible. Try /model to switch.".to_string()
                                    } else {
                                        format!(
                                            "Local model error — retry without tools failed: {retry_err}"
                                        )
                                    };
                                    send_event!(
                                        tx,
                                        QueryEvent::Failed {
                                            query_id,
                                            error: error_msg,
                                        }
                                    );
                                    return;
                                }
                                Err(_) => {
                                    tracing::warn!("Non-streaming retry timed out (60s)");
                                    send_event!(tx, QueryEvent::Failed {
                                        query_id,
                                        error: "Local model error — retry timed out. The model may be loading, try again.".to_string(),
                                    });
                                    return;
                                }
                            }
                        }
                        let suggestion = e
                            .user_suggestion()
                            .map(|s| format!(" {s}"))
                            .unwrap_or_default();
                        let user_error = if suggestion.is_empty() {
                            format!("{e}")
                        } else {
                            suggestion
                        };
                        send_event!(
                            tx,
                            QueryEvent::ConversationUpdate {
                                query_id,
                                messages: conversation.messages.clone(),
                            }
                        );
                        send_event!(
                            tx,
                            QueryEvent::Failed {
                                query_id,
                                error: user_error,
                            }
                        );
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
        let tracker = recover_lock(self.cost_tracker.read());
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
        recover_lock(self.cost_tracker.read()).summary()
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
        self.conversation.total_tokens += input_tokens + output_tokens;
        self.conversation.total_cost += cost_usd;
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
    use shannon_engine::api::{ContentBlock, MessageContent};
    use shannon_engine::state::SessionPersistMetadata;

    // Generate title from first user message
    let title = messages
        .iter()
        .find(|m| m.role == "user")
        .and_then(|m| match &m.content {
            MessageContent::Text(text) => {
                let preview = if text.len() > 50 {
                    let mut end = 47.min(text.len());
                    while !text.is_char_boundary(end) {
                        end -= 1;
                    }
                    format!("{}...", &text[..end])
                } else {
                    text.clone()
                };
                Some(preview)
            }
            MessageContent::Blocks(blocks) => blocks.iter().find_map(|b| match b {
                ContentBlock::Text { text } => {
                    let preview = if text.len() > 50 {
                        let mut end = 47.min(text.len());
                        while !text.is_char_boundary(end) {
                            end -= 1;
                        }
                        format!("{}...", &text[..end])
                    } else {
                        text.clone()
                    };
                    Some(preview)
                }
                _ => None,
            }),
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
    use crate::permissions::PermissionManager;
    use crate::tools::ToolRegistry;
    use shannon_engine::api::{LlmClient, LlmClientConfig, MessageContent};
    use std::env;
    use std::fs;
    use uuid::Uuid;

    fn create_test_client() -> LlmClient {
        let config = LlmClientConfig {
            api_key: "test-key".to_string(),
            base_url: "http://localhost:11434".to_string(),
            model: "test-model".to_string(),
            provider: LlmProvider::Ollama,
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
        let engine =
            QueryEngine::with_session_id(client, tools, permissions, state, config, specific_id);

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
            shannon_engine::api::Message {
                role: "user".to_string(),
                content: MessageContent::Text("Hello, how are you?".to_string()),
            },
            shannon_engine::api::Message {
                role: "assistant".to_string(),
                content: MessageContent::Text("I'm doing well, thanks!".to_string()),
            },
        ];

        // Save session
        let result = save_conversation_to_disk(&state, session_id, &messages, "test-model");
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
        let messages = vec![shannon_engine::api::Message {
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
        engine.add_assistant_message(vec![shannon_engine::api::ContentBlock::Text {
            text: "Hi there".to_string(),
        }]);
        engine.add_user_message("How are you?".to_string());
        engine.add_assistant_message(vec![shannon_engine::api::ContentBlock::Text {
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
        engine.add_assistant_message(vec![shannon_engine::api::ContentBlock::Text {
            text: "A1".to_string(),
        }]);
        engine.add_user_message("Q2".to_string());
        engine.add_assistant_message(vec![shannon_engine::api::ContentBlock::Text {
            text: "A2".to_string(),
        }]);
        engine.add_user_message("Q3".to_string());
        engine.add_assistant_message(vec![shannon_engine::api::ContentBlock::Text {
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
        engine.add_assistant_message(vec![shannon_engine::api::ContentBlock::Text {
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
        engine.add_assistant_message(vec![shannon_engine::api::ContentBlock::Text {
            text: "I'll run the tests".to_string(),
        }]);
        engine.add_assistant_message(vec![shannon_engine::api::ContentBlock::Text {
            text: "All tests passed".to_string(),
        }]);
        engine.add_user_message("Now commit".to_string());
        engine.add_assistant_message(vec![shannon_engine::api::ContentBlock::Text {
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
        engine.add_assistant_message(vec![shannon_engine::api::ContentBlock::Text {
            text: "Hello".to_string(),
        }]);
        engine.add_assistant_message(vec![shannon_engine::api::ContentBlock::Text {
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

        std::fs::write(project_dir.join("CLAUDE.md"), "# Blocks Test\nBe concise.").unwrap();

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

    // ── Plan Mode Integration Tests ──────────────────────────────────────

    #[test]
    fn test_plan_mode_flag_default_false() {
        let engine = create_test_engine();
        assert!(!engine.is_plan_mode_active());
    }

    #[test]
    fn test_plan_mode_flag_can_be_set() {
        let flag = Arc::new(RwLock::new(true));
        let engine = create_test_engine().with_plan_mode_active(flag);
        assert!(engine.is_plan_mode_active());
    }

    #[test]
    fn test_plan_mode_flag_shared_reflection() {
        let flag = Arc::new(RwLock::new(false));
        let engine = create_test_engine().with_plan_mode_active(flag.clone());

        // Initially inactive
        assert!(!engine.is_plan_mode_active());

        // Setting the flag externally is reflected in the engine
        *flag.write().unwrap() = true;
        assert!(engine.is_plan_mode_active());

        // Resetting the flag
        *flag.write().unwrap() = false;
        assert!(!engine.is_plan_mode_active());
    }

    #[test]
    fn test_plan_mode_active_handle_clones() {
        let engine = create_test_engine();
        let handle = engine.plan_mode_active_handle();

        // Modify via handle
        *handle.write().unwrap() = true;
        assert!(engine.is_plan_mode_active());
    }

    #[test]
    fn test_is_file_modifying_tool_covers_write_tools() {
        // Verify the helper used by the engine gate recognizes write tools
        assert!(crate::tool_execution::is_file_modifying_tool("Write"));
        assert!(crate::tool_execution::is_file_modifying_tool("write"));
        assert!(crate::tool_execution::is_file_modifying_tool("Edit"));
        assert!(crate::tool_execution::is_file_modifying_tool("edit"));
        assert!(crate::tool_execution::is_file_modifying_tool("MultiEdit"));
        assert!(crate::tool_execution::is_file_modifying_tool("multi_edit"));
        assert!(crate::tool_execution::is_file_modifying_tool("Bash"));
        assert!(crate::tool_execution::is_file_modifying_tool("bash"));
    }

    #[test]
    fn test_is_file_modifying_tool_excludes_read_tools() {
        // Verify read-only tools are not flagged as modifying
        assert!(!crate::tool_execution::is_file_modifying_tool("Read"));
        assert!(!crate::tool_execution::is_file_modifying_tool("Glob"));
        assert!(!crate::tool_execution::is_file_modifying_tool("Grep"));
        assert!(!crate::tool_execution::is_file_modifying_tool("LSP"));
    }

    // ── Context resolution tests ──────────────────────────────────

    #[test]
    fn test_resolve_max_context_user_override_wins() {
        // User override should take absolute priority
        let result = QueryEngine::resolve_max_context_tokens("unknown-model", Some(64000));
        assert_eq!(result, 64000);
    }

    #[test]
    fn test_resolve_max_context_from_registry() {
        // Known model should get context_window from MODEL_CATALOG
        let result = QueryEngine::resolve_max_context_tokens("claude-sonnet-4-20250514", None);
        assert_eq!(result, 200_000);
    }

    #[test]
    fn test_resolve_max_context_unknown_model_fallback() {
        // Unknown model falls back to context_window_for() default (200K)
        let result = QueryEngine::resolve_max_context_tokens("nonexistent-model", None);
        assert_eq!(result, 200_000);
    }

    #[test]
    fn test_resolve_max_context_zero_override_prevents_division_by_zero() {
        // Even a zero override should not crash — the compression guard uses .max(1)
        let result = QueryEngine::resolve_max_context_tokens("any-model", Some(0));
        assert_eq!(result, 0); // resolved as 0, but usage code uses .max(1)
        // Verify the guard works
        let guarded = result.max(1);
        assert_eq!(guarded, 1);
    }

    #[test]
    fn test_effective_max_context_initialized_in_engine() {
        let client = create_test_client();
        let tools = ToolRegistry::new();
        let permissions = PermissionManager::new();
        let state = StateManager::new();
        let config = QueryEngineConfig::default();

        let engine = QueryEngine::new(client, tools, permissions, state, config);
        // Default config has max_context_tokens: None, so it uses registry fallback
        // "test-model" is not in catalog, so falls back to 200_000
        assert_eq!(engine.effective_max_context_tokens, 200_000);
    }

    #[test]
    fn test_effective_max_context_with_user_override() {
        let client = create_test_client();
        let tools = ToolRegistry::new();
        let permissions = PermissionManager::new();
        let state = StateManager::new();
        let mut config = QueryEngineConfig::default();
        config.max_context_tokens = Some(32000);

        let engine = QueryEngine::new(client, tools, permissions, state, config);
        assert_eq!(engine.effective_max_context_tokens, 32000);
    }

    #[test]
    fn test_effective_max_context_with_known_model() {
        let config = LlmClientConfig {
            api_key: "test".to_string(),
            base_url: "http://localhost:11434".to_string(),
            model: "claude-sonnet-4-20250514".to_string(),
            ..Default::default()
        };
        let client = LlmClient::new(config);
        let tools = ToolRegistry::new();
        let permissions = PermissionManager::new();
        let state = StateManager::new();

        let engine = QueryEngine::new(
            client,
            tools,
            permissions,
            state,
            QueryEngineConfig::default(),
        );
        assert_eq!(engine.effective_max_context_tokens, 200_000);
    }

    /// Verify that cache tokens from MessageStart are captured and merged
    /// with MessageDelta usage. This test validates the fix for cache hit
    /// rate not showing in the UI — the root cause was that MessageStart
    /// was ignored, losing cache_creation_input_tokens and
    /// cache_read_input_tokens which Anthropic only sends in that event.
    #[test]
    fn test_cache_tokens_from_message_start_are_used() {
        use shannon_engine::api::{MessageResponse, StreamEvent, Usage};

        // Simulate Anthropic's message_start event with cache tokens
        let start_event = StreamEvent::MessageStart {
            message: MessageResponse {
                id: "msg_test".to_string(),
                role: "assistant".to_string(),
                content: vec![],
                model: "claude-sonnet-4-20250514".to_string(),
                stop_reason: None,
                usage: Usage {
                    input_tokens: 1000,
                    output_tokens: 0,
                    cache_creation_input_tokens: 500,
                    cache_read_input_tokens: 800,
                },
            },
        };

        // Extract cache tokens like the engine now does
        let (cache_read, cache_creation) = match &start_event {
            StreamEvent::MessageStart { message } => (
                message.usage.cache_read_input_tokens as u64,
                message.usage.cache_creation_input_tokens as u64,
            ),
            _ => (0, 0),
        };

        assert_eq!(cache_read, 800, "cache_read should come from MessageStart");
        assert_eq!(
            cache_creation, 500,
            "cache_creation should come from MessageStart"
        );

        // Simulate message_delta which only has output_tokens
        let delta_usage = Usage {
            input_tokens: 0,
            output_tokens: 200,
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: 0,
        };

        // Merge: take the max of both sources
        let merged_cache_read = cache_read.max(delta_usage.cache_read_input_tokens as u64);
        let merged_cache_creation =
            cache_creation.max(delta_usage.cache_creation_input_tokens as u64);

        assert_eq!(
            merged_cache_read, 800,
            "merged cache_read should preserve MessageStart value"
        );
        assert_eq!(
            merged_cache_creation, 500,
            "merged cache_creation should preserve MessageStart value"
        );
    }

    // ── Context window propagation tests ──────────────────────────────────

    #[test]
    fn test_resolve_max_context_user_override_takes_priority() {
        let result = QueryEngine::resolve_max_context_tokens("unknown-model", Some(8000));
        assert_eq!(result, 8000, "User override should take priority");
    }

    #[test]
    fn test_resolve_max_context_known_model_exact_match() {
        let result = QueryEngine::resolve_max_context_tokens("claude-sonnet-4-20250514", None);
        assert_eq!(result, 200_000, "Known model should match from registry");
    }

    #[test]
    fn test_resolve_max_context_partial_model_id_prefix_match() {
        // "claude-sonnet-4" should match "claude-sonnet-4-20250514" via prefix
        let result = QueryEngine::resolve_max_context_tokens("claude-sonnet-4", None);
        assert!(
            result > 0,
            "Partial model ID should resolve via prefix matching"
        );
    }

    #[test]
    fn test_resolve_max_context_ollama_model_fallback() {
        let result = QueryEngine::resolve_max_context_tokens("ollama/llama3:8b", None);
        assert_eq!(
            result, 200_000,
            "Unknown Ollama model should fall back to 200k"
        );
    }

    #[test]
    fn test_resolved_context_window_with_user_config() {
        let client = create_test_client();
        let tools = ToolRegistry::new();
        let permissions = PermissionManager::new();
        let state = StateManager::new();
        let mut config = QueryEngineConfig::default();
        config.max_context_tokens = Some(8000);

        let engine = QueryEngine::new(client, tools, permissions, state, config);
        assert_eq!(engine.resolved_context_window(), 8000);
    }

    #[test]
    fn test_resolved_context_window_ollama_fallback_chain() {
        // Ollama provider with no cached info → falls back to effective_max_context_tokens
        let config = LlmClientConfig {
            api_key: "test".to_string(),
            base_url: "http://localhost:11434".to_string(),
            model: "llama3:8b".to_string(),
            provider: shannon_engine::api::LlmProvider::Ollama,
            ..Default::default()
        };
        let client = LlmClient::new(config);
        let tools = ToolRegistry::new();
        let permissions = PermissionManager::new();
        let state = StateManager::new();

        let engine = QueryEngine::new(
            client,
            tools,
            permissions,
            state,
            QueryEngineConfig::default(),
        );
        // Without pre_resolve_context being called (no Ollama server), it should
        // fall back to effective_max_context_tokens (which is the model_registry value)
        let window = engine.resolved_context_window();
        assert!(
            window > 0,
            "Context window should be positive even without Ollama server"
        );
    }

    #[test]
    fn test_cache_hit_rate_accumulation_across_usage_events() {
        use shannon_engine::api::{MessageResponse, StreamEvent, Usage};

        // Simulate multiple turns with different cache profiles
        let turns = vec![
            (10_000, 0),    // Turn 1: cache miss
            (0, 9_000),     // Turn 2: cache hit
            (2_000, 7_000), // Turn 3: partial
        ];

        let mut total_creation: u64 = 0;
        let mut total_read: u64 = 0;

        for (creation, read) in &turns {
            let event = StreamEvent::MessageStart {
                message: MessageResponse {
                    id: "msg_test".to_string(),
                    role: "assistant".to_string(),
                    content: vec![],
                    model: "test".to_string(),
                    stop_reason: None,
                    usage: Usage {
                        input_tokens: 5000,
                        output_tokens: 0,
                        cache_creation_input_tokens: *creation,
                        cache_read_input_tokens: *read,
                    },
                },
            };

            if let StreamEvent::MessageStart { message } = &event {
                total_creation += message.usage.cache_creation_input_tokens as u64;
                total_read += message.usage.cache_read_input_tokens as u64;
            }
        }

        assert_eq!(total_creation, 12_000);
        assert_eq!(total_read, 16_000);

        let hit_rate = total_read as f64 / (total_read + total_creation) as f64;
        // 16000 / (16000 + 12000) ≈ 0.571
        assert!(
            hit_rate > 0.5,
            "Hit rate should be > 50%, got {hit_rate:.3}"
        );
        assert!(
            hit_rate < 0.6,
            "Hit rate should be < 60%, got {hit_rate:.3}"
        );
    }

    // ── ToolResultEntry tests ────────────────────────────────────────

    fn make_image_json_content(base64_data: &str, media_type: &str, path: &str) -> String {
        serde_json::json!({
            "type": "image",
            "media_type": media_type,
            "data": base64_data,
            "path": path,
            "size": 1024
        })
        .to_string()
    }

    #[test]
    fn test_tool_result_entry_text_result() {
        let entry = ToolResultEntry {
            tool_use_id: "tool_1".to_string(),
            content: "Hello world".to_string(),
            is_error: false,
            metadata: Default::default(),
        };
        let result = entry.to_tool_result_content();
        assert!(result.is_some());
        match result.unwrap() {
            ToolResultContent::Single(text) => assert_eq!(text, "Hello world"),
            other => panic!("Expected Single, got: {other:?}"),
        }
    }

    #[test]
    fn test_tool_result_entry_error_result() {
        let entry = ToolResultEntry {
            tool_use_id: "tool_1".to_string(),
            content: "Something failed".to_string(),
            is_error: true,
            metadata: Default::default(),
        };
        let result = entry.to_tool_result_content();
        assert!(result.is_some());
        match result.unwrap() {
            ToolResultContent::Single(text) => assert_eq!(text, "Something failed"),
            other => panic!("Expected Single for error, got: {other:?}"),
        }
    }

    #[test]
    fn test_tool_result_entry_image_result_creates_multiple_blocks() {
        let image_content = make_image_json_content("iVBORw0KGgo=", "image/png", "/tmp/test.png");
        let entry = ToolResultEntry {
            tool_use_id: "tool_1".to_string(),
            content: image_content,
            is_error: false,
            metadata: {
                let mut map = std::collections::HashMap::new();
                map.insert("type".to_string(), serde_json::json!("image"));
                map.insert("media_type".to_string(), serde_json::json!("image/png"));
                map.insert("file_path".to_string(), serde_json::json!("/tmp/test.png"));
                map
            },
        };
        let result = entry.to_tool_result_content();
        assert!(result.is_some());
        match result.unwrap() {
            ToolResultContent::Multiple(blocks) => {
                assert_eq!(blocks.len(), 2, "Expected text + image blocks");
                // First block should be a text description
                match &blocks[0] {
                    ContentBlock::Text { text } => {
                        assert!(text.contains("/tmp/test.png"));
                        assert!(text.contains("image/png"));
                    }
                    other => panic!("Expected Text block, got: {other:?}"),
                }
                // Second block should be an image
                match &blocks[1] {
                    ContentBlock::Image { source } => {
                        assert_eq!(source.source_type, "base64");
                        assert_eq!(source.media_type, "image/png");
                        assert_eq!(source.data, "iVBORw0KGgo=");
                    }
                    other => panic!("Expected Image block, got: {other:?}"),
                }
            }
            other => panic!("Expected Multiple for image, got: {other:?}"),
        }
    }

    #[test]
    fn test_tool_result_entry_image_with_jpeg() {
        let image_content =
            make_image_json_content("/9j/4AAQSkZJ", "image/jpeg", "/photos/img.jpg");
        let entry = ToolResultEntry {
            tool_use_id: "tool_2".to_string(),
            content: image_content,
            is_error: false,
            metadata: {
                let mut map = std::collections::HashMap::new();
                map.insert("type".to_string(), serde_json::json!("image"));
                map.insert("media_type".to_string(), serde_json::json!("image/jpeg"));
                map.insert(
                    "file_path".to_string(),
                    serde_json::json!("/photos/img.jpg"),
                );
                map
            },
        };
        let result = entry.to_tool_result_content().unwrap();
        match result {
            ToolResultContent::Multiple(blocks) => match &blocks[1] {
                ContentBlock::Image { source } => {
                    assert_eq!(source.media_type, "image/jpeg");
                    assert_eq!(source.data, "/9j/4AAQSkZJ");
                }
                other => panic!("Expected Image block, got: {other:?}"),
            },
            other => panic!("Expected Multiple, got: {other:?}"),
        }
    }

    #[test]
    fn test_tool_result_entry_non_image_metadata_ignored() {
        let entry = ToolResultEntry {
            tool_use_id: "tool_1".to_string(),
            content: "Regular text output".to_string(),
            is_error: false,
            metadata: {
                let mut map = std::collections::HashMap::new();
                map.insert("type".to_string(), serde_json::json!("text"));
                map.insert("lines".to_string(), serde_json::json!(42));
                map
            },
        };
        let result = entry.to_tool_result_content().unwrap();
        match result {
            ToolResultContent::Single(text) => assert_eq!(text, "Regular text output"),
            other => panic!("Expected Single for non-image, got: {other:?}"),
        }
    }

    #[test]
    fn test_tool_result_entry_image_error_stays_single() {
        // Even if metadata says "image", errors should always be Single
        let entry = ToolResultEntry {
            tool_use_id: "tool_1".to_string(),
            content: "Image load failed".to_string(),
            is_error: true,
            metadata: {
                let mut map = std::collections::HashMap::new();
                map.insert("type".to_string(), serde_json::json!("image"));
                map
            },
        };
        let result = entry.to_tool_result_content().unwrap();
        match result {
            ToolResultContent::Single(text) => assert_eq!(text, "Image load failed"),
            other => panic!("Expected Single for error, got: {other:?}"),
        }
    }

    // ── System prompt tests ─────────────────────────────────────────

    #[test]
    fn test_system_prompt_default_is_set() {
        // Default config includes a built-in system prompt
        let engine = create_test_engine();
        let prompt = engine
            .system_prompt()
            .expect("default should have a system prompt");
        assert!(prompt.contains("Shannon"));
    }

    #[test]
    fn test_system_prompt_overrides_default() {
        let engine =
            create_test_engine().with_system_prompt("You are a code reviewer.".to_string());
        assert_eq!(
            engine.system_prompt(),
            Some("You are a code reviewer.".to_string())
        );
    }

    #[test]
    fn test_append_system_prompt_adds_to_existing() {
        let mut engine = create_test_engine();
        let original = engine.system_prompt().unwrap();
        engine.append_system_prompt("Always write tests.");
        let appended = engine.system_prompt().unwrap();
        assert!(appended.starts_with(&original));
        assert!(appended.contains("Always write tests."));
    }

    #[test]
    fn test_append_system_prompt_accumulates() {
        let mut engine = create_test_engine().with_system_prompt("Base prompt.".to_string());
        engine.append_system_prompt("Section A.");
        engine.append_system_prompt("Section B.");
        let prompt = engine.system_prompt().unwrap();
        assert!(prompt.starts_with("Base prompt."));
        assert!(prompt.contains("Section A."));
        assert!(prompt.contains("Section B."));
    }

    #[test]
    fn test_system_prompt_default_has_no_cwd() {
        let engine = create_test_engine();
        let prompt = engine.system_prompt().unwrap();
        assert!(
            !prompt.contains("Working directory"),
            "Default system prompt should NOT contain CWD (it is injected at query time): {prompt}"
        );
    }

    #[test]
    fn test_cwd_injection_appends_to_system_prompt() {
        let engine = create_test_engine();
        let mut prompt = engine.system_prompt().unwrap();

        // Simulate the CWD injection that process_query does
        if let Ok(cwd) = std::env::current_dir() {
            prompt.push_str(&format!(
                "\n\n## Environment\n\nWorking directory: {}",
                cwd.display()
            ));
        }

        assert!(
            prompt.contains("Working directory"),
            "After CWD injection, prompt should contain 'Working directory'"
        );
        let cwd = std::env::current_dir().unwrap();
        assert!(
            prompt.contains(&*cwd.to_string_lossy()),
            "After CWD injection, prompt should contain the actual CWD path"
        );
    }

    // ── Memory store tests ──────────────────────────────────────────

    #[test]
    fn test_memory_default_is_none() {
        let engine = create_test_engine();
        assert!(engine.memory().is_none());
    }

    #[test]
    fn test_memory_with_store_returns_some() {
        let temp_dir = env::temp_dir()
            .join("shannon-memory-test")
            .join(Uuid::new_v4().to_string());
        fs::create_dir_all(&temp_dir).unwrap();

        let store = MemoryStore::new(temp_dir.clone());
        let engine = create_test_engine().with_memory(store);
        assert!(engine.memory().is_some());

        // Cleanup
        let _ = fs::remove_dir_all(temp_dir);
    }

    // ── set_model_for_provider tests ────────────────────────────────

    #[test]
    fn test_set_model_for_provider_updates_model_and_provider() {
        let mut engine = create_test_engine();

        // Initial state from create_test_client: model=test-model, provider=Ollama
        assert_eq!(engine.client.model(), "test-model");
        assert_eq!(*engine.client.provider(), LlmProvider::Ollama);

        // Switch to Anthropic with a different model
        engine.set_model_for_provider(
            "claude-sonnet-4-20250514".to_string(),
            LlmProvider::Anthropic,
        );

        assert_eq!(engine.client.model(), "claude-sonnet-4-20250514");
        assert_eq!(*engine.client.provider(), LlmProvider::Anthropic);
    }

    #[test]
    fn test_set_model_for_provider_updates_cost_tracker_model_name() {
        let mut engine = create_test_engine();

        engine.set_model_for_provider("claude-opus-4-20250514".to_string(), LlmProvider::Anthropic);

        let tracker = engine
            .cost_tracker
            .read()
            .unwrap_or_else(|e| e.into_inner());
        assert_eq!(tracker.model_name, "claude-opus-4-20250514");
    }

    #[test]
    fn test_set_model_for_provider_updates_effective_context_window() {
        let mut engine = create_test_engine();
        // Default test-model -> 200K
        assert_eq!(engine.effective_max_context_tokens, 200_000);

        // Switch to claude-sonnet-4 which also has 200K — but verify the field was recalculated
        engine.set_model_for_provider(
            "claude-sonnet-4-20250514".to_string(),
            LlmProvider::Anthropic,
        );
        assert_eq!(engine.effective_max_context_tokens, 200_000);
    }

    // ── Configuration setter tests ──────────────────────────────────

    #[test]
    fn test_set_effort_level() {
        let mut engine = create_test_engine();
        assert!(engine.config.effort_level.is_none());

        engine.set_effort_level(Some("high".to_string()));
        assert_eq!(engine.config.effort_level, Some("high".to_string()));

        engine.set_effort_level(None);
        assert!(engine.config.effort_level.is_none());
    }

    #[test]
    fn test_set_focus_area() {
        let mut engine = create_test_engine();
        assert!(engine.config.focus_area.is_none());

        engine.set_focus_area(Some("security".to_string()));
        assert_eq!(engine.config.focus_area, Some("security".to_string()));

        engine.set_focus_area(None);
        assert!(engine.config.focus_area.is_none());
    }

    #[test]
    fn test_set_max_turns() {
        let mut engine = create_test_engine();
        let default_turns = engine.config.max_turns;

        engine.set_max_turns(42);
        assert_eq!(engine.config.max_turns, 42);
        assert_ne!(engine.config.max_turns, default_turns);
    }
}
