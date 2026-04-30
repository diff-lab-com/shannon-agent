//! GitHub integration tools using the `gh` CLI
//!
//! Provides implementations for:
//! - GhIssueList: List GitHub issues for the current repository
//! - GhIssueView: View a specific issue with comments
//! - GhPrCreate: Create a pull request
//! - GhPrList: List pull requests
//! - GhPrView: View a specific PR with diff

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use shannon_core::{Tool, ToolOutput, ToolResult};
use shannon_core::tools::ToolError;
use std::process::Command;

// ---------------------------------------------------------------------------
// Helper: run a gh command and capture output
// ---------------------------------------------------------------------------

/// Run a gh command and return stdout, stderr, exit status.
fn run_gh(args: &[&str]) -> Result<(String, String, bool), ToolError> {
    let output = Command::new("gh")
        .args(args)
        .output()
        .map_err(|e| ToolError::ExecutionFailed(format!("Failed to execute gh: {e}")))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    Ok((stdout, stderr, output.status.success()))
}

/// Check if gh is installed and authenticated.
fn check_gh_available() -> Result<(), ToolError> {
    let (_, _, success) = run_gh(&["--version"])
        .map_err(|e| ToolError::ExecutionFailed(format!("gh CLI not found: {e}. Please install from https://cli.github.com/")))?;

    if !success {
        return Err(ToolError::ExecutionFailed(
            "gh CLI command failed. Please ensure it is installed correctly.".to_string(),
        ));
    }

    // Check authentication
    let (_, _, success) = run_gh(&["auth", "status"])
        .map_err(|e| ToolError::ExecutionFailed(format!("Failed to check gh auth status: {e}")))?;

    if !success {
        return Err(ToolError::ExecutionFailed(
            "gh CLI is not authenticated. Please run `gh auth login`.".to_string(),
        ));
    }

    Ok(())
}

