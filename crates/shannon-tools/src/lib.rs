//! Shannon Tools - Claude Code tool implementations
//!
//! This crate provides implementations of Claude Code tools including:
//! - File operations (Read, Write, Edit, Glob)
//! - System operations (Bash commands, Sleep)
//! - Web operations (WebFetch, WebSearch)
//! - Agent operations (Agent spawning, messaging)
//! - Task operations (Todo management, task lists)
//! - Notebook operations (NotebookEdit for Jupyter notebooks)
//! - Worktree operations (EnterWorktree, ExitWorktree for git worktrees)
//! - MCP operations (ReadMcpResource, ListMcpResources for MCP servers)
//! - Skill operations (Skill for user-callable skills)
//! - Cron operations (CronCreate, CronDelete, CronList for scheduling)
//! - Messaging operations (SendMessage for team communication)
//! - Plan mode operations (EnterPlanMode, ExitPlanMode for read-only planning)
//! - Git operations (GitBranch, GitDiff, GitLog, GitStash, GitSafety)
//! - LSP operations (GoToDefinition, FindReferences, Hover, DocumentSymbol)
//! - Grep operations (Grep for content search across files)
//! - Tool search operations (ToolSearch for tool discovery)
//! - Ask user operations (AskUserQuestion for interactive confirmation and option selection)
//! - Structured output operations (StructuredOutput for AI-returned JSON data)
//! - REPL operations (REPL for batch command execution)
//! - MCP auth operations (McpAuth for OAuth authentication with MCP servers)

use std::sync::Arc;

pub mod agent;
pub mod ask_user;
pub mod brief;
pub mod config;
pub mod cron;
pub mod file;
pub mod git;
pub mod github;
pub mod grep;
pub mod lsp;
pub mod lsp_diagnostics;
pub mod mcp;
pub mod mcp_auth;
pub mod mcp_tools;
pub mod messaging;
pub mod notebook;
pub mod plan_mode;
pub mod pty;
pub mod remote_trigger;
pub mod repl_tool;
pub mod schedule_wakeup;
pub mod skill;
pub mod synthetic_output;
pub mod system;
pub mod task;
pub mod task_output;
pub mod task_stop;
pub mod team_delete;
pub mod todo;
pub mod tool_search;
pub mod web;
pub mod worktree;

