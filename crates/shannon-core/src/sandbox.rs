//! # Sandboxed Tool Execution
//!
//! Provides sandboxed execution for Bash commands and file operations using
//! platform-appropriate sandboxing:
//!
//! - **Linux**: `bubblewrap` (`bwrap`) — lightweight namespace sandbox
//! - **macOS**: `sandbox-exec` (Seatbelt) — macOS built-in sandbox
//!
//! When neither sandboxer is available, commands run unsandboxed with a warning.
//!
//! ## Architecture
//!
//! ```text
//! SandboxConfig
//!     |
//!     v
//! SandboxProvider (trait)
//!     |
//!     +--> BwrapSandbox (Linux)
//!     +--> SeatbeltSandbox (macOS)
//!     +--> NoSandbox (fallback)
//!
//! Command execution request
//!     |
//!     v
//! SandboxProvider::wrap_command() --> modified command string or script
//!     |
//!     v
//! Normal Command::new("sh").arg("-c").arg(wrapped_command)
//! ```

use serde::{Deserialize, Serialize};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[cfg(feature = "landlock")]
use landlock::{Access, AccessFs, Bitflags, Compatible, RulesetCreated, RulesetStatus};

// ============================================================================
// Error Types
// ============================================================================

/// Errors during sandbox setup or execution.
#[derive(Error, Debug)]
pub enum SandboxError {
    #[error("Sandbox binary not found: {0}")]
    BinaryNotFound(String),

    #[error("Sandbox execution failed: {0}")]
    ExecutionFailed(String),

    #[error("Sandbox profile error: {0}")]
    ProfileError(String),

    #[error("Platform not supported for sandboxing: {0}")]
    PlatformNotSupported(String),

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
}

// ============================================================================
// Configuration
// ============================================================================

/// Which network access to allow in the sandbox.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum NetworkAccess {
    /// No network access (default)
    #[default]
    None,
    /// Full network access
    Full,
    /// Allow only specific hosts
    #[serde(skip)]
    AllowList(Vec<String>),
}


/// Configuration for sandboxed command execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxConfig {
    /// Project root directory (mounted read-write in sandbox).
    pub project_dir: PathBuf,
    /// Whether to allow network access.
    #[serde(default)]
    pub network: NetworkAccess,
    /// Additional directories to mount read-only.
    #[serde(default)]
    pub readonly_mounts: Vec<PathBuf>,
    /// Additional directories to mount read-write.
    #[serde(default)]
    pub readwrite_mounts: Vec<PathBuf>,
    /// Environment variables to pass into the sandbox.
    #[serde(default)]
    pub env_vars: Vec<String>,
    /// Whether sandboxing is enabled (can be disabled for trusted commands).
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

impl SandboxConfig {
    /// Create a new sandbox config for the given project directory.
    pub fn new(project_dir: impl Into<PathBuf>) -> Self {
        Self {
            project_dir: project_dir.into(),
            network: NetworkAccess::None,
            readonly_mounts: Vec::new(),
            readwrite_mounts: Vec::new(),
            env_vars: Vec::new(),
            enabled: true,
        }
    }

    /// Allow network access in the sandbox.
    pub fn with_network(mut self, network: NetworkAccess) -> Self {
        self.network = network;
        self
    }

    /// Disable sandboxing entirely.
    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }

    /// Add a read-only mount.
    pub fn readonly_mount(mut self, path: impl Into<PathBuf>) -> Self {
        self.readonly_mounts.push(path.into());
        self
    }

    /// Add a read-write mount.
    pub fn readwrite_mount(mut self, path: impl Into<PathBuf>) -> Self {
        self.readwrite_mounts.push(path.into());
        self
    }

    /// Add environment variable name to pass through.
    pub fn env_var(mut self, var: impl Into<String>) -> Self {
        self.env_vars.push(var.into());
        self
    }
}

// ============================================================================
// Sandbox Provider Trait
// ============================================================================

/// Trait for platform-specific sandbox implementations.
pub trait SandboxProvider: Send + Sync {
    /// Check if the sandbox binary is available on this system.
    fn is_available(&self) -> bool;

    /// Wrap a shell command string for sandboxed execution.
    ///
    /// Returns the full command string that should be passed to `sh -c`.
    fn wrap_command(&self, command: &str, config: &SandboxConfig) -> Result<String, SandboxError>;

    /// Name of the sandbox provider (for logging).
    fn name(&self) -> &str;
}

// ============================================================================
// Bubblewrap (Linux) Implementation
// ============================================================================

/// Linux sandbox using `bubblewrap` (`bwrap`).
pub struct BwrapSandbox {
    bwrap_path: PathBuf,
}

impl BwrapSandbox {
    /// Create a new bwrap sandbox provider.
    pub fn new() -> Result<Self, SandboxError> {
        let bwrap_path = which_bwrap().ok_or_else(|| {
            SandboxError::BinaryNotFound("bwrap".to_string())
        })?;
        Ok(Self { bwrap_path })
    }

    /// Try to discover bwrap, returns None if not found.
    pub fn try_new() -> Option<Self> {
        which_bwrap().map(|bwrap_path| Self { bwrap_path })
    }
}

fn which_bwrap() -> Option<PathBuf> {
    let candidates = ["/usr/bin/bwrap", "/usr/local/bin/bwrap"];
    for candidate in &candidates {
        if Path::new(candidate).exists() {
            return Some(PathBuf::from(candidate));
        }
    }
    // Try PATH lookup
    std::process::Command::new("which")
        .arg("bwrap")
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                String::from_utf8(o.stdout).ok().map(|s| PathBuf::from(s.trim()))
            } else {
                None
            }
        })
}

impl SandboxProvider for BwrapSandbox {
    fn is_available(&self) -> bool {
        self.bwrap_path.exists()
    }

    fn wrap_command(&self, command: &str, config: &SandboxConfig) -> Result<String, SandboxError> {
        let mut args = Vec::new();

        // Unshare everything we can
        args.push("--unshare-net".to_string());

        // Mount /usr, /lib, /lib64, /bin, /sbin as read-only from host
        for dir in &["/usr", "/lib", "/lib64", "/bin", "/sbin"] {
            if Path::new(dir).exists() {
                args.push("--ro-bind".to_string());
                args.push(dir.to_string());
                args.push(dir.to_string());
            }
        }

        // /etc mostly read-only but allow resolv.conf for DNS
        args.push("--ro-bind".to_string());
        args.push("/etc".to_string());
        args.push("/etc".to_string());

        // Proc and dev
        args.push("--proc".to_string());
        args.push("/proc".to_string());
        args.push("--dev".to_string());
        args.push("/dev".to_string());

        // /tmp as tmpfs
        args.push("--tmpfs".to_string());
        args.push("/tmp".to_string());

        // Mount project directory read-write
        let project = config.project_dir.to_string_lossy().to_string();
        args.push("--bind".to_string());
        args.push(project.clone());
        args.push(project.clone());

        // Deny writes to .git/ inside the project (mount read-only)
        let git_dir = config.project_dir.join(".git");
        if git_dir.exists() {
            let git_str = git_dir.to_string_lossy().to_string();
            args.push("--ro-bind".to_string());
            args.push(git_str.clone());
            args.push(git_str);
        }

        // Additional read-only mounts
        for mount in &config.readonly_mounts {
            let m = mount.to_string_lossy().to_string();
            if Path::new(&m).exists() {
                args.push("--ro-bind".to_string());
                args.push(m.clone());
                args.push(m);
            }
        }

        // Additional read-write mounts
        for mount in &config.readwrite_mounts {
            let m = mount.to_string_lossy().to_string();
            if Path::new(&m).exists() {
                args.push("--bind".to_string());
                args.push(m.clone());
                args.push(m);
            }
        }

        // Die on parent exit
        args.push("--die-with-parent".to_string());

        // Pass through specified env vars
        for var in &config.env_vars {
            if std::env::var(var).is_ok() {
                args.push("--setenv".to_string());
                args.push(var.clone());
                // We'll set the actual value via the sh -c wrapper
            }
        }

        // Network: if full access, don't unshare net
        if matches!(config.network, NetworkAccess::Full) {
            // Remove --unshare-net
            args.retain(|a| a != "--unshare-net");
        }

        let bwrap = self.bwrap_path.to_string_lossy();
        let args_str = args.iter().map(|a| shell_escape(a)).collect::<Vec<_>>().join(" ");
        let env_exports: String = config
            .env_vars
            .iter()
            .filter_map(|v| std::env::var(v).ok().map(|val| format!("export {v}={}", shell_escape(&val))))
            .collect::<Vec<_>>()
            .join("; ");

        let cd = format!("cd {}", shell_escape(&project));
        let full = format!("{env_exports}; {cd}; {command}");

        Ok(format!("{} {} -- sh -c {}", bwrap, args_str, shell_escape(&full)))
    }

