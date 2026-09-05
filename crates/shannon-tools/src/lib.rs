// Suppress lints that conflict with rustfmt or are style preferences from newer clippy.
#![allow(
    clippy::collapsible_if,
    clippy::collapsible_match,
    clippy::derivable_impls
)]

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
//! - Image analysis operations (AnalyzeImage for LLM-powered image analysis)
//! - Tool search operations (ToolSearch for tool discovery)
//! - Ask user operations (AskUserQuestion for interactive confirmation and option selection)
//! - Structured output operations (StructuredOutput for AI-returned JSON data)
//! - REPL operations (REPL for batch command execution)
//! - MCP auth operations (McpAuth for OAuth authentication with MCP servers)

use std::sync::Arc;

mod defaults;
pub mod sandbox;

pub mod agent;
pub mod ask_user;
pub mod brief;
pub mod computer_use;
pub mod config;
pub mod cron;
pub mod docs_query;
pub mod file;
pub mod git;
pub mod github;
pub mod goal;
pub mod grep;
pub mod image_analysis;
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
pub use computer_use::{
    ComputerAction, ComputerUseConfig, ComputerUseInput, ComputerUseTool, REFERENCE_HEIGHT,
    REFERENCE_WIDTH, ScrollDirection,
};
pub use config::{ConfigAction, ConfigInput, ConfigManager, ConfigTool, SharedConfigManager};
pub use cron::{
    CronCreateInput, CronCreateOutput, CronDeleteInput, CronDeleteOutput, CronListInput,
    CronListOutput, CronTool,
};
pub use docs_query::{DocsQueryInput, DocsQueryOutput, DocsQueryTool};
pub use file::diff_renderer::{
    ColorScheme, DiffHunk as DiffRenderHunk, DiffLine, DiffLineType, DiffRenderer, DiffStats,
};
pub use file::history::{
    DiffHunk, FileDiff, FileHistory, FileHistoryConfig, FileHistoryError, FileHistoryManager,
    FileOperation as FileHistoryOperation, FileSnapshot, RewindAction,
};
pub use file::sandbox::{PathSandbox, SandboxConfig as PathSandboxConfig, SandboxError};
pub use file::sandbox_adapter::{
    PathSandboxAdapter, SandboxAdapter, SandboxConfig as SandboxAdapterConfig, SandboxResult,
    SandboxViolation,
};
pub use file::{
    EditTool, FileOperation, GlobTool, MergeResolveTool, MultiEditTool, ReadTool, WriteTool,
};
pub use git::{
    AutoCommitTool, GitBranchTool, GitDiffTool, GitLogTool, GitSafetyTool, GitStashTool,
};
pub use github::{GhIssueListTool, GhIssueViewTool, GhPrCreateTool, GhPrListTool, GhPrViewTool};
pub use grep::GrepTool;
pub use image_analysis::{AnalyzeImageInput, AnalyzeImageTool};
pub use lsp::{
    CodeActionItem, CodeActionsInput, CodeActionsOutput, CodeActionsTool, DocumentSymbolInput,
    DocumentSymbolItem, DocumentSymbolOutput, DocumentSymbolTool, FindReferencesInput,
    FindReferencesOutput, FindReferencesTool, GoToDefinitionInput, GoToDefinitionOutput,
    GoToDefinitionTool, HoverInput, HoverOutput, HoverResult, HoverTool, LspLocation, LspPosition,
    LspRange, LspToolWorlds, RenameSymbolInput, RenameSymbolOutput, RenameSymbolTool,
    WorkspaceSymbolInput, WorkspaceSymbolItem, WorkspaceSymbolOutput, WorkspaceSymbolTool,
    detect_language_id,
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

/// Execution worlds injected into the standard tool set (§4.11 W3-3a).
///
/// `LocalFs` / `LocalProcess` are the defaults; a §4.12 sandbox assembly (or
/// any future remote-execution world) swaps both handles here and every
/// tool registered through [`register_default_tools_with_providers`] /
/// [`register_default_tools_with_project_dir_ex_with_providers`] runs against
/// the replacement — no tool code changes.
#[derive(Clone)]
pub struct ToolProviders {
    /// Filesystem world for read/write/edit/list-style tools.
    pub fs: std::sync::Arc<dyn shannon_tool_interface::FileSystemProvider>,
    /// Process world for bash/git/gh/lsp-style tools.
    pub process: std::sync::Arc<dyn shannon_tool_interface::ProcessProvider>,
    /// §4.12: classifier tagging kernel-denied bash runs as `sandbox_denied`.
    /// `None` unless a kernel-enforcing backend produced this set.
    pub denial_classifier: Option<crate::sandbox::DenialClassifier>,
    /// Shared world-roots override for swappable execution worlds (remote
    /// targets). `None` keeps every tool's sandbox fully local.
    pub world_sandbox: Option<std::sync::Arc<crate::file::sandbox::WorldSandboxHandle>>,
}

impl Default for ToolProviders {
    fn default() -> Self {
        Self {
            fs: defaults::fs(),
            process: defaults::process(),
            denial_classifier: None,
            world_sandbox: None,
        }
    }
}

/// Unified registration. All public entry points funnel through this so the
/// registered set and ordering stay byte-identical across variants.
fn register_all_tools(
    registry: &mut ToolRegistry,
    project_dir: Option<&std::path::Path>,
    wire_history: bool,
    providers: &ToolProviders,
) -> Result<ToolRegistrationResult, Box<dyn std::error::Error>> {
    use crate::file::sandbox::{PathSandbox, SandboxConfig as PathSandboxConfig};
    let ToolProviders {
        fs,
        process,
        denial_classifier,
        world_sandbox,
    } = providers;

    // Project-scoped sandbox when a project directory is given (same config
    // shape as the pre-provider `register_default_tools_with_project_dir`).
    let sandbox_base = match project_dir {
        Some(dir) => {
            // B3 (docs/eval-findings-2026-09-glm.md): grant the file tools
            // the same writable roots the command sandbox grants — the
            // project dir plus the temp root. Without the temp root, Write
            // refuses /tmp while sandboxed Bash happily writes it, and the
            // model splits its writes across inconsistent tool worlds.
            let allowed_roots =
                crate::file::sandbox::SandboxConfig::command_aligned_roots(dir);
            // A3: when the command sandbox backends relocate the project dir
            // to /workspace (bwrap/Docker — see `SANDBOX_BIND_ALIAS`), echo
            // output paths in that same view so `cd`/`ls` on an echoed path
            // succeeds. Remote worlds swap roots through `world_sandbox` and
            // address files by their real remote paths, so aliasing stays
            // off there.
            let bind_alias_outputs = world_sandbox.is_none()
                && matches!(
                    shannon_core::sandbox::SandboxExecutor::detect_sandboxer(),
                    shannon_core::sandbox::SandboxType::Bubblewrap
                        | shannon_core::sandbox::SandboxType::Docker
                );
            PathSandbox::with_config(PathSandboxConfig {
                allowed_roots,
                denied_patterns: PathSandboxConfig::default_denied_patterns(),
                strict_mode: true,
            })
            .with_bind_alias_output(bind_alias_outputs)
        }
        None => PathSandbox::new(),
    };
    // TOCTOU canonicalization follows the injected world too; a shared
    // world-roots handle lets remote assemblies retarget every tool's
    // sandbox at runtime (`/remote use`) without a registry rebuild.
    let sandbox = match world_sandbox {
        Some(handle) => sandbox_base
            .with_fs_provider(fs.clone())
            .with_world_sandbox(handle.clone()),
        None => sandbox_base.with_fs_provider(fs.clone()),
    };

    // ── File-history snapshots (shared manager for file-level `/undo`; W6-2) ──
    // Wired only by the project-dir entry points (pre-provider semantics);
    // `from_env` honors SHANNON_FILE_HISTORY (disable), _DIR, _TTL overrides.
    let history = if wire_history {
        FileHistoryConfig::from_env().map(|cfg| {
            Arc::new(std::sync::Mutex::new(
                FileHistoryManager::new(cfg).with_fs(fs.clone()),
            ))
        })
    } else {
        None
    };

    // ── File operations ────────────────────────────────────────────────
    registry.register(Box::new(
        ReadTool::with_sandbox(sandbox.clone()).with_fs(fs.clone()),
    ))?;
    registry.register(Box::new(
        WriteTool::with_sandbox(sandbox.clone())
            .with_history_opt(history.clone())
            .with_fs(fs.clone()),
    ))?;
    registry.register(Box::new(
        EditTool::with_sandbox(sandbox.clone())
            .with_history_opt(history.clone())
            .with_worlds(fs.clone(), process.clone()),
    ))?;
    registry.register(Box::new(
        MultiEditTool::with_sandbox(sandbox.clone())
            .with_history_opt(history.clone())
            .with_fs(fs.clone()),
    ))?;
    registry.register(Box::new(
        GlobTool::with_sandbox(sandbox.clone()).with_fs(fs.clone()),
    ))?;
    // The plain entry point historically also exposes MergeResolve here.
    if !wire_history {
        registry.register(Box::new(MergeResolveTool::new().with_fs(fs.clone())))?;
    }

    // ── System operations ──────────────────────────────────────────────
    let mut bash = match project_dir {
        Some(dir) => BashTool::with_process_sandbox(dir),
        None => BashTool::new(),
    }
    .with_worlds(process.clone());
    if let Some(classifier) = denial_classifier {
        bash = bash.with_denial_classifier(classifier.clone());
    }
    registry.register(Box::new(bash))?;
    registry.register(Box::new(SleepTool::new()))?;
    registry.register(Box::new(
        PowerShellTool::new().with_process(process.clone()),
    ))?;
    registry.register(Box::new(ReplTool::new().with_process(process.clone())))?;

    // ── Git operations ─────────────────────────────────────────────────
    registry.register(Box::new(GitBranchTool::new().with_process(process.clone())))?;
    registry.register(Box::new(GitDiffTool::new().with_process(process.clone())))?;
    registry.register(Box::new(GitLogTool::new().with_process(process.clone())))?;
    registry.register(Box::new(GitStashTool::new().with_process(process.clone())))?;
    registry.register(Box::new(GitSafetyTool::new().with_process(process.clone())))?;
    registry.register(Box::new(
        AutoCommitTool::new().with_process(process.clone()),
    ))?;

    // ── GitHub operations ───────────────────────────────────────────────
    registry.register(Box::new(
        GhIssueListTool::new().with_process(process.clone()),
    ))?;
    registry.register(Box::new(
        GhIssueViewTool::new().with_process(process.clone()),
    ))?;
    registry.register(Box::new(
        GhPrCreateTool::new().with_process(process.clone()),
    ))?;
    registry.register(Box::new(GhPrListTool::new().with_process(process.clone())))?;
    registry.register(Box::new(GhPrViewTool::new().with_process(process.clone())))?;

    // ── Web operations ─────────────────────────────────────────────────
    registry.register(Box::new(WebFetchTool::new()))?;
    registry.register(Box::new(WebSearchTool::new()))?;
    registry.register(Box::new(DocsQueryTool::new()))?;

    // ── Search ─────────────────────────────────────────────────────────
    registry.register(Box::new(
        GrepTool::with_sandbox(sandbox.clone()).with_fs(fs.clone()),
    ))?;

    // ── Multimodal ──────────────────────────────────────────────────────
    registry.register(Box::new(AnalyzeImageTool::new().with_fs(fs.clone())))?;

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
    registry.register(Box::new(NotebookEditTool::new().with_fs(fs.clone())))?;

    // ── Worktree ───────────────────────────────────────────────────────
    registry.register(Box::new(WorktreeTool::new().with_process(process.clone())))?;

    // ── Plan mode (shared state + PlanManager; persistence via fs world) ──
    let plan_manager = PlanManager::new().with_fs(fs.clone());
    registry.register(Box::new(EnterPlanModeTool::with_manager(
        plan_manager.clone(),
    )))?;
    registry.register(Box::new(ExitPlanModeTool::with_manager(
        plan_manager.clone(),
    )))?;
    registry.register(Box::new(GetPlanStatusTool::new(plan_manager.clone())))?;

    // ── LSP (spawns + didOpen reads ride the injected worlds) ───────────
    macro_rules! register_lsp {
        ($t:expr) => {{
            let t = $t;
            registry.register(Box::new(t.with_worlds(process.clone(), fs.clone())))?;
        }};
    }
    register_lsp!(GoToDefinitionTool::new());
    register_lsp!(FindReferencesTool::new());
    register_lsp!(HoverTool::new());
    register_lsp!(DocumentSymbolTool::new());
    register_lsp!(WorkspaceSymbolTool::new());
    register_lsp!(RenameSymbolTool::new());
    register_lsp!(CodeActionsTool::new());

    // ── Interactive ────────────────────────────────────────────────────
    registry.register(Box::new(AskUserQuestionTool::with_terminal_handler()))?;

    // ── Skill & discovery ──────────────────────────────────────────────
    registry.register(Box::new(SkillTool::new()))?;

    // ── Cron (persistence rides the fs world) ──────────────────────────
    registry.register(Box::new(CronTool::with_persistence().with_fs(fs.clone())))?;

    // ── ScheduleWakeup (/loop dynamic pacing) ──────────────────────────
    registry.register(Box::new(ScheduleWakeupTool::new()))?;

    // ── Config ─────────────────────────────────────────────────────────
    registry.register(Box::new(ConfigTool::new()))?;

    // ── Utility tools ──────────────────────────────────────────────────
    registry.register(Box::new(BriefTool::new()))?;
    registry.register(Box::new(StructuredOutputTool::new()))?;
    registry.register(Box::new(McpAuthTool::new()))?;

    // ── Computer Use (desktop automation) ────────────────────────────────
    registry.register(Box::new(ComputerUseTool::new()))?;

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
        file_history: history,
    })
}

