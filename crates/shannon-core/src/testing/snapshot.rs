//! Snapshot regression testing helpers for API request shape verification.
//!
//! Renders API requests into deterministic strings for snapshot comparison,
//! catching unintended changes in prompt construction, context management,
//! and tool definition structure.

use serde_json::Value;
use std::fmt::Write;

/// A rendered snapshot of an API request.
#[derive(Debug, Clone)]
pub struct RequestSnapshot {
    pub messages: Vec<SnapshotMessage>,
    pub tools: Vec<String>,
    pub system_prompt_hash: String,
    pub total_tokens_estimated: usize,
}

/// A single message in a request snapshot.
#[derive(Debug, Clone)]
pub struct SnapshotMessage {
    pub role: String,
    pub kind: String,
    pub content_preview: String,
}

/// How much detail to include in the snapshot render.
#[derive(Debug, Clone, Copy)]
pub enum RenderMode {
    /// Full message content.
    FullText,
    /// Strip large bodies, keep structure.
    RedactedText,
    /// Just message types (e.g., "user/text", "assistant/tool_use").
    KindOnly,
}

/// Result of comparing two snapshots.
#[derive(Debug)]
pub struct SnapshotDiff {
    pub matches: bool,
    pub message_count_diff: Option<String>,
    pub tool_count_diff: Option<String>,
    pub role_differences: Vec<String>,
    pub kind_differences: Vec<String>,
    pub content_differences: Vec<String>,
}

impl SnapshotDiff {
    pub fn is_match(&self) -> bool {
        self.matches
    }
}

/// Render an API request body into a deterministic string for snapshot comparison.
///
/// The request should be a JSON value with `messages`, `tools`, and optionally `system` fields.
pub fn render_request_snapshot(request: &Value, mode: RenderMode) -> String {
    let mut output = String::new();

    // System prompt
    if let Some(system) = request.get("system") {
        match mode {
            RenderMode::KindOnly => {
                writeln!(output, "system: present").expect("snapshot render");
            }
            RenderMode::RedactedText => {
                let hash = simple_hash(&system.to_string());
                writeln!(output, "system: [hash:{hash}]").expect("snapshot render");
            }
            RenderMode::FullText => {
                writeln!(output, "system: {}", truncate(&system.to_string(), 500)).expect("snapshot render");
            }
        }
    }

    // Model
    if let Some(model) = request.get("model").and_then(|m| m.as_str()) {
        writeln!(output, "model: {model}").expect("snapshot render");
    }

    // Tools
    if let Some(tools) = request.get("tools").and_then(|t| t.as_array()) {
        writeln!(output, "tools: {}", tools.len()).expect("snapshot render");
        for (i, tool) in tools.iter().enumerate() {
            let name = tool
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("unknown");
            match mode {
                RenderMode::KindOnly => {
                    writeln!(output, "  tool[{i}]: {name}").expect("snapshot render");
                }
                RenderMode::RedactedText | RenderMode::FullText => {
                    let desc = tool
                        .get("description")
                        .and_then(|d| d.as_str())
                        .unwrap_or("");
                    writeln!(output, "  tool[{i}]: {name} - {}", truncate(desc, 80)).expect("snapshot render");
                }
            }
        }
    }

    // Messages
    if let Some(messages) = request.get("messages").and_then(|m| m.as_array()) {
        writeln!(output, "messages: {}", messages.len()).expect("snapshot render");
        for (i, msg) in messages.iter().enumerate() {
            let role = msg
                .get("role")
                .and_then(|r| r.as_str())
                .unwrap_or("unknown");
            let content = msg.get("content");

            match mode {
                RenderMode::KindOnly => {
                    let kind = classify_content_kind(content);
                    writeln!(output, "  {i:03}: {role}/{kind}").expect("snapshot render");
                }
                RenderMode::RedactedText => {
                    let kind = classify_content_kind(content);
                    let preview = content_preview(content, 100);
                    writeln!(output, "  {i:03}: {role}/{kind} {preview}").expect("snapshot render");
                }
                RenderMode::FullText => {
                    let kind = classify_content_kind(content);
                    let preview = content_preview(content, 300);
                    writeln!(output, "  {i:03}: {role}/{kind} {preview}").expect("snapshot render");
                }
            }
        }
    }

    output
}

