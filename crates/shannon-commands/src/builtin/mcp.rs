//! /mcp command - MCP server management
//!
//! Provides subcommands for listing servers, viewing status,
//! restarting servers, and listing available tools.

#![allow(dead_code)]

use crate::command::{
    Command, CommandAvailability, CommandBase, CommandSource, ExecutionContext, PromptCommand,
};

/// MCP prompt template
const MCP_PROMPT: &str = r##"
MCP (Model Context Protocol) server management.

Arguments: {args}

Subcommands:
- **list** — Show all connected MCP servers with status and tool count
- **status [server]** — Detailed server info (state, uptime, request/error counts, capabilities)
- **restart [server]** — Gracefully restart a specific MCP server
- **tools [server]** — List available MCP tools with annotations (all tools, or filtered by server)
- **prompts [server]** — List MCP prompts exposed by servers; each prompt is invocable as `/<server>:<prompt>` or `/mcp__<server>__<prompt>`
- **help** — Show this help

If no subcommand is given, default to `list`.

When gathering information:
- For `list`, show a table of servers: name, state (Running/Stopped/Unhealthy/Starting), tool count
- For `status`, show: state, uptime, total requests, error count, restart count, last health check
- For `restart`, confirm the restart and report success/failure
- For `tools`, show: tool name, description, and annotations (R=read-only, D=destructive, I=idempotent, O=open-world)
"##;

/// Create the /mcp command
pub fn command() -> Command {
    Command::Prompt(Box::new(PromptCommand {
        base: CommandBase {
            name: "mcp".to_string(),
            aliases: vec!["servers".to_string()],
            description: "Manage MCP servers: list, status, restart, tools, prompts".to_string(),
            has_user_specified_description: false,
            availability: vec![CommandAvailability::All],
            source: CommandSource::Builtin,
            is_enabled: true,
            is_hidden: false,
            argument_hint: Some("[list|status|restart|tools|prompts] [server]".to_string()),
            when_to_use: Some(
                "To view or manage MCP server connections, check server health, or list available tools".to_string(),
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
        arg_names: vec!["subcommand".to_string(), "server".to_string()],
        allowed_tools: vec![],
        model: None,
        hooks: std::collections::HashMap::new(),
        context: ExecutionContext::Inline,
        agent: None,
        paths: vec![],
        prompt_template: Some(MCP_PROMPT.to_string()),
    }))
}

/// MCP subcommands
#[derive(Debug, Clone, PartialEq)]
pub enum McpSubcommand {
    /// List all servers
    List,
    /// Show detailed server status
    Status,
    /// Restart a server
    Restart,
    /// List available tools
    Tools,
    /// List available prompts
    Prompts,
    /// Show help
    Help,
}

/// Parse MCP subcommand from argument string.
pub fn parse_mcp_subcommand(arg: &str) -> McpSubcommand {
    match arg.to_lowercase().as_str() {
        "list" | "ls" => McpSubcommand::List,
        "status" | "info" => McpSubcommand::Status,
        "restart" | "reload" => McpSubcommand::Restart,
        "tools" | "tools-list" => McpSubcommand::Tools,
        "prompts" | "prompt-list" => McpSubcommand::Prompts,
        "help" | "?" => McpSubcommand::Help,
        _ => McpSubcommand::List,
    }
}

/// Format the MCP help output.
pub fn format_mcp_help() -> String {
    let mut output = String::from("MCP Server Management:\n\n");

    output.push_str("  /mcp list              - Show all MCP servers and their status\n");
    output.push_str("  /mcp status [server]   - Detailed server info (uptime, requests, errors)\n");
    output.push_str("  /mcp restart [server]  - Restart a specific MCP server\n");
    output.push_str("  /mcp tools [server]    - List available tools with annotations\n");
    output.push_str("  /mcp prompts [server]  - List MCP prompts (invoke as /<server>:<prompt>)\n");
    output.push_str("\nAnnotations:\n");
    output.push_str("  R = read-only   (safe to call without confirmation)\n");
    output.push_str("  D = destructive (may perform irreversible operations)\n");
    output.push_str("  I = idempotent  (same args produce same result)\n");
    output.push_str("  O = open-world  (interacts with external systems)\n");

    output
}

/// Format a server list for display.
pub fn format_server_list(servers: &[(String, ServerStateInfo)]) -> String {
    if servers.is_empty() {
        return "No MCP servers connected.".to_string();
    }

    let mut output = String::from("MCP Servers:\n\n");

    let name_width = servers
        .iter()
        .map(|(name, _)| name.len())
        .max()
        .unwrap_or(10)
        .max(4);

    for (name, info) in servers {
        output.push_str(&format!(
            "  {:<name_width$}  {:<10}  {} tool(s)\n",
            name,
            format!("{}", info.state),
            info.tool_count,
            name_width = name_width,
        ));
    }

    output
}

/// Format detailed server status.
pub fn format_server_status(name: &str, status: &ServerStatusInfo) -> String {
    let mut output = format!("MCP Server: {name}\n\n");

    output.push_str(&format!("  State:             {}\n", status.state));
    if let Some(ref uptime) = status.uptime {
        output.push_str(&format!(
            "  Uptime:            {}\n",
            format_duration(uptime)
        ));
    }
    output.push_str(&format!("  Requests:          {}\n", status.request_count));
    output.push_str(&format!("  Errors:            {}\n", status.error_count));
    output.push_str(&format!("  Restarts:          {}\n", status.restart_count));
    if let Some(ref last_check) = status.last_health_check {
        output.push_str(&format!(
            "  Last health check: {} ago\n",
            format_duration(last_check)
        ));
    }

    output
}

/// Format a tool list with annotations.
pub fn format_tool_list(tools: &[ToolInfo]) -> String {
    if tools.is_empty() {
        return "No tools available.".to_string();
    }

    let mut output = String::from("MCP Tools:\n\n");

    for tool in tools {
        let annotations = format_annotations(&tool.annotations);
        if annotations.is_empty() {
            output.push_str(&format!("  {} — {}\n", tool.name, tool.description));
        } else {
            output.push_str(&format!(
                "  {} [{}] — {}\n",
                tool.name, annotations, tool.description
            ));
        }
    }

    output
}

/// Info about an MCP prompt for display purposes.
#[derive(Debug, Clone, Default)]
pub struct PromptDisplayInfo {
    /// Name of the prompt.
    pub name: String,
    /// Server that exposes the prompt.
    pub server: String,
    /// Human-readable description.
    pub description: String,
    /// Declared argument names (may be empty).
    pub argument_names: Vec<String>,
}

/// Format a prompt list for display.
pub fn format_prompt_list(prompts: &[PromptDisplayInfo]) -> String {
    if prompts.is_empty() {
        return "No MCP prompts available.".to_string();
    }

    let mut output = String::from("MCP Prompts:\n\n");

    for prompt in prompts {
        let args = if prompt.argument_names.is_empty() {
            String::new()
        } else {
            format!(" --{} <value>", prompt.argument_names.join(" --"))
        };
        output.push_str(&format!(
            "  /{}:{}{}\n    {}\n",
            prompt.server, prompt.name, args, prompt.description
        ));
    }

    output.push_str("\nInvoke with /<server>:<prompt> or /mcp__<server>__<prompt>.");
    output
}

/// Format annotation flags as a compact string.
pub fn format_annotations(annotations: &AnnotationInfo) -> String {
    let mut flags = Vec::new();
    if annotations.read_only {
        flags.push("R");
    }
    if annotations.destructive {
        flags.push("D");
    }
    if annotations.idempotent {
        flags.push("I");
    }
    if annotations.open_world {
        flags.push("O");
    }
    flags.join("")
}

/// Format a duration in a human-readable form.
pub fn format_duration(duration: &std::time::Duration) -> String {
    let secs = duration.as_secs();
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    }
}

// ---------------------------------------------------------------------------
// Display/info types (no dependency on shannon-mcp types)
// ---------------------------------------------------------------------------

/// Server state for display purposes.
#[derive(Debug, Clone, PartialEq)]
pub enum ServerStateDisplay {
    Starting,
    Running,
    Stopped,
    Unhealthy,
}

impl std::fmt::Display for ServerStateDisplay {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ServerStateDisplay::Starting => write!(f, "Starting"),
            ServerStateDisplay::Running => write!(f, "Running"),
            ServerStateDisplay::Stopped => write!(f, "Stopped"),
            ServerStateDisplay::Unhealthy => write!(f, "Unhealthy"),
        }
    }
}

