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

/// Security level for command execution
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SecurityLevel {
    /// Safe operations (read-only, informational)
    Safe = 0,
    /// Low risk (write to user directories, git operations)
    Low = 1,
    /// Medium risk (package installation, system config)
    Medium = 2,
    /// High risk (file deletion, system modifications)
    High = 3,
    /// Critical (data destruction, system compromise)
    Critical = 4,
}

/// Security analysis result
#[derive(Debug, Clone)]
pub struct SecurityAnalysis {
    pub risk_level: SecurityLevel,
    pub warnings: Vec<String>,
    pub is_destructive: bool,
    pub is_read_only: bool,
    pub contains_path_traversal: bool,
    pub requires_confirmation: bool,
}

/// Dangerous command patterns that should trigger warnings
const DESTRUCTIVE_PATTERNS: &[&str] = &[
    "rm -rf /",           // Delete root filesystem
    "rm -rf /*",          // Delete all files
    ":>",                  // Zero out files
    "dd if=/dev/zero",    // Disk destruction
    "mkfs",               // Format filesystem
    "fdisk",              // Partition manipulation
    "shutdown",           // System shutdown
    "reboot",             // System reboot
    "init 0",             // Switch to runlevel 0
    "kill -9",            // Force kill processes
    "chmod 000",          // Remove all permissions
];

/// Confirmation-required patterns
const CONFIRMATION_PATTERNS: &[&str] = &[
    "rm -rf",             // Recursive force delete
    "del /q",             // Windows quiet delete
    "format",             // Windows format
    "shred",              // Secure delete
];

/// Path traversal patterns
const PATH_TRAVERSAL_PATTERNS: &[&str] = &[
    "../",                  // Parent directory traversal
    "./../",                // Multiple parent traversal
    "~/../",                // From home parent traversal
    "/../",                 // Root parent traversal
    "..\\",                 // Windows-style traversal
];

/// Sed injection patterns (command injection through sed)
const SED_INJECTION_PATTERNS: &[&str] = &[
    "sed.*e.*;",          // Command execution via sed
    "sed.*s/.*/[command]",  // Replace with command
    "sed.*y/.*/[command]",  // Translate with command
    "|.*sh",               // Pipe to shell
    "|.*bash",             // Pipe to bash
    "|.*python",           // Pipe to python
    ";.*rm",               // Command chaining
    "&.*rm",               // Background command chaining
    "`.*rm",               // Backtick execution
    "$(*rm",               // Command substitution
];

/// Read-only command patterns
const READ_ONLY_PATTERNS: &[&str] = &[
    "ls", "ll", "la",       // List operations
    "cat", "head", "tail",  // File reading
    "grep", "egrep", "fgrep", // Search
    "find", "locate",      // File search
    "file", "stat", "du",    // File info
    "echo", "pwd", "whoami", // System info
    "git status", "git log", // Git read ops
    "git diff",            // Git diff
];

/// PowerShell-specific destructive patterns
const PS_DESTRUCTIVE_PATTERNS: &[&str] = &[
    "Remove-Item -Recurse -Force",  // Recursive force delete
    "rm -Recurse -Force",           // Alias recursive delete
    "ri -Recurse -Force",           // Alias recursive delete
    "Remove-Item * -Recurse",       // Delete all in dir
    "Format-Volume",                // Format volume
    "Stop-Computer",                // Shutdown
    "Restart-Computer",             // Reboot
    "Clear-Content",                // Clear file contents
    "Remove-Service",               // Remove service
    "Set-ExecutionPolicy Unrestricted", // Lower security policy
    "Invoke-WebRequest | Invoke-Expression", // Download & execute
    "iex",                          // Invoke-Expression (code execution)
    "IEX",                          // Invoke-Expression variant
    "Invoke-Expression",            // Direct code execution
    "& 'cmd.exe /c'",               // Cmd bypass
    "Start-Process -Verb RunAs",    // Privilege escalation
    "net user",                     // User manipulation
    "net localgroup",               // Group manipulation
    "reg delete",                   // Registry deletion
    "reg add HKLM",                 // Registry modification (system)
];

