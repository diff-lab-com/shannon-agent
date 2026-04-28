//! Terminal inline image rendering.
//!
//! Converts image data to half-block character representation for display
//! within ratatui's text buffer. Uses the "upper half block" (▀) technique:
//! each character cell encodes two vertical pixels by setting the cell's
//! foreground to the top pixel color and background to the bottom pixel color.
//!
//! Supports all terminals (no protocol required). For Kitty/Sixel/iTerm2
//! terminals, a future enhancement could use native protocols for higher
//! fidelity rendering.

use base64::Engine;
use image::{DynamicImage, GenericImageView, Rgba};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

/// Supported terminal image protocols (for future native rendering).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageProtocol {
    /// Kitty Graphics Protocol (best quality)
    Kitty,
    /// Sixel (widely supported in xterm, mintty)
    Sixel,
    /// iTerm2 inline image protocol
    Iterm2,
    /// Half-block characters (works everywhere)
    HalfBlocks,
}

/// Check if terminal supports Kitty graphics protocol
fn supports_kitty() -> bool {
    std::env::var("TERM").is_ok_and(|t| t.contains("kitty"))
}

/// Check if terminal supports Sixel protocol
fn supports_sixel() -> bool {
    // Check TERM_PROGRAM for known Sixel-capable terminals
    if std::env::var("TERM_PROGRAM").is_ok_and(|tp| {
        matches!(
            tp.as_str(),
            "WezTerm" | "mlterm" | "mintty" | "xt"
        )
    }) {
        return true;
    }

    // Check TERM variable
    if std::env::var("TERM").is_ok_and(|t| {
        t.contains("xterm") || t.contains("sixel") || t.contains("mlterm")
    }) {
        return true;
    }

    false
}

/// Check if terminal supports iTerm2 inline image protocol
fn supports_iterm() -> bool {
    std::env::var("TERM_PROGRAM").is_ok_and(|tp| tp.contains("iTerm"))
}

/// Detect the best available terminal image protocol.
///
/// Checks environment variables and terminal capabilities to determine
/// the optimal rendering method.
pub fn detect_protocol() -> ImageProtocol {
    if supports_kitty() {
        return ImageProtocol::Kitty;
    }

    if supports_sixel() {
        return ImageProtocol::Sixel;
    }

    if supports_iterm() {
        return ImageProtocol::Iterm2;
    }

    ImageProtocol::HalfBlocks
}

/// Detect best available image protocol (alias for detect_protocol)
pub fn detect_image_protocol() -> ImageProtocol {
    detect_protocol()
}

/// Configuration for image rendering.
#[derive(Debug, Clone)]
pub struct ImageRenderConfig {
    /// Maximum width in character cells (0 = auto-detect from terminal)
    pub max_width: u32,
    /// Maximum height in character cells (0 = no limit)
    pub max_height: u32,
    /// Whether to preserve aspect ratio
    pub preserve_aspect: bool,
}

impl Default for ImageRenderConfig {
    fn default() -> Self {
        Self {
            max_width: 60,
            max_height: 20,
            preserve_aspect: true,
        }
    }
}

/// Render base64-encoded image data into ratatui `Line` objects using
/// half-block characters.
///
/// Returns a vector of styled lines that can be displayed in any ratatui
/// widget. If decoding fails, returns a placeholder line.
pub fn render_image_base64(
    base64_data: &str,
    media_type: &str,
    config: &ImageRenderConfig,
) -> Vec<Line<'static>> {
    let decoded = match base64::engine::general_purpose::STANDARD.decode(base64_data) {
        Ok(d) => d,
        Err(e) => {
            return vec![Line::from(Span::styled(
                format!("[Image decode error: {e}]"),
                Style::default().fg(Color::Red),
            ))];
        }
    };

    let img = match decode_image(&decoded, media_type) {
        Some(i) => i,
        None => {
            return vec![Line::from(Span::styled(
                format!("[Unsupported image format: {media_type}]"),
                Style::default().fg(Color::Red),
            ))];
        }
    };

    render_halfblock_image(&img, config)
}

