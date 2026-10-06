use super::*;

// ── ToolResultEntry tests ────────────────────────────────────────

fn make_image_json_content(base64_data: &str, media_type: &str, path: &str) -> String {
    serde_json::json!({
        "type": "image",
        "media_type": media_type,
        "data": base64_data,
        "path": path,
        "size": 1024
    })
    .to_string()
}

#[test]
fn test_tool_result_entry_text_result() {
    let entry = ToolResultEntry {
        tool_use_id: "tool_1".to_string(),
        content: "Hello world".to_string(),
        is_error: false,
        metadata: Default::default(),
    };
    let result = entry.to_tool_result_content();
    assert!(result.is_some());
    match result.unwrap() {
        ToolResultContent::Single(text) => assert_eq!(text, "Hello world"),
        other => panic!("Expected Single, got: {other:?}"),
    }
}

#[test]
fn test_tool_result_entry_error_result() {
    let entry = ToolResultEntry {
        tool_use_id: "tool_1".to_string(),
        content: "Something failed".to_string(),
        is_error: true,
        metadata: Default::default(),
    };
    let result = entry.to_tool_result_content();
    assert!(result.is_some());
    match result.unwrap() {
        ToolResultContent::Single(text) => assert_eq!(text, "Something failed"),
        other => panic!("Expected Single for error, got: {other:?}"),
    }
}

#[test]
fn test_tool_result_entry_image_result_creates_multiple_blocks() {
    let image_content = make_image_json_content("iVBORw0KGgo=", "image/png", "/tmp/test.png");
    let entry = ToolResultEntry {
        tool_use_id: "tool_1".to_string(),
        content: image_content,
        is_error: false,
        metadata: {
            let mut map = std::collections::HashMap::new();
            map.insert("type".to_string(), serde_json::json!("image"));
            map.insert("media_type".to_string(), serde_json::json!("image/png"));
            map.insert("file_path".to_string(), serde_json::json!("/tmp/test.png"));
            map
        },
    };
    let result = entry.to_tool_result_content();
    assert!(result.is_some());
    match result.unwrap() {
        ToolResultContent::Multiple(blocks) => {
            assert_eq!(blocks.len(), 2, "Expected text + image blocks");
            // First block should be a text description
            match &blocks[0] {
                ContentBlock::Text { text } => {
                    assert!(text.contains("/tmp/test.png"));
                    assert!(text.contains("image/png"));
                }
                other => panic!("Expected Text block, got: {other:?}"),
            }
            // Second block should be an image
            match &blocks[1] {
                ContentBlock::Image { source } => {
                    assert_eq!(source.source_type, "base64");
                    assert_eq!(source.media_type, "image/png");
                    assert_eq!(source.data, "iVBORw0KGgo=");
                }
                other => panic!("Expected Image block, got: {other:?}"),
            }
        }
        other => panic!("Expected Multiple for image, got: {other:?}"),
    }
}

#[test]
fn test_tool_result_entry_image_with_jpeg() {
    let image_content = make_image_json_content("/9j/4AAQSkZJ", "image/jpeg", "/photos/img.jpg");
    let entry = ToolResultEntry {
        tool_use_id: "tool_2".to_string(),
        content: image_content,
        is_error: false,
        metadata: {
            let mut map = std::collections::HashMap::new();
            map.insert("type".to_string(), serde_json::json!("image"));
            map.insert("media_type".to_string(), serde_json::json!("image/jpeg"));
            map.insert(
                "file_path".to_string(),
                serde_json::json!("/photos/img.jpg"),
            );
            map
        },
    };
    let result = entry.to_tool_result_content().unwrap();
    match result {
        ToolResultContent::Multiple(blocks) => match &blocks[1] {
            ContentBlock::Image { source } => {
                assert_eq!(source.media_type, "image/jpeg");
                assert_eq!(source.data, "/9j/4AAQSkZJ");
            }
            other => panic!("Expected Image block, got: {other:?}"),
        },
        other => panic!("Expected Multiple, got: {other:?}"),
    }
}