    fn name(&self) -> &str {
        "bubblewrap"
    }
}

// ============================================================================
// Seatbelt (macOS) Implementation
// ============================================================================

/// macOS sandbox using `sandbox-exec` (Seatbelt).
pub struct SeatbeltSandbox {
    sandbox_exec_path: PathBuf,
}

impl SeatbeltSandbox {
    /// Create a new Seatbelt sandbox provider.
    pub fn new() -> Result<Self, SandboxError> {
        let path = PathBuf::from("/usr/bin/sandbox-exec");
        if !path.exists() {
            return Err(SandboxError::BinaryNotFound("sandbox-exec".to_string()));
        }
        Ok(Self { sandbox_exec_path: path })
    }

    /// Try to discover sandbox-exec.
    pub fn try_new() -> Option<Self> {
        let path = PathBuf::from("/usr/bin/sandbox-exec");
        if path.exists() {
            Some(Self { sandbox_exec_path: path })
        } else {
            None
        }
    }

    /// Generate a Seatbelt profile string.
    fn generate_profile(&self, config: &SandboxConfig) -> String {
        let project = config.project_dir.to_string_lossy();
        let mut rules = Vec::new();

        // Allow reading standard system paths
        rules.push("(allow file-read* (subpath \"/usr\"))".to_string());
        rules.push("(allow file-read* (subpath \"/lib\"))".to_string());
        rules.push("(allow file-read* (subpath \"/System\"))".to_string());
        rules.push("(allow file-read* (subpath \"/etc\"))".to_string());
        rules.push("(allow file-read* (subpath \"/dev\"))".to_string());
        rules.push("(allow file-read* (subpath \"/tmp\"))".to_string());

        // Allow full access to project directory
        rules.push(format!("(allow file* (subpath \"{project}\"))"));

        // Deny writes to .git/ inside the project
        let git_dir = config.project_dir.join(".git");
        let git_str = git_dir.to_string_lossy();
        rules.push(format!("(deny file-write* (subpath \"{git_str}\"))"));

        // Additional read-only mounts
        for mount in &config.readonly_mounts {
            let m = mount.to_string_lossy();
            rules.push(format!("(allow file-read* (subpath \"{m}\"))"));
        }

        // Additional read-write mounts
        for mount in &config.readwrite_mounts {
            let m = mount.to_string_lossy();
            rules.push(format!("(allow file* (subpath \"{m}\"))"));
        }

        // Process execution
        rules.push("(allow process-exec (subpath \"/usr\"))".to_string());
        rules.push("(allow process-exec (subpath \"/bin\"))".to_string());
        rules.push("(allow process-exec (subpath \"/sbin\"))".to_string());

        // Network
        match &config.network {
            NetworkAccess::None => {
                rules.push("(deny network*)".to_string());
            }
            NetworkAccess::Full => {
                rules.push("(allow network*)".to_string());
            }
            NetworkAccess::AllowList(hosts) => {
                rules.push("(deny network*)".to_string());
                for host in hosts {
                    rules.push(format!("(allow network* (host \"{host}\"))"));
                }
            }
        }

        // Default deny
        rules.push("(deny default)".to_string());

        format!("(version 1)\n(deny default)\n{}\n", rules.join("\n"))
    }
}

impl SandboxProvider for SeatbeltSandbox {
    fn is_available(&self) -> bool {
        self.sandbox_exec_path.exists()
    }

    fn wrap_command(&self, command: &str, config: &SandboxConfig) -> Result<String, SandboxError> {
        let profile = self.generate_profile(config);
        let project = config.project_dir.to_string_lossy();

        let env_exports: String = config
            .env_vars
            .iter()
            .filter_map(|v| std::env::var(v).ok().map(|val| format!("export {v}={}", shell_escape(&val))))
            .collect::<Vec<_>>()
            .join("; ");

        let cd = format!("cd {}", shell_escape(&project));

        // sandbox-exec -p <profile> sh -c "<command>"
        Ok(format!(
            "sandbox-exec -p {} -- sh -c {}",
            shell_escape(&profile),
            shell_escape(&format!("{env_exports}; {cd}; {command}")),
        ))
    }

    fn name(&self) -> &str {
        "seatbelt"
    }
}

// ============================================================================
// No-op Fallback
// ============================================================================

/// Fallback when no sandbox is available. Runs commands unsandboxed.
pub struct NoSandbox;

impl SandboxProvider for NoSandbox {
    fn is_available(&self) -> bool {
        true
    }

    fn wrap_command(&self, command: &str, _config: &SandboxConfig) -> Result<String, SandboxError> {
        Ok(command.to_string())
    }

    fn name(&self) -> &str {
        "none"
    }
}

// ============================================================================
// Sandbox Type Enum
// ============================================================================

/// Identifies which sandbox backend is available on the current system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SandboxType {
    /// Linux bubblewrap (`bwrap`) namespace sandbox.
    Bubblewrap,
    /// macOS Seatbelt (`sandbox-exec`) sandbox.
    Seatbelt,
    /// No sandbox available — commands run unsandboxed.
    None,
}

impl std::fmt::Display for SandboxType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SandboxType::Bubblewrap => write!(f, "bubblewrap"),
            SandboxType::Seatbelt => write!(f, "seatbelt"),
            SandboxType::None => write!(f, "none"),
        }
    }
}

/// User-facing sandbox mode controlling when sandboxing is applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SandboxMode {
    /// No sandboxing — all commands run directly.
    Off,
    /// Use platform sandbox (bwrap/seatbelt) for high-risk commands.
    #[default]
    Auto,
    /// Always sandbox all commands (strict mode).
    Always,
}

impl std::fmt::Display for SandboxMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SandboxMode::Off => write!(f, "off"),
            SandboxMode::Auto => write!(f, "auto"),
            SandboxMode::Always => write!(f, "always"),
        }
    }
}

impl SandboxMode {
    /// Parse a sandbox mode from string.
    pub fn from_str_lossy(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "off" | "disabled" | "none" => SandboxMode::Off,
            "always" | "strict" | "on" => SandboxMode::Always,
            _ => SandboxMode::Auto,
        }
    }

    /// Whether sandboxing should be applied for a given risk level.
    pub fn should_sandbox(self, is_high_risk: bool) -> bool {
        match self {
            SandboxMode::Off => false,
            SandboxMode::Auto => is_high_risk,
            SandboxMode::Always => true,
        }
    }
}

// ============================================================================
// Sandbox Executor
// ============================================================================

