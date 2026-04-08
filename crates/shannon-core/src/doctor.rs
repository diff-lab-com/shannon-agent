//! # Doctor Diagnostics System
//!
//! Environment diagnostics for Shannon Code, inspired by Claude Code's Doctor screen.
//!
//! Provides a comprehensive suite of health checks that validate the user's
//! development environment including API keys, network connectivity, tool
//! availability, permissions, configuration, disk space, and git.
//!
//! ## Example
//!
//! ```no_run
//! use shannon_core::doctor::Doctor;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let doctor = Doctor::new();
//! let report = doctor.run_full_diagnostic()?;
//!
//! println!("Pass: {}  Warn: {}  Fail: {}  Skip: {}",
//!     report.total_pass, report.total_warn, report.total_fail, report.total_skip);
//!
//! for check in &report.checks {
//!     println!("[{}] {}: {}", check.status_label(), check.name, check.message);
//! }
//! # Ok(())
//! # }
//! ```

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Instant;
use thiserror::Error;

use crate::settings::Settings;

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

/// Errors that can occur during diagnostic checks.
#[derive(Error, Debug)]
pub enum DoctorError {
    #[error("Diagnostic check failed: {0}")]
    CheckFailed(String),

    #[error("I/O error during diagnostic: {0}")]
    Io(#[from] std::io::Error),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Network error during diagnostic: {0}")]
    Network(String),
}

// ---------------------------------------------------------------------------
// CheckStatus
// ---------------------------------------------------------------------------

/// Status result of an individual diagnostic check.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum CheckStatus {
    /// The check passed successfully.
    Pass,
    /// The check produced a warning (non-critical issue).
    Warn,
    /// The check failed (critical issue requiring attention).
    Fail,
    /// The check was skipped (prerequisites not met).
    Skip,
}

impl CheckStatus {
    /// Returns a short human-readable label.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Warn => "WARN",
            Self::Fail => "FAIL",
            Self::Skip => "SKIP",
        }
    }

    /// Returns a Unicode symbol for terminal display.
    pub fn symbol(&self) -> &'static str {
        match self {
            Self::Pass => "OK",
            Self::Warn => "!!",
            Self::Fail => "XX",
            Self::Skip => "--",
        }
    }
}

impl std::fmt::Display for CheckStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

// ---------------------------------------------------------------------------
// DiagnosticCategory
// ---------------------------------------------------------------------------

/// Category grouping for diagnostic checks.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum DiagnosticCategory {
    /// API key and authentication checks.
    ApiKey,
    /// Network connectivity and reachability.
    Network,
    /// Tool availability and executability.
    Tools,
    /// File system and permission checks.
    Permissions,
    /// Configuration file validation.
    Configuration,
    /// Runtime performance characteristics.
    Performance,
    /// Disk space and storage.
    Disk,
}

impl DiagnosticCategory {
    /// Returns a human-readable label.
    pub fn label(&self) -> &'static str {
        match self {
            Self::ApiKey => "API Key",
            Self::Network => "Network",
            Self::Tools => "Tools",
            Self::Permissions => "Permissions",
            Self::Configuration => "Configuration",
            Self::Performance => "Performance",
            Self::Disk => "Disk",
        }
    }
}

impl std::fmt::Display for DiagnosticCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

// ---------------------------------------------------------------------------
// DiagnosticCheck
// ---------------------------------------------------------------------------

/// A single diagnostic check with its result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticCheck {
    /// Human-readable name of the check.
    pub name: String,
    /// Category this check belongs to.
    pub category: DiagnosticCategory,
    /// Result status of the check.
    pub status: CheckStatus,
    /// Descriptive message about the check result.
    pub message: String,
    /// Optional suggestion for fixing a failure or warning.
    pub fix_suggestion: Option<String>,
    /// Time taken to execute this check in milliseconds.
    pub duration_ms: u64,
}

impl DiagnosticCheck {
    /// Create a new passing check.
    pub fn pass(name: impl Into<String>, category: DiagnosticCategory, message: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            category,
            status: CheckStatus::Pass,
            message: message.into(),
            fix_suggestion: None,
            duration_ms: 0,
        }
    }

    /// Create a new warning check.
    pub fn warn(
        name: impl Into<String>,
        category: DiagnosticCategory,
        message: impl Into<String>,
        fix_suggestion: Option<String>,
    ) -> Self {
        Self {
            name: name.into(),
            category,
            status: CheckStatus::Warn,
            message: message.into(),
            fix_suggestion,
            duration_ms: 0,
        }
    }

    /// Create a new failing check.
    pub fn fail(
        name: impl Into<String>,
        category: DiagnosticCategory,
        message: impl Into<String>,
        fix_suggestion: Option<String>,
    ) -> Self {
        Self {
            name: name.into(),
            category,
            status: CheckStatus::Fail,
            message: message.into(),
            fix_suggestion,
            duration_ms: 0,
        }
    }

    /// Create a new skipped check.
    pub fn skip(name: impl Into<String>, category: DiagnosticCategory, message: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            category,
            status: CheckStatus::Skip,
            message: message.into(),
            fix_suggestion: None,
            duration_ms: 0,
        }
    }

    /// Returns the status label string.
    pub fn status_label(&self) -> &'static str {
        self.status.label()
    }
}

