//! /outline command - Show symbol outline for a file

use crate::command::{Command, CommandBase, CommandSource, PromptCommand, ExecutionContext, CommandAvailability};

/// File outline prompt template
const OUTLINE_PROMPT: &str = r##"
Show the symbol outline for a source file.

Arguments: {args}

If a file path is provided:
1. Parse the file and extract symbols (functions, classes, structs, etc.)
2. Show symbol names, kinds, and line numbers
3. Include nested symbols (e.g., methods in classes)

If no file is provided:
1. Ask the user which file to outline
2. Or suggest common entry points (main.rs, lib.rs, index.js, etc.)

Format the output as a structured outline with indentation showing symbol hierarchy.
"##;

/// Create the /outline command
pub fn command() -> Command {
    Command::Prompt(Box::new(PromptCommand {
        base: CommandBase {
            name: "outline".to_string(),
            aliases: vec!["symbols".to_string(), "sym".to_string()],
            description: "Show symbol outline for a source file".to_string(),
            has_user_specified_description: false,
            availability: vec![CommandAvailability::All],
            source: CommandSource::Builtin,
            is_enabled: true,
            is_hidden: false,
            argument_hint: Some("[file-path]".to_string()),
            when_to_use: Some(
                "To see the structure of a file - functions, classes, and their locations".to_string(),
            ),
            version: Some("0.1.0".to_string()),
            disable_model_invocation: false,
            user_invocable: true,
            is_workflow: false,
            immediate: false,
            is_sensitive: false,
            user_facing_name: None,
        },
        progress_message: "Analyzing file structure...".to_string(),
        content_length: 1000,
        arg_names: vec!["file-path".to_string()],
        allowed_tools: vec![
            "Bash(cat*)".to_string(),
            "Bash(head*)".to_string(),
        ],
        model: None,
        hooks: std::collections::HashMap::new(),
        context: ExecutionContext::Inline,
        agent: None,
        paths: vec![],
        prompt_template: Some(OUTLINE_PROMPT.to_string()),
    }))
}
