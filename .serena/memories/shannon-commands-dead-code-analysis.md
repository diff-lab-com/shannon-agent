# Shannon Commands Dead Code Analysis

## Summary of Investigation (2026-04-07)

Analyzed all dead code in shannon-commands crate as reported by clippy.

## Dead Code by File

### commit.rs
- **DELETE**: `GIT_SAFETY`, `COMMIT_ATTRIBUTION`, `get_prompt_template()`, `get_default_branch()`
  - These are scaffolding for prompt generation that was never integrated
  - Only used in tests, never in production code

### review_pr.rs  
- **DELETE**: `REVIEW_PROMPT`, `get_review_prompt()` - unused scaffolding
- **KEEP_FUTURE_USE**: `ReviewCategory` enum, `Assessment` enum - well-designed types for future PR review feature

### pdf.rs
- **DELETE**: `PDF_PROMPT`, `get_pdf_prompt()` - unused scaffolding
- **KEEP_FUTURE_USE**: `PdfOptions` struct + builder methods, `ImageFormat` enum - useful for PDF processing

### help.rs
- **DELETE**: `HelpCategory` methods (`all()`, `display_name()`, `description()`), `get_command_help()`, `all_help_entries()`, `generate_help()` - all unused scaffolding

### status.rs
- **DELETE**: `parse_git_status()`, `format_status()`, `format_branch_short()`, `format_branch_verbose()` - unused parsing/formatting utilities

### diff.rs
- **DELETE**: `FUNCTION_PATTERN`, `IMPORT_PATTERN`, `STRUCT_PATTERN`, `TEST_PATTERN` - unused pattern module
- **KEEP_FUTURE_USE**: `DiffOptions` struct + methods, `build_diff_command()`, `parse_diff_stat()`, `STATS_REGEX` - useful diff utilities

### executor.rs
- **KEEP_INFRASTRUCTURE**: `CommandExecutor` public API methods - core infrastructure

### command.rs
- **KEEP_PUBLIC_API**: `Executable` trait - public contract for extensibility

## Recommendation Categories

1. **DELETE (22 items)**: Pure scaffolding with no references
2. **KEEP_INFRASTRUCTURE (5 items)**: Core executor infrastructure
3. **KEEP_PUBLIC_API (1 item)**: Executable trait for extensibility
4. **KEEP_FUTURE_USE (13 items)**: Well-designed types for future features
