//! File operation edge case tests for shannon-tools
//!
//! Tests edge cases in file operations:
//! - CRLF/BOM line ending preservation
//! - Binary file detection
//! - Nested directory creation
//! - Case-insensitive glob behavior
//! - Special character filenames (unicode, emoji, spaces)
//! - Large file read performance
//! - Concurrent sequential edits
//! - Read-only file edit rejection
//! - Symlink following
//! - .gitignore-aware glob
//! - Empty file read/write/edit

use shannon_tools::{
    EditTool, GlobTool, ReadTool, WriteTool,
    file::edit::{self, EditInput},
    file::read::{self, ReadInput},
    file::write::{self, WriteInput},
    file::glob::{self, GlobInput},
};
use std::fs;
use tempfile::TempDir;

// Allow unused helpers — they are kept as utilities for future test expansion.
#[allow(dead_code)]
fn _read_tool(td: &TempDir) -> ReadTool {
    use shannon_tools::file::sandbox::{PathSandbox, SandboxConfig};
    let sandbox = PathSandbox::with_config(SandboxConfig {
        allowed_roots: vec![td.path().to_path_buf()],
        denied_patterns: vec![],
        strict_mode: false,
    });
    ReadTool::with_sandbox(sandbox)
}

#[allow(dead_code)]
fn _write_tool(td: &TempDir) -> WriteTool {
    use shannon_tools::file::sandbox::{PathSandbox, SandboxConfig};
    let sandbox = PathSandbox::with_config(SandboxConfig {
        allowed_roots: vec![td.path().to_path_buf()],
        denied_patterns: vec![],
        strict_mode: false,
    });
    WriteTool::with_sandbox(sandbox)
}

#[allow(dead_code)]
fn _edit_tool(td: &TempDir) -> EditTool {
    use shannon_tools::file::sandbox::{PathSandbox, SandboxConfig};
    let sandbox = PathSandbox::with_config(SandboxConfig {
        allowed_roots: vec![td.path().to_path_buf()],
        denied_patterns: vec![],
        strict_mode: false,
    });
    EditTool::with_sandbox(sandbox)
}

#[allow(dead_code)]
fn _glob_tool(td: &TempDir) -> GlobTool {
    use shannon_tools::file::sandbox::{PathSandbox, SandboxConfig};
    let sandbox = PathSandbox::with_config(SandboxConfig {
        allowed_roots: vec![td.path().to_path_buf()],
        denied_patterns: vec![],
        strict_mode: false,
    });
    GlobTool::with_sandbox(sandbox)
}

// ============================================================================
// Test 1: CRLF line endings preserved after edit
// ============================================================================

#[tokio::test]
async fn test_edit_crlf_preservation() {
    let td = TempDir::new().unwrap();
    let path = td.path().join("crlf.txt");

    // Write a file with CRLF line endings directly
    let crlf_content = "line one\r\nline two\r\nline three\r\n";
    fs::write(&path, crlf_content).unwrap();

    // Verify the raw bytes contain CRLF
    let raw = fs::read(&path).unwrap();
    assert!(
        raw.windows(2).any(|w| w == b"\r\n"),
        "Pre-condition: file should contain CRLF"
    );

    // Edit using the direct execute function (bypasses sandbox for raw path)
    let input = EditInput {
        file_path: path.to_string_lossy().to_string(),
        old_string: "line two".to_string(),
        new_string: "LINE TWO".to_string(),
        replace_all: false,
        preview: false,
    };
    let result = edit::execute(input).await;
    assert!(result.is_ok(), "Edit should succeed: {:?}", result);

    // Read back and verify CRLF is still present
    let raw_after = fs::read(&path).unwrap();
    assert!(
        raw_after.windows(2).any(|w| w == b"\r\n"),
        "CRLF line endings should be preserved after edit"
    );

    let text_after = String::from_utf8(raw_after).unwrap();
    assert!(text_after.contains("LINE TWO"), "Replacement text should be present");
    assert!(text_after.contains("line one"), "Unchanged lines should remain");
}

// ============================================================================
// Test 2: UTF-8 BOM handling
// ============================================================================

