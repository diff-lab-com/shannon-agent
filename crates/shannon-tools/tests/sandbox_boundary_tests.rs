//! Sandbox boundary enforcement tests
//!
//! Tests that verify the sandbox correctly enforces boundaries across
//! multiple attack vectors:
//! - Write operations blocked outside workspace root
//! - Symlink escapes prevented
//! - Parent directory traversal via ../ blocked
//! - Absolute paths outside workspace blocked
//! - Environment variable injection in paths
//! - Command chaining/shell injection in REPL tool
//! - Shell special characters rejected
//! - Resource limits enforced
//! - Concurrent violations all caught
//! - Nested subdirectory operations stay within bounds

use serde_json::json;
use shannon_tools::{
    Tool, WriteTool,
    file::sandbox::{PathSandbox, SandboxConfig, SandboxError},
    repl_tool::ReplTool,
    system::analyze_command_security,
};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

// ============================================================================
// Helpers
// ============================================================================

/// Create a temporary workspace directory with sample files.
fn setup_workspace() -> TempDir {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("main.rs"), "fn main() {}").unwrap();
    fs::write(dir.path().join("README.md"), "# Test Project").unwrap();
    dir
}

/// Build a strict sandbox scoped to the given workspace root.
fn strict_sandbox(root: &Path) -> PathSandbox {
    PathSandbox::with_config(SandboxConfig {
        allowed_roots: vec![root.to_path_buf()],
        denied_patterns: SandboxConfig::default_denied_patterns(),
        strict_mode: true,
    })
}

/// Execute a command through ReplTool and return the result.
async fn exec_repl(command: &str) -> Result<String, String> {
    let tool = ReplTool::new();
    let input = json!({ "command": command });
    tool.execute(input)
        .await
        .map(|output| output.content)
        .map_err(|e| e.to_string())
}

/// Execute a write through WriteTool with a sandbox scoped to root.
async fn exec_write(root: &Path, file_path: &str, content: &str) -> Result<String, String> {
    let tool = WriteTool::with_sandbox(strict_sandbox(root));
    let input = json!({
        "file_path": file_path,
        "content": content,
    });
    tool.execute(input)
        .await
        .map(|output| output.content)
        .map_err(|e| e.to_string())
}

// ============================================================================
// Test 1: Write blocked outside workspace
// ============================================================================

#[tokio::test]
async fn test_write_blocked_outside_workspace() {
    let workspace = setup_workspace();
    let outside = TempDir::new().unwrap();
    let outside_file = outside.path().join("should_not_write.txt");

    let result = exec_write(
        workspace.path(),
        &outside_file.to_string_lossy(),
        "malicious content",
    )
    .await;

    assert!(
        result.is_err(),
        "Writing outside workspace root should be blocked"
    );
    let err = result.unwrap_err();
    assert!(
        err.contains("sandbox") || err.contains("not within") || err.contains("allowed root"),
        "Error should mention sandbox boundary: {err}"
    );

    // Verify the file was NOT created
    assert!(
        !outside_file.exists(),
        "File should not have been created outside workspace"
    );
}

// ============================================================================
// Test 2: Symlink escape prevented
// ============================================================================

#[tokio::test]
async fn test_symlink_escape_prevented() {
    let workspace = setup_workspace();
    let outside = TempDir::new().unwrap();
    let outside_target = outside.path().join("secret_data.txt");
    fs::write(&outside_target, "top secret").unwrap();

    // Create a symlink inside the workspace that points outside
    let symlink_path = workspace.path().join("escape_link.txt");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside_target, &symlink_path).unwrap();

    let sandbox = strict_sandbox(workspace.path());
    let result = sandbox.validate(&symlink_path).await;

    assert!(
        result.is_err(),
        "Symlink escaping workspace root should be blocked"
    );
    match result.unwrap_err() {
        SandboxError::OutsideAllowedRoots(_) | SandboxError::SymlinkEscape { .. } => {}
        other => panic!("Expected OutsideAllowedRoots or SymlinkEscape, got: {other:?}"),
    }
}

// ============================================================================
// Test 3: Parent directory traversal
// ============================================================================

