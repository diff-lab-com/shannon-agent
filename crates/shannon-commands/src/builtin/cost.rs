//! /cost command - Show token usage and cost tracking

use crate::command::{Command, CommandBase, CommandSource, PromptCommand, ExecutionContext, CommandAvailability};

/// Cost prompt template
const COST_PROMPT: &str = r##"
Display the current session's token usage and cost information.

Arguments: {args}
- If args contains "--detailed", show per-turn breakdown
- If args contains "--reset", reset the cost counter (with confirmation)

Show:
1. Total tokens used (input + output)
2. Total estimated cost in USD
3. Current model and its pricing
4. Context window usage percentage
5. Budget limit (if set) and remaining budget

Format the output as a clean, readable summary.
"##;

/// Create the /cost command
pub fn command() -> Command {
    Command::Prompt(Box::new(PromptCommand {
        base: CommandBase {
            name: "cost".to_string(),
            aliases: vec!["usage".to_string(), "tokens".to_string()],
            description: "Show token usage and cost tracking for the current session".to_string(),
            has_user_specified_description: false,
            availability: vec![CommandAvailability::All],
            source: CommandSource::Builtin,
            is_enabled: true,
            is_hidden: false,
            argument_hint: Some("[--detailed] [--reset]".to_string()),
            when_to_use: Some(
                "To check how many tokens have been used and the estimated cost of the session".to_string(),
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
        arg_names: vec!["options".to_string()],
        allowed_tools: vec![],
        model: None,
        hooks: std::collections::HashMap::new(),
        context: ExecutionContext::Inline,
        agent: None,
        paths: vec![],
        prompt_template: Some(COST_PROMPT.to_string()),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cost_command() {
        let cmd = command();
        assert_eq!(cmd.name(), "cost");
        assert!(cmd.aliases().contains(&"usage".to_string()));
        assert!(cmd.aliases().contains(&"tokens".to_string()));
    }
}