// Re-exports for convenience
pub use agent::{AgentOperation, AgentTool, AgentToolContext};
pub use ask_user::{
    AskUserError, AskUserInput, AskUserQuestionTool, ErrorQuestionHandler, MockQuestionHandler,
    Question, QuestionAnswer, QuestionHandler, QuestionOption, SharedQuestionHandler,
    TerminalQuestionHandler,
};
pub use brief::{BriefFormat, BriefInput, BriefMessage, BriefTool};
pub use config::{ConfigAction, ConfigInput, ConfigManager, ConfigTool, SharedConfigManager};
pub use cron::{
    CronCreateInput, CronCreateOutput, CronDeleteInput, CronDeleteOutput, CronListInput,
    CronListOutput, CronTool,
};
pub use file::diff_renderer::{
    ColorScheme, DiffHunk as DiffRenderHunk, DiffLine, DiffLineType, DiffRenderer, DiffStats,
};
pub use file::history::{
    DiffHunk, FileDiff, FileHistory, FileHistoryConfig, FileHistoryError, FileHistoryManager,
    FileOperation as FileHistoryOperation, FileSnapshot,
};
pub use file::sandbox::{PathSandbox, SandboxConfig as PathSandboxConfig, SandboxError};
pub use file::sandbox_adapter::{
    PathSandboxAdapter, SandboxAdapter, SandboxConfig as SandboxAdapterConfig, SandboxResult,
    SandboxViolation,
};
pub use file::{EditTool, FileOperation, GlobTool, MultiEditTool, ReadTool, WriteTool};
pub use git::{
    AutoCommitTool, GitBranchTool, GitDiffTool, GitLogTool, GitSafetyTool, GitStashTool,
};
pub use github::{GhIssueListTool, GhIssueViewTool, GhPrCreateTool, GhPrListTool, GhPrViewTool};
pub use grep::GrepTool;
pub use lsp::{
    CodeActionItem, CodeActionsInput, CodeActionsOutput, CodeActionsTool, DocumentSymbolInput,
    DocumentSymbolItem, DocumentSymbolOutput, DocumentSymbolTool, FindReferencesInput,
    FindReferencesOutput, FindReferencesTool, GoToDefinitionInput, GoToDefinitionOutput,
    GoToDefinitionTool, HoverInput, HoverOutput, HoverResult, HoverTool, LspLocation, LspPosition,
    LspRange, RenameSymbolInput, RenameSymbolOutput, RenameSymbolTool, WorkspaceSymbolInput,
    WorkspaceSymbolItem, WorkspaceSymbolOutput, WorkspaceSymbolTool, detect_language_id,
};
pub use lsp_diagnostics::{
    CliDiagnosticResult, DiagnosticRegistry, DiagnosticSeverity, DiagnosticSummary, LspDiagnostic,
    RelatedInfo, run_cli_diagnostics,
};
pub use mcp::{
    ListMcpResourcesInput, ListMcpResourcesOutput, McpResourceTool, ReadMcpResourceInput,
    ReadMcpResourceOutput,
};
pub use mcp_auth::{McpAuthAction, McpAuthTool, McpOAuthConfig, OAuthToken, OAuthTokenStore};
pub use mcp_tools::{
    GetPromptTool, ListMcpResourcesTool, ListPromptsTool, McpToolSearchTool, ReadMcpResourceTool,
};
pub use messaging::{SendMessageInput, SendMessageOutput, SendMessageTool};
pub use notebook::{NotebookEditInput, NotebookEditOutput, NotebookEditTool};
pub use plan_mode::{
    EnterPlanModeTool, ExitPlanModeTool, GetPlanStatusTool, PlanEntry, PlanManager, PlanModeState,
    is_plan_mode_active, new_plan_mode_state,
};
pub use remote_trigger::{
    RemoteTriggerInput, RemoteTriggerServer, RemoteTriggerTool, TriggerAction,
};
pub use repl_tool::{REPL_TOOL_NAME, ReplInput, ReplOutput, ReplTool};
pub use schedule_wakeup::{
    AUTONOMOUS_LOOP_SENTINEL, ScheduleWakeupInput, ScheduleWakeupTool, WakeupRequest,
};
pub use skill::{SkillInvokeInput, SkillInvokeOutput, SkillTool};
pub use synthetic_output::{
    STRUCTURED_OUTPUT_TOOL_NAME, StructuredOutputInput, StructuredOutputOutput,
    StructuredOutputTool,
};
pub use system::{
    BashTool, DockerSandbox, DockerSandboxConfig, PathValidationError, PowerShellTool, SandboxMode,
    ShellCommand, SleepTool, SystemTool,
};
pub use system::{CommandOutput, SecurityAnalysis, SecurityLevel, analyze_command_security};
pub use task::{TaskOperation, TaskTool};
pub use task_output::{TaskOutputInput, TaskOutputOutput, TaskOutputTool};
pub use task_stop::{TaskStopInput, TaskStopOutput, TaskStopTool};
pub use team_delete::{TeamDeleteInput, TeamDeleteOutput, TeamDeleteTool, TeamEntry, TeamRegistry};
pub use todo::{
    TaskCreateInput, TaskCreateOutput, TaskCreateTool, TaskGetInput, TaskGetOutput, TaskGetTool,
    TaskListInput, TaskListOutput, TaskListTool, TaskStore, TaskUpdateInput, TaskUpdateOutput,
    TaskUpdateTool, TodoItem, TodoStatus, TodoWriteInput, TodoWriteOutput, TodoWriteTool,
};
pub use tool_search::{ToolSearchInput, ToolSearchOutput, ToolSearchTool};
pub use web::{WebFetchTool, WebOperation, WebSearchTool};
pub use worktree::{
    EnterWorktreeInput, EnterWorktreeOutput, ExitWorktreeInput, ExitWorktreeOutput, WorktreeTool,
};