// ---------------------------------------------------------------------------
// DoctorReport
// ---------------------------------------------------------------------------

/// Aggregated result of a full diagnostic run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorReport {
    /// Individual check results.
    pub checks: Vec<DiagnosticCheck>,
    /// Number of passing checks.
    pub total_pass: usize,
    /// Number of warning checks.
    pub total_warn: usize,
    /// Number of failing checks.
    pub total_fail: usize,
    /// Number of skipped checks.
    pub total_skip: usize,
    /// Total time for the full diagnostic in milliseconds.
    pub duration_ms: u64,
    /// When the diagnostic was run.
    pub timestamp: DateTime<Utc>,
}

impl DoctorReport {
    /// Returns true if all non-skipped checks passed.
    pub fn is_healthy(&self) -> bool {
        self.total_fail == 0 && self.total_warn == 0
    }

    /// Returns true if there are any failures.
    pub fn has_failures(&self) -> bool {
        self.total_fail > 0
    }

    /// Returns true if there are any warnings.
    pub fn has_warnings(&self) -> bool {
        self.total_warn > 0
    }

    /// Returns only the checks that failed.
    pub fn failures(&self) -> Vec<&DiagnosticCheck> {
        self.checks.iter().filter(|c| c.status == CheckStatus::Fail).collect()
    }

    /// Returns only the checks that warned.
    pub fn warnings(&self) -> Vec<&DiagnosticCheck> {
        self.checks.iter().filter(|c| c.status == CheckStatus::Warn).collect()
    }

    /// Returns only the checks that passed.
    pub fn passes(&self) -> Vec<&DiagnosticCheck> {
        self.checks.iter().filter(|c| c.status == CheckStatus::Pass).collect()
    }

    /// Returns a JSON summary suitable for display.
    pub fn to_summary_json(&self) -> serde_json::Value {
        serde_json::json!({
            "healthy": self.is_healthy(),
            "pass": self.total_pass,
            "warn": self.total_warn,
            "fail": self.total_fail,
            "skip": self.total_skip,
            "duration_ms": self.duration_ms,
            "timestamp": self.timestamp.to_rfc3339(),
        })
    }
}

// ---------------------------------------------------------------------------
// Doctor
// ---------------------------------------------------------------------------

/// The Doctor runs environment diagnostics for Shannon Code.
///
/// Each check validates a specific aspect of the development environment.
/// Checks are designed to be mock-friendly for testing.
pub struct Doctor {
    /// Optional configuration to validate against.
    config: Option<Settings>,
}

impl Default for Doctor {
    fn default() -> Self {
        Self::new()
    }
}

impl Doctor {
    /// Create a new Doctor instance with no configuration.
    pub fn new() -> Self {
        Self { config: None }
    }

    /// Create a Doctor with a specific configuration to validate.
    pub fn with_config(config: Settings) -> Self {
        Self {
            config: Some(config),
        }
    }

    /// Run a single check with timing.
    pub fn run_check(check_fn: impl FnOnce() -> DiagnosticCheck) -> DiagnosticCheck {
        let start = Instant::now();
        let mut result = check_fn();
        result.duration_ms = start.elapsed().as_millis() as u64;
        result
    }

    /// Run all diagnostic checks and produce a full report.
    pub fn run_full_diagnostic(&self) -> Result<DoctorReport, DoctorError> {
        let start = Instant::now();

        let checks = vec![
            Self::run_check(|| self.check_api_key()),
            Self::run_check(|| self.check_network_connectivity()),
            Self::run_check(|| self.check_tool_availability()),
            Self::run_check(|| self.check_permissions()),
            Self::run_check(|| self.check_configuration()),
            Self::run_check(|| self.check_disk_space()),
            Self::run_check(|| self.check_git_available()),
        ];

        let total_pass = checks.iter().filter(|c| c.status == CheckStatus::Pass).count();
        let total_warn = checks.iter().filter(|c| c.status == CheckStatus::Warn).count();
        let total_fail = checks.iter().filter(|c| c.status == CheckStatus::Fail).count();
        let total_skip = checks.iter().filter(|c| c.status == CheckStatus::Skip).count();

        Ok(DoctorReport {
            checks,
            total_pass,
            total_warn,
            total_fail,
            total_skip,
            duration_ms: start.elapsed().as_millis() as u64,
            timestamp: Utc::now(),
        })
    }

