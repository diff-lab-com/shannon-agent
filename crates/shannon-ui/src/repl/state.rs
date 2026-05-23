//! REPL state types: ReplState, LoopState, RalphState, SidebarTab, etc.

use crate::{
    theme::Theme,
    widgets::{
        StreamingState,
        attachment_bar::AttachmentBarWidget,
        command_palette::CommandPaletteWidget,
        progress::{MultiProgressWidget, ProgressBarWidget, SpinnerWidget},
        session_tab::SessionTabWidget,
        tool_approval::ToolApprovalWidget,
    },
};

/// Tool output verbosity level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    /// Collapsed tool output (default).
    Default,
    /// All tool output expanded, streaming details visible.
    Verbose,
}

impl ViewMode {
    /// Cycle to the next mode: Default → Verbose → Default.
    pub fn cycle(self) -> Self {
        match self {
            Self::Default => Self::Verbose,
            Self::Verbose => Self::Default,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Default => "Default",
            Self::Verbose => "Verbose",
        }
    }
}

/// Application state for the REPL
#[derive(Debug, Clone)]
pub struct ReplState {
    /// Current status message
    pub status: String,
    /// Model name being used
    pub model: Option<String>,
    /// Provider associated with the selected model (synced to QueryEngine)
    pub selected_provider: Option<shannon_core::api::LlmProvider>,
    /// Resolved context window size (from engine: Ollama num_ctx > model registry)
    pub context_window: usize,
    /// Total tokens used
    pub tokens_used: u64,
    /// Detailed token breakdown: input tokens
    pub input_tokens: u64,
    /// Detailed token breakdown: output tokens
    pub output_tokens: u64,
    /// Detailed token breakdown: cache read tokens (hits)
    pub cache_read_tokens: u64,
    /// Detailed token breakdown: cache creation tokens (writes/misses)
    pub cache_creation_tokens: u64,
    /// Total cost in USD accumulated across all queries
    pub total_cost_usd: f64,
    /// Working directory for the session
    pub working_directory: String,
    /// Active permission dialog (if any)
    pub permission_dialog: Option<shannon_core::permissions::PermissionPrompt>,
    /// Permission response channel sender (if dialog is active)
    pub permission_response_tx:
        Option<tokio::sync::mpsc::UnboundedSender<shannon_core::permissions::PermissionChoice>>,
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
    /// Active theme picker widget (for /theme preview)
    pub theme_picker: Option<crate::widgets::select::FuzzyPickerWidget>,
    /// Current completion suggestions to display (populated by Tab, cleared by typing)
    pub completion_suggestions: Vec<String>,
    /// Scheduled routine manager for recurring tasks
    pub routine_manager: shannon_core::scheduled_routines::RoutineManager,
    /// Cron-based scheduler for /schedule command
    pub cron_tool: shannon_tools::CronTool,
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
    /// Reduced motion: disables animations (spinner, shimmer) for accessibility
    pub reduced_motion: bool,
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
    /// Total number of matches for current search query
    pub incremental_search_match_count: usize,
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
    /// Queued messages to auto-submit sequentially after current streaming completes.
    /// Messages are processed in FIFO order: first queued, first sent.
    pub queued_messages: Vec<String>,
    /// When the current streaming operation started
    pub streaming_start: Option<std::time::Instant>,
    /// When this session started (for duration display)
    pub session_start: Option<std::time::Instant>,
    /// Current vim mode label for display ("INSERT" or "NORMAL")
    pub vim_mode: String,
    /// Whether leader key mode is active (waiting for second key after Ctrl+X)
    pub leader_active: bool,
    /// Timestamp of last Esc press (for double-Esc detection)
    pub last_esc_time: Option<std::time::Instant>,
    /// Whether the fuzzy picker is in session-resume mode
    pub session_picker_active: bool,
    /// Custom prompt bar color (set via /color, e.g. "red", "#ff0000", "default")
    pub prompt_bar_color: Option<String>,
    /// Active tab in the sidebar panel
    pub sidebar_tab: SidebarTab,
    /// Cached approval mode label for display (updated on mode change)
    pub approval_mode_label: String,
    /// Active sub-agents for sidebar display (refreshed from agent_registry)
    pub active_agents: Vec<AgentDisplay>,
    /// Agent dashboard state (auto-created when agents exist, auto-removed when none)
    pub agent_dashboard: Option<crate::widgets::agent_bar::AgentDashboardState>,
    /// LSP diagnostic store for displaying code diagnostics
    pub diagnostic_store: crate::lsp_bridge::DiagnosticStore,
    /// Whether focus mode is active (header/statusbar hidden)
    pub focus_mode: bool,
    /// Tool output verbosity level
    pub view_mode: ViewMode,
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
    /// Whether to auto-scroll to bottom during streaming
    pub auto_follow: bool,
    /// Message count at the moment auto_follow was disabled (for "new" counter)
    pub messages_at_scroll_pause: usize,
    /// Turn counter for context visualization
    pub turn_count: usize,
    /// Recent background task notifications (message + timestamp)
    pub pending_notifications: Vec<(String, std::time::Instant)>,
    /// Whether the first-run onboarding dialog is active
    pub onboarding_active: bool,
    /// Whether mouse capture is enabled (F8 toggles; when off, terminal handles text selection)
    pub mouse_capture_enabled: bool,
    /// Tool approval overlay widget (shows when tool needs confirmation)
    pub tool_approval: ToolApprovalWidget,
    /// Attachment bar widget (shows attached files/images above prompt)
    pub attachment_bar: AttachmentBarWidget,
    /// Command palette overlay widget (Ctrl+P)
    pub command_palette: Option<CommandPaletteWidget>,
    /// Agents panel overlay visible (Ctrl+A to toggle)
    pub agents_panel_visible: bool,
    /// Session tab bar widget (top of terminal, Ctrl+T to toggle)
    pub session_tab: SessionTabWidget,
    /// Detailed streaming state for status indicator
    pub streaming_state: StreamingState,
    /// Loop engine state for autonomous iteration
    pub loop_state: Option<LoopState>,
    /// Ralph Wiggum completion-based loop state
    pub ralph_state: Option<RalphState>,
    /// Billing manager for per-model cost tracking and budget alerts
    pub billing_manager: shannon_core::billing::BillingManager,
    /// Additional working directories added via /add-dir
    pub extra_dirs: Vec<String>,
    /// Custom session title (set via /rename)
    pub session_title: Option<String>,
    /// Current git branch name (refreshed periodically)
    pub git_branch: Option<String>,
    /// Thinking effort level for the model (set via /effort)
    pub effort_level: Option<String>,
    /// Context focus area to limit model attention (set via /focus)
    pub focus_area: Option<String>,
    /// Whether LLM tool calling is enabled (false = no tools sent to model)
    pub tools_enabled: bool,
    /// Custom statusline command (shell script receiving JSON via stdin)
    pub statusline_command: Option<String>,
    /// Cached output from the last statusline command run
    pub cached_statusline: Option<String>,
    /// When the statusline was last refreshed
    pub statusline_last_update: Option<std::time::Instant>,
    /// Rate limit info from API response headers
    pub rate_limit_5h: Option<(u32, u32)>, // (used, total)
    pub rate_limit_7d: Option<(u32, u32)>, // (used, total)
    /// Token output rate during streaming (tokens/sec)
    pub streaming_token_rate: f64,
    /// When the first output token arrived during streaming (for rate calculation)
    pub streaming_output_start: Option<std::time::Instant>,
    /// Previous output token count (for instantaneous rate calculation)
    pub prev_output_tokens: u64,
    /// Desktop notification sent for current query
    pub desktop_notified: bool,
    /// Auto-compact deferred until after streaming completes
    pub pending_auto_compact: bool,
    /// Persisted UI state for session restore (fold states, scroll, view mode)
    pub persisted_ui_state: Option<PersistedUiState>,
    /// Pending undo preview (set by /undo before confirmation dialog)
    pub undo_preview: Option<shannon_core::RevertPreview>,
    /// Target checkpoint index for pending undo operation
    pub undo_target_index: Option<usize>,
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

/// State for the Ralph Wiggum completion-based loop.
///
/// Unlike [`LoopState`] which iterates on a timer, Ralph re-injects the
/// task prompt every time the model stops without emitting one of the
/// configured completion keywords.
#[derive(Debug, Clone)]
pub struct RalphState {
    /// The task description
    pub task: String,
    /// Keywords that signal task completion (case-insensitive)
    pub completion_keywords: Vec<String>,
    /// Maximum iterations before forced stop
    pub max_iterations: usize,
    /// Current iteration count
    pub iteration: usize,
    /// Whether the loop is active
    pub active: bool,
}

/// Persisted UI state saved across sessions for restore.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PersistedUiState {
    /// Collapsed tool message indices
    pub collapsed_tools: bool,
    /// View mode (Default or Verbose)
    pub view_mode: String,
    /// Theme name
    pub theme_name: String,
    /// Scroll offset (message index)
    pub scroll_offset: usize,
    /// Focus mode
    pub focus_mode: bool,
    /// Fullscreen mode
    pub fullscreen_mode: bool,
}

