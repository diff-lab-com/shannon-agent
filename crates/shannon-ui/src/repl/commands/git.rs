use crate::{widgets::ChatRole, Result};

use super::super::Repl;

pub(crate) fn handle_create_pr(repl: &mut Repl, args: &str) -> Result<()> {
    let trimmed = args.trim();

    // Show help
    if trimmed == "help" || trimmed == "--help" || trimmed == "-h" {
        repl.chat.add_message(ChatRole::System,
            "Create a GitHub pull request\n\n\
             Usage:\n  /create-pr            — interactive PR creation\n  \
             /create-pr <title>     — create with custom title\n  \
             /create-pr --draft     — create as draft PR\n  \
             /create-pr --base X    — set target branch (default: main)\n  \
             /create-pr --web       — open in browser to continue editing".to_string(),
        );
        return Ok(());
    }

    // Check if gh CLI is available
    let gh_check = std::process::Command::new("gh")
        .arg("--version")
        .output();
    if gh_check.is_err() {
        repl.chat.add_message(ChatRole::System,
            "GitHub CLI (gh) is not installed. Install it: https://cli.github.com".to_string(),
        );
        return Ok(());
    }

    // Check if we're in a git repo
    let git_check = std::process::Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(&repl.state.working_directory)
        .output();
    if git_check.is_err() || !git_check.as_ref().map(|o| o.status.success()).unwrap_or(false) {
        repl.chat.add_message(ChatRole::System, "Not inside a git repository.".to_string());
        return Ok(());
    }

    // Get current branch
    let branch_output = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(&repl.state.working_directory)
        .output();
    let current_branch = match branch_output {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).trim().to_string(),
        _ => {
            repl.chat.add_message(ChatRole::System, "Failed to determine current branch.".to_string());
            return Ok(());
        }
    };

    // Determine base branch (main or master)
    let base_branch = if trimmed.contains("--base") {
        if let Some(idx) = trimmed.find("--base") {
            let after = &trimmed[idx + 6..].trim_start();
            after.split_whitespace().next().unwrap_or("main").to_string()
        } else {
            "main".to_string()
        }
    } else {
        // Check if main or master exists
        let main_check = std::process::Command::new("git")
            .args(["rev-parse", "--verify", "main"])
            .current_dir(&repl.state.working_directory)
            .output();
        if main_check.as_ref().map(|o| o.status.success()).unwrap_or(false) {
            "main".to_string()
        } else {
            "master".to_string()
        }
    };

    // Don't create PR from the base branch itself
    if current_branch == base_branch {
        repl.chat.add_message(ChatRole::System,
            format!("Currently on '{current_branch}'. Create a feature branch first:\n  git checkout -b my-feature"));
        return Ok(());
    }

    // Get commits between base and HEAD
    let log_output = std::process::Command::new("git")
        .args(["log", &format!("{base_branch}..HEAD"), "--oneline"])
        .current_dir(&repl.state.working_directory)
        .output();
    let commits = match log_output {
        Ok(out) => String::from_utf8_lossy(&out.stdout).trim().to_string(),
        Err(_) => String::new(),
    };

    if commits.is_empty() {
        repl.chat.add_message(ChatRole::System,
            format!("No commits found between {base_branch} and {current_branch}. Make some changes first."));
        return Ok(());
    }

    // Generate PR title from first commit or custom args
    let is_draft = trimmed.contains("--draft");
    let open_web = trimmed.contains("--web");

    let title = {
        let non_flag_args: Vec<&str> = trimmed.split_whitespace()
            .filter(|s| !s.starts_with('-'))
            .collect();
        if non_flag_args.is_empty() {
            commits.lines().next()
                .map(|line| line.split_once(' ').map(|(_, msg)| msg.to_string()).unwrap_or(line.to_string()))
                .unwrap_or_else(|| format!("PR from {current_branch}"))
        } else {
            non_flag_args.join(" ")
        }
    };

    // Build PR body from commits
    let mut body = String::from("## Summary\n\n");
    for line in commits.lines() {
        body.push_str("- ");
        body.push_str(line);
        body.push('\n');
    }

    // Get diff stats for context
    let diff_stat = std::process::Command::new("git")
        .args(["diff", "--stat", &format!("{base_branch}...HEAD")])
        .current_dir(&repl.state.working_directory)
        .output();
    if let Ok(out) = diff_stat {
        let stat = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !stat.is_empty() {
            body.push_str("\n## Changes\n\n```\n");
            body.push_str(&stat);
            body.push_str("\n```\n");
        }
    }

    // Check for uncommitted changes and warn
    let status_output = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(&repl.state.working_directory)
        .output();
    if let Ok(out) = &status_output {
        let changes = String::from_utf8_lossy(&out.stdout);
        if !changes.trim().is_empty() {
            body.push_str("\n> **Note:** This PR was created with uncommitted changes.\n");
        }
    }

    // Push the branch first (if not already pushed)
    let push_result = std::process::Command::new("git")
        .args(["push", "-u", "origin", &current_branch])
        .current_dir(&repl.state.working_directory)
        .output();
    match push_result {
        Ok(out) if out.status.success() => {
            let push_output = String::from_utf8_lossy(&out.stderr);
            if !push_output.is_empty() {
                tracing::debug!("Push output: {}", push_output);
            }
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            // It's ok if already pushed (error contains "already up-to-date" or similar)
            if !stderr.contains("up-to-date") && !stderr.contains("Everything up-to-date") {
                super::set_error(repl, &format!("pushing branch: {stderr}"));
                return Ok(());
            }
        }
        Err(e) => {
            super::set_error(repl, &format!("pushing branch: {e}"));
            return Ok(());
        }
    }

    // Build gh pr create command
    let mut gh_args = vec!["pr", "create", "--title", &title, "--body", &body, "--base", &base_branch];
    if is_draft {
        gh_args.push("--draft");
    }
    if open_web {
        gh_args.push("--web");
    }

    let pr_result = std::process::Command::new("gh")
        .args(&gh_args)
        .current_dir(&repl.state.working_directory)
        .output();

    match pr_result {
        Ok(out) if out.status.success() => {
            let url = String::from_utf8_lossy(&out.stdout).trim().to_string();
            let msg = if is_draft { "Draft PR created" } else { "PR created" };
            repl.chat.add_message(ChatRole::System,
                format!("{msg}: {url}\n\nBranch: {current_branch} → {base_branch}\nCommits:\n{commits}"));

            // Send desktop notification if enabled
            if repl.notifications_enabled {
                let _ = repl.notifier.info("Shannon", &format!("{msg}: {url}"));
            }
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            if stderr.contains("already exists") {
                // Find existing PR URL
                let existing = std::process::Command::new("gh")
                    .args(["pr", "view", &current_branch, "--json", "url"])
                    .current_dir(&repl.state.working_directory)
                    .output();
                match existing {
                    Ok(eout) if eout.status.success() => {
                        let url: serde_json::Value = serde_json::from_str(&String::from_utf8_lossy(&eout.stdout)).unwrap_or_default();
                        let pr_url = url.get("url").and_then(|u| u.as_str()).unwrap_or("unknown");
                        repl.chat.add_message(ChatRole::System,
                            format!("PR already exists: {pr_url}\nBranch: {current_branch} → {base_branch}"));
                    }
                    _ => {
                        repl.chat.add_message(ChatRole::System,
                            format!("PR already exists for branch {current_branch}.\n{stderr}"));
                    }
                }
            } else {
                super::set_error(repl, &format!("creating PR: {stderr}"));
            }
        }
        Err(e) => {
            super::set_error(repl, &format!("running gh pr create: {e}"));
        }
    }

    Ok(())
}