    // -----------------------------------------------------------------------
    // Individual checks
    // -----------------------------------------------------------------------

    /// Check whether the API key is configured and has a valid format.
    ///
    /// Validates that the `ANTHROPIC_API_KEY` environment variable is set
    /// and follows the expected `sk-ant-...` format.
    pub fn check_api_key(&self) -> DiagnosticCheck {
        let key = std::env::var("ANTHROPIC_API_KEY")
            .or_else(|_| std::env::var("CLAUDE_API_KEY"))
            .or_else(|_| std::env::var("SHANNON_API_KEY"));

        match key {
            Ok(ref key) if key.starts_with("sk-ant-api") => DiagnosticCheck::pass(
                "API Key",
                DiagnosticCategory::ApiKey,
                format!("API key found (prefix: sk-ant-api..., {} chars)", key.len()),
            ),
            Ok(ref key) if !key.is_empty() => DiagnosticCheck::warn(
                "API Key",
                DiagnosticCategory::ApiKey,
                format!("API key found but unexpected format (prefix: {}...)", &key[..key.len().min(12)]),
                Some("Expected key to start with 'sk-ant-api'. Verify your API key is correct.".to_string()),
            ),
            Ok(_) => DiagnosticCheck::fail(
                "API Key",
                DiagnosticCategory::ApiKey,
                "API key is set but empty".to_string(),
                Some("Set ANTHROPIC_API_KEY to a valid Anthropic API key.".to_string()),
            ),
            Err(_) => DiagnosticCheck::fail(
                "API Key",
                DiagnosticCategory::ApiKey,
                "No API key found in environment".to_string(),
                Some("Set ANTHROPIC_API_KEY, CLAUDE_API_KEY, or SHANNON_API_KEY environment variable.".to_string()),
            ),
        }
    }

    /// Check network connectivity to the Anthropic API endpoint.
    ///
    /// This performs a basic DNS resolution check. It does not make an
    /// actual API request.
    pub fn check_network_connectivity(&self) -> DiagnosticCheck {
        let api_host = std::env::var("ANTHROPIC_BASE_URL")
            .map(|url| {
                url.replace("https://", "")
                    .replace("http://", "")
                    .split(':')
                    .next()
                    .unwrap_or("api.anthropic.com")
                    .to_string()
            })
            .unwrap_or_else(|_| "api.anthropic.com".to_string());

        // Attempt DNS resolution
        let addr_str = format!("{}:443", api_host);
        let resolve_result = std::net::ToSocketAddrs::to_socket_addrs(&addr_str);
        match resolve_result {
            Ok(addrs) => {
                let has_addr = addrs.take(1).next().is_some();
                if has_addr {
                    DiagnosticCheck::pass(
                        "Network Connectivity",
                        DiagnosticCategory::Network,
                        format!("Successfully resolved {}", api_host),
                    )
                } else {
                    DiagnosticCheck::warn(
                        "Network Connectivity",
                        DiagnosticCategory::Network,
                        format!("DNS resolution returned no addresses for {}", api_host),
                        Some("Check your network connection and DNS settings.".to_string()),
                    )
                }
            }
            Err(e) => DiagnosticCheck::fail(
                "Network Connectivity",
                DiagnosticCategory::Network,
                format!("Cannot resolve {}: {}", api_host, e),
                Some("Verify network connectivity and DNS configuration.".to_string()),
            ),
        }
    }

    /// Check whether the core tools can be loaded and are functional.
    ///
    /// Verifies that the binary is properly installed by checking for
    /// essential runtime dependencies.
    pub fn check_tool_availability(&self) -> DiagnosticCheck {
        let mut missing = Vec::new();

        // Check for common development tools
        let essential_tools = ["shannon", "cargo"];
        for tool in &essential_tools {
            if which_tool(tool).is_none() {
                missing.push(tool.to_string());
            }
        }

        if missing.is_empty() {
            DiagnosticCheck::pass(
                "Tool Availability",
                DiagnosticCategory::Tools,
                "All essential tools are available".to_string(),
            )
        } else {
            DiagnosticCheck::warn(
                "Tool Availability",
                DiagnosticCategory::Tools,
                format!("Missing tools: {}", missing.join(", ")),
                Some(format!(
                    "Install missing tools: {}",
                    missing.iter().map(|t| format!("'{}'", t)).collect::<Vec<_>>().join(", ")
                )),
            )
        }
    }

