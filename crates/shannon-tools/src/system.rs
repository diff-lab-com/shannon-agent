//! System operation tools
//!
//! Provides implementations for:
//! - Bash: Execute shell commands on Unix-like systems
//! - PowerShell: Execute commands on Windows systems

use crate::{Tool, ToolError, ToolResult, ToolOutput, BoxedProgressSender};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use shannon_core::sandbox::{SandboxConfig, SandboxExecutor, SandboxType};
use std::collections::HashMap;
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

/// Errors that can occur during path validation.
#[derive(Debug, thiserror::Error)]
pub enum PathValidationError {
    /// The path contains a traversal pattern (e.g. `../`).
    #[error("Path traversal detected in: {0}")]
    Traversal(String),
    /// The path is not in the configured allow-list.
    #[error("Path not in allowed list: {0}")]
    NotAllowed(String),
    /// The path points to a protected system directory.
    #[error("System path modification blocked: {0}")]
    SystemPath(String),
}

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
    "dd if=",             // Disk destruction (any input)
    "mkfs",               // Format filesystem
    "fdisk",              // Partition manipulation
    "shutdown",           // System shutdown
    "reboot",             // System reboot
    "init 0",             // Switch to runlevel 0
    "kill -9",            // Force kill processes
    "chmod 000",          // Remove all permissions
    "chmod -r 777 /",     // Open up root permissions
    "chmod -r 777",       // Recursive permission change
    "chown -r",           // Recursive ownership change
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

/// Shell variable expansion patterns used for bypass detection
const SHELL_EXPANSION_PATTERNS: &[&str] = &[
    "$'",                  // ANSI-C quoting
    "$(",                  // Command substitution
    "${",                  // Parameter expansion
    "`",                   // Backtick command substitution
    "$((",                 // Arithmetic expansion
    "$[",                  // Legacy arithmetic expansion
];

/// Sensitive system paths that should never be accessed
const SENSITIVE_PATHS: &[&str] = &[
    "/etc/passwd",         // Password database
    "/etc/shadow",         // Shadow password file
    "/etc/sudoers",        // Sudo configuration
    "/root/",              // Root home directory
    "/boot/",              // Boot files
    "/sys/",               // System filesystem
    "/proc/sys/",          // System configuration
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

    // FIRST: Check for shell expansion bypass attempts
    // These patterns indicate attempts to hide dangerous commands
    for pattern in SHELL_EXPANSION_PATTERNS {
        if command.contains(pattern) {
            risk_level = SecurityLevel::Critical;
            warnings.push(format!("Shell expansion bypass detected: {pattern} - variable expansion or command substitution can hide dangerous commands"));

            // Check if it contains ANSI-C quoting (common bypass technique)
            if command.contains("$'") {
                warnings.push("ANSI-C quoting detected: Can encode dangerous commands as hex escapes".to_string());
            }

            // Check for command substitution
            if command.contains("$(") || command.contains('`') {
                warnings.push("Command substitution detected: Can execute arbitrary commands".to_string());
            }

            // Check for parameter expansion
            if command.contains("${") {
                warnings.push("Parameter expansion detected: Can be used for obfuscation".to_string());
            }

            is_destructive = true;
            break;
        }
    }

    // Check for sensitive system paths in arguments
    for path in SENSITIVE_PATHS {
        if lower_command.contains(path) {
            risk_level = SecurityLevel::Critical;
            warnings.push(format!("Sensitive system path access detected: {path}"));
            is_destructive = true;
            break;
        }
    }

    // Check for IFS (Internal Field Separator) manipulation
    // Used to bypass word splitting detection
    if lower_command.contains("${ifs}") || lower_command.contains("ifs=") {
        risk_level = SecurityLevel::Critical;
        warnings.push("IFS manipulation detected: Common technique to bypass security checks".to_string());
        is_destructive = true;
    }

    // Check for base64/xxd encoding bypasses
    if (lower_command.contains("base64") || lower_command.contains("xxd") || lower_command.contains("od "))
        && (lower_command.contains("|") || lower_command.contains("$(")) {
            risk_level = SecurityLevel::Critical;
            warnings.push("Encoded command execution detected: base64/xxd used to hide commands".to_string());
            is_destructive = true;
        }

    // Check for destructive patterns (including the newly added ones)
    for pattern in DESTRUCTIVE_PATTERNS {
        if lower_command.contains(pattern) {
            if risk_level < SecurityLevel::Critical {
                risk_level = SecurityLevel::Critical;
            }
            is_destructive = true;
            warnings.push(format!("Destructive pattern detected: {pattern}"));
            // Don't break - collect all destructive patterns
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

    // Check for path traversal (enhanced to catch more patterns)
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

    // Additional path traversal checks for encoded variants
    if lower_command.contains("..\\") || lower_command.contains("%2e%2e") {
        contains_path_traversal = true;
        if risk_level < SecurityLevel::Medium {
            risk_level = SecurityLevel::Medium;
        }
        warnings.push("Encoded or Windows-style path traversal detected".to_string());
    }

    // Check for sed injection
    for pattern in SED_INJECTION_PATTERNS {
        if lower_command.contains(pattern) {
            if risk_level < SecurityLevel::Critical {
                risk_level = SecurityLevel::Critical;
            }
            warnings.push(format!("Sed injection pattern detected: {pattern}"));
            // Don't break - collect all injection patterns
        }
    }

    // Check if read-only (must come before pipe check so is_read_only is set)
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

    // Check for pipe-based command chaining that could bypass filters
    if command.contains('|') {
        // Always check what's being piped to, even for read-only commands
        let parts: Vec<&str> = command.split('|').collect();
        if parts.len() > 1 {
            for part in &parts[1..] {
                let part_lower = part.to_lowercase();
                // Check if piping to dangerous commands
                if part_lower.trim().starts_with("sh")
                    || part_lower.trim().starts_with("bash")
                    || part_lower.trim().starts_with("python")
                    || part_lower.trim().starts_with("perl")
                    || part_lower.trim().starts_with("ruby")
                    || part_lower.trim().starts_with("node")
                    || part_lower.trim().starts_with("eval") {
                    if risk_level < SecurityLevel::Critical {
                        risk_level = SecurityLevel::Critical;
                    }
                    warnings.push(format!("Dangerous pipe-to-shell detected: | {}", part.trim()));
                    is_destructive = true;
                }
            }
        }

        // Non-read-only pipes are medium risk
        if !is_read_only && risk_level < SecurityLevel::Medium {
            risk_level = SecurityLevel::Medium;
        }
    }

    // Additional heuristic: commands with sudo are higher risk
    if lower_command.starts_with("sudo ") {
        if risk_level < SecurityLevel::Medium {
            risk_level = SecurityLevel::Medium;
        }
        warnings.push("Elevated privileges requested (sudo)".to_string());
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
pub fn validate_path(path: &str, allowed_paths: &[String]) -> Result<(), PathValidationError> {
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
            return Err(PathValidationError::Traversal(path.to_string()));
        }
    }

    // Check against allowed paths if provided
    if !allowed_paths.is_empty() {
        let is_allowed = allowed_paths.iter().any(|allowed| {
            normalized.starts_with(allowed) || normalized == *allowed
        });

        if !is_allowed {
            return Err(PathValidationError::NotAllowed(path.to_string()));
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
            return Err(PathValidationError::SystemPath(path.to_string()));
        }
    }

    Ok(())
}

/// Execution sandbox mode for command isolation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum SandboxMode {
    /// Direct execution on host (default)
    Direct,
    /// Docker container isolation
    Docker(DockerSandboxConfig),
}

impl Default for SandboxMode {
    fn default() -> Self {
        Self::Direct
    }
}

impl SandboxMode {
    /// Parse from string (for env var / config)
    pub fn from_str_loose(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "docker" => Self::Docker(DockerSandboxConfig::default()),
            _ => Self::Direct,
        }
    }

    /// Check if sandbox mode is Docker
    pub fn is_docker(&self) -> bool {
        matches!(self, SandboxMode::Docker(_))
    }
}

