//! Path sandbox integration tests
//!
//! Tests SandboxConfig, PathSandbox, and SandboxError from file::sandbox
//! through the public API, complementing the inline unit tests and
//! security_tests.rs.

use shannon_tools::file::sandbox::{PathSandbox, SandboxConfig, SandboxError};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

// ============================================================================
// SandboxConfig construction and validation
// ============================================================================

#[test]
fn test_sandbox_config_default_has_cwd_as_allowed_root() {
    let config = SandboxConfig::default();
    let cwd = std::env::current_dir().unwrap();
    assert!(config.allowed_roots.contains(&cwd));
    assert!(config.strict_mode);
}

#[test]
fn test_sandbox_config_default_denied_patterns_comprehensive() {
    let patterns = SandboxConfig::default_denied_patterns();
    // Verify all critical system directories are covered
    assert!(patterns.contains(&"/etc/".to_string()));
    assert!(patterns.contains(&"/boot/".to_string()));
    assert!(patterns.contains(&"/usr/bin/".to_string()));
    assert!(patterns.contains(&"/usr/sbin/".to_string()));
    assert!(patterns.contains(&"/bin/".to_string()));
    assert!(patterns.contains(&"/sbin/".to_string()));
    assert!(patterns.contains(&"/dev/".to_string()));
    assert!(patterns.contains(&"/proc/".to_string()));
    assert!(patterns.contains(&"/sys/".to_string()));
    assert!(patterns.contains(&"/run/".to_string()));
    assert!(patterns.contains(&"/var/log/".to_string()));
    assert!(patterns.contains(&"/var/run/".to_string()));
}

#[test]
fn test_sandbox_config_custom_allowed_roots() {
    let config = SandboxConfig {
        allowed_roots: vec![PathBuf::from("/custom/project")],
        denied_patterns: vec![],
        strict_mode: false,
    };
    assert_eq!(config.allowed_roots.len(), 1);
    assert_eq!(config.allowed_roots[0], PathBuf::from("/custom/project"));
    assert!(!config.strict_mode);
}

// ============================================================================
// Path validation - project dir allowed, outside blocked
// ============================================================================

#[tokio::test]
async fn test_validate_file_within_allowed_root() {
    let td = TempDir::new().unwrap();
    let file_path = td.path().join("src").join("main.rs");
    fs::create_dir_all(file_path.parent().unwrap()).unwrap();
    fs::write(&file_path, "fn main() {}").unwrap();

    let sandbox = PathSandbox::with_config(SandboxConfig {
        allowed_roots: vec![td.path().to_path_buf()],
        denied_patterns: vec![],
        strict_mode: true,
    });

    let result = sandbox.validate(&file_path).await;
    assert!(result.is_ok(), "File within allowed root should be valid: {result:?}");
}

#[tokio::test]
async fn test_validate_path_outside_allowed_root_rejected() {
    let td = TempDir::new().unwrap();
    let other = TempDir::new().unwrap();
    let other_file = other.path().join("secret.txt");
    fs::write(&other_file, "secret").unwrap();

    let sandbox = PathSandbox::with_config(SandboxConfig {
        allowed_roots: vec![td.path().to_path_buf()],
        denied_patterns: vec![],
        strict_mode: true,
    });

    let result = sandbox.validate(&other_file).await;
    assert!(result.is_err());
    match result.unwrap_err() {
        SandboxError::OutsideAllowedRoots(msg) => {
            assert!(msg.contains("not within any allowed root"));
        }
        other => panic!("Expected OutsideAllowedRoots, got: {other:?}"),
    }
}

#[tokio::test]
async fn test_validate_nested_subdirectory_allowed() {
    let td = TempDir::new().unwrap();
    let nested = td.path().join("a").join("b").join("c").join("file.txt");
    fs::create_dir_all(nested.parent().unwrap()).unwrap();
    fs::write(&nested, "deep").unwrap();

    let sandbox = PathSandbox::with_config(SandboxConfig {
        allowed_roots: vec![td.path().to_path_buf()],
        denied_patterns: vec![],
        strict_mode: true,
    });

    let result = sandbox.validate(&nested).await;
    assert!(result.is_ok(), "Deeply nested file should be allowed: {result:?}");
}

// ============================================================================
// SandboxError variants
// ============================================================================

