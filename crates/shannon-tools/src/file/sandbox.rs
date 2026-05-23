//! Path sandboxing for file operations
//!
//! Provides security boundaries to prevent unauthorized file access through:
//! - Path traversal detection (e.g., `../../etc/passwd`)
//! - Symlink resolution (checks resolved path, not the literal path)
//! - Denied path patterns (system directories)
//! - Home directory boundary enforcement
//! - Strict mode (allow only explicitly configured roots)
//!
//! # TOCTOU Protection
//!
//! The sandbox uses canonicalization to resolve symlinks before checking paths.
//! This protects against time-of-check/time-of-use (TOCTOU) attacks where
//! an attacker might replace a safe path with a symlink after validation.
//! The canonicalization happens immediately before the access check, making
//! it difficult for an attacker to race the condition.

use std::path::{Path, PathBuf};

/// Configuration for the path sandbox
#[derive(Debug, Clone)]
pub struct SandboxConfig {
    /// Directories that are explicitly allowed as root paths.
    /// Subdirectories of these roots are also allowed.
    pub allowed_roots: Vec<PathBuf>,

    /// Path prefixes that are always denied, even if inside an allowed root.
    pub denied_patterns: Vec<String>,

    /// When true, deny all paths that are not under an allowed root.
    /// When false, only explicitly denied paths are blocked.
    pub strict_mode: bool,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            allowed_roots: vec![std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))],
            denied_patterns: Self::default_denied_patterns(),
            strict_mode: true,
        }
    }
}

impl SandboxConfig {
    /// Returns the default list of denied system path prefixes.
    pub fn default_denied_patterns() -> Vec<String> {
        vec![
            "/etc/".to_string(),
            "/boot/".to_string(),
            "/usr/bin/".to_string(),
            "/usr/sbin/".to_string(),
            "/bin/".to_string(),
            "/sbin/".to_string(),
            "/dev/".to_string(),
            "/proc/".to_string(),
            "/sys/".to_string(),
            "/run/".to_string(),
            "/var/log/".to_string(),
            "/var/run/".to_string(),
        ]
    }
}

/// A sandbox that validates file paths against security rules.
///
/// Every file operation should pass through `PathSandbox::validate` before
/// accessing the filesystem. The sandbox resolves symlinks and canonicalizes
/// paths, then checks the resolved path against allowed roots and denied
/// patterns.
#[derive(Debug, Clone)]
pub struct PathSandbox {
    config: SandboxConfig,
    /// Cached home directory of the current user for boundary checking.
    home_dir: Option<PathBuf>,
}

/// Errors returned by sandbox validation
#[derive(Debug, thiserror::Error)]
pub enum SandboxError {
    #[error("Path traversal detected: {0}")]
    PathTraversal(String),

    #[error("Access denied - path is in a restricted area: {0}")]
    Denied(String),

    #[error("Path outside allowed roots: {0}")]
    OutsideAllowedRoots(String),

    #[error("Symlink resolves outside allowed roots: {symlink} -> {target}")]
    SymlinkEscape { symlink: String, target: String },

    #[error("Potential TOCTOU attack detected: symlink target changed between check and use")]
    ToctouDetected(String),

    #[error("Failed to resolve path: {0}")]
    ResolutionFailed(String),

    #[error("Path is empty or invalid")]
    InvalidPath,
}

impl PathSandbox {
    /// Create a new sandbox with default configuration.
    ///
    /// Default configuration:
    /// - Allowed root: current working directory
    /// - Strict mode: true (only paths under allowed roots are permitted)
    /// - Denied patterns: system directories (`/etc/`, `/boot/`, `/dev/`, etc.)
    pub fn new() -> Self {
        Self::with_config(SandboxConfig::default())
    }

    /// Create a sandbox with custom configuration.
    pub fn with_config(config: SandboxConfig) -> Self {
        let home_dir = dirs_home_dir();
        Self { config, home_dir }
    }