/// Docker sandbox configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DockerSandboxConfig {
    /// Docker image to use
    #[serde(default = "DockerSandboxConfig::default_image")]
    pub image: String,
    /// Working directory inside container
    #[serde(default = "DockerSandboxConfig::default_workdir")]
    pub workdir: String,
    /// Network mode: "none", "bridge", "host"
    #[serde(default = "DockerSandboxConfig::default_network")]
    pub network: String,
    /// Memory limit (e.g., "512m", "1g")
    pub memory: Option<String>,
    /// CPU limit (e.g., "1.0", "0.5")
    pub cpus: Option<String>,
    /// Read-only root filesystem
    #[serde(default = "DockerSandboxConfig::default_readonly")]
    pub readonly_root: bool,
    /// Additional host paths to mount (host:container pairs)
    #[serde(default)]
    pub extra_mounts: Vec<String>,
}

impl DockerSandboxConfig {
    fn default_image() -> String { "ubuntu:22.04".to_string() }
    fn default_workdir() -> String { "/workspace".to_string() }
    fn default_network() -> String { "none".to_string() }
    fn default_readonly() -> bool { true }
}

impl Default for DockerSandboxConfig {
    fn default() -> Self {
        Self {
            image: Self::default_image(),
            workdir: Self::default_workdir(),
            network: Self::default_network(),
            memory: Some("512m".to_string()),
            cpus: Some("1.0".to_string()),
            readonly_root: Self::default_readonly(),
            extra_mounts: Vec::new(),
        }
    }
}

/// Docker sandbox for isolated command execution
pub struct DockerSandbox {
    config: DockerSandboxConfig,
}

impl DockerSandbox {
    /// Create a new Docker sandbox with the given configuration
    pub fn new(config: DockerSandboxConfig) -> Self {
        Self { config }
    }