#[test]
fn test_sandbox_error_display_messages() {
    let err = SandboxError::PathTraversal("test traversal".to_string());
    assert!(err.to_string().contains("traversal"));

    let err = SandboxError::Denied("test denied".to_string());
    assert!(err.to_string().contains("restricted"));

    let err = SandboxError::OutsideAllowedRoots("test outside".to_string());
    assert!(err.to_string().contains("allowed roots"));

    let err = SandboxError::SymlinkEscape {
        symlink: "/link".to_string(),
        target: "/target".to_string(),
    };
    assert!(
        err.to_string().to_lowercase().contains("symlink"),
        "Expected 'symlink' in error message, got: {err}"
    );

    let err = SandboxError::ToctouDetected("test toctou".to_string());
    assert!(err.to_string().contains("TOCTOU"));

    let err = SandboxError::ResolutionFailed("test resolution".to_string());
    assert!(err.to_string().contains("resolve"));

    let err = SandboxError::InvalidPath;
    assert!(err.to_string().contains("empty") || err.to_string().contains("invalid"));
}

#[tokio::test]
async fn test_empty_path_returns_invalid_path_error() {
    let sandbox = PathSandbox::new();
    let result = sandbox.validate(Path::new("")).await;
    assert!(result.is_err());
    match result.unwrap_err() {
        SandboxError::InvalidPath => {}
        other => panic!("Expected InvalidPath, got: {other:?}"),
    }
}

#[tokio::test]
async fn test_denied_pattern_exact_match() {
    let sandbox = PathSandbox::with_config(SandboxConfig {
        allowed_roots: vec![PathBuf::from("/")],
        denied_patterns: vec!["/etc/".to_string()],
        strict_mode: true,
    });

    let result = sandbox.validate(Path::new("/etc")).await;
    // /etc itself should match the denied pattern (trimmed trailing /)
    assert!(result.is_err());
}

// ============================================================================
// PathSandbox with allowed/blocked patterns
// ============================================================================

#[test]
fn test_add_allowed_root_deduplicates() {
    let mut sandbox = PathSandbox::new();
    let root = PathBuf::from("/tmp/sandbox_test_dedup");
    sandbox.add_allowed_root(root.clone());
    sandbox.add_allowed_root(root.clone());
    let count = sandbox.config().allowed_roots.iter().filter(|r| **r == root).count();
    assert_eq!(count, 1, "add_allowed_root should deduplicate");
}

#[test]
fn test_add_denied_pattern_deduplicates() {
    let mut sandbox = PathSandbox::new();
    let pattern = "/custom/denied/".to_string();
    sandbox.add_denied_pattern(pattern.clone());
    sandbox.add_denied_pattern(pattern.clone());
    let count = sandbox.config().denied_patterns.iter().filter(|p| *p == &pattern).count();
    assert_eq!(count, 1, "add_denied_pattern should deduplicate");
}

#[tokio::test]
async fn test_non_strict_mode_with_denied_pattern() {
    let _td = TempDir::new().unwrap();
    let sandbox = PathSandbox::with_config(SandboxConfig {
        allowed_roots: vec![],
        denied_patterns: vec!["/etc/".to_string()],
        strict_mode: false,
    });

    // /etc/passwd should be denied even in non-strict mode
    let result = sandbox.validate(Path::new("/etc/passwd")).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_non_strict_mode_allows_undenied_paths() {
    let td = TempDir::new().unwrap();

    let sandbox = PathSandbox::with_config(SandboxConfig {
        allowed_roots: vec![],
        denied_patterns: vec![],
        strict_mode: false,
    });

    // Non-strict mode with no denied patterns should allow any existing path
    let result = sandbox.validate(td.path()).await;
    assert!(result.is_ok(), "Non-strict mode should allow: {result:?}");
}

// ============================================================================
// Edge cases: symlinks, relative paths, parent directory traversal
// ============================================================================

#[tokio::test]
async fn test_symlink_inside_root_resolves_correctly() {
    let td = TempDir::new().unwrap();
    let target = td.path().join("real_file.txt");
    fs::write(&target, "content").unwrap();

    let link = td.path().join("link_to_file.txt");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, &link).unwrap();

    let sandbox = PathSandbox::with_config(SandboxConfig {
        allowed_roots: vec![td.path().to_path_buf()],
        denied_patterns: vec![],
        strict_mode: true,
    });

    let result = sandbox.validate(&link).await;
    assert!(result.is_ok(), "Symlink inside root should resolve fine: {result:?}");
}

