//! Hook decision types, result types, and hook definitions.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use super::events::HookEventType;
use super::types::HookError;

/// Decision returned by a hook command
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HookDecision {
    /// Allow the operation to proceed
    Allow,
    /// Deny the operation with a reason
    Deny { reason: String },
    /// Modify the tool input or output
    Modify {
        /// Modified input (for PreToolUse)
        #[serde(rename = "modified_input", skip_serializing_if = "Option::is_none")]
        modified_input: Option<Value>,
        /// Modified output (for PostToolUse)
        #[serde(rename = "modified_output", skip_serializing_if = "Option::is_none")]
        modified_output: Option<Value>,
    },
}

impl Default for HookDecision {
    fn default() -> Self {
        Self::Allow
    }
}

/// Result of executing a single hook command
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookResult {
    /// Exit code of the hook command
    pub exit_code: i32,
    /// Standard output from the hook command
    pub stdout: String,
    /// Standard error from the hook command
    pub stderr: String,
    /// The decision the hook made
    pub decision: HookDecision,
    /// The command that was executed
    pub command: String,
}

impl HookResult {
    /// Parse a hook decision from the stdout of a hook command.
    ///
    /// Hook commands can output JSON to the first line of stdout to communicate
    /// a decision. The format is:
    /// ```json
    /// {"decision": "deny", "reason": "not allowed"}
    /// {"decision": "modify", "modified_input": {...}}
    /// ```
    ///
    /// If no JSON is found, the default decision is Allow.
    pub fn parse_decision(stdout: &str) -> HookDecision {
        // Try to parse the first line as JSON
        if let Some(first_line) = stdout.lines().next() {
            let trimmed = first_line.trim();
            if trimmed.starts_with('{') {
                if let Ok(val) = serde_json::from_str::<Value>(trimmed) {
                    if let Some(obj) = val.as_object() {
                        if let Some(decision) = obj.get("decision").and_then(|d| d.as_str()) {
                            return match decision {
                                "deny" => HookDecision::Deny {
                                    reason: obj
                                        .get("reason")
                                        .and_then(|r| r.as_str())
                                        .unwrap_or("Hook denied operation")
                                        .to_string(),
                                },
                                "modify" => HookDecision::Modify {
                                    modified_input: obj.get("modified_input").cloned(),
                                    modified_output: obj.get("modified_output").cloned(),
                                },
                                _ => HookDecision::Allow,
                            };
                        }
                    }
                }
            }
        }

        HookDecision::Allow
    }

    /// Check if this result indicates the operation should be denied
    pub fn is_denied(&self) -> bool {
        matches!(self.decision, HookDecision::Deny { .. })
    }

    /// Check if this result has modifications
    pub fn has_modifications(&self) -> bool {
        matches!(self.decision, HookDecision::Modify { .. })
    }
}

/// The type of hook execution strategy
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HookType {
    /// Shell command execution (default). Receives event JSON on stdin.
    /// Exit 0 = success, exit 2 = block operation, other = non-blocking error.
    Command,
    /// HTTP POST to a URL with event JSON as body.
    Http,
    /// LLM-based evaluation with prompt template substitution.
    /// Uses the configured LlmClient to evaluate the event and return a decision.
    Llm,
    /// Single-turn LLM evaluation. The prompt receives the event JSON
    /// and must return a JSON decision on stdout.
    Prompt,
    /// Call a tool on a connected MCP server.
    McpTool,
    /// Spawn a sub-agent (with Read/Grep/Glob tools) for validation.
    Agent,
}

impl Default for HookType {
    fn default() -> Self {
        Self::Command
    }
}

