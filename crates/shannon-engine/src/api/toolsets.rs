//! Anthropic browser/computer toolset support (T12 Option C).
//!
//! Anthropic consolidated browser/computer automation around versioned
//! *toolsets* — a single nameless `tools[]` entry (e.g.
//! `{"type":"browser_toolset_20260801"}`) whose member tools are expanded
//! server-side. This module implements Shannon's Option C dispatch:
//! toolset entries are only ever injected for Anthropic requests (opt-in),
//! and they supersede locally-registered tools they make redundant —
//! the `computer` desktop tool and browser MCP servers (Playwright /
//! Chrome DevTools). Non-Anthropic providers never see toolset entries.
//!
//! Only the **browser** toolset is injected today: its members execute
//! server-side (Anthropic-hosted Chrome), so no local execution path is
//! needed. The computer toolset's members are client-executed; routing
//! toolset-form `tool_use` blocks to local execution is a documented
//! follow-up (see docs/plans/2026-09-06-p3-future-research.md §T12).

use serde_json::{Value, json};

/// Browser toolset version (per Anthropic Python SDK; supersedes
/// `browser_toolset_20260302`, which never shipped).
pub const BROWSER_TOOLSET_TYPE: &str = "browser_toolset_20260801";

/// Computer toolset version (client-executed members; not yet injected —
/// see module docs).
pub const COMPUTER_TOOLSET_TYPE: &str = "computer_toolset_20260801";

/// Umbrella beta header for the computer/browser toolset family.
pub const COMPUTER_USE_BETA: &str = "computer-use-2025-11-24";

/// Whether `model_id` is in a family known to serve the toolset API.
/// Conservative prefix match over the Claude 4.x / 5.x Opus & Sonnet lines.
pub fn model_supports_toolsets(model_id: &str) -> bool {
    let m = model_id.to_ascii_lowercase();
    [
        "claude-opus-4",
        "claude-sonnet-4",
        "claude-opus-5",
        "claude-sonnet-5",
    ]
    .iter()
    .any(|prefix| m.starts_with(prefix))
}

/// The `tools[]` entry for the browser toolset. `configs` is omitted to
/// take the server defaults for every member (30 tools).
pub fn browser_toolset_entry() -> Value {
    json!({ "type": BROWSER_TOOLSET_TYPE })
}

/// The `tools[]` entry for the computer toolset (all-default members).
#[allow(dead_code)] // KEEP: wired alongside local computer-toolset routing (follow-up)
pub fn computer_toolset_entry() -> Value {
    json!({ "type": COMPUTER_TOOLSET_TYPE })
}

/// Env knob gating toolset injection (`SHANNON_ANTHROPIC_TOOLSETS=1`).
/// Opt-in so default builds are byte-identical to pre-toolset behavior.
pub fn anthropic_toolsets_from_env() -> bool {
    std::env::var("SHANNON_ANTHROPIC_TOOLSETS")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// Whether a locally-registered tool name is superseded by an injected
/// browser toolset: the local desktop `computer` tool (web pages are
/// better served server-side) and browser MCP servers (Playwright /
/// Chrome DevTools / generic `browser_*` members).
pub fn is_superseded_by_browser_toolset(tool_name: &str) -> bool {
    if tool_name == "computer" {
        return true;
    }
    if let Some(rest) = tool_name.strip_prefix("mcp__") {
        let lower = rest.to_ascii_lowercase();
        return lower.contains("playwright")
            || lower.contains("chrome-devtools")
            || lower.contains("browser");
    }
    false
}

/// Mutate a serialized Anthropic request body in place: drop tools
/// superseded by the browser toolset and append the toolset entry.
///
/// No-op unless `enabled` is true. When the request carries no `tools`
/// array (tools disabled for the query), injection is skipped — a toolset
/// must not silently re-enable tools the caller turned off.
pub fn apply_browser_toolset(body: &mut Value, enabled: bool) {
    if !enabled {
        return;
    }
    let Some(tools) = body.get_mut("tools").and_then(|t| t.as_array_mut()) else {
        return;
    };
    tools.retain(|t| {
        t.get("name")
            .and_then(|n| n.as_str())
            .map(|n| !is_superseded_by_browser_toolset(n))
            .unwrap_or(true) // keep opaque entries (defensive)
    });
    tools.push(browser_toolset_entry());
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_browser_toolset_entry_shape() {
        let entry = browser_toolset_entry();
        assert_eq!(entry["type"], json!(BROWSER_TOOLSET_TYPE));
        // Nameless: toolset entries carry no `name`/`input_schema`.
        assert!(entry.get("name").is_none());
        assert!(entry.get("input_schema").is_none());
    }

    #[test]
    fn test_model_gate() {
        assert!(model_supports_toolsets("claude-opus-4-5"));
        assert!(model_supports_toolsets("claude-sonnet-4-5"));
        assert!(model_supports_toolsets("Claude-Opus-4-6"));
        assert!(!model_supports_toolsets("claude-3-5-sonnet"));
        assert!(!model_supports_toolsets("gpt-5"));
        assert!(!model_supports_toolsets(""));
    }

    #[test]
    fn test_superseded_names() {
        assert!(is_superseded_by_browser_toolset("computer"));
        assert!(is_superseded_by_browser_toolset(
            "mcp__playwright__browser_navigate"
        ));
        assert!(is_superseded_by_browser_toolset(
            "mcp__chrome-devtools__take_screenshot"
        ));
        assert!(!is_superseded_by_browser_toolset("Read"));
        assert!(!is_superseded_by_browser_toolset(
            "mcp__github__create_issue"
        ));
    }

    #[test]
    fn test_apply_injects_and_prunes() {
        let mut body = json!({
            "model": "claude-opus-4-5",
            "tools": [
                {"name": "Read", "description": "read", "input_schema": {}},
                {"name": "computer", "description": "desktop", "input_schema": {}},
                {"name": "mcp__playwright__browser_click", "description": "x", "input_schema": {}}
            ]
        });
        apply_browser_toolset(&mut body, true);
        let tools = body["tools"].as_array().unwrap();
        let names: Vec<&str> = tools
            .iter()
            .filter_map(|t| t.get("name").and_then(|n| n.as_str()))
            .collect();
        assert_eq!(names, vec!["Read"]);
        assert_eq!(tools.last().unwrap()["type"], json!(BROWSER_TOOLSET_TYPE));
    }

    #[test]
    fn test_apply_disabled_and_empty_tools_are_noops() {
        let mut disabled = json!({"tools": [{"name": "computer", "input_schema": {}}]});
        apply_browser_toolset(&mut disabled, false);
        assert_eq!(disabled["tools"].as_array().unwrap().len(), 1);

        // Tools turned off for the query: must NOT re-enable via toolset.
        let mut no_tools = json!({"model": "claude-opus-4-5"});
        apply_browser_toolset(&mut no_tools, true);
        assert!(no_tools.get("tools").is_none());
    }
}
