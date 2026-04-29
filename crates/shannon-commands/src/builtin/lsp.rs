//! /lsp command - LSP (Language Server Protocol) management
//!
//! Provides subcommands for listing language servers, viewing status,
//! and managing LSP server connections.

#![allow(dead_code)]

use crate::command::{Command, CommandBase, CommandSource, PromptCommand, ExecutionContext, CommandAvailability};

/// LSP prompt template
const LSP_PROMPT: &str = r##"
LSP (Language Server Protocol) server management.

Arguments: {args}

Subcommands:
- **status** — Show connected LSP servers and their status
- **start <language>** — Manually start a language server (rust, typescript, python, go, c, cpp, etc.)
- **stop <language>** — Stop a running language server
- **help** — Show this help

Language servers supported:
- rust — rust-analyzer
- typescript / javascript — typescript-language-server
- python — pylsp
- go — gopls
- c / cpp — clangd
- java — jdtls

Configuration:
Servers can be configured in .lsp.json with custom commands and args.
Search order: ~/.shannon/.lsp.json, ~/.claude/.lsp.json, ./.lsp.json

When gathering information:
- For `status`, show a table of servers: language, state (Running/Stopped), server command
- For `start`, attempt to launch the server and report success/failure
- For `stop`, gracefully shut down the server and confirm
"##;

/// Create the /lsp command
pub fn command() -> Command {
    Command::Prompt(Box::new(PromptCommand {
        base: CommandBase {
            name: "lsp".to_string(),
            aliases: vec!["language-server".to_string()],
            description: "Manage LSP servers: status, start, stop".to_string(),
            has_user_specified_description: false,
            availability: vec![CommandAvailability::All],
            source: CommandSource::Builtin,
            is_enabled: true,
            is_hidden: false,
            argument_hint: Some("[status|start|stop] [language]".to_string()),
            when_to_use: Some(
                "To view or manage language server connections for code intelligence features".to_string(),
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
        arg_names: vec!["subcommand".to_string(), "language".to_string()],
        allowed_tools: vec![],
        model: None,
        hooks: std::collections::HashMap::new(),
        context: ExecutionContext::Inline,
        agent: None,
        paths: vec![],
        prompt_template: Some(LSP_PROMPT.to_string()),
    }))
}

/// LSP subcommands
#[derive(Debug, Clone, PartialEq)]
pub enum LspSubcommand {
    /// Show server status
    Status,
    /// Start a server
    Start,
    /// Stop a server
    Stop,
    /// Show help
    Help,
}

/// Parse LSP subcommand from argument string.
pub fn parse_lsp_subcommand(arg: &str) -> LspSubcommand {
    match arg.to_lowercase().as_str() {
        "status" | "list" | "ls" => LspSubcommand::Status,
        "start" | "launch" => LspSubcommand::Start,
        "stop" | "kill" => LspSubcommand::Stop,
        "help" | "?" => LspSubcommand::Help,
        _ => LspSubcommand::Status,
    }
}

/// Format the LSP help output.
pub fn format_lsp_help() -> String {
    let mut output = String::from("LSP Server Management:\n\n");

    output.push_str("  /lsp status              - Show all LSP servers and their status\n");
    output.push_str("  /lsp start <language>    - Start a language server\n");
    output.push_str("  /lsp stop <language>     - Stop a running language server\n");
    output.push_str("\nSupported Languages:\n");
    output.push_str("  rust        - rust-analyzer\n");
    output.push_str("  typescript  - typescript-language-server\n");
    output.push_str("  javascript  - typescript-language-server\n");
    output.push_str("  python      - pylsp\n");
    output.push_str("  go          - gopls\n");
    output.push_str("  c / cpp     - clangd\n");
    output.push_str("  java        - jdtls\n");
    output.push_str("\nConfiguration:\n");
    output.push_str("  Create .lsp.json in your project root or home directory:\n");
    output.push_str("  {\n");
    output.push_str("    \"rust\": {\n");
    output.push_str("      \"command\": \"rust-analyzer\",\n");
    output.push_str("      \"args\": []\n");
    output.push_str("    }\n");
    output.push_str("  }\n");

    output
}

/// Format a server list for display.
pub fn format_server_list(servers: &[(String, LspServerState)]) -> String {
    if servers.is_empty() {
        return "No LSP servers configured.".to_string();
    }

    let mut output = String::from("LSP Servers:\n\n");

    let name_width = servers
        .iter()
        .map(|(name, _)| name.len())
        .max()
        .unwrap_or(10)
        .max(8);

    for (name, state) in servers {
        output.push_str(&format!(
            "  {:<name_width$}  {:<12}  {}\n",
            name,
            format!("{}", state.status),
            state.command.clone().unwrap_or_else(|| "(default)".to_string()),
            name_width = name_width,
        ));
    }

    output
}

/// LSP server state for display purposes.
#[derive(Debug, Clone, PartialEq)]
pub enum LspServerStatus {
    Running,
    Stopped,
    Starting,
    Error,
}

impl std::fmt::Display for LspServerStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LspServerStatus::Running => write!(f, "Running"),
            LspServerStatus::Stopped => write!(f, "Stopped"),
            LspServerStatus::Starting => write!(f, "Starting"),
            LspServerStatus::Error => write!(f, "Error"),
        }
    }
}

