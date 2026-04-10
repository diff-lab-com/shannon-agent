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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

    /// Icon/emoji for visual display
    pub fn icon(&self) -> &'static str {
        match self {
            ReviewCategory::Correctness => "✓",
            ReviewCategory::Style => "📐",
            ReviewCategory::Performance => "⚡",
            ReviewCategory::Security => "🔒",
            ReviewCategory::Testing => "🧪",
            ReviewCategory::Documentation => "📝",
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

impl ReviewIssue {
    /// Create a new review issue
    pub fn new(category: ReviewCategory, severity: IssueSeverity, description: String) -> Self {
        Self {
            category,
            severity,
            location: None,
            description,
            suggestion: None,
        }
    }

    /// Set the file location
    pub fn with_location(mut self, location: String) -> Self {
        self.location = Some(location);
        self
    }

    /// Set the suggestion
    pub fn with_suggestion(mut self, suggestion: String) -> Self {
        self.suggestion = Some(suggestion);
        self
    }

    /// Format as a single-line summary
    pub fn to_summary(&self) -> String {
        let loc = self.location.as_deref().unwrap_or("(general)");
        format!(
            "{} [{}] {} {}: {}",
            self.severity.indicator(),
            self.severity.display_name(),
            self.category.icon(),
            loc,
            self.description
        )
    }
}

/// Issue severity levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum IssueSeverity {
    Critical,
    High,
    Medium,
    Low,
    Info,
}

impl IssueSeverity {
    /// Human-readable label
    pub fn display_name(&self) -> &'static str {
        match self {
            IssueSeverity::Critical => "CRITICAL",
            IssueSeverity::High => "HIGH",
            IssueSeverity::Medium => "MEDIUM",
            IssueSeverity::Low => "LOW",
            IssueSeverity::Info => "INFO",
        }
    }

    /// Visual indicator for terminal display
    pub fn indicator(&self) -> &'static str {
        match self {
            IssueSeverity::Critical => "🔴",
            IssueSeverity::High => "🟠",
            IssueSeverity::Medium => "🟡",
            IssueSeverity::Low => "🟢",
            IssueSeverity::Info => "ℹ️",
        }
    }
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

impl ReviewResult {
    /// Create a new review result
    pub fn new(overview: String, assessment: Assessment) -> Self {
        Self {
            pr_number: None,
            overview,
            issues: Vec::new(),
            positives: Vec::new(),
            overall_assessment: assessment,
        }
    }

    /// Set the PR number
    pub fn with_pr_number(mut self, pr: String) -> Self {
        self.pr_number = Some(pr);
        self
    }

    /// Add an issue
    pub fn with_issue(mut self, issue: ReviewIssue) -> Self {
        self.issues.push(issue);
        self
    }

    /// Add a positive finding
    pub fn with_positive(mut self, positive: String) -> Self {
        self.positives.push(positive);
        self
    }

    /// Count issues by severity
    pub fn count_by_severity(&self, severity: IssueSeverity) -> usize {
        self.issues.iter().filter(|i| i.severity == severity).count()
    }

    /// Count issues by category
    pub fn count_by_category(&self, category: ReviewCategory) -> usize {
        self.issues.iter().filter(|i| i.category == category).count()
    }

    /// Format the review result as a markdown report
    pub fn to_markdown(&self) -> String {
        let mut md = String::new();

        // Header
        md.push_str(&format!(
            "{} PR Review: {}\n\n",
            self.overall_assessment.indicator(),
            self.overall_assessment.display_name()
        ));

        if let Some(pr) = &self.pr_number {
            md.push_str(&format!("**PR #{}**\n\n", pr));
        }

        // Overview
        md.push_str(&format!("## Overview\n\n{}\n\n", self.overview));

        // Issues by severity (highest first)
        if !self.issues.is_empty() {
            md.push_str("## Issues\n\n");

            let mut sorted_issues = self.issues.clone();
            sorted_issues.sort_by(|a, b| a.severity.cmp(&b.severity));

            for issue in &sorted_issues {
                md.push_str(&format!(
                    "- {} **[{}] {}** ({}): {}",
                    issue.severity.indicator(),
                    issue.severity.display_name(),
                    issue.category.display_name(),
                    issue.location.as_deref().unwrap_or("general"),
                    issue.description,
                ));
                if let Some(suggestion) = &issue.suggestion {
                    md.push_str(&format!("\n  - Suggestion: {}", suggestion));
                }
                md.push('\n');
            }
            md.push('\n');
        }

        // Positives
        if !self.positives.is_empty() {
            md.push_str("## Positives\n\n");
            for positive in &self.positives {
                md.push_str(&format!("- {}\n", positive));
            }
            md.push('\n');
        }

        // Summary
        md.push_str(&format!(
            "**Assessment:** {} {}\n",
            self.overall_assessment.indicator(),
            self.overall_assessment.display_name()
        ));
        if !self.issues.is_empty() {
            md.push_str(&format!("**Issues:** {} total\n", self.issues.len()));
        }

        md
    }
}

/// Overall assessment rating
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Assessment {
    Approve,
    ApproveWithSuggestions,
    RequestChanges,
    NeedsWork,
}

