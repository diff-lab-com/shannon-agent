//! /effort command - Control model reasoning depth

use crate::command::{
    Command, CommandAvailability, CommandBase, CommandSource, ExecutionContext, PromptCommand,
};

const EFFORT_PROMPT: &str = r##"
Control the model's reasoning depth and effort level.

Arguments: {args}

## Levels

| Level | Description | Use Case |
|-------|-------------|----------|
| low | Quick responses, minimal reasoning | Simple questions, formatting |
| medium | Balanced reasoning (default) | Normal coding tasks |
| high | Deep analysis, thorough reasoning | Architecture, debugging |
| max | Maximum reasoning effort | Critical decisions, complex problems |

## Actions

- **No args** or **show** — Display current effort level
- **low/medium/high/max** — Set the effort level
- **reset** — Reset to default (medium)

## Implementation

Use `/config set effort_level <level>` to persist the setting.
The effort level controls how many tokens the model allocates to internal reasoning:
- Low: ~10% of context budget for reasoning
- Medium: ~25% of context budget
- High: ~50% of context budget
- Max: ~80% of context budget (extended thinking)

This maps to provider-specific parameters:
- OpenAI: `reasoning_effort` parameter
- Anthropic: `budget_tokens` for extended thinking
- Others: May not support all levels

Example: `/effort high` or `/config set effort_level high`
"##;

/// Create the /effort command
pub fn command() -> Command {
    Command::Prompt(Box::new(PromptCommand {
        base: CommandBase {
            name: "effort".to_string(),
            aliases: vec!["reasoning".to_string(), "think".to_string()],
            description: "Control model reasoning depth (low/medium/high/max)".to_string(),
            has_user_specified_description: false,
            availability: vec![CommandAvailability::All],
            source: CommandSource::Builtin,
            is_enabled: true,
            is_hidden: false,
            argument_hint: Some("[low|medium|high|max|show|reset]".to_string()),
            when_to_use: Some("Adjust how deeply the model reasons before responding".to_string()),
            version: Some("0.1.0".to_string()),
            disable_model_invocation: false,
            user_invocable: true,
            is_workflow: false,
            immediate: false,
            is_sensitive: false,
            user_facing_name: None,
        },
        progress_message: "".to_string(),
        content_length: 800,
        arg_names: vec!["level".to_string()],
        allowed_tools: vec![],
        model: None,
        hooks: std::collections::HashMap::new(),
        context: ExecutionContext::Inline,
        agent: None,
        paths: vec![],
        prompt_template: Some(EFFORT_PROMPT.to_string()),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_effort_command() {
        let cmd = command();
        assert_eq!(cmd.name(), "effort");
        assert!(cmd.aliases().contains(&"think".to_string()));
    }
}
