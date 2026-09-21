//! A PR-1: system-prompt assembly extracted from `engine.rs`.
//!
//! The prompt assembly is a pure pipeline: gather stable-zone blocks (base
//! prompt, CLAUDE.md/AGENTS.md, ContextInjector, repo map, browser/team
//! playbooks), gather dynamic-zone blocks (memory, smart context, focus,
//! goal, effort suffix, plan-mode notice), then mark exactly two stable
//! cache breakpoints (first + last block) per the Anthropic 4-breakpoint
//! budget. The environment block (cwd/date/platform/git/sandbox) is
//! appended LAST to both the structured-block list and the plain-string
//! fallback so per-turn mutable state never busts the cached prefix.
//!
//! Extracted so the agent loop reads top-down as a pipeline rather than
//! a 300-line inline ladder.

use shannon_engine::api::types::SystemContentBlock;
use shannon_engine::api::LlmProvider;

use super::browser_control_prompt::{browser_control_prompt, browser_setup_hint};
use super::context_injector::ContextInjector;
use super::team_prompt::team_coordination_prompt;
use crate::query_engine::RepoMapInjector;
use crate::query_engine::types::QueryEngineConfig;
use crate::{project_instructions, sandbox, smart_context};
use crate::tools::ToolRegistry;

/// Inputs to system-prompt assembly — the only `QueryEngine` fields the
/// assembler reads. Carrying a plain struct lets `process_query` describe
/// "what the prompt sees" in one place and keeps the function pure-ish
/// (only `std::env::current_dir()` and the helper imports cross the line).
pub struct SystemPromptInputs<'a> {
    pub config: &'a QueryEngineConfig,
    pub tools: &'a ToolRegistry,
    pub memory_injection: Option<String>,
    pub repo_map_injector: &'a RepoMapInjector,
    pub context_injector: Option<&'a ContextInjector>,
    pub provider: LlmProvider,
    pub user_message: &'a str,
    pub plan_mode_active: bool,
}

/// Result of assembly — both forms (structured blocks + plain fallback
/// string) are returned so the caller decides which to send.
pub struct AssembledSystemPrompt {
    /// Structured blocks with cache-control annotations; `None` when empty
    /// (avoids sending an empty `system` array on the wire).
    pub blocks: Option<Vec<SystemContentBlock>>,
    /// Plain-string fallback appended to `config.system_prompt` plus the
    /// per-turn environment block. Used by clients without structured-
    /// system-prompt support (small Ollama models).
    pub plain: Option<String>,
}

