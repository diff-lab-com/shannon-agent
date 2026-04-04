//! Read tool implementation

use crate::{ToolOutput, ToolError};
use serde::{Deserialize, Serialize};
use serde_json::json;
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

    // Check if file appears to be an image
    if is_image_path(&input.file_path) {
        // Read as binary
        let bytes = fs::read(&input.file_path)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to read image file: {}", e)))?;

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

            let json_output = serde_json::to_string_pretty(&image_output)
                .map_err(|e| ToolError::ExecutionFailed(format!("Failed to serialize image data: {}", e)))?;

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
        .map_err(|e| ToolError::ExecutionFailed(format!("Failed to read file: {}", e)))?;

    let lines: Vec<&str> = content.lines().collect();

    let (start, end) = match (input.offset, input.limit) {
        (Some(offset), Some(limit)) => {
            let start = offset.min(lines.len());
            let end = (offset + limit).min(lines.len());
            (start, end)
        }
        (Some(offset), None) => (offset.min(lines.len()), lines.len()),
        (None, Some(limit)) => (0, limit.min(lines.len())),
        (None, None) => (0, lines.len()),
    };

    let selected_lines = lines[start..end].join("\n");

    Ok(ToolOutput {
        content: selected_lines.clone(),
        is_error: false,
        metadata: {
            let mut map = HashMap::new();
            map.insert("lines".to_string(), json!(end - start));
            map.insert("file_path".to_string(), json!(input.file_path));
            map
        },
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
        assert_eq!(detect_media_type("no_extension"), "application/octet-stream");
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
}
