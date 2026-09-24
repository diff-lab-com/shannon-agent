//! System operation tools
//!
//! Provides implementations for:
//! - Bash: Execute shell commands on Unix-like systems
//! - PowerShell: Execute commands on Windows systems

use crate::sandbox::DenialClassifier;
use crate::{BoxedProgressSender, Tool, ToolError, ToolOutput, ToolResult};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use shannon_core::providers::{LocalProcess, SandboxExecutorRewrite};
use shannon_core::sandbox::{SandboxConfig, SandboxExecutor, SandboxType};
use shannon_tool_interface::{ProcessProvider, ProcessRequest};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, BufReader};

/// Default command timeout applied when the caller passes no `timeout`.
/// Previously `None` meant *unbounded*, so a single hung command could stall
/// a turn forever.
const DEFAULT_BASH_TIMEOUT_MS: u64 = 120_000;

/// Hard cap on the resolved command timeout, even when the caller or the
/// `SHANNON_BASH_TIMEOUT_MS` override asks for more (10 minutes).
const MAX_BASH_TIMEOUT_MS: u64 = 600_000;

/// review §P2-10: maximum bytes of captured stdout/stderr the harness
/// keeps per command. Anything beyond this is dropped — the rest of the
/// output is unrecoverable from inside the process, so the model sees a
/// truncated string and a clear marker. Set to 2 MiB which is comfortably
/// large for normal command output but small enough that one bad `cat
/// huge.log` cannot blow the conversation context.
const MAX_CAPTURED_BYTES: usize = 2 * 1024 * 1024;

/// review §P2-10: clip `output` to at most `cap` bytes while preserving a
/// valid UTF-8 char boundary at the cut, and append a truncation marker
/// so the consumer knows data was dropped.
pub(crate) fn truncate_bytes(output: &[u8], cap: usize) -> Vec<u8> {
    if output.len() <= cap {
        return output.to_vec();
    }
    let mut cut = cap;
    while cut > 0 && std::str::from_utf8(&output[..cut]).is_err() {
        cut -= 1;
    }
    let mut out = output[..cut].to_vec();
    out.extend_from_slice(
        format!(
            "\n\n[truncated by harness — {} bytes dropped]",
            output.len() - cut
        )
        .as_bytes(),
    );
    out
}

/// Timeout-resolution core: an explicit `timeout` wins, then the
/// `SHANNON_BASH_TIMEOUT_MS` env override, then the default — clamped to the
/// hard cap. Split from [`resolve_timeout_ms`] so the env lookup can be
/// unit-tested without mutating process-global state.
fn resolve_timeout_ms_with_env(timeout_ms: Option<u64>, env_value: Option<&str>) -> u64 {
    let requested = timeout_ms
        .or_else(|| env_value.and_then(|v| v.trim().parse::<u64>().ok()))
        .unwrap_or(DEFAULT_BASH_TIMEOUT_MS);
    requested.min(MAX_BASH_TIMEOUT_MS)
}

/// Resolve the effective command timeout against the live environment.
fn resolve_timeout_ms(timeout_ms: Option<u64>) -> u64 {
    resolve_timeout_ms_with_env(
        timeout_ms,
        std::env::var("SHANNON_BASH_TIMEOUT_MS").ok().as_deref(),
    )
}

/// Shared captured-run helper: builds the request, applies the resolved
/// timeout (never unbounded — see [`resolve_timeout_ms`]), and projects the
/// provider result onto [`CommandOutput`].
async fn run_shell_captured(
    world: &dyn ProcessProvider,
    program: &str,
    shell_flag: &str,
    command: &str,
    cwd: Option<&str>,
    env: Option<&std::collections::HashMap<String, String>>,
    timeout_ms: Option<u64>,
) -> Result<CommandOutput, std::io::Error> {
    let mut request = ProcessRequest::new(program, &[shell_flag, command]);
    if let Some(dir) = cwd {
        request.cwd = Some(dir.into());
    }
    if let Some(env_vars) = env {
        for (key, value) in env_vars {
            request.env.push((key.clone(), value.clone()));
        }
    }

    let timeout = resolve_timeout_ms(timeout_ms);
    let duration = Duration::from_millis(timeout);
    let output = tokio::time::timeout(duration, world.run_async(&request))
        .await
        .map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!("Command timed out after {timeout}ms"),
            )
        })?
        .map_err(|e| shell_spawn_error(program, &e))?;

    // review §P2-10: bound the captured stdout/stderr bytes before
    // constructing the String the model will see. `cat huge.log` or
    // `find /` could otherwise ship gigabytes into the conversation.
    let stdout_bytes = truncate_bytes(&output.stdout, MAX_CAPTURED_BYTES);
    let stderr_bytes = truncate_bytes(&output.stderr, MAX_CAPTURED_BYTES);
    let stdout = String::from_utf8_lossy(&stdout_bytes).to_string();
    let stderr = String::from_utf8_lossy(&stderr_bytes).to_string();
    Ok(CommandOutput {
        stdout,
        stderr,
        exit_code: output.exit.code.unwrap_or(-1),
        success: output.exit.success,
    })
}

/// Actionable spawn-failure message. The Bash tool hardcodes `bash -c`; on
/// Windows that needs Git Bash (or WSL) on PATH, and without it every Bash
/// call dies with a bare "program not found" — point the model at the
/// PowerShell tool instead (always present on Windows).
fn shell_spawn_error(program: &str, e: &std::io::Error) -> std::io::Error {
    let text = e.to_string();
    let not_found = matches!(e.kind(), std::io::ErrorKind::NotFound)
        || text.contains("not found")
        || text.contains("cannot find");
    if cfg!(target_os = "windows") && program == "bash" && not_found {
        std::io::Error::other(
            "The Bash tool requires `bash` on PATH (from Git for Windows or WSL), \
             which was not found. Use the PowerShell tool instead — it is always \
             available on Windows — or install Git for Windows \
             (https://git-scm.com/download/win) and restart Shannon.",
        )
    } else {
        std::io::Error::other(format!("Failed to execute command: {e}"))
    }
}

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
    "rm -rf /",        // Delete root filesystem
    "rm -rf /*",       // Delete all files
    ":>",              // Zero out files
    "dd if=/dev/zero", // Disk destruction
    "dd if=",          // Disk destruction (any input)
    "mkfs",            // Format filesystem
    "fdisk",           // Partition manipulation
    "shutdown",        // System shutdown
    "reboot",          // System reboot
    "init 0",          // Switch to runlevel 0
    "kill -9",         // Force kill processes
    "chmod 000",       // Remove all permissions
    "chmod -r 777 /",  // Open up root permissions
    "chmod -r 777",    // Recursive permission change
    "chown -r",        // Recursive ownership change
];

/// Confirmation-required patterns
const CONFIRMATION_PATTERNS: &[&str] = &[
    "rm -rf", // Recursive force delete
    "del /q", // Windows quiet delete
    "format", // Windows format
    "shred",  // Secure delete
];

/// Path traversal patterns
const PATH_TRAVERSAL_PATTERNS: &[&str] = &[
    "../",   // Parent directory traversal
    "./../", // Multiple parent traversal
    "~/../", // From home parent traversal
    "/../",  // Root parent traversal
    "..\\",  // Windows-style traversal
];

/// Sed injection patterns (command injection through sed)
const SED_INJECTION_PATTERNS: &[&str] = &[
    "sed.*e.*;",           // Command execution via sed
    "sed.*s/.*/[command]", // Replace with command
    "sed.*y/.*/[command]", // Translate with command
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
    "ls",
    "ll",
    "la", // List operations
    "cat",
    "head",
    "tail", // File reading
    "grep",
    "egrep",
    "fgrep", // Search
    "find",
    "locate", // File search
    "file",
    "stat",
    "du", // File info
    "echo",
    "pwd",
    "whoami",     // System info
    "which",      // Toolchain availability probes
    "command -v", // Toolchain availability probes
    "git status",
    "git log",  // Git read ops
    "git diff", // Git diff
    // PowerShell read-only cmdlets (the PowerShell tool runs the same
    // security analyzer; without these every Windows command scored as
    // risky). Unambiguous `Get-*`/probe cmdlets only — bare aliases like
    // `dir`/`type` substring-match too much ordinary prose.
    "get-childitem",
    "get-content",
    "get-item",
    "get-location",
    "get-command",
    "get-help",
    "get-process",
    "get-service",
    "get-member",
    "get-date",
    "get-psdrive",
    "get-volume",
    "select-string",
    "test-path",
    "measure-object",
];

/// PowerShell-specific destructive patterns
const PS_DESTRUCTIVE_PATTERNS: &[&str] = &[
    "Remove-Item -Recurse -Force",           // Recursive force delete
    "rm -Recurse -Force",                    // Alias recursive delete
    "ri -Recurse -Force",                    // Alias recursive delete
    "Remove-Item * -Recurse",                // Delete all in dir
    "Format-Volume",                         // Format volume
    "Stop-Computer",                         // Shutdown
    "Restart-Computer",                      // Reboot
    "Clear-Content",                         // Clear file contents
    "Remove-Service",                        // Remove service
    "Set-ExecutionPolicy Unrestricted",      // Lower security policy
    "Invoke-WebRequest | Invoke-Expression", // Download & execute
    "iex",                                   // Invoke-Expression (code execution)
    "IEX",                                   // Invoke-Expression variant
    "Invoke-Expression",                     // Direct code execution
    "& 'cmd.exe /c'",                        // Cmd bypass
    "Start-Process -Verb RunAs",             // Privilege escalation
    "net user",                              // User manipulation
    "net localgroup",                        // Group manipulation
    "reg delete",                            // Registry deletion
    "reg add HKLM",                          // Registry modification (system)
];