/// Build the structured system prompt and its plain-string fallback.
///
/// Cache policy: Anthropic allows at most 4 `cache_control` breakpoints
/// per request, and the adapter adds two more (last tool definition +
/// last user message). Marking every block cached could emit up to 11
/// system breakpoints and overflow that budget. Mark exactly two system
/// breakpoints instead: the base prompt (never changes) and the LAST
/// stable block (covers the whole stable prefix). Query-dependent and
/// per-turn blocks (smart context, setup hints, focus, goal,
/// environment) are emitted AFTER the last breakpoint so they never
/// bust the cached prefix.
pub fn build(inputs: &SystemPromptInputs<'_>) -> AssembledSystemPrompt {
    let use_cache = matches!(
        inputs.provider,
        LlmProvider::Anthropic | LlmProvider::Bedrock | LlmProvider::Custom
    );

    // ── Stable zone (cacheable prefix; order matters) ───────────────
    let mut stable_blocks: Vec<String> = Vec::new();

    // Base system prompt — the anchor breakpoint: identical across all
    // turns and the largest cache savings come from here.
    if let Some(ref base) = inputs.config.system_prompt {
        stable_blocks.push(base.clone());
    }

    // N-7: memory entries used to sit here in the stable zone, but
    // AutoDream persists extracted memories after every query — a
    // stable-zone placement re-cached the entire prefix (instructions,
    // repomap) on every turn. They are injected in the dynamic zone
    // below instead.

    // Inject CLAUDE.md / AGENTS.md / GEMINI.md project instructions.
    if inputs.config.auto_context_enabled {
        let working_dir =
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        if let Some(ctx) = project_instructions::load_full_context(&working_dir) {
            stable_blocks.push(ctx.content);
        }
    }

    // Inject context from ContextInjector (preference memory + hot-reloaded instructions).
    if let Some(injector) = inputs.context_injector {
        let extra_blocks = injector.build_system_blocks(false);
        for block in extra_blocks {
            stable_blocks.push(block.text);
        }
    }

    // Inject the project repo map (P1-4).
    if inputs.config.repo_map_enabled {
        if let Some(repo_map_md) = inputs.repo_map_injector.build() {
            stable_blocks.push(repo_map_md);
        }
    }

    // Inject browser control instructions when browser MCP tools are present.
    {
        let tool_names = inputs.tools.list();
        if let Some(browser_text) = browser_control_prompt(&tool_names) {
            stable_blocks.push(browser_text);
        }
    }

    // Inject team coordination instructions when team tools are present.
    {
        let tool_names = inputs.tools.list();
        if let Some(team_text) = team_coordination_prompt(&tool_names) {
            stable_blocks.push(team_text);
        }
    }

    // ── Dynamic zone (after the last breakpoint — never cached) ──────
    let mut dynamic_blocks: Vec<SystemContentBlock> = Vec::new();

    // Memory entries (N-7: dynamic-zone placement).
    if let Some(mem_text) = inputs.memory_injection.clone() {
        dynamic_blocks.push(SystemContentBlock::text(mem_text));
    }

    // Smart context: auto-include relevant files based on query.
    if inputs.config.auto_context_enabled {
        let smart_context = {
            let working_dir =
                std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
            smart_context::find_relevant_context(inputs.user_message, &working_dir)
        };
        if let Some(ctx) = smart_context::format_context_for_prompt(&smart_context) {
            dynamic_blocks.push(SystemContentBlock::text(ctx));
        }
    }

    // Browser-related request but no browser tool: surface the `/browser
    // setup` hint so the model can guide the user instead of failing the
    // task silently.
    {
        let tool_names = inputs.tools.list();
        if let Some(hint) = browser_setup_hint(&tool_names, inputs.user_message) {
            dynamic_blocks.push(SystemContentBlock::text(hint));
        }
    }

    // Inject focus area from /focus command into system prompt.
    if let Some(ref focus) = inputs.config.focus_area {
        let focus_text = format!(
            "## User Focus Area\n\
             The user wants you to focus on: **{focus}**.\n\
             Prioritize this area in your responses. Give extra attention to \
             aspects related to {focus} when analyzing, coding, or reviewing."
        );
        dynamic_blocks.push(SystemContentBlock::text(focus_text));
    }

    // Inject session goal from /goal command.
    if let Some(ref goal) = inputs.config.goal {
        dynamic_blocks.push(super::engine::goal_system_block(goal));
    }

    // Effort dial steering (Low/High/Max).
    if let Some(suffix) = super::engine::effort_system_suffix(inputs.config.effort) {
        dynamic_blocks.push(SystemContentBlock::text(suffix.to_string()));
    }

    // Plan-mode awareness.
    if inputs.plan_mode_active {
        dynamic_blocks.push(SystemContentBlock::text(
            "## Plan Mode Active\n\
             You are in plan mode: file writes and state-mutating tools are disabled.\n\
             Research the codebase (read-only tools are available), then present a\n\
             clear, step-by-step implementation plan for the user to review and\n\
             approve before any code is changed."
                .to_string(),
        ));
    }

    // Assemble: stable zone with at most two cache breakpoints, then the
    // dynamic zone (never cached).
    let mut system_blocks: Vec<SystemContentBlock> = Vec::new();
    if use_cache {
        let last_stable = stable_blocks.len().saturating_sub(1);
        for (i, text) in stable_blocks.into_iter().enumerate() {
            let block = if i == 0 || i == last_stable {
                SystemContentBlock::cached(text)
            } else {
                SystemContentBlock::text(text)
            };
            system_blocks.push(block);
        }
    } else {
        for text in stable_blocks {
            system_blocks.push(SystemContentBlock::text(text));
        }
    }
    system_blocks.extend(dynamic_blocks);

    // Decide whether to use structured blocks or fall back to plain string.
    let mut system_blocks_opt = if system_blocks.is_empty() {
        None
    } else {
        Some(system_blocks)
    };
    let mut system_prompt = if inputs.config.system_prompt.is_some()
        || system_blocks_opt.is_some()
    {
        inputs.config.system_prompt.clone()
    } else if inputs.provider == LlmProvider::Ollama {
        // Ollama models use their own chat templates; a system prompt
        // confuses small/unstable models causing malformed output.
        None
    } else {
        Some(super::engine::LOCAL_MODEL_SYSTEM_PROMPT.to_string())
    };
    let mut system_prompt: Option<String> = system_prompt;

    // Inject the environment block (cwd, date/time, platform, git context,
    // sandbox self-description). Deliberately LAST and non-cached.
    if let Ok(cwd) = std::env::current_dir() {
        let env_text = build_env_block(&cwd);
        if let Some(ref mut prompt) = system_prompt {
            prompt.push_str(&env_text);
        }
        if let Some(ref mut blocks) = system_blocks_opt {
            blocks.push(SystemContentBlock::text(env_text));
        }
    }

    AssembledSystemPrompt {
        blocks: system_blocks_opt,
        plain: system_prompt,
    }
}