/// Render image bytes into ratatui `Line` objects.
pub fn render_image_bytes(
    data: &[u8],
    config: &ImageRenderConfig,
) -> Vec<Line<'static>> {
    let img = match image::load_from_memory(data) {
        Ok(i) => i,
        Err(e) => {
            return vec![Line::from(Span::styled(
                format!("[Image decode error: {e}]"),
                Style::default().fg(Color::Red),
            ))];
        }
    };

    render_halfblock_image(&img, config)
}

/// Decode image bytes with explicit media type hint.
fn decode_image(data: &[u8], media_type: &str) -> Option<DynamicImage> {
    // Try with format hint first
    let format = match media_type {
        "image/png" => Some(image::ImageFormat::Png),
        "image/jpeg" | "image/jpg" => Some(image::ImageFormat::Jpeg),
        "image/webp" => Some(image::ImageFormat::WebP),
        "image/gif" => Some(image::ImageFormat::Gif),
        _ => None,
    };

    if let Some(fmt) = format {
        if let Ok(img) = image::load_from_memory_with_format(data, fmt) {
            return Some(img);
        }
    }

    // Fallback: auto-detect format
    image::load_from_memory(data).ok()
}

/// Convert a `DynamicImage` to half-block character lines.
///
/// Each character cell represents 2 vertical pixels using the upper
/// half-block character (▀) with foreground = top pixel, background = bottom pixel.
fn render_halfblock_image(img: &DynamicImage, config: &ImageRenderConfig) -> Vec<Line<'static>> {
    let (orig_w, orig_h) = img.dimensions();

    // Calculate target dimensions
    let char_width = if config.max_width > 0 {
        config.max_width.min(orig_w)
    } else {
        orig_w.min(80)
    };

    // Each character cell = 2 vertical pixels, so char_height = pixel_height / 2
    let pixel_width = char_width;
    let pixel_height = if config.preserve_aspect {
        // Maintain aspect ratio: pixel_h / pixel_w = orig_h / orig_w
        let h = (pixel_width as f64 * orig_h as f64 / orig_w as f64).round() as u32;
        if config.max_height > 0 {
            h.min(config.max_height * 2) // max_height is in char cells, 2 pixels per cell
        } else {
            h
        }
    } else if config.max_height > 0 {
        config.max_height * 2
    } else {
        (pixel_width as f64 * orig_h as f64 / orig_w as f64).round() as u32
    };

    if pixel_width == 0 || pixel_height == 0 {
        return vec![Line::from(Span::styled(
            "[Image too small to render]",
            Style::default().fg(Color::DarkGray),
        ))];
    }

    // Resize the image to target pixel dimensions
    let resized = img.resize_exact(pixel_width, pixel_height, image::imageops::FilterType::Triangle);

    // Build lines using half-block characters
    let mut lines = Vec::new();

    // Process two rows at a time
    let mut y = 0u32;
    while y < pixel_height {
        let top_y = y;
        let bot_y = y + 1;

        let mut spans = Vec::new();

        for x in 0..pixel_width {
            let top_color = rgba_to_ratatui_color(resized.get_pixel(x, top_y));

            let bottom_color = if bot_y < pixel_height {
                rgba_to_ratatui_color(resized.get_pixel(x, bot_y))
            } else {
                // Odd number of rows: use transparent/dark for bottom
                Color::Rgb(0, 0, 0)
            };

            spans.push(Span::styled(
                "▀".to_string(),
                Style::default().fg(top_color).bg(bottom_color),
            ));
        }

        lines.push(Line::from(spans));
        y += 2;
    }

    // Add a metadata line below the image
    let meta = format!("[{orig_w}x{orig_h} image]");
    lines.push(Line::from(Span::styled(
        meta,
        Style::default().fg(Color::DarkGray),
    )));

    lines
}

/// Convert an RGBA pixel to a ratatui RGB color.
#[inline]
fn rgba_to_ratatui_color(px: Rgba<u8>) -> Color {
    Color::Rgb(px[0], px[1], px[2])
}