/// PowerShell confirmation-required patterns
const PS_CONFIRMATION_PATTERNS: &[&str] = &[
    "Remove-Item",      // Delete files
    "Move-Item",        // Move files
    "Copy-Item -Force", // Force copy
    "Set-Content",      // Overwrite file content
    "New-Item -Force",  // Force create
    "Stop-Process",     // Kill process
    "taskkill",         // Kill process
];

/// Shell variable expansion patterns used for bypass detection
const SHELL_EXPANSION_PATTERNS: &[&str] = &[
    "$'",  // ANSI-C quoting
    "$(",  // Command substitution
    "${",  // Parameter expansion
    "`",   // Backtick command substitution
    "$((", // Arithmetic expansion
    "$[",  // Legacy arithmetic expansion
];

/// Dangerous verb patterns (A4): when shell expansion syntax appears in a
/// command that also matches one of these, the expansion is treated as a
/// genuine bypass attempt and the command stays critical. Expansion syntax
/// alone (read-only probes such as `$(command -v shasum)`) is downgraded to
/// a warning instead of a rejection.
const DANGEROUS_VERB_PATTERNS: &[&str] = &[
    "rm -rf /",    // Recursive force delete of absolute paths
    "rm -fr /",    // Same, flag order swapped
    "mkfs",        // Format filesystem
    "fdisk",       // Partition table manipulation
    "dd of=/dev/", // Raw write to a device node
    "chmod 777 /", // World-writable root
    "chown -r",    // Recursive ownership change
    "sudo",        // Privilege escalation
    "| sh",        // Pipe into shell
    "|sh",         // Pipe into shell (no space)
    "| bash",      // Pipe into bash
    "|bash",       // Pipe into bash (no space)
    "-delete",     // find(1) bulk deletion
    "-exec rm",    // find(1) delegated deletion
];

/// Sensitive system paths that should never be accessed
const SENSITIVE_PATHS: &[&str] = &[
    "/etc/passwd",  // Password database
    "/etc/shadow",  // Shadow password file
    "/etc/sudoers", // Sudo configuration
    "/root/",       // Root home directory
    "/boot/",       // Boot files
    "/sys/",        // System filesystem
    "/proc/sys/",   // System configuration
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

    // FIRST: classify shell expansion syntax (eval finding A4: every command
    // containing `$(`, `${}` or backticks used to be rejected as a critical
    // "bypass", which stranded read-only probes such as
    // `$(command -v shasum)`). Now:
    //   - ANSI-C quoting (`$'...'`) stays critical: hex escapes can encode
    //     payloads that the textual checks below cannot see.
    //   - Plain expansion escalates only when a dangerous verb rides along
    //     (see `DANGEROUS_VERB_PATTERNS`); the dedicated checks below
    //     (sensitive paths, destructive patterns, pipe-to-shell, ...) still
    //     escalate on their own. Otherwise the command runs with warnings.
    let has_ansi_c_quoting = command.contains("$'");
    let has_command_substitution = command.contains("$(") || command.contains('`');
    let has_param_expansion = command.contains("${");
    let has_arith_expansion = command.contains("$[") || command.contains("$((");
    let has_expansion = SHELL_EXPANSION_PATTERNS.iter().any(|p| command.contains(p));

    if has_ansi_c_quoting {
        risk_level = SecurityLevel::Critical;
        warnings.push(
            "ANSI-C quoting detected: Can encode dangerous commands as hex escapes".to_string(),
        );
        is_destructive = true;
    }

    if has_expansion {
        // Informational: expansion syntax alone is no longer a rejection
        // reason, but the surface stays visible in the analysis output.
        if has_command_substitution {
            warnings
                .push("Command substitution detected: Can execute arbitrary commands".to_string());
        }

        if has_param_expansion {
            warnings.push("Parameter expansion detected: Can be used for obfuscation".to_string());
        }

        if has_arith_expansion {
            warnings
                .push("Arithmetic expansion detected: Review the computed expression".to_string());
        }

        // Escalate only when the expansion hides a genuinely dangerous verb.
        if DANGEROUS_VERB_PATTERNS
            .iter()
            .any(|p| lower_command.contains(p))
        {
            risk_level = SecurityLevel::Critical;
            warnings.push(
                "Dangerous verb combined with shell expansion: rewrite without expansion for review"
                    .to_string(),
            );
            is_destructive = true;
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
        warnings.push(
            "IFS manipulation detected: Common technique to bypass security checks".to_string(),
        );
        is_destructive = true;
    }

    // Check for base64/xxd encoding bypasses
    if (lower_command.contains("base64")
        || lower_command.contains("xxd")
        || lower_command.contains("od "))
        && (lower_command.contains("|") || lower_command.contains("$("))
    {
        risk_level = SecurityLevel::Critical;
        warnings.push(
            "Encoded command execution detected: base64/xxd used to hide commands".to_string(),
        );
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

    // review §P2-12: a head token in READ_ONLY_PATTERNS is necessary but
    // not sufficient for the whole command to be read-only. `find` was
    // the worst offender: \`find . -name '*.tmp' -delete\` was tagged
    // read-only + Low risk, so it slipped past RunBackground's
    // High-risk gate and could bulk-delete under the table. Do a second
    // pass looking for destructive predicates on read-only-tagged
    // commands; any hit escalates back to High so the existing
    // background-task gate (review §P1-2) blocks it.
    if is_read_only {
        const WRITE_PREDICATES: &[&str] = &[
            " -delete",
            " -exec ",
            " -execdir ",
            " -fdelete",
            " -print -delete", // belt-and-braces
        ];
        for p in WRITE_PREDICATES {
            if lower_command.contains(p) {
                if risk_level < SecurityLevel::High {
                    risk_level = SecurityLevel::High;
                }
                warnings.push(format!(
                    "read-only head token paired with destructive predicate '{p}'"
                ));
                is_read_only = false;
                break;
            }
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
                    || part_lower.trim().starts_with("eval")
                {
                    if risk_level < SecurityLevel::Critical {
                        risk_level = SecurityLevel::Critical;
                    }
                    warnings.push(format!(
                        "Dangerous pipe-to-shell detected: | {}",
                        part.trim()
                    ));
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
    if command.contains(">") && !is_read_only && risk_level < SecurityLevel::Medium {
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

/// One-line, executable remediation advice for a rejected command (A4).
/// Without it, a headless agent retries a near-identical command instead of
/// rewriting the approach.
fn security_rejection_hint(command: &str) -> &'static str {
    if SHELL_EXPANSION_PATTERNS.iter().any(|p| command.contains(p)) {
        "Suggestion: avoid command substitution/expansion - split the command into two steps (run the inner command first and use its output) or use absolute paths."
    } else {
        "Suggestion: rewrite with a safer equivalent - scope destructive operations to specific files, drop elevated privileges, or use read-only flags."
    }
}

/// Build the structured rejection output for a critical-risk command,
/// including the remediation hint (A4). Shared by the blocking and the
/// streaming Bash paths so both rejections carry the same contract.
fn security_rejected_output(command: &str, analysis: &SecurityAnalysis) -> ToolOutput {
    let error_msg = format!(
        "Command rejected due to critical security risk:\n{}\n\nRisk Level: {}\n\nWarnings:\n  - {}\n\n{}",
        command,
        describe_risk_level(analysis.risk_level),
        analysis.warnings.join("\n  - "),
        security_rejection_hint(command),
    );
    ToolOutput {
        content: error_msg,
        is_error: true,
        metadata: {
            let mut map = HashMap::new();
            map.insert("security_rejected".to_string(), json!(true));
            map.insert("risk_level".to_string(), json!(analysis.risk_level as i32));
            map.insert("warnings".to_string(), json!(analysis.warnings));
            map
        },
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
        let is_allowed = allowed_paths
            .iter()
            .any(|allowed| normalized.starts_with(allowed) || normalized == *allowed);

        if !is_allowed {
            return Err(PathValidationError::NotAllowed(path.to_string()));
        }
    }

    // Check for dangerous system paths
    let dangerous_prefixes = &[
        "/bin/",
        "/sbin/",
        "/usr/bin/",
        "/usr/sbin/",
        "/etc/",
        "/boot/",
        "/sys/",
        "/dev/",
        "/proc/",
        "/root/",
        "/var/run/",
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
    fn default_image() -> String {
        "ubuntu:22.04".to_string()
    }
    fn default_workdir() -> String {
        "/workspace".to_string()
    }
    fn default_network() -> String {
        "none".to_string()
    }
    fn default_readonly() -> bool {
        true
    }
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
        // Probe docker availability through the default process world (§4.11).
        let request = ProcessRequest::new("docker", &["info"]);
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            crate::defaults::process().run_async(&request),
        )
        .await;
        match result {
            Ok(Ok(o)) => o.exit.success,
            _ => false,
        }
    }

    /// Build the docker run argument list.
    ///
    /// review §P2-11: refuses to mount a workspace that canonicalises to "/"
    /// (model-controlled `cwd=/` would expose the host root filesystem to
    /// the container despite --read-only on rootfs / --network=none) and
    /// binds the mount :ro by default.
    fn build_args(
        &self,
        command: &str,
        cwd: Option<&str>,
        env: Option<&std::collections::HashMap<String, String>>,
    ) -> Result<Vec<String>, String> {
        let mut args = vec!["run".to_string(), "--rm".to_string()];

        // review §P2-11: the workspace mount is the most exposed surface
        // here. A model-controlled `cwd` of `/` would mount the entire
        // host root filesystem into the container; even a sandboxed
        // container (--read-only on rootfs, --network=none) can still
        // *read* every host file through this bind mount. Reject any
        // workspace that resolves outside the project's known safe root,
        // and pin the bind to read-only by default. Callers that really
        // need write access must explicitly opt out via the runtime env.
        let workspace_raw = cwd.unwrap_or(".");
        let abs_workspace = std::path::Path::new(workspace_raw)
            .canonicalize()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| workspace_raw.to_string());

        // Hard-coded safe root: refuse "/" and any path that canonicalizes
        // to it (e.g. "/.", "/usr/../"). This is intentionally simple
        // — the docker sandbox is for off-host execution, not for binding
        // arbitrary host roots.
        if abs_workspace == "/" {
            return Err(format!(
                "refusing to mount workspace '{workspace_raw}' as container root: \
                 use a project directory, not '/'"
            ));
        }

        // Bind mount with explicit :ro. The container's writable surface
        // is the small overlay that docker creates on top of rootfs; the
        // bind is read-only so the model can't tamper with the host even
        // when it has shell inside the container.
        args.push("-v".to_string());
        args.push(format!("{}:{}:ro", abs_workspace, self.config.workdir));
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

        Ok(args)
    }

    /// Execute a command inside a Docker container
    pub async fn execute(
        &self,
        command: &str,
        cwd: Option<&str>,
        env: Option<&std::collections::HashMap<String, String>>,
        timeout_ms: Option<u64>,
    ) -> Result<CommandOutput, std::io::Error> {
        let docker_args = self
            .build_args(command, cwd, env)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;

        let args: Vec<&str> = docker_args.iter().map(String::as_str).collect();
        let request = ProcessRequest::new("docker", &args);
        let world = crate::defaults::process();

        // Same resolution as the direct path: `None` means the 120 s default,
        // not unbounded.
        let timeout = resolve_timeout_ms(timeout_ms);
        let duration = std::time::Duration::from_millis(timeout);
        let output = tokio::time::timeout(duration, world.run_async(&request))
            .await
            .map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!("Docker command timed out after {timeout}ms"),
                )
            })?
            .map_err(|e| std::io::Error::other(format!("Docker execution failed: {e}")))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let exit_code = output.exit.code.unwrap_or(-1);
        let success = output.exit.success;

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
    /// Direct (unsandboxed) execution world.
    direct_process: Arc<dyn ProcessProvider>,
    /// Execution world with argv-level platform sandbox wrapping installed
    /// through the §4.11 spawn hook (`SandboxExecutorRewrite` over bwrap /
    /// Seatbelt / Docker). `None` when no backend was detected or the user
    /// disabled sandboxing via `SHANNON_SANDBOX=off`.
    process_sandbox: Option<Arc<dyn ProcessProvider>>,
    /// §4.12 sandbox denial classifier: inspects a failed captured run of an
    /// enforcing world and, when it looks kernel-denied, yields structured
    /// `sandbox_denied` metadata for the L0 record. `None` = no enforcing
    /// world (the historical shape).
    denial_classifier: Option<crate::sandbox::DenialClassifier>,
    /// Enforcement posture behind the structured `sandbox` metadata on
    /// every result.
    sandbox_posture: SandboxPosture,
}

impl Default for BashTool {
    fn default() -> Self {
        Self::new()
    }
}

/// One-line sandbox orientation appended to failed command output when a
/// process sandbox is active (§ sandbox self-description).
///
/// Without it the model sees a bare "Command failed with exit code 2" and
/// burns turns probing the filesystem or installing toolchains: it does not
/// know that paths outside the project are invisible inside the sandbox, or
/// that host toolchains (node, ...) may not exist there at all. Heuristics
/// stay conservative — only failures that look like a path/environment miss
/// get the note, so ordinary command failures stay clean.
fn sandbox_failure_note(sandboxed: bool, output: &CommandOutput) -> Option<String> {
    if !sandboxed || output.success {
        return None;
    }
    let stderr = output.stderr.to_lowercase();
    let looks_like_env_miss = stderr.contains("no such file")
        || stderr.contains("cannot access")
        || stderr.contains("permission denied")
        || stderr.contains("command not found")
        || stderr.contains("not found")
        || output.exit_code == 127;
    if !looks_like_env_miss {
        return None;
    }
    Some(
        "[sandbox] Commands run inside a sandbox with limited visibility: the project \
         root is available (Docker sandboxes mount it at /workspace) and /tmp is \
         writable, but paths outside the project are not visible and host toolchains \
         may be absent — probe with `command -v <tool>` and adapt instead of \
         installing packages."
            .to_string(),
    )
}

/// Sandbox enforcement posture of a [`BashTool`] — drives the structured
/// `sandbox` metadata stamped on every tool result.
///
/// The default posture is sandbox-on: when a platform backend is detected it
/// is used without any opt-in. Unsandboxed execution is either a degraded
/// host ([`SandboxPosture::Missing`], warned about loudly on every result)
/// or an explicit [`SandboxPosture::OptedOut`] (`SHANNON_SANDBOX=off`).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SandboxPosture {
    /// A platform backend (bubblewrap/Seatbelt/Docker) wraps every command.
    Active,
    /// Detection ran but no backend exists: commands run unsandboxed and
    /// every result carries the loud structured warning.
    Missing,
    /// Explicitly disabled via `SHANNON_SANDBOX=off`.
    OptedOut,
    /// No detection performed (plain constructor: sub-agent registries,
    /// remote worlds). No sandbox metadata is emitted.
    Undetected,
}

impl SandboxPosture {
    /// Value of the structured `sandbox` metadata entry (`None` = emit
    /// nothing).
    fn metadata_label(&self) -> Option<&'static str> {
        match self {
            SandboxPosture::Active => Some("on"),
            SandboxPosture::Missing | SandboxPosture::OptedOut => Some("off"),
            SandboxPosture::Undetected => None,
        }
    }

    /// One-line warning carried in the result metadata. `None` when the
    /// posture needs no warning.
    fn warning(&self) -> Option<&'static str> {
        match self {
            SandboxPosture::Missing => Some(
                "Sandbox: OFF — no sandbox backend (bubblewrap/Seatbelt/Docker) was detected, \
                 so commands run unsandboxed on the host. Shannon sandboxes by default when \
                 a backend is available; set SHANNON_SANDBOX=off to disable sandboxing \
                 explicitly.",
            ),
            SandboxPosture::OptedOut => Some(
                "Sandbox: OFF — disabled via SHANNON_SANDBOX=off; commands run unsandboxed \
                 on the host.",
            ),
            SandboxPosture::Active | SandboxPosture::Undetected => None,
        }
    }
}

