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

// ── Concurrency-safety contract ──────────────────────────────────────────
// All eight tools share one process-global ChromeSession. Mutating tools
// (navigate / click / type / close) flip page and tab state, so parallel
// invocations race the same session (a click landing on a tab another call
// just navigated away) — they are NOT concurrency-safe and serialize.
// Read-only tools (snapshot / screenshot / tabs / console) only observe
// state and stay concurrency-safe, per the repo invariant that read-only
// tools must always be concurrency-safe (tool_trait_compliance).

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
        // Mutating: opens/reuses tabs and flips page state on the shared
        // ChromeSession — concurrent navigations race each other.
        false
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

// ── browser_select_option ────────────────────────────────────────────────
pub struct BrowserSelectOptionTool;

#[async_trait]
impl Tool for BrowserSelectOptionTool {
    fn name(&self) -> &str {
        "browser_select_option"
    }
    fn description(&self) -> &str {
        "Choose an option in a <select> dropdown by ref (from browser_snapshot). Matches the option by value first, then by visible label (case-insensitive); fires input+change events. Dropdowns cannot be operated with browser_click/browser_fill."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "tab_id": {"type": "string"},
                "ref": {"type": "string", "description": "Element ref of the <select> from browser_snapshot, e.g. \"e4\""},
                "value": {"type": "string", "description": "Option value or visible label to select"}
            },
            "required": ["tab_id", "ref", "value"]
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
        let value = input["value"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidInput("missing field \"value\"".into()))?
            .to_string();
        let session = ChromeSession::global().await?;
        let p = session.get_page(&tab).await?;
        chrome_session::select_option(&p, &reference, &value).await?;
        let mut m = HashMap::new();
        m.insert("tab_id".into(), json!(tab.0));
        m.insert("ref".into(), json!(reference));
        Ok(crate::ToolOutput {
            content: format!("selected {value:?} in {reference}"),
            is_error: false,
            metadata: m,
        })
    }
}

// ── browser_hover ────────────────────────────────────────────────────────
pub struct BrowserHoverTool;

#[async_trait]
impl Tool for BrowserHoverTool {
    fn name(&self) -> &str {
        "browser_hover"
    }
    fn description(&self) -> &str {
        "Move the mouse over an element by ref (from browser_snapshot) — a real mouse move that triggers CSS :hover states, mouseover menus, and tooltips. Run browser_snapshot afterwards if the hover reveals new controls."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "tab_id": {"type": "string"},
                "ref": {"type": "string", "description": "Element ref from browser_snapshot, e.g. \"e2\""}
            },
            "required": ["tab_id", "ref"]
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
        let reference = input["ref"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidInput("missing field \"ref\"".into()))?
            .to_string();
        let session = ChromeSession::global().await?;
        let p = session.get_page(&tab).await?;
        chrome_session::hover_element(&p, &reference).await?;
        let mut m = HashMap::new();
        m.insert("tab_id".into(), json!(tab.0));
        m.insert("ref".into(), json!(reference));
        Ok(crate::ToolOutput {
            content: format!("hovered {reference}"),
            is_error: false,
            metadata: m,
        })
    }
}

// ── browser_wait_for ─────────────────────────────────────────────────────
pub struct BrowserWaitForTool;

#[async_trait]
impl Tool for BrowserWaitForTool {
    fn name(&self) -> &str {
        "browser_wait_for"
    }
    fn description(&self) -> &str {
        "Wait until the given text appears anywhere on the page (polls every 250ms) or the timeout elapses. Use after actions that trigger async loading (form submits, search, navigation) instead of repeatedly taking screenshots."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "tab_id": {"type": "string"},
                "text": {"type": "string", "description": "Text to wait for (exact substring of the page text)"},
                "timeout_ms": {"type": "integer", "default": 10000, "maximum": 60000, "description": "Give up after this many milliseconds"}
            },
            "required": ["tab_id", "text"]
        })
    }
    fn is_read_only(&self) -> bool {
        true
    }
    fn is_concurrency_safe(&self) -> bool {
        // Purely observational (polls innerText); the repo invariant
        // read-only ⇒ concurrency-safe applies even though a poll can span
        // tens of seconds.
        true
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
        let timeout_ms = input
            .get("timeout_ms")
            .and_then(|v| v.as_i64())
            .unwrap_or(10_000)
            .clamp(100, 60_000) as u64;
        let session = ChromeSession::global().await?;
        let p = session.get_page(&tab).await?;
        match chrome_session::wait_for_text(&p, &text, std::time::Duration::from_millis(timeout_ms))
            .await
        {
            Ok(()) => Ok(crate::ToolOutput {
                content: format!("text {text:?} appeared"),
                is_error: false,
                metadata: HashMap::new(),
            }),
            Err(e) => Ok(crate::ToolOutput {
                content: e,
                is_error: true,
                metadata: HashMap::new(),
            }),
        }
    }
}

