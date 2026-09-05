//! `preview_screenshot` tool (P1-5 C-1 — dev-server preview self-check loop).
//!
//! Lets the model capture the current in-app live preview as an image so it
//! can *verify its own frontend changes* ("改一个前端组件 → 预览面板可见变化 →
//! 模型调用 preview_screenshot → 模型确认修复"). The tool does not own any
//! preview state — it delegates to a [`PreviewAccess`] trait object the
//! caller (the desktop shell) supplies at registration time, mirroring the
//! [`crate::goal`] `GoalStateAccess` pattern. This keeps `shannon-tools`
//! dependency-free of Tauri/webview specifics.
//!
//! # Desktop-only surface
//!
//! The tool is NOT part of [`crate::register_default_tools`] — the CLI never
//! registers it. The desktop shell registers it into its global tool
//! registry (see `desktop/src/preview_commands.rs`), binding the access
//! object to the live `PreviewManager`.

use crate::{Tool, ToolError, ToolRegistry};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use shannon_tool_interface::{ToolOutput, ToolResult};

/// Pixel data for one preview capture, produced by the desktop side.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewScreenshot {
    /// Base64 (standard alphabet) encoded image bytes.
    pub image_base64: String,
    /// IANA media type, e.g. `image/png`.
    pub media_type: String,
    /// Image width in pixels.
    pub width: u32,
    /// Image height in pixels.
    pub height: u32,
    /// Present when the desktop could not grab the Shannon app window and
    /// captured the primary monitor instead (`"monitor"`): the image is the
    /// ENTIRE screen, not an isolated preview-panel crop.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback: Option<String>,
}

/// Mirror of the desktop preview lifecycle state the model may observe.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewStatusInfo {
    pub running: bool,
    pub url: Option<String>,
    pub started_at_ms: Option<i64>,
}

/// Capture/state accessor the desktop shell injects at tool-registration
/// time. Implementations must be thread-safe; `capture` may block briefly
/// (native window snapshot) — the same budget as the computer-use tool.
pub trait PreviewAccess: Send + Sync {
    /// Current preview lifecycle state.
    fn status(&self) -> PreviewStatusInfo;
    /// Capture the current preview content. Errors are user-presentable.
    fn capture(&self) -> Result<PreviewScreenshot, String>;
}

/// `preview_screenshot` — capture the running live preview for self-check.
pub struct PreviewScreenshotTool {
    pub access: std::sync::Arc<dyn PreviewAccess>,
}

#[async_trait]
impl Tool for PreviewScreenshotTool {
    fn name(&self) -> &str {
        "preview_screenshot"
    }

    fn description(&self) -> &str {
        "Capture a screenshot of the running live preview (the dev server \
         rendered in the desktop preview panel) and return it as an image. \
         Use this to visually verify your frontend changes after editing \
         components: make the edit, then call this tool and check the \
         rendered result. Fails when no preview is running — tell the user \
         to start it from the artifact panel's Live tab."
    }

    fn input_schema(&self) -> Value {
        json!({ "type": "object", "properties": {}, "additionalProperties": false })
    }

    async fn execute(&self, _input: Value) -> ToolResult<ToolOutput> {
        let status = self.access.status();
        if !status.running {
            return Ok(ToolOutput::error(String::from(
                "No preview is running. Ask the user to start the dev server \
                 from the artifact panel's Live tab, then retry.",
            )));
        }
        let url = status.url.unwrap_or_default();
        let shot = match self.access.capture() {
            Ok(s) => s,
            Err(e) => return Ok(ToolOutput::error(format!("Preview capture failed: {e}"))),
        };
        // Honest disclosure: a monitor-fallback image is the whole screen,
        // not the preview panel — the model must not treat it as isolated
        // preview content.
        let fallback_note = if shot.fallback.is_some() {
            " Note: this image is the ENTIRE screen (the app window could not \
             be isolated), so it contains more than the preview panel."
        } else {
            ""
        };
        let content = format!(
            "Captured preview screenshot of {url} ({}x{}, {}).{fallback_note} \
             Verify the rendered result against the change you just made.",
            shot.width, shot.height, shot.media_type
        );
        Ok(ToolOutput::success(content)
            .with_metadata("type".into(), json!("image"))
            .with_metadata("media_type".into(), json!(shot.media_type))
            .with_metadata("data".into(), json!(shot.image_base64))
            .with_metadata("width".into(), json!(shot.width))
            .with_metadata("height".into(), json!(shot.height)))
    }

    fn category(&self) -> &str {
        "preview"
    }
}

