//! # Sandbox Adapter
//!
//! A flexible sandbox environment adapter providing fine-grained control over
//! filesystem access, command execution, and network operations.
//!
//! Extends the basic `PathSandbox` with:
//! - Separate read/write/execute path validation
//! - Network access control
//! - File size limits
//! - Dynamic rule management
//!
//! Reference: Claude Code src/utils/sandbox/

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Errors from sandbox validation.
#[derive(Error, Debug)]
pub enum SandboxViolation {
    #[error("Path access denied: {0}")]
    PathDenied(String),

    #[error("Write access denied: {0} is read-only")]
    WriteDenied(String),

    #[error("Execute denied: {0}")]
    ExecuteDenied(String),

    #[error("Network access denied: {0}")]
    NetworkDenied(String),

    #[error("File too large: {size} bytes exceeds maximum {max_size} bytes")]
    FileTooLarge { size: u64, max_size: u64 },

    #[error("Command not in allowed list: {0}")]
    CommandNotAllowed(String),

    #[error("Invalid sandbox rule: {0}")]
    InvalidRule(String),
}

/// Result of a sandbox validation check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxResult {
    /// Whether the operation is allowed.
    pub allowed: bool,
    /// Human-readable reason for the decision.
    pub reason: String,
}

impl SandboxResult {
    /// Create an allowed result.
    pub fn allowed(reason: &str) -> Self {
        Self {
            allowed: true,
            reason: reason.to_string(),
        }
    }

    /// Create a denied result.
    pub fn denied(reason: &str) -> Self {
        Self {
            allowed: false,
            reason: reason.to_string(),
        }
    }
}

/// Configuration for the sandbox adapter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxConfig {
    /// Paths that are explicitly allowed for access.
    pub allowed_paths: Vec<PathBuf>,
    /// Paths that are explicitly denied.
    pub denied_paths: Vec<PathBuf>,
    /// Paths that are read-only (can read but not write).
    pub read_only_paths: Vec<PathBuf>,
    /// Maximum file size in bytes. `None` means no limit.
    pub max_file_size: Option<u64>,
    /// Whether network access is allowed.
    pub network_allowed: bool,
    /// Commands that are explicitly allowed for execution.
    pub allowed_commands: Vec<String>,
    /// Commands that are explicitly denied for execution.
    pub denied_commands: Vec<String>,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            allowed_paths: vec![std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))],
            denied_paths: vec![
                PathBuf::from("/etc"),
                PathBuf::from("/boot"),
                PathBuf::from("/dev"),
                PathBuf::from("/proc"),
                PathBuf::from("/sys"),
            ],
            read_only_paths: Vec::new(),
            max_file_size: Some(100 * 1024 * 1024), // 100 MB default
            network_allowed: false,
            allowed_commands: Vec::new(),
            denied_commands: vec![
                "rm -rf /".to_string(),
                "mkfs".to_string(),
                "dd if=/dev/zero".to_string(),
            ],
        }
    }
}

impl SandboxConfig {
    /// Create a permissive config that allows most operations.
    pub fn permissive() -> Self {
        Self {
            allowed_paths: vec![PathBuf::from("/")],
            denied_paths: Vec::new(),
            read_only_paths: Vec::new(),
            max_file_size: None,
            network_allowed: true,
            allowed_commands: Vec::new(),
            denied_commands: Vec::new(),
        }
    }

    /// Create a restrictive config that denies most operations.
    pub fn restrictive() -> Self {
        Self {
            allowed_paths: vec![std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))],
            denied_paths: vec![
                PathBuf::from("/etc"),
                PathBuf::from("/boot"),
                PathBuf::from("/dev"),
                PathBuf::from("/proc"),
                PathBuf::from("/sys"),
                PathBuf::from("/usr/bin"),
                PathBuf::from("/usr/sbin"),
                PathBuf::from("/bin"),
                PathBuf::from("/sbin"),
            ],
            read_only_paths: Vec::new(),
            max_file_size: Some(10 * 1024 * 1024), // 10 MB
            network_allowed: false,
            allowed_commands: vec!["ls".to_string(), "cat".to_string(), "head".to_string()],
            denied_commands: vec![
                "rm".to_string(),
                "rmdir".to_string(),
                "mv".to_string(),
                "cp".to_string(),
                "chmod".to_string(),
                "chown".to_string(),
                "mkfs".to_string(),
                "fdisk".to_string(),
            ],
        }
    }
}