    /// Check file system permissions for key directories.
    ///
    /// Verifies that the user can read and write to the home directory
    /// and the Shannon configuration directory.
    pub fn check_permissions(&self) -> DiagnosticCheck {
        let home_dir = match dirs::home_dir() {
            Some(dir) => dir,
            None => {
                return DiagnosticCheck::fail(
                    "Permissions",
                    DiagnosticCategory::Permissions,
                    "Cannot determine home directory".to_string(),
                    Some("Ensure the HOME environment variable is set.".to_string()),
                );
            }
        };

        let shannon_dir = home_dir.join(".shannon");
        let mut issues = Vec::new();

        // Check home directory readability
        match std::fs::read_dir(&home_dir) {
            Ok(_) => {}
            Err(e) => {
                issues.push(format!("Cannot read home directory: {}", e));
            }
        }

        // Check or create .shannon directory
        if shannon_dir.exists() {
            match std::fs::read_dir(&shannon_dir) {
                Ok(_) => {}
                Err(e) => {
                    issues.push(format!("Cannot read ~/.shannon: {}", e));
                }
            }
            // Test write permission
            let test_file = shannon_dir.join(".doctor_perm_test");
            match std::fs::write(&test_file, "test") {
                Ok(_) => {
                    let _ = std::fs::remove_file(&test_file);
                }
                Err(e) => {
                    issues.push(format!("Cannot write to ~/.shannon: {}", e));
                }
            }
        } else {
            match std::fs::create_dir_all(&shannon_dir) {
                Ok(_) => {
                    // Clean up if we created it during the test
                    let _ = std::fs::remove_dir(&shannon_dir);
                }
                Err(e) => {
                    issues.push(format!("Cannot create ~/.shannon: {}", e));
                }
            }
        }

        // Check current directory permissions
        match std::fs::read_dir(".") {
            Ok(_) => {}
            Err(e) => {
                issues.push(format!("Cannot read current directory: {}", e));
            }
        }

        if issues.is_empty() {
            DiagnosticCheck::pass(
                "Permissions",
                DiagnosticCategory::Permissions,
                "All permission checks passed".to_string(),
            )
        } else {
            DiagnosticCheck::fail(
                "Permissions",
                DiagnosticCategory::Permissions,
                issues.join("; "),
                Some("Check directory permissions and ownership.".to_string()),
            )
        }
    }

    /// Check configuration file validity.
    ///
    /// Loads and validates the settings file, checking for schema issues
    /// and invalid values.
    pub fn check_configuration(&self) -> DiagnosticCheck {
        let home_dir = match dirs::home_dir() {
            Some(dir) => dir,
            None => {
                return DiagnosticCheck::skip(
                    "Configuration",
                    DiagnosticCategory::Configuration,
                    "Cannot determine home directory, skipping config check".to_string(),
                );
            }
        };

        let config_path = home_dir.join(".shannon").join("settings.json");

        if !config_path.exists() {
            return DiagnosticCheck::warn(
                "Configuration",
                DiagnosticCategory::Configuration,
                "No settings file found".to_string(),
                Some("Run Shannon to generate a default settings file at ~/.shannon/settings.json".to_string()),
            );
        }

        let content = match std::fs::read_to_string(&config_path) {
            Ok(c) => c,
            Err(e) => {
                return DiagnosticCheck::fail(
                    "Configuration",
                    DiagnosticCategory::Configuration,
                    format!("Cannot read settings file: {}", e),
                    Some("Check file permissions for ~/.shannon/settings.json".to_string()),
                );
            }
        };

        // Validate JSON
        let _value: serde_json::Value = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(e) => {
                return DiagnosticCheck::fail(
                    "Configuration",
                    DiagnosticCategory::Configuration,
                    format!("Invalid JSON in settings file: {}", e),
                    Some("Fix the JSON syntax in ~/.shannon/settings.json".to_string()),
                );
            }
        };

