//! Media command handlers: image paste, URL fetch, clipboard, browse, copy/paste.

use crate::{widgets::ChatRole, Result};
use rust_i18n::t;
use super::super::Repl;

/// Handle Ctrl+V — paste image from system clipboard.
pub fn handle_image_paste_from_input(repl: &mut Repl) -> Result<()> {
    handle_image_paste(repl, "Describe this image.")
}

pub(crate) fn handle_image_paste(repl: &mut Repl, prompt_args: &str) -> Result<()> {
    use base64::Engine;
    use shannon_core::api::{ContentBlock, ImageSource};

    let prompt = if prompt_args.is_empty() {
        "Describe this image.".to_string()
    } else {
        prompt_args.to_string()
    };

    // Try reading clipboard image via platform tools
    let tmp_path = std::env::temp_dir().join("shannon_clipboard_paste.png");
    let tmp_str = tmp_path.to_string_lossy().to_string();

    let result = if cfg!(target_os = "macos") {
        std::process::Command::new("pngpaste")
            .arg(&tmp_str)
            .output()
    } else {
        // Linux: try xclip first, then wl-paste for Wayland
        let file = std::fs::File::create(&tmp_path);
        match file {
            Ok(f) => {
                let xclip = std::process::Command::new("xclip")
                    .args(["-selection", "clipboard", "-t", "image/png", "-o"])
                    .stdout(std::process::Stdio::from(f))
                    .output();
                match xclip {
                    Ok(o) if o.status.success() => Ok(o),
                    _ => {
                        // Fallback: wl-paste for Wayland
                        let f2 = std::fs::File::create(&tmp_path);
                        match f2 {
                            Ok(f2) => std::process::Command::new("wl-paste")
                                .args(["--type", "image/png"])
                                .stdout(std::process::Stdio::from(f2))
                                .output(),
                            Err(e) => Err(e),
                        }
                    }
                }
            }
            Err(e) => Err(e),
        }
    };

    match result {
        Ok(output) if output.status.success() && tmp_path.exists() => {
            let bytes = std::fs::read(&tmp_path)?;
            let _ = std::fs::remove_file(&tmp_path); // cleanup

            if bytes.len() < 10 {
                repl.chat.add_message(ChatRole::System,
                    "Clipboard does not contain a valid image.".to_string());
                return Ok(());
            }

            let base64_data = base64::engine::general_purpose::STANDARD.encode(&bytes);
            let engine = match repl.query_engine.as_mut() {
                Some(e) => e,
                None => {
                    repl.chat.add_message(ChatRole::System, t!("commands.image.no_engine").to_string());
                    return Ok(());
                }
            };

            let blocks = vec![
                ContentBlock::Text { text: prompt },
                ContentBlock::Image {
                    source: ImageSource::base64("image/png", base64_data),
                },
            ];
            engine.add_user_message_blocks(blocks);
            // Generate inline image preview from clipboard bytes
            let preview_config = crate::terminal_image::ImageRenderConfig::default();
            let preview_lines = crate::terminal_image::render_image_bytes(&bytes, &preview_config);
            repl.chat.add_message_with_image(
                ChatRole::User,
                "[Image pasted from clipboard]".to_string(),
                preview_lines,
            );
            repl.chat.add_message(ChatRole::System, t!("commands.image.clipboard_sent").to_string());

            super::super::query::handle_query(repl, "Please analyze the image I just shared from my clipboard.")?;
        }
        _ => {
            repl.chat.add_message(ChatRole::System,
                "Failed to read image from clipboard.\n\
                 Install xclip (X11) or wl-clipboard (Wayland) for Linux, or pngpaste for macOS.".to_string());
        }
    }
    Ok(())
}

