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

/// Maximum number of images accepted in one `AnalyzeImages` batch (C-ImgBatch).
///
/// Caps a single tool call so one model turn cannot balloon the request body
/// (and the follow-up vision turn) without bound. Batches larger than this
/// are rejected with `InvalidInput` so the model can split them.
pub const MAX_BATCH_IMAGES: usize = 20;

/// Input parameters for the `AnalyzeImages` batch tool (C-ImgBatch).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AnalyzeImagesInput {
    /// Absolute paths to the image files to analyze (1..=[`MAX_BATCH_IMAGES`]).
    #[serde(default)]
    pub paths: Vec<String>,

    /// What to analyze or describe about each image. `question` is accepted
    /// as an alias for call-site compatibility.
    #[serde(default, alias = "question")]
    pub prompt: Option<String>,
}

/// Per-image entry in the `AnalyzeImages` output payload.
#[derive(Debug, Serialize)]
struct BatchImageEntry {
    /// 1-based position of this image within the batch.
    index: usize,
    /// Source file path.
    source: String,
    /// MIME type (e.g. "image/png").
    media_type: String,
    /// File size in bytes.
    size: u64,
    /// Base64-encoded image data.
    data: String,
}

/// Output payload for `AnalyzeImages` (C-ImgBatch).
///
/// The query engine detects `metadata["type"] == "images"` and expands the
/// `images` array into interleaved `## <path>` text headings and
/// `ContentBlock::Image` blocks inside a **single** tool_result, so the whole
/// batch reaches the LLM as one vision request instead of one request per
/// image. `report` carries the same per-image sectioning as plain text for
/// adapters that flatten tool_result images (see the tool's doc comment).
#[derive(Debug, Serialize)]
struct BatchImageAnalysisOutput {
    /// Type identifier for downstream detection ("images").
    #[serde(rename = "type")]
    output_type: String,

    /// Number of images in the batch.
    count: usize,

    /// The user's analysis prompt (applied to every image).
    prompt: String,

    /// Per-image sectioned manifest (`## <path>` per line) in batch order.
    report: String,