/// Definition of a single hook command within a hook group
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookDef {
    /// Shell command to execute (required for command type)
    pub command: String,
    /// Hook execution type (default: "command")
    #[serde(default)]
    pub r#type: HookType,
    /// URL to POST to (required for http type)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// HTTP headers for http type hooks
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub headers: HashMap<String, String>,
    /// Timeout in seconds (default: 30)
    #[serde(default = "default_hook_timeout")]
    pub timeout: u64,
    /// Whether to wait for the result before continuing (default: true)
    #[serde(default = "default_hook_blocking")]
    pub blocking: bool,
    /// Environment variables to pass to the hook command.
    /// Only listed vars from the current process env are forwarded.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_env_vars: Vec<String>,
    /// Shell to use for command execution (default: system shell).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shell: Option<String>,
    /// Prompt template for LLM hooks. Supports variable substitution:
    /// - `{{tool_name}}`: Name of the tool being executed
    /// - `{{input}}`: Tool input as JSON
    /// - `{{event_type}}`: Type of hook event
    /// - `{{event_json}}`: Full event JSON
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_template: Option<String>,
    /// Model override for LLM hooks (uses default client model if not specified)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

fn default_hook_timeout() -> u64 {
    30
}

fn default_hook_blocking() -> bool {
    true
}

impl HookDef {
    /// Create a new command hook definition
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            r#type: HookType::Command,
            url: None,
            headers: HashMap::new(),
            timeout: default_hook_timeout(),
            blocking: default_hook_blocking(),
            allowed_env_vars: Vec::new(),
            shell: None,
            prompt_template: None,
            model: None,
        }
    }

    /// Create an HTTP hook that POSTs event JSON to a URL
    pub fn new_http(url: impl Into<String>) -> Self {
        Self {
            command: String::new(),
            r#type: HookType::Http,
            url: Some(url.into()),
            headers: HashMap::new(),
            timeout: default_hook_timeout(),
            blocking: default_hook_blocking(),
            allowed_env_vars: Vec::new(),
            shell: None,
            prompt_template: None,
            model: None,
        }
    }

    /// Create a prompt hook that uses single-turn LLM evaluation
    pub fn new_prompt(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            r#type: HookType::Prompt,
            url: None,
            headers: HashMap::new(),
            timeout: default_hook_timeout(),
            blocking: default_hook_blocking(),
            allowed_env_vars: Vec::new(),
            shell: None,
            prompt_template: None,
            model: None,
        }
    }

    /// Create an LLM hook that evaluates the event using the LlmClient
    ///
    /// The prompt_template should contain variable placeholders like:
    /// - `{{tool_name}}`: Name of the tool being executed
    /// - `{{input}}`: Tool input as JSON
    /// - `{{event_type}}`: Type of hook event
    /// - `{{event_json}}`: Full event JSON
    pub fn new_llm(prompt_template: impl Into<String>) -> Self {
        Self {
            command: String::new(),
            r#type: HookType::Llm,
            url: None,
            headers: HashMap::new(),
            timeout: default_hook_timeout(),
            blocking: default_hook_blocking(),
            allowed_env_vars: Vec::new(),
            shell: None,
            prompt_template: Some(prompt_template.into()),
            model: None,
        }
    }

    /// Set the timeout for this hook
    pub fn with_timeout(mut self, timeout_secs: u64) -> Self {
        self.timeout = timeout_secs;
        self
    }

    /// Set whether this hook is blocking
    pub fn with_blocking(mut self, blocking: bool) -> Self {
        self.blocking = blocking;
        self
    }

    /// Add a header for HTTP hooks
    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(key.into(), value.into());
        self
    }

    /// Get the timeout as a Duration
    pub fn timeout_duration(&self) -> Duration {
        Duration::from_secs(self.timeout)
    }
}

/// A group of hooks with a matcher pattern
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookConfig {
    /// Matcher pattern: exact, `"*"` wildcard, pipe-separated `"Edit|Write"`, or regex
    pub matcher: String,
    /// Conditional: only fire if this tool+pattern matches (e.g. `"Bash(rm *)"`)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub if_condition: Option<String>,
    /// List of hook definitions to execute
    pub hooks: Vec<HookDef>,
}

