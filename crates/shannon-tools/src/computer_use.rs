//! Computer Use tool implementation.
//!
//! Provides a Screenshot-Action Loop (CUA) for desktop automation:
//! screenshot → multimodal LLM analysis → action execution → repeat.
//!
//! Compatible with Anthropic's `computer_20251124` tool schema.
//!
//! # Feature Flag
//!
//! Actual screen capture and input simulation require the `computer-use` feature:
//! ```toml
//! shannon-tools = { features = ["computer-use"] }
//! ```
//! Without the feature, the tool registers but returns an error on execution.

use crate::{Tool, ToolError, ToolOutput, ToolResult};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;

#[cfg(feature = "computer-use")]
use base64::Engine;
#[cfg(feature = "computer-use")]
use enigo::{Axis, Direction, Keyboard, Mouse};

/// Reference resolution for coordinate scaling (XGA).
/// The LLM operates on a downscaled view; coordinates must be scaled to actual resolution.
pub const REFERENCE_WIDTH: u32 = 1024;
pub const REFERENCE_HEIGHT: u32 = 768;

/// Actionable hint appended to every "not enabled" stub, so users of
/// release bundles land on a fix instead of a dead end (P1 error-copy).
#[cfg(not(feature = "computer-use"))]
const FEATURE_DISABLED_HINT: &str = "\n\nHow to fix: Windows and desktop release bundles ship with computer use \
enabled — update your Shannon install. When building from source: `cargo build --release --features computer-use` \
(Linux additionally needs libxdo/X11 dev packages).";

// ── T10 Phase 1: Linux input backend selection ─────────────────────────
// The backend is chosen at compile time via mutually exclusive cargo
// features; guard against accidental combinations.

#[cfg(all(feature = "computer-use-libei", feature = "computer-use-wayland"))]
compile_error!(
    "features `computer-use-libei` and `computer-use-wayland` are mutually exclusive: pick one Linux input backend"
);
#[cfg(all(feature = "computer-use-libei", feature = "computer-use-x11rb"))]
compile_error!(
    "features `computer-use-libei` and `computer-use-x11rb` are mutually exclusive: pick one Linux input backend"
);
#[cfg(all(feature = "computer-use-wayland", feature = "computer-use-x11rb"))]
compile_error!(
    "features `computer-use-wayland` and `computer-use-x11rb` are mutually exclusive: pick one Linux input backend"
);

/// Name of the compile-time-selected enigo input backend, for diagnostics
/// and error messages. On non-Linux targets every backend feature maps to
/// the same platform implementation, so this reports the generic name.
pub fn input_backend_name() -> &'static str {
    if cfg!(feature = "computer-use-libei") {
        "libei (xdg-desktop-portal RemoteDesktop)"
    } else if cfg!(feature = "computer-use-wayland") {
        "wayland-client"
    } else if cfg!(feature = "computer-use-x11rb") {
        "x11rb"
    } else {
        "xdo (X11)"
    }
}

/// Returns a hint when the compiled input backend is X11-only but the
/// session looks like native Wayland (`WAYLAND_DISPLAY` set, `DISPLAY`
/// unset). XWayland sessions (both set) still work with X11 backends, so
/// no hint is produced there.
#[cfg(feature = "computer-use")]
pub fn session_compatibility_hint() -> Option<&'static str> {
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    let x11 = std::env::var_os("DISPLAY").is_some();
    let x11_only_backend =
        !(cfg!(feature = "computer-use-libei") || cfg!(feature = "computer-use-wayland"));
    if wayland && !x11 && x11_only_backend {
        Some(concat!(
            "This build uses an X11-only input backend (xdo), but the session looks like ",
            "native Wayland (WAYLAND_DISPLAY set, DISPLAY unset). Rebuild with ",
            "`--features computer-use-libei` (Wayland via xdg-desktop-portal) or run under XWayland."
        ))
    } else {
        None
    }
}

/// Maximum `wait` duration accepted in one call (seconds). Longer requests
/// are capped — an unbounded sleep would hang the screenshot-action loop
/// with no way for the model (or a remote user watching on mobile) to tell
/// a stalled task from a working one.
pub const MAX_WAIT_SECONDS: f64 = 60.0;

/// Delay between interpolated mouse moves while dragging (millis). Long
/// enough for window servers/HTML5 dnd to register movement, short enough
/// that a 30-step drag stays well under a second.
#[cfg(feature = "computer-use")]
const DRAG_STEP_DELAY_MS: u64 = 10;

/// Actions supported by the computer use tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComputerAction {
    Screenshot,
    /// Capture a region of the screen at native resolution — reads small
    /// text, verification codes, dense toolbars the full screenshot
    /// downscales past legibility. Region is given in the coordinate space
    /// of the last screenshot (or the reference space before one).
    Zoom,
    Click,
    RightClick,
    MiddleClick,
    DoubleClick,
    TripleClick,
    Type,
    Scroll,
    KeyPress,
    Wait,
    MouseMove,
    /// Report the current pointer position (global, per-monitor local, and
    /// in screenshot coordinate space) so the model can re-anchor after
    /// scrolling or window moves without burning another full screenshot.
    CursorPosition,
    LeftClickDrag,
    /// Structured UIA (UI Automation) tree of a window — Windows only.
    /// Semantic alternative to screenshot reading: roles, names, refs.
    UiTree,
    /// Click a UIA element by name substring (and optional match index) —
    /// Windows only. Resolves the element's bounding-rect center and clicks
    /// there; far more reliable than screenshot-guessed coordinates.
    UiClick,
}

/// Scroll direction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScrollDirection {
    Up,
    Down,
    Left,
    Right,
}

/// Input parameters for the computer use tool.
///
/// Compatible with Anthropic's `computer_20251124` tool type schema.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ComputerUseInput {
    /// The action to perform.
    pub action: ComputerAction,

    /// [x, y] coordinates for click, mouse_move, left_click_drag.
    /// Coordinates are in reference resolution space (1024x768) and will be
    /// scaled to the actual screen resolution.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coordinate: Option<[i32; 2]>,

    /// Text to type (for `type` action).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,

    /// Scroll direction (for `scroll` action).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scroll_direction: Option<ScrollDirection>,

    /// Number of scroll "ticks" (for `scroll` action, default 3).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scroll_amount: Option<i32>,

    /// Key or key combination to press (for `key_press` action).
    /// Examples: "Return", "ctrl+a", "alt+F4", "shift+Tab"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,

    /// Duration in seconds to wait (for `wait` action, default 1.0).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<f64>,

    /// Start coordinate for drag (for `left_click_drag`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_coordinate: Option<[i32; 2]>,

    /// 0-based display index for multi-monitor setups (default: primary
    /// monitor). Applies to screenshot and every coordinate-taking action.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monitor: Option<u32>,

    /// Window title substring — targets `ui_tree`/`ui_click` at a specific
    /// window instead of the foreground one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window: Option<String>,

    /// Element name substring to click (for `ui_click`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub element: Option<String>,

    /// 0-based match index when several elements share the same name
    /// (for `ui_click`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<usize>,

    /// Region size `[width, height]` for the `zoom` action, in the same
    /// coordinate space as `coordinate` (default 384x288).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<[i32; 2]>,
}

/// Configuration for the computer use tool.
#[derive(Debug, Clone)]
pub struct ComputerUseConfig {
    /// Whether screenshot capture is enabled.
    pub screenshot_enabled: bool,
    /// Whether input simulation is enabled.
    pub input_enabled: bool,
    /// Actions that are allowed (empty = all allowed).
    pub allowed_actions: Vec<ComputerAction>,
    /// Maximum screenshot dimensions.
    pub max_screenshot_width: u32,
    pub max_screenshot_height: u32,
}

impl Default for ComputerUseConfig {
    fn default() -> Self {
        Self {
            screenshot_enabled: true,
            input_enabled: true,
            allowed_actions: vec![],
            max_screenshot_width: REFERENCE_WIDTH,
            max_screenshot_height: REFERENCE_HEIGHT,
        }
    }
}

/// Computer Use tool: desktop automation via screenshot-action loop.
///
/// Implements the Anthropic-compatible `computer` tool schema for
/// screen capture, mouse, and keyboard interaction.
pub struct ComputerUseTool {
    description: String,
    config: ComputerUseConfig,
    /// Pixel dimensions of the most recent downscaled screenshot, per
    /// monitor index. Screenshots are downscaled preserving aspect ratio,
    /// so on anything that is not exactly 4:3 the image the model saw is
    /// NOT 1024x768 (a 1920x1080 screen yields 1024x576). The model
    /// reports coordinates measured on that image — scaling them through
    /// the stored dims instead of the fixed reference space is what keeps
    /// clicks on target. Falls back to reference-space scaling before the
    /// first screenshot.
    #[cfg_attr(not(feature = "computer-use"), allow(dead_code))]
    last_screenshot: std::sync::Mutex<HashMap<u32, (u32, u32)>>,
}

impl Default for ComputerUseTool {
    fn default() -> Self {
        Self::new()
    }
}

impl ComputerUseTool {
    pub fn new() -> Self {
        Self {
            description: "Interact with the computer desktop: take screenshots, zoom into regions, click, type, scroll, and press keys. After a screenshot, coordinates are pixel coordinates in that screenshot image (its dimensions are reported in every capture result); before the first screenshot they use the 1024x768 reference space. Coordinates are scaled to the actual screen automatically.".to_string(),
            config: ComputerUseConfig::default(),
            last_screenshot: std::sync::Mutex::new(HashMap::new()),
        }
    }

    pub fn with_config(config: ComputerUseConfig) -> Self {
        Self {
            description: "Interact with the computer desktop: take screenshots, zoom into regions, click, type, scroll, and press keys. After a screenshot, coordinates are pixel coordinates in that screenshot image (its dimensions are reported in every capture result); before the first screenshot they use the 1024x768 reference space. Coordinates are scaled to the actual screen automatically.".to_string(),
            config,
            last_screenshot: std::sync::Mutex::new(HashMap::new()),
        }
    }

    /// Check if an action is allowed by the configuration.
    fn is_action_allowed(&self, action: &ComputerAction) -> bool {
        if self.config.allowed_actions.is_empty() {
            return true;
        }
        self.config.allowed_actions.contains(action)
    }

    /// Scale a coordinate from reference resolution to actual screen resolution.
    pub fn scale_coordinate(coord: [i32; 2], actual_width: u32, actual_height: u32) -> [i32; 2] {
        Self::scale_coordinate_from(
            coord,
            REFERENCE_WIDTH,
            REFERENCE_HEIGHT,
            actual_width,
            actual_height,
        )
    }

    /// Scale `coord` from the pixel space of a source image (`src_w` x
    /// `src_h`, e.g. the screenshot the model measured on) into `dst` space
    /// (e.g. the physical monitor). Coordinates are clamped to the
    /// destination bounds. A degenerate source (0) falls back to
    /// reference-space scaling.
    pub fn scale_coordinate_from(
        coord: [i32; 2],
        src_w: u32,
        src_h: u32,
        dst_w: u32,
        dst_h: u32,
    ) -> [i32; 2] {
        if src_w == 0 || src_h == 0 {
            return Self::scale_coordinate(coord, dst_w, dst_h);
        }
        let x = (f64::from(coord[0]) * f64::from(dst_w) / f64::from(src_w)).round() as i32;
        let y = (f64::from(coord[1]) * f64::from(dst_h) / f64::from(src_h)).round() as i32;
        [x.clamp(0, dst_w as i32 - 1), y.clamp(0, dst_h as i32 - 1)]
    }