/// High-level sandbox executor that wraps `std::process::Command` objects
/// with platform-appropriate sandbox isolation.
///
/// # Platform Behaviour
///
/// | Platform | Backend | Isolation |
/// |----------|---------|-----------|
/// | Linux | `bwrap` | Filesystem namespace, optional network namespace |
/// | macOS | `sandbox-exec` | Seatbelt profile (allow/deny rules) |
/// | Other | *none* | Warning log, command runs unsandboxed |
///
/// # Example
///
/// ```no_run
/// use std::process::Command;
/// use shannon_core::sandbox::{SandboxConfig, SandboxExecutor};
///
/// let config = SandboxConfig::new("/my/project");
/// let executor = SandboxExecutor::new(config);
///
/// if SandboxExecutor::is_available() {
///     let mut cmd = Command::new("sh");
///     cmd.arg("-c").arg("ls -la");
///     executor.wrap_command(&mut cmd).unwrap();
///     // `cmd` now runs inside the sandbox
/// }
/// ```
pub struct SandboxExecutor {
    config: SandboxConfig,
    sandbox_type: SandboxType,
}

impl SandboxExecutor {
    /// Create a new executor with the given configuration.
    ///
    /// The sandbox backend is auto-detected from the current platform.
    pub fn new(config: SandboxConfig) -> Self {
        let sandbox_type = Self::detect_sandboxer();
        tracing::debug!(
            sandbox_type = %sandbox_type,
            project_dir = %config.project_dir.display(),
            "SandboxExecutor created"
        );
        Self { config, sandbox_type }
    }

    /// Detect which sandbox backend is available on this system.
    pub fn detect_sandboxer() -> SandboxType {
        if cfg!(target_os = "linux") {
            if BwrapSandbox::try_new().is_some() {
                tracing::debug!("Detected sandbox backend: bubblewrap");
                return SandboxType::Bubblewrap;
            }
            tracing::debug!("bwrap not found on this Linux system");
        } else if cfg!(target_os = "macos") {
            if SeatbeltSandbox::try_new().is_some() {
                tracing::debug!("Detected sandbox backend: seatbelt");
                return SandboxType::Seatbelt;
            }
            tracing::debug!("sandbox-exec not found on this macOS system");
        }
        tracing::debug!("No sandbox backend available");
        SandboxType::None
    }

    /// Check whether a sandbox backend is available on this system.
    pub fn is_available() -> bool {
        !matches!(Self::detect_sandboxer(), SandboxType::None)
    }

    /// Return the detected sandbox type.
    pub fn sandbox_type(&self) -> SandboxType {
        self.sandbox_type
    }

    /// Return a reference to the executor's configuration.
    pub fn config(&self) -> &SandboxConfig {
        &self.config
    }

    /// Wrap a `std::process::Command` with sandbox arguments.
    ///
    /// The command's program is replaced with the sandbox binary, and the
    /// original program + arguments are appended after the sandbox flags so
    /// that the command ultimately runs inside the sandbox.
    ///
    /// On unsupported platforms (or when the sandbox binary is missing) a
    /// warning is logged and the command is left unmodified.
    pub fn wrap_command(&self, command: &mut std::process::Command) -> Result<(), SandboxError> {
        if !self.config.enabled {
            tracing::debug!("Sandbox disabled by config, command runs unsandboxed");
            return Ok(());
        }

        match self.sandbox_type {
            SandboxType::Bubblewrap => self.wrap_command_bwrap(command),
            SandboxType::Seatbelt => self.wrap_command_seatbelt(command),
            SandboxType::None => {
                tracing::warn!(
                    "No sandbox backend available; command will run unsandboxed"
                );
                Ok(())
            }
        }
    }

    // -- Linux (bwrap) implementation ------------------------------------

    fn wrap_command_bwrap(&self, command: &mut std::process::Command) -> Result<(), SandboxError> {
        let bwrap = BwrapSandbox::try_new().ok_or_else(|| {
            SandboxError::BinaryNotFound("bwrap".to_string())
        })?;

        // Collect the original program and args.
        let original_program = command.get_program().to_string_lossy().to_string();
        let original_args: Vec<String> = command
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();

        let mut args: Vec<String> = Vec::new();

        // Network isolation (deny by default).
        if !matches!(self.config.network, NetworkAccess::Full) {
            args.push("--unshare-net".to_string());
        }

        // Read-only bind mounts for standard system directories.
        for dir in ["/usr", "/lib", "/lib64", "/bin", "/sbin"] {
            if Path::new(dir).exists() {
                args.extend_from_slice(&[
                    "--ro-bind".to_string(),
                    dir.to_string(),
                    dir.to_string(),
                ]);
            }
        }

        // /etc as read-only.
        if Path::new("/etc").exists() {
            args.extend_from_slice(&[
                "--ro-bind".to_string(),
                "/etc".to_string(),
                "/etc".to_string(),
            ]);
        }

        // Proc and dev.
        args.extend_from_slice(&["--proc".to_string(), "/proc".to_string()]);
        args.extend_from_slice(&["--dev".to_string(), "/dev".to_string()]);

        // /tmp as tmpfs.
        args.extend_from_slice(&["--tmpfs".to_string(), "/tmp".to_string()]);

        // Project directory as read-write.
        let project = self.config.project_dir.to_string_lossy().to_string();
        args.extend_from_slice(&[
            "--bind".to_string(),
            project.clone(),
            project.clone(),
        ]);

        // Deny writes to .git/ inside the project.
        let git_dir = self.config.project_dir.join(".git");
        if git_dir.exists() {
            let git_str = git_dir.to_string_lossy().to_string();
            // Mount .git/ as read-only to prevent writes.
            args.extend_from_slice(&[
                "--ro-bind".to_string(),
                git_str.clone(),
                git_str,
            ]);
        }

        // Additional read-only mounts from config.
        for mount in &self.config.readonly_mounts {
            let m = mount.to_string_lossy().to_string();
            if Path::new(&m).exists() {
                args.extend_from_slice(&["--ro-bind".to_string(), m.clone(), m]);
            }
        }

        // Additional read-write mounts from config.
        for mount in &self.config.readwrite_mounts {
            let m = mount.to_string_lossy().to_string();
            if Path::new(&m).exists() {
                args.extend_from_slice(&["--bind".to_string(), m.clone(), m]);
            }
        }

        // Die when the parent process exits.
        args.push("--die-with-parent".to_string());

        // Pass through requested environment variables.
        for var in &self.config.env_vars {
            if let Ok(val) = std::env::var(var) {
                args.extend_from_slice(&[
                    "--setenv".to_string(),
                    var.clone(),
                    val,
                ]);
            }
        }

        // Append the original command after `--`.
        args.push("--".to_string());
        args.push(original_program);
        args.extend(original_args);

        tracing::debug!(bwrap_args = ?args, "Wrapping command with bwrap");

        // Replace the command with the bwrap invocation.
        // We collect env/cwd from the original command, then overwrite it.
        let env_pairs: Vec<(String, Option<String>)> = command
            .get_envs()
            .map(|(k, v)| (k.to_string_lossy().to_string(), v.map(|v| v.to_string_lossy().to_string())))
            .collect();
        let working_dir = command.get_current_dir().map(|p| p.to_path_buf());

        *command = std::process::Command::new(&bwrap.bwrap_path);
        command.args(&args);

        // Restore environment and working directory.
        for (key, val) in &env_pairs {
            match val {
                Some(v) => { command.env(key, v); }
                None => { command.env_remove(key); }
            }
        }
        if let Some(dir) = working_dir {
            command.current_dir(dir);
        }

        Ok(())
    }

    // -- macOS (Seatbelt) implementation ---------------------------------