// Re-export from shannon_core
pub use shannon_core::tools::{
    BoxedProgressSender, ProgressSender, Tool, ToolError, ToolOutput, ToolRegistry, ToolResult,
};

/// Register all standard tools into the given registry.
///
/// Some tools (plan mode) require shared state and are registered with sensible
/// defaults. Callers can override by re-registering with custom instances after this call.
///
/// Returns the AgentTool's context handle for late injection of LLM client config.
pub fn register_default_tools(
    registry: &mut ToolRegistry,
) -> Result<std::sync::Arc<std::sync::Mutex<Option<AgentToolContext>>>, Box<dyn std::error::Error>>
{
    // ── File operations ────────────────────────────────────────────────
    registry.register(Box::new(ReadTool::new()))?;
    registry.register(Box::new(WriteTool::new()))?;
    registry.register(Box::new(EditTool::new()))?;
    registry.register(Box::new(MultiEditTool::new()))?;
    registry.register(Box::new(GlobTool::new()))?;

    // ── System operations ──────────────────────────────────────────────
    registry.register(Box::new(BashTool::new()))?;
    registry.register(Box::new(SleepTool::new()))?;
    registry.register(Box::new(PowerShellTool::new()))?;
    registry.register(Box::new(ReplTool::new()))?;

    // ── Git operations ─────────────────────────────────────────────────
    registry.register(Box::new(GitBranchTool::new()))?;
    registry.register(Box::new(GitDiffTool::new()))?;
    registry.register(Box::new(GitLogTool::new()))?;
    registry.register(Box::new(GitStashTool::new()))?;
    registry.register(Box::new(GitSafetyTool::new()))?;
    registry.register(Box::new(AutoCommitTool::new()))?;

    // ── GitHub operations ───────────────────────────────────────────────
    registry.register(Box::new(GhIssueListTool::new()))?;
    registry.register(Box::new(GhIssueViewTool::new()))?;
    registry.register(Box::new(GhPrCreateTool::new()))?;
    registry.register(Box::new(GhPrListTool::new()))?;
    registry.register(Box::new(GhPrViewTool::new()))?;

    // ── Web operations ─────────────────────────────────────────────────
    registry.register(Box::new(WebFetchTool::new()))?;
    registry.register(Box::new(WebSearchTool::new()))?;

    // ── Search ─────────────────────────────────────────────────────────
    registry.register(Box::new(GrepTool::new()))?;

    // ── Agent & team ───────────────────────────────────────────────────
    let agent_tool = AgentTool::new();
    let agent_context_handle = agent_tool.context_handle();
    registry.register(Box::new(agent_tool))?;
    registry.register(Box::new(SendMessageTool::new()))?;
    registry.register(Box::new(TeamDeleteTool::new()))?;

    // ── Task management ────────────────────────────────────────────────
    registry.register(Box::new(TodoWriteTool::new()))?;
    registry.register(Box::new(TaskCreateTool::new()))?;
    registry.register(Box::new(TaskListTool::new()))?;
    registry.register(Box::new(TaskUpdateTool::new()))?;
    registry.register(Box::new(TaskGetTool::new()))?;
    registry.register(Box::new(TaskTool::new()))?;
    registry.register(Box::new(TaskOutputTool::new()))?;
    registry.register(Box::new(TaskStopTool::new()))?;

    // ── Notebook ───────────────────────────────────────────────────────
    registry.register(Box::new(NotebookEditTool::new()))?;

    // ── Worktree ───────────────────────────────────────────────────────
    registry.register(Box::new(WorktreeTool::new()))?;

    // ── Plan mode (shared state + PlanManager) ──────────────────────────
    let plan_manager = PlanManager::new();
    registry.register(Box::new(EnterPlanModeTool::with_manager(
        plan_manager.clone(),
    )))?;
    registry.register(Box::new(ExitPlanModeTool::with_manager(
        plan_manager.clone(),
    )))?;
    registry.register(Box::new(GetPlanStatusTool::new(plan_manager)))?;

    // ── LSP ────────────────────────────────────────────────────────────
    registry.register(Box::new(GoToDefinitionTool::new()))?;
    registry.register(Box::new(FindReferencesTool::new()))?;
    registry.register(Box::new(HoverTool::new()))?;
    registry.register(Box::new(DocumentSymbolTool::new()))?;
    registry.register(Box::new(WorkspaceSymbolTool::new()))?;
    registry.register(Box::new(RenameSymbolTool::new()))?;
    registry.register(Box::new(CodeActionsTool::new()))?;

    // ── Interactive ────────────────────────────────────────────────────
    registry.register(Box::new(AskUserQuestionTool::with_terminal_handler()))?;

    // ── Skill & discovery ──────────────────────────────────────────────
    registry.register(Box::new(SkillTool::new()))?;
    // Note: ToolSearchTool requires Arc<RwLock<ToolRegistry>> — register separately if needed

    // ── Cron ───────────────────────────────────────────────────────────
    registry.register(Box::new(CronTool::with_persistence()))?;

    // ── ScheduleWakeup (/loop dynamic pacing) ──────────────────────────
    registry.register(Box::new(ScheduleWakeupTool::new()))?;

    // ── Config ─────────────────────────────────────────────────────────
    registry.register(Box::new(ConfigTool::new()))?;

    // ── Utility tools ──────────────────────────────────────────────────
    registry.register(Box::new(BriefTool::new()))?;
    registry.register(Box::new(StructuredOutputTool::new()))?;
    registry.register(Box::new(McpAuthTool::new()))?;

    // ── MCP resource tools ─────────────────────────────────────────────
    registry.register(Box::new(McpResourceTool::new()))?;
    let mcp_manager = Arc::new(shannon_mcp::McpResourceManager::new());
    registry.register(Box::new(ListMcpResourcesTool::new(mcp_manager.clone())))?;
    registry.register(Box::new(ReadMcpResourceTool::new(mcp_manager)))?;

    // ── MCP prompt tools (register with an empty pool; re-register with a live pool) ──
    let mcp_pool = Arc::new(shannon_mcp::McpProcessPool::new());
    registry.register(Box::new(ListPromptsTool::new(mcp_pool.clone())))?;
    registry.register(Box::new(GetPromptTool::new(mcp_pool)))?;

    Ok(agent_context_handle)
}

