//! /theme command - List, preview, and switch color themes

use crate::command::{Command, CommandBase, CommandSource, PromptCommand, ExecutionContext, CommandAvailability};

const THEME_PROMPT: &str = r##"
Manage Shannon Code color themes.

Arguments: {args}

## Subcommands

- **list** (or no args) — Show all available themes with a brief description
- **preview <name>** — Show a color swatch preview of a specific theme
- **set <name>** — Switch to a theme (persists across sessions)
- **current** — Show the currently active theme
- **reset** — Reset to auto-detected theme

## Built-in Themes

| Theme | Description |
|-------|-------------|
| dark | Default dark theme (blue/purple tones) |
| light | Light theme for bright terminals |
| dracula | Dracula-inspired (purple/green/pink) |
| tokyonight | Tokyo Night (blue/cyan/green) |
| catppuccin_mocha | Catppuccin Mocha (warm pastels) |
| gruvbox_dark | Gruvbox Dark (warm earthy tones) |
| nord | Nord (cool blue-gray palette) |
| kanagawa | Kanagawa (Japanese ink-inspired) |
| monokai | Monokai (vivid classic editor) |
| onedark | One Dark (Atom-inspired) |
| everforest | Everforest (nature-inspired, easy on eyes) |
| ayu | Ayu Dark (deep dark with vibrant accents) |
| flexoki | Flexoki (warm, paper-like tones) |
| dark_daltonized | Dark theme adapted for colorblind users |
| light_daltonized | Light theme adapted for colorblind users |

## Custom Themes

Users can create custom themes in `~/.shannon/themes/<name>.json`.
Each field is an optional hex color string (e.g., "#7E9CD8").
Fields not specified inherit from the default dark theme.

## Actions

1. If **list**: Use Bash to read theme names and display in a table.
2. If **set <name>**: Use Bash to run: echo 'theme = "<name>"' and inform the user to restart or use /config set theme <name>.
3. If **preview <name>**: Describe the theme's color palette in words.
4. If **current**: Check the current theme setting.
5. If no args: Show the list.

Keep the output concise and well-formatted.
"##;

/// Create the /theme command
pub fn command() -> Command {
    Command::Prompt(Box::new(PromptCommand {
        base: CommandBase {
            name: "theme".to_string(),
            aliases: vec!["themes".to_string(), "colors".to_string()],
            description: "List, preview, and switch color themes".to_string(),
            has_user_specified_description: false,
            availability: vec![CommandAvailability::All],
            source: CommandSource::Builtin,
            is_enabled: true,
            is_hidden: false,
            argument_hint: Some("[list|set|preview|current|reset] [theme_name]".to_string()),
            when_to_use: Some(
                "Customize the look of Shannon with different color themes".to_string(),
            ),
            version: Some("0.1.0".to_string()),
            disable_model_invocation: false,
            user_invocable: true,
            is_workflow: false,
            immediate: false,
            is_sensitive: false,
            user_facing_name: None,
        },
        progress_message: "Loading themes...".to_string(),
        content_length: 1500,
        arg_names: vec!["action".to_string(), "theme_name".to_string()],
        allowed_tools: vec![
            "Bash(cat ~/.shannon/config.toml:*)".to_string(),
            "Bash(ls ~/.shannon/themes/:*)".to_string(),
            "Read".to_string(),
        ],
        model: None,
        hooks: std::collections::HashMap::new(),
        context: ExecutionContext::Inline,
        agent: None,
        paths: vec![],
        prompt_template: Some(THEME_PROMPT.to_string()),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_theme_command() {
        let cmd = command();
        assert_eq!(cmd.name(), "theme");
        assert!(cmd.aliases().contains(&"themes".to_string()));
    }

    #[test]
    fn test_theme_command_structure() {
        let cmd = command();
        assert!(!cmd.description().is_empty());
    }
}