/// PowerShell confirmation-required patterns
const PS_CONFIRMATION_PATTERNS: &[&str] = &[
    "Remove-Item",          // Delete files
    "Move-Item",            // Move files
    "Copy-Item -Force",     // Force copy
    "Set-Content",          // Overwrite file content
    "New-Item -Force",      // Force create
    "Stop-Process",         // Kill process
    "taskkill",             // Kill process
];

/// Analyze a bash command for security risks
pub fn analyze_command_security(command: &str) -> SecurityAnalysis {
    let mut warnings = Vec::new();
    let mut risk_level = SecurityLevel::Safe;
    let mut is_destructive = false;
    let mut is_read_only = false;
    let mut contains_path_traversal = false;
    let mut requires_confirmation = false;

    let lower_command = command.to_lowercase();

    // Check for destructive patterns
    for pattern in DESTRUCTIVE_PATTERNS {
        if lower_command.contains(pattern) {
            risk_level = SecurityLevel::Critical;
            is_destructive = true;
            warnings.push(format!("Destructive pattern detected: {pattern}"));
            break;
        }
    }

    // Check for confirmation-required patterns
    for pattern in CONFIRMATION_PATTERNS {
        if lower_command.contains(pattern) {
            if risk_level < SecurityLevel::High {
                risk_level = SecurityLevel::High;
            }
            is_destructive = true;
            requires_confirmation = true;
            warnings.push(format!("Confirmation required: {pattern}"));
        }
    }

    // Check for path traversal
    for pattern in PATH_TRAVERSAL_PATTERNS {
        if lower_command.contains(pattern) {
            contains_path_traversal = true;
            if risk_level < SecurityLevel::Medium {
                risk_level = SecurityLevel::Medium;
            }
            warnings.push(format!("Path traversal pattern detected: {pattern}"));
            // Don't break, collect all warnings
        }
    }

    // Check for sed injection
    for pattern in SED_INJECTION_PATTERNS {
        if lower_command.contains(pattern) {
            risk_level = SecurityLevel::Critical;
            warnings.push(format!("Sed injection pattern detected: {pattern}"));
            break;
        }
    }

    // Check if read-only
    for pattern in READ_ONLY_PATTERNS {
        if lower_command.starts_with(pattern) || lower_command.contains(&format!(" {pattern}")) {
            is_read_only = true;
            // Read-only commands are safe unless already marked risky
            if risk_level == SecurityLevel::Safe {
                risk_level = SecurityLevel::Low;
            }
            break;
        }
    }

    // Additional heuristic: commands with sudo are higher risk
    if lower_command.starts_with("sudo ") {
        if risk_level < SecurityLevel::Medium {
            risk_level = SecurityLevel::Medium;
        }
        warnings.push("Elevated privileges requested (sudo)".to_string());
    }

    // Pipe chains are medium risk
    if command.contains('|') && !is_read_only
        && risk_level < SecurityLevel::Medium {
            risk_level = SecurityLevel::Medium;
        }

    // Redirects that overwrite files are medium risk
    if command.contains(">") && !is_read_only
        && risk_level < SecurityLevel::Medium {
            risk_level = SecurityLevel::Medium;
        }

    SecurityAnalysis {
        risk_level,
        warnings,
        is_destructive,
        is_read_only,
        contains_path_traversal,
        requires_confirmation,
    }
}