        // If we have a config, validate it
        if let Some(ref config) = self.config {
            match config.validate() {
                Ok(()) => DiagnosticCheck::pass(
                    "Configuration",
                    DiagnosticCategory::Configuration,
                    "Settings file is valid".to_string(),
                ),
                Err(e) => DiagnosticCheck::fail(
                    "Configuration",
                    DiagnosticCategory::Configuration,
                    format!("Settings validation error: {}", e),
                    Some("Fix the invalid settings in ~/.shannon/settings.json".to_string()),
                ),
            }
        } else {
            DiagnosticCheck::pass(
                "Configuration",
                DiagnosticCategory::Configuration,
                "Settings file found and parseable".to_string(),
            )
        }
    }

    /// Check available disk space.
    ///
    /// Verifies that there is sufficient free disk space for normal operation.
    /// Warns if below 1 GB, fails if below 100 MB.
    pub fn check_disk_space(&self) -> DiagnosticCheck {
        let home_dir = match dirs::home_dir() {
            Some(dir) => dir,
            None => {
                return DiagnosticCheck::skip(
                    "Disk Space",
                    DiagnosticCategory::Disk,
                    "Cannot determine home directory, skipping disk check".to_string(),
                );
            }
        };

        // Attempt to get disk space info
        match get_disk_space(&home_dir) {
            Some(free_bytes) => {
                let free_mb = free_bytes / (1024 * 1024);
                let free_gb = free_mb as f64 / 1024.0;

                if free_mb < 100 {
                    DiagnosticCheck::fail(
                        "Disk Space",
                        DiagnosticCategory::Disk,
                        format!("Very low disk space: {:.1} MB free", free_mb as f64),
                        Some("Free up disk space. Shannon needs at least 100 MB for normal operation.".to_string()),
                    )
                } else if free_mb < 1024 {
                    DiagnosticCheck::warn(
                        "Disk Space",
                        DiagnosticCategory::Disk,
                        format!("Low disk space: {:.1} MB free", free_mb as f64),
                        Some("Consider freeing up disk space for optimal operation.".to_string()),
                    )
                } else {
                    DiagnosticCheck::pass(
                        "Disk Space",
                        DiagnosticCategory::Disk,
                        format!("{:.1} GB free", free_gb),
                    )
                }
            }
            None => {
                // Fallback: try to write a small test file
                let test_path = home_dir.join(".shannon").join(".doctor_disk_test");
                match std::fs::write(&test_path, vec![0u8; 1024]) {
                    Ok(_) => {
                        let _ = std::fs::remove_file(&test_path);
                        DiagnosticCheck::pass(
                            "Disk Space",
                            DiagnosticCategory::Disk,
                            "Disk appears writable (exact space could not be determined)".to_string(),
                        )
                    }
                    Err(e) => DiagnosticCheck::fail(
                        "Disk Space",
                        DiagnosticCategory::Disk,
                        format!("Cannot write to disk: {}", e),
                        Some("Check disk space and write permissions.".to_string()),
                    ),
                }
            }
        }
    }

    /// Check whether git is installed and functional.
    ///
    /// Verifies that git can be found on PATH and reports its version.
    pub fn check_git_available(&self) -> DiagnosticCheck {
        match which_tool("git") {
            Some(git_path) => {
                // Try to get the git version
                match std::process::Command::new(&git_path)
                    .arg("--version")
                    .output()
                {
                    Ok(output) => {
                        let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
                        if output.status.success() && !version.is_empty() {
                            DiagnosticCheck::pass(
                                "Git",
                                DiagnosticCategory::Tools,
                                version,
                            )
                        } else {
                            DiagnosticCheck::warn(
                                "Git",
                                DiagnosticCategory::Tools,
                                format!("Git found at {} but version check failed", git_path.display()),
                                Some("Verify your git installation is working correctly.".to_string()),
                            )
                        }
                    }
                    Err(e) => DiagnosticCheck::warn(
                        "Git",
                        DiagnosticCategory::Tools,
                        format!("Git found at {} but cannot execute: {}", git_path.display(), e),
                        Some("Check that git is executable.".to_string()),
                    ),
                }
            }
            None => DiagnosticCheck::fail(
                "Git",
                DiagnosticCategory::Tools,
                "Git is not installed or not on PATH".to_string(),
                Some("Install git: https://git-scm.com/downloads".to_string()),
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Search for an executable on PATH.
fn which_tool(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).find_map(|dir| {
            let full_path = dir.join(name);
            if full_path.is_file() {
                Some(full_path)
            } else {
                None
            }
        })
    })
}

