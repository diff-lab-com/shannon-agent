//! /help command - Show command help and documentation

use crate::command::{Command, CommandBase, CommandSource, PromptCommand, ExecutionContext, CommandAvailability};

/// Create the /help command
pub fn command() -> Command {
    Command::Prompt(PromptCommand {
        base: CommandBase {
            name: "help".to_string(),
            aliases: vec!["?".to_string(), "commands".to_string()],
            description: "Show available commands and usage information".to_string(),
            has_user_specified_description: false,
            availability: vec![CommandAvailability::All],
            source: CommandSource::Builtin,
            is_enabled: true,
            is_hidden: false,
            argument_hint: Some("[command name]".to_string()),
            when_to_use: Some(
                "Use to discover available commands or get detailed help for a specific command".to_string(),
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
        content_length: 3000,
        arg_names: vec!["command".to_string()],
        allowed_tools: vec![],
        model: None,
        hooks: std::collections::HashMap::new(),
        context: ExecutionContext::Inline,
        agent: None,
        paths: vec![],
    })
}

/// Help category for organizing commands
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelpCategory {
    /// Git and version control commands
    Git,

    /// Code review and analysis commands
    Review,

    /// File and document processing
    Files,

    /// System and configuration
    System,

    /// User interface and display
    Ui,

    /// MCP and tool integration
    Mcp,

    /// Skills and automation
    Skills,

    /// Other commands
    Other,
}

impl HelpCategory {
    /// Get all categories
    pub fn all() -> &'static [HelpCategory] {
        &[
            HelpCategory::Git,
            HelpCategory::Review,
            HelpCategory::Files,
            HelpCategory::System,
            HelpCategory::Ui,
            HelpCategory::Mcp,
            HelpCategory::Skills,
            HelpCategory::Other,
        ]
    }

    /// Get category display name
    pub fn display_name(&self) -> &'static str {
        match self {
            HelpCategory::Git => "Git & Version Control",
            HelpCategory::Review => "Code Review & Analysis",
            HelpCategory::Files => "Files & Documents",
            HelpCategory::System => "System & Configuration",
            HelpCategory::Ui => "User Interface",
            HelpCategory::Mcp => "MCP Integration",
            HelpCategory::Skills => "Skills & Automation",
            HelpCategory::Other => "Other",
        }
    }

    /// Get category description
    pub fn description(&self) -> &'static str {
        match self {
            HelpCategory::Git => "Commands for git operations like commits, branches, and PRs",
            HelpCategory::Review => "Code review, PR analysis, and security checks",
            HelpCategory::Files => "File reading, PDF processing, and document analysis",
            HelpCategory::System => "Configuration, settings, and system operations",
            HelpCategory::Ui => "Display customization and terminal UI controls",
            HelpCategory::Mcp => "Model Context Protocol server management",
            HelpCategory::Skills => "Custom skills, workflows, and automation",
            HelpCategory::Other => "Other miscellaneous commands",
        }
    }
}

/// Command help entry
#[derive(Debug, Clone)]
pub struct CommandHelpEntry {
    /// Command name
    pub name: String,

    /// Aliases
    pub aliases: Vec<String>,

    /// Description
    pub description: String,

    /// Argument hint
    pub arg_hint: Option<String>,

    /// Usage examples
    pub examples: Vec<String>,

    /// Category
    pub category: HelpCategory,

    /// When to use
    pub when_to_use: Option<String>,

    /// Related commands
    pub related: Vec<String>,
}

impl CommandHelpEntry {
    /// Create a new help entry
    pub fn new(name: String, description: String, category: HelpCategory) -> Self {
        Self {
            name,
            aliases: vec![],
            description,
            arg_hint: None,
            examples: vec![],
            category,
            when_to_use: None,
            related: vec![],
        }
    }

    /// Add aliases
    pub fn with_aliases(mut self, aliases: Vec<&str>) -> Self {
        self.aliases = aliases.into_iter().map(String::from).collect();
        self
    }

    /// Add argument hint
    pub fn with_arg_hint(mut self, hint: &str) -> Self {
        self.arg_hint = Some(hint.to_string());
        self
    }

    /// Add examples
    pub fn with_examples(mut self, examples: Vec<&str>) -> Self {
        self.examples = examples.into_iter().map(String::from).collect();
        self
    }

    /// Add when to use
    pub fn with_when_to_use(mut self, when: &str) -> Self {
        self.when_to_use = Some(when.to_string());
        self
    }

    /// Add related commands
    pub fn with_related(mut self, related: Vec<&str>) -> Self {
        self.related = related.into_iter().map(String::from).collect();
        self
    }