impl Default for PersistedUiState {
    fn default() -> Self {
        Self {
            collapsed_tools: true,
            view_mode: "Default".to_string(),
            theme_name: "dark".to_string(),
            scroll_offset: 0,
            focus_mode: false,
            fullscreen_mode: false,
        }
    }
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
    /// Cycle to the next tab: Context -> Files -> Agents -> Perf -> Context
    pub fn next(self) -> Self {
        match self {
            SidebarTab::Context => SidebarTab::Files,
            SidebarTab::Files => SidebarTab::Agents,
            SidebarTab::Agents => SidebarTab::Perf,
            SidebarTab::Perf => SidebarTab::Context,
        }
    }

    /// Cycle to the previous tab: Context -> Perf -> Agents -> Files -> Context
    pub fn prev(self) -> Self {
        match self {
            SidebarTab::Context => SidebarTab::Perf,
            SidebarTab::Files => SidebarTab::Context,
            SidebarTab::Agents => SidebarTab::Files,
            SidebarTab::Perf => SidebarTab::Agents,
        }
    }
}

/// Display info for a single sub-agent in the sidebar.
///
/// The sidebar Agents tab shows all active sub-agents from `SubAgentRegistry`.
/// Each agent displays its name, status (spawning→running→idle/completed/failed),
/// team affiliation, and turn budget usage. Active agents are highlighted;
/// completed/failed agents are dimmed.
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
            model: None,
            selected_provider: None,
            context_window: 200_000,
            tokens_used: 0,
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            total_cost_usd: 0.0,
            working_directory: cwd,
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
            theme_picker: None,
            completion_suggestions: Vec::new(),
            routine_manager: shannon_core::scheduled_routines::RoutineManager::new(),
            cron_tool: if std::env::var("SHANNON_DISABLE_CRON").is_ok() {
                shannon_tools::CronTool::new()
            } else {
                shannon_tools::CronTool::with_persistence()
            },
            completion_suggestion_index: 0,
            plan: PlanState::default(),
            sandbox_mode: shannon_tools::SandboxMode::Direct,
            theme: Theme::detect(),
            accessibility_mode: std::env::var("NO_GRAPHICS").is_ok()
                || std::env::var("ACCESSIBILITY").is_ok(),
            reduced_motion: std::env::var("NO_COLOR").is_ok()
                || std::env::var("REDUCED_MOTION").is_ok()
                || std::env::var("NO_GRAPHICS").is_ok()
                || std::env::var("ACCESSIBILITY").is_ok(),
            keybindings: crate::keybindings::load_keybindings(),
            sidebar_visible: true,
            diff_viewer: None,
            interactive_hunks: Vec::new(),
            interactive_selected: 0,
            diff_interactive: false,
            show_key_hints: false,
            incremental_search_active: false,
            incremental_search_query: String::new(),
            incremental_search_match_index: 0,
            incremental_search_match_count: 0,
            incremental_search_saved_input: String::new(),
            pasted_texts: std::collections::HashMap::new(),
            paste_counter: 0,
            file_selector_for_at: false,
            toast: None,
            thinking_phase: false,
            streaming_active: false,
            queued_messages: Vec::new(),
            streaming_start: None,
            session_start: Some(std::time::Instant::now()),
            vim_mode: "INSERT".to_string(),
            leader_active: false,
            last_esc_time: None,
            session_picker_active: false,
            prompt_bar_color: None,
            sidebar_tab: SidebarTab::default(),
            approval_mode_label: "EDIT".to_string(),
            active_agents: Vec::new(),
            agent_dashboard: None,
            diagnostic_store: crate::lsp_bridge::DiagnosticStore::new(),
            focus_mode: false,
            view_mode: ViewMode::Default,
            fullscreen_mode: false,
            chat_search_active: false,
            chat_search_query: String::new(),
            chat_search_match_index: 0,
            chat_search_total_matches: 0,
            pager_active: false,
            pager_scroll: 0,
            auto_follow: true,
            messages_at_scroll_pause: 0,
            turn_count: 0,
            pending_notifications: Vec::new(),
            onboarding_active: false,
            mouse_capture_enabled: true,
            tool_approval: ToolApprovalWidget::new(),
            attachment_bar: AttachmentBarWidget::new(5),
            command_palette: None,
            agents_panel_visible: true,
            session_tab: SessionTabWidget::new(),
            streaming_state: StreamingState::Idle,
            loop_state: None,
            ralph_state: None,
            billing_manager: shannon_core::billing::BillingManager::new(),
            extra_dirs: Vec::new(),
            session_title: None,
            git_branch: None,
            effort_level: None,
            focus_area: None,
            tools_enabled: true,
            statusline_command: None,
            cached_statusline: None,
            statusline_last_update: None,
            rate_limit_5h: None,
            rate_limit_7d: None,
            streaming_token_rate: 0.0,
            streaming_output_start: None,
            prev_output_tokens: 0u64,
            desktop_notified: false,
            pending_auto_compact: false,
            persisted_ui_state: None,
            undo_preview: None,
            undo_target_index: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_mode_cycle_default_to_verbose() {
        assert_eq!(ViewMode::Default.cycle(), ViewMode::Verbose);
    }

    #[test]
    fn view_mode_cycle_verbose_to_default() {
        assert_eq!(ViewMode::Verbose.cycle(), ViewMode::Default);
    }

    #[test]
    fn view_mode_label() {
        assert_eq!(ViewMode::Default.label(), "Default");
        assert_eq!(ViewMode::Verbose.label(), "Verbose");
    }

    #[test]
    fn view_mode_cycle_roundtrip() {
        let mode = ViewMode::Default;
        assert_eq!(mode.cycle().cycle(), ViewMode::Default);
    }

    #[test]
    fn sidebar_tab_next_cycles() {
        assert_eq!(SidebarTab::Context.next(), SidebarTab::Files);
        assert_eq!(SidebarTab::Files.next(), SidebarTab::Agents);
        assert_eq!(SidebarTab::Agents.next(), SidebarTab::Perf);
        assert_eq!(SidebarTab::Perf.next(), SidebarTab::Context);
    }

    #[test]
    fn sidebar_tab_prev_cycles() {
        assert_eq!(SidebarTab::Context.prev(), SidebarTab::Perf);
        assert_eq!(SidebarTab::Perf.prev(), SidebarTab::Agents);
        assert_eq!(SidebarTab::Agents.prev(), SidebarTab::Files);
        assert_eq!(SidebarTab::Files.prev(), SidebarTab::Context);
    }

    #[test]
    fn sidebar_tab_next_prev_roundtrip() {
        assert_eq!(SidebarTab::Context.next().prev(), SidebarTab::Context);
        assert_eq!(SidebarTab::Files.next().prev(), SidebarTab::Files);
    }

    #[test]
    fn sidebar_tab_default_is_context() {
        assert_eq!(SidebarTab::default(), SidebarTab::Context);
    }

    #[test]
    fn plan_state_default() {
        let plan = PlanState::default();
        assert!(!plan.active);
        assert!(plan.content.is_empty());
        assert!(plan.description.is_empty());
        assert!(!plan.approved);
        assert_eq!(plan.scroll_offset, 0);
    }

    #[test]
    fn agent_display_fields() {
        let agent = AgentDisplay {
            name: "worker-1".to_string(),
            status: "running".to_string(),
            active: true,
            team: Some("team-a".to_string()),
            turns_used: 3,
            max_turns: 10,
        };
        assert_eq!(agent.name, "worker-1");
        assert!(agent.active);
        assert_eq!(agent.turns_used, 3);
    }

    #[test]
    fn persisted_ui_state_default() {
        let state = PersistedUiState::default();
        assert!(state.collapsed_tools);
        assert_eq!(state.view_mode, "Default");
        assert!(!state.focus_mode);
        assert!(!state.fullscreen_mode);
        assert_eq!(state.scroll_offset, 0);
    }

    #[test]
    fn loop_state_fields() {
        let ls = LoopState {
            task: "fix bugs".to_string(),
            max_iterations: 5,
            iteration: 2,
            active: true,
        };
        assert_eq!(ls.task, "fix bugs");
        assert!(ls.active);
        assert_eq!(ls.iteration, 2);
    }
}