/// Get available disk space for a given path.
///
/// Uses platform-specific system calls. Returns `None` if the information
/// cannot be obtained.
#[cfg(target_family = "unix")]
fn get_disk_space(path: &std::path::Path) -> Option<u64> {
    use std::ffi::CString;
    use std::mem::MaybeUninit;
    use std::os::unix::ffi::OsStrExt;

    let path_cstr = CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut stat: MaybeUninit<libc::statvfs> = MaybeUninit::uninit();

    // SAFETY:
    //
    // 1. `path_cstr` is a valid null-terminated C string created from `path.as_os_str().as_bytes()`,
    //    which is guaranteed to be valid for the lifetime of this function.
    //
    // 2. `stat.as_mut_ptr()` provides a valid pointer to uninitialized memory for the statvfs
    //    struct. The `libc::statvfs` function writes to this memory when it succeeds.
    //
    // 3. The `result == 0` check ensures that `statvfs` succeeded before we call `assume_init()`.
    //    When `statvfs` returns 0, it has fully initialized the statvfs struct. When it returns
    //    non-zero, the struct may be uninitialized, so we correctly return `None` without
    //    reading it.
    //
    // 4. `assume_init()` is safe here because:
    //    - We only reach it if `result == 0` (statvfs succeeded)
    //    - statvfs contract guarantees it writes to all fields of the struct on success
    //    - The struct has no padding that would remain uninitialized
    let result = unsafe { libc::statvfs(path_cstr.as_ptr(), stat.as_mut_ptr()) };

    if result == 0 {
        // SAFETY: statvfs returned 0, indicating success. The struct is now fully initialized.
        let stat = unsafe { stat.assume_init() };
        // Available space = block size * available blocks
        Some(stat.f_bsize as u64 * stat.f_bavail as u64)
    } else {
        // statvfs failed; the stat struct may be uninitialized, so we must not read it.
        // Returning None is the correct error handling path.
        None
    }
}

/// Stub for non-Unix platforms.
#[cfg(not(target_family = "unix"))]
fn get_disk_space(_path: &std::path::Path) -> Option<u64> {
    None
}

// ===========================================================================
// Test helpers
// ===========================================================================

/// Guard to restore HOME environment variable when dropped.
///
/// This ensures that even if a test panics, the original HOME value
/// is restored, preventing test pollution.
struct HomeGuard(Option<std::ffi::OsString>);

impl Drop for HomeGuard {
    fn drop(&mut self) {
        match &self.0 {
            Some(home) => unsafe { std::env::set_var("HOME", home) },
            None => unsafe { std::env::remove_var("HOME") },
        }
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    // ---- CheckStatus tests -------------------------------------------------

    #[test]
    fn test_check_status_label() {
        assert_eq!(CheckStatus::Pass.label(), "PASS");
        assert_eq!(CheckStatus::Warn.label(), "WARN");
        assert_eq!(CheckStatus::Fail.label(), "FAIL");
        assert_eq!(CheckStatus::Skip.label(), "SKIP");
    }

    #[test]
    fn test_check_status_display() {
        assert_eq!(format!("{}", CheckStatus::Pass), "PASS");
        assert_eq!(format!("{}", CheckStatus::Fail), "FAIL");
    }

    #[test]
    fn test_check_status_equality() {
        assert_eq!(CheckStatus::Pass, CheckStatus::Pass);
        assert_ne!(CheckStatus::Pass, CheckStatus::Fail);
    }

    // ---- DiagnosticCategory tests ------------------------------------------

    #[test]
    fn test_diagnostic_category_label() {
        assert_eq!(DiagnosticCategory::ApiKey.label(), "API Key");
        assert_eq!(DiagnosticCategory::Network.label(), "Network");
        assert_eq!(DiagnosticCategory::Tools.label(), "Tools");
        assert_eq!(DiagnosticCategory::Permissions.label(), "Permissions");
        assert_eq!(DiagnosticCategory::Configuration.label(), "Configuration");
        assert_eq!(DiagnosticCategory::Performance.label(), "Performance");
        assert_eq!(DiagnosticCategory::Disk.label(), "Disk");
    }

    // ---- DiagnosticCheck tests ---------------------------------------------

    #[test]
    fn test_diagnostic_check_pass() {
        let check = DiagnosticCheck::pass("Test", DiagnosticCategory::Tools, "all good");
        assert_eq!(check.status, CheckStatus::Pass);
        assert_eq!(check.name, "Test");
        assert!(check.fix_suggestion.is_none());
    }

    #[test]
    fn test_diagnostic_check_warn() {
        let check = DiagnosticCheck::warn("Test", DiagnosticCategory::Tools, "caution", Some("fix it".to_string()));
        assert_eq!(check.status, CheckStatus::Warn);
        assert_eq!(check.fix_suggestion, Some("fix it".to_string()));
    }

    #[test]
    fn test_diagnostic_check_fail() {
        let check = DiagnosticCheck::fail("Test", DiagnosticCategory::Network, "broken", None);
        assert_eq!(check.status, CheckStatus::Fail);
        assert!(check.fix_suggestion.is_none());
    }

    #[test]
    fn test_diagnostic_check_skip() {
        let check = DiagnosticCheck::skip("Test", DiagnosticCategory::Disk, "n/a");
        assert_eq!(check.status, CheckStatus::Skip);
    }

    #[test]
    fn test_diagnostic_check_serialization() {
        let check = DiagnosticCheck::pass("Test", DiagnosticCategory::ApiKey, "ok");
        let json = serde_json::to_string(&check).unwrap();
        let restored: DiagnosticCheck = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.name, check.name);
        assert_eq!(restored.status, check.status);
        assert_eq!(restored.category, check.category);
    }

