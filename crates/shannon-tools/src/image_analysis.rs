//! AnalyzeImage tool implementation
//!
//! Loads an image from a file path or URL and returns it as base64-encoded
//! data so the query engine can construct a `ContentBlock::Image` for the
//! LLM. The LLM then "sees" the image and can describe or analyze it based
//! on the user's prompt.

use crate::{Tool, ToolError, ToolOutput, ToolResult};
use async_trait::async_trait;
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;

/// Maximum image file size: 20 MB
const MAX_IMAGE_SIZE: u64 = 20 * 1024 * 1024;

/// Image file extensions we support
const IMAGE_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "bmp", "ico", "tiff", "tif",
];

/// Input parameters for the AnalyzeImage tool
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AnalyzeImageInput {
    /// Absolute path to the image file to analyze.
    /// Either `file_path` or `url` must be provided.
    pub file_path: Option<String>,

    /// URL of the image to analyze.
    /// Either `file_path` or `url` must be provided.
    pub url: Option<String>,

    /// What to analyze or describe about the image.
    pub prompt: String,
}

/// Output structure for image analysis results
#[derive(Debug, Serialize)]
struct ImageAnalysisOutput {
    /// Type identifier for downstream detection
    #[serde(rename = "type")]
    output_type: String,

    /// Media type (e.g., "image/png")
    media_type: String,

    /// Base64-encoded image data
    data: String,

    /// Source description (file path or URL)
    source: String,

    /// File size in bytes (0 for URLs)
    size: u64,

    /// The user's analysis prompt
    prompt: String,
}

/// AnalyzeImage tool: loads an image and returns base64 data for LLM analysis.
///
/// The tool supports loading images from:
/// - Local file paths (absolute paths)
/// - Remote URLs (http/https)
///
/// The returned `ToolOutput` includes metadata with `"type": "image"` so
/// the query engine can construct a `ContentBlock::Image` block for the LLM.
pub struct AnalyzeImageTool {
    description: String,
}

impl Default for AnalyzeImageTool {
    fn default() -> Self {
        Self::new()
    }
}

impl AnalyzeImageTool {
    pub fn new() -> Self {
        Self {
            description: "Analyze an image file or URL. The image is sent to the LLM for visual analysis based on your prompt. Supports PNG, JPEG, GIF, WebP, BMP, ICO, and TIFF formats.".to_string(),
        }
    }