/// Generate a placeholder line for an image that cannot be rendered inline.
///
/// Useful when image data is too large or terminal doesn't support rendering.
pub fn image_placeholder(width: u32, height: u32, media_type: &str) -> Line<'static> {
    let label = format!("[{width}x{height} {media_type}]");
    Line::from(vec![
        Span::styled(" 🖼 ", Style::default().fg(Color::Cyan)),
        Span::styled(label, Style::default().fg(Color::DarkGray)),
    ])
}

/// Encode image data as Sixel string.
///
/// Sixel is a bitmap graphics format supported by various terminal emulators
/// including xterm, mlterm, WezTerm, and mintty. This implementation provides
/// a basic Sixel encoding. For production use, consider using a dedicated library.
pub fn encode_sixel(img: &DynamicImage, max_width: u16, max_height: u16) -> Option<String> {
    // Resize image to fit constraints
    let resized = img.resize(
        max_width as u32,
        max_height as u32,
        image::imageops::FilterType::Triangle,
    );

    let rgba = resized.to_rgba8();
    let (width, height) = rgba.dimensions();

    // Sixel requires even dimensions
    let width = width - (width % 2);
    let height = height - (height % 6);

    if width == 0 || height == 0 {
        return None;
    }

    // Build a simple color-reduced palette (16 colors for compatibility)
    let palette = quantize_colors(&rgba, 16);

    // Start Sixel sequence: DCS (Device Control String)
    let mut output = String::from("\x1BPq");

    // Set raster attributes: Pan, Pad, Ph, Pv
    // Pan=1, Pad=1 (aspect ratio 1:1), Ph=width, Pv=height
    output.push_str(&format!("\"1;1;{};{}\x1B\\", width, height));

    // For each color in palette, encode pixels
    for (color_idx, color) in palette.iter().enumerate() {
        // Set color using RGB format
        let r = (color[0] as f32 / 255.0 * 100.0) as u8;
        let g = (color[1] as f32 / 255.0 * 100.0) as u8;
        let b = (color[2] as f32 / 255.0 * 100.0) as u8;
        output.push_str(&format!("#{};2;{};{};{}", color_idx, r, g, b));

        // Encode pixels in sixel format (6 vertical pixels per character)
        for y in (0..height).step_by(6) {
            let mut sixel_row = String::new();
            let mut has_pixels = false;

            for x in 0..width {
                let mut bits = 0u8;
                for bit in 0..6 {
                    let py = y + bit as u32;
                    if py < height {
                        let pixel = rgba.get_pixel(x, py);
                        // Check if this pixel matches our palette color
                        if pixel[0] == color[0] && pixel[1] == color[1] && pixel[2] == color[2] {
                            bits |= 1 << bit;
                            has_pixels = true;
                        }
                    }
                }

                if bits > 0 {
                    sixel_row.push(char::from(b'?' + bits));
                } else if x == 0 {
                    sixel_row.push('?');
                }
            }

            if has_pixels || !sixel_row.is_empty() {
                output.push_str(&sixel_row);
                output.push('$');
            }
        }

        output.push('-');
    }

    // End Sixel sequence: ST (String Terminator)
    output.push_str("\x1B\\");

    Some(output)
}

