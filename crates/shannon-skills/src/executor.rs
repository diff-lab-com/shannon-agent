//! Skill execution engine

use crate::definition::{Skill, SkillContext};
use crate::error::{SkillError, SkillResult};
use crate::definition::SkillResult as SkillExecutionResult;
use regex::Regex;
use std::path::Path;
use tracing::debug;

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
    pub fn execute(&self, skill: &Skill, context: &SkillContext) -> SkillResult<SkillExecutionResult> {
        let start = std::time::Instant::now();

        // Start with the skill content
        let mut content = skill.content.clone();

        // Add base directory prefix if applicable
        if let Some(skill_root) = &skill.skill_root {
            let prefix = format!("Base directory for this skill: {}\n\n", skill_root.display());
            content = prefix + &content;
        }

        // Substitute arguments
        content = self.substitute_arguments(&content, &context.arguments)?;

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

        // ${0}, ${1}, etc. - indexed arguments
        for (i, arg) in args.iter().enumerate() {
            let placeholder = format!("${{{}}}", i);
            result = result.replace(&placeholder, arg);
        }

        // ${args} - all arguments joined by space
        let all_args = args.join(" ");
        result = result.replace("${args}", &all_args);

        // ${args:quote} - all arguments shell-quoted
        let quoted_args = args.iter()
            .map(|a| shell_words::quote(a))
            .collect::<Vec<_>>()
            .join(" ");
        result = result.replace("${args:quote}", &quoted_args);

        Ok(result)
    }

    /// Substitute environment variables
    fn substitute_variables(&self, content: &str, context: &SkillContext) -> SkillResult<String> {
        let mut result = content.to_string();

        // ${CLAUDE_SESSION_ID}
        result = result.replace("${CLAUDE_SESSION_ID}", &context.session_id);

        // ${CLAUDE_SKILL_DIR}
        if let Some(skill_root) = &context.cwd.parent() {
            result = result.replace("${CLAUDE_SKILL_DIR}", &skill_root.display().to_string());
        }

        // ${CWD}
        result = result.replace("${CWD}", &context.cwd.display().to_string());

        Ok(result)
    }

    /// Execute shell commands in the content
    fn execute_shell_commands(&self, content: &mut String, context: &SkillContext) -> SkillResult<bool> {
        let Some(executor) = &self.shell_executor else {
            return Ok(false);
        };

        // Pattern for shell commands: !`command` or ```!\ncommand\n```
        let inline_pattern = Regex::new(r"!`([^`]+)`").unwrap();
        let block_pattern = Regex::new(r"```!\n(.+?)\n```").unwrap();

        let mut had_commands = false;

        // Execute inline commands
        while inline_pattern.is_match(content) {
            had_commands = true;
            *content = inline_pattern.replace_all(content, |caps: &regex::Captures| {
                let cmd = &caps[1];
                match executor.execute(cmd, &context.cwd) {
                    Ok(output) => output,
                    Err(e) => format!("[Command failed: {}]", e),
                }
            }).to_string();
        }

        // Execute block commands
        while block_pattern.is_match(content) {
            had_commands = true;
            *content = block_pattern.replace_all(content, |caps: &regex::Captures| {
                let cmd = &caps[1];
                match executor.execute(cmd, &context.cwd) {
                    Ok(output) => output,
                    Err(e) => format!("[Command failed: {}]", e),
                }
            }).to_string();
        }

        Ok(had_commands)
    }
}

/// Shell command executor
pub struct ShellExecutor {
    /// Environment variables for commands
    env: std::collections::HashMap<String, String>,
}

impl ShellExecutor {
    /// Create a new shell executor
    pub fn new() -> Self {
        Self {
            env: std::collections::HashMap::new(),
        }
    }

    /// Execute a shell command and return its output
    pub fn execute(&self, command: &str, cwd: &Path) -> SkillResult<String> {
        debug!("Executing shell command: {}", command);

        let output = std::process::Command::new("sh")
            .arg("-c")
            .arg(command)
            .current_dir(cwd)
            .envs(&self.env)
            .output()
            .map_err(|e| SkillError::ExecutionFailed {
                name: "shell".to_string(),
                message: format!("Failed to execute command: {}", e),
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(SkillError::ExecutionFailed {
                name: "shell".to_string(),
                message: format!("Command failed: {}", stderr),
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
            permissions: SkillPermissions::default(),
        };

        let result = executor.execute(&skill, &context).unwrap();
        assert_eq!(result.prompt_content, "Hello World!");
    }
}
