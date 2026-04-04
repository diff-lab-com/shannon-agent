//! Structured result summarization for sub-agent execution results.
//!
//! Provides:
//! - `AgentExecutionSummary`: Rich summary of a single agent's execution
//! - `SummaryGenerator`: Stateless service that produces summaries from `ToolOutput` slices
//! - `SuccessMetrics`: Aggregate statistics across multiple agent runs

use serde::{Deserialize, Serialize};
use serde_json::Value;
use shannon_core::tools::ToolOutput;
use std::collections::{HashMap, HashSet};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Outcome of an agent execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SummaryStatus {
    /// All tool calls succeeded with no errors.
    Success,
    /// Some tool calls succeeded but others produced errors.
    PartialSuccess,
    /// Every tool call returned an error.
    Failed,
    /// The agent was terminated due to a turn limit.
    Timeout,
}

impl std::fmt::Display for SummaryStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SummaryStatus::Success => write!(f, "success"),
            SummaryStatus::PartialSuccess => write!(f, "partial_success"),
            SummaryStatus::Failed => write!(f, "failed"),
            SummaryStatus::Timeout => write!(f, "timeout"),
        }
    }
}

/// Structured summary of a single agent execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentExecutionSummary {
    pub agent_name: String,
    pub task_description: String,
    pub status: SummaryStatus,
    pub duration_ms: u64,
    pub files_modified: Vec<String>,
    pub files_created: Vec<String>,
    pub tools_used: Vec<String>,
    pub errors: Vec<String>,
    pub key_findings: Vec<String>,
    pub recommendations: Vec<String>,
    pub metadata: HashMap<String, Value>,
}

/// Aggregate success metrics across multiple agent summaries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuccessMetrics {
    pub total_agents: usize,
    pub successful: usize,
    pub partial: usize,
    pub failed: usize,
    pub timed_out: usize,
    pub total_files_modified: usize,
    pub total_files_created: usize,
    pub total_errors: usize,
    pub success_rate: f64,
}

// ---------------------------------------------------------------------------
// SummaryGenerator
// ---------------------------------------------------------------------------

/// Stateless summary generation service.
pub struct SummaryGenerator;

impl SummaryGenerator {
    // -----------------------------------------------------------------------
    // Public API
    // -----------------------------------------------------------------------

    /// Generate a summary from a slice of tool execution results.
    ///
    /// The `duration_ms` parameter records wall-clock time of the entire
    /// agent run.  When unavailable, callers should pass `0`.
    pub fn summarize(
        results: &[ToolOutput],
        agent_name: &str,
        task: &str,
    ) -> AgentExecutionSummary {
        let mut tools_used: Vec<String> = Vec::new();
        let mut errors: Vec<String> = Vec::new();
        let mut files_modified: HashSet<String> = HashSet::new();
        let mut files_created: HashSet<String> = HashSet::new();
        let mut all_metadata: HashMap<String, Value> = HashMap::new();

        for result in results {
            // Extract tool name from metadata
            if let Some(tool) = result.metadata.get("tool_name") {
                if let Some(name) = tool.as_str() {
                    if !tools_used.contains(&name.to_string()) {
                        tools_used.push(name.to_string());
                    }
                }
            }

            // Collect errors
            if result.is_error {
                errors.push(result.content.clone());
            }

            // Parse content for file paths and structured data
            Self::extract_file_operations(
                &result.content,
                &mut files_modified,
                &mut files_created,
            );

            // Merge metadata (later entries win)
            for (k, v) in &result.metadata {
                all_metadata.insert(k.clone(), v.clone());
            }
        }

        // Extract key findings from non-error content
        let key_findings = Self::extract_key_findings(results);
        let recommendations = Self::extract_recommendations(results);

        // Determine status
        let status = Self::determine_status(results);

        let mut files_modified: Vec<String> = files_modified.into_iter().collect();
        files_modified.sort();
        let mut files_created: Vec<String> = files_created.into_iter().collect();
        files_created.sort();

        AgentExecutionSummary {
            agent_name: agent_name.to_string(),
            task_description: task.to_string(),
            status,
            duration_ms: 0,
            files_modified,
            files_created,
            tools_used,
            errors,
            key_findings,
            recommendations,
            metadata: all_metadata,
        }
    }

    /// Generate a summary with a recorded duration.
    pub fn summarize_with_duration(
        results: &[ToolOutput],
        agent_name: &str,
        task: &str,
        duration_ms: u64,
    ) -> AgentExecutionSummary {
        let mut summary = Self::summarize(results, agent_name, task);
        summary.duration_ms = duration_ms;
        summary
    }

