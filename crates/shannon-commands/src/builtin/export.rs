//! /export command - Export session data to various formats

use crate::command::{
    Command, CommandAvailability, CommandBase, CommandSource, ExecutionContext, PromptCommand,
};
use serde_json::Value as JsonValue;

// ─────────────────────────────────────────────────────────────────────────────
// session_transcript → ExportSession adapter (P1-1 §step 2)
//
// The persistent [shannon_core::session_transcript] module already stores
// transcript entries as JSONL under `~/.shannon/transcripts/<sid>.jsonl`.
// This adapter converts a sequence of `TranscriptEntry` items into the
// in-memory `ExportSession` shape consumed by `export_to_markdown` /
// `export_to_json`, so `/export <session-id>` can pull real history instead
// of relying solely on the LLM to scrape the conversation.
// ─────────────────────────────────────────────────────────────────────────────

/// Convert a [`shannon_core::session_transcript::TranscriptRole`] into the
/// lowercase role string used by [`ExportMessage`].
fn transcript_role_to_string(
    role: shannon_core::session_transcript::TranscriptRole,
) -> &'static str {
    use shannon_core::session_transcript::TranscriptRole;
    match role {
        TranscriptRole::User => "user",
        TranscriptRole::Assistant => "assistant",
        TranscriptRole::System => "system",
        TranscriptRole::Tool => "tool",
    }
}

/// Convert a [`chrono::DateTime<Utc>`] to a Unix-seconds timestamp that fits
/// the [`ExportMessage::timestamp`] / [`ExportSession::started_at`] shape.
fn datetime_to_unix_secs(dt: &chrono::DateTime<chrono::Utc>) -> u64 {
    dt.timestamp().max(0) as u64
}

/// Build an [`ExportSession`] from an ordered list of transcript entries.
///
/// `title` is supplied by the caller (typically the CLI flag or generated
/// from `session_id`). `working_dir` and `model` are best-effort: if the
/// caller has no value, the empty string is used.
pub fn export_session_from_transcript(
    title: impl Into<String>,
    entries: &[shannon_core::session_transcript::TranscriptEntry],
    working_dir: impl Into<String>,
    model: impl Into<String>,
) -> ExportSession {
    let working_dir = working_dir.into();
    let model = model.into();
    let messages: Vec<ExportMessage> = entries
        .iter()
        .map(|e| ExportMessage {
            role: transcript_role_to_string(e.role).to_string(),
            content: e.content.clone(),
            timestamp: Some(datetime_to_unix_secs(&e.timestamp)),
        })
        .collect();

    let started_at = entries
        .first()
        .map(|e| datetime_to_unix_secs(&e.timestamp))
        .unwrap_or(0);

    let total_chars: usize = entries.iter().map(|e| e.content.len()).sum();
    let commands_run = entries
        .iter()
        .filter(|e| {
            matches!(
                e.role,
                shannon_core::session_transcript::TranscriptRole::User
            )
        })
        .count();
    let tools_invoked: usize = entries.iter().map(|e| e.tool_calls.len()).sum();

    ExportSession {
        title: title.into(),
        started_at,
        messages,
        metadata: SessionMetadata {
            model,
            tokens_used: total_chars / 4, // crude char/4 estimate — not authoritative
            working_dir,
            commands_run,
            tools_invoked,
        },
    }
}

/// Read a session from the default transcript store (`~/.shannon/transcripts/`),
/// build an [`ExportSession`], and return it. Returns an error string if the
/// store cannot be opened or the session cannot be found.
pub fn export_session_from_store(
    session_id: &str,
    title: impl Into<String>,
    working_dir: impl Into<String>,
    model: impl Into<String>,
) -> Result<ExportSession, String> {
    use shannon_core::session_transcript::TranscriptStore;

    let store = TranscriptStore::new().map_err(|e| format!("transcript store: {e}"))?;
    let entries = store
        .get_session(session_id)
        .map_err(|e| format!("transcript session '{session_id}': {e}"))?;
    Ok(export_session_from_transcript(
        title,
        &entries,
        working_dir,
        model,
    ))
}

/// Parse the `/export` arguments and, if the user passed a session ID
/// (positional first arg, named `--session <id>`, or `--from-transcript <id>`),
/// pull the transcript and build an [`ExportSession`]. Otherwise returns
/// `Ok(None)` to signal "no transcript input — handler should fall back to
/// the LLM-driven export path".
///
/// Currently this is an opt-in adapter: REPL integration that wants live
/// session export wires this in; the LLM prompt path remains in place so
/// the model can still synthesise an export when no transcript is supplied.
pub fn maybe_build_session_from_args(args: &str) -> Result<Option<ExportSession>, String> {
    let options = parse_export_args(args)?;

    let session_id = extract_session_id_arg(args);
    if let Some(session_id) = session_id {
        // Title = session_id is the deterministic fallback; user can override via /export
        // prompt later.
        let session = export_session_from_store(&session_id, session_id.clone(), "", "")?;
        // Honour the sanitisation flag from parsed options.
        let _ = options.sanitize; // currently applied inside to_*_export; here we just
        // acknowledge it.  Future: pipe through a sanitisation pass
        // before handing to the formatter.
        return Ok(Some(session));
    }
    Ok(None)
}

