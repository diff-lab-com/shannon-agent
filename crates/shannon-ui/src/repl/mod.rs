//! REPL main loop and terminal management

/// Number of lines above which a paste is shown as "[Pasted Text #N X lines]"
const PASTE_THRESHOLD_LINES: usize = 5;

mod at_reference;
mod commands;
mod input;
pub(crate) mod preferences;
mod query;
pub(crate) mod render;

use crate::{
    events::EventHandler,
    render::Renderer,
    repl_enhancement::{DiffData, ReplHistory, ReplRenderer},
    theme::Theme,
    vim::VimHandler,
    widgets::{
        ChatWidget, ChatRole, PromptWidget,
        progress::{ProgressBarWidget, SpinnerWidget, MultiProgressWidget},
        tool_approval::ToolApprovalWidget,
        attachment_bar::AttachmentBarWidget,
        command_palette::CommandPaletteWidget,
        session_tab::SessionTabWidget,
        StreamingState,
    },
    Result,
};
use rust_i18n::t;
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    Terminal,
};
use std::collections::HashMap;
use std::io;
use std::sync::Arc;
use tokio::runtime::Runtime;


// Import core functionality
use shannon_core::{
    api::LlmClientConfig,
    permissions::PermissionManager,
    PromptInfo,
    query_engine::QueryEngine,
    state::StateManager,
    tools::ToolRegistry,
    ContentBlock, MessageContent,
};
use shannon_commands::{Command, CommandBase, CommandRegistry, CommandParser, ExecutionContext, PromptCommand, builtin_commands, SharedExecutor};

// Tool registration
use shannon_tools::register_default_tools_with_project_dir;
use crate::skill_bridge::register_skills_as_tools;
use shannon_mcp::{McpProcessPool, discover_pooled_tools, discover_pooled_remote_tools, HeaderSource};

/// Application state for the REPL
#[derive(Debug, Clone)]
pub struct ReplState {
    /// Current status message
    pub status: String,
    /// Model name being used
    pub model: Option<String>,
    /// Provider associated with the selected model (synced to QueryEngine)
    pub selected_provider: Option<shannon_core::api::LlmProvider>,
    /// Total tokens used
    pub tokens_used: u64,
    /// Total cost in USD accumulated across all queries
    pub total_cost_usd: f64,
    /// Working directory for the session
    pub working_directory: String,
    /// Welcome screen active
    pub welcome_active: bool,
    /// Active permission dialog (if any)
    pub permission_dialog: Option<shannon_core::permissions::PermissionPrompt>,
    /// Permission response channel sender (if dialog is active)
    pub permission_response_tx: Option<tokio::sync::mpsc::UnboundedSender<shannon_core::permissions::PermissionChoice>>,
    /// Active confirm/alert dialog (if any)
    pub active_dialog: Option<crate::widgets::dialog::DialogWidget>,
    /// Pending action to execute when dialog is confirmed
    pub pending_dialog_action: Option<String>,
    /// Currently active tool name (for progress display)
    pub active_tool: Option<String>,
    /// Spinner widget for progress indication
    pub spinner: SpinnerWidget,
    /// Progress bar widget for tool execution progress
    pub progress_bar: ProgressBarWidget,
    /// Whether the progress bar is currently visible (tool is executing)
    pub progress_bar_visible: bool,
    /// Number of steps completed in current query
    pub query_steps_done: usize,
    /// Total steps estimated for current query (0 = indeterminate)
    pub query_steps_total: usize,
    /// Active input dialog (if any)
    pub input_dialog: Option<Box<crate::widgets::dialog::InputDialog>>,
    /// Callback action when input dialog is submitted
    pub input_dialog_action: Option<String>,
    /// Active fuzzy picker for command palette (Ctrl+P)
    pub fuzzy_picker: Option<crate::widgets::select::FuzzyPickerWidget>,
    /// Active file selector for /browse command
    pub file_selector: Option<crate::widgets::select::FileSelectorWidget>,
    /// Multi-progress widget for tracking parallel tool execution
    pub multi_progress: MultiProgressWidget,
    /// Whether multi-progress is visible (tools running in parallel)
    pub multi_progress_visible: bool,
    /// Active multi-select widget (e.g., for /select-tools)
    pub multi_select: Option<crate::widgets::select::MultiSelectWidget>,
    /// Active model picker widget (for /models command)
    pub model_picker: Option<crate::widgets::select::ModelPickerWidget>,
    /// Current completion suggestions to display (populated by Tab, cleared by typing)
    pub completion_suggestions: Vec<String>,
    /// Scheduled routine manager for recurring tasks
    pub routine_manager: shannon_core::scheduled_routines::RoutineManager,
    /// Index of the currently highlighted completion suggestion
    pub completion_suggestion_index: usize,
    /// Plan mode state
    pub plan: PlanState,
    /// Execution sandbox mode (direct or Docker isolation)
    pub sandbox_mode: shannon_tools::SandboxMode,
    /// Color theme for the terminal UI
    pub theme: Theme,
    /// Accessibility mode: replace decorative chars with plain text
    pub accessibility_mode: bool,
    /// Configurable keybindings
    pub keybindings: crate::keybindings::KeyBindings,
    /// Whether the right sidebar panel is visible
    pub sidebar_visible: bool,
    /// Active diff viewer overlay (activated by /diff command)
    pub diff_viewer: Option<crate::widgets::diff_viewer::DiffViewerWidget>,
    /// Interactive diff hunks (when in interactive review mode)
    pub interactive_hunks: Vec<crate::widgets::diff_viewer::InteractiveHunk>,
    /// Selected hunk index for interactive diff viewer
    pub interactive_selected: usize,
    /// Whether the diff viewer is in interactive mode
    pub diff_interactive: bool,
    /// Whether the full key hints overlay is shown (toggled by F1)
    pub show_key_hints: bool,
    /// Whether incremental reverse search (Ctrl+R) is active
    pub incremental_search_active: bool,
    /// Current search query for incremental search
    pub incremental_search_query: String,
    /// Match index within search results
    pub incremental_search_match_index: usize,
    /// Input saved before entering incremental search (restored on cancel)
    pub incremental_search_saved_input: String,
    /// Stored pasted texts awaiting submission: (paste_number, content)
    pub pasted_texts: std::collections::HashMap<usize, String>,
    /// Counter for the next paste number (increments with each large paste)
    pub paste_counter: usize,
    /// Whether the file selector was opened by typing `@` (insert mode vs replace mode)
    pub file_selector_for_at: bool,
    /// Toast notification: (message, when it started)
    pub toast: Option<(String, std::time::Instant)>,
    /// Whether the model is thinking (no text tokens received yet)
    pub thinking_phase: bool,
    /// Whether streaming is currently active
    pub streaming_active: bool,
    /// When the current streaming operation started
    pub streaming_start: Option<std::time::Instant>,
    /// When this session started (for duration display)
    pub session_start: Option<std::time::Instant>,
    /// Current vim mode label for display ("INSERT" or "NORMAL")
    pub vim_mode: String,
    /// Whether leader key mode is active (waiting for second key after Ctrl+X)
    pub leader_active: bool,
    /// Active tab in the sidebar panel
    pub sidebar_tab: SidebarTab,
    /// Cached approval mode label for display (updated on mode change)
    pub approval_mode_label: String,
    /// Active sub-agents for sidebar display (refreshed from agent_registry)
    pub active_agents: Vec<AgentDisplay>,
    /// LSP diagnostic store for displaying code diagnostics
    pub diagnostic_store: crate::lsp_bridge::DiagnosticStore,
    /// Whether focus mode is active (header/statusbar hidden)
    pub focus_mode: bool,
    /// Whether fullscreen mode is active (ALL chrome hidden, chat fills terminal)
    pub fullscreen_mode: bool,
    /// Whether a chat search is active (user triggered via Ctrl+Shift+F or /search)
    pub chat_search_active: bool,
    /// Current search query for chat content search
    pub chat_search_query: String,
    /// Index of the currently focused search match
    pub chat_search_match_index: usize,
    /// Total number of search matches found
    pub chat_search_total_matches: usize,
    /// Whether the transcript pager is active
    pub pager_active: bool,
    /// Scroll position for the transcript pager (line offset from top)
    pub pager_scroll: usize,
    /// Turn counter for context visualization
    pub turn_count: usize,
    /// Recent background task notifications (message + timestamp)
    pub pending_notifications: Vec<(String, std::time::Instant)>,
    /// Whether the first-run onboarding dialog is active
    pub onboarding_active: bool,
    /// Tool approval overlay widget (shows when tool needs confirmation)
    pub tool_approval: ToolApprovalWidget,
    /// Attachment bar widget (shows attached files/images above prompt)
    pub attachment_bar: AttachmentBarWidget,
    /// Command palette overlay widget (Ctrl+P)
    pub command_palette: Option<CommandPaletteWidget>,
    /// Session tab bar widget (top of terminal, Ctrl+T to toggle)
    pub session_tab: SessionTabWidget,
    /// Detailed streaming state for status indicator
    pub streaming_state: StreamingState,
    /// Loop engine state for autonomous iteration
    pub loop_state: Option<LoopState>,
    /// Billing manager for per-model cost tracking and budget alerts
    pub billing_manager: shannon_core::billing::BillingManager,
}

/// State for the autonomous loop iteration engine.
#[derive(Debug, Clone)]
pub struct LoopState {
    /// The task to iterate on
    pub task: String,
    /// Maximum iterations (0 = unlimited until stopped)
    pub max_iterations: usize,
    /// Current iteration count
    pub iteration: usize,
    /// Whether the loop is active
    pub active: bool,
}

/// Tabs available in the sidebar panel
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SidebarTab {
    /// Model info, tokens, cost, tools
    #[default]
    Context,
    /// Modified files list
    Files,
    /// Active sub-agents status
    Agents,
    /// Performance metrics
    Perf,
}

impl SidebarTab {
    /// Cycle to the next tab: Context → Files → Agents → Perf → Context
    pub fn next(self) -> Self {
        match self {
            SidebarTab::Context => SidebarTab::Files,
            SidebarTab::Files => SidebarTab::Agents,
            SidebarTab::Agents => SidebarTab::Perf,
            SidebarTab::Perf => SidebarTab::Context,
        }
    }
}

/// Display info for a single sub-agent in the sidebar
#[derive(Debug, Clone)]
pub struct AgentDisplay {
    /// Agent name
    pub name: String,
    /// Current status string (spawning/running/idle/completed/failed)
    pub status: String,
    /// Whether the agent is still active (not completed/failed)
    pub active: bool,
    /// Team this agent belongs to
    pub team: Option<String>,
    /// Number of turns used / max turns
    pub turns_used: u32,
    pub max_turns: u32,
}