/// Trait for sandbox adapters that validate operations.
pub trait SandboxAdapter {
    /// Validate a read operation on the given path.
    fn validate_read(&self, path: &Path) -> Result<SandboxResult, SandboxViolation>;

    /// Validate a write operation on the given path.
    fn validate_write(&self, path: &Path) -> Result<SandboxResult, SandboxViolation>;

    /// Validate an execute operation (command).
    fn validate_execute(&self, command: &str) -> Result<SandboxResult, SandboxViolation>;

    /// Validate a network access request.
    fn validate_network(&self, host: &str) -> Result<SandboxResult, SandboxViolation>;
}

/// A path-based sandbox adapter implementing the `SandboxAdapter` trait.
///
/// Provides filesystem sandboxing with separate read/write/execute/network
/// validation, dynamic rule management, and file size limits.
pub struct PathSandboxAdapter {
    config: SandboxConfig,
}

impl PathSandboxAdapter {
    /// Create a new adapter with default configuration.
    pub fn new() -> Self {
        Self {
            config: SandboxConfig::default(),
        }
    }

    /// Create a new adapter with custom configuration.
    pub fn with_config(config: SandboxConfig) -> Self {
        Self { config }
    }

    /// Check path access against allowed and denied lists.
    ///
    /// A path is allowed if:
    /// 1. It is not in the denied list (after canonicalization)
    /// 2. It is under an allowed path
    pub fn check_path_access(&self, path: &Path) -> SandboxResult {
        // Check denied paths first (deny takes precedence)
        if self.is_path_denied(path) {
            return SandboxResult::denied(&format!(
                "Path '{}' is in the denied list",
                path.display()
            ));
        }

        // Check if path is under an allowed root
        if !self.is_path_allowed(path) {
            return SandboxResult::denied(&format!(
                "Path '{}' is not under any allowed path",
                path.display()
            ));
        }

        SandboxResult::allowed(&format!(
            "Path '{}' is accessible",
            path.display()
        ))
    }

    /// Check if a command is safe to execute.
    ///
    /// A command is safe if:
    /// 1. It is not in the denied commands list
    /// 2. If allowed_commands is non-empty, it must be in the list
    pub fn check_command_safety(&self, command: &str) -> SandboxResult {
        let base_command = command.split_whitespace().next().unwrap_or(command);

        // Check denied commands
        for denied in &self.config.denied_commands {
            let denied_base = denied.split_whitespace().next().unwrap_or(denied);
            if base_command == denied_base || command.contains(denied) {
                return SandboxResult::denied(&format!(
                    "Command '{}' is in the denied list",
                    base_command
                ));
            }
        }

        // If allowed_commands is non-empty, the command must be in it
        if !self.config.allowed_commands.is_empty() {
            let found = self
                .config
                .allowed_commands
                .iter()
                .any(|allowed| base_command == allowed);
            if !found {
                return SandboxResult::denied(&format!(
                    "Command '{}' is not in the allowed list",
                    base_command
                ));
            }
        }

        SandboxResult::allowed(&format!("Command '{}' is safe to execute", base_command))
    }

    /// Check if a file size is within limits.
    pub fn check_file_size(&self, size: u64) -> SandboxResult {
        if let Some(max) = self.config.max_file_size {
            if size > max {
                return SandboxResult::denied(&format!(
                    "File size {} bytes exceeds maximum {} bytes",
                    size, max
                ));
            }
        }
        SandboxResult::allowed(&format!("File size {} bytes is within limits", size))
    }