    /// The loaded images, in batch order.
    images: Vec<BatchImageEntry>,
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
    /// Filesystem world backing local image loads (§4.11).
    fs: std::sync::Arc<dyn shannon_tool_interface::FileSystemProvider>,
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
            fs: crate::defaults::fs(),
        }
    }

    /// Inject a filesystem world override (sandbox/remote assemblies).
    pub fn with_fs(
        mut self,
        fs: std::sync::Arc<dyn shannon_tool_interface::FileSystemProvider>,
    ) -> Self {
        self.fs = fs;
        self
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
    ///
    /// Legacy associated-function shape retained for existing tests; routes
    /// through the local filesystem world by default.
    #[cfg_attr(not(test), allow(dead_code))]
    async fn load_from_file(file_path: &str) -> Result<(String, &'static str, u64), ToolError> {
        Self::load_from_file_with(crate::defaults::fs().as_ref(), file_path).await
    }

    /// Provider-injected variant (§4.11): metadata and byte reads flow
    /// through the injected filesystem world.
    async fn load_from_file_with(
        fs: &dyn shannon_tool_interface::FileSystemProvider,
        file_path: &str,
    ) -> Result<(String, &'static str, u64), ToolError> {
        // Validate extension
        if !Self::is_image_extension(file_path) {
            return Err(ToolError::InvalidInput(format!(
                "File does not appear to be an image: {file_path}. \
                 Supported formats: {}",
                IMAGE_EXTENSIONS.join(", ")
            )));
        }

        let metadata = fs
            .metadata(std::path::Path::new(file_path))
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to stat file: {e}")))?;

        let size = metadata.len;
        if size > MAX_IMAGE_SIZE {
            return Err(ToolError::ExecutionFailed(format!(
                "Image file too large: {size} bytes (max {MAX_IMAGE_SIZE} bytes)",
            )));
        }

        let bytes = fs
            .read_bytes(std::path::Path::new(file_path))
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to read image file: {e}")))?;

        let engine = base64::engine::general_purpose::STANDARD;
        let base64_data = engine.encode(&bytes);
        let mime_type = Self::mime_type_from_path(file_path);

        Ok((base64_data, mime_type, size))
    }

    /// Load an image from a URL, returning (base64_data, mime_type, size).
    async fn load_from_url(url: &str) -> Result<(String, &'static str, u64), ToolError> {
        // Validate URL scheme
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err(ToolError::InvalidInput(
                "URL must use http:// or https:// scheme".to_string(),
            ));
        }

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| {
                ToolError::ExecutionFailed(format!("Failed to create HTTP client: {e}"))
            })?;

        let response =
            client.get(url).send().await.map_err(|e| {
                ToolError::ExecutionFailed(format!("Failed to fetch image URL: {e}"))
            })?;

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

        let bytes = response.bytes().await.map_err(|e| {
            ToolError::ExecutionFailed(format!("Failed to read image response: {e}"))
        })?;

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

        let (base64_data, media_type, size) = if let Some(ref file_path) = analyze_input.file_path {
            Self::load_from_file_with(self.fs.as_ref(), file_path).await?
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

        let json_output = serde_json::to_string_pretty(&output).map_err(|e| {
            ToolError::ExecutionFailed(format!("Failed to serialize image data: {e}"))
        })?;

        Ok(ToolOutput {
            content: json_output,
            is_error: false,
            metadata: {
                let mut map = HashMap::new();
                map.insert("type".to_string(), json!("image"));
                map.insert("media_type".to_string(), json!(media_type));
                map.insert("size".to_string(), json!(size));
                map.insert("source".to_string(), json!(source));
                map.insert("prompt".to_string(), json!(analyze_input.prompt));
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

/// AnalyzeImages tool: batch variant of [`AnalyzeImageTool`] (C-ImgBatch).
///
/// # Motivation
///
/// Image-heavy evaluation tasks (gcode-to-text, extract-moves-from-video)
/// made the model call `AnalyzeImage` once per image — one LLM vision turn
/// per image, compounding to 300k+ tokens per task. This tool accepts up to
/// [`MAX_BATCH_IMAGES`] paths in a **single** tool call.
///
/// # How the single-request path works
///
/// The tool never calls the LLM itself; it loads every image and returns a
/// payload tagged `metadata["type"] = "images"` with an `images[]` array.
/// The query engine (`ToolResultEntry::to_tool_result_content` in
/// shannon-core) expands that array into interleaved `## <path>` text
/// headings and `ContentBlock::Image` blocks inside **one** `tool_result`
/// content array — the Anthropic wire format passes those through verbatim,
/// so one tool call becomes exactly one vision request containing all N
/// images (previously: N tool calls and N vision turns).
///
/// # Known limitation (degraded providers)
///
/// The OpenAI/Ollama/Gemini adapters flatten `tool_result` content to text
/// (`convert_message_for_openai` / `serialize_gemini_request`), dropping
/// image blocks — true for the pre-existing single-image path as well. On
/// those providers the model receives the sectioned text manifest (`report`)
/// without the image pixels; the tool output therefore labels the request
/// path so the degradation is visible. Follow-ups: (1) teach the
/// OpenAI/Gemini adapters to emit multi-image tool results, (2) per-batch
/// pixel budget / downscaling to bound request size.
pub struct AnalyzeImagesTool {
    description: String,
    /// Filesystem world backing local image loads (§4.11).
    fs: std::sync::Arc<dyn shannon_tool_interface::FileSystemProvider>,
}

impl Default for AnalyzeImagesTool {
    fn default() -> Self {
        Self::new()
    }
}

impl AnalyzeImagesTool {
    pub fn new() -> Self {
        Self {
            description: "Analyze multiple image files in ONE call (batch). Each image is sent to the LLM as part of a single vision request, with results organized under a '## <path>' heading per image. Accepts up to 20 paths; use this instead of calling AnalyzeImage repeatedly. Supports PNG, JPEG, GIF, WebP, BMP, ICO, and TIFF formats.".to_string(),
            fs: crate::defaults::fs(),
        }
    }

    /// Inject a filesystem world override (sandbox/remote assemblies).
    pub fn with_fs(
        mut self,
        fs: std::sync::Arc<dyn shannon_tool_interface::FileSystemProvider>,
    ) -> Self {
        self.fs = fs;
        self
    }

    /// Build the per-image sectioned manifest (`## <path>` per image).
    fn build_report(entries: &[BatchImageEntry]) -> String {
        entries
            .iter()
            .map(|e| {
                format!(
                    "## {source}\n(image {index} of {count}, {media_type})",
                    source = e.source,
                    index = e.index,
                    count = entries.len(),
                    media_type = e.media_type,
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}

#[async_trait]
impl Tool for AnalyzeImagesTool {
    fn name(&self) -> &str {
        "AnalyzeImages"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "paths": {
                    "type": "array",
                    "items": { "type": "string" },
                    "minItems": 1,
                    "maxItems": MAX_BATCH_IMAGES,
                    "description": "Absolute paths to the image files to analyze, in order. Results are sectioned per image under a '## <path>' heading."
                },
                "prompt": {
                    "type": "string",
                    "description": "What to analyze or describe about each image (applied to every image in the batch)"
                },
                "question": {
                    "type": "string",
                    "description": "Alias for prompt"
                }
            },
            "required": ["paths", "prompt"]
        })
    }

    async fn execute(&self, input: serde_json::Value) -> ToolResult<ToolOutput> {
        let analyze_input: AnalyzeImagesInput = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("Invalid analyze_images input: {e}")))?;

        // Validate batch size BEFORE touching the filesystem so oversized
        // batches fail fast and cheaply (C-ImgBatch avalanche guard).
        if analyze_input.paths.is_empty() {
            return Err(ToolError::InvalidInput(
                "paths must contain at least one image path".to_string(),
            ));
        }
        if analyze_input.paths.len() > MAX_BATCH_IMAGES {
            return Err(ToolError::InvalidInput(format!(
                "Too many images: {} (max {MAX_BATCH_IMAGES}). Split the batch into smaller AnalyzeImages calls.",
                analyze_input.paths.len()
            )));
        }

        // Validate prompt is not empty (accept `question` via serde alias).
        let prompt = analyze_input
            .prompt
            .as_deref()
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .ok_or_else(|| ToolError::InvalidInput("prompt must not be empty".to_string()))?;

        // Load every image up-front; fail fast on the first bad path so the
        // model can correct it instead of receiving a partial batch.
        let mut entries = Vec::with_capacity(analyze_input.paths.len());
        for (i, path) in analyze_input.paths.iter().enumerate() {
            let (base64_data, media_type, size) =
                AnalyzeImageTool::load_from_file_with(self.fs.as_ref(), path)
                    .await
                    .map_err(|e| {
                        ToolError::ExecutionFailed(format!(
                            "Failed to load image {i} of {}: {path}: {e}",
                            analyze_input.paths.len()
                        ))
                    })?;
            entries.push(BatchImageEntry {
                index: i + 1,
                source: path.clone(),
                media_type: media_type.to_string(),
                size,
                data: base64_data,
            });
        }

        let report = Self::build_report(&entries);
        let count = entries.len();

        let output = BatchImageAnalysisOutput {
            output_type: "images".to_string(),
            count,
            prompt: prompt.to_string(),
            report,
            images: entries,
        };

        let json_output = serde_json::to_string_pretty(&output).map_err(|e| {
            ToolError::ExecutionFailed(format!("Failed to serialize batch image data: {e}"))
        })?;

        let sources: Vec<serde_json::Value> =
            analyze_input.paths.iter().map(|p| json!(p)).collect();

        Ok(ToolOutput {
            content: json_output,
            is_error: false,
            metadata: {
                let mut map = HashMap::new();
                // "images" (plural) marks the batch payload the engine expands
                // into multiple image blocks within a single tool_result.
                map.insert("type".to_string(), json!("images"));
                map.insert("count".to_string(), json!(count));
                map.insert("sources".to_string(), json!(sources));
                map.insert("prompt".to_string(), json!(prompt));
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
#[allow(clippy::unwrap_used)]
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
        assert!(
            schema["required"]
                .as_array()
                .unwrap()
                .contains(&json!("prompt"))
        );
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
        let (base64_data, mime_type, size) = AnalyzeImageTool::load_from_file(&path_str)
            .await
            .expect("load from file");

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
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9C, 0x62, 0x00, 0x00, 0x00, 0x02, 0x00, 0x01, 0xE5, 0x27, 0xDE, 0xFC, 0x00, 0x00,
            0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
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

    // ── AnalyzeImages batch tool (C-ImgBatch) ────────────────────────

    /// Minimal valid 1x1 PNG (signature + IHDR + IDAT + IEND).
    fn minimal_png() -> Vec<u8> {
        vec![
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9C, 0x62, 0x00, 0x00, 0x00, 0x02, 0x00, 0x01, 0xE5, 0x27, 0xDE, 0xFC, 0x00, 0x00,
            0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
        ]
    }

    #[test]
    fn test_analyze_images_tool_defaults() {
        let tool = AnalyzeImagesTool::new();
        assert_eq!(tool.name(), "AnalyzeImages");
        assert!(tool.description().contains("ONE call"));
        assert!(tool.is_read_only());
        assert_eq!(tool.category(), "multimodal");
        assert_eq!(AnalyzeImagesTool::default().name(), "AnalyzeImages");
    }

    #[test]
    fn test_analyze_images_schema() {
        let tool = AnalyzeImagesTool::new();
        let schema = tool.input_schema();
        assert_eq!(schema["properties"]["paths"]["type"], "array");
        assert_eq!(schema["properties"]["paths"]["items"]["type"], "string");
        assert_eq!(schema["properties"]["paths"]["maxItems"], MAX_BATCH_IMAGES);
        assert!(schema["properties"]["prompt"].is_object());
        assert!(schema["properties"]["question"].is_object());
        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&json!("paths")));
        assert!(required.contains(&json!("prompt")));
    }

    #[test]
    fn test_analyze_images_input_parses_paths_and_question_alias() {
        // Canonical `prompt` field
        let input: AnalyzeImagesInput =
            serde_json::from_value(json!({"paths": ["/a.png"], "prompt": "describe"}))
                .expect("parse prompt form");
        assert_eq!(input.paths, vec!["/a.png"]);
        assert_eq!(input.prompt.as_deref(), Some("describe"));

        // `question` alias (task spec shape: { paths, question? })
        let input: AnalyzeImagesInput =
            serde_json::from_value(json!({"paths": ["/a.png"], "question": "what is this?"}))
                .expect("parse question alias");
        assert_eq!(input.prompt.as_deref(), Some("what is this?"));

        // Missing prompt parses (validated at execute time)
        let input: AnalyzeImagesInput =
            serde_json::from_value(json!({"paths": ["/a.png"]})).expect("parse no prompt");
        assert!(input.prompt.is_none());
    }

    #[tokio::test]
    async fn test_analyze_images_over_limit_rejected() {
        let tool = AnalyzeImagesTool::new();
        let paths: Vec<String> = (0..=MAX_BATCH_IMAGES)
            .map(|i| format!("/tmp/img{i}.png"))
            .collect();
        assert_eq!(paths.len(), MAX_BATCH_IMAGES + 1);
        let result = tool
            .execute(json!({ "paths": paths, "prompt": "describe" }))
            .await;
        match result {
            Err(ToolError::InvalidInput(msg)) => {
                assert!(
                    msg.contains("Too many images") && msg.contains("20"),
                    "Expected over-limit message, got: {msg}"
                );
            }
            other => panic!("Expected InvalidInput over-limit, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_analyze_images_empty_paths_rejected() {
        let tool = AnalyzeImagesTool::new();
        let result = tool
            .execute(json!({ "paths": [], "prompt": "describe" }))
            .await;
        match result {
            Err(ToolError::InvalidInput(msg)) => {
                assert!(msg.contains("at least one"), "got: {msg}");
            }
            other => panic!("Expected InvalidInput empty paths, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_analyze_images_empty_prompt_rejected() {
        let tool = AnalyzeImagesTool::new();
        let result = tool
            .execute(json!({ "paths": ["/tmp/a.png"], "prompt": "   " }))
            .await;
        match result {
            Err(ToolError::InvalidInput(msg)) => {
                assert!(msg.contains("prompt"), "got: {msg}");
            }
            other => panic!("Expected InvalidInput empty prompt, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_analyze_images_batch_output_is_sectioned_per_image() {
        let png = minimal_png();
        let dir = tempfile::TempDir::new().expect("create temp dir");
        let p1 = dir.path().join("img1.png");
        let p2 = dir.path().join("img2.png");
        tokio::fs::write(&p1, &png).await.expect("write img1");
        tokio::fs::write(&p2, &png).await.expect("write img2");
        let path1 = p1.to_string_lossy().to_string();
        let path2 = p2.to_string_lossy().to_string();

        let tool = AnalyzeImagesTool::new();
        let result = tool
            .execute(json!({
                "paths": [path1, path2],
                "prompt": "Describe each image"
            }))
            .await
            .expect("batch execute should succeed");

        assert!(!result.is_error);
        assert_eq!(result.metadata.get("type"), Some(&json!("images")));
        assert_eq!(result.metadata.get("count"), Some(&json!(2)));
        assert_eq!(result.metadata.get("sources"), Some(&json!([path1, path2])));

        let content: serde_json::Value =
            serde_json::from_str(&result.content).expect("content should be valid JSON");
        assert_eq!(content["type"], "images");
        assert_eq!(content["count"], 2);
        assert_eq!(content["prompt"], "Describe each image");

        // Per-image sectioning: a `## <path>` heading for every image.
        let report = content["report"].as_str().unwrap();
        assert!(report.contains(&format!("## {path1}")), "report: {report}");
        assert!(report.contains(&format!("## {path2}")), "report: {report}");

        // Images array preserves batch order and decodable data.
        let images = content["images"].as_array().unwrap();
        assert_eq!(images.len(), 2);
        assert_eq!(images[0]["source"], path1.as_str());
        assert_eq!(images[1]["source"], path2.as_str());
        assert_eq!(images[0]["media_type"], "image/png");
        assert_eq!(images[0]["index"], 1);
        assert_eq!(images[1]["index"], 2);
        let engine = base64::engine::general_purpose::STANDARD;
        for img in images {
            let decoded = engine
                .decode(img["data"].as_str().unwrap())
                .expect("decode base64");
            assert_eq!(decoded, png);
        }
    }

    #[tokio::test]
    async fn test_analyze_images_missing_file_fails_with_path_context() {
        let dir = tempfile::TempDir::new().expect("create temp dir");
        let good = dir.path().join("good.png");
        tokio::fs::write(&good, minimal_png()).await.expect("write");
        let bad = dir.path().join("missing.png");

        let tool = AnalyzeImagesTool::new();
        let result = tool
            .execute(json!({
                "paths": [good.to_string_lossy(), bad.to_string_lossy()],
                "prompt": "describe"
            }))
            .await;
        match result {
            Err(ToolError::ExecutionFailed(msg)) => {
                assert!(
                    msg.contains(bad.to_string_lossy().as_ref()),
                    "Expected failing path in error, got: {msg}"
                );
            }
            other => panic!("Expected ExecutionFailed, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_analyze_images_non_image_file_rejected() {
        let dir = tempfile::TempDir::new().expect("create temp dir");
        let txt = dir.path().join("notes.txt");
        tokio::fs::write(&txt, b"not an image")
            .await
            .expect("write");

        let tool = AnalyzeImagesTool::new();
        let result = tool
            .execute(json!({ "paths": [txt.to_string_lossy()], "prompt": "describe" }))
            .await;
        assert!(result.is_err());
    }

    #[test]
    fn test_analyze_images_tool_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<AnalyzeImagesTool>();
    }
}
