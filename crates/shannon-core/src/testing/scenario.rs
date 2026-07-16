//! Declarative YAML scenario testing framework.
//!
//! Defines test scenarios in YAML files with setup, mock responses, and validation
//! rules. Each scenario creates an isolated workspace, runs Shannon with mockito-
//! backed LLM responses, and validates the results against declared rules.

use serde::Deserialize;
use serde_json::Value;
use std::path::{Path, PathBuf};

use crate::testing::mock_dsl::{MockResponse, text_response, tool_call_response};

// ── YAML Schema Types ─────────────────────────────────────────────────

/// Top-level scenario definition loaded from YAML.
#[derive(Debug, Deserialize)]
pub struct ScenarioYaml {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub prompt: String,
    #[serde(default)]
    pub provider: String,
    #[serde(default = "default_model")]
    pub model: String,
    pub setup: ScenarioSetup,
    #[serde(default)]
    pub mock_responses: Vec<MockTurn>,
    pub validate: Vec<ValidationRule>,
}

fn default_model() -> String {
    "test-model".to_string()
}

/// Setup configuration for the test workspace.
#[derive(Debug, Deserialize)]
pub struct ScenarioSetup {
    #[serde(default)]
    pub files: Vec<FileSetup>,
    #[serde(default)]
    pub permission_mode: String,
}

/// A file to create in the workspace before running.
#[derive(Debug, Deserialize)]
pub struct FileSetup {
    pub path: String,
    pub content: String,
}

/// A single mock LLM response for one turn.
#[derive(Debug, Deserialize)]
pub struct MockTurn {
    pub response: MockResponseYaml,
}

/// YAML representation of a mock response.
#[derive(Debug, Deserialize)]
pub struct MockResponseYaml {
    #[serde(rename = "type")]
    pub response_type: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub tool: String,
    #[serde(default)]
    pub tool_id: String,
    #[serde(default)]
    pub input: Value,
}

/// A validation rule to check after the scenario runs.
#[derive(Debug, Deserialize, Clone)]
#[serde(tag = "rule")]
pub enum ValidationRule {
    #[serde(rename = "file_exists")]
    FileExists { path: String },
    #[serde(rename = "file_content")]
    FileContent {
        path: String,
        #[serde(default)]
        contains: String,
        #[serde(default)]
        matches_regex: String,
    },
    #[serde(rename = "file_not_exists")]
    FileNotExists { path: String },
    #[serde(rename = "exit_code")]
    ExitCode { value: String },
    #[serde(rename = "tool_called")]
    ToolCalled { tool: String },
    #[serde(rename = "response_contains")]
    ResponseContains { text: String },
    #[serde(rename = "max_duration_ms")]
    MaxDurationMs { limit: u64 },
}

// ── Scenario Result ───────────────────────────────────────────────────

/// Result of running a scenario.
#[derive(Debug)]
pub struct ScenarioResult {
    pub name: String,
    pub passed: bool,
    pub failures: Vec<String>,
    pub duration_ms: u64,
}

impl ScenarioResult {
    pub fn pass(name: &str, duration_ms: u64) -> Self {
        Self {
            name: name.to_string(),
            passed: true,
            failures: Vec::new(),
            duration_ms,
        }
    }

    pub fn fail(name: &str, duration_ms: u64, failures: Vec<String>) -> Self {
        Self {
            name: name.to_string(),
            passed: false,
            failures,
            duration_ms,
        }
    }
}

// ── YAML → MockResponse Conversion ────────────────────────────────────

/// Convert a YAML mock response into a MockResponse for the mock DSL.
pub fn yaml_to_mock_response(yaml: &MockResponseYaml) -> MockResponse {
    match yaml.response_type.as_str() {
        "text" => text_response(&yaml.content),
        "tool_use" => {
            let id = if yaml.tool_id.is_empty() {
                format!("toolu_{}", uuid::Uuid::new_v4().as_simple())
            } else {
                yaml.tool_id.clone()
            };
            tool_call_response(&id, &yaml.tool, yaml.input.clone())
        }
        "thinking" => crate::testing::mock_dsl::thinking_response(&yaml.content),
        "text_and_tool" => {
            let id = if yaml.tool_id.is_empty() {
                format!("toolu_{}", uuid::Uuid::new_v4().as_simple())
            } else {
                yaml.tool_id.clone()
            };
            crate::testing::mock_dsl::text_and_tool_response(
                &yaml.content,
                &id,
                &yaml.tool,
                yaml.input.clone(),
            )
        }
        _ => text_response(&yaml.content),
    }
}

/// Convert all YAML mock turns into MockResponse objects.
pub fn yaml_to_mock_responses(turns: &[MockTurn]) -> Vec<MockResponse> {
    turns
        .iter()
        .map(|t| yaml_to_mock_response(&t.response))
        .collect()
}

