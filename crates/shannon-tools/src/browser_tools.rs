//! Built-in browser tools (T14 Phase 1).
//!
//! Seven builtin tools that share the [`chrome_session::ChromeSession`]
//! singleton — together they cover the equivalent of MCP Playwright's
//! core surface (navigate / click / type / snapshot / screenshot /
//! tabs / close) but with the browser process owned by Shannon itself.
//!
//! All seven tools register unconditionally (the feature flag only
//! gates their behavior — `#[cfg(feature = "local-browser")]`); when
//! the flag is off, every action returns an explanatory stub error so
//! the tool surface stays stable across platforms.

use crate::Tool;
use crate::ToolError;
use crate::chrome_session::{self, ChromeSession, TabId};
use async_trait::async_trait;
use base64::Engine as _;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;

// ── browser_navigate ────────────────────────────────────────────────────
pub struct BrowserNavigateTool;

#[async_trait]
impl Tool for BrowserNavigateTool {
    fn name(&self) -> &str {
        "browser_navigate"
    }
    fn description(&self) -> &str {
        "Open a URL in a new browser tab. Returns the new tab_id for subsequent actions. Requires the `local-browser` feature."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": {"type": "string", "description": "The URL to navigate to (http/https)"},
                "tab_id": {"type": "string", "description": "Optional existing tab to reuse; if omitted a new tab is opened"}
            },
            "required": ["url"]
        })
    }
    fn is_read_only(&self) -> bool {
        false
    }
    fn is_destructive(&self) -> bool {
        false
    }
    fn is_concurrency_safe(&self) -> bool {
        true
    }

    async fn execute(&self, input: Value) -> crate::ToolResult<crate::ToolOutput> {
        let url = input["url"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidInput("missing field \"url\"".into()))?
            .to_string();
        let existing: Option<TabId> = input
            .get("tab_id")
            .and_then(|v| v.as_str())
            .map(|s| TabId(s.to_string()));
        let session: Arc<ChromeSession> = ChromeSession::global().await?;
        let tab_id = match existing {
            Some(id) => {
                let p = session.get_page(&id).await?;
                chrome_session::navigate(&p, &url).await?;
                id
            }
            None => {
                // open_page returns the new tab's id directly — deriving it
                // from list_tabs() would race the HashMap iteration order
                // and could pick an unrelated pre-existing tab.
                session.open_page(&url).await?
            }
        };
        let mut m = HashMap::new();
        m.insert("tab_id".into(), json!(tab_id.0));
        m.insert("url".into(), json!(url));
        Ok(crate::ToolOutput {
            content: format!("navigated to {url}"),
            is_error: false,
            metadata: m,
        })
    }
}

// ── browser_click ────────────────────────────────────────────────────────
pub struct BrowserClickTool;

#[async_trait]
impl Tool for BrowserClickTool {
    fn name(&self) -> &str {
        "browser_click"
    }
    fn description(&self) -> &str {
        "Click on the given tab. Prefer `ref` (an element id like \"e3\" from browser_snapshot) — real mouse events fire at the element's center. Fall back to viewport coordinates (x, y) when no snapshot exists."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "tab_id": {"type": "string"},
                "ref": {"type": "string", "description": "Element ref from browser_snapshot, e.g. \"e3\""},
                "x": {"type": "number", "description": "Viewport x (when no ref)"},
                "y": {"type": "number", "description": "Viewport y (when no ref)"}
            },
            "required": ["tab_id"]
        })
    }
    fn is_read_only(&self) -> bool {
        false
    }
    fn is_destructive(&self) -> bool {
        true
    }
    fn is_concurrency_safe(&self) -> bool {
        false
    }

    async fn execute(&self, input: Value) -> crate::ToolResult<crate::ToolOutput> {
        let tab = TabId(
            input["tab_id"]
                .as_str()
                .ok_or_else(|| ToolError::InvalidInput("missing field \"tab_id\"".into()))?
                .to_string(),
        );
        let session = ChromeSession::global().await?;
        let p = session.get_page(&tab).await?;
        let (label, metadata_ref) =
            if let Some(reference) = input.get("ref").and_then(|v| v.as_str()) {
                chrome_session::click_element(&p, reference).await?;
                (
                    format!("clicked element {reference}"),
                    reference.to_string(),
                )
            } else {
                let x = input.get("x").and_then(|v| v.as_f64()).ok_or_else(|| {
                    ToolError::InvalidInput(
                        "provide \"ref\" (from browser_snapshot) or numeric \"x\"/\"y\"".into(),
                    )
                })?;
                let y = input.get("y").and_then(|v| v.as_f64()).ok_or_else(|| {
                    ToolError::InvalidInput(
                        "provide \"ref\" (from browser_snapshot) or numeric \"x\"/\"y\"".into(),
                    )
                })?;
                chrome_session::click_at(&p, x, y).await?;
                (format!("clicked at ({x}, {y})"), String::new())
            };
        let mut m = HashMap::new();
        m.insert("tab_id".into(), json!(tab.0));
        if !metadata_ref.is_empty() {
            m.insert("ref".into(), json!(metadata_ref));
        }
        Ok(crate::ToolOutput {
            content: label,
            is_error: false,
            metadata: m,
        })
    }
}

