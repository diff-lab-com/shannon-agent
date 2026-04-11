//! /commit command - Create git commits

use crate::command::{Command, CommandBase, CommandSource, PromptCommand, ExecutionContext, CommandAvailability};

/// Git safety protocol text
const GIT_SAFETY: &str = r##"
## Git Safety Protocol

- NEVER update the git config
- NEVER skip hooks (--no-verify, --no-gpg-sign, etc) unless the user explicitly requests it
- CRITICAL: ALWAYS create NEW commits. NEVER use git commit --amend, unless the user explicitly requests it
- Do not commit files that likely contain secrets (.env, credentials.json, etc). Warn the user if they specifically request to commit those files
- If there are no changes to commit (i.e., no untracked files and no modifications), do not create an empty commit
- Never use git commands with the -i flag (like git rebase -i or git add -i) since they require interactive input which is not supported
"##;

/// Commit attribution template
const COMMIT_ATTRIBUTION: &str = "\n\nCo-Authored-By: Shannon Code <noreply@shannon.dev>";

/// Create the /commit command
pub fn command() -> Command {
    Command::Prompt(PromptCommand {
        base: CommandBase {
            name: "commit".to_string(),
            aliases: vec!["ci".to_string()],
            description: "Create a git commit with AI-generated message".to_string(),
            has_user_specified_description: false,
            availability: vec![CommandAvailability::All],
            source: CommandSource::Builtin,
            is_enabled: true,
            is_hidden: false,
            argument_hint: Some("[optional instructions]".to_string()),
            when_to_use: Some(
                "Use after making changes to stage and commit them with an appropriate message".to_string(),
            ),
            version: Some("0.1.0".to_string()),
            disable_model_invocation: false,
            user_invocable: true,
            is_workflow: false,
            immediate: false,
            is_sensitive: false,
            user_facing_name: None,
        },
        progress_message: "Creating commit...".to_string(),
        content_length: 2000,
        arg_names: vec!["instructions".to_string()],
        allowed_tools: vec![
            "Bash(git add:*)".to_string(),
            "Bash(git status:*)".to_string(),
            "Bash(git commit:*)".to_string(),
            "Bash(git log:*)".to_string(),
            "Bash(git diff:*)".to_string(),
            "Bash(git branch:*)".to_string(),
        ],
        model: None,
        hooks: std::collections::HashMap::new(),
        context: ExecutionContext::Inline,
        agent: None,
        paths: vec![],
    })
}

/// Get the prompt template for the commit command
pub fn get_prompt_template(_default_branch: &str, attribution: bool) -> String {
    let attribution_text = if attribution { COMMIT_ATTRIBUTION } else { "" };

    format!(
        r##"## Context

- Current git status: !`git status`
- Current git diff (staged and unstaged changes): !`git diff HEAD`
- Current branch: !`git branch --show-current`
- Recent commits: !`git log --oneline -10`
{GIT_SAFETY}
## Your task

Based on the above changes, create a single git commit:

1. Analyze all staged changes and draft a commit message:
   - Look at the recent commits above to follow this repository's commit message style
   - Summarize the nature of the changes (new feature, enhancement, bug fix, refactoring, test, docs, etc.)
   - Ensure the message accurately reflects the changes and their purpose (i.e. "add" means a wholly new feature, "update" means an enhancement to an existing feature, "fix" means a bug fix, etc.)
   - Draft a concise (1-2 sentences) commit message that focuses on the "why" rather than the "what"

2. Stage relevant files and create the commit using HEREDOC syntax:
```bash
git commit -m "$(cat <<'EOF'
Commit message here.{attribution}
EOF
)"
```

You have the capability to call multiple tools in a single response. Stage and create the commit using a single message. Do not use any other tools or do anything else. Do not send any other text or messages besides these tool calls."##,
        attribution = attribution_text
    )
}

/// Get default git branch
pub fn get_default_branch() -> &'static str {
    "main"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_commit_command() {
        let cmd = command();
        assert_eq!(cmd.name(), "commit");
        assert_eq!(cmd.aliases(), &["ci".to_string()]);
    }

    #[test]
    fn test_get_prompt_template() {
        let prompt = get_prompt_template("main", true);
        assert!(prompt.contains("Git Safety Protocol"));
        assert!(prompt.contains("Co-Authored-By"));
    }

    #[test]
    fn test_get_prompt_template_no_attribution() {
        let prompt = get_prompt_template("main", false);
        assert!(prompt.contains("Git Safety Protocol"));
        assert!(!prompt.contains("Co-Authored-By"));
    }
}
