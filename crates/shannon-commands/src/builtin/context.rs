//! /context command - Show context window usage visualization
//!
//! Displays a visual usage bar, pressure level, token breakdown by category
//! (system prompt, tool schemas, conversation), message count, and compaction
//! savings.

use crate::command::{
    Command, CommandAvailability, CommandBase, CommandSource, ExecutionContext, PromptCommand,
};

/// Context usage prompt template (fallback for non-REPL invocation)
const CONTEXT_PROMPT: &str = r##"
Display context window usage information for the current session.

Arguments: {args}
- If args contains "usage", show detailed token usage breakdown
- If args contains "reload", reload project context files
- If args is empty, show a summary of the current context state

Show:
1. Visual usage bar with percentage
2. Pressure level (Low/Normal/High/Critical/Emergency)
3. Token breakdown: system prompt, tool schemas, conversation
4. Message count and conversation turns
5. Last compaction savings if available

Format as a clean, readable summary.
"##;

/// Create the /context command
pub fn command() -> Command {
    Command::Prompt(Box::new(PromptCommand {
        base: CommandBase {
            name: "context".to_string(),
            aliases: vec!["ctx".to_string()],
            description: "Show context window usage visualization and project context info"
                .to_string(),
            has_user_specified_description: false,
            availability: vec![CommandAvailability::All],
            source: CommandSource::Builtin,
            is_enabled: true,
            is_hidden: false,
            argument_hint: Some("[usage|reload]".to_string()),
            when_to_use: Some(
                "To check how full the context window is and see token usage breakdown".to_string(),
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
        content_length: 800,
        arg_names: vec!["subcommand".to_string()],
        allowed_tools: vec![],
        model: None,
        hooks: std::collections::HashMap::new(),
        context: ExecutionContext::Inline,
        agent: None,
        paths: vec![],
        prompt_template: Some(CONTEXT_PROMPT.to_string()),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_command() {
        let cmd = command();
        assert_eq!(cmd.name(), "context");
        assert!(cmd.aliases().contains(&"ctx".to_string()));
    }
}
