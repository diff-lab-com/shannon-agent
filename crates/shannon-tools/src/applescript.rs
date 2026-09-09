//! AppleScript / Shortcuts automation tool (T13 Tier 1).
//!
//! Gives the model first-class access to scriptable macOS applications
//! (Mail, Calendar, Reminders, Messages, Finder, Notes, …) and the
//! Shortcuts app — the same "connectors first, screen last" strategy
//! Claude Cowork markets, implemented with the standard `osascript` and
//! `shortcuts` CLIs so there are no Swift bindings, no AX complexity and
//! no new dependency surface.
//!
//! # Platform
//!
//! Real execution requires macOS. On other platforms the tool registers
//! and returns a clear explanatory error (same pattern as the `computer`
//! tool's feature stub), so tool lists stay stable across platforms.
//!
//! # Permissions
//!
//! First invocation triggers Apple's standard **Automation** TCC prompt
//! per target app — the same permission users already grant to
//! Shortcuts.app. `is_destructive()` is `true`: AppleScript can read and
//! mutate application state, and the tool is registered with a High-risk
//! permission policy in `shannon-engine`.

use crate::{Tool, ToolError, ToolOutput, ToolResult};
use async_trait::async_trait;
use serde_json::json;
use std::collections::HashMap;
#[cfg(target_os = "macos")]
use std::time::Duration;

/// Upper bound for one osascript/shortcuts invocation. AppleScript hitting
/// a busy app can block indefinitely; anything longer is a hang.
#[cfg(target_os = "macos")]
const SCRIPT_TIMEOUT: Duration = Duration::from_secs(30);

/// Cap on returned output (osascript prints arbitrarily long results).
#[cfg(target_os = "macos")]
const MAX_OUTPUT_CHARS: usize = 50 * 1024;

/// Scripting backend selected by the `target` argument.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptTarget {
    /// `osascript -e <script>` — AppleScript / JXA against any scriptable app.
    Osascript,
    /// `shortcuts run <name>` — the Shortcuts app.
    Shortcuts,
}

/// Input for the applescript tool.
#[derive(Debug, Clone)]
pub struct AppleScriptInput {
    pub target: ScriptTarget,
    /// AppleScript source (target=applescript) or shortcut name (target=shortcuts).
    pub script: String,
    /// Optional AppleScript language override: "AppleScript" or "JavaScript"
    /// (JXA). Only valid for target=applescript.
    pub language: Option<String>,
}

impl AppleScriptInput {
    pub fn from_value(v: serde_json::Value) -> Result<Self, ToolError> {
        let obj = v
            .as_object()
            .ok_or_else(|| ToolError::InvalidInput("input must be an object".into()))?;
        let target = match obj.get("target").and_then(|t| t.as_str()) {
            Some("shortcuts") => ScriptTarget::Shortcuts,
            // Default keeps the common case (plain AppleScript) one-argument.
            None | Some("applescript") => ScriptTarget::Osascript,
            Some(other) => {
                return Err(ToolError::InvalidInput(format!(
                    "unknown target \"{other}\" (expected \"applescript\" or \"shortcuts\")"
                )));
            }
        };
        let script = obj
            .get("script")
            .and_then(|s| s.as_str())
            .map(str::to_string)
            .or_else(|| {
                // `shortcuts run` names the shortcut via `name`, which reads
                // better in tool calls than reusing "script".
                if target == ScriptTarget::Shortcuts {
                    obj.get("name").and_then(|s| s.as_str()).map(str::to_string)
                } else {
                    None
                }
            })
            .ok_or_else(|| ToolError::InvalidInput("missing required field \"script\"".into()))?;
        if script.trim().is_empty() {
            return Err(ToolError::InvalidInput(
                "\"script\" must not be empty".into(),
            ));
        }
        let language = obj
            .get("language")
            .and_then(|l| l.as_str())
            .map(str::to_string);
        if target == ScriptTarget::Shortcuts && language.is_some() {
            return Err(ToolError::InvalidInput(
                "\"language\" is only valid with target=applescript".into(),
            ));
        }
        if let Some(lang) = &language {
            if !matches!(lang.as_str(), "AppleScript" | "JavaScript") {
                return Err(ToolError::InvalidInput(
                    "\"language\" must be \"AppleScript\" or \"JavaScript\" (JXA)".into(),
                ));
            }
        }
        Ok(Self {
            target,
            script,
            language,
        })
    }
}