    /// Determine MIME type from a file extension.
    fn mime_type_from_path(path: &str) -> &'static str {
        let path_lower = path.to_ascii_lowercase();
        let ext = path_lower.rsplit('.').next().unwrap_or("");
        Self::mime_type_from_ext(ext)
    }

    /// Determine MIME type from a file extension string.
    fn mime_type_from_ext(ext: &str) -> &'static str {
        match ext.to_ascii_lowercase().as_str() {
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "webp" => "image/webp",
            "bmp" => "image/bmp",
            "ico" => "image/x-icon",
            "tiff" | "tif" => "image/tiff",
            "svg" => "image/svg+xml",
            _ => "application/octet-stream",
        }
    }

    /// Determine MIME type from a URL (extracts extension from path component).
    fn mime_type_from_url(url: &str) -> &'static str {
        // Strip query string and fragment, then extract extension
        let path_part = url.split('?').next().unwrap_or(url);
        let path_part = path_part.split('#').next().unwrap_or(path_part);
        let path_lower = path_part.to_ascii_lowercase();
        // Get the last segment after '/', then the extension after '.'
        let filename = path_lower.rsplit('/').next().unwrap_or("");
        let ext = filename.rsplit('.').next().unwrap_or("");
        Self::mime_type_from_ext(ext)
    }

    /// Check if a file path has an image extension.
    fn is_image_extension(path: &str) -> bool {
        let path_lower = path.to_ascii_lowercase();
        path_lower
            .rsplit('.')
            .next()
            .map(|ext| IMAGE_EXTENSIONS.contains(&ext))
            .unwrap_or(false)
    }

    /// Load an image from a local file path, returning (base64_data, mime_type, size).
    async fn load_from_file(
        file_path: &str,
    ) -> Result<(String, &'static str, u64), ToolError> {
        use tokio::fs;

        // Validate extension
        if !Self::is_image_extension(file_path) {
            return Err(ToolError::InvalidInput(format!(
                "File does not appear to be an image: {file_path}. \
                 Supported formats: {}",
                IMAGE_EXTENSIONS.join(", ")
            )));
        }

        let metadata = fs::metadata(file_path)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to stat file: {e}")))?;

        let size = metadata.len();
        if size > MAX_IMAGE_SIZE {
            return Err(ToolError::ExecutionFailed(format!(
                "Image file too large: {size} bytes (max {MAX_IMAGE_SIZE} bytes)",
            )));
        }

        let bytes = fs::read(file_path)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to read image file: {e}")))?;

        let engine = base64::engine::general_purpose::STANDARD;
        let base64_data = engine.encode(&bytes);
        let mime_type = Self::mime_type_from_path(file_path);

        Ok((base64_data, mime_type, size))
    }

    /// Load an image from a URL, returning (base64_data, mime_type, size).
    async fn load_from_url(
        url: &str,
    ) -> Result<(String, &'static str, u64), ToolError> {
        // Validate URL scheme
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err(ToolError::InvalidInput(
                "URL must use http:// or https:// scheme".to_string(),
            ));
        }

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to create HTTP client: {e}")))?;

        let response = client
            .get(url)
            .send()
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to fetch image URL: {e}")))?;

        if !response.status().is_success() {
            return Err(ToolError::ExecutionFailed(format!(
                "HTTP error fetching image: {}",
                response.status()
            )));
        }

        let content_length = response.content_length().unwrap_or(0);
        if content_length > MAX_IMAGE_SIZE {
            return Err(ToolError::ExecutionFailed(format!(
                "Image from URL too large: {content_length} bytes (max {MAX_IMAGE_SIZE} bytes)",
            )));
        }

        // Extract Content-Type header before consuming the response
        let mime_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .map(|ct| {
                // Extract just the MIME type part (before semicolon)
                ct.split(';').next().unwrap_or(ct).trim()
            })
            .filter(|ct| ct.starts_with("image/"))
            .map(|ct| -> &'static str {
                // Return the header value as a static str if it matches known types
                match ct {
                    "image/png" => "image/png",
                    "image/jpeg" => "image/jpeg",
                    "image/gif" => "image/gif",
                    "image/webp" => "image/webp",
                    "image/bmp" => "image/bmp",
                    "image/x-icon" => "image/x-icon",
                    "image/tiff" => "image/tiff",
                    "image/svg+xml" => "image/svg+xml",
                    _ => "application/octet-stream",
                }
            })
            .unwrap_or_else(|| Self::mime_type_from_url(url));

        let bytes = response
            .bytes()
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to read image response: {e}")))?;

        let size = bytes.len() as u64;
        if size > MAX_IMAGE_SIZE {
            return Err(ToolError::ExecutionFailed(format!(
                "Image from URL too large: {size} bytes (max {MAX_IMAGE_SIZE} bytes)",
            )));
        }

        let engine = base64::engine::general_purpose::STANDARD;
        let base64_data = engine.encode(&bytes);

        Ok((base64_data, mime_type, size))
    }
}