// ── browser_upload ───────────────────────────────────────────────────────
pub struct BrowserUploadTool;

#[async_trait]
impl Tool for BrowserUploadTool {
    fn name(&self) -> &str {
        "browser_upload"
    }
    fn description(&self) -> &str {
        "Attach one or more local files to the page's file input (the input_index-th <input type=file>; hidden inputs count). Fires input+change so the site's upload handler runs. Max 20MB total."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "tab_id": {"type": "string"},
                "paths": {"type": "array", "items": {"type": "string"}, "description": "Absolute paths of files to upload (read from the machine running the browser)"},
                "input_index": {"type": "integer", "default": 0, "description": "0-based index among the page's file inputs (default: the first)"}
            },
            "required": ["tab_id", "paths"]
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
        let paths: Vec<String> = input
            .get("paths")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|p| p.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        if paths.is_empty() {
            return Err(ToolError::InvalidInput(
                "browser_upload requires a non-empty \"paths\" array".into(),
            ));
        }
        let input_index = input
            .get("input_index")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;
        let mut files = Vec::with_capacity(paths.len());
        for path in &paths {
            let bytes = std::fs::read(path)
                .map_err(|e| ToolError::ExecutionFailed(format!("read {}: {e}", path)))?;
            let name = std::path::Path::new(path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("upload.bin")
                .to_string();
            files.push((name, bytes));
        }
        let total: usize = files.iter().map(|(_, b)| b.len()).sum();
        let names: Vec<String> = files.iter().map(|(n, _)| n.clone()).collect();
        let session = ChromeSession::global().await?;
        let p = session.get_page(&tab).await?;
        chrome_session::upload_files(&p, input_index, &files).await?;
        let mut m = HashMap::new();
        m.insert("tab_id".into(), json!(tab.0));
        m.insert("uploaded".into(), json!(names));
        Ok(crate::ToolOutput {
            content: format!(
                "attached {} file(s) ({} bytes) to file input #{input_index}",
                files.len(),
                total
            ),
            is_error: false,
            metadata: m,
        })
    }
}

// ── browser_pdf ──────────────────────────────────────────────────────────
pub struct BrowserPdfTool;