/// Look for `<session_id>` / `--session <id>` / `--from-transcript <id>` in
/// the raw argument string. Returns the first match.
fn extract_session_id_arg(args: &str) -> Option<String> {
    let tokens: Vec<&str> = args.split_whitespace().collect();
    let mut i = 0;
    while i < tokens.len() {
        match tokens[i] {
            "--session" | "--from-transcript" => {
                if let Some(id) = tokens.get(i + 1) {
                    return Some((*id).to_string());
                }
            }
            t if t.starts_with("--session=") => {
                let id = &t["--session=".len()..];
                if !id.is_empty() {
                    return Some(id.to_string());
                }
            }
            t if t.starts_with("--from-transcript=") => {
                let id = &t["--from-transcript=".len()..];
                if !id.is_empty() {
                    return Some(id.to_string());
                }
            }
            _ => {}
        }
        i += 1;
    }
    // Bare first arg that doesn't match any flag prefix and isn't a known
    // format flag may also be a session ID. Reject obviously non-ID tokens.
    for t in &tokens {
        let lower = t.to_lowercase();
        if matches!(lower.as_str(), "md" | "markdown" | "json" | "") {
            continue;
        }
        if t.starts_with('-') {
            continue;
        }
        if lower.contains("--no") || lower == "--sanitize" || lower == "--help" {
            continue;
        }
        if lower.ends_with(".md") || lower.ends_with(".json") {
            // Treat as filename, not session id
            continue;
        }
        return Some((*t).to_string());
    }
    None
}

/// Prompt template for the /export command.
///
/// Instructs the AI to format the conversation for export using the
/// markdown and JSON structures defined in this module.
const EXPORT_PROMPT: &str = r##"
Export the current session conversation to a file.

Arguments: {args}
- If args contains "json": output as structured JSON with title, messages, and metadata
- If args contains "md" or is empty: output as formatted markdown
- If args contains "--sanitize" or subcommand is "share": redact API keys, tokens, and home directory paths
- If a filename is provided (e.g., "session.md" or "output.json"), write to that file
- If no filename is provided, generate one like "shannon_session_YYYYMMDD_HHMMSS.md"

For **Markdown** format, include:
- Title: "Shannon Session Export"
- Metadata section: model used, working directory, session duration
- Conversation: each message with role header (User/Assistant/System/Tool) and timestamp
- Separator lines between messages

For **JSON** format, include:
```json
{
  "title": "...",
  "started_at": <timestamp>,
  "format_version": "1.0",
  "metadata": { "model": "...", "tokens_used": N, ... },
  "messages": [{ "role": "...", "content": "...", "timestamp": N }]
}
```

Use the Bash tool to write the output file.
"##;