// ── browser_type ─────────────────────────────────────────────────────────
pub struct BrowserTypeTool;

#[async_trait]
impl Tool for BrowserTypeTool {
    fn name(&self) -> &str {
        "browser_type"
    }
    fn description(&self) -> &str {
        "Type text into the focused field on the given tab."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "tab_id": {"type": "string"},
                "text": {"type": "string"}
            },
            "required": ["tab_id", "text"]
        })
    }
    fn is_read_only(&self) -> bool {
        false
    }
    fn is_destructive(&self) -> bool {
        true
    }
    fn is_concurrency_safe(&self) -> bool {
        false
    }

    async fn execute(&self, input: Value) -> crate::ToolResult<crate::ToolOutput> {
        let tab = TabId(
            input["tab_id"]
                .as_str()
                .ok_or_else(|| ToolError::InvalidInput("missing field \"tab_id\"".into()))?
                .to_string(),
        );
        let text = input["text"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidInput("missing field \"text\"".into()))?
            .to_string();
        let session = ChromeSession::global().await?;
        let p = session.get_page(&tab).await?;
        chrome_session::type_text(&p, &text).await?;
        let mut m = HashMap::new();
        m.insert("tab_id".into(), json!(tab.0));
        Ok(crate::ToolOutput {
            content: format!("typed {} chars", text.len()),
            is_error: false,
            metadata: m,
        })
    }
}

// ── browser_snapshot ─────────────────────────────────────────────────────
pub struct BrowserSnapshotTool;

#[async_trait]
impl Tool for BrowserSnapshotTool {
    fn name(&self) -> &str {
        "browser_snapshot"
    }
    fn description(&self) -> &str {
        "Return the page's title, URL, and an index of visible interactive elements (links, buttons, inputs, …) with element refs usable by browser_click/browser_fill."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {"tab_id": {"type": "string"}},
            "required": ["tab_id"]
        })
    }
    fn is_read_only(&self) -> bool {
        true
    }
    fn is_concurrency_safe(&self) -> bool {
        true
    }

    async fn execute(&self, input: Value) -> crate::ToolResult<crate::ToolOutput> {
        let tab = TabId(
            input["tab_id"]
                .as_str()
                .ok_or_else(|| ToolError::InvalidInput("missing field \"tab_id\"".into()))?
                .to_string(),
        );
        let session = ChromeSession::global().await?;
        let p = session.get_page(&tab).await?;
        let text = chrome_session::element_snapshot(&p).await?;
        Ok(crate::ToolOutput {
            content: text,
            is_error: false,
            metadata: HashMap::new(),
        })
    }
}

// ── browser_text (plain page text, replaces the old innerText snapshot) ──
pub struct BrowserTextTool;

#[async_trait]
impl Tool for BrowserTextTool {
    fn name(&self) -> &str {
        "browser_text"
    }
    fn description(&self) -> &str {
        "Return the page's full text content (title + URL + innerText, ≤8KB) for reading articles/documentation. For interacting with controls use browser_snapshot instead."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {"tab_id": {"type": "string"}},
            "required": ["tab_id"]
        })
    }
    fn is_read_only(&self) -> bool {
        true
    }
    fn is_concurrency_safe(&self) -> bool {
        true
    }

    async fn execute(&self, input: Value) -> crate::ToolResult<crate::ToolOutput> {
        let tab = TabId(
            input["tab_id"]
                .as_str()
                .ok_or_else(|| ToolError::InvalidInput("missing field \"tab_id\"".into()))?
                .to_string(),
        );
        let session = ChromeSession::global().await?;
        let p = session.get_page(&tab).await?;
        let text = chrome_session::page_text(&p).await?;
        Ok(crate::ToolOutput {
            content: text,
            is_error: false,
            metadata: HashMap::new(),
        })
    }
}

