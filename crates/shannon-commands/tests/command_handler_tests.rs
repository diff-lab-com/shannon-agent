//! Integration tests for the 7 newly-wired command handlers.
//!
//! Tests the structured parsing, formatting, and analysis utilities
//! that are now connected to REPL command dispatch.

// ── Export Utils Tests ───────────────────────────────────────────

mod export_tests {
    use shannon_commands::export_utils::{
        ExportFormat, ExportMessage, ExportOptions, ExportSession, SessionMetadata, export_to_json,
        export_to_markdown, generate_filename, parse_export_args,
    };

    #[test]
    fn test_parse_format_markdown() {
        assert_eq!(
            ExportFormat::parse_format("md"),
            Some(ExportFormat::Markdown)
        );
        assert_eq!(
            ExportFormat::parse_format("markdown"),
            Some(ExportFormat::Markdown)
        );
        assert_eq!(
            ExportFormat::parse_format("Markdown"),
            Some(ExportFormat::Markdown)
        );
    }

    #[test]
    fn test_parse_format_json() {
        assert_eq!(ExportFormat::parse_format("json"), Some(ExportFormat::Json));
        assert_eq!(ExportFormat::parse_format("JSON"), Some(ExportFormat::Json));
    }

    #[test]
    fn test_parse_format_invalid() {
        assert_eq!(ExportFormat::parse_format("xml"), None);
        assert_eq!(ExportFormat::parse_format(""), None);
    }

    #[test]
    fn test_extension() {
        assert_eq!(ExportFormat::Markdown.extension(), "md");
        assert_eq!(ExportFormat::Json.extension(), "json");
    }

    #[test]
    fn test_parse_export_args_default() {
        let opts = parse_export_args("").unwrap();
        assert_eq!(opts.format, ExportFormat::Markdown);
        assert!(opts.filename.is_none());
    }

    #[test]
    fn test_parse_export_args_json() {
        let opts = parse_export_args("json").unwrap();
        assert_eq!(opts.format, ExportFormat::Json);
    }

    #[test]
    fn test_parse_export_args_with_filename() {
        let opts = parse_export_args("output.md").unwrap();
        assert_eq!(opts.filename.as_deref(), Some("output.md"));
    }

    #[test]
    fn test_parse_export_args_json_with_filename() {
        let opts = parse_export_args("report.json json").unwrap();
        assert_eq!(opts.format, ExportFormat::Json);
        assert_eq!(opts.filename.as_deref(), Some("report.json"));
    }

    #[test]
    fn test_generate_filename_markdown() {
        let name = generate_filename(ExportFormat::Markdown);
        assert!(name.ends_with(".md"));
        assert!(name.starts_with("shannon_session_"));
    }

    #[test]
    fn test_generate_filename_json() {
        let name = generate_filename(ExportFormat::Json);
        assert!(name.ends_with(".json"));
        assert!(name.starts_with("shannon_session_"));
    }

    #[test]
    fn test_export_to_markdown() {
        let session = make_test_session();
        let md = export_to_markdown(&session, &ExportOptions::default());
        assert!(md.contains("Shannon Session Export"));
        assert!(md.contains("User"));
        assert!(md.contains("hello"));
        assert!(md.contains("Assistant"));
        assert!(md.contains("world"));
    }