impl HookConfig {
    /// Create a new hook config
    pub fn new(matcher: impl Into<String>) -> Self {
        Self {
            matcher: matcher.into(),
            if_condition: None,
            hooks: Vec::new(),
        }
    }

    /// Add a hook definition
    pub fn with_hook(mut self, hook: HookDef) -> Self {
        self.hooks.push(hook);
        self
    }

    /// Set the conditional expression
    pub fn with_condition(mut self, cond: impl Into<String>) -> Self {
        self.if_condition = Some(cond.into());
        self
    }

    /// Check if this config matches the given subject string.
    ///
    /// Matching supports:
    /// - `"*"` wildcard: matches everything
    /// - Pipe-separated: `"Edit|Write"` matches either
    /// - Exact string match
    /// - Regex pattern match (anchored to full string)
    /// - Fallback substring match if the pattern is not valid regex
    pub fn matches(&self, subject: &str) -> bool {
        if self.matcher == "*" {
            return true;
        }

        // Try exact match first
        if self.matcher == subject {
            return true;
        }

        // Try as anchored regex (handles patterns like Ba(sh|z), Bash.*, etc.)
        if let Ok(re) = regex::Regex::new(&format!("^{}$", self.matcher)) {
            return re.is_match(subject);
        }

        // Pipe-separated matching: "Edit|Write" matches either
        if self.matcher.contains('|') {
            return self.matcher.split('|').any(|part| {
                let part = part.trim();
                part == "*" || part == subject
            });
        }

        // Fall back to substring match
        subject.contains(&self.matcher)
    }
}

/// Top-level hooks configuration file structure
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HooksFile {
    /// Map of event type to list of hook configs
    pub hooks: HashMap<String, Vec<HookConfig>>,
}

impl HooksFile {
    /// Create an empty hooks file
    pub fn new() -> Self {
        Self::default()
    }

    /// Parse hooks file from a JSON string
    pub fn from_json(json: &str) -> Result<Self, HookError> {
        let hooks: HooksFile = serde_json::from_str(json)?;
        Ok(hooks)
    }

    /// Load hooks file from disk
    pub fn load_from_file(path: &Path) -> Result<Self, HookError> {
        if !path.exists() {
            return Ok(Self::new());
        }
        let content = std::fs::read_to_string(path)?;
        Self::from_json(&content)
    }

    /// Serialize to a JSON string
    pub fn to_json(&self) -> Result<String, HookError> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    /// Get hook configs for a specific event type
    pub fn get_for_event(&self, event_type: &HookEventType) -> Vec<&HookConfig> {
        let key = event_type.to_string();
        self.hooks
            .get(&key)
            .map(|configs| configs.iter().collect())
            .unwrap_or_default()
    }