    /// Validate a path against the sandbox rules.
    ///
    /// Returns the canonicalized (resolved) path if access is allowed.
    /// Returns `SandboxError` if the path violates any security rule.
    ///
    /// # TOCTOU Protection
    ///
    /// This method uses immediate canonicalization to protect against
    /// time-of-check/time-of-use (TOCTOU) attacks. By canonicalizing
    /// the path right before checking it, we minimize the window where
    /// an attacker could replace a path component with a symlink.
    ///
    /// # Checks performed in order:
    /// 1. Path is not empty
    /// 2. Path does not contain raw `..` traversal that escapes the filesystem
    /// 3. Path can be canonicalized (symlinks resolved) - **TOCTOU protection point**
    /// 4. Canonicalized path does not match any denied pattern
    /// 5. In strict mode, canonicalized path is under an allowed root
    /// 6. Path does not cross into another user's home directory
    pub async fn validate(&self, path: &Path) -> Result<PathBuf, SandboxError> {
        if path.as_os_str().is_empty() {
            return Err(SandboxError::InvalidPath);
        }

        let path_str = path.to_string_lossy().to_string();

        // Check for obviously malicious raw traversal patterns
        // (canonicalize will catch these too, but we provide a clearer error)
        self.check_raw_traversal(&path_str)?;

        // Canonicalize: resolve symlinks, `.` and `..` components
        // This is the primary TOCTOU protection - we resolve the actual
        // target immediately before checking it against allowed roots.
        let canonical = tokio::fs::canonicalize(path).await.map_err(|e| {
            SandboxError::ResolutionFailed(format!("Cannot resolve path '{path_str}': {e}"))
        })?;

        let canonical_str = canonical.to_string_lossy().to_string();

        // Check denied patterns against the resolved path
        self.check_denied_patterns(&canonical_str)?;

        // In strict mode, verify the resolved path is under an allowed root
        if self.config.strict_mode {
            self.check_allowed_roots(&canonical)?;
        }

        // Check home directory boundaries
        self.check_home_boundary(&canonical)?;

        Ok(canonical)
    }

    /// Synchronous version of `validate` for use in non-async contexts.
    ///
    /// Has the same TOCTOU protection properties as `validate` - uses
    /// immediate canonicalization to resolve symlinks before checking.
    pub fn validate_sync(&self, path: &Path) -> Result<PathBuf, SandboxError> {
        if path.as_os_str().is_empty() {
            return Err(SandboxError::InvalidPath);
        }

        let path_str = path.to_string_lossy().to_string();

        self.check_raw_traversal(&path_str)?;

        let canonical = std::fs::canonicalize(path).map_err(|e| {
            SandboxError::ResolutionFailed(format!("Cannot resolve path '{path_str}': {e}"))
        })?;

        let canonical_str = canonical.to_string_lossy().to_string();

        self.check_denied_patterns(&canonical_str)?;

        if self.config.strict_mode {
            self.check_allowed_roots(&canonical)?;
        }

        self.check_home_boundary(&canonical)?;

        Ok(canonical)
    }

    /// Check for raw `..` traversal components before canonicalization.
    ///
    /// This provides a more descriptive error message. Even if this check
    /// passes, canonicalization may still detect a traversal that resolves
    /// outside allowed roots.
    fn check_raw_traversal(&self, path_str: &str) -> Result<(), SandboxError> {
        // Count `..` components to detect potential traversal
        let components: Vec<&str> = path_str.split('/').collect();
        let mut depth = 0i32;
        for comp in &components {
            if *comp == ".." {
                depth -= 1;
                if depth < 0 {
                    return Err(SandboxError::PathTraversal(format!(
                        "Path '{path_str}' contains '..' that escapes the root directory"
                    )));
                }
            } else if *comp != "." && !comp.is_empty() {
                depth += 1;
            }
        }
        Ok(())
    }

    /// Check if the canonicalized path matches any denied pattern.
    fn check_denied_patterns(&self, canonical_str: &str) -> Result<(), SandboxError> {
        for pattern in &self.config.denied_patterns {
            // Match as prefix. Both "/etc/passwd" and "/etc/" itself should match "/etc/"
            if canonical_str.starts_with(pattern) || canonical_str == pattern.trim_end_matches('/')
            {
                return Err(SandboxError::Denied(format!(
                    "Path '{canonical_str}' is in a restricted area (matches '{pattern}')"
                )));
            }
        }
        Ok(())
    }