/// Check if the current directory is a git repository with a GitHub remote.
fn check_github_repo() -> Result<(), ToolError> {
    let output = Command::new("git")
        .args(&["remote", "-v"])
        .output()
        .map_err(|e| ToolError::ExecutionFailed(format!("Failed to check git remotes: {e}")))?;

    let stdout = String::from_utf8_lossy(&output.stdout);

    if !stdout.contains("github.com") {
        return Err(ToolError::ExecutionFailed(
            "Not a GitHub repository. No github.com remote found.".to_string(),
        ));
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Data structures for GitHub API responses
// ---------------------------------------------------------------------------

/// GitHub issue from API
#[derive(Debug, Clone, Deserialize, Serialize)]
struct GhIssue {
    number: u64,
    title: String,
    state: String,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    user: GhUser,
    #[serde(default)]
    comments: u64,
    #[serde(default)]
    created_at: String,
    #[serde(default)]
    updated_at: String,
    #[serde(default)]
    body: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
struct GhUser {
    #[serde(default)]
    login: String,
}

/// GitHub pull request from API
#[derive(Debug, Clone, Deserialize, Serialize)]
struct GhPullRequest {
    number: u64,
    title: String,
    state: String,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    user: GhUser,
    #[serde(default)]
    head: GhRef,
    #[serde(default)]
    base: GhRef,
    #[serde(default)]
    created_at: String,
    #[serde(default)]
    updated_at: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    mergeable: Option<bool>,
    #[serde(default)]
    review_decision: Option<String>,
    #[serde(default)]
    additions: u64,
    #[serde(default)]
    deletions: u64,
    #[serde(default)]
    changed_files: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
struct GhRef {
    #[serde(default)]
    ref_name: String,
    #[serde(default)]
    sha: String,
}

/// Issue comment from API
#[derive(Debug, Clone, Deserialize, Serialize)]
struct GhComment {
    #[serde(default)]
    id: u64,
    #[serde(default)]
    user: GhUser,
    #[serde(default)]
    created_at: String,
    #[serde(default)]
    updated_at: String,
    #[serde(default)]
    body: String,
}

// ---------------------------------------------------------------------------
// GhIssueList
// ---------------------------------------------------------------------------

/// Input for GhIssueList
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GhIssueListInput {
    /// Number of issues to return (default: 30)
    pub limit: Option<u64>,
    /// Filter by state: open, closed, all (default: open)
    pub state: Option<String>,
    /// Filter by assignee
    pub assignee: Option<String>,
    /// Filter by labels (comma-separated)
    pub labels: Option<String>,
}

/// Tool for listing GitHub issues
pub struct GhIssueListTool {
    description: String,
}

impl Default for GhIssueListTool {
    fn default() -> Self {
        Self::new()
    }
}

impl GhIssueListTool {
    pub fn new() -> Self {
        Self {
            description: "List GitHub issues for the current repository".to_string(),
        }
    }
}

#[async_trait]
impl Tool for GhIssueListTool {
    fn name(&self) -> &str {
        "gh_issue_list"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "limit": {
                    "type": "integer",
                    "description": "Number of issues to return (default: 30)",
                    "default": 30
                },
                "state": {
                    "type": "string",
                    "description": "Filter by state: open, closed, all",
                    "enum": ["open", "closed", "all"],
                    "default": "open"
                },
                "assignee": {
                    "type": "string",
                    "description": "Filter by assignee username"
                },
                "labels": {
                    "type": "string",
                    "description": "Filter by labels (comma-separated)"
                }
            }
        })
    }

    async fn execute(&self, input: Value) -> ToolResult<ToolOutput> {
        let input: GhIssueListInput = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("Invalid input: {e}")))?;

        check_gh_available()?;
        check_github_repo()?;

        let limit = input.limit.unwrap_or(30);
        let limit_str = limit.to_string();
        let state = input.state.as_deref().unwrap_or("open").to_string();

        let mut args = vec![
            "issue", "list",
            "--json", "number,title,state,htmlUrl,user,comments,createdAt,updatedAt,body",
            "--limit", &limit_str,
            "--state", &state,
        ];

        if let Some(assignee) = &input.assignee {
            args.extend(["--assignee", assignee]);
        }

        if let Some(labels) = &input.labels {
            args.extend(["--labels", labels]);
        }

        let (stdout, stderr, success) = run_gh(&args)?;

        if !success {
            return Ok(ToolOutput::error(format!("Failed to list issues: {stderr}")));
        }

        let issues: Vec<GhIssue> = serde_json::from_str(&stdout)
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to parse issues: {e}")))?;

        if issues.is_empty() {
            return Ok(ToolOutput::success("No issues found.".to_string()));
        }

        let mut content = String::new();
        for issue in &issues {
            content.push_str(&format!(
                "#{} {} - {}\n",
                issue.number,
                issue.title,
                issue.state
            ));
            content.push_str(&format!("  URL: {}\n", issue.html_url));
            content.push_str(&format!("  Author: {}\n", issue.user.login));
            content.push_str(&format!("  Comments: {}\n", issue.comments));
            content.push_str(&format!("  Created: {}\n", issue.created_at));
            if let Some(body) = &issue.body {
                let preview = body.lines().next().unwrap_or("");
                content.push_str(&format!("  Preview: {}{}\n", preview, if body.len() > 100 { "..." } else { "" }));
            }
            content.push('\n');
        }

        Ok(ToolOutput::success(content))
    }
}

// ---------------------------------------------------------------------------
// GhIssueView
// ---------------------------------------------------------------------------

/// Input for GhIssueView
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GhIssueViewInput {
    /// Issue number
    pub number: u64,
    /// Include comments (default: true)
    pub include_comments: Option<bool>,
}

/// Tool for viewing a specific GitHub issue
pub struct GhIssueViewTool {
    description: String,
}

impl Default for GhIssueViewTool {
    fn default() -> Self {
        Self::new()
    }
}

impl GhIssueViewTool {
    pub fn new() -> Self {
        Self {
            description: "View a specific GitHub issue with comments".to_string(),
        }
    }
}

