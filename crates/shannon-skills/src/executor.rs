//! Skill execution engine

use crate::definition::SkillResult as SkillExecutionResult;
use crate::definition::{Skill, SkillContext};
use crate::error::{SkillError, SkillResult};
use regex::Regex;
use std::path::Path;
use std::sync::OnceLock;
use tracing::debug;

/// Optional sink for emitting hook events from skill execution.
///
/// Passed in by callers (CLI/UI/REPL) so `shannon-skills` does not need a
/// hard dependency on `shannon-engine`. The signature mirrors the
/// `HookManager::run_hooks` future so callers can route events to the manager
/// without changing the public API.
pub trait HookEmitter: Send + Sync {
    /// Emit a hook event. Implementations should be fire-and-forget — failures
    /// here must not break skill execution.
    fn emit(&self, event_json: Vec<u8>);
}

/// No-op emitter used when no hook system is wired up.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopHookEmitter;

impl HookEmitter for NoopHookEmitter {
    fn emit(&self, _event_json: Vec<u8>) {}
}

/// Cached regex pattern for inline shell commands: !`command`
fn inline_shell_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(r"!`([^`]+)`").expect("inline shell pattern is valid"))
}

/// Cached regex pattern for block shell commands: ```!\ncommand\n```
fn block_shell_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(r"```!\n(.+?)\n```").expect("block shell pattern is valid"))
}

/// True when the content contains any inline `!`cmd`` or block ```!``` shell
/// command markers.
fn contains_shell_commands(content: &str) -> bool {
    inline_shell_pattern().is_match(content) || block_shell_pattern().is_match(content)
}

/// Emit the F26 "shell blocked" warning at most once per skill id.
///
/// Project-sourced skills default to `allow_shell = false`; the warning tells
/// the skill author why their `!`cmd`` blocks are being passed through as
/// literal text instead of executing.
fn warn_shell_denied_once(
    skill_id: &str,
    skill_name: &str,
    source: &crate::definition::SkillSource,
) {
    static WARNED: OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> = OnceLock::new();
    let warned = WARNED.get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()));
    let Ok(mut set) = warned.lock() else {
        return;
    };
    if set.insert(skill_id.to_string()) {
        tracing::warn!(
            skill_id = %skill_id,
            skill = %skill_name,
            source = ?source,
            "Skill contains shell commands (!`cmd`) but shell execution is not \
             allowed for its source; blocks are left as literal text. Project \
             skills default to allow_shell=false — move the skill to \
             ~/.shannon/skills or grant shell permissions explicitly to enable \
             execution."
        );
    }
}

/// Engine for executing skills and generating prompt content
pub struct SkillExecutor {
    /// Shell command executor
    shell_executor: Option<ShellExecutor>,
    /// Optional hook emitter for `UserPromptExpansion` events.
    hook_emitter: Option<Box<dyn HookEmitter>>,
}

impl Default for SkillExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl SkillExecutor {
    /// Create a new skill executor
    pub fn new() -> Self {
        Self {
            shell_executor: Some(ShellExecutor::new()),
            hook_emitter: None,
        }
    }

    /// Attach a hook emitter so this executor fires `UserPromptExpansion`
    /// events after template substitution completes.
    ///
    /// The emitter is invoked once per `execute()` call with a serialized
    /// `HookEvent::UserPromptExpansion` event. If unset, no event is emitted.
    pub fn with_hook_emitter(mut self, emitter: Box<dyn HookEmitter>) -> Self {
        self.hook_emitter = Some(emitter);
        self
    }