    /// Produce a single-line human-readable summary.
    pub fn brief_summary(summary: &AgentExecutionSummary) -> String {
        let status_icon = match summary.status {
            SummaryStatus::Success => "OK",
            SummaryStatus::PartialSuccess => "PARTIAL",
            SummaryStatus::Failed => "FAIL",
            SummaryStatus::Timeout => "TIMEOUT",
        };

        let file_count = summary.files_modified.len() + summary.files_created.len();
        let tool_count = summary.tools_used.len();

        if summary.errors.is_empty() {
            format!(
                "[{}] {} - {} tools, {} files",
                status_icon, summary.agent_name, tool_count, file_count,
            )
        } else {
            format!(
                "[{}] {} - {} errors, {} tools, {} files",
                status_icon,
                summary.agent_name,
                summary.errors.len(),
                tool_count,
                file_count,
            )
        }
    }

    /// Produce a detailed Markdown report.
    pub fn detailed_report(summary: &AgentExecutionSummary) -> String {
        let mut lines = Vec::new();

        lines.push(format!("# Agent Execution Report: {}", summary.agent_name));
        lines.push(String::new());
        lines.push(format!("**Task**: {}", summary.task_description));
        lines.push(format!("**Status**: {}", summary.status));
        if summary.duration_ms > 0 {
            let secs = summary.duration_ms as f64 / 1000.0;
            lines.push(format!("**Duration**: {:.2}s", secs));
        }
        lines.push(String::new());

        // Tools used
        lines.push("## Tools Used".to_string());
        if summary.tools_used.is_empty() {
            lines.push("*(none)*".to_string());
        } else {
            for tool in &summary.tools_used {
                lines.push(format!("- {}", tool));
            }
        }
        lines.push(String::new());

        // Files
        if !summary.files_created.is_empty() || !summary.files_modified.is_empty() {
            lines.push("## Files".to_string());
            if !summary.files_created.is_empty() {
                lines.push("### Created".to_string());
                for f in &summary.files_created {
                    lines.push(format!("- `{}`", f));
                }
            }
            if !summary.files_modified.is_empty() {
                lines.push("### Modified".to_string());
                for f in &summary.files_modified {
                    lines.push(format!("- `{}`", f));
                }
            }
            lines.push(String::new());
        }

        // Errors
        if !summary.errors.is_empty() {
            lines.push("## Errors".to_string());
            for err in &summary.errors {
                lines.push(format!("- {}", Self::truncate(err, 200)));
            }
            lines.push(String::new());
        }

        // Key findings
        if !summary.key_findings.is_empty() {
            lines.push("## Key Findings".to_string());
            for finding in &summary.key_findings {
                lines.push(format!("- {}", finding));
            }
            lines.push(String::new());
        }

        // Recommendations
        if !summary.recommendations.is_empty() {
            lines.push("## Recommendations".to_string());
            for rec in &summary.recommendations {
                lines.push(format!("- {}", rec));
            }
            lines.push(String::new());
        }

        lines.join("\n")
    }

    /// Merge multiple agent summaries into a single consolidated summary.
    pub fn merge_summaries(summaries: &[AgentExecutionSummary]) -> AgentExecutionSummary {
        if summaries.is_empty() {
            return AgentExecutionSummary::empty();
        }

        if summaries.len() == 1 {
            return summaries[0].clone();
        }

        let mut tools_used: HashSet<String> = HashSet::new();
        let mut files_modified: HashSet<String> = HashSet::new();
        let mut files_created: HashSet<String> = HashSet::new();
        let mut errors: Vec<String> = Vec::new();
        let mut key_findings: HashSet<String> = HashSet::new();
        let mut recommendations: HashSet<String> = HashSet::new();
        let mut all_metadata: HashMap<String, Value> = HashMap::new();
        let mut total_duration: u64 = 0;
        let mut agent_names: Vec<String> = Vec::new();

        for summary in summaries {
            agent_names.push(summary.agent_name.clone());
            total_duration += summary.duration_ms;
            tools_used.extend(summary.tools_used.iter().cloned());
            files_modified.extend(summary.files_modified.iter().cloned());
            files_created.extend(summary.files_created.iter().cloned());
            errors.extend(summary.errors.iter().cloned());
            key_findings.extend(summary.key_findings.iter().cloned());
            recommendations.extend(summary.recommendations.iter().cloned());
            for (k, v) in &summary.metadata {
                all_metadata.insert(k.clone(), v.clone());
            }
        }

        // Determine merged status: worst status wins
        let merged_status = summaries
            .iter()
            .map(|s| &s.status)
            .max_by_key(|s| match s {
                SummaryStatus::Failed => 3u8,
                SummaryStatus::Timeout => 2,
                SummaryStatus::PartialSuccess => 1,
                SummaryStatus::Success => 0,
            })
            .cloned()
            .unwrap_or(SummaryStatus::Failed);

        let mut tools_used: Vec<String> = tools_used.into_iter().collect();
        tools_used.sort();
        let mut files_modified: Vec<String> = files_modified.into_iter().collect();
        files_modified.sort();
        let mut files_created: Vec<String> = files_created.into_iter().collect();
        files_created.sort();
        let mut key_findings: Vec<String> = key_findings.into_iter().collect();
        key_findings.sort();
        let mut recommendations: Vec<String> = recommendations.into_iter().collect();
        recommendations.sort();

        let merged_task = format!(
            "Merged task from {} agent(s)",
            summaries.len()
        );

        AgentExecutionSummary {
            agent_name: format!("merged({})", agent_names.join(", ")),
            task_description: merged_task,
            status: merged_status,
            duration_ms: total_duration,
            files_modified,
            files_created,
            tools_used,
            errors,
            key_findings,
            recommendations,
            metadata: all_metadata,
        }
    }

