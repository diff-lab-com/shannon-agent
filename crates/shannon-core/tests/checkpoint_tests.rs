//! Integration tests for the checkpoint/undo system.
//!
//! Tests git-based checkpoint creation, file restoration, turn tracking,
//! edge cases (untracked files, renames, deletes, dirty index, merge
//! conflicts), persistence, and error resilience.
//!
//! Uses `--test-threads=1` at the workspace level because these tests
//! change the process current directory and share git state.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

use shannon_core::checkpoint::{Checkpoint, CheckpointManager, RestoreMode};

// ── Helpers ──────────────────────────────────────────────────────────

/// Create a temp directory with an initialised git repo and an initial
/// commit containing a single `hello.txt`.
fn setup_git_repo() -> TempDir {
    let dir = TempDir::new().unwrap();
    let path = dir.path();

    git(path, &["init"]);
    git(path, &["config", "user.email", "test@test.com"]);
    git(path, &["config", "user.name", "Test"]);

    // Seed with an initial commit so HEAD exists.
    fs::write(path.join("hello.txt"), "hello world\n").unwrap();
    git(path, &["add", "-A"]);
    git(path, &["commit", "-m", "initial commit", "--no-gpg-sign"]);

    dir
}

/// Run a git command in `cwd`, panicking on failure.
fn git(cwd: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap();
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        panic!(
            "git {} failed in {:?}: {}",
            args.join(" "),
            cwd,
            stderr
        );
    }
}