    /// Execute a skill with the given context
    pub fn execute(
        &self,
        skill: &Skill,
        context: &SkillContext,
    ) -> SkillResult<SkillExecutionResult> {
        let start = std::time::Instant::now();

        // Capture the pre-substitution content so we can emit
        // `UserPromptExpansion { original_prompt, expanded_prompt }` after
        // template substitution. The base-directory prefix is part of the
        // expanded form; the raw skill content is the original template.
        let original_prompt = skill.content.clone();

        // Start with the skill content
        let mut content = skill.content.clone();

        // Add base directory prefix if applicable
        if let Some(skill_root) = &skill.skill_root {
            let prefix = format!(
                "Base directory for this skill: {}\n\n",
                skill_root.display()
            );
            content = prefix + &content;
        }

        // Substitute positional arguments
        content = self.substitute_arguments(&content, &context.arguments)?;

        // Substitute named arguments (if the skill defines argument names)
        if let Some(ref arg_config) = skill.arguments {
            // ArgumentConfig::Single may contain space-separated names (e.g. "issue branch")
            let names: Vec<String> = match arg_config {
                crate::frontmatter::ArgumentConfig::Single(s) => {
                    s.split_whitespace().map(String::from).collect()
                }
                crate::frontmatter::ArgumentConfig::Multiple(names) => names.clone(),
            };
            content = self.substitute_named_arguments(&content, &names, &context.arguments)?;
        }

        // Substitute environment variables
        content = self.substitute_variables(&content, context)?;

        // Execute shell commands if allowed. MCP-sourced skills never run
        // shell. F26: skills whose permissions deny shell (project-sourced
        // skills default to deny — see `SkillPermissions::for_source`) skip
        // execution and leave the !`cmd` blocks as literal text, with a
        // one-time warning so the author knows why nothing ran.
        let had_shell = if skill.source == crate::definition::SkillSource::Mcp {
            false
        } else if context.permissions.allow_shell {
            self.execute_shell_commands(&mut content, context)?
        } else {
            if contains_shell_commands(&content) {
                warn_shell_denied_once(&skill.id, &skill.name, &skill.source);
            }
            false
        };

        // Emit `UserPromptExpansion` hook event now that all template
        // variables (`$ARGUMENTS`, `${0}`, `${CLAUDE_SESSION_ID}`, ...) have
        // been resolved. The emitter is optional; if not wired up we silently
        // skip to keep this crate's dep surface minimal.
        self.emit_user_prompt_expansion(&original_prompt, &content);

        let duration = start.elapsed();

        Ok(SkillExecutionResult {
            skill_id: skill.id.clone(),
            prompt_content: content,
            skip_model_invocation: skill.disable_model_invocation,
            metadata: crate::definition::SkillResultMetadata {
                executed_at: chrono::Utc::now(),
                duration_ms: duration.as_millis() as u64,
                had_shell_commands: had_shell,
            },
        })
    }

    /// Fire the `UserPromptExpansion` hook event when an emitter is attached.
    ///
    /// The event JSON is the same shape `HookManager::run_hooks` consumes, so
    /// callers can wrap an existing `HookManager` with a thin adapter. Schema
    /// matches `HookEvent::UserPromptExpansion` in
    /// `crates/shannon-engine/src/hooks/events.rs`.
    fn emit_user_prompt_expansion(&self, original_prompt: &str, expanded_prompt: &str) {
        if let Some(emitter) = &self.hook_emitter {
            let event = shannon_engine::hooks::HookEvent::UserPromptExpansion {
                expanded_prompt: expanded_prompt.to_string(),
                original_prompt: original_prompt.to_string(),
            };
            match serde_json::to_vec(&event) {
                Ok(bytes) => {
                    debug!("Emitting UserPromptExpansion event ({} bytes)", bytes.len());
                    emitter.emit(bytes);
                }
                Err(e) => {
                    debug!("Failed to serialize UserPromptExpansion event: {e}");
                }
            }
        }
    }

    /// Substitute argument placeholders in content
    fn substitute_arguments(&self, content: &str, args: &[String]) -> SkillResult<String> {
        let mut result = content.to_string();

        // $ARGUMENTS[N] syntax (must run before bare $N to avoid conflicts)
        for (i, arg) in args.iter().enumerate() {
            let placeholder = format!("$ARGUMENTS[{i}]");
            result = result.replace(&placeholder, arg);
        }

        // ${0}, ${1}, etc. - indexed arguments (with braces)
        for (i, arg) in args.iter().enumerate() {
            let placeholder = format!("${{{i}}}");
            result = result.replace(&placeholder, arg);
        }

        // ${args} - all arguments joined by space
        let all_args = args.join(" ");
        result = result.replace("${args}", &all_args);

        // $ARGUMENTS - all arguments (without braces)
        result = result.replace("$ARGUMENTS", &all_args);

        // $ARGUMENTS[N] and ${N} handled above; bare $N is intentionally NOT
        // replaced to avoid ambiguity with shell variable syntax.

        // ${args:quote} - all arguments shell-quoted
        let quoted_args = args
            .iter()
            .map(|a| shell_words::quote(a))
            .collect::<Vec<_>>()
            .join(" ");
        result = result.replace("${args:quote}", &quoted_args);

        Ok(result)
    }