impl Assessment {
    /// Human-readable label
    pub fn display_name(&self) -> &'static str {
        match self {
            Assessment::Approve => "Approve",
            Assessment::ApproveWithSuggestions => "Approve with Suggestions",
            Assessment::RequestChanges => "Request Changes",
            Assessment::NeedsWork => "Needs Work",
        }
    }

    /// Visual indicator
    pub fn indicator(&self) -> &'static str {
        match self {
            Assessment::Approve => "✅",
            Assessment::ApproveWithSuggestions => "👍",
            Assessment::RequestChanges => "🔄",
            Assessment::NeedsWork => "⚠️",
        }
    }
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

    #[test]
    fn test_category_icons() {
        assert!(!ReviewCategory::Security.icon().is_empty());
        assert!(!ReviewCategory::Testing.icon().is_empty());
    }

    #[test]
    fn test_issue_severity_display() {
        assert_eq!(IssueSeverity::Critical.display_name(), "CRITICAL");
        assert_eq!(IssueSeverity::Medium.display_name(), "MEDIUM");
        assert_eq!(IssueSeverity::Info.display_name(), "INFO");
    }

    #[test]
    fn test_issue_severity_ordering() {
        assert!(IssueSeverity::Critical < IssueSeverity::High);
        assert!(IssueSeverity::High < IssueSeverity::Medium);
        assert!(IssueSeverity::Medium < IssueSeverity::Low);
        assert!(IssueSeverity::Low < IssueSeverity::Info);
    }

    #[test]
    fn test_assessment_variants() {
        assert_eq!(Assessment::Approve.display_name(), "Approve");
        assert_eq!(Assessment::RequestChanges.display_name(), "Request Changes");
        assert_eq!(Assessment::NeedsWork.display_name(), "Needs Work");
        assert!(!Assessment::Approve.indicator().is_empty());
    }

    #[test]
    fn test_review_issue_builder() {
        let issue = ReviewIssue::new(
            ReviewCategory::Security,
            IssueSeverity::High,
            "SQL injection vulnerability".to_string(),
        )
        .with_location("src/db.rs:42".to_string())
        .with_suggestion("Use parameterized queries".to_string());

        assert_eq!(issue.category, ReviewCategory::Security);
        assert_eq!(issue.severity, IssueSeverity::High);
        assert_eq!(issue.location, Some("src/db.rs:42".to_string()));
        assert!(issue.suggestion.is_some());
    }

    #[test]
    fn test_review_issue_summary() {
        let issue = ReviewIssue::new(
            ReviewCategory::Performance,
            IssueSeverity::Medium,
            "N+1 query pattern".to_string(),
        )
        .with_location("src/api.rs:100".to_string());

        let summary = issue.to_summary();
        assert!(summary.contains("MEDIUM"));
        assert!(summary.contains("api.rs:100"));
        assert!(summary.contains("N+1 query"));
    }

    #[test]
    fn test_review_result_builder() {
        let result = ReviewResult::new(
            "Adds user authentication".to_string(),
            Assessment::ApproveWithSuggestions,
        )
        .with_pr_number("42".to_string())
        .with_issue(ReviewIssue::new(
            ReviewCategory::Style,
            IssueSeverity::Low,
            "Missing doc comment".to_string(),
        ))
        .with_positive("Good test coverage".to_string());

        assert_eq!(result.pr_number, Some("42".to_string()));
        assert_eq!(result.issues.len(), 1);
        assert_eq!(result.positives.len(), 1);
    }

    #[test]
    fn test_review_result_count_by_severity() {
        let result = ReviewResult::new("Overview".to_string(), Assessment::NeedsWork)
            .with_issue(ReviewIssue::new(ReviewCategory::Security, IssueSeverity::Critical, "CVE".to_string()))
            .with_issue(ReviewIssue::new(ReviewCategory::Security, IssueSeverity::High, "XSS".to_string()))
            .with_issue(ReviewIssue::new(ReviewCategory::Style, IssueSeverity::Low, "Fmt".to_string()));

        assert_eq!(result.count_by_severity(IssueSeverity::Critical), 1);
        assert_eq!(result.count_by_severity(IssueSeverity::High), 1);
        assert_eq!(result.count_by_severity(IssueSeverity::Low), 1);
        assert_eq!(result.count_by_severity(IssueSeverity::Medium), 0);
    }

    #[test]
    fn test_review_result_count_by_category() {
        let result = ReviewResult::new("Overview".to_string(), Assessment::RequestChanges)
            .with_issue(ReviewIssue::new(ReviewCategory::Security, IssueSeverity::Critical, "A".to_string()))
            .with_issue(ReviewIssue::new(ReviewCategory::Security, IssueSeverity::High, "B".to_string()))
            .with_issue(ReviewIssue::new(ReviewCategory::Testing, IssueSeverity::Medium, "C".to_string()));

        assert_eq!(result.count_by_category(ReviewCategory::Security), 2);
        assert_eq!(result.count_by_category(ReviewCategory::Testing), 1);
        assert_eq!(result.count_by_category(ReviewCategory::Style), 0);
    }

    #[test]
    fn test_review_result_to_markdown() {
        let result = ReviewResult::new(
            "Adds new feature".to_string(),
            Assessment::ApproveWithSuggestions,
        )
        .with_pr_number("99".to_string())
        .with_issue(ReviewIssue::new(
            ReviewCategory::Testing,
            IssueSeverity::Medium,
            "Missing edge case test".to_string(),
        ).with_suggestion("Add test for empty input".to_string()))
        .with_positive("Clean code structure".to_string());

        let md = result.to_markdown();
        assert!(md.contains("PR #99"));
        assert!(md.contains("Adds new feature"));
        assert!(md.contains("Missing edge case test"));
        assert!(md.contains("Add test for empty input"));
        assert!(md.contains("Clean code structure"));
        assert!(md.contains("Approve with Suggestions"));
        assert!(md.contains("Issues:** 1 total"));
    }

    #[test]
    fn test_review_result_markdown_no_issues() {
        let result = ReviewResult::new(
            "Simple fix".to_string(),
            Assessment::Approve,
        )
        .with_positive("Fixes the bug correctly".to_string());

        let md = result.to_markdown();
        assert!(md.contains("Approve"));
        assert!(!md.contains("## Issues"));
        assert!(md.contains("Positives"));
    }
}