pub(crate) fn handle_patch(repl: &mut Repl, args: &str) -> Result<()> {
    let args = args.trim();

    if args.is_empty() || args == "--help" || args == "help" {
        repl.chat.add_message(ChatRole::System,
            "Patch — search/replace with diff preview\n\n\
             Usage:\n\
               /patch <file> <search> --- <replace>          Preview change\n\
               /patch --apply <file> <search> --- <replace>  Apply change\n\
               /patch --all <file> <search> --- <replace>    Preview (replace all)\n\
               /patch --apply --all <file> <search> --- <replace>  Apply all\n\n\
             The preview shows the diff without modifying the file.\n\
             Add --apply to write the change.".to_string());
        return Ok(());
    }

    // Parse flags
    let apply = args.contains("--apply");
    let replace_all = args.contains("--all");
    let cleaned = args.replace("--apply", "").replace("--all", "");
    let cleaned = cleaned.trim();

    // Split on --- separator
    let parts: Vec<&str> = cleaned.splitn(2, "---").collect();
    if parts.len() < 2 {
        repl.chat.add_message(ChatRole::System,
            "Usage: /patch <file> <search> --- <replace>\nUse --- to separate search and replace text.".to_string());
        return Ok(());
    }

    // Parse file path and search text from the first part
    let first_part = parts[0].trim();
    let new_text = parts[1].trim().to_string();

    // First word is the file path, rest is the search text
    let mut words = first_part.splitn(2, char::is_whitespace);
    let file_path = match words.next() {
        Some(f) if !f.is_empty() => f.to_string(),
        _ => {
            repl.chat.add_message(ChatRole::System,
                "Usage: /patch <file> <search> --- <replace>".to_string());
            return Ok(());
        }
    };
    let old_text = words.next().unwrap_or("").to_string();

    if old_text.is_empty() {
        repl.chat.add_message(ChatRole::System,
            "Error: search text is empty.\nUsage: /patch <file> <search> --- <replace>".to_string());
        return Ok(());
    }

    // Resolve to absolute path if relative
    let abs_path = if std::path::Path::new(&file_path).is_absolute() {
        file_path.clone()
    } else {
        format!("{}/{}", repl.state.working_directory.trim_end_matches('/'), file_path)
    };

    let Some(ref engine) = repl.query_engine else {
        repl.chat.add_message(ChatRole::System, "No query engine available.".to_string());
        return Ok(());
    };

    let input = serde_json::json!({
        "file_path": abs_path,
        "old_string": old_text,
        "new_string": new_text,
        "replace_all": replace_all,
        "preview": !apply,
    });

    let tool_name = "Edit";
    match repl.runtime.block_on(engine.tools().execute(tool_name, input)) {
        Ok(result) => {
            let prefix = if apply { "Applied" } else { "Preview" };
            let msg = format!("{prefix}: {}\n{}", file_path, result.content);
            { repl.chat.add_message(ChatRole::System, msg); }
        }
        Err(e) => {
            { repl.chat.add_message(ChatRole::System, format!("Patch failed: {e}")); }
        }
    }

    Ok(())
}