/// Create the /export command
pub fn command() -> Command {
    Command::Prompt(Box::new(PromptCommand {
        base: CommandBase {
            name: "export".to_string(),
            aliases: vec![
                "save".to_string(),
                "export-session".to_string(),
                "share".to_string(),
            ],
            description: "Export or share current session to markdown or JSON format".to_string(),
            has_user_specified_description: false,
            availability: vec![CommandAvailability::All],
            source: CommandSource::Builtin,
            is_enabled: true,
            is_hidden: false,
            argument_hint: Some("[md|json] [filename]".to_string()),
            when_to_use: Some(
                "To save your current conversation or session data to a file for later reference"
                    .to_string(),
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
        prompt_template: Some(EXPORT_PROMPT.to_string()),
    }))
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
    pub fn parse_format(s: &str) -> Option<Self> {
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

    /// Sanitize/redact sensitive content (API keys, home paths)
    pub sanitize: bool,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            format: ExportFormat::Markdown,
            filename: None,
            include_metadata: true,
            include_timestamps: true,
            sanitize: false,
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
            "--sanitize" => {
                options.sanitize = true;
            }
            t if t.starts_with('-') => {
                return Err(format!("Unknown option: {t}"));
            }
            t => {
                // Assume it's a filename — reject path traversal
                if options.filename.is_none() {
                    if t.contains('/') || t.contains('\\') || t.contains("..") {
                        return Err(format!(
                            "Invalid filename (path separators not allowed): {t}"
                        ));
                    }
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
        md.push_str(&format!("- **Tokens:** {}\n", session.metadata.tokens_used));
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

    if options.sanitize {
        md = sanitize_content(
            &md,
            &dirs::home_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default(),
        );
    }

    md
}

/// Export session to JSON
pub fn export_to_json(session: &ExportSession, options: &ExportOptions) -> JsonValue {
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

    if options.sanitize {
        sanitize_json_value(
            &mut json_obj,
            &dirs::home_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default(),
        );
        json_obj
    } else {
        json_obj
    }
}

fn sanitize_json_value(value: &mut JsonValue, home_dir: &str) {
    match value {
        JsonValue::String(text) => *text = sanitize_content(text, home_dir),
        JsonValue::Array(values) => {
            for value in values {
                sanitize_json_value(value, home_dir);
            }
        }
        JsonValue::Object(map) => {
            for value in map.values_mut() {
                sanitize_json_value(value, home_dir);
            }
        }
        JsonValue::Null | JsonValue::Bool(_) | JsonValue::Number(_) => {}
    }
}

/// Format a Unix timestamp as a UTC `YYYY-MM-DD HH:MM:SS` string.
///
/// Unix epochs are timezone-agnostic, so the exported markdown uses UTC
/// to keep session snapshots stable regardless of the reader's local
/// timezone — critical for the `/export` insta snapshots, which must
/// produce byte-identical output in CI (`TZ=UTC`) and on developer
/// machines (`TZ=America/Los_Angeles`, etc.).
fn format_timestamp(secs: u64) -> String {
    use chrono::{DateTime, Utc};

    let dt = DateTime::<Utc>::from_timestamp(secs as i64, 0).unwrap_or_default();
    dt.format("%Y-%m-%d %H:%M:%S").to_string()
}

/// Write export to file
pub fn write_export(content: &str, filename: &str) -> Result<(), String> {
    std::fs::write(filename, content)
        .map_err(|e| format!("Failed to write export to '{filename}': {e}"))
}

/// Sanitize sensitive content from text (API keys, tokens, private paths).
pub fn sanitize_content(content: &str, home_dir: &str) -> String {
    let mut sanitized = content.to_string();

    // Replace common API key patterns
    let patterns = [
        (r"sk-[a-zA-Z0-9]{20,}", "<REDACTED_API_KEY>"),
        (
            r"sk-ant-api03-[a-zA-Z0-9\-]{20,}",
            "<REDACTED_ANTHROPIC_KEY>",
        ),
        (r"ghp_[a-zA-Z0-9]{36}", "<REDACTED_GITHUB_TOKEN>"),
        (r"gho_[a-zA-Z0-9]{36}", "<REDACTED_GITHUB_OAUTH>"),
        (r"glpat-[a-zA-Z0-9\-]{20,}", "<REDACTED_GITLAB_TOKEN>"),
        (
            r#"(?i)(api[_-]?key|token|secret|password|credential)\s*[:=]\s*["']?[a-zA-Z0-9\-_.]{8,}"#,
            "$1: <REDACTED>",
        ),
    ];

    for (pattern, replacement) in &patterns {
        if let Ok(re) = regex::Regex::new(pattern) {
            sanitized = re.replace_all(&sanitized, *replacement).to_string();
        }
    }

    // Replace home directory paths
    if !home_dir.is_empty() {
        sanitized = sanitized.replace(home_dir, "~");
    }

    sanitized
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
        assert_eq!(
            ExportFormat::parse_format("md"),
            Some(ExportFormat::Markdown)
        );
        assert_eq!(
            ExportFormat::parse_format("markdown"),
            Some(ExportFormat::Markdown)
        );
        assert_eq!(ExportFormat::parse_format("json"), Some(ExportFormat::Json));
        assert_eq!(ExportFormat::parse_format("invalid"), None);
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
    fn markdown_export_produces_headings() {
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
    fn json_export_produces_valid_json() {
        let session = ExportSession {
            title: "Test Session".to_string(),
            started_at: 1_600_000_000,
            messages: vec![ExportMessage {
                role: "user".to_string(),
                content: "Hello".to_string(),
                timestamp: Some(1_600_000_000),
            }],
            metadata: SessionMetadata {
                model: "claude-3".to_string(),
                tokens_used: 100,
                working_dir: "/home/user".to_string(),
                commands_run: 2,
                tools_invoked: 1,
            },
        };

        let json = export_to_json(&session, &ExportOptions::default());
        let serialized = serde_json::to_string(&json).expect("export should produce valid JSON");
        let parsed: JsonValue = serde_json::from_str(&serialized).expect("JSON should parse");

        assert_eq!(parsed["title"], "Test Session");
        assert_eq!(parsed["messages"][0]["role"], "user");
    }

    #[test]
    fn test_sanitize_removes_api_keys() {
        let content = "My key is sk-ant-api03-abcdefghijklmnopqrstuvwx and token ghp_123456789012345678901234567890123456";
        let sanitized = sanitize_content(content, "/home/user");
        assert!(!sanitized.contains("sk-ant-api03-"));
        assert!(!sanitized.contains("ghp_"));
        assert!(sanitized.contains("<REDACTED"));
    }

    #[test]
    fn test_sanitize_replaces_home_dir() {
        let content = "File at /home/user/project/src/main.rs";
        let sanitized = sanitize_content(content, "/home/user");
        assert_eq!(sanitized, "File at ~/project/src/main.rs");
    }

    // ── Transcript → ExportSession Adapter Tests (P1-1 §step 2) ────────

    fn entry(
        role: shannon_core::session_transcript::TranscriptRole,
        content: &str,
        ts_secs: i64,
    ) -> shannon_core::session_transcript::TranscriptEntry {
        use chrono::TimeZone;
        let timestamp = chrono::Utc.timestamp_opt(ts_secs, 0).single().unwrap();
        shannon_core::session_transcript::TranscriptEntry::with_timestamp(
            role,
            content.to_string(),
            "sess-1".to_string(),
            timestamp,
        )
    }

    #[test]
    fn export_session_from_transcript_maps_roles_and_timestamps() {
        use shannon_core::session_transcript::TranscriptRole;
        let entries = vec![
            entry(TranscriptRole::User, "hi", 1_700_000_000),
            entry(TranscriptRole::Assistant, "hello", 1_700_000_010),
            entry(TranscriptRole::Tool, "ok", 1_700_000_020),
        ];
        let session = export_session_from_transcript("S1", &entries, "/tmp", "claude");
        assert_eq!(session.title, "S1");
        assert_eq!(session.started_at, 1_700_000_000);
        assert_eq!(session.messages.len(), 3);
        assert_eq!(session.messages[0].role, "user");
        assert_eq!(session.messages[1].role, "assistant");
        assert_eq!(session.messages[2].role, "tool");
        assert_eq!(session.messages[1].content, "hello");
        assert_eq!(session.messages[0].timestamp, Some(1_700_000_000));
        assert_eq!(session.metadata.working_dir, "/tmp");
        assert_eq!(session.metadata.model, "claude");
        assert_eq!(session.metadata.commands_run, 1, "user entries");
        assert_eq!(session.metadata.tools_invoked, 0);
    }

    #[test]
    fn export_session_from_transcript_counts_tool_calls() {
        use shannon_core::session_transcript::{ToolCallRecord, TranscriptRole};
        let mut e = entry(TranscriptRole::Assistant, "running tools", 1_700_000_000);
        e = e.with_tool_call(ToolCallRecord {
            name: "bash".to_string(),
            input: serde_json::json!({"cmd": "ls"}),
            success: true,
            duration_ms: Some(12),
        });
        e = e.with_tool_call(ToolCallRecord {
            name: "read".to_string(),
            input: serde_json::json!({"path": "x.rs"}),
            success: true,
            duration_ms: Some(5),
        });
        let session = export_session_from_transcript("S2", &[e], "/tmp", "claude");
        assert_eq!(session.metadata.tools_invoked, 2);
        assert!(session.metadata.tokens_used > 0);
    }

    #[test]
    fn export_session_from_transcript_empty_yields_zero_started_at() {
        let session = export_session_from_transcript("Empty", &[], "/tmp", "claude");
        assert_eq!(session.messages.len(), 0);
        assert_eq!(session.started_at, 0);
        assert_eq!(session.metadata.commands_run, 0);
        assert_eq!(session.metadata.tools_invoked, 0);
    }

    #[test]
    fn maybe_build_session_no_args_returns_none() {
        let result = maybe_build_session_from_args("").unwrap();
        assert!(result.is_none(), "no session id → no auto-pull");
    }

    #[test]
    fn maybe_build_session_format_only_returns_none() {
        // Pure format/filename args should never trigger a transcript pull.
        let result = maybe_build_session_from_args("md notes.md").unwrap();
        assert!(result.is_none());
        let result = maybe_build_session_from_args("json").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn export_session_from_store_missing_session_returns_err() {
        // Random session id; no transcript file exists.
        let result = export_session_from_store(
            "definitely-not-a-real-session-id-12345",
            "MISSING",
            "/tmp",
            "claude",
        );
        // Either an Err (transcript store lookup failed) or an Ok(empty)
        // is acceptable — what matters is no panic.
        match result {
            Ok(s) => assert_eq!(s.messages.len(), 0),
            Err(e) => assert!(e.contains("transcript")),
        }
    }
}
