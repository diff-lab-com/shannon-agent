//! /help command - Show command help and documentation

use crate::command::{Command, CommandBase, CommandSource, PromptCommand, ExecutionContext, CommandAvailability};

/// Help prompt template
const HELP_PROMPT: &str = r##"
Show help information for Shannon Code commands.

Arguments: {args}
- If a command name is provided (e.g., "commit", "diff"), show detailed help for that command
- If no argument is provided, show a categorized list of all available commands

Categories:
- Git & Version Control: commit, status, diff, review-pr, worktree
- Code Review & Analysis: review-pr, diff
- Files & Documents: pdf, export
- System & Configuration: config, debug, credentials
- User Interface: search, clear, help, history

For each command, show: name, aliases, description, argument hint, usage examples, and related commands.
"##;

/// Create the /help command
pub fn command() -> Command {
    Command::Prompt(Box::new(PromptCommand {
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
        prompt_template: Some(HELP_PROMPT.to_string()),
    }))
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
            md.push_str(&format!(" `{hint}`"));
        }

        md.push_str(&format!("\n\n{}\n", self.description));

        if !self.examples.is_empty() {
            md.push_str("\n### Examples\n\n");
            for example in &self.examples {
                md.push_str(&format!("```\n{example}\n```\n"));
            }
        }

        if let Some(when) = &self.when_to_use {
            md.push_str(&format!("\n**When to use:** {when}\n"));
        }

        if !self.related.is_empty() {
            md.push_str(&format!(
                "\n**Related:** {}\n",
                self.related
                    .iter()
                    .map(|c| format!("/{c}"))
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
        "search" => Some(
            CommandHelpEntry::new(
                "search".to_string(),
                "Search command history with regex patterns".to_string(),
                HelpCategory::Ui,
            )
            .with_aliases(vec!["history-search", "hist"])
            .with_arg_hint("[pattern] [--count N] [--regex] [--case-sensitive]")
            .with_examples(vec![
                "/search git",
                "/search cargo --count=10",
                "/search commit --regex",
                "/search ERROR --case-sensitive",
            ])
            .with_when_to_use("To find previously run commands matching a pattern")
            .with_related(vec!["history", "export"])
        ),
        "find" => Some(
            CommandHelpEntry::new(
                "find".to_string(),
                "Search through conversation messages (not command history)".to_string(),
                HelpCategory::Ui,
            )
            .with_aliases(vec!["grep", "conv-search"])
            .with_arg_hint("<query>")
            .with_examples(vec![
                "/find error",
                "/find authentication",
                "/find TODO",
            ])
            .with_when_to_use("To find past messages in the current conversation matching a keyword")
            .with_related(vec!["search", "history"])
        ),
        "export" => Some(
            CommandHelpEntry::new(
                "export".to_string(),
                "Export current session to markdown or JSON format".to_string(),
                HelpCategory::Files,
            )
            .with_aliases(vec!["save", "export-session"])
            .with_arg_hint("[md|json] [filename]")
            .with_examples(vec![
                "/export md",
                "/export json session.json",
                "/export md --no-metadata",
            ])
            .with_when_to_use("To save your current conversation or session data to a file")
            .with_related(vec!["history", "config"])
        ),
        "config" => Some(
            CommandHelpEntry::new(
                "config".to_string(),
                "View and modify Shannon configuration settings".to_string(),
                HelpCategory::System,
            )
            .with_aliases(vec!["settings", "cfg"])
            .with_arg_hint("[show|set|reset|list|get] [args]")
            .with_examples(vec![
                "/config show",
                "/config set model=claude-3-opus",
                "/config set temperature 0.5",
                "/config get model",
                "/config list",
                "/config reset",
            ])
            .with_when_to_use("To view or change configuration settings like model, temperature, or other preferences")
            .with_related(vec!["debug", "profile"])
        ),
        "debug" => Some(
            CommandHelpEntry::new(
                "debug".to_string(),
                "Developer tools and diagnostics".to_string(),
                HelpCategory::System,
            )
            .with_aliases(vec!["developer", "dev"])
            .with_arg_hint("[on|off|toggle|status|log level|profile]")
            .with_examples(vec![
                "/debug on",
                "/debug off",
                "/debug toggle",
                "/debug status",
                "/debug log debug",
                "/debug profile",
            ])
            .with_when_to_use("When developing or debugging Shannon to enable debug mode, change log levels, or view performance metrics")
            .with_related(vec!["config", "profile"])
        ),
        // REPL-specific commands
        "clear" => Some(
            CommandHelpEntry::new(
                "clear".to_string(),
                "Clear chat history".to_string(),
                HelpCategory::Ui,
            )
            .with_aliases(vec!["cls"])
            .with_examples(vec!["/clear"])
            .with_when_to_use("Use to clear the chat history when the screen gets cluttered")
            .with_related(vec!["history", "export"])
        ),
        "quit" | "exit" => Some(
            CommandHelpEntry::new(
                "quit".to_string(),
                "Exit Shannon REPL".to_string(),
                HelpCategory::Ui,
            )
            .with_aliases(vec!["exit", "q"])
            .with_examples(vec!["/quit", "/exit", "/q"])
            .with_when_to_use("Use to exit the Shannon REPL")
            .with_related(vec![])
        ),
        "model" => Some(
            CommandHelpEntry::new(
                "model".to_string(),
                "Show or set the AI model".to_string(),
                HelpCategory::System,
            )
            .with_arg_hint("[model name]")
            .with_examples(vec!["/model", "/model claude-3-opus"])
            .with_when_to_use("Use to change the AI model or see the current model")
            .with_related(vec!["config"])
        ),
        "init" => Some(
            CommandHelpEntry::new(
                "init".to_string(),
                "Initialize project configuration (CLAUDE.md, detect git)".to_string(),
                HelpCategory::System,
            )
            .with_aliases(vec!["initialize"])
            .with_examples(vec!["/init"])
            .with_when_to_use("Use when starting work in a new project directory")
            .with_related(vec!["config"])
        ),
        "sessions" => Some(
            CommandHelpEntry::new(
                "sessions".to_string(),
                "List saved sessions".to_string(),
                HelpCategory::Ui,
            )
            .with_aliases(vec!["list-sessions"])
            .with_arg_hint("[--all] [--search <query>]")
            .with_examples(vec!["/sessions", "/sessions --all", "/sessions --search bugfix"])
            .with_when_to_use("Use to see previously saved sessions that can be resumed")
            .with_related(vec!["resume", "history"])
        ),
        "resume" => Some(
            CommandHelpEntry::new(
                "resume".to_string(),
                "Resume a saved session by UUID or number".to_string(),
                HelpCategory::Ui,
            )
            .with_aliases(vec!["restore"])
            .with_arg_hint("<number-or-uuid>")
            .with_examples(vec!["/resume 1", "/resume <uuid>"])
            .with_when_to_use("Use to continue a previous session")
            .with_related(vec!["sessions", "history"])
        ),
        "history" => Some(
            CommandHelpEntry::new(
                "history".to_string(),
                "Show current session stats or export to file".to_string(),
                HelpCategory::Ui,
            )
            .with_aliases(vec!["stats"])
            .with_arg_hint("[--export <path>]")
            .with_examples(vec!["/history", "/history --export session.md"])
            .with_when_to_use("Use to see session statistics or export the conversation")
            .with_related(vec!["sessions", "resume", "export"])
        ),
        "worktree" => Some(
            CommandHelpEntry::new(
                "worktree".to_string(),
                "Manage git worktrees (enter, exit, status)".to_string(),
                HelpCategory::Git,
            )
            .with_arg_hint("[enter <name>|exit [--keep|--remove]|status]")
            .with_examples(vec!["/worktree status", "/worktree enter feature-branch", "/worktree exit --remove"])
            .with_when_to_use("Use to work in isolated git branches using worktrees")
            .with_related(vec!["status", "commit"])
        ),
        "doctor" => Some(
            CommandHelpEntry::new(
                "doctor".to_string(),
                "Run system diagnostics and health checks".to_string(),
                HelpCategory::System,
            )
            .with_aliases(vec!["check", "diagnostics"])
            .with_arg_hint("[check name]")
            .with_examples(vec!["/doctor", "/doctor network"])
            .with_when_to_use("Use to diagnose issues with your Shannon Code installation and environment")
            .with_related(vec!["config", "debug"])
        ),
        "hooks" => Some(
            CommandHelpEntry::new(
                "hooks".to_string(),
                "View and manage lifecycle hooks (PreToolUse, PostToolUse, etc.)".to_string(),
                HelpCategory::System,
            )
            .with_arg_hint("[reload|path]")
            .with_examples(vec!["/hooks", "/hooks reload", "/hooks path"])
            .with_when_to_use("Use to inspect configured shell hooks or reload hook configuration after editing hooks.json")
            .with_related(vec!["config", "permissions"])
        ),
        "remember" => Some(
            CommandHelpEntry::new(
                "remember".to_string(),
                "Save a memory for future sessions".to_string(),
                HelpCategory::System,
            )
            .with_aliases(vec!["mem", "memo"])
            .with_arg_hint("<text to remember>")
            .with_examples(vec!["/remember Always use tabs not spaces in this project", "/remember The API endpoint is /v2/graphql"])
            .with_when_to_use("Use to save important context, preferences, or decisions that should persist across sessions")
            .with_related(vec!["recall", "forget", "memory"])
        ),
        "recall" => Some(
            CommandHelpEntry::new(
                "recall".to_string(),
                "Search and retrieve saved memories".to_string(),
                HelpCategory::System,
            )
            .with_aliases(vec!["search-memory"])
            .with_arg_hint("[query]")
            .with_examples(vec!["/recall", "/recall API endpoint", "/recall tabs"])
            .with_when_to_use("Use to find previously saved memories. Without a query, lists all memories for the current project")
            .with_related(vec!["remember", "forget", "memory"])
        ),
        "forget" => Some(
            CommandHelpEntry::new(
                "forget".to_string(),
                "Delete a saved memory by ID prefix".to_string(),
                HelpCategory::System,
            )
            .with_arg_hint("<memory-id-prefix>")
            .with_examples(vec!["/forget abc12345"])
            .with_when_to_use("Use to remove a memory you no longer need. Use /recall first to find the ID")
            .with_related(vec!["remember", "recall", "memory"])
        ),
        "memory" => Some(
            CommandHelpEntry::new(
                "memory".to_string(),
                "Memory store stats and maintenance".to_string(),
                HelpCategory::System,
            )
            .with_arg_hint("[stats|cleanup]")
            .with_examples(vec!["/memory", "/memory cleanup"])
            .with_when_to_use("Use to see memory store statistics or run cleanup to remove stale entries")
            .with_related(vec!["remember", "recall", "forget"])
        ),
        "image" => Some(
            CommandHelpEntry::new(
                "image".to_string(),
                "Attach an image file for the AI to analyze".to_string(),
                HelpCategory::System,
            )
            .with_aliases(vec!["img", "screenshot"])
            .with_arg_hint("<file-path> [prompt]")
            .with_examples(vec!["/image screenshot.png", "/image diagram.png Explain this architecture", "/img ~/photos/error.jpg What went wrong?"])
            .with_when_to_use("Use to share a screenshot, diagram, or photo with the AI for visual analysis")
            .with_related(vec!["pdf"])
        ),
        "mode" => Some(
            CommandHelpEntry::new(
                "mode".to_string(),
                "View or change the approval policy mode for tool execution".to_string(),
                HelpCategory::System,
            )
            .with_arg_hint("[suggest|auto-edit|full-auto|readonly]")
            .with_examples(vec!["/mode", "/mode auto-edit", "/mode full-auto", "/mode readonly"])
            .with_when_to_use("Use to control how tools are approved: suggest asks every time, auto-edit auto-approves file edits, full-auto approves everything, readonly blocks writes")
            .with_related(vec!["permissions", "config"])
        ),
        "context" => Some(
            CommandHelpEntry::new(
                "context".to_string(),
                "Show or reload project context (CLAUDE.md, git info)".to_string(),
                HelpCategory::System,
            )
            .with_arg_hint("[reload]")
            .with_examples(vec!["/context", "/context reload"])
            .with_when_to_use("Use to see what project context is loaded or to reload after changes to CLAUDE.md")
            .with_related(vec!["mode", "init"])
        ),
        "undo" => Some(
            CommandHelpEntry::new(
                "undo".to_string(),
                "Revert file changes using git checkpoints".to_string(),
                HelpCategory::System,
            )
            .with_arg_hint("[list|<number>]")
            .with_examples(vec!["/undo", "/undo list", "/undo 2"])
            .with_when_to_use("Use to undo file changes made by AI tools. Checkpoints are created automatically before file modifications.")
            .with_related(vec!["diff", "status"])
        ),
        "notify" => Some(
            CommandHelpEntry::new(
                "notify".to_string(),
                "Toggle desktop notifications for query completion".to_string(),
                HelpCategory::System,
            )
            .with_arg_hint("[on|off|test]")
            .with_examples(vec!["/notify", "/notify on", "/notify off", "/notify test"])
            .with_when_to_use("Use to enable OS desktop notifications when long-running queries finish")
            .with_related(vec!["mode", "cost"])
        ),
        "create-pr" => Some(
            CommandHelpEntry::new(
                "create-pr".to_string(),
                "Create a GitHub pull request from current branch".to_string(),
                HelpCategory::Git,
            )
            .with_arg_hint("[<title>] [--draft] [--base <branch>] [--web]")
            .with_examples(vec!["/create-pr", "/create-pr fix login bug", "/create-pr --draft", "/create-pr --base develop"])
            .with_when_to_use("Use when ready to create a PR from your current feature branch")
            .with_related(vec!["status", "diff", "branch"])
        ),
        "patch" => Some(
            CommandHelpEntry::new(
                "patch".to_string(),
                "Search/replace with diff preview before applying".to_string(),
                HelpCategory::Files,
            )
            .with_arg_hint("<file> <search> --- <replace> [--apply] [--all]")
            .with_examples(vec!["/patch src/main.rs old_fn --- new_fn", "/patch --apply src/lib.rs foo --- bar", "/patch --all config.rs old_val --- new_val"])
            .with_when_to_use("Use to preview and apply targeted text replacements in files")
            .with_related(vec!["edit", "diff", "undo"])
        ),
        "sandbox" => Some(
            CommandHelpEntry::new(
                "sandbox".to_string(),
                "Execution sandbox for isolated shell command execution".to_string(),
                HelpCategory::System,
            )
            .with_arg_hint("[status|docker|direct|check]")
            .with_examples(vec!["/sandbox", "/sandbox docker", "/sandbox direct", "/sandbox check"])
            .with_when_to_use("Use to enable Docker isolation for shell commands or check sandbox status")
            .with_related(vec!["doctor", "config", "permissions"])
        ),
        "compact" => Some(
            CommandHelpEntry::new(
                "compact".to_string(),
                "Compact conversation context to reduce token usage".to_string(),
                HelpCategory::System,
            )
            .with_arg_hint("[status|truncate|micro|group]")
            .with_examples(vec!["/compact", "/compact status", "/compact truncate", "/compact micro"])
            .with_when_to_use("Use when context is getting large and you want to reduce token usage")
            .with_related(vec!["history", "cost"])
        ),
        "cost" => Some(
            CommandHelpEntry::new(
                "cost".to_string(),
                "Show cost summary for the current session".to_string(),
                HelpCategory::System,
            )
            .with_arg_hint("[budget <amount_usd>]")
            .with_examples(vec!["/cost", "/cost budget 5.00"])
            .with_when_to_use("Use to check token usage, session cost, per-model breakdown, and set budget limits")
            .with_related(vec!["history", "compact"])
        ),
        "team" => Some(
            CommandHelpEntry::new(
                "team".to_string(),
                "Multi-agent team orchestration".to_string(),
                HelpCategory::Skills,
            )
            .with_arg_hint("[create|add|task|assign|status|list|run|shutdown]")
            .with_examples(vec!["/team create my-team", "/team add my-team agent1", "/team task my-team fix bug", "/team run"])
            .with_when_to_use("Use to create and manage multi-agent teams for parallel task execution")
            .with_related(vec!["worktree"])
        ),
        "permissions" => Some(
            CommandHelpEntry::new(
                "permissions".to_string(),
                "View and manage tool permissions".to_string(),
                HelpCategory::System,
            )
            .with_aliases(vec!["perms", "perm"])
            .with_arg_hint("[status|allow <tool>|deny <tool>|reset]")
            .with_examples(vec!["/permissions", "/permissions status", "/permissions allow Bash", "/permissions deny FileWrite", "/permissions reset"])
            .with_when_to_use("Use to view current permission policies, allow or deny tools without prompting, or reset overrides")
            .with_related(vec!["config", "doctor"])
        ),
        "plan" => Some(
            CommandHelpEntry::new(
                "plan".to_string(),
                "Plan mode — create, review, and approve implementation plans".to_string(),
                HelpCategory::Skills,
            )
            .with_arg_hint("[<description>|status|approve|reject|done]")
            .with_examples(vec!["/plan add user authentication", "/plan refactor database layer", "/plan fix login bug", "/plan status", "/plan approve", "/plan reject", "/plan done"])
            .with_when_to_use("Use to plan complex tasks before implementation, review the steps, and approve before proceeding")
            .with_related(vec!["team", "worktree"])
        ),
        "branch" => Some(
            CommandHelpEntry::new(
                "branch".to_string(),
                "Create a branch from an existing session".to_string(),
                HelpCategory::Ui,
            )
            .with_aliases(vec!["fork"])
            .with_arg_hint("<session-id-or-number> [message-index]")
            .with_examples(vec!["/branch 1", "/branch abc-123-uuid 4", "/branch 2 10"])
            .with_when_to_use("Use to fork a conversation from a specific point, creating a new independent session")
            .with_related(vec!["sessions", "resume"])
        ),
        "credentials" => Some(
            CommandHelpEntry::new(
                "credentials".to_string(),
                "Manage stored credentials".to_string(),
                HelpCategory::System,
            )
            .with_aliases(vec!["creds", "cred"])
            .with_arg_hint("[list|store|get|delete|count]")
            .with_examples(vec!["/credentials list", "/credentials store my-service my-token"])
            .with_when_to_use("Use to store or retrieve API keys and other credentials")
            .with_related(vec!["config"])
        ),
        "browse" => Some(
            CommandHelpEntry::new(
                "browse".to_string(),
                "Browse files and directories".to_string(),
                HelpCategory::Files,
            )
            .with_aliases(vec!["files"])
            .with_arg_hint("[path]")
            .with_examples(vec!["/browse", "/browse ./src"])
            .with_when_to_use("Use to interactively browse and select files from the filesystem")
            .with_related(vec!["search"])
        ),
        "tools" => Some(
            CommandHelpEntry::new(
                "tools".to_string(),
                "Select and manage available tools".to_string(),
                HelpCategory::System,
            )
            .with_aliases(vec!["select-tools"])
            .with_examples(vec!["/tools"])
            .with_when_to_use("Use to enable or disable specific tools for the current session")
            .with_related(vec!["config"])
        ),
        "go_to_definition" => Some(
            CommandHelpEntry::new(
                "go_to_definition".to_string(),
                "Find where a symbol is defined using LSP".to_string(),
                HelpCategory::Review,
            )
            .with_arg_hint("<file> <line> <character>")
            .with_examples(vec!["/go_to_definition src/main.rs 10 5"])
            .with_when_to_use("Use to navigate to the definition of a symbol at a position in a file")
            .with_related(vec!["find_references", "hover"])
        ),
        "find_references" => Some(
            CommandHelpEntry::new(
                "find_references".to_string(),
                "Find all references to a symbol using LSP".to_string(),
                HelpCategory::Review,
            )
            .with_arg_hint("<file> <line> <character>")
            .with_examples(vec!["/find_references src/main.rs 10 5"])
            .with_when_to_use("Use to find all usages of a symbol across the codebase")
            .with_related(vec!["go_to_definition", "rename_symbol"])
        ),
        "hover" => Some(
            CommandHelpEntry::new(
                "hover".to_string(),
                "Get type info and docs for a symbol using LSP".to_string(),
                HelpCategory::Review,
            )
            .with_arg_hint("<file> <line> <character>")
            .with_examples(vec!["/hover src/main.rs 10 5"])
            .with_when_to_use("Use to see type signatures, doc comments, and contextual info for a symbol")
            .with_related(vec!["go_to_definition", "document_symbol"])
        ),
        "document_symbol" => Some(
            CommandHelpEntry::new(
                "document_symbol".to_string(),
                "List all symbols in a file using LSP".to_string(),
                HelpCategory::Review,
            )
            .with_arg_hint("<file>")
            .with_examples(vec!["/document_symbol src/main.rs"])
            .with_when_to_use("Use to get a hierarchical view of functions, classes, and other symbols in a file")
            .with_related(vec!["hover", "workspace_symbol"])
        ),
        "workspace_symbol" => Some(
            CommandHelpEntry::new(
                "workspace_symbol".to_string(),
                "Search for symbols across the workspace using LSP".to_string(),
                HelpCategory::Review,
            )
            .with_arg_hint("<query>")
            .with_examples(vec!["/workspace_symbol MyStruct", "/workspace_symbol parse_"])
            .with_when_to_use("Use to find functions, classes, structs matching a query pattern across all project files")
            .with_related(vec!["document_symbol", "find_references"])
        ),
        "rename_symbol" => Some(
            CommandHelpEntry::new(
                "rename_symbol".to_string(),
                "Rename a symbol across the codebase using LSP".to_string(),
                HelpCategory::Review,
            )
            .with_arg_hint("<file> <line> <character> <new_name>")
            .with_examples(vec!["/rename_symbol src/main.rs 10 5 new_name"])
            .with_when_to_use("Use to safely rename a variable, function, class, or other symbol across the entire project")
            .with_related(vec!["find_references", "go_to_definition"])
        ),
        "code_actions" => Some(
            CommandHelpEntry::new(
                "code_actions".to_string(),
                "Get available quick fixes and refactorings using LSP".to_string(),
                HelpCategory::Review,
            )
            .with_arg_hint("<file> <start_line> <start_char> <end_line> <end_char>")
            .with_examples(vec!["/code_actions src/main.rs 10 0 10 20"])
            .with_when_to_use("Use to get suggested fixes for diagnostics and available refactorings for a code range")
            .with_related(vec!["hover", "rename_symbol"])
        ),
        "web-search" => Some(
            CommandHelpEntry::new(
                "web-search".to_string(),
                "Search the web using Tavily API".to_string(),
                HelpCategory::Skills,
            )
            .with_aliases(vec!["websearch", "search-web"])
            .with_arg_hint("<query>")
            .with_examples(vec!["/web-search Rust async best practices", "/web-search latest TypeScript features"])
            .with_when_to_use("Use to search the web for current information, documentation, or solutions")
            .with_related(vec!["review"])
        ),
        "review" => Some(
            CommandHelpEntry::new(
                "review".to_string(),
                "Review code changes with automated analysis".to_string(),
                HelpCategory::Review,
            )
            .with_arg_hint("[diff target]")
            .with_examples(vec!["/review", "/review HEAD~1", "/review main...HEAD"])
            .with_when_to_use("Use to review staged/uncommitted changes or compare against a specific target for secrets, size, and test coverage")
            .with_related(vec!["diff", "status"])
        ),
        "local-models" => Some(
            CommandHelpEntry::new(
                "local-models".to_string(),
                "Detect and list locally available AI models".to_string(),
                HelpCategory::System,
            )
            .with_aliases(vec!["local"])
            .with_examples(vec!["/local-models"])
            .with_when_to_use("Use to check which local model servers (Ollama, LM Studio) are running and what models are available")
            .with_related(vec!["model", "config"])
        ),
        "ci" => Some(
            CommandHelpEntry::new(
                "ci".to_string(),
                "View GitHub Actions workflows, runs, and trigger workflows".to_string(),
                HelpCategory::Skills,
            )
            .with_aliases(vec!["gh-actions"])
            .with_arg_hint("[status|runs|workflows|view|trigger|help]")
            .with_examples(vec!["/ci", "/ci runs 20", "/ci workflows", "/ci view 12345", "/ci trigger build"])
            .with_when_to_use("Use to check CI status, view workflow runs, or trigger workflows via GitHub CLI")
            .with_related(vec!["diff", "review"])
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
        get_command_help("search").unwrap(),
        get_command_help("export").unwrap(),
        get_command_help("config").unwrap(),
        get_command_help("debug").unwrap(),
        get_command_help("clear").unwrap(),
        get_command_help("quit").unwrap(),
        get_command_help("model").unwrap(),
        get_command_help("init").unwrap(),
        get_command_help("sessions").unwrap(),
        get_command_help("resume").unwrap(),
        get_command_help("history").unwrap(),
        get_command_help("worktree").unwrap(),
        get_command_help("doctor").unwrap(),
        get_command_help("compact").unwrap(),
        get_command_help("cost").unwrap(),
        get_command_help("team").unwrap(),
        get_command_help("permissions").unwrap(),
        get_command_help("plan").unwrap(),
        get_command_help("branch").unwrap(),
        get_command_help("credentials").unwrap(),
        get_command_help("browse").unwrap(),
        get_command_help("tools").unwrap(),
        get_command_help("go_to_definition").unwrap(),
        get_command_help("find_references").unwrap(),
        get_command_help("hover").unwrap(),
        get_command_help("document_symbol").unwrap(),
        get_command_help("workspace_symbol").unwrap(),
        get_command_help("rename_symbol").unwrap(),
        get_command_help("code_actions").unwrap(),
        get_command_help("web-search").unwrap(),
        get_command_help("review").unwrap(),
        get_command_help("local-models").unwrap(),
        get_command_help("ci").unwrap(),
        get_command_help("hooks").unwrap(),
        get_command_help("remember").unwrap(),
        get_command_help("recall").unwrap(),
        get_command_help("forget").unwrap(),
        get_command_help("memory").unwrap(),
        get_command_help("image").unwrap(),
        get_command_help("mode").unwrap(),
        get_command_help("context").unwrap(),
        get_command_help("undo").unwrap(),
        get_command_help("notify").unwrap(),
        get_command_help("create-pr").unwrap(),
        get_command_help("patch").unwrap(),
        get_command_help("sandbox").unwrap(),
    ]
}

/// Generate help output
pub fn generate_help(command_filter: Option<&str>) -> String {
    if let Some(cmd) = command_filter {
        if let Some(entry) = get_command_help(cmd) {
            entry.to_markdown()
        } else {
            format!("No help found for command: {cmd}")
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
                        .map(|h| format!(" {h}"))
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