/// Validate a path is safe for execution
pub fn validate_path(path: &str, allowed_paths: &[String]) -> Result<(), String> {
    // Normalize the path
    let normalized = if path.starts_with('~') {
        // Expand home directory (simplified)
        if let Ok(home) = std::env::var("HOME") {
            path.replacen('~', &home, 1)
        } else {
            path.to_string()
        }
    } else if path.starts_with('.') {
        // Resolve relative path against current directory
        if let Ok(current) = std::env::current_dir() {
            current.join(path).to_string_lossy().to_string()
        } else {
            path.to_string()
        }
    } else {
        path.to_string()
    };

    // Check for path traversal in normalized path
    for pattern in PATH_TRAVERSAL_PATTERNS {
        if normalized.contains(pattern) {
            return Err(format!("Path traversal detected in: {path}"));
        }
    }

    // Check against allowed paths if provided
    if !allowed_paths.is_empty() {
        let is_allowed = allowed_paths.iter().any(|allowed| {
            normalized.starts_with(allowed) || normalized == *allowed
        });

        if !is_allowed {
            return Err(format!("Path not in allowed list: {path}"));
        }
    }

    // Check for dangerous system paths
    let dangerous_prefixes = &[
        "/bin/", "/sbin/", "/usr/bin/", "/usr/sbin/",
        "/etc/", "/boot/", "/sys/", "/dev/",
        "/proc/", "/root/", "/var/run/",
    ];

    for prefix in dangerous_prefixes {
        if normalized.starts_with(prefix) {
            // Only allow read operations on system paths
            return Err(format!("System path modification blocked: {path}"));
        }
    }

    Ok(())
}

/// Get a human-readable description of the risk level
pub fn describe_risk_level(level: SecurityLevel) -> &'static str {
    match level {
        SecurityLevel::Safe => "✓ Safe - Read-only or informational",
        SecurityLevel::Low => "⚠ Low Risk - File operations in user space",
        SecurityLevel::Medium => "⚡ Medium Risk - System modifications or multi-step operations",
        SecurityLevel::High => "🔥 High Risk - Destructive operations",
        SecurityLevel::Critical => "☢️ Critical - Data destruction or system compromise",
    }
}

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