/// Posture resolution from the detected backend type plus the
/// `SHANNON_SANDBOX` env value (the explicit opt-out). Pure so tests can
/// pin every combination without touching the host.
pub(crate) fn resolve_sandbox_posture(
    sandbox_type: SandboxType,
    shannon_sandbox_env: Option<&str>,
) -> SandboxPosture {
    let backend_available = !matches!(sandbox_type, SandboxType::None);
    match shannon_sandbox_env
        .map(str::trim)
        .map(str::to_ascii_lowercase)
    {
        Some(ref value) if value == "off" => SandboxPosture::OptedOut,
        _ if backend_available => SandboxPosture::Active,
        _ => SandboxPosture::Missing,
    }
}

impl BashTool {
    /// Description advertised to the model. Shared by the plain and
    /// sandboxed constructors (runtime behavior differs; the contract is
    /// the same — the sandbox self-description in the system prompt
    /// explains the active restrictions).
    fn default_description() -> &'static str {
        "Executes a bash command and returns stdout/stderr.\n\
         \n\
         Each call runs in a fresh shell in the working directory (no state\n\
         carries over; use `&&` to combine steps). Output is capped by the\n\
         harness — avoid commands that dump large files; use head/tail/grep\n\
         to scope output. A per-call `timeout` (ms) is supported: it defaults\n\
         to 120000 when omitted (override with SHANNON_BASH_TIMEOUT_MS) and\n\
         is hard-capped at 600000. Long-running or server processes should\n\
         use RunBackground and be polled with WaitForLog. When a sandbox is\n\
         active the command runs with restricted filesystem/network access —\n\
         the tool result reports denials. Sandboxing is applied by default\n\
         when a platform sandbox backend is available; set SHANNON_SANDBOX=off\n\
         to opt out."
    }

    pub fn new() -> Self {
        Self {
            description: Self::default_description().to_string(),
            sandbox: None,
            direct_process: crate::defaults::process(),
            process_sandbox: None,
            denial_classifier: None,
            // No detection was performed on this constructor (used for
            // sub-agent registries / remote worlds) — emit no sandbox
            // metadata rather than a misleading on/off.
            sandbox_posture: SandboxPosture::Undetected,
        }
    }

    /// Create a BashTool that routes commands through a Docker sandbox
    pub fn with_docker_sandbox(config: DockerSandboxConfig) -> Self {
        Self {
            // Keep the full default guidance (the contract is the same; only
            // the execution environment differs) and append the sandbox
            // specifics the model needs to plan around.
            description: format!(
                "{}\n\
                 \n\
                 All commands run inside a Docker sandbox: the project is\n\
                 mounted at {}, the container network mode is '{}', and the\n\
                 root filesystem is{} read-only.",
                Self::default_description(),
                config.workdir,
                config.network,
                if config.readonly_root { "" } else { " not" },
            ),
            sandbox: Some(DockerSandbox::new(config)),
            direct_process: crate::defaults::process(),
            process_sandbox: None,
            denial_classifier: None,
            sandbox_posture: SandboxPosture::Active,
        }
    }

    /// Inject the execution worlds used for non-Docker spawns.
    ///
    /// `direct_process` handles plain `bash -c` runs; `sandboxed_process`
    /// (when supplied) is consulted for sandboxed runs — typically a
    /// [`shannon_core::providers::LocalProcess`] carrying a `SpawnRewrite`.
    pub fn with_worlds(mut self, direct_process: Arc<dyn ProcessProvider>) -> Self {
        self.direct_process = direct_process;
        self
    }

    /// Create a BashTool with a platform process sandbox (bwrap/Seatbelt/Docker).
    ///
    /// Default-on posture: the auto-detected backend is used **without any
    /// opt-in**; when no backend is available commands run unsandboxed and
    /// every tool result carries a loud, structured `"sandbox": "off"`
    /// warning. `SHANNON_SANDBOX=off` disables sandboxing explicitly.
    ///
    /// `SHANNON_SANDBOX_EXTRA_RO_MOUNTS` (colon-separated host directories) is
    /// added as extra read-only mounts on the Docker backend — the escape
    /// hatch for making host toolchains (e.g. `/usr/local`, a nvm checkout)
    /// visible inside the sandbox without changing code.
    pub fn with_process_sandbox(project_dir: impl Into<std::path::PathBuf>) -> Self {
        let mut config = SandboxConfig::new(project_dir);
        if let Ok(extra) = std::env::var("SHANNON_SANDBOX_EXTRA_RO_MOUNTS") {
            for dir in extra.split(':').filter(|s| !s.is_empty()) {
                config = config.readonly_mount(dir);
            }
        }
        let env_override = std::env::var("SHANNON_SANDBOX").ok();
        Self::with_detected_sandbox(SandboxExecutor::new(config), env_override.as_deref())
    }

    /// Assemble the tool from an already-constructed executor plus the
    /// `SHANNON_SANDBOX` env value — the seam tests use to pin behavior per
    /// detected backend without depending on the host's installed tooling.
    pub(crate) fn with_detected_sandbox(
        executor: SandboxExecutor,
        shannon_sandbox_env: Option<&str>,
    ) -> Self {
        let sandbox_type = executor.sandbox_type();
        let posture = resolve_sandbox_posture(sandbox_type, shannon_sandbox_env);
        // The legacy argv-level sandbox becomes a §4.11 SpawnRewrite installed
        // on a LocalProcess — identical wrapping, one seam further down.
        let sandboxed_process: Option<Arc<dyn ProcessProvider>> = match posture {
            SandboxPosture::Active => Some(Arc::new(LocalProcess::with_rewrite(Arc::new(
                SandboxExecutorRewrite::new(Arc::new(executor)),
            )))),
            _ => None,
        };
        Self {
            description: match posture {
                SandboxPosture::Active => format!(
                    "Executes bash commands (sandboxed via {sandbox_type}). Inside the \
                     sandbox the project is available at its mounted path (Docker: \
                     /workspace) and only the project plus /tmp are writable; paths \
                     outside the project are not visible and host toolchains may be \
                     absent — probe availability with `command -v <tool>` and adapt \
                     instead of installing packages. Sandboxing is applied by default \
                     when a backend is available; set SHANNON_SANDBOX=off to opt out."
                ),
                _ => Self::default_description().to_string(),
            },
            sandbox: None,
            direct_process: crate::defaults::process(),
            process_sandbox: sandboxed_process,
            denial_classifier: None,
            sandbox_posture: posture,
        }
    }

    /// Structured sandbox metadata for every result: `"sandbox"`:
    /// `"on"|"off"` plus a one-line `"sandbox_warning"` whenever commands
    /// run unsandboxed — so a degraded host is visible on every tool result,
    /// not just in startup logs.
    fn apply_sandbox_metadata(&self, map: &mut HashMap<String, serde_json::Value>) {
        if let Some(label) = self.sandbox_posture.metadata_label() {
            map.insert("sandbox".to_string(), json!(label));
        }
        if let Some(warning) = self.sandbox_posture.warning() {
            map.insert("sandbox_warning".to_string(), json!(warning));
        }
    }

    /// Content-suffix warning for the degraded (no-backend) posture. An
    /// explicit `SHANNON_SANDBOX=off` stays metadata-only — the user chose
    /// it — while a missing backend is warned about loudly in the content
    /// the model reads.
    fn sandbox_content_warning(&self) -> Option<String> {
        match self.sandbox_posture {
            SandboxPosture::Missing => self.sandbox_posture.warning().map(|w| format!("\n{w}")),
            _ => None,
        }
    }

    /// Attach a §4.12 sandbox-denial classifier (assembly-time seam).
    pub fn with_denial_classifier(mut self, classifier: DenialClassifier) -> Self {
        self.denial_classifier = Some(classifier);
        self
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

    /// Execute a command through the sandboxed execution world.
    ///
    /// The platform wrap (bwrap/Seatbelt/Docker argv rewriting) happens
    /// inside the injected [`ProcessProvider`] via its §4.11 `SpawnRewrite`
    /// seam (`SandboxExecutorRewrite`) — this layer only describes intent.
    async fn execute_command_sandboxed(
        command: &str,
        cwd: Option<&str>,
        env: Option<&std::collections::HashMap<String, String>>,
        timeout_ms: Option<u64>,
        world: &dyn ProcessProvider,
    ) -> Result<CommandOutput, std::io::Error> {
        run_shell_captured(world, "bash", "-c", command, cwd, env, timeout_ms).await
    }

    /// Provider-injected captured run (§4.11): executes through the given
    /// process world instead of building a spawn locally.
    async fn execute_command_with_world(
        world: &dyn ProcessProvider,
        command: &str,
        cwd: Option<&str>,
        env: Option<&std::collections::HashMap<String, String>>,
        timeout_ms: Option<u64>,
    ) -> Result<CommandOutput, std::io::Error> {
        run_shell_captured(world, "bash", "-c", command, cwd, env, timeout_ms).await
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
                    "description": "Optional timeout in milliseconds (default 120000, hard cap 600000)",
                    "default": 120000
                },
                "env": {
                    "type": "object",
                    "description": "Optional environment variables",
                    "additionalProperties": { "type": "string" }
                },
                "use_pty": {
                    "type": "boolean",
                    "description": "Run in a pseudo-terminal for interactive command support (default: false)"
                },
                "stream_delay_ms": {
                    "type": "integer",
                    "description": "Delay in ms before streamed output begins (default: 500); faster commands skip streaming entirely"
                }
            },
            "required": ["command"],
            "additionalProperties": false
        })
    }

    async fn execute(
        &self,
        input: serde_json::Value,
    ) -> Result<ToolOutput, shannon_core::tools::ToolError> {
        let bash_input: BashInput = match serde_json::from_value(input) {
            Ok(input) => input,
            Err(e) => {
                return Ok(ToolOutput {
                    content: format!("Invalid bash input: {e}"),
                    is_error: true,
                    metadata: HashMap::new(),
                });
            }
        };

        // Perform security analysis before execution
        let analysis = analyze_command_security(&bash_input.command);

        // Reject critical risk commands
        if analysis.risk_level >= SecurityLevel::Critical {
            return Ok(security_rejected_output(&bash_input.command, &analysis));
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

        // World capability gate: the PTY path and the two local argv-sandbox
        // branches hold *local* providers that would silently shadow an
        // injected remote world (Bash would run locally while file tools run
        // remotely). On remote worlds everything routes through the injected
        // world; the local-only features degrade with an explicit note.
        let remote_world = self.direct_process.capabilities().is_remote;
        let local_only_requested =
            bash_input.use_pty || self.sandbox.is_some() || self.process_sandbox.is_some();

        // Execute the command (PTY mode for interactive, otherwise sandboxed/direct)
        // P0-11: PTY execution is inherently unsandboxed (raw pty, no argv
        // rewrite). When a process sandbox is active, PTY would silently
        // bypass it — refuse the combination instead of escaping the sandbox.
        let output_result = if bash_input.use_pty
            && !remote_world
            && (self.sandbox.is_some() || self.process_sandbox.is_some())
        {
            Ok(CommandOutput {
                stdout: String::new(),
                stderr: "PTY mode is unavailable while a process sandbox is active \
                         (PTY cannot be sandboxed). Re-run without use_pty."
                    .to_string(),
                exit_code: 126,
                success: false,
            })
        } else if bash_input.use_pty && !remote_world {
            let cmd = bash_input.command.clone();
            let cwd = bash_input.cwd.clone();
            let env = bash_input.env.clone();
            let timeout = bash_input.timeout;
            tokio::task::spawn_blocking(move || {
                match crate::pty::execute_in_pty(&cmd, cwd.as_deref(), env.as_ref(), timeout) {
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
        } else if let Some(sandbox) = self.sandbox.as_ref().filter(|_| !remote_world) {
            sandbox
                .execute(
                    &bash_input.command,
                    bash_input.cwd.as_deref(),
                    bash_input.env.as_ref(),
                    bash_input.timeout,
                )
                .await
        } else if let Some(ps) = self.process_sandbox.as_ref().filter(|_| !remote_world) {
            Self::execute_command_sandboxed(
                &bash_input.command,
                bash_input.cwd.as_deref(),
                bash_input.env.as_ref(),
                bash_input.timeout,
                ps.as_ref(),
            )
            .await
        } else {
            Self::execute_command_with_world(
                self.direct_process.as_ref(),
                &bash_input.command,
                bash_input.cwd.as_deref(),
                bash_input.env.as_ref(),
                bash_input.timeout,
            )
            .await
        };

        let output = match output_result {
            Ok(mut output) => {
                if remote_world && local_only_requested {
                    output.stdout = format!(
                        "[remote target] PTY and local sandbox modes are unavailable; \
                         executed as a piped command on the remote target.\n{}",
                        output.stdout
                    );
                }
                output
            }
            Err(e) => {
                return Ok(ToolOutput {
                    content: format!("Command execution failed: {e}"),
                    is_error: true,
                    metadata: HashMap::new(),
                });
            }
        };

        let sandbox_off_warning = self.sandbox_content_warning();
        let content = if output.success {
            format!(
                "{}{}{}",
                output.stdout,
                command_description,
                sandbox_off_warning.unwrap_or_default()
            )
        } else {
            let sandbox_note = sandbox_failure_note(self.process_sandbox.is_some(), &output);
            format!(
                "{}Command failed with exit code {}: {}{}{}{}",
                command_description,
                output.exit_code,
                output.stderr,
                if command_description.is_empty() {
                    "\n"
                } else {
                    ""
                },
                sandbox_note
                    .map(|note| format!("\n{note}"))
                    .unwrap_or_default(),
                sandbox_off_warning.unwrap_or_default(),
            )
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
                // Structured sandbox posture: "sandbox": "on"|"off" (plus a
                // one-line warning when off) on EVERY result — a degraded
                // host is visible per-call, not just at startup.
                self.apply_sandbox_metadata(&mut map);
                // §4.12: kernel-denied operations of an enforcing world get
                // the canonical classification so the L0 `tool/result.meta`
                // records them.
                if !output.success {
                    if let Some(classifier) = self.denial_classifier.as_ref() {
                        if let Some(denial) = classifier(&output) {
                            for (key, value) in crate::sandbox::denial_metadata(&denial) {
                                map.insert(key, value);
                            }
                        }
                    }
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
/// cursor movement, screen clearing, and other control sequences. The regex
/// is compiled once (this runs per streamed line) via `OnceLock`.
fn strip_ansi(s: &str) -> String {
    // Strip all CSI sequences except SGR (which ends with 'm')
    static STRIP_ANSI_RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = STRIP_ANSI_RE.get_or_init(|| {
        regex::Regex::new(r"\x1b\[[0-9;]*[A-HJ-Za-ln-z]").expect("strip_ansi regex is valid")
    });
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
            Err(e) => {
                return Ok(ToolOutput {
                    content: format!("Invalid bash input: {e}"),
                    is_error: true,
                    metadata: HashMap::new(),
                });
            }
        };

        let analysis = analyze_command_security(&bash_input.command);

        if analysis.risk_level >= SecurityLevel::Critical {
            return Ok(security_rejected_output(&bash_input.command, &analysis));
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

        // Only stream direct (non-PTY, non-sandbox) commands; a remote world
        // has no local PTY/sandbox branches (capability-gated below), so it
        // always streams.
        let remote_world = self.direct_process.capabilities().is_remote;
        let use_streaming = remote_world
            || (!bash_input.use_pty && self.sandbox.is_none() && self.process_sandbox.is_none());

        if !use_streaming {
            // Delegate to blocking execute — wraps in a helper to reuse
            // the same logic. We call execute() directly to avoid duplicating.
            return self
                .execute(serde_json::to_value(&bash_input).unwrap_or_default())
                .await;
        }

        // Streaming path: spawn the process and read stdout line-by-line.
        // The child comes from the injected process world via the §4.11
        // piped-spawn seam; the provider keeps kill-on-drop semantics so a
        // cancelled future cannot leave orphans behind.
        let mut request = ProcessRequest::new("bash", &["-c", &bash_input.command]);
        if let Some(ref dir) = bash_input.cwd {
            request.cwd = Some(dir.clone().into());
        }
        if let Some(ref env_vars) = bash_input.env {
            for (key, value) in env_vars {
                request.env.push((key.clone(), value.clone()));
            }
        }

        let spec = shannon_tool_interface::PipedSpawn {
            request,
            pipe_stdin: false,
            pipe_stdout: true,
            pipe_stderr: true,
            kill_on_drop: true,
        };

        let mut child = self
            .direct_process
            .spawn_piped(&spec)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to spawn command: {e}")))?;

        let stdout = child
            .take_stdout()
            .ok_or_else(|| ToolError::ExecutionFailed("Failed to capture stdout".to_string()))?;
        let stderr = child
            .take_stderr()
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
                                        progress.send(bl).await;
                                    }
                                    buffered_lines.clear();
                                }
                            } else {
                                progress.send(&cleaned).await;
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
                                        progress.send(bl).await;
                                    }
                                    buffered_lines.clear();
                                }
                            } else {
                                progress.send(&tagged).await;
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

        let status = child
            .wait()
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to wait for command: {e}")))?;

        let exit_code = status.code.unwrap_or(-1);
        let success = status.success;

        let sandbox_off_warning = self.sandbox_content_warning();
        let content = if success {
            format!(
                "{stdout_buf}{command_description}{}",
                sandbox_off_warning.unwrap_or_default()
            )
        } else {
            let sandbox_note = sandbox_failure_note(
                self.process_sandbox.is_some(),
                &CommandOutput {
                    stdout: stdout_buf.clone(),
                    stderr: stderr_buf.clone(),
                    exit_code,
                    success,
                },
            );
            format!(
                "{}Command failed with exit code {}: {}{}{}{}",
                command_description,
                exit_code,
                stderr_buf,
                if command_description.is_empty() {
                    "\n"
                } else {
                    ""
                },
                sandbox_note
                    .map(|note| format!("\n{note}"))
                    .unwrap_or_default(),
                sandbox_off_warning.unwrap_or_default(),
            )
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
                // Same structured posture metadata as the captured path.
                self.apply_sandbox_metadata(&mut map);
                map
            },
        })
    }
}