/// Register all standard tools with project-specific sandbox configuration.
///
/// This is the preferred entry point over [`register_default_tools`] because it:
/// - Constrains file tools (Read/Write/Edit/Glob/Grep) to the project directory
/// - Enables platform process sandboxing (bwrap/Seatbelt) for BashTool
///
/// Returns the AgentTool's context handle for late injection of LLM client config.
pub fn register_default_tools_with_project_dir(
    registry: &mut ToolRegistry,
    project_dir: &std::path::Path,
) -> Result<Arc<std::sync::Mutex<Option<AgentToolContext>>>, Box<dyn std::error::Error>> {
    use crate::file::sandbox::{PathSandbox, SandboxConfig as PathSandboxConfig};

    let sandbox = PathSandbox::with_config(PathSandboxConfig {
        allowed_roots: vec![project_dir.to_path_buf()],
        denied_patterns: PathSandboxConfig::default_denied_patterns(),
        strict_mode: true,
    });

    // ── File operations (project-scoped sandbox) ───────────────────────
    registry.register(Box::new(ReadTool::with_sandbox(sandbox.clone())))?;
    registry.register(Box::new(WriteTool::with_sandbox(sandbox.clone())))?;
    registry.register(Box::new(EditTool::with_sandbox(sandbox.clone())))?;
    registry.register(Box::new(MultiEditTool::with_sandbox(sandbox.clone())))?;
    registry.register(Box::new(GlobTool::with_sandbox(sandbox.clone())))?;
    registry.register(Box::new(GrepTool::with_sandbox(sandbox)))?;

    // ── System operations (process sandbox for Bash) ───────────────────
    registry.register(Box::new(BashTool::with_process_sandbox(project_dir)))?;
    registry.register(Box::new(SleepTool::new()))?;
    registry.register(Box::new(PowerShellTool::new()))?;
    registry.register(Box::new(ReplTool::new()))?;

    // ── Git operations ─────────────────────────────────────────────────
    registry.register(Box::new(GitBranchTool::new()))?;
    registry.register(Box::new(GitDiffTool::new()))?;
    registry.register(Box::new(GitLogTool::new()))?;
    registry.register(Box::new(GitStashTool::new()))?;
    registry.register(Box::new(GitSafetyTool::new()))?;
    registry.register(Box::new(AutoCommitTool::new()))?;

    // ── GitHub operations ───────────────────────────────────────────────
    registry.register(Box::new(GhIssueListTool::new()))?;
    registry.register(Box::new(GhIssueViewTool::new()))?;
    registry.register(Box::new(GhPrCreateTool::new()))?;
    registry.register(Box::new(GhPrListTool::new()))?;
    registry.register(Box::new(GhPrViewTool::new()))?;

    // ── Web operations ─────────────────────────────────────────────────
    registry.register(Box::new(WebFetchTool::new()))?;
    registry.register(Box::new(WebSearchTool::new()))?;

    // ── Agent & team ───────────────────────────────────────────────────
    let agent_tool = AgentTool::new();
    let agent_context_handle = agent_tool.context_handle();
    registry.register(Box::new(agent_tool))?;
    registry.register(Box::new(SendMessageTool::new()))?;
    registry.register(Box::new(TeamDeleteTool::new()))?;

    // ── Task management ────────────────────────────────────────────────
    registry.register(Box::new(TodoWriteTool::new()))?;
    registry.register(Box::new(TaskCreateTool::new()))?;
    registry.register(Box::new(TaskListTool::new()))?;
    registry.register(Box::new(TaskUpdateTool::new()))?;
    registry.register(Box::new(TaskGetTool::new()))?;
    registry.register(Box::new(TaskTool::new()))?;
    registry.register(Box::new(TaskOutputTool::new()))?;
    registry.register(Box::new(TaskStopTool::new()))?;

    // ── Notebook ───────────────────────────────────────────────────────
    registry.register(Box::new(NotebookEditTool::new()))?;

    // ── Worktree ───────────────────────────────────────────────────────
    registry.register(Box::new(WorktreeTool::new()))?;

    // ── Plan mode (shared state + PlanManager) ──────────────────────────
    let plan_manager = PlanManager::new();
    registry.register(Box::new(EnterPlanModeTool::with_manager(
        plan_manager.clone(),
    )))?;
    registry.register(Box::new(ExitPlanModeTool::with_manager(
        plan_manager.clone(),
    )))?;
    registry.register(Box::new(GetPlanStatusTool::new(plan_manager)))?;

    // ── LSP ────────────────────────────────────────────────────────────
    registry.register(Box::new(GoToDefinitionTool::new()))?;
    registry.register(Box::new(FindReferencesTool::new()))?;
    registry.register(Box::new(HoverTool::new()))?;
    registry.register(Box::new(DocumentSymbolTool::new()))?;
    registry.register(Box::new(WorkspaceSymbolTool::new()))?;
    registry.register(Box::new(RenameSymbolTool::new()))?;
    registry.register(Box::new(CodeActionsTool::new()))?;

    // ── Interactive ────────────────────────────────────────────────────
    registry.register(Box::new(AskUserQuestionTool::with_terminal_handler()))?;

    // ── Skill & discovery ──────────────────────────────────────────────
    registry.register(Box::new(SkillTool::new()))?;

    // ── Cron ───────────────────────────────────────────────────────────
    registry.register(Box::new(CronTool::with_persistence()))?;

    // ── ScheduleWakeup (/loop dynamic pacing) ──────────────────────────
    registry.register(Box::new(ScheduleWakeupTool::new()))?;

    // ── Config ─────────────────────────────────────────────────────────
    registry.register(Box::new(ConfigTool::new()))?;

    // ── Utility tools ──────────────────────────────────────────────────
    registry.register(Box::new(BriefTool::new()))?;
    registry.register(Box::new(StructuredOutputTool::new()))?;
    registry.register(Box::new(McpAuthTool::new()))?;

    // ── MCP resource tools ─────────────────────────────────────────────
    registry.register(Box::new(McpResourceTool::new()))?;
    let mcp_manager = Arc::new(shannon_mcp::McpResourceManager::new());
    registry.register(Box::new(ListMcpResourcesTool::new(mcp_manager.clone())))?;
    registry.register(Box::new(ReadMcpResourceTool::new(mcp_manager)))?;

    // ── MCP prompt tools ───────────────────────────────────────────────
    let mcp_pool = Arc::new(shannon_mcp::McpProcessPool::new());
    registry.register(Box::new(ListPromptsTool::new(mcp_pool.clone())))?;
    registry.register(Box::new(GetPromptTool::new(mcp_pool)))?;

    Ok(agent_context_handle)
}