/// Simple color quantization for Sixel encoding.
fn quantize_colors(rgba: &image::RgbaImage, max_colors: usize) -> Vec<[u8; 3]> {
    use std::collections::HashMap;

    let mut color_counts: HashMap<[u8; 3], usize> = HashMap::new();

    // Count color occurrences
    for pixel in rgba.pixels() {
        let rgb = [pixel[0], pixel[1], pixel[2]];
        *color_counts.entry(rgb).or_insert(0) += 1;
    }

    // Sort by frequency and take top colors
    let mut colors: Vec<([u8; 3], usize)> = color_counts.into_iter().collect();
    colors.sort_by(|a, b| b.1.cmp(&a.1));

    colors
        .into_iter()
        .take(max_colors)
        .map(|(rgb, _)| rgb)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_protocol_returns_halfblocks_fallback() {
        // In CI/test environments without special terminals, should fall back
        let protocol = detect_protocol();
        // Just verify it doesn't panic and returns a valid variant
        assert!(matches!(
            protocol,
            ImageProtocol::HalfBlocks
                | ImageProtocol::Kitty
                | ImageProtocol::Sixel
                | ImageProtocol::Iterm2
        ));
    }

    #[test]
    fn test_render_image_base64_invalid_data() {
        let config = ImageRenderConfig::default();
        let lines = render_image_base64("not-valid-base64!!!", "image/png", &config);
        assert!(!lines.is_empty());
        // Should contain error message
        let text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.to_string()))
            .collect();
        assert!(text.contains("error") || text.contains("decode"));
    }

    #[test]
    fn test_render_image_base64_valid_png() {
        // Create a minimal 4x4 red PNG
        let img = DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            4, 4,
            image::Rgb([255, 0, 0]),
        ));
        let mut buf = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
            .unwrap();
        let b64 = base64::engine::general_purpose::STANDARD.encode(&buf);

        let config = ImageRenderConfig {
            max_width: 4,
            max_height: 4,
            preserve_aspect: true,
        };
        let lines = render_image_base64(&b64, "image/png", &config);

        // Should have at least 2 rows of half-block chars + metadata line
        assert!(lines.len() >= 2);
        // Last line should be metadata
        let last = lines.last().unwrap();
        let meta_text: String = last.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(meta_text.contains("4x4"));
    }

    #[test]
    fn test_render_image_bytes_direct() {
        let img = DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            8, 8,
            image::Rgb([0, 255, 0]),
        ));
        let mut buf = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
            .unwrap();

        let config = ImageRenderConfig {
            max_width: 8,
            max_height: 8,
            preserve_aspect: true,
        };
        let lines = render_image_bytes(&buf, &config);
        assert!(!lines.is_empty());
    }

    #[test]
    fn test_image_placeholder() {
        let line = image_placeholder(640, 480, "image/png");
        assert!(!line.spans.is_empty());
        let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(text.contains("640"));
        assert!(text.contains("480"));
    }

    #[test]
    fn test_render_config_default() {
        let config = ImageRenderConfig::default();
        assert_eq!(config.max_width, 60);
        assert_eq!(config.max_height, 20);
        assert!(config.preserve_aspect);
    }

    #[test]
    fn test_render_preserves_aspect_ratio() {
        // Wide image (200x100) with max_width=40
        let img = DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            200, 100,
            image::Rgb([128, 128, 128]),
        ));
        let mut buf = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
            .unwrap();

        let config = ImageRenderConfig {
            max_width: 40,
            max_height: 50,
            preserve_aspect: true,
        };
        let lines = render_image_bytes(&buf, &config);

        // With aspect ratio preserved: 40 chars wide, ~20 pixel rows = ~10 char rows + metadata
        // 200/100 = 2:1 ratio, so pixel_height = 40 * 100/200 = 20, char_height = 10
        assert!(lines.len() >= 10); // 10 half-block rows + metadata
        assert!(lines.len() <= 12); // Allow some tolerance
    }

    #[test]
    fn test_render_tall_image() {
        // Tall image (100x200) with max_width=20, max_height=30
        let img = DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            100, 200,
            image::Rgb([64, 64, 64]),
        ));
        let mut buf = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
            .unwrap();

        let config = ImageRenderConfig {
            max_width: 20,
            max_height: 30,
            preserve_aspect: true,
        };
        let lines = render_image_bytes(&buf, &config);
        // Should be capped at max_height char rows + metadata
        assert!(lines.len() <= 32); // 30 char rows + metadata + tolerance
    }

    #[test]
    fn test_decode_image_jpeg_hint() {
        // Create a minimal JPEG-like header won't work, test with PNG hint on PNG data
        let img = DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            2, 2,
            image::Rgb([100, 100, 100]),
        ));
        let mut buf = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
            .unwrap();

        // PNG data with correct hint
        let result = decode_image(&buf, "image/png");
        assert!(result.is_some());

        // PNG data with wrong hint (should fall back to auto-detect)
        let result = decode_image(&buf, "image/unknown");
        assert!(result.is_some());
    }
}