    /// Substitute named argument placeholders in content.
    ///
    /// For each `(name, value)` pair, replaces occurrences of `$name` in the
    /// content with the corresponding value. For example, if `names` is
    /// `["issue", "branch"]` and `values` is `["42", "main"]`, then `$issue`
    /// becomes `42` and `$branch` becomes `main`.
    ///
    /// Values that are shorter than names are simply missing — unmatched names
    /// are left as-is.
    fn substitute_named_arguments(
        &self,
        content: &str,
        names: &[String],
        values: &[String],
    ) -> SkillResult<String> {
        let mut result = content.to_string();

        for (i, name) in names.iter().enumerate() {
            if let Some(value) = values.get(i) {
                let placeholder = format!("${name}");
                result = result.replace(&placeholder, value);
            }
        }

        Ok(result)
    }

    /// Substitute environment variables
    fn substitute_variables(&self, content: &str, context: &SkillContext) -> SkillResult<String> {
        let mut result = content.to_string();

        // ${CLAUDE_SESSION_ID}
        result = result.replace("${CLAUDE_SESSION_ID}", &context.session_id);

        // ${CLAUDE_EFFORT}
        result = result.replace("${CLAUDE_EFFORT}", &context.effort_level);

        // ${CLAUDE_SKILL_DIR}
        if let Some(skill_root) = &context.cwd.parent() {
            result = result.replace("${CLAUDE_SKILL_DIR}", &skill_root.display().to_string());
        }

        // ${CWD}
        result = result.replace("${CWD}", &context.cwd.display().to_string());

        Ok(result)
    }

    /// Execute shell commands in the content
    fn execute_shell_commands(
        &self,
        content: &mut String,
        context: &SkillContext,
    ) -> SkillResult<bool> {
        let Some(executor) = &self.shell_executor else {
            return Ok(false);
        };

        // Use cached regex patterns for shell commands: !`command` or ```!\ncommand\n```
        let inline_pattern = inline_shell_pattern();
        let block_pattern = block_shell_pattern();

        // F25: a single replace_all pass per pattern. The previous `while
        // is_match` loop re-scanned the whole content — including the stdout
        // of commands that had just run — so a command whose output contained
        // a !`cmd` pattern got that output executed too, and self-reproducing
        // output looped forever. Command output must be inert: it is inserted
        // verbatim and never re-scanned.
        let had_inline = inline_pattern.is_match(content);
        if had_inline {
            *content = inline_pattern
                .replace_all(content, |caps: &regex::Captures| {
                    let cmd = &caps[1];
                    match executor.execute(cmd, &context.cwd) {
                        Ok(output) => output,
                        Err(e) => format!("[Command failed: {e}]"),
                    }
                })
                .to_string();
        }

        let had_block = block_pattern.is_match(content);
        if had_block {
            *content = block_pattern
                .replace_all(content, |caps: &regex::Captures| {
                    let cmd = &caps[1];
                    match executor.execute(cmd, &context.cwd) {
                        Ok(output) => output,
                        Err(e) => format!("[Command failed: {e}]"),
                    }
                })
                .to_string();
        }

        Ok(had_inline || had_block)
    }
}

/// Shell command executor
pub struct ShellExecutor {
    /// Environment variables for commands
    env: std::collections::HashMap<String, String>,
}