#[async_trait]
impl Tool for GhIssueViewTool {
    fn name(&self) -> &str {
        "gh_issue_view"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["number"],
            "properties": {
                "number": {
                    "type": "integer",
                    "description": "Issue number"
                },
                "include_comments": {
                    "type": "boolean",
                    "description": "Include comments (default: true)",
                    "default": true
                }
            }
        })
    }

    async fn execute(&self, input: Value) -> ToolResult<ToolOutput> {
        let input: GhIssueViewInput = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("Invalid input: {e}")))?;

        check_gh_available()?;
        check_github_repo()?;

        let include_comments = input.include_comments.unwrap_or(true);

        // Get issue details
        let issue_args = [
            "issue", "view", &input.number.to_string(),
            "--json", "number,title,state,htmlUrl,user,comments,createdAt,updatedAt,body"
        ];

        let (stdout, stderr, success) = run_gh(&issue_args)?;

        if !success {
            return Ok(ToolOutput::error(format!("Failed to view issue: {stderr}")));
        }

        let issue: GhIssue = serde_json::from_str(&stdout)
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to parse issue: {e}")))?;

        let mut content = format!(
            "#{}: {}\n\n",
            issue.number, issue.title
        );
        content.push_str(&format!("State: {}\n", issue.state));
        content.push_str(&format!("URL: {}\n", issue.html_url));
        content.push_str(&format!("Author: {}\n", issue.user.login));
        content.push_str(&format!("Created: {}\n", issue.created_at));
        content.push_str(&format!("Updated: {}\n", issue.updated_at));
        content.push_str(&format!("Comments: {}\n\n", issue.comments));

        if let Some(body) = &issue.body {
            content.push_str(&format!("{}\n\n", body));
        }

        // Get comments if requested
        if include_comments && issue.comments > 0 {
            let comments_args = [
                "issue", "view", &input.number.to_string(),
                "--json", "comments", "--jq", ".comments"
            ];

            let (comments_stdout, _comments_stderr, comments_success) = run_gh(&comments_args)?;

            if comments_success {
                if let Ok(comments) = serde_json::from_str::<Vec<GhComment>>(&comments_stdout) {
                    content.push_str("--- Comments ---\n\n");
                    for comment in &comments {
                        content.push_str(&format!(
                            "{} @{}\n\n{}\n\n",
                            comment.created_at, comment.user.login, comment.body
                        ));
                    }
                }
            }
        }

        Ok(ToolOutput::success(content))
    }
}

// ---------------------------------------------------------------------------
// GhPrCreate
// ---------------------------------------------------------------------------

/// Input for GhPrCreate
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GhPrCreateInput {
    /// PR title
    pub title: String,
    /// Branch to create PR from
    pub head: Option<String>,
    /// Branch to merge into (default: main branch)
    pub base: Option<String>,
    /// PR body/description
    pub body: Option<String>,
    /// Mark as draft
    pub draft: Option<bool>,
    /// Add labels (comma-separated)
    pub labels: Option<String>,
    /// Add assignees (comma-separated)
    pub assignees: Option<String>,
}

/// Tool for creating a pull request
pub struct GhPrCreateTool {
    description: String,
}

impl Default for GhPrCreateTool {
    fn default() -> Self {
        Self::new()
    }
}

impl GhPrCreateTool {
    pub fn new() -> Self {
        Self {
            description: "Create a pull request on GitHub".to_string(),
        }
    }
}

#[async_trait]
impl Tool for GhPrCreateTool {
    fn name(&self) -> &str {
        "gh_pr_create"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["title"],
            "properties": {
                "title": {
                    "type": "string",
                    "description": "Pull request title"
                },
                "head": {
                    "type": "string",
                    "description": "Branch to create PR from (default: current branch)"
                },
                "base": {
                    "type": "string",
                    "description": "Branch to merge into (default: repository default branch)"
                },
                "body": {
                    "type": "string",
                    "description": "Pull request description"
                },
                "draft": {
                    "type": "boolean",
                    "description": "Create as draft PR",
                    "default": false
                },
                "labels": {
                    "type": "string",
                    "description": "Add labels (comma-separated)"
                },
                "assignees": {
                    "type": "string",
                    "description": "Add assignees (comma-separated)"
                }
            }
        })
    }

    async fn execute(&self, input: Value) -> ToolResult<ToolOutput> {
        let input: GhPrCreateInput = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("Invalid input: {e}")))?;

        check_gh_available()?;
        check_github_repo()?;

        let mut args = vec!["pr", "create", "--title", &input.title];

        if let Some(head) = &input.head {
            args.extend(["--head", head]);
        }

        if let Some(base) = &input.base {
            args.extend(["--base", base]);
        }

        if let Some(body) = &input.body {
            args.extend(["--body", body]);
        }

        if input.draft.unwrap_or(false) {
            args.push("--draft");
        }

        if let Some(labels) = &input.labels {
            args.extend(["--label", labels]);
        }

        if let Some(assignees) = &input.assignees {
            for assignee in assignees.split(',') {
                args.extend(["--assignee", assignee.trim()]);
            }
        }

        let (stdout, stderr, success) = run_gh(&args)?;

        if !success {
            return Ok(ToolOutput::error(format!("Failed to create PR: {stderr}")));
        }

        Ok(ToolOutput::success(format!("Pull request created:\n{stdout}")))
    }
}