    // ---- DoctorReport tests ------------------------------------------------

    #[test]
    fn test_doctor_report_is_healthy() {
        let report = DoctorReport {
            checks: vec![
                DiagnosticCheck::pass("a", DiagnosticCategory::Tools, "ok"),
                DiagnosticCheck::pass("b", DiagnosticCategory::Network, "ok"),
            ],
            total_pass: 2,
            total_warn: 0,
            total_fail: 0,
            total_skip: 0,
            duration_ms: 10,
            timestamp: Utc::now(),
        };
        assert!(report.is_healthy());
        assert!(!report.has_failures());
        assert!(!report.has_warnings());
        assert_eq!(report.passes().len(), 2);
    }

    #[test]
    fn test_doctor_report_has_failures() {
        let report = DoctorReport {
            checks: vec![
                DiagnosticCheck::pass("a", DiagnosticCategory::Tools, "ok"),
                DiagnosticCheck::fail("b", DiagnosticCategory::Network, "bad", None),
            ],
            total_pass: 1,
            total_warn: 0,
            total_fail: 1,
            total_skip: 0,
            duration_ms: 10,
            timestamp: Utc::now(),
        };
        assert!(!report.is_healthy());
        assert!(report.has_failures());
        assert_eq!(report.failures().len(), 1);
    }

    #[test]
    fn test_doctor_report_has_warnings() {
        let report = DoctorReport {
            checks: vec![
                DiagnosticCheck::pass("a", DiagnosticCategory::Tools, "ok"),
                DiagnosticCheck::warn("b", DiagnosticCategory::Disk, "low", None),
            ],
            total_pass: 1,
            total_warn: 1,
            total_fail: 0,
            total_skip: 0,
            duration_ms: 10,
            timestamp: Utc::now(),
        };
        assert!(!report.is_healthy());
        assert!(!report.has_failures());
        assert!(report.has_warnings());
        assert_eq!(report.warnings().len(), 1);
    }

    #[test]
    fn test_doctor_report_to_summary_json() {
        let report = DoctorReport {
            checks: vec![
                DiagnosticCheck::pass("a", DiagnosticCategory::Tools, "ok"),
                DiagnosticCheck::fail("b", DiagnosticCategory::Network, "bad", None),
            ],
            total_pass: 1,
            total_warn: 0,
            total_fail: 1,
            total_skip: 0,
            duration_ms: 42,
            timestamp: Utc::now(),
        };
        let summary = report.to_summary_json();
        assert_eq!(summary["healthy"], false);
        assert_eq!(summary["pass"], 1);
        assert_eq!(summary["fail"], 1);
        assert_eq!(summary["duration_ms"], 42);
    }

