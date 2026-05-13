//! /review command - Review local code changes

use crate::command::{Command, CommandBase, CommandSource, PromptCommand, ExecutionContext, CommandAvailability};

const REVIEW_PROMPT: &str = r##"
You are an expert code reviewer. Analyze local code changes and provide a structured review.

## Steps

1. Run `git diff --stat` to see what files changed.
2. Run `git diff` to get the full diff of unstaged changes.
3. Run `git diff --cached` to get staged changes.
4. If no changes found, run `git diff HEAD~1` to review the last commit.
5. Analyze all changes and provide a structured code review.

## Review Categories

Check each category and report issues found:
- **Code Correctness**: Logic errors, off-by-one bugs, null handling, race conditions, incorrect error handling
- **Style & Conventions**: Naming, formatting, project pattern adherence, dead code
- **Performance**: Unnecessary allocations, O(n^2) patterns, missing early returns, redundant clones
- **Security**: Input validation, injection risks, credential exposure, unsafe operations
- **Test Coverage**: Missing tests for new behavior, edge cases, assertion quality
- **Documentation**: Missing/wrong docs, broken examples, misleading comments

## Severity Levels

- **CRITICAL**: Will cause bugs, security vulnerabilities, or data loss
- **HIGH**: Likely to cause problems or significantly degrade quality
- **MEDIUM**: Should be fixed but not blocking
- **LOW**: Minor improvement, style nit
- **INFO**: Observation, no action required

## Output Format

### Summary
Brief summary of what changed and why.

### Issues
For each issue found:
- Severity level and category
- File and line location
- Description of the problem
- Suggested fix (code snippet if helpful)

### Positives
What the changes do well.

### Verdict
One of: **Ship it**, **Ship with notes**, **Fix before merging**
Brief justification.

Be concise. Skip categories with no issues. Focus on actionable feedback.
"##;

/// Create the /review command
pub fn command() -> Command {
    Command::Prompt(Box::new(PromptCommand {
        base: CommandBase {
            name: "review".to_string(),
            aliases: vec!["code-review".to_string()],
            description: "Review local code changes with AI analysis".to_string(),
            has_user_specified_description: false,
            availability: vec![CommandAvailability::All],
            source: CommandSource::Builtin,
            is_enabled: true,
            is_hidden: false,
            argument_hint: Some("[file or path]".to_string()),
            when_to_use: Some(
                "Review your local changes before committing or merging".to_string(),
            ),
            version: Some("0.1.0".to_string()),
            disable_model_invocation: false,
            user_invocable: true,
            is_workflow: false,
            immediate: false,
            is_sensitive: false,
            user_facing_name: None,
        },
        progress_message: "Reviewing local changes...".to_string(),
        content_length: 2000,
        arg_names: vec!["target".to_string()],
        allowed_tools: vec![
            "Bash(git diff:*)".to_string(),
            "Bash(git diff --stat:*)".to_string(),
            "Bash(git diff --cached:*)".to_string(),
            "Bash(git log:*)".to_string(),
            "Bash(git status:*)".to_string(),
            "Read".to_string(),
        ],
        model: None,
        hooks: std::collections::HashMap::new(),
        context: ExecutionContext::Inline,
        agent: None,
        paths: vec![],
        prompt_template: Some(REVIEW_PROMPT.to_string()),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_review_command() {
        let cmd = command();
        assert_eq!(cmd.name(), "review");
        assert!(cmd.aliases().contains(&"code-review".to_string()));
    }

    #[test]
    fn test_review_command_structure() {
        let cmd = command();
        assert!(!cmd.description().is_empty());
    }
}