/// Result of registering tools with project-specific sandbox configuration.
///
/// Contains handles that callers need to wire up cross-cutting features.
pub struct ToolRegistrationResult {
    /// Handle for injecting LLM client config into the AgentTool.
    pub agent_context_handle: std::sync::Arc<std::sync::Mutex<Option<AgentToolContext>>>,
    /// The `PlanManager` shared by `EnterPlanMode`/`ExitPlanMode`/`GetPlanStatus`.
    /// Use `plan_manager.plan_mode_flag()` to obtain the flag for the query engine.
    pub plan_manager: PlanManager,
}

/// Register all standard tools with project-specific sandbox configuration.
///
/// This is the extended variant of [`register_default_tools_with_project_dir`] that
/// also returns the [`PlanManager`] so callers can wire up plan-mode write-blocking
/// in the query engine via [`PlanManager::plan_mode_flag`].
///
/// ```ignore
/// let result = register_default_tools_with_project_dir_ex(&mut registry, &project_dir)?;
/// let engine = QueryEngine::with_defaults_arc(client, Arc::new(registry), perms, state)
///     .with_plan_mode_active(result.plan_manager.plan_mode_flag());
/// ```
pub fn register_default_tools_with_project_dir_ex(
    registry: &mut ToolRegistry,
    project_dir: &std::path::Path,
) -> Result<ToolRegistrationResult, Box<dyn std::error::Error>> {
    use crate::file::sandbox::{PathSandbox, SandboxConfig as PathSandboxConfig};

    let sandbox = PathSandbox::with_config(PathSandboxConfig {
        allowed_roots: vec![project_dir.to_path_buf()],
        denied_patterns: PathSandboxConfig::default_denied_patterns(),
        strict_mode: true,
    });

    // ── File operations (project-scoped sandbox) ───────────────────────
    registry.register(Box::new(ReadTool::with_sandbox(sandbox.clone())))?;
    registry.register(Box::new(WriteTool::with_sandbox(sandbox.clone())))?;
    registry.register(Box::new(EditTool::with_sandbox(sandbox.clone())))?;
    registry.register(Box::new(MultiEditTool::with_sandbox(sandbox.clone())))?;
    registry.register(Box::new(GlobTool::with_sandbox(sandbox.clone())))?;
    registry.register(Box::new(GrepTool::with_sandbox(sandbox)))?;

    // ── System operations (process sandbox for Bash) ───────────────────
    registry.register(Box::new(BashTool::with_process_sandbox(project_dir)))?;
    registry.register(Box::new(SleepTool::new()))?;
    registry.register(Box::new(PowerShellTool::new()))?;
    registry.register(Box::new(ReplTool::new()))?;

    // ── Git operations ─────────────────────────────────────────────────
    registry.register(Box::new(GitBranchTool::new()))?;
    registry.register(Box::new(GitDiffTool::new()))?;
    registry.register(Box::new(GitLogTool::new()))?;
    registry.register(Box::new(GitStashTool::new()))?;
    registry.register(Box::new(GitSafetyTool::new()))?;
    registry.register(Box::new(AutoCommitTool::new()))?;

    // ── GitHub operations ───────────────────────────────────────────────
    registry.register(Box::new(GhIssueListTool::new()))?;
    registry.register(Box::new(GhIssueViewTool::new()))?;
    registry.register(Box::new(GhPrCreateTool::new()))?;
    registry.register(Box::new(GhPrListTool::new()))?;
    registry.register(Box::new(GhPrViewTool::new()))?;

    // ── Web operations ─────────────────────────────────────────────────
    registry.register(Box::new(WebFetchTool::new()))?;
    registry.register(Box::new(WebSearchTool::new()))?;

    // ── Agent & team ───────────────────────────────────────────────────
    let agent_tool = AgentTool::new();
    let agent_context_handle = agent_tool.context_handle();
    registry.register(Box::new(agent_tool))?;
    registry.register(Box::new(SendMessageTool::new()))?;
    registry.register(Box::new(TeamDeleteTool::new()))?;

    // ── Task management ────────────────────────────────────────────────
    registry.register(Box::new(TodoWriteTool::new()))?;
    registry.register(Box::new(TaskCreateTool::new()))?;
    registry.register(Box::new(TaskListTool::new()))?;
    registry.register(Box::new(TaskUpdateTool::new()))?;
    registry.register(Box::new(TaskGetTool::new()))?;
    registry.register(Box::new(TaskTool::new()))?;
    registry.register(Box::new(TaskOutputTool::new()))?;
    registry.register(Box::new(TaskStopTool::new()))?;

    // ── Notebook ───────────────────────────────────────────────────────
    registry.register(Box::new(NotebookEditTool::new()))?;

    // ── Worktree ───────────────────────────────────────────────────────
    registry.register(Box::new(WorktreeTool::new()))?;

    // ── Plan mode (shared state + PlanManager) ──────────────────────────
    let plan_manager = PlanManager::new();
    registry.register(Box::new(EnterPlanModeTool::with_manager(
        plan_manager.clone(),
    )))?;
    registry.register(Box::new(ExitPlanModeTool::with_manager(
        plan_manager.clone(),
    )))?;
    registry.register(Box::new(GetPlanStatusTool::new(plan_manager.clone())))?;

    // ── LSP ────────────────────────────────────────────────────────────
    registry.register(Box::new(GoToDefinitionTool::new()))?;
    registry.register(Box::new(FindReferencesTool::new()))?;
    registry.register(Box::new(HoverTool::new()))?;
    registry.register(Box::new(DocumentSymbolTool::new()))?;
    registry.register(Box::new(WorkspaceSymbolTool::new()))?;
    registry.register(Box::new(RenameSymbolTool::new()))?;
    registry.register(Box::new(CodeActionsTool::new()))?;

    // ── Interactive ────────────────────────────────────────────────────
    registry.register(Box::new(AskUserQuestionTool::with_terminal_handler()))?;

    // ── Skill & discovery ──────────────────────────────────────────────
    registry.register(Box::new(SkillTool::new()))?;

    // ── Cron ───────────────────────────────────────────────────────────
    registry.register(Box::new(CronTool::with_persistence()))?;

    // ── ScheduleWakeup (/loop dynamic pacing) ──────────────────────────
    registry.register(Box::new(ScheduleWakeupTool::new()))?;

    // ── Config ─────────────────────────────────────────────────────────
    registry.register(Box::new(ConfigTool::new()))?;

    // ── Utility tools ──────────────────────────────────────────────────
    registry.register(Box::new(BriefTool::new()))?;
    registry.register(Box::new(StructuredOutputTool::new()))?;
    registry.register(Box::new(McpAuthTool::new()))?;

    // ── MCP resource tools ─────────────────────────────────────────────
    registry.register(Box::new(McpResourceTool::new()))?;
    let mcp_manager = Arc::new(shannon_mcp::McpResourceManager::new());
    registry.register(Box::new(ListMcpResourcesTool::new(mcp_manager.clone())))?;
    registry.register(Box::new(ReadMcpResourceTool::new(mcp_manager)))?;

    // ── MCP prompt tools ───────────────────────────────────────────────
    let mcp_pool = Arc::new(shannon_mcp::McpProcessPool::new());
    registry.register(Box::new(ListPromptsTool::new(mcp_pool.clone())))?;
    registry.register(Box::new(GetPromptTool::new(mcp_pool)))?;

    Ok(ToolRegistrationResult {
        agent_context_handle,
        plan_manager,
    })
}

