//! /issue command - Manage GitHub issues
//!
//! Provides the `/issue` slash command which wraps `gh issue` operations:
//! create, list, view, comment, close, reopen.

use crate::command::{
    Command, CommandAvailability, CommandBase, CommandSource, ExecutionContext, PromptCommand,
};

/// Create the /issue command
pub fn command() -> Command {
    Command::Prompt(Box::new(PromptCommand {
        base: CommandBase {
            name: "issue".to_string(),
            aliases: vec!["gh-issue".to_string()],
            description: "Create or manage GitHub issues".to_string(),
            has_user_specified_description: false,
            availability: vec![CommandAvailability::All],
            source: CommandSource::Builtin,
            is_enabled: true,
            is_hidden: false,
            argument_hint: Some("[create|list|view|close] [args]".to_string()),
            when_to_use: Some(
                "Use when you need to create a GitHub issue or manage existing issues".to_string(),
            ),
            version: Some("0.1.0".to_string()),
            disable_model_invocation: false,
            user_invocable: true,
            is_workflow: false,
            immediate: false,
            is_sensitive: false,
            user_facing_name: None,
        },
        progress_message: "Working on GitHub issue...".to_string(),
        content_length: 3000,
        arg_names: vec!["subcommand".to_string(), "args".to_string()],
        allowed_tools: vec![
            "Bash(gh issue:*)".to_string(),
            "Bash(git log:*)".to_string(),
            "Bash(git diff:*)".to_string(),
            "Bash(git status:*)".to_string(),
        ],
        model: None,
        hooks: std::collections::HashMap::new(),
        context: ExecutionContext::Inline,
        agent: None,
        paths: vec![],
        prompt_template: Some(ISSUE_PROMPT.to_string()),
    }))
}

/// Prompt template for /issue command
const ISSUE_PROMPT: &str = r##"## Context

Run these git commands yourself first (Shannon does not pre-execute them for you):
- `git status --short` — current working-tree state
- `git branch --show-current` — current branch
- `git log --oneline -5` — recent commits

## Your task

Manage a GitHub issue based on the arguments: {args}

### If creating an issue (default or "create"):
1. Understand what the user wants the issue to be about from the arguments
2. Create the issue using:
```bash
gh issue create --title "Title" --body "Description" [optional flags]
```
- The title should be concise and descriptive
- The body should include:
  - Summary of the issue/feature
  - Steps to reproduce (for bugs) or motivation (for features)
  - Expected vs actual behavior (for bugs)
  - Any relevant code references

### If "list" or "ls":
```bash
gh issue list --limit 20
```

### If "view" with a number:
```bash
gh issue view {args}
```

### If "close" with a number:
```bash
gh issue close {args} --comment "Closing reason"
```

### If "comment" with a number:
```bash
gh issue comment {args} --body "Comment text"
```

## Important
- Use the `gh` CLI tool for all operations
- If `gh` is not authenticated, suggest running `gh auth login`
- Do not make up issue numbers — use `list` first if unsure
- Keep issue titles under 80 characters
- Use markdown formatting in issue bodies

Execute the appropriate command based on the arguments. Do not send any other text."##;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_issue_command() {
        let cmd = command();
        assert_eq!(cmd.name(), "issue");
        assert_eq!(cmd.aliases(), &["gh-issue".to_string()]);
    }

    #[test]
    fn test_issue_prompt_contains_gh() {
        assert!(ISSUE_PROMPT.contains("gh issue"));
    }

    /// Shannon does not implement Claude-Code `!`cmd`` interpolation: the
    /// literal text would reach the model (the same drift /commit had).
    /// The template must instruct the model to run the git context commands
    /// itself instead — pin both halves so it cannot regress quietly.
    #[test]
    fn test_issue_prompt_has_no_bang_interpolation() {
        assert!(
            !ISSUE_PROMPT.contains("!`"),
            "issue template must not contain unimplemented !`...` interpolation"
        );
        assert!(
            ISSUE_PROMPT.contains("Run these git commands yourself first"),
            "issue template must tell the model to gather git context itself"
        );
        // Each formerly-interpolated command survives as an instruction.
        assert!(ISSUE_PROMPT.contains("`git status --short`"));
        assert!(ISSUE_PROMPT.contains("`git branch --show-current`"));
        assert!(ISSUE_PROMPT.contains("`git log --oneline -5`"));
    }
}
