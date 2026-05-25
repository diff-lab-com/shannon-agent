//! Read tool implementation

use crate::{ToolError, ToolOutput};
use serde::{Deserialize, Serialize};
use serde_json::json;
use shannon_core::progressive_loader::{ProgressiveLoaderConfig, truncate_content};
use std::collections::HashMap;

/// Image file extensions we support
const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg"];

/// Magic byte signatures for image detection
const PNG_MAGIC: &[u8] = &[0x89, 0x50, 0x4E, 0x47];
const JPEG_MAGIC: &[u8] = &[0xFF, 0xD8, 0xFF];
const GIF_MAGIC: &[u8] = &[0x47, 0x49, 0x46];
const WEBP_MAGIC: &[u8] = &[0x52, 0x49, 0x46, 0x46];

/// Image output format for binary image files
#[derive(Debug, Serialize)]
struct ImageOutput {
    /// Type identifier
    #[serde(rename = "type")]
    output_type: String,

    /// Media type (e.g., "image/png")
    media_type: String,

    /// Base64 encoded image data
    data: String,

    /// Original file path
    path: String,

    /// File size in bytes
    size: u64,
}

/// Check if a file path appears to be an image based on extension
fn is_image_path(path: &str) -> bool {
    path.to_ascii_lowercase()
        .rsplit('.')
        .next()
        .map(|ext| IMAGE_EXTENSIONS.contains(&ext))
        .unwrap_or(false)
}

/// Detect media type from file extension
fn detect_media_type(path: &str) -> &'static str {
    let path_lower = path.to_ascii_lowercase();
    let ext = path_lower.rsplit('.').next().unwrap_or("");
    match ext {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        _ => "application/octet-stream",
    }
}

/// Check if bytes match a magic byte prefix
fn has_magic_prefix(data: &[u8], magic: &[u8]) -> bool {
    data.len() >= magic.len() && &data[..magic.len()] == magic
}