/// PowerShell tool implementation
pub struct PowerShellTool {
    description: String,
    /// Process world backing powershell invocations (§4.11).
    process: Arc<dyn ProcessProvider>,
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
            process: crate::defaults::process(),
        }
    }

    /// Inject a process-world override (sandbox/remote assemblies).
    pub fn with_process(mut self, process: Arc<dyn ProcessProvider>) -> Self {
        self.process = process;
        self
    }

    async fn execute_command(
        &self,
        command: &str,
        cwd: Option<&str>,
        env: Option<&std::collections::HashMap<String, String>>,
        timeout_ms: Option<u64>,
    ) -> Result<CommandOutput, std::io::Error> {
        run_shell_captured(
            self.process.as_ref(),
            "powershell",
            "-Command",
            command,
            cwd,
            env,
            timeout_ms,
        )
        .await
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
                    "description": "Optional environment variables",
                    "additionalProperties": { "type": "string" }
                }
            },
            "required": ["command"],
            "additionalProperties": false
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

        let output = self
            .execute_command(
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
            format!(
                "Command failed with exit code {}: {}",
                output.exit_code, output.stderr
            )
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
            description: "Wait for a specified duration without holding a shell process"
                .to_string(),
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

        let output = self
            .execute_sleep(sleep_input.duration_ms)
            .await
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
    fn is_read_only(&self) -> bool {
        true
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::{CommandOutput, sandbox_failure_note};

    #[test]
    fn sandbox_failure_note_only_fires_for_sandboxed_env_misses() {
        let miss = CommandOutput {
            stdout: String::new(),
            stderr: "ls: cannot access '/opt/x': No such file or directory".to_string(),
            exit_code: 2,
            success: false,
        };
        let note = sandbox_failure_note(true, &miss).expect("env-miss failure gets a note");
        assert!(note.contains("[sandbox]"));
        assert!(note.contains("command -v"));

        // Ordinary command failure (e.g. grep no-match: exit 1, empty
        // stderr) stays clean — the note must not add noise to every
        // non-zero exit.
        let ordinary = CommandOutput {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 1,
            success: false,
        };
        assert!(sandbox_failure_note(true, &ordinary).is_none());

        // Successful runs never get the note.
        let ok = CommandOutput {
            stdout: "ok".to_string(),
            stderr: String::new(),
            exit_code: 0,
            success: true,
        };
        assert!(sandbox_failure_note(true, &ok).is_none());

        // Without an active sandbox: never.
        assert!(sandbox_failure_note(false, &miss).is_none());
    }

    #[test]
    fn sandbox_failure_note_covers_command_not_found() {
        let miss = CommandOutput {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 127,
            success: false,
        };
        let note = sandbox_failure_note(true, &miss).expect("127 gets a note");
        assert!(note.contains("[sandbox]"));
    }

    /// Remote-capable fake process world: records the last program it was
    /// asked to run and reports `is_remote`.
    #[derive(Default)]
    struct RemoteProbeProcess {
        last_program: std::sync::Mutex<Option<String>>,
    }

    #[async_trait]
    impl shannon_tool_interface::ProcessProvider for RemoteProbeProcess {
        fn run_blocking(
            &self,
            request: &ProcessRequest,
        ) -> std::io::Result<shannon_tool_interface::CapturedOutput> {
            *self.last_program.lock().unwrap() = Some(request.program.clone());
            Ok(shannon_tool_interface::CapturedOutput {
                stdout: format!("ran:{}", request.program).into_bytes(),
                stderr: Vec::new(),
                exit: shannon_tool_interface::ProcessExit::from_code(0),
            })
        }

        async fn run_async(
            &self,
            request: &ProcessRequest,
        ) -> std::io::Result<shannon_tool_interface::CapturedOutput> {
            self.run_blocking(request)
        }

        async fn spawn_piped(
            &self,
            _spec: &shannon_tool_interface::PipedSpawn,
        ) -> std::io::Result<Box<dyn shannon_tool_interface::PipedChild>> {
            Err(std::io::Error::other("remote probe has no children"))
        }

        fn capabilities(&self) -> shannon_tool_interface::ExecCaps {
            shannon_tool_interface::ExecCaps { is_remote: true }
        }
    }

    #[tokio::test]
    async fn remote_world_bypasses_local_pty_and_sandbox_branches() {
        let probe = Arc::new(RemoteProbeProcess::default());
        // PTY requested + local argv sandbox installed: both local-only
        // branches must be skipped on a remote world.
        let tool = BashTool::with_process_sandbox("/tmp").with_worlds(probe.clone() as _);
        let input = serde_json::json!({
            "command": "echo hi",
            "use_pty": true,
        });
        let output = Tool::execute(&tool, input).await.unwrap();
        assert!(
            output.content.contains("[remote target]"),
            "remote fallback must be announced, got: {}",
            output.content
        );
        assert!(
            output.content.contains("ran:bash"),
            "command must run through the injected remote world, got: {}",
            output.content
        );
        let last = probe.last_program.lock().unwrap().clone();
        assert_eq!(last.as_deref(), Some("bash"));
    }

    #[tokio::test]
    async fn local_world_keeps_pty_and_sandbox_preference() {
        // On the local world the sandbox description is preserved and no
        // remote note is emitted.
        let tool = BashTool::with_process_sandbox("/tmp");
        let input = serde_json::json!({ "command": "echo hi" });
        let output = Tool::execute(&tool, input).await.unwrap();
        assert!(!output.content.contains("[remote target]"));
    }

    use super::*;

    // ── Sandbox default-on posture (backend used without opt-in;
    //    SHANNON_SANDBOX=off opts out; degraded hosts warn per result) ────

    #[test]
    fn sandbox_posture_is_active_by_default_when_backend_detected() {
        assert_eq!(
            resolve_sandbox_posture(SandboxType::Bubblewrap, None),
            SandboxPosture::Active
        );
        assert_eq!(
            resolve_sandbox_posture(SandboxType::Seatbelt, None),
            SandboxPosture::Active
        );
        assert_eq!(
            resolve_sandbox_posture(SandboxType::Docker, None),
            SandboxPosture::Active
        );
        // Non-off env values keep the default-on posture.
        assert_eq!(
            resolve_sandbox_posture(SandboxType::Seatbelt, Some("local")),
            SandboxPosture::Active
        );
    }

    #[test]
    fn sandbox_posture_is_missing_without_a_backend() {
        assert_eq!(
            resolve_sandbox_posture(SandboxType::None, None),
            SandboxPosture::Missing
        );
    }

    #[test]
    fn sandbox_posture_env_off_overrides_an_available_backend() {
        assert_eq!(
            resolve_sandbox_posture(SandboxType::Bubblewrap, Some("off")),
            SandboxPosture::OptedOut
        );
        // Case/whitespace tolerant, and off wins even with no backend.
        assert_eq!(
            resolve_sandbox_posture(SandboxType::None, Some(" OFF ")),
            SandboxPosture::OptedOut
        );
    }

    #[tokio::test]
    async fn bash_without_backend_warns_in_metadata_and_content() {
        let mut tool = BashTool::new();
        tool.sandbox_posture = SandboxPosture::Missing; // degraded-host posture
        let output = Tool::execute(&tool, json!({ "command": "echo hi" }))
            .await
            .unwrap();
        assert_eq!(output.metadata["sandbox"], "off");
        let warning = output.metadata["sandbox_warning"].as_str().unwrap();
        assert!(warning.contains("no sandbox backend"), "{warning}");
        assert!(
            warning.contains("SHANNON_SANDBOX=off"),
            "warning documents the opt-out: {warning}"
        );
        // Loud in the content too: the model reads the result, not the logs.
        assert!(
            output.content.contains("Sandbox: OFF"),
            "{}",
            output.content
        );
    }

    #[tokio::test]
    async fn bash_env_opt_out_reports_off_metadata_without_content_warning() {
        let mut tool = BashTool::new();
        tool.sandbox_posture = SandboxPosture::OptedOut; // explicit user choice
        let output = Tool::execute(&tool, json!({ "command": "echo hi" }))
            .await
            .unwrap();
        assert_eq!(output.metadata["sandbox"], "off");
        assert!(
            output.metadata["sandbox_warning"]
                .as_str()
                .unwrap()
                .contains("SHANNON_SANDBOX=off")
        );
        assert!(
            !output.content.contains("Sandbox: OFF"),
            "an explicit opt-out must not spam every result: {}",
            output.content
        );
    }

    #[tokio::test]
    async fn bash_active_posture_reports_on() {
        let mut tool = BashTool::new();
        tool.sandbox_posture = SandboxPosture::Active;
        let output = Tool::execute(&tool, json!({ "command": "echo hi" }))
            .await
            .unwrap();
        assert_eq!(output.metadata["sandbox"], "on");
        assert!(output.metadata.get("sandbox_warning").is_none());
        assert!(!output.content.contains("Sandbox: OFF"));
    }

    #[tokio::test]
    async fn bash_undetected_posture_emits_no_sandbox_metadata() {
        // Plain BashTool::new(): no detection ran, so no claim is made.
        let tool = BashTool::new();
        let output = Tool::execute(&tool, json!({ "command": "echo hi" }))
            .await
            .unwrap();
        assert!(output.metadata.get("sandbox").is_none());
    }

    #[test]
    fn with_detected_sandbox_honors_env_off_over_available_backend() {
        let executor = SandboxExecutor::new(SandboxConfig::new("/tmp"));
        let tool = BashTool::with_detected_sandbox(executor, Some("off"));
        assert_eq!(tool.sandbox_posture, SandboxPosture::OptedOut);
        assert!(
            tool.process_sandbox.is_none(),
            "SHANNON_SANDBOX=off must remove the argv-level sandbox"
        );
        assert_eq!(tool.description(), BashTool::default_description());
    }

    #[test]
    fn with_detected_sandbox_installs_backend_by_default() {
        // Host-shape-dependent like the other seam tests: on a backend-
        // capable host the argv sandbox is installed with no opt-in; on a
        // degraded host the posture degrades to Missing (never silently off).
        let executor = SandboxExecutor::new(SandboxConfig::new("/tmp"));
        let tool = BashTool::with_detected_sandbox(executor, None);
        match tool.sandbox_posture {
            SandboxPosture::Active => {
                assert!(tool.process_sandbox.is_some());
                assert!(tool.description.contains("sandboxed via"));
                assert!(tool.description.contains("SHANNON_SANDBOX=off"));
            }
            SandboxPosture::Missing => {
                assert!(tool.process_sandbox.is_none());
            }
            other => panic!("unexpected posture from detection: {other:?}"),
        }
    }

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
        let args = sandbox
            .build_args("echo hello", None, None)
            .expect("build_args must succeed for default config");

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
        let args = sandbox
            .build_args("env", None, Some(&env))
            .expect("build_args must succeed");

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
        let args = sandbox
            .build_args("ls", None, None)
            .expect("build_args must succeed");

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
        let args = sandbox
            .build_args("ls", None, None)
            .expect("build_args must succeed");

        assert!(args.contains(&"/host/path:/container/path".to_string()));
    }

    // ---- review §P2-11: workspace mount hardening ----

    #[test]
    fn test_docker_build_args_rejects_root_workspace() {
        // review §P2-11: a model-controlled `cwd` of "/" would expose the
        // entire host root filesystem to the container. build_args must
        // refuse this with an explicit Err.
        let sandbox = DockerSandbox::new(DockerSandboxConfig::default());
        let err = sandbox
            .build_args("ls", Some("/"), None)
            .expect_err("must refuse '/'");
        assert!(
            err.contains("refusing to mount workspace"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn test_docker_build_args_bind_is_read_only() {
        // review §P2-11: workspace bind mount must be :ro by default so a
        // model shell inside the container cannot tamper with the host
        // filesystem even when --read-only on rootfs is in effect (the
        // bind mount is independent of the overlay on top of rootfs).
        let sandbox = DockerSandbox::new(DockerSandboxConfig::default());
        let args = sandbox
            .build_args("ls", Some("/tmp"), None)
            .expect("build_args must succeed");
        let mount = args
            .iter()
            .find(|a| a.starts_with("/tmp:"))
            .expect("workspace mount must be present");
        assert!(
            mount.ends_with(":ro"),
            "workspace mount must be :ro by default, got: {mount}"
        );
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
    assert!(
        analysis
            .warnings
            .iter()
            .any(|w| w.contains("ANSI-C quoting"))
    );

    // Command substitution bypass
    let analysis = analyze_command_security("echo $(rm -rf /)");
    assert_eq!(analysis.risk_level, SecurityLevel::Critical);
    assert!(
        analysis
            .warnings
            .iter()
            .any(|w| w.contains("Command substitution"))
    );

    // Parameter expansion bypass
    let analysis = analyze_command_security("echo ${HOME}/../../etc/passwd");
    assert_eq!(analysis.risk_level, SecurityLevel::Critical);
    assert!(
        analysis
            .warnings
            .iter()
            .any(|w| w.contains("Parameter expansion"))
    );
}

// ── A4: shell expansion risk grading ─────────────────────────────────────
//
// Eval finding A4 (docs/eval-findings-2026-09-glm.md): the analyzer used to
// reject every command containing `$(`, `${}` or backticks as a critical
// security risk, stranding read-only probes such as `$(command -v shasum)`
// and making headless agents retry near-identical commands.

#[test]
fn test_read_only_expansion_downgraded_from_critical() {
    // Expansion syntax in an otherwise read-only/harmless command must not
    // be rejected anymore.
    for cmd in [
        "$(command -v shasum)",
        "echo $(pwd)",
        "ls $(pwd)",
        "echo ${HOME}",
        "cat `pwd`/README.md",
        "grep foo $(pwd)/bar.txt",
        "which $(echo cargo)",
    ] {
        let analysis = analyze_command_security(cmd);
        assert!(
            analysis.risk_level < SecurityLevel::Critical,
            "read-only expansion must be allowed to execute: {cmd} -> {:?}",
            analysis.risk_level
        );
    }
}

#[test]
fn test_expansion_with_dangerous_verb_still_critical() {
    // Expansion combined with a genuinely dangerous verb keeps the critical
    // rating and is rejected.
    for cmd in [
        "echo $(rm -rf /)",
        "`rm -rf /`",
        "echo $(sudo ls /root)",
        "echo $(dd if=/dev/zero of=/dev/sda)",
        "echo $(chmod 777 /)",
        "echo $(mkfs.ext4 /dev/sda1)",
        "find $(pwd) -name '*.tmp' -delete",
        "find $(pwd) -exec rm {} \\;",
        "echo $(pwd) | bash",
    ] {
        let analysis = analyze_command_security(cmd);
        assert_eq!(
            analysis.risk_level,
            SecurityLevel::Critical,
            "expansion combined with a dangerous verb must stay critical: {cmd}"
        );
    }
}

#[tokio::test]
async fn test_read_only_expansion_command_executes_end_to_end() {
    let tool = BashTool::new();
    let output = Tool::execute(&tool, json!({"command": "echo $(pwd)"}))
        .await
        .unwrap();
    assert!(
        !output.is_error,
        "read-only expansion must execute, got: {}",
        output.content
    );
    assert!(
        !output.content.contains("security risk"),
        "allowed expansion must not look like a rejection, got: {}",
        output.content
    );
}

#[tokio::test]
async fn test_security_rejection_includes_remediation_hint() {
    let tool = BashTool::new();
    let output = Tool::execute(&tool, json!({"command": "$(rm -rf /)"}))
        .await
        .unwrap();
    assert!(output.is_error);
    assert!(
        output.content.contains("Suggestion:"),
        "rejection must carry an actionable remediation hint, got: {}",
        output.content
    );
    assert!(
        output.content.contains("absolute paths"),
        "expansion rejection must suggest the two-step/absolute-path rewrite, got: {}",
        output.content
    );
}

#[tokio::test]
async fn test_streaming_security_rejection_includes_remediation_hint() {
    let tool = BashTool::new();
    let sender = std::sync::Arc::new(CollectSender {
        lines: std::sync::Mutex::new(Vec::new()),
    });
    let result = tool
        .execute_streaming(json!({"command": "rm -rf /"}), sender)
        .await
        .unwrap();
    assert!(result.is_error);
    assert!(
        result.content.contains("Suggestion:"),
        "streaming rejection must carry a remediation hint, got: {}",
        result.content
    );
}

#[test]
fn test_ifs_manipulation_detection() {
    let analysis = analyze_command_security("IFS=/; echo rm");
    assert_eq!(analysis.risk_level, SecurityLevel::Critical);
    assert!(
        analysis
            .warnings
            .iter()
            .any(|w| w.contains("IFS manipulation"))
    );

    let analysis2 = analyze_command_security("cat ${IFS}etc${IFS}passwd");
    assert_eq!(analysis2.risk_level, SecurityLevel::Critical);
}

#[test]
fn test_base64_encoding_bypass_detection() {
    let analysis = analyze_command_security("echo 'cm0gLXJmIC8=' | base64 -d | bash");
    assert_eq!(analysis.risk_level, SecurityLevel::Critical);
    assert!(
        analysis
            .warnings
            .iter()
            .any(|w| w.contains("Encoded command"))
    );
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
    assert!(
        analysis
            .warnings
            .iter()
            .any(|w| w.contains("pipe-to-shell"))
    );

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

#[allow(dead_code)] // KEEP: test helper
struct CollectSender {
    lines: std::sync::Mutex<Vec<String>>,
}

#[async_trait]
impl crate::ProgressSender for CollectSender {
    async fn send(&self, line: &str) {
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

    let result = tool
        .execute_streaming(
            json!({"command": "echo line1; echo line2; echo line3"}),
            sender.clone(),
        )
        .await
        .unwrap();

    assert!(!result.is_error);
    assert!(result.content.contains("line1"));
    assert!(result.content.contains("line2"));
    assert!(result.content.contains("line3"));

    let lines = sender.lines.lock().unwrap();
    assert!(
        lines.is_empty(),
        "fast command should not emit streaming events, got {:?}",
        *lines
    );
}

#[tokio::test]
async fn test_bash_streaming_slow_command_emits_lines() {
    // A slow command (sleep > 500ms) should flush the buffer and stream.
    let tool = BashTool::new();
    let sender = std::sync::Arc::new(CollectSender {
        lines: std::sync::Mutex::new(Vec::new()),
    });

    let result = tool
        .execute_streaming(
            json!({"command": "echo line1; sleep 0.6; echo line2; echo line3"}),
            sender.clone(),
        )
        .await
        .unwrap();

    assert!(!result.is_error);
    assert!(result.content.contains("line1"));
    assert!(result.content.contains("line2"));
    assert!(result.content.contains("line3"));

    let lines = sender.lines.lock().unwrap();
    assert!(
        !lines.is_empty(),
        "slow command should emit streaming events"
    );
    // After 500ms buffer, all buffered lines + subsequent lines should stream
    assert!(
        lines.contains(&"line1".to_string()),
        "line1 should be streamed after buffer flush"
    );
    assert!(
        lines.contains(&"line2".to_string()),
        "line2 should be streamed"
    );
    assert!(
        lines.contains(&"line3".to_string()),
        "line3 should be streamed"
    );
}

#[tokio::test]
async fn test_bash_streaming_captures_exit_code() {
    let tool = BashTool::new();
    let sender = std::sync::Arc::new(CollectSender {
        lines: std::sync::Mutex::new(Vec::new()),
    });

    let result = tool
        .execute_streaming(json!({"command": "echo ok; exit 42"}), sender)
        .await
        .unwrap();

    assert!(result.is_error);
    assert_eq!(result.metadata.get("exit_code"), Some(&json!(42)));
}

#[tokio::test]
async fn test_bash_streaming_rejects_critical_commands() {
    let tool = BashTool::new();
    let sender = std::sync::Arc::new(CollectSender {
        lines: std::sync::Mutex::new(Vec::new()),
    });

    let result = tool
        .execute_streaming(json!({"command": "rm -rf /"}), sender.clone())
        .await
        .unwrap();

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
    assert_eq!(
        strip_ansi(multi),
        "\x1b[1;34mheader\x1b[0m\n\x1b[31merror\x1b[0m"
    );

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

// ── Error boundary: shell command injection and dangerous command blocking ──

#[test]
fn test_command_injection_via_command_substitution_detected() {
    let analysis = analyze_command_security("$(rm -rf /)");
    assert_eq!(analysis.risk_level, SecurityLevel::Critical);
    assert!(
        analysis.is_destructive,
        "Command substitution should be flagged as destructive"
    );
    assert!(
        analysis
            .warnings
            .iter()
            .any(|w| w.contains("Command substitution")),
        "Should warn about command substitution"
    );
}

#[test]
fn test_command_injection_via_backtick_detected() {
    let analysis = analyze_command_security("`rm -rf /`");
    assert_eq!(analysis.risk_level, SecurityLevel::Critical);
    assert!(analysis.is_destructive);
}

#[test]
fn test_fork_bomb_pattern_detected() {
    // Fork bomb uses pipe and background (&) which triggers Medium risk via pipe check.
    // The function definition syntax `:(){ ... };:` is not directly in DESTRUCTIVE_PATTERNS,
    // but the pipe detection elevates it to at least Medium risk.
    let analysis = analyze_command_security(":(){ :|:& };:");
    assert!(
        analysis.risk_level >= SecurityLevel::Medium,
        "Fork bomb should be at least Medium risk, got {:?}",
        analysis.risk_level
    );
    // The command is not read-only and contains a pipe
    assert!(
        !analysis.is_read_only,
        "Fork bomb should not be classified as read-only"
    );
}

#[test]
fn test_base64_pipe_to_shell_detected() {
    let analysis = analyze_command_security("echo 'cm0gLXJmIC8=' | base64 -d | bash");
    assert_eq!(analysis.risk_level, SecurityLevel::Critical);
    assert!(
        analysis
            .warnings
            .iter()
            .any(|w| w.contains("Encoded command") || w.contains("pipe-to-shell")),
        "Should detect encoded command execution"
    );
}

#[test]
fn test_validate_path_rejects_traversal() {
    let result = validate_path("../../../etc/passwd", &[]);
    assert!(result.is_err(), "Should reject path traversal");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("traversal") || err.contains("Traversal"),
        "Error should mention traversal, got: {err}"
    );
}

#[test]
fn test_validate_path_rejects_system_path() {
    let result = validate_path("/etc/passwd", &[]);
    assert!(result.is_err(), "Should reject system path");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("system") || err.contains("System"),
        "Error should mention system path, got: {err}"
    );
}

#[test]
fn test_validate_path_rejects_disallowed_path() {
    let allowed = vec!["/home/user/project".to_string()];
    let result = validate_path("/opt/secret/data", &allowed);
    assert!(result.is_err(), "Should reject path not in allowed list");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("not in allowed") || err.contains("NotAllowed"),
        "Error should mention not allowed, got: {err}"
    );
}

#[test]
fn test_dd_disk_destruction_detected() {
    let analysis = analyze_command_security("dd if=/dev/zero of=/dev/sda");
    assert_eq!(analysis.risk_level, SecurityLevel::Critical);
    assert!(analysis.is_destructive);
}

#[test]
fn test_mkfs_format_detected() {
    let analysis = analyze_command_security("mkfs.ext4 /dev/sda1");
    assert_eq!(analysis.risk_level, SecurityLevel::Critical);
    assert!(analysis.is_destructive);
}

#[test]
fn test_rm_rf_root_detected() {
    let analysis = analyze_command_security("rm -rf /");
    assert_eq!(analysis.risk_level, SecurityLevel::Critical);
    assert!(analysis.is_destructive);
    assert!(
        analysis.warnings.iter().any(|w| w.contains("rm -rf /")),
        "Should detect rm -rf / pattern"
    );
}

#[test]
fn test_shell_redirect_to_etc_detected() {
    let analysis = analyze_command_security("echo data > /etc/passwd");
    assert_eq!(analysis.risk_level, SecurityLevel::Critical);
    assert!(analysis.is_destructive);
    assert!(
        analysis.warnings.iter().any(|w| w.contains("/etc/passwd")),
        "Should detect sensitive system path access"
    );
}

// ---- review §P2-12: read-only + destructive predicate ----

#[test]
fn test_find_delete_promotes_to_high_risk() {
    // review §P2-12: `find` matches READ_ONLY_PATTERNS as the head
    // token, but `-delete` post-fix turns the command into a bulk-delete.
    // Previously this slipped through as Low risk, which RunBackground's
    // High-risk gate (review §P1-2) would not block — bulk delete could
    // be backgrounded silently.
    let analysis = analyze_command_security("find . -name '*.tmp' -delete");
    assert!(
        analysis.risk_level >= SecurityLevel::High,
        "find -delete must be at least High, got: {:?}",
        analysis.risk_level
    );
    assert!(
        !analysis.is_read_only,
        "find -delete must not be flagged read-only"
    );
    assert!(
        analysis
            .warnings
            .iter()
            .any(|w| w.contains("destructive predicate")),
        "warning should call out the destructive predicate, got: {:?}",
        analysis.warnings
    );
}

#[test]
fn test_find_exec_promotes_to_high_risk() {
    let analysis = analyze_command_security("find /tmp -name 'core.*' -exec rm {} \\;");
    assert!(
        analysis.risk_level >= SecurityLevel::High,
        "find -exec rm must be at least High, got: {:?}",
        analysis.risk_level
    );
    assert!(!analysis.is_read_only);
}

#[test]
fn test_find_without_destructive_predicate_remains_read_only() {
    // Sanity: the new predicate gate must not over-trigger on plain
    // read-only find invocations.
    let analysis = analyze_command_security("find . -name '*.rs' -type f");
    assert!(analysis.is_read_only, "plain find must stay read-only");
    assert!(
        analysis.risk_level <= SecurityLevel::Low,
        "plain find must stay Low/lower, got: {:?}",
        analysis.risk_level
    );
}

// ─── Test runner detection (P1-5) ────────────────────────────────────────────
//
// The auto-test loop in `shannon-core::auto_test` runs a test command after
// the agent modifies a file. To pick the right command for the project, the
// runner needs to know what language ecosystem is in play — this module
// supplies that detection independently so `shannon-tools` does not have to
// depend on internal types from `shannon-core`.
//
// This mirrors `shannon_core::auto_test::Language::detect_in`; both layers
// must agree because the runner resolves a command based on the first
// detected language and the bash tool exposes the same enumeration to UI.

/// Test runner language detected from project files.
///
/// Mirrors `shannon_core::auto_test::Language`. Kept in sync intentionally —
/// this enum is the public-facing enumeration that the Bash tool and any UI
/// picker use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestLanguage {
    /// Rust (`cargo nextest run` or `cargo test`)
    Rust,
    /// Node.js (`npm test`)
    Node,
    /// Python (`pytest`)
    Python,
    /// Go (`go test ./...`)
    Go,
}

impl TestLanguage {
    /// Detect languages present in `dir` by scanning well-known manifest files.
    pub fn detect_in(dir: &std::path::Path) -> Vec<TestLanguage> {
        let mut langs = Vec::new();
        if dir.join("Cargo.toml").exists() {
            langs.push(TestLanguage::Rust);
        }
        if dir.join("package.json").exists() {
            langs.push(TestLanguage::Node);
        }
        if dir.join("pyproject.toml").exists()
            || dir.join("pytest.ini").exists()
            || dir.join("setup.py").exists()
        {
            langs.push(TestLanguage::Python);
        }
        if dir.join("go.mod").exists() {
            langs.push(TestLanguage::Go);
        }
        langs
    }

    /// Default test command for this language.
    pub fn default_command(self) -> &'static str {
        match self {
            TestLanguage::Rust => "cargo nextest run --no-fail-fast",
            TestLanguage::Node => "npm test --silent",
            TestLanguage::Python => "pytest -x --tb=short",
            TestLanguage::Go => "go test ./...",
        }
    }
}

/// Detect the dominant test runner language in `project_dir`. Returns the
/// first detected language (Rust takes priority because Cargo is the most
/// common Shannon project layout).
pub fn detect_test_runner(project_dir: &std::path::Path) -> Option<TestLanguage> {
    TestLanguage::detect_in(project_dir).into_iter().next()
}

/// Resolve a default test command for `project_dir`, falling back to a generic
/// `make test` invocation if no manifest is present. Returns `None` if the
/// caller explicitly asked to suppress resolution.
pub fn default_test_command(project_dir: &std::path::Path) -> Option<String> {
    detect_test_runner(project_dir).map(|l| l.default_command().to_string())
}

#[cfg(test)]
mod test_runner_detection_tests {
    use super::*;

    #[test]
    fn detects_rust_when_cargo_toml_present() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\n").unwrap();
        let langs = TestLanguage::detect_in(dir.path());
        assert_eq!(langs, vec![TestLanguage::Rust]);
    }

    #[test]
    fn detects_node_when_package_json_present() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        let langs = TestLanguage::detect_in(dir.path());
        assert_eq!(langs, vec![TestLanguage::Node]);
    }

    #[test]
    fn detects_python_via_pyproject_or_pytest_ini() {
        for manifest in ["pyproject.toml", "pytest.ini", "setup.py"] {
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(dir.path().join(manifest), "").unwrap();
            let langs = TestLanguage::detect_in(dir.path());
            assert!(
                langs.contains(&TestLanguage::Python),
                "expected Python detected for {manifest}, got {langs:?}"
            );
        }
    }

    #[test]
    fn detects_go_when_go_mod_present() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("go.mod"), "module example.com\n").unwrap();
        let langs = TestLanguage::detect_in(dir.path());
        assert_eq!(langs, vec![TestLanguage::Go]);
    }

    #[test]
    fn returns_empty_for_unknown_project() {
        let dir = tempfile::tempdir().unwrap();
        let langs = TestLanguage::detect_in(dir.path());
        assert!(langs.is_empty());
    }

    #[test]
    fn detect_returns_first_with_rust_priority() {
        // Rust manifests before Node manifests.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\n").unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        let langs = TestLanguage::detect_in(dir.path());
        assert_eq!(langs[0], TestLanguage::Rust);
        assert_eq!(detect_test_runner(dir.path()), Some(TestLanguage::Rust));
    }

    #[test]
    fn default_command_matches_language() {
        assert!(TestLanguage::Rust.default_command().contains("cargo"));
        assert!(TestLanguage::Node.default_command().contains("npm"));
        assert!(TestLanguage::Python.default_command().contains("pytest"));
        assert!(TestLanguage::Go.default_command().contains("go test"));
    }

    #[test]
    fn default_test_command_resolves_to_runner_command() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\n").unwrap();
        let cmd = default_test_command(dir.path()).unwrap();
        assert!(cmd.contains("cargo"));
    }

    #[test]
    fn default_test_command_returns_none_for_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        assert!(default_test_command(dir.path()).is_none());
    }

    // ── Bash timeout resolution (R0: no more unbounded runs) ──────────

    #[test]
    fn timeout_resolution_defaults_to_120s_when_none() {
        assert_eq!(resolve_timeout_ms_with_env(None, None), 120_000);
    }

    #[test]
    fn timeout_resolution_prefers_explicit_timeout() {
        // Explicit per-call timeout beats both default and env override.
        assert_eq!(
            resolve_timeout_ms_with_env(Some(5_000), Some("9_999")),
            5_000
        );
        assert_eq!(resolve_timeout_ms_with_env(Some(5_000), None), 5_000);
    }

    #[test]
    fn timeout_resolution_honors_env_override() {
        assert_eq!(resolve_timeout_ms_with_env(None, Some("30000")), 30_000);
        assert_eq!(resolve_timeout_ms_with_env(None, Some(" 45000 ")), 45_000);
    }

    #[test]
    fn timeout_resolution_ignores_invalid_env() {
        // Unparseable or negative-looking env values fall back to default.
        assert_eq!(
            resolve_timeout_ms_with_env(None, Some("not-a-number")),
            120_000
        );
        assert_eq!(resolve_timeout_ms_with_env(None, Some("")), 120_000);
        assert_eq!(resolve_timeout_ms_with_env(None, Some("-5")), 120_000);
    }

    #[test]
    fn timeout_resolution_caps_at_600s() {
        // Hard cap applies to explicit timeouts…
        assert_eq!(resolve_timeout_ms_with_env(Some(u64::MAX), None), 600_000);
        // …and to env overrides.
        assert_eq!(
            resolve_timeout_ms_with_env(None, Some("999999999")),
            600_000
        );
    }

    #[tokio::test]
    async fn bash_tool_explicit_timeout_aborts_hanging_command() {
        // End-to-end: the resolved timeout actually reaches the execution
        // world — a `sleep 5` under a 300ms timeout fails with the timeout
        // message instead of hanging the call.
        let tool = BashTool::new();
        let input = serde_json::json!({
            "command": "sleep 5",
            "timeout": 300,
        });
        let output = Tool::execute(&tool, input).await.unwrap();
        assert!(output.is_error, "timed-out command must be an error");
        assert!(
            output.content.contains("timed out after 300ms"),
            "expected timeout message, got: {}",
            output.content
        );
    }

    // ---- review §P2-10: command output byte cap ----

    #[test]
    fn truncate_bytes_passes_through_short_output() {
        let out = b"hello world";
        assert_eq!(truncate_bytes(out, 100), out.to_vec());
    }

    #[test]
    fn truncate_bytes_clips_to_cap_and_marks_dropped() {
        let out = vec![b'x'; 4096];
        let clipped = truncate_bytes(&out, 1024);
        // Marker appended, total length may slightly exceed 1024.
        assert!(clipped.starts_with(b"xxx"), "still the prefix");
        assert!(clipped.ends_with(b"]"), "marker suffix present");
        let marker = String::from_utf8_lossy(&clipped);
        assert!(
            marker.contains("[truncated by harness"),
            "missing truncation marker: {marker}"
        );
    }

    #[test]
    fn truncate_bytes_respects_utf8_boundary() {
        // Three 3-byte CJK chars at the cap so the cut would land inside
        // a multi-byte sequence without the boundary walk.
        let mut out = vec![b'a'; 1000];
        out.extend_from_slice("中国人".as_bytes());
        let clipped = truncate_bytes(&out, 1001);
        // Result must be valid UTF-8.
        let s = std::str::from_utf8(&clipped).expect("clipped is valid UTF-8");
        assert!(s.starts_with('a'), "prefix preserved");
        assert!(s.contains("[truncated by harness"));
    }
}
