//! /review-pr command - Review pull requests

use crate::command::{Command, CommandBase, CommandSource, PromptCommand, ExecutionContext, CommandAvailability};

/// PR review prompt template
const REVIEW_PROMPT: &str = r##"
You are an expert code reviewer. Follow these steps:

1. If no PR number is provided in the args, run `gh pr list` to show open PRs
2. If a PR number is provided, run `gh pr view <number>` to get PR details
3. Run `gh pr diff <number>` to get the diff
4. Analyze the changes and provide a thorough code review that includes:
   - Overview of what the PR does
   - Analysis of code quality and style
   - Specific suggestions for improvements
   - Any potential issues or risks

Keep your review concise but thorough. Focus on:
- Code correctness
- Following project conventions
- Performance implications
- Test coverage
- Security considerations

Format your review with clear sections and bullet points.
"##;

/// Create the /review-pr command
pub fn command() -> Command {
    Command::Prompt(PromptCommand {
        base: CommandBase {
            name: "review-pr".to_string(),
            aliases: vec!["pr-review".to_string(), "ultrareview".to_string()],
            description: "Review a pull request with AI analysis".to_string(),
            has_user_specified_description: false,
            availability: vec![CommandAvailability::All],
            source: CommandSource::Builtin,
            is_enabled: true,
            is_hidden: false,
            argument_hint: Some("[PR number]".to_string()),
            when_to_use: Some(
                "Use to review code changes before merging. Can be triggered by users or models".to_string(),
            ),
            version: Some("0.1.0".to_string()),
            disable_model_invocation: false,
            user_invocable: true,
            is_workflow: false,
            immediate: false,
            is_sensitive: false,
            user_facing_name: None,
        },
        progress_message: "Reviewing pull request...".to_string(),
        content_length: 1500,
        arg_names: vec!["pr_number".to_string()],
        allowed_tools: vec![
            "Bash(gh pr view:*)".to_string(),
            "Bash(gh pr diff:*)".to_string(),
            "Bash(gh pr list:*)".to_string(),
            "Bash(gh pr checks:*)".to_string(),
        ],
        model: None,
        hooks: std::collections::HashMap::new(),
        context: ExecutionContext::Inline,
        agent: None,
        paths: vec![],
    })
}

/// Get the review prompt with PR number
pub fn get_review_prompt(pr_number: Option<&str>) -> String {
    let pr_info = if let Some(number) = pr_number {
        format!("PR number: {}", number)
    } else {
        "No PR number provided - will list open PRs".to_string()
    };

    format!("{}\n\n{}", REVIEW_PROMPT, pr_info)
}

/// Review category
#[derive(Debug, Clone, Copy)]
pub enum ReviewCategory {
    Correctness,
    Style,
    Performance,
    Security,
    Testing,
    Documentation,
}

impl ReviewCategory {
    pub fn all() -> &'static [ReviewCategory] {
        &[
            ReviewCategory::Correctness,
            ReviewCategory::Style,
            ReviewCategory::Performance,
            ReviewCategory::Security,
            ReviewCategory::Testing,
            ReviewCategory::Documentation,
        ]
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            ReviewCategory::Correctness => "Code Correctness",
            ReviewCategory::Style => "Style & Conventions",
            ReviewCategory::Performance => "Performance",
            ReviewCategory::Security => "Security",
            ReviewCategory::Testing => "Test Coverage",
            ReviewCategory::Documentation => "Documentation",
        }
    }
}

/// Review issue with severity
#[derive(Debug, Clone)]
pub struct ReviewIssue {
    pub category: ReviewCategory,
    pub severity: IssueSeverity,
    pub location: Option<String>,
    pub description: String,
    pub suggestion: Option<String>,
}

/// Issue severity levels
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueSeverity {
    Critical,
    High,
    Medium,
    Low,
    Info,
}

/// Structured review result
#[derive(Debug, Clone)]
pub struct ReviewResult {
    pub pr_number: Option<String>,
    pub overview: String,
    pub issues: Vec<ReviewIssue>,
    pub positives: Vec<String>,
    pub overall_assessment: Assessment,
}

/// Overall assessment rating
#[derive(Debug, Clone, Copy)]
pub enum Assessment {
    Approve,
    ApproveWithSuggestions,
    RequestChanges,
    NeedsWork,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_review_pr_command() {
        let cmd = command();
        assert_eq!(cmd.name(), "review-pr");
        assert!(cmd.aliases().contains(&"pr-review".to_string()));
    }

    #[test]
    fn test_get_review_prompt() {
        let prompt = get_review_prompt(Some("123"));
        assert!(prompt.contains("123"));

        let prompt_no_pr = get_review_prompt(None);
        assert!(prompt_no_pr.contains("No PR number provided"));
    }

    #[test]
    fn test_review_categories() {
        let categories = ReviewCategory::all();
        assert_eq!(categories.len(), 6);
    }

    #[test]
    fn test_assessment_display() {
        assert_eq!(ReviewCategory::Correctness.display_name(), "Code Correctness");
        assert_eq!(ReviewCategory::Security.display_name(), "Security");
    }
}