pub(crate) fn handle_diff(repl: &mut Repl, args: &str) -> Result<()> {
    use shannon_commands::diff_utils;

    let trimmed = args.trim();

    // /diff view — open interactive diff viewer overlay
    if trimmed == "view" || trimmed == "--view" {
        let file_count = {
            let diff = &repl.diff_data;
            let mut count = 0usize;
            let mut seen = std::collections::HashSet::new();
            for turn in diff.get_session_diffs() {
                for fc in &turn.files_modified {
                    if seen.insert(fc.path.clone()) {
                        count += 1;
                    }
                }
                count += turn.files_created.len() + turn.files_deleted.len();
            }
            count
        };
        let mut viewer = crate::widgets::diff_viewer::DiffViewerWidget::new();
        viewer.sync_expanded(file_count);
        repl.state.diff_viewer = Some(viewer);
        return Ok(());
    }

    // /diff interactive — open interactive hunk-by-hunk review
    if trimmed == "interactive" || trimmed == "--interactive" {
        let output = std::process::Command::new("git")
            .args(["diff", "HEAD"])
            .current_dir(&repl.state.working_directory)
            .output();

        match output {
            Ok(o) if o.status.success() => {
                let diff_str = String::from_utf8_lossy(&o.stdout);
                let hunks = crate::widgets::diff_viewer::InteractiveHunk::parse_from_diff(&diff_str, None);
                if hunks.is_empty() {
                    repl.chat.add_message(ChatRole::System, "No diff hunks found.".to_string());
                } else {
                    repl.state.interactive_hunks = hunks;
                    repl.state.interactive_selected = 0;
                    repl.state.diff_interactive = true;
                    repl.state.diff_viewer = Some(crate::widgets::diff_viewer::DiffViewerWidget::new());
                }
            }
            Ok(o) => {
                let err = String::from_utf8_lossy(&o.stderr);
                super::set_error(repl, &format!("git diff: {err}"));
            }
            Err(e) => {
                super::set_error(repl, &format!("running git diff: {e}"));
            }
        }
        return Ok(());
    }

    // /diff accept-all — keep all unstaged changes
    if trimmed == "accept-all" || trimmed == "keep-all" {
        let output = std::process::Command::new("git")
            .args(["add", "-A"])
            .current_dir(&repl.state.working_directory)
            .output();
        match output {
            Ok(o) if o.status.success() => {
                repl.chat.add_message(ChatRole::System, "All changes accepted and staged.".to_string());
            }
            Ok(o) => {
                super::set_error(repl, &format!("staging: {}", String::from_utf8_lossy(&o.stderr)));
            }
            Err(e) => { super::set_error(repl, &format!("{e}")); }
        }
        return Ok(());
    }

    // /diff reject-all — discard all unstaged changes
    if trimmed == "reject-all" || trimmed == "discard-all" {
        // First warn about destructive action
        let status_output = std::process::Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(&repl.state.working_directory)
            .output();
        let file_count = status_output
            .map(|o| String::from_utf8_lossy(&o.stdout).lines().count())
            .unwrap_or(0);
        if file_count == 0 {
            repl.chat.add_message(ChatRole::System, "No changes to reject.".to_string());
            return Ok(());
        }
        let output = std::process::Command::new("git")
            .args(["checkout", "--", "."])
            .current_dir(&repl.state.working_directory)
            .output();
        match output {
            Ok(o) if o.status.success() => {
                repl.chat.add_message(ChatRole::System, format!("All unstaged changes discarded ({file_count} files)."));
            }
            Ok(o) => {
                super::set_error(repl, &format!("discarding changes: {}", String::from_utf8_lossy(&o.stderr)));
            }
            Err(e) => { super::set_error(repl, &format!("discarding changes: {e}")); }
        }
        // Also clean untracked files
        let _ = std::process::Command::new("git")
            .args(["clean", "-fd"])
            .current_dir(&repl.state.working_directory)
            .output();
        return Ok(());
    }

    // /diff accept <file> — accept changes to a specific file
    if let Some(file) = trimmed.strip_prefix("accept ") {
        let file = file.trim();
        let output = std::process::Command::new("git")
            .args(["add", "--", file])
            .current_dir(&repl.state.working_directory)
            .output();
        match output {
            Ok(o) if o.status.success() => {
                repl.chat.add_message(ChatRole::System, format!("Changes to '{file}' accepted (staged)."));
            }
            Ok(o) => {
                super::set_error(repl, &format!("git operation failed: {}", String::from_utf8_lossy(&o.stderr)));
            }
            Err(e) => { super::set_error(repl, &format!("{e}")); }
        }
        return Ok(());
    }

    // /diff reject <file> — reject changes to a specific file
    if let Some(file) = trimmed.strip_prefix("reject ") {
        let file = file.trim();
        let output = std::process::Command::new("git")
            .args(["checkout", "--", file])
            .current_dir(&repl.state.working_directory)
            .output();
        match output {
            Ok(o) if o.status.success() => {
                repl.chat.add_message(ChatRole::System, format!("Changes to '{file}' rejected (reverted)."));
            }
            Ok(o) => {
                super::set_error(repl, &format!("git operation failed: {}", String::from_utf8_lossy(&o.stderr)));
            }
            Err(e) => { super::set_error(repl, &format!("{e}")); }
        }
        return Ok(());
    }

    // /diff review — interactive per-file review
    if trimmed == "review" || trimmed == "--review" {
        let status_output = std::process::Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(&repl.state.working_directory)
            .output();

        match status_output {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                if stdout.trim().is_empty() {
                    repl.chat.add_message(ChatRole::System, "No changes to review.".to_string());
                    return Ok(());
                }

                let mut msg = String::from("Interactive Diff Review\n\nChanged files:\n\n");
                for (i, line) in stdout.lines().enumerate() {
                    let status = &line[..2];
                    let file = &line[3..];
                    let status_desc = match status.trim() {
                        "M" => "modified",
                        "A" => "added",
                        "D" => "deleted",
                        "R" => "renamed",
                        "C" => "copied",
                        "??" => "untracked",
                        "!!" => "ignored",
                        s if s.ends_with('M') => "modified (staged)",
                        s if s.starts_with('M') => "modified",
                        _ => status,
                    };
                    msg.push_str(&format!("  [{}] {} ({})\n", i + 1, file, status_desc));
                }

                msg.push_str("\nCommands:\n");
                msg.push_str("  /diff review <n>    — show diff for file #n\n");
                msg.push_str("  /diff accept <file> — keep changes to file\n");
                msg.push_str("  /diff reject <file> — discard changes to file\n");
                msg.push_str("  /diff accept-all    — keep all changes\n");
                msg.push_str("  /diff reject-all    — discard all changes\n");

                repl.chat.add_message(ChatRole::System, msg);
            }
            Err(e) => { super::set_error(repl, &format!("getting git status: {e}")); }
        }
        return Ok(());
    }

    // /diff review <n> — show diff for a specific file by number
    if let Some(num_str) = trimmed.strip_prefix("review ") {
        if let Ok(num) = num_str.trim().parse::<usize>() {
            let status_output = std::process::Command::new("git")
                .args(["status", "--porcelain"])
                .current_dir(&repl.state.working_directory)
                .output();
            if let Ok(output) = status_output {
                let stdout = String::from_utf8_lossy(&output.stdout);
                if let Some(line) = stdout.lines().nth(num - 1) {
                    let file = &line[3..];
                    let diff_output = std::process::Command::new("git")
                        .args(["diff", "--", file])
                        .current_dir(&repl.state.working_directory)
                        .output();
                    match diff_output {
                        Ok(result) => {
                            let diff = String::from_utf8_lossy(&result.stdout);
                            if diff.is_empty() {
                                repl.chat.add_message(ChatRole::System, format!("No unstaged diff for '{file}'."));
                            } else {
                                let max = 8000;
                                let end = if diff.len() > max {
                                    let mut e = max; while !diff.is_char_boundary(e) { e -= 1; } e
                                } else { diff.len() };
                                let truncated = &diff[..end];
                                let mut msg = format!("Diff for '{file}':\n```\n{truncated}");
                                if diff.len() > 8000 { msg.push_str("\n... (truncated)"); }
                                msg.push_str("\n```\n\n");
                                msg.push_str(&format!("Accept: /diff accept {file}\nReject: /diff reject {file}"));
                                repl.chat.add_message(ChatRole::System, msg);
                            }
                        }
                        Err(e) => { super::set_error(repl, &format!("{e}")); }
                    }
                } else {
                    repl.chat.add_message(ChatRole::System, format!("Invalid file number: {num}. Use /diff review to list files."));
                }
            }
            return Ok(());
        }
    }

    // /diff review branch [name] — compare current branch vs a base branch
    if let Some(rest) = trimmed.strip_prefix("review branch") {
        let base = if rest.trim().is_empty() { "main" } else { rest.trim() };
        let output = std::process::Command::new("git")
            .args(["diff", &format!("{base}...HEAD"), "--stat"])
            .current_dir(&repl.state.working_directory)
            .output();
        match output {
            Ok(o) if o.status.success() => {
                let stat = String::from_utf8_lossy(&o.stdout);
                let mut msg = format!("Diff: {base}...HEAD\n```\n{stat}```\n\n");
                msg.push_str("Use /diff interactive for hunk-by-hunk review");
                repl.chat.add_message(ChatRole::System, msg);
            }
            Ok(o) => {
                let err = String::from_utf8_lossy(&o.stderr);
                super::set_error(repl, &format!("git diff: {err}"));
            }
            Err(e) => {
                super::set_error(repl, &format!("running git diff: {e}"));
            }
        }
        return Ok(());
    }

    // /diff review <ref> (e.g., HEAD~3, abc123) — compare working tree vs a ref
    if let Some(gitref) = trimmed.strip_prefix("review ") {
        let gitref = gitref.trim();
        if !gitref.is_empty() && !gitref.chars().all(|c| c.is_ascii_digit()) {
            let output = std::process::Command::new("git")
                .args(["diff", gitref, "--stat"])
                .current_dir(&repl.state.working_directory)
                .output();
            match output {
                Ok(o) if o.status.success() => {
                    let stat = String::from_utf8_lossy(&o.stdout);
                    let mut msg = format!("Diff vs {gitref}\n```\n{stat}```\n\n");
                    msg.push_str("Use /diff interactive for hunk-by-hunk review");
                    repl.chat.add_message(ChatRole::System, msg);
                }
                Ok(o) => {
                    let err = String::from_utf8_lossy(&o.stderr);
                    super::set_error(repl, &format!("git diff: {err}"));
                }
                Err(e) => {
                    super::set_error(repl, &format!("running git diff: {e}"));
                }
            }
            return Ok(());
        }
    }

    let options = diff_utils::DiffOptions::from_args(args);
    let show_overview = args.trim().is_empty() || args.contains("--overview");

    // When no args or --overview, show both staged and unstaged stats side-by-side
    if show_overview && options.revision_range.is_none() {
        let mut overview = String::from("Diff Overview\n\n");

        // Unstaged changes
        let unstaged = std::process::Command::new("git")
            .args(["diff", "--stat"])
            .current_dir(&repl.state.working_directory)
            .output();
        match unstaged {
            Ok(result) => {
                let stdout = String::from_utf8_lossy(&result.stdout);
                if stdout.is_empty() {
                    overview.push_str("Unstaged: no changes\n");
                } else {
                    overview.push_str("Unstaged changes:\n");
                    overview.push_str(&format_file_diff_stats(&stdout));
                    overview.push('\n');
                }
            }
            Err(e) => overview.push_str(&format!("Unstaged: error ({e})\n")),
        }

        // Staged changes
        let staged = std::process::Command::new("git")
            .args(["diff", "--staged", "--stat"])
            .current_dir(&repl.state.working_directory)
            .output();
        match staged {
            Ok(result) => {
                let stdout = String::from_utf8_lossy(&result.stdout);
                if stdout.is_empty() {
                    overview.push_str("Staged: no changes\n");
                } else {
                    overview.push_str("Staged changes:\n");
                    overview.push_str(&format_file_diff_stats(&stdout));
                    overview.push('\n');
                }
            }
            Err(e) => overview.push_str(&format!("Staged: error ({e})\n")),
        }

        overview.push_str("Use /diff --staged, /diff HEAD~1, /diff --stat for detailed views");
        repl.chat.add_message(ChatRole::System, overview);
        return Ok(());
    }

    let cmd_str = diff_utils::build_diff_command(&options);

    let cmd_parts: Vec<&str> = cmd_str.split_whitespace().collect();
    if cmd_parts.is_empty() {
        repl.chat.add_message(ChatRole::System, "Failed to build git diff command.".to_string());
        return Ok(());
    }

    let output = std::process::Command::new(cmd_parts[0])
        .args(&cmd_parts[1..])
        .current_dir(&repl.state.working_directory)
        .output();

    match output {
        Ok(result) => {
            let stdout = String::from_utf8_lossy(&result.stdout);
            let stderr = String::from_utf8_lossy(&result.stderr);

            if !stderr.is_empty() && stdout.is_empty() {
                repl.chat.add_message(ChatRole::System, format!("Git diff error: {stderr}"));
            } else if stdout.is_empty() {
                repl.chat.add_message(ChatRole::System, "No changes found.".to_string());
            } else {
                let analyzer = diff_utils::DiffAnalyzer::new();
                let analysis = analyzer.analyze(&stdout);

                // Per-file breakdown
                let mut file_stats: Vec<(String, i32, i32)> = Vec::new();
                let mut current_file = String::new();
                for line in stdout.lines() {
                    if let Some(rest) = line.strip_prefix("diff --git a/") {
                        if let Some(name) = rest.split(' ').next() {
                            current_file = name.to_string();
                        }
                    } else if line.starts_with('+') && !line.starts_with("+++") {
                        if let Some(entry) = file_stats.iter_mut().find(|(f, _, _)| f == &current_file) {
                            entry.1 += 1;
                        } else {
                            file_stats.push((current_file.clone(), 1, 0));
                        }
                    } else if line.starts_with('-') && !line.starts_with("---") {
                        if let Some(entry) = file_stats.iter_mut().find(|(f, _, _)| f == &current_file) {
                            entry.2 += 1;
                        } else {
                            file_stats.push((current_file.clone(), 0, 1));
                        }
                    }
                }

                let total_lines = stdout.lines().count();
                let category_summary = analysis.summary();
                let test_flag = if analysis.has_test_changes() { " [has test changes]" } else { "" };

                let mut report = format!(
                    "Git diff ({} files, {} lines){test_flag}\nCategories: {category_summary}\n",
                    file_stats.len(), total_lines,
                );

                // File-by-file summary
                if !file_stats.is_empty() {
                    report.push_str("\nFiles:\n");
                    for (file, adds, dels) in &file_stats {
                        let bar = format_change_bar(*adds, *dels);
                        report.push_str(&format!("  {bar} {file} (+{adds}/-{dels})\n"));
                    }
                }

                // Raw diff (truncated)
                let raw_diff = if stdout.len() > 4000 {
                    format!("{}\n... (truncated)", &stdout[..4000])
                } else {
                    stdout.to_string()
                };
                report.push_str(&format!("\n{raw_diff}"));

                repl.chat.add_message(ChatRole::System, report);
            }
        }
        Err(e) => { super::set_error(repl, &format!("running git diff: {e}")); }
    }
    Ok(())
}

