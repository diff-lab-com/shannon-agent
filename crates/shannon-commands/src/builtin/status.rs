//! /status command - Show git repository status

use crate::command::{Command, CommandBase, CommandSource, PromptCommand, ExecutionContext, CommandAvailability};

/// Status prompt template
const STATUS_PROMPT: &str = r##"
Show the git repository status.

Arguments: {args}
- If args contains "--short", show a compact summary
- Otherwise, show full status with file details

Steps:
1. Run `git status {args}` to get the current status
2. Run `git branch --show-current` to get the branch name
3. Run `git log --oneline -5` to show recent commits

Present the output clearly:
- Current branch and upstream tracking
- Staged changes (files ready to commit)
- Unstaged changes (modified but not staged)
- Untracked files
- Conflicts (if any)
- Recent commit history
"##;

/// Create the /status command
pub fn command() -> Command {
    Command::Prompt(PromptCommand {
        base: CommandBase {
            name: "status".to_string(),
            aliases: vec!["st".to_string(), "git-status".to_string()],
            description: "Show git repository status and current changes".to_string(),
            has_user_specified_description: false,
            availability: vec![CommandAvailability::All],
            source: CommandSource::Builtin,
            is_enabled: true,
            is_hidden: false,
            argument_hint: Some("[--short]".to_string()),
            when_to_use: Some(
                "To see current git status, branch, staged/unstaged changes, and untracked files".to_string(),
            ),
            version: Some("0.1.0".to_string()),
            disable_model_invocation: false,
            user_invocable: true,
            is_workflow: false,
            immediate: true,
            is_sensitive: false,
            user_facing_name: None,
        },
        progress_message: "".to_string(),
        content_length: 500,
        arg_names: vec!["options".to_string()],
        allowed_tools: vec![
            "Bash(git status:*)".to_string(),
            "Bash(git branch:*)".to_string(),
            "Bash(git log:*)".to_string(),
        ],
        model: None,
        hooks: std::collections::HashMap::new(),
        context: ExecutionContext::Inline,
        agent: None,
        paths: vec![],
        prompt_template: Some(STATUS_PROMPT.to_string()),
    })
}

/// Git status information
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct GitStatusInfo {
    /// Current branch
    pub branch: String,

    /// Default branch
    pub default_branch: String,

    /// Is repository clean
    pub is_clean: bool,

    /// Staged changes
    pub staged: Vec<StatusFile>,

    /// Unstaged changes
    pub unstaged: Vec<StatusFile>,

    /// Untracked files
    pub untracked: Vec<String>,

    /// Conflicts
    pub conflicts: Vec<String>,

    /// Ahead/behind info
    pub ahead_behind: Option<AheadBehind>,
}

/// File status information
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct StatusFile {
    /// File path
    pub path: String,

    /// Status code (e.g., "M", "A", "D")
    pub status: String,

    /// Staged status
    pub staged_status: Option<String>,

    /// Original path (for renames)
    pub old_path: Option<String>,
}

/// Ahead/behind information
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct AheadBehind {
    /// Commits ahead of upstream
    pub ahead: usize,

    /// Commits behind upstream
    pub behind: usize,

    /// Upstream branch
    pub upstream: String,
}