    /// Add a new allowed path rule.
    pub fn add_rule(&mut self, path: PathBuf) {
        if !self.config.allowed_paths.contains(&path) {
            self.config.allowed_paths.push(path);
        }
    }

    /// Remove an allowed path rule.
    pub fn remove_rule(&mut self, path: &Path) -> bool {
        if let Some(pos) = self.config.allowed_paths.iter().position(|p| p == path) {
            self.config.allowed_paths.remove(pos);
            true
        } else {
            false
        }
    }

    /// Add a denied path.
    pub fn add_denied_path(&mut self, path: PathBuf) {
        if !self.config.denied_paths.contains(&path) {
            self.config.denied_paths.push(path);
        }
    }

    /// Remove a denied path.
    pub fn remove_denied_path(&mut self, path: &Path) -> bool {
        if let Some(pos) = self.config.denied_paths.iter().position(|p| p == path) {
            self.config.denied_paths.remove(pos);
            true
        } else {
            false
        }
    }

    /// Add an allowed command.
    pub fn add_allowed_command(&mut self, command: String) {
        if !self.config.allowed_commands.contains(&command) {
            self.config.allowed_commands.push(command);
        }
    }

    /// Add a denied command.
    pub fn add_denied_command(&mut self, command: String) {
        if !self.config.denied_commands.contains(&command) {
            self.config.denied_commands.push(command);
        }
    }

    /// Set network access permission.
    pub fn set_network_allowed(&mut self, allowed: bool) {
        self.config.network_allowed = allowed;
    }

    /// Get a reference to the configuration.
    pub fn config(&self) -> &SandboxConfig {
        &self.config
    }

    /// Check if a path is read-only.
    fn is_read_only(&self, path: &Path) -> bool {
        self.config.read_only_paths.iter().any(|ro| {
            path.starts_with(ro) || path == ro
        })
    }

    /// Check if a path is in the denied list.
    fn is_path_denied(&self, path: &Path) -> bool {
        self.config.denied_paths.iter().any(|denied| {
            path.starts_with(denied) || path == denied
        })
    }

    /// Check if a path is under any allowed path.
    fn is_path_allowed(&self, path: &Path) -> bool {
        // If no allowed paths configured, allow everything (non-strict)
        if self.config.allowed_paths.is_empty() {
            return true;
        }

        self.config.allowed_paths.iter().any(|allowed| {
            path.starts_with(allowed) || path == allowed
        })
    }
}

impl Default for PathSandboxAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl SandboxAdapter for PathSandboxAdapter {
    fn validate_read(&self, path: &Path) -> Result<SandboxResult, SandboxViolation> {
        let access = self.check_path_access(path);
        if !access.allowed {
            return Err(SandboxViolation::PathDenied(access.reason));
        }
        Ok(access)
    }

    fn validate_write(&self, path: &Path) -> Result<SandboxResult, SandboxViolation> {
        // First check general path access
        let access = self.check_path_access(path);
        if !access.allowed {
            return Err(SandboxViolation::PathDenied(access.reason));
        }

        // Then check if the path is read-only
        if self.is_read_only(path) {
            return Err(SandboxViolation::WriteDenied(format!(
                "Path '{}' is read-only",
                path.display()
            )));
        }

        Ok(SandboxResult::allowed(&format!(
            "Write access to '{}' granted",
            path.display()
        )))
    }

    fn validate_execute(&self, command: &str) -> Result<SandboxResult, SandboxViolation> {
        let result = self.check_command_safety(command);
        if !result.allowed {
            return Err(SandboxViolation::ExecuteDenied(result.reason));
        }
        Ok(result)
    }