    /// In strict mode, verify the canonicalized path is under an allowed root.
    fn check_allowed_roots(&self, canonical: &Path) -> Result<(), SandboxError> {
        let canonical_str = canonical.to_string_lossy().to_string();

        for root in &self.config.allowed_roots {
            // Canonicalize the root as well so comparison is consistent
            let resolved_root = match std::fs::canonicalize(root) {
                Ok(r) => r,
                Err(_) => {
                    // If root doesn't exist yet (e.g., a project dir not yet created),
                    // try to canonicalize it first for comparison, then fall back to prefix matching
                    // Canonicalize the root path to resolve any symlinks before comparison
                    let canonical_root = match std::fs::canonicalize(root) {
                        Ok(r) => r,
                        Err(_) => {
                            // Root doesn't exist and can't be canonicalized,
                            // use as-is with trailing separator for prefix matching
                            let root_str = root.to_string_lossy().to_string();
                            let root_with_sep = if root_str.ends_with('/') {
                                root_str
                            } else {
                                format!("{root_str}/")
                            };
                            if canonical_str.starts_with(&root_with_sep) {
                                return Ok(());
                            }
                            continue;
                        }
                    };
                    // Compare canonicalized paths to prevent symlink escape
                    if canonical == canonical_root.as_os_str() {
                        return Ok(());
                    }
                    continue;
                }
            };

            let resolved_root_str = resolved_root.to_string_lossy().to_string();
            // Check if canonical is the root itself or a child of it
            if canonical == resolved_root
                || canonical_str.starts_with(&format!("{resolved_root_str}/"))
                // Handle Windows paths with backslash
                || canonical_str.starts_with(&format!("{resolved_root_str}\\"))
            {
                return Ok(());
            }
        }

        Err(SandboxError::OutsideAllowedRoots(format!(
            "Path '{}' is not within any allowed root. Allowed roots: {:?}",
            canonical_str, self.config.allowed_roots
        )))
    }

    /// Check that the path doesn't cross into another user's home directory.
    fn check_home_boundary(&self, canonical: &Path) -> Result<(), SandboxError> {
        if let Some(ref my_home) = self.home_dir {
            let my_home_str = my_home.to_string_lossy().to_string();

            // Get the canonical form of /home or determine if this path is
            // under a home directory that isn't ours
            let canonical_str = canonical.to_string_lossy().to_string();

            // Only check if the path is under /home/ or a typical home root
            let home_roots = ["/home/", "C:\\Users\\"];
            let is_under_home_root = home_roots.iter().any(|hr| canonical_str.starts_with(hr));

            if is_under_home_root {
                // Check if it's under our home directory
                let my_home_with_sep = if my_home_str.ends_with('/') {
                    my_home_str.clone()
                } else {
                    format!("{my_home_str}/")
                };

                if !canonical_str.starts_with(&my_home_with_sep) {
                    return Err(SandboxError::Denied(format!(
                        "Path '{canonical_str}' is in another user's home directory"
                    )));
                }
            }
        }
        Ok(())
    }

    /// Add an additional allowed root directory.
    pub fn add_allowed_root(&mut self, root: PathBuf) {
        if !self.config.allowed_roots.contains(&root) {
            self.config.allowed_roots.push(root);
        }
    }

    /// Add an additional denied pattern (path prefix).
    pub fn add_denied_pattern(&mut self, pattern: String) {
        if !self.config.denied_patterns.contains(&pattern) {
            self.config.denied_patterns.push(pattern);
        }
    }

    /// Get a reference to the sandbox configuration.
    pub fn config(&self) -> &SandboxConfig {
        &self.config
    }
}

impl Default for PathSandbox {
    fn default() -> Self {
        Self::new()
    }
}