    fn wrap_command_seatbelt(&self, command: &mut std::process::Command) -> Result<(), SandboxError> {
        let seatbelt = SeatbeltSandbox::try_new().ok_or_else(|| {
            SandboxError::BinaryNotFound("sandbox-exec".to_string())
        })?;

        // Collect the original program and args.
        let original_program = command.get_program().to_string_lossy().to_string();
        let original_args: Vec<String> = command
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();

        // Generate a profile that also blocks .git/ writes.
        let profile = self.generate_seatbelt_profile_with_git_deny(&seatbelt);

        // Preserve working directory and env.
        let working_dir = command.get_current_dir().map(|p| p.to_path_buf());
        let env_pairs: Vec<(String, Option<String>)> = command
            .get_envs()
            .map(|(k, v)| (k.to_string_lossy().to_string(), v.map(|v| v.to_string_lossy().to_string())))
            .collect();

        // Rebuild as: sandbox-exec -p "<profile>" -- <original_program> [args...]
        *command = std::process::Command::new(&seatbelt.sandbox_exec_path);
        command.arg("-p").arg(&profile);
        command.arg("--");
        command.arg(&original_program);
        command.args(&original_args);

        // Restore environment and working directory.
        for (key, val) in &env_pairs {
            match val {
                Some(v) => { command.env(key, v); }
                None => { command.env_remove(key); }
            }
        }
        if let Some(dir) = working_dir {
            command.current_dir(dir);
        }

        tracing::debug!("Wrapped command with sandbox-exec (Seatbelt)");
        Ok(())
    }

    /// Generate a Seatbelt profile that includes `.git/` write denial.
    ///
    /// This delegates to `SeatbeltSandbox::generate_profile`, which already
    /// includes a `.git/` write deny rule.
    fn generate_seatbelt_profile_with_git_deny(&self, seatbelt: &SeatbeltSandbox) -> String {
        seatbelt.generate_profile(&self.config)
    }
}

// ============================================================================
// Auto-detect Provider
// ============================================================================

/// Detect the best available sandbox provider for the current platform.
pub fn detect_sandbox_provider() -> Box<dyn SandboxProvider> {
    if cfg!(target_os = "linux") {
        if let Some(bwrap) = BwrapSandbox::try_new() {
            tracing::info!("Sandbox: using bubblewrap ({:?})", bwrap.bwrap_path);
            return Box::new(bwrap);
        }
        tracing::warn!("Sandbox: bwrap not found, running unsandboxed");
    } else if cfg!(target_os = "macos") {
        if let Some(seatbelt) = SeatbeltSandbox::try_new() {
            tracing::info!("Sandbox: using Seatbelt (sandbox-exec)");
            return Box::new(seatbelt);
        }
        tracing::warn!("Sandbox: sandbox-exec not found, running unsandboxed");
    } else {
        tracing::warn!("Sandbox: unsupported platform, running unsandboxed");
    }
    Box::new(NoSandbox)
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Minimal shell escaping for a string.
fn shell_escape(s: &str) -> String {
    if s.is_empty() {
        return "''".to_string();
    }
    // If it only contains safe characters, no quoting needed
    let safe = s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | ':'));
    if safe {
        return s.to_string();
    }
    // Single-quote, escaping embedded single quotes
    format!("'{}'", s.replace('\'', "'\\''"))
}

// ============================================================================
// Protected Path Checks
// ============================================================================

/// Check if a path points into a protected directory (.git, .shannon, etc.).
pub fn is_protected_path(path: &Path) -> bool {
    let protected = build_protected_paths(&[]);
    let path_str = path.to_string_lossy();
    let path_str = path_str.trim_start_matches("./");

    for p in &protected {
        let trimmed = p.trim_end_matches('/');
        // Exact match (e.g. ".git" or ".shannon")
        if path_str == trimmed {
            return true;
        }
        // Direct child (e.g. ".git/HEAD", ".shannon/config.toml")
        if path_str.starts_with(&format!("{trimmed}/")) {
            return true;
        }
        // Absolute path that contains the protected dir as a component
        // (e.g. "/home/user/project/.git/HEAD" contains "/.git/")
        if path_str.contains(&format!("/{trimmed}/")) || path_str.ends_with(&format!("/{trimmed}")) {
            return true;
        }
    }
    false
}

/// Check whether writing to the given path is allowed.
///
/// Returns `Err` if the path is inside a protected directory, with a message
/// mentioning the `--dangerously-skip-protected` escape hatch.
pub fn check_write_allowed(path: &Path) -> Result<(), SandboxError> {
    if is_protected_path(path) {
        return Err(SandboxError::InvalidConfig(format!(
            "Write to protected path {path:?} is denied. Use --dangerously-skip-protected to override."
        )));
    }
    Ok(())
}

/// Check whether writing to the given path is allowed, with additional
/// user-specified protected directories.
pub fn check_write_allowed_with_extras(path: &Path, extras: &[String]) -> Result<(), SandboxError> {
    let protected = build_protected_paths(extras);
    let path_str = path.to_string_lossy();
    let path_str = path_str.trim_start_matches("./");

    for p in &protected {
        let trimmed = p.trim_end_matches('/');
        if path_str == trimmed || path_str.starts_with(&format!("{trimmed}/")) {
            return Err(SandboxError::InvalidConfig(format!(
                "Write to protected path {path:?} is denied."
            )));
        }
    }
    Ok(())
}

/// Build the full list of protected paths, combining defaults with
/// user-specified extras.  Deduplicates entries so that `.git` only
/// appears once even if the user also lists it.
pub fn build_protected_paths(user_extras: &[String]) -> Vec<String> {
    let mut paths: Vec<String> = vec![
        ".git".to_string(),
        ".shannon".to_string(),
    ];

    for extra in user_extras {
        let normalized = extra.trim_end_matches('/').to_string();
        if !paths.iter().any(|p| p.trim_end_matches('/') == normalized) {
            paths.push(format!("{}/", extra.trim_end_matches('/')));
        }
    }

    paths
}

/// Check a shell command string for operations that touch protected paths.
///
/// Returns a list of warning messages for each suspicious operation found.
pub fn check_command_protected_paths(command: &str, extras: &[String]) -> Vec<String> {
    let mut warnings = Vec::new();
    let protected = build_protected_paths(extras);

    // Detect destructive git operations.
    if command.contains("git push")
        && (command.contains("--force") || command.contains("-f "))
    {
        warnings.push("git push --force detected: this rewrites remote history".to_string());
    }

    // Detect rm -rf targeting protected directories.
    if command.contains("rm") && command.contains("-rf") {
        for p in &protected {
            let trimmed = p.trim_end_matches('/');
            if command.contains(trimmed) {
                warnings.push(format!("rm -rf targets protected directory: {trimmed}"));
            }
        }
    }

    // Detect shell redirects into protected directories.
    if command.contains('>') {
        for p in &protected {
            let trimmed = p.trim_end_matches('/');
            if command.contains(trimmed) {
                warnings.push(format!("Shell redirect targets protected directory: {trimmed}"));
            }
        }
    }

    warnings
}

// ============================================================================
// SandboxProfile
// ============================================================================

/// High-level sandbox profile for controlling command execution.
///
/// This profile defines what resources a sandboxed command can access:
/// - Filesystem paths (allowed and writable)
/// - Network access
/// - Allowed commands (optional whitelist)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxProfile {
    /// Directories the sandbox can access (read-only by default).
    pub allowed_paths: Vec<PathBuf>,
    /// Optional whitelist of allowed commands.
    ///
    /// If empty, all commands are allowed (subject to other restrictions).
    pub allowed_commands: Vec<String>,
    /// Whether network access is allowed.
    pub network: bool,
    /// Paths with write access (subset of allowed_paths).
    pub writable_paths: Vec<PathBuf>,
}