/// Register all standard tools into the given registry.
///
/// Default execution worlds (`LocalFs` / `LocalProcess`) back every spawned
/// or file-backed operation; see [`ToolProviders`] for whole-world swaps.
pub fn register_default_tools(
    registry: &mut ToolRegistry,
) -> Result<std::sync::Arc<std::sync::Mutex<Option<AgentToolContext>>>, Box<dyn std::error::Error>>
{
    let worlds = ToolProviders::default();
    Ok(register_all_tools(registry, None, false, &worlds)?.agent_context_handle)
}

/// Like [`register_default_tools`] but with caller-supplied execution worlds.
///
/// This is the seam consumed by sandbox assemblies (§4.12): pass decorated
/// `FileSystemProvider`/`ProcessProvider` implementations and every registered
/// tool runs against them.
pub fn register_default_tools_with_providers(
    registry: &mut ToolRegistry,
    providers: &ToolProviders,
) -> Result<std::sync::Arc<std::sync::Mutex<Option<AgentToolContext>>>, Box<dyn std::error::Error>>
{
    Ok(register_all_tools(registry, None, false, providers)?.agent_context_handle)
}

/// Register all standard tools with project-specific sandbox configuration.
pub fn register_default_tools_with_project_dir(
    registry: &mut ToolRegistry,
    project_dir: &std::path::Path,
) -> Result<Arc<std::sync::Mutex<Option<AgentToolContext>>>, Box<dyn std::error::Error>> {
    let worlds = ToolProviders::default();
    Ok(register_all_tools(registry, Some(project_dir), true, &worlds)?.agent_context_handle)
}

