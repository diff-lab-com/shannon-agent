//! /export command - Export session data to various formats

use crate::command::{Command, CommandBase, CommandSource, PromptCommand, ExecutionContext, CommandAvailability};
use serde_json::Value as JsonValue;

/// Create the /export command
pub fn command() -> Command {
    Command::Prompt(PromptCommand {
        base: CommandBase {
            name: "export".to_string(),
            aliases: vec!["save".to_string(), "export-session".to_string()],
            description: "Export current session to markdown or JSON format".to_string(),
            has_user_specified_description: false,
            availability: vec![CommandAvailability::All],
            source: CommandSource::Builtin,
            is_enabled: true,
            is_hidden: false,
            argument_hint: Some("[md|json] [filename]".to_string()),
            when_to_use: Some(
                "To save your current conversation or session data to a file for later reference".to_string(),
            ),
            version: Some("0.1.0".to_string()),
            disable_model_invocation: false,
            user_invocable: true,
            is_workflow: false,
            immediate: true,
            is_sensitive: false,
            user_facing_name: None,
        },
        progress_message: "".to_string(),
        content_length: 1000,
        arg_names: vec!["format".to_string(), "filename".to_string()],
        allowed_tools: vec![],
        model: None,
        hooks: std::collections::HashMap::new(),
        context: ExecutionContext::Inline,
        agent: None,
        paths: vec![],
    })
}

/// Export format
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    /// Markdown format
    Markdown,
    /// JSON format
    Json,
}

impl ExportFormat {
    /// Parse format from string
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "md" | "markdown" => Some(ExportFormat::Markdown),
            "json" => Some(ExportFormat::Json),
            _ => None,
        }
    }

    /// Get file extension for this format
    pub fn extension(&self) -> &str {
        match self {
            ExportFormat::Markdown => "md",
            ExportFormat::Json => "json",
        }
    }

    /// Get mime type for this format
    pub fn mime_type(&self) -> &str {
        match self {
            ExportFormat::Markdown => "text/markdown",
            ExportFormat::Json => "application/json",
        }
    }
}

/// Session message for export
#[derive(Debug, Clone)]
pub struct ExportMessage {
    /// Message role (user, assistant, system, tool)
    pub role: String,

    /// Message content
    pub content: String,

    /// Optional timestamp
    pub timestamp: Option<u64>,
}

/// Session data for export
#[derive(Debug, Clone)]
pub struct ExportSession {
    /// Session title
    pub title: String,

    /// Session start time
    pub started_at: u64,

    /// Messages in the session
    pub messages: Vec<ExportMessage>,

    /// Session metadata
    pub metadata: SessionMetadata,
}

/// Session metadata
#[derive(Debug, Clone)]
pub struct SessionMetadata {
    /// Model used
    pub model: String,

    /// Total tokens used
    pub tokens_used: usize,

    /// Working directory
    pub working_dir: String,

    /// Commands run
    pub commands_run: usize,

    /// Tools invoked
    pub tools_invoked: usize,
}

/// Export options
#[derive(Debug, Clone)]
pub struct ExportOptions {
    /// Export format
    pub format: ExportFormat,

    /// Output filename (optional, will generate if not provided)
    pub filename: Option<String>,

    /// Include metadata
    pub include_metadata: bool,

    /// Include timestamps
    pub include_timestamps: bool,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            format: ExportFormat::Markdown,
            filename: None,
            include_metadata: true,
            include_timestamps: true,
        }
    }
}

/// Parse export arguments into options
pub fn parse_export_args(args: &str) -> Result<ExportOptions, String> {
    let tokens: Vec<&str> = args.split_whitespace().collect();
    let mut options = ExportOptions::default();

    let mut i = 0;
    while i < tokens.len() {
        match tokens[i] {
            "md" | "markdown" => {
                options.format = ExportFormat::Markdown;
            }
            "json" => {
                options.format = ExportFormat::Json;
            }
            "--no-metadata" => {
                options.include_metadata = false;
            }
            "--no-timestamps" => {
                options.include_timestamps = false;
            }
            t if t.starts_with('-') => {
                return Err(format!("Unknown option: {}", t));
            }
            t => {
                // Assume it's a filename
                if options.filename.is_none() {
                    options.filename = Some(t.to_string());
                }
            }
        }
        i += 1;
    }

    Ok(options)
}