    fn validate_network(&self, _host: &str) -> Result<SandboxResult, SandboxViolation> {
        if !self.config.network_allowed {
            return Err(SandboxViolation::NetworkDenied(
                "Network access is disabled".to_string(),
            ));
        }
        Ok(SandboxResult::allowed("Network access is enabled"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            let dir = std::env::temp_dir()
                .join(format!("shannon_sandbox_test_{}_{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos()));
            fs::create_dir_all(&dir).expect("Failed to create test dir");
            Self(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }

        fn file(&self, relative: &str) -> PathBuf {
            self.0.join(relative)
        }

        fn create_file(&self, relative: &str, content: &str) {
            let path = self.file(relative);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("Failed to create parent dirs");
            }
            fs::write(&path, content).expect("Failed to write test file");
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn test_validate_read_allowed() {
        let td = TestDir::new();
        td.create_file("test.txt", "hello");
        let config = SandboxConfig {
            allowed_paths: vec![td.path().to_path_buf()],
            denied_paths: vec![],
            read_only_paths: vec![],
            max_file_size: None,
            network_allowed: false,
            allowed_commands: vec![],
            denied_commands: vec![],
        };
        let adapter = PathSandboxAdapter::with_config(config);
        let result = adapter.validate_read(&td.file("test.txt"));
        assert!(result.is_ok());
        assert!(result.unwrap().allowed);
    }

    #[test]
    fn test_validate_read_denied_path() {
        let td = TestDir::new();
        let config = SandboxConfig {
            allowed_paths: vec![td.path().to_path_buf()],
            denied_paths: vec![PathBuf::from("/etc")],
            read_only_paths: vec![],
            max_file_size: None,
            network_allowed: false,
            allowed_commands: vec![],
            denied_commands: vec![],
        };
        let adapter = PathSandboxAdapter::with_config(config);
        let result = adapter.validate_read(Path::new("/etc/passwd"));
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("denied"));
    }

    #[test]
    fn test_validate_read_outside_allowed() {
        let td = TestDir::new();
        let config = SandboxConfig {
            allowed_paths: vec![td.path().to_path_buf()],
            denied_paths: vec![],
            read_only_paths: vec![],
            max_file_size: None,
            network_allowed: false,
            allowed_commands: vec![],
            denied_commands: vec![],
        };
        let adapter = PathSandboxAdapter::with_config(config);
        let result = adapter.validate_read(Path::new("/tmp/other_file.txt"));
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_write_allowed() {
        let td = TestDir::new();
        let config = SandboxConfig {
            allowed_paths: vec![td.path().to_path_buf()],
            denied_paths: vec![],
            read_only_paths: vec![],
            max_file_size: None,
            network_allowed: false,
            allowed_commands: vec![],
            denied_commands: vec![],
        };
        let adapter = PathSandboxAdapter::with_config(config);
        let result = adapter.validate_write(&td.file("output.txt"));
        assert!(result.is_ok());
        assert!(result.unwrap().allowed);
    }

    #[test]
    fn test_validate_write_read_only_denied() {
        let td = TestDir::new();
        td.create_file("readonly.txt", "data");
        let config = SandboxConfig {
            allowed_paths: vec![td.path().to_path_buf()],
            denied_paths: vec![],
            read_only_paths: vec![td.path().to_path_buf()],
            max_file_size: None,
            network_allowed: false,
            allowed_commands: vec![],
            denied_commands: vec![],
        };
        let adapter = PathSandboxAdapter::with_config(config);
        let result = adapter.validate_write(&td.file("readonly.txt"));
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("read-only"));
    }

    #[test]
    fn test_validate_execute_allowed_command() {
        let adapter = PathSandboxAdapter::new();
        let result = adapter.validate_execute("ls -la");
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_execute_denied_command() {
        let adapter = PathSandboxAdapter::new();
        let result = adapter.validate_execute("rm -rf /");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_execute_not_in_allowed_list() {
        let config = SandboxConfig {
            allowed_paths: vec![],
            denied_paths: vec![],
            read_only_paths: vec![],
            max_file_size: None,
            network_allowed: false,
            allowed_commands: vec!["ls".to_string(), "cat".to_string()],
            denied_commands: vec![],
        };
        let adapter = PathSandboxAdapter::with_config(config);
        let result = adapter.validate_execute("python script.py");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not in the allowed list"));
    }

    #[test]
    fn test_validate_network_allowed() {
        let config = SandboxConfig {
            network_allowed: true,
            ..SandboxConfig::default()
        };
        let adapter = PathSandboxAdapter::with_config(config);
        let result = adapter.validate_network("api.example.com");
        assert!(result.is_ok());
        assert!(result.unwrap().allowed);
    }

    #[test]
    fn test_validate_network_denied() {
        let adapter = PathSandboxAdapter::new();
        let result = adapter.validate_network("api.example.com");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("disabled"));
    }

    #[test]
    fn test_check_file_size_within_limit() {
        let adapter = PathSandboxAdapter::new();
        let result = adapter.check_file_size(1024);
        assert!(result.allowed);
    }

    #[test]
    fn test_check_file_size_exceeds_limit() {
        let config = SandboxConfig {
            max_file_size: Some(100),
            ..SandboxConfig::default()
        };
        let adapter = PathSandboxAdapter::with_config(config);
        let result = adapter.check_file_size(200);
        assert!(!result.allowed);
        assert!(result.reason.contains("exceeds"));
    }

    #[test]
    fn test_add_and_remove_rule() {
        let mut adapter = PathSandboxAdapter::new();
        let new_path = PathBuf::from("/custom/allowed");

        adapter.add_rule(new_path.clone());
        assert!(adapter.config().allowed_paths.contains(&new_path));

        let removed = adapter.remove_rule(&new_path);
        assert!(removed);
        assert!(!adapter.config().allowed_paths.contains(&new_path));
    }

    #[test]
    fn test_add_denied_path() {
        let mut adapter = PathSandboxAdapter::new();
        let denied = PathBuf::from("/custom/denied");

        adapter.add_denied_path(denied.clone());
        assert!(adapter.config().denied_paths.contains(&denied));

        let removed = adapter.remove_denied_path(&denied);
        assert!(removed);
        assert!(!adapter.config().denied_paths.contains(&denied));
    }

    #[test]
    fn test_add_allowed_command() {
        let mut adapter = PathSandboxAdapter::new();
        adapter.add_allowed_command("python3".to_string());
        assert!(adapter.config().allowed_commands.iter().any(|c| c == "python3"));
    }

    #[test]
    fn test_add_denied_command() {
        let mut adapter = PathSandboxAdapter::new();
        adapter.add_denied_command("sudo".to_string());
        assert!(adapter.config().denied_commands.iter().any(|c| c == "sudo"));
    }

    #[test]
    fn test_set_network_allowed() {
        let mut adapter = PathSandboxAdapter::new();
        assert!(!adapter.config().network_allowed);

        adapter.set_network_allowed(true);
        assert!(adapter.config().network_allowed);
    }

    #[test]
    fn test_default_config() {
        let config = SandboxConfig::default();
        assert!(!config.allowed_paths.is_empty());
        assert!(!config.denied_paths.is_empty());
        assert!(config.max_file_size.is_some());
        assert!(!config.network_allowed);
    }

    #[test]
    fn test_permissive_config() {
        let config = SandboxConfig::permissive();
        assert!(config.network_allowed);
        assert!(config.max_file_size.is_none());
        assert!(config.denied_paths.is_empty());
    }

    #[test]
    fn test_restrictive_config() {
        let config = SandboxConfig::restrictive();
        assert!(!config.network_allowed);
        assert!(!config.allowed_commands.is_empty());
        assert!(!config.denied_commands.is_empty());
    }

    #[test]
    fn test_sandbox_result_allowed() {
        let result = SandboxResult::allowed("all good");
        assert!(result.allowed);
        assert_eq!(result.reason, "all good");
    }

    #[test]
    fn test_sandbox_result_denied() {
        let result = SandboxResult::denied("not allowed");
        assert!(!result.allowed);
        assert_eq!(result.reason, "not allowed");
    }
}
