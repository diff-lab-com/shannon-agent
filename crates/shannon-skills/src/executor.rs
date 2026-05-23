//! Skill execution engine

use crate::definition::SkillResult as SkillExecutionResult;
use crate::definition::{Skill, SkillContext};
use crate::error::{SkillError, SkillResult};
use regex::Regex;
use std::path::Path;
use std::sync::OnceLock;
use tracing::debug;

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

/// Engine for executing skills and generating prompt content
pub struct SkillExecutor {
    /// Shell command executor
    shell_executor: Option<ShellExecutor>,
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
        }
    }

    /// Execute a skill with the given context
    pub fn execute(
        &self,
        skill: &Skill,
        context: &SkillContext,
    ) -> SkillResult<SkillExecutionResult> {
        let start = std::time::Instant::now();

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

        // Execute shell commands if allowed
        let had_shell = if skill.source != crate::definition::SkillSource::Mcp
            && context.permissions.allow_shell
        {
            self.execute_shell_commands(&mut content, context)?
        } else {
            false
        };

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

        let mut had_commands = false;

        // Execute inline commands
        while inline_pattern.is_match(content) {
            had_commands = true;
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

        // Execute block commands
        while block_pattern.is_match(content) {
            had_commands = true;
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

        Ok(had_commands)
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
}
