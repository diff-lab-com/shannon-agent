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

pub mod file;
pub mod system;
pub mod git;
pub mod github;
pub mod web;
pub mod agent;
pub mod task;
pub mod notebook;
pub mod worktree;
pub mod mcp;
pub mod mcp_tools;
pub mod messaging;
pub mod todo;
pub mod skill;
pub mod cron;
pub mod plan_mode;
pub mod lsp;
pub mod lsp_diagnostics;
pub mod grep;
pub mod ask_user;
pub mod tool_search;
pub mod brief;
pub mod config;
pub mod remote_trigger;
pub mod task_output;
pub mod task_stop;
pub mod team_delete;
pub mod synthetic_output;
pub mod repl_tool;
pub mod mcp_auth;
pub mod schedule_wakeup;
pub mod pty;

// Re-exports for convenience
pub use file::{ReadTool, WriteTool, EditTool, GlobTool, FileOperation};
pub use system::{SystemTool, ShellCommand, SleepTool, BashTool, PowerShellTool, DockerSandbox, DockerSandboxConfig, SandboxMode};
pub use git::{GitBranchTool, GitDiffTool, GitLogTool, GitStashTool, GitSafetyTool, AutoCommitTool};
pub use github::{GhIssueListTool, GhIssueViewTool, GhPrCreateTool, GhPrListTool, GhPrViewTool};
pub use web::{WebFetchTool, WebSearchTool, WebOperation};
pub use agent::{AgentTool, AgentOperation, AgentToolContext};
pub use task::{TaskTool, TaskOperation};
pub use notebook::{NotebookEditTool, NotebookEditInput, NotebookEditOutput};
pub use worktree::{WorktreeTool, EnterWorktreeInput, EnterWorktreeOutput, ExitWorktreeInput, ExitWorktreeOutput};
pub use mcp::{McpResourceTool, ReadMcpResourceInput, ReadMcpResourceOutput, ListMcpResourcesInput, ListMcpResourcesOutput};
pub use mcp_tools::{ListMcpResourcesTool, ReadMcpResourceTool, ListPromptsTool, GetPromptTool, McpToolSearchTool};
pub use messaging::{SendMessageTool, SendMessageInput, SendMessageOutput};
pub use todo::{
    TodoWriteTool, TodoWriteInput, TodoWriteOutput,
    TaskCreateTool, TaskCreateInput, TaskCreateOutput,
    TaskListTool, TaskListInput, TaskListOutput,
    TaskUpdateTool, TaskUpdateInput, TaskUpdateOutput,
    TaskGetTool, TaskGetInput, TaskGetOutput,
    TaskStore, TodoItem, TodoStatus,
};
pub use skill::{SkillTool, SkillInvokeInput, SkillInvokeOutput};
pub use cron::{CronTool, CronCreateInput, CronCreateOutput, CronDeleteInput, CronDeleteOutput, CronListInput, CronListOutput};
pub use schedule_wakeup::{ScheduleWakeupTool, WakeupRequest, ScheduleWakeupInput, AUTONOMOUS_LOOP_SENTINEL};
pub use plan_mode::{
    PlanModeState, EnterPlanModeTool, ExitPlanModeTool, GetPlanStatusTool,
    PlanManager, PlanEntry,
    new_plan_mode_state, is_plan_mode_active,
};
pub use lsp::{
    GoToDefinitionTool, FindReferencesTool, HoverTool, DocumentSymbolTool,
    WorkspaceSymbolTool, RenameSymbolTool, CodeActionsTool,
    LspPosition, LspRange, LspLocation, HoverResult, DocumentSymbolItem,
    GoToDefinitionInput, FindReferencesInput, HoverInput, DocumentSymbolInput,
    GoToDefinitionOutput, FindReferencesOutput, HoverOutput, DocumentSymbolOutput,
    WorkspaceSymbolInput, WorkspaceSymbolOutput, WorkspaceSymbolItem,
    RenameSymbolInput, RenameSymbolOutput,
    CodeActionsInput, CodeActionsOutput, CodeActionItem,
    detect_language_id,
};
pub use lsp_diagnostics::{
    LspDiagnostic, DiagnosticSeverity, RelatedInfo,
    DiagnosticRegistry, DiagnosticSummary,
};
pub use grep::GrepTool;
pub use ask_user::{
    AskUserQuestionTool, AskUserInput, Question, QuestionOption, QuestionAnswer,
    QuestionHandler, SharedQuestionHandler, TerminalQuestionHandler,
    MockQuestionHandler, ErrorQuestionHandler,
};
pub use tool_search::{ToolSearchTool, ToolSearchInput, ToolSearchOutput};
pub use brief::{BriefTool, BriefInput, BriefMessage, BriefFormat};
pub use config::{ConfigTool, ConfigInput, ConfigAction, ConfigManager, SharedConfigManager};
pub use remote_trigger::{
    RemoteTriggerTool, RemoteTriggerServer, RemoteTriggerInput, TriggerAction,
};
pub use task_output::{TaskOutputTool, TaskOutputInput, TaskOutputOutput};
pub use task_stop::{TaskStopTool, TaskStopInput, TaskStopOutput};
pub use team_delete::{TeamDeleteTool, TeamDeleteInput, TeamDeleteOutput, TeamEntry, TeamRegistry};
pub use synthetic_output::{StructuredOutputTool, StructuredOutputInput, StructuredOutputOutput, STRUCTURED_OUTPUT_TOOL_NAME};
pub use repl_tool::{ReplTool, ReplInput, ReplOutput, REPL_TOOL_NAME};
pub use mcp_auth::{
    McpAuthTool, McpAuthAction, McpOAuthConfig, OAuthToken, OAuthTokenStore,
};
pub use file::history::{
    FileHistoryManager, FileHistoryConfig, FileHistoryError,
    FileSnapshot, FileHistory, FileDiff, FileOperation as FileHistoryOperation,
    DiffHunk,
};
pub use file::diff_renderer::{
    DiffRenderer, DiffHunk as DiffRenderHunk, DiffLine, DiffLineType, DiffStats, ColorScheme,
};
pub use file::sandbox_adapter::{
    SandboxAdapter, PathSandboxAdapter, SandboxViolation, SandboxResult,
    SandboxConfig as SandboxAdapterConfig,
};
pub use file::sandbox::{PathSandbox, SandboxConfig as PathSandboxConfig, SandboxError};
pub use system::{SecurityLevel, SecurityAnalysis, CommandOutput, analyze_command_security};