impl Default for SandboxProfile {
    fn default() -> Self {
        Self::new()
    }
}

impl SandboxProfile {
    /// Create a new empty sandbox profile (no access).
    pub fn new() -> Self {
        Self {
            allowed_paths: Vec::new(),
            allowed_commands: Vec::new(),
            network: false,
            writable_paths: Vec::new(),
        }
    }

    /// Create the default Shannon sandbox profile.
    ///
    /// Allows:
    /// - Project directory (read/write)
    /// - /tmp (read/write)
    /// - ~/.ssh (read-only)
    ///
    /// Denies everything else by default.
    pub fn shannon_default(project_dir: &Path) -> Self {
        let mut profile = Self::new();

        // Project directory (read/write)
        profile.allowed_paths.push(project_dir.to_path_buf());
        profile.writable_paths.push(project_dir.to_path_buf());

        // /tmp for temporary files (read/write)
        if let Ok(tmp) = std::env::var("TMPDIR") {
            profile.allowed_paths.push(PathBuf::from(&tmp));
            profile.writable_paths.push(PathBuf::from(&tmp));
        } else {
            profile.allowed_paths.push(PathBuf::from("/tmp"));
            profile.writable_paths.push(PathBuf::from("/tmp"));
        }

        // ~/.ssh for Git operations (read-only)
        if let Some(home) = dirs::home_dir() {
            let ssh_dir = home.join(".ssh");
            if ssh_dir.exists() {
                profile.allowed_paths.push(ssh_dir);
            }
        }

        profile
    }

    /// Add an allowed path (read-only).
    pub fn allow_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.allowed_paths.push(path.into());
        self
    }

    /// Add a writable path.
    pub fn allow_write(mut self, path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        self.writable_paths.push(path.clone());
        // Also add to allowed_paths if not already present
        if !self.allowed_paths.contains(&path) {
            self.allowed_paths.push(path);
        }
        self
    }

    /// Set network access.
    pub fn with_network(mut self, allow: bool) -> Self {
        self.network = allow;
        self
    }

    /// Add an allowed command to the whitelist.
    pub fn allow_command(mut self, command: impl Into<String>) -> Self {
        self.allowed_commands.push(command.into());
        self
    }

    /// Check if a command is allowed (when whitelist is enabled).
    pub fn is_command_allowed(&self, command: &str) -> bool {
        if self.allowed_commands.is_empty() {
            return true; // No whitelist = all commands allowed
        }

        // Extract base command (first word)
        let base_cmd = command.split_whitespace().next().unwrap_or(command);

        self.allowed_commands.iter().any(|allowed| {
            allowed == base_cmd || command.starts_with(&format!("{allowed} "))
        })
    }

    /// Check if a path is allowed for read access.
    pub fn is_path_allowed(&self, path: &Path) -> bool {
        self.allowed_paths.iter().any(|allowed| {
            path.starts_with(allowed) || path == allowed
        })
    }

    /// Check if a path is allowed for write access.
    pub fn is_path_writable(&self, path: &Path) -> bool {
        self.writable_paths.iter().any(|allowed| {
            path.starts_with(allowed) || path == allowed
        })
    }

    /// Convert to SandboxConfig for use with SandboxExecutor.
    pub fn to_config(&self, project_dir: &Path) -> SandboxConfig {
        let mut config = SandboxConfig::new(project_dir);

        // Set network access
        config.network = if self.network {
            NetworkAccess::Full
        } else {
            NetworkAccess::None
        };

        // Add read-only mounts (allowed_paths that aren't writable)
        for path in &self.allowed_paths {
            if !self.writable_paths.contains(path) {
                config.readonly_mounts.push(path.clone());
            }
        }

        // Add read-write mounts
        for path in &self.writable_paths {
            config.readwrite_mounts.push(path.clone());
        }

        config
    }
}

// ============================================================================
// Landlock Support (Linux, optional feature)
// ============================================================================

#[cfg(feature = "landlock")]
/// Landlock sandbox provider for Linux kernel-level access control.
pub struct LandlockSandbox {
    profile: SandboxProfile,
    ruleset: Option<RulesetCreated>,
}

#[cfg(feature = "landlock")]
impl LandlockSandbox {
    /// Create a new Landlock sandbox with the given profile.
    pub fn new(profile: SandboxProfile) -> Result<Self, SandboxError> {
        use landlock::{Ruleset, AccessFs};

        // Build the ruleset based on the profile
        let mut ruleset = Ruleset::new()
            .handle_access(AccessFs::from_bitflags(
                Access::from_read(|access| {
                    // Allow read access to allowed paths
                    for path in &profile.allowed_paths {
                        if let Ok(path_str) = path.to_str().ok_or_else(|| {
                            SandboxError::InvalidConfig("Invalid path in profile".to_string())
                        }) {
                            let _ = access.path_add_beneath(path_str, Access::FS_READ);
                        }
                    }
                })
            ))
            .handle_access(AccessFs::from_bitflags(
                Access::from_write(|access| {
                    // Allow write access to writable paths
                    for path in &profile.writable_paths {
                        if let Ok(path_str) = path.to_str().ok_or_else(|| {
                            SandboxError::InvalidConfig("Invalid path in profile".to_string())
                        }) {
                            let _ = access.path_add_beneath(path_str, Access::FS_WRITE);
                        }
                    }
                })
            ));

        // Try to create the ruleset
        let ruleset = match ruleset.create() {
            Ok(r) => Some(r),
            Err(_) => {
                // Landlock might not be supported, fall back to no enforcement
                tracing::warn!("Landlock not supported by kernel, running unsandboxed");
                None
            }
        };

        Ok(Self { profile, ruleset })
    }

    /// Try to create a Landlock sandbox, returns None if not available.
    pub fn try_new(profile: SandboxProfile) -> Option<Self> {
        Self::new(profile).ok()
    }

    /// Apply the Landlock restrictions to the current thread.
    pub fn apply_restrictions(&self) -> Result<(), SandboxError> {
        if let Some(ref ruleset) = self.ruleset {
            ruleset
                .restrict()
                .map_err(|e| SandboxError::ExecutionFailed(format!("Failed to apply Landlock: {e}")))?;
        }
        Ok(())
    }

    /// Check if Landlock is available on this system.
    pub fn is_available() -> bool {
        Ruleset::new().create().is_ok()
    }

    /// Get the sandbox profile.
    pub fn profile(&self) -> &SandboxProfile {
        &self.profile
    }

    /// Get the program being executed.
    pub fn program(&self) -> &str {
        &self.program
    }

    /// Get the arguments for the command.
    pub fn args(&self) -> &[String] {
        &self.args
    }
}

// ============================================================================
// SandboxedCommand Builder
// ============================================================================

/// Builder for creating sandboxed tokio processes.
///
/// Wraps a `tokio::process::Command` with appropriate sandbox restrictions
/// based on the platform and provided profile.
pub struct SandboxedCommand {
    profile: SandboxProfile,
    program: String,
    args: Vec<String>,
    current_dir: Option<PathBuf>,
    envs: Vec<(String, Option<String>)>,
    #[cfg(feature = "landlock")]
    landlock: Option<LandlockSandbox>,
}

impl SandboxedCommand {
    /// Create a new sandboxed command builder.
    ///
    /// # Arguments
    /// - `profile`: Sandbox profile defining access restrictions
    /// - `program`: Command to execute
    /// - `args`: Arguments for the command
    pub fn new(profile: SandboxProfile, program: &str, args: &[&str]) -> Self {
        #[cfg(feature = "landlock")]
        let landlock = LandlockSandbox::try_new(profile.clone());

        Self {
            profile,
            program: program.to_string(),
            args: args.iter().map(|s| s.to_string()).collect(),
            current_dir: None,
            envs: Vec::new(),
            #[cfg(feature = "landlock")]
            landlock,
        }
    }