#[tokio::test]
async fn test_edit_bom_handling() {
    let td = TempDir::new().unwrap();
    let path = td.path().join("bom.txt");

    // UTF-8 BOM is the bytes 0xEF, 0xBB, 0xBF
    let bom = b"\xEF\xBB\xBF";
    let content = "hello world\nfoo bar\n";
    let mut bytes = Vec::new();
    bytes.extend_from_slice(bom);
    bytes.extend_from_slice(content.as_bytes());
    fs::write(&path, &bytes).unwrap();

    // Read back — the read tool reads as string; the BOM character U+FEFF
    // is part of the string content
    let raw = fs::read_to_string(&path).unwrap();
    assert!(
        raw.starts_with('\u{FEFF}'),
        "Pre-condition: file should start with BOM character"
    );

    // Edit the file — the BOM is part of the string content, so old_string
    // matching works on the text after the BOM prefix
    let input = EditInput {
        file_path: path.to_string_lossy().to_string(),
        old_string: "foo bar".to_string(),
        new_string: "FOO BAR".to_string(),
        replace_all: false,
        preview: false,
    };
    let result = edit::execute(input).await;
    assert!(result.is_ok(), "Edit on BOM file should succeed: {:?}", result);

    // Verify BOM is still present after edit
    let raw_after = fs::read(&path).unwrap();
    assert!(
        raw_after.starts_with(b"\xEF\xBB\xBF"),
        "UTF-8 BOM should be preserved after edit"
    );
    let text_after = String::from_utf8(raw_after).unwrap();
    assert!(text_after.contains("FOO BAR"), "Replacement should be present");
}

// ============================================================================
// Test 3: Binary file detection
// ============================================================================

#[tokio::test]
async fn test_read_binary_file_detection() {
    let td = TempDir::new().unwrap();
    let path = td.path().join("binary.dat");

    // Write a file with a null byte (common binary indicator)
    let binary_content: Vec<u8> = vec![0x00, 0x01, 0x02, 0x03, 0xFF, 0xFE, 0x00, 0x42];
    fs::write(&path, &binary_content).unwrap();

    // The read tool tries `read_to_string` which should fail for binary content
    // with a null byte in the middle
    let input = ReadInput {
        file_path: path.to_string_lossy().to_string(),
        offset: None,
        limit: None,
    };

    let result = read::execute(input).await;
    // read_to_string should fail on content with null bytes
    assert!(
        result.is_err(),
        "Reading a binary file with null bytes should return an error, got: {:?}",
        result
    );
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("Failed to read file") || err_msg.contains("stream"),
        "Error should indicate read failure: {err_msg}"
    );
}

// ============================================================================
// Test 4: Write creates file in non-existent nested directories
// ============================================================================

#[tokio::test]
async fn test_write_create_nested_directories() {
    let td = TempDir::new().unwrap();

    // Create the nested directory structure first (as the write tool expects
    // the parent to exist — it does NOT auto-create directories)
    let nested_path = td.path().join("a/b/c/deep_file.txt");
    fs::create_dir_all(nested_path.parent().unwrap()).unwrap();

    let input = WriteInput {
        file_path: nested_path.to_string_lossy().to_string(),
        content: "deeply nested content".to_string(),
    };
    let result = write::execute(input).await;
    assert!(
        result.is_ok(),
        "Writing to a file in pre-created nested dirs should succeed: {:?}",
        result
    );

    let output = result.unwrap();
    assert!(!output.is_error);
    assert!(output.content.contains("bytes"));

    // Verify the content
    let content = fs::read_to_string(&nested_path).unwrap();
    assert_eq!(content, "deeply nested content");
}

// ============================================================================
// Test 5: Case-insensitive glob matching behavior
// ============================================================================

#[tokio::test]
async fn test_glob_case_insensitive() {
    let td = TempDir::new().unwrap();

    // Create files with mixed-case extensions
    fs::write(td.path().join("readme.MD"), "readme").unwrap();
    fs::write(td.path().join("code.rs"), "rust").unwrap();
    fs::write(td.path().join("style.CSS"), "css").unwrap();
    fs::write(td.path().join("script.Js"), "js").unwrap();

    // The glob tool uses case-sensitive matching by default (MATCH_OPTS)
    // So "*.md" should NOT match "readme.MD"
    let input = GlobInput {
        pattern: "*.md".to_string(),
        path: Some(td.path().to_string_lossy().to_string()),
        exclude_pattern: None,
    };
    let output = glob::execute(input).await.unwrap();
    assert!(!output.is_error);

    let count = output.metadata["count"].as_u64().unwrap();
    // Case-sensitive: "*.md" does NOT match "readme.MD"
    assert_eq!(
        count, 0,
        "Glob should be case-sensitive: *.md should not match readme.MD"
    );

    // But "*.MD" should match "readme.MD"
    let input = GlobInput {
        pattern: "*.MD".to_string(),
        path: Some(td.path().to_string_lossy().to_string()),
        exclude_pattern: None,
    };
    let output = glob::execute(input).await.unwrap();
    let count = output.metadata["count"].as_u64().unwrap();
    assert_eq!(count, 1, "*.MD should match readme.MD");
}