/// LSP server state info for formatting.
#[derive(Debug, Clone)]
pub struct LspServerState {
    pub status: LspServerStatus,
    pub command: Option<String>,
    pub pid: Option<u32>,
}

/// Get default server command for a language.
pub fn get_default_server_command(language: &str) -> Option<&'static str> {
    match language {
        "rust" => Some("rust-analyzer"),
        "typescript" | "javascript" => Some("typescript-language-server"),
        "python" => Some("pylsp"),
        "go" => Some("gopls"),
        "c" | "cpp" => Some("clangd"),
        "java" => Some("jdtls"),
        _ => None,
    }
}

/// Validate language identifier.
pub fn is_valid_language(language: &str) -> bool {
    matches!(
        language,
        "rust" | "typescript" | "javascript" | "python" | "go" | "c" | "cpp" | "java"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_properties() {
        let cmd = command();
        assert_eq!(cmd.name(), "lsp");
        assert!(cmd.aliases().contains(&"language-server".to_string()));
    }

    #[test]
    fn test_parse_lsp_subcommand() {
        assert_eq!(parse_lsp_subcommand("status"), LspSubcommand::Status);
        assert_eq!(parse_lsp_subcommand("list"), LspSubcommand::Status);
        assert_eq!(parse_lsp_subcommand("ls"), LspSubcommand::Status);
        assert_eq!(parse_lsp_subcommand("start"), LspSubcommand::Start);
        assert_eq!(parse_lsp_subcommand("launch"), LspSubcommand::Start);
        assert_eq!(parse_lsp_subcommand("stop"), LspSubcommand::Stop);
        assert_eq!(parse_lsp_subcommand("kill"), LspSubcommand::Stop);
        assert_eq!(parse_lsp_subcommand("help"), LspSubcommand::Help);
        assert_eq!(parse_lsp_subcommand("unknown"), LspSubcommand::Status);
    }

    #[test]
    fn test_format_lsp_help() {
        let help = format_lsp_help();
        assert!(help.contains("/lsp status"));
        assert!(help.contains("/lsp start"));
        assert!(help.contains("/lsp stop"));
    }

    #[test]
    fn test_format_server_list_empty() {
        let output = format_server_list(&[]);
        assert!(output.contains("No LSP servers"));
    }

    #[test]
    fn test_format_server_list_with_servers() {
        let servers = vec![
            (
                "rust".to_string(),
                LspServerState {
                    status: LspServerStatus::Running,
                    command: Some("rust-analyzer".to_string()),
                    pid: Some(12345),
                },
            ),
            (
                "python".to_string(),
                LspServerState {
                    status: LspServerStatus::Stopped,
                    command: None,
                    pid: None,
                },
            ),
        ];
        let output = format_server_list(&servers);
        assert!(output.contains("rust"));
        assert!(output.contains("Running"));
        assert!(output.contains("python"));
        assert!(output.contains("Stopped"));
    }

    #[test]
    fn test_get_default_server_command() {
        assert_eq!(get_default_server_command("rust"), Some("rust-analyzer"));
        assert_eq!(get_default_server_command("typescript"), Some("typescript-language-server"));
        assert_eq!(get_default_server_command("python"), Some("pylsp"));
        assert_eq!(get_default_server_command("unknown"), None);
    }

    #[test]
    fn test_is_valid_language() {
        assert!(is_valid_language("rust"));
        assert!(is_valid_language("typescript"));
        assert!(is_valid_language("python"));
        assert!(is_valid_language("go"));
        assert!(is_valid_language("c"));
        assert!(is_valid_language("cpp"));
        assert!(is_valid_language("java"));
        assert!(!is_valid_language("unknown"));
    }

    #[test]
    fn test_lsp_server_status_display() {
        assert_eq!(LspServerStatus::Running.to_string(), "Running");
        assert_eq!(LspServerStatus::Stopped.to_string(), "Stopped");
        assert_eq!(LspServerStatus::Starting.to_string(), "Starting");
        assert_eq!(LspServerStatus::Error.to_string(), "Error");
    }
}