/// Generate a default filename based on current time
pub fn generate_filename(format: ExportFormat) -> String {
    use chrono::Local;

    let now = Local::now();
    let timestamp = now.format("%Y%m%d_%H%M%S");

    format!("shannon_session_{}.{}", timestamp, format.extension())
}

/// Export session to markdown
pub fn export_to_markdown(session: &ExportSession, options: &ExportOptions) -> String {
    let mut md = String::new();

    md.push_str("# Shannon Session Export\n\n");

    if options.include_metadata {
        md.push_str("## Session Metadata\n\n");
        md.push_str(&format!("- **Title:** {}\n", session.title));
        md.push_str(&format!(
            "- **Started:** {}\n",
            format_timestamp(session.started_at)
        ));
        md.push_str(&format!("- **Model:** {}\n", session.metadata.model));
        md.push_str(&format!(
            "- **Tokens:** {}\n",
            session.metadata.tokens_used
        ));
        md.push_str(&format!(
            "- **Working Directory:** {}\n",
            session.metadata.working_dir
        ));
        md.push_str(&format!(
            "- **Commands Run:** {}\n",
            session.metadata.commands_run
        ));
        md.push_str(&format!(
            "- **Tools Invoked:** {}\n",
            session.metadata.tools_invoked
        ));
        md.push_str("\n---\n\n");
    }

    md.push_str("## Conversation\n\n");

    for msg in &session.messages {
        let role_header = match msg.role.as_str() {
            "user" => "## User",
            "assistant" => "## Assistant",
            "system" => "## System",
            "tool" => "## Tool",
            _ => "## Unknown",
        };

        md.push_str(role_header);

        if options.include_timestamps {
            if let Some(ts) = msg.timestamp {
                md.push_str(&format!(" ({})", format_timestamp(ts)));
            }
        }

        md.push_str("\n\n");
        md.push_str(&msg.content);
        md.push_str("\n\n---\n\n");
    }

    md
}

/// Export session to JSON
pub fn export_to_json(session: &ExportSession, options: &ExportOptions) -> String {
    let mut json_obj = serde_json::json!({
        "title": session.title,
        "started_at": session.started_at,
        "format_version": "1.0",
    });

    if options.include_metadata {
        json_obj["metadata"] = serde_json::json!({
            "model": session.metadata.model,
            "tokens_used": session.metadata.tokens_used,
            "working_dir": session.metadata.working_dir,
            "commands_run": session.metadata.commands_run,
            "tools_invoked": session.metadata.tools_invoked,
        });
    }

    let messages: Vec<JsonValue> = session
        .messages
        .iter()
        .map(|msg| {
            let mut msg_obj = serde_json::json!({
                "role": msg.role,
                "content": msg.content,
            });

            if options.include_timestamps {
                if let Some(ts) = msg.timestamp {
                    msg_obj["timestamp"] = serde_json::json!(ts);
                }
            }

            msg_obj
        })
        .collect();

    json_obj["messages"] = serde_json::json!(messages);

    serde_json::to_string_pretty(&json_obj).unwrap_or_default()
}

/// Format a Unix timestamp as readable string
fn format_timestamp(secs: u64) -> String {
    use chrono::{DateTime, Local, Utc};

    let dt = DateTime::<Utc>::from_timestamp(secs as i64, 0)
        .unwrap_or_default();
    let local: DateTime<Local> = dt.into();

    local.format("%Y-%m-%d %H:%M:%S").to_string()
}