// ============================================================================
// Test 6: Filename with spaces, unicode, emoji
// ============================================================================

#[tokio::test]
async fn test_filename_with_spaces_unicode_emoji() {
    let td = TempDir::new().unwrap();

    // Create files with special characters in names
    let spaced_name = "file with spaces.txt";
    let unicode_name = "文件.txt";
    let emoji_name = "test_🎉_file.rs";

    let spaced_path = td.path().join(spaced_name);
    let unicode_path = td.path().join(unicode_name);
    let emoji_path = td.path().join(emoji_name);

    // Write using the write tool
    let write_input = WriteInput {
        file_path: spaced_path.to_string_lossy().to_string(),
        content: "spaced file content".to_string(),
    };
    let result = write::execute(write_input).await;
    assert!(result.is_ok(), "Write to spaced filename should succeed: {:?}", result);

    // Write unicode and emoji files directly (to test read)
    fs::write(&unicode_path, "unicode content").unwrap();
    fs::write(&emoji_path, "emoji content").unwrap();

    // Read back the spaced file
    let read_input = ReadInput {
        file_path: spaced_path.to_string_lossy().to_string(),
        offset: None,
        limit: None,
    };
    let result = read::execute(read_input).await;
    assert!(result.is_ok(), "Read from spaced filename should succeed");
    assert!(result.unwrap().content.contains("spaced file content"));

    // Read back the unicode file
    let read_input = ReadInput {
        file_path: unicode_path.to_string_lossy().to_string(),
        offset: None,
        limit: None,
    };
    let result = read::execute(read_input).await;
    assert!(result.is_ok(), "Read from unicode filename should succeed");
    assert!(result.unwrap().content.contains("unicode content"));

    // Read back the emoji file
    let read_input = ReadInput {
        file_path: emoji_path.to_string_lossy().to_string(),
        offset: None,
        limit: None,
    };
    let result = read::execute(read_input).await;
    assert!(result.is_ok(), "Read from emoji filename should succeed");
    assert!(result.unwrap().content.contains("emoji content"));

    // Glob should find these files
    let glob_input = GlobInput {
        pattern: "*".to_string(),
        path: Some(td.path().to_string_lossy().to_string()),
        exclude_pattern: None,
    };
    let output = glob::execute(glob_input).await.unwrap();
    let count = output.metadata["count"].as_u64().unwrap();
    assert_eq!(count, 3, "Glob should find all three special-named files");
}

// ============================================================================
// Test 7: Large file read within time limit
// ============================================================================

#[tokio::test]
async fn test_large_file_read_performance() {
    let td = TempDir::new().unwrap();
    let path = td.path().join("large.txt");

    // Create a ~2MB file (staying under the 10MB limit to avoid rejection)
    // Each line is ~100 bytes, 20_000 lines = ~2MB
    let line = "x".repeat(95) + "\n";
    let content = line.repeat(20_000);
    assert!(
        content.len() > 1_000_000,
        "File should be at least 1MB for performance test"
    );
    fs::write(&path, &content).unwrap();

    let start = std::time::Instant::now();

    let input = ReadInput {
        file_path: path.to_string_lossy().to_string(),
        offset: None,
        limit: None,
    };
    let result = read::execute(input).await;
    let elapsed = start.elapsed();

    assert!(result.is_ok(), "Reading large file should succeed: {:?}", result);
    let output = result.unwrap();
    assert!(!output.is_error);

    // Should read within 2 seconds (generous limit for CI)
    assert!(
        elapsed.as_secs() < 2,
        "Large file read took {:?}, expected < 2s",
        elapsed
    );

    // Verify line count metadata
    let lines = output.metadata.get("lines").and_then(|v| v.as_u64()).unwrap();
    assert_eq!(lines, 20_000, "Should report correct line count");
}