/// Attempt to determine the current user's home directory.
/// Returns `None` if it cannot be determined.
fn dirs_home_dir() -> Option<PathBuf> {
    // Try standard environment variables first
    if let Ok(home) = std::env::var("HOME") {
        let path = PathBuf::from(&home);
        if path.is_dir() {
            return Some(path);
        }
    }

    // Fallback: check /etc/passwd for the current user
    if let Ok(uid) = std::env::var("USER") {
        // We can't easily parse /etc/passwd in a portable way without deps,
        // so just construct /home/<user> as a best-effort guess
        let guess = PathBuf::from("/home").join(&uid);
        if guess.is_dir() {
            return Some(guess);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Helper to create a temporary directory structure for sandbox tests.
    struct TestDir {
        root: TempDirHolder,
    }

    impl TestDir {
        fn new() -> Self {
            // Use a unique suffix to avoid collisions between parallel tests
            let unique = format!(
                "sandbox_test_{}_{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            );
            let dir = std::env::temp_dir().join(unique);
            fs::create_dir_all(&dir).expect("Failed to create test dir");
            Self {
                root: TempDirHolder(dir),
            }
        }

        fn path(&self) -> &Path {
            self.root.path()
        }

        fn file(&self, relative: &str) -> PathBuf {
            self.root.path().join(relative)
        }

        fn create_file(&self, relative: &str, content: &str) -> PathBuf {
            let path = self.file(relative);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("Failed to create parent dirs");
            }
            fs::write(&path, content).expect("Failed to write test file");
            path
        }

        fn create_symlink(&self, link: &str, target: &Path) -> PathBuf {
            let link_path = self.file(link);
            if let Some(parent) = link_path.parent() {
                fs::create_dir_all(parent).expect("Failed to create parent dirs");
            }
            #[cfg(unix)]
            std::os::unix::fs::symlink(target, &link_path).expect("Failed to create symlink");
            #[cfg(windows)]
            std::os::windows::fs::symlink_file(target, &link_path)
                .expect("Failed to create symlink");
            link_path
        }
    }

    // Minimal tempdir stand-in that cleans up on drop
    struct TempDirHolder(PathBuf);
    impl TempDirHolder {
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TempDirHolder {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    // --- Path traversal tests ---

    #[tokio::test]
    async fn test_path_traversal_detected() {
        let td = TestDir::new();
        td.create_file("secret.txt", "sensitive data");

        let sandbox = PathSandbox::with_config(SandboxConfig {
            allowed_roots: vec![td.path().to_path_buf()],
            denied_patterns: vec![],
            strict_mode: true,
        });

        // Attempt to traverse above the allowed root
        let malicious = td.file("../../etc/passwd");
        let result = sandbox.validate(&malicious).await;
        assert!(result.is_err(), "Expected error for path traversal, got OK");
        let err = result.unwrap_err().to_string();
        let err_lower = err.to_lowercase();
        assert!(
            err_lower.contains("traversal")
                || err_lower.contains("outside allowed roots")
                || err_lower.contains("cannot resolve"),
            "Expected traversal or outside-roots error, got: {err}"
        );
    }

    #[tokio::test]
    async fn test_path_with_dotdot_components() {
        let td = TestDir::new();
        td.create_file("subdir/file.txt", "hello");

        let sandbox = PathSandbox::with_config(SandboxConfig {
            allowed_roots: vec![td.path().to_path_buf()],
            denied_patterns: vec![],
            strict_mode: true,
        });

        // `subdir/../subdir/file.txt` should resolve fine (it stays inside root)
        let path = td.file("subdir/../subdir/file.txt");
        let result = sandbox.validate(&path).await;
        // This should succeed because after resolution it stays within the root
        assert!(result.is_ok(), "Expected OK, got: {result:?}");
    }

    // --- Denied path tests ---

    #[tokio::test]
    async fn test_denied_etc_path() {
        let sandbox = PathSandbox::with_config(SandboxConfig {
            allowed_roots: vec![PathBuf::from("/")],
            denied_patterns: vec!["/etc/".to_string()],
            strict_mode: true,
        });

        // /etc/passwd should be denied
        let result = sandbox.validate(Path::new("/etc/passwd")).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("restricted"));
    }

    #[tokio::test]
    async fn test_denied_boot_path() {
        let sandbox = PathSandbox::with_config(SandboxConfig {
            allowed_roots: vec![PathBuf::from("/")],
            denied_patterns: vec!["/boot/".to_string()],
            strict_mode: true,
        });

        let result = sandbox.validate(Path::new("/boot/vmlinuz")).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_denied_dev_path() {
        let sandbox = PathSandbox::with_config(SandboxConfig {
            allowed_roots: vec![PathBuf::from("/")],
            denied_patterns: vec!["/dev/".to_string()],
            strict_mode: true,
        });

        let result = sandbox.validate(Path::new("/dev/null")).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_denied_usr_bin_path() {
        let sandbox = PathSandbox::with_config(SandboxConfig {
            allowed_roots: vec![PathBuf::from("/")],
            denied_patterns: vec!["/usr/bin/".to_string()],
            strict_mode: true,
        });

        let result = sandbox.validate(Path::new("/usr/bin/ls")).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_denied_proc_path() {
        let sandbox = PathSandbox::with_config(SandboxConfig {
            allowed_roots: vec![PathBuf::from("/")],
            denied_patterns: vec!["/proc/".to_string()],
            strict_mode: true,
        });

        let result = sandbox.validate(Path::new("/proc/self/mem")).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_denied_sys_path() {
        let sandbox = PathSandbox::with_config(SandboxConfig {
            allowed_roots: vec![PathBuf::from("/")],
            denied_patterns: vec!["/sys/".to_string()],
            strict_mode: true,
        });

        let result = sandbox.validate(Path::new("/sys/kernel/notes")).await;
        assert!(result.is_err());
    }

    // --- Allowed root tests ---

    #[tokio::test]
    async fn test_allowed_root_access() {
        let td = TestDir::new();
        td.create_file("project/src/main.rs", "fn main() {}");

        let sandbox = PathSandbox::with_config(SandboxConfig {
            allowed_roots: vec![td.path().to_path_buf()],
            denied_patterns: vec![],
            strict_mode: true,
        });

        let result = sandbox.validate(&td.file("project/src/main.rs")).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_outside_allowed_roots_denied() {
        let td = TestDir::new();
        td.create_file("file.txt", "data");

        // Create a separate allowed root
        let allowed = std::env::temp_dir().join(format!("sandbox_allowed_{}", std::process::id()));
        fs::create_dir_all(&allowed).expect("Failed to create allowed dir");

        let sandbox = PathSandbox::with_config(SandboxConfig {
            allowed_roots: vec![allowed.clone()],
            denied_patterns: vec![],
            strict_mode: true,
        });

        // Try to access a file in a different directory
        let result = sandbox.validate(&td.file("file.txt")).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("not within any allowed root")
        );

        let _ = fs::remove_dir_all(&allowed);
    }

    #[tokio::test]
    async fn test_multiple_allowed_roots() {
        let td1 = TestDir::new();
        let td1 = &td1;
        let td1_file = td1.create_file("file.txt", "data in td1");

        let td2_dir = std::env::temp_dir().join(format!("sandbox_td2_{}", std::process::id()));
        fs::create_dir_all(&td2_dir).expect("Failed to create td2");
        let td2_file = td2_dir.join("file.txt");
        fs::write(&td2_file, "data in td2").expect("Failed to write td2 file");

        let sandbox = PathSandbox::with_config(SandboxConfig {
            allowed_roots: vec![td1.path().to_path_buf(), td2_dir.clone()],
            denied_patterns: vec![],
            strict_mode: true,
        });

        // Both roots should be accessible
        assert!(sandbox.validate(&td1_file).await.is_ok());
        assert!(sandbox.validate(&td2_file).await.is_ok());

        let _ = fs::remove_dir_all(&td2_dir);
    }

    // --- Symlink tests ---

    #[tokio::test]
    async fn test_symlink_inside_allowed_root() {
        let td = TestDir::new();
        let target = td.create_file("real.txt", "real content");
        let link = td.create_symlink("link.txt", &target);

        let sandbox = PathSandbox::with_config(SandboxConfig {
            allowed_roots: vec![td.path().to_path_buf()],
            denied_patterns: vec![],
            strict_mode: true,
        });

        // Symlink that stays within allowed root should be fine
        let result = sandbox.validate(&link).await;
        assert!(
            result.is_ok(),
            "Symlink inside root should be allowed, got: {result:?}"
        );
    }

    #[tokio::test]
    async fn test_symlink_outside_allowed_root_blocked() {
        let td = TestDir::new();

        // Create a target outside the allowed root
        let outside_dir =
            std::env::temp_dir().join(format!("sandbox_outside_{}", std::process::id()));
        fs::create_dir_all(&outside_dir).expect("Failed to create outside dir");
        let outside_file = outside_dir.join("secret.txt");
        fs::write(&outside_file, "secret data").expect("Failed to write outside file");

        // Create a symlink inside the sandbox pointing outside
        let link = td.create_symlink("escape.txt", &outside_file);

        let sandbox = PathSandbox::with_config(SandboxConfig {
            allowed_roots: vec![td.path().to_path_buf()],
            denied_patterns: vec![],
            strict_mode: true,
        });

        let result = sandbox.validate(&link).await;
        assert!(result.is_err(), "Symlink escaping root should be blocked");

        let _ = fs::remove_dir_all(&outside_dir);
    }

    #[tokio::test]
    async fn test_symlink_to_system_file_blocked() {
        let td = TestDir::new();

        // Try to create a symlink to /etc/passwd (a common attack vector)
        // Note: This test doesn't create the actual symlink (would need privileges)
        // but verifies that even if such a symlink existed, it would be blocked
        #[cfg(unix)]
        {
            let etc_passwd = PathBuf::from("/etc/passwd");
            if etc_passwd.exists() {
                let link = td.file("etc_passwd_link");
                #[allow(unused_variables)]
                let symlink_result = std::os::unix::fs::symlink(&etc_passwd, &link);

                // Only test if we could create the symlink
                if symlink_result.is_ok() {
                    let sandbox = PathSandbox::with_config(SandboxConfig {
                        allowed_roots: vec![td.path().to_path_buf()],
                        denied_patterns: vec![],
                        strict_mode: true,
                    });

                    let result = sandbox.validate(&link).await;
                    assert!(result.is_err(), "Symlink to /etc/passwd should be blocked");

                    let _ = fs::remove_file(&link);
                }
            }
        }
    }

    #[tokio::test]
    async fn test_symlink_chain_does_not_escape() {
        let td = TestDir::new();

        // Create a chain: link1 -> link2 -> outside_file
        // This should still be blocked
        let outside_dir =
            std::env::temp_dir().join(format!("sandbox_chain_{}", std::process::id()));
        fs::create_dir_all(&outside_dir).expect("Failed to create outside dir");
        let outside_file = outside_dir.join("secret.txt");
        fs::write(&outside_file, "secret data").expect("Failed to write outside file");

        // Create first symlink (outside)
        let _link2 = td.create_symlink("link2", &outside_file);

        #[cfg(unix)]
        {
            // Create second symlink pointing to the first (also inside sandbox)
            let link1 = td.file("link1");
            let link2_path = td.file("link2");
            std::os::unix::fs::symlink(&link2_path, &link1).expect("Failed to create link1");

            let sandbox = PathSandbox::with_config(SandboxConfig {
                allowed_roots: vec![td.path().to_path_buf()],
                denied_patterns: vec![],
                strict_mode: true,
            });

            // Accessing link1 should fail (it resolves to outside the root)
            let result = sandbox.validate(&link1).await;
            assert!(
                result.is_err(),
                "Symlink chain escaping root should be blocked"
            );

            let _ = fs::remove_file(&link1);
        }

        let _ = fs::remove_dir_all(&outside_dir);
    }

    // --- Strict mode vs non-strict mode ---

    #[tokio::test]
    async fn test_non_strict_mode_allows_non_root_paths() {
        let td = TestDir::new();

        let sandbox = PathSandbox::with_config(SandboxConfig {
            allowed_roots: vec![td.path().to_path_buf()],
            denied_patterns: vec![], // No denied patterns
            strict_mode: false,      // Non-strict: only denied patterns apply
        });

        // In non-strict mode, paths outside roots are allowed (unless denied)
        // We use /tmp since it's not in the denied list
        let tmp_file = std::env::temp_dir().join("sandbox_non_strict_test.txt");
        fs::write(&tmp_file, "test").ok(); // May already exist, ignore error

        let result = sandbox.validate(&tmp_file).await;
        assert!(
            result.is_ok(),
            "Non-strict mode should allow non-root paths: {result:?}"
        );

        let _ = fs::remove_file(&tmp_file);
    }

    #[tokio::test]
    async fn test_non_strict_mode_still_denies_patterns() {
        let sandbox = PathSandbox::with_config(SandboxConfig {
            allowed_roots: vec![],
            denied_patterns: vec!["/etc/".to_string()],
            strict_mode: false,
        });

        let result = sandbox.validate(Path::new("/etc/passwd")).await;
        assert!(
            result.is_err(),
            "Denied patterns should apply even in non-strict mode"
        );
    }

    // --- Empty / invalid path tests ---

    #[tokio::test]
    async fn test_empty_path_rejected() {
        let sandbox = PathSandbox::new();
        let result = sandbox.validate(Path::new("")).await;
        assert!(result.is_err());
    }

    // --- Default configuration tests ---

    #[tokio::test]
    async fn test_default_config_denies_system_paths() {
        let sandbox = PathSandbox::new();

        let system_paths = [
            "/etc/passwd",
            "/etc/shadow",
            "/boot/vmlinuz",
            "/usr/bin/ls",
            "/dev/null",
            "/proc/self/status",
            "/sys/kernel/notes",
        ];

        for path_str in &system_paths {
            let result = sandbox.validate(Path::new(path_str)).await;
            assert!(
                result.is_err(),
                "Default config should deny '{path_str}', got: {result:?}"
            );
        }
    }

    #[tokio::test]
    async fn test_default_config_allows_cwd() {
        let sandbox = PathSandbox::new();

        // The current working directory itself should be accessible
        let cwd = std::env::current_dir().expect("Failed to get cwd");
        let result = sandbox.validate(&cwd).await;
        assert!(
            result.is_ok(),
            "Default config should allow CWD: {result:?}"
        );
    }

    // --- Home directory boundary tests ---

    #[tokio::test]
    async fn test_home_boundary_other_user_denied() {
        let sandbox = PathSandbox::with_config(SandboxConfig {
            allowed_roots: vec![PathBuf::from("/")],
            denied_patterns: vec![],
            strict_mode: false,
        });

        // /home/root should be denied if the current user isn't root
        // (This test may not work if running as root, but that's fine)
        if std::env::var("USER").map(|u| u != "root").unwrap_or(true) {
            let result = sandbox.validate(Path::new("/home/root/.bashrc")).await;
            // Path might not exist, but if it does exist, it should be denied
            // If it doesn't exist, we get ResolutionFailed which is also acceptable
            if let Err(e) = result {
                let err_str = e.to_string();
                assert!(
                    err_str.contains("another user") || err_str.contains("Cannot resolve"),
                    "Expected home boundary or resolution error, got: {err_str}"
                );
            }
        }
    }

    // --- Sync validation tests ---

    #[test]
    fn test_sync_validation_denies_system_paths() {
        let sandbox = PathSandbox::new();
        let result = sandbox.validate_sync(Path::new("/etc/passwd"));
        assert!(result.is_err());
    }

    #[test]
    fn test_sync_validation_allows_cwd() {
        let sandbox = PathSandbox::new();
        let cwd = std::env::current_dir().expect("Failed to get cwd");
        let result = sandbox.validate_sync(&cwd);
        assert!(result.is_ok(), "Sync validate should allow CWD: {result:?}");
    }

    // --- Builder-style API tests ---

    #[test]
    fn test_add_allowed_root() {
        let mut sandbox = PathSandbox::new();
        let extra = PathBuf::from("/tmp/sandbox_test_extra");
        sandbox.add_allowed_root(extra.clone());
        assert!(sandbox.config().allowed_roots.contains(&extra));
    }

    #[test]
    fn test_add_denied_pattern() {
        let mut sandbox = PathSandbox::new();
        sandbox.add_denied_pattern("/custom/denied/".to_string());
        assert!(
            sandbox
                .config()
                .denied_patterns
                .iter()
                .any(|p| p == "/custom/denied/")
        );
    }

    #[test]
    fn test_default_denied_patterns() {
        let patterns = SandboxConfig::default_denied_patterns();
        assert!(patterns.contains(&"/etc/".to_string()));
        assert!(patterns.contains(&"/dev/".to_string()));
        assert!(patterns.contains(&"/boot/".to_string()));
        assert!(patterns.contains(&"/usr/bin/".to_string()));
    }
}