// ── browser_fill ─────────────────────────────────────────────────────────
pub struct BrowserFillTool;

#[async_trait]
impl Tool for BrowserFillTool {
    fn name(&self) -> &str {
        "browser_fill"
    }
    fn description(&self) -> &str {
        "Set the value of an input/textarea/contenteditable element by ref (from browser_snapshot). Fires input+change events so reactive frameworks see the value."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "tab_id": {"type": "string"},
                "ref": {"type": "string", "description": "Element ref from browser_snapshot, e.g. \"e7\""},
                "text": {"type": "string"}
            },
            "required": ["tab_id", "ref", "text"]
        })
    }
    fn is_read_only(&self) -> bool {
        false
    }
    fn is_destructive(&self) -> bool {
        true
    }
    fn is_concurrency_safe(&self) -> bool {
        false
    }

    async fn execute(&self, input: Value) -> crate::ToolResult<crate::ToolOutput> {
        let tab = TabId(
            input["tab_id"]
                .as_str()
                .ok_or_else(|| ToolError::InvalidInput("missing field \"tab_id\"".into()))?
                .to_string(),
        );
        let reference = input["ref"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidInput("missing field \"ref\"".into()))?
            .to_string();
        let text = input["text"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidInput("missing field \"text\"".into()))?
            .to_string();
        let session = ChromeSession::global().await?;
        let p = session.get_page(&tab).await?;
        chrome_session::fill_element(&p, &reference, &text).await?;
        let mut m = HashMap::new();
        m.insert("tab_id".into(), json!(tab.0));
        m.insert("ref".into(), json!(reference));
        Ok(crate::ToolOutput {
            content: format!("filled {reference} ({} chars)", text.chars().count()),
            is_error: false,
            metadata: m,
        })
    }
}

// ── browser_press_key ────────────────────────────────────────────────────
pub struct BrowserPressKeyTool;

#[async_trait]
impl Tool for BrowserPressKeyTool {
    fn name(&self) -> &str {
        "browser_press_key"
    }
    fn description(&self) -> &str {
        "Press a key on the given tab (e.g. \"Enter\", \"Escape\", \"Tab\", \"ArrowDown\", \"a\", \"F5\"). Goes to the focused element."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "tab_id": {"type": "string"},
                "key": {"type": "string", "description": "Key name, e.g. Enter/Escape/Tab/ArrowDown/F5 or a single character"}
            },
            "required": ["tab_id", "key"]
        })
    }
    fn is_read_only(&self) -> bool {
        false
    }
    fn is_destructive(&self) -> bool {
        true
    }
    fn is_concurrency_safe(&self) -> bool {
        false
    }

    async fn execute(&self, input: Value) -> crate::ToolResult<crate::ToolOutput> {
        let tab = TabId(
            input["tab_id"]
                .as_str()
                .ok_or_else(|| ToolError::InvalidInput("missing field \"tab_id\"".into()))?
                .to_string(),
        );
        let key = input["key"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidInput("missing field \"key\"".into()))?
            .to_string();
        let session = ChromeSession::global().await?;
        let p = session.get_page(&tab).await?;
        chrome_session::press_key(&p, &key).await?;
        let mut m = HashMap::new();
        m.insert("tab_id".into(), json!(tab.0));
        Ok(crate::ToolOutput {
            content: format!("pressed {key}"),
            is_error: false,
            metadata: m,
        })
    }
}

// ── browser_scroll ───────────────────────────────────────────────────────
pub struct BrowserScrollTool;

