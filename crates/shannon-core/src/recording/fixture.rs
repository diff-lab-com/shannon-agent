//! ToolChainTest builder for fixture-based deterministic testing.

use serde_json::Value;

/// A single step in a tool chain test.
#[derive(Debug, Clone)]
struct ToolChainStep {
    tool: String,
    input: Value,
    mock_result: Option<String>,
    mock_is_error: bool,
}

/// Result of running a tool chain test.
#[derive(Debug)]
pub struct ToolChainTestResult {
    pub steps_matched: usize,
    pub steps_total: usize,
    pub passed: bool,
    pub errors: Vec<String>,
}

/// Builder for deterministic tool chain tests.
///
/// Define an expected tool sequence with mock results, then verify
/// that the orchestration logic follows the correct path.
///
/// # Example
/// ```no_run
/// use shannon_core::recording::ToolChainTest;
/// use serde_json::json;
///
/// let chain = ToolChainTest::new()
///     .expect_tool("Read", json!({"path": "src/main.rs"}))
///     .respond_with("fn main() {}")
///     .expect_tool("Bash", json!({"command": "cargo check"}))
///     .respond_with("error[E0425]: unresolved name")
///     .expect_tool("Edit", json!({"path": "src/main.rs"}))
///     .respond_with("edited successfully");
/// let tools = chain.expected_tools();
/// assert_eq!(tools.len(), 3);
/// ```
pub struct ToolChainTest {
    steps: Vec<ToolChainStep>,
    _final_assertion_description: Option<String>,
}

impl ToolChainTest {
    /// Create a new empty tool chain test.
    pub fn new() -> Self {
        Self {
            steps: Vec::new(),
            _final_assertion_description: None,
        }
    }

    /// Expect a tool call with the given name and input.
    pub fn expect_tool(mut self, tool: &str, input: Value) -> Self {
        self.steps.push(ToolChainStep {
            tool: tool.to_string(),
            input,
            mock_result: None,
            mock_is_error: false,
        });
        self
    }

    /// Set the mock result for the last expected tool.
    pub fn respond_with(mut self, result: &str) -> Self {
        if let Some(step) = self.steps.last_mut() {
            step.mock_result = Some(result.to_string());
            step.mock_is_error = false;
        }
        self
    }

    /// Set the mock result as an error for the last expected tool.
    pub fn respond_error(mut self, msg: &str) -> Self {
        if let Some(step) = self.steps.last_mut() {
            step.mock_result = Some(msg.to_string());
            step.mock_is_error = true;
        }
        self
    }

    /// Get the expected tool sequence.
    pub fn expected_tools(&self) -> Vec<(&str, &Value)> {
        self.steps.iter().map(|s| (s.tool.as_str(), &s.input)).collect()
    }

    /// Get the mock results for each step.
    pub fn mock_results(&self) -> Vec<(Option<&str>, bool)> {
        self.steps
            .iter()
            .map(|s| (s.mock_result.as_deref(), s.mock_is_error))
            .collect()
    }

    /// Run the tool chain test against actual tool calls.
    /// `actual_calls` is a list of (tool_name, input, result, is_error) tuples.
    pub fn verify_against(
        &self,
        actual_calls: &[(&str, &Value, &str, bool)],
    ) -> ToolChainTestResult {
        let mut errors = Vec::new();
        let steps_matched = actual_calls.len().min(self.steps.len());

        for (i, actual) in actual_calls.iter().enumerate() {
            if i >= self.steps.len() {
                errors.push(format!(
                    "Extra tool call at step {i}: {} (expected only {} steps)",
                    actual.0,
                    self.steps.len()
                ));
                continue;
            }

            let expected = &self.steps[i];
            if expected.tool != actual.0 {
                errors.push(format!(
                    "Step {i}: expected tool '{}', got '{}'",
                    expected.tool, actual.0
                ));
            }
        }

        if actual_calls.len() < self.steps.len() {
            for i in actual_calls.len()..self.steps.len() {
                errors.push(format!(
                    "Missing tool call at step {i}: expected '{}'",
                    self.steps[i].tool
                ));
            }
        }

        let passed = errors.is_empty();
        ToolChainTestResult {
            steps_matched,
            steps_total: self.steps.len(),
            passed,
            errors,
        }
    }

    /// Number of expected tool steps.
    pub fn len(&self) -> usize {
        self.steps.len()
    }

    /// Whether the test has any steps.
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }
}

impl Default for ToolChainTest {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_tool_chain_builder() {
        let chain = ToolChainTest::new()
            .expect_tool("Read", json!({"path": "src/main.rs"}))
            .respond_with("fn main() {}")
            .expect_tool("Edit", json!({"path": "src/main.rs"}))
            .respond_with("ok");

        assert_eq!(chain.len(), 2);
        let tools = chain.expected_tools();
        assert_eq!(tools[0].0, "Read");
        assert_eq!(tools[1].0, "Edit");
    }

    #[test]
    fn test_tool_chain_verify_success() {
        let chain = ToolChainTest::new()
            .expect_tool("Read", json!({"path": "a.rs"}))
            .respond_with("contents")
            .expect_tool("Bash", json!({"command": "cargo check"}))
            .respond_with("ok");

        let read_input = json!({"path": "a.rs"});
        let bash_input = json!({"command": "cargo check"});
        let actual = vec![
            ("Read", &read_input, "contents", false),
            ("Bash", &bash_input, "ok", false),
        ];
        let result = chain.verify_against(&actual);
        assert!(result.passed);
        assert_eq!(result.steps_matched, 2);
    }

    #[test]
    fn test_tool_chain_verify_mismatch() {
        let chain = ToolChainTest::new()
            .expect_tool("Read", json!({"path": "a.rs"}))
            .respond_with("contents");

        let bash_input = json!({"command": "ls"});
        let actual = vec![
            ("Bash", &bash_input, "file1.rs", false),
        ];
        let result = chain.verify_against(&actual);
        assert!(!result.passed);
        assert!(result.errors[0].contains("expected tool 'Read'"));
    }

    #[test]
    fn test_tool_chain_verify_missing() {
        let chain = ToolChainTest::new()
            .expect_tool("Read", json!({"path": "a.rs"}))
            .respond_with("ok")
            .expect_tool("Edit", json!({"path": "a.rs"}))
            .respond_with("ok");

        let read_input = json!({"path": "a.rs"});
        let actual: Vec<(&str, &Value, &str, bool)> = vec![
            ("Read", &read_input, "ok", false),
        ];
        let result = chain.verify_against(&actual);
        assert!(!result.passed);
        assert!(result.errors.iter().any(|e| e.contains("Missing")));
    }

    #[test]
    fn test_tool_chain_error_response() {
        let chain = ToolChainTest::new()
            .expect_tool("Bash", json!({"command": "cargo build"}))
            .respond_error("compilation failed");

        let results = chain.mock_results();
        assert_eq!(results[0].0, Some("compilation failed"));
        assert!(results[0].1);
    }
}