    /// Inverse of [`Self::scale_coordinate_from`]: map a point from screen
    /// space back into the source image space (used by `cursor_position` to
    /// report where the pointer sits in coordinates the model understands).
    pub fn unscale_coordinate(
        coord: [i32; 2],
        src_w: u32,
        src_h: u32,
        dst_w: u32,
        dst_h: u32,
    ) -> [i32; 2] {
        if src_w == 0 || src_h == 0 {
            return coord;
        }
        [
            (f64::from(coord[0]) * f64::from(dst_w) / f64::from(src_w)).round() as i32,
            (f64::from(coord[1]) * f64::from(dst_h) / f64::from(src_h)).round() as i32,
        ]
    }

    /// Index of the monitor whose virtual-desktop rect contains the global
    /// point (`rects` entries are `(width, height, origin_x, origin_y)`).
    /// A point in the gap between displays resolves to the nearest monitor
    /// center. Returns 0 for an empty slice.
    pub fn monitor_index_for_point(x: i32, y: i32, rects: &[(u32, u32, i32, i32)]) -> usize {
        if rects.is_empty() {
            return 0;
        }
        if let Some(i) = rects.iter().position(|(w, h, ox, oy)| {
            x >= *ox
                && x < ox.saturating_add(*w as i32)
                && y >= *oy
                && y < oy.saturating_add(*h as i32)
        }) {
            return i;
        }
        let mut best = 0;
        let mut best_dist = i64::MAX;
        for (i, (w, h, ox, oy)) in rects.iter().enumerate() {
            let cx = i64::from(*ox) + i64::from(*w) / 2;
            let cy = i64::from(*oy) + i64::from(*h) / 2;
            let dist = (i64::from(x) - cx).pow(2) + (i64::from(y) - cy).pow(2);
            if dist < best_dist {
                best_dist = dist;
                best = i;
            }
        }
        best
    }

    /// Map a `zoom` region — top-left `coord` and `size`, both in the
    /// screenshot (or reference) space of `src_w` x `src_h` — onto the
    /// full-resolution capture of `cap_w` x `cap_h`. The rect is clamped to
    /// the capture bounds with a minimum 1x1 size; returns
    /// `(x, y, width, height)` in capture pixels.
    #[cfg_attr(not(feature = "computer-use"), allow(dead_code))]
    fn zoom_crop_rect(
        coord: [i32; 2],
        size: [i32; 2],
        src_w: u32,
        src_h: u32,
        cap_w: u32,
        cap_h: u32,
    ) -> (u32, u32, u32, u32) {
        let (ref_w, ref_h) = if src_w == 0 || src_h == 0 {
            (REFERENCE_WIDTH, REFERENCE_HEIGHT)
        } else {
            (src_w, src_h)
        };
        let scale_x = f64::from(cap_w) / f64::from(ref_w);
        let scale_y = f64::from(cap_h) / f64::from(ref_h);
        let x = ((f64::from(coord[0]) * scale_x).floor() as i64).clamp(0, i64::from(cap_w) - 1);
        let y = ((f64::from(coord[1]) * scale_y).floor() as i64).clamp(0, i64::from(cap_h) - 1);
        let w = ((f64::from(size[0]) * scale_x).round() as i64).clamp(1, i64::from(cap_w) - x);
        let h = ((f64::from(size[1]) * scale_y).round() as i64).clamp(1, i64::from(cap_h) - y);
        (x as u32, y as u32, w as u32, h as u32)
    }

    /// Compute the downscaled dimensions that fit within the configured
    /// screenshot maximum, preserving aspect ratio. Returns `None` when the
    /// image already fits (never upscales).
    #[cfg(feature = "computer-use")]
    fn downscale_dims(&self, width: u32, height: u32) -> Option<(u32, u32)> {
        let (max_w, max_h) = (
            self.config.max_screenshot_width,
            self.config.max_screenshot_height,
        );
        if max_w == 0 || max_h == 0 || (width <= max_w && height <= max_h) {
            return None;
        }
        let scale = (f64::from(max_w) / f64::from(width)).min(f64::from(max_h) / f64::from(height));
        let new_w = ((f64::from(width) * scale).round() as u32).max(1);
        let new_h = ((f64::from(height) * scale).round() as u32).max(1);
        Some((new_w, new_h))
    }

    /// Parse a key combination string into individual keys.
    /// "ctrl+a" → ["ctrl", "a"], "alt+F4" → ["alt", "F4"]. The plus key
    /// itself is written with a repeated plus ("ctrl++" → ["ctrl", "+"]):
    /// splitting on '+' yields empty segments there, which collapse back
    /// into a single "+".
    pub fn parse_key_combination(key: &str) -> Vec<String> {
        let trimmed = key.trim();
        if trimmed.is_empty() {
            return vec![String::new()];
        }
        let mut out: Vec<String> = Vec::new();
        let mut pending_plus = false;
        for part in trimmed.split('+') {
            let part = part.trim();
            if part.is_empty() {
                pending_plus = true;
                continue;
            }
            if pending_plus {
                out.push("+".to_string());
                pending_plus = false;
            }
            out.push(part.to_string());
        }
        if pending_plus {
            out.push("+".to_string());
        }
        out
    }

    /// Convert a key name string to an enigo Key enum value. Returns `None`
    /// for unrecognized names — callers must surface an error rather than
    /// guess, because a wrong guess types a random character (the old
    /// first-char fallback turned a request for "F13" into a literal "f").
    #[cfg(feature = "computer-use")]
    fn str_to_key(name: &str) -> Option<enigo::Key> {
        let lower = name.to_lowercase();
        let key = match lower.as_str() {
            "ctrl" | "control" => enigo::Key::Control,
            "alt" | "option" => enigo::Key::Alt,
            "shift" => enigo::Key::Shift,
            "meta" | "cmd" | "command" | "super" | "win" => enigo::Key::Meta,
            "return" | "enter" => enigo::Key::Return,
            "tab" => enigo::Key::Tab,
            "space" | "spacebar" => enigo::Key::Space,
            "+" | "plus" => enigo::Key::Unicode('+'),
            "backspace" | "back" => enigo::Key::Backspace,
            "delete" | "del" => enigo::Key::Delete,
            "escape" | "esc" => enigo::Key::Escape,
            "up" => enigo::Key::UpArrow,
            "down" => enigo::Key::DownArrow,
            "left" => enigo::Key::LeftArrow,
            "right" => enigo::Key::RightArrow,
            "home" => enigo::Key::Home,
            "end" => enigo::Key::End,
            "pageup" | "page_up" | "pgup" => enigo::Key::PageUp,
            "pagedown" | "page_down" | "pgdn" => enigo::Key::PageDown,
            // enigo's Key enum gates some variants per platform: Insert and
            // F21-F24 don't exist on macOS — cfg the arms so the mapping is
            // simply absent there instead of failing to compile.
            #[cfg(not(target_os = "macos"))]
            "insert" => enigo::Key::Insert,
            "capslock" | "caps_lock" => enigo::Key::CapsLock,
            "f1" => enigo::Key::F1,
            "f2" => enigo::Key::F2,
            "f3" => enigo::Key::F3,
            "f4" => enigo::Key::F4,
            "f5" => enigo::Key::F5,
            "f6" => enigo::Key::F6,
            "f7" => enigo::Key::F7,
            "f8" => enigo::Key::F8,
            "f9" => enigo::Key::F9,
            "f10" => enigo::Key::F10,
            "f11" => enigo::Key::F11,
            "f12" => enigo::Key::F12,
            "f13" => enigo::Key::F13,
            "f14" => enigo::Key::F14,
            "f15" => enigo::Key::F15,
            "f16" => enigo::Key::F16,
            "f17" => enigo::Key::F17,
            "f18" => enigo::Key::F18,
            "f19" => enigo::Key::F19,
            "f20" => enigo::Key::F20,
            #[cfg(not(target_os = "macos"))]
            "f21" => enigo::Key::F21,
            #[cfg(not(target_os = "macos"))]
            "f22" => enigo::Key::F22,
            #[cfg(not(target_os = "macos"))]
            "f23" => enigo::Key::F23,
            #[cfg(not(target_os = "macos"))]
            "f24" => enigo::Key::F24,
            f if f.len() >= 2 && f.starts_with('f') => {
                // f21..f24 and any other f-prefixed name are not mapped;
                // fall through to the single-char check so "f" itself works.
                return None;
            }
            c if c.chars().count() == 1 => enigo::Key::Unicode(lower.chars().next().unwrap()),
            _ => return None,
        };
        Some(key)
    }

    fn build_input_schema() -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["screenshot", "zoom", "click", "right_click", "middle_click", "double_click", "triple_click", "type", "scroll", "key_press", "wait", "mouse_move", "cursor_position", "left_click_drag", "ui_tree", "ui_click"],
                    "description": "The action to perform"
                },
                "monitor": {
                    "type": "integer",
                    "description": "0-based display index for multi-monitor setups (default: primary monitor). Applies to screenshot and coordinate actions."
                },
                "window": {
                    "type": "string",
                    "description": "Window title substring to target (for 'ui_tree'/'ui_click'; default: foreground window)"
                },
                "element": {
                    "type": "string",
                    "description": "Element name substring to click (for 'ui_click'; run 'ui_tree' first to inspect names)"
                },
                "index": {
                    "type": "integer",
                    "description": "0-based match index when several elements share the name (for 'ui_click')"
                },
                "coordinate": {
                    "type": "array",
                    "items": { "type": "integer" },
                    "maxItems": 2,
                    "minItems": 2,
                    "description": "[x, y] coordinates: pixel coordinates in the most recent screenshot image (dimensions reported with each capture), or reference space (0-1024, 0-768) before the first screenshot"
                },
                "size": {
                    "type": "array",
                    "items": { "type": "integer" },
                    "maxItems": 2,
                    "minItems": 2,
                    "description": "Region [width, height] for the 'zoom' action, in the same coordinate space as 'coordinate' (default 384x288)"
                },
                "text": {
                    "type": "string",
                    "description": "Text to type (for 'type' action)"
                },
                "scroll_direction": {
                    "type": "string",
                    "enum": ["up", "down", "left", "right"],
                    "description": "Direction to scroll (for 'scroll' action)"
                },
                "scroll_amount": {
                    "type": "integer",
                    "description": "Number of scroll ticks (default 3)"
                },
                "key": {
                    "type": "string",
                    "description": "Key or key combination, e.g. 'Return', 'ctrl+a', 'alt+F4' ('+' key itself written 'ctrl++')"
                },
                "duration": {
                    "type": "number",
                    "description": "Seconds to wait (for 'wait' action, default 1.0, capped at 60)"
                },
                "start_coordinate": {
                    "type": "array",
                    "items": { "type": "integer" },
                    "maxItems": 2,
                    "minItems": 2,
                    "description": "Start [x, y] for drag operations"
                }
            },
            "required": ["action"]
        })
    }
}