/// Render a tool call sequence as a deterministic string for snapshot comparison.
pub fn snapshot_tool_chain(calls: &[(String, Value, String, bool)]) -> String {
    let mut output = String::new();
    writeln!(output, "tool_chain: {} steps", calls.len()).expect("snapshot render");
    for (i, (name, input, _result, is_error)) in calls.iter().enumerate() {
        let status = if *is_error { "ERROR" } else { "OK" };
        let input_preview = truncate(&input.to_string(), 120);
        writeln!(output, "  {i}: {name}({input_preview}) [{status}]").expect("snapshot render");
    }
    output
}

/// Compare two snapshot strings and produce a structured diff.
pub fn diff_snapshots(expected: &str, actual: &str) -> SnapshotDiff {
    let expected_lines: Vec<&str> = expected.lines().collect();
    let actual_lines: Vec<&str> = actual.lines().collect();

    let mut diff = SnapshotDiff {
        matches: expected == actual,
        message_count_diff: None,
        tool_count_diff: None,
        role_differences: Vec::new(),
        kind_differences: Vec::new(),
        content_differences: Vec::new(),
    };

    // Compare line by line, categorizing differences
    let max_len = expected_lines.len().max(actual_lines.len());
    for i in 0..max_len {
        let e = expected_lines.get(i).copied().unwrap_or("<missing>");
        let a = actual_lines.get(i).copied().unwrap_or("<extra>");

        if e != a {
            if e.starts_with("messages:") || a.starts_with("messages:") {
                diff.message_count_diff = Some(format!("expected: {e}, actual: {a}"));
            } else if e.starts_with("tools:") || a.starts_with("tools:") {
                diff.tool_count_diff = Some(format!("expected: {e}, actual: {a}"));
            } else if e.contains("/") && a.contains("/") {
                // message line with role/kind
                let e_parts: Vec<&str> = e.split('/').collect();
                let a_parts: Vec<&str> = a.split('/').collect();
                if e_parts.len() >= 2 && a_parts.len() >= 2 {
                    if e_parts[0] != a_parts[0] {
                        diff.role_differences.push(format!("line {i}: {e} != {a}"));
                    }
                    if e_parts.get(1) != a_parts.get(1) {
                        diff.kind_differences.push(format!("line {i}: {e} != {a}"));
                    }
                }
            } else {
                diff.content_differences
                    .push(format!("line {i}: {e} != {a}"));
            }
        }
    }

    diff
}

// ── Internal helpers ───────────────────────────────────────────────────

fn classify_content_kind(content: Option<&Value>) -> String {
    match content {
        None => "none".to_string(),
        Some(Value::String(_)) => "text".to_string(),
        Some(Value::Array(blocks)) => {
            let kinds: Vec<String> = blocks
                .iter()
                .map(|b| {
                    b.get("type")
                        .and_then(|t| t.as_str())
                        .unwrap_or("unknown")
                        .to_string()
                })
                .collect();
            if kinds.len() == 1 {
                kinds[0].clone()
            } else {
                "multi".to_string()
            }
        }
        Some(_) => "other".to_string(),
    }
}

fn content_preview(content: Option<&Value>, max_len: usize) -> String {
    match content {
        None => String::new(),
        Some(Value::String(s)) => truncate(s, max_len),
        Some(Value::Array(blocks)) => {
            let previews: Vec<String> = blocks
                .iter()
                .map(|b| {
                    let kind = b.get("type").and_then(|t| t.as_str()).unwrap_or("?");
                    match kind {
                        "text" => {
                            let text = b.get("text").and_then(|t| t.as_str()).unwrap_or("");
                            format!("text:{}", truncate(text, 40))
                        }
                        "tool_use" => {
                            let name = b.get("name").and_then(|n| n.as_str()).unwrap_or("?");
                            format!("tool_use:{name}")
                        }
                        "tool_result" => {
                            let id = b.get("tool_use_id").and_then(|t| t.as_str()).unwrap_or("?");
                            format!("tool_result:{id}")
                        }
                        "thinking" => {
                            let text = b.get("thinking").and_then(|t| t.as_str()).unwrap_or("");
                            format!("thinking:{}", truncate(text, 30))
                        }
                        _ => kind.to_string(),
                    }
                })
                .collect();
            truncate(&previews.join(", "), max_len)
        }
        Some(v) => truncate(&v.to_string(), max_len),
    }
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        let mut end = max_len;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &s[..end])
    }
}