#[async_trait]
impl Tool for BrowserScrollTool {
    fn name(&self) -> &str {
        "browser_scroll"
    }
    fn description(&self) -> &str {
        "Scroll the page by `amount` pixels (default 600). direction: \"down\" (default) or \"up\"."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "tab_id": {"type": "string"},
                "direction": {"type": "string", "enum": ["up", "down"], "default": "down"},
                "amount": {"type": "integer", "default": 600}
            },
            "required": ["tab_id"]
        })
    }
    fn is_read_only(&self) -> bool {
        false
    }
    fn is_destructive(&self) -> bool {
        false
    }
    fn is_concurrency_safe(&self) -> bool {
        false
    }

    async fn execute(&self, input: Value) -> crate::ToolResult<crate::ToolOutput> {
        let tab = TabId(
            input["tab_id"]
                .as_str()
                .ok_or_else(|| ToolError::InvalidInput("missing field \"tab_id\"".into()))?
                .to_string(),
        );
        let down = !matches!(
            input.get("direction").and_then(|v| v.as_str()),
            Some("up") | Some("Up")
        );
        let amount = input
            .get("amount")
            .and_then(|v| v.as_i64())
            .unwrap_or(600)
            .clamp(1, 20_000) as f64;
        let delta = if down { amount } else { -amount };
        let session = ChromeSession::global().await?;
        let p = session.get_page(&tab).await?;
        chrome_session::scroll_at(&p, delta).await?;
        let mut m = HashMap::new();
        m.insert("tab_id".into(), json!(tab.0));
        Ok(crate::ToolOutput {
            content: format!("scrolled {} {}px", if down { "down" } else { "up" }, amount),
            is_error: false,
            metadata: m,
        })
    }
}

// ── browser_evaluate ─────────────────────────────────────────────────────
pub struct BrowserEvaluateTool;

#[async_trait]
impl Tool for BrowserEvaluateTool {
    fn name(&self) -> &str {
        "browser_evaluate"
    }
    fn description(&self) -> &str {
        "Run a JavaScript expression in the page and return its value (strings verbatim, other values as JSON). Full DOM access — use for data extraction or UI state the dedicated tools can't reach."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "tab_id": {"type": "string"},
                "expression": {"type": "string", "description": "JavaScript expression, e.g. \"document.querySelectorAll('h2').length\""}
            },
            "required": ["tab_id", "expression"]
        })
    }
    fn is_read_only(&self) -> bool {
        false
    }
    fn is_destructive(&self) -> bool {
        true
    }
    fn is_concurrency_safe(&self) -> bool {
        false
    }

    async fn execute(&self, input: Value) -> crate::ToolResult<crate::ToolOutput> {
        let tab = TabId(
            input["tab_id"]
                .as_str()
                .ok_or_else(|| ToolError::InvalidInput("missing field \"tab_id\"".into()))?
                .to_string(),
        );
        let expression = input["expression"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidInput("missing field \"expression\"".into()))?
            .to_string();
        let session = ChromeSession::global().await?;
        let p = session.get_page(&tab).await?;
        let value = chrome_session::evaluate_js(&p, &expression).await?;
        let mut m = HashMap::new();
        m.insert("tab_id".into(), json!(tab.0));
        Ok(crate::ToolOutput {
            content: value,
            is_error: false,
            metadata: m,
        })
    }
}

// ── browser_screenshot ──────────────────────────────────────────────────
pub struct BrowserScreenshotTool;

#[async_trait]
impl Tool for BrowserScreenshotTool {
    fn name(&self) -> &str {
        "browser_screenshot"
    }
    fn description(&self) -> &str {
        "Capture a PNG screenshot of the given tab (viewport or full page). PNG bytes are returned in tool-result metadata under `image/png`."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "tab_id": {"type": "string"},
                "full_page": {"type": "boolean", "default": false}
            },
            "required": ["tab_id"]
        })
    }
    fn is_read_only(&self) -> bool {
        true
    }
    fn is_concurrency_safe(&self) -> bool {
        true
    }

    async fn execute(&self, input: Value) -> crate::ToolResult<crate::ToolOutput> {
        let tab = TabId(
            input["tab_id"]
                .as_str()
                .ok_or_else(|| ToolError::InvalidInput("missing field \"tab_id\"".into()))?
                .to_string(),
        );
        let full_page = input
            .get("full_page")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let session = ChromeSession::global().await?;
        let p = session.get_page(&tab).await?;
        let png = chrome_session::screenshot_png(&p, full_page).await?;
        // The model consumes screenshots through the engine's image pipeline
        // (metadata["type"] == "image" + base64 `data`) — the same contract
        // as the `computer` and `preview_screenshot` tools. Without `data`
        // the pixels are dropped and the model only sees a byte count.
        let b64 = base64::engine::general_purpose::STANDARD.encode(&png);
        let mut m = HashMap::new();
        m.insert("type".into(), json!("image"));
        m.insert("media_type".into(), json!("image/png"));
        m.insert("data".into(), json!(b64));
        m.insert("bytes".into(), json!(png.len()));
        m.insert("tab_id".into(), json!(tab.0));
        Ok(crate::ToolOutput {
            content: format!("screenshot captured ({} bytes)", png.len()),
            is_error: false,
            metadata: m,
        })
    }
}