#[test]
fn test_tool_result_entry_non_image_metadata_ignored() {
    let entry = ToolResultEntry {
        tool_use_id: "tool_1".to_string(),
        content: "Regular text output".to_string(),
        is_error: false,
        metadata: {
            let mut map = std::collections::HashMap::new();
            map.insert("type".to_string(), serde_json::json!("text"));
            map.insert("lines".to_string(), serde_json::json!(42));
            map
        },
    };
    let result = entry.to_tool_result_content().unwrap();
    match result {
        ToolResultContent::Single(text) => assert_eq!(text, "Regular text output"),
        other => panic!("Expected Single for non-image, got: {other:?}"),
    }
}

#[test]
fn test_tool_result_entry_image_error_stays_single() {
    // Even if metadata says "image", errors should always be Single
    let entry = ToolResultEntry {
        tool_use_id: "tool_1".to_string(),
        content: "Image load failed".to_string(),
        is_error: true,
        metadata: {
            let mut map = std::collections::HashMap::new();
            map.insert("type".to_string(), serde_json::json!("image"));
            map
        },
    };
    let result = entry.to_tool_result_content().unwrap();
    match result {
        ToolResultContent::Single(text) => assert_eq!(text, "Image load failed"),
        other => panic!("Expected Single for error, got: {other:?}"),
    }
}

// ── Multi-image batch results (C-ImgBatch) ──────────────────────

/// Build an AnalyzeImages-style content payload with the given images.
fn make_batch_images_json(images: &[(&str, &str, &str)], prompt: &str) -> String {
    let arr: Vec<serde_json::Value> = images
        .iter()
        .map(|(source, media_type, data)| {
            serde_json::json!({
                "source": source,
                "media_type": media_type,
                "data": data,
            })
        })
        .collect();
    serde_json::json!({
        "type": "images",
        "count": arr.len(),
        "prompt": prompt,
        "images": arr,
    })
    .to_string()
}

#[test]
fn test_tool_result_entry_multi_image_creates_sectioned_blocks() {
    let content = make_batch_images_json(
        &[
            ("/tmp/a.png", "image/png", "AAAA"),
            ("/tmp/b.jpg", "image/jpeg", "BBBB"),
        ],
        "describe each",
    );
    let entry = ToolResultEntry {
        tool_use_id: "batch_1".to_string(),
        content,
        is_error: false,
        metadata: {
            let mut map = std::collections::HashMap::new();
            map.insert("type".to_string(), serde_json::json!("images"));
            map.insert("count".to_string(), serde_json::json!(2));
            map
        },
    };

    let result = entry.to_tool_result_content().unwrap();
    match result {
        ToolResultContent::Multiple(blocks) => {
            // intro + (heading + image) per image = 1 + 2*2 = 5 blocks
            assert_eq!(blocks.len(), 5, "blocks: {blocks:?}");
            match &blocks[0] {
                ContentBlock::Text { text } => {
                    assert!(text.contains("2 images"), "intro: {text}");
                    assert!(text.contains("describe each"), "intro: {text}");
                }
                other => panic!("Expected intro Text, got: {other:?}"),
            }
            // Per-image `## <path>` heading directly before its image.
            for (heading, image, expected) in [
                (&blocks[1], &blocks[2], ("/tmp/a.png", "image/png", "AAAA")),
                (&blocks[3], &blocks[4], ("/tmp/b.jpg", "image/jpeg", "BBBB")),
            ] {
                match heading {
                    ContentBlock::Text { text } => {
                        assert_eq!(text, &format!("## {}", expected.0));
                    }
                    other => panic!("Expected heading Text, got: {other:?}"),
                }
                match image {
                    ContentBlock::Image { source } => {
                        assert_eq!(source.media_type, expected.1);
                        assert_eq!(source.data, expected.2);
                    }
                    other => panic!("Expected Image block, got: {other:?}"),
                }
            }
        }
        other => panic!("Expected Multiple for batch images, got: {other:?}"),
    }
}

