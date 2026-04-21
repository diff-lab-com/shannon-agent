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
use std::path::{Path, PathBuf};
use thiserror::Error;

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
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn test_bwrap_wrap_command() {
        // This test only works if bwrap is installed
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

    #[test]
    fn test_detect_sandbox_provider() {
        // Should always return a valid provider
        let provider = detect_sandbox_provider();
        assert!(provider.is_available());
        // On Linux with bwrap, should be "bubblewrap"
        // On macOS, should be "seatbelt"
        // Otherwise "none"
        let name = provider.name();
        assert!(["bubblewrap", "seatbelt", "none"].contains(&name));
    }

    #[test]
    fn test_seatbelt_profile_generation() {
        // Only test profile generation logic, not actual execution
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
    }
}