/// Validates a shell command string for dangerous metacharacters to prevent injection.
///
/// Rejects commands containing characters that enable command chaining,
/// substitution, or redirection while allowing basic commands with arguments.
fn validate_shell_command(command: &str) -> SkillResult<()> {
    // Reject patterns that shell_words::split can't safely tokenize
    if command.contains('\n') {
        return Err(SkillError::ExecutionFailed {
            name: "shell".to_string(),
            message: "Command rejected: contains newline. Only single basic commands are allowed."
                .to_string(),
        });
    }
    if command.contains('$') && (command.contains('(') || command.contains('{')) {
        return Err(SkillError::ExecutionFailed {
            name: "shell".to_string(),
            message: "Command rejected: contains command/variable substitution. Only basic commands are allowed.".to_string(),
        });
    }
    if command.contains('`') {
        return Err(SkillError::ExecutionFailed {
            name: "shell".to_string(),
            message:
                "Command rejected: contains command substitution. Only basic commands are allowed."
                    .to_string(),
        });
    }

    // Split into tokens to validate operators, respecting quoting.
    // shell_words doesn't recognize ; as a separator, so check raw string for it.
    if command.contains(';') {
        return Err(SkillError::ExecutionFailed {
            name: "shell".to_string(),
            message: "Command rejected: contains command chaining (;). Only single basic commands are allowed.".to_string(),
        });
    }

    match shell_words::split(command) {
        Ok(tokens) => {
            // Check for shell operators as standalone tokens (pipe, redirect, etc.)
            let dangerous_tokens: &[&str] = &["|", "||", "&&", ">", ">>", "<", "<<"];
            for token in &tokens {
                if dangerous_tokens.contains(&token.as_str()) {
                    return Err(SkillError::ExecutionFailed {
                        name: "shell".to_string(),
                        message: format!(
                            "Command rejected: contains shell operator ({token:?}). \
                             Only single basic commands with arguments are allowed."
                        ),
                    });
                }
            }
            Ok(())
        }
        Err(e) => Err(SkillError::ExecutionFailed {
            name: "shell".to_string(),
            message: format!("Failed to parse command: {e}"),
        }),
    }
}

impl Default for ShellExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl ShellExecutor {
    /// Create a new shell executor
    pub fn new() -> Self {
        Self {
            env: std::collections::HashMap::new(),
        }
    }