    /// Calculate aggregate success metrics across multiple summaries.
    pub fn success_metrics(summaries: &[AgentExecutionSummary]) -> SuccessMetrics {
        let total_agents = summaries.len();
        let mut successful = 0usize;
        let mut partial = 0usize;
        let mut failed = 0usize;
        let mut timed_out = 0usize;
        let mut total_files_modified = 0usize;
        let mut total_files_created = 0usize;
        let mut total_errors = 0usize;

        for s in summaries {
            match s.status {
                SummaryStatus::Success => successful += 1,
                SummaryStatus::PartialSuccess => partial += 1,
                SummaryStatus::Failed => failed += 1,
                SummaryStatus::Timeout => timed_out += 1,
            }
            total_files_modified += s.files_modified.len();
            total_files_created += s.files_created.len();
            total_errors += s.errors.len();
        }

        let success_rate = if total_agents == 0 {
            0.0
        } else {
            (successful as f64 / total_agents as f64) * 100.0
        };

        SuccessMetrics {
            total_agents,
            successful,
            partial,
            failed,
            timed_out,
            total_files_modified,
            total_files_created,
            total_errors,
            success_rate,
        }
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    /// Determine the overall status from a list of tool results.
    fn determine_status(results: &[ToolOutput]) -> SummaryStatus {
        if results.is_empty() {
            return SummaryStatus::Success;
        }

        let total = results.len();
        let error_count = results.iter().filter(|r| r.is_error).count();

        if error_count == 0 {
            SummaryStatus::Success
        } else if error_count == total {
            SummaryStatus::Failed
        } else {
            SummaryStatus::PartialSuccess
        }
    }

    /// Extract file paths mentioned in tool output content.
    ///
    /// Looks for patterns like "Created file: /path/to/file", "Modified: src/main.rs",
    /// and common file-path-like tokens.
    fn extract_file_operations(
        content: &str,
        modified: &mut HashSet<String>,
        created: &mut HashSet<String>,
    ) {
        let lower = content.to_lowercase();

        // Pattern: "created" / "wrote" / "wrote to" followed by a file path
        let create_indicators = [
            "created file",
            "created ",
            "wrote ",
            "writing to ",
            "new file",
            "created:",
            "wrote to",
        ];

        // Pattern: "modified" / "updated" / "edited" / "changed" followed by a file path
        let modify_indicators = [
            "modified ",
            "updated ",
            "edited ",
            "changed ",
            "modified:",
            "updated:",
            "edited:",
        ];

        for indicator in &create_indicators {
            if let Some(idx) = lower.find(indicator) {
                // Use original content at same index to preserve case in paths
                let after = &content[idx + indicator.len()..];
                let path = Self::extract_path(after);
                if let Some(p) = path {
                    created.insert(p);
                }
            }
        }

        for indicator in &modify_indicators {
            if let Some(idx) = lower.find(indicator) {
                let after = &content[idx + indicator.len()..];
                let path = Self::extract_path(after);
                if let Some(p) = path {
                    modified.insert(p);
                }
            }
        }
    }

    /// Try to extract a file path from the beginning of a string fragment.
    /// Recognizes backtick-quoted paths, absolute paths, and relative paths with extensions.
    fn extract_path(fragment: &str) -> Option<String> {
        // Strip leading whitespace and punctuation (colons, dashes, etc.)
        let trimmed = fragment
            .trim_start()
            .trim_start_matches(|c: char| c == ':' || c == '-' || c == '=' || c == '>')
            .trim_start();

        // Backtick-quoted path: `src/main.rs`
        if trimmed.starts_with('`') {
            if let Some(end) = trimmed[1..].find('`') {
                let candidate = &trimmed[1..=end];
                if Self::looks_like_path(candidate) {
                    return Some(candidate.to_string());
                }
            }
        }

        // Quoted path: "src/main.rs" or 'src/main.rs'
        if trimmed.starts_with('"') {
            if let Some(end) = trimmed[1..].find('"') {
                let candidate = &trimmed[1..=end];
                if Self::looks_like_path(candidate) {
                    return Some(candidate.to_string());
                }
            }
        }

        // Absolute or relative path
        let path_end = trimmed
            .find(|c: char| c == '\n' || c == ',' || c == ')' || c == '}' || c == ';');
        let candidate = match path_end {
            Some(end) => trimmed[..end].trim(),
            None => trimmed.trim(),
        };

        if Self::looks_like_path(candidate) {
            return Some(candidate.to_string());
        }

        None
    }

    /// Heuristic: does this string look like a file path?
    fn looks_like_path(s: &str) -> bool {
        if s.is_empty() || s.len() > 512 {
            return false;
        }

        // Absolute path
        if s.starts_with('/') || s.starts_with("./") || s.starts_with("../") {
            return true;
        }

        // Relative path with extension (e.g., "src/main.rs", "Cargo.toml")
        if s.contains('/') || s.contains('\\') {
            return true;
        }

        // Filename with extension
        if let Some(dot_pos) = s.rfind('.') {
            if dot_pos > 0 && dot_pos < s.len() - 1 {
                let ext = &s[dot_pos + 1..];
                // Common code file extensions
                let known_exts = [
                    "rs", "toml", "json", "yaml", "yml", "md", "txt", "py", "js", "ts",
                    "go", "java", "c", "h", "cpp", "hpp", "rb", "sh", "bash", "zsh",
                    "html", "css", "scss", "sql", "lock", "cfg", "ini", "xml",
                ];
                if known_exts.contains(&ext) {
                    return true;
                }
            }
        }

        false
    }

    /// Extract key findings from non-error tool outputs.
    /// Simple heuristic: take first non-empty line from each successful result
    /// that is not a metadata line or file operation indicator.
    fn extract_key_findings(results: &[ToolOutput]) -> Vec<String> {
        let mut findings: Vec<String> = Vec::new();

        for result in results {
            if result.is_error {
                continue;
            }

            // Try parsing as JSON first
            if let Ok(value) = serde_json::from_str::<Value>(&result.content) {
                // If it has a "summary" or "result" key, use that
                for key in &["summary", "result", "finding", "output"] {
                    if let Some(val) = value.get(key) {
                        if let Some(s) = val.as_str() {
                            if !s.is_empty() && findings.len() < 10 {
                                findings.push(Self::truncate(s, 200));
                            }
                        }
                    }
                }
                continue;
            }

            // For plain text, take the first substantive line
            for line in result.content.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                // Skip metadata-like lines
                if trimmed.starts_with('#') || trimmed.starts_with("//") {
                    continue;
                }
                if findings.len() < 10 {
                    findings.push(Self::truncate(trimmed, 200));
                }
                // Only take one line per result for plain text
                break;
            }
        }

        findings
    }