/// Handle `/image url <url>` — fetch image from URL and send to API.
pub(crate) fn handle_image_url(repl: &mut Repl, input: &str) -> Result<()> {
    use base64::Engine;
    use shannon_core::api::{ContentBlock, ImageSource};

    // Split URL from optional prompt
    let (url, prompt) = if input.starts_with("http://") || input.starts_with("https://") {
        let mut parts = input.splitn(2, ' ');
        let url = parts.next().unwrap_or("").to_string();
        let prompt = parts.next().map(|p| p.trim().to_string())
            .unwrap_or_else(|| "Describe this image.".to_string());
        (url, prompt)
    } else {
        // Input starts after "url " prefix
        let mut parts = input.splitn(2, ' ');
        let url = parts.next().unwrap_or("").to_string();
        let prompt = parts.next().map(|p| p.trim().to_string())
            .unwrap_or_else(|| "Describe this image.".to_string());
        (url, prompt)
    };

    if url.is_empty() {
        repl.chat.add_message(ChatRole::System,
            "Usage: /image url <url> [prompt]\n\nFetch an image from a URL and send it for analysis.".to_string());
        return Ok(());
    }

    repl.chat.add_message(ChatRole::System, format!("Fetching image from {url}..."));

    // Fetch the image using the async runtime
    let fetch_result = repl.runtime.block_on(async {
        match reqwest::get(&url).await {
            Ok(resp) => {
                if !resp.status().is_success() {
                    return Err(format!("HTTP {}", resp.status()));
                }
                // Detect media type from Content-Type header
                let media_type = resp.headers()
                    .get("content-type")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("")
                    .to_string();

                let media_type = if media_type.starts_with("image/") {
                    media_type
                } else {
                    match url.rsplit('.').next().unwrap_or("").to_lowercase().as_str() {
                        "png" => "image/png".to_string(),
                        "jpg" | "jpeg" => "image/jpeg".to_string(),
                        "gif" => "image/gif".to_string(),
                        "webp" => "image/webp".to_string(),
                        "svg" => "image/svg+xml".to_string(),
                        _ => "image/png".to_string(),
                    }
                };

                match resp.bytes().await {
                    Ok(b) => Ok((b.to_vec(), media_type)),
                    Err(e) => Err(format!("Failed to read image data: {e}")),
                }
            }
            Err(e) => Err(format!("Failed to fetch image: {e}")),
        }
    });

    match fetch_result {
        Ok((bytes, media_type)) => {
            if bytes.len() < 10 {
                repl.chat.add_message(ChatRole::System,
                    "Response does not contain valid image data.".to_string());
                return Ok(());
            }

            let engine = match repl.query_engine.as_mut() {
                Some(e) => e,
                None => {
                    repl.chat.add_message(ChatRole::System, t!("commands.image.no_engine").to_string());
                    return Ok(());
                }
            };

            let base64_data = base64::engine::general_purpose::STANDARD.encode(&bytes);
            let blocks = vec![
                ContentBlock::Text { text: prompt },
                ContentBlock::Image {
                    source: ImageSource::base64(&media_type, base64_data),
                },
            ];

            engine.add_user_message_blocks(blocks);

            // Generate inline image preview
            let preview_config = crate::terminal_image::ImageRenderConfig::default();
            let preview_lines = crate::terminal_image::render_image_bytes(&bytes, &preview_config);
            repl.chat.add_message_with_image(
                ChatRole::User,
                format!("[Image from URL: {url}]"),
                preview_lines,
            );

            super::super::query::handle_query(repl, "Please analyze the image I just shared from the URL.")?;
        }
        Err(e) => {
            repl.chat.add_message(ChatRole::System,
                format!("Failed to fetch image: {e}"));
        }
    }

    Ok(())
}