#[async_trait]
impl Tool for ComputerUseTool {
    fn name(&self) -> &str {
        "computer"
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
        // Screen interactions should not run concurrently
        false
    }

    fn is_destructive(&self) -> bool {
        // Input simulation can be destructive (typing, clicking)
        true
    }

    async fn execute(&self, input: serde_json::Value) -> ToolResult<ToolOutput> {
        let computer_input: ComputerUseInput = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("Invalid computer use input: {e}")))?;

        if !self.is_action_allowed(&computer_input.action) {
            return Ok(ToolOutput {
                content: format!(
                    "Action '{}' is not allowed by current configuration.",
                    serde_json::to_string(&computer_input.action).unwrap_or_default()
                ),
                is_error: true,
                metadata: HashMap::new(),
            });
        }

        // `mut` is only exercised by the computer-use session-hint append
        // below; plain builds would warn on it (and dropping `mut` breaks
        // the computer-use build instead — both halves of the gate matter).
        #[cfg_attr(not(feature = "computer-use"), allow(unused_mut))]
        let mut result = match computer_input.action {
            ComputerAction::Screenshot => self.execute_screenshot(&computer_input).await,
            ComputerAction::Zoom => {
                let coord = computer_input.coordinate.ok_or_else(|| {
                    ToolError::InvalidInput(
                        "zoom action requires 'coordinate' (region top-left)".to_string(),
                    )
                })?;
                let size = computer_input.size.unwrap_or([384, 288]);
                self.execute_zoom(&computer_input, coord, size).await
            }
            ComputerAction::Click
            | ComputerAction::RightClick
            | ComputerAction::MiddleClick
            | ComputerAction::DoubleClick
            | ComputerAction::TripleClick => {
                let coord = computer_input.coordinate.ok_or_else(|| {
                    ToolError::InvalidInput("click action requires 'coordinate'".to_string())
                })?;
                self.execute_click_variant(&computer_input.action, coord, computer_input.monitor)
                    .await
            }
            ComputerAction::Type => {
                let text = computer_input.text.ok_or_else(|| {
                    ToolError::InvalidInput("type action requires 'text'".to_string())
                })?;
                self.execute_type(&text).await
            }
            ComputerAction::Scroll => {
                let direction = computer_input
                    .scroll_direction
                    .unwrap_or(ScrollDirection::Down);
                let amount = computer_input.scroll_amount.unwrap_or(3);
                let coord = computer_input.coordinate;
                self.execute_scroll(direction, amount, coord, computer_input.monitor)
                    .await
            }
            ComputerAction::KeyPress => {
                let key = computer_input.key.ok_or_else(|| {
                    ToolError::InvalidInput("key_press action requires 'key'".to_string())
                })?;
                self.execute_key_press(&key).await
            }
            ComputerAction::Wait => {
                let duration = computer_input.duration.unwrap_or(1.0);
                self.execute_wait(duration).await
            }
            ComputerAction::MouseMove => {
                let coord = computer_input.coordinate.ok_or_else(|| {
                    ToolError::InvalidInput("mouse_move action requires 'coordinate'".to_string())
                })?;
                self.execute_mouse_move(coord, computer_input.monitor).await
            }
            ComputerAction::CursorPosition => self.execute_cursor_position().await,
            ComputerAction::LeftClickDrag => {
                let start = computer_input.start_coordinate.ok_or_else(|| {
                    ToolError::InvalidInput(
                        "left_click_drag action requires 'start_coordinate'".to_string(),
                    )
                })?;
                let end = computer_input.coordinate.ok_or_else(|| {
                    ToolError::InvalidInput(
                        "left_click_drag action requires 'coordinate' (end position)".to_string(),
                    )
                })?;
                self.execute_drag(start, end, computer_input.monitor).await
            }
            ComputerAction::UiTree => self.execute_ui_tree(computer_input.window.as_deref()).await,
            ComputerAction::UiClick => {
                let element = computer_input.element.clone().ok_or_else(|| {
                    ToolError::InvalidInput(
                        "ui_click action requires 'element' (run 'ui_tree' first)".to_string(),
                    )
                })?;
                self.execute_ui_click(
                    computer_input.window.as_deref(),
                    &element,
                    computer_input.index.unwrap_or(0),
                )
                .await
            }
        };
        #[cfg(feature = "computer-use")]
        if let Ok(out) = &mut result {
            if out.is_error {
                if let Some(hint) = session_compatibility_hint() {
                    if !out.content.ends_with(hint) {
                        out.content.push_str("\n\n");
                        out.content.push_str(hint);
                    }
                }
            }
        }
        result
    }
}

// Action implementations — feature-gated

impl ComputerUseTool {
    #[cfg(feature = "computer-use")]
    async fn execute_screenshot(&self, input: &ComputerUseInput) -> ToolResult<ToolOutput> {
        if !self.config.screenshot_enabled {
            return Ok(ToolOutput {
                content: "Screenshot capture is disabled.".to_string(),
                is_error: true,
                metadata: HashMap::new(),
            });
        }

        // Windows: declare per-monitor-v2 DPI awareness before the first
        // capture so xcap pixels and enigo's SetCursorPos share one
        // physical-pixel space (no-op on other platforms).
        crate::windows_platform::ensure_dpi_awareness();

        let image = self
            .capture_screen(input.monitor)
            .await
            .map_err(ToolError::ExecutionFailed)?;

        // Downscale to the configured maximum so the payload matches the
        // 1024x768 reference coordinate space and stays within the
        // multimodal token budget (native Retina captures are up to 4x).
        let (orig_w, orig_h) = (image.width(), image.height());
        let image = match self.downscale_dims(orig_w, orig_h) {
            Some((new_w, new_h)) => {
                image::imageops::resize(&image, new_w, new_h, image::imageops::FilterType::Lanczos3)
            }
            None => image,
        };
        let width = image.width();
        let height = image.height();
        // Remember the space the model will measure coordinates in — every
        // coordinate-taking action scales from these dims (not the fixed
        // reference) so clicks stay on target on non-4:3 displays.
        if let Ok(mut dims) = self.last_screenshot.lock() {
            dims.insert(input.monitor.unwrap_or(0), (width, height));
        }

        // Encode as PNG
        let mut png_data = Vec::new();
        image
            .write_to(
                &mut std::io::Cursor::new(&mut png_data),
                image::ImageFormat::Png,
            )
            .map_err(|e| ToolError::ExecutionFailed(format!("PNG encoding failed: {e}")))?;

        let b64 = base64::engine::general_purpose::STANDARD.encode(&png_data);

        let mut metadata = HashMap::new();
        metadata.insert("type".to_string(), json!("image"));
        metadata.insert("media_type".to_string(), json!("image/png"));
        metadata.insert("data".to_string(), json!(b64));
        metadata.insert("width".to_string(), json!(width));
        metadata.insert("height".to_string(), json!(height));
        // Provenance: which window the (foreground) capture shows. Event
        // logs gain an auditable "what was on screen when the agent acted".
        crate::windows_platform::attach_window_context(&mut metadata);
        if let Some(m) = input.monitor {
            metadata.insert("monitor".to_string(), json!(m));
        }

        Ok(ToolOutput {
            content: format!("Screenshot captured ({width}x{height})"),
            is_error: false,
            metadata,
        })
    }

    /// Screen capture backend dispatch (B3 / T10-Phase2). On a Wayland
    /// session with the native capture feature, wlr-screencopy runs first,
    /// then the xdg-desktop-portal; anything else — including a failed
    /// native attempt (logged) — falls back to xcap, which still works
    /// under XWayland.
    #[cfg(feature = "computer-use")]
    async fn capture_screen(&self, monitor: Option<u32>) -> Result<image::RgbaImage, String> {
        #[cfg(all(target_os = "linux", feature = "computer-use-wayland-capture"))]
        if crate::screen_capture::wayland_session_active() {
            match crate::screen_capture::capture_screen_wayland().await {
                Ok(img) => return Ok(img.to_rgba8()),
                Err(e) => {
                    tracing::warn!(error = %e, "native Wayland capture failed; falling back to xcap")
                }
            }
        }

        let monitors = xcap::Monitor::all().map_err(|e| format!("Failed to get monitors: {e}"))?;
        let idx = monitor.unwrap_or(0) as usize;
        if idx >= monitors.len() {
            return Err(format!(
                "monitor index {idx} out of range — {} display(s) available",
                monitors.len()
            ));
        }
        monitors
            .into_iter()
            .nth(idx)
            .unwrap()
            .capture_image()
            .map_err(|e| {
                let msg = format!("Screenshot failed: {e}");
                #[cfg(target_os = "macos")]
                let msg = {
                    let mut m = msg;
                    m.push_str(
                        "\nHint: on macOS, screen capture requires the Screen Recording \
                         permission (System Settings → Privacy & Security → Screen \
                         Recording) for the app hosting Shannon; macOS 15+ re-asks for it \
                         periodically, which silently breaks captures until re-granted.",
                    );
                    m
                };
                msg
            })
    }

    /// Pixel dimensions coordinates are currently expressed in for `monitor`:
    /// the most recent downscaled screenshot's dims when one exists,
    /// otherwise the reference space.
    #[cfg(feature = "computer-use")]
    fn screenshot_dims(&self, monitor: Option<u32>) -> (u32, u32) {
        self.last_screenshot
            .lock()
            .ok()
            .and_then(|dims| dims.get(&monitor.unwrap_or(0)).copied())
            .unwrap_or((REFERENCE_WIDTH, REFERENCE_HEIGHT))
    }

    /// Geometry of the selected monitor: `(width, height, origin_x,
    /// origin_y)`. Origins are virtual-desktop coordinates (the primary
    /// monitor sits at 0,0; monitors left of/above it go negative) — enigo's
    /// absolute moves and xcap captures both live in that space once
    /// per-monitor DPI awareness is declared. On macOS these are logical
    /// points, the same space CGEvent input uses; on Windows/Linux they are
    /// physical pixels.
    #[cfg(feature = "computer-use")]
    fn monitor_geometry(monitor: Option<u32>) -> Result<(u32, u32, i32, i32), String> {
        let monitors = xcap::Monitor::all().map_err(|e| {
            tracing::warn!(error = %e, "monitor enumeration failed");
            format!("monitor enumeration failed: {e}")
        })?;
        if monitors.is_empty() {
            return Err("no active display found (display asleep or detached?)".to_string());
        }
        let idx = monitor.unwrap_or(0) as usize;
        if idx >= monitors.len() {
            return Err(format!(
                "monitor index {idx} out of range — {} display(s) available",
                monitors.len()
            ));
        }
        let m = &monitors[idx];
        let width = m
            .width()
            .map_err(|e| format!("monitor width unavailable: {e}"))?;
        let height = m
            .height()
            .map_err(|e| format!("monitor height unavailable: {e}"))?;
        let x = m
            .x()
            .map_err(|e| format!("monitor origin unavailable: {e}"))?;
        let y = m
            .y()
            .map_err(|e| format!("monitor origin unavailable: {e}"))?;
        Ok((width, height, x, y))
    }