#[async_trait]
impl Tool for BrowserPdfTool {
    fn name(&self) -> &str {
        "browser_pdf"
    }
    fn description(&self) -> &str {
        "Save the current page as a PDF to `path` (default: a timestamped file in the temp dir). The delivery format for receipts, confirmations, tickets, and reports."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "tab_id": {"type": "string"},
                "path": {"type": "string", "description": "Absolute path of the PDF file to write"}
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
        let path = match input.get("path").and_then(|v| v.as_str()) {
            Some(p) => std::path::PathBuf::from(p),
            None => {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                std::env::temp_dir().join(format!("shannon-page-{ts}.pdf"))
            }
        };
        let session = ChromeSession::global().await?;
        let p = session.get_page(&tab).await?;
        let bytes = chrome_session::pdf_bytes(&p).await?;
        std::fs::write(&path, &bytes)
            .map_err(|e| ToolError::ExecutionFailed(format!("write {}: {e}", path.display())))?;
        let mut m = HashMap::new();
        m.insert("tab_id".into(), json!(tab.0));
        m.insert("path".into(), json!(path.display().to_string()));
        m.insert("bytes".into(), json!(bytes.len()));
        Ok(crate::ToolOutput {
            content: format!("saved {} ({} bytes)", path.display(), bytes.len()),
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
        "Press a key or key combination on the given tab — goes to the focused element. Single keys: \"Enter\", \"Escape\", \"Tab\", \"ArrowDown\", \"a\", \"F5\". Combinations: \"Control+a\", \"Shift+Enter\", \"Meta+v\" (alt/option, ctrl/control, meta/cmd, shift)."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "tab_id": {"type": "string"},
                "key": {"type": "string", "description": "Key or combination, e.g. Enter/Escape/Tab/ArrowDown/F5, a single character, or \"Control+Shift+Tab\""}
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
        "Capture a screenshot of the given tab (viewport or full page). PNG is lossless; jpeg at quality 0-100 is ~10x smaller — prefer it when the image only needs to be reviewed (e.g. streaming progress to a phone). Bytes are returned in tool-result metadata under `image/png` or `image/jpeg`."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "tab_id": {"type": "string"},
                "full_page": {"type": "boolean", "default": false},
                "format": {"type": "string", "enum": ["png", "jpeg"], "default": "png", "description": "Image format; jpeg needs 'quality'"},
                "quality": {"type": "integer", "minimum": 0, "maximum": 100, "default": 60, "description": "JPEG quality (ignored for png)"}
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
        let format = input
            .get("format")
            .and_then(|v| v.as_str())
            .unwrap_or("png")
            .to_string();
        let quality = input.get("quality").and_then(|v| v.as_i64()).unwrap_or(60);
        let session = ChromeSession::global().await?;
        let p = session.get_page(&tab).await?;
        let bytes = chrome_session::screenshot_with_format(&p, full_page, &format, quality).await?;
        let media_type =
            if format.eq_ignore_ascii_case("jpeg") || format.eq_ignore_ascii_case("jpg") {
                "image/jpeg"
            } else {
                "image/png"
            };
        // The model consumes screenshots through the engine's image pipeline
        // (metadata["type"] == "image" + base64 `data`) — the same contract
        // as the `computer` and `preview_screenshot` tools. Without `data`
        // the pixels are dropped and the model only sees a byte count.
        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
        let mut m = HashMap::new();
        m.insert("type".into(), json!("image"));
        m.insert("media_type".into(), json!(media_type));
        m.insert("data".into(), json!(b64));
        m.insert("bytes".into(), json!(bytes.len()));
        m.insert("tab_id".into(), json!(tab.0));
        Ok(crate::ToolOutput {
            content: format!("screenshot captured ({media_type}, {} bytes)", bytes.len()),
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
            (&BrowserSelectOptionTool, "browser_select_option"),
            (&BrowserHoverTool, "browser_hover"),
            (&BrowserPressKeyTool, "browser_press_key"),
            (&BrowserScrollTool, "browser_scroll"),
            (&BrowserWaitForTool, "browser_wait_for"),
            (&BrowserEvaluateTool, "browser_evaluate"),
            (&BrowserUploadTool, "browser_upload"),
            (&BrowserPdfTool, "browser_pdf"),
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
            &BrowserSelectOptionTool,
            &BrowserPressKeyTool,
            &BrowserEvaluateTool,
            &BrowserUploadTool,
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
        // race the same tab. Scroll/hover mutate transient state only —
        // also serialized but not destructive. PDF writes a file (not a
        // page mutation); wait_for is read-only ⇒ concurrency-safe.
        assert!(!BrowserNavigateTool.is_destructive());
        assert!(!BrowserNavigateTool.is_read_only());
        assert!(
            !BrowserNavigateTool.is_concurrency_safe(),
            "navigate mutates the shared ChromeSession and must serialize"
        );
        assert!(!BrowserScrollTool.is_destructive());
        assert!(!BrowserScrollTool.is_read_only());
        assert!(!BrowserHoverTool.is_destructive());
        assert!(!BrowserPdfTool.is_destructive());
        assert!(BrowserWaitForTool.is_read_only());
        assert!(BrowserWaitForTool.is_concurrency_safe());
        for tool in [
            &BrowserHoverTool as &dyn Tool,
            &BrowserScrollTool,
            &BrowserPdfTool,
        ] {
            assert!(
                !tool.is_concurrency_safe(),
                "{} must serialize",
                tool.name()
            );
        }
        // Read-only tools observe the shared session without mutating it, so
        // they stay concurrency-safe (read-only ⇒ concurrency-safe is a
        // repo-wide invariant enforced by tool_trait_compliance).
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

    #[test]
    fn test_new_tool_schemas_shape() {
        assert_eq!(
            BrowserSelectOptionTool.input_schema()["required"],
            json!(["tab_id", "ref", "value"])
        );
        assert_eq!(
            BrowserHoverTool.input_schema()["required"],
            json!(["tab_id", "ref"])
        );
        assert_eq!(
            BrowserWaitForTool.input_schema()["required"],
            json!(["tab_id", "text"])
        );
        assert_eq!(
            BrowserUploadTool.input_schema()["required"],
            json!(["tab_id", "paths"])
        );
        // Screenshot gained format/quality knobs.
        let shot = BrowserScreenshotTool.input_schema();
        assert_eq!(shot["properties"]["format"]["enum"], json!(["png", "jpeg"]));
        assert!(shot["properties"]["quality"]["maximum"].is_number());
    }
}
