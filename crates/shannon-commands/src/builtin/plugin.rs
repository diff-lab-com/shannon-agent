//! /plugin command - Plugin management

use crate::command::{Command, CommandBase, CommandSource, PromptCommand, ExecutionContext, CommandAvailability};

/// Plugin prompt template
const PLUGIN_PROMPT: &str = r##"
Plugin management for Shannon Code.

Arguments: {args}

Subcommands:
- **install <name-or-url>** — Install a plugin from the registry or git URL
- **uninstall <name>** — Remove an installed plugin
- **list** — List all installed plugins
- **search <query>** — Search the plugin registry (ranked by relevance)
- **update [name]** — Update plugins (all or specific)
- **info <name>** — Show detailed info about a plugin from the index
- **enable <name>** — Enable a plugin
- **disable <name>** — Disable a plugin
- **help** — Show this help

Plugin Types:
- **Tool** — MCP server tool
- **Command** — Slash command extension
- **Skill** — Skill/prompt template

Plugin Permissions:
- read_files — Read files from filesystem
- write_files — Write files to filesystem
- execute_commands — Execute shell commands
- network — Network access
- mcp_tools — Access to MCP tools
- llm_api — Access to LLM API

When performing operations:
- For `install`, show download progress and confirm installation
- For `list`, show: name, version, type, status (enabled/disabled), description
- For `search`, show: name, description, author, downloads (ranked by relevance)
- For `info`, show: name, version, description, author, repository, type, keywords
- For `uninstall`, confirm before removing
"##;

/// Create the /plugin command
pub fn command() -> Command {
    Command::Prompt(Box::new(PromptCommand {
        base: CommandBase {
            name: "plugin".to_string(),
            aliases: vec!["plugins".to_string()],
            description: "Manage plugins: install, uninstall, list, search, update, info".to_string(),
            has_user_specified_description: false,
            availability: vec![CommandAvailability::All],
            source: CommandSource::Builtin,
            is_enabled: true,
            is_hidden: false,
            argument_hint: Some("[install|uninstall|list|search|update|info|enable|disable] [args]".to_string()),
            when_to_use: Some(
                "To install, manage, or discover plugins for Shannon Code".to_string(),
            ),
            version: Some("0.1.0".to_string()),
            disable_model_invocation: false,
            user_invocable: true,
            is_workflow: false,
            immediate: false,
            is_sensitive: false,
            user_facing_name: None,
        },
        progress_message: "".to_string(),
        content_length: 4000,
        arg_names: vec!["subcommand".to_string(), "args".to_string()],
        allowed_tools: vec![],
        model: None,
        hooks: std::collections::HashMap::new(),
        context: ExecutionContext::Inline,
        agent: None,
        paths: vec![],
        prompt_template: Some(PLUGIN_PROMPT.to_string()),
    }))
}

/// Plugin subcommands
#[derive(Debug, Clone, PartialEq)]
pub enum PluginSubcommand {
    /// Install a plugin
    Install,
    /// Uninstall a plugin
    Uninstall,
    /// List installed plugins
    List,
    /// Search registry
    Search,
    /// Update plugins
    Update,
    /// Enable a plugin
    Enable,
    /// Disable a plugin
    Disable,
    /// Show plugin info from the index
    Info,
    /// Show help
    Help,
}

/// Parse plugin subcommand from argument string
pub fn parse_plugin_subcommand(arg: &str) -> (PluginSubcommand, Option<String>) {
    let parts: Vec<&str> = arg.splitn(2, ' ').collect();
    let subcommand = parts.first().map(|s| *s).unwrap_or("");
    let argument = parts.get(1).map(|s| s.to_string());

    let cmd = match subcommand.to_lowercase().as_str() {
        "install" | "add" => PluginSubcommand::Install,
        "uninstall" | "remove" | "rm" => PluginSubcommand::Uninstall,
        "list" | "ls" => PluginSubcommand::List,
        "search" | "find" => PluginSubcommand::Search,
        "update" | "upgrade" => PluginSubcommand::Update,
        "enable" | "on" => PluginSubcommand::Enable,
        "disable" | "off" => PluginSubcommand::Disable,
        "info" => PluginSubcommand::Info,
        "help" | "?" => PluginSubcommand::Help,
        _ => PluginSubcommand::Help,
    };

    (cmd, argument)
}

