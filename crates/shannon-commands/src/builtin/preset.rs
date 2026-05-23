//! /preset command - Conversation templates/presets
//!
//! Provides pre-configured conversation starters for common workflows
//! (code review, refactoring, debugging, etc.) with custom system prompts,
//! initial messages, and model settings.
//!
//! Built-in presets can be overridden by user-defined presets in `.shannon.toml`:
//!
//! ```toml
//! [presets.my-custom-preset]
//! description = "My custom workflow"
//! system_prompt = "You are a specialist in..."
//! initial_message = "Let's get started..."
//! model = "claude-sonnet-4"
//! temperature = 0.7
//!
//! [presets.performance-review]
//! system_prompt = "Focus on performance bottlenecks..."
//! tools = ["Read", "Grep", "Bash"]
//! ```

use crate::command::{
    Command, CommandAvailability, CommandBase, CommandSource, ExecutionContext, PromptCommand,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A conversation preset with pre-configured settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationPreset {
    /// Custom system prompt addition (appended to base system prompt).
    pub system_prompt: Option<String>,
    /// Initial message to send as the user's first message.
    pub initial_message: Option<String>,
    /// Model override for this preset.
    pub model: Option<String>,
    /// Temperature override.
    pub temperature: Option<f32>,
    /// Max tokens override.
    pub max_tokens: Option<usize>,
    /// Tools to enable (whitelist). None means all tools.
    pub tools: Option<Vec<String>>,
    /// Description shown in /preset list.
    pub description: Option<String>,
}

impl Default for ConversationPreset {
    fn default() -> Self {
        Self {
            system_prompt: None,
            initial_message: None,
            model: None,
            temperature: None,
            max_tokens: None,
            tools: None,
            description: None,
        }
    }
}

/// Returns all built-in presets.
pub fn builtin_presets() -> HashMap<String, ConversationPreset> {
    let mut presets = HashMap::new();

    presets.insert(
        "code-review".to_string(),
        ConversationPreset {
            system_prompt: Some(
                "You are performing a thorough code review. Focus on: bugs, security \
                 vulnerabilities, performance issues, code style, and maintainability. \
                 Be specific about line numbers and suggest concrete fixes."
                    .to_string(),
            ),
            initial_message: Some(
                "I'll review the code you share. Please provide the file path or paste \
                 the code you'd like reviewed."
                    .to_string(),
            ),
            model: None,
            temperature: Some(0.3),
            max_tokens: None,
            tools: Some(vec![
                "Read".to_string(),
                "Grep".to_string(),
                "Glob".to_string(),
            ]),
            description: Some(
                "Thorough code review focused on bugs, security, and quality".to_string(),
            ),
        },
    );

    presets.insert(
        "refactor".to_string(),
        ConversationPreset {
            system_prompt: Some(
                "You are a refactoring specialist. Analyze code structure, identify \
                 improvement opportunities, and apply clean code principles. Prefer \
                 small, safe transformations. Always preserve existing behavior."
                    .to_string(),
            ),
            initial_message: Some(
                "I'll help refactor your code. Share the file path and describe what \
                 you'd like to improve, or I can suggest improvements after reading the code."
                    .to_string(),
            ),
            model: None,
            temperature: Some(0.2),
            max_tokens: None,
            tools: None,
            description: Some("Code refactoring with clean code principles".to_string()),
        },
    );

    presets.insert(
        "debug".to_string(),
        ConversationPreset {
            system_prompt: Some(
                "You are a debugging specialist. Use systematic root cause analysis: \
                 reproduce the issue, form hypotheses, test them one at a time. Always \
                 explain your reasoning before making changes."
                    .to_string(),
            ),
            initial_message: Some(
                "I'll help debug the issue. Please describe the problem, expected vs \
                 actual behavior, and any error messages."
                    .to_string(),
            ),
            model: None,
            temperature: Some(0.1),
            max_tokens: None,
            tools: None,
            description: Some("Systematic debugging with root cause analysis".to_string()),
        },
    );

    presets.insert(
        "explain".to_string(),
        ConversationPreset {
            system_prompt: Some(
                "You are a code explanation specialist. Explain code clearly and \
                 concisely, adapting to the reader's level. Use analogies where \
                 helpful. Focus on WHY, not just WHAT."
                    .to_string(),
            ),
            initial_message: Some(
                "I'll explain the code you're interested in. Share a file path or \
                 paste code and tell me what you'd like to understand."
                    .to_string(),
            ),
            model: None,
            temperature: Some(0.5),
            max_tokens: None,
            tools: Some(vec![
                "Read".to_string(),
                "Grep".to_string(),
                "Glob".to_string(),
            ]),
            description: Some("Code explanation adapted to your level".to_string()),
        },
    );

    presets.insert(
        "test".to_string(),
        ConversationPreset {
            system_prompt: Some(
                "You are a test engineering specialist. Write thorough tests covering \
                 happy paths, edge cases, and error conditions. Follow the project's \
                 existing test patterns and conventions."
                    .to_string(),
            ),
            initial_message: Some(
                "I'll help write tests. Share the file path you want to test and \
                 I'll analyze the code and create comprehensive test coverage."
                    .to_string(),
            ),
            model: None,
            temperature: Some(0.2),
            max_tokens: None,
            tools: None,
            description: Some("Test engineering with comprehensive coverage".to_string()),
        },
    );

    presets
}

/// Merge built-in presets with user-defined presets from config.
/// User presets override built-in presets of the same name.
pub fn merge_presets(
    builtin: &HashMap<String, ConversationPreset>,
    user: &HashMap<String, ConversationPreset>,
) -> HashMap<String, ConversationPreset> {
    let mut merged = builtin.clone();
    for (name, preset) in user {
        merged.insert(name.clone(), preset.clone());
    }
    merged
}

/// Format a preset listing for display.
pub fn format_preset_list(presets: &HashMap<String, ConversationPreset>) -> String {
    let mut names: Vec<&String> = presets.keys().collect();
    names.sort();

    let mut output = String::from("Available Presets:\n\n");

    for name in &names {
        let preset = &presets[*name];
        let desc = preset
            .description
            .as_deref()
            .unwrap_or("No description");
        output.push_str(&format!("  {} - {}\n", name, desc));

        if let Some(ref model) = preset.model {
            output.push_str(&format!("    model: {}\n", model));
        }
        if let Some(temp) = preset.temperature {
            output.push_str(&format!("    temperature: {}\n", temp));
        }
        if let Some(ref tools) = preset.tools {
            output.push_str(&format!("    tools: {}\n", tools.join(", ")));
        }
    }

    output.push_str("\nUsage:\n");
    output.push_str("  /preset              - List all presets\n");
    output.push_str("  /preset <name>       - Apply a preset\n");
    output.push_str("  /preset show <name>  - Show preset details\n");

    output
}

/// Format detailed information about a single preset.
pub fn format_preset_detail(name: &str, preset: &ConversationPreset) -> String {
    let mut output = format!("Preset: {}\n", name);
    output.push_str(&format!(
        "  Description: {}\n",
        preset
            .description
            .as_deref()
            .unwrap_or("No description")
    ));

    if let Some(ref sp) = preset.system_prompt {
        output.push_str(&format!("  System prompt: {}\n", sp));
    }
    if let Some(ref msg) = preset.initial_message {
        output.push_str(&format!("  Initial message: {}\n", msg));
    }
    if let Some(ref model) = preset.model {
        output.push_str(&format!("  Model: {}\n", model));
    }
    if let Some(temp) = preset.temperature {
        output.push_str(&format!("  Temperature: {}\n", temp));
    }
    if let Some(tokens) = preset.max_tokens {
        output.push_str(&format!("  Max tokens: {}\n", tokens));
    }
    if let Some(ref tools) = preset.tools {
        output.push_str(&format!("  Tools: {}\n", tools.join(", ")));
    }

    output
}

/// Preset prompt template
const PRESET_PROMPT: &str = r##"
Apply or manage conversation presets (pre-configured session templates).

Arguments: {args}

## Actions

- **No args** or **list** — Show all available presets
- **show <name>** — Display details for a specific preset
- **<name>** — Apply the named preset to the current session

## How Presets Work

When you apply a preset, the following changes are made to the current session:
1. **system_prompt** — Appended to the current system prompt
2. **model** — Switches the LLM model for this session
3. **temperature** — Adjusts response randomness
4. **tools** — Restricts which tools are available
5. **initial_message** — Injected as the user's first message

## Built-in Presets

- **code-review** — Thorough code review focused on bugs, security, and quality
- **refactor** — Code refactoring with clean code principles
- **debug** — Systematic debugging with root cause analysis
- **explain** — Code explanation adapted to your level
- **test** — Test engineering with comprehensive coverage

## Custom Presets

Add custom presets to `.shannon.toml`:

```toml
[presets.my-workflow]
description = "My custom workflow"
system_prompt = "You are a specialist in..."
initial_message = "Let's get started..."
model = "claude-sonnet-4"
temperature = 0.7
tools = ["Read", "Grep", "Bash"]
```

User presets override built-in presets of the same name.
"##;

/// Create the /preset command
pub fn command() -> Command {
    Command::Prompt(Box::new(PromptCommand {
        base: CommandBase {
            name: "preset".to_string(),
            aliases: vec!["template".to_string(), "profile".to_string()],
            description: "Apply conversation presets for specialized sessions".to_string(),
            has_user_specified_description: false,
            availability: vec![CommandAvailability::All],
            source: CommandSource::Builtin,
            is_enabled: true,
            is_hidden: false,
            argument_hint: Some("[list|show|<name>]".to_string()),
            when_to_use: Some(
                "Use to start a specialized session (code review, debugging, refactoring, etc.)"
                    .to_string(),
            ),
            version: Some("0.1.0".to_string()),
            disable_model_invocation: false,
            user_invocable: true,
            is_workflow: false,
            immediate: false,
            is_sensitive: false,
            user_facing_name: None,
        },
        progress_message: "Loading preset...".to_string(),
        content_length: 3000,
        arg_names: vec!["action".to_string(), "name".to_string()],
        allowed_tools: vec![
            "Read".to_string(),
            "Glob".to_string(),
            "Grep".to_string(),
        ],
        model: None,
        hooks: HashMap::new(),
        context: ExecutionContext::Inline,
        agent: None,
        paths: vec![],
        prompt_template: Some(PRESET_PROMPT.to_string()),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_preset_command() {
        let cmd = command();
        assert_eq!(cmd.name(), "preset");
        assert!(cmd.aliases().contains(&"template".to_string()));
        assert!(cmd.aliases().contains(&"profile".to_string()));
    }

    #[test]
    fn test_builtin_presets_loaded() {
        let presets = builtin_presets();
        assert_eq!(presets.len(), 5);
        assert!(presets.contains_key("code-review"));
        assert!(presets.contains_key("refactor"));
        assert!(presets.contains_key("debug"));
        assert!(presets.contains_key("explain"));
        assert!(presets.contains_key("test"));
    }

    #[test]
    fn test_builtin_preset_has_required_fields() {
        let presets = builtin_presets();
        for (name, preset) in &presets {
            assert!(
                preset.system_prompt.is_some() || preset.initial_message.is_some(),
                "Preset '{}' must have system_prompt or initial_message",
                name
            );
            assert!(
                preset.description.is_some(),
                "Preset '{}' must have a description",
                name
            );
        }
    }

    #[test]
    fn test_preset_serialization_roundtrip() {
        let original = ConversationPreset {
            system_prompt: Some("You are a specialist.".to_string()),
            initial_message: Some("Let's begin.".to_string()),
            model: Some("claude-sonnet-4".to_string()),
            temperature: Some(0.7),
            max_tokens: Some(4096),
            tools: Some(vec!["Read".to_string(), "Grep".to_string()]),
            description: Some("Test preset".to_string()),
        };

        let json = serde_json::to_string(&original).unwrap();
        let restored: ConversationPreset = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.system_prompt, original.system_prompt);
        assert_eq!(restored.initial_message, original.initial_message);
        assert_eq!(restored.model, original.model);
        assert_eq!(restored.temperature, original.temperature);
        assert_eq!(restored.max_tokens, original.max_tokens);
        assert_eq!(restored.tools, original.tools);
        assert_eq!(restored.description, original.description);
    }

    #[test]
    fn test_preset_merge_builtin_with_user() {
        let builtin = builtin_presets();

        // User overrides "debug" preset
        let mut user = HashMap::new();
        user.insert(
            "debug".to_string(),
            ConversationPreset {
                system_prompt: Some("Custom debugger.".to_string()),
                temperature: Some(0.5),
                ..Default::default()
            },
        );
        // User adds a new preset
        user.insert(
            "my-custom".to_string(),
            ConversationPreset {
                system_prompt: Some("Custom workflow.".to_string()),
                description: Some("My custom workflow".to_string()),
                ..Default::default()
            },
        );

        let merged = merge_presets(&builtin, &user);

        // User's debug overrides builtin
        assert_eq!(merged.len(), 6);
        assert_eq!(
            merged["debug"].system_prompt.as_deref(),
            Some("Custom debugger.")
        );
        assert_eq!(merged["debug"].temperature, Some(0.5));
        // Custom preset exists
        assert!(merged.contains_key("my-custom"));
        // Other builtins unchanged
        assert!(merged.contains_key("code-review"));
    }

    #[test]
    fn test_preset_default_values() {
        let preset = ConversationPreset::default();
        assert!(preset.system_prompt.is_none());
        assert!(preset.initial_message.is_none());
        assert!(preset.model.is_none());
        assert!(preset.temperature.is_none());
        assert!(preset.max_tokens.is_none());
        assert!(preset.tools.is_none());
        assert!(preset.description.is_none());
    }

    #[test]
    fn test_preset_list_formatting() {
        let presets = builtin_presets();
        let output = format_preset_list(&presets);

        assert!(output.contains("Available Presets"));
        assert!(output.contains("code-review"));
        assert!(output.contains("refactor"));
        assert!(output.contains("debug"));
        assert!(output.contains("explain"));
        assert!(output.contains("test"));
        assert!(output.contains("/preset"));
        assert!(output.contains("temperature:"));
    }

    #[test]
    fn test_preset_detail_formatting() {
        let preset = ConversationPreset {
            system_prompt: Some("You are a specialist.".to_string()),
            initial_message: Some("Let's begin.".to_string()),
            model: Some("gpt-4o".to_string()),
            temperature: Some(0.7),
            max_tokens: Some(4096),
            tools: Some(vec!["Read".to_string()]),
            description: Some("Test preset".to_string()),
        };

        let detail = format_preset_detail("my-preset", &preset);
        assert!(detail.contains("Preset: my-preset"));
        assert!(detail.contains("Test preset"));
        assert!(detail.contains("You are a specialist."));
        assert!(detail.contains("gpt-4o"));
        assert!(detail.contains("0.7"));
        assert!(detail.contains("4096"));
        assert!(detail.contains("Read"));
    }

    #[test]
    fn test_preset_detail_minimal() {
        let preset = ConversationPreset {
            description: Some("Minimal preset".to_string()),
            ..Default::default()
        };
        let detail = format_preset_detail("minimal", &preset);
        assert!(detail.contains("Preset: minimal"));
        assert!(detail.contains("Minimal preset"));
        assert!(!detail.contains("Model:"));
        assert!(!detail.contains("Temperature:"));
    }

    #[test]
    fn test_preset_toml_parsing() {
        // Simulate what serde would parse from TOML [presets.*] sections
        let toml_str = r#"
            system_prompt = "Focus on performance"
            initial_message = "Let's optimize"
            model = "claude-sonnet-4"
            temperature = 0.5
            max_tokens = 8192
            tools = ["Read", "Grep", "Bash"]
            description = "Performance review preset"
        "#;

        let preset: ConversationPreset = toml::from_str(toml_str).unwrap();
        assert_eq!(
            preset.system_prompt.as_deref(),
            Some("Focus on performance")
        );
        assert_eq!(preset.initial_message.as_deref(), Some("Let's optimize"));
        assert_eq!(preset.model.as_deref(), Some("claude-sonnet-4"));
        assert_eq!(preset.temperature, Some(0.5));
        assert_eq!(preset.max_tokens, Some(8192));
        assert_eq!(
            preset.tools,
            Some(vec![
                "Read".to_string(),
                "Grep".to_string(),
                "Bash".to_string()
            ])
        );
        assert_eq!(
            preset.description.as_deref(),
            Some("Performance review preset")
        );
    }

    #[test]
    fn test_preset_partial_toml_parsing() {
        let toml_str = r#"
            system_prompt = "Quick review"
        "#;
        let preset: ConversationPreset = toml::from_str(toml_str).unwrap();
        assert_eq!(preset.system_prompt.as_deref(), Some("Quick review"));
        assert!(preset.initial_message.is_none());
        assert!(preset.model.is_none());
        assert!(preset.temperature.is_none());
        assert!(preset.max_tokens.is_none());
        assert!(preset.tools.is_none());
        assert!(preset.description.is_none());
    }

    #[test]
    fn test_preset_save_creates_toml() {
        let preset = ConversationPreset {
            system_prompt: Some("Custom workflow".to_string()),
            description: Some("My workflow".to_string()),
            temperature: Some(0.5),
            ..Default::default()
        };

        // Serialize to TOML to verify it produces valid output
        let toml_str = toml::to_string(&preset).unwrap();
        assert!(toml_str.contains("system_prompt"));
        assert!(toml_str.contains("Custom workflow"));
        assert!(toml_str.contains("temperature"));

        // Round-trip back
        let restored: ConversationPreset = toml::from_str(&toml_str).unwrap();
        assert_eq!(restored.system_prompt, preset.system_prompt);
        assert_eq!(restored.description, preset.description);
        assert_eq!(restored.temperature, preset.temperature);
    }

    #[test]
    fn test_preset_command_is_prompt_variant() {
        let cmd = command();
        match cmd {
            Command::Prompt(pc) => {
                assert_eq!(pc.base.name, "preset");
                assert!(pc.base.user_invocable);
                assert!(!pc.base.is_hidden);
                assert!(pc.prompt_template.is_some());
                assert!(!pc.allowed_tools.is_empty());
            }
            _ => panic!("Expected Prompt command"),
        }
    }

    #[test]
    fn test_code_review_preset_read_only_tools() {
        let presets = builtin_presets();
        let review = &presets["code-review"];
        let tools = review.tools.as_ref().unwrap();
        // All tools should be read-only
        assert!(tools.contains(&"Read".to_string()));
        assert!(tools.contains(&"Grep".to_string()));
        assert!(tools.contains(&"Glob".to_string()));
        assert!(!tools.contains(&"Edit".to_string()));
        assert!(!tools.contains(&"Write".to_string()));
        assert!(!tools.contains(&"Bash".to_string()));
    }

    #[test]
    fn test_debug_preset_low_temperature() {
        let presets = builtin_presets();
        let debug = &presets["debug"];
        assert_eq!(debug.temperature, Some(0.1));
    }

    #[test]
    fn test_explain_preset_higher_temperature() {
        let presets = builtin_presets();
        let explain = &presets["explain"];
        assert_eq!(explain.temperature, Some(0.5));
    }
}