    /// Execute a shell command and return its output.
    ///
    /// Commands are parsed into executable + args and executed directly
    /// (no shell invocation) to prevent command injection.
    pub fn execute(&self, command: &str, cwd: &Path) -> SkillResult<String> {
        debug!("Executing shell command: {}", command);

        validate_shell_command(command)?;

        let parts = shell_words::split(command).map_err(|e| SkillError::ExecutionFailed {
            name: "shell".to_string(),
            message: format!("Failed to parse command: {e}"),
        })?;

        if parts.is_empty() {
            return Err(SkillError::ExecutionFailed {
                name: "shell".to_string(),
                message: "Empty command".to_string(),
            });
        }

        let output = std::process::Command::new(&parts[0])
            .args(&parts[1..])
            .current_dir(cwd)
            .envs(&self.env)
            .output()
            .map_err(|e| SkillError::ExecutionFailed {
                name: "shell".to_string(),
                message: format!("Failed to execute command: {e}"),
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(SkillError::ExecutionFailed {
                name: "shell".to_string(),
                message: format!("Command failed: {stderr}"),
            });
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(stdout.into_owned())
    }

    /// Set an environment variable for commands
    pub fn set_env(&mut self, key: String, value: String) {
        self.env.insert(key, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::definition::SkillPermissions;
    use std::path::PathBuf;

    #[test]
    fn test_substitute_arguments() {
        let executor = SkillExecutor::new();
        let content = "Hello ${0}, you have ${1} messages";
        let args = vec!["Alice".to_string(), "5".to_string()];
        let result = executor.substitute_arguments(content, &args).unwrap();
        assert_eq!(result, "Hello Alice, you have 5 messages");
    }

    #[test]
    fn test_substitute_all_args() {
        let executor = SkillExecutor::new();
        let content = "Processing: ${args}";
        let args = vec!["file1.txt".to_string(), "file2.txt".to_string()];
        let result = executor.substitute_arguments(content, &args).unwrap();
        assert_eq!(result, "Processing: file1.txt file2.txt");
    }

    #[test]
    fn test_skill_execution() {
        let executor = SkillExecutor::new();
        let skill = Skill::new(
            "test".to_string(),
            "Test".to_string(),
            "A test skill".to_string(),
            "Hello ${0}!".to_string(),
        );

        let context = SkillContext {
            arguments: vec!["World".to_string()],
            cwd: PathBuf::from("/tmp"),
            session_id: "test-session".to_string(),
            effort_level: "medium".to_string(),
            permissions: SkillPermissions::default(),
        };

        let result = executor.execute(&skill, &context).unwrap();
        assert_eq!(result.prompt_content, "Hello World!");
    }

    #[test]
    fn test_validate_shell_command_accepts_safe() {
        assert!(validate_shell_command("echo hello").is_ok());
        assert!(validate_shell_command("ls -la /tmp").is_ok());
        assert!(validate_shell_command("cat file.txt").is_ok());
    }

    #[test]
    fn test_validate_shell_command_rejects_chaining() {
        assert!(validate_shell_command("echo hello; rm -rf /").is_err());
        assert!(validate_shell_command("echo hello && rm -rf /").is_err());
        assert!(validate_shell_command("echo hello || rm -rf /").is_err());
        assert!(validate_shell_command("echo hello | cat").is_err());
    }

    #[test]
    fn test_validate_shell_command_rejects_substitution() {
        assert!(validate_shell_command("echo $(whoami)").is_err());
        assert!(validate_shell_command("echo `whoami`").is_err());
    }

    #[test]
    fn test_validate_shell_command_rejects_redirection() {
        assert!(validate_shell_command("echo hello > /tmp/out").is_err());
        assert!(validate_shell_command("echo hello >> /tmp/out").is_err());
        assert!(validate_shell_command("cat < /etc/passwd").is_err());
    }

    #[test]
    fn test_validate_shell_command_rejects_newlines() {
        assert!(validate_shell_command("echo hello\nrm -rf /").is_err());
    }

    // --- Shell command execution (F25 / F26) ---

    #[test]
    fn test_shell_commands_are_executed_when_allowed() {
        let executor = SkillExecutor::new();
        let mut skill = Skill::new(
            "sh".to_string(),
            "Sh".to_string(),
            "runs shell".to_string(),
            "OUT !`echo shannon-skill-exec-ok`".to_string(),
        );
        skill.source = crate::definition::SkillSource::User;

        let context = SkillContext {
            arguments: vec![],
            cwd: std::env::temp_dir(),
            session_id: "test".to_string(),
            effort_level: "medium".to_string(),
            permissions: SkillPermissions::default(),
        };

        let result = executor.execute(&skill, &context).unwrap();
        assert!(result.metadata.had_shell_commands);
        assert!(
            result.prompt_content.contains("shannon-skill-exec-ok"),
            "command output should be inlined, got: {}",
            result.prompt_content
        );
        assert!(!result.prompt_content.contains("!`"), "marker consumed");
    }

    /// F25 regression: command stdout is inert. A command whose output
    /// contains a `!`cmd`` pattern must have that pattern passed through
    /// literally — never re-scanned and executed. `\140` is printf's octal
    /// escape for a backtick, keeping the executed command itself free of
    /// backticks (which validate_shell_command rejects).
    #[test]
    fn test_shell_output_patterns_not_reexecuted() {
        let executor = SkillExecutor::new();
        let mut skill = Skill::new(
            "injection".to_string(),
            "Injection".to_string(),
            "output contains a nested pattern".to_string(),
            "!`printf 'A !\\140echo pwned\\140 B'`".to_string(),
        );
        skill.source = crate::definition::SkillSource::User;

        let context = SkillContext {
            arguments: vec![],
            cwd: std::env::temp_dir(),
            session_id: "test".to_string(),
            effort_level: "medium".to_string(),
            permissions: SkillPermissions::default(),
        };

        let result = executor.execute(&skill, &context).unwrap();
        // The nested pattern appears verbatim in the final content...
        assert!(
            result.prompt_content.contains("!`echo pwned`"),
            "nested pattern must pass through literally, got: {}",
            result.prompt_content
        );
        // ...and the nested command was never executed (single pass).
        assert_eq!(
            result.prompt_content, "A !`echo pwned` B",
            "command output must not be re-scanned"
        );
    }

    /// F26 regression: project-sourced skills default to allow_shell=false,
    /// so their `!`cmd`` blocks are left as literal text instead of running.
    #[test]
    fn test_project_skill_shell_blocks_not_executed() {
        let executor = SkillExecutor::new();
        let mut skill = Skill::new(
            "proj".to_string(),
            "Proj".to_string(),
            "project skill with shell".to_string(),
            "Run !`echo should-not-run` now".to_string(),
        );
        skill.source = crate::definition::SkillSource::Project;

        let context = SkillContext {
            arguments: vec![],
            cwd: std::env::temp_dir(),
            session_id: "test".to_string(),
            effort_level: "medium".to_string(),
            permissions: SkillPermissions::for_source(&skill.source),
        };

        let result = executor.execute(&skill, &context).unwrap();
        assert!(!result.metadata.had_shell_commands, "nothing may execute");
        assert_eq!(
            result.prompt_content, "Run !`echo should-not-run` now",
            "shell blocks stay as literal text"
        );
    }

    /// F26: user-domain skills keep shell execution (the default grants it),
    /// so the gate only engages for untrusted sources.
    #[test]
    fn test_user_skill_shell_blocks_still_execute() {
        let executor = SkillExecutor::new();
        let mut skill = Skill::new(
            "user".to_string(),
            "User".to_string(),
            "user skill with shell".to_string(),
            "!`echo user-shell-ok`".to_string(),
        );
        skill.source = crate::definition::SkillSource::User;

        let context = SkillContext {
            arguments: vec![],
            cwd: std::env::temp_dir(),
            session_id: "test".to_string(),
            effort_level: "medium".to_string(),
            permissions: SkillPermissions::for_source(&skill.source),
        };

        let result = executor.execute(&skill, &context).unwrap();
        assert!(result.metadata.had_shell_commands);
        assert!(result.prompt_content.contains("user-shell-ok"));
    }

    // --- Named argument substitution tests ---

    #[test]
    fn test_substitute_named_arguments_basic() {
        let executor = SkillExecutor::new();
        let names = vec!["issue".to_string(), "branch".to_string()];
        let values = vec!["42".to_string(), "fix-login".to_string()];
        let result = executor
            .substitute_named_arguments("Fix $issue on branch $branch", &names, &values)
            .unwrap();
        assert_eq!(result, "Fix 42 on branch fix-login");
    }

    #[test]
    fn test_substitute_named_arguments_fewer_values_than_names() {
        let executor = SkillExecutor::new();
        let names = vec!["issue".to_string(), "branch".to_string()];
        let values = vec!["42".to_string()];
        // $branch has no corresponding value, so it stays as-is
        let result = executor
            .substitute_named_arguments("Fix $issue on $branch", &names, &values)
            .unwrap();
        assert_eq!(result, "Fix 42 on $branch");
    }

    #[test]
    fn test_substitute_named_arguments_no_placeholders() {
        let executor = SkillExecutor::new();
        let names = vec!["issue".to_string()];
        let values = vec!["42".to_string()];
        let result = executor
            .substitute_named_arguments("No placeholders here", &names, &values)
            .unwrap();
        assert_eq!(result, "No placeholders here");
    }

    #[test]
    fn test_substitute_named_arguments_empty() {
        let executor = SkillExecutor::new();
        let names: Vec<String> = vec![];
        let values: Vec<String> = vec![];
        let result = executor
            .substitute_named_arguments("Hello $issue", &names, &values)
            .unwrap();
        assert_eq!(result, "Hello $issue");
    }

    #[test]
    fn test_named_arguments_via_execute_with_single_config() {
        use crate::frontmatter::ArgumentConfig;

        let executor = SkillExecutor::new();
        let mut skill = Skill::new(
            "checkout".to_string(),
            "Checkout".to_string(),
            "Checkout branch".to_string(),
            "Fix $issue on branch $branch".to_string(),
        );
        // Single variant with space-separated names
        skill.arguments = Some(ArgumentConfig::Single("issue branch".to_string()));

        let context = SkillContext {
            arguments: vec!["42".to_string(), "fix-login".to_string()],
            cwd: PathBuf::from("/tmp"),
            session_id: "test-session".to_string(),
            effort_level: "medium".to_string(),
            permissions: SkillPermissions::default(),
        };

        let result = executor.execute(&skill, &context).unwrap();
        assert_eq!(result.prompt_content, "Fix 42 on branch fix-login");
    }

    #[test]
    fn test_named_arguments_via_execute_with_multiple_config() {
        use crate::frontmatter::ArgumentConfig;

        let executor = SkillExecutor::new();
        let mut skill = Skill::new(
            "deploy".to_string(),
            "Deploy".to_string(),
            "Deploy service".to_string(),
            "Deploy $env with tag $tag".to_string(),
        );
        skill.arguments = Some(ArgumentConfig::Multiple(vec![
            "env".to_string(),
            "tag".to_string(),
        ]));

        let context = SkillContext {
            arguments: vec!["production".to_string(), "v2.1.0".to_string()],
            cwd: PathBuf::from("/tmp"),
            session_id: "test-session".to_string(),
            effort_level: "medium".to_string(),
            permissions: SkillPermissions::default(),
        };

        let result = executor.execute(&skill, &context).unwrap();
        assert_eq!(result.prompt_content, "Deploy production with tag v2.1.0");
    }

    // --- UserPromptExpansion hook emission tests ---

    /// Test sink that records every emitted event payload.
    #[derive(Default)]
    struct RecordingEmitter(std::sync::Mutex<Vec<Vec<u8>>>);

    impl HookEmitter for RecordingEmitter {
        fn emit(&self, event_json: Vec<u8>) {
            if let Ok(mut g) = self.0.lock() {
                g.push(event_json);
            }
        }
    }

    #[test]
    fn test_user_prompt_expansion_emitted_when_wired() {
        let recorder = std::sync::Arc::new(RecordingEmitter::default());
        let recorder_clone: std::sync::Arc<RecordingEmitter> = recorder.clone();
        let emitter: Box<dyn HookEmitter> = Box::new(RecordingAdapter(recorder_clone));
        let executor = SkillExecutor::new().with_hook_emitter(emitter);

        let skill = Skill::new(
            "demo".to_string(),
            "Demo".to_string(),
            "demo skill".to_string(),
            "Hello ${0}".to_string(),
        );
        let context = SkillContext {
            arguments: vec!["World".to_string()],
            cwd: PathBuf::from("/tmp"),
            session_id: "sess-1".to_string(),
            effort_level: "medium".to_string(),
            permissions: SkillPermissions::default(),
        };

        executor.execute(&skill, &context).unwrap();

        let events = recorder.0.lock().unwrap();
        assert_eq!(events.len(), 1, "expected exactly one hook event");
        let parsed: serde_json::Value = serde_json::from_slice(&events[0]).unwrap();
        let inner = parsed.get("UserPromptExpansion").expect("event tag");
        assert_eq!(inner.get("original_prompt").unwrap(), "Hello ${0}");
        assert_eq!(inner.get("expanded_prompt").unwrap(), "Hello World");
    }

    /// Thin adapter so we can box an `Arc<RecordingEmitter>` into a
    /// `Box<dyn HookEmitter>` without cloning the inner state.
    struct RecordingAdapter(std::sync::Arc<RecordingEmitter>);
    impl HookEmitter for RecordingAdapter {
        fn emit(&self, event_json: Vec<u8>) {
            self.0.emit(event_json);
        }
    }

    #[test]
    fn test_no_emit_when_emitter_not_wired() {
        // Default executor must NOT panic or call any external code path.
        let executor = SkillExecutor::new();
        let skill = Skill::new(
            "demo".to_string(),
            "Demo".to_string(),
            "demo skill".to_string(),
            "Hello ${0}".to_string(),
        );
        let context = SkillContext {
            arguments: vec!["World".to_string()],
            cwd: PathBuf::from("/tmp"),
            session_id: "sess-1".to_string(),
            effort_level: "medium".to_string(),
            permissions: SkillPermissions::default(),
        };

        // Just verify it returns successfully.
        let result = executor.execute(&skill, &context).unwrap();
        assert_eq!(result.prompt_content, "Hello World");
    }
}