/// Detect if binary data is an image using magic bytes
fn is_image_by_magic_bytes(data: &[u8]) -> bool {
    has_magic_prefix(data, PNG_MAGIC)
        || has_magic_prefix(data, JPEG_MAGIC)
        || has_magic_prefix(data, GIF_MAGIC)
        || has_magic_prefix(data, WEBP_MAGIC)
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReadInput {
    /// Absolute path to the file
    pub file_path: String,

    /// Optional line offset for reading specific ranges
    pub offset: Option<usize>,

    /// Optional line limit
    pub limit: Option<usize>,

    /// Whether to truncate large files automatically.
    ///
    /// When `true` (default), files exceeding [`ProgressiveLoaderConfig::max_read_lines`]
    /// are summarised to a head/tail preview with an omission notice.
    /// When `false`, the full content is returned (subject to the file-size limit).
    #[serde(default = "default_true")]
    pub truncate_large_files: bool,
}

fn default_true() -> bool {
    true
}

impl Default for ReadInput {
    fn default() -> Self {
        Self {
            file_path: String::new(),
            offset: None,
            limit: None,
            truncate_large_files: true,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ReadOutput {
    /// File contents as string (for text files)
    pub content: String,

    /// Number of lines read (for text files)
    pub lines: usize,

    /// File path
    pub file_path: String,
}

pub async fn execute(input: ReadInput) -> Result<ToolOutput, ToolError> {
    use tokio::fs;

    // Check file size before reading to prevent memory exhaustion
    const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024; // 10 MB
    let metadata = fs::metadata(&input.file_path)
        .await
        .map_err(|e| ToolError::ExecutionFailed(format!("Failed to stat file: {e}")))?;
    if metadata.len() > MAX_FILE_SIZE {
        return Err(ToolError::ExecutionFailed(format!(
            "File too large: {} bytes (max {} bytes). Use offset/limit to read portions.",
            metadata.len(),
            MAX_FILE_SIZE
        )));
    }

    // Check if file appears to be an image
    if is_image_path(&input.file_path) {
        // Read as binary
        let bytes = fs::read(&input.file_path)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to read image file: {e}")))?;

        // Verify using magic bytes as fallback
        if !is_image_by_magic_bytes(&bytes) {
            // Might be a false positive (text file with .png extension), fall through to text handling
        } else {
            // Encode to base64
            use base64::Engine;
            let engine = base64::engine::general_purpose::STANDARD;
            let base64_data = engine.encode(&bytes);

            let media_type = detect_media_type(&input.file_path);
            let size = bytes.len() as u64;

            let image_output = ImageOutput {
                output_type: "image".to_string(),
                media_type: media_type.to_string(),
                data: base64_data,
                path: input.file_path.clone(),
                size,
            };

            let json_output = serde_json::to_string_pretty(&image_output).map_err(|e| {
                ToolError::ExecutionFailed(format!("Failed to serialize image data: {e}"))
            })?;

            return Ok(ToolOutput {
                content: json_output,
                is_error: false,
                metadata: {
                    let mut map = HashMap::new();
                    map.insert("type".to_string(), json!("image"));
                    map.insert("media_type".to_string(), json!(media_type));
                    map.insert("size".to_string(), json!(size));
                    map.insert("file_path".to_string(), json!(input.file_path));
                    map
                },
            });
        }
    }

    // Original text file handling
    let content = fs::read_to_string(&input.file_path)
        .await
        .map_err(|e| ToolError::ExecutionFailed(format!("Failed to read file: {e}")))?;

    let lines: Vec<&str> = content.lines().collect();
    let total_lines = lines.len();

    let (start, end) = match (input.offset, input.limit) {
        (Some(offset), Some(limit)) => {
            let start = offset.min(total_lines);
            let end = (offset + limit).min(total_lines);
            (start, end)
        }
        (Some(offset), None) => (offset.min(total_lines), total_lines),
        (None, Some(limit)) => (0, limit.min(total_lines)),
        (None, None) => (0, total_lines),
    };

    let selected_lines = lines[start..end].join("\n");

    // Apply progressive truncation when no explicit offset/limit was given
    // and truncation is enabled (the default).
    let (output_content, was_truncated) =
        if input.truncate_large_files && input.offset.is_none() && input.limit.is_none() {
            let config = ProgressiveLoaderConfig::default();
            let truncated = truncate_content(&selected_lines, &config);
            let did_truncate = truncated.len() < selected_lines.len();
            (truncated, did_truncate)
        } else {
            (selected_lines.clone(), false)
        };

    // Count actual lines in the output (may differ from total after truncation)
    let output_lines = output_content.lines().count();

    let mut metadata = HashMap::new();
    metadata.insert("lines".to_string(), json!(output_lines));
    metadata.insert("total_lines".to_string(), json!(total_lines));
    metadata.insert("file_path".to_string(), json!(input.file_path));

    // Flag when progressive truncation actually happened so callers can tell.
    if was_truncated {
        metadata.insert("truncated".to_string(), json!(true));
        metadata.insert(
            "truncation_note".to_string(),
            json!("Use offset/limit parameters to read specific sections"),
        );
    }

    Ok(ToolOutput {
        content: output_content,
        is_error: false,
        metadata,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_image_path_png() {
        assert!(is_image_path("/path/to/image.png"));
        assert!(is_image_path("/path/to/image.PNG"));
        assert!(is_image_path("image.png"));
    }

    #[test]
    fn test_is_image_path_various_formats() {
        assert!(is_image_path("photo.jpg"));
        assert!(is_image_path("photo.jpeg"));
        assert!(is_image_path("animation.gif"));
        assert!(is_image_path("modern.webp"));
        assert!(is_image_path("bitmap.bmp"));
        assert!(is_image_path("vector.svg"));
    }

    #[test]
    fn test_is_image_path_non_image() {
        assert!(!is_image_path("document.txt"));
        assert!(!is_image_path("script.js"));
        assert!(!is_image_path("style.css"));
        assert!(!is_image_path("no_extension"));
        assert!(!is_image_path(""));
    }

    #[test]
    fn test_detect_media_type_png() {
        assert_eq!(detect_media_type("image.png"), "image/png");
        assert_eq!(detect_media_type("photo.PNG"), "image/png");
    }

    #[test]
    fn test_detect_media_type_jpeg() {
        assert_eq!(detect_media_type("photo.jpg"), "image/jpeg");
        assert_eq!(detect_media_type("photo.jpeg"), "image/jpeg");
    }

    #[test]
    fn test_detect_media_type_gif() {
        assert_eq!(detect_media_type("animation.gif"), "image/gif");
    }

    #[test]
    fn test_detect_media_type_webp() {
        assert_eq!(detect_media_type("modern.webp"), "image/webp");
    }

    #[test]
    fn test_detect_media_type_bmp() {
        assert_eq!(detect_media_type("bitmap.bmp"), "image/bmp");
    }

    #[test]
    fn test_detect_media_type_svg() {
        assert_eq!(detect_media_type("vector.svg"), "image/svg+xml");
    }

    #[test]
    fn test_detect_media_type_unknown() {
        assert_eq!(detect_media_type("file.txt"), "application/octet-stream");
        assert_eq!(
            detect_media_type("no_extension"),
            "application/octet-stream"
        );
    }

    #[test]
    fn test_has_magic_prefix_png() {
        let png_header = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        assert!(has_magic_prefix(&png_header, PNG_MAGIC));
    }

    #[test]
    fn test_has_magic_prefix_jpeg() {
        let jpeg_header = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
        assert!(has_magic_prefix(&jpeg_header, JPEG_MAGIC));
    }

    #[test]
    fn test_has_magic_prefix_too_short() {
        let short_data = vec![0x89, 0x50];
        assert!(!has_magic_prefix(&short_data, PNG_MAGIC));
    }

    #[test]
    fn test_is_image_by_magic_bytes() {
        let png_data = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        assert!(is_image_by_magic_bytes(&png_data));

        let jpeg_data = vec![0xFF, 0xD8, 0xFF, 0xE0];
        assert!(is_image_by_magic_bytes(&jpeg_data));

        let gif_data = vec![0x47, 0x49, 0x46, 0x38];
        assert!(is_image_by_magic_bytes(&gif_data));

        let webp_data = vec![0x52, 0x49, 0x46, 0x46];
        assert!(is_image_by_magic_bytes(&webp_data));

        let text_data = b"Hello, world!";
        assert!(!is_image_by_magic_bytes(text_data));
    }

    #[tokio::test]
    async fn test_read_large_file_truncated() {
        // Create a temp file with 3000 lines (exceeds default max_read_lines of 2000).
        let dir = tempfile::tempdir().expect("create temp dir");
        let path = dir.path().join("large.txt");
        let lines: Vec<String> = (1..=3000).map(|i| format!("line {i}")).collect();
        let content = lines.join("\n");
        std::fs::write(&path, &content).expect("write temp file");

        let input = ReadInput {
            file_path: path.to_string_lossy().to_string(),
            offset: None,
            limit: None,
            truncate_large_files: true,
        };

        let result = execute(input).await.expect("execute should succeed");
        assert!(!result.is_error, "should not be an error");

        // Verify metadata
        assert_eq!(result.metadata["total_lines"], 3000);
        assert_eq!(result.metadata["truncated"], true);

        // Verify content contains head and tail
        assert!(
            result.content.contains("line 1"),
            "should contain first head line"
        );
        assert!(
            result.content.contains("line 50"),
            "should contain last head line (line 50)"
        );
        assert!(
            result.content.contains("line 3000"),
            "should contain last tail line"
        );

        // Verify middle lines are omitted
        assert!(
            !result.content.contains("line 1000"),
            "should omit middle lines"
        );

        // Verify truncation notice
        assert!(
            result.content.contains("lines omitted"),
            "should contain omission notice"
        );
        assert!(
            result.content.contains("Total: 3000 lines"),
            "should contain total line count"
        );
    }

    #[tokio::test]
    async fn test_read_small_file_not_truncated() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let path = dir.path().join("small.txt");
        let content = "line 1\nline 2\nline 3";
        std::fs::write(&path, content).expect("write temp file");

        let input = ReadInput {
            file_path: path.to_string_lossy().to_string(),
            offset: None,
            limit: None,
            truncate_large_files: true,
        };

        let result = execute(input).await.expect("execute should succeed");
        assert_eq!(result.content, content);
        assert!(
            !result.metadata.contains_key("truncated"),
            "small files should not be truncated"
        );
    }

    #[tokio::test]
    async fn test_read_with_offset_limit_bypasses_truncation() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let path = dir.path().join("large2.txt");
        let lines: Vec<String> = (1..=3000).map(|i| format!("line {i}")).collect();
        let content = lines.join("\n");
        std::fs::write(&path, &content).expect("write temp file");

        let input = ReadInput {
            file_path: path.to_string_lossy().to_string(),
            offset: Some(100),
            limit: Some(50),
            truncate_large_files: true,
        };

        let result = execute(input).await.expect("execute should succeed");
        // With explicit offset/limit, truncation is skipped
        assert!(
            !result.metadata.contains_key("truncated"),
            "explicit offset/limit should bypass truncation"
        );
        assert!(result.content.contains("line 101"));
        assert!(result.content.contains("line 150"));
    }

    #[tokio::test]
    async fn test_read_truncation_disabled() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let path = dir.path().join("large3.txt");
        let lines: Vec<String> = (1..=2500).map(|i| format!("line {i}")).collect();
        let content = lines.join("\n");
        std::fs::write(&path, &content).expect("write temp file");

        let input = ReadInput {
            file_path: path.to_string_lossy().to_string(),
            offset: None,
            limit: None,
            truncate_large_files: false,
        };

        let result = execute(input).await.expect("execute should succeed");
        assert!(
            !result.metadata.contains_key("truncated"),
            "truncation disabled should return full content"
        );
        assert!(
            result.content.contains("line 1250"),
            "full content should be present"
        );
    }
}