// ── browser_tabs ────────────────────────────────────────────────────────
pub struct BrowserTabsTool;

#[async_trait]
impl Tool for BrowserTabsTool {
    fn name(&self) -> &str {
        "browser_tabs"
    }
    fn description(&self) -> &str {
        "List the open browser tabs in this Shannon process."
    }
    fn input_schema(&self) -> Value {
        json!({"type": "object", "properties": {}})
    }
    fn is_read_only(&self) -> bool {
        true
    }
    fn is_concurrency_safe(&self) -> bool {
        true
    }

    async fn execute(&self, _input: Value) -> crate::ToolResult<crate::ToolOutput> {
        let session = ChromeSession::global().await?;
        let tabs = session.list_tabs().await;
        if tabs.is_empty() {
            return Ok(crate::ToolOutput {
                content: "(no open tabs)".to_string(),
                is_error: false,
                metadata: HashMap::new(),
            });
        }
        let lines: Vec<String> = tabs
            .iter()
            .map(|(id, desc)| format!("- {}\topen\t{}", id.0, desc))
            .collect();
        Ok(crate::ToolOutput {
            content: lines.join("\n"),
            is_error: false,
            metadata: HashMap::new(),
        })
    }
}

// ── browser_close ───────────────────────────────────────────────────────
pub struct BrowserCloseTool;

#[async_trait]
impl Tool for BrowserCloseTool {
    fn name(&self) -> &str {
        "browser_close"
    }
    fn description(&self) -> &str {
        "Close a browser tab. Use `\"all\": true` to close every tab (the Chrome process keeps running)."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "tab_id": {"type": "string"},
                "all": {"type": "boolean", "default": false}
            }
        })
    }
    fn is_read_only(&self) -> bool {
        false
    }
    fn is_destructive(&self) -> bool {
        true
    }
    fn is_concurrency_safe(&self) -> bool {
        false
    }

    async fn execute(&self, input: Value) -> crate::ToolResult<crate::ToolOutput> {
        let close_all = input.get("all").and_then(|v| v.as_bool()).unwrap_or(false);
        let session = ChromeSession::global().await?;
        if close_all {
            let tabs = session.list_tabs().await;
            let ids: Vec<TabId> = tabs.into_iter().map(|(id, _)| id).collect();
            for id in &ids {
                let _ = session.close_tab(id).await;
            }
            return Ok(crate::ToolOutput {
                content: format!("closed {} tab(s)", ids.len()),
                is_error: false,
                metadata: HashMap::new(),
            });
        }
        let tab = TabId(
            input["tab_id"]
                .as_str()
                .ok_or_else(|| {
                    ToolError::InvalidInput("missing field \"tab_id\" with `all=false`".into())
                })?
                .to_string(),
        );
        session.close_tab(&tab).await?;
        Ok(crate::ToolOutput {
            content: format!("closed {}", tab.0),
            is_error: false,
            metadata: HashMap::new(),
        })
    }
}

// ── browser_console ─────────────────────────────────────────────────────
pub struct BrowserConsoleTool;