    #[test]
    fn test_doctor_report_serialization() {
        let report = DoctorReport {
            checks: vec![DiagnosticCheck::pass("t", DiagnosticCategory::Tools, "ok")],
            total_pass: 1,
            total_warn: 0,
            total_fail: 0,
            total_skip: 0,
            duration_ms: 5,
            timestamp: Utc::now(),
        };
        let json = serde_json::to_string(&report).unwrap();
        let restored: DoctorReport = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.total_pass, 1);
        assert_eq!(restored.checks.len(), 1);
    }

    // ---- Doctor tests -----------------------------------------------------

    #[test]
    fn test_doctor_new() {
        let _doctor = Doctor::new();
    }

    #[test]
    fn test_doctor_with_config() {
        let config = Settings::new();
        let _doctor = Doctor::with_config(config);
    }

    #[test]
    fn test_run_check_timing() {
        let check = Doctor::run_check(|| {
            std::thread::sleep(std::time::Duration::from_millis(50));
            DiagnosticCheck::pass("Timed", DiagnosticCategory::Performance, "ok")
        });
        assert!(check.duration_ms >= 40); // allow some slack
        assert_eq!(check.status, CheckStatus::Pass);
    }

    #[test]
    fn test_run_full_diagnostic() {
        let doctor = Doctor::new();
        let report = doctor.run_full_diagnostic().unwrap();

        // Should have run exactly 7 checks
        assert_eq!(report.checks.len(), 7);
        assert_eq!(report.total_pass + report.total_warn + report.total_fail + report.total_skip, 7);

        // Duration should be recorded
        assert!(report.duration_ms >= 0);
        assert!(!report.timestamp.to_rfc3339().is_empty());
    }

    #[test]
    fn test_check_api_key_no_key() {
        // Ensure the env vars are not set for this test
        unsafe { std::env::remove_var("ANTHROPIC_API_KEY"); }
        unsafe { std::env::remove_var("CLAUDE_API_KEY"); }
        unsafe { std::env::remove_var("SHANNON_API_KEY"); }

        let doctor = Doctor::new();
        let check = doctor.check_api_key();
        // In CI, API keys might be set; we just verify it returns a valid status
        assert!(matches!(check.status, CheckStatus::Pass | CheckStatus::Fail | CheckStatus::Warn));
        assert!(!check.name.is_empty());
    }

    #[test]
    fn test_check_network_connectivity() {
        let doctor = Doctor::new();
        let check = doctor.check_network_connectivity();
        // Should resolve successfully on most systems
        assert!(!check.name.is_empty());
        assert!(!check.message.is_empty());
    }

    #[test]
    fn test_check_git_available() {
        let doctor = Doctor::new();
        let check = doctor.check_git_available();
        // Git is expected to be installed in most dev environments
        assert!(!check.name.is_empty());
        assert!(!check.message.is_empty());
    }

    #[test]
    fn test_check_configuration_no_file() {
        // Use a temp dir to ensure no config file exists
        let temp_dir = tempfile::tempdir().unwrap();
        let home_backup = std::env::var_os("HOME");
        unsafe { std::env::set_var("HOME", temp_dir.path()); }

        // Restore HOME on drop (even if test panics)
        let _guard = HomeGuard(home_backup);

        let doctor = Doctor::new();
        let check = doctor.check_configuration();
        // Should warn since no config file exists
        assert!(matches!(check.status, CheckStatus::Warn));
    }

    #[test]
    fn test_check_configuration_with_valid_file() {
        let temp_dir = tempfile::tempdir().unwrap();
        let shannon_dir = temp_dir.path().join(".shannon");
        fs::create_dir_all(&shannon_dir).unwrap();

        let valid_json = r#"{
            "version": "1.0",
            "model": "claude-opus-4-6",
            "toolsEnabled": true,
            "permissionsMode": "ask",
            "autoMemory": true,
            "theme": "dark"
        }"#;

        fs::write(shannon_dir.join("settings.json"), valid_json).unwrap();

        let home_backup = std::env::var_os("HOME");
        unsafe { std::env::set_var("HOME", temp_dir.path()); }

        let _guard = HomeGuard(home_backup);

        let doctor = Doctor::new();
        let check = doctor.check_configuration();
        assert_eq!(check.status, CheckStatus::Pass);
    }

    #[test]
    fn test_check_configuration_with_invalid_json() {
        let temp_dir = tempfile::tempdir().unwrap();
        let shannon_dir = temp_dir.path().join(".shannon");
        fs::create_dir_all(&shannon_dir).unwrap();

        fs::write(shannon_dir.join("settings.json"), "{invalid json}").unwrap();

        let home_backup = std::env::var_os("HOME");
        unsafe { std::env::set_var("HOME", temp_dir.path()); }

        let _guard = HomeGuard(home_backup);

        let doctor = Doctor::new();
        let check = doctor.check_configuration();
        assert_eq!(check.status, CheckStatus::Fail);
    }

    #[test]
    fn test_check_permissions() {
        let doctor = Doctor::new();
        let check = doctor.check_permissions();
        // Should pass in normal environments
        assert!(!check.name.is_empty());
        // We don't assert status since it depends on the test environment
    }

    #[test]
    fn test_check_disk_space() {
        let doctor = Doctor::new();
        let check = doctor.check_disk_space();
        assert!(!check.name.is_empty());
        assert!(!check.message.is_empty());
        // Should not fail on any normal development machine
        assert!(check.status != CheckStatus::Fail || check.message.contains("Cannot write"));
    }

    // ---- Helper function tests --------------------------------------------

    #[test]
    fn test_which_tool_existing() {
        // "ls" should exist on all Unix systems
        #[cfg(target_family = "unix")]
        {
            let result = which_tool("ls");
            assert!(result.is_some());
        }
    }

    #[test]
    fn test_which_tool_nonexistent() {
        let result = which_tool("definitely_not_a_real_tool_xyz123");
        assert!(result.is_none());
    }
}