    /// Check if Docker is available on the system
    pub async fn is_available() -> bool {
        if cfg!(test) {
            return false;
        }
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            Command::new("docker")
                .arg("info")
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output(),
        )
        .await;
        match result {
            Ok(Ok(o)) => o.status.success(),
            _ => false,
        }
    }

    /// Build the docker run argument list
    fn build_args(
        &self,
        command: &str,
        cwd: Option<&str>,
        env: Option<&std::collections::HashMap<String, String>>,
    ) -> Vec<String> {
        let mut args = vec![
            "run".to_string(),
            "--rm".to_string(),
        ];

        // Mount workspace: resolve cwd or use current directory
        let workspace = cwd.unwrap_or(".");
        let abs_workspace = std::path::Path::new(workspace)
            .canonicalize()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| workspace.to_string());

        args.push("-v".to_string());
        args.push(format!("{}:{}", abs_workspace, self.config.workdir));
        args.push("-w".to_string());
        args.push(self.config.workdir.clone());

        // Network isolation
        args.push("--network".to_string());
        args.push(self.config.network.clone());

        // Resource limits
        if let Some(ref mem) = self.config.memory {
            args.push("--memory".to_string());
            args.push(mem.clone());
        }
        if let Some(ref cpus) = self.config.cpus {
            args.push("--cpus".to_string());
            args.push(cpus.clone());
        }

        // Read-only root filesystem
        if self.config.readonly_root {
            args.push("--read-only".to_string());
            // /tmp needs to be writable for many commands
            args.push("--tmpfs".to_string());
            args.push("/tmp:rw,noexec,nosuid,size=100m".to_string());
        }

        // Extra mounts
        for mount in &self.config.extra_mounts {
            args.push("-v".to_string());
            args.push(mount.clone());
        }

        // Environment variables
        if let Some(env_vars) = env {
            for (key, value) in env_vars {
                args.push("-e".to_string());
                args.push(format!("{key}={value}"));
            }
        }

        // Image and command
        args.push(self.config.image.clone());
        args.push("bash".to_string());
        args.push("-c".to_string());
        args.push(command.to_string());

        args
    }

    /// Execute a command inside a Docker container
    pub async fn execute(
        &self,
        command: &str,
        cwd: Option<&str>,
        env: Option<&std::collections::HashMap<String, String>>,
        timeout_ms: Option<u64>,
    ) -> Result<CommandOutput, std::io::Error> {
        let docker_args = self.build_args(command, cwd, env);

        let mut cmd = Command::new("docker");
        cmd.args(&docker_args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let output = if let Some(timeout) = timeout_ms {
            let duration = std::time::Duration::from_millis(timeout);
            tokio::time::timeout(duration, cmd.output())
                .await
                .map_err(|_| std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!("Docker command timed out after {timeout}ms"),
                ))?
                .map_err(|e| std::io::Error::other(format!("Docker execution failed: {e}")))?
        } else {
            cmd.output()
                .await
                .map_err(|e| std::io::Error::other(format!("Docker execution failed: {e}")))?
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

    /// Use PTY (pseudo-terminal) for interactive command support
    #[serde(default)]
    pub use_pty: bool,

    /// Delay in milliseconds before streaming output begins (default: 500).
    /// Fast commands finishing within this window skip streaming entirely.
    #[serde(default)]
    pub stream_delay_ms: Option<u64>,

    /// Shared cancellation flag — when set to true, the streaming loop
    /// will kill the child process and return. Not deserialized from JSON.
    #[serde(skip)]
    pub cancelled: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
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
    sandbox: Option<DockerSandbox>,
    /// Platform sandbox (bwrap on Linux, Seatbelt on macOS)
    process_sandbox: Option<SandboxExecutor>,
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
            sandbox: None,
            process_sandbox: None,
        }
    }

    /// Create a BashTool that routes commands through a Docker sandbox
    pub fn with_docker_sandbox(config: DockerSandboxConfig) -> Self {
        Self {
            description: "Executes bash commands in Docker sandbox".to_string(),
            sandbox: Some(DockerSandbox::new(config)),
            process_sandbox: None,
        }
    }

    /// Create a BashTool with a platform process sandbox (bwrap/Seatbelt).
    ///
    /// The `SandboxExecutor` is auto-detected from the current platform.
    /// If no sandbox backend is available, commands run unsandboxed.
    pub fn with_process_sandbox(project_dir: impl Into<std::path::PathBuf>) -> Self {
        let config = SandboxConfig::new(project_dir);
        let executor = SandboxExecutor::new(config);
        let sandbox_type = executor.sandbox_type();
        let has_sandbox = !matches!(sandbox_type, SandboxType::None);
        Self {
            description: if has_sandbox {
                format!("Executes bash commands (sandboxed via {sandbox_type})")
            } else {
                "Executes bash commands and returns output".to_string()
            },
            sandbox: None,
            process_sandbox: if has_sandbox { Some(executor) } else { None },
        }
    }

    /// Update the sandbox mode
    pub fn set_sandbox(&mut self, mode: SandboxMode) {
        match mode {
            SandboxMode::Direct => self.sandbox = None,
            SandboxMode::Docker(config) => self.sandbox = Some(DockerSandbox::new(config)),
        }
    }

    /// Get the current sandbox mode
    pub fn sandbox_mode(&self) -> SandboxMode {
        match &self.sandbox {
            None => SandboxMode::Direct,
            Some(s) => SandboxMode::Docker(s.config.clone()),
        }
    }

    /// Execute a command through the platform process sandbox (bwrap/Seatbelt).
    ///
    /// Creates a `std::process::Command`, wraps it via `SandboxExecutor`,
    /// then converts to `tokio::process::Command` for async execution.
    async fn execute_command_sandboxed(
        command: &str,
        cwd: Option<&str>,
        env: Option<&std::collections::HashMap<String, String>>,
        timeout_ms: Option<u64>,
        executor: &SandboxExecutor,
    ) -> Result<CommandOutput, std::io::Error> {
        let mut cmd = std::process::Command::new("bash");
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

        // Wrap the command with the platform sandbox (bwrap/Seatbelt).
        executor
            .wrap_command(&mut cmd)
            .map_err(|e| std::io::Error::other(format!("Sandbox wrap failed: {e}")))?;

        // Convert std::process::Command → tokio::process::Command
        let mut cmd = Command::from(cmd);
        cmd.kill_on_drop(true);

        let output = if let Some(timeout) = timeout_ms {
            let duration = std::time::Duration::from_millis(timeout);
            tokio::time::timeout(duration, cmd.output())
                .await
                .map_err(|_| {
                    std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        format!("Command timed out after {timeout}ms"),
                    )
                })?
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
            .stderr(Stdio::piped())
            .kill_on_drop(true);

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

        // Execute the command (PTY mode for interactive, otherwise sandboxed/direct)
        let output_result = if bash_input.use_pty {
            let cmd = bash_input.command.clone();
            let cwd = bash_input.cwd.clone();
            let env = bash_input.env.clone();
            let timeout = bash_input.timeout;
            tokio::task::spawn_blocking(move || {
                match crate::pty::execute_in_pty(
                    &cmd,
                    cwd.as_deref(),
                    env.as_ref(),
                    timeout,
                ) {
                    Ok(pty_out) => Ok(CommandOutput {
                        stdout: pty_out.stdout,
                        stderr: String::new(),
                        exit_code: pty_out.exit_code,
                        success: pty_out.exit_code == 0,
                    }),
                    Err(e) => Err(std::io::Error::other(e)),
                }
            })
            .await
            .unwrap_or_else(|e| Err(std::io::Error::other(e.to_string())))
        } else if let Some(ref sandbox) = self.sandbox {
            sandbox.execute(
                &bash_input.command,
                bash_input.cwd.as_deref(),
                bash_input.env.as_ref(),
                bash_input.timeout,
            )
            .await
        } else if let Some(ref ps) = self.process_sandbox {
            Self::execute_command_sandboxed(
                &bash_input.command,
                bash_input.cwd.as_deref(),
                bash_input.env.as_ref(),
                bash_input.timeout,
                ps,
            )
            .await
        } else {
            Self::execute_command(
                &bash_input.command,
                bash_input.cwd.as_deref(),
                bash_input.env.as_ref(),
                bash_input.timeout,
            )
            .await
        };

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

    async fn execute_streaming(
        &self,
        input: serde_json::Value,
        progress: BoxedProgressSender,
    ) -> ToolResult<ToolOutput> {
        self.execute_streaming_inner(input, progress).await
    }
}