#[async_trait]
impl Tool for AnalyzeImageTool {
    fn name(&self) -> &str {
        "AnalyzeImage"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Absolute path to the image file to analyze. Either file_path or url must be provided."
                },
                "url": {
                    "type": "string",
                    "description": "URL of the image to analyze. Either file_path or url must be provided."
                },
                "prompt": {
                    "type": "string",
                    "description": "What to analyze or describe about the image"
                }
            },
            "required": ["prompt"]
        })
    }

    async fn execute(&self, input: serde_json::Value) -> ToolResult<ToolOutput> {
        let analyze_input: AnalyzeImageInput = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("Invalid analyze_image input: {e}")))?;

        // Validate that at least one source is provided
        match (&analyze_input.file_path, &analyze_input.url) {
            (None, None) => {
                return Err(ToolError::InvalidInput(
                    "Either file_path or url must be provided".to_string(),
                ));
            }
            (Some(_), Some(_)) => {
                return Err(ToolError::InvalidInput(
                    "Provide either file_path or url, not both".to_string(),
                ));
            }
            _ => {}
        }

        // Validate prompt is not empty
        if analyze_input.prompt.trim().is_empty() {
            return Err(ToolError::InvalidInput(
                "Prompt must not be empty".to_string(),
            ));
        }

        let (base64_data, media_type, size) =
            if let Some(ref file_path) = analyze_input.file_path {
                Self::load_from_file(file_path).await?
            } else if let Some(ref url) = analyze_input.url {
                Self::load_from_url(url).await?
            } else {
                unreachable!()
            };

        let source = analyze_input
            .file_path
            .as_deref()
            .or(analyze_input.url.as_deref())
            .unwrap_or("unknown");

        let output = ImageAnalysisOutput {
            output_type: "image".to_string(),
            media_type: media_type.to_string(),
            data: base64_data,
            source: source.to_string(),
            size,
            prompt: analyze_input.prompt.clone(),
        };

        let json_output = serde_json::to_string_pretty(&output)
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to serialize image data: {e}")))?;

        Ok(ToolOutput {
            content: json_output,
            is_error: false,
            metadata: {
                let mut map = HashMap::new();
                map.insert("type".to_string(), json!("image"));
                map.insert("media_type".to_string(), json!(media_type));
                map.insert("size".to_string(), json!(size));
                map.insert("source".to_string(), json!(source));
                map.insert(
                    "prompt".to_string(),
                    json!(analyze_input.prompt),
                );
                map
            },
        })
    }

    fn is_read_only(&self) -> bool {
        true
    }

    fn category(&self) -> &str {
        "multimodal"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── MIME type detection ──────────────────────────────────────────

    #[test]
    fn test_mime_type_from_ext_png() {
        assert_eq!(AnalyzeImageTool::mime_type_from_ext("png"), "image/png");
        assert_eq!(AnalyzeImageTool::mime_type_from_ext("PNG"), "image/png");
    }

    #[test]
    fn test_mime_type_from_ext_jpeg() {
        assert_eq!(AnalyzeImageTool::mime_type_from_ext("jpg"), "image/jpeg");
        assert_eq!(AnalyzeImageTool::mime_type_from_ext("jpeg"), "image/jpeg");
        assert_eq!(AnalyzeImageTool::mime_type_from_ext("JPG"), "image/jpeg");
    }

    #[test]
    fn test_mime_type_from_ext_all_formats() {
        assert_eq!(AnalyzeImageTool::mime_type_from_ext("gif"), "image/gif");
        assert_eq!(AnalyzeImageTool::mime_type_from_ext("webp"), "image/webp");
        assert_eq!(AnalyzeImageTool::mime_type_from_ext("bmp"), "image/bmp");
        assert_eq!(AnalyzeImageTool::mime_type_from_ext("ico"), "image/x-icon");
        assert_eq!(AnalyzeImageTool::mime_type_from_ext("tiff"), "image/tiff");
        assert_eq!(AnalyzeImageTool::mime_type_from_ext("tif"), "image/tiff");
        assert_eq!(AnalyzeImageTool::mime_type_from_ext("svg"), "image/svg+xml");
    }

    #[test]
    fn test_mime_type_from_ext_unknown() {
        assert_eq!(
            AnalyzeImageTool::mime_type_from_ext("txt"),
            "application/octet-stream"
        );
        assert_eq!(
            AnalyzeImageTool::mime_type_from_ext(""),
            "application/octet-stream"
        );
        assert_eq!(
            AnalyzeImageTool::mime_type_from_ext("exe"),
            "application/octet-stream"
        );
    }

    #[test]
    fn test_mime_type_from_path() {
        assert_eq!(
            AnalyzeImageTool::mime_type_from_path("/tmp/photo.png"),
            "image/png"
        );
        assert_eq!(
            AnalyzeImageTool::mime_type_from_path("photo.JPG"),
            "image/jpeg"
        );
        assert_eq!(
            AnalyzeImageTool::mime_type_from_path("/path/to/image.webp"),
            "image/webp"
        );
    }

    #[test]
    fn test_mime_type_from_url() {
        assert_eq!(
            AnalyzeImageTool::mime_type_from_url("https://example.com/img.png"),
            "image/png"
        );
        assert_eq!(
            AnalyzeImageTool::mime_type_from_url("https://example.com/img.jpg?w=100"),
            "image/jpeg"
        );
        assert_eq!(
            AnalyzeImageTool::mime_type_from_url("https://example.com/path/image.webp#anchor"),
            "image/webp"
        );
    }

    // ── Image extension check ────────────────────────────────────────

    #[test]
    fn test_is_image_extension_valid() {
        assert!(AnalyzeImageTool::is_image_extension("photo.png"));
        assert!(AnalyzeImageTool::is_image_extension("photo.jpg"));
        assert!(AnalyzeImageTool::is_image_extension("photo.jpeg"));
        assert!(AnalyzeImageTool::is_image_extension("photo.gif"));
        assert!(AnalyzeImageTool::is_image_extension("photo.webp"));
        assert!(AnalyzeImageTool::is_image_extension("photo.bmp"));
        assert!(AnalyzeImageTool::is_image_extension("photo.ico"));
        assert!(AnalyzeImageTool::is_image_extension("photo.tiff"));
        assert!(AnalyzeImageTool::is_image_extension("photo.tif"));
    }

    #[test]
    fn test_is_image_extension_case_insensitive() {
        assert!(AnalyzeImageTool::is_image_extension("photo.PNG"));
        assert!(AnalyzeImageTool::is_image_extension("photo.JPEG"));
        assert!(AnalyzeImageTool::is_image_extension("photo.WebP"));
    }

    #[test]
    fn test_is_image_extension_invalid() {
        assert!(!AnalyzeImageTool::is_image_extension("document.txt"));
        assert!(!AnalyzeImageTool::is_image_extension("script.rs"));
        assert!(!AnalyzeImageTool::is_image_extension("no_extension"));
        assert!(!AnalyzeImageTool::is_image_extension(""));
    }

    // ── Tool trait ───────────────────────────────────────────────────

    #[test]
    fn test_tool_name() {
        let tool = AnalyzeImageTool::new();
        assert_eq!(tool.name(), "AnalyzeImage");
    }

    #[test]
    fn test_tool_description() {
        let tool = AnalyzeImageTool::new();
        assert!(tool.description().contains("image"));
    }

    #[test]
    fn test_tool_schema() {
        let tool = AnalyzeImageTool::new();
        let schema = tool.input_schema();
        assert!(schema["properties"]["file_path"].is_object());
        assert!(schema["properties"]["url"].is_object());
        assert!(schema["properties"]["prompt"].is_object());
        assert!(schema["required"].as_array().unwrap().contains(&json!("prompt")));
    }

    #[test]
    fn test_tool_is_read_only() {
        let tool = AnalyzeImageTool::new();
        assert!(tool.is_read_only());
    }

    #[test]
    fn test_tool_category() {
        let tool = AnalyzeImageTool::new();
        assert_eq!(tool.category(), "multimodal");
    }

    #[test]
    fn test_tool_default() {
        let tool = AnalyzeImageTool::default();
        assert_eq!(tool.name(), "AnalyzeImage");
    }

    // ── Input validation ─────────────────────────────────────────────

    #[tokio::test]
    async fn test_execute_no_source_returns_error() {
        let tool = AnalyzeImageTool::new();
        let result = tool
            .execute(json!({
                "prompt": "describe this image"
            }))
            .await;
        assert!(result.is_err());
        match result.unwrap_err() {
            ToolError::InvalidInput(msg) => {
                assert!(msg.contains("file_path") || msg.contains("url"));
            }
            other => panic!("Expected InvalidInput, got: {other}"),
        }
    }

    #[tokio::test]
    async fn test_execute_both_sources_returns_error() {
        let tool = AnalyzeImageTool::new();
        let result = tool
            .execute(json!({
                "file_path": "/tmp/test.png",
                "url": "https://example.com/img.png",
                "prompt": "describe"
            }))
            .await;
        assert!(result.is_err());
        match result.unwrap_err() {
            ToolError::InvalidInput(msg) => {
                assert!(msg.contains("either") || msg.contains("both"));
            }
            other => panic!("Expected InvalidInput, got: {other}"),
        }
    }

    #[tokio::test]
    async fn test_execute_empty_prompt_returns_error() {
        let tool = AnalyzeImageTool::new();
        let result = tool
            .execute(json!({
                "file_path": "/tmp/test.png",
                "prompt": "   "
            }))
            .await;
        assert!(result.is_err());
        match result.unwrap_err() {
            ToolError::InvalidInput(msg) => {
                assert!(
                    msg.to_lowercase().contains("prompt"),
                    "Expected 'prompt' in error message, got: {msg}"
                );
            }
            other => panic!("Expected InvalidInput, got: {other}"),
        }
    }

    #[tokio::test]
    async fn test_execute_nonexistent_file_returns_error() {
        let tool = AnalyzeImageTool::new();
        let result = tool
            .execute(json!({
                "file_path": "/nonexistent/path/image.png",
                "prompt": "describe"
            }))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_non_image_file_returns_error() {
        let tool = AnalyzeImageTool::new();
        let result = tool
            .execute(json!({
                "file_path": "/tmp/test.txt",
                "prompt": "describe"
            }))
            .await;
        assert!(result.is_err());
        match result.unwrap_err() {
            ToolError::InvalidInput(msg) => {
                assert!(msg.contains("image"));
            }
            other => panic!("Expected InvalidInput, got: {other}"),
        }
    }

    #[tokio::test]
    async fn test_execute_invalid_url_scheme_returns_error() {
        let tool = AnalyzeImageTool::new();
        let result = tool
            .execute(json!({
                "url": "ftp://example.com/img.png",
                "prompt": "describe"
            }))
            .await;
        assert!(result.is_err());
        match result.unwrap_err() {
            ToolError::InvalidInput(msg) => {
                assert!(msg.contains("http"));
            }
            other => panic!("Expected InvalidInput, got: {other}"),
        }
    }

    // ── Load from file (integration with temp files) ─────────────────

    #[tokio::test]
    async fn test_load_from_file_small_png() {
        // Create a minimal PNG file (1x1 transparent pixel)
        // Minimal valid PNG: 8-byte signature + IHDR + IDAT + IEND
        let png_bytes: Vec<u8> = vec![
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG signature
            0x00, 0x00, 0x00, 0x0D, // IHDR length
            0x49, 0x48, 0x44, 0x52, // "IHDR"
            0x00, 0x00, 0x00, 0x01, // width: 1
            0x00, 0x00, 0x00, 0x01, // height: 1
            0x08, 0x06, // bit depth: 8, color type: RGBA
            0x00, 0x00, 0x00, // compression, filter, interlace
            0x1F, 0x15, 0xC4, 0x89, // CRC
            0x00, 0x00, 0x00, 0x0A, // IDAT length
            0x49, 0x44, 0x41, 0x54, // "IDAT"
            0x78, 0x9C, 0x62, 0x00, 0x00, 0x00, 0x02, 0x00, 0x01, // compressed data
            0xE5, 0x27, 0xDE, 0xFC, // CRC
            0x00, 0x00, 0x00, 0x00, // IEND length
            0x49, 0x45, 0x4E, 0x44, // "IEND"
            0xAE, 0x42, 0x60, 0x82, // CRC
        ];

        let dir = tempfile::TempDir::new().expect("create temp dir");
        let file_path = dir.path().join("test.png");
        tokio::fs::write(&file_path, &png_bytes)
            .await
            .expect("write test png");

        let path_str = file_path.to_string_lossy().to_string();
        let (base64_data, mime_type, size) =
            AnalyzeImageTool::load_from_file(&path_str).await.expect("load from file");

        assert_eq!(mime_type, "image/png");
        assert_eq!(size, png_bytes.len() as u64);
        assert!(!base64_data.is_empty());

        // Verify the base64 data decodes back to the original
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(&base64_data)
            .expect("decode base64");
        assert_eq!(decoded, png_bytes);
    }

    #[tokio::test]
    async fn test_execute_with_valid_file() {
        // Create a minimal PNG file
        let png_bytes: Vec<u8> = vec![
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A,
            0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
            0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
            0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
            0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41,
            0x54, 0x78, 0x9C, 0x62, 0x00, 0x00, 0x00, 0x02,
            0x00, 0x01, 0xE5, 0x27, 0xDE, 0xFC, 0x00, 0x00,
            0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42,
            0x60, 0x82,
        ];

        let dir = tempfile::TempDir::new().expect("create temp dir");
        let file_path = dir.path().join("test.png");
        tokio::fs::write(&file_path, &png_bytes)
            .await
            .expect("write test png");

        let tool = AnalyzeImageTool::new();
        let result = tool
            .execute(json!({
                "file_path": file_path.to_string_lossy().to_string(),
                "prompt": "Describe this image"
            }))
            .await
            .expect("execute should succeed");

        assert!(!result.is_error);
        assert_eq!(result.metadata.get("type"), Some(&json!("image")));
        assert_eq!(result.metadata.get("media_type"), Some(&json!("image/png")));

        // Verify the content is valid JSON with image data
        let content: serde_json::Value =
            serde_json::from_str(&result.content).expect("content should be valid JSON");
        assert_eq!(content["type"], "image");
        assert_eq!(content["media_type"], "image/png");
        assert!(!content["data"].as_str().unwrap().is_empty());
    }

    // ── Thread safety ────────────────────────────────────────────────

    #[test]
    fn test_tool_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<AnalyzeImageTool>();
    }
}