/// Parse git status output
#[allow(dead_code)]
pub fn parse_git_status(output: &str) -> Option<GitStatusInfo> {
    let mut branch = String::new();
    let mut staged = Vec::new();
    let mut unstaged = Vec::new();
    let untracked = Vec::new();
    let mut conflicts = Vec::new();

    for line in output.lines() {
        // Branch line: "On branch main" or "HEAD detached at abc123"
        if line.starts_with("On branch ") {
            branch = line["On branch ".len()..].to_string();
        } else if line.starts_with("HEAD detached at ") {
            branch = format!("(detached) {}", line["HEAD detached at ".len()..].to_string());
        }

        // Short format parsing: "MM file.txt" (status codes in first two chars)
        if line.len() > 3 && !line.starts_with(' ') && line.chars().next().map_or(false, |c| c == 'M' || c == 'A' || c == 'D' || c == 'R' || c == 'C' || c == 'U' || c == '?' || c == '!') {
            let staged_status = line.chars().next().filter(|c| *c != ' ');
            let unstaged_status = line.chars().nth(1).filter(|c| *c != ' ');
            let path = line[3..].trim().to_string();

            if let Some(ss) = staged_status {
                if ss == 'U' || unstaged_status == Some('U') {
                    conflicts.push(path.clone());
                }
                staged.push(StatusFile {
                    path: path.clone(),
                    status: ss.to_string(),
                    staged_status: None,
                    old_path: None,
                });
            }

            if let Some(us) = unstaged_status {
                unstaged.push(StatusFile {
                    path,
                    status: us.to_string(),
                    staged_status: None,
                    old_path: None,
                });
            }
        }

        // Untracked files: "Untracked files:" section
        if line.contains("Untracked files:") {
            // In a real implementation, we'd parse the following section
        }
    }

    Some(GitStatusInfo {
        branch: if branch.is_empty() {
            "unknown".to_string()
        } else {
            branch
        },
        default_branch: "main".to_string(),
        is_clean: staged.is_empty() && unstaged.is_empty() && untracked.is_empty(),
        staged,
        unstaged,
        untracked,
        conflicts,
        ahead_behind: None,
    })
}

/// Format status for display
#[allow(dead_code)]
pub fn format_status(info: &GitStatusInfo, short: bool) -> String {
    if short {
        format_branch_short(info)
    } else {
        format_branch_verbose(info)
    }
}

fn format_branch_short(info: &GitStatusInfo) -> String {
    let mut output = format!("{}\n", info.branch);

    if info.is_clean {
        output.push_str("clean\n");
    } else {
        if !info.staged.is_empty() {
            output.push_str(&format!("{} staged\n", info.staged.len()));
        }
        if !info.unstaged.is_empty() {
            output.push_str(&format!("{} unstaged\n", info.unstaged.len()));
        }
        if !info.untracked.is_empty() {
            output.push_str(&format!("{} untracked\n", info.untracked.len()));
        }
        if !info.conflicts.is_empty() {
            output.push_str(&format!("{} conflicts\n", info.conflicts.len()));
        }
    }

    output
}

fn format_branch_verbose(info: &GitStatusInfo) -> String {
    let mut output = String::new();

    output.push_str(&format!("On branch {}\n", info.branch));

    if let Some(ab) = &info.ahead_behind {
        output.push_str(&format!(
            "Your branch is ahead of '{}' by {} commit{}\n",
            ab.upstream, ab.ahead, if ab.ahead == 1 { "" } else { "s" }
        ));
    }

    if !info.is_clean {
        output.push_str("\nChanges to be committed:\n");
        for file in &info.staged {
            output.push_str(&format!("  {} {}\n", file.status, file.path));
        }

        if !info.unstaged.is_empty() {
            output.push_str("\nChanges not staged for commit:\n");
            for file in &info.unstaged {
                output.push_str(&format!("  {} {}\n", file.status, file.path));
            }
        }

        if !info.untracked.is_empty() {
            output.push_str("\nUntracked files:\n");
            for file in &info.untracked {
                output.push_str(&format!("  {}\n", file));
            }
        }

        if !info.conflicts.is_empty() {
            output.push_str("\nUnmerged paths:\n");
            for file in &info.conflicts {
                output.push_str(&format!("  {}\n", file));
            }
        }
    } else {
        output.push_str("\nnothing to commit, working tree clean\n");
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_command() {
        let cmd = command();
        assert_eq!(cmd.name(), "status");
        assert!(cmd.aliases().contains(&"st".to_string()));
    }

    #[test]
    fn test_parse_git_status() {
        let output = "On branch main\nYour branch is up to date with 'origin/main'.\n\nnothing to commit, working tree clean";
        let info = parse_git_status(output).unwrap();
        assert_eq!(info.branch, "main");
        assert!(info.is_clean);
    }

    #[test]
    fn test_format_status_short() {
        let info = GitStatusInfo {
            branch: "main".to_string(),
            default_branch: "main".to_string(),
            is_clean: true,
            staged: vec![],
            unstaged: vec![],
            untracked: vec![],
            conflicts: vec![],
            ahead_behind: None,
        };

        let formatted = format_status(&info, true);
        assert!(formatted.contains("main"));
        assert!(formatted.contains("clean"));
    }
}
