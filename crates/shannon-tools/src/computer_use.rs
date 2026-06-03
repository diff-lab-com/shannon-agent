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

/// Actions supported by the computer use tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComputerAction {
    Screenshot,
    Click,
    Type,
    Scroll,
    KeyPress,
    Wait,
    MouseMove,
    LeftClickDrag,
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
}

impl Default for ComputerUseTool {
    fn default() -> Self {
        Self::new()
    }
}

impl ComputerUseTool {
    pub fn new() -> Self {
        Self {
            description: "Interact with the computer desktop: take screenshots, click, type, scroll, and press keys. Coordinates are in [0-1024, 0-768] range and scaled to actual screen resolution.".to_string(),
            config: ComputerUseConfig::default(),
        }
    }

    pub fn with_config(config: ComputerUseConfig) -> Self {
        Self {
            description: "Interact with the computer desktop: take screenshots, click, type, scroll, and press keys. Coordinates are in [0-1024, 0-768] range and scaled to actual screen resolution.".to_string(),
            config,
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
        let x = (coord[0] as f64 * actual_width as f64 / REFERENCE_WIDTH as f64).round() as i32;
        let y = (coord[1] as f64 * actual_height as f64 / REFERENCE_HEIGHT as f64).round() as i32;
        [
            x.clamp(0, actual_width as i32 - 1),
            y.clamp(0, actual_height as i32 - 1),
        ]
    }

    /// Parse a key combination string into individual keys.
    /// "ctrl+a" → ["ctrl", "a"], "alt+F4" → ["alt", "F4"]
    pub fn parse_key_combination(key: &str) -> Vec<String> {
        key.split('+').map(|s| s.trim().to_string()).collect()
    }

    /// Convert a key name string to an enigo Key enum value.
    #[cfg(feature = "computer-use")]
    fn str_to_key(name: &str) -> enigo::Key {
        match name.to_lowercase().as_str() {
            "ctrl" | "control" => enigo::Key::Control,
            "alt" => enigo::Key::Alt,
            "shift" => enigo::Key::Shift,
            "meta" | "cmd" | "command" | "super" | "win" => enigo::Key::Meta,
            "return" | "enter" => enigo::Key::Return,
            "tab" => enigo::Key::Tab,
            "space" => enigo::Key::Space,
            "backspace" | "back" => enigo::Key::Backspace,
            "delete" | "del" => enigo::Key::Delete,
            "escape" | "esc" => enigo::Key::Escape,
            "up" => enigo::Key::UpArrow,
            "down" => enigo::Key::DownArrow,
            "left" => enigo::Key::LeftArrow,
            "right" => enigo::Key::RightArrow,
            "home" => enigo::Key::Home,
            "end" => enigo::Key::End,
            "pageup" | "page_up" => enigo::Key::PageUp,
            "pagedown" | "page_down" => enigo::Key::PageDown,
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
            c if c.len() == 1 => enigo::Key::Unicode(c.chars().next().unwrap()),
            _ => enigo::Key::Unicode(name.chars().next().unwrap_or('\0')),
        }
    }

    fn build_input_schema() -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["screenshot", "click", "type", "scroll", "key_press", "wait", "mouse_move", "left_click_drag"],
                    "description": "The action to perform"
                },
                "coordinate": {
                    "type": "array",
                    "items": { "type": "integer" },
                    "maxItems": 2,
                    "minItems": 2,
                    "description": "[x, y] coordinates in reference space (0-1024, 0-768)"
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
                    "description": "Key or key combination, e.g. 'Return', 'ctrl+a', 'alt+F4'"
                },
                "duration": {
                    "type": "number",
                    "description": "Seconds to wait (for 'wait' action, default 1.0)"
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

        match computer_input.action {
            ComputerAction::Screenshot => self.execute_screenshot().await,
            ComputerAction::Click => {
                let coord = computer_input.coordinate.ok_or_else(|| {
                    ToolError::InvalidInput("click action requires 'coordinate'".to_string())
                })?;
                self.execute_click(coord).await
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
                self.execute_scroll(direction, amount, coord).await
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
                self.execute_mouse_move(coord).await
            }
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
                self.execute_drag(start, end).await
            }
        }
    }
}