#[async_trait]
impl Tool for BrowserConsoleTool {
    fn name(&self) -> &str {
        "browser_console"
    }
    fn description(&self) -> &str {
        "Return the buffered console messages (log/debug/info/error/warning) for the given tab, oldest first. Messages are captured from the moment the tab was opened; the buffer holds the most recent 500 entries."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {"tab_id": {"type": "string"}},
            "required": ["tab_id"]
        })
    }
    fn is_read_only(&self) -> bool {
        true
    }
    fn is_concurrency_safe(&self) -> bool {
        true
    }

    async fn execute(&self, input: Value) -> crate::ToolResult<crate::ToolOutput> {
        let tab = TabId(
            input["tab_id"]
                .as_str()
                .ok_or_else(|| ToolError::InvalidInput("missing field \"tab_id\"".into()))?
                .to_string(),
        );
        let session = ChromeSession::global().await?;
        // Touch the page first so an unknown tab surfaces as an error
        // instead of an empty list.
        let _ = session.get_page(&tab).await?;
        let msgs = session.console_messages(&tab).await;
        if msgs.is_empty() {
            return Ok(crate::ToolOutput {
                content: "(no console messages captured for this tab)".to_string(),
                is_error: false,
                metadata: HashMap::new(),
            });
        }
        Ok(crate::ToolOutput {
            content: msgs.join("\n"),
            is_error: false,
            metadata: HashMap::new(),
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_input_schemas_require_correct_fields() {
        assert_eq!(
            BrowserNavigateTool.input_schema()["required"][0],
            json!("url")
        );
        // Click accepts a snapshot `ref` OR viewport coordinates; only the
        // tab is structurally required.
        assert_eq!(
            BrowserClickTool.input_schema()["required"],
            json!(["tab_id"])
        );
        assert_eq!(
            BrowserTypeTool.input_schema()["required"],
            json!(["tab_id", "text"])
        );
        assert_eq!(
            BrowserSnapshotTool.input_schema()["required"],
            json!(["tab_id"])
        );
        assert_eq!(
            BrowserScreenshotTool.input_schema()["required"],
            json!(["tab_id"])
        );
        assert_eq!(
            BrowserFillTool.input_schema()["required"],
            json!(["tab_id", "ref", "text"])
        );
        assert_eq!(
            BrowserPressKeyTool.input_schema()["required"],
            json!(["tab_id", "key"])
        );
        assert_eq!(
            BrowserEvaluateTool.input_schema()["required"],
            json!(["tab_id", "expression"])
        );
        assert_eq!(
            BrowserCloseTool.input_schema()["properties"]["all"]["type"],
            json!("boolean")
        );
    }

    #[test]
    fn test_names_are_stable() {
        for (tool, expected) in [
            (&BrowserNavigateTool as &dyn Tool, "browser_navigate"),
            (&BrowserClickTool, "browser_click"),
            (&BrowserTypeTool, "browser_type"),
            (&BrowserSnapshotTool, "browser_snapshot"),
            (&BrowserTextTool, "browser_text"),
            (&BrowserFillTool, "browser_fill"),
            (&BrowserPressKeyTool, "browser_press_key"),
            (&BrowserScrollTool, "browser_scroll"),
            (&BrowserEvaluateTool, "browser_evaluate"),
            (&BrowserScreenshotTool, "browser_screenshot"),
            (&BrowserTabsTool, "browser_tabs"),
            (&BrowserCloseTool, "browser_close"),
            (&BrowserConsoleTool, "browser_console"),
        ] {
            assert_eq!(tool.name(), expected);
        }
    }

    #[test]
    fn test_destructive_tools_are_serialized() {
        // Repo invariant (tool_trait_compliance): destructive ⇒ not
        // concurrency-safe. Read-only browser tools may run parallel;
        // state-changing ones serialize.
        let destructive: Vec<&dyn Tool> = vec![
            &BrowserClickTool,
            &BrowserTypeTool,
            &BrowserCloseTool,
            &BrowserFillTool,
            &BrowserPressKeyTool,
            &BrowserEvaluateTool,
        ];
        for tool in &destructive {
            assert!(
                tool.is_destructive(),
                "{} should be destructive",
                tool.name()
            );
            assert!(
                !tool.is_concurrency_safe(),
                "{} must not be concurrency-safe",
                tool.name()
            );
        }
        // Navigate flips page state but is flagged neither destructive
        // nor read-only (a navigation discards form state yet is routinely
        // reversible); it still serializes because concurrent navigations
        // race the same tab. Scroll mutates scroll position only — also
        // serialized but not destructive.
        assert!(!BrowserNavigateTool.is_destructive());
        assert!(!BrowserNavigateTool.is_read_only());
        assert!(!BrowserScrollTool.is_destructive());
        assert!(!BrowserScrollTool.is_read_only());
        let readonly: Vec<&dyn Tool> = vec![
            &BrowserSnapshotTool,
            &BrowserTextTool,
            &BrowserScreenshotTool,
            &BrowserTabsTool,
            &BrowserConsoleTool,
        ];
        for tool in &readonly {
            assert!(tool.is_read_only(), "{} should be read-only", tool.name());
            assert!(tool.is_concurrency_safe());
        }
    }
}
