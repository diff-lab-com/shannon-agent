//! P2-3 (improvement plan §P2-3): snapshot tests for the `/export` command.
//!
//! Locks down the JSON and markdown output shape produced by
//! `export_to_json` / `export_to_markdown` against a fixed session. The
//! shape is the public contract for any downstream consumer (CLI,
//! desktop REPL, future web export tool), so drift here is a breaking
//! change. The user-described "if diff stub" branch is intentionally
//! skipped — `/diff` is implemented end-to-end, so `/export` is the
//! richer test target.

use shannon_commands::export_utils::{
    ExportMessage, ExportOptions, ExportSession, SessionMetadata, export_to_json,
    export_to_markdown, generate_filename, parse_export_args,
};

fn fixture_session() -> ExportSession {
    ExportSession {
        title: "Snapshot fixture".to_string(),
        started_at: 1_700_000_000,
        messages: vec![
            ExportMessage {
                role: "user".to_string(),
                content: "hello world".to_string(),
                timestamp: Some(1_700_000_010),
            },
            ExportMessage {
                role: "assistant".to_string(),
                content: "hi!".to_string(),
                timestamp: Some(1_700_000_020),
            },
        ],
        metadata: SessionMetadata {
            model: "test-model".to_string(),
            tokens_used: 42,
            working_dir: "/tmp/fixture".to_string(),
            commands_run: 1,
            tools_invoked: 0,
        },
    }
}

/// JSON output with the default options. Any change to the field set,
/// ordering, or `format_version` constant will surface as a diff here.
#[test]
fn export_json_default_snapshot() {
    let session = fixture_session();
    let options = ExportOptions::default();
    let json = export_to_json(&session, &options);
    insta::assert_json_snapshot!("export_json_default", json);
}

/// Markdown output with default options. Headers and separators are part
/// of the contract — third-party consumers parse them.
#[test]
fn export_markdown_default_snapshot() {
    let session = fixture_session();
    let options = ExportOptions::default();
    let md = export_to_markdown(&session, &options);
    insta::assert_snapshot!("export_markdown_default", md);
}

/// Filename generation: shape only (timestamp is non-deterministic).
/// We assert the prefix and extension so a refactor that drops the
/// `shannon_session_` prefix or renames the extension breaks here. The
/// timestamp segment is normalized to a placeholder.
#[test]
fn export_filename_shape_snapshot() {
    use shannon_commands::export_utils::ExportFormat;
    let markdown = generate_filename(ExportFormat::Markdown);
    let json = generate_filename(ExportFormat::Json);

    // `generate_filename` returns e.g. `shannon_session_20260803_184933.md`.
    // Replace the timestamp segment with `<ts>` and assert the shape.
    fn shape(name: &str) -> String {
        let (head, ext) = name.split_once('.').expect("filename has an extension");
        // Drop the last `_YYYYMMDD_HHMMSS` segment — that's the timestamp.
        let prefix = head.rsplit('_').nth_back(2).map_or(head, |_| {
            // Split off everything from the second-to-last underscore.
            // Equivalent to dropping the trailing `_digits_digits`.
            let mut parts = head.rsplitn(3, '_');
            let _tail = parts.next();
            let _ts = parts.next();
            parts.next().unwrap_or(head)
        });
        format!("{prefix}_<ts>.{ext}\n")
    }
    insta::assert_snapshot!("export_filename_markdown_shape", shape(&markdown));
    insta::assert_snapshot!("export_filename_json_shape", shape(&json));
}

/// Parsing the CLI args must agree with the documented option set; this
/// guards against accidentally renaming a flag in a refactor.
#[test]
fn export_arg_parsing_snapshot() {
    let parsed = parse_export_args("json --no-metadata --sanitize session.json").unwrap();
    insta::assert_debug_snapshot!("export_arg_parsing_json_sanitize", parsed);
}