// Action implementations — feature-gated

impl ComputerUseTool {
    #[cfg(feature = "computer-use")]
    async fn execute_screenshot(&self) -> ToolResult<ToolOutput> {
        if !self.config.screenshot_enabled {
            return Ok(ToolOutput {
                content: "Screenshot capture is disabled.".to_string(),
                is_error: true,
                metadata: HashMap::new(),
            });
        }

        let monitors = xcap::Monitor::all()
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to get monitors: {e}")))?;

        let monitor = monitors
            .into_iter()
            .next()
            .ok_or_else(|| ToolError::ExecutionFailed("No monitors found".to_string()))?;

        let width = monitor.width();
        let height = monitor.height();

        let image = monitor
            .capture_image()
            .map_err(|e| ToolError::ExecutionFailed(format!("Screenshot failed: {e}")))?;

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

        Ok(ToolOutput {
            content: format!("Screenshot captured ({}x{})", width, height),
            is_error: false,
            metadata,
        })
    }

    #[cfg(not(feature = "computer-use"))]
    async fn execute_screenshot(&self) -> ToolResult<ToolOutput> {
        Ok(ToolOutput {
            content: "Computer use feature is not enabled. Rebuild with `--features computer-use` to enable screen capture and input simulation.".to_string(),
            is_error: true,
            metadata: HashMap::new(),
        })
    }

    #[cfg(feature = "computer-use")]
    async fn execute_click(&self, coord: [i32; 2]) -> ToolResult<ToolOutput> {
        if !self.config.input_enabled {
            return Ok(ToolOutput {
                content: "Input simulation is disabled.".to_string(),
                is_error: true,
                metadata: HashMap::new(),
            });
        }

        let (actual_w, actual_h) = Self::screen_size();
        let scaled = Self::scale_coordinate(coord, actual_w, actual_h);

        let mut enigo = enigo::Enigo::new(&enigo::Settings::default())
            .map_err(|e| ToolError::ExecutionFailed(format!("Input init failed: {e}")))?;

        enigo
            .move_mouse(scaled[0], scaled[1], enigo::Coordinate::Abs)
            .map_err(|e| ToolError::ExecutionFailed(format!("Mouse move failed: {e}")))?;

        enigo
            .button(enigo::Button::Left, Direction::Press)
            .map_err(|e| ToolError::ExecutionFailed(format!("Mouse press failed: {e}")))?;

        enigo
            .button(enigo::Button::Left, Direction::Release)
            .map_err(|e| ToolError::ExecutionFailed(format!("Mouse release failed: {e}")))?;

        Ok(ToolOutput {
            content: format!(
                "Clicked at ({}, {}) [scaled from ({}, {})]",
                scaled[0], scaled[1], coord[0], coord[1]
            ),
            is_error: false,
            metadata: HashMap::new(),
        })
    }