/// State for plan mode
#[derive(Debug, Clone, Default)]
pub struct PlanState {
    /// Whether plan mode is active
    pub active: bool,
    /// The plan content (markdown steps)
    pub content: String,
    /// Plan description (what user wants to accomplish)
    pub description: String,
    /// Whether the plan has been approved
    pub approved: bool,
    /// Scroll offset for long plans
    pub scroll_offset: usize,
}

impl Default for ReplState {
    fn default() -> Self {
        // Get current working directory
        let cwd = std::env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| ".".to_string());

        Self {
            status: "Ready".to_string(),
            model: Some("claude-sonnet-4-20250514".to_string()),
            selected_provider: None,
            tokens_used: 0,
            total_cost_usd: 0.0,
            working_directory: cwd,
            welcome_active: false,
            permission_dialog: None,
            permission_response_tx: None,
            active_dialog: None,
            pending_dialog_action: None,
            active_tool: None,
            spinner: SpinnerWidget::new(),
            progress_bar: ProgressBarWidget::new(),
            progress_bar_visible: false,
            query_steps_done: 0,
            query_steps_total: 0,
            input_dialog: None,
            input_dialog_action: None,
            fuzzy_picker: None,
            file_selector: None,
            multi_progress: MultiProgressWidget::new(),
            multi_progress_visible: false,
            multi_select: None,
            model_picker: None,
            completion_suggestions: Vec::new(),
            routine_manager: shannon_core::scheduled_routines::RoutineManager::new(),
            completion_suggestion_index: 0,
            plan: PlanState::default(),
            sandbox_mode: shannon_tools::SandboxMode::Direct,
            theme: Theme::detect(),
            accessibility_mode: std::env::var("NO_GRAPHICS").is_ok() || std::env::var("ACCESSIBILITY").is_ok(),
            keybindings: crate::keybindings::load_keybindings(),
            sidebar_visible: false,
            diff_viewer: None,
            interactive_hunks: Vec::new(),
            interactive_selected: 0,
            diff_interactive: false,
            show_key_hints: false,
            incremental_search_active: false,
            incremental_search_query: String::new(),
            incremental_search_match_index: 0,
            incremental_search_saved_input: String::new(),
            pasted_texts: std::collections::HashMap::new(),
            paste_counter: 0,
            file_selector_for_at: false,
            toast: None,
            thinking_phase: false,
            streaming_active: false,
            streaming_start: None,
            session_start: Some(std::time::Instant::now()),
            vim_mode: "INSERT".to_string(),
            leader_active: false,
            sidebar_tab: SidebarTab::default(),
            approval_mode_label: "AUTO".to_string(),
            active_agents: Vec::new(),
            diagnostic_store: crate::lsp_bridge::DiagnosticStore::new(),
            focus_mode: false,
            fullscreen_mode: false,
            chat_search_active: false,
            chat_search_query: String::new(),
            chat_search_match_index: 0,
            chat_search_total_matches: 0,
            pager_active: false,
            pager_scroll: 0,
            turn_count: 0,
            pending_notifications: Vec::new(),
            onboarding_active: false,
            tool_approval: ToolApprovalWidget::new(),
            attachment_bar: AttachmentBarWidget::new(5),
            command_palette: None,
            session_tab: SessionTabWidget::new(),
            streaming_state: StreamingState::Idle,
            loop_state: None,
            billing_manager: shannon_core::billing::BillingManager::new(),
        }
    }
}

/// Recursively collect custom commands from a directory.
///
/// - `dir`: root directory to scan
/// - `prefix`: path prefix for nested dirs (e.g. "project:" for `.claude/commands/project/`)
/// - `results`: accumulated (command_name, template_text, file_path) triples
/// Extract a field value from simple YAML-like frontmatter text.
fn parse_frontmatter_field(frontmatter: &str, field: &str) -> Option<String> {
    for line in frontmatter.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix(field).and_then(|s| s.strip_prefix(':')) {
            let val = rest.trim().trim_matches('"').trim_matches('\'').to_string();
            if !val.is_empty() {
                return Some(val);
            }
        }
    }
    None
}

/// Parsed custom command entry with optional frontmatter metadata.
pub(super) struct CustomCommandEntry {
    pub name: String,
    pub template: String,
    pub path: std::path::PathBuf,
    /// Optional description from frontmatter `description:` field.
    pub description: Option<String>,
    /// Argument names from frontmatter `arguments:` field.
    pub arguments: Vec<String>,
    /// Optional model override from frontmatter `model:` field.
    pub model: Option<String>,
    /// Optional allowed tools from frontmatter `allowed-tools:` field.
    pub allowed_tools: Vec<String>,
    /// Optional agent from frontmatter `agent:` field.
    pub agent: Option<String>,
}

pub(super) fn collect_custom_commands(
    dir: &std::path::Path,
    prefix: &str,
    results: &mut Vec<CustomCommandEntry>,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // Skip hidden directories
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.starts_with('.') {
                    continue;
                }
                let subdir_prefix = format!("{prefix}{name}:");
                collect_custom_commands(&path, &subdir_prefix, results);
            }
        } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
            if stem.is_empty() {
                continue;
            }
            let command_name = format!("{prefix}{stem}");
            let content = match std::fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            // Parse YAML frontmatter (---\n...\n---)
            let (template, description, arguments, model, allowed_tools, agent) = if content.starts_with("---") {
                let parts: Vec<&str> = content.splitn(3, "---").collect();
                let frontmatter = parts.get(1).unwrap_or(&"");
                let body = parts.get(2).map(|s| s.trim_start()).unwrap_or("");
                let desc = parse_frontmatter_field(frontmatter, "description");
                let args_str = parse_frontmatter_field(frontmatter, "arguments")
                    .or_else(|| parse_frontmatter_field(frontmatter, "args"));
                let args = args_str
                    .map(|s| s.split(',').map(|a| a.trim().to_string()).filter(|a| !a.is_empty()).collect())
                    .unwrap_or_default();
                let m = parse_frontmatter_field(frontmatter, "model");
                let tools_str = parse_frontmatter_field(frontmatter, "allowed-tools")
                    .or_else(|| parse_frontmatter_field(frontmatter, "allowed_tools"));
                let tools = tools_str
                    .map(|s| s.split(',').map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect())
                    .unwrap_or_default();
                let a = parse_frontmatter_field(frontmatter, "agent");
                (body.to_string(), desc, args, m, tools, a)
            } else {
                (content, None, Vec::new(), None, Vec::new(), None)
            };
            results.push(CustomCommandEntry { name: command_name, template, path, description, arguments, model, allowed_tools, agent });
        }
    }
}

/// Deduplicate custom commands by name, keeping the last occurrence.
/// Since project-level dirs are scanned after user-level dirs, project commands
/// override user-level commands with the same name.
pub(super) fn dedup_custom_commands(commands: &mut Vec<CustomCommandEntry>) {
    let mut seen = std::collections::HashSet::new();
    commands.reverse();
    commands.retain(|c| seen.insert(c.name.clone()));
    commands.reverse();
}