    /// Set the working directory for the command.
    pub fn current_dir(&mut self, dir: impl AsRef<Path>) -> &mut Self {
        self.current_dir = Some(dir.as_ref().to_path_buf());
        self
    }

    /// Set an environment variable for the command.
    pub fn env(&mut self, key: impl AsRef<OsStr>, val: impl AsRef<OsStr>) -> &mut Self {
        self.envs.push((
            key.as_ref().to_string_lossy().to_string(),
            Some(val.as_ref().to_string_lossy().to_string()),
        ));
        self
    }

    /// Remove an environment variable.
    pub fn env_remove(&mut self, key: impl AsRef<OsStr>) -> &mut Self {
        self.envs.push((
            key.as_ref().to_string_lossy().to_string(),
            None,
        ));
        self
    }

    /// Set environment variables from a map.
    pub fn envs(&mut self, vars: impl IntoIterator<Item = (impl AsRef<OsStr>, impl AsRef<OsStr>)>) -> &mut Self {
        for (key, val) in vars {
            self.envs.push((
                key.as_ref().to_string_lossy().to_string(),
                Some(val.as_ref().to_string_lossy().to_string()),
            ));
        }
        self
    }

    /// Spawn the sandboxed command.
    ///
    /// This applies sandbox restrictions before spawning the process.
    pub async fn spawn(&mut self) -> Result<tokio::process::Child, SandboxError> {
        // Validate command is allowed
        if !self.profile.is_command_allowed(&self.program) {
            return Err(SandboxError::ExecutionFailed(format!(
                "Command not allowed by sandbox profile: {}",
                self.program
            )));
        }

        // Apply Landlock restrictions if available
        #[cfg(feature = "landlock")]
        if let Some(ref landlock) = self.landlock {
            landlock.apply_restrictions()?;
        }

        // Build and spawn the process
        let mut cmd = tokio::process::Command::new(&self.program);
        cmd.args(&self.args);
        if let Some(ref dir) = self.current_dir {
            cmd.current_dir(dir);
        }
        for (k, v) in &self.envs {
            if let Some(val) = v {
                cmd.env(k, val);
            } else {
                cmd.env_remove(k);
            }
        }
        cmd.spawn()
            .map_err(|e| SandboxError::ExecutionFailed(format!("Failed to spawn: {e}")))
    }

    /// Get the sandbox profile.
    pub fn profile(&self) -> &SandboxProfile {
        &self.profile
    }

    /// Get the program being executed.
    pub fn program(&self) -> &str {
        &self.program
    }