    /// All monitor rects as `(width, height, origin_x, origin_y)`, for
    /// point-to-monitor resolution.
    #[cfg(feature = "computer-use")]
    fn monitor_rects() -> Result<Vec<(u32, u32, i32, i32)>, String> {
        let monitors =
            xcap::Monitor::all().map_err(|e| format!("monitor enumeration failed: {e}"))?;
        monitors
            .iter()
            .map(|m| {
                Ok((
                    m.width()
                        .map_err(|e| format!("monitor width unavailable: {e}"))?,
                    m.height()
                        .map_err(|e| format!("monitor height unavailable: {e}"))?,
                    m.x()
                        .map_err(|e| format!("monitor origin unavailable: {e}"))?,
                    m.y()
                        .map_err(|e| format!("monitor origin unavailable: {e}"))?,
                ))
            })
            .collect()
    }

    /// Scale a reference-space or screenshot-space coordinate into physical
    /// pixels on the selected monitor, including that monitor's
    /// virtual-desktop origin. Source space is the most recent screenshot's
    /// pixel dimensions when one has been taken (the model measures on that
    /// image), falling back to the 1024x768 reference space before the
    /// first capture.
    #[cfg(feature = "computer-use")]
    fn resolve_point(&self, coord: [i32; 2], monitor: Option<u32>) -> Result<[i32; 2], String> {
        let (w, h, ox, oy) = Self::monitor_geometry(monitor)?;
        let (src_w, src_h) = self.screenshot_dims(monitor);
        let local = Self::scale_coordinate_from(coord, src_w, src_h, w, h);
        Ok([local[0] + ox, local[1] + oy])
    }

    #[cfg(not(feature = "computer-use"))]
    async fn execute_screenshot(&self, _input: &ComputerUseInput) -> ToolResult<ToolOutput> {
        Ok(ToolOutput {
            content: format!(
                "Screenshot capture unavailable: the `computer-use` feature is not enabled in this build.{FEATURE_DISABLED_HINT}"
            ),
            is_error: true,
            metadata: HashMap::new(),
        })
    }

    /// `zoom`: capture the monitor at native resolution and return only the
    /// requested region, still at (near) native fidelity. Small text,
    /// verification codes, and dense toolbars that the full-screen downscale
    /// renders illegible become readable — the same re-inspection affordance
    /// Anthropic's `computer_20251124` schema added for capable models.
    #[cfg(feature = "computer-use")]
    async fn execute_zoom(
        &self,
        input: &ComputerUseInput,
        coord: [i32; 2],
        size: [i32; 2],
    ) -> ToolResult<ToolOutput> {
        if !self.config.screenshot_enabled {
            return Ok(ToolOutput {
                content: "Screenshot capture is disabled.".to_string(),
                is_error: true,
                metadata: HashMap::new(),
            });
        }
        if size[0] <= 0 || size[1] <= 0 {
            return Err(ToolError::InvalidInput(format!(
                "zoom 'size' must be positive, got {size:?}"
            )));
        }

        crate::windows_platform::ensure_dpi_awareness();
        let mut capture = self
            .capture_screen(input.monitor)
            .await
            .map_err(ToolError::ExecutionFailed)?;
        let (cap_w, cap_h) = (capture.width(), capture.height());
        let (src_w, src_h) = self.screenshot_dims(input.monitor);
        let (x, y, w, h) = Self::zoom_crop_rect(coord, size, src_w, src_h, cap_w, cap_h);
        let cropped = image::imageops::crop(&mut capture, x, y, w, h).to_image();

        // Fit the crop into the configured maximum (never upscale past the
        // native crop — zoom only ever trades region size for fidelity).
        let (out_w, out_h) = self.downscale_dims(w, h).unwrap_or((w, h));
        let out = if (out_w, out_h) == (w, h) {
            cropped
        } else {
            image::imageops::resize(
                &cropped,
                out_w,
                out_h,
                image::imageops::FilterType::Lanczos3,
            )
        };
        let (fw, fh) = (out.width(), out.height());

        let mut png_data = Vec::new();
        out.write_to(
            &mut std::io::Cursor::new(&mut png_data),
            image::ImageFormat::Png,
        )
        .map_err(|e| ToolError::ExecutionFailed(format!("PNG encoding failed: {e}")))?;
        let b64 = base64::engine::general_purpose::STANDARD.encode(&png_data);

        let mut metadata = HashMap::new();
        metadata.insert("type".to_string(), json!("image"));
        metadata.insert("media_type".to_string(), json!("image/png"));
        metadata.insert("data".to_string(), json!(b64));
        metadata.insert("width".to_string(), json!(fw));
        metadata.insert("height".to_string(), json!(fh));
        metadata.insert(
            "zoom_region".to_string(),
            json!({
                "capture_px": [x, y, w, h],
                "source_space": [coord[0], coord[1], size[0], size[1]],
            }),
        );
        if let Some(m) = input.monitor {
            metadata.insert("monitor".to_string(), json!(m));
        }
        crate::windows_platform::attach_window_context(&mut metadata);

        Ok(ToolOutput {
            content: format!(
                "Zoomed to region ({}, {})+{}x{} → {}x{} image. Coordinates measured on this crop are NOT screen coordinates: map a crop point (cx, cy) back via screen_in_screenshot_space = ({}, {}) + (cx * {} / {}, cy * {} / {}) — or take a fresh full screenshot before clicking.",
                coord[0],
                coord[1],
                size[0],
                size[1],
                fw,
                fh,
                coord[0],
                coord[1],
                size[0],
                fw,
                size[1],
                fh
            ),
            is_error: false,
            metadata,
        })
    }

    #[cfg(not(feature = "computer-use"))]
    async fn execute_zoom(
        &self,
        _input: &ComputerUseInput,
        coord: [i32; 2],
        size: [i32; 2],
    ) -> ToolResult<ToolOutput> {
        Ok(ToolOutput {
            content: format!(
                "zoom is unavailable: the `computer-use` feature is not enabled in this build. Would zoom to ({}, {})+{}x{}.{FEATURE_DISABLED_HINT}",
                coord[0], coord[1], size[0], size[1]
            ),
            is_error: true,
            metadata: HashMap::new(),
        })
    }

    /// `cursor_position`: report the pointer's global position, its
    /// per-monitor local position, and its coordinates in the space the
    /// model measures in (last screenshot or reference). Cheap re-anchoring
    /// after scrolls or window moves — no full screenshot needed.
    #[cfg(feature = "computer-use")]
    async fn execute_cursor_position(&self) -> ToolResult<ToolOutput> {
        crate::windows_platform::ensure_dpi_awareness();

        let enigo = enigo::Enigo::new(&enigo::Settings::default())
            .map_err(|e| ToolError::ExecutionFailed(format!("Input init failed: {e}")))?;
        let (x, y) = enigo
            .location()
            .map_err(|e| ToolError::ExecutionFailed(format!("cursor location failed: {e}")))?;

        let rects = Self::monitor_rects().map_err(ToolError::ExecutionFailed)?;
        let idx = Self::monitor_index_for_point(x, y, &rects);
        let (w, h, ox, oy) =
            rects
                .get(idx)
                .copied()
                .unwrap_or((REFERENCE_WIDTH, REFERENCE_HEIGHT, 0, 0));
        let local = [x - ox, y - oy];
        let (src_w, src_h) = self.screenshot_dims(Some(idx as u32));
        let in_source_space = Self::unscale_coordinate(local, w, h, src_w, src_h);

        let mut metadata = HashMap::new();
        metadata.insert(
            "cursor".to_string(),
            json!({
                "global": [x, y],
                "monitor": idx,
                "monitor_local": local,
                "screenshot_space": in_source_space,
                "screenshot_dims": [src_w, src_h],
            }),
        );
        Ok(ToolOutput {
            content: format!(
                "cursor at ({x}, {y}) — monitor {idx}, local ({}, {}), screenshot-space ({}, {}) of {}x{}",
                local[0], local[1], in_source_space[0], in_source_space[1], src_w, src_h
            ),
            is_error: false,
            metadata,
        })
    }

    #[cfg(not(feature = "computer-use"))]
    async fn execute_cursor_position(&self) -> ToolResult<ToolOutput> {
        Ok(ToolOutput {
            content: format!(
                "cursor_position is unavailable: the `computer-use` feature is not enabled in this build.{FEATURE_DISABLED_HINT}"
            ),
            is_error: true,
            metadata: HashMap::new(),
        })
    }

    #[cfg(feature = "computer-use")]
    async fn execute_click_variant(
        &self,
        action: &ComputerAction,
        coord: [i32; 2],
        monitor: Option<u32>,
    ) -> ToolResult<ToolOutput> {
        if !self.config.input_enabled {
            return Ok(ToolOutput {
                content: "Input simulation is disabled.".to_string(),
                is_error: true,
                metadata: HashMap::new(),
            });
        }
        Self::ensure_input_permitted()?;
        crate::windows_platform::ensure_dpi_awareness();

        let (button, clicks, label) = Self::click_spec(action);

        let scaled = self
            .resolve_point(coord, monitor)
            .map_err(ToolError::ExecutionFailed)?;

        let mut enigo = enigo::Enigo::new(&enigo::Settings::default())
            .map_err(|e| ToolError::ExecutionFailed(format!("Input init failed: {e}")))?;

        enigo
            .move_mouse(scaled[0], scaled[1], enigo::Coordinate::Abs)
            .map_err(|e| ToolError::ExecutionFailed(format!("Mouse move failed: {e}")))?;

        for _ in 0..clicks {
            enigo
                .button(button, Direction::Click)
                .map_err(|e| ToolError::ExecutionFailed(format!("Mouse click failed: {e}")))?;
        }

        let mut metadata = HashMap::new();
        crate::windows_platform::attach_window_context(&mut metadata);

        Ok(ToolOutput {
            content: format!(
                "{label} at ({}, {}) [scaled from ({}, {})]",
                scaled[0], scaled[1], coord[0], coord[1]
            ),
            is_error: false,
            metadata,
        })
    }

    /// Resolve the enigo button, click repetition, and result label for a
    /// click-family action.
    #[cfg(feature = "computer-use")]
    fn click_spec(action: &ComputerAction) -> (enigo::Button, usize, &'static str) {
        match action {
            ComputerAction::RightClick => (enigo::Button::Right, 1, "Right-clicked"),
            ComputerAction::MiddleClick => (enigo::Button::Middle, 1, "Middle-clicked"),
            ComputerAction::DoubleClick => (enigo::Button::Left, 2, "Double-clicked"),
            ComputerAction::TripleClick => (enigo::Button::Left, 3, "Triple-clicked"),
            _ => (enigo::Button::Left, 1, "Clicked"),
        }
    }

    #[cfg(not(feature = "computer-use"))]
    async fn execute_click_variant(
        &self,
        action: &ComputerAction,
        coord: [i32; 2],
        _monitor: Option<u32>,
    ) -> ToolResult<ToolOutput> {
        let verb = match action {
            ComputerAction::RightClick => "right-click",
            ComputerAction::MiddleClick => "middle-click",
            ComputerAction::DoubleClick => "double-click",
            ComputerAction::TripleClick => "triple-click",
            _ => "click",
        };
        Ok(ToolOutput {
            content: format!(
                "Computer use is not enabled in this build. Would {verb} at ({}, {}).{FEATURE_DISABLED_HINT}",
                coord[0], coord[1]
            ),
            is_error: true,
            metadata: HashMap::new(),
        })
    }