    /// Extract recommendations from tool output content.
    /// Looks for lines containing "recommend", "suggest", "should", "consider".
    fn extract_recommendations(results: &[ToolOutput]) -> Vec<String> {
        let mut recs: Vec<String> = Vec::new();
        let rec_keywords = [
            "recommend",
            "suggest",
            "should",
            "consider",
            "advise",
            "propose",
            "improve",
        ];

        for result in results {
            for line in result.content.lines() {
                let trimmed = line.trim().to_lowercase();
                if rec_keywords.iter().any(|kw| trimmed.contains(kw)) {
                    let original = line.trim();
                    if !original.is_empty() && recs.len() < 10 {
                        recs.push(Self::truncate(original, 200));
                    }
                }
            }
        }

        recs
    }

    /// Truncate a string to `max_len` characters, appending "..." if truncated.
    fn truncate(s: &str, max_len: usize) -> String {
        if s.len() <= max_len {
            s.to_string()
        } else {
            format!("{}...", &s[..max_len])
        }
    }
}

// ---------------------------------------------------------------------------
// AgentExecutionSummary helpers
// ---------------------------------------------------------------------------

impl AgentExecutionSummary {
    /// Create an empty summary with zeroed-out fields.
    pub fn empty() -> Self {
        Self {
            agent_name: String::new(),
            task_description: String::new(),
            status: SummaryStatus::Success,
            duration_ms: 0,
            files_modified: Vec::new(),
            files_created: Vec::new(),
            tools_used: Vec::new(),
            errors: Vec::new(),
            key_findings: Vec::new(),
            recommendations: Vec::new(),
            metadata: HashMap::new(),
        }
    }

    /// Whether this summary represents a fully successful execution.
    pub fn is_success(&self) -> bool {
        self.status == SummaryStatus::Success
    }

    /// Whether this summary has any errors.
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }
}

// ---------------------------------------------------------------------------
// SuccessMetrics helpers
// ---------------------------------------------------------------------------

impl SuccessMetrics {
    /// Create metrics for an empty set of summaries.
    pub fn empty() -> Self {
        Self {
            total_agents: 0,
            successful: 0,
            partial: 0,
            failed: 0,
            timed_out: 0,
            total_files_modified: 0,
            total_files_created: 0,
            total_errors: 0,
            success_rate: 0.0,
        }
    }

    /// Whether all agents succeeded.
    pub fn all_succeeded(&self) -> bool {
        self.total_agents > 0 && self.successful == self.total_agents
    }