    /// Get the arguments for the command.
    pub fn args(&self) -> &[String] {
        &self.args
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    // ------------------------------------------------------------------
    // SandboxConfig tests
    // ------------------------------------------------------------------

    #[test]
    fn test_sandbox_config_new() {
        let config = SandboxConfig::new("/tmp/project");
        assert_eq!(config.project_dir, PathBuf::from("/tmp/project"));
        assert!(config.enabled);
        assert!(matches!(config.network, NetworkAccess::None));
    }

    #[test]
    fn test_sandbox_config_builder() {
        let config = SandboxConfig::new("/tmp/project")
            .with_network(NetworkAccess::Full)
            .readonly_mount("/data")
            .readwrite_mount("/output")
            .env_var("HOME")
            .disabled();
        assert!(!config.enabled);
        assert!(matches!(config.network, NetworkAccess::Full));
        assert_eq!(config.readonly_mounts.len(), 1);
        assert_eq!(config.readwrite_mounts.len(), 1);
        assert_eq!(config.env_vars.len(), 1);
    }

    #[test]
    fn test_network_access_serde() {
        let none: NetworkAccess = serde_json::from_str("\"none\"").unwrap();
        assert_eq!(none, NetworkAccess::None);
        let full: NetworkAccess = serde_json::from_str("\"full\"").unwrap();
        assert_eq!(full, NetworkAccess::Full);
    }

    // ------------------------------------------------------------------
    // SandboxType tests
    // ------------------------------------------------------------------

    #[test]
    fn test_sandbox_type_display() {
        assert_eq!(SandboxType::Bubblewrap.to_string(), "bubblewrap");
        assert_eq!(SandboxType::Seatbelt.to_string(), "seatbelt");
        assert_eq!(SandboxType::None.to_string(), "none");
    }

    #[test]
    fn test_sandbox_type_serde() {
        let bw: SandboxType = serde_json::from_str("\"bubblewrap\"").unwrap();
        assert_eq!(bw, SandboxType::Bubblewrap);
        let sb: SandboxType = serde_json::from_str("\"seatbelt\"").unwrap();
        assert_eq!(sb, SandboxType::Seatbelt);
        let none: SandboxType = serde_json::from_str("\"none\"").unwrap();
        assert_eq!(none, SandboxType::None);
    }

    #[test]
    fn test_detect_sandboxer_returns_valid_type() {
        let st = SandboxExecutor::detect_sandboxer();
        assert!(matches!(
            st,
            SandboxType::Bubblewrap | SandboxType::Seatbelt | SandboxType::None
        ));
    }

    // ------------------------------------------------------------------
    // NoSandbox tests
    // ------------------------------------------------------------------

    #[test]
    fn test_no_sandbox_passthrough() {
        let sandbox = NoSandbox;
        let config = SandboxConfig::new("/tmp/project");
        let result = sandbox.wrap_command("echo hello", &config).unwrap();
        assert_eq!(result, "echo hello");
    }

    #[test]
    fn test_no_sandbox_is_always_available() {
        let sandbox = NoSandbox;
        assert!(sandbox.is_available());
        assert_eq!(sandbox.name(), "none");
    }

    // ------------------------------------------------------------------
    // Shell escaping tests
    // ------------------------------------------------------------------

    #[test]
    fn test_shell_escape_simple() {
        assert_eq!(shell_escape("hello"), "hello");
        assert_eq!(shell_escape("/usr/bin/node"), "/usr/bin/node");
    }

    #[test]
    fn test_shell_escape_special_chars() {
        let escaped = shell_escape("hello world");
        assert!(escaped.starts_with('\''));
        assert!(escaped.ends_with('\''));
    }

    #[test]
    fn test_shell_escape_empty() {
        assert_eq!(shell_escape(""), "''");
    }

    #[test]
    fn test_shell_escape_single_quotes() {
        let escaped = shell_escape("it's");
        assert!(escaped.contains("'\\''"));
    }

    // ------------------------------------------------------------------
    // BwrapSandbox string-based wrap_command tests
    // ------------------------------------------------------------------

    #[test]
    fn test_bwrap_wrap_command() {
        let bwrap = BwrapSandbox {
            bwrap_path: PathBuf::from("/usr/bin/bwrap"),
        };
        if !bwrap.is_available() {
            return; // Skip if bwrap not installed
        }
        let config = SandboxConfig::new("/tmp/my-project");
        let result = bwrap.wrap_command("ls -la", &config);
        assert!(result.is_ok());
        let cmd = result.unwrap();
        assert!(cmd.contains("bwrap"));
        assert!(cmd.contains("--unshare-net"));
        assert!(cmd.contains("/tmp/my-project"));
        assert!(cmd.contains("ls -la"));
    }

    #[test]
    fn test_bwrap_with_network() {
        let bwrap = BwrapSandbox {
            bwrap_path: PathBuf::from("/usr/bin/bwrap"),
        };
        if !bwrap.is_available() {
            return;
        }
        let config = SandboxConfig::new("/tmp/project").with_network(NetworkAccess::Full);
        let result = bwrap.wrap_command("curl https://example.com", &config).unwrap();
        assert!(!result.contains("--unshare-net"));
    }

    // ------------------------------------------------------------------
    // SeatbeltSandbox profile generation tests
    // ------------------------------------------------------------------

    #[test]
    fn test_seatbelt_profile_generation() {
        let sandbox = SeatbeltSandbox {
            sandbox_exec_path: PathBuf::from("/usr/bin/sandbox-exec"),
        };
        let config = SandboxConfig::new("/Users/test/project")
            .readonly_mount("/data")
            .with_network(NetworkAccess::Full);
        let profile = sandbox.generate_profile(&config);
        assert!(profile.contains("/Users/test/project"));
        assert!(profile.contains("allow network*"));
        assert!(profile.contains("/data"));
        assert!(profile.contains("deny default"));
        // Should deny writes to .git/
        assert!(profile.contains("deny file-write*"));
        assert!(profile.contains(".git"));
    }

    #[test]
    fn test_seatbelt_profile_no_network() {
        let sandbox = SeatbeltSandbox {
            sandbox_exec_path: PathBuf::from("/usr/bin/sandbox-exec"),
        };
        let config = SandboxConfig::new("/Users/test/project");
        let profile = sandbox.generate_profile(&config);
        assert!(profile.contains("deny network*"));
    }

    // ------------------------------------------------------------------
    // detect_sandbox_provider tests
    // ------------------------------------------------------------------

    #[test]
    fn test_detect_sandbox_provider() {
        let provider = detect_sandbox_provider();
        assert!(provider.is_available());
        let name = provider.name();
        assert!(["bubblewrap", "seatbelt", "none"].contains(&name));
    }

    // ------------------------------------------------------------------
    // SandboxExecutor tests
    // ------------------------------------------------------------------

    #[test]
    fn test_executor_new() {
        let config = SandboxConfig::new("/tmp/project");
        let executor = SandboxExecutor::new(config);
        assert!(matches!(
            executor.sandbox_type(),
            SandboxType::Bubblewrap | SandboxType::Seatbelt | SandboxType::None
        ));
        assert_eq!(executor.config().project_dir, PathBuf::from("/tmp/project"));
    }

    #[test]
    fn test_executor_disabled_config() {
        let config = SandboxConfig::new("/tmp/project").disabled();
        let executor = SandboxExecutor::new(config);
        let mut cmd = Command::new("echo");
        cmd.arg("hello");
        let result = executor.wrap_command(&mut cmd);
        assert!(result.is_ok());
        // Command program should still be "echo" (unchanged).
        assert_eq!(cmd.get_program(), "echo");
    }

    #[test]
    fn test_executor_wrap_command_no_panic() {
        let config = SandboxConfig::new("/nonexistent/project");
        let executor = SandboxExecutor::new(config);
        let mut cmd = Command::new("ls");
        cmd.arg("-la");
        let result = executor.wrap_command(&mut cmd);
        assert!(result.is_ok());
    }

    #[test]
    fn test_executor_config_access() {
        let config = SandboxConfig::new("/tmp/test-project")
            .with_network(NetworkAccess::Full)
            .readonly_mount("/data");
        let executor = SandboxExecutor::new(config);
        assert_eq!(
            executor.config().project_dir,
            PathBuf::from("/tmp/test-project")
        );
        assert!(matches!(executor.config().network, NetworkAccess::Full));
        assert_eq!(executor.config().readonly_mounts.len(), 1);
    }

    #[test]
    fn test_executor_preserves_env_and_cwd() {
        let config = SandboxConfig::new("/tmp/project");
        let executor = SandboxExecutor::new(config);
        let mut cmd = Command::new("echo");
        cmd.arg("hello");
        cmd.env("MY_TEST_VAR", "test_value");
        cmd.current_dir("/tmp");

        let result = executor.wrap_command(&mut cmd);
        assert!(result.is_ok());
        assert!(cmd.get_current_dir().is_some());
    }

    #[test]
    fn test_is_available_consistency() {
        let detected = SandboxExecutor::detect_sandboxer();
        let available = SandboxExecutor::is_available();
        if matches!(detected, SandboxType::None) {
            assert!(!available);
        } else {
            assert!(available);
        }
    }

    #[test]
    fn test_executor_bwrap_command_wrapping() {
        if !matches!(SandboxExecutor::detect_sandboxer(), SandboxType::Bubblewrap) {
            return;
        }
        let config = SandboxConfig::new("/tmp/project");
        let executor = SandboxExecutor::new(config);
        let mut cmd = Command::new("ls");
        cmd.arg("-la");

        let result = executor.wrap_command(&mut cmd);
        assert!(result.is_ok());

        let program = cmd.get_program().to_string_lossy();
        assert!(
            program.contains("bwrap"),
            "Expected bwrap program, got: {program}"
        );

        let args: Vec<String> = cmd.get_args().map(|a| a.to_string_lossy().to_string()).collect();
        let args_joined = args.join(" ");
        assert!(
            args_joined.contains("--unshare-net"),
            "Expected --unshare-net in args: {args_joined}"
        );
        assert!(
            args_joined.contains("/tmp/project"),
            "Expected project dir in args: {args_joined}"
        );
        assert!(
            args_joined.contains("--die-with-parent"),
            "Expected --die-with-parent in args: {args_joined}"
        );
        assert!(
            args_joined.contains("-- ls"),
            "Expected original program after -- separator: {args_joined}"
        );
    }

    #[test]
    fn test_executor_bwrap_with_network_allowed() {
        if !matches!(SandboxExecutor::detect_sandboxer(), SandboxType::Bubblewrap) {
            return;
        }
        let config = SandboxConfig::new("/tmp/project").with_network(NetworkAccess::Full);
        let executor = SandboxExecutor::new(config);
        let mut cmd = Command::new("curl");
        cmd.arg("https://example.com");

        let result = executor.wrap_command(&mut cmd);
        assert!(result.is_ok());

        let args: Vec<String> = cmd.get_args().map(|a| a.to_string_lossy().to_string()).collect();
        let args_joined = args.join(" ");
        assert!(
            !args_joined.contains("--unshare-net"),
            "--unshare-net should not be present when network is allowed: {args_joined}"
        );
    }

    // ------------------------------------------------------------------
    // Protected path tests
    // ------------------------------------------------------------------

    #[test]
    fn test_is_protected_path_git_head() {
        assert!(is_protected_path(Path::new(".git/HEAD")));
    }

    #[test]
    fn test_is_protected_path_git_config() {
        assert!(is_protected_path(Path::new(".git/config")));
    }

    #[test]
    fn test_is_protected_path_git_refs() {
        assert!(is_protected_path(Path::new(".git/refs/heads/main")));
    }

    #[test]
    fn test_is_protected_path_git_dir_only() {
        assert!(is_protected_path(Path::new(".git")));
    }

    #[test]
    fn test_is_protected_path_shannon_dir() {
        assert!(is_protected_path(Path::new(".shannon/config.toml")));
    }

    #[test]
    fn test_is_protected_path_absolute_git() {
        assert!(is_protected_path(Path::new("/home/user/project/.git/HEAD")));
    }

    #[test]
    fn test_is_protected_path_dot_slash_prefix() {
        assert!(is_protected_path(Path::new("./.git/config")));
    }

    #[test]
    fn test_is_not_protected_path_src() {
        assert!(!is_protected_path(Path::new("src/main.rs")));
    }

    #[test]
    fn test_is_not_protected_path_readme() {
        assert!(!is_protected_path(Path::new("README.md")));
    }

    #[test]
    fn test_is_not_protected_path_gitignore() {
        assert!(!is_protected_path(Path::new(".gitignore")));
    }

    #[test]
    fn test_is_not_protected_path_cargo_toml() {
        assert!(!is_protected_path(Path::new("Cargo.toml")));
    }

    #[test]
    fn test_check_write_allowed_normal_path() {
        assert!(check_write_allowed(Path::new("src/main.rs")).is_ok());
    }

    #[test]
    fn test_check_write_allowed_git_path_rejected() {
        let result = check_write_allowed(Path::new(".git/config"));
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("protected"));
        assert!(err.contains("--dangerously-skip-protected"));
    }