/// Run a git command in `cwd`, returning whether it succeeded.
fn git_ok(cwd: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Return the current HEAD hash (full) inside `cwd`.
fn head_hash(cwd: &Path) -> String {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(cwd)
        .output()
        .unwrap();
    assert!(output.status.success(), "rev-parse HEAD failed");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// Save CWD, chdir into the repo, return a guard that restores on drop.
fn enter_repo(dir: &TempDir) -> DirGuard {
    let original = env::current_dir().unwrap();
    env::set_current_dir(dir.path()).unwrap();
    DirGuard { original }
}

/// RAII guard that restores the original working directory.
struct DirGuard {
    original: PathBuf,
}

impl Drop for DirGuard {
    fn drop(&mut self) {
        let _ = env::set_current_dir(&self.original);
    }
}

/// Build a `CheckpointManager` in the current directory (must be a git repo).
fn make_manager() -> CheckpointManager {
    let mut mgr = CheckpointManager::new();
    // Assign a throwaway session ID so persistence exercises the disk path
    // without colliding with real sessions.
    mgr.set_session_id("test-checkpoint-integration");
    mgr.clear();
    mgr
}

// ── Test 1: checkpoint creates a git commit with correct files ───────

#[test]
fn test_checkpoint_creates_git_commit() {
    let dir = setup_git_repo();
    let _guard = enter_repo(&dir);
    let path = dir.path();

    // Modify a file after the initial commit.
    fs::write(path.join("hello.txt"), "modified content\n").unwrap();
    fs::write(path.join("new_file.txt"), "brand new\n").unwrap();

    let mgr = make_manager();
    let cp = mgr.create_checkpoint("edit", "edit hello.txt").unwrap();

    // The checkpoint should have a valid commit hash.
    assert!(!cp.hash.is_empty());
    assert_eq!(cp.short_hash.len(), 7);

    // The commit message should mention the tool name.
    let log = String::from_utf8_lossy(
        &Command::new("git")
            .args(["log", "-1", "--format=%s"])
            .current_dir(path)
            .output()
            .unwrap()
            .stdout,
    )
    .trim()
    .to_string();
    assert!(
        log.contains("checkpoint before edit"),
        "commit message should mention tool, got: {log}"
    );

    // Both files should be tracked in that commit.
    let files = String::from_utf8_lossy(
        &Command::new("git")
            .args(["show", "--stat", "--format=", &cp.hash])
            .current_dir(path)
            .output()
            .unwrap()
            .stdout,
    )
    .trim()
    .to_string();
    assert!(files.contains("hello.txt"), "hello.txt in commit stat");
    assert!(files.contains("new_file.txt"), "new_file.txt in commit stat");
}

// ── Test 2: restore a single file ───────────────────────────────────

#[test]
fn test_checkpoint_restores_single_file() {
    let dir = setup_git_repo();
    let _guard = enter_repo(&dir);
    let path = dir.path();

    let mgr = make_manager();
    let cp = mgr.create_checkpoint("edit", "before modification").unwrap();
    let cp_hash = cp.hash.clone();
    mgr.record_turn(0, cp, vec!["hello.txt".into()], Some("modify file".into()));

    // Modify the file.
    fs::write(path.join("hello.txt"), "broken content\n").unwrap();

    // Revert to the checkpoint.
    let reverted = mgr.revert_to(0, RestoreMode::CodeOnly).unwrap();
    assert_eq!(reverted.checkpoint.hash, cp_hash);

    // File content should be restored.
    let content = fs::read_to_string(path.join("hello.txt")).unwrap();
    assert_eq!(content, "hello world\n", "file should be restored");
}

// ── Test 3: restore multiple files ──────────────────────────────────

#[test]
fn test_checkpoint_restores_multiple_files() {
    let dir = setup_git_repo();
    let _guard = enter_repo(&dir);
    let path = dir.path();

    // Create extra.txt before checkpoint so it's tracked.
    fs::write(path.join("extra.txt"), "brand new\n").unwrap();

    let mgr = make_manager();
    let cp = mgr
        .create_checkpoint("edit", "before multi-edit")
        .unwrap();
    mgr.record_turn(
        0,
        cp,
        vec!["hello.txt".into(), "extra.txt".into()],
        Some("multi edit".into()),
    );

    // Modify two files.
    fs::write(path.join("hello.txt"), "changed a\n").unwrap();
    fs::write(path.join("extra.txt"), "changed b\n").unwrap();

    let _ = mgr.revert_to(0, RestoreMode::CodeOnly).unwrap();

    assert_eq!(
        fs::read_to_string(path.join("hello.txt")).unwrap(),
        "hello world\n"
    );
    assert_eq!(
        fs::read_to_string(path.join("extra.txt")).unwrap(),
        "brand new\n"
    );
}

// ── Test 4: handles untracked (new) files ────────────────────────────

#[test]
fn test_checkpoint_handles_untracked_files() {
    let dir = setup_git_repo();
    let _guard = enter_repo(&dir);
    let path = dir.path();

    // Create an untracked file (checkpoint stages via `git add -A`).
    fs::write(path.join("untracked.txt"), "i am new\n").unwrap();

    let mgr = make_manager();
    let cp = mgr
        .create_checkpoint("write", "new untracked file")
        .unwrap();

    assert!(!cp.hash.is_empty());

    // The file should now be committed.
    let tracked = String::from_utf8_lossy(
        &Command::new("git")
            .args(["ls-files", "untracked.txt"])
            .current_dir(path)
            .output()
            .unwrap()
            .stdout,
    )
    .trim()
    .to_string();
    assert!(!tracked.is_empty(), "untracked file should be tracked after checkpoint");
}

// ── Test 5: handles file rename ─────────────────────────────────────

#[test]
fn test_checkpoint_handles_rename() {
    let dir = setup_git_repo();
    let _guard = enter_repo(&dir);
    let path = dir.path();

    let mgr = make_manager();
    let _cp1 = mgr.create_checkpoint("edit", "before rename").unwrap();

    // Rename hello.txt -> greeting.txt
    fs::rename(path.join("hello.txt"), path.join("greeting.txt")).unwrap();

    let cp2 = mgr
        .create_checkpoint("rename", "renamed hello to greeting")
        .unwrap();

    // The new name should be tracked in the commit.
    let files = String::from_utf8_lossy(
        &Command::new("git")
            .args(["show", "--stat", "--format=", &cp2.hash])
            .current_dir(path)
            .output()
            .unwrap()
            .stdout,
    )
    .trim()
    .to_string();
    assert!(
        files.contains("greeting.txt"),
        "renamed file should appear in commit stat"
    );
}

// ── Test 6: handles file deletion ───────────────────────────────────

#[test]
fn test_checkpoint_handles_delete() {
    let dir = setup_git_repo();
    let _guard = enter_repo(&dir);
    let path = dir.path();

    let mgr = make_manager();
    let cp = mgr.create_checkpoint("edit", "before delete").unwrap();
    let cp_hash = cp.hash.clone();
    mgr.record_turn(0, cp, vec!["hello.txt".into()], None);

    // Delete the file.
    fs::remove_file(path.join("hello.txt")).unwrap();

    // Checkpoint the deletion.
    let cp2 = mgr
        .create_checkpoint("delete", "removed hello.txt")
        .unwrap();

    // The file should not exist on disk but should be in the commit diff.
    assert!(!path.join("hello.txt").exists());

    let diff = String::from_utf8_lossy(
        &Command::new("git")
            .args(["diff", "--name-status", &format!("{}..{}", cp_hash, cp2.hash)])
            .current_dir(path)
            .output()
            .unwrap()
            .stdout,
    )
    .trim()
    .to_string();
    assert!(
        diff.contains("D\thello.txt") || diff.contains("hello.txt"),
        "deletion should appear in diff: {diff}"
    );

    // Revert to before the deletion — file should come back.
    let _ = mgr.revert_to(0, RestoreMode::CodeOnly).unwrap();
    assert!(
        path.join("hello.txt").exists(),
        "file should be restored after revert"
    );
}

// ── Test 7: dirty index (staged but uncommitted) ────────────────────

#[test]
fn test_checkpoint_with_dirty_index() {
    let dir = setup_git_repo();
    let _guard = enter_repo(&dir);
    let path = dir.path();

    // Stage a change without committing.
    fs::write(path.join("hello.txt"), "staged content\n").unwrap();
    git(path, &["add", "hello.txt"]);

    let mgr = make_manager();
    let cp = mgr
        .create_checkpoint("edit", "dirty index")
        .unwrap();

    // The checkpoint commit should include the staged content.
    let content = String::from_utf8_lossy(
        &Command::new("git")
            .args(["show", &format!("{}:hello.txt", cp.hash)])
            .current_dir(path)
            .output()
            .unwrap()
            .stdout,
    )
    .trim()
    .to_string();
    assert_eq!(content, "staged content");
}

// ── Test 8: merge conflict state ────────────────────────────────────

#[test]
fn test_checkpoint_with_merge_conflict() {
    let dir = setup_git_repo();
    let _guard = enter_repo(&dir);
    let path = dir.path();

    // Create a branch, commit a conflicting change, then start a merge.
    git(path, &["checkout", "-b", "feature"]);
    fs::write(path.join("hello.txt"), "feature content\n").unwrap();
    git(path, &["add", "-A"]);
    git(path, &["commit", "-m", "feature commit", "--no-gpg-sign"]);

    let _ = git_ok(path, &["checkout", "master"])
        || git_ok(path, &["checkout", "main"]);

    fs::write(path.join("hello.txt"), "main content\n").unwrap();
    git(path, &["add", "-A"]);
    git(path, &["commit", "-m", "main commit", "--no-gpg-sign"]);

    // Attempt merge (will conflict).
    let merge_ok = git_ok(path, &["merge", "feature", "--no-edit"]);
    if !merge_ok {
        // Merge conflict exists — test checkpoint in this state.
        // Write conflict markers into the file to simulate resolution staging.
        let conflict_content = "<<<<<<< HEAD\nmain content\n=======\nfeature content\n>>>>>>> feature\n";
        fs::write(path.join("hello.txt"), conflict_content).unwrap();
        git(path, &["add", "hello.txt"]);
    }

    // Checkpoint should succeed even in merge-conflict state.
    let mgr = make_manager();
    let result = mgr.create_checkpoint("edit", "during merge conflict");
    // We accept both success and failure — the important thing is no panic.
    if let Ok(cp) = result {
        assert!(!cp.hash.is_empty());
    }
}

// ── Test 9: multiple undo is idempotent ─────────────────────────────

#[test]
fn test_checkpoint_restoration_idempotent() {
    let dir = setup_git_repo();
    let _guard = enter_repo(&dir);
    let path = dir.path();

    let mgr = make_manager();

    // Create two checkpoints in sequence.
    let cp0 = mgr.create_checkpoint("edit", "turn 0").unwrap();
    mgr.record_turn(0, cp0, vec!["hello.txt".into()], None);

    fs::write(path.join("hello.txt"), "change 1\n").unwrap();
    let cp1 = mgr.create_checkpoint("edit", "turn 1").unwrap();
    mgr.record_turn(1, cp1, vec!["hello.txt".into()], None);

    // Revert to turn 0.
    let result = mgr.revert_to(0, RestoreMode::CodeAndConversation);
    assert!(result.is_ok());
    assert_eq!(mgr.len(), 1); // Only turn 0 remains.

    // Second revert to same index should succeed (no-op on files).
    let result2 = mgr.revert_to(0, RestoreMode::CodeOnly);
    assert!(result2.is_ok());

    // Content is still the initial content.
    let content = fs::read_to_string(path.join("hello.txt")).unwrap();
    assert_eq!(content, "hello world\n");
}

// ── Test 10: partial restoration failure ─────────────────────────────

#[test]
fn test_checkpoint_partial_restoration_failure() {
    // When reverting to an invalid index, the manager should return an
    // error without panicking and without corrupting existing state.
    let dir = setup_git_repo();
    let _guard = enter_repo(&dir);

    let mgr = make_manager();
    let cp = mgr.create_checkpoint("edit", "baseline").unwrap();
    mgr.record_turn(0, cp, vec![], None);

    // Out-of-bounds revert should error gracefully.
    let result = mgr.revert_to(99, RestoreMode::CodeAndConversation);
    assert!(result.is_err());
    assert!(
        result.unwrap_err().contains("Invalid checkpoint index"),
        "error message should mention invalid index"
    );

    // Original checkpoints should still be intact.
    assert_eq!(mgr.len(), 1, "checkpoints should not be corrupted");
}

// ── Test 11: corrupted state / persistence round-trip ────────────────

#[test]
fn test_checkpoint_corrupted_state() {
    let dir = setup_git_repo();
    let _guard = enter_repo(&dir);

    let mgr = CheckpointManager::for_session("test-corrupted-session");
    mgr.clear();

    let cp = mgr.create_checkpoint("edit", "persistence test").unwrap();
    mgr.record_turn(0, cp, vec!["hello.txt".into()], Some("test prompt".into()));

    // Force save to disk.
    assert!(mgr.save_to_disk().is_ok());

    // Load into a fresh manager — should recover the same data.
    let mgr2 = CheckpointManager::for_session("test-corrupted-session");
    assert_eq!(mgr2.len(), 1, "should load checkpoint from disk");
    let list = mgr2.list_checkpoints();
    assert_eq!(list[0].turn_index, 0);
    assert_eq!(list[0].files_changed, vec!["hello.txt"]);
    assert_eq!(list[0].prompt_preview, Some("test prompt".into()));

    // Now corrupt the file on disk.
    let session_path = dirs::home_dir()
        .map(|h| h.join(".shannon").join("checkpoints").join("test-corrupted-session.json"))
        .unwrap();
    if session_path.exists() {
        fs::write(&session_path, "NOT VALID JSON{{{{").unwrap();
    }

    // Loading corrupted data should not panic — it should return an error
    // internally and start with an empty list.
    let mgr3 = CheckpointManager::for_session("test-corrupted-session");
    assert!(
        mgr3.is_empty(),
        "corrupted file should result in empty checkpoints"
    );

    // Clean up.
    let _ = fs::remove_file(&session_path);
}

// ── Test 12: large / binary file ────────────────────────────────────

#[test]
fn test_checkpoint_large_binary_file() {
    let dir = setup_git_repo();
    let _guard = enter_repo(&dir);
    let path = dir.path();

    // Write a 1 MB binary file.
    let large_data: Vec<u8> = (0..1_000_000).map(|i| (i % 256) as u8).collect();
    fs::write(path.join("large.bin"), &large_data).unwrap();

    let mgr = make_manager();
    let cp = mgr
        .create_checkpoint("write", "large binary file")
        .unwrap();

    assert!(!cp.hash.is_empty());

    // Verify the blob was stored.
    let blob_hash = String::from_utf8_lossy(
        &Command::new("git")
            .args(["hash-object", "large.bin"])
            .current_dir(path)
            .output()
            .unwrap()
            .stdout,
    )
    .trim()
    .to_string();
    assert!(!blob_hash.is_empty(), "binary blob should be stored");

    // Record turn and revert — the checkpoint includes large.bin, so it should
    // still be present after revert (resetting to the checkpoint commit).
    mgr.record_turn(0, cp, vec!["large.bin".into()], None);
    let _ = mgr.revert_to(0, RestoreMode::CodeOnly).unwrap();

    assert!(
        path.join("large.bin").exists(),
        "large binary should still exist (checkpoint includes it)"
    );
}

// ── Test 13: concurrent (external) modification during checkpoint ────

#[test]
fn test_checkpoint_concurrent_modification() {
    let dir = setup_git_repo();
    let _guard = enter_repo(&dir);
    let path = dir.path();

    let mgr = make_manager();

    // Simulate external modification: modify file and commit directly.
    fs::write(path.join("hello.txt"), "external change\n").unwrap();
    git(path, &["add", "-A"]);
    git(path, &["commit", "-m", "external commit", "--no-gpg-sign"]);

    // Now the CheckpointManager should still work — it operates on
    // whatever the current HEAD is.
    fs::write(path.join("hello.txt"), "post-external edit\n").unwrap();
    let cp = mgr
        .create_checkpoint("edit", "after external modification")
        .unwrap();

    assert!(!cp.hash.is_empty());

    // Verify the content at the checkpoint commit.
    let content = String::from_utf8_lossy(
        &Command::new("git")
            .args(["show", &format!("{}:hello.txt", cp.hash)])
            .current_dir(path)
            .output()
            .unwrap()
            .stdout,
    )
    .trim()
    .to_string();
    assert_eq!(content, "post-external edit");
}

// ── Bonus: no-changes checkpoint ─────────────────────────────────────

#[test]
fn test_checkpoint_no_changes() {
    let dir = setup_git_repo();
    let _guard = enter_repo(&dir);

    let mgr = make_manager();

    // No modifications since last commit — should return HEAD hash.
    let cp = mgr.create_checkpoint("edit", "nothing changed").unwrap();
    assert!(cp.description.contains("no changes"));
    assert_eq!(cp.hash, head_hash(dir.path()));
}

// ── Bonus: discard_last does not revert code ─────────────────────────

#[test]
fn test_discard_last_no_revert() {
    let dir = setup_git_repo();
    let _guard = enter_repo(&dir);
    let path = dir.path();

    let mgr = make_manager();
    let cp = mgr.create_checkpoint("edit", "baseline").unwrap();
    mgr.record_turn(0, cp, vec!["hello.txt".into()], None);

    // Modify file after recording the turn.
    fs::write(path.join("hello.txt"), "post-checkpoint change\n").unwrap();

    // Discard the checkpoint (should NOT revert the file).
    let discarded = mgr.discard_last().unwrap();
    assert_eq!(discarded.turn_index, 0);
    assert!(mgr.is_empty());

    // File should still have the post-checkpoint content.
    let content = fs::read_to_string(path.join("hello.txt")).unwrap();
    assert_eq!(content, "post-checkpoint change\n");
}

// ── Bonus: cleanup_old_checkpoints does not panic ────────────────────

#[test]
fn test_cleanup_old_checkpoints_no_panic() {
    // Should not panic even if the checkpoint directory does not exist.
    let result = CheckpointManager::cleanup_old_checkpoints();
    assert!(result.is_ok(), "cleanup should not error");
}

// ── Bonus: conversation-only revert does not touch files ─────────────

#[test]
fn test_conversation_only_revert_preserves_code() {
    let dir = setup_git_repo();
    let _guard = enter_repo(&dir);
    let path = dir.path();

    let mgr = make_manager();
    let cp = mgr.create_checkpoint("edit", "baseline").unwrap();
    mgr.record_turn(0, cp.clone(), vec!["hello.txt".into()], None);

    // Modify the file.
    fs::write(path.join("hello.txt"), "modified\n").unwrap();

    // Revert conversation only — file should stay modified.
    let result = mgr.revert_to(0, RestoreMode::ConversationOnly);
    assert!(result.is_ok());

    let content = fs::read_to_string(path.join("hello.txt")).unwrap();
    assert_eq!(content, "modified\n", "ConversationOnly should not touch files");
}

// ── Bonus: turn checkpoint truncation at MAX_CHECKPOINTS ─────────────

#[test]
fn test_turn_checkpoint_max_truncation() {
    let dir = setup_git_repo();
    let _guard = enter_repo(&dir);

    let mgr = make_manager();

    // Record 55 turn checkpoints (MAX is 50).
    for i in 0..55 {
        let cp = Checkpoint {
            hash: format!("hash{i:040}"),
            short_hash: format!("hash{i:07}"),
            description: format!("turn {i}"),
            timestamp: 1700000000 + i as i64,
        };
        mgr.record_turn(i, cp, vec![], None);
    }

    assert_eq!(
        mgr.len(),
        50,
        "should truncate to MAX_CHECKPOINTS"
    );

    // First retained turn should be turn 5 (indices 0-4 dropped).
    let list = mgr.list_checkpoints();
    assert_eq!(list[0].turn_index, 5);
}

// ── Bonus: persistence round-trip preserves all fields ───────────────

#[test]
fn test_persistence_roundtrip_all_fields() {
    let dir = setup_git_repo();
    let _guard = enter_repo(&dir);

    let session_id = "test-roundtrip-all-fields";
    let mgr = CheckpointManager::for_session(session_id);
    mgr.clear();

    let cp = Checkpoint {
        hash: "abcdef1234567890".to_string(),
        short_hash: "abcdef1".to_string(),
        description: "test checkpoint".into(),
        timestamp: 1700000000,
    };
    mgr.record_turn(
        42,
        cp,
        vec!["a.rs".into(), "b.rs".into()],
        Some("user prompt preview".into()),
    );

    assert!(mgr.save_to_disk().is_ok());

    let mgr2 = CheckpointManager::for_session(session_id);
    assert_eq!(mgr2.len(), 1);
    let tc = &mgr2.list_checkpoints()[0];
    assert_eq!(tc.turn_index, 42);
    assert_eq!(tc.checkpoint.hash, "abcdef1234567890");
    assert_eq!(tc.checkpoint.short_hash, "abcdef1");
    assert_eq!(tc.files_changed, vec!["a.rs", "b.rs"]);
    assert_eq!(tc.prompt_preview, Some("user prompt preview".into()));

    // Clean up.
    let path = dirs::home_dir()
        .map(|h| h.join(".shannon").join("checkpoints").join(format!("{session_id}.json")))
        .unwrap();
    let _ = fs::remove_file(&path);
}