/// Format a visual change bar for a file.
pub(crate) fn format_change_bar(additions: i32, deletions: i32) -> String {
    let total = (additions + deletions).min(20) as usize;
    let add_chars = (additions as f32 / (additions + deletions).max(1) as f32 * total as f32).round() as usize;
    let del_chars = total - add_chars;
    format!("{}{}", "+".repeat(add_chars), "-".repeat(del_chars))
}

/// Format diff --stat output into per-file lines.
pub(crate) fn format_file_diff_stats(stat_output: &str) -> String {
    let mut result = String::new();
    for line in stat_output.lines() {
        if line.starts_with(' ') || line.contains('|') {
            result.push_str(&format!("  {line}\n"));
        }
    }
    result
}

pub(crate) fn handle_stage(repl: &mut Repl, args: &str) -> Result<()> {
    let target = args.trim();

    if target.is_empty() {
        // Show unstaged changes summary
        let output = std::process::Command::new("git")
            .args(["diff", "--stat"])
            .current_dir(&repl.state.working_directory)
            .output();

        let mut msg = String::from("Interactive Stage\n\n");

        match output {
            Ok(result) => {
                let stdout = String::from_utf8_lossy(&result.stdout);
                let stderr = String::from_utf8_lossy(&result.stderr);

                if !stderr.is_empty() && stdout.is_empty() {
                    msg.push_str(&format!("Git error: {stderr}"));
                } else if stdout.is_empty() {
                    // Check for untracked files
                    let untracked = std::process::Command::new("git")
                        .args(["ls-files", "--others", "--exclude-standard"])
                        .current_dir(&repl.state.working_directory)
                        .output();
                    if let Ok(ut_result) = untracked {
                        let ut_files = String::from_utf8_lossy(&ut_result.stdout);
                        if ut_files.trim().is_empty() {
                            msg.push_str("No unstaged or untracked changes.\n");
                        } else {
                            let count = ut_files.lines().filter(|l| !l.is_empty()).count();
                            msg.push_str(&format!("No unstaged changes, but {count} untracked file(s):\n"));
                            for line in ut_files.lines().filter(|l| !l.is_empty()).take(20) {
                                msg.push_str(&format!("  ? {line}\n"));
                            }
                            msg.push_str("\nUse /stage <file> to stage a file, or /stage --all to stage everything.");
                        }
                    }
                } else {
                    msg.push_str("Unstaged changes:\n```\n");
                    msg.push_str(&stdout);
                    msg.push_str("```\n\n");

                    // List changed files for easy staging
                    let files_output = std::process::Command::new("git")
                        .args(["diff", "--name-only"])
                        .current_dir(&repl.state.working_directory)
                        .output();
                    if let Ok(fo) = files_output {
                        let files = String::from_utf8_lossy(&fo.stdout);
                        let file_list: Vec<&str> = files.lines().filter(|l| !l.is_empty()).collect();
                        if !file_list.is_empty() {
                            msg.push_str("Files to stage:\n");
                            for f in &file_list {
                                msg.push_str(&format!("  /stage {f}\n"));
                            }
                            msg.push_str("\nTip: /stage --all to stage all changes.");
                        }
                    }
                }
            }
            Err(e) => {
                msg.push_str(&format!("Failed to run git diff: {e}"));
            }
        }

        repl.chat.add_message(ChatRole::System, msg);
    } else if target == "--all" || target == "-A" {
        // Stage all changes
        let output = std::process::Command::new("git")
            .args(["add", "--all"])
            .current_dir(&repl.state.working_directory)
            .output();
        match output {
            Ok(o) if o.status.success() => {
                repl.chat.add_message(ChatRole::System, "All changes staged.".to_string());
            }
            Ok(o) => {
                let err = String::from_utf8_lossy(&o.stderr);
                repl.chat.add_message(ChatRole::System, format!("git add failed: {err}"));
            }
            Err(e) => {
                super::set_error(repl, &format!("running git add: {e}"));
            }
        }
    } else {
        // Stage specific files
        let files: Vec<&str> = target.split_whitespace().collect();
        let output = std::process::Command::new("git")
            .args(["add"])
            .args(&files)
            .current_dir(&repl.state.working_directory)
            .output();
        match output {
            Ok(o) if o.status.success() => {
                let count = files.len();
                repl.chat.add_message(ChatRole::System, format!("Staged {count} file(s)."));
            }
            Ok(o) => {
                let err = String::from_utf8_lossy(&o.stderr);
                repl.chat.add_message(ChatRole::System, format!("git add failed: {err}"));
            }
            Err(e) => {
                super::set_error(repl, &format!("running git add: {e}"));
            }
        }
    }

    Ok(())
}