/// Register the tool. Desktop-only: the caller passes `None`-free access —
/// this function is intentionally not called by `register_default_tools`,
/// so the CLI never surfaces `preview_screenshot`.
pub fn register_preview_screenshot_tool(
    registry: &mut ToolRegistry,
    access: std::sync::Arc<dyn PreviewAccess>,
) -> Result<(), ToolError> {
    registry.register(Box::new(PreviewScreenshotTool { access }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use std::sync::Mutex;

    /// 1x1 transparent PNG fixture (67 bytes) for format assertions.
    const PNG_1X1_BASE64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";

    struct MockAccess {
        status: PreviewStatusInfo,
        captures: Mutex<u32>,
        result: Result<PreviewScreenshot, String>,
    }

    impl PreviewAccess for MockAccess {
        fn status(&self) -> PreviewStatusInfo {
            self.status.clone()
        }
        fn capture(&self) -> Result<PreviewScreenshot, String> {
            *self.captures.lock().unwrap() += 1;
            self.result.clone()
        }
    }

    fn running_access(result: Result<PreviewScreenshot, String>) -> MockAccess {
        MockAccess {
            status: PreviewStatusInfo {
                running: true,
                url: Some("http://127.0.0.1:5173".into()),
                started_at_ms: Some(1_000),
            },
            captures: Mutex::new(0),
            result,
        }
    }

    fn png_shot() -> PreviewScreenshot {
        PreviewScreenshot {
            image_base64: PNG_1X1_BASE64.into(),
            media_type: "image/png".into(),
            width: 1,
            height: 1,
            fallback: None,
        }
    }

    #[test]
    fn tool_metadata_carries_png_image_block() {
        let access = std::sync::Arc::new(running_access(Ok(png_shot())));
        let tool = PreviewScreenshotTool {
            access: access.clone(),
        };
        let out = futures::executor::block_on(tool.execute(json!({}))).unwrap();
        assert!(!out.is_error);
        assert_eq!(out.metadata.get("type"), Some(&json!("image")));
        assert_eq!(out.metadata.get("media_type"), Some(&json!("image/png")));
        assert_eq!(out.metadata.get("width"), Some(&json!(1)));
        assert_eq!(out.metadata.get("height"), Some(&json!(1)));
        // Format assertion: the payload must decode to real PNG magic bytes.
        let data = out.metadata.get("data").unwrap().as_str().unwrap();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data)
            .expect("base64 must decode");
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "payload is a PNG");
        // Dimensions reported in the human-readable content too.
        assert!(out.content.contains("1x1"));
        assert_eq!(*access.captures.lock().unwrap(), 1);
    }

    #[test]
    fn tool_errors_when_no_preview_running() {
        let access = std::sync::Arc::new(MockAccess {
            status: PreviewStatusInfo {
                running: false,
                url: None,
                started_at_ms: None,
            },
            captures: Mutex::new(0),
            result: Ok(png_shot()),
        });
        let tool = PreviewScreenshotTool { access };
        let out = futures::executor::block_on(tool.execute(json!({}))).unwrap();
        assert!(out.is_error);
        assert!(out.content.contains("No preview is running"));
    }

    #[test]
    fn tool_discloses_monitor_fallback_in_content() {
        let mut shot = png_shot();
        shot.fallback = Some("monitor".into());
        let access = std::sync::Arc::new(running_access(Ok(shot)));
        let tool = PreviewScreenshotTool { access };
        let out = futures::executor::block_on(tool.execute(json!({}))).unwrap();
        assert!(!out.is_error);
        assert!(out.content.contains("ENTIRE screen"), "{}", out.content);
    }

    #[test]
    fn tool_surfaces_capture_failure_as_tool_error_output() {
        let access = std::sync::Arc::new(running_access(Err("headless display".into())));
        let tool = PreviewScreenshotTool { access };
        let out = futures::executor::block_on(tool.execute(json!({}))).unwrap();
        assert!(out.is_error);
        assert!(out.content.contains("headless display"));
    }

    #[test]
    fn registration_adds_tool_to_registry() {
        let mut registry = ToolRegistry::new();
        register_preview_screenshot_tool(
            &mut registry,
            std::sync::Arc::new(running_access(Ok(png_shot()))),
        )
        .unwrap();
        assert!(registry.get("preview_screenshot").is_some());
    }

    #[test]
    fn screenshot_struct_roundtrips_serde() {
        let shot = png_shot();
        let v = serde_json::to_value(&shot).unwrap();
        assert_eq!(v["media_type"], json!("image/png"));
        let back: PreviewScreenshot = serde_json::from_value(v).unwrap();
        assert_eq!(back.width, 1);
    }
}