/// Watches custom command directories for changes using filesystem events.
///
/// Uses the `notify` crate to watch `.claude/commands/` and `.shannon/commands/`
/// (project and user level). When changes are detected, commands are re-scanned
/// and re-registered in the [`CommandRegistry`].
pub(crate) struct CustomCommandWatcher {
    dirs: Vec<std::path::PathBuf>,
    #[allow(dead_code)]
    watcher: Option<notify::RecommendedWatcher>,
    dirty: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl CustomCommandWatcher {
    fn new() -> Self {
        let mut dirs = Vec::new();
        let cwd = std::env::current_dir().unwrap_or_default();
        dirs.push(cwd.join(".claude").join("commands"));
        dirs.push(cwd.join(".shannon").join("commands"));
        if let Some(home) = dirs::home_dir() {
            dirs.push(home.join(".claude").join("commands"));
            dirs.push(home.join(".shannon").join("commands"));
        }

        let dirty = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let dirty_flag = dirty.clone();

        let handler = move |event: notify::Result<notify::Event>| {
            if let Ok(event) = event {
                use notify::EventKind;
                if matches!(event.kind,
                    EventKind::Create(_) |
                    EventKind::Modify(_) |
                    EventKind::Remove(_)
                ) {
                    dirty_flag.store(true, std::sync::atomic::Ordering::Relaxed);
                }
            }
        };

        let watcher_result = notify::recommended_watcher(handler);

        let watcher = match watcher_result {
            Ok(mut w) => {
                use notify::Watcher;
                for dir in &dirs {
                    if dir.exists() {
                        let _ = w.watch(dir, notify::RecursiveMode::Recursive);
                    }
                }
                Some(w)
            }
            Err(_) => None,
        };

        Self { dirs, watcher, dirty }
    }

    /// Check if filesystem events were received and reload if needed.
    /// Returns count of re-registered commands.
    fn check_and_reload(&mut self, registry: &CommandRegistry) -> usize {
        if !self.dirty.swap(false, std::sync::atomic::Ordering::Relaxed) {
            return 0;
        }

        // Re-scan and re-register all custom commands
        let mut current_files: Vec<CustomCommandEntry> = Vec::new();
        for dir in &self.dirs {
            collect_custom_commands(dir, "", &mut current_files);
        }
        dedup_custom_commands(&mut current_files);

        for entry in &current_files {
            let description = entry.description.clone()
                .unwrap_or_else(|| format!("Custom command (from {})", entry.path.display()));
            let arg_names = if entry.arguments.is_empty() {
                vec!["$ARGUMENTS".to_string()]
            } else {
                entry.arguments.clone()
            };
            let argument_hint = if entry.arguments.is_empty() {
                Some("$ARGUMENTS".to_string())
            } else {
                Some(entry.arguments.join(" "))
            };
            let command = Command::Prompt(Box::new(PromptCommand {
                base: CommandBase {
                    name: entry.name.clone(),
                    aliases: Vec::new(),
                    description,
                    has_user_specified_description: entry.description.is_some(),
                    availability: vec![shannon_commands::CommandAvailability::All],
                    source: shannon_commands::CommandSource::Builtin,
                    is_enabled: true,
                    is_hidden: false,
                    argument_hint,
                    when_to_use: None,
                    version: None,
                    disable_model_invocation: false,
                    user_invocable: true,
                    is_workflow: false,
                    immediate: false,
                    is_sensitive: false,
                    user_facing_name: None,
                },
                progress_message: format!("Running /{}...", entry.name),
                content_length: entry.template.len(),
                arg_names,
                allowed_tools: entry.allowed_tools.clone(),
                model: entry.model.clone(),
                hooks: std::collections::HashMap::new(),
                context: ExecutionContext::Inline,
                agent: entry.agent.clone(),
                paths: Vec::new(),
                prompt_template: Some(entry.template.clone()),
            }));
            registry.register_sync(command);
        }

        let count = current_files.len();
        tracing::info!("Custom commands hot-reloaded ({} commands)", count);
        count
    }
}

/// Main REPL application struct
pub struct Repl {
    /// Event handler for user input
    pub(crate) events: EventHandler,
    /// Renderer for UI drawing
    pub(crate) renderer: Renderer,
    /// Chat widget for displaying messages
    pub(crate) chat: ChatWidget,
    /// Prompt widget for user input
    pub(crate) prompt: PromptWidget,
    /// Application state
    pub(crate) state: ReplState,
    /// Running state
    pub(crate) running: bool,
    /// Query engine for AI processing
    pub(crate) query_engine: Option<QueryEngine>,
    /// State manager for session persistence (separate from QueryEngine's internal one)
    pub(crate) state_manager: StateManager,
    /// Command registry with all built-in commands
    pub(crate) command_registry: CommandRegistry,
    /// Command parser for parsing /commands
    pub(crate) command_parser: CommandParser,
    /// Shared command executor for concurrent command dispatch
    pub(crate) shared_executor: SharedExecutor,
    /// Tokio runtime for async operations
    pub(crate) runtime: Runtime,
    /// Permission request receiver (from QueryEngine to REPL UI)
    pub(crate) permission_req_rx: tokio::sync::mpsc::UnboundedReceiver<shannon_core::query_engine::PermissionRequest>,
    /// Permission request sender (from REPL to QueryEngine)
    pub(crate) permission_req_tx: tokio::sync::mpsc::UnboundedSender<shannon_core::query_engine::PermissionRequest>,
    /// Last session listing cache (for /resume by number)
    pub(crate) last_session_list: Vec<shannon_core::state::SessionInfo>,
    /// Command history with cursor navigation
    pub(crate) command_history: ReplHistory,
    /// Saved input before history navigation (to restore on down-to-bottom)
    pub(crate) saved_input: String,
    /// Per-turn diff tracking
    pub(crate) diff_data: DiffData,
    /// Current turn index
    pub(crate) current_turn: usize,
    /// Session start time
    pub(crate) session_started_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Markdown renderer for assistant output
    pub(crate) output_renderer: ReplRenderer,
    /// Total commands run in this session
    pub(crate) commands_run: usize,
    /// Total tools invoked in this session
    pub(crate) tools_invoked: usize,
    /// Tab completion state for cycling through matches
    pub(crate) tab_completion_state: TabCompletionState,
    /// Vim key handler for vim mode support (yy/yw/p yank/paste)
    pub(crate) vim_handler: VimHandler,
    /// Multi-agent team coordinator (lazy-initialized on /team create)
    pub(crate) team_coordinator: Option<std::sync::Arc<shannon_agents::AgentCoordinator>>,
    /// Sub-agent registry for background agent management
    pub(crate) agent_registry: Option<std::sync::Arc<shannon_agents::SubAgentRegistry>>,
    /// MCP process pool for hot-reload support
    pub(crate) mcp_pool: std::sync::Arc<McpProcessPool>,
    /// Tool registry for MCP hot-reload tool registration
    pub(crate) tool_registry: std::sync::Arc<shannon_core::tools::ToolRegistry>,
    /// MCP progress update receiver (from McpProcessPool to REPL UI)
    pub(crate) mcp_progress_rx: Option<tokio::sync::mpsc::UnboundedReceiver<(String, f64, Option<f64>)>>,
    /// Model routing rules: (pattern, model_name) pairs
    pub(crate) model_routes: Vec<(String, String)>,
    /// Checkpoint manager for undo/revert operations
    pub(crate) checkpoint_manager: shannon_core::CheckpointManager,
    /// Desktop notification dispatcher
    pub(crate) notifier: shannon_core::notifier::Notifier,
    /// Whether desktop notifications are enabled
    pub(crate) notifications_enabled: bool,
    /// Webhook receiver for external event injection
    pub(crate) webhook_receiver: Option<shannon_core::webhook::WebhookReceiver>,
    /// Instruction file watcher for hot-reloading CLAUDE.md / AGENTS.md / GEMINI.md
    pub(crate) instruction_watcher: Option<shannon_core::project_instructions::InstructionWatcher>,
    /// Custom command file watcher for hot-reloading .claude/commands/ and .shannon/commands/
    pub(crate) command_watcher: Option<CustomCommandWatcher>,
}

/// State for tab completion cycling
#[derive(Debug, Clone, Default)]
pub(crate) struct TabCompletionState {
    /// The prefix text being completed (to detect when completion should reset)
    pub(crate) last_prefix: String,
    /// Current match index for cycling through completions
    pub(crate) current_index: usize,
    /// Available completion candidates
    pub(crate) candidates: Vec<String>,
}

/// Load permission allow/deny rules from settings files into the PermissionManager.
///
/// Reads from (in order, later files override earlier):
/// 1. `~/.shannon/settings.json`  (user-level)
/// 2. `.shannon/settings.json`    (project-level)
/// 3. `.claude/settings.json`     (Claude Code compatibility)
///
/// Expected format:
/// ```json
/// {
///   "permissions": {
///     "allow": ["Tool(name)", "Bash(git *)"],
///     "deny": ["Bash(rm -rf *)"]
///   }
/// }
/// ```
fn load_permission_rules(pm: &mut PermissionManager) {
    let cwd = std::env::current_dir().unwrap_or_default();
    let home = dirs::home_dir();

    let mut paths = Vec::new();
    if let Some(ref h) = home {
        paths.push(h.join(".shannon").join("settings.json"));
    }
    paths.push(cwd.join(".shannon").join("settings.json"));
    paths.push(cwd.join(".claude").join("settings.json"));

    for path in paths {
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let doc: serde_json::Value = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("Skipping invalid settings file {}: {e}", path.display());
                continue;
            }
        };

        let perms = match doc.get("permissions") {
            Some(p) => p,
            None => continue,
        };

        if let Some(allow_arr) = perms.get("allow").and_then(|v| v.as_array()) {
            for item in allow_arr {
                if let Some(s) = item.as_str() {
                    // Simple tool names like "Bash" or glob patterns like "mcp__*"
                    if s.contains('(') || s.contains('*') || s.contains('?') {
                        pm.allow_pattern(s);
                    } else {
                        pm.allow_tool(s);
                    }
                }
            }
        }

        if let Some(deny_arr) = perms.get("deny").and_then(|v| v.as_array()) {
            for item in deny_arr {
                if let Some(s) = item.as_str() {
                    if s.contains('(') || s.contains('*') || s.contains('?') {
                        pm.deny_pattern(s);
                    } else {
                        pm.deny_tool(s);
                    }
                }
            }
        }

        tracing::info!("Loaded permission rules from {}", path.display());
    }
}