pub(crate) fn handle_status(repl: &mut Repl, args: &str) -> Result<()> {
    use shannon_commands::status_utils::{parse_git_status, format_status};

    let short = args.contains("--short");

    let output = std::process::Command::new("git")
        .args(["status", "--short", "--branch"])
        .current_dir(&repl.state.working_directory)
        .output();

    let status_output = match output {
        Ok(result) => {
            let stdout = String::from_utf8_lossy(&result.stdout);
            let stderr = String::from_utf8_lossy(&result.stderr);
            if !stderr.is_empty() && stdout.is_empty() {
                repl.chat.add_message(ChatRole::System, format!("Git error: {stderr}"));
                return Ok(());
            }
            stdout.to_string()
        }
        Err(e) => {
            super::set_error(repl, &format!("running git status: {e}"));
            return Ok(());
        }
    };

    if let Some(info) = parse_git_status(&status_output) {
        let mut full_output = format_status(&info, short);

        let log_output = std::process::Command::new("git")
            .args(["log", "--oneline", "-5"])
            .current_dir(&repl.state.working_directory)
            .output();

        if let Ok(log_result) = log_output {
            let log_stdout = String::from_utf8_lossy(&log_result.stdout);
            if !log_stdout.is_empty() {
                full_output.push_str("\nRecent commits:\n");
                full_output.push_str(&log_stdout);
            }
        }

        repl.chat.add_message(ChatRole::System, full_output);
    } else {
        repl.chat.add_message(ChatRole::System, status_output);
    }

    Ok(())
}