    #[cfg(not(feature = "computer-use"))]
    async fn execute_click(&self, coord: [i32; 2]) -> ToolResult<ToolOutput> {
        Ok(ToolOutput {
            content: format!(
                "Computer use not enabled. Would click at ({}, {}). Rebuild with --features computer-use.",
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

        let mut enigo = enigo::Enigo::new(&enigo::Settings::default())
            .map_err(|e| ToolError::ExecutionFailed(format!("Input init failed: {e}")))?;

        enigo
            .text(text)
            .map_err(|e| ToolError::ExecutionFailed(format!("Text input failed: {e}")))?;

        Ok(ToolOutput {
            content: format!("Typed {} characters", text.len()),
            is_error: false,
            metadata: HashMap::new(),
        })
    }

    #[cfg(not(feature = "computer-use"))]
    async fn execute_type(&self, text: &str) -> ToolResult<ToolOutput> {
        Ok(ToolOutput {
            content: format!(
                "Computer use not enabled. Would type '{}' ({} chars). Rebuild with --features computer-use.",
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
    ) -> ToolResult<ToolOutput> {
        if !self.config.input_enabled {
            return Ok(ToolOutput {
                content: "Input simulation is disabled.".to_string(),
                is_error: true,
                metadata: HashMap::new(),
            });
        }

        let mut enigo = enigo::Enigo::new(&enigo::Settings::default())
            .map_err(|e| ToolError::ExecutionFailed(format!("Input init failed: {e}")))?;

        // Move to coordinate if provided
        if let Some(c) = coord {
            let (actual_w, actual_h) = Self::screen_size();
            let scaled = Self::scale_coordinate(c, actual_w, actual_h);
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

        Ok(ToolOutput {
            content: format!("Scrolled {:?} x{}", direction, amount),
            is_error: false,
            metadata: HashMap::new(),
        })
    }

    #[cfg(not(feature = "computer-use"))]
    async fn execute_scroll(
        &self,
        direction: ScrollDirection,
        amount: i32,
        _coord: Option<[i32; 2]>,
    ) -> ToolResult<ToolOutput> {
        Ok(ToolOutput {
            content: format!(
                "Computer use not enabled. Would scroll {:?} x{}. Rebuild with --features computer-use.",
                direction, amount
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

        let mut enigo = enigo::Enigo::new(&enigo::Settings::default())
            .map_err(|e| ToolError::ExecutionFailed(format!("Input init failed: {e}")))?;

        let keys = Self::parse_key_combination(key);
        let enigo_keys: Vec<enigo::Key> = keys.iter().map(|k| Self::str_to_key(k)).collect();
        // For simple single keys, click directly
        if enigo_keys.len() == 1 {
            enigo
                .key(enigo_keys[0].clone(), Direction::Click)
                .map_err(|e| ToolError::ExecutionFailed(format!("Key press failed: {e}")))?;
        } else {
            // Press modifiers first, then the main key, then release in reverse
            for k in &enigo_keys {
                enigo
                    .key(k.clone(), Direction::Press)
                    .map_err(|e| ToolError::ExecutionFailed(format!("Key press failed: {e}")))?;
            }
            for k in enigo_keys.iter().rev() {
                enigo
                    .key(k.clone(), Direction::Release)
                    .map_err(|e| ToolError::ExecutionFailed(format!("Key release failed: {e}")))?;
            }
        }

        Ok(ToolOutput {
            content: format!("Pressed key: {}", key),
            is_error: false,
            metadata: HashMap::new(),
        })
    }

    #[cfg(not(feature = "computer-use"))]
    async fn execute_key_press(&self, key: &str) -> ToolResult<ToolOutput> {
        Ok(ToolOutput {
            content: format!(
                "Computer use not enabled. Would press '{}'. Rebuild with --features computer-use.",
                key
            ),
            is_error: true,
            metadata: HashMap::new(),
        })
    }

    #[cfg(feature = "computer-use")]
    async fn execute_wait(&self, duration: f64) -> ToolResult<ToolOutput> {
        let millis = (duration * 1000.0) as u64;
        tokio::time::sleep(std::time::Duration::from_millis(millis)).await;
        Ok(ToolOutput {
            content: format!("Waited {:.1}s", duration),
            is_error: false,
            metadata: HashMap::new(),
        })
    }

    #[cfg(not(feature = "computer-use"))]
    async fn execute_wait(&self, duration: f64) -> ToolResult<ToolOutput> {
        // Even without the feature, wait is safe to execute
        let millis = (duration * 1000.0) as u64;
        tokio::time::sleep(std::time::Duration::from_millis(millis)).await;
        Ok(ToolOutput {
            content: format!("Waited {:.1}s", duration),
            is_error: false,
            metadata: HashMap::new(),
        })
    }

    #[cfg(feature = "computer-use")]
    async fn execute_mouse_move(&self, coord: [i32; 2]) -> ToolResult<ToolOutput> {
        if !self.config.input_enabled {
            return Ok(ToolOutput {
                content: "Input simulation is disabled.".to_string(),
                is_error: true,
                metadata: HashMap::new(),
            });
        }

        let (actual_w, actual_h) = Self::screen_size();
        let scaled = Self::scale_coordinate(coord, actual_w, actual_h);

        let mut enigo = enigo::Enigo::new(&enigo::Settings::default())
            .map_err(|e| ToolError::ExecutionFailed(format!("Input init failed: {e}")))?;

        enigo
            .move_mouse(scaled[0], scaled[1], enigo::Coordinate::Abs)
            .map_err(|e| ToolError::ExecutionFailed(format!("Mouse move failed: {e}")))?;

        Ok(ToolOutput {
            content: format!(
                "Moved mouse to ({}, {}) [scaled from ({}, {})]",
                scaled[0], scaled[1], coord[0], coord[1]
            ),
            is_error: false,
            metadata: HashMap::new(),
        })
    }

    #[cfg(not(feature = "computer-use"))]
    async fn execute_mouse_move(&self, coord: [i32; 2]) -> ToolResult<ToolOutput> {
        Ok(ToolOutput {
            content: format!(
                "Computer use not enabled. Would move to ({}, {}). Rebuild with --features computer-use.",
                coord[0], coord[1]
            ),
            is_error: true,
            metadata: HashMap::new(),
        })
    }

    #[cfg(feature = "computer-use")]
    async fn execute_drag(&self, start: [i32; 2], end: [i32; 2]) -> ToolResult<ToolOutput> {
        if !self.config.input_enabled {
            return Ok(ToolOutput {
                content: "Input simulation is disabled.".to_string(),
                is_error: true,
                metadata: HashMap::new(),
            });
        }

        let (actual_w, actual_h) = Self::screen_size();
        let scaled_start = Self::scale_coordinate(start, actual_w, actual_h);
        let scaled_end = Self::scale_coordinate(end, actual_w, actual_h);

        let mut enigo = enigo::Enigo::new(&enigo::Settings::default())
            .map_err(|e| ToolError::ExecutionFailed(format!("Input init failed: {e}")))?;

        // Move to start, press, drag to end, release
        enigo
            .move_mouse(scaled_start[0], scaled_start[1], enigo::Coordinate::Abs)
            .map_err(|e| ToolError::ExecutionFailed(format!("Mouse move failed: {e}")))?;

        enigo
            .button(enigo::Button::Left, Direction::Press)
            .map_err(|e| ToolError::ExecutionFailed(format!("Mouse press failed: {e}")))?;

        enigo
            .move_mouse(scaled_end[0], scaled_end[1], enigo::Coordinate::Abs)
            .map_err(|e| ToolError::ExecutionFailed(format!("Mouse move failed: {e}")))?;

        enigo
            .button(enigo::Button::Left, Direction::Release)
            .map_err(|e| ToolError::ExecutionFailed(format!("Mouse release failed: {e}")))?;

        Ok(ToolOutput {
            content: format!(
                "Dragged from ({}, {}) to ({}, {})",
                scaled_start[0], scaled_start[1], scaled_end[0], scaled_end[1]
            ),
            is_error: false,
            metadata: HashMap::new(),
        })
    }

    #[cfg(not(feature = "computer-use"))]
    async fn execute_drag(&self, start: [i32; 2], end: [i32; 2]) -> ToolResult<ToolOutput> {
        Ok(ToolOutput {
            content: format!(
                "Computer use not enabled. Would drag ({}, {}) to ({}, {}). Rebuild with --features computer-use.",
                start[0], start[1], end[0], end[1]
            ),
            is_error: true,
            metadata: HashMap::new(),
        })
    }

    /// Get the actual screen size.
    #[cfg(feature = "computer-use")]
    fn screen_size() -> (u32, u32) {
        match xcap::Monitor::all() {
            Ok(monitors) => {
                if let Some(m) = monitors.into_iter().next() {
                    (m.width(), m.height())
                } else {
                    (REFERENCE_WIDTH, REFERENCE_HEIGHT)
                }
            }
            Err(_) => (REFERENCE_WIDTH, REFERENCE_HEIGHT),
        }
    }
}

#[cfg(test)]
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
}