// ---------------------------------------------------------------------------
// GhPrList
// ---------------------------------------------------------------------------

/// Input for GhPrList
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GhPrListInput {
    /// Number of PRs to return (default: 30)
    pub limit: Option<u64>,
    /// Filter by state: open, closed, merged, all (default: open)
    pub state: Option<String>,
    /// Filter by author
    pub author: Option<String>,
}

/// Tool for listing pull requests
pub struct GhPrListTool {
    description: String,
}

impl Default for GhPrListTool {
    fn default() -> Self {
        Self::new()
    }
}

impl GhPrListTool {
    pub fn new() -> Self {
        Self {
            description: "List pull requests for the current repository".to_string(),
        }
    }
}

#[async_trait]
impl Tool for GhPrListTool {
    fn name(&self) -> &str {
        "gh_pr_list"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "limit": {
                    "type": "integer",
                    "description": "Number of PRs to return (default: 30)",
                    "default": 30
                },
                "state": {
                    "type": "string",
                    "description": "Filter by state: open, closed, merged, all",
                    "enum": ["open", "closed", "merged", "all"],
                    "default": "open"
                },
                "author": {
                    "type": "string",
                    "description": "Filter by author username"
                }
            }
        })
    }

    async fn execute(&self, input: Value) -> ToolResult<ToolOutput> {
        let input: GhPrListInput = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("Invalid input: {e}")))?;

        check_gh_available()?;
        check_github_repo()?;

        let limit = input.limit.unwrap_or(30);
        let limit_str = limit.to_string();
        let state = input.state.as_deref().unwrap_or("open").to_string();

        let mut args = vec![
            "pr", "list",
            "--json", "number,title,state,htmlUrl,user,head,base,createdAt,updatedAt,body,mergeable,reviewDecision,additions,deletions,changedFiles",
            "--limit", &limit_str,
            "--state", &state,
        ];

        if let Some(author) = &input.author {
            args.extend(["--author", author]);
        }

        let (stdout, stderr, success) = run_gh(&args)?;

        if !success {
            return Ok(ToolOutput::error(format!("Failed to list PRs: {stderr}")));
        }

        let prs: Vec<GhPullRequest> = serde_json::from_str(&stdout)
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to parse PRs: {e}")))?;

        if prs.is_empty() {
            return Ok(ToolOutput::success("No pull requests found.".to_string()));
        }

        let mut content = String::new();
        for pr in &prs {
            content.push_str(&format!(
                "#{} {} - {}\n",
                pr.number, pr.title, pr.state
            ));
            content.push_str(&format!("  URL: {}\n", pr.html_url));
            content.push_str(&format!("  Author: {}\n", pr.user.login));
            content.push_str(&format!("  Branch: {} → {}\n", pr.head.ref_name, pr.base.ref_name));
            content.push_str(&format!("  Changes: +{} -{} ({} files)\n", pr.additions, pr.deletions, pr.changed_files));

            if let Some(mergeable) = pr.mergeable {
                content.push_str(&format!("  Mergeable: {}\n", mergeable));
            }

            if let Some(review_decision) = &pr.review_decision {
                if !review_decision.is_empty() {
                    content.push_str(&format!("  Review: {}\n", review_decision));
                }
            }

            content.push('\n');
        }

        Ok(ToolOutput::success(content))
    }
}

// ---------------------------------------------------------------------------
// GhPrView
// ---------------------------------------------------------------------------

/// Input for GhPrView
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GhPrViewInput {
    /// PR number
    pub number: u64,
    /// Include diff (default: false)
    pub include_diff: Option<bool>,
}

/// Tool for viewing a specific pull request
pub struct GhPrViewTool {
    description: String,
}

impl Default for GhPrViewTool {
    fn default() -> Self {
        Self::new()
    }
}

impl GhPrViewTool {
    pub fn new() -> Self {
        Self {
            description: "View a specific pull request with optional diff".to_string(),
        }
    }
}