/// Server state info for formatting.
#[derive(Debug, Clone)]
pub struct ServerStateInfo {
    pub state: ServerStateDisplay,
    pub tool_count: usize,
}

/// Detailed server status for formatting.
#[derive(Debug, Clone)]
pub struct ServerStatusInfo {
    pub state: ServerStateDisplay,
    pub uptime: Option<std::time::Duration>,
    pub request_count: u64,
    pub error_count: u64,
    pub restart_count: u64,
    pub last_health_check: Option<std::time::Duration>,
}

/// Tool info for formatting.
#[derive(Debug, Clone)]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
    pub annotations: AnnotationInfo,
}

/// Tool annotation info for formatting.
#[derive(Debug, Clone, Default)]
pub struct AnnotationInfo {
    pub read_only: bool,
    pub destructive: bool,
    pub idempotent: bool,
    pub open_world: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_properties() {
        let cmd = command();
        assert_eq!(cmd.name(), "mcp");
        assert!(cmd.aliases().contains(&"servers".to_string()));
    }

    #[test]
    fn test_parse_mcp_subcommand() {
        assert_eq!(parse_mcp_subcommand("list"), McpSubcommand::List);
        assert_eq!(parse_mcp_subcommand("ls"), McpSubcommand::List);
        assert_eq!(parse_mcp_subcommand("status"), McpSubcommand::Status);
        assert_eq!(parse_mcp_subcommand("info"), McpSubcommand::Status);
        assert_eq!(parse_mcp_subcommand("restart"), McpSubcommand::Restart);
        assert_eq!(parse_mcp_subcommand("reload"), McpSubcommand::Restart);
        assert_eq!(parse_mcp_subcommand("tools"), McpSubcommand::Tools);
        assert_eq!(parse_mcp_subcommand("unknown"), McpSubcommand::List);
        assert_eq!(parse_mcp_subcommand("help"), McpSubcommand::Help);
    }

