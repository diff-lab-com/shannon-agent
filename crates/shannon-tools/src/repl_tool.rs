//! REPL Tool
//!
//! Wraps primitive tool calls allowing batch operations through a single tool invocation.
//! Used for optimized workflows where multiple operations can be combined.
//!
//! Executes commands via `bash -c` and captures stdout, stderr, and exit code.

use crate::{Tool, ToolError, ToolResult, ToolOutput};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;

pub const REPL_TOOL_NAME: &str = "REPL";

/// Input for the REPL tool.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReplInput {
    /// The raw command to execute in the REPL context.
    pub command: String,
    /// Optional working directory for the command.
    pub cwd: Option<String>,
    /// Optional environment variables to pass to the command.
    pub env: Option<HashMap<String, String>>,
}

/// Output from a REPL command execution.
#[derive(Debug, Clone, Serialize)]
pub struct ReplOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

/// REPL tool implementation.
///
/// Executes shell commands via `bash -c`, capturing output and exit status.
/// Supports custom working directories and environment variables.
pub struct ReplTool;

impl ReplTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ReplTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for ReplTool {
    fn name(&self) -> &str {
        REPL_TOOL_NAME
    }

    fn description(&self) -> &str {
        "Execute a REPL command that wraps primitive tool calls for batch operations"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The REPL command to execute"
                },
                "cwd": {
                    "type": "string",
                    "description": "Optional working directory"
                },
                "env": {
                    "type": "object",
                    "description": "Optional environment variables",
                    "additionalProperties": { "type": "string" }
                }
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, input: Value) -> ToolResult<ToolOutput> {
        let repl_input: ReplInput = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("Invalid REPL input: {}", e)))?;

        use std::process::Stdio;
        use tokio::process::Command;

        let mut cmd = Command::new("bash");
        cmd.arg("-c")
            .arg(&repl_input.command)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if let Some(ref cwd) = repl_input.cwd {
            cmd.current_dir(cwd);
        }

        if let Some(ref env) = repl_input.env {
            for (k, v) in env {
                cmd.env(k, v);
            }
        }

        let output = cmd
            .output()
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("REPL command failed: {}", e)))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let exit_code = output.status.code().unwrap_or(-1);

        let success = output.status.success();

        Ok(ToolOutput {
            content: if success {
                stdout.clone()
            } else {
                format!(
                    "REPL command failed (exit {}): {}",
                    exit_code, stderr
                )
            },
            is_error: !success,
            metadata: {
                let mut m = HashMap::new();
                m.insert("exit_code".to_string(), json!(exit_code));
                if !stderr.is_empty() {
                    m.insert("stderr".to_string(), json!(stderr));
                }
                if !stdout.is_empty() {
                    m.insert("stdout".to_string(), json!(stdout));
                }
                m
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_basic_command() {
        let tool = ReplTool::new();
        let input = json!({
            "command": "echo hello world"
        });

        let result = tool.execute(input).await;
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(!output.is_error);
        assert!(output.content.contains("hello world"));
        assert_eq!(output.metadata.get("exit_code").unwrap(), 0);
    }

    #[tokio::test]
    async fn test_with_cwd() {
        let tool = ReplTool::new();
        let tmp = tempfile::TempDir::new().unwrap();
        let tmp_path = tmp.path().to_string_lossy().to_string();

        let input = json!({
            "command": "pwd",
            "cwd": tmp_path
        });

        let result = tool.execute(input).await;
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(!output.is_error);
        assert!(output.content.contains(tmp.path().file_name().unwrap().to_str().unwrap()));
    }

    #[tokio::test]
    async fn test_with_env() {
        let tool = ReplTool::new();
        let input = json!({
            "command": "echo $MY_TEST_VAR",
            "env": {
                "MY_TEST_VAR": "repl_test_value"
            }
        });

        let result = tool.execute(input).await;
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(!output.is_error);
        assert!(output.content.contains("repl_test_value"));
    }

    #[tokio::test]
    async fn test_failed_command() {
        let tool = ReplTool::new();
        let input = json!({
            "command": "exit 42"
        });

        let result = tool.execute(input).await;
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(output.is_error);
        assert_eq!(output.metadata.get("exit_code").unwrap(), 42);
        assert!(output.content.contains("failed"));
    }

    #[tokio::test]
    async fn test_empty_command() {
        let tool = ReplTool::new();
        let input = json!({
            "command": ""
        });

        let result = tool.execute(input).await;
        // Empty command succeeds in bash with exit code 0
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(!output.is_error);
        assert_eq!(output.metadata.get("exit_code").unwrap(), 0);
    }
}