// Re-export from shannon_core
pub use shannon_core::{
    tools::{Tool, ToolError, ToolResult, ToolOutput, ToolRegistry},
};

/// Register all standard tools into the given registry.
///
/// Some tools (plan mode) require shared state and are registered with sensible
/// defaults. Callers can override by re-registering with custom instances after this call.
///
/// Returns the AgentTool's context handle for late injection of LLM client config.
pub fn register_default_tools(registry: &mut ToolRegistry) -> Result<std::sync::Arc<std::sync::Mutex<Option<AgentToolContext>>>, Box<dyn std::error::Error>> {
    // ── File operations ────────────────────────────────────────────────
    registry.register(Box::new(ReadTool::new()))?;
    registry.register(Box::new(WriteTool::new()))?;
    registry.register(Box::new(EditTool::new()))?;
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
    registry.register(Box::new(EnterPlanModeTool::with_manager(plan_manager.clone())))?;
    registry.register(Box::new(ExitPlanModeTool::with_manager(plan_manager.clone())))?;
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
    registry.register(Box::new(EnterPlanModeTool::with_manager(plan_manager.clone())))?;
    registry.register(Box::new(ExitPlanModeTool::with_manager(plan_manager.clone())))?;
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
    registry.register(Box::new(EnterPlanModeTool::with_manager(plan_manager.clone())))?;
    registry.register(Box::new(ExitPlanModeTool::with_manager(plan_manager.clone())))?;
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
    registry.register(Box::new(shannon_agents::TeamTaskCreateTool::new(coordinator.clone())))?;
    registry.register(Box::new(shannon_agents::TeamTaskUpdateTool::new(coordinator.clone())))?;
    registry.register(Box::new(shannon_agents::TeamTaskListTool::new(coordinator)))?;
    Ok(())
}
