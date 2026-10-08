//! 2026-10-08 status bar (设计 02-chat 六段) — workspace facts for the chat
//! status bar's branch segment.
//!
//! One small command instead of a generic "git info" surface: the segment
//! needs exactly one fact (the working dir's current branch), refreshed when
//! the session's working dir changes. Not-a-repo / git-unavailable map to
//! `None` and the frontend hides the segment (honesty rule — same contract
//! as the other data-backed segments in `ChatStatusBar`).

/// Current git branch of `working_dir`. `None` when the directory is not
/// inside a git work tree (or git is unavailable) — the caller hides the
/// segment rather than showing a placeholder.
#[cfg(feature = "tauri")]
#[tauri::command]
pub async fn current_git_branch(working_dir: String) -> Result<Option<String>, String> {
    if working_dir.trim().is_empty() {
        return Ok(None);
    }
    // `git rev-parse` is a fast subprocess, but still a subprocess — keep it
    // off the main thread (spawn_blocking precedent: commands_files).
    tokio::task::spawn_blocking(move || {
        Ok::<Option<String>, String>(shannon_tools::git::current_branch_of(Some(
            working_dir.as_str(),
        )))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(all(test, feature = "tauri"))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn empty_or_blank_dir_yields_none() {
        assert_eq!(current_git_branch(String::new()).await.unwrap(), None);
        assert_eq!(current_git_branch("   ".into()).await.unwrap(), None);
    }

    #[tokio::test]
    async fn non_repo_dir_yields_none() {
        let tmp = std::env::temp_dir().join("shannon-statusbar-nonrepo");
        std::fs::create_dir_all(&tmp).unwrap();
        assert_eq!(
            current_git_branch(tmp.to_string_lossy().to_string())
                .await
                .unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn repo_dir_yields_branch_name() {
        let tmp =
            std::env::temp_dir().join(format!("shannon-statusbar-repo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let run = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(&tmp)
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@t")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@t")
                .output()
                .unwrap()
        };
        run(&["init", "-q"]);
        run(&["commit", "--allow-empty", "-q", "-m", "init"]);
        let branch = current_git_branch(tmp.to_string_lossy().to_string())
            .await
            .unwrap();
        assert!(branch.is_some());
        assert!(!branch.unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