pub(crate) fn handle_image(repl: &mut Repl, args: &str) -> Result<()> {
    use base64::Engine;
    use shannon_core::api::{ContentBlock, ImageSource};

    let input = args.trim();
    if input.is_empty() {
        repl.chat.add_message(ChatRole::System,
            "Usage: /image <path> [optional prompt]\n       /image paste [prompt]\n       /image url <url> [prompt]\n\nAttach an image file, paste from clipboard, or fetch from URL.\nSupports PNG, JPG, GIF, WebP, BMP, SVG.".to_string());
        return Ok(());
    }

    // Handle /image paste subcommand
    if input.starts_with("paste") {
        return handle_image_paste(repl, input.strip_prefix("paste").expect("checked starts_with").trim());
    }

    // Handle /image url <url> subcommand
    if input.starts_with("url ") {
        return handle_image_url(repl, input.strip_prefix("url ").expect("checked starts_with").trim());
    }

    // Auto-detect URL (starts with http:// or https://)
    if input.starts_with("http://") || input.starts_with("https://") {
        return handle_image_url(repl, input);
    }

    // Split path from optional prompt
    let (path, prompt) = if input.starts_with('"') {
        // Quoted path: "path with spaces" prompt
        if let Some(end) = input.strip_prefix('"').expect("checked starts_with").find('"') {
            let path = &input[1..end + 1];
            let prompt = input[end + 2..].trim();
            (path.to_string(), if prompt.is_empty() { "Describe this image.".to_string() } else { prompt.to_string() })
        } else {
            (input.to_string(), "Describe this image.".to_string())
        }
    } else {
        let mut parts = input.splitn(2, ' ');
        let path = parts.next().unwrap_or("").to_string();
        let prompt = parts.next().map(|p| p.trim().to_string())
            .unwrap_or_else(|| "Describe this image.".to_string());
        (path, prompt)
    };

    // Expand ~ to home dir
    let expanded_path = if path.starts_with("~/") {
        if let Some(home) = dirs::home_dir() {
            home.join(path.strip_prefix("~/").expect("checked starts_with")).to_string_lossy().to_string()
        } else {
            path.clone()
        }
    } else {
        path.clone()
    };

    let file_path = std::path::Path::new(&expanded_path);
    if !file_path.exists() {
        repl.chat.add_message(ChatRole::System, format!("File not found: {path}"));
        return Ok(());
    }

    let bytes = match std::fs::read(file_path) {
        Ok(b) => b,
        Err(e) => {
            super::set_error(repl, &format!("reading file: {e}"));
            return Ok(());
        }
    };

    // Detect media type from extension
    let media_type = match file_path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        _ => {
            repl.chat.add_message(ChatRole::System,
                format!("Unsupported image format: {path}. Supported: PNG, JPG, GIF, WebP, BMP, SVG"));
            return Ok(());
        }
    };

    let engine = match repl.query_engine.as_mut() {
        Some(e) => e,
        None => {
            repl.chat.add_message(ChatRole::System, t!("commands.image.no_engine").to_string());
            return Ok(());
        }
    };

    let base64_data = base64::engine::general_purpose::STANDARD.encode(&bytes);
    let blocks = vec![
        ContentBlock::Text { text: prompt },
        ContentBlock::Image {
            source: ImageSource::base64(media_type, base64_data),
        },
    ];

    engine.add_user_message_blocks(blocks);
    // Generate inline image preview
    let preview_config = crate::terminal_image::ImageRenderConfig::default();
    let preview_lines = crate::terminal_image::render_image_bytes(&bytes, &preview_config);
    repl.chat.add_message_with_image(
        ChatRole::User,
        format!("[Image attached: {}]", file_path.display()),
        preview_lines,
    );
    repl.chat.add_message(ChatRole::System, t!("commands.image.image_sent").to_string());

    // Trigger query processing
    super::super::query::handle_query(repl, &format!("Please analyze the image I just shared: {}", file_path.display()))?;
    Ok(())
}

pub(crate) fn handle_browse(repl: &mut Repl, args: &str) -> Result<()> {
    let path = if args.trim().is_empty() {
        repl.state.working_directory.clone()
    } else {
        args.trim().to_string()
    };

    let mut selector = crate::widgets::select::FileSelectorWidget::new("File Browser".to_string())
        .with_path(&path);
    if let Err(e) = selector.refresh() {
        super::set_error(repl, &format!("browsing {path}: {e}"));
        return Ok(());
    }
    repl.state.file_selector = Some(selector);
    Ok(())
}

fn copy_nth_response(repl: &mut Repl, n: usize) -> Option<String> {
    if n == 0 {
        repl.chat.add_message(ChatRole::System, "Invalid index. Use /copy 1 for the latest response.".to_string());
        return None;
    }
    let mut responses: Vec<String> = Vec::new();
    for (_, m) in repl.chat.iter_messages() {
        if m.role == ChatRole::Assistant {
            responses.push(m.content.clone());
        }
    }
    let total = responses.len();
    if n > total {
        repl.chat.add_message(ChatRole::System, format!("Only {total} assistant response(s) in this session. Use /copy 1 for the latest."));
        return None;
    }
    Some(responses[total - n].clone())
}

pub(crate) fn handle_copy(repl: &mut Repl, args: &str) -> Result<()> {
    let trimmed = args.trim();

    // Determine what to copy
    let content = if trimmed.is_empty() || trimmed == "last" || trimmed == "response" || trimmed == "1" {
        match copy_nth_response(repl, 1) {
            Some(c) => c,
            None => return Ok(()),
        }
    } else if trimmed.starts_with(|c: char| c.is_ascii_digit()) && !trimmed.contains(' ') {
        let n: usize = trimmed.parse().unwrap_or(1);
        match copy_nth_response(repl, n) {
            Some(c) => c,
            None => return Ok(()),
        }
    } else if trimmed == "status" {
        repl.state.status.clone()
    } else {
        trimmed.to_string()
    };

    if content.is_empty() {
        repl.chat.add_message(ChatRole::System, "Nothing to copy (empty content).".to_string());
        return Ok(());
    }

    // Try platform-specific clipboard commands
    let success = copy_to_clipboard(&content);
    if success {
        let preview = if content.len() > 60 { format!("{}...", &content[..60]) } else { content.clone() };
        repl.chat.add_message(ChatRole::System, format!("Copied to clipboard: {preview}"));
    } else {
        // Fallback: write to temp file
        let tmp = std::env::temp_dir().join("shannon-clipboard.txt");
        if std::fs::write(&tmp, &content).is_ok() {
            repl.chat.add_message(ChatRole::System,
                format!("Clipboard unavailable. Content saved to: {}\nInstall xclip or xsel for clipboard support.", tmp.display()));
        } else {
            repl.chat.add_message(ChatRole::System, "Failed to copy: no clipboard tool available.".to_string());
        }
    }
    Ok(())
}