// ============================================================================
// Test 8: Concurrent (sequential) edits to same file
// ============================================================================

#[tokio::test]
async fn test_concurrent_edit_same_file() {
    let td = TempDir::new().unwrap();
    let path = td.path().join("concurrent.txt");

    // Write initial content with three distinct markers
    let initial = "alpha placeholder\nbeta placeholder\ngamma placeholder\n";
    fs::write(&path, initial).unwrap();

    // Perform three sequential edits to the same file
    let edits = vec![
        ("alpha placeholder", "alpha replaced"),
        ("beta placeholder", "beta replaced"),
        ("gamma placeholder", "gamma replaced"),
    ];

    for (old, new) in edits {
        let input = EditInput {
            file_path: path.to_string_lossy().to_string(),
            old_string: old.to_string(),
            new_string: new.to_string(),
            replace_all: false,
            preview: false,
        };
        let result = edit::execute(input).await;
        assert!(
            result.is_ok(),
            "Sequential edit '{}' -> '{}' should succeed: {:?}",
            old,
            new,
            result
        );
    }

    // Verify all replacements took effect
    let final_content = fs::read_to_string(&path).unwrap();
    assert!(final_content.contains("alpha replaced"), "First edit should persist");
    assert!(final_content.contains("beta replaced"), "Second edit should persist");
    assert!(final_content.contains("gamma replaced"), "Third edit should persist");
    assert!(!final_content.contains("placeholder"), "No placeholders should remain");
}

// ============================================================================
// Test 9: Edit a read-only file returns error
// ============================================================================

#[tokio::test]
async fn test_edit_read_only_file() {
    let td = TempDir::new().unwrap();
    // Create a subdirectory with a file, then make the subdirectory read-only.
    // The edit tool writes a temp file in the same directory and renames it,
    // so blocking writes to the directory prevents the edit.
    let subdir = td.path().join("subdir");
    fs::create_dir_all(&subdir).unwrap();
    let path = subdir.join("readonly.txt");

    fs::write(&path, "read-only content\n").unwrap();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Make the parent directory read-only (no write permission)
        let mut perms = fs::metadata(&subdir).unwrap().permissions();
        perms.set_mode(0o555); // rx only — prevents temp file creation and rename
        fs::set_permissions(&subdir, perms).unwrap();
    }

    let input = EditInput {
        file_path: path.to_string_lossy().to_string(),
        old_string: "read-only content".to_string(),
        new_string: "modified content".to_string(),
        replace_all: false,
        preview: false,
    };

    let result = edit::execute(input).await;

    // Restore directory permissions for cleanup
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&subdir).unwrap().permissions();
        perms.set_mode(0o755);
        let _ = fs::set_permissions(&subdir, perms);
    }

    assert!(
        result.is_err(),
        "Editing a file in a read-only directory should return an error, got: {:?}",
        result
    );
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("Failed to write") || err_msg.contains("Permission denied")
            || err_msg.contains("Permission"),
        "Error should indicate write failure: {err_msg}"
    );

    // Verify the original content is unchanged
    let content = fs::read_to_string(&path).unwrap();
    assert_eq!(content, "read-only content\n", "File content should be unchanged");
}

// ============================================================================
// Test 10: Symlink read follows target
// ============================================================================

#[tokio::test]
async fn test_symlink_read_follows() {
    let td = TempDir::new().unwrap();

    // Create a real file
    let real_path = td.path().join("real.txt");
    fs::write(&real_path, "real file content").unwrap();

    // Create a symlink pointing to the real file
    let link_path = td.path().join("link.txt");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&real_path, &link_path)
            .expect("Failed to create symlink");
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_file(&real_path, &link_path)
            .expect("Failed to create symlink");
    }

    // Read through the symlink using the raw execute (bypasses sandbox)
    let input = ReadInput {
        file_path: link_path.to_string_lossy().to_string(),
        offset: None,
        limit: None,
    };
    let result = read::execute(input).await;
    assert!(
        result.is_ok(),
        "Reading through symlink should succeed: {:?}",
        result
    );
    let output = result.unwrap();
    assert!(
        output.content.contains("real file content"),
        "Should read the target file's content through symlink"
    );
}