#[tokio::test]
async fn test_symlink_escaping_root_is_blocked() {
    let td = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    let outside_file = outside.path().join("escaped.txt");
    fs::write(&outside_file, "escaped content").unwrap();

    let link = td.path().join("escape_link");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside_file, &link).unwrap();

    let sandbox = PathSandbox::with_config(SandboxConfig {
        allowed_roots: vec![td.path().to_path_buf()],
        denied_patterns: vec![],
        strict_mode: true,
    });

    let result = sandbox.validate(&link).await;
    assert!(result.is_err(), "Symlink escaping root should be blocked");
}

#[tokio::test]
async fn test_parent_traversal_with_existing_paths() {
    let td = TempDir::new().unwrap();
    let subdir = td.path().join("subdir");
    fs::create_dir_all(&subdir).unwrap();

    // Path that uses .. but stays within root should be rejected by
    // check_raw_traversal because depth goes negative
    let traversal_path = subdir.join("../../../../etc/passwd");
    let sandbox = PathSandbox::with_config(SandboxConfig {
        allowed_roots: vec![td.path().to_path_buf()],
        denied_patterns: vec![],
        strict_mode: true,
    });

    let result = sandbox.validate(&traversal_path).await;
    assert!(result.is_err(), "Traversal escaping root should be blocked");
}

#[tokio::test]
async fn test_dot_components_within_root_are_allowed() {
    let td = TempDir::new().unwrap();
    let file = td.path().join("file.txt");
    fs::write(&file, "data").unwrap();

    // Path with . components that stays within root
    let dotted = td.path().join("./././file.txt");
    // This may not resolve if canonicalize has issues, but let's test
    let sandbox = PathSandbox::with_config(SandboxConfig {
        allowed_roots: vec![td.path().to_path_buf()],
        denied_patterns: vec![],
        strict_mode: true,
    });

    let result = sandbox.validate(&dotted).await;
    assert!(result.is_ok(), "Path with . components should resolve: {result:?}");
}

#[tokio::test]
async fn test_relative_path_back_and_forth_within_root() {
    let td = TempDir::new().unwrap();
    let subdir = td.path().join("subdir");
    fs::create_dir_all(&subdir).unwrap();
    let file = subdir.join("file.txt");
    fs::write(&file, "data").unwrap();

    // subdir/../subdir/file.txt should resolve back to the same place
    let relative = td.path().join("subdir/../subdir/file.txt");

    let sandbox = PathSandbox::with_config(SandboxConfig {
        allowed_roots: vec![td.path().to_path_buf()],
        denied_patterns: vec![],
        strict_mode: true,
    });

    let result = sandbox.validate(&relative).await;
    assert!(result.is_ok(), "Relative back-and-forth should be allowed: {result:?}");
}

// ============================================================================
// Sync validation tests
// ============================================================================

#[test]
fn test_sync_validate_empty_path_rejected() {
    let sandbox = PathSandbox::new();
    let result = sandbox.validate_sync(Path::new(""));
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), SandboxError::InvalidPath));
}

#[test]
fn test_sync_validate_allowed_root_file() {
    let td = TempDir::new().unwrap();
    let file = td.path().join("test.txt");
    fs::write(&file, "hello").unwrap();

    let sandbox = PathSandbox::with_config(SandboxConfig {
        allowed_roots: vec![td.path().to_path_buf()],
        denied_patterns: vec![],
        strict_mode: true,
    });

    let result = sandbox.validate_sync(&file);
    assert!(result.is_ok(), "Sync validate should allow file in root: {result:?}");
}

#[test]
fn test_sync_validate_denied_path() {
    let sandbox = PathSandbox::with_config(SandboxConfig {
        allowed_roots: vec![PathBuf::from("/")],
        denied_patterns: vec!["/etc/".to_string()],
        strict_mode: true,
    });

    let result = sandbox.validate_sync(Path::new("/etc/passwd"));
    assert!(result.is_err());
}

// ============================================================================
// Default sandbox convenience tests
// ============================================================================

#[test]
fn test_default_sandbox_has_cwd_allowed() {
    let sandbox = PathSandbox::new();
    let cwd = std::env::current_dir().unwrap();
    assert!(sandbox.config().allowed_roots.contains(&cwd));
}

#[test]
fn test_default_sandbox_is_strict() {
    let sandbox = PathSandbox::new();
    assert!(sandbox.config().strict_mode);
}