/// Format plugin help output
pub fn format_plugin_help() -> String {
    let mut output = String::from("Plugin Management:\n\n");

    output.push_str("  /plugin list                    - List installed plugins\n");
    output.push_str("  /plugin search <query>          - Search plugin registry\n");
    output.push_str("  /plugin info <name>             - Show plugin details from index\n");
    output.push_str("  /plugin install <name-or-url>   - Install a plugin\n");
    output.push_str("  /plugin uninstall <name>        - Remove a plugin\n");
    output.push_str("  /plugin update [name]           - Update plugins\n");
    output.push_str("  /plugin enable <name>           - Enable a plugin\n");
    output.push_str("  /plugin disable <name>          - Disable a plugin\n");
    output.push_str("\nPlugin Sources:\n");
    output.push_str("  - Registry name (e.g., \"example-plugin\")\n");
    output.push_str("  - Git URL (e.g., \"https://github.com/user/plugin\")\n");
    output.push_str("  - Local path (e.g., \"/path/to/plugin\")\n");

    output
}

/// Plugin display info
#[derive(Debug, Clone)]
pub struct PluginDisplayInfo {
    pub name: String,
    pub version: String,
    pub plugin_type: String,
    pub description: String,
    pub enabled: bool,
    pub author: Option<String>,
}

/// Format a plugin list for display
pub fn format_plugin_list(plugins: &[PluginDisplayInfo]) -> String {
    if plugins.is_empty() {
        return "No plugins installed.\n\nInstall plugins with:\n  /plugin install <name-or-url>".to_string();
    }

    let mut output = String::from("Installed Plugins:\n\n");

    let name_width = plugins.iter()
        .map(|p| p.name.len())
        .max()
        .unwrap_or(10)
        .max(4);

    for plugin in plugins {
        let status = if plugin.enabled { "enabled" } else { "disabled" };
        output.push_str(&format!(
            "  {:<name_width$}  {:<8}  {} — {}\n",
            plugin.name,
            status,
            plugin.version,
            plugin.description,
            name_width = name_width,
        ));
    }

    output
}

/// Format search results
pub fn format_search_results(results: &[(String, String, String, u64)]) -> String {
    // (name, description, author, downloads)
    if results.is_empty() {
        return "No plugins found. Try a different search query.".to_string();
    }

    let mut output = String::from("Search Results:\n\n");

    for (name, description, author, downloads) in results {
        output.push_str(&format!(
            "  **{}** — {}\n    by {} — {} downloads\n\n",
            name, description, author, downloads
        ));
    }

    output
}

/// Format ranked search results with scores
pub fn format_ranked_search_results(results: &[(f64, String, String, String, u64)]) -> String {
    // (score, name, description, author, downloads)
    if results.is_empty() {
        return "No plugins found. Try a different search query.".to_string();
    }

    let mut output = String::from("Search Results (ranked by relevance):\n\n");

    for (score, name, description, author, downloads) in results {
        output.push_str(&format!(
            "  **{}** (relevance: {:.0}) — {}\n    by {} — {} downloads\n\n",
            name, score, description, author, downloads
        ));
    }

    output
}

/// Detailed plugin info for display
#[derive(Debug, Clone)]
pub struct PluginInfoDisplay {
    pub name: String,
    pub description: String,
    pub author: String,
    pub repository: String,
    pub latest_version: String,
    pub plugin_type: String,
    pub downloads: u64,
    pub keywords: Vec<String>,
}