pub(crate) fn handle_ci(repl: &mut Repl, args: &str) -> Result<()> {
    // Check if gh CLI is available
    let gh_check = std::process::Command::new("gh")
        .arg("--version")
        .output();

    if gh_check.is_err() {
        repl.chat.add_message(ChatRole::System,
            "GitHub CLI (gh) is not installed.\nInstall it from: https://cli.github.com/".to_string());
        return Ok(());
    }

    let parts: Vec<&str> = args.splitn(2, ' ').collect();
    let subcommand = parts.first().copied().unwrap_or("");

    match subcommand {
        "" | "status" => {
            // Show recent workflow runs
            let output = std::process::Command::new("gh")
                .args(["run", "list", "--limit", "10"])
                .current_dir(&repl.state.working_directory)
                .output();

            match output {
                Ok(result) => {
                    let stdout = String::from_utf8_lossy(&result.stdout);
                    let stderr = String::from_utf8_lossy(&result.stderr);
                    if !stderr.is_empty() && stdout.is_empty() {
                        repl.chat.add_message(ChatRole::System, format!("CI error: {stderr}"));
                    } else if stdout.is_empty() {
                        repl.chat.add_message(ChatRole::System, "No workflow runs found.".to_string());
                    } else {
                        repl.chat.add_message(ChatRole::System, format!("Recent workflow runs:\n{stdout}"));
                    }
                }
                Err(e) => {
                    super::set_error(repl, &format!("querying CI: {e}"));
                }
            }
        }
        "runs" => {
            let limit = parts.get(1).and_then(|s| s.parse::<usize>().ok()).unwrap_or(10);
            let output = std::process::Command::new("gh")
                .args(["run", "list", "--limit", &limit.to_string()])
                .current_dir(&repl.state.working_directory)
                .output();

            match output {
                Ok(result) => {
                    let stdout = String::from_utf8_lossy(&result.stdout);
                    let stderr = String::from_utf8_lossy(&result.stderr);
                    if !stderr.is_empty() && stdout.is_empty() {
                        repl.chat.add_message(ChatRole::System, format!("CI error: {stderr}"));
                    } else {
                        repl.chat.add_message(ChatRole::System, format!("Workflow runs (limit: {limit}):\n{stdout}"));
                    }
                }
                Err(e) => {
                    super::set_error(repl, &format!("listing CI runs: {e}"));
                }
            }
        }
        "workflows" => {
            let output = std::process::Command::new("gh")
                .args(["workflow", "list"])
                .current_dir(&repl.state.working_directory)
                .output();

            match output {
                Ok(result) => {
                    let stdout = String::from_utf8_lossy(&result.stdout);
                    let stderr = String::from_utf8_lossy(&result.stderr);
                    if !stderr.is_empty() && stdout.is_empty() {
                        repl.chat.add_message(ChatRole::System, format!("CI error: {stderr}"));
                    } else {
                        repl.chat.add_message(ChatRole::System, format!("Workflows:\n{stdout}"));
                    }
                }
                Err(e) => {
                    super::set_error(repl, &format!("listing workflows: {e}"));
                }
            }
        }
        "view" => {
            let run_id = parts.get(1).copied().unwrap_or("");
            if run_id.is_empty() {
                repl.chat.add_message(ChatRole::System, "Usage: /ci view <run-id>".to_string());
                return Ok(());
            }
            let output = std::process::Command::new("gh")
                .args(["run", "view", run_id])
                .current_dir(&repl.state.working_directory)
                .output();

            match output {
                Ok(result) => {
                    let stdout = String::from_utf8_lossy(&result.stdout);
                    let stderr = String::from_utf8_lossy(&result.stderr);
                    if !stderr.is_empty() && stdout.is_empty() {
                        repl.chat.add_message(ChatRole::System, format!("CI error: {stderr}"));
                    } else {
                        repl.chat.add_message(ChatRole::System, format!("Run details:\n{stdout}"));
                    }
                }
                Err(e) => {
                    super::set_error(repl, &format!("viewing CI run: {e}"));
                }
            }
        }
        "trigger" => {
            let workflow = parts.get(1).copied().unwrap_or("");
            if workflow.is_empty() {
                repl.chat.add_message(ChatRole::System,
                    "Usage: /ci trigger <workflow-name>\nUse /ci workflows to see available workflows.".to_string());
                return Ok(());
            }
            let output = std::process::Command::new("gh")
                .args(["workflow", "run", workflow])
                .current_dir(&repl.state.working_directory)
                .output();

            match output {
                Ok(result) => {
                    let stderr = String::from_utf8_lossy(&result.stderr);
                    if result.status.success() {
                        repl.chat.add_message(ChatRole::System,
                            format!("Workflow '{workflow}' triggered successfully."));
                    } else {
                        repl.chat.add_message(ChatRole::System,
                            format!("Failed to trigger workflow: {stderr}"));
                    }
                }
                Err(e) => {
                    super::set_error(repl, &format!("triggering workflow: {e}"));
                }
            }
        }
        _ => {
            repl.chat.add_message(ChatRole::System, "\
CI/GitHub Actions Commands:
  /ci            — Show recent workflow runs (default: 10)
  /ci status     — Same as above
  /ci runs [N]   — List recent N workflow runs
  /ci workflows  — List all workflows
  /ci view <id>  — View details of a specific run
  /ci trigger <name> — Trigger a workflow
  /ci help       — Show this help

Requires GitHub CLI (gh) to be installed.".to_string());
        }
    }

    Ok(())
}

pub(crate) fn handle_review(repl: &mut Repl, args: &str) -> Result<()> {
    let target = args.trim();

    // Get the diff to review
    let diff_output = if target.is_empty() {
        std::process::Command::new("git")
            .args(["diff", "--stat"])
            .current_dir(&repl.state.working_directory)
            .output()
    } else {
        std::process::Command::new("git")
            .args(["diff", target])
            .current_dir(&repl.state.working_directory)
            .output()
    };

    let mut review = String::from("Code Review\n\n");

    match diff_output {
        Ok(result) => {
            let stdout = String::from_utf8_lossy(&result.stdout);
            let stderr = String::from_utf8_lossy(&result.stderr);

            if !stderr.is_empty() && stdout.is_empty() {
                review.push_str(&format!("Git error: {stderr}"));
            } else if stdout.is_empty() {
                review.push_str("No uncommitted changes to review.\n");
                review.push_str("Usage: /review [diff target]\n");
                review.push_str("Examples: /review, /review HEAD~1, /review main...HEAD");
            } else {
                review.push_str("Changes found:\n```\n");
                review.push_str(&stdout);
                review.push_str("\n```\n\n");

                // Get full diff for analysis (truncated)
                let full_diff = std::process::Command::new("git")
                    .args(["diff"])
                    .current_dir(&repl.state.working_directory)
                    .output();

                if let Ok(diff_result) = full_diff {
                    let diff_text = String::from_utf8_lossy(&diff_result.stdout);
                    let files: Vec<&str> = diff_text.lines().filter(|l| l.starts_with("diff --git")).collect();
                    let additions = diff_text.lines().filter(|l| l.starts_with('+') && !l.starts_with("+++")).count();
                    let deletions = diff_text.lines().filter(|l| l.starts_with('-') && !l.starts_with("---")).count();

                    review.push_str(&format!("Summary: {} files changed, +{}/-{} lines\n\n", files.len(), additions, deletions));

                    // Basic automated checks
                    let mut findings: Vec<String> = Vec::new();

                    // Check for potential secrets
                    let secret_patterns = ["API_KEY", "api_key", "password", "secret_key", "access_token",
                        "private_key", "credential", "auth_token", "BEGIN RSA", "BEGIN PRIVATE"];
                    let added_lines: Vec<&str> = diff_text.lines()
                        .filter(|l| l.starts_with('+') && !l.starts_with("+++"))
                        .collect();
                    for pat in &secret_patterns {
                        if added_lines.iter().any(|l| l.contains(pat)) {
                            findings.push("[SECURITY] Potential secret/credential detected — review for accidental exposure".to_string());
                            break;
                        }
                    }

                    // Check for large diffs
                    if additions + deletions > 500 {
                        findings.push("[WARN] Large diff — consider splitting into smaller changes".to_string());
                    }

                    // Check for debug prints left in
                    let debug_patterns = ["println!", "console.log", "print(", "dbg!", "eprintln!", "fmt.Println"];
                    for pat in &debug_patterns {
                        if added_lines.iter().any(|l| l.contains(pat)) {
                            findings.push(format!("[WARN] Debug output detected: `{pat}` — remove before commit"));
                            break;
                        }
                    }

                    // Check for unsafe code in Rust
                    if added_lines.iter().any(|l| l.contains("unsafe ")) {
                        findings.push("[REVIEW] Unsafe code block added — requires careful review".to_string());
                    }

                    // Check for unwrap() calls that could panic
                    let unwrap_count = added_lines.iter().filter(|l| l.contains(".unwrap()")).count();
                    if unwrap_count > 3 {
                        findings.push(format!("[WARN] {unwrap_count} .unwrap() calls added — consider proper error handling"));
                    }

                    // Check for TODO/FIXME
                    if added_lines.iter().any(|l| l.contains("TODO") || l.contains("FIXME") || l.contains("HACK")) {
                        findings.push("[INFO] New TODO/FIXME/HACK comments added".to_string());
                    }

                    // Check for hardcoded IPs or URLs
                    let has_hardcoded = added_lines.iter().any(|l| {
                        (l.contains("127.0.0.1") || l.contains("localhost")) && !l.contains("test") && !l.contains("example")
                    });
                    if has_hardcoded {
                        findings.push("[WARN] Hardcoded localhost/127.0.0.1 detected — use configurable endpoints".to_string());
                    }

                    // Check for test changes
                    let has_test_changes = diff_text.lines()
                        .filter(|l| l.starts_with("diff --git"))
                        .any(|l| l.contains("test") || l.contains("spec"));
                    if has_test_changes {
                        findings.push("[PASS] Test changes detected".to_string());
                    } else if additions + deletions > 50 {
                        findings.push("[WARN] No test changes — consider adding tests for new code".to_string());
                    }

                    if findings.is_empty() {
                        review.push_str("Automated checks: No issues detected.\n");
                    } else {
                        review.push_str(&format!("Automated findings ({}):\n", findings.len()));
                        for finding in &findings {
                            review.push_str(&format!("  {finding}\n"));
                        }
                    }

                    review.push_str("\nTo get AI-powered review, ask in the chat after these changes.");
                }
            }
        }
        Err(e) => {
            review.push_str(&format!("Failed to run git diff: {e}"));
        }
    }

    repl.chat.add_message(ChatRole::System, review);
    Ok(())
}

pub(crate) fn handle_worktree(repl: &mut Repl, args: &str) -> Result<()> {
    let arg = args.trim();

    if arg.is_empty() || arg == "status" {
        let status = if arg.is_empty() {
            "Usage: /worktree [enter <name>|exit [--keep|--remove]|status]\n".to_string()
        } else {
            String::new()
        };

        let active = shannon_agents::get_active_worktree();
        match active.as_ref() {
            Some(session) => {
                repl.chat.add_message(ChatRole::System, format!(
                    "{}Active worktree:\n  Branch: {}\n  Path: {}\n  Created: {}",
                    status, session.branch_name, session.path.display(),
                    session.created_at.format("%Y-%m-%d %H:%M"),
                ));
            }
            None => {
                repl.chat.add_message(ChatRole::System, format!("{status}No active worktree. Working in main repository."));
            }
        }
        return Ok(());
    }

    let parts: Vec<&str> = arg.splitn(3, ' ').collect();
    match parts[0] {
        "enter" => {
            let name = parts.get(1).copied().unwrap_or("");
            if name.is_empty() {
                repl.chat.add_message(ChatRole::System, "Usage: /worktree enter <name>".to_string());
                return Ok(());
            }
            let input = serde_json::json!({ "name": name });
            let Some(engine) = repl.query_engine.as_ref() else {
                repl.chat.add_message(ChatRole::System, "No query engine available.".to_string());
                return Ok(());
            };
            match repl.runtime.block_on(engine.tools().execute("enter_worktree", input)) {
                Ok(result) => { repl.chat.add_message(ChatRole::System, format!("Entered worktree: {}", result.content)); }
                Err(e) => { super::set_error(repl, &format!("entering worktree: {e}")); }
            }
        }
        "exit" => {
            let action = parts.get(1).copied().unwrap_or("keep");
            let exit_action = match action { "--remove" => "remove", _ => "keep" };
            let input = serde_json::json!({ "action": exit_action });
            let Some(engine) = repl.query_engine.as_ref() else {
                repl.chat.add_message(ChatRole::System, "No query engine available.".to_string());
                return Ok(());
            };
            match repl.runtime.block_on(engine.tools().execute("exit_worktree", input)) {
                Ok(result) => { repl.chat.add_message(ChatRole::System, format!("Exited worktree: {}", result.content)); }
                Err(e) => { super::set_error(repl, &format!("exiting worktree: {e}")); }
            }
        }
        _ => {
            repl.chat.add_message(ChatRole::System, "Unknown worktree action. Use: enter <name>, exit [--keep|--remove], or status".to_string());
        }
    }

    Ok(())
}