// ── Validation ────────────────────────────────────────────────────────

/// Validate a set of rules against workspace state.
pub fn validate_rules(
    rules: &[ValidationRule],
    workspace_dir: &Path,
    exit_code: &str,
    stdout: &str,
) -> Vec<String> {
    let mut failures = Vec::new();

    for rule in rules {
        match rule {
            ValidationRule::FileExists { path } => {
                let full_path = workspace_dir.join(path);
                if !full_path.exists() {
                    failures.push(format!("file_exists: {path} does not exist"));
                }
            }
            ValidationRule::FileContent {
                path,
                contains,
                matches_regex,
            } => {
                let full_path = workspace_dir.join(path);
                if !full_path.exists() {
                    failures.push(format!("file_content: {path} does not exist"));
                    continue;
                }
                let content = std::fs::read_to_string(&full_path).unwrap_or_default();
                if !contains.is_empty() && !content.contains(contains) {
                    failures.push(format!(
                        "file_content: {path} does not contain '{contains}'"
                    ));
                }
                if !matches_regex.is_empty() {
                    if let Ok(re) = regex::Regex::new(matches_regex) {
                        if !re.is_match(&content) {
                            failures.push(format!(
                                "file_content: {path} does not match regex '{matches_regex}'"
                            ));
                        }
                    }
                }
            }
            ValidationRule::FileNotExists { path } => {
                let full_path = workspace_dir.join(path);
                if full_path.exists() {
                    failures.push(format!("file_not_exists: {path} should not exist"));
                }
            }
            ValidationRule::ExitCode { value } => {
                if exit_code != value {
                    failures.push(format!("exit_code: expected '{value}', got '{exit_code}'"));
                }
            }
            ValidationRule::ToolCalled { tool } => {
                // Check stdout for tool use indicators in JSON output
                if !stdout.contains(tool) {
                    failures.push(format!("tool_called: tool '{tool}' not found in output"));
                }
            }
            ValidationRule::ResponseContains { text } => {
                if !stdout.contains(text) {
                    failures.push(format!("response_contains: '{text}' not found in output"));
                }
            }
            ValidationRule::MaxDurationMs { limit } => {
                // Duration is checked externally; this is a placeholder for
                // integration with timing logic
                let _ = limit;
            }
        }
    }

    failures
}

// ── Parsing ───────────────────────────────────────────────────────────

/// Parse a YAML scenario file.
pub fn parse_scenario(path: &Path) -> Result<ScenarioYaml, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
    let scenario: ScenarioYaml = serde_yaml::from_str(&content)
        .map_err(|e| format!("Failed to parse {}: {e}", path.display()))?;

    if scenario.name.is_empty() {
        return Err(format!("Scenario at {} has no name", path.display()));
    }
    if scenario.prompt.is_empty() {
        return Err(format!("Scenario '{}' has no prompt", scenario.name));
    }
    if scenario.mock_responses.is_empty() {
        return Err(format!(
            "Scenario '{}' has no mock_responses",
            scenario.name
        ));
    }

    Ok(scenario)
}

/// Parse all YAML files in a directory.
pub fn parse_scenarios_dir(dir: &Path) -> Result<Vec<(PathBuf, ScenarioYaml)>, String> {
    let mut scenarios = Vec::new();
    let entries =
        std::fs::read_dir(dir).map_err(|e| format!("Failed to read dir {}: {e}", dir.display()))?;

    let mut paths: Vec<_> = entries
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .is_some_and(|ext| ext == "yaml" || ext == "yml")
        })
        .map(|e| e.path())
        .collect();
    paths.sort();

    for path in paths {
        let scenario = parse_scenario(&path)?;
        scenarios.push((path, scenario));
    }

    Ok(scenarios)
}

// ── Workspace Setup ───────────────────────────────────────────────────