#[test]
fn test_tool_result_entry_multi_image_batch_error_stays_single() {
    let entry = ToolResultEntry {
        tool_use_id: "batch_1".to_string(),
        content: "Failed to load image 1 of 2".to_string(),
        is_error: true,
        metadata: {
            let mut map = std::collections::HashMap::new();
            map.insert("type".to_string(), serde_json::json!("images"));
            map
        },
    };
    let result = entry.to_tool_result_content().unwrap();
    match result {
        ToolResultContent::Single(text) => {
            assert_eq!(text, "Failed to load image 1 of 2");
        }
        other => panic!("Expected Single for batch error, got: {other:?}"),
    }
}

#[test]
fn test_tool_result_entry_multi_image_unparseable_falls_back_to_single() {
    let entry = ToolResultEntry {
        tool_use_id: "batch_1".to_string(),
        content: "not json".to_string(),
        is_error: false,
        metadata: {
            let mut map = std::collections::HashMap::new();
            map.insert("type".to_string(), serde_json::json!("images"));
            map
        },
    };
    let result = entry.to_tool_result_content().unwrap();
    match result {
        ToolResultContent::Single(text) => assert_eq!(text, "not json"),
        other => panic!("Expected Single fallback, got: {other:?}"),
    }
}

#[test]
fn test_tool_result_entry_multi_image_empty_data_entries_skipped() {
    let content = make_batch_images_json(
        &[
            ("/tmp/empty.png", "image/png", ""),
            ("/tmp/ok.png", "image/png", "CCCC"),
        ],
        "",
    );
    let entry = ToolResultEntry {
        tool_use_id: "batch_1".to_string(),
        content,
        is_error: false,
        metadata: {
            let mut map = std::collections::HashMap::new();
            map.insert("type".to_string(), serde_json::json!("images"));
            map
        },
    };
    let result = entry.to_tool_result_content().unwrap();
    match result {
        ToolResultContent::Multiple(blocks) => {
            // intro + 1 usable image pair (empty-data entry skipped)
            assert_eq!(blocks.len(), 3, "blocks: {blocks:?}");
            match &blocks[1] {
                ContentBlock::Text { text } => assert_eq!(text, "## /tmp/ok.png"),
                other => panic!("Expected heading Text, got: {other:?}"),
            }
        }
        other => panic!("Expected Multiple, got: {other:?}"),
    }
}

// use-browser-computer-upload branch: the `computer` tool returns
// base64 in `metadata["data"]` with plain text in `content`, while
// Read/AnalyzeImage return a JSON object in `content` with a `data`
// field. Both paths must produce an Image content block for the LLM
// to "see" the file; failure to parse must fall back to plain text.

fn make_entry(
    content: &str,
    metadata: std::collections::HashMap<String, serde_json::Value>,
) -> ToolResultEntry {
    ToolResultEntry {
        tool_use_id: "test".into(),
        content: content.into(),
        is_error: false,
        metadata,
    }
}

fn image_metadata() -> std::collections::HashMap<String, serde_json::Value> {
    let mut m = std::collections::HashMap::new();
    m.insert("type".into(), serde_json::json!("image"));
    m.insert("media_type".into(), serde_json::json!("image/png"));
    m.insert("data".into(), serde_json::json!("iVBORw0KGgo="));
    m.insert("width".into(), serde_json::json!(1920u32));
    m.insert("height".into(), serde_json::json!(1080u32));
    m.insert("file_path".into(), serde_json::json!("/tmp/shot.png"));
    m
}