/// Run AppleScript / JXA via `osascript` or a Shortcuts shortcut via the
/// `shortcuts` CLI. macOS-only in real execution; stub elsewhere.
pub struct AppleScriptTool {
    description: String,
}

impl Default for AppleScriptTool {
    fn default() -> Self {
        Self::new()
    }
}

impl AppleScriptTool {
    pub fn new() -> Self {
        Self {
            description: "Run AppleScript or JXA against scriptable macOS applications (Mail, Calendar, Reminders, Messages, Finder, Notes, …), or run a Shortcuts shortcut. Requires macOS; the first call per application triggers Apple's standard Automation permission prompt.".to_string(),
        }
    }

    fn build_input_schema() -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "target": {
                    "type": "string",
                    "enum": ["applescript", "shortcuts"],
                    "description": "Scripting backend: \"applescript\" (osascript, default) or \"shortcuts\" (run a Shortcuts shortcut)"
                },
                "script": {
                    "type": "string",
                    "description": "AppleScript/JXA source (target=applescript) or shortcut name (target=shortcuts)"
                },
                "name": {
                    "type": "string",
                    "description": "Alternative field name for the shortcut name when target=shortcuts"
                },
                "language": {
                    "type": "string",
                    "enum": ["AppleScript", "JavaScript"],
                    "description": "Scripting language for target=applescript (default AppleScript; JavaScript = JXA)"
                }
            },
            "required": ["script"]
        })
    }

    #[cfg(target_os = "macos")]
    async fn execute_impl(&self, input: AppleScriptInput) -> ToolResult<ToolOutput> {
        use tokio::process::Command;

        let (mut cmd, label) = match input.target {
            ScriptTarget::Osascript => {
                let mut cmd = Command::new("osascript");
                if input.language.as_deref() == Some("JavaScript") {
                    cmd.arg("-l").arg("JavaScript");
                }
                cmd.arg("-e").arg(&input.script);
                (cmd, "osascript".to_string())
            }
            ScriptTarget::Shortcuts => {
                let mut cmd = Command::new("shortcuts");
                cmd.arg("run").arg(&input.script);
                (cmd, format!("shortcuts run {}", input.script))
            }
        };

        let output = tokio::time::timeout(SCRIPT_TIMEOUT, cmd.output())
            .await
            .map_err(|_| {
                // The 30s ceiling also swallows first-run Automation TCC
                // prompts (they wait for a human click), so name that case —
                // otherwise "first use" reads as a bare timeout (roadmap E5).
                ToolError::ExecutionFailed(format!(
                    "{label}: timed out after 30s (if a macOS Automation \
                     permission prompt is waiting to be allowed, allow it and \
                     retry)"
                ))
            })?
            .map_err(|e| ToolError::ExecutionFailed(format!("{label}: failed to spawn: {e}")))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if !output.status.success() {
            return Ok(ToolOutput {
                content: format!(
                    "{label} failed (exit {}):\n{}",
                    output.status.code().unwrap_or(-1),
                    truncate(stderr.trim())
                ),
                is_error: true,
                metadata: HashMap::new(),
            });
        }

        let mut content = String::new();
        if !stdout.trim().is_empty() {
            content.push_str(truncate(stdout.trim()));
        } else {
            content.push_str("(no output)");
        }
        if !stderr.trim().is_empty() {
            content.push_str("\n--- stderr ---\n");
            content.push_str(truncate(stderr.trim()));
        }

        let mut metadata = HashMap::new();
        metadata.insert(
            "target".into(),
            json!(label.split(' ').next().unwrap_or("")),
        );
        metadata.insert("exit_code".into(), json!(0));

        Ok(ToolOutput {
            content,
            is_error: false,
            metadata,
        })
    }

    #[cfg(not(target_os = "macos"))]
    async fn execute_impl(&self, input: AppleScriptInput) -> ToolResult<ToolOutput> {
        let _ = input;
        Ok(ToolOutput {
            content: "The applescript tool requires macOS. On this platform, use MCP servers or shell tools for application automation.".to_string(),
            is_error: true,
            metadata: HashMap::new(),
        })
    }
}