    #[test]
    fn test_format_mcp_help() {
        let help = format_mcp_help();
        assert!(help.contains("/mcp list"));
        assert!(help.contains("/mcp status"));
        assert!(help.contains("/mcp restart"));
        assert!(help.contains("/mcp tools"));
    }

    #[test]
    fn test_format_server_list_empty() {
        let output = format_server_list(&[]);
        assert!(output.contains("No MCP servers"));
    }

    #[test]
    fn test_format_server_list_with_servers() {
        let servers = vec![
            (
                "github".to_string(),
                ServerStateInfo {
                    state: ServerStateDisplay::Running,
                    tool_count: 5,
                },
            ),
            (
                "filesystem".to_string(),
                ServerStateInfo {
                    state: ServerStateDisplay::Stopped,
                    tool_count: 3,
                },
            ),
        ];
        let output = format_server_list(&servers);
        assert!(output.contains("github"));
        assert!(output.contains("Running"));
        assert!(output.contains("5 tool"));
        assert!(output.contains("filesystem"));
        assert!(output.contains("Stopped"));
    }

    #[test]
    fn test_format_server_status() {
        let status = ServerStatusInfo {
            state: ServerStateDisplay::Running,
            uptime: Some(std::time::Duration::from_secs(3661)),
            request_count: 42,
            error_count: 2,
            restart_count: 0,
            last_health_check: Some(std::time::Duration::from_secs(30)),
        };
        let output = format_server_status("github", &status);
        assert!(output.contains("github"));
        assert!(output.contains("Running"));
        assert!(output.contains("1h 1m"));
        assert!(output.contains("42"));
        assert!(output.contains("30s ago"));
    }

    #[test]
    fn test_format_tool_list() {
        let tools = vec![
            ToolInfo {
                name: "read_file".to_string(),
                description: "Read a file".to_string(),
                annotations: AnnotationInfo {
                    read_only: true,
                    ..Default::default()
                },
            },
            ToolInfo {
                name: "delete_file".to_string(),
                description: "Delete a file".to_string(),
                annotations: AnnotationInfo {
                    destructive: true,
                    open_world: true,
                    ..Default::default()
                },
            },
        ];
        let output = format_tool_list(&tools);
        assert!(output.contains("read_file [R]"));
        assert!(output.contains("delete_file [DO]"));
    }

    #[test]
    fn test_format_annotations() {
        let ann = AnnotationInfo {
            read_only: true,
            destructive: false,
            idempotent: true,
            open_world: false,
        };
        assert_eq!(format_annotations(&ann), "RI");

        let empty = AnnotationInfo::default();
        assert_eq!(format_annotations(&empty), "");
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(&std::time::Duration::from_secs(45)), "45s");
        assert_eq!(
            format_duration(&std::time::Duration::from_secs(125)),
            "2m 5s"
        );
        assert_eq!(
            format_duration(&std::time::Duration::from_secs(3661)),
            "1h 1m"
        );
    }

    #[test]
    fn test_server_state_display() {
        assert_eq!(ServerStateDisplay::Running.to_string(), "Running");
        assert_eq!(ServerStateDisplay::Stopped.to_string(), "Stopped");
        assert_eq!(ServerStateDisplay::Unhealthy.to_string(), "Unhealthy");
        assert_eq!(ServerStateDisplay::Starting.to_string(), "Starting");
    }

    #[test]
    fn test_parse_mcp_prompts_subcommand() {
        assert_eq!(parse_mcp_subcommand("prompts"), McpSubcommand::Prompts);
        assert_eq!(parse_mcp_subcommand("prompt-list"), McpSubcommand::Prompts);
    }

    #[test]
    fn test_format_mcp_help_includes_prompts() {
        let help = format_mcp_help();
        assert!(help.contains("/mcp prompts"));
        assert!(help.contains("/<server>:<prompt>"));
    }

    #[test]
    fn test_format_prompt_list_empty() {
        let output = format_prompt_list(&[]);
        assert!(output.contains("No MCP prompts"));
    }

    #[test]
    fn test_format_prompt_list_with_entries() {
        let prompts = vec![
            PromptDisplayInfo {
                name: "code-review".to_string(),
                server: "reviewer".to_string(),
                description: "Review code changes".to_string(),
                argument_names: vec!["file".to_string()],
            },
            PromptDisplayInfo {
                name: "summarize".to_string(),
                server: "writer".to_string(),
                description: "Summarize text".to_string(),
                argument_names: vec![],
            },
        ];
        let output = format_prompt_list(&prompts);
        assert!(output.contains("/reviewer:code-review"));
        assert!(output.contains("--file <value>"));
        assert!(output.contains("/writer:summarize"));
        assert!(output.contains("Review code changes"));
    }
}