/// Create workspace files from scenario setup.
/// Returns the temp directory (caller must keep it alive).
pub fn create_scenario_workspace(setup: &ScenarioSetup) -> tempfile::TempDir {
    let dir = tempfile::TempDir::new().expect("create scenario workspace");

    for file in &setup.files {
        let full_path = dir.path().join(&file.path);
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent).expect("create parent dir");
        }
        std::fs::write(&full_path, &file.content).expect("write scenario file");
    }

    dir
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::testing::mock_dsl::MockContentBlock;
    use std::io::Write;

    fn write_temp_yaml(content: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().expect("temp file");
        write!(f, "{content}").expect("write yaml");
        f
    }

    #[test]
    fn test_parse_minimal_scenario() {
        let f = write_temp_yaml(
            r#"
name: hello
prompt: "Say hello"
setup:
  files: []
mock_responses:
  - response:
      type: text
      content: "Hello!"
validate:
  - rule: exit_code
    value: success
"#,
        );
        let scenario = parse_scenario(f.path()).expect("parse");
        assert_eq!(scenario.name, "hello");
        assert_eq!(scenario.prompt, "Say hello");
        assert_eq!(scenario.mock_responses.len(), 1);
        assert_eq!(scenario.validate.len(), 1);
    }

    #[test]
    fn test_parse_tool_use_scenario() {
        let f = write_temp_yaml(
            r#"
name: write_file
description: "Create a file"
prompt: "Create hello.txt"
provider: anthropic
setup:
  files:
    - path: src/main.rs
      content: "fn main() {}"
mock_responses:
  - response:
      type: tool_use
      tool: Write
      input: { path: "hello.txt", content: "world" }
  - response:
      type: text
      content: "Done!"
validate:
  - rule: file_exists
    path: hello.txt
  - rule: file_content
    path: hello.txt
    contains: "world"
"#,
        );
        let scenario = parse_scenario(f.path()).expect("parse");
        assert_eq!(scenario.name, "write_file");
        assert_eq!(scenario.setup.files.len(), 1);
        assert_eq!(scenario.mock_responses.len(), 2);
        assert_eq!(scenario.validate.len(), 2);
    }

    #[test]
    fn test_yaml_to_mock_response_text() {
        let yaml = MockResponseYaml {
            response_type: "text".to_string(),
            content: "Hello!".to_string(),
            tool: String::new(),
            tool_id: String::new(),
            input: Value::Null,
        };
        let mock = yaml_to_mock_response(&yaml);
        assert_eq!(mock.content_blocks.len(), 1);
        assert_eq!(mock.stop_reason, "end_turn");
    }

    #[test]
    fn test_yaml_to_mock_response_tool_use() {
        let yaml = MockResponseYaml {
            response_type: "tool_use".to_string(),
            content: String::new(),
            tool: "Write".to_string(),
            tool_id: "toolu_1".to_string(),
            input: serde_json::json!({"path": "hello.txt", "content": "world"}),
        };
        let mock = yaml_to_mock_response(&yaml);
        assert_eq!(mock.stop_reason, "tool_use");
        assert!(matches!(
            &mock.content_blocks[0],
            MockContentBlock::ToolUse { name, .. } if name == "Write"
        ));
    }

    #[test]
    fn test_validate_file_exists_pass() {
        let dir = tempfile::TempDir::new().expect("dir");
        std::fs::write(dir.path().join("hello.txt"), "world").expect("write");

        let failures = validate_rules(
            &[ValidationRule::FileExists {
                path: "hello.txt".to_string(),
            }],
            dir.path(),
            "success",
            "",
        );
        assert!(failures.is_empty(), "{failures:?}");
    }

    #[test]
    fn test_validate_file_exists_fail() {
        let dir = tempfile::TempDir::new().expect("dir");

        let failures = validate_rules(
            &[ValidationRule::FileExists {
                path: "missing.txt".to_string(),
            }],
            dir.path(),
            "success",
            "",
        );
        assert_eq!(failures.len(), 1);
        assert!(failures[0].contains("missing.txt"));
    }

    #[test]
    fn test_validate_file_content_contains() {
        let dir = tempfile::TempDir::new().expect("dir");
        std::fs::write(dir.path().join("hello.txt"), "hello world").expect("write");

        let failures = validate_rules(
            &[ValidationRule::FileContent {
                path: "hello.txt".to_string(),
                contains: "world".to_string(),
                matches_regex: String::new(),
            }],
            dir.path(),
            "success",
            "",
        );
        assert!(failures.is_empty(), "{failures:?}");
    }

    #[test]
    fn test_validate_file_content_regex() {
        let dir = tempfile::TempDir::new().expect("dir");
        std::fs::write(
            dir.path().join("code.rs"),
            "fn add(a: i32, b: i32) -> i32 { a + b }",
        )
        .expect("write");

        let failures = validate_rules(
            &[ValidationRule::FileContent {
                path: "code.rs".to_string(),
                contains: String::new(),
                matches_regex: r"fn \w+\(".to_string(),
            }],
            dir.path(),
            "success",
            "",
        );
        assert!(failures.is_empty(), "{failures:?}");
    }

    #[test]
    fn test_validate_exit_code() {
        let dir = tempfile::TempDir::new().expect("dir");
        let failures = validate_rules(
            &[ValidationRule::ExitCode {
                value: "success".to_string(),
            }],
            dir.path(),
            "success",
            "",
        );
        assert!(failures.is_empty());

        let failures = validate_rules(
            &[ValidationRule::ExitCode {
                value: "success".to_string(),
            }],
            dir.path(),
            "error",
            "",
        );
        assert_eq!(failures.len(), 1);
    }

    #[test]
    fn test_validate_tool_called() {
        let dir = tempfile::TempDir::new().expect("dir");
        let failures = validate_rules(
            &[ValidationRule::ToolCalled {
                tool: "Write".to_string(),
            }],
            dir.path(),
            "success",
            r#"{"tool_use": {"name": "Write", "input": {}}}"#,
        );
        assert!(failures.is_empty());
    }

    #[test]
    fn test_validate_response_contains() {
        let dir = tempfile::TempDir::new().expect("dir");
        let failures = validate_rules(
            &[ValidationRule::ResponseContains {
                text: "created".to_string(),
            }],
            dir.path(),
            "success",
            "I've created the file successfully",
        );
        assert!(failures.is_empty());
    }

    #[test]
    fn test_create_scenario_workspace() {
        let setup = ScenarioSetup {
            files: vec![
                FileSetup {
                    path: "src/main.rs".to_string(),
                    content: "fn main() {}".to_string(),
                },
                FileSetup {
                    path: "src/lib.rs".to_string(),
                    content: "pub fn add(a: i32, b: i32) -> i32 { a + b }".to_string(),
                },
            ],
            permission_mode: "full_auto".to_string(),
        };

        let dir = create_scenario_workspace(&setup);
        assert!(dir.path().join("src/main.rs").exists());
        assert!(dir.path().join("src/lib.rs").exists());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("src/main.rs")).unwrap(),
            "fn main() {}"
        );
    }

    #[test]
    fn test_yaml_to_mock_responses_sequence() {
        let turns = vec![
            MockTurn {
                response: MockResponseYaml {
                    response_type: "tool_use".to_string(),
                    content: String::new(),
                    tool: "Read".to_string(),
                    tool_id: "toolu_1".to_string(),
                    input: serde_json::json!({"path": "src/main.rs"}),
                },
            },
            MockTurn {
                response: MockResponseYaml {
                    response_type: "text".to_string(),
                    content: "The file looks good.".to_string(),
                    tool: String::new(),
                    tool_id: String::new(),
                    input: Value::Null,
                },
            },
        ];

        let mocks = yaml_to_mock_responses(&turns);
        assert_eq!(mocks.len(), 2);
        assert_eq!(mocks[0].stop_reason, "tool_use");
        assert_eq!(mocks[1].stop_reason, "end_turn");
    }

    #[test]
    fn test_parse_scenario_missing_name() {
        let f = write_temp_yaml(
            r#"
prompt: "test"
setup:
  files: []
mock_responses:
  - response:
      type: text
      content: "hi"
validate: []
"#,
        );
        // name defaults to empty string from serde, but our validation catches it
        // Actually serde requires the field (no #[serde(default)]), so this should fail
        let result = parse_scenario(f.path());
        // Either parse fails (no name field) or validation fails (empty name)
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_scenario_no_mock_responses() {
        let f = write_temp_yaml(
            r#"
name: empty
prompt: "test"
setup:
  files: []
mock_responses: []
validate: []
"#,
        );
        let result = parse_scenario(f.path());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("no mock_responses"));
    }

    #[test]
    fn test_parse_scenarios_dir() {
        let dir = tempfile::TempDir::new().expect("dir");

        std::fs::write(
            dir.path().join("a.yaml"),
            r#"
name: scenario_a
prompt: "test a"
setup:
  files: []
mock_responses:
  - response:
      type: text
      content: "a"
validate: []
"#,
        )
        .expect("write a");

        std::fs::write(
            dir.path().join("b.yaml"),
            r#"
name: scenario_b
prompt: "test b"
setup:
  files: []
mock_responses:
  - response:
      type: text
      content: "b"
validate: []
"#,
        )
        .expect("write b");

        let scenarios = parse_scenarios_dir(dir.path()).expect("parse dir");
        assert_eq!(scenarios.len(), 2);
        assert_eq!(scenarios[0].1.name, "scenario_a");
        assert_eq!(scenarios[1].1.name, "scenario_b");
    }

    #[test]
    fn test_file_not_exists_rule() {
        let dir = tempfile::TempDir::new().expect("dir");

        let failures = validate_rules(
            &[ValidationRule::FileNotExists {
                path: "should_not_exist.txt".to_string(),
            }],
            dir.path(),
            "success",
            "",
        );
        assert!(failures.is_empty());

        std::fs::write(dir.path().join("should_not_exist.txt"), "oops").expect("write");
        let failures = validate_rules(
            &[ValidationRule::FileNotExists {
                path: "should_not_exist.txt".to_string(),
            }],
            dir.path(),
            "success",
            "",
        );
        assert_eq!(failures.len(), 1);
    }
}