#[async_trait]
impl Tool for GhPrViewTool {
    fn name(&self) -> &str {
        "gh_pr_view"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["number"],
            "properties": {
                "number": {
                    "type": "integer",
                    "description": "Pull request number"
                },
                "include_diff": {
                    "type": "boolean",
                    "description": "Include diff in output (default: false)",
                    "default": false
                }
            }
        })
    }

    async fn execute(&self, input: Value) -> ToolResult<ToolOutput> {
        let input: GhPrViewInput = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("Invalid input: {e}")))?;

        check_gh_available()?;
        check_github_repo()?;

        let include_diff = input.include_diff.unwrap_or(false);

        // Get PR details
        let pr_args = [
            "pr", "view", &input.number.to_string(),
            "--json", "number,title,state,htmlUrl,user,head,base,createdAt,updatedAt,body,mergeable,reviewDecision,additions,deletions,changedFiles"
        ];

        let (stdout, stderr, success) = run_gh(&pr_args)?;

        if !success {
            return Ok(ToolOutput::error(format!("Failed to view PR: {stderr}")));
        }

        let pr: GhPullRequest = serde_json::from_str(&stdout)
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to parse PR: {e}")))?;

        let mut content = format!(
            "#{}: {}\n\n",
            pr.number, pr.title
        );
        content.push_str(&format!("State: {}\n", pr.state));
        content.push_str(&format!("URL: {}\n", pr.html_url));
        content.push_str(&format!("Author: {}\n", pr.user.login));
        content.push_str(&format!("Branch: {} → {}\n", pr.head.ref_name, pr.base.ref_name));
        content.push_str(&format!("Created: {}\n", pr.created_at));
        content.push_str(&format!("Updated: {}\n", pr.updated_at));
        content.push_str(&format!("Changes: +{} -{} ({} files)\n", pr.additions, pr.deletions, pr.changed_files));

        if let Some(mergeable) = pr.mergeable {
            content.push_str(&format!("Mergeable: {}\n", mergeable));
        }

        if let Some(review_decision) = &pr.review_decision {
            if !review_decision.is_empty() {
                content.push_str(&format!("Review: {}\n", review_decision));
            }
        }

        content.push('\n');

        if let Some(body) = &pr.body {
            content.push_str(&format!("{}\n\n", body));
        }

        // Get diff if requested
        if include_diff {
            let diff_args = ["pr", "diff", &input.number.to_string()];
            let (diff_stdout, diff_stderr, diff_success) = run_gh(&diff_args)?;

            if diff_success {
                content.push_str("--- Diff ---\n\n");
                content.push_str(&diff_stdout);
            } else {
                content.push_str(&format!("Failed to load diff: {}\n", diff_stderr));
            }
        }

        Ok(ToolOutput::success(content))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gh_issue_list_input_schema() {
        let tool = GhIssueListTool::new();
        let schema = tool.input_schema();

        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["limit"].is_object());
        assert_eq!(schema["properties"]["limit"]["default"], 30);
    }

    #[test]
    fn test_gh_issue_view_input_schema() {
        let tool = GhIssueViewTool::new();
        let schema = tool.input_schema();

        assert_eq!(schema["type"], "object");
        assert!(schema["required"].is_array());
        assert_eq!(schema["required"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_gh_pr_create_input_schema() {
        let tool = GhPrCreateTool::new();
        let schema = tool.input_schema();

        assert_eq!(schema["type"], "object");
        assert!(schema["required"].is_array());
        assert_eq!(schema["properties"]["title"]["description"], "Pull request title");
    }

    #[test]
    fn test_tool_names() {
        assert_eq!(GhIssueListTool::new().name(), "gh_issue_list");
        assert_eq!(GhIssueViewTool::new().name(), "gh_issue_view");
        assert_eq!(GhPrCreateTool::new().name(), "gh_pr_create");
        assert_eq!(GhPrListTool::new().name(), "gh_pr_list");
        assert_eq!(GhPrViewTool::new().name(), "gh_pr_view");
    }

    #[test]
    fn test_deserialize_issue_list_input() {
        let json = json!({
            "limit": 50,
            "state": "closed",
            "labels": "bug,enhancement"
        });

        let input: GhIssueListInput = serde_json::from_value(json).unwrap();
        assert_eq!(input.limit, Some(50));
        assert_eq!(input.state, Some("closed".to_string()));
        assert_eq!(input.labels, Some("bug,enhancement".to_string()));
    }

    #[test]
    fn test_deserialize_pr_create_input() {
        let json = json!({
            "title": "My PR",
            "head": "feature-branch",
            "base": "main",
            "draft": true
        });

        let input: GhPrCreateInput = serde_json::from_value(json).unwrap();
        assert_eq!(input.title, "My PR");
        assert_eq!(input.head, Some("feature-branch".to_string()));
        assert_eq!(input.base, Some("main".to_string()));
        assert_eq!(input.draft, Some(true));
    }
}