#[tokio::test]
async fn test_parent_directory_traversal() {
    let workspace = setup_workspace();
    let sandbox = strict_sandbox(workspace.path());

    let traversal_attempts = vec![
        workspace.path().join("../../etc/passwd"),
        workspace.path().join("src/../../../etc/shadow"),
        workspace.path().join("./../../../boot/vmlinuz"),
        workspace.path().join("src/main.rs/../../../../dev/null"),
    ];

    for attempt in traversal_attempts {
        let result = sandbox.validate(&attempt).await;
        assert!(
            result.is_err(),
            "Traversal attempt should be blocked: {attempt:?}"
        );
    }
}

// ============================================================================
// Test 4: Absolute path outside workspace
// ============================================================================

#[tokio::test]
async fn test_absolute_path_outside_workspace() {
    let workspace = setup_workspace();
    let sandbox = strict_sandbox(workspace.path());

    let absolute_paths = vec![
        PathBuf::from("/etc/passwd"),
        PathBuf::from("/etc/shadow"),
        PathBuf::from("/boot/vmlinuz"),
        PathBuf::from("/usr/bin/ls"),
        PathBuf::from("/dev/null"),
        PathBuf::from("/proc/self/mem"),
        PathBuf::from("/sys/kernel/notes"),
        PathBuf::from("/var/log/syslog"),
    ];

    for path in absolute_paths {
        let result = sandbox.validate(&path).await;
        assert!(
            result.is_err(),
            "Absolute path outside workspace should be blocked: {path:?}"
        );
    }
}

// ============================================================================
// Test 5: Environment variable injection in paths
// ============================================================================

#[tokio::test]
async fn test_env_var_injection() {
    let workspace = setup_workspace();
    let sandbox = strict_sandbox(workspace.path());

    // Paths containing shell variable syntax should not resolve to
    // locations outside the workspace. Since these literal strings
    // won't exist on disk, canonicalization fails.
    let injected_paths = vec![
        PathBuf::from("$HOME/secret.txt"),
        PathBuf::from("${HOME}/.ssh/id_rsa"),
        PathBuf::from("${IFS}malicious"),
        PathBuf::from("$RANDOM/../etc/passwd"),
    ];

    for path in injected_paths {
        let result = sandbox.validate(&path).await;
        assert!(
            result.is_err(),
            "Path with env var injection should be blocked: {path:?}"
        );
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("resolve") || err.contains("Cannot resolve") || err.contains("traversal"),
            "Error should indicate resolution failure: {err}"
        );
    }
}

// ============================================================================
// Test 6: Command chaining injection
// ============================================================================

#[tokio::test]
async fn test_command_chaining_injection() {
    let chain_attempts = vec![
        ("echo hello; cat /etc/passwd", "semicolon"),
        ("ls | grep secret", "pipe"),
        ("cd /tmp && rm -rf important", "double ampersand"),
        ("false || echo fallback", "double pipe"),
    ];

    for (command, label) in chain_attempts {
        let result = exec_repl(command).await;
        assert!(
            result.is_err(),
            "Command chaining via {label} should be rejected: {command}"
        );
        let err = result.unwrap_err();
        assert!(
            err.contains("command chaining")
                || err.contains("pipe")
                || err.contains(";")
                || err.contains("|")
                || err.contains("&&"),
            "Error should mention the injection vector ({label}): {err}"
        );
    }
}

// ============================================================================
// Test 7: Shell special characters
// ============================================================================

#[tokio::test]
async fn test_shell_special_characters() {
    let special_char_commands = vec![
        ("echo $(whoami)", "dollar-paren substitution"),
        ("echo `whoami`", "backtick substitution"),
        ("echo data > file.txt", "output redirect"),
        ("echo data >> file.txt", "append redirect"),
        ("wc -l < file.txt", "input redirect"),
    ];

    for (command, label) in special_char_commands {
        let result = exec_repl(command).await;
        assert!(
            result.is_err(),
            "Shell special character ({label}) should be rejected: {command}"
        );
        let err = result.unwrap_err();
        assert!(
            err.contains("substitution")
                || err.contains("redirection")
                || err.contains("$(")
                || err.contains("`")
                || err.contains(">"),
            "Error should mention the specific issue ({label}): {err}"
        );
    }
}

// ============================================================================
// Test 8: Resource limit enforcement
// ============================================================================