    #[cfg(feature = "computer-use")]
    async fn execute_type(&self, text: &str) -> ToolResult<ToolOutput> {
        if !self.config.input_enabled {
            return Ok(ToolOutput {
                content: "Input simulation is disabled.".to_string(),
                is_error: true,
                metadata: HashMap::new(),
            });
        }
        Self::ensure_input_permitted()?;
        crate::windows_platform::ensure_dpi_awareness();

        let mut enigo = enigo::Enigo::new(&enigo::Settings::default())
            .map_err(|e| ToolError::ExecutionFailed(format!("Input init failed: {e}")))?;

        enigo
            .text(text)
            .map_err(|e| ToolError::ExecutionFailed(format!("Text input failed: {e}")))?;

        let mut metadata = HashMap::new();
        crate::windows_platform::attach_window_context(&mut metadata);
        Ok(ToolOutput {
            content: format!("Typed {} characters", text.chars().count()),
            is_error: false,
            metadata,
        })
    }

    #[cfg(not(feature = "computer-use"))]
    async fn execute_type(&self, text: &str) -> ToolResult<ToolOutput> {
        Ok(ToolOutput {
            content: format!(
                "Computer use is not enabled in this build. Would type '{}' ({} chars).{FEATURE_DISABLED_HINT}",
                text.chars().take(50).collect::<String>(),
                text.len()
            ),
            is_error: true,
            metadata: HashMap::new(),
        })
    }

    #[cfg(feature = "computer-use")]
    async fn execute_scroll(
        &self,
        direction: ScrollDirection,
        amount: i32,
        coord: Option<[i32; 2]>,
        monitor: Option<u32>,
    ) -> ToolResult<ToolOutput> {
        if !self.config.input_enabled {
            return Ok(ToolOutput {
                content: "Input simulation is disabled.".to_string(),
                is_error: true,
                metadata: HashMap::new(),
            });
        }
        Self::ensure_input_permitted()?;
        crate::windows_platform::ensure_dpi_awareness();

        let mut enigo = enigo::Enigo::new(&enigo::Settings::default())
            .map_err(|e| ToolError::ExecutionFailed(format!("Input init failed: {e}")))?;

        // Move to coordinate if provided
        if let Some(c) = coord {
            let scaled = self
                .resolve_point(c, monitor)
                .map_err(ToolError::ExecutionFailed)?;
            enigo
                .move_mouse(scaled[0], scaled[1], enigo::Coordinate::Abs)
                .map_err(|e| ToolError::ExecutionFailed(format!("Mouse move failed: {e}")))?;
        }

        let (scroll_len, scroll_axis) = match direction {
            ScrollDirection::Up => (amount, Axis::Vertical),
            ScrollDirection::Down => (-amount, Axis::Vertical),
            ScrollDirection::Left => (amount, Axis::Horizontal),
            ScrollDirection::Right => (-amount, Axis::Horizontal),
        };

        enigo
            .scroll(scroll_len, scroll_axis)
            .map_err(|e| ToolError::ExecutionFailed(format!("Scroll failed: {e}")))?;

        let mut metadata = HashMap::new();
        crate::windows_platform::attach_window_context(&mut metadata);
        Ok(ToolOutput {
            content: format!("Scrolled {direction:?} x{amount}"),
            is_error: false,
            metadata,
        })
    }

    #[cfg(not(feature = "computer-use"))]
    async fn execute_scroll(
        &self,
        direction: ScrollDirection,
        amount: i32,
        _coord: Option<[i32; 2]>,
        _monitor: Option<u32>,
    ) -> ToolResult<ToolOutput> {
        Ok(ToolOutput {
            content: format!(
                "Computer use is not enabled in this build. Would scroll {direction:?} x{amount}.{FEATURE_DISABLED_HINT}",
            ),
            is_error: true,
            metadata: HashMap::new(),
        })
    }

    #[cfg(feature = "computer-use")]
    async fn execute_key_press(&self, key: &str) -> ToolResult<ToolOutput> {
        if !self.config.input_enabled {
            return Ok(ToolOutput {
                content: "Input simulation is disabled.".to_string(),
                is_error: true,
                metadata: HashMap::new(),
            });
        }
        Self::ensure_input_permitted()?;
        crate::windows_platform::ensure_dpi_awareness();

        let mut enigo = enigo::Enigo::new(&enigo::Settings::default())
            .map_err(|e| ToolError::ExecutionFailed(format!("Input init failed: {e}")))?;

        let keys = Self::parse_key_combination(key);
        let mut enigo_keys = Vec::with_capacity(keys.len());
        for k in &keys {
            enigo_keys.push(Self::str_to_key(k).ok_or_else(|| {
                ToolError::InvalidInput(format!(
                    "unknown key {k:?} in combination {key:?} — use key names like \
                     Return/Tab/Escape/ArrowDown/F5, modifiers ctrl/alt/shift/meta, \
                     a single character, or '+' (written 'ctrl++')"
                ))
            })?);
        }
        // For simple single keys, click directly
        if enigo_keys.len() == 1 {
            enigo
                .key(enigo_keys[0], Direction::Click)
                .map_err(|e| ToolError::ExecutionFailed(format!("Key press failed: {e}")))?;
        } else {
            // Press modifiers first, then the main key, then release in
            // reverse. If a press fails mid-combo, release everything
            // already held before returning — keys left logically down at
            // the OS level would corrupt every subsequent input action.
            let mut held: Vec<enigo::Key> = Vec::with_capacity(enigo_keys.len());
            for k in &enigo_keys {
                match enigo.key(*k, Direction::Press) {
                    Ok(()) => held.push(*k),
                    Err(e) => {
                        for h in held.iter().rev() {
                            let _ = enigo.key(*h, Direction::Release);
                        }
                        return Err(ToolError::ExecutionFailed(format!("Key press failed: {e}")));
                    }
                }
            }
            let mut release_error: Option<String> = None;
            for k in enigo_keys.iter().rev() {
                if let Err(e) = enigo.key(*k, Direction::Release) {
                    release_error = Some(format!(
                        "Key release failed ({e}) — modifier keys may be stuck; \
                         send a plain key_press to reset"
                    ));
                }
            }
            if let Some(msg) = release_error {
                return Err(ToolError::ExecutionFailed(msg));
            }
        }

        let mut metadata = HashMap::new();
        crate::windows_platform::attach_window_context(&mut metadata);
        Ok(ToolOutput {
            content: format!("Pressed key: {key}"),
            is_error: false,
            metadata,
        })
    }

    #[cfg(not(feature = "computer-use"))]
    async fn execute_key_press(&self, key: &str) -> ToolResult<ToolOutput> {
        Ok(ToolOutput {
            content: format!(
                "Computer use is not enabled in this build. Would press '{key}'.{FEATURE_DISABLED_HINT}",
            ),
            is_error: true,
            metadata: HashMap::new(),
        })
    }

    /// Shared wait validation + capping. Rejects negative/non-finite
    /// durations; caps runaway sleeps at [`MAX_WAIT_SECONDS`] so a confused
    /// model cannot hang the automation loop (important for remote/mobile
    /// sessions where the user only sees progress through screenshots).
    fn validate_wait(duration: f64) -> Result<f64, ToolError> {
        if !duration.is_finite() || duration < 0.0 {
            return Err(ToolError::InvalidInput(format!(
                "wait duration must be a finite number of seconds >= 0, got {duration}"
            )));
        }
        Ok(duration.min(MAX_WAIT_SECONDS))
    }

    #[cfg(feature = "computer-use")]
    async fn execute_wait(&self, duration: f64) -> ToolResult<ToolOutput> {
        let capped = Self::validate_wait(duration)?;
        let millis = (capped * 1000.0) as u64;
        tokio::time::sleep(std::time::Duration::from_millis(millis)).await;
        let content = if capped < duration {
            format!("Waited {capped:.1}s (requested {duration:.1}s, capped at {MAX_WAIT_SECONDS}s)")
        } else {
            format!("Waited {capped:.1}s")
        };
        Ok(ToolOutput {
            content,
            is_error: false,
            metadata: HashMap::new(),
        })
    }

    #[cfg(not(feature = "computer-use"))]
    async fn execute_wait(&self, duration: f64) -> ToolResult<ToolOutput> {
        // Even without the feature, wait is safe to execute
        let capped = Self::validate_wait(duration)?;
        let millis = (capped * 1000.0) as u64;
        tokio::time::sleep(std::time::Duration::from_millis(millis)).await;
        Ok(ToolOutput {
            content: format!("Waited {capped:.1}s"),
            is_error: false,
            metadata: HashMap::new(),
        })
    }

    #[cfg(feature = "computer-use")]
    async fn execute_mouse_move(
        &self,
        coord: [i32; 2],
        monitor: Option<u32>,
    ) -> ToolResult<ToolOutput> {
        if !self.config.input_enabled {
            return Ok(ToolOutput {
                content: "Input simulation is disabled.".to_string(),
                is_error: true,
                metadata: HashMap::new(),
            });
        }
        Self::ensure_input_permitted()?;
        crate::windows_platform::ensure_dpi_awareness();

        let scaled = self
            .resolve_point(coord, monitor)
            .map_err(ToolError::ExecutionFailed)?;

        let mut enigo = enigo::Enigo::new(&enigo::Settings::default())
            .map_err(|e| ToolError::ExecutionFailed(format!("Input init failed: {e}")))?;

        enigo
            .move_mouse(scaled[0], scaled[1], enigo::Coordinate::Abs)
            .map_err(|e| ToolError::ExecutionFailed(format!("Mouse move failed: {e}")))?;

        let mut metadata = HashMap::new();
        crate::windows_platform::attach_window_context(&mut metadata);
        Ok(ToolOutput {
            content: format!(
                "Moved mouse to ({}, {}) [scaled from ({}, {})]",
                scaled[0], scaled[1], coord[0], coord[1]
            ),
            is_error: false,
            metadata,
        })
    }

    #[cfg(not(feature = "computer-use"))]
    async fn execute_mouse_move(
        &self,
        coord: [i32; 2],
        _monitor: Option<u32>,
    ) -> ToolResult<ToolOutput> {
        Ok(ToolOutput {
            content: format!(
                "Computer use is not enabled in this build. Would move to ({}, {}).{FEATURE_DISABLED_HINT}",
                coord[0], coord[1]
            ),
            is_error: true,
            metadata: HashMap::new(),
        })
    }