pub(crate) fn handle_paste(repl: &mut Repl) -> Result<()> {
    let content = paste_from_clipboard();
    match content {
        Some(text) if !text.is_empty() => {
            repl.prompt.insert_text(&text);
            repl.chat.add_message(ChatRole::System, format!("Pasted {} chars into prompt.", text.len()));
        }
        Some(_) => {
            repl.chat.add_message(ChatRole::System, "Clipboard is empty.".to_string());
        }
        None => {
            // Fallback: try temp file
            let tmp = std::env::temp_dir().join("shannon-clipboard.txt");
            if tmp.exists() {
                if let Ok(text) = std::fs::read_to_string(&tmp) {
                    repl.prompt.insert_text(&text);
                    repl.chat.add_message(ChatRole::System, format!("Pasted {} chars from temp file.", text.len()));
                }
            } else {
                repl.chat.add_message(ChatRole::System,
                    "Clipboard unavailable. Install xclip or xsel for clipboard support.".to_string());
            }
        }
    }
    Ok(())
}

/// Copy text to system clipboard using platform tools.
pub(crate) fn copy_to_clipboard(content: &str) -> bool {
    // Try xclip first (Linux)
    if let Ok(mut child) = std::process::Command::new("xclip")
        .args(["-selection", "clipboard"])
        .stdin(std::process::Stdio::piped())
        .spawn()
    {
        if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write;
            let _ = stdin.write_all(content.as_bytes());
        }
        return child.wait().map(|s| s.success()).unwrap_or(false);
    }
    // Try xsel (Linux alternative)
    if let Ok(mut child) = std::process::Command::new("xsel")
        .args(["--clipboard", "--input"])
        .stdin(std::process::Stdio::piped())
        .spawn()
    {
        if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write;
            let _ = stdin.write_all(content.as_bytes());
        }
        return child.wait().map(|s| s.success()).unwrap_or(false);
    }
    // Try pbcopy (macOS)
    if let Ok(mut child) = std::process::Command::new("pbcopy")
        .stdin(std::process::Stdio::piped())
        .spawn()
    {
        if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write;
            let _ = stdin.write_all(content.as_bytes());
        }
        return child.wait().map(|s| s.success()).unwrap_or(false);
    }
    // Try wl-copy (Wayland)
    if let Ok(mut child) = std::process::Command::new("wl-copy")
        .stdin(std::process::Stdio::piped())
        .spawn()
    {
        if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write;
            let _ = stdin.write_all(content.as_bytes());
        }
        return child.wait().map(|s| s.success()).unwrap_or(false);
    }
    false
}

/// Paste text from system clipboard.
pub(crate) fn paste_from_clipboard() -> Option<String> {
    // Try xclip (Linux)
    if let Ok(output) = std::process::Command::new("xclip")
        .args(["-selection", "clipboard", "-o"])
        .output()
    {
        if output.status.success() {
            return Some(String::from_utf8_lossy(&output.stdout).to_string());
        }
    }
    // Try xsel (Linux alternative)
    if let Ok(output) = std::process::Command::new("xsel")
        .args(["--clipboard", "--output"])
        .output()
    {
        if output.status.success() {
            return Some(String::from_utf8_lossy(&output.stdout).to_string());
        }
    }
    // Try pbpaste (macOS)
    if let Ok(output) = std::process::Command::new("pbpaste").output() {
        if output.status.success() {
            return Some(String::from_utf8_lossy(&output.stdout).to_string());
        }
    }
    // Try wl-paste (Wayland)
    if let Ok(output) = std::process::Command::new("wl-paste").output() {
        if output.status.success() {
            return Some(String::from_utf8_lossy(&output.stdout).to_string());
        }
    }
    None
}