#[tokio::test]
async fn test_resource_limit_enforcement() {
    // Verify that resource limits exist and are enforced:

    // 8a: Write tool has a 10 MB max write size
    // We test this through the low-level write::execute which checks size
    // before filesystem access (the sandbox validates the path first, but
    // for non-existent files canonicalization fails). So we use the raw
    // write module directly to verify the size guard.
    let large_content = "x".repeat(11 * 1024 * 1024); // 11 MB
    let write_input = shannon_tools::file::write::WriteInput {
        file_path: "/tmp/sandbox_boundary_size_test.txt".to_string(),
        content: large_content,
    };
    let result = shannon_tools::file::write::execute(write_input).await;
    assert!(
        result.is_err(),
        "Write exceeding size limit should be rejected"
    );
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("too large") || err.contains("max"),
        "Error should mention size limit: {err}"
    );

    // 8b: Command security analysis provides risk levels
    let critical_cmd = analyze_command_security("rm -rf /");
    assert!(
        critical_cmd.is_destructive,
        "Destructive commands should be flagged"
    );
    assert!(
        critical_cmd.risk_level >= shannon_tools::system::SecurityLevel::Critical,
        "rm -rf / should be Critical risk level"
    );

    // 8c: Default denied patterns cover critical system directories
    let denied = SandboxConfig::default_denied_patterns();
    for critical in &["/etc/", "/boot/", "/dev/", "/proc/", "/sys/"] {
        assert!(
            denied.contains(&critical.to_string()),
            "Default denied patterns should include {critical}"
        );
    }
}

// ============================================================================
// Test 9: Concurrent sandbox violations
// ============================================================================

#[tokio::test]
async fn test_concurrent_sandbox_violations() {
    let workspace = setup_workspace();
    let sandbox = strict_sandbox(workspace.path());
    let sandbox = std::sync::Arc::new(sandbox);

    // Attempt multiple violations concurrently
    let mut handles = vec![];

    // Spawn tasks that each attempt a different violation
    for i in 0..8 {
        let sb = sandbox.clone();
        handles.push(tokio::spawn(async move {
            match i % 4 {
                0 => sb.validate(Path::new("/etc/passwd")).await,
                1 => sb.validate(Path::new("../../etc/shadow")).await,
                2 => sb.validate(Path::new("/dev/null")).await,
                3 => sb.validate(Path::new("/proc/self/mem")).await,
                _ => unreachable!(),
            }
        }));
    }

    // Every single violation should be caught
    for (i, handle) in handles.into_iter().enumerate() {
        let result = handle.await.unwrap();
        assert!(
            result.is_err(),
            "Concurrent violation #{i} should be blocked"
        );
    }
}

// ============================================================================
// Test 10: Nested workspace boundary
// ============================================================================

#[tokio::test]
async fn test_nested_workspace_boundary() {
    let workspace = setup_workspace();

    // Create nested subdirectories
    let deep_dir = workspace.path().join("src").join("module").join("sub");
    fs::create_dir_all(&deep_dir).unwrap();
    let deep_file = deep_dir.join("deep.rs");
    fs::write(&deep_file, "// deep file").unwrap();

    let sandbox = strict_sandbox(workspace.path());

    // All nested paths within workspace should be allowed
    let allowed_paths = vec![
        workspace.path().join("src/main.rs"),
        workspace.path().join("src/module/sub/deep.rs"),
        workspace.path().join("README.md"),
    ];

    for path in &allowed_paths {
        let result = sandbox.validate(path).await;
        assert!(
            result.is_ok(),
            "Nested file within workspace should be allowed: {path:?}"
        );
    }

    // Paths that use .. to navigate within the workspace but stay in bounds
    let in_bounds_traversal = workspace.path().join("src/../README.md");
    let result = sandbox.validate(&in_bounds_traversal).await;
    assert!(
        result.is_ok(),
        "Traversal that stays within workspace should be allowed: {in_bounds_traversal:?}"
    );

    // But navigation that escapes should still be blocked
    let escaping = workspace.path().join("src/../../../etc/passwd");
    let result = sandbox.validate(&escaping).await;
    assert!(
        result.is_err(),
        "Traversal escaping workspace should be blocked: {escaping:?}"
    );
}