/// Like [`register_default_tools_with_project_dir_ex`] but with
/// caller-supplied execution worlds (§4.11/§4.12 assembly point).
pub fn register_default_tools_with_project_dir_ex_with_providers(
    registry: &mut ToolRegistry,
    project_dir: &std::path::Path,
    providers: &ToolProviders,
) -> Result<ToolRegistrationResult, Box<dyn std::error::Error>> {
    register_all_tools(registry, Some(project_dir), true, providers)
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
    /// The provider-wired file history the file tools share. REPL-side
    /// `/rewind` must reuse this same manager or it reads a different
    /// (local-only) snapshot store than the tools wrote.
    pub file_history: Option<Arc<std::sync::Mutex<FileHistoryManager>>>,
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
///
/// ## Sandbox selection (§4.12 W3-3b)
///
/// Before assembling, [`crate::sandbox::SandboxSettings::detect`] resolves
/// `sandbox = off|local|landlock` from env/TOML:
///
/// - `off` (default): the exact legacy body below runs — byte-for-byte the
///   §4.11 passthrough, including the auto-detected argv-level wrappers.
/// - `local`: user-space policy decorators ride the same registration
///   ordering (see [`crate::sandbox::assemble_local`]).
/// - `landlock`: the kernel-enforced world decorates both providers; tool
///   code is unchanged. A host that cannot enforce degrades **loudly** to
///   the legacy body (`tracing::error`) rather than pretending to restrict.
pub fn register_default_tools_with_project_dir_ex(
    registry: &mut ToolRegistry,
    project_dir: &std::path::Path,
) -> Result<ToolRegistrationResult, Box<dyn std::error::Error>> {
    let detected = crate::sandbox::SandboxSettings::detect();
    let worlds = match detected.mode {
        // Default + explicit passthrough: legacy assembly, untouched.
        shannon_tool_interface::SandboxMode::Off => None,

        shannon_tool_interface::SandboxMode::Local => {
            Some(crate::sandbox::assemble_local(&detected, project_dir))
        }
        shannon_tool_interface::SandboxMode::Landlock => {
            match crate::sandbox::assemble(&detected, project_dir) {
                Ok(assembled) => Some(assembled),
                Err(e) => {
                    // Explicit degrade to the status-quo world — never a
                    // silent fake sandbox.
                    tracing::error!(
                        "sandbox=landlock requested but unavailable ({e}); \
                         falling back to unrestricted local execution"
                    );
                    None
                }
            }
        }
    };

    if let Some(assembled) = worlds {
        for notice in &assembled.notices {
            tracing::warn!(tag = %notice.tag, "sandbox: {}", notice.detail);
        }
        return register_all_tools(registry, Some(project_dir), true, &assembled.providers);
    }

    register_all_tools(registry, Some(project_dir), true, &ToolProviders::default())
}
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
#[allow(clippy::unwrap_used)]
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

    // ── A3/B3: project registration aligns file tools with the command sandbox ──

    /// B3 (docs/eval-findings-2026-09-glm.md): after a project-dir
    /// registration the Write tool must accept the temp root — the same
    /// writable root the sandboxed Bash tool grants. Before the alignment,
    /// Write rejected /tmp with "outside allowed roots" while Bash wrote
    /// there freely.
    #[test]
    fn project_registration_lets_write_use_tmp_root() {
        let project = tempfile::tempdir().unwrap();
        let mut registry = ToolRegistry::new();
        register_default_tools_with_project_dir(&mut registry, project.path())
            .expect("project registration");

        let write = registry.get("Write").expect("Write tool registered");
        let target = std::env::temp_dir().join(format!(
            "shannon_b3_registration_{}.txt",
            std::process::id()
        ));
        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(write.execute(serde_json::json!({
                "file_path": target.to_string_lossy(),
                "content": "scratch"
            })));
        assert!(
            result.is_ok(),
            "Write to the temp root must succeed after project registration: {result:?}"
        );
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "scratch");
        let _ = std::fs::remove_file(&target);
    }

    /// A3 (docs/eval-findings-2026-09-glm.md): when the command sandbox
    /// backends bind the project dir at /workspace, Read output must echo
    /// the /workspace spelling; otherwise (no relocating backend) the host
    /// path is kept.
    #[test]
    fn project_registration_echo_matches_command_sandbox_view() {
        let project = tempfile::tempdir().unwrap();
        std::fs::write(project.path().join("r.txt"), "content").unwrap();
        let host_path = project.path().join("r.txt");
        let host_str = host_path.to_string_lossy().to_string();

        let mut registry = ToolRegistry::new();
        register_default_tools_with_project_dir(&mut registry, project.path())
            .expect("project registration");

        let read = registry.get("Read").expect("Read tool registered");
        let output = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(read.execute(serde_json::json!({
                "file_path": host_str
            })))
            .expect("read should succeed");
        let echoed = output.metadata["file_path"].as_str().unwrap().to_string();

        let command_sandbox_relocates = matches!(
            shannon_core::sandbox::SandboxExecutor::detect_sandboxer(),
            shannon_core::sandbox::SandboxType::Bubblewrap
                | shannon_core::sandbox::SandboxType::Docker
        );
        if command_sandbox_relocates {
            assert_eq!(
                echoed, "/workspace/r.txt",
                "with a relocating backend the echo must be the sandbox view"
            );
        } else {
            assert_eq!(
                echoed, host_str,
                "without a relocating backend the host path is the sandbox view"
            );
        }
    }
}