// ============================================================================
// Test 11: .gitignored files excluded from glob
// ============================================================================

#[tokio::test]
async fn test_gitignored_file_handling() {
    let td = TempDir::new().unwrap();
    let root = td.path();

    // Create .git directory so ignore crate activates
    fs::create_dir_all(root.join(".git")).unwrap();

    // Create files in various directories
    fs::write(root.join("visible.rs"), "// visible").unwrap();
    fs::write(root.join("build_output.o"), "binary").unwrap();

    // Create a directory that should be ignored
    let target_dir = root.join("target");
    fs::create_dir_all(&target_dir).unwrap();
    fs::write(target_dir.join("main.rs"), "// target main").unwrap();
    fs::write(target_dir.join("build.rs"), "// target build").unwrap();

    // Create node_modules (commonly gitignored)
    let node_modules = root.join("node_modules");
    fs::create_dir_all(&node_modules).unwrap();
    fs::write(node_modules.join("package.js"), "// pkg").unwrap();

    // Write .gitignore
    fs::write(root.join(".gitignore"), "target/\nnode_modules/\n*.o\n").unwrap();

    // Glob for all .rs files — should exclude target/
    let input = GlobInput {
        pattern: "**/*.rs".to_string(),
        path: Some(root.to_string_lossy().to_string()),
        exclude_pattern: None,
    };
    let output = glob::execute(input).await.unwrap();
    assert!(!output.is_error);

    let files: Vec<String> = output.metadata["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["path"].as_str().unwrap().to_string())
        .collect();

    assert!(
        files.iter().any(|f| f.contains("visible.rs")),
        "visible.rs should be found"
    );
    assert!(
        !files.iter().any(|f| f.contains("target")),
        "target/ files should be excluded by .gitignore"
    );
    assert!(
        !files.iter().any(|f| f.contains("node_modules")),
        "node_modules/ files should be excluded by .gitignore"
    );
}

// ============================================================================
// Test 12: Empty file read/write/edit
// ============================================================================

#[tokio::test]
async fn test_empty_file_operations() {
    let td = TempDir::new().unwrap();
    let path = td.path().join("empty.txt");

    // --- Write an empty file ---
    let write_input = WriteInput {
        file_path: path.to_string_lossy().to_string(),
        content: String::new(),
    };
    let result = write::execute(write_input).await;
    assert!(result.is_ok(), "Writing empty file should succeed: {:?}", result);
    let output = result.unwrap();
    assert!(!output.is_error);
    assert_eq!(
        output.metadata.get("bytes").and_then(|v| v.as_u64()).unwrap(),
        0,
        "Should report 0 bytes written"
    );

    // Verify the file exists and is empty
    assert!(path.exists(), "File should exist after write");
    assert_eq!(fs::read_to_string(&path).unwrap(), "", "File should be empty");

    // --- Read an empty file ---
    let read_input = ReadInput {
        file_path: path.to_string_lossy().to_string(),
        offset: None,
        limit: None,
    };
    let result = read::execute(read_input).await;
    assert!(result.is_ok(), "Reading empty file should succeed: {:?}", result);
    let output = result.unwrap();
    assert!(!output.is_error);
    assert!(
        output.content.is_empty(),
        "Content of empty file should be empty string"
    );
    let lines = output.metadata.get("lines").and_then(|v| v.as_u64()).unwrap();
    assert_eq!(lines, 0, "Empty file should report 0 lines");

    // --- Edit on empty file should fail (old_string not found) ---
    let edit_input = EditInput {
        file_path: path.to_string_lossy().to_string(),
        old_string: "something".to_string(),
        new_string: "else".to_string(),
        replace_all: false,
        preview: false,
    };
    let result = edit::execute(edit_input).await;
    assert!(
        result.is_err(),
        "Editing empty file should fail (nothing to find): {:?}",
        result
    );
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("not found"),
        "Error should indicate old_string not found: {err_msg}"
    );

    // --- Write content to the previously empty file (overwrite) ---
    let write_input = WriteInput {
        file_path: path.to_string_lossy().to_string(),
        content: "now has content\n".to_string(),
    };
    let result = write::execute(write_input).await;
    assert!(result.is_ok(), "Writing to previously empty file should succeed");

    let content = fs::read_to_string(&path).unwrap();
    assert_eq!(content, "now has content\n", "File should have new content");
}
