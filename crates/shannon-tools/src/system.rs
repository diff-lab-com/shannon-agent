//! System operation tools
//!
//! Provides implementations for:
//! - Bash: Execute shell commands on Unix-like systems
//! - PowerShell: Execute commands on Windows systems

use crate::{Tool, ToolError, ToolResult, ToolOutput};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::process::Stdio;
use tokio::process::Command;

/// Shell command types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "shell_type")]
pub enum ShellCommand {
    Bash(BashInput),
    PowerShell(PowerShellInput),
}

/// Bash/Unix shell command input
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BashInput {
    /// Command to execute
    pub command: String,

    /// Optional working directory
    pub cwd: Option<String>,

    /// Optional timeout in milliseconds
    pub timeout: Option<u64>,

    /// Optional environment variables
    pub env: Option<std::collections::HashMap<String, String>>,
}

/// PowerShell command input
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PowerShellInput {
    /// Command to execute
    pub command: String,

    /// Optional working directory
    pub cwd: Option<String>,

    /// Optional timeout in milliseconds
    pub timeout: Option<u64>,

    /// Optional environment variables
    pub env: Option<std::collections::HashMap<String, String>>,
}

/// Command execution output
#[derive(Debug, Serialize)]
pub struct CommandOutput {
    /// Standard output from command
    pub stdout: String,

    /// Standard error from command
    pub stderr: String,

    /// Exit code (0 = success)
    pub exit_code: i32,

    /// Whether command completed successfully
    pub success: bool,
}

/// Bash tool implementation
pub struct BashTool {
    description: String,
}

impl BashTool {
    pub fn new() -> Self {
        Self {
            description: "Executes bash commands and returns output".to_string(),
        }
    }

    async fn execute_command(
        command: &str,
        cwd: Option<&str>,
        env: Option<&std::collections::HashMap<String, String>>,
        timeout_ms: Option<u64>,
    ) -> Result<CommandOutput, std::io::Error> {
        let mut cmd = Command::new("bash");
        cmd.arg("-c")
            .arg(command)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }

        if let Some(env_vars) = env {
            for (key, value) in env_vars {
                cmd.env(key, value);
            }
        }

        // Execute with timeout if specified
        let output = if let Some(timeout) = timeout_ms {
            let duration = std::time::Duration::from_millis(timeout);
            tokio::time::timeout(duration, cmd.output())
                .await
                .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, format!("Command timed out after {}ms", timeout)))?
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("Failed to execute command: {}", e)))?
        } else {
            cmd.output()
                .await
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("Failed to execute command: {}", e)))?
        };

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let exit_code = output.status.code().unwrap_or(-1);
        let success = output.status.success();

        Ok(CommandOutput {
            stdout,
            stderr,
            exit_code,
            success,
        })
    }
}

#[async_trait]
impl Tool for BashTool {
    fn name(&self) -> &str {
        "Bash"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The bash command to execute"
                },
                "cwd": {
                    "type": "string",
                    "description": "Optional working directory"
                },
                "timeout": {
                    "type": "integer",
                    "description": "Optional timeout in milliseconds"
                },
                "env": {
                    "type": "object",
                    "description": "Optional environment variables"
                }
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, input: serde_json::Value) -> ToolResult<ToolOutput> {
        let bash_input: BashInput = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("Invalid bash input: {}", e)))?;

        let output = Self::execute_command(
            &bash_input.command,
            bash_input.cwd.as_deref(),
            bash_input.env.as_ref(),
            bash_input.timeout,
        )
        .await
        .map_err(|e| ToolError::ExecutionFailed(format!("Command failed: {}", e)))?;

        let content = if output.success {
            output.stdout
        } else {
            format!("Command failed with exit code {}: {}", output.exit_code, output.stderr)
        };

        Ok(ToolOutput {
            content,
            is_error: !output.success,
            metadata: {
                let mut map = HashMap::new();
                map.insert("exit_code".to_string(), json!(output.exit_code));
                if !output.stderr.is_empty() {
                    map.insert("stderr".to_string(), json!(output.stderr));
                }
                map
            },
        })
    }
}

/// PowerShell tool implementation
pub struct PowerShellTool {
    description: String,
}

impl PowerShellTool {
    pub fn new() -> Self {
        Self {
            description: "Executes PowerShell commands and returns output".to_string(),
        }
    }

    async fn execute_command(
        command: &str,
        cwd: Option<&str>,
        env: Option<&std::collections::HashMap<String, String>>,
        timeout_ms: Option<u64>,
    ) -> Result<CommandOutput, std::io::Error> {
        let mut cmd = Command::new("powershell");
        cmd.arg("-Command")
            .arg(command)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }

        if let Some(env_vars) = env {
            for (key, value) in env_vars {
                cmd.env(key, value);
            }
        }

        // Execute with timeout if specified
        let output = if let Some(timeout) = timeout_ms {
            let duration = std::time::Duration::from_millis(timeout);
            tokio::time::timeout(duration, cmd.output())
                .await
                .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, format!("Command timed out after {}ms", timeout)))?
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("Failed to execute command: {}", e)))?
        } else {
            cmd.output()
                .await
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("Failed to execute command: {}", e)))?
        };

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let exit_code = output.status.code().unwrap_or(-1);
        let success = output.status.success();

        Ok(CommandOutput {
            stdout,
            stderr,
            exit_code,
            success,
        })
    }
}

#[async_trait]
impl Tool for PowerShellTool {
    fn name(&self) -> &str {
        "PowerShell"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The PowerShell command to execute"
                },
                "cwd": {
                    "type": "string",
                    "description": "Optional working directory"
                },
                "timeout": {
                    "type": "integer",
                    "description": "Optional timeout in milliseconds"
                },
                "env": {
                    "type": "object",
                    "description": "Optional environment variables"
                }
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, input: serde_json::Value) -> ToolResult<ToolOutput> {
        let ps_input: PowerShellInput = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("Invalid PowerShell input: {}", e)))?;

        let output = Self::execute_command(
            &ps_input.command,
            ps_input.cwd.as_deref(),
            ps_input.env.as_ref(),
            ps_input.timeout,
        )
        .await
        .map_err(|e| ToolError::ExecutionFailed(format!("Command failed: {}", e)))?;

        let content = if output.success {
            output.stdout
        } else {
            format!("Command failed with exit code {}: {}", output.exit_code, output.stderr)
        };

        Ok(ToolOutput {
            content,
            is_error: !output.success,
            metadata: {
                let mut map = std::collections::HashMap::new();
                map.insert("exit_code".to_string(), json!(output.exit_code));
                if !output.stderr.is_empty() {
                    map.insert("stderr".to_string(), json!(output.stderr));
                }
                map
            },
        })
    }
}

/// System tool enum for unified interface
pub enum SystemTool {
    Bash(BashTool),
    PowerShell(PowerShellTool),
    Sleep(SleepTool),
}

impl SystemTool {
    pub fn from_platform() -> Self {
        #[cfg(target_os = "windows")]
        return SystemTool::PowerShell(PowerShellTool::new());

        #[cfg(not(target_os = "windows"))]
        return SystemTool::Bash(BashTool::new());
    }

    pub fn sleep() -> Self {
        SystemTool::Sleep(SleepTool::new())
    }
}

/// Input for sleep operation
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SleepInput {
    /// Duration to sleep in milliseconds
    pub duration_ms: u64,
}

/// Sleep tool for waiting a specified duration
#[derive(Debug)]
pub struct SleepTool {
    description: String,
}

impl SleepTool {
    pub fn new() -> Self {
        Self {
            description: "Wait for a specified duration without holding a shell process".to_string(),
        }
    }

    pub async fn execute_sleep(&self, duration_ms: u64) -> Result<CommandOutput, std::io::Error> {
        tokio::time::sleep(tokio::time::Duration::from_millis(duration_ms)).await;

        Ok(CommandOutput {
            stdout: format!("Slept for {}ms", duration_ms),
            stderr: String::new(),
            exit_code: 0,
            success: true,
        })
    }
}

#[async_trait]
impl Tool for SleepTool {
    fn name(&self) -> &str {
        "Sleep"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "duration_ms": {
                    "type": "integer",
                    "description": "Duration to sleep in milliseconds (max 3600000 = 1 hour)"
                }
            },
            "required": ["duration_ms"]
        })
    }

    async fn execute(&self, input: serde_json::Value) -> ToolResult<ToolOutput> {
        let sleep_input: SleepInput = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("Invalid sleep input: {}", e)))?;

        // Validate duration is reasonable
        if sleep_input.duration_ms > 3600000 {
            return Err(ToolError::InvalidInput(
                "Duration too long (max 1 hour / 3600000ms)".to_string(),
            ));
        }

        let output = self.execute_sleep(sleep_input.duration_ms).await
            .map_err(|e| ToolError::ExecutionFailed(format!("Sleep failed: {}", e)))?;

        Ok(ToolOutput {
            content: output.stdout,
            is_error: false,
            metadata: {
                let mut map = std::collections::HashMap::new();
                map.insert("duration_ms".to_string(), json!(sleep_input.duration_ms));
                map
            },
        })
    }
}