/// Register team coordination tools that require an AgentCoordinator.
///
/// Call this after `register_default_tools` when a team context is available.
/// These tools let the LLM manage the shared team TaskBoard for multi-agent coordination.
pub fn register_team_tools(
    registry: &mut ToolRegistry,
    coordinator: Arc<shannon_agents::AgentCoordinator>,
) -> Result<(), Box<dyn std::error::Error>> {
    registry.register(Box::new(shannon_agents::TeamTaskCreateTool::new(
        coordinator.clone(),
    )))?;
    registry.register(Box::new(shannon_agents::TeamTaskUpdateTool::new(
        coordinator.clone(),
    )))?;
    registry.register(Box::new(shannon_agents::TeamTaskListTool::new(coordinator)))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use shannon_core::tools::ToolRegistry;

    #[test]
    fn register_default_tools_succeeds() {
        let mut registry = ToolRegistry::new();
        let result = register_default_tools(&mut registry);
        assert!(result.is_ok(), "register_default_tools should succeed");
    }

    #[test]
    fn register_default_tools_returns_agent_context() {
        let mut registry = ToolRegistry::new();
        let handle = register_default_tools(&mut registry).unwrap();
        let ctx = handle.lock().unwrap();
        assert!(ctx.is_none(), "Agent context should start as None");
    }

    #[test]
    fn register_default_tools_registers_core_tools() {
        let mut registry = ToolRegistry::new();
        register_default_tools(&mut registry).unwrap();

        let names: Vec<String> = registry
            .list_tools_info()
            .iter()
            .map(|t| t.name.clone())
            .collect();
        assert!(
            names.contains(&"Read".to_string()),
            "Read tool should be registered"
        );
        assert!(
            names.contains(&"Write".to_string()),
            "Write tool should be registered"
        );
        assert!(
            names.contains(&"Edit".to_string()),
            "Edit tool should be registered"
        );
        assert!(
            names.contains(&"Bash".to_string()),
            "Bash tool should be registered"
        );
        assert!(
            names.contains(&"Glob".to_string()),
            "Glob tool should be registered"
        );
    }

    #[test]
    fn register_default_tools_registers_lsp_tools() {
        let mut registry = ToolRegistry::new();
        register_default_tools(&mut registry).unwrap();

        let names: Vec<String> = registry
            .list_tools_info()
            .iter()
            .map(|t| t.name.clone())
            .collect();
        assert!(names.contains(&"go_to_definition".to_string()));
        assert!(names.contains(&"find_references".to_string()));
        assert!(names.contains(&"hover".to_string()));
        assert!(names.contains(&"document_symbol".to_string()));
        assert!(names.contains(&"workspace_symbol".to_string()));
        assert!(names.contains(&"rename_symbol".to_string()));
        assert!(names.contains(&"code_actions".to_string()));
    }

    #[test]
    fn register_default_tools_registers_task_tools() {
        let mut registry = ToolRegistry::new();
        register_default_tools(&mut registry).unwrap();

        let names: Vec<String> = registry
            .list_tools_info()
            .iter()
            .map(|t| t.name.clone())
            .collect();
        assert!(names.contains(&"TodoWrite".to_string()));
        assert!(names.contains(&"TaskCreate".to_string()));
        assert!(names.contains(&"TaskList".to_string()));
        assert!(names.contains(&"TaskUpdate".to_string()));
        assert!(names.contains(&"TaskGet".to_string()));
    }

    #[test]
    fn register_tools_no_duplicates() {
        let mut registry = ToolRegistry::new();
        register_default_tools(&mut registry).unwrap();

        let names: Vec<String> = registry
            .list_tools_info()
            .iter()
            .map(|t| t.name.clone())
            .collect();
        let mut seen = std::collections::HashSet::new();
        for name in &names {
            assert!(seen.insert(name.clone()), "Duplicate tool name: {name}");
        }
    }

    #[test]
    fn register_default_tools_tool_count() {
        let mut registry = ToolRegistry::new();
        register_default_tools(&mut registry).unwrap();

        let tools = registry.list_tools_info();
        // Should have a substantial number of tools registered
        assert!(tools.len() > 30, "Expected >30 tools, got {}", tools.len());
    }
}