    /// Format as markdown
    pub fn to_markdown(&self) -> String {
        let mut md = format!("## /{}", self.name);

        if !self.aliases.is_empty() {
            md.push_str(&format!(" (aliases: {})", self.aliases.join(", ")));
        }

        if let Some(hint) = &self.arg_hint {
            md.push_str(&format!(" `{}`", hint));
        }

        md.push_str(&format!("\n\n{}\n", self.description));

        if !self.examples.is_empty() {
            md.push_str("\n### Examples\n\n");
            for example in &self.examples {
                md.push_str(&format!("```\n{}\n```\n", example));
            }
        }

        if let Some(when) = &self.when_to_use {
            md.push_str(&format!("\n**When to use:** {}\n", when));
        }

        if !self.related.is_empty() {
            md.push_str(&format!(
                "\n**Related:** {}\n",
                self.related
                    .iter()
                    .map(|c| format!("/{}", c))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }

        md
    }
}

/// Get help entry for a command
pub fn get_command_help(command_name: &str) -> Option<CommandHelpEntry> {
    match command_name {
        "commit" => Some(
            CommandHelpEntry::new(
                "commit".to_string(),
                "Create a git commit with AI-generated message".to_string(),
                HelpCategory::Git,
            )
            .with_aliases(vec!["ci"])
            .with_arg_hint("[optional instructions]")
            .with_examples(vec!["/commit", "/commit fix authentication bug"])
            .with_when_to_use("After making changes that you want to commit with an appropriate message")
            .with_related(vec!["status", "diff", "review-pr"])
        ),
        "review-pr" => Some(
            CommandHelpEntry::new(
                "review-pr".to_string(),
                "Review a pull request with AI analysis".to_string(),
                HelpCategory::Review,
            )
            .with_aliases(vec!["pr-review", "ultrareview"])
            .with_arg_hint("[PR number]")
            .with_examples(vec!["/review-pr", "/review-pr 123"])
            .with_when_to_use("To review code changes before merging")
            .with_related(vec!["commit", "diff"])
        ),
        "pdf" => Some(
            CommandHelpEntry::new(
                "pdf".to_string(),
                "Extract and analyze content from PDF files".to_string(),
                HelpCategory::Files,
            )
            .with_aliases(vec!["read-pdf", "analyze-pdf"])
            .with_arg_hint("<file.pdf>")
            .with_examples(vec!["/pdf document.pdf", "/pdf research.pdf --pages 1-5"])
            .with_when_to_use("When you need to read or analyze PDF documents")
            .with_related(vec![])
        ),
        "status" => Some(
            CommandHelpEntry::new(
                "status".to_string(),
                "Show git repository status".to_string(),
                HelpCategory::Git,
            )
            .with_arg_hint("[options]")
            .with_examples(vec!["/status", "/status --short"])
            .with_when_to_use("To see current git status and changes")
            .with_related(vec!["commit", "diff", "branch"])
        ),
        "diff" => Some(
            CommandHelpEntry::new(
                "diff".to_string(),
                "Show git diff of changes".to_string(),
                HelpCategory::Git,
            )
            .with_arg_hint("[options]")
            .with_examples(vec!["/diff", "/diff HEAD~1", "/diff main...HEAD"])
            .with_when_to_use("To see what has changed in the repository")
            .with_related(vec!["status", "commit"])
        ),
        _ => None,
    }
}

/// Get all command help entries
pub fn all_help_entries() -> Vec<CommandHelpEntry> {
    vec![
        get_command_help("commit").unwrap(),
        get_command_help("review-pr").unwrap(),
        get_command_help("pdf").unwrap(),
        get_command_help("status").unwrap(),
        get_command_help("diff").unwrap(),
    ]
}

/// Generate help output
pub fn generate_help(command_filter: Option<&str>) -> String {
    if let Some(cmd) = command_filter {
        if let Some(entry) = get_command_help(cmd) {
            entry.to_markdown()
        } else {
            format!("No help found for command: {}", cmd)
        }
    } else {
        // Generate categorized help
        let mut output = String::from("# Shannon Code Commands\n\n");

        for category in HelpCategory::all() {
            let all_entries = all_help_entries();
            let entries: Vec<_> = all_entries
                .iter()
                .filter(|e| e.category == *category)
                .collect();

            if !entries.is_empty() {
                output.push_str(&format!("## {}\n\n", category.display_name()));
                output.push_str(&format!("{}\n\n", category.description()));

                for entry in entries {
                    let aliases = if entry.aliases.is_empty() {
                        String::new()
                    } else {
                        format!(" ({})", entry.aliases.join(", "))
                    };
                    let arg_hint = entry
                        .arg_hint
                        .as_ref()
                        .map(|h| format!(" {}", h))
                        .unwrap_or_default();
                    output.push_str(&format!(
                        "- **/{}{}**{} - {}\n",
                        entry.name, aliases, arg_hint, entry.description
                    ));
                }
                output.push('\n');
            }
        }

        output.push_str("Use `/help <command>` for detailed information about a specific command.\n");
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_help_command() {
        let cmd = command();
        assert_eq!(cmd.name(), "help");
        assert!(cmd.aliases().contains(&"?".to_string()));
    }

    #[test]
    fn test_get_command_help() {
        let help = get_command_help("commit").unwrap();
        assert_eq!(help.name, "commit");
        assert_eq!(help.category, HelpCategory::Git);
    }

    #[test]
    fn test_command_help_to_markdown() {
        let help = get_command_help("commit").unwrap();
        let md = help.to_markdown();
        assert!(md.contains("/commit"));
        assert!(md.contains("Create a git commit"));
    }

    #[test]
    fn test_generate_help() {
        let all_help = generate_help(None);
        assert!(all_help.contains("# Shannon Code Commands"));
        assert!(all_help.contains("Git & Version Control"));
    }

    #[test]
    fn test_generate_help_filtered() {
        let help = generate_help(Some("commit"));
        assert!(help.contains("/commit"));
        // Related commands are shown in command help, so review-pr is expected
        assert!(help.contains("/review-pr"));
    }
}