    /// Whether any agent failed.
    pub fn has_failures(&self) -> bool {
        self.failed > 0
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Helper: build a ToolOutput with metadata.
    fn tool_output(content: &str, is_error: bool, tool_name: &str) -> ToolOutput {
        let mut metadata = HashMap::new();
        metadata.insert("tool_name".into(), json!(tool_name));
        ToolOutput {
            content: content.to_string(),
            is_error,
            metadata,
        }
    }

    /// Helper: build a ToolOutput with no metadata.
    fn simple_output(content: &str, is_error: bool) -> ToolOutput {
        ToolOutput {
            content: content.to_string(),
            is_error,
            metadata: HashMap::new(),
        }
    }

    // ---- Status determination ----

    #[test]
    fn test_empty_results_is_success() {
        let results: Vec<ToolOutput> = vec![];
        let summary = SummaryGenerator::summarize(&results, "agent", "task");
        assert_eq!(summary.status, SummaryStatus::Success);
    }

    #[test]
    fn test_all_success_is_success_status() {
        let results = vec![
            tool_output("OK", false, "read"),
            tool_output("Done", false, "write"),
        ];
        let summary = SummaryGenerator::summarize(&results, "agent", "task");
        assert_eq!(summary.status, SummaryStatus::Success);
        assert!(summary.errors.is_empty());
    }

    #[test]
    fn test_all_errors_is_failed_status() {
        let results = vec![
            tool_output("Error A", true, "read"),
            tool_output("Error B", true, "write"),
        ];
        let summary = SummaryGenerator::summarize(&results, "agent", "task");
        assert_eq!(summary.status, SummaryStatus::Failed);
        assert_eq!(summary.errors.len(), 2);
    }

    #[test]
    fn test_mixed_results_is_partial_success() {
        let results = vec![
            tool_output("OK", false, "read"),
            tool_output("Failed", true, "write"),
        ];
        let summary = SummaryGenerator::summarize(&results, "agent", "task");
        assert_eq!(summary.status, SummaryStatus::PartialSuccess);
        assert_eq!(summary.errors.len(), 1);
    }

    // ---- File extraction ----

    #[test]
    fn test_extracts_created_files() {
        let results = vec![simple_output(
            "Created file: src/main.rs\nAll good",
            false,
        )];
        let summary = SummaryGenerator::summarize(&results, "agent", "task");
        assert!(summary.files_created.contains(&"src/main.rs".to_string()));
    }

    #[test]
    fn test_extracts_modified_files() {
        let results = vec![simple_output(
            "Modified: Cargo.toml\nUpdated dependencies",
            false,
        )];
        let summary = SummaryGenerator::summarize(&results, "agent", "task");
        assert!(summary.files_modified.contains(&"Cargo.toml".to_string()));
    }

    #[test]
    fn test_extracts_backtick_quoted_paths() {
        let results = vec![simple_output(
            "Created file: `src/lib.rs`\nDone",
            false,
        )];
        let summary = SummaryGenerator::summarize(&results, "agent", "task");
        assert!(summary.files_created.contains(&"src/lib.rs".to_string()));
    }

    #[test]
    fn test_no_false_positive_file_paths() {
        let results = vec![simple_output(
            "The operation completed successfully in 2 seconds",
            false,
        )];
        let summary = SummaryGenerator::summarize(&results, "agent", "task");
        assert!(summary.files_created.is_empty());
        assert!(summary.files_modified.is_empty());
    }

    // ---- Tool usage extraction ----

    #[test]
    fn test_extracts_tool_names_from_metadata() {
        let results = vec![
            tool_output("ok", false, "read"),
            tool_output("ok", false, "write"),
            tool_output("ok", false, "read"), // duplicate tool
        ];
        let summary = SummaryGenerator::summarize(&results, "agent", "task");
        assert_eq!(summary.tools_used.len(), 2);
        assert!(summary.tools_used.contains(&"read".to_string()));
        assert!(summary.tools_used.contains(&"write".to_string()));
    }

    #[test]
    fn test_no_tools_when_metadata_absent() {
        let results = vec![simple_output("ok", false)];
        let summary = SummaryGenerator::summarize(&results, "agent", "task");
        assert!(summary.tools_used.is_empty());
    }

    // ---- Key findings ----

    #[test]
    fn test_extracts_key_findings_from_plain_text() {
        let results = vec![simple_output(
            "Found 3 unused imports in src/main.rs\nAlso found memory leak in parser",
            false,
        )];
        let summary = SummaryGenerator::summarize(&results, "agent", "task");
        assert!(!summary.key_findings.is_empty());
        assert_eq!(summary.key_findings[0], "Found 3 unused imports in src/main.rs");
    }

    #[test]
    fn test_extracts_key_findings_from_json() {
        let content = serde_json::to_string(&json!({
            "result": "All tests passed",
            "files": 5
        }))
        .unwrap();
        let results = vec![simple_output(&content, false)];
        let summary = SummaryGenerator::summarize(&results, "agent", "task");
        assert!(summary.key_findings.contains(&"All tests passed".to_string()));
    }

    // ---- Recommendations ----

    #[test]
    fn test_extracts_recommendations() {
        let results = vec![simple_output(
            "Analysis complete.\nRecommend refactoring the parser module.\nConsider adding error handling.",
            false,
        )];
        let summary = SummaryGenerator::summarize(&results, "agent", "task");
        assert!(summary.recommendations.len() >= 2);
    }

    // ---- Brief summary ----

    #[test]
    fn test_brief_summary_success() {
        let summary = AgentExecutionSummary {
            agent_name: "worker-1".to_string(),
            task_description: "Fix bug".to_string(),
            status: SummaryStatus::Success,
            duration_ms: 1500,
            files_modified: vec!["src/main.rs".to_string()],
            files_created: vec![],
            tools_used: vec!["read".to_string(), "edit".to_string()],
            errors: vec![],
            key_findings: vec![],
            recommendations: vec![],
            metadata: HashMap::new(),
        };

        let brief = SummaryGenerator::brief_summary(&summary);
        assert!(brief.contains("[OK]"));
        assert!(brief.contains("worker-1"));
        assert!(brief.contains("2 tools"));
        assert!(brief.contains("1 files"));
    }

    #[test]
    fn test_brief_summary_with_errors() {
        let summary = AgentExecutionSummary {
            agent_name: "worker-2".to_string(),
            task_description: "Deploy".to_string(),
            status: SummaryStatus::PartialSuccess,
            duration_ms: 0,
            files_modified: vec![],
            files_created: vec![],
            tools_used: vec!["deploy".to_string()],
            errors: vec!["connection refused".to_string()],
            key_findings: vec![],
            recommendations: vec![],
            metadata: HashMap::new(),
        };

        let brief = SummaryGenerator::brief_summary(&summary);
        assert!(brief.contains("[PARTIAL]"));
        assert!(brief.contains("1 errors"));
    }

    // ---- Detailed report ----

    #[test]
    fn test_detailed_report_contains_sections() {
        let summary = AgentExecutionSummary {
            agent_name: "test-agent".to_string(),
            task_description: "Analyze codebase".to_string(),
            status: SummaryStatus::Success,
            duration_ms: 5000,
            files_modified: vec!["src/lib.rs".to_string()],
            files_created: vec!["src/new.rs".to_string()],
            tools_used: vec!["read".to_string()],
            errors: vec![],
            key_findings: vec!["Found dead code".to_string()],
            recommendations: vec!["Remove unused imports".to_string()],
            metadata: HashMap::new(),
        };

        let report = SummaryGenerator::detailed_report(&summary);
        assert!(report.contains("# Agent Execution Report: test-agent"));
        assert!(report.contains("**Task**: Analyze codebase"));
        assert!(report.contains("**Status**: success"));
        assert!(report.contains("## Tools Used"));
        assert!(report.contains("## Files"));
        assert!(report.contains("### Created"));
        assert!(report.contains("src/new.rs"));
        assert!(report.contains("### Modified"));
        assert!(report.contains("src/lib.rs"));
        assert!(report.contains("## Key Findings"));
        assert!(report.contains("## Recommendations"));
        // Should NOT contain Errors section when there are none
        assert!(!report.contains("## Errors"));
    }

    #[test]
    fn test_detailed_report_contains_errors_section() {
        let summary = AgentExecutionSummary {
            agent_name: "fail-agent".to_string(),
            task_description: "task".to_string(),
            status: SummaryStatus::Failed,
            duration_ms: 0,
            files_modified: vec![],
            files_created: vec![],
            tools_used: vec![],
            errors: vec!["something broke badly".to_string()],
            key_findings: vec![],
            recommendations: vec![],
            metadata: HashMap::new(),
        };

        let report = SummaryGenerator::detailed_report(&summary);
        assert!(report.contains("## Errors"));
        assert!(report.contains("something broke badly"));
    }

    // ---- Merge summaries ----

    #[test]
    fn test_merge_empty_summaries() {
        let merged = SummaryGenerator::merge_summaries(&[]);
        assert_eq!(merged.agent_name, "");
        assert_eq!(merged.status, SummaryStatus::Success);
    }

    #[test]
    fn test_merge_single_summary_unchanged() {
        let summary = AgentExecutionSummary {
            agent_name: "solo".to_string(),
            task_description: "task".to_string(),
            status: SummaryStatus::Success,
            duration_ms: 100,
            files_modified: vec!["a.rs".to_string()],
            files_created: vec![],
            tools_used: vec!["read".to_string()],
            errors: vec![],
            key_findings: vec![],
            recommendations: vec![],
            metadata: HashMap::new(),
        };

        let merged = SummaryGenerator::merge_summaries(&[summary.clone()]);
        assert_eq!(merged.agent_name, "solo");
        assert_eq!(merged.files_modified.len(), 1);
    }

    #[test]
    fn test_merge_multiple_summaries() {
        let summaries = vec![
            AgentExecutionSummary {
                agent_name: "agent-a".to_string(),
                task_description: "read files".to_string(),
                status: SummaryStatus::Success,
                duration_ms: 100,
                files_modified: vec!["a.rs".to_string()],
                files_created: vec!["new_a.rs".to_string()],
                tools_used: vec!["read".to_string()],
                errors: vec![],
                key_findings: vec!["finding A".to_string()],
                recommendations: vec![],
                metadata: HashMap::new(),
            },
            AgentExecutionSummary {
                agent_name: "agent-b".to_string(),
                task_description: "write files".to_string(),
                status: SummaryStatus::PartialSuccess,
                duration_ms: 200,
                files_modified: vec!["b.rs".to_string()],
                files_created: vec![],
                tools_used: vec!["write".to_string(), "read".to_string()],
                errors: vec!["permission denied".to_string()],
                key_findings: vec!["finding B".to_string()],
                recommendations: vec![],
                metadata: HashMap::new(),
            },
        ];

        let merged = SummaryGenerator::merge_summaries(&summaries);
        assert!(merged.agent_name.contains("agent-a"));
        assert!(merged.agent_name.contains("agent-b"));
        assert_eq!(merged.status, SummaryStatus::PartialSuccess);
        assert_eq!(merged.duration_ms, 300);
        assert_eq!(merged.files_modified.len(), 2);
        assert_eq!(merged.files_created.len(), 1);
        assert_eq!(merged.errors.len(), 1);
        assert_eq!(merged.tools_used.len(), 2); // deduplicated: read, write
        assert_eq!(merged.key_findings.len(), 2);
    }

    #[test]
    fn test_merge_worst_status_wins() {
        let summaries = vec![
            AgentExecutionSummary {
                agent_name: "ok".to_string(),
                task_description: "t".to_string(),
                status: SummaryStatus::Success,
                duration_ms: 0,
                files_modified: vec![],
                files_created: vec![],
                tools_used: vec![],
                errors: vec![],
                key_findings: vec![],
                recommendations: vec![],
                metadata: HashMap::new(),
            },
            AgentExecutionSummary {
                agent_name: "fail".to_string(),
                task_description: "t".to_string(),
                status: SummaryStatus::Failed,
                duration_ms: 0,
                files_modified: vec![],
                files_created: vec![],
                tools_used: vec![],
                errors: vec!["err".to_string()],
                key_findings: vec![],
                recommendations: vec![],
                metadata: HashMap::new(),
            },
        ];

        let merged = SummaryGenerator::merge_summaries(&summaries);
        assert_eq!(merged.status, SummaryStatus::Failed);
    }

    // ---- Success metrics ----

    #[test]
    fn test_success_metrics_empty() {
        let metrics = SummaryGenerator::success_metrics(&[]);
        assert_eq!(metrics.total_agents, 0);
        assert_eq!(metrics.success_rate, 0.0);
        assert!(!metrics.all_succeeded());
    }

    #[test]
    fn test_success_metrics_all_successful() {
        let summaries = vec![
            make_summary("a", SummaryStatus::Success, 0),
            make_summary("b", SummaryStatus::Success, 1),
            make_summary("c", SummaryStatus::Success, 2),
        ];
        let metrics = SummaryGenerator::success_metrics(&summaries);
        assert_eq!(metrics.total_agents, 3);
        assert_eq!(metrics.successful, 3);
        assert_eq!(metrics.partial, 0);
        assert_eq!(metrics.failed, 0);
        assert!((metrics.success_rate - 100.0).abs() < 0.01);
        assert!(metrics.all_succeeded());
        assert!(!metrics.has_failures());
    }

    #[test]
    fn test_success_metrics_mixed() {
        let summaries = vec![
            make_summary("a", SummaryStatus::Success, 0),
            make_summary("b", SummaryStatus::PartialSuccess, 0),
            make_summary("c", SummaryStatus::Failed, 2),
            make_summary("d", SummaryStatus::Timeout, 0),
            make_summary("e", SummaryStatus::Success, 0),
        ];
        let metrics = SummaryGenerator::success_metrics(&summaries);
        assert_eq!(metrics.total_agents, 5);
        assert_eq!(metrics.successful, 2);
        assert_eq!(metrics.partial, 1);
        assert_eq!(metrics.failed, 1);
        assert_eq!(metrics.timed_out, 1);
        assert!((metrics.success_rate - 40.0).abs() < 0.01);
        assert!(!metrics.all_succeeded());
        assert!(metrics.has_failures());
    }

    #[test]
    fn test_success_metrics_file_counts() {
        let summaries = vec![
            make_summary_with_files("a", 3, 2, 1),
            make_summary_with_files("b", 1, 4, 0),
        ];
        let metrics = SummaryGenerator::success_metrics(&summaries);
        assert_eq!(metrics.total_files_modified, 4);
        assert_eq!(metrics.total_files_created, 6);
        assert_eq!(metrics.total_errors, 1);
    }

    // ---- Duration tracking ----

    #[test]
    fn test_summarize_with_duration() {
        let results = vec![tool_output("done", false, "read")];
        let summary = SummaryGenerator::summarize_with_duration(&results, "agent", "task", 3500);
        assert_eq!(summary.duration_ms, 3500);
    }

    #[test]
    fn test_summarize_default_duration_is_zero() {
        let results = vec![tool_output("done", false, "read")];
        let summary = SummaryGenerator::summarize(&results, "agent", "task");
        assert_eq!(summary.duration_ms, 0);
    }

    // ---- SummaryStatus Display ----

    #[test]
    fn test_summary_status_display() {
        assert_eq!(SummaryStatus::Success.to_string(), "success");
        assert_eq!(SummaryStatus::PartialSuccess.to_string(), "partial_success");
        assert_eq!(SummaryStatus::Failed.to_string(), "failed");
        assert_eq!(SummaryStatus::Timeout.to_string(), "timeout");
    }

    // ---- AgentExecutionSummary helpers ----

    #[test]
    fn test_empty_summary_helpers() {
        let summary = AgentExecutionSummary::empty();
        assert!(summary.is_success());
        assert!(!summary.has_errors());
        assert!(summary.agent_name.is_empty());
    }

    #[test]
    fn test_summary_helpers_with_data() {
        let summary = AgentExecutionSummary {
            agent_name: "agent".to_string(),
            task_description: "task".to_string(),
            status: SummaryStatus::PartialSuccess,
            duration_ms: 0,
            files_modified: vec![],
            files_created: vec![],
            tools_used: vec![],
            errors: vec!["error".to_string()],
            key_findings: vec![],
            recommendations: vec![],
            metadata: HashMap::new(),
        };
        assert!(!summary.is_success());
        assert!(summary.has_errors());
    }

    // ---- SuccessMetrics helpers ----

    #[test]
    fn test_empty_metrics_helpers() {
        let metrics = SuccessMetrics::empty();
        assert!(!metrics.all_succeeded());
        assert!(!metrics.has_failures());
    }

    // ---- Truncation ----

    #[test]
    fn test_truncate_short_string() {
        assert_eq!(SummaryGenerator::truncate("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_long_string() {
        let long = "a".repeat(300);
        let truncated = SummaryGenerator::truncate(&long, 100);
        assert_eq!(truncated.len(), 103); // 100 + "..."
        assert!(truncated.ends_with("..."));
    }

    // ---- Path detection ----

    #[test]
    fn test_looks_like_path_absolute() {
        assert!(SummaryGenerator::looks_like_path("/usr/local/bin"));
        assert!(SummaryGenerator::looks_like_path("./src/main.rs"));
        assert!(SummaryGenerator::looks_like_path("../parent.rs"));
    }

    #[test]
    fn test_looks_like_path_with_slash() {
        assert!(SummaryGenerator::looks_like_path("src/lib.rs"));
        assert!(SummaryGenerator::looks_like_path("crates/shannon-core/src/main.rs"));
    }

    #[test]
    fn test_looks_like_path_with_known_extension() {
        assert!(SummaryGenerator::looks_like_path("Cargo.toml"));
        assert!(SummaryGenerator::looks_like_path("main.rs"));
        assert!(SummaryGenerator::looks_like_path("config.json"));
    }

    #[test]
    fn test_rejects_non_paths() {
        assert!(!SummaryGenerator::looks_like_path(""));
        assert!(!SummaryGenerator::looks_like_path("hello world"));
        assert!(!SummaryGenerator::looks_like_path("the quick brown fox"));
    }

    // ---- Helpers for tests ----

    fn make_summary(name: &str, status: SummaryStatus, error_count: usize) -> AgentExecutionSummary {
        AgentExecutionSummary {
            agent_name: name.to_string(),
            task_description: "test task".to_string(),
            status,
            duration_ms: 0,
            files_modified: vec![],
            files_created: vec![],
            tools_used: vec![],
            errors: (0..error_count)
                .map(|i| format!("error {}", i))
                .collect(),
            key_findings: vec![],
            recommendations: vec![],
            metadata: HashMap::new(),
        }
    }

    fn make_summary_with_files(
        name: &str,
        modified: usize,
        created: usize,
        errors: usize,
    ) -> AgentExecutionSummary {
        AgentExecutionSummary {
            agent_name: name.to_string(),
            task_description: "task".to_string(),
            status: if errors > 0 {
                SummaryStatus::PartialSuccess
            } else {
                SummaryStatus::Success
            },
            duration_ms: 0,
            files_modified: (0..modified)
                .map(|i| format!("modified_{}.rs", i))
                .collect(),
            files_created: (0..created)
                .map(|i| format!("created_{}.rs", i))
                .collect(),
            tools_used: vec![],
            errors: (0..errors)
                .map(|i| format!("error {}", i))
                .collect(),
            key_findings: vec![],
            recommendations: vec![],
            metadata: HashMap::new(),
        }
    }
}