    /// Merge another hooks file into this one.
    /// Entries from `other` are appended to existing entries for the same event type.
    pub fn merge(&mut self, other: HooksFile) {
        for (event_type, configs) in other.hooks {
            self.hooks.entry(event_type).or_default().extend(configs);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── HookDecision serialization ────────────────────────────────────────

    #[test]
    fn test_decision_allow_serialization() {
        let json = serde_json::to_string(&HookDecision::Allow).unwrap();
        assert!(json.contains("Allow"));
        let de: HookDecision = serde_json::from_str(&json).unwrap();
        assert_eq!(de, HookDecision::Allow);
    }

    #[test]
    fn test_decision_deny_serialization() {
        let d = HookDecision::Deny {
            reason: "bad".into(),
        };
        let json = serde_json::to_string(&d).unwrap();
        assert!(json.contains("Deny"));
        assert!(json.contains("bad"));
    }

    #[test]
    fn test_decision_modify_serialization() {
        let d = HookDecision::Modify {
            modified_input: Some(serde_json::json!({"key": "val"})),
            modified_output: None,
        };
        let json = serde_json::to_string(&d).unwrap();
        assert!(json.contains("Modify"));
        assert!(json.contains("modified_input"));
    }

    #[test]
    fn test_decision_default_is_allow() {
        assert_eq!(HookDecision::default(), HookDecision::Allow);
    }

    // ── HookResult::parse_decision ─────────────────────────────────────────

    #[test]
    fn test_parse_decision_allow_json() {
        let d = HookResult::parse_decision(r#"{"decision":"allow"}"#);
        assert_eq!(d, HookDecision::Allow);
    }

    #[test]
    fn test_parse_decision_deny_json() {
        let d = HookResult::parse_decision(r#"{"decision":"deny","reason":"nope"}"#);
        assert!(matches!(d, HookDecision::Deny { .. }));
    }

    #[test]
    fn test_parse_decision_modify_json() {
        let d = HookResult::parse_decision(r#"{"decision":"modify","modified_input":{"x":1}}"#);
        assert!(matches!(d, HookDecision::Modify { .. }));
    }

    #[test]
    fn test_parse_decision_empty_returns_allow() {
        assert_eq!(HookResult::parse_decision(""), HookDecision::Allow);
    }

    #[test]
    fn test_parse_decision_non_json_returns_allow() {
        assert_eq!(
            HookResult::parse_decision("just some text"),
            HookDecision::Allow
        );
    }

    #[test]
    fn test_parse_decision_invalid_json_returns_allow() {
        assert_eq!(
            HookResult::parse_decision("{not valid}"),
            HookDecision::Allow
        );
    }

    // ── HookResult helpers ─────────────────────────────────────────────────

    #[test]
    fn test_is_denied() {
        let r = HookResult {
            exit_code: 2,
            stdout: String::new(),
            stderr: "blocked".into(),
            decision: HookDecision::Deny {
                reason: "nope".into(),
            },
            command: "test".into(),
        };
        assert!(r.is_denied());
    }

    #[test]
    fn test_has_modifications() {
        let r = HookResult {
            exit_code: 0,
            stdout: String::new(),
            stderr: String::new(),
            decision: HookDecision::Modify {
                modified_input: None,
                modified_output: None,
            },
            command: "test".into(),
        };
        assert!(r.has_modifications());
    }

    #[test]
    fn test_allow_result_not_denied_no_mods() {
        let r = HookResult {
            exit_code: 0,
            stdout: String::new(),
            stderr: String::new(),
            decision: HookDecision::Allow,
            command: "test".into(),
        };
        assert!(!r.is_denied());
        assert!(!r.has_modifications());
    }

    // ── HookType serialization ──────────────────────────────────────────────

    #[test]
    fn test_hook_type_serialization() {
        assert_eq!(
            serde_json::to_string(&HookType::Command).unwrap(),
            "\"command\""
        );
        assert_eq!(serde_json::to_string(&HookType::Http).unwrap(), "\"http\"");
        assert_eq!(serde_json::to_string(&HookType::Llm).unwrap(), "\"llm\"");
        assert_eq!(
            serde_json::to_string(&HookType::Prompt).unwrap(),
            "\"prompt\""
        );
        assert_eq!(
            serde_json::to_string(&HookType::McpTool).unwrap(),
            "\"mcptool\""
        );
        assert_eq!(
            serde_json::to_string(&HookType::Agent).unwrap(),
            "\"agent\""
        );
    }

    #[test]
    fn test_hook_type_default() {
        assert_eq!(HookType::default(), HookType::Command);
    }

    // ── HookDef construction ────────────────────────────────────────────────

    #[test]
    fn test_hook_def_new_command() {
        let def = HookDef::new("echo hello");
        assert_eq!(def.command, "echo hello");
        assert_eq!(def.r#type, HookType::Command);
        assert!(def.blocking);
        assert_eq!(def.timeout, 30);
    }

    #[test]
    fn test_hook_def_new_http() {
        let def = HookDef::new_http("https://example.com/hook");
        assert_eq!(def.r#type, HookType::Http);
        assert_eq!(def.url.unwrap(), "https://example.com/hook");
    }

    #[test]
    fn test_hook_def_new_prompt() {
        let def = HookDef::new_prompt("evaluate-event");
        assert_eq!(def.r#type, HookType::Prompt);
        assert_eq!(def.command, "evaluate-event");
    }

    #[test]
    fn test_hook_def_new_llm() {
        let def = HookDef::new_llm("Check if {{tool_name}} is safe");
        assert_eq!(def.r#type, HookType::Llm);
        assert_eq!(
            def.prompt_template.unwrap(),
            "Check if {{tool_name}} is safe"
        );
    }

    #[test]
    fn test_hook_def_builder_methods() {
        let def = HookDef::new("test")
            .with_timeout(60)
            .with_blocking(false)
            .with_header("Authorization", "Bearer token");
        assert_eq!(def.timeout, 60);
        assert!(!def.blocking);
        assert_eq!(def.headers.get("Authorization").unwrap(), "Bearer token");
        assert_eq!(def.timeout_duration(), Duration::from_secs(60));
    }

    // ── HookConfig::matches ────────────────────────────────────────────────

    #[test]
    fn test_matches_wildcard() {
        let cfg = HookConfig::new("*");
        assert!(cfg.matches("anything"));
        assert!(cfg.matches("Bash"));
    }

    #[test]
    fn test_matches_exact() {
        let cfg = HookConfig::new("Bash");
        assert!(cfg.matches("Bash"));
        assert!(!cfg.matches("Read"));
    }

    #[test]
    fn test_matches_pipe_separated() {
        let cfg = HookConfig::new("Edit|Write");
        assert!(cfg.matches("Edit"));
        assert!(cfg.matches("Write"));
        assert!(!cfg.matches("Bash"));
    }

    #[test]
    fn test_matches_regex() {
        let cfg = HookConfig::new("Ba.*");
        assert!(cfg.matches("Bash"));
        assert!(!cfg.matches("Read"));
    }

    #[test]
    fn test_matches_substring_fallback() {
        // Pattern "[unclosed" is not valid regex, so falls through to substring
        let cfg = HookConfig::new("[unclosed");
        assert!(cfg.matches("test [unclosed bracket"));
        assert!(!cfg.matches("no match here"));
    }

    // ── HooksFile ───────────────────────────────────────────────────────────

    #[test]
    fn test_hooks_file_new_empty() {
        let hf = HooksFile::new();
        assert!(hf.hooks.is_empty());
    }

    #[test]
    fn test_hooks_file_from_json() {
        let json = r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"command":"echo"}]}]}}"#;
        let hf = HooksFile::from_json(json).unwrap();
        assert!(hf.hooks.contains_key("PreToolUse"));
    }

    #[test]
    fn test_hooks_file_merge() {
        let mut hf1 = HooksFile::new();
        let mut hf2 = HooksFile::new();
        hf2.hooks
            .insert("PreToolUse".into(), vec![HookConfig::new("Bash")]);
        hf1.merge(hf2);
        assert!(hf1.hooks.contains_key("PreToolUse"));
    }

    #[test]
    fn test_hooks_file_to_json_roundtrip() {
        let mut hf = HooksFile::new();
        hf.hooks
            .insert("PostToolUse".into(), vec![HookConfig::new("Write")]);
        let json = hf.to_json().unwrap();
        let hf2 = HooksFile::from_json(&json).unwrap();
        assert!(hf2.hooks.contains_key("PostToolUse"));
    }

    // ── Send + Sync ────────────────────────────────────────────────────────

    #[test]
    fn test_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<HookDecision>();
        assert_send_sync::<HookResult>();
        assert_send_sync::<HookType>();
        assert_send_sync::<HookDef>();
        assert_send_sync::<HookConfig>();
        assert_send_sync::<HooksFile>();
    }
}