impl Repl {
    /// Create a new REPL instance
    pub fn new() -> Result<Self> {
        let runtime = Runtime::new()?;
        let _rt_guard = runtime.enter();

        // Create tool registry and register all tools (sandboxed to project dir)
        let project_dir = std::env::current_dir().unwrap_or_default();
        let mut tool_registry = ToolRegistry::new();
        let agent_context_handle = register_default_tools_with_project_dir(&mut tool_registry, &project_dir).map_err(|e| anyhow::anyhow!("Failed to register tools: {e}"))?;

        // Load and register skills from shannon-skills as tools.
        // Also capture the formatted skills list for LLM context injection.
        let (_, skills_for_llm) = register_skills_as_tools(&mut tool_registry);

        // Discover MCP server configurations and register their tools dynamically.
        // Servers are batched to avoid file descriptor exhaustion:
        //   - Local (stdio) servers: batches of 3
        //   - Remote (http/sse) servers: batches of 20
        let mut discovered_mcp_prompts: Vec<(String, PromptInfo)> = Vec::new(); // populated during pooled discovery
        let mcp_pool = Arc::new(McpProcessPool::new()); // persistent pool for all MCP servers
        {
            let mut mcp_registry = shannon_core::mcp_advanced::McpServerRegistry::new();
            let mcp_count = mcp_registry.load_from_default_paths();
            if mcp_count > 0 {
                tracing::info!("Discovered {} MCP server configuration(s)", mcp_count);

                // Load approval state for MCP server gating
                let approval_path = std::path::PathBuf::from(".shannon/mcp_approvals.json");
                let mut approval_manager = shannon_core::McpApprovalManager::with_defaults();
                if let Err(e) = approval_manager.load_from_file(&approval_path) {
                    tracing::debug!("Could not load MCP approval state: {}", e);
                }

                let discovery_rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap_or_else(|_| tokio::runtime::Runtime::new().unwrap());

                // Classify servers into local (stdio) and remote (http/sse) buckets
                let mut local_servers: Vec<(String, String, Vec<String>, HashMap<String, String>, Vec<String>)> = Vec::new();
                let mut http_servers: Vec<(String, String, HashMap<String, String>, Vec<String>)> = Vec::new(); // (name, url, headers, oauth_scopes)

                for config in mcp_registry.enabled_servers() {
                    // Check server approval before attempting discovery
                    let approval_transport = match config.transport_type {
                        shannon_core::mcp_advanced::TransportType::Stdio => shannon_core::mcp_server_approval::McpTransportType::Stdio,
                        shannon_core::mcp_advanced::TransportType::Http => shannon_core::mcp_server_approval::McpTransportType::StreamableHttp,
                        shannon_core::mcp_advanced::TransportType::Sse => shannon_core::mcp_server_approval::McpTransportType::Sse,
                    };
                    let mut approval_req = shannon_core::McpServerApprovalRequest::new(
                        &config.name,
                        approval_transport,
                    );
                    if let Some(ref url) = config.url {
                        approval_req.server_url = Some(url.clone());
                    }
                    approval_req.capabilities.push("tools".to_string());
                    let decision = approval_manager.request_approval(approval_req)
                        .unwrap_or(shannon_core::ApprovalDecision::Deny);
                    match decision {
                        shannon_core::ApprovalDecision::Deny => {
                            tracing::warn!(
                                "MCP server '{}' denied by approval policy, skipping",
                                config.name
                            );
                            continue;
                        }
                        shannon_core::ApprovalDecision::ApproveWithRestrictions { .. } => {
                            tracing::warn!(
                                "MCP server '{}' requires manual approval. \
                                 Use /mcp approve {} to enable on next startup.",
                                config.name, config.name
                            );
                            continue;
                        }
                        shannon_core::ApprovalDecision::Approve => {}
                    }

                    match (&config.command, &config.url) {
                        (Some(cmd), _) => {
                            // Stdio transport
                            let entry = (config.name.clone(), cmd.clone(), config.args.clone(), config.env.clone(), config.oauth_scopes.clone());
                            local_servers.push(entry);
                        }
                        (None, Some(url)) => {
                            // HTTP/SSE transport — discover via HTTP
                            http_servers.push((config.name.clone(), url.clone(), config.headers.clone(), config.oauth_scopes.clone()));
                        }
                        (None, None) => {
                            tracing::warn!(
                                "Skipping '{}' (no command or URL configured)",
                                config.name
                            );
                            continue;
                        }
                    }
                }

                const LOCAL_BATCH_SIZE: usize = 3;
                const REMOTE_BATCH_SIZE: usize = 20;

                // Use the persistent pool created above the discovery block.
                // This replaces one-shot process spawning with persistent connections,
                // eliminating per-call initialization overhead.
                let mcp_pool = mcp_pool.clone();

                // Collect all pooled MCP tool adapters
                let mut all_pooled_adapters: Vec<shannon_mcp::PooledMcpToolAdapter> = Vec::new();

                // Discover local (stdio) servers via persistent pool connections
                for batch in local_servers.chunks(LOCAL_BATCH_SIZE) {
                    let futures: Vec<_> = batch
                        .iter()
                        .map(|(name, cmd, args, env, _scopes)| {
                            discover_pooled_tools(
                                mcp_pool.clone(),
                                name,
                                cmd,
                                args,
                                env,
                            )
                        })
                        .collect();
                    let results = discovery_rt.block_on(futures::future::join_all(futures));
                    for (result, (name, _, _, _, _scopes)) in results.into_iter().zip(batch.iter()) {
                        match result {
                            Ok(discovery) => {
                                let tool_count = discovery.tools.len();
                                all_pooled_adapters.extend(discovery.tools);
                                tracing::info!(
                                    "Discovered {} tool(s) from '{}' (pooled)",
                                    tool_count,
                                    name
                                );
                            }
                            Err(e) => {
                                tracing::warn!("MCP server '{}' discovery failed: {e}", name);
                            }
                        }
                    }
                }

                // Discover remote (http/sse) servers via persistent pool connections
                for batch in http_servers.chunks(REMOTE_BATCH_SIZE) {
                    let futures: Vec<_> = batch
                        .iter()
                        .map(|(name, url, headers, _scopes)| {
                            let header_sources: HashMap<String, HeaderSource> = headers
                                .iter()
                                .map(|(k, v)| (k.clone(), HeaderSource::Static(v.clone())))
                                .collect();
                            discover_pooled_remote_tools(
                                mcp_pool.clone(),
                                name,
                                url,
                                header_sources,
                                None,
                            )
                        })
                        .collect();
                    let results = discovery_rt.block_on(futures::future::join_all(futures));
                    for (result, (name, _, _, _scopes)) in results.into_iter().zip(batch.iter()) {
                        match result {
                            Ok(discovery) => {
                                let tool_count = discovery.tools.len();
                                all_pooled_adapters.extend(discovery.tools);
                                tracing::info!(
                                    "Discovered {} tool(s) from '{}' (pooled, remote)",
                                    tool_count,
                                    name
                                );
                            }
                            Err(e) => {
                                tracing::warn!("MCP server '{}' discovery failed: {e}", name);
                            }
                        }
                    }
                }

                // Auto-enable deferred schema loading when there are many MCP tools.
                // Note: deferred mode is set AFTER discovery for pooled adapters since the
                // adapters already stored their real schemas during discovery if the pool's
                // deferred flag was enabled. We set it now and rebuild with minimal schemas.
                if all_pooled_adapters.len() > shannon_core::DEFERRED_SCHEMA_THRESHOLD {
                    tracing::info!(
                        "Enabling deferred schema loading for {} MCP tools (threshold: {})",
                        all_pooled_adapters.len(),
                        shannon_core::DEFERRED_SCHEMA_THRESHOLD
                    );
                    mcp_pool.set_defer_tool_schemas(true);

                    // Build a DeferredSchemaStore from the pool's stored schemas
                    let store = shannon_core::DeferredSchemaStore::default();
                    for name in mcp_pool.deferred_schema_tool_names() {
                        if let Some(schema) = mcp_pool.get_deferred_schema(&name) {
                            store.lock().unwrap().insert(name, schema);
                        }
                    }
                    let search_tool = shannon_core::DeferredSchemaSearchTool::new(store);
                    if let Err(e) = tool_registry.register(Box::new(search_tool)) {
                        tracing::debug!("mcp__tool_search registration skipped: {}", e);
                    }
                }

                // Register all pooled MCP tool adapters
                for tool in all_pooled_adapters {
                    if let Err(e) = tool_registry.register(Box::new(tool)) {
                        tracing::debug!("MCP tool registration skipped: {}", e);
                    }
                }

                if mcp_pool.is_defer_tool_schemas() {
                    tracing::info!(
                        "Deferred mode active: {} tool schemas stored",
                        mcp_pool.deferred_schema_tool_names().len()
                    );
                }

                // Discover prompts from all connected servers and populate
                // discovered_mcp_prompts for slash-command registration below.
                let pooled_prompts = discovery_rt.block_on(mcp_pool.list_all_prompts());
                for (server_name, prompts) in pooled_prompts {
                    for p in prompts {
                        let arg_names = p.arguments
                            .map(|args| args.into_iter().map(|a| a.name).collect())
                            .unwrap_or_default();
                        discovered_mcp_prompts.push((
                            server_name.clone(),
                            PromptInfo {
                                name: p.name,
                                description: p.description,
                                argument_names: arg_names,
                            },
                        ));
                    }
                }

                // Persist approval state (auto-approved servers, any new denies)
                if let Err(e) = approval_manager.save_to_file(&approval_path) {
                    tracing::debug!("Could not save MCP approval state: {}", e);
                }
            }
        }

        // Create LLM client
        let client_config = LlmClientConfig::default();

        // Inject team context into AgentTool for sub-agent execution + team coordination
        // This requires a tokio runtime; skip gracefully in test contexts without one.
        let mut shared_coordinator: Option<std::sync::Arc<shannon_agents::AgentCoordinator>> = None;
        if let Ok(mut guard) = agent_context_handle.lock() {
            let team_ctx = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(
                        shannon_tools::AgentToolContext::new(client_config.clone())
                    )
                })
            }));
            match team_ctx {
                Ok(Ok(ctx)) => {
                    // Inject shared LLM executor so teammates can make real LLM calls
                    let ctx = {
                        let llm_client = shannon_core::api::LlmClient::new(ctx.client_config.clone());
                        let executor = shannon_agents::shared_executor(llm_client);
                        ctx.with_executor(executor)
                    };
                    // Register team coordination tools (team_task_create/update/list)
                    if let Err(e) = shannon_tools::register_team_tools(&mut tool_registry, ctx.coordinator.clone()) {
                        tracing::warn!("Team tool registration failed: {e}");
                    }
                    shared_coordinator = Some(ctx.coordinator.clone());
                    *guard = Some(ctx);
                }
                Ok(Err(e)) if e.to_string().contains("Agent teams disabled") => {}
                Ok(Err(e)) => tracing::warn!("Team context init failed: {e}"),
                Err(_) => {} // No tokio runtime (test context) — team features disabled
            }
        }

        // Validate config and show warning if not fully configured
        if let Err(e) = client_config.validate() {
            eprintln!("Warning: {e}");
        }
        tracing::info!("LLM config: {}", client_config.describe());

        let client = if client_config.provider.requires_auth() {
            shannon_core::api::LlmClient::new(client_config)
        } else {
            shannon_core::api::LlmClient::new_unauthenticated(client_config)
        };

        // Wrap tool registry in Arc so it can be shared with MCP callbacks
        // for dynamic tool re-registration.
        let tool_registry = std::sync::Arc::new(tool_registry);

        // Wire MCP sampling and elicitation providers so MCP servers can
        // request LLM completions (sampling) and ask the user questions (elicitation).
        {
            let pool = mcp_pool.clone();
            let llm = std::sync::Arc::new(client.clone());
            let sampling = shannon_mcp::make_sampling_provider(llm);
            // For now, elicitation auto-declines (no TUI callback wired yet).
            // Future: wire to input_dialog for interactive elicitation.
            let elicitation = shannon_mcp::make_elicitation_provider(None);
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap_or_else(|_| tokio::runtime::Runtime::new().unwrap());
            rt.block_on(async {
                pool.set_sampling_provider(sampling).await;
                pool.set_elicitation_provider(elicitation).await;
                // Expose the project directory as a filesystem root so MCP servers
                // (e.g. filesystem, git) know the workspace boundaries.
                let project_dir = std::env::current_dir().unwrap_or_default();
                pool.set_roots_provider(std::sync::Arc::new(move || {
                    let uri = format!("file://{}", project_dir.display());
                    vec![shannon_mcp::Root {
                        uri,
                        name: Some("project".to_string()),
                    }]
                }))
                .await;

                // Dynamic tool re-registration: when a server reports
                // tools/list_changed, swap out its old tools for the new ones.
                let reg = tool_registry.clone();
                pool.set_on_tools_changed(std::sync::Arc::new(move |server_name, new_tools| {
                    let prefix = format!("mcp__{server_name}__");
                    // Unregister old tools from this server.
                    {
                        let tools_to_remove: Vec<String> = reg.list()
                            .into_iter()
                            .filter(|n| n.starts_with(&prefix))
                            .collect();
                        for name in tools_to_remove {
                            if let Err(e) = reg.unregister(&name) {
                                tracing::debug!("Dynamic unregister {}: {}", name, e);
                            }
                        }
                    }
                    // Register new tools.
                    for tool in new_tools {
                        if let Err(e) = reg.register(Box::new(tool)) {
                            tracing::debug!("Dynamic register: {}", e);
                        }
                    }
                    tracing::info!(
                        server = %server_name,
                        "Dynamically re-registered tools from notification"
                    );
                })).await;
            });
        }

        // Start MCP config hot-reload watcher.
        // Polls config files every 5 seconds and applies changes dynamically.
        {
            let pool = mcp_pool.clone();
            let project_dir = std::env::current_dir().unwrap_or_default();
            pool.start_config_watcher(project_dir, std::time::Duration::from_secs(5));
        }

        // Wire MCP progress updates to the UI.
        // Progress notifications from MCP servers are forwarded to a channel
        // that the main event loop drains into the multi-progress widget.
        let mcp_progress_rx = {
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<(String, f64, Option<f64>)>();
            let pool = mcp_pool.clone();
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap_or_else(|_| tokio::runtime::Runtime::new().unwrap());
            rt.block_on(async move {
                pool.set_progress_callback(std::sync::Arc::new(move |tool_name, progress, total| {
                    let _ = tx.send((tool_name.to_string(), progress, total));
                })).await;
            });
            Some(rx)
        };

        // Create permission manager
        let mut permission_manager = PermissionManager::new();

        // Register destructive MCP tools with permission manager
        for name in tool_registry.destructive_tool_names() {
            permission_manager.register_destructive_tool(name);
        }

        // Load permission allow/deny rules from settings files
        load_permission_rules(&mut permission_manager);

        // Create state manager
        let state_manager = StateManager::new();

        // Create query engine with optional memory store
        let base_engine = QueryEngine::with_defaults_arc(
            client,
            tool_registry.clone(),
            permission_manager,
            state_manager,
        );

        // Initialize memory store at ~/.shannon/memories/
        let mut query_engine = {
            let memory_path = dirs::home_dir()
                .map(|h| h.join(".shannon").join("memories"))
                .unwrap_or_else(|| std::path::PathBuf::from(".shannon/memories"));
            let mut mem_store = shannon_core::MemoryStore::new(memory_path);
            // Load existing memories from disk (ignore errors on first run)
            let _ = mem_store.load();
            base_engine.with_memory(mem_store)
        };

        // Auto-load project instructions (Claude Code compatible hierarchy)
        {
            let cwd = std::env::current_dir().unwrap_or_default();

            // 1. Load full CLAUDE.md hierarchy (global → project → parents)
            let mem_manager = shannon_core::project_memory::ProjectMemoryManager::new(cwd.clone());
            if let Ok(merged) = mem_manager.load_merged() {
                if !merged.instructions.is_empty() {
                    let resolved = shannon_core::project_memory::resolve_imports(
                        &merged.instructions, &cwd,
                    );
                    query_engine.append_system_prompt(&format!(
                        "# Project Instructions\n\n{resolved}"
                    ));
                }
                tracing::info!("Loaded {} project memory source(s)", merged.sources.len());
            }

            // 2. Load MEMORY.md index (first 200 lines)
            if let Some(memory_content) = shannon_core::project_memory::load_memory_index(&cwd) {
                query_engine.append_system_prompt(&memory_content);
            }

            // 3. Load .claude/rules/*.md
            if let Some(rules) = shannon_core::project_memory::load_rules(&cwd) {
                query_engine.append_system_prompt(&rules);
            }

            // 4. Load git context (branch, recent commits, status)
            if let Some(git_ctx) = shannon_core::project_instructions::git_context(&cwd) {
                query_engine.append_system_prompt(&git_ctx);
            }

            // 5. Inject available skills list so the LLM knows what slash commands exist
            if !skills_for_llm.is_empty() {
                query_engine.append_system_prompt(&skills_for_llm);
            }

            // 6. Attach ContextInjector for hot-reload + compaction reinjection
            let storage_dir = dirs::home_dir()
                .map(|h| h.join(".shannon"))
                .unwrap_or_else(|| cwd.clone());
            let injector = shannon_core::query_engine::ContextInjector::new(
                cwd, storage_dir,
            );
            query_engine = query_engine.with_context_injector(injector);
        }

        // Create permission request channel
        let (permission_req_tx, permission_req_rx) = tokio::sync::mpsc::unbounded_channel();

        // Create command registry inside the runtime context so register_sync
        // can access the tokio runtime handle.
        let command_registry = runtime.block_on(async {
            let registry = CommandRegistry::new();
            builtin_commands::register_all(&registry);

            // Register MCP prompts as slash commands: /mcp__{server}__{prompt}
            for (server, prompt) in &discovered_mcp_prompts {
                let cmd_name = format!("mcp__{}__{}", server, prompt.name);
                let arg_hint = if prompt.argument_names.is_empty() {
                    None
                } else {
                    Some(prompt.argument_names.join(", "))
                };
                let prompt_template = format!(
                    "Use the get_mcp_prompt tool to retrieve and execute the '{}' prompt from the '{}' MCP server with these arguments: {{args}}",
                    prompt.name, server
                );
                let command = Command::Prompt(Box::new(PromptCommand {
                    base: CommandBase {
                        name: cmd_name,
                        aliases: Vec::new(),
                        description: prompt.description.clone(),
                        has_user_specified_description: false,
                        availability: vec![shannon_commands::CommandAvailability::All],
                        source: shannon_commands::CommandSource::Builtin,
                        is_enabled: true,
                        is_hidden: false,
                        argument_hint: arg_hint,
                        when_to_use: None,
                        version: None,
                        disable_model_invocation: false,
                        user_invocable: true,
                        is_workflow: false,
                        immediate: false,
                        is_sensitive: false,
                        user_facing_name: None,
                    },
                    progress_message: format!("Loading MCP prompt '{}' from '{}'", prompt.name, server),
                    content_length: 0,
                    arg_names: prompt.argument_names.clone(),
                    allowed_tools: vec!["get_mcp_prompt".to_string()],
                    model: None,
                    hooks: HashMap::new(),
                    context: ExecutionContext::Inline,
                    agent: None,
                    paths: Vec::new(),
                    prompt_template: Some(prompt_template),
                }));
                registry.register_sync(command);
            }

            // Discover custom commands from .claude/commands/ and .shannon/commands/
            // Claude Code compatible: .claude/commands/*.md → /command-name
            // Subdirectories: .claude/commands/project/foo.md → /project:foo
            {
                let mut custom_command_dirs: Vec<std::path::PathBuf> = Vec::new();

                // Project-level commands
                let cwd = std::env::current_dir().unwrap_or_default();
                custom_command_dirs.push(cwd.join(".claude").join("commands"));
                custom_command_dirs.push(cwd.join(".shannon").join("commands"));

                // User-level commands
                if let Some(home) = dirs::home_dir() {
                    custom_command_dirs.push(home.join(".claude").join("commands"));
                    custom_command_dirs.push(home.join(".shannon").join("commands"));
                }

                // Collect custom commands from all command directories
                let mut custom_commands: Vec<CustomCommandEntry> = Vec::new();
                for dir in &custom_command_dirs {
                    collect_custom_commands(dir, "", &mut custom_commands);
                }
                dedup_custom_commands(&mut custom_commands);

                for entry in &custom_commands {
                    let description = entry.description.clone()
                        .unwrap_or_else(|| format!("Custom command (from {})", entry.path.display()));
                    let arg_names = if entry.arguments.is_empty() {
                        vec!["$ARGUMENTS".to_string()]
                    } else {
                        entry.arguments.clone()
                    };
                    let argument_hint = if entry.arguments.is_empty() {
                        Some("$ARGUMENTS".to_string())
                    } else {
                        Some(entry.arguments.join(" "))
                    };
                    let command = Command::Prompt(Box::new(PromptCommand {
                        base: CommandBase {
                            name: entry.name.clone(),
                            aliases: Vec::new(),
                            description,
                            has_user_specified_description: entry.description.is_some(),
                            availability: vec![shannon_commands::CommandAvailability::All],
                            source: shannon_commands::CommandSource::Builtin,
                            is_enabled: true,
                            is_hidden: false,
                            argument_hint,
                            when_to_use: None,
                            version: None,
                            disable_model_invocation: false,
                            user_invocable: true,
                            is_workflow: false,
                            immediate: false,
                            is_sensitive: false,
                            user_facing_name: None,
                        },
                        progress_message: format!("Running /{}...", entry.name),
                        content_length: entry.template.len(),
                        arg_names,
                        allowed_tools: entry.allowed_tools.clone(),
                        model: entry.model.clone(),
                        hooks: HashMap::new(),
                        context: ExecutionContext::Inline,
                        agent: entry.agent.clone(),
                        paths: Vec::new(),
                        prompt_template: Some(entry.template.clone()),
                    }));
                    registry.register_sync(command);
                }
                if !custom_commands.is_empty() {
                    tracing::info!("Registered {} custom command(s) from .claude/commands/ and .shannon/commands/", custom_commands.len());
                }
            }

            registry
        });

        // Wrap the executor in SharedExecutor for concurrent command dispatch
        let shared_executor = {
            use shannon_commands::CommandExecutor;
            SharedExecutor::new(CommandExecutor::new(command_registry.clone()))
        };

        let mut repl = Self {
            events: EventHandler::new(50)?,
            renderer: Renderer::new(),
            chat: ChatWidget::new(1000),
            prompt: PromptWidget::new(),
            state: {
                let mut s = ReplState::default();
                let prefs = preferences::load_preferences();
                if let Some(model) = prefs.model {
                    s.model = Some(model);
                }
                if let Some(provider) = prefs.provider {
                    s.selected_provider = Some(provider);
                }
                if let Some(theme_name) = prefs.theme {
                    if let Some(theme) = Theme::named(&theme_name) {
                        s.theme = theme;
                    }
                }
                s
            },
            running: false,
            query_engine: Some(query_engine),
            state_manager: StateManager::new(),
            command_registry,
            command_parser: CommandParser::new(),
            shared_executor,
            runtime,
            permission_req_rx,
            permission_req_tx,
            last_session_list: Vec::new(),
            command_history: ReplHistory::new(1000),
            saved_input: String::new(),
            diff_data: DiffData::new(),
            current_turn: 0,
            session_started_at: Some(chrono::Utc::now()),
            output_renderer: ReplRenderer::new(),
            commands_run: 0,
            tools_invoked: 0,
            tab_completion_state: TabCompletionState::default(),
            vim_handler: VimHandler::new(),
            team_coordinator: shared_coordinator,
            agent_registry: None,
            mcp_pool,
            tool_registry,
            mcp_progress_rx,
            model_routes: Vec::new(),
            checkpoint_manager: shannon_core::CheckpointManager::new(),
            notifier: {
                let mut n = shannon_core::notifier::Notifier::new();
                // Add desktop notifier if available
                if shannon_core::notifier::DesktopNotifier::is_available() {
                    n.add_handler(Box::new(shannon_core::notifier::DesktopNotifier::new()));
                }
                n
            },
            notifications_enabled: false, // Disabled by default; enable via /notify
            webhook_receiver: None,
            instruction_watcher: {
                let cwd = std::env::current_dir().unwrap_or_default();
                if cwd.exists() {
                    Some(shannon_core::project_instructions::InstructionWatcher::new(cwd))
                } else {
                    None
                }
            },
            command_watcher: Some(CustomCommandWatcher::new()),
        };

        repl.sync_approval_mode_label();
        repl.renderer.set_theme(&repl.state.theme);
        Ok(repl)
    }

    /// Restore conversation history from a previously persisted session.
    ///
    /// Loads messages from the given `SessionData` and injects them into the
    /// query engine so the next user message continues the prior conversation.
    /// Also populates the chat widget so the user can see the restored history.
    /// Returns the number of messages restored.
    pub fn restore_session(&mut self, session_data: shannon_core::state::SessionData) -> usize {
        let msg_count = session_data.messages.len();
        if msg_count == 0 {
            return 0;
        }

        // Populate chat widget with restored messages so the user can see them
        for msg in &session_data.messages {
            let role = match msg.role.as_str() {
                "user" => ChatRole::User,
                "assistant" => ChatRole::Assistant,
                "system" => ChatRole::System,
                _ => ChatRole::Tool, // "tool" and any unknown roles
            };
            let text = match &msg.content {
                MessageContent::Text(t) => t.clone(),
                MessageContent::Blocks(blocks) => blocks
                    .iter()
                    .filter_map(|b| match b {
                        ContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            };
            self.chat.add_message(role, text);
        }

        if let Some(ref mut engine) = self.query_engine {
            let preview = session_data.first_user_message_preview(60);
            engine.replace_conversation(session_data.messages);
            tracing::info!(
                "Resumed session {} ({} messages, preview: {:?})",
                session_data.session_id,
                msg_count,
                preview,
            );
        }
        msg_count
    }

    /// Check for the most recent session and auto-restore it if it was
    /// active within the last 2 hours. Shows a system message to inform
    /// the user; they can start fresh with `/clear` if unwanted.
    fn auto_restore_last_session(&mut self) {
        let sessions = match self.state_manager.list_persisted_sessions() {
            Ok(s) => s,
            Err(_) => return,
        };
        if sessions.is_empty() {
            return;
        }

        // Find the most recently updated session
        let most_recent = sessions
            .iter()
            .max_by_key(|s| s.updated_at)
            .unwrap(); // safe: sessions is non-empty

        // Only auto-restore if updated within the last 2 hours
        let two_hours_ago = chrono::Utc::now() - chrono::Duration::hours(2);
        if most_recent.updated_at < two_hours_ago {
            return;
        }

        // Skip sessions with no turns (empty/stub sessions)
        if most_recent.turn_count == 0 {
            return;
        }

        let session_id = most_recent.session_id;
        let title = most_recent.title.as_deref()
            .or(most_recent.preview.as_deref())
            .unwrap_or("Untitled");

        if let Ok(Some(data)) = self.state_manager.load_session(&session_id) {
            let msg_count = data.messages.len();
            if msg_count == 0 {
                return;
            }

            // Show notice before restoring messages
            self.chat.add_message(ChatRole::System, format!(
                "Auto-restored session: \"{}\" ({} messages, {})\nType /clear to start fresh.",
                title, msg_count, most_recent.model,
            ));

            // Populate chat widget with restored messages
            for msg in &data.messages {
                let role = match msg.role.as_str() {
                    "user" => ChatRole::User,
                    "assistant" => ChatRole::Assistant,
                    "system" => ChatRole::System,
                    _ => ChatRole::Tool,
                };
                let text = match &msg.content {
                    MessageContent::Text(t) => t.clone(),
                    MessageContent::Blocks(blocks) => blocks
                        .iter()
                        .filter_map(|b| match b {
                            ContentBlock::Text { text } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n"),
                };
                self.chat.add_message(role, text);
            }

            // Restore in query engine
            if let Some(ref mut engine) = self.query_engine {
                engine.replace_conversation(data.messages);
                if let Err(e) = engine.restore_session(session_id) {
                    tracing::warn!("Auto-restore engine session failed: {e}");
                }
            }

            self.state.tokens_used = most_recent.total_input_tokens + most_recent.total_output_tokens;
            if !most_recent.model.is_empty() {
                self.state.model = Some(most_recent.model.clone());
            }

            tracing::info!(
                "Auto-restored session {} (\"{}\", {} msgs)",
                session_id, title, msg_count,
            );
        }
    }

    /// Cycle the approval mode and sync UI state.
    ///
    /// Advances through: Suggest -> AutoEdit -> Plan -> FullAuto -> Readonly -> Suggest.
    /// BypassPermissions requires a confirmation dialog before applying.
    pub fn cycle_approval_mode(&mut self) {
        if let Some(ref query_engine) = self.query_engine {
            let current = {
                let perms = query_engine.permissions().read().expect("permissions rwlock poisoned");
                perms.approval_mode()
            };

            let next = current.cycle_next();

            if next == shannon_core::permissions::ApprovalMode::BypassPermissions {
                self.show_confirm_dialog(
                    "Bypass Permissions",
                    "This will skip ALL permission checks. Only use in trusted environments.\n\nAre you sure?",
                    "set_bypass_mode",
                );
            } else {
                let mut perms = query_engine.permissions().write().expect("permissions rwlock poisoned");
                perms.set_approval_mode(next);
                let label = next.short_label().to_string();
                drop(perms);
                self.state.approval_mode_label = label.clone();
                self.state.status = format!("Mode: {label}");
                self.state.toast = Some((format!("  Mode: {label}  "), std::time::Instant::now()));
            }
        }
    }

    /// Sync the approval mode label from the PermissionManager to UI state.
    fn sync_approval_mode_label(&mut self) {
        if let Some(ref query_engine) = self.query_engine {
            let label = {
                let perms = query_engine.permissions().read().expect("permissions rwlock poisoned");
                perms.approval_mode().short_label().to_string()
            };
            self.state.approval_mode_label = label;
        }
    }

    /// Open the current input in an external editor ($EDITOR / $VISUAL).
    ///
    /// Writes the current prompt text to a temp file, spawns the editor,
    /// waits for it to exit, then reads the file back and updates the prompt.
    pub fn open_external_editor(&mut self) {
        let editor = std::env::var("VISUAL")
            .or_else(|_| std::env::var("EDITOR"))
            .unwrap_or_else(|_| "vi".to_string());

        let tmp_dir = std::env::temp_dir();
        let tmp_path = tmp_dir.join("shannon-input.md");

        // Write current input to temp file
        let current_input = self.prompt.input().to_string();
        if let Err(e) = std::fs::write(&tmp_path, &current_input) {
            self.state.toast = Some((format!("  Failed to write temp file: {e}  "), std::time::Instant::now()));
            return;
        }

        // Suspend raw mode, spawn editor, wait
        let _ = crossterm::terminal::disable_raw_mode();
        let result = std::process::Command::new(&editor)
            .arg(&tmp_path)
            .status();
        let _ = crossterm::terminal::enable_raw_mode();

        match result {
            Ok(status) if status.success() => {
                if let Ok(new_text) = std::fs::read_to_string(&tmp_path) {
                    let trimmed = new_text.trim_end().to_string();
                    self.prompt.set_input(trimmed);
                    self.state.toast = Some(("  Editor saved  ".to_string(), std::time::Instant::now()));
                }
            }
            Ok(_) => {
                self.state.toast = Some(("  Editor exited with error  ".to_string(), std::time::Instant::now()));
            }
            Err(e) => {
                self.state.toast = Some((format!("  Failed to launch editor: {e}  "), std::time::Instant::now()));
            }
        }

        let _ = std::fs::remove_file(&tmp_path);
    }

    /// Toggle focus mode (hide/show header and statusbar).
    pub fn toggle_focus_mode(&mut self) {
        self.state.focus_mode = !self.state.focus_mode;
        if self.state.focus_mode {
            // Entering focus mode disables fullscreen (focus is a subset)
            self.state.fullscreen_mode = false;
        }
        let label = if self.state.focus_mode { "Focus ON" } else { "Focus OFF" };
        self.state.toast = Some((format!("  {label}  "), std::time::Instant::now()));
    }

    /// Toggle fullscreen mode (hide ALL chrome, chat fills terminal).
    /// Bound to F11.
    pub fn toggle_fullscreen_mode(&mut self) {
        self.state.fullscreen_mode = !self.state.fullscreen_mode;
        if self.state.fullscreen_mode {
            // Fullscreen implies focus mode too
            self.state.focus_mode = true;
        }
        let label = if self.state.fullscreen_mode { "Fullscreen ON (F11)" } else { "Fullscreen OFF" };
        self.state.toast = Some((format!("  {label}  "), std::time::Instant::now()));
    }

    /// Toggle chat search mode (highlight matches in chat).
    pub fn toggle_chat_search(&mut self) {
        if self.state.chat_search_active {
            // Deactivate search
            self.state.chat_search_active = false;
            self.state.chat_search_query.clear();
            self.state.chat_search_match_index = 0;
            self.state.chat_search_total_matches = 0;
        } else {
            // Activate search
            self.state.chat_search_active = true;
            self.state.chat_search_query.clear();
            self.state.chat_search_match_index = 0;
            self.state.chat_search_total_matches = 0;
        }
    }

    /// Update chat search results based on current query.
    pub fn update_chat_search(&mut self) {
        if !self.state.chat_search_active || self.state.chat_search_query.is_empty() {
            self.state.chat_search_total_matches = 0;
            self.state.chat_search_match_index = 0;
            return;
        }
        let matches = self.chat.find_search_matches(&self.state.chat_search_query);
        self.state.chat_search_total_matches = matches.len();
        if self.state.chat_search_match_index >= matches.len() {
            self.state.chat_search_match_index = 0;
        }
    }

    /// Navigate to the next search match.
    pub fn chat_search_next(&mut self) {
        if self.state.chat_search_total_matches > 0 {
            self.state.chat_search_match_index =
                (self.state.chat_search_match_index + 1) % self.state.chat_search_total_matches;
        }
    }

    /// Navigate to the previous search match.
    pub fn chat_search_prev(&mut self) {
        if self.state.chat_search_total_matches > 0 {
            self.state.chat_search_match_index = if self.state.chat_search_match_index == 0 {
                self.state.chat_search_total_matches - 1
            } else {
                self.state.chat_search_match_index - 1
            };
        }
    }

    /// Push a notification into the pending queue (shown in status bar).
    /// Old notifications (>30s) are pruned automatically.
    pub fn notify(&mut self, message: impl Into<String>) {
        let msg = message.into();
        self.state.pending_notifications.retain(|(_, t)| t.elapsed().as_secs() < 30);
        self.state.pending_notifications.push((msg, std::time::Instant::now()));
    }

    /// Check if this is a first run (no config files) and activate onboarding.
    pub fn check_first_run(&mut self) {
        let local = std::path::Path::new(".shannon.toml").exists();
        let home = std::path::Path::new(&format!(
            "{}/.shannon/config.toml",
            std::env::var("HOME").unwrap_or_default()
        ))
        .exists();
        if !local && !home {
            self.state.onboarding_active = true;
        }
    }

    /// Toggle the transcript pager on/off.
    pub fn toggle_pager(&mut self) {
        self.state.pager_active = !self.state.pager_active;
        self.state.pager_scroll = 0;
    }

    /// Scroll the pager by `delta` lines (negative = up, positive = down).
    pub fn pager_scroll(&mut self, delta: isize) {
        let total = self.chat.message_count();
        if let Some(area_height) = self.terminal_height() {
            let max_scroll = total.saturating_sub(area_height);
            let new = self.state.pager_scroll as isize + delta;
            self.state.pager_scroll = new.clamp(0, max_scroll as isize) as usize;
        }
    }

    /// Scroll pager to top.
    pub fn pager_scroll_top(&mut self) {
        self.state.pager_scroll = 0;
    }

    /// Scroll pager to bottom.
    pub fn pager_scroll_bottom(&mut self) {
        let total = self.chat.message_count();
        if let Some(area_height) = self.terminal_height() {
            self.state.pager_scroll = total.saturating_sub(area_height);
        }
    }

    /// Get the terminal height (content area, excluding borders).
    fn terminal_height(&self) -> Option<usize> {
        // Approximate: use 80% of terminal height for content
        crossterm::terminal::size().ok().map(|(_, h)| (h as usize).saturating_sub(6))
    }

    /// Check if project instruction files have changed and hot-reload them.
    ///
    /// Returns true if instructions were reloaded, false if unchanged.
    pub fn check_reload_instructions(&mut self) -> bool {
        let changed_info = match self.instruction_watcher.as_mut() {
            Some(w) => w.check_and_reload(),
            None => return false,
        };

        match changed_info {
            Some((files, new_content)) => {
                if let Some(ref mut engine) = self.query_engine {
                    // Reset system prompt to base + reloaded instructions
                    // The engine's append_system_prompt adds cumulatively, so we
                    // need to be smarter: just log the change and append a note.
                    tracing::info!("Hot-reloaded project instructions: {:?}", files);
                    if !new_content.is_empty() {
                        let reload_msg = format!(
                            "\n\n[SYSTEM: Project instructions were hot-reloaded from: {}]",
                            files.join(", ")
                        );
                        engine.append_system_prompt(&reload_msg);
                    }
                }
                true
            }
            None => false,
        }
    }

    /// Check if custom command files have changed and hot-reload them.
    pub fn check_reload_commands(&mut self) {
        if let Some(ref mut watcher) = self.command_watcher {
            let count = watcher.check_and_reload(&self.command_registry);
            if count > 0 {
                self.chat.add_message(
                    crate::widgets::ChatRole::System,
                    format!("[Custom commands hot-reloaded: {count} command(s)]"),
                );
            }
        }
    }

    /// Run the main REPL loop
    pub fn run(&mut self) -> Result<()> {
        // Check for interactive terminal
        if !atty::is(atty::Stream::Stdout) || !atty::is(atty::Stream::Stdin) {
            // Stdin pipe mode: read input and process as a single query
            return self.run_pipe_mode();
        }

        // Setup terminal
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        // Enable bracketed paste mode for proper multi-line paste handling
        execute!(stdout, crossterm::event::EnableBracketedPaste)?;
        // Enable mouse capture for scroll support
        execute!(stdout, crossterm::event::EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        self.running = true;

        // Show onboarding dialog on first run (no config files)
        self.check_first_run();

        // Fire SessionStart hooks (Claude Code compatible lifecycle)
        if let Some(ref engine) = self.query_engine {
            let session_id = engine.session_id().to_string();
            let hook_mgr = engine.hook_manager();
            let event = shannon_core::hooks::HookEvent::SessionStart {
                session_id: session_id.clone(),
            };
            self.runtime.block_on(async {
                let mgr = hook_mgr.read().await;
                if let Err(e) = mgr.run_hooks(&event).await {
                    tracing::debug!("SessionStart hook error: {e}");
                }
            });
        }

        // Show welcome message rendered through the markdown renderer
        let welcome_md = self.renderer.render_markdown(
            &format!("# {}\n\n{}", t!("repl.welcome"), t!("repl.welcome_help"))
        );
        let welcome_text: String = welcome_md.iter()
            .flat_map(|line| line.spans.iter().map(|s| s.content.clone()))
            .collect::<Vec<_>>()
            .join("");
        self.chat.add_message(
            ChatRole::System,
            welcome_text,
        );

        // Check for updates on startup (non-blocking)
        if let Some(update_msg) = self.check_for_updates() {
            self.chat.add_message(ChatRole::System, update_msg);
        }

        // Auto-restore the most recent session if it was active within the last 2 hours.
        self.auto_restore_last_session();

        // Main event loop
        while self.running {
            // Check for permission requests (non-blocking)
            if self.state.permission_dialog.is_none() {
                if let Ok(permission_req) = self.permission_req_rx.try_recv() {
                    // Store the permission prompt and response channel
                    self.state.permission_dialog = Some(permission_req.prompt.clone());
                    self.state.permission_response_tx = Some(permission_req.response_tx);

                    // Also populate the tool approval widget for enhanced display
                    let risk = match permission_req.prompt.risk_level {
                        shannon_core::permissions::RiskLevel::Safe
                        | shannon_core::permissions::RiskLevel::Low => {
                            crate::widgets::tool_approval::RiskLevel::Low
                        }
                        shannon_core::permissions::RiskLevel::Medium => {
                            crate::widgets::tool_approval::RiskLevel::Medium
                        }
                        shannon_core::permissions::RiskLevel::High
                        | shannon_core::permissions::RiskLevel::Critical => {
                            crate::widgets::tool_approval::RiskLevel::High
                        }
                    };
                    self.state.tool_approval.show_request(
                        crate::widgets::tool_approval::ToolApprovalRequest {
                            tool_name: permission_req.prompt.tool_name.clone(),
                            description: permission_req.prompt.description.clone(),
                            risk_level: risk,
                            detail: None,
                        },
                    );
                }
            }

            // Drain MCP progress updates into the multi-progress widget
            if let Some(ref mut rx) = self.mcp_progress_rx {
                let mut had_updates = false;
                while let Ok((tool_name, progress, total)) = rx.try_recv() {
                    if !had_updates {
                        self.state.multi_progress_visible = true;
                        had_updates = true;
                    }
                    let pct = if let Some(t) = total {
                        if t > 0.0 { (progress / t).clamp(0.0, 1.0) } else { progress.clamp(0.0, 1.0) }
                    } else {
                        progress.clamp(0.0, 1.0)
                    };
                    self.state.multi_progress.add_or_update(&tool_name, pct, ratatui::style::Color::Cyan);
                }
            }

            // Refresh agent states for sidebar display
            if self.agent_registry.is_some() {
                self.refresh_agents();
            }

            // Check custom command files for filesystem changes (notify-based)
            self.check_reload_commands();

            // Check scheduled routines and inject due prompts
            let due = self.state.routine_manager.drain_due();
            for (name, prompt) in due {
                self.chat.add_message(ChatRole::System,
                    format!("[Routine: {name}] {prompt}"));
            }

            // Draw UI
            render::draw_frame(&mut terminal, self)?;

            // Handle events
            if let Some(event) = self.events.next()? {
                self.handle_event(event);
            }
        }

        // Fire SessionEnd hooks before shutting down
        if let Some(ref engine) = self.query_engine {
            let session_id = engine.session_id().to_string();
            let hook_mgr = engine.hook_manager();
            let event = shannon_core::hooks::HookEvent::SessionEnd {
                session_id: session_id.clone(),
            };
            self.runtime.block_on(async {
                let mgr = hook_mgr.read().await;
                if let Err(e) = mgr.run_hooks(&event).await {
                    tracing::debug!("SessionEnd hook error: {e}");
                }
            });

            // Auto-save session for --resume support
            if self.current_turn > 0 {
                let messages = engine.conversation_history();
                let metadata = shannon_core::state::SessionPersistMetadata {
                    model: self.state.model.clone().unwrap_or_default(),
                    created_at: self.session_started_at.unwrap_or_else(chrono::Utc::now),
                    updated_at: chrono::Utc::now(),
                    total_input_tokens: self.state.tokens_used,
                    total_output_tokens: 0,
                    turn_count: messages.iter().filter(|m| m.role == "user").count(),
                    title: None,
                    parent_session_id: None,
                    branch_point_message_index: None,
                };
                if let Err(e) = self.state_manager.save_session(
                    &engine.session_id(),
                    &messages,
                    &metadata,
                ) {
                    tracing::debug!("Auto-save session error: {e}");
                }
            }
        }

        // Restore terminal
        disable_raw_mode()?;
        execute!(
            terminal.backend_mut(),
            crossterm::event::DisableMouseCapture
        )?;
        execute!(
            terminal.backend_mut(),
            crossterm::event::DisableBracketedPaste
        )?;
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen
        )?;
        terminal.show_cursor()?;

        // Print session cost summary to stdout after terminal is restored
        if let Some(ref engine) = self.query_engine {
            if let Ok(tracker) = engine.cost_tracker().read() {
                let total_cost = tracker.total_cost();
                if tracker.total_input_tokens > 0 {
                    println!();
                    println!("── Session Summary ──");
                    println!("  Tokens: {} in + {} out  |  Cost: ${total_cost:.4}",
                        tracker.total_input_tokens, tracker.total_output_tokens);
                    if let Some(budget) = tracker.budget_limit_usd {
                        let pct = (total_cost / budget) * 100.0;
                        println!("  Budget: ${total_cost:.4} / ${budget:.2} ({pct:.0}%)");
                    }
                    println!("  Model: {}", tracker.model_name);
                    if let Some(started) = &self.session_started_at {
                        let elapsed = chrono::Utc::now() - *started;
                        let mins = elapsed.num_minutes();
                        let secs = elapsed.num_seconds() % 60;
                        println!("  Duration: {mins}m {secs}s");
                    }
                    println!("─────────────────────");
                }
            }
        }

        Ok(())
    }

    /// Handle individual events
    fn handle_event(&mut self, event: crate::events::Event) {
        match event {
            crate::events::Event::Input(key) => {
                if let Err(e) = input::handle_input(self, key) {
                    // Display error in UI chat instead of stderr to prevent escape sequence leakage
                    self.chat.add_message(
                        ChatRole::System,
                        format!("Input error: {e}")
                    );
                }
            }
            crate::events::Event::Paste(content) => {
                let line_count = content.lines().count();
                if line_count > PASTE_THRESHOLD_LINES {
                    self.state.paste_counter += 1;
                    let num = self.state.paste_counter;
                    self.state.pasted_texts.insert(num, content);
                    let display = format!("[Pasted Text #{num} {line_count} lines]");
                    self.prompt.insert_text(&display);
                } else {
                    self.prompt.insert_text(&content);
                }
                self.state.completion_suggestions.clear();
            }
            crate::events::Event::Mouse(mouse) => {
                use crossterm::event::{MouseEventKind, MouseButton};
                match mouse.kind {
                    MouseEventKind::ScrollUp => {
                        for _ in 0..3 { self.chat.scroll_up(); }
                    }
                    MouseEventKind::ScrollDown => {
                        for _ in 0..3 { self.chat.scroll_down(); }
                    }
                    MouseEventKind::Down(MouseButton::Left) => {}
                    _ => {}
                }
            }
            crate::events::Event::Tick => {
                // Advance spinner animation during query processing
                if self.state.status != "Ready" {
                    // Update streaming state for status indicator
                    self.state.streaming_state = if self.state.thinking_phase {
                        StreamingState::Thinking
                    } else if self.state.streaming_active {
                        StreamingState::Generating {
                            elapsed_secs: self.state.streaming_start
                                .map(|t| t.elapsed().as_secs())
                                .unwrap_or(0),
                        }
                    } else if let Some(ref tool) = self.state.active_tool {
                        StreamingState::CallingTool { name: tool.clone() }
                    } else {
                        StreamingState::Idle
                    };

                    // Set phase based on current state for diverse animation
                    let phase = if self.state.thinking_phase {
                        crate::widgets::progress::SpinnerPhase::Thinking
                    } else if self.state.streaming_active {
                        crate::widgets::progress::SpinnerPhase::Streaming
                    } else if self.state.active_tool.is_some() {
                        crate::widgets::progress::SpinnerPhase::Tool
                    } else {
                        crate::widgets::progress::SpinnerPhase::Default
                    };
                    self.state.spinner.set_phase(phase);
                    self.state.spinner.tick();
                }
            }
        }
    }

    /// Check for Shannon updates on startup (non-blocking)
    fn check_for_updates(&self) -> Option<String> {
        use shannon_core::updater::{AutoUpdater, UpdaterConfig};
        use std::time::Duration;

        let config = UpdaterConfig {
            repo: "shannon-code/shannon".to_string(),
            check_interval: Duration::from_secs(86400),
            enabled: true,
            include_prereleases: false,
        };
        let mut updater = AutoUpdater::new(config);

        match self.runtime.block_on(updater.check_for_update()) {
            shannon_core::updater::UpdateStatus::UpdateAvailable { current, latest, release } => {
                Some(format!(
                    "Update available: {} → {} ({}). Download: {}",
                    current, latest, release.tag_name, release.html_url
                ))
            }
            shannon_core::updater::UpdateStatus::CheckFailed { error } => {
                // Silently ignore update check failures — don't block startup
                let _ = error;
                None
            }
            _ => None,
        }
    }

    /// Get the current REPL state
    pub fn state(&self) -> &ReplState {
        &self.state
    }

    /// Build sidebar info from the current state, if the sidebar is visible.
    pub fn sidebar_info(&self) -> Option<crate::widgets::SidebarInfo> {
        if !self.state.sidebar_visible {
            return None;
        }
        let mut modified_files: Vec<(String, usize, usize)> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for turn in self.diff_data.get_session_diffs() {
            for fc in &turn.files_modified {
                if seen.insert(fc.path.clone()) {
                    modified_files.push((fc.path.clone(), fc.additions, fc.deletions));
                }
            }
        }
        let error_count = self.chat.iter_messages()
            .filter(|(_, m)| m.role == ChatRole::Tool && m.is_error)
            .count();
        let context_window = self.state.model.as_deref()
            .map(shannon_core::model_registry::context_window_for)
            .unwrap_or(200_000);

        // Refresh active_agents from registry if available
        let active_agents = if self.agent_registry.is_some() {
            // We can't easily call async .list() from this sync method,
            // so use the cached state.active_agents which is refreshed
            // in the main loop after coordinator events.
            self.state.active_agents.clone()
        } else {
            Vec::new()
        };

        let diagnostics: Vec<_> = self.state.diagnostic_store.diagnostics.iter().take(50).map(|d| crate::lsp_bridge::Diagnostic {
            severity: d.severity,
            message: d.message.clone(),
            file_path: d.file_path.clone(),
            line: d.line,
            source: d.source.clone(),
        }).collect();

        Some(crate::widgets::SidebarInfo {
            model: self.state.model.clone(),
            tokens_used: self.state.tokens_used,
            cost_usd: self.state.total_cost_usd,
            tools_invoked: self.tools_invoked,
            modified_files,
            total_additions: self.diff_data.total_additions(),
            total_deletions: self.diff_data.total_deletions(),
            error_count,
            context_window,
            active_agents,
            diagnostics,
            session_duration_secs: self.state.session_start
                .map(|t| t.elapsed().as_secs())
                .unwrap_or(0),
            turn_count: self.current_turn,
            commands_run: self.commands_run,
            tokens_per_sec: {
                let dur = self.state.session_start.map(|t| t.elapsed().as_secs_f64()).unwrap_or(0.0);
                if dur > 0.0 && self.state.tokens_used > 0 {
                    Some(self.state.tokens_used as f64 / dur)
                } else {
                    None
                }
            },
        })
    }

    /// Check context pressure and auto-compact if needed.
    /// Returns true if auto-compaction was performed.
    pub fn check_context_pressure(&mut self) -> bool {
        let context_window = self.state.model.as_deref()
            .map(shannon_core::model_registry::context_window_for)
            .unwrap_or(200_000) as u64;

        if context_window == 0 || self.state.tokens_used == 0 {
            return false;
        }

        let usage_ratio = self.state.tokens_used as f64 / context_window as f64;

        if usage_ratio > 0.85 {
            // Auto-compact: context pressure critical (>85%)
            self.do_auto_compact();
            return true;
        } else if usage_ratio > 0.70 {
            // Warning: context pressure high (>70%)
            let pct = (usage_ratio * 100.0) as u32;
            let remaining = context_window.saturating_sub(self.state.tokens_used);
            self.state.toast = Some((
                format!("  Context: {pct}% used ({remaining} tokens remaining) — /compact to reduce  "),
                std::time::Instant::now(),
            ));
        }
        false
    }

    /// Refresh active_agents from the SubAgentRegistry for sidebar display.
    /// Called from the main loop tick; uses the tokio runtime for async access.
    /// Detects agent completions and sends desktop notifications.
    pub fn refresh_agents(&mut self) {
        if let Some(ref registry) = self.agent_registry {
            let agents = self.runtime.block_on(registry.list_agents());

            // Detect agents that transitioned from active to completed/failed
            let prev_names: std::collections::HashSet<String> = self.state.active_agents
                .iter()
                .filter(|a| a.active)
                .map(|a| a.name.clone())
                .collect();

            let new_agents: Vec<AgentDisplay> = agents.into_iter().map(|a| {
                let active = matches!(a.status, shannon_agents::AgentStatus::Running | shannon_agents::AgentStatus::Spawning | shannon_agents::AgentStatus::Idle);
                AgentDisplay {
                    name: a.name,
                    status: a.status.to_string(),
                    active,
                    team: a.team,
                    turns_used: a.turns_used,
                    max_turns: a.config.max_turns,
                }
            }).collect();

            // Send desktop notification for newly completed agents
            use shannon_core::notifier::{DesktopNotifier, NotificationHandler, Notification, NotificationLevel};
            use chrono::Utc;

            for agent in &new_agents {
                if !agent.active && prev_names.contains(&agent.name) {
                    let notifier = DesktopNotifier::new();
                    let status = &agent.status;
                    if status == "completed" {
                        let notification = Notification {
                            title: format!("Agent {} completed", agent.name),
                            body: format!("Finished after {} turns", agent.turns_used),
                            level: NotificationLevel::Success,
                            id: format!("agent-{}-done", agent.name),
                            timestamp: Utc::now(),
                        };
                        let _ = notifier.send(&notification);
                    } else if status.starts_with("failed") {
                        let notification = Notification {
                            title: format!("Agent {} failed", agent.name),
                            body: status.clone(),
                            level: NotificationLevel::Error,
                            id: format!("agent-{}-fail", agent.name),
                            timestamp: Utc::now(),
                        };
                        let _ = notifier.send(&notification);
                    }
                }
            }

            self.state.active_agents = new_agents;
        }
    }

    /// Perform auto-compaction using truncate strategy (no LLM call needed).
    fn do_auto_compact(&mut self) {
        use shannon_core::compact::CompactEngine;

        let Some(ref mut engine) = self.query_engine else { return };

        let history = engine.conversation_history();
        if history.len() < 4 {
            return; // Not enough to compact
        }

        let compact_engine = match CompactEngine::with_defaults() {
            Ok(e) => e,
            Err(_) => return,
        };

        let before = history.len();
        let mut messages = history;

        // Use truncate strategy for auto-compact — fast, no extra API call
        if let Ok(result) = compact_engine.micro_compact(&mut messages) {
            let _ = compact_engine.post_compact_cleanup(&mut messages);
            engine.replace_conversation(messages);

            let after = engine.conversation_history().len();
            self.state.toast = Some((
                format!("  Auto-compacted: {before}→{after} messages  "),
                std::time::Instant::now(),
            ));
            tracing::info!("Auto-compacted context: {before}→{after} messages, {:.0}% reduction",
                result.reduction_ratio * 100.0);
        }
    }

    /// Get mutable reference to the REPL state
    pub fn state_mut(&mut self) -> &mut ReplState {
        &mut self.state
    }

    /// Run in pipe mode: read stdin, process as a single query, output result.
    fn run_pipe_mode(&mut self) -> Result<()> {
        use std::io::Read;
        let mut input = String::new();
        std::io::stdin().read_to_string(&mut input)?;
        let input = input.trim().to_string();

        if input.is_empty() {
            return Err("No input provided on stdin.".into());
        }

        // Process the input as a query (no TUI needed)
        self.chat.add_message(ChatRole::User, input.clone());

        if input.starts_with('/') {
            // Handle commands in pipe mode
            commands::submit_input(self)?;
            // Output last system/assistant message
            if let Some(msg) = self.chat.last_message() {
                println!("{}", msg.content);
            }
        } else {
            // Process as AI query
            query::handle_query(self, &input)?;
            // Output the assistant response
            if let Some(msg) = self.chat.last_message() {
                if msg.role == ChatRole::Assistant {
                    println!("{}", msg.content);
                }
            }
        }
        Ok(())
    }
}

// ── UiAdapter Implementation for Repl ─────────────────────────────────────

use crate::adapter::{UiAdapter, UiError, UiResult, DisplayMessage};
use async_trait::async_trait;

/// Implement UiAdapter for Repl to allow it to be used as a UI backend.
#[async_trait]
impl UiAdapter for Repl {
    fn supports_streaming(&self) -> bool {
        true // Terminal UI supports streaming output
    }

    async fn display(&self, message: &DisplayMessage) -> UiResult<()> {
        // The TUI event loop handles rendering via the chat widget.
        // This method exists so the Repl satisfies the trait; actual output
        // flows through QueryEvent streams in the main loop.
        let _ = message;
        Ok(())
    }

    async fn display_progress(&self, message: &str, percent: Option<u8>) -> UiResult<()> {
        // Update status with progress message.
        let _ = (message, percent);
        Ok(())
    }

    async fn read_input(&self, prompt: &str) -> UiResult<String> {
        // In the current terminal UI, input is handled by the event loop.
        let _ = prompt;
        Err(UiError::NotSupported(
            "read_input not supported in terminal UI - use the prompt widget instead".to_string(),
        ))
    }

    async fn confirm(&self, message: &str) -> UiResult<bool> {
        // Confirmation is handled through dialog widgets in the event loop.
        let _ = message;
        Err(UiError::NotSupported(
            "confirm not directly supported - use dialog widgets instead".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests;
