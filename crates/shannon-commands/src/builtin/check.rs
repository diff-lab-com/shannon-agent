//! /check command - LSP diagnostic feedback loop
//!
//! Runs compiler/LSP diagnostics after code edits and feeds the results
//! back to the LLM for iterative fixing. Inspired by OpenCode's diagnostic
//! feedback loop feature.

use crate::command::{Command, CommandBase, CommandSource, PromptCommand, ExecutionContext, CommandAvailability};

const CHECK_PROMPT: &str = r##"
Run diagnostics on the codebase and report issues.

Arguments: {args}

## Steps

1. Run `cargo check --workspace 2>&1` to get compiler diagnostics
2. If there are errors, run `cargo check --workspace --message-format=json 2>&1` for structured output
3. Analyze each error/warning
4. For each issue, provide:
   - File and line number
   - Error message
   - Suggested fix (with code snippet if helpful)

## Diagnostic Sources

- **Rust**: `cargo check`, `cargo clippy`
- **TypeScript/JavaScript**: `npx tsc --noEmit`, `npx eslint .`
- **Python**: `python -m py_compile`, `ruff check`
- **Go**: `go vet ./...`
- **General**: Check for `.gitignore` patterns, missing files, broken imports

## Loop Behavior

After running diagnostics:
1. If errors found, fix the most critical errors first
2. Re-run diagnostics after fixes
3. Repeat until clean or user stops
4. Report final status: clean / warnings only / errors remaining

## Output Format

### Diagnostics Summary
- Total errors: N
- Total warnings: N
- Files affected: N

### Issues
For each:
```
[SEVERITY] file.rs:LINE
  Message
  Suggestion: fix description
```

### Fix Plan
If errors, propose the fix order (dependencies matter).

Keep output concise — summarize groups of similar errors.
"##;

/// Create the /check command
pub fn command() -> Command {
    Command::Prompt(Box::new(PromptCommand {
        base: CommandBase {
            name: "check".to_string(),
            aliases: vec!["diagnostics".to_string(), "diag".to_string(), "lint".to_string()],
            description: "Run diagnostics and fix issues iteratively".to_string(),
            has_user_specified_description: false,
            availability: vec![CommandAvailability::All],
            source: CommandSource::Builtin,
            is_enabled: true,
            is_hidden: false,
            argument_hint: Some("[file or path]".to_string()),
            when_to_use: Some(
                "Check for compilation errors and fix them automatically".to_string(),
            ),
            version: Some("0.1.0".to_string()),
            disable_model_invocation: false,
            user_invocable: true,
            is_workflow: false,
            immediate: false,
            is_sensitive: false,
            user_facing_name: None,
        },
        progress_message: "Running diagnostics...".to_string(),
        content_length: 2500,
        arg_names: vec!["target".to_string()],
        allowed_tools: vec![
            "Bash(cargo check:*)".to_string(),
            "Bash(cargo clippy:*)".to_string(),
            "Bash(cargo test:*)".to_string(),
            "Bash(npx tsc:*)".to_string(),
            "Bash(npx eslint:*)".to_string(),
            "Bash(go vet:*)".to_string(),
            "Read".to_string(),
            "Edit".to_string(),
            "Write".to_string(),
        ],
        model: None,
        hooks: std::collections::HashMap::new(),
        context: ExecutionContext::Inline,
        agent: None,
        paths: vec![],
        prompt_template: Some(CHECK_PROMPT.to_string()),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_command() {
        let cmd = command();
        assert_eq!(cmd.name(), "check");
        assert!(cmd.aliases().contains(&"diag".to_string()));
    }
}