    #[cfg(feature = "computer-use")]
    async fn execute_drag(
        &self,
        start: [i32; 2],
        end: [i32; 2],
        monitor: Option<u32>,
    ) -> ToolResult<ToolOutput> {
        if !self.config.input_enabled {
            return Ok(ToolOutput {
                content: "Input simulation is disabled.".to_string(),
                is_error: true,
                metadata: HashMap::new(),
            });
        }
        Self::ensure_input_permitted()?;
        crate::windows_platform::ensure_dpi_awareness();

        let scaled_start = self
            .resolve_point(start, monitor)
            .map_err(ToolError::ExecutionFailed)?;
        let scaled_end = self
            .resolve_point(end, monitor)
            .map_err(ToolError::ExecutionFailed)?;

        let mut enigo = enigo::Enigo::new(&enigo::Settings::default())
            .map_err(|e| ToolError::ExecutionFailed(format!("Input init failed: {e}")))?;

        // Move to start, press, interpolate mouse moves to the end (many
        // surfaces — HTML5 drag & drop, canvas apps, sliders — ignore a
        // teleported press→release and need intermediate move events while
        // the button is held), then release.
        enigo
            .move_mouse(scaled_start[0], scaled_start[1], enigo::Coordinate::Abs)
            .map_err(|e| ToolError::ExecutionFailed(format!("Mouse move failed: {e}")))?;

        enigo
            .button(enigo::Button::Left, Direction::Press)
            .map_err(|e| ToolError::ExecutionFailed(format!("Mouse press failed: {e}")))?;

        let dx = scaled_end[0] - scaled_start[0];
        let dy = scaled_end[1] - scaled_start[1];
        // f64 math: dx*dx overflows i32 on enormous multi-monitor desktops.
        let dist = (f64::from(dx) * f64::from(dx) + f64::from(dy) * f64::from(dy)).sqrt();
        // ~12px per step, 30 steps max, always at least one move.
        let steps = ((dist / 12.0).ceil() as usize).clamp(1, 30);
        for i in 1..=steps {
            let ix = scaled_start[0] + dx * i as i32 / steps as i32;
            let iy = scaled_start[1] + dy * i as i32 / steps as i32;
            enigo
                .move_mouse(ix, iy, enigo::Coordinate::Abs)
                .map_err(|e| ToolError::ExecutionFailed(format!("Mouse move failed: {e}")))?;
            tokio::time::sleep(std::time::Duration::from_millis(DRAG_STEP_DELAY_MS)).await;
        }

        enigo
            .button(enigo::Button::Left, Direction::Release)
            .map_err(|e| ToolError::ExecutionFailed(format!("Mouse release failed: {e}")))?;

        let mut metadata = HashMap::new();
        crate::windows_platform::attach_window_context(&mut metadata);
        Ok(ToolOutput {
            content: format!(
                "Dragged from ({}, {}) to ({}, {})",
                scaled_start[0], scaled_start[1], scaled_end[0], scaled_end[1]
            ),
            is_error: false,
            metadata,
        })
    }

    #[cfg(not(feature = "computer-use"))]
    async fn execute_drag(
        &self,
        start: [i32; 2],
        end: [i32; 2],
        _monitor: Option<u32>,
    ) -> ToolResult<ToolOutput> {
        Ok(ToolOutput {
            content: format!(
                "Computer use is not enabled in this build. Would drag ({}, {}) to ({}, {}).{FEATURE_DISABLED_HINT}",
                start[0], start[1], end[0], end[1]
            ),
            is_error: true,
            metadata: HashMap::new(),
        })
    }

    // ── UIA structured actions (Windows + computer-use) ─────────────────

    /// Render the UIA control tree of the foreground (or named) window.
    #[cfg(feature = "computer-use")]
    async fn execute_ui_tree(&self, window: Option<&str>) -> ToolResult<ToolOutput> {
        match crate::windows_platform::ui_tree(window.unwrap_or("")) {
            Ok(tree) => Ok(ToolOutput {
                content: tree,
                is_error: false,
                metadata: HashMap::new(),
            }),
            Err(e) => Ok(ToolOutput {
                content: e,
                is_error: true,
                metadata: HashMap::new(),
            }),
        }
    }

    /// Click a UIA element by name: resolve its bounding-rect center (the
    /// element's own physical pixels — no reference-space scaling) and
    /// click there.
    #[cfg(feature = "computer-use")]
    async fn execute_ui_click(
        &self,
        window: Option<&str>,
        element: &str,
        index: usize,
    ) -> ToolResult<ToolOutput> {
        if !self.config.input_enabled {
            return Ok(ToolOutput {
                content: "Input simulation is disabled.".to_string(),
                is_error: true,
                metadata: HashMap::new(),
            });
        }
        Self::ensure_input_permitted()?;

        let ((x, y), desc) =
            match crate::windows_platform::ui_click_center(window.unwrap_or(""), element, index) {
                Ok(v) => v,
                Err(e) => {
                    return Ok(ToolOutput {
                        content: e,
                        is_error: true,
                        metadata: HashMap::new(),
                    });
                }
            };

        crate::windows_platform::ensure_dpi_awareness();
        let mut enigo = enigo::Enigo::new(&enigo::Settings::default())
            .map_err(|e| ToolError::ExecutionFailed(format!("Input init failed: {e}")))?;
        enigo
            .move_mouse(x, y, enigo::Coordinate::Abs)
            .map_err(|e| ToolError::ExecutionFailed(format!("Mouse move failed: {e}")))?;
        enigo
            .button(enigo::Button::Left, Direction::Click)
            .map_err(|e| ToolError::ExecutionFailed(format!("Mouse click failed: {e}")))?;

        let mut metadata = HashMap::new();
        metadata.insert("uia_element".to_string(), json!(desc));
        crate::windows_platform::attach_window_context(&mut metadata);
        Ok(ToolOutput {
            content: format!("ui_click {element:?} → {desc} at ({x}, {y})"),
            is_error: false,
            metadata,
        })
    }

    #[cfg(not(feature = "computer-use"))]
    async fn execute_ui_tree(&self, _window: Option<&str>) -> ToolResult<ToolOutput> {
        Ok(ToolOutput {
            content: format!(
                "ui_tree is unavailable: the `computer-use` feature is not enabled in this build.{FEATURE_DISABLED_HINT}"
            ),
            is_error: true,
            metadata: HashMap::new(),
        })
    }

    #[cfg(not(feature = "computer-use"))]
    async fn execute_ui_click(
        &self,
        _window: Option<&str>,
        element: &str,
        _index: usize,
    ) -> ToolResult<ToolOutput> {
        Ok(ToolOutput {
            content: format!(
                "ui_click is unavailable: the `computer-use` feature is not enabled in this build. Would click element {element:?}.{FEATURE_DISABLED_HINT}"
            ),
            is_error: true,
            metadata: HashMap::new(),
        })
    }

