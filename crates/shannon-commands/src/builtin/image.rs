//! /image command - Attach image files for AI analysis
//!
//! Registers the `/image` command in the command registry so it appears in
//! help text and tab-completion. Actual execution is handled by the REPL's
//! media module (`shannon-ui/src/repl/commands/media.rs`).

use crate::command::{Command, CommandAvailability, CommandBase, CommandSource, LocalCommand};

/// Create the /image command
pub fn command() -> Command {
    Command::Local(LocalCommand {
        base: CommandBase {
            name: "image".to_string(),
            aliases: vec!["img".to_string(), "screenshot".to_string()],
            description:
                "Attach an image file, paste from clipboard, or fetch from URL for AI analysis"
                    .to_string(),
            has_user_specified_description: false,
            availability: vec![CommandAvailability::All],
            source: CommandSource::Builtin,
            is_enabled: true,
            is_hidden: false,
            argument_hint: Some(
                "<path> [prompt] | paste [prompt] | url <url> [prompt]".to_string(),
            ),
            when_to_use: Some(
                "Use to share a screenshot, diagram, or photo with the AI for visual analysis"
                    .to_string(),
            ),
            version: Some("0.1.0".to_string()),
            disable_model_invocation: false,
            user_invocable: true,
            is_workflow: false,
            immediate: false,
            is_sensitive: false,
            user_facing_name: None,
        },
        supports_non_interactive: false,
    })
}

/// Detect media type from a file extension.
///
/// Returns the MIME type string for supported image formats, or `None`
/// if the extension is not recognised.
pub fn detect_media_type(extension: &str) -> Option<&'static str> {
    match extension.to_lowercase().as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "bmp" => Some("image/bmp"),
        "svg" => Some("image/svg+xml"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_image_command() {
        let cmd = command();
        assert_eq!(cmd.name(), "image");
        assert!(cmd.aliases().contains(&"img".to_string()));
        assert!(cmd.aliases().contains(&"screenshot".to_string()));
    }

    #[test]
    fn test_image_command_argument_hint() {
        let cmd = command();
        assert!(cmd.argument_hint().is_some());
        let hint = cmd.argument_hint().unwrap();
        assert!(hint.contains("paste"));
        assert!(hint.contains("url"));
    }

    #[test]
    fn test_detect_media_type() {
        assert_eq!(detect_media_type("png"), Some("image/png"));
        assert_eq!(detect_media_type("jpg"), Some("image/jpeg"));
        assert_eq!(detect_media_type("jpeg"), Some("image/jpeg"));
        assert_eq!(detect_media_type("gif"), Some("image/gif"));
        assert_eq!(detect_media_type("webp"), Some("image/webp"));
        assert_eq!(detect_media_type("bmp"), Some("image/bmp"));
        assert_eq!(detect_media_type("svg"), Some("image/svg+xml"));
        assert_eq!(detect_media_type("txt"), None);
        assert_eq!(detect_media_type("pdf"), None);
        assert_eq!(detect_media_type("PNG"), Some("image/png"));
        assert_eq!(detect_media_type("JPG"), Some("image/jpeg"));
    }
}