    #[test]
    fn test_export_to_json() {
        let session = make_test_session();
        let json_str = export_to_json(&session, &ExportOptions::default());
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        assert_eq!(parsed["title"], "Shannon Session");
        assert!(parsed["messages"].is_array());
        assert_eq!(parsed["messages"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_export_session_field_access() {
        // ExportSession does not derive Serialize/Deserialize,
        // so we test field access instead.
        let session = make_test_session();
        assert_eq!(session.title, "Shannon Session");
        assert_eq!(session.messages.len(), 2);
        assert_eq!(session.metadata.model, "test-model");
        assert_eq!(session.metadata.tokens_used, 500);
    }

    #[test]
    fn test_export_message_field_access() {
        // ExportMessage does not derive Serialize/Deserialize,
        // so we test field access instead.
        let msg = ExportMessage {
            role: "user".to_string(),
            content: "test content".to_string(),
            timestamp: Some(1234567890),
        };
        assert_eq!(msg.role, "user");
        assert_eq!(msg.content, "test content");
        assert_eq!(msg.timestamp, Some(1234567890));
    }

    fn make_test_session() -> ExportSession {
        ExportSession {
            title: "Shannon Session".to_string(),
            started_at: 1000000,
            messages: vec![
                ExportMessage {
                    role: "user".to_string(),
                    content: "hello".to_string(),
                    timestamp: Some(1000001),
                },
                ExportMessage {
                    role: "assistant".to_string(),
                    content: "world".to_string(),
                    timestamp: Some(1000002),
                },
            ],
            metadata: SessionMetadata {
                model: "test-model".to_string(),
                tokens_used: 500,
                working_dir: "/tmp/test".to_string(),
                commands_run: 3,
                tools_invoked: 2,
            },
        }
    }
}

// ── Search Utils Tests ───────────────────────────────────────────

mod search_tests {
    use shannon_commands::search_utils::{
        SearchOptions, format_results, parse_search_args, search_history,
    };

    #[test]
    fn test_parse_search_args_simple() {
        let opts = parse_search_args("hello world").unwrap();
        assert_eq!(opts.pattern, "hello world");
        assert_eq!(opts.count, 20);
        assert!(!opts.regex);
        assert!(!opts.case_sensitive);
    }

    #[test]
    fn test_parse_search_args_with_count() {
        let opts = parse_search_args("test --count=5").unwrap();
        assert_eq!(opts.pattern, "test");
        assert_eq!(opts.count, 5);
    }

    #[test]
    fn test_parse_search_args_regex() {
        let opts = parse_search_args("hello.*world --regex").unwrap();
        assert_eq!(opts.pattern, "hello.*world");
        assert!(opts.regex);
    }

    #[test]
    fn test_parse_search_args_case_sensitive() {
        let opts = parse_search_args("Hello --case-sensitive").unwrap();
        assert!(opts.case_sensitive);
    }

    #[test]
    fn test_parse_search_empty_pattern() {
        let result = parse_search_args("");
        assert!(result.is_err());
    }

    #[test]
    fn test_search_history_simple_match() {
        let entries = vec![
            "cargo build".to_string(),
            "cargo test".to_string(),
            "git commit -m fix".to_string(),
            "cargo clippy".to_string(),
        ];
        let opts = SearchOptions {
            pattern: "cargo".to_string(),
            count: 10,
            regex: false,
            case_sensitive: false,
            show_timestamps: true,
        };
        let results = search_history(&entries, &opts);
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn test_search_history_regex() {
        let entries = vec![
            "cargo build".to_string(),
            "cargo test".to_string(),
            "git push".to_string(),
        ];
        let opts = SearchOptions {
            pattern: "cargo (build|test)".to_string(),
            count: 10,
            regex: true,
            case_sensitive: false,
            show_timestamps: true,
        };
        let results = search_history(&entries, &opts);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_search_history_case_sensitive() {
        let entries = vec![
            "Hello world".to_string(),
            "hello World".to_string(),
            "HELLO WORLD".to_string(),
        ];
        let opts_case_insensitive = SearchOptions {
            pattern: "hello".to_string(),
            count: 10,
            regex: false,
            case_sensitive: false,
            show_timestamps: true,
        };
        let results = search_history(&entries, &opts_case_insensitive);
        assert_eq!(results.len(), 3);

        let opts_case_sensitive = SearchOptions {
            pattern: "hello".to_string(),
            count: 10,
            regex: false,
            case_sensitive: true,
            show_timestamps: true,
        };
        let results = search_history(&entries, &opts_case_sensitive);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_search_history_count_limit() {
        let entries: Vec<String> = (0..100).map(|i| format!("cargo cmd {i}")).collect();
        let opts = SearchOptions {
            pattern: "cargo".to_string(),
            count: 5,
            regex: false,
            case_sensitive: false,
            show_timestamps: true,
        };
        let results = search_history(&entries, &opts);
        assert!(results.len() <= 5);
    }

    #[test]
    fn test_search_history_no_match() {
        let entries = vec!["cargo build".to_string()];
        let opts = SearchOptions {
            pattern: "nonexistent".to_string(),
            count: 10,
            regex: false,
            case_sensitive: false,
            show_timestamps: true,
        };
        let results = search_history(&entries, &opts);
        assert!(results.is_empty());
    }

    #[test]
    fn test_format_results_empty() {
        let results = vec![];
        let opts = SearchOptions {
            pattern: "test".to_string(),
            count: 10,
            regex: false,
            case_sensitive: false,
            show_timestamps: true,
        };
        let output = format_results(&results, &opts);
        assert!(output.contains("No matches") || output.contains("0 matches"));
    }

    #[test]
    fn test_format_results_with_matches() {
        let entries = vec!["cargo build".to_string(), "cargo test".to_string()];
        let opts = SearchOptions {
            pattern: "cargo".to_string(),
            count: 10,
            regex: false,
            case_sensitive: false,
            show_timestamps: true,
        };
        let results = search_history(&entries, &opts);
        let output = format_results(&results, &opts);
        assert!(output.contains("cargo build"));
        assert!(output.contains("cargo test"));
    }
}

// ── Diff Utils Tests ─────────────────────────────────────────────

mod diff_tests {
    use shannon_commands::diff_utils::{
        ChangeCategory, DiffAnalyzer, DiffOptions, DiffScope, build_diff_command,
    };

    #[test]
    fn test_diff_options_default() {
        let opts = DiffOptions::from_args("");
        assert!(matches!(opts.scope, DiffScope::Working));
        assert!(opts.path_filter.is_none());
    }

    #[test]
    fn test_diff_options_staged() {
        let opts = DiffOptions::from_args("--staged");
        assert!(matches!(opts.scope, DiffScope::Staged));
    }

    #[test]
    fn test_diff_options_commit_range() {
        let opts = DiffOptions::from_args("main...HEAD");
        assert!(matches!(opts.scope, DiffScope::Commits));
        assert_eq!(opts.revision_range.as_deref(), Some("main...HEAD"));
    }

    #[test]
    fn test_diff_options_with_path() {
        // Use the builder pattern since from_args treats bare paths as revision ranges
        let opts = DiffOptions::new().path("src/main.rs".to_string());
        assert_eq!(opts.path_filter.as_deref(), Some("src/main.rs"));
    }

    #[test]
    fn test_diff_options_context_lines() {
        let opts = DiffOptions::from_args("-U5");
        assert_eq!(opts.context_lines, Some(5));
    }

    #[test]
    fn test_build_diff_command_working_tree() {
        let opts = DiffOptions::from_args("");
        let cmd = build_diff_command(&opts);
        assert!(cmd.starts_with("git diff"));
    }

    #[test]
    fn test_build_diff_command_staged() {
        let opts = DiffOptions::from_args("--staged");
        let cmd = build_diff_command(&opts);
        assert!(cmd.contains("--staged"));
    }

    #[test]
    fn test_build_diff_command_commit_range() {
        let opts = DiffOptions::from_args("main...HEAD");
        let cmd = build_diff_command(&opts);
        assert!(cmd.contains("main...HEAD"));
    }

    #[test]
    fn test_diff_analyzer_categorize_function() {
        let analyzer = DiffAnalyzer::new();
        let analysis = analyzer.analyze(
            "+fn new_function() {\n\
             +    let x = 1;\n\
             +}\n",
        );
        assert!(analysis.total() > 0);
        assert!(analysis.count(ChangeCategory::Function) > 0);
    }

    #[test]
    fn test_diff_analyzer_categorize_import() {
        let analyzer = DiffAnalyzer::new();
        let analysis = analyzer.analyze(
            "+use std::collections::HashMap;\n\
             +import os from 'os';\n",
        );
        assert!(analysis.total() > 0);
        assert!(analysis.count(ChangeCategory::Import) > 0);
    }

    #[test]
    fn test_diff_analyzer_categorize_test() {
        let analyzer = DiffAnalyzer::new();
        let analysis = analyzer.analyze(
            "+#[test]\n\
             +fn test_something() {\n\
             +    assert!(true);\n\
             +}\n",
        );
        assert!(analysis.has_test_changes());
    }

    #[test]
    fn test_diff_analyzer_empty() {
        let analyzer = DiffAnalyzer::new();
        let analysis = analyzer.analyze("");
        assert_eq!(analysis.total(), 0);
        assert!(!analysis.has_test_changes());
    }

    #[test]
    fn test_diff_analyzer_summary() {
        let analyzer = DiffAnalyzer::new();
        let analysis = analyzer.analyze(
            "+fn foo() {}\n\
             -fn bar() {}\n",
        );
        let summary = analysis.summary();
        assert!(!summary.is_empty());
    }
}

// ── Config Utils Tests ───────────────────────────────────────────

mod config_tests {
    use shannon_commands::config_utils::{
        ConfigAction, format_config_get, format_config_list, format_config_reset,
        format_config_set, parse_config_action,
    };

    #[test]
    fn test_parse_config_action_list() {
        // Empty or unknown args default to List
        assert!(matches!(parse_config_action("unknown"), ConfigAction::List));
        assert!(matches!(parse_config_action("list"), ConfigAction::List));
    }

    #[test]
    fn test_parse_config_action_get() {
        assert!(matches!(parse_config_action("get"), ConfigAction::Get));
    }

    #[test]
    fn test_parse_config_action_set() {
        assert!(matches!(parse_config_action("set"), ConfigAction::Set));
    }

    #[test]
    fn test_parse_config_action_reset() {
        assert!(matches!(parse_config_action("reset"), ConfigAction::Reset));
    }

    #[test]
    fn test_parse_config_action_help() {
        assert!(matches!(parse_config_action("help"), ConfigAction::Help));
    }

    #[test]
    fn test_format_config_list_not_empty() {
        let output = format_config_list();
        assert!(!output.is_empty());
        assert!(output.contains("model"));
        assert!(output.contains("temperature"));
    }

    #[test]
    fn test_format_config_get() {
        let output = format_config_get("model");
        assert!(output.contains("model"));
    }

    #[test]
    fn test_format_config_set() {
        let output = format_config_set("model", "gpt-4o");
        assert!(output.contains("model"));
        assert!(output.contains("gpt-4o"));
    }

    #[test]
    fn test_format_config_reset() {
        let output = format_config_reset("model");
        assert!(output.contains("model"));
    }
}

// ── Debug Utils Tests ────────────────────────────────────────────

mod debug_tests {
    use shannon_commands::debug_utils::{
        DebugSubcommand, LogLevel, format_debug_help, format_system_info, parse_debug_subcommand,
        parse_log_level,
    };

    #[test]
    fn test_parse_debug_subcommand_help() {
        // Unknown/empty args map to Help
        assert!(matches!(
            parse_debug_subcommand("unknown"),
            DebugSubcommand::Help
        ));
        assert!(matches!(
            parse_debug_subcommand("help"),
            DebugSubcommand::Help
        ));
    }

    #[test]
    fn test_parse_debug_subcommand_info() {
        assert!(matches!(
            parse_debug_subcommand("info"),
            DebugSubcommand::Info
        ));
    }

    #[test]
    fn test_parse_debug_subcommand_log() {
        assert!(matches!(
            parse_debug_subcommand("log"),
            DebugSubcommand::Log
        ));
    }

    #[test]
    fn test_parse_debug_subcommand_profile() {
        assert!(matches!(
            parse_debug_subcommand("profile"),
            DebugSubcommand::Profile
        ));
    }

    #[test]
    fn test_parse_debug_subcommand_trace() {
        assert!(matches!(
            parse_debug_subcommand("trace"),
            DebugSubcommand::Trace
        ));
    }

    #[test]
    fn test_parse_log_level_valid() {
        assert!(matches!(parse_log_level("trace"), Some(LogLevel::Trace)));
        assert!(matches!(parse_log_level("debug"), Some(LogLevel::Debug)));
        assert!(matches!(parse_log_level("info"), Some(LogLevel::Info)));
        assert!(matches!(parse_log_level("warn"), Some(LogLevel::Warn)));
        assert!(matches!(parse_log_level("error"), Some(LogLevel::Error)));
    }

    #[test]
    fn test_parse_log_level_case_insensitive() {
        assert!(matches!(parse_log_level("INFO"), Some(LogLevel::Info)));
        assert!(matches!(parse_log_level("Warn"), Some(LogLevel::Warn)));
    }

    #[test]
    fn test_parse_log_level_invalid() {
        assert!(parse_log_level("invalid").is_none());
        assert!(parse_log_level("").is_none());
    }

    #[test]
    fn test_format_debug_help_not_empty() {
        let output = format_debug_help();
        assert!(!output.is_empty());
        assert!(output.contains("/debug"));
    }

    #[test]
    fn test_format_system_info() {
        let output = format_system_info();
        assert!(!output.is_empty());
        assert!(output.contains("OS:"));
    }

    #[test]
    fn test_log_level_display() {
        assert_eq!(format!("{}", LogLevel::Trace), "trace");
        assert_eq!(format!("{}", LogLevel::Info), "info");
        assert_eq!(format!("{}", LogLevel::Error), "error");
    }
}

// ── PDF Utils Tests ──────────────────────────────────────────────

mod pdf_tests {
    use shannon_commands::pdf_utils::{
        ImageFormat, PdfContent, PdfMetadata, PdfOptions, PdfPage, PdfTable, get_pdf_prompt,
    };

    #[test]
    fn test_pdf_options_default() {
        let opts = PdfOptions::default();
        assert!(opts.pages.is_none());
        assert!(!opts.extract_images);
        assert!(!opts.use_ocr);
        assert!(!opts.extract_tables);
    }

    #[test]
    fn test_pdf_metadata_creation() {
        let meta = PdfMetadata {
            title: Some("Test PDF".to_string()),
            author: Some("Test Author".to_string()),
            page_count: 10,
            ..Default::default()
        };
        assert_eq!(meta.title.as_deref(), Some("Test PDF"));
        assert_eq!(meta.page_count, 10);
    }

    #[test]
    fn test_pdf_content_from_ai_output() {
        let ai_output = "### Document Metadata\n- **Title**: Test Document\n- **Pages**: 5\n\n### Key Findings\n- Finding one\n";
        let content = PdfContent::from_ai_output("test.pdf", ai_output);
        assert_eq!(content.source_path, "test.pdf");
        assert_eq!(content.total_pages, 5);
        assert_eq!(content.metadata.title.as_deref(), Some("Test Document"));
    }

    #[test]
    fn test_pdf_page_creation() {
        let page = PdfPage::new(1, "Hello world".to_string());
        assert_eq!(page.number, 1);
        assert_eq!(page.text, "Hello world");
        assert!(page.images.is_empty());
        assert!(page.tables.is_empty());
    }

    #[test]
    fn test_pdf_image_format_properties() {
        // ImageFormat does not derive Serialize/Deserialize,
        // so test extension and from_extension instead.
        assert_eq!(ImageFormat::Png.extension(), "png");
        assert_eq!(ImageFormat::Jpeg.extension(), "jpg");
        assert_eq!(ImageFormat::Tiff.extension(), "tiff");
        assert_eq!(ImageFormat::Pnm.extension(), "pnm");
        assert_eq!(ImageFormat::Pdf.extension(), "pdf");

        assert_eq!(ImageFormat::from_extension("png"), Some(ImageFormat::Png));
        assert_eq!(ImageFormat::from_extension("jpg"), Some(ImageFormat::Jpeg));
        assert_eq!(ImageFormat::from_extension("tiff"), Some(ImageFormat::Tiff));
    }

    #[test]
    fn test_pdf_table_creation() {
        let table = PdfTable::new(
            0,
            1,
            vec!["Name".to_string(), "Value".to_string()],
            vec![
                vec!["key1".to_string(), "val1".to_string()],
                vec!["key2".to_string(), "val2".to_string()],
            ],
        );
        assert_eq!(table.headers.len(), 2);
        assert_eq!(table.rows.len(), 2);
        assert_eq!(table.row_count(), 2);
        assert_eq!(table.column_count(), 2);
    }

    #[test]
    fn test_get_pdf_prompt_with_file() {
        let opts = PdfOptions::default();
        let prompt = get_pdf_prompt("test.pdf", &opts);
        assert!(prompt.contains("test.pdf"));
    }
}

// ── Review PR Utils Tests ────────────────────────────────────────

mod review_pr_tests {
    use shannon_commands::review_utils::{
        Assessment, IssueSeverity, ReviewCategory, ReviewIssue, ReviewResult, get_review_prompt,
    };

    #[test]
    fn test_review_categories() {
        let cats = ReviewCategory::all();
        assert!(!cats.is_empty());
        assert!(cats.contains(&ReviewCategory::Correctness));
        assert!(cats.contains(&ReviewCategory::Security));
        assert!(cats.contains(&ReviewCategory::Performance));
    }

    #[test]
    fn test_review_category_display_name() {
        // ReviewCategory does not impl Display; use display_name() method.
        assert_eq!(
            ReviewCategory::Correctness.display_name(),
            "Code Correctness"
        );
        assert_eq!(ReviewCategory::Security.display_name(), "Security");
    }

    #[test]
    fn test_issue_severity_ordering() {
        assert!(IssueSeverity::Critical < IssueSeverity::High);
        assert!(IssueSeverity::High < IssueSeverity::Medium);
        assert!(IssueSeverity::Medium < IssueSeverity::Low);
        assert!(IssueSeverity::Low < IssueSeverity::Info);
    }

    #[test]
    fn test_issue_severity_display_name() {
        // IssueSeverity does not impl Display; use display_name() method.
        assert_eq!(IssueSeverity::Critical.display_name(), "CRITICAL");
        assert_eq!(IssueSeverity::Low.display_name(), "LOW");
    }

    #[test]
    fn test_review_issue_creation() {
        let issue = ReviewIssue::new(
            ReviewCategory::Security,
            IssueSeverity::High,
            "User input not sanitized".to_string(),
        )
        .with_location("src/db.rs:42".to_string())
        .with_suggestion("Use parameterized queries".to_string());

        assert_eq!(issue.category, ReviewCategory::Security);
        assert_eq!(issue.severity, IssueSeverity::High);
        assert_eq!(issue.location.as_deref(), Some("src/db.rs:42"));
        assert!(issue.suggestion.is_some());
    }

    #[test]
    fn test_review_result_creation() {
        let result = ReviewResult::new("PR overview".to_string(), Assessment::NeedsWork)
            .with_pr_number("123".to_string())
            .with_issue(ReviewIssue::new(
                ReviewCategory::Correctness,
                IssueSeverity::Medium,
                "Off-by-one".to_string(),
            ));

        assert_eq!(result.pr_number.as_deref(), Some("123"));
        assert_eq!(result.issues.len(), 1);
        assert!(matches!(result.overall_assessment, Assessment::NeedsWork));
    }

    #[test]
    fn test_review_result_to_markdown() {
        let result = ReviewResult::new("LGTM".to_string(), Assessment::Approve)
            .with_pr_number("42".to_string())
            .with_positive("Good test coverage".to_string());

        let md = result.to_markdown();
        assert!(md.contains("LGTM"));
        assert!(md.contains("Approve"));
        assert!(md.contains("PR #42"));
        assert!(md.contains("Good test coverage"));
    }

    #[test]
    fn test_assessment_variants() {
        assert!(matches!(Assessment::Approve, Assessment::Approve));
        assert!(matches!(Assessment::NeedsWork, Assessment::NeedsWork));
        assert!(matches!(
            Assessment::RequestChanges,
            Assessment::RequestChanges
        ));
        assert!(matches!(
            Assessment::ApproveWithSuggestions,
            Assessment::ApproveWithSuggestions
        ));
    }

    #[test]
    fn test_get_review_prompt_with_number() {
        let prompt = get_review_prompt(Some("42"));
        assert!(prompt.contains("42"));
    }

    #[test]
    fn test_get_review_prompt_no_number() {
        let prompt = get_review_prompt(None);
        assert!(!prompt.is_empty());
        assert!(prompt.contains("No PR number provided"));
    }

    #[test]
    fn test_review_result_empty_issues() {
        let result = ReviewResult::new("Clean PR".to_string(), Assessment::Approve);
        assert!(result.issues.is_empty());
        assert!(result.positives.is_empty());
        assert_eq!(result.pr_number, None);
    }
}

// ── REPL Command Dispatch Tests ──────────────────────────────────

mod repl_command_dispatch_tests {
    use shannon_commands::CommandParser;

    #[test]
    fn test_parse_export_command() {
        let parser = CommandParser::new();
        let parsed = parser.parse("/export json output.json").unwrap();
        assert_eq!(parsed.name, "export");
        assert!(parsed.args.contains("json"));
        assert!(parsed.args.contains("output.json"));
    }

    #[test]
    fn test_parse_search_command() {
        let parser = CommandParser::new();
        let parsed = parser.parse("/search --regex hello.*world").unwrap();
        assert_eq!(parsed.name, "search");
        assert!(parsed.args.contains("regex"));
    }

    #[test]
    fn test_parse_search_alias() {
        // The "hist" alias for the search command.
        // Note: the "?" alias can't be used here because the CommandParser
        // only accepts alphanumeric, hyphen, and underscore in command names.
        let parser = CommandParser::new();
        let parsed = parser.parse("/hist pattern").unwrap();
        assert_eq!(parsed.name, "hist");
    }

    #[test]
    fn test_parse_diff_command() {
        let parser = CommandParser::new();
        let parsed = parser.parse("/diff --staged").unwrap();
        assert_eq!(parsed.name, "diff");
        assert!(parsed.args.contains("staged"));
    }

    #[test]
    fn test_parse_config_command() {
        let parser = CommandParser::new();
        let parsed = parser.parse("/config set model gpt-4o").unwrap();
        assert_eq!(parsed.name, "config");
        assert!(parsed.args.contains("set"));
        assert!(parsed.args.contains("model"));
    }

    #[test]
    fn test_parse_debug_command() {
        let parser = CommandParser::new();
        let parsed = parser.parse("/debug log info").unwrap();
        assert_eq!(parsed.name, "debug");
        assert!(parsed.args.contains("log"));
    }

    #[test]
    fn test_parse_debug_alias() {
        let parser = CommandParser::new();
        let parsed = parser.parse("/dbg info").unwrap();
        assert_eq!(parsed.name, "dbg");
    }

    #[test]
    fn test_parse_browse_command() {
        let parser = CommandParser::new();
        let parsed = parser.parse("/browse /tmp").unwrap();
        assert_eq!(parsed.name, "browse");
        assert!(parsed.args.contains("/tmp"));
    }

    #[test]
    fn test_parse_browse_alias() {
        let parser = CommandParser::new();
        let parsed = parser.parse("/files").unwrap();
        assert_eq!(parsed.name, "files");
    }

    #[test]
    fn test_parse_select_tools() {
        let parser = CommandParser::new();
        let parsed = parser.parse("/select-tools").unwrap();
        assert_eq!(parsed.name, "select-tools");
        assert_eq!(parsed.args, "");
    }

    #[test]
    fn test_parse_tools_alias() {
        let parser = CommandParser::new();
        let parsed = parser.parse("/tools").unwrap();
        assert_eq!(parsed.name, "tools");
    }

    #[test]
    fn test_parse_team_command() {
        let parser = CommandParser::new();
        let parsed = parser.parse("/team create my-team").unwrap();
        assert_eq!(parsed.name, "team");
        assert!(parsed.args.contains("create"));
        assert!(parsed.args.contains("my-team"));
    }

    #[test]
    fn test_parse_team_add() {
        let parser = CommandParser::new();
        let parsed = parser.parse("/team add my-team agent-1").unwrap();
        assert_eq!(parsed.name, "team");
        assert!(parsed.args.contains("add"));
        assert!(parsed.args.contains("agent-1"));
    }

    #[test]
    fn test_parse_team_task() {
        let parser = CommandParser::new();
        let parsed = parser.parse("/team task my-team implement auth").unwrap();
        assert_eq!(parsed.name, "team");
        assert!(parsed.args.contains("task"));
        assert!(parsed.args.contains("implement"));
    }

    #[test]
    fn test_parse_team_status() {
        let parser = CommandParser::new();
        let parsed = parser.parse("/team status my-team").unwrap();
        assert_eq!(parsed.name, "team");
        assert!(parsed.args.contains("status"));
    }

    #[test]
    fn test_parse_team_run() {
        let parser = CommandParser::new();
        let parsed = parser.parse("/team run").unwrap();
        assert_eq!(parsed.name, "team");
        assert!(parsed.args.contains("run"));
    }

    #[test]
    fn test_parse_team_list() {
        let parser = CommandParser::new();
        let parsed = parser.parse("/team list").unwrap();
        assert_eq!(parsed.name, "team");
        assert!(parsed.args.contains("list"));
    }

    #[test]
    fn test_parse_team_shutdown() {
        let parser = CommandParser::new();
        let parsed = parser.parse("/team shutdown").unwrap();
        assert_eq!(parsed.name, "team");
        assert!(parsed.args.contains("shutdown"));
    }
}