/// Format detailed plugin info
pub fn format_plugin_info(info: &PluginInfoDisplay) -> String {
    let mut output = String::new();

    output.push_str(&format!("**{}** v{}\n", info.name, info.latest_version));
    output.push_str(&format!("  {}\n\n", info.description));
    output.push_str(&format!("  Author:       {}\n", info.author));
    output.push_str(&format!("  Type:         {}\n", info.plugin_type));
    output.push_str(&format!("  Repository:   {}\n", info.repository));
    output.push_str(&format!("  Downloads:    {}\n", info.downloads));

    if !info.keywords.is_empty() {
        output.push_str(&format!("  Keywords:     {}\n", info.keywords.join(", ")));
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_properties() {
        let cmd = command();
        assert_eq!(cmd.name(), "plugin");
        assert!(cmd.aliases().contains(&"plugins".to_string()));
    }

    #[test]
    fn test_parse_subcommands() {
        let (cmd, arg) = parse_plugin_subcommand("install");
        assert_eq!(cmd, PluginSubcommand::Install);
        assert!(arg.is_none());

        let (cmd, arg) = parse_plugin_subcommand("install my-plugin");
        assert_eq!(cmd, PluginSubcommand::Install);
        assert_eq!(arg, Some("my-plugin".to_string()));

        let (cmd, _) = parse_plugin_subcommand("add plugin");
        assert_eq!(cmd, PluginSubcommand::Install);

        let (cmd, _) = parse_plugin_subcommand("rm plugin");
        assert_eq!(cmd, PluginSubcommand::Uninstall);

        let (cmd, _) = parse_plugin_subcommand("unknown");
        assert_eq!(cmd, PluginSubcommand::Help);
    }

    #[test]
    fn test_format_help() {
        let help = format_plugin_help();
        assert!(help.contains("/plugin install"));
        assert!(help.contains("/plugin uninstall"));
        assert!(help.contains("/plugin list"));
        assert!(help.contains("/plugin info"));
    }

    #[test]
    fn test_format_empty_list() {
        let output = format_plugin_list(&[]);
        assert!(output.contains("No plugins installed"));
    }

    #[test]
    fn test_format_plugin_list() {
        let plugins = vec![
            PluginDisplayInfo {
                name: "example-plugin".to_string(),
                version: "1.0.0".to_string(),
                plugin_type: "Tool".to_string(),
                description: "An example plugin".to_string(),
                enabled: true,
                author: Some("Shannon Team".to_string()),
            },
            PluginDisplayInfo {
                name: "disabled-plugin".to_string(),
                version: "0.5.0".to_string(),
                plugin_type: "Command".to_string(),
                description: "A disabled plugin".to_string(),
                enabled: false,
                author: None,
            },
        ];

        let output = format_plugin_list(&plugins);
        assert!(output.contains("example-plugin"));
        assert!(output.contains("disabled-plugin"));
        assert!(output.contains("enabled"));
        assert!(output.contains("disabled"));
    }

    #[test]
    fn test_format_search_results() {
        let results = vec![
            ("example-plugin".to_string(), "An example".to_string(), "Author".to_string(), 1000),
            ("another-plugin".to_string(), "Another one".to_string(), "Author2".to_string(), 500),
        ];

        let output = format_search_results(&results);
        assert!(output.contains("example-plugin"));
        assert!(output.contains("1000 downloads"));
    }

    #[test]
    fn test_format_empty_search() {
        let output = format_search_results(&[]);
        assert!(output.contains("No plugins found"));
    }

    #[test]
    fn test_parse_info_subcommand() {
        let (cmd, arg) = parse_plugin_subcommand("info my-plugin");
        assert_eq!(cmd, PluginSubcommand::Info);
        assert_eq!(arg, Some("my-plugin".to_string()));
    }

    #[test]
    fn test_parse_info_no_arg() {
        let (cmd, arg) = parse_plugin_subcommand("info");
        assert_eq!(cmd, PluginSubcommand::Info);
        assert!(arg.is_none());
    }

    #[test]
    fn test_format_plugin_info() {
        let info = PluginInfoDisplay {
            name: "example-plugin".to_string(),
            description: "An example plugin for Shannon Code".to_string(),
            author: "Shannon Team".to_string(),
            repository: "https://github.com/shannon-code/example-plugin".to_string(),
            latest_version: "1.0.0".to_string(),
            plugin_type: "tool".to_string(),
            downloads: 1500,
            keywords: vec!["example".to_string(), "demo".to_string()],
        };

        let output = format_plugin_info(&info);
        assert!(output.contains("example-plugin"));
        assert!(output.contains("1.0.0"));
        assert!(output.contains("Shannon Team"));
        assert!(output.contains("1500"));
        assert!(output.contains("example, demo"));
    }

    #[test]
    fn test_format_plugin_info_no_keywords() {
        let info = PluginInfoDisplay {
            name: "minimal".to_string(),
            description: "Minimal".to_string(),
            author: "Author".to_string(),
            repository: "https://github.com/test/min".to_string(),
            latest_version: "0.1.0".to_string(),
            plugin_type: "skill".to_string(),
            downloads: 0,
            keywords: vec![],
        };

        let output = format_plugin_info(&info);
        assert!(output.contains("minimal"));
        assert!(!output.contains("Keywords"));
    }

    #[test]
    fn test_format_ranked_search_results() {
        let results = vec![
            (30.0, "exact-match".to_string(), "Perfect match".to_string(), "Author".to_string(), 100),
            (4.0, "partial-match".to_string(), "Partial match".to_string(), "Author2".to_string(), 50),
        ];

        let output = format_ranked_search_results(&results);
        assert!(output.contains("exact-match"));
        assert!(output.contains("relevance"));
        assert!(output.contains("100 downloads"));
    }

    #[test]
    fn test_format_ranked_search_empty() {
        let output = format_ranked_search_results(&[]);
        assert!(output.contains("No plugins found"));
    }
}