#[test]
fn tool_result_entry_image_from_metadata_data() {
    let entry = make_entry("Screenshot captured (1920x1080)", image_metadata());
    let out = entry
        .to_tool_result_content()
        .expect("image result yields content");
    let ToolResultContent::Multiple(blocks) = out else {
        panic!("expected Multiple for image metadata, got single");
    };
    assert_eq!(blocks.len(), 2);
    // text block describes the file (path + media type + dims)
    let ContentBlock::Text { text } = &blocks[0] else {
        panic!("first block must be Text");
    };
    assert!(text.contains("/tmp/shot.png"));
    assert!(text.contains("image/png"));
    assert!(text.contains("1920x1080"));
    // image block carries the base64 payload untouched
    let ContentBlock::Image { source } = &blocks[1] else {
        panic!("second block must be Image");
    };
    assert_eq!(source.media_type, "image/png");
    assert_eq!(source.data, "iVBORw0KGgo=");
}

#[test]
fn tool_result_entry_image_from_content_json_convention() {
    // Read/AnalyzeImage convention: base64 lives in a `data` field of
    // a JSON object in `content`; metadata only carries image markers.
    let mut meta = std::collections::HashMap::new();
    meta.insert("type".into(), serde_json::json!("image"));
    meta.insert("media_type".into(), serde_json::json!("image/jpeg"));
    meta.insert("file_path".into(), serde_json::json!("pic.jpg"));
    let entry = make_entry(r#"{"type":"image","data":"/9j/4AAQSk=="}"#, meta);
    let out = entry
        .to_tool_result_content()
        .expect("image result yields content");
    let ToolResultContent::Multiple(blocks) = out else {
        panic!("expected Multiple");
    };
    assert_eq!(blocks.len(), 2);
    let ContentBlock::Image { source } = &blocks[1] else {
        panic!("second block must be Image");
    };
    assert_eq!(source.data, "/9j/4AAQSk==");
}

#[test]
fn tool_result_entry_image_without_path_uses_generic_label() {
    let mut meta = std::collections::HashMap::new();
    meta.insert("type".into(), serde_json::json!("image"));
    meta.insert("media_type".into(), serde_json::json!("image/png"));
    meta.insert("data".into(), serde_json::json!("AAA="));
    let entry = make_entry("Screenshot captured (WxH)", meta);
    let out = entry.to_tool_result_content().expect("ok");
    let ToolResultContent::Multiple(blocks) = out else {
        panic!("expected Multiple");
    };
    let ContentBlock::Text { text } = &blocks[0] else {
        panic!("first block must be Text");
    };
    // No file_path → generic label, still descriptive
    assert!(text.starts_with("Image (image/png)"));
    assert!(text.contains("The image content is provided"));
}

#[test]
fn tool_result_entry_image_no_payload_falls_back_to_single() {
    // Both conventions absent: we still want a Text fallback rather
    // than dropping the result on the floor.
    let mut meta = std::collections::HashMap::new();
    meta.insert("type".into(), serde_json::json!("image"));
    meta.insert("media_type".into(), serde_json::json!("image/png"));
    // No data field anywhere
    let entry = make_entry("plain text no base64", meta);
    let out = entry.to_tool_result_content().expect("ok");
    assert!(matches!(out, ToolResultContent::Single(_)));
}

#[test]
fn tool_result_entry_non_image_returns_single() {
    let mut meta = std::collections::HashMap::new();
    meta.insert("exit_code".into(), serde_json::json!(0));
    let entry = make_entry("hello world", meta);
    assert!(matches!(
        entry.to_tool_result_content(),
        Some(ToolResultContent::Single(_))
    ));
}

#[test]
fn tool_result_entry_image_with_error_returns_single() {
    let entry = ToolResultEntry {
        tool_use_id: "t".into(),
        content: "boom".into(),
        is_error: true,
        metadata: image_metadata(),
    };
    // is_error short-circuits to Single regardless of metadata type.
    assert!(matches!(
        entry.to_tool_result_content(),
        Some(ToolResultContent::Single(_))
    ));
}

// ---- T4 slice: client-boundary end-to-end with an installed transform --
// A local mock Anthropic /v1/messages captures the request body that
// LlmClient actually puts on the wire after `transform_outgoing_messages`
// has run. The full query-loop drive (run_query) needs a broader harness
// and remains follow-up work; this proves serialization + HTTP honor the
// transform chain byte-for-byte.