/// Simple deterministic hash for content identification.
fn simple_hash(s: &str) -> String {
    let mut hash: u64 = 5381;
    for byte in s.bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(byte as u64);
    }
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_render_empty_request() {
        let request = json!({});
        let snapshot = render_request_snapshot(&request, RenderMode::KindOnly);
        assert!(snapshot.is_empty() || snapshot.lines().all(|l| l.is_empty()));
    }

    #[test]
    fn test_render_basic_request_kind_only() {
        let request = json!({
            "model": "test-model",
            "messages": [
                {"role": "user", "content": "Hello"},
                {"role": "assistant", "content": "Hi there"}
            ]
        });
        let snapshot = render_request_snapshot(&request, RenderMode::KindOnly);
        assert!(snapshot.contains("model: test-model"));
        assert!(snapshot.contains("messages: 2"));
        assert!(snapshot.contains("000: user/text"));
        assert!(snapshot.contains("001: assistant/text"));
    }

    #[test]
    fn test_render_request_with_tools() {
        let request = json!({
            "model": "test-model",
            "tools": [
                {"name": "Read", "description": "Read a file"},
                {"name": "Edit", "description": "Edit a file"}
            ],
            "messages": [
                {"role": "user", "content": "Fix the bug"}
            ]
        });
        let snapshot = render_request_snapshot(&request, RenderMode::KindOnly);
        assert!(snapshot.contains("tools: 2"));
        assert!(snapshot.contains("tool[0]: Read"));
        assert!(snapshot.contains("tool[1]: Edit"));
    }

    #[test]
    fn test_render_request_redacted() {
        let long_content = "x".repeat(1000);
        let request = json!({
            "system": "You are a helpful assistant",
            "messages": [
                {"role": "user", "content": long_content}
            ]
        });
        let snapshot = render_request_snapshot(&request, RenderMode::RedactedText);
        assert!(snapshot.contains("system: [hash:"));
        assert!(snapshot.contains("user/text"));
    }

    #[test]
    fn test_snapshot_tool_chain() {
        let calls = vec![
            (
                "Read".to_string(),
                json!({"path": "src/main.rs"}),
                "fn main() {}".to_string(),
                false,
            ),
            (
                "Edit".to_string(),
                json!({"path": "src/main.rs"}),
                "ok".to_string(),
                false,
            ),
            (
                "Bash".to_string(),
                json!({"command": "cargo check"}),
                "error".to_string(),
                true,
            ),
        ];
        let snapshot = snapshot_tool_chain(&calls);
        assert!(snapshot.contains("tool_chain: 3 steps"));
        assert!(snapshot.contains("Read"));
        assert!(snapshot.contains("[OK]"));
        assert!(snapshot.contains("[ERROR]"));
    }

    #[test]
    fn test_diff_matching_snapshots() {
        let snap = "messages: 1\n  000: user/text\n";
        let diff = diff_snapshots(snap, snap);
        assert!(diff.is_match());
        assert!(diff.content_differences.is_empty());
    }

    #[test]
    fn test_diff_different_message_counts() {
        let expected = "messages: 2\n  000: user/text\n  001: assistant/text\n";
        let actual = "messages: 1\n  000: user/text\n";
        let diff = diff_snapshots(expected, actual);
        assert!(!diff.is_match());
        assert!(diff.message_count_diff.is_some());
    }

    #[test]
    fn test_simple_hash_deterministic() {
        let h1 = simple_hash("hello world");
        let h2 = simple_hash("hello world");
        let h3 = simple_hash("hello earth");
        assert_eq!(h1, h2);
        assert_ne!(h1, h3);
    }

    #[test]
    fn test_truncate_short_string() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_long_string() {
        let result = truncate(&"x".repeat(100), 50);
        assert_eq!(result.len(), 53); // 50 chars + "..."
        assert!(result.ends_with("..."));
    }

    #[test]
    fn test_render_multi_block_content() {
        let request = json!({
            "messages": [
                {"role": "assistant", "content": [
                    {"type": "text", "text": "I'll read the file"},
                    {"type": "tool_use", "id": "toolu_1", "name": "Read", "input": {"path": "a.rs"}}
                ]}
            ]
        });
        let snapshot = render_request_snapshot(&request, RenderMode::KindOnly);
        assert!(snapshot.contains("assistant/multi"));
    }
}