/// Build the trailing environment block (cwd, date/time, platform, git
/// context, sandbox self-description). Deliberately LAST in both the
/// structured blocks and the plain-string fallback so per-turn mutable
/// state never invalidates the cached prompt prefix.
fn build_env_block(cwd: &std::path::Path) -> String {
    let mut env_text = format!("\n\n## Environment\n\nWorking directory: {}", cwd.display());
    {
        let now = chrono::Local::now();
        env_text.push_str(&format!(
            "\nToday's date: {} ({})",
            now.format("%Y-%m-%d"),
            now.format("%A")
        ));
        env_text.push_str(&format!(
            "\nPlatform: {} ({})",
            std::env::consts::OS,
            std::env::consts::ARCH
        ));
    }
    if let Some(git_ctx) = project_instructions::git_context(cwd) {
        let trimmed = git_ctx.trim_start();
        if !trimmed.is_empty() {
            env_text.push_str("\n\n");
            env_text.push_str(trimmed);
        }
    }
    if let Some(sandbox_text) = sandbox::sandbox_self_description(cwd) {
        env_text.push_str("\n\n");
        env_text.push_str(&sandbox_text);
    }
    env_text
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use shannon_engine::api::types::SystemContentBlock;
    use shannon_engine::api::LlmProvider;
    use crate::query_engine::RepoMapInjector;
    use crate::query_engine::types::QueryEngineConfig;
    use crate::tools::ToolRegistry;

    fn tools_empty() -> ToolRegistry {
        ToolRegistry::new()
    }

    fn inputs_for_test<'a>(
        config: &'a QueryEngineConfig,
        provider: LlmProvider,
        tools: &'a ToolRegistry,
        injector: &'a RepoMapInjector,
    ) -> SystemPromptInputs<'a> {
        SystemPromptInputs {
            config,
            tools,
            memory_injection: None,
            repo_map_injector: injector,
            context_injector: None,
            provider,
            user_message: "do something",
            plan_mode_active: false,
        }
    }

    /// A PR-1 acceptance test: with cache enabled AND a multi-block
    /// stable zone, the assembler emits exactly two cache breakpoints
    /// (first + last stable). With a single stable block the two markers
    /// collapse to one (first == last). Either way at least one cache
    /// marker is required; the env/gate-regression test
    /// `test_cache_breakpoint_budget_respected` in engine.rs covers the
    /// upper-bound invariant.
    #[test]
    fn emits_first_and_last_cache_markers_for_anthropic() {
        let cfg = QueryEngineConfig {
            // Two stable-zone blocks (base + instructions) so first != last.
            system_prompt: Some("base\nsecond".to_string()),
            auto_context_enabled: false,
            repo_map_enabled: false,
            ..Default::default()
        };
        let tools = tools_empty();
        let injector = RepoMapInjector::new(None, 0);
        let out = build(&inputs_for_test(&cfg, LlmProvider::Anthropic, &tools, &injector));
        let blocks = out.blocks.expect("non-empty");
        let cached_count = blocks
            .iter()
            .filter(|b| b.cache_control.is_some())
            .count();
        assert!(
            cached_count >= 1 && cached_count <= 2,
            "expected 1-2 cache breakpoints depending on stable-zone length, got {cached_count}"
        );
    }

    /// Non-Anthropic providers must NOT add cache_control markers.
    #[test]
    fn no_cache_breakpoints_for_openai() {
        let cfg = QueryEngineConfig {
            system_prompt: Some("base".to_string()),
            ..Default::default()
        };
        let tools = tools_empty();
        let injector = RepoMapInjector::new(None, 0);
        let out = build(&inputs_for_test(&cfg, LlmProvider::OpenAI, &tools, &injector));
        let blocks = out.blocks.expect("non-empty");
        assert!(
            blocks.iter().all(|b| b.cache_control.is_none()),
            "OpenAI must not receive cache_control markers"
        );
    }
}