/// Strip non-renderable ANSI escape sequences, preserving SGR color/style codes.
///
/// Keeps `\x1b[...m` sequences (colors, bold, underline, reset) but removes
/// cursor movement, screen clearing, and other control sequences.
fn strip_ansi(s: &str) -> String {
    // Strip all CSI sequences except SGR (which ends with 'm')
    let re = regex::Regex::new(r"\x1b\[[0-9;]*[A-HJ-Za-ln-z]").unwrap();
    re.replace_all(s, "").into_owned()
}

impl BashTool {
    async fn execute_streaming_inner(
        &self,
        input: serde_json::Value,
        progress: BoxedProgressSender,
    ) -> ToolResult<ToolOutput> {
        let bash_input: BashInput = match serde_json::from_value(input) {
            Ok(input) => input,
            Err(e) => return Ok(ToolOutput {
                content: format!("Invalid bash input: {e}"),
                is_error: true,
                metadata: HashMap::new(),
            })
        };

        let analysis = analyze_command_security(&bash_input.command);

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

        // Only stream direct (non-PTY, non-sandbox) commands.
        // PTY and sandbox modes fall back to blocking execute().
        let use_streaming = !bash_input.use_pty
            && self.sandbox.is_none()
            && self.process_sandbox.is_none();

        if !use_streaming {
            // Delegate to blocking execute — wraps in a helper to reuse
            // the same logic. We call execute() directly to avoid duplicating.
            return self.execute(serde_json::to_value(&bash_input).unwrap_or_default()).await;
        }

        // Streaming path: spawn the process and read stdout line-by-line.
        let mut cmd = Command::new("bash");
        cmd.arg("-c")
            .arg(&bash_input.command)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        if let Some(ref dir) = bash_input.cwd {
            cmd.current_dir(dir);
        }
        if let Some(ref env_vars) = bash_input.env {
            for (key, value) in env_vars {
                cmd.env(key, value);
            }
        }

        let mut child = cmd.spawn()
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to spawn command: {e}")))?;

        let stdout = child.stdout.take()
            .ok_or_else(|| ToolError::ExecutionFailed("Failed to capture stdout".to_string()))?;
        let stderr = child.stderr.take()
            .ok_or_else(|| ToolError::ExecutionFailed("Failed to capture stderr".to_string()))?;

        let mut stdout_lines = BufReader::new(stdout).lines();
        let mut stderr_lines = BufReader::new(stderr).lines();

        let mut stdout_buf = String::new();
        let mut stderr_buf = String::new();

        // Buffer streaming lines before sending progress events.
        // This avoids flicker for fast commands — if the process finishes
        // within the buffer window, no streaming events are emitted at all.
        let stream_delay = Duration::from_millis(bash_input.stream_delay_ms.unwrap_or(500));
        let start = Instant::now();
        let mut streaming_active = false;
        let mut buffered_lines: Vec<String> = Vec::new();

        // Read stdout and stderr concurrently, sending progress for each stdout line.
        let cancel_flag = bash_input.cancelled.clone();
        loop {
            // Check cancellation
            if let Some(ref flag) = cancel_flag {
                if flag.load(std::sync::atomic::Ordering::Relaxed) {
                    let _ = child.kill().await;
                    stderr_buf.push_str("Command cancelled by user\n");
                    break;
                }
            }
            tokio::select! {
                line = stdout_lines.next_line() => {
                    match line {
                        Ok(Some(line)) => {
                            let cleaned = strip_ansi(&line);
                            stdout_buf.push_str(&cleaned);
                            stdout_buf.push('\n');

                            if !streaming_active {
                                buffered_lines.push(cleaned.clone());
                                if start.elapsed() >= stream_delay {
                                    streaming_active = true;
                                    for bl in &buffered_lines {
                                        progress.send(bl);
                                    }
                                    buffered_lines.clear();
                                }
                            } else {
                                progress.send(&cleaned);
                            }
                        }
                        Ok(None) => break,
                        Err(e) => {
                            stderr_buf.push_str(&format!("stdout read error: {e}\n"));
                            break;
                        }
                    }
                }
                line = stderr_lines.next_line() => {
                    match line {
                        Ok(Some(line)) => {
                            let cleaned = strip_ansi(&line);
                            stderr_buf.push_str(&cleaned);
                            stderr_buf.push('\n');
                            let tagged = format!("⚠ {cleaned}");
                            if !streaming_active {
                                buffered_lines.push(tagged);
                                if start.elapsed() >= stream_delay {
                                    streaming_active = true;
                                    for bl in &buffered_lines {
                                        progress.send(bl);
                                    }
                                    buffered_lines.clear();
                                }
                            } else {
                                progress.send(&tagged);
                            }
                        }
                        Ok(None) => {}
                        Err(_) => {}
                    }
                }
            }
        }

        // Drain remaining stderr
        while let Ok(Some(line)) = stderr_lines.next_line().await {
            stderr_buf.push_str(&line);
            stderr_buf.push('\n');
        }

        let status = child.wait().await
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to wait for command: {e}")))?;

        let exit_code = status.code().unwrap_or(-1);
        let success = status.success();

        let content = if success {
            format!("{stdout_buf}{command_description}")
        } else {
            format!("{}Command failed with exit code {}: {}{}",
                command_description,
                exit_code,
                stderr_buf,
                if command_description.is_empty() { "\n" } else { "" })
        };

        Ok(ToolOutput {
            content,
            is_error: !success,
            metadata: {
                let mut map = HashMap::new();
                map.insert("exit_code".to_string(), json!(exit_code));
                map.insert("risk_level".to_string(), json!(analysis.risk_level as i32));
                map.insert("is_destructive".to_string(), json!(analysis.is_destructive));
                map.insert("is_read_only".to_string(), json!(analysis.is_read_only));
                if !analysis.warnings.is_empty() {
                    map.insert("warnings".to_string(), json!(analysis.warnings));
                }
                if !stderr_buf.is_empty() {
                    map.insert("stderr".to_string(), json!(stderr_buf));
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
#[allow(clippy::large_enum_variant)]
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
    fn is_read_only(&self) -> bool {        true    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── SandboxMode tests ──────────────────────────────────────────────

    #[test]
    fn test_sandbox_mode_default_is_direct() {
        assert_eq!(SandboxMode::default(), SandboxMode::Direct);
    }

    #[test]
    fn test_sandbox_mode_from_str_loose() {
        assert!(SandboxMode::from_str_loose("docker").is_docker());
        assert!(SandboxMode::from_str_loose("Docker").is_docker());
        assert!(SandboxMode::from_str_loose("DOCKER").is_docker());
        assert!(!SandboxMode::from_str_loose("direct").is_docker());
        assert!(!SandboxMode::from_str_loose("none").is_docker());
        assert!(!SandboxMode::from_str_loose("").is_docker());
    }

    // ── DockerSandboxConfig tests ──────────────────────────────────────

    #[test]
    fn test_docker_config_defaults() {
        let config = DockerSandboxConfig::default();
        assert_eq!(config.image, "ubuntu:22.04");
        assert_eq!(config.workdir, "/workspace");
        assert_eq!(config.network, "none");
        assert_eq!(config.memory, Some("512m".to_string()));
        assert_eq!(config.cpus, Some("1.0".to_string()));
        assert!(config.readonly_root);
        assert!(config.extra_mounts.is_empty());
    }

    #[test]
    fn test_docker_config_custom() {
        let config = DockerSandboxConfig {
            image: "alpine:3.19".to_string(),
            workdir: "/app".to_string(),
            network: "bridge".to_string(),
            memory: Some("1g".to_string()),
            cpus: None,
            readonly_root: false,
            extra_mounts: vec!["/data:/data".to_string()],
        };
        assert_eq!(config.image, "alpine:3.19");
        assert_eq!(config.workdir, "/app");
        assert_eq!(config.network, "bridge");
        assert!(config.cpus.is_none());
        assert!(!config.readonly_root);
        assert_eq!(config.extra_mounts.len(), 1);
    }

    #[test]
    fn test_docker_config_serialization_roundtrip() {
        let config = DockerSandboxConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: DockerSandboxConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config, deserialized);
    }

    // ── SandboxMode serialization tests ────────────────────────────────

    #[test]
    fn test_sandbox_mode_serialization_direct() {
        let mode = SandboxMode::Direct;
        let json = serde_json::to_string(&mode).unwrap();
        assert!(json.contains("\"mode\":\"direct\""));
        let back: SandboxMode = serde_json::from_str(&json).unwrap();
        assert_eq!(mode, back);
    }

    #[test]
    fn test_sandbox_mode_serialization_docker() {
        let mode = SandboxMode::Docker(DockerSandboxConfig::default());
        let json = serde_json::to_string(&mode).unwrap();
        assert!(json.contains("\"mode\":\"docker\""));
        let back: SandboxMode = serde_json::from_str(&json).unwrap();
        assert_eq!(mode, back);
    }

    // ── Docker args construction tests ─────────────────────────────────

    #[test]
    fn test_docker_build_args_basic() {
        let config = DockerSandboxConfig::default();
        let sandbox = DockerSandbox::new(config);
        let args = sandbox.build_args("echo hello", None, None);

        // Should start with run --rm
        assert!(args.contains(&"run".to_string()));
        assert!(args.contains(&"--rm".to_string()));
        // Should have network=none
        let net_idx = args.iter().position(|a| a == "--network").unwrap();
        assert_eq!(args[net_idx + 1], "none");
        // Should have --read-only
        assert!(args.contains(&"--read-only".to_string()));
        // Should mount workspace
        assert!(args.contains(&"-v".to_string()));
        assert!(args.iter().any(|a| a.contains(":/workspace")));
        // Should have image
        assert!(args.contains(&"ubuntu:22.04".to_string()));
        // Command at end
        assert_eq!(args.last(), Some(&"echo hello".to_string()));
    }

    #[test]
    fn test_docker_build_args_with_env() {
        let config = DockerSandboxConfig::default();
        let sandbox = DockerSandbox::new(config);
        let mut env = HashMap::new();
        env.insert("FOO".to_string(), "bar".to_string());
        let args = sandbox.build_args("env", None, Some(&env));

        let env_idx = args.iter().position(|a| a == "FOO=bar").unwrap();
        assert!(args[env_idx - 1] == "-e");
    }

    #[test]
    fn test_docker_build_args_no_readonly() {
        let config = DockerSandboxConfig {
            readonly_root: false,
            ..DockerSandboxConfig::default()
        };
        let sandbox = DockerSandbox::new(config);
        let args = sandbox.build_args("ls", None, None);

        assert!(!args.contains(&"--read-only".to_string()));
        assert!(!args.iter().any(|a| a.starts_with("/tmp:")));
    }

    #[test]
    fn test_docker_build_args_with_extra_mounts() {
        let config = DockerSandboxConfig {
            extra_mounts: vec!["/host/path:/container/path".to_string()],
            ..DockerSandboxConfig::default()
        };
        let sandbox = DockerSandbox::new(config);
        let args = sandbox.build_args("ls", None, None);

        assert!(args.contains(&"/host/path:/container/path".to_string()));
    }

    // ── BashTool sandbox integration tests ─────────────────────────────

    #[test]
    fn test_bash_tool_default_no_sandbox() {
        let tool = BashTool::new();
        assert_eq!(tool.sandbox_mode(), SandboxMode::Direct);
    }

    #[test]
    fn test_bash_tool_with_docker_sandbox() {
        let tool = BashTool::with_docker_sandbox(DockerSandboxConfig::default());
        assert!(tool.sandbox_mode().is_docker());
    }

    #[test]
    fn test_bash_tool_set_sandbox() {
        let mut tool = BashTool::new();
        assert_eq!(tool.sandbox_mode(), SandboxMode::Direct);

        tool.set_sandbox(SandboxMode::Docker(DockerSandboxConfig::default()));
        assert!(tool.sandbox_mode().is_docker());

        tool.set_sandbox(SandboxMode::Direct);
        assert_eq!(tool.sandbox_mode(), SandboxMode::Direct);
    }

    // ── Security analysis unchanged by sandbox ─────────────────────────

    #[test]
    fn test_security_analysis_independent_of_sandbox() {
        let analysis = analyze_command_security("rm -rf /");
        assert!(analysis.is_destructive);
        assert_eq!(analysis.risk_level, SecurityLevel::Critical);

        let analysis2 = analyze_command_security("ls");
        assert!(analysis2.is_read_only);
    }
}

    // ── Security bypass detection tests ─────────────────────────────────────

    #[test]
    fn test_shell_expansion_bypass_detection() {
        // ANSI-C quoting bypass
        let analysis = analyze_command_security("$'rm\\x20-rf\\x20/'");
        assert_eq!(analysis.risk_level, SecurityLevel::Critical);
        assert!(analysis.warnings.iter().any(|w| w.contains("ANSI-C quoting")));

        // Command substitution bypass
        let analysis = analyze_command_security("echo $(rm -rf /)");
        assert_eq!(analysis.risk_level, SecurityLevel::Critical);
        assert!(analysis.warnings.iter().any(|w| w.contains("Command substitution")));

        // Parameter expansion bypass
        let analysis = analyze_command_security("echo ${HOME}/../../etc/passwd");
        assert_eq!(analysis.risk_level, SecurityLevel::Critical);
        assert!(analysis.warnings.iter().any(|w| w.contains("Parameter expansion")));
    }

    #[test]
    fn test_ifs_manipulation_detection() {
        let analysis = analyze_command_security("IFS=/; echo rm");
        assert_eq!(analysis.risk_level, SecurityLevel::Critical);
        assert!(analysis.warnings.iter().any(|w| w.contains("IFS manipulation")));

        let analysis2 = analyze_command_security("cat ${IFS}etc${IFS}passwd");
        assert_eq!(analysis2.risk_level, SecurityLevel::Critical);
    }

    #[test]
    fn test_base64_encoding_bypass_detection() {
        let analysis = analyze_command_security("echo 'cm0gLXJmIC8=' | base64 -d | bash");
        assert_eq!(analysis.risk_level, SecurityLevel::Critical);
        assert!(analysis.warnings.iter().any(|w| w.contains("Encoded command")));
    }

    #[test]
    fn test_sensitive_path_detection() {
        let analysis = analyze_command_security("cat /etc/passwd");
        assert_eq!(analysis.risk_level, SecurityLevel::Critical);
        assert!(analysis.warnings.iter().any(|w| w.contains("/etc/passwd")));

        let analysis2 = analyze_command_security("cat /etc/shadow");
        assert_eq!(analysis2.risk_level, SecurityLevel::Critical);
    }

    #[test]
    fn test_pipe_to_shell_detection() {
        let analysis = analyze_command_security("cat file | sh");
        assert_eq!(analysis.risk_level, SecurityLevel::Critical);
        assert!(analysis.warnings.iter().any(|w| w.contains("pipe-to-shell")));

        let analysis2 = analyze_command_security("ls | bash");
        assert_eq!(analysis2.risk_level, SecurityLevel::Critical);
    }

    #[test]
    fn test_encoded_path_traversal_detection() {
        let analysis = analyze_command_security("cat %2e%2e/%2e%2e/etc/passwd");
        assert!(analysis.contains_path_traversal);
        assert!(analysis.warnings.iter().any(|w| w.contains("Encoded")));

        let analysis2 = analyze_command_security("cat ..\\..\\windows\\system32");
        assert!(analysis2.contains_path_traversal);
    }

    #[test]
    fn test_new_destructive_patterns() {
        // dd if= pattern
        let analysis = analyze_command_security("dd if=/dev/zero of=file");
        assert_eq!(analysis.risk_level, SecurityLevel::Critical);

        // chmod -r 777 pattern
        let analysis2 = analyze_command_security("chmod -r 777 /etc");
        assert_eq!(analysis2.risk_level, SecurityLevel::Critical);

        // chown -r pattern
        let analysis3 = analyze_command_security("chown -r user file");
        assert_eq!(analysis3.risk_level, SecurityLevel::Critical);
    }

    // ── BashTool streaming tests ──────────────────────────────────────────

    struct CollectSender {
        lines: std::sync::Mutex<Vec<String>>,
    }

    impl crate::ProgressSender for CollectSender {
        fn send(&self, line: &str) {
            self.lines.lock().unwrap().push(line.to_string());
        }
    }

    #[tokio::test]
    async fn test_bash_streaming_fast_command_no_streaming_events() {
        // Fast commands finish within the 500ms buffer window, so no
        // streaming progress events should be emitted.
        let tool = BashTool::new();
        let sender = std::sync::Arc::new(CollectSender {
            lines: std::sync::Mutex::new(Vec::new()),
        });

        let result = tool.execute_streaming(
            json!({"command": "echo line1; echo line2; echo line3"}),
            sender.clone(),
        ).await.unwrap();

        assert!(!result.is_error);
        assert!(result.content.contains("line1"));
        assert!(result.content.contains("line2"));
        assert!(result.content.contains("line3"));

        let lines = sender.lines.lock().unwrap();
        assert!(lines.is_empty(), "fast command should not emit streaming events, got {:?}", *lines);
    }

    #[tokio::test]
    async fn test_bash_streaming_slow_command_emits_lines() {
        // A slow command (sleep > 500ms) should flush the buffer and stream.
        let tool = BashTool::new();
        let sender = std::sync::Arc::new(CollectSender {
            lines: std::sync::Mutex::new(Vec::new()),
        });

        let result = tool.execute_streaming(
            json!({"command": "echo line1; sleep 0.6; echo line2; echo line3"}),
            sender.clone(),
        ).await.unwrap();

        assert!(!result.is_error);
        assert!(result.content.contains("line1"));
        assert!(result.content.contains("line2"));
        assert!(result.content.contains("line3"));

        let lines = sender.lines.lock().unwrap();
        assert!(!lines.is_empty(), "slow command should emit streaming events");
        // After 500ms buffer, all buffered lines + subsequent lines should stream
        assert!(lines.contains(&"line1".to_string()), "line1 should be streamed after buffer flush");
        assert!(lines.contains(&"line2".to_string()), "line2 should be streamed");
        assert!(lines.contains(&"line3".to_string()), "line3 should be streamed");
    }

    #[tokio::test]
    async fn test_bash_streaming_captures_exit_code() {
        let tool = BashTool::new();
        let sender = std::sync::Arc::new(CollectSender {
            lines: std::sync::Mutex::new(Vec::new()),
        });

        let result = tool.execute_streaming(
            json!({"command": "echo ok; exit 42"}),
            sender,
        ).await.unwrap();

        assert!(result.is_error);
        assert_eq!(result.metadata.get("exit_code"), Some(&json!(42)));
    }

    #[tokio::test]
    async fn test_bash_streaming_rejects_critical_commands() {
        let tool = BashTool::new();
        let sender = std::sync::Arc::new(CollectSender {
            lines: std::sync::Mutex::new(Vec::new()),
        });

        let result = tool.execute_streaming(
            json!({"command": "rm -rf /"}),
            sender.clone(),
        ).await.unwrap();

        assert!(result.is_error);
        assert!(result.content.contains("rejected"));
        // No lines streamed for rejected commands
        assert!(sender.lines.lock().unwrap().is_empty());
    }

    #[test]
    fn test_strip_ansi_preserves_colors_removes_control() {
        // Color codes (SGR) are preserved
        let colored = "\x1b[32mok\x1b[0m done";
        assert_eq!(strip_ansi(colored), "\x1b[32mok\x1b[0m done");

        let no_ansi = "plain text";
        assert_eq!(strip_ansi(no_ansi), "plain text");

        let multi = "\x1b[1;34mheader\x1b[0m\n\x1b[31merror\x1b[0m";
        assert_eq!(strip_ansi(multi), "\x1b[1;34mheader\x1b[0m\n\x1b[31merror\x1b[0m");

        // Cursor movement and clear screen are stripped
        let cursor = "\x1b[2J\x1b[H\x1b[1mbold\x1b[0m";
        assert_eq!(strip_ansi(cursor), "\x1b[1mbold\x1b[0m");

        // Cursor up/down are stripped
        let movement = "line1\x1b[A\x1b[2Kline2";
        assert_eq!(strip_ansi(movement), "line1line2");
    }

    // ── SecurityLevel and PathValidation tests ────────────────────────────

    #[test]
    fn test_security_level_ordering() {
        assert!(SecurityLevel::Safe < SecurityLevel::Low);
        assert!(SecurityLevel::Low < SecurityLevel::Medium);
        assert!(SecurityLevel::Medium < SecurityLevel::High);
        assert!(SecurityLevel::High < SecurityLevel::Critical);
        assert_eq!(SecurityLevel::Safe, SecurityLevel::Safe);
    }

    #[test]
    fn test_security_level_ord_values() {
        assert_eq!(SecurityLevel::Safe as u8, 0);
        assert_eq!(SecurityLevel::Low as u8, 1);
        assert_eq!(SecurityLevel::Medium as u8, 2);
        assert_eq!(SecurityLevel::High as u8, 3);
        assert_eq!(SecurityLevel::Critical as u8, 4);
    }

    #[test]
    fn test_path_validation_error_display() {
        let err = PathValidationError::Traversal("../etc/passwd".into());
        assert!(err.to_string().contains("../etc/passwd"));

        let err = PathValidationError::NotAllowed("/root".into());
        assert!(err.to_string().contains("/root"));

        let err = PathValidationError::SystemPath("/etc/shadow".into());
        assert!(err.to_string().contains("/etc/shadow"));
    }

    #[test]
    fn test_security_analysis_default_fields() {
        let analysis = SecurityAnalysis {
            risk_level: SecurityLevel::Safe,
            warnings: vec![],
            is_destructive: false,
            is_read_only: true,
            contains_path_traversal: false,
            requires_confirmation: false,
        };
        assert!(analysis.is_read_only);
        assert!(!analysis.is_destructive);
        assert!(!analysis.requires_confirmation);
        assert!(analysis.warnings.is_empty());
    }