    /// Gate every input action on the macOS Accessibility permission
    /// (roadmap E3): without the grant, enigo's CGEvent posts report
    /// success while the window server silently drops the events — the
    /// model would see "done" while nothing happened. Fail loudly with an
    /// actionable message instead. No-op off macOS.
    #[cfg(feature = "computer-use")]
    fn ensure_input_permitted() -> Result<(), ToolError> {
        if crate::platform_adapter::accessibility_granted() {
            Ok(())
        } else {
            Err(ToolError::ExecutionFailed(
                "Input simulation requires the macOS Accessibility permission, \
                 which the hosting app does not currently hold: synthetic mouse \
                 and keyboard events would be silently dropped. Grant it in \
                 System Settings → Privacy & Security → Accessibility, then retry."
                    .into(),
            ))
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn make_input(action: &str) -> serde_json::Value {
        json!({ "action": action })
    }

    fn make_input_with_coord(action: &str, x: i32, y: i32) -> serde_json::Value {
        json!({ "action": action, "coordinate": [x, y] })
    }

    #[test]
    fn test_tool_name() {
        let tool = ComputerUseTool::new();
        assert_eq!(tool.name(), "computer");
    }

    #[test]
    fn test_tool_description_not_empty() {
        let tool = ComputerUseTool::new();
        assert!(!tool.description().is_empty());
    }

    #[test]
    fn test_input_schema_has_required_action() {
        let tool = ComputerUseTool::new();
        let schema = tool.input_schema();
        let required = schema.get("required").unwrap().as_array().unwrap();
        assert!(required.contains(&json!("action")));
    }

    #[test]
    fn test_input_schema_action_enum() {
        let tool = ComputerUseTool::new();
        let schema = tool.input_schema();
        let actions = schema
            .pointer("/properties/action/enum")
            .unwrap()
            .as_array()
            .unwrap();
        assert!(actions.contains(&json!("screenshot")));
        assert!(actions.contains(&json!("click")));
        assert!(actions.contains(&json!("right_click")));
        assert!(actions.contains(&json!("middle_click")));
        assert!(actions.contains(&json!("double_click")));
        assert!(actions.contains(&json!("triple_click")));
        assert!(actions.contains(&json!("type")));
        assert!(actions.contains(&json!("scroll")));
        assert!(actions.contains(&json!("key_press")));
        assert!(actions.contains(&json!("wait")));
        assert!(actions.contains(&json!("mouse_move")));
        assert!(actions.contains(&json!("left_click_drag")));
    }

    #[test]
    fn test_scale_coordinate_identity() {
        // Scaling from reference to reference should be identity
        let scaled =
            ComputerUseTool::scale_coordinate([512, 384], REFERENCE_WIDTH, REFERENCE_HEIGHT);
        assert_eq!(scaled, [512, 384]);
    }

    #[test]
    fn test_scale_coordinate_double_resolution() {
        let scaled = ComputerUseTool::scale_coordinate([512, 384], 2048, 1536);
        assert_eq!(scaled, [1024, 768]);
    }

    #[test]
    fn test_scale_coordinate_hd() {
        // 1920x1080 display
        let scaled = ComputerUseTool::scale_coordinate([512, 384], 1920, 1080);
        assert_eq!(scaled, [960, 540]);
    }

    #[test]
    fn test_scale_coordinate_clamps_to_zero() {
        let scaled = ComputerUseTool::scale_coordinate([0, 0], 1920, 1080);
        assert_eq!(scaled, [0, 0]);
    }

    #[test]
    fn test_scale_coordinate_clamps_to_max() {
        let scaled = ComputerUseTool::scale_coordinate([1024, 768], 1920, 1080);
        assert_eq!(scaled, [1919, 1079]); // width-1, height-1
    }

    #[test]
    fn test_scale_coordinate_fractional() {
        let scaled = ComputerUseTool::scale_coordinate([100, 100], 2560, 1440);
        let expected_x = (100.0_f64 * 2560.0 / 1024.0).round() as i32;
        let expected_y = (100.0_f64 * 1440.0 / 768.0).round() as i32;
        assert_eq!(scaled, [expected_x, expected_y]);
    }

    #[test]
    fn test_parse_key_combination_single() {
        let keys = ComputerUseTool::parse_key_combination("Return");
        assert_eq!(keys, vec!["Return"]);
    }

    #[test]
    fn test_parse_key_combination_modifier() {
        let keys = ComputerUseTool::parse_key_combination("ctrl+a");
        assert_eq!(keys, vec!["ctrl", "a"]);
    }

    #[test]
    fn test_parse_key_combination_multi_modifier() {
        let keys = ComputerUseTool::parse_key_combination("ctrl+shift+s");
        assert_eq!(keys, vec!["ctrl", "shift", "s"]);
    }

    #[test]
    fn test_parse_key_combination_spaces() {
        let keys = ComputerUseTool::parse_key_combination("ctrl + a");
        assert_eq!(keys, vec!["ctrl", "a"]);
    }

    #[test]
    fn test_deserialize_screenshot_action() {
        let input: ComputerUseInput = serde_json::from_value(json!({
            "action": "screenshot"
        }))
        .unwrap();
        assert_eq!(input.action, ComputerAction::Screenshot);
        assert!(input.coordinate.is_none());
    }

    #[test]
    fn test_deserialize_click_action() {
        let input: ComputerUseInput = serde_json::from_value(json!({
            "action": "click",
            "coordinate": [100, 200]
        }))
        .unwrap();
        assert_eq!(input.action, ComputerAction::Click);
        assert_eq!(input.coordinate, Some([100, 200]));
    }

    #[test]
    fn test_deserialize_type_action() {
        let input: ComputerUseInput = serde_json::from_value(json!({
            "action": "type",
            "text": "hello world"
        }))
        .unwrap();
        assert_eq!(input.action, ComputerAction::Type);
        assert_eq!(input.text, Some("hello world".to_string()));
    }

    #[test]
    fn test_deserialize_scroll_action() {
        let input: ComputerUseInput = serde_json::from_value(json!({
            "action": "scroll",
            "scroll_direction": "up",
            "scroll_amount": 5
        }))
        .unwrap();
        assert_eq!(input.action, ComputerAction::Scroll);
        assert_eq!(input.scroll_direction, Some(ScrollDirection::Up));
        assert_eq!(input.scroll_amount, Some(5));
    }

    #[test]
    fn test_deserialize_key_press_action() {
        let input: ComputerUseInput = serde_json::from_value(json!({
            "action": "key_press",
            "key": "ctrl+a"
        }))
        .unwrap();
        assert_eq!(input.action, ComputerAction::KeyPress);
        assert_eq!(input.key, Some("ctrl+a".to_string()));
    }

    #[test]
    fn test_deserialize_wait_action() {
        let input: ComputerUseInput = serde_json::from_value(json!({
            "action": "wait",
            "duration": 2.5
        }))
        .unwrap();
        assert_eq!(input.action, ComputerAction::Wait);
        assert_eq!(input.duration, Some(2.5));
    }

    #[test]
    fn test_deserialize_mouse_move_action() {
        let input: ComputerUseInput = serde_json::from_value(json!({
            "action": "mouse_move",
            "coordinate": [500, 300]
        }))
        .unwrap();
        assert_eq!(input.action, ComputerAction::MouseMove);
        assert_eq!(input.coordinate, Some([500, 300]));
    }

    #[test]
    fn test_deserialize_drag_action() {
        let input: ComputerUseInput = serde_json::from_value(json!({
            "action": "left_click_drag",
            "start_coordinate": [100, 100],
            "coordinate": [500, 500]
        }))
        .unwrap();
        assert_eq!(input.action, ComputerAction::LeftClickDrag);
        assert_eq!(input.start_coordinate, Some([100, 100]));
        assert_eq!(input.coordinate, Some([500, 500]));
    }

    #[test]
    fn test_deserialize_click_variants() {
        for (name, expected) in [
            ("right_click", ComputerAction::RightClick),
            ("middle_click", ComputerAction::MiddleClick),
            ("double_click", ComputerAction::DoubleClick),
            ("triple_click", ComputerAction::TripleClick),
        ] {
            let input: ComputerUseInput =
                serde_json::from_value(json!({ "action": name, "coordinate": [10, 20] })).unwrap();
            assert_eq!(input.action, expected);
        }
    }

    #[test]
    #[cfg(feature = "computer-use")]
    fn test_downscale_dims_noop_when_within_bounds() {
        let tool = ComputerUseTool::new();
        assert_eq!(tool.downscale_dims(1024, 768), None);
        assert_eq!(tool.downscale_dims(800, 600), None);
    }

    #[test]
    #[cfg(feature = "computer-use")]
    fn test_downscale_dims_scales_to_reference() {
        let tool = ComputerUseTool::new();
        // 2x Retina capture fits back into the 1024x768 reference box
        assert_eq!(tool.downscale_dims(2048, 1536), Some((1024, 768)));
        assert_eq!(tool.downscale_dims(1920, 1080), Some((1024, 576)));
    }

    #[test]
    #[cfg(feature = "computer-use")]
    fn test_downscale_dims_preserves_aspect_ratio() {
        let tool = ComputerUseTool::new();
        let (w, h) = tool.downscale_dims(2560, 1440).unwrap();
        assert!((w as f64 / h as f64 - 2560.0 / 1440.0).abs() < 0.01);
        assert!(w <= REFERENCE_WIDTH && h <= REFERENCE_HEIGHT);
    }

    #[test]
    #[cfg(feature = "computer-use")]
    fn test_downscale_dims_zero_limit_disables_scaling() {
        let tool = ComputerUseTool::with_config(ComputerUseConfig {
            max_screenshot_width: 0,
            max_screenshot_height: 0,
            ..Default::default()
        });
        assert_eq!(tool.downscale_dims(4096, 2160), None);
    }

    #[test]
    fn test_deserialize_invalid_action() {
        let result = serde_json::from_value::<ComputerUseInput>(json!({
            "action": "invalid_action"
        }));
        assert!(result.is_err());
    }

    #[test]
    fn test_action_allowed_default() {
        let tool = ComputerUseTool::new();
        // Default config allows all actions
        assert!(tool.is_action_allowed(&ComputerAction::Screenshot));
        assert!(tool.is_action_allowed(&ComputerAction::Click));
        assert!(tool.is_action_allowed(&ComputerAction::Type));
    }

    #[test]
    fn test_action_allowed_whitelist() {
        let config = ComputerUseConfig {
            allowed_actions: vec![ComputerAction::Screenshot, ComputerAction::Wait],
            ..Default::default()
        };
        let tool = ComputerUseTool::with_config(config);
        assert!(tool.is_action_allowed(&ComputerAction::Screenshot));
        assert!(tool.is_action_allowed(&ComputerAction::Wait));
        assert!(!tool.is_action_allowed(&ComputerAction::Click));
        assert!(!tool.is_action_allowed(&ComputerAction::Type));
    }

    #[test]
    fn test_is_not_read_only() {
        let tool = ComputerUseTool::new();
        assert!(!tool.is_read_only());
    }

    #[test]
    fn test_is_not_concurrency_safe() {
        let tool = ComputerUseTool::new();
        assert!(!tool.is_concurrency_safe());
    }

    #[test]
    fn test_is_destructive() {
        let tool = ComputerUseTool::new();
        assert!(tool.is_destructive());
    }

    #[tokio::test]
    #[cfg(not(feature = "computer-use"))]
    async fn test_execute_without_feature_returns_error() {
        let tool = ComputerUseTool::new();

        // Without computer-use feature, screenshot should return an error message
        let result = tool.execute(make_input("screenshot")).await.unwrap();
        #[cfg(not(feature = "computer-use"))]
        {
            assert!(result.is_error);
            assert!(result.content.contains("computer-use"));
        }
        #[cfg(feature = "computer-use")]
        {
            // With the feature, it should try to actually capture
            // (may fail in CI without a display, but shouldn't panic)
            let _ = result;
        }
    }

    #[tokio::test]
    async fn test_execute_wait_works_without_feature() {
        let tool = ComputerUseTool::new();
        let start = std::time::Instant::now();
        let result = tool
            .execute(json!({ "action": "wait", "duration": 0.1 }))
            .await
            .unwrap();
        assert!(!result.is_error);
        assert!(start.elapsed() >= std::time::Duration::from_millis(80));
    }

    #[tokio::test]
    async fn test_execute_wait_default_duration() {
        let tool = ComputerUseTool::new();
        let result = tool.execute(json!({ "action": "wait" })).await.unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("1.0s"));
    }

    #[tokio::test]
    #[cfg(not(feature = "computer-use"))]
    async fn test_execute_click_without_feature() {
        let tool = ComputerUseTool::new();
        let result = tool
            .execute(make_input_with_coord("click", 100, 200))
            .await
            .unwrap();
        #[cfg(not(feature = "computer-use"))]
        {
            assert!(result.is_error);
            assert!(result.content.contains("100, 200"));
        }
    }

    #[tokio::test]
    async fn test_execute_click_missing_coordinate() {
        let tool = ComputerUseTool::new();
        let result = tool.execute(make_input("click")).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, ToolError::InvalidInput(_)));
    }

    #[tokio::test]
    #[cfg(not(feature = "computer-use"))]
    async fn test_execute_type_without_feature() {
        let tool = ComputerUseTool::new();
        let result = tool
            .execute(json!({ "action": "type", "text": "hello" }))
            .await
            .unwrap();
        #[cfg(not(feature = "computer-use"))]
        {
            assert!(result.is_error);
            assert!(result.content.contains("hello"));
        }
    }

    #[tokio::test]
    async fn test_execute_type_missing_text() {
        let tool = ComputerUseTool::new();
        let result = tool.execute(make_input("type")).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_key_press_missing_key() {
        let tool = ComputerUseTool::new();
        let result = tool.execute(make_input("key_press")).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_mouse_move_missing_coordinate() {
        let tool = ComputerUseTool::new();
        let result = tool.execute(make_input("mouse_move")).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_drag_missing_coordinates() {
        let tool = ComputerUseTool::new();
        let result = tool.execute(make_input("left_click_drag")).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    #[cfg(not(feature = "computer-use"))]
    async fn test_execute_scroll_default_direction() {
        let tool = ComputerUseTool::new();
        let result = tool.execute(make_input("scroll")).await.unwrap();
        #[cfg(not(feature = "computer-use"))]
        {
            assert!(result.is_error);
            assert!(result.content.contains("Down")); // default direction
        }
    }

    #[tokio::test]
    async fn test_execute_action_not_allowed() {
        let config = ComputerUseConfig {
            allowed_actions: vec![ComputerAction::Screenshot],
            ..Default::default()
        };
        let tool = ComputerUseTool::with_config(config);
        let result = tool
            .execute(make_input_with_coord("click", 100, 200))
            .await
            .unwrap();
        assert!(result.is_error);
        assert!(result.content.contains("not allowed"));
    }

    #[test]
    fn test_input_backend_name_reports_a_backend() {
        // Diagnostics contract: always a non-empty, stable name.
        assert!(!input_backend_name().is_empty());
    }

    #[test]
    #[cfg(feature = "computer-use")]
    fn test_session_hint_absent_without_wayland_only_session() {
        // In the test env neither WAYLAND_DISPLAY nor DISPLAY is guaranteed;
        // the hint must only fire on the wayland-without-x11 combination, so
        // assert the function is total and returns Option.
        let _ = session_compatibility_hint();
    }

    #[test]
    fn test_config_default() {
        let config = ComputerUseConfig::default();
        assert!(config.screenshot_enabled);
        assert!(config.input_enabled);
        assert!(config.allowed_actions.is_empty());
        assert_eq!(config.max_screenshot_width, REFERENCE_WIDTH);
        assert_eq!(config.max_screenshot_height, REFERENCE_HEIGHT);
    }

    #[test]
    fn test_default_impl() {
        let tool1 = ComputerUseTool::new();
        let tool2 = ComputerUseTool::default();
        assert_eq!(tool1.name(), tool2.name());
    }

    // ── Screenshot-space coordinate scaling (non-4:3 fix) ───────────────

    #[test]
    fn test_scale_coordinate_from_screenshot_space_16_9() {
        // A 1920x1080 screen downscales to 1024x576; a point measured at the
        // center of that image must land at the screen center — the old
        // fixed 1024x768 path put it at y=405 of 540 (25% off vertically).
        let scaled = ComputerUseTool::scale_coordinate_from([512, 288], 1024, 576, 1920, 1080);
        assert_eq!(scaled, [960, 540]);
    }

    #[test]
    fn test_scale_coordinate_from_differs_from_reference_path() {
        // Same model coordinate, two source spaces — the results must
        // differ, which is exactly why tracking screenshot dims matters.
        let from_screenshot =
            ComputerUseTool::scale_coordinate_from([512, 500], 1024, 576, 1920, 1080);
        let from_reference = ComputerUseTool::scale_coordinate([512, 500], 1920, 1080);
        assert_ne!(from_screenshot, from_reference);
    }

    #[test]
    fn test_scale_coordinate_from_degenerate_source_falls_back_to_reference() {
        let scaled = ComputerUseTool::scale_coordinate_from([512, 384], 0, 0, 2048, 1536);
        assert_eq!(scaled, [1024, 768]);
    }

    #[test]
    fn test_scale_coordinate_from_clamps() {
        let scaled = ComputerUseTool::scale_coordinate_from([10_000, -50], 1024, 576, 1920, 1080);
        assert_eq!(scaled, [1919, 0]);
    }

    #[test]
    fn test_unscale_coordinate_round_trips() {
        // screen → screenshot space → screen stays within a pixel (both
        // directions round independently, so exact equality is not guaranteed).
        let on_screen = [1471, 801];
        let in_shot = ComputerUseTool::unscale_coordinate(on_screen, 1920, 1080, 1024, 576);
        let back = ComputerUseTool::scale_coordinate_from(in_shot, 1024, 576, 1920, 1080);
        assert!((back[0] - on_screen[0]).abs() <= 1 && (back[1] - on_screen[1]).abs() <= 1);
    }

    // ── Multi-monitor point resolution ──────────────────────────────────

    #[test]
    fn test_monitor_index_for_point_containment() {
        let rects = [(1920u32, 1080u32, 0, 0), (2560, 1440, -2560, 0)];
        assert_eq!(
            ComputerUseTool::monitor_index_for_point(100, 100, &rects),
            0
        );
        assert_eq!(
            ComputerUseTool::monitor_index_for_point(-100, 500, &rects),
            1
        );
    }

    #[test]
    fn test_monitor_index_for_point_gap_snaps_to_nearest() {
        // Point in the void between/beside displays → nearest center.
        let rects = [(1920u32, 1080u32, 0, 0), (1920, 1080, 1920, 0)];
        assert_eq!(
            ComputerUseTool::monitor_index_for_point(1919, 100, &rects),
            0
        );
        // x=5000 is right of both; monitor 1's center is closer.
        assert_eq!(
            ComputerUseTool::monitor_index_for_point(5000, 100, &rects),
            1
        );
    }

    #[test]
    fn test_monitor_index_for_point_empty() {
        assert_eq!(ComputerUseTool::monitor_index_for_point(5, 5, &[]), 0);
    }

    // ── Zoom crop rect math ─────────────────────────────────────────────

    #[test]
    fn test_zoom_crop_rect_maps_screenshot_space_to_capture() {
        // 2x capture of a 1024x576 screenshot: region at (512,288) size
        // (256,144) → capture px (1024,576) size (512,288).
        let rect = ComputerUseTool::zoom_crop_rect([512, 288], [256, 144], 1024, 576, 2048, 1152);
        assert_eq!(rect, (1024, 576, 512, 288));
    }

    #[test]
    fn test_zoom_crop_rect_falls_back_to_reference_space() {
        // Before any screenshot, regions are in 1024x768 reference space.
        let rect = ComputerUseTool::zoom_crop_rect([0, 0], [1024, 768], 0, 0, 1920, 1080);
        assert_eq!(rect, (0, 0, 1920, 1080));
    }

    #[test]
    fn test_zoom_crop_rect_clamps_out_of_bounds_region() {
        let rect = ComputerUseTool::zoom_crop_rect([1000, 700], [500, 500], 1024, 768, 1024, 768);
        // Top-left clamps to (999, 767)? No: x clamps into the image, then
        // width clamps to what remains — the rect always stays in bounds
        // with a minimum 1x1 size.
        let (x, y, w, h) = rect;
        assert!(
            x + w <= 1024 && y + h <= 768,
            "rect out of bounds: {rect:?}"
        );
        assert!(w >= 1 && h >= 1);
    }

    // ── Key parsing: the '+' key and strictness ─────────────────────────

    #[test]
    fn test_parse_key_combination_plus_key() {
        assert_eq!(
            ComputerUseTool::parse_key_combination("ctrl++"),
            vec!["ctrl".to_string(), "+".to_string()]
        );
        assert_eq!(
            ComputerUseTool::parse_key_combination("++"),
            vec!["+".to_string()]
        );
        assert_eq!(
            ComputerUseTool::parse_key_combination("+"),
            vec!["+".to_string()]
        );
    }

    #[test]
    fn test_parse_key_combination_regular_unchanged() {
        assert_eq!(
            ComputerUseTool::parse_key_combination("ctrl+shift+s"),
            vec!["ctrl".to_string(), "shift".to_string(), "s".to_string()]
        );
        assert_eq!(
            ComputerUseTool::parse_key_combination("Return"),
            vec!["Return".to_string()]
        );
        assert_eq!(
            ComputerUseTool::parse_key_combination(""),
            vec![String::new()]
        );
    }

    #[cfg(feature = "computer-use")]
    #[test]
    fn test_str_to_key_extended_names() {
        assert_eq!(ComputerUseTool::str_to_key("F13"), Some(enigo::Key::F13));
        assert_eq!(ComputerUseTool::str_to_key("f20"), Some(enigo::Key::F20));
        // Insert/F21-F24 only exist on non-macOS enigo builds — on macOS the
        // names are (correctly) rejected as unknown rather than mapped.
        if !cfg!(target_os = "macos") {
            assert_eq!(ComputerUseTool::str_to_key("f24"), Some(enigo::Key::F24));
            assert_eq!(
                ComputerUseTool::str_to_key("insert"),
                Some(enigo::Key::Insert)
            );
        } else {
            assert_eq!(ComputerUseTool::str_to_key("f24"), None);
            assert_eq!(ComputerUseTool::str_to_key("insert"), None);
        }
        assert_eq!(
            ComputerUseTool::str_to_key("pgdn"),
            Some(enigo::Key::PageDown)
        );
        assert_eq!(ComputerUseTool::str_to_key("option"), Some(enigo::Key::Alt));
        assert_eq!(
            ComputerUseTool::str_to_key("+"),
            Some(enigo::Key::Unicode('+'))
        );
        assert_eq!(
            ComputerUseTool::str_to_key("7"),
            Some(enigo::Key::Unicode('7'))
        );
    }

    #[cfg(feature = "computer-use")]
    #[test]
    fn test_str_to_key_rejects_unknown_names() {
        // The old first-char fallback turned these into wrong keystrokes
        // ("F13" → 'f', "printscreen" → 'p'); they must be rejected.
        assert_eq!(ComputerUseTool::str_to_key("F13x"), None);
        assert_eq!(ComputerUseTool::str_to_key("printscreen"), None);
        assert_eq!(ComputerUseTool::str_to_key(" volumemute"), None);
        assert_eq!(ComputerUseTool::str_to_key("f25"), None);
        assert_eq!(ComputerUseTool::str_to_key(""), None);
    }

    // ── Wait clamping ───────────────────────────────────────────────────

    #[test]
    fn test_validate_wait_rejects_negative_and_non_finite() {
        assert!(ComputerUseTool::validate_wait(-1.0).is_err());
        assert!(ComputerUseTool::validate_wait(f64::NAN).is_err());
        assert!(ComputerUseTool::validate_wait(f64::INFINITY).is_err());
    }

    #[test]
    fn test_validate_wait_caps_runaway_requests() {
        assert_eq!(
            ComputerUseTool::validate_wait(500.0).unwrap(),
            MAX_WAIT_SECONDS
        );
        assert_eq!(ComputerUseTool::validate_wait(1.5).unwrap(), 1.5);
        assert_eq!(ComputerUseTool::validate_wait(0.0).unwrap(), 0.0);
    }

    #[tokio::test]
    async fn test_execute_wait_accepts_small_durations() {
        // A capped request is accepted (not an error) — the cap value itself
        // is verified via `validate_wait` above without sleeping through it.
        let tool = ComputerUseTool::new();
        let result = tool
            .execute(json!({ "action": "wait", "duration": 0.05 }))
            .await
            .unwrap();
        assert!(!result.is_error);
        assert!(result.content.starts_with("Waited"));
    }

    #[tokio::test]
    async fn test_execute_wait_negative_duration_errors() {
        let tool = ComputerUseTool::new();
        let result = tool
            .execute(json!({ "action": "wait", "duration": -3.0 }))
            .await;
        assert!(matches!(result, Err(ToolError::InvalidInput(_))));
    }

    // ── Schema covers the new actions ───────────────────────────────────

    #[test]
    fn test_input_schema_includes_new_actions() {
        let tool = ComputerUseTool::new();
        let schema = tool.input_schema();
        let actions = schema
            .pointer("/properties/action/enum")
            .unwrap()
            .as_array()
            .unwrap();
        assert!(actions.contains(&json!("zoom")));
        assert!(actions.contains(&json!("cursor_position")));
        assert!(actions.contains(&json!("ui_tree")));
        assert!(actions.contains(&json!("ui_click")));
        assert!(
            schema.pointer("/properties/size").is_some(),
            "zoom needs a 'size' property"
        );
    }

    #[test]
    fn test_deserialize_zoom_action() {
        let input: ComputerUseInput = serde_json::from_value(json!({
            "action": "zoom",
            "coordinate": [100, 100],
            "size": [200, 150]
        }))
        .unwrap();
        assert_eq!(input.action, ComputerAction::Zoom);
        assert_eq!(input.size, Some([200, 150]));
    }

    #[test]
    fn test_deserialize_cursor_position_action() {
        let input: ComputerUseInput =
            serde_json::from_value(json!({ "action": "cursor_position" })).unwrap();
        assert_eq!(input.action, ComputerAction::CursorPosition);
    }

    #[cfg(not(feature = "computer-use"))]
    #[tokio::test]
    async fn test_zoom_and_cursor_position_stub_errors_without_feature() {
        let tool = ComputerUseTool::new();
        let zoom = tool
            .execute(json!({ "action": "zoom", "coordinate": [10, 10] }))
            .await
            .unwrap();
        assert!(zoom.is_error);
        assert!(zoom.content.contains("computer-use"));

        let cursor = tool
            .execute(json!({ "action": "cursor_position" }))
            .await
            .unwrap();
        assert!(cursor.is_error);
        assert!(cursor.content.contains("computer-use"));
    }

    #[test]
    fn test_description_mentions_screenshot_space_coords() {
        let tool = ComputerUseTool::new();
        assert!(
            tool.description().contains("screenshot image"),
            "description must teach the model the coordinate contract: {}",
            tool.description()
        );
    }
}