#[cfg(target_os = "macos")]
fn truncate(s: &str) -> &str {
    if s.len() <= MAX_OUTPUT_CHARS {
        s
    } else {
        let mut end = MAX_OUTPUT_CHARS;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        &s[..end]
    }
}

#[async_trait]
impl Tool for AppleScriptTool {
    fn name(&self) -> &str {
        "applescript"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> serde_json::Value {
        Self::build_input_schema()
    }

    fn is_read_only(&self) -> bool {
        false
    }

    fn is_concurrency_safe(&self) -> bool {
        false
    }

    fn is_destructive(&self) -> bool {
        // AppleScript can read and mutate application state.
        true
    }

    async fn execute(&self, input: serde_json::Value) -> ToolResult<ToolOutput> {
        let input = AppleScriptInput::from_value(input)?;
        self.execute_impl(input).await
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_tool_name_and_metadata() {
        let tool = AppleScriptTool::new();
        assert_eq!(tool.name(), "applescript");
        assert!(!tool.is_read_only());
        assert!(!tool.is_concurrency_safe());
        assert!(tool.is_destructive());
    }

    #[test]
    fn test_schema_default_target_and_required_script() {
        let schema = AppleScriptTool::build_input_schema();
        assert_eq!(schema["required"][0], json!("script"));
        assert_eq!(
            schema["properties"]["target"]["enum"][0],
            json!("applescript")
        );
    }

    #[test]
    fn test_parse_default_target() {
        let input = AppleScriptInput::from_value(json!({"script": "return 1"})).unwrap();
        assert_eq!(input.target, ScriptTarget::Osascript);
        assert!(input.language.is_none());
    }

    #[test]
    fn test_parse_shortcuts_with_name_field() {
        let input = AppleScriptInput::from_value(json!({
            "target": "shortcuts",
            "name": "Backup Notes"
        }))
        .unwrap();
        assert_eq!(input.target, ScriptTarget::Shortcuts);
        assert_eq!(input.script, "Backup Notes");
    }

    #[test]
    fn test_parse_rejects_unknown_target_and_language() {
        assert!(AppleScriptInput::from_value(json!({"target": "bash", "script": "x"})).is_err());
        assert!(
            AppleScriptInput::from_value(json!({"language": "python", "script": "x"})).is_err()
        );
        // language is applescript-only
        assert!(
            AppleScriptInput::from_value(
                json!({"target": "shortcuts", "name": "x", "language": "AppleScript"})
            )
            .is_err()
        );
    }

    #[test]
    fn test_parse_rejects_empty_script() {
        assert!(AppleScriptInput::from_value(json!({"script": "   "})).is_err());
        assert!(AppleScriptInput::from_value(json!({})).is_err());
    }

    #[tokio::test]
    async fn test_execute_stub_on_non_macos() {
        // On macOS this attempts osascript (absent in CI sandboxes → error,
        // which is fine); elsewhere it must return the explanatory stub.
        let tool = AppleScriptTool::new();
        let result = tool.execute(json!({"script": "return 1"})).await.unwrap();
        #[cfg(not(target_os = "macos"))]
        {
            assert!(result.is_error);
            assert!(result.content.contains("requires macOS"));
        }
        #[cfg(target_os = "macos")]
        let _ = result;
    }
}