/// Write export to file
pub fn write_export(content: &str, filename: &str) -> Result<(), String> {
    std::fs::write(filename, content)
        .map_err(|e| format!("Failed to write export to '{}': {}", filename, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_export_command() {
        let cmd = command();
        assert_eq!(cmd.name(), "export");
        assert!(cmd.aliases().contains(&"save".to_string()));
    }

    #[test]
    fn test_export_format_from_str() {
        assert_eq!(ExportFormat::from_str("md"), Some(ExportFormat::Markdown));
        assert_eq!(ExportFormat::from_str("markdown"), Some(ExportFormat::Markdown));
        assert_eq!(ExportFormat::from_str("json"), Some(ExportFormat::Json));
        assert_eq!(ExportFormat::from_str("invalid"), None);
    }

    #[test]
    fn test_export_format_extension() {
        assert_eq!(ExportFormat::Markdown.extension(), "md");
        assert_eq!(ExportFormat::Json.extension(), "json");
    }

    #[test]
    fn test_parse_export_args_default() {
        let options = parse_export_args("").unwrap();
        assert_eq!(options.format, ExportFormat::Markdown);
        assert!(options.filename.is_none());
        assert!(options.include_metadata);
        assert!(options.include_timestamps);
    }

    #[test]
    fn test_parse_export_args_json() {
        let options = parse_export_args("json").unwrap();
        assert_eq!(options.format, ExportFormat::Json);
    }

    #[test]
    fn test_parse_export_args_with_filename() {
        let options = parse_export_args("md my_session.md").unwrap();
        assert_eq!(options.format, ExportFormat::Markdown);
        assert_eq!(options.filename, Some("my_session.md".to_string()));
    }

    #[test]
    fn test_parse_export_args_no_metadata() {
        let options = parse_export_args("md --no-metadata").unwrap();
        assert!(!options.include_metadata);
        assert!(options.include_timestamps);
    }

    #[test]
    fn test_generate_filename() {
        let filename = generate_filename(ExportFormat::Markdown);
        assert!(filename.starts_with("shannon_session_"));
        assert!(filename.ends_with(".md"));

        let json_filename = generate_filename(ExportFormat::Json);
        assert!(json_filename.ends_with(".json"));
    }

    #[test]
    fn test_export_to_markdown() {
        let session = ExportSession {
            title: "Test Session".to_string(),
            started_at: 1_600_000_000,
            messages: vec![
                ExportMessage {
                    role: "user".to_string(),
                    content: "Hello".to_string(),
                    timestamp: Some(1_600_000_000),
                },
                ExportMessage {
                    role: "assistant".to_string(),
                    content: "Hi there!".to_string(),
                    timestamp: Some(1_600_000_001),
                },
            ],
            metadata: SessionMetadata {
                model: "claude-3".to_string(),
                tokens_used: 100,
                working_dir: "/home/user".to_string(),
                commands_run: 2,
                tools_invoked: 1,
            },
        };

        let options = ExportOptions::default();
        let md = export_to_markdown(&session, &options);

        assert!(md.contains("# Shannon Session Export"));
        assert!(md.contains("**Title:** Test Session"));
        assert!(md.contains("## User"));
        assert!(md.contains("Hello"));
        assert!(md.contains("## Assistant"));
        assert!(md.contains("Hi there!"));
    }

    #[test]
    fn test_export_to_json() {
        let session = ExportSession {
            title: "Test Session".to_string(),
            started_at: 1_600_000_000,
            messages: vec![
                ExportMessage {
                    role: "user".to_string(),
                    content: "Hello".to_string(),
                    timestamp: Some(1_600_000_000),
                },
            ],
            metadata: SessionMetadata {
                model: "claude-3".to_string(),
                tokens_used: 100,
                working_dir: "/home/user".to_string(),
                commands_run: 2,
                tools_invoked: 1,
            },
        };

        let options = ExportOptions::default();
        let json = export_to_json(&session, &options);

        assert!(json.contains("\"title\""));
        assert!(json.contains("\"Test Session\""));
        assert!(json.contains("\"messages\""));
        assert!(json.contains("\"role\""));
        assert!(json.contains("\"user\""));
    }
}