    #[test]
    fn test_check_write_allowed_with_extras() {
        let extras = vec!["custom_dir/".to_string()];
        assert!(
            check_write_allowed_with_extras(Path::new("custom_dir/file.txt"), &extras).is_err()
        );
        assert!(
            check_write_allowed_with_extras(Path::new("src/main.rs"), &extras).is_ok()
        );
    }

    #[test]
    fn test_build_protected_paths_includes_defaults() {
        let paths = build_protected_paths(&[]);
        assert!(paths.iter().any(|p| p.contains(".git")));
        assert!(paths.iter().any(|p| p.contains(".shannon")));
    }

    #[test]
    fn test_build_protected_paths_adds_user_paths() {
        let user = vec!["my_custom_dir/".to_string()];
        let paths = build_protected_paths(&user);
        assert!(paths.iter().any(|p| p.contains("my_custom_dir")));
        assert!(paths.iter().any(|p| p.trim_end_matches('/') == ".git"));
    }

    #[test]
    fn test_build_protected_paths_git_cannot_be_removed() {
        let paths = build_protected_paths(&[]);
        assert!(paths.iter().any(|p| p.trim_end_matches('/') == ".git"));
    }

    #[test]
    fn test_build_protected_paths_no_duplicates() {
        let user = vec![".git/".to_string(), ".shannon/".to_string()];
        let paths = build_protected_paths(&user);
        let git_count = paths.iter().filter(|p| p.trim_end_matches('/') == ".git").count();
        assert_eq!(git_count, 1, "Should not duplicate .git entries");
    }

    #[test]
    fn test_check_command_protected_paths_git_push_force() {
        let warnings = check_command_protected_paths("git push --force origin main", &[]);
        assert!(!warnings.is_empty(), "Should warn about git push --force");
    }

    #[test]
    fn test_check_command_protected_paths_rm_git() {
        let warnings = check_command_protected_paths("rm -rf .git", &[]);
        assert!(!warnings.is_empty(), "Should warn about rm -rf .git");
    }

    #[test]
    fn test_check_command_protected_paths_normal_command() {
        let warnings = check_command_protected_paths("cargo build", &[]);
        assert!(warnings.is_empty(), "Normal commands should have no warnings");
    }

    #[test]
    fn test_check_command_protected_paths_redirect_to_git() {
        let warnings = check_command_protected_paths("echo foo > .git/config", &[]);
        assert!(!warnings.is_empty(), "Should warn about redirect to .git/");
    }

    // ------------------------------------------------------------------
    // SandboxProfile tests
    // ------------------------------------------------------------------

    #[test]
    fn test_sandbox_profile_new() {
        let profile = SandboxProfile::new();
        assert!(profile.allowed_paths.is_empty());
        assert!(profile.allowed_commands.is_empty());
        assert!(!profile.network);
        assert!(profile.writable_paths.is_empty());
    }

    #[test]
    fn test_sandbox_profile_default() {
        let profile = SandboxProfile::default();
        assert!(profile.allowed_paths.is_empty());
        assert!(!profile.network);
    }

    #[test]
    fn test_sandbox_profile_builder() {
        let profile = SandboxProfile::new()
            .allow_path("/usr/bin")
            .allow_write("/tmp")
            .with_network(true)
            .allow_command("ls")
            .allow_command("cat");

        assert_eq!(profile.allowed_paths.len(), 2);
        assert_eq!(profile.writable_paths.len(), 1);
        assert!(profile.network);
        assert_eq!(profile.allowed_commands.len(), 2);
    }

    #[test]
    fn test_sandbox_profile_allow_write_adds_to_allowed() {
        let profile = SandboxProfile::new()
            .allow_write("/tmp");

        assert_eq!(profile.allowed_paths.len(), 1);
        assert_eq!(profile.writable_paths.len(), 1);
        assert!(profile.allowed_paths.contains(&PathBuf::from("/tmp")));
    }

    #[test]
    fn test_sandbox_profile_is_command_allowed_empty_whitelist() {
        let profile = SandboxProfile::new();
        assert!(profile.is_command_allowed("ls"));
        assert!(profile.is_command_allowed("rm -rf /"));
    }

    #[test]
    fn test_sandbox_profile_is_command_allowed_with_whitelist() {
        let profile = SandboxProfile::new()
            .allow_command("ls")
            .allow_command("cat");

        assert!(profile.is_command_allowed("ls"));
        assert!(profile.is_command_allowed("ls -la"));
        assert!(profile.is_command_allowed("cat file.txt"));
        assert!(!profile.is_command_allowed("rm file.txt"));
    }

    #[test]
    fn test_sandbox_profile_is_path_allowed() {
        let profile = SandboxProfile::new()
            .allow_path("/tmp")
            .allow_path("/home/user/project");

        assert!(profile.is_path_allowed(Path::new("/tmp/file.txt")));
        assert!(profile.is_path_allowed(Path::new("/home/user/project/src/main.rs")));
        assert!(!profile.is_path_allowed(Path::new("/etc/passwd")));
    }

    #[test]
    fn test_sandbox_profile_is_path_writable() {
        let profile = SandboxProfile::new()
            .allow_path("/tmp")
            .allow_write("/home/user/project");

        assert!(profile.is_path_writable(Path::new("/home/user/project/src/main.rs")));
        assert!(!profile.is_path_writable(Path::new("/tmp/file.txt")));
    }

    #[test]
    fn test_sandbox_profile_shannon_default() {
        let project_dir = PathBuf::from("/home/user/project");
        let profile = SandboxProfile::shannon_default(&project_dir);

        assert!(!profile.allowed_paths.is_empty());
        assert!(!profile.writable_paths.is_empty());
        assert!(!profile.network);
        assert!(profile.is_path_allowed(&project_dir));
        assert!(profile.is_path_writable(&project_dir));
    }

    #[test]
    fn test_sandbox_profile_to_config() {
        let profile = SandboxProfile::new()
            .allow_path("/usr/share")
            .allow_write("/tmp")
            .with_network(true);

        let config = profile.to_config(Path::new("/project"));
        assert!(matches!(config.network, NetworkAccess::Full));
        assert_eq!(config.readonly_mounts.len(), 1);
        assert_eq!(config.readwrite_mounts.len(), 1);
    }

    // ------------------------------------------------------------------
    // SandboxedCommand tests
    // ------------------------------------------------------------------

    #[test]
    fn test_sandboxed_command_new() {
        let profile = SandboxProfile::new()
            .allow_command("echo")
            .allow_write("/tmp");

        let cmd = SandboxedCommand::new(profile, "echo", &["hello"]);
        assert!(cmd.profile().allowed_commands.contains(&"echo".to_string()));
    }

    #[test]
    fn test_sandboxed_command_builder_methods() {
        let profile = SandboxProfile::new()
            .allow_command("ls")
            .allow_path("/tmp");

        let mut cmd = SandboxedCommand::new(profile, "ls", &["-la"]);
        cmd.current_dir("/tmp")
            .env("TEST_VAR", "value")
            .env_remove("PATH");

        assert_eq!(cmd.program(), "ls");
    }
}
