//! /repomap command - Generate repository map with code symbols

use crate::command::{Command, CommandBase, CommandSource, PromptCommand, ExecutionContext, CommandAvailability};

/// Repository map prompt template
const REPOMAP_PROMPT: &str = r##"
Generate a repository map showing the code structure and symbols.

Arguments: {args}

Steps:
1. Use the repomap tool to analyze the project structure
2. Present a summary showing:
   - Total files, symbols, and lines of code
   - File-by-file breakdown with top-level symbols
   - Symbol kinds (functions, classes, structs, etc.)
   - Line numbers for each symbol

Format the output as a structured overview that helps understand the codebase organization.
"##;

/// Create the /repomap command
pub fn command() -> Command {
    Command::Prompt(Box::new(PromptCommand {
        base: CommandBase {
            name: "repomap".to_string(),
            aliases: vec!["repo-map".to_string(), "map".to_string()],
            description: "Generate repository map with code symbols and structure".to_string(),
            has_user_specified_description: false,
            availability: vec![CommandAvailability::All],
            source: CommandSource::Builtin,
            is_enabled: true,
            is_hidden: false,
            argument_hint: Some("[--max-files N] [--extensions ext1,ext2]".to_string()),
            when_to_use: Some(
                "To understand the codebase structure, see what files exist, and get an overview of symbols".to_string(),
            ),
            version: Some("0.1.0".to_string()),
            disable_model_invocation: false,
            user_invocable: true,
            is_workflow: false,
            immediate: false,
            is_sensitive: false,
            user_facing_name: None,
        },
        progress_message: "Analyzing repository structure...".to_string(),
        content_length: 2000,
        arg_names: vec!["options".to_string()],
        allowed_tools: vec![
            "Bash(find*)".to_string(),
            "Bash(ls*)".to_string(),
            "Bash(wc*)".to_string(),
        ],
        model: None,
        hooks: std::collections::HashMap::new(),
        context: ExecutionContext::Inline,
        agent: None,
        paths: vec![],
        prompt_template: Some(REPOMAP_PROMPT.to_string()),
    }))
}