impl Default for BashTool {
    fn default() -> Self {
        Self::new()
    }
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
                .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, format!("Command timed out after {timeout}ms")))?
                .map_err(|e| std::io::Error::other(format!("Failed to execute command: {e}")))?
        } else {
            cmd.output()
                .await
                .map_err(|e| std::io::Error::other(format!("Failed to execute command: {e}")))?
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

    async fn execute(&self, input: serde_json::Value) -> Result<ToolOutput, shannon_core::tools::ToolError> {
        let bash_input: BashInput = match serde_json::from_value(input) {
            Ok(input) => input,
            Err(e) => return Ok(ToolOutput {
                content: format!("Invalid bash input: {e}"),
                is_error: true,
                metadata: HashMap::new(),
            })
        };

        // Perform security analysis before execution
        let analysis = analyze_command_security(&bash_input.command);

        // Reject critical risk commands
        if analysis.risk_level >= SecurityLevel::Critical {
            let error_msg = format!(
                "Command rejected due to critical security risk:\n{}\n\nRisk Level: {}\n\nWarnings:\n  - {}",
                bash_input.command,
                describe_risk_level(analysis.risk_level),
                analysis.warnings.join("\n  - ")
            );
            return Ok(ToolOutput {
                content: error_msg,
                is_error: true,
                metadata: {
                    let mut map = HashMap::new();
                    map.insert("security_rejected".to_string(), json!(true));
                    map.insert("risk_level".to_string(), json!(analysis.risk_level as i32));
                    map.insert("warnings".to_string(), json!(analysis.warnings));
                    map
                },
            });
        }

        // For medium/high risk commands, add security warnings to the output
        let command_description = if analysis.risk_level >= SecurityLevel::Medium {
            format!(
                "\n[SECURITY WARNING]\nRisk: {}\nCommand: {}\nWarnings:\n  - {}\n",
                describe_risk_level(analysis.risk_level),
                bash_input.command,
                analysis.warnings.join("\n  - ")
            )
        } else {
            String::new()
        };

        // Execute the command
        let output_result = Self::execute_command(
            &bash_input.command,
            bash_input.cwd.as_deref(),
            bash_input.env.as_ref(),
            bash_input.timeout,
        )
        .await;

        let output = match output_result {
            Ok(output) => output,
            Err(e) => {
                return Ok(ToolOutput {
                    content: format!("Command execution failed: {e}"),
                    is_error: true,
                    metadata: HashMap::new(),
                });
            }
        };

        let content = if output.success {
            format!("{}{}", output.stdout, command_description)
        } else {
            format!("{}Command failed with exit code {}: {}{}",
                command_description,
                output.exit_code,
                output.stderr,
                if command_description.is_empty() { "\n" } else { "" })
        };

        Ok(ToolOutput {
            content,
            is_error: !output.success,
            metadata: {
                let mut map = HashMap::new();
                map.insert("exit_code".to_string(), json!(output.exit_code));
                map.insert("risk_level".to_string(), json!(analysis.risk_level as i32));
                map.insert("is_destructive".to_string(), json!(analysis.is_destructive));
                map.insert("is_read_only".to_string(), json!(analysis.is_read_only));
                if !analysis.warnings.is_empty() {
                    map.insert("warnings".to_string(), json!(analysis.warnings));
                }
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

impl Default for PowerShellTool {
    fn default() -> Self {
        Self::new()
    }
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
                .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, format!("Command timed out after {timeout}ms")))?
                .map_err(|e| std::io::Error::other(format!("Failed to execute command: {e}")))?
        } else {
            cmd.output()
                .await
                .map_err(|e| std::io::Error::other(format!("Failed to execute command: {e}")))?
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
            .map_err(|e| ToolError::InvalidInput(format!("Invalid PowerShell input: {e}")))?;

        // PowerShell security analysis
        let lower_cmd = ps_input.command.to_lowercase();

        // Check destructive patterns - reject immediately
        for pattern in PS_DESTRUCTIVE_PATTERNS {
            if lower_cmd.contains(&pattern.to_lowercase()) {
                return Ok(ToolOutput {
                    content: format!(
                        "PowerShell command rejected due to critical security risk:\n{}\n\nPattern: {}",
                        ps_input.command, pattern
                    ),
                    is_error: true,
                    metadata: {
                        let mut map = std::collections::HashMap::new();
                        map.insert("security_rejected".to_string(), json!(true));
                        map.insert("pattern".to_string(), json!(pattern));
                        map
                    },
                });
            }
        }

        // Check confirmation-required patterns
        for pattern in PS_CONFIRMATION_PATTERNS {
            if lower_cmd.contains(&pattern.to_lowercase()) {
                return Ok(ToolOutput {
                    content: format!(
                        "PowerShell command requires confirmation:\n{}\n\nPattern: {}\nUse with explicit approval only.",
                        ps_input.command, pattern
                    ),
                    is_error: true,
                    metadata: {
                        let mut map = std::collections::HashMap::new();
                        map.insert("requires_confirmation".to_string(), json!(true));
                        map.insert("pattern".to_string(), json!(pattern));
                        map
                    },
                });
            }
        }

        let output = Self::execute_command(
            &ps_input.command,
            ps_input.cwd.as_deref(),
            ps_input.env.as_ref(),
            ps_input.timeout,
        )
        .await
        .map_err(|e| ToolError::ExecutionFailed(format!("Command failed: {e}")))?;

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

impl Default for SleepTool {
    fn default() -> Self {
        Self::new()
    }
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
            stdout: format!("Slept for {duration_ms}ms"),
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
            .map_err(|e| ToolError::InvalidInput(format!("Invalid sleep input: {e}")))?;

        // Validate duration is reasonable
        if sleep_input.duration_ms > 3600000 {
            return Err(ToolError::InvalidInput(
                "Duration too long (max 1 hour / 3600000ms)".to_string(),
            ));
        }

        let output = self.execute_sleep(sleep_input.duration_ms).await
            .map_err(|e| ToolError::ExecutionFailed(format!("Sleep failed: {e}")))?;

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
