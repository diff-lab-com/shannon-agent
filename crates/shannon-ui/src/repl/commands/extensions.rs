use crate::{widgets::ChatRole, Result};

use super::super::Repl;

pub(crate) fn handle_mcp(repl: &mut Repl, args: &str) -> Result<()> {
    use serde::{Deserialize, Serialize};
    use std::collections::HashMap;
    use std::path::PathBuf;

    #[derive(Debug, Clone, Serialize, Deserialize, Default)]
    struct McpServerEntry {
        pub command: String,
        #[serde(default)]
        pub args: Vec<String>,
        #[serde(default)]
        pub env: HashMap<String, String>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, Default)]
    struct McpConfig {
        #[serde(default)]
        pub mcp_servers: HashMap<String, McpServerEntry>,
    }

    fn config_path() -> PathBuf {
        PathBuf::from(".shannon/mcp.json")
    }

    fn load_config() -> McpConfig {
        let path = config_path();
        if path.exists() {
            let content = std::fs::read_to_string(&path).unwrap_or_default();
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            McpConfig::default()
        }
    }

    fn save_config(config: &McpConfig) -> std::result::Result<(), String> {
        let path = config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("Failed to create .shannon dir: {e}"))?;
        }
        let content = serde_json::to_string_pretty(config).map_err(|e| format!("Failed to serialize: {e}"))?;
        std::fs::write(&path, content).map_err(|e| format!("Failed to write config: {e}"))?;
        Ok(())
    }

    let parts: Vec<&str> = args.splitn(4, ' ').collect();
    let subcommand = parts.first().copied().unwrap_or("help");

    match subcommand {
        "help" | "" => {
            repl.chat.add_message(ChatRole::System, "\
/mcp list                        — List configured MCP servers
/mcp add <name> <command> [args] — Add an MCP server
/mcp remove <name>               — Remove an MCP server
/mcp show <name>                 — Show server details
/mcp test <name>                 — Test server connection
/mcp approve <name>              — Approve a server for next startup
/mcp deny <name>                 — Deny a server
/mcp reset-approvals             — Clear all approval decisions
/mcp reload                      — Reload MCP config and restart servers
/mcp resources [server]          — List available MCP resources
/mcp subscribe <server> <uri>    — Subscribe to resource updates
/mcp unsubscribe <server> <uri>  — Unsubscribe from resource updates
/mcp path                        — Show config file path".to_string());
        }
        "list" => {
            let config = load_config();
            if config.mcp_servers.is_empty() {
                repl.chat.add_message(ChatRole::System, "No MCP servers configured. Use /mcp add <name> <command>.".to_string());
            } else {
                let mut out = format!("MCP servers ({}):\n", config.mcp_servers.len());
                for (name, entry) in &config.mcp_servers {
                    let args_str = if entry.args.is_empty() { String::new() } else { format!(" {}", entry.args.join(" ")) };
                    out.push_str(&format!("  {} → {}{}\n", name, entry.command, args_str));
                }
                out.push_str(&format!("\nConfig: {}", config_path().display()));
                repl.chat.add_message(ChatRole::System, out);
            }
        }
        "add" => {
            let name = parts.get(1).copied().unwrap_or("");
            let command = parts.get(2).copied().unwrap_or("");
            if name.is_empty() || command.is_empty() {
                repl.chat.add_message(ChatRole::System, "Usage: /mcp add <name> <command> [args...]".to_string());
                return Ok(());
            }
            let extra_args: Vec<String> = parts.get(3..)
                .map(|s| s.iter().map(|a| a.to_string()).collect())
                .unwrap_or_default();
            let mut config = load_config();
            let existed = config.mcp_servers.contains_key(name);
            config.mcp_servers.insert(name.to_string(), McpServerEntry {
                command: command.to_string(),
                args: extra_args,
                env: HashMap::new(),
            });
            match save_config(&config) {
                Ok(()) => {
                    if existed {
                        repl.chat.add_message(ChatRole::System, format!("Updated MCP server '{name}' → {command}"));
                    } else {
                        repl.chat.add_message(ChatRole::System, format!("Added MCP server '{name}' → {command}"));
                    }
                }
                Err(e) => {
                    super::set_error(repl, &format!("saving config: {e}"));
                }
            }
        }
        "remove" => {
            let name = parts.get(1).copied().unwrap_or("");
            if name.is_empty() {
                repl.chat.add_message(ChatRole::System, "Usage: /mcp remove <name>".to_string());
                return Ok(());
            }
            let mut config = load_config();
            if config.mcp_servers.remove(name).is_some() {
                match save_config(&config) {
                    Ok(()) => { repl.chat.add_message(ChatRole::System, format!("Removed MCP server '{name}'.")); }
                    Err(e) => { super::set_error(repl, &format!("saving config: {e}")); }
                }
            } else {
                repl.chat.add_message(ChatRole::System, format!("Server '{name}' not found in config."));
            }
        }
        "show" => {
            let name = parts.get(1).copied().unwrap_or("");
            if name.is_empty() {
                repl.chat.add_message(ChatRole::System, "Usage: /mcp show <name>".to_string());
                return Ok(());
            }
            let config = load_config();
            match config.mcp_servers.get(name) {
                Some(entry) => {
                    let env_str = if entry.env.is_empty() {
                        "none".to_string()
                    } else {
                        entry.env.keys().cloned().collect::<Vec<_>>().join(", ")
                    };
                    repl.chat.add_message(ChatRole::System, format!(
                        "Server: {}\n  Command: {}\n  Args: {}\n  Env vars: {}",
                        name, entry.command,
                        if entry.args.is_empty() { "(none)".to_string() } else { entry.args.join(" ") },
                        env_str,
                    ));
                }
                None => {
                    repl.chat.add_message(ChatRole::System, format!("Server '{name}' not found."));
                }
            }
        }
        "test" => {
            let name = parts.get(1).copied().unwrap_or("");
            if name.is_empty() {
                repl.chat.add_message(ChatRole::System, "Usage: /mcp test <name>".to_string());
                return Ok(());
            }
            let config = load_config();
            match config.mcp_servers.get(name) {
                Some(entry) => {
                    repl.chat.add_message(ChatRole::System, format!("Testing connection to '{name}'..."));
                    // Try to create a stdio transport and check if the command exists
                    let command = &entry.command;
                    let which_output = std::process::Command::new("which")
                        .arg(command)
                        .output();
                    match which_output {
                        Ok(output) if output.status.success() => {
                            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                            repl.chat.add_message(ChatRole::System, format!(
                                "Server '{name}': command found at {path}. Ready to connect.",
                            ));
                        }
                        Ok(_) => {
                            repl.chat.add_message(ChatRole::System, format!(
                                "Server '{name}': command '{command}' not found in PATH.",
                            ));
                        }
                        Err(e) => {
                            repl.chat.add_message(ChatRole::System, format!("Test failed: {e}"));
                        }
                    }
                }
                None => {
                    repl.chat.add_message(ChatRole::System, format!("Server '{name}' not found."));
                }
            }
        }
        "path" => {
            repl.chat.add_message(ChatRole::System, format!("MCP config: {}", config_path().display()));
        }
        "approve" => {
            let name = parts.get(1).copied().unwrap_or("");
            if name.is_empty() {
                repl.chat.add_message(ChatRole::System, "Usage: /mcp approve <name>".to_string());
                return Ok(());
            }
            let approval_path = PathBuf::from(".shannon/mcp_approvals.json");
            let mut mgr = shannon_core::McpApprovalManager::with_defaults();
            let _ = mgr.load_from_file(&approval_path);
            mgr.approve_server(name);
            match mgr.save_to_file(&approval_path) {
                Ok(()) => { repl.chat.add_message(ChatRole::System, format!("Approved '{name}'. It will connect on next startup.")); }
                Err(e) => { super::set_error(repl, &format!("saving approval: {e}")); }
            }
        }
        "deny" => {
            let name = parts.get(1).copied().unwrap_or("");
            if name.is_empty() {
                repl.chat.add_message(ChatRole::System, "Usage: /mcp deny <name>".to_string());
                return Ok(());
            }
            let approval_path = PathBuf::from(".shannon/mcp_approvals.json");
            let mut mgr = shannon_core::McpApprovalManager::with_defaults();
            let _ = mgr.load_from_file(&approval_path);
            mgr.deny_server(name);
            match mgr.save_to_file(&approval_path) {
                Ok(()) => { repl.chat.add_message(ChatRole::System, format!("Denied '{name}'. It will be skipped on next startup.")); }
                Err(e) => { super::set_error(repl, &format!("saving denial: {e}")); }
            }
        }
        "reset-approvals" => {
            let approval_path = PathBuf::from(".shannon/mcp_approvals.json");
            match shannon_core::McpApprovalManager::reset_persisted(&approval_path) {
                Ok(()) => { repl.chat.add_message(ChatRole::System, "All approval decisions cleared. Servers will be re-evaluated on next startup.".to_string()); }
                Err(e) => { super::set_error(repl, &format!("resetting approvals: {e}")); }
            }
        }
        "reload" => {
            let cwd = std::env::current_dir().unwrap_or_default();
            match shannon_mcp::config::discover_config(&cwd) {
                Ok(config) => {
                    let pool = repl.mcp_pool.clone();
                    let changes = repl.runtime.block_on(pool.reload_from_config(&config));
                    match changes {
                        Ok(changes) => {
                            // Discover tools from newly started servers and register them
                            let mut new_tool_count = 0;
                            let new_servers: Vec<String> = changes.iter()
                                .filter(|c| c.starts_with("Started "))
                                .map(|c| {
                                    // Extract server name from "Started stdio server 'name'" etc.
                                    let s = c.trim_start_matches("Started ");
                                    s.split('\'').nth(1).unwrap_or("").to_string()
                                })
                                .filter(|s| !s.is_empty())
                                .collect();

                            if !new_servers.is_empty() {
                                let registry = repl.tool_registry.clone();
                                for server_name in &new_servers {
                                    let result = repl.runtime.block_on(
                                        pool.send_batch_server_request(
                                            server_name,
                                            vec![("tools/list", serde_json::json!({}))],
                                        )
                                    );
                                    if let Ok(responses) = result {
                                        if let Some((_, Ok(response))) = responses.first() {
                                            if let Some(tools_array) = response.get("tools").and_then(|t| t.as_array()) {
                                                for tool_value in tools_array {
                                                    let tool_name = tool_value.get("name")
                                                        .and_then(|n| n.as_str())
                                                        .unwrap_or("unknown")
                                                        .to_string();
                                                    let description = tool_value.get("description")
                                                        .and_then(|d| d.as_str())
                                                        .unwrap_or("")
                                                        .to_string();
                                                    let input_schema = tool_value.get("inputSchema")
                                                        .cloned()
                                                        .unwrap_or(serde_json::json!({"type": "object"}));
                                                    let annotations: Option<shannon_mcp::ToolAnnotations> =
                                                        tool_value.get("annotations")
                                                        .and_then(|a| serde_json::from_value(a.clone()).ok());

                                                    let adapter = shannon_mcp::PooledMcpToolAdapter::new(
                                                        pool.clone(),
                                                        server_name.clone(),
                                                        tool_name,
                                                        description,
                                                        input_schema,
                                                        annotations,
                                                    );
                                                    if let Err(e) = registry.register(Box::new(adapter)) {
                                                        tracing::warn!("Failed to register MCP tool: {e}");
                                                    } else {
                                                        new_tool_count += 1;
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }

                            // Report prompts from all connected servers and register them as slash commands
                            let all_prompts = repl.runtime.block_on(pool.list_all_prompts());

                            // Register MCP prompts as slash commands: /mcp__{server}__{prompt}
                            let mut new_prompt_count = 0;
                            for (server_name, prompts) in &all_prompts {
                                for prompt in prompts {
                                    let cmd_name = format!("mcp__{}__{}", server_name, prompt.name);
                                    let arg_names: Vec<String> = prompt.arguments
                                        .as_ref()
                                        .map(|args| args.iter().map(|a| a.name.clone()).collect())
                                        .unwrap_or_default();
                                    let arg_hint = if arg_names.is_empty() { None } else { Some(arg_names.join(", ")) };
                                    let prompt_template = format!(
                                        "Use the get_mcp_prompt tool to retrieve and execute the '{}' prompt from the '{}' MCP server with these arguments: {{args}}",
                                        prompt.name, server_name
                                    );
                                    use shannon_commands::{Command, CommandBase, ExecutionContext, PromptCommand};
                                    use std::collections::HashMap;
                                    let command = Command::Prompt(Box::new(PromptCommand {
                                        base: CommandBase {
                                            name: cmd_name,
                                            aliases: Vec::new(),
                                            description: prompt.description.clone(),
                                            has_user_specified_description: false,
                                            availability: vec![shannon_commands::CommandAvailability::All],
                                            source: shannon_commands::CommandSource::Builtin,
                                            is_enabled: true,
                                            is_hidden: false,
                                            argument_hint: arg_hint,
                                            when_to_use: None,
                                            version: None,
                                            disable_model_invocation: false,
                                            user_invocable: true,
                                            is_workflow: false,
                                            immediate: false,
                                            is_sensitive: false,
                                            user_facing_name: None,
                                        },
                                        progress_message: format!("Loading MCP prompt '{}' from '{}'", prompt.name, server_name),
                                        content_length: 0,
                                        arg_names,
                                        allowed_tools: vec!["get_mcp_prompt".to_string()],
                                        model: None,
                                        hooks: HashMap::new(),
                                        context: ExecutionContext::Inline,
                                        agent: None,
                                        paths: Vec::new(),
                                        prompt_template: Some(prompt_template),
                                    }));
                                    repl.command_registry.register_sync(command);
                                    new_prompt_count += 1;
                                }
                            }

                            let prompt_count: usize = all_prompts.iter().map(|(_, p)| p.len()).sum();

                            let mut msg = if changes.is_empty() {
                                "MCP config reloaded — no changes detected.".to_string()
                            } else {
                                let mut m = format!("MCP config reloaded ({} change(s)):\n", changes.len());
                                for change in &changes {
                                    m.push_str(&format!("  • {change}\n"));
                                }
                                m
                            };
                            if new_tool_count > 0 {
                                msg.push_str(&format!("  • Registered {new_tool_count} new tool(s)\n"));
                            }
                            if new_prompt_count > 0 {
                                msg.push_str(&format!("  • Registered {new_prompt_count} prompt command(s)\n"));
                            }
                            msg.push_str(&format!("  • {prompt_count} prompt(s) available from {} server(s)\n", all_prompts.len()));
                            repl.chat.add_message(ChatRole::System, msg);
                        }
                        Err(e) => {
                            repl.chat.add_message(ChatRole::System, format!("MCP reload failed: {e}"));
                        }
                    }
                }
                Err(e) => {
                    super::set_error(repl, &format!("discovering MCP config: {e}"));
                }
            }
        }
        "resources" => {
            let server = parts.get(1).copied().unwrap_or("");
            let pool = repl.mcp_pool.clone();
            if server.is_empty() {
                // List resources from all servers that support them
                let servers = repl.runtime.block_on(pool.list_servers());
                let mut msg = String::new();
                for (name, _) in &servers {
                    let has_res = repl.runtime.block_on(pool.has_resources(name));
                    if has_res {
                        let result = repl.runtime.block_on(
                            pool.send_batch_server_request(name, vec![("resources/list", serde_json::json!({}))])
                        );
                        match result {
                            Ok(responses) => {
                                if let Some((_, Ok(response))) = responses.first() {
                                    if let Some(resources) = response.get("resources").and_then(|r| r.as_array()) {
                                        if !resources.is_empty() {
                                            msg.push_str(&format!("  {name}:\n"));
                                            for res in resources {
                                                let uri = res.get("uri").and_then(|u| u.as_str()).unwrap_or("?");
                                                let name_field = res.get("name").and_then(|n| n.as_str()).unwrap_or("");
                                                msg.push_str(&format!("    {uri} ({name_field})\n"));
                                            }
                                        }
                                    }
                                }
                            }
                            Err(e) => { msg.push_str(&format!("  {name}: error — {e}\n")); }
                        }
                    }
                }
                if msg.is_empty() {
                    repl.chat.add_message(ChatRole::System, "No MCP servers with resource support found.".to_string());
                } else {
                    repl.chat.add_message(ChatRole::System, format!("MCP Resources:\n{msg}"));
                }
            } else {
                let result = repl.runtime.block_on(
                    pool.send_batch_server_request(server, vec![("resources/list", serde_json::json!({}))])
                );
                match result {
                    Ok(responses) => {
                        if let Some((_, Ok(response))) = responses.first() {
                            if let Some(resources) = response.get("resources").and_then(|r| r.as_array()) {
                                if resources.is_empty() {
                                    repl.chat.add_message(ChatRole::System, format!("Server '{server}' has no resources."));
                                } else {
                                    let mut msg = format!("Resources from '{server}':\n");
                                    for res in resources {
                                        let uri = res.get("uri").and_then(|u| u.as_str()).unwrap_or("?");
                                        let name_field = res.get("name").and_then(|n| n.as_str()).unwrap_or("");
                                        msg.push_str(&format!("  {uri} ({name_field})\n"));
                                    }
                                    repl.chat.add_message(ChatRole::System, msg);
                                }
                            } else {
                                repl.chat.add_message(ChatRole::System, format!("Server '{server}' returned no resource list."));
                            }
                        } else {
                            super::set_error(repl, &format!("listing resources from '{server}'"));
                        }
                    }
                    Err(e) => { super::set_error(repl, &format!("{e}")); }
                }
            }
        }
        "subscribe" => {
            let server = parts.get(1).copied().unwrap_or("");
            let uri = parts.get(2).copied().unwrap_or("");
            if server.is_empty() || uri.is_empty() {
                repl.chat.add_message(ChatRole::System, "Usage: /mcp subscribe <server> <resource_uri>".to_string());
                return Ok(());
            }
            let pool = repl.mcp_pool.clone();
            let result = repl.runtime.block_on(
                pool.send_batch_server_request(server, vec![("resources/subscribe", serde_json::json!({"uri": uri}))])
            );
            match result {
                Ok(responses) => {
                    if let Some((_, Ok(_))) = responses.first() {
                        repl.chat.add_message(ChatRole::System, format!("Subscribed to '{uri}' on '{server}'."));
                    } else {
                        repl.chat.add_message(ChatRole::System, format!("Server '{server}' did not confirm subscription."));
                    }
                }
                Err(e) => { repl.chat.add_message(ChatRole::System, format!("Subscribe failed: {e}")); }
            }
        }
        "unsubscribe" => {
            let server = parts.get(1).copied().unwrap_or("");
            let uri = parts.get(2).copied().unwrap_or("");
            if server.is_empty() || uri.is_empty() {
                repl.chat.add_message(ChatRole::System, "Usage: /mcp unsubscribe <server> <resource_uri>".to_string());
                return Ok(());
            }
            let pool = repl.mcp_pool.clone();
            let result = repl.runtime.block_on(
                pool.send_batch_server_request(server, vec![("resources/unsubscribe", serde_json::json!({"uri": uri}))])
            );
            match result {
                Ok(responses) => {
                    if let Some((_, Ok(_))) = responses.first() {
                        repl.chat.add_message(ChatRole::System, format!("Unsubscribed from '{uri}' on '{server}'."));
                    } else {
                        repl.chat.add_message(ChatRole::System, format!("Server '{server}' did not confirm unsubscription."));
                    }
                }
                Err(e) => { repl.chat.add_message(ChatRole::System, format!("Unsubscribe failed: {e}")); }
            }
        }
        _ => {
            repl.chat.add_message(ChatRole::System, format!("Unknown subcommand: {subcommand}. Use /mcp help."));
        }
    }

    Ok(())
}

pub(crate) fn handle_agents(repl: &mut Repl, args: &str) -> Result<()> {
    use shannon_agents::{AgentCoordinator, CoordinatorConfig, SubAgentRegistry, AgentConfig};

    let parts: Vec<&str> = args.splitn(3, ' ').collect();
    let subcommand = parts.first().copied().unwrap_or("help");

    // Lazily initialize agent registry if needed
    fn ensure_registry(repl: &mut Repl) {
        if repl.agent_registry.is_none() {
            let config = CoordinatorConfig::default();
            let coordinator = match repl.runtime.block_on(AgentCoordinator::new(config)) {
                Ok(c) => c,
                Err(e) => {
                    super::set_error(repl, &format!("creating agent coordinator: {e}"));
                    return;
                }
            };
            repl.agent_registry = Some(std::sync::Arc::new(SubAgentRegistry::new(
                std::sync::Arc::new(coordinator),
            )));
        }
    }

    match subcommand {
        "help" | "" => {
            repl.chat.add_message(ChatRole::System, "\
/agents spawn <name> <prompt>  — Spawn a background agent
/agents list                   — List all agents and status
/agents status <name>          — Show agent details
/agents message <name> <text>  — Send message to agent
/agents kill <name>            — Kill a running agent
/agents run-bg <name> <task>   — Run task in background with notification".to_string());
        }
        "spawn" => {
            let name = parts.get(1).copied().unwrap_or("");
            let prompt = parts.get(2).copied().unwrap_or("");
            if name.is_empty() || prompt.is_empty() {
                repl.chat.add_message(ChatRole::System, "Usage: /agents spawn <name> <system-prompt>".to_string());
                return Ok(());
            }
            ensure_registry(repl);
            let registry = match repl.agent_registry.as_ref() {
                Some(r) => r.clone(),
                None => return Ok(()),
            };
            let config = AgentConfig {
                name: name.to_string(),
                system_prompt: prompt.to_string(),
                ..Default::default()
            };
            match repl.runtime.block_on(registry.spawn(config)) {
                Ok(agent) => {
                    repl.chat.add_message(ChatRole::System, format!(
                        "Agent '{}' spawned (id: {}, status: {})",
                        agent.name, agent.id, agent.status
                    ));
                }
                Err(e) => {
                    super::set_error(repl, &format!("spawning agent: {e}"));
                }
            }
        }
        "list" => {
            ensure_registry(repl);
            let registry = match repl.agent_registry.as_ref() {
                Some(r) => r.clone(),
                None => return Ok(()),
            };
            let agents = repl.runtime.block_on(registry.list_agents());
            if agents.is_empty() {
                repl.chat.add_message(ChatRole::System, "No agents spawned yet.".to_string());
            } else {
                let mut out = format!("Agents ({}):\n", agents.len());
                for a in &agents {
                    out.push_str(&format!(
                        "  {} [{}] model={} turns={}/{}{}\n",
                        a.name, a.status, a.config.model,
                        a.turns_used, a.config.max_turns,
                        a.team.as_ref().map(|t| format!(" team={t}")).unwrap_or_default(),
                    ));
                }
                repl.chat.add_message(ChatRole::System, out);
            }
        }
        "status" => {
            let name = parts.get(1).copied().unwrap_or("");
            if name.is_empty() {
                repl.chat.add_message(ChatRole::System, "Usage: /agents status <name>".to_string());
                return Ok(());
            }
            ensure_registry(repl);
            let registry = match repl.agent_registry.as_ref() {
                Some(r) => r.clone(),
                None => return Ok(()),
            };
            match repl.runtime.block_on(registry.get_agent(name)) {
                Some(agent) => {
                    repl.chat.add_message(ChatRole::System, format!(
                        "Agent: {}\n  ID: {}\n  Status: {}\n  Model: {}\n  Turns: {}/{}\n  Team: {}\n  Created: {}{}",
                        agent.name, agent.id, agent.status, agent.config.model,
                        agent.turns_used, agent.config.max_turns,
                        agent.team.as_deref().unwrap_or("none"),
                        agent.created_at.to_rfc3339(),
                        agent.last_output.as_ref().map(|o| format!("\n  Last output: {}", if o.len() > 200 { &o[..200] } else { o })).unwrap_or_default(),
                    ));
                }
                None => {
                    repl.chat.add_message(ChatRole::System, format!("Agent '{name}' not found."));
                }
            }
        }
        "message" => {
            let name = parts.get(1).copied().unwrap_or("");
            let msg = parts.get(2).copied().unwrap_or("");
            if name.is_empty() || msg.is_empty() {
                repl.chat.add_message(ChatRole::System, "Usage: /agents message <name> <text>".to_string());
                return Ok(());
            }
            ensure_registry(repl);
            let registry = match repl.agent_registry.as_ref() {
                Some(r) => r.clone(),
                None => return Ok(()),
            };
            match repl.runtime.block_on(registry.send_message("repl", name, serde_json::json!(msg))) {
                Ok(responses) => {
                    let mut out = format!("Message sent to '{name}', {} response(s):\n", responses.len());
                    for r in responses {
                        let content = match &r.content {
                            shannon_agents::MessageContent::Text(t) => t.clone(),
                            shannon_agents::MessageContent::Structured(v) => v.to_string(),
                            shannon_agents::MessageContent::Protocol(p) => format!("{p:?}"),
                        };
                        out.push_str(&format!("  [{}] {}\n", r.from, content));
                    }
                    repl.chat.add_message(ChatRole::System, out);
                }
                Err(e) => {
                    super::set_error(repl, &format!("sending message: {e}"));
                }
            }
        }
        "kill" => {
            let name = parts.get(1).copied().unwrap_or("");
            if name.is_empty() {
                repl.chat.add_message(ChatRole::System, "Usage: /agents kill <name>".to_string());
                return Ok(());
            }
            ensure_registry(repl);
            let registry = match repl.agent_registry.as_ref() {
                Some(r) => r.clone(),
                None => return Ok(()),
            };
            match repl.runtime.block_on(registry.get_agent(name)) {
                Some(mut agent) => {
                    agent.mark_failed("killed by user".to_string());
                    repl.chat.add_message(ChatRole::System, format!("Agent '{name}' killed."));
                }
                None => {
                    repl.chat.add_message(ChatRole::System, format!("Agent '{name}' not found."));
                }
            }
        }
        "run-bg" => {
            use shannon_agents::{MultiAgentSpawner, SpawnAgentConfig, MultiAgentConfig, shared_executor};

            let name = parts.get(1).copied().unwrap_or("");
            let task = parts.get(2).copied().unwrap_or("");
            if name.is_empty() || task.is_empty() {
                repl.chat.add_message(ChatRole::System, "Usage: /agents run-bg <name> <task>".to_string());
                return Ok(());
            }
            let agent_config = SpawnAgentConfig::new(name.to_string(), task.to_string());
            let config = MultiAgentConfig::new(vec![agent_config]);

            let executor = repl.query_engine.as_ref().map(|engine| {
                let client = engine.client().clone();
                shared_executor(client)
            });

            repl.chat.add_message(ChatRole::System, format!("Running agent '{name}'..."));
            let result = repl.runtime.block_on(MultiAgentSpawner::spawn(config, executor));
            let status = if result.all_succeeded() { "completed" } else { "failed" };

            // Show output from agent if available
            if let Some(ar) = result.agent_results.first() {
                if let Some(ref output) = ar.output {
                    let preview = if output.content.len() > 500 {
                        format!("{}...", &output.content[..500])
                    } else {
                        output.content.clone()
                    };
                    repl.chat.add_message(ChatRole::System, format!(
                        "Agent '{}' {} in {:.1}s:\n{}",
                        name, status, result.total_duration.as_secs_f64(), preview,
                    ));
                } else {
                    repl.chat.add_message(ChatRole::System, format!(
                        "Agent '{}' {} in {:.1}s",
                        name, status, result.total_duration.as_secs_f64(),
                    ));
                }
            }
        }
        _ => {
            repl.chat.add_message(ChatRole::System, format!("Unknown subcommand: {subcommand}. Use /agents help."));
        }
    }

    Ok(())
}

pub(crate) fn handle_team(repl: &mut Repl, args: &str) -> Result<()> {
    use shannon_agents::{AgentCoordinator, CoordinatorConfig, TeammateConfig, TaskPriority};

    let parts: Vec<&str> = args.splitn(4, ' ').collect();
    let subcommand = parts.first().copied().unwrap_or("help");

    match subcommand {
        "help" | "" => {
            repl.chat.add_message(ChatRole::System, "\
/team create <name> [description]  — Create a new agent team
/team add <team> <agent-name>  — Add agent to team
/team task <team> <subject>  — Add a task
/team assign <team>  — Assign pending tasks to available agents
/team status [team]  — Show team status
/team list  — List all teams
/team run  — Execute pending tasks in parallel
/team shutdown  — Shutdown team
/team disband <team>  — Disband team and clean up
/team delegate  — Toggle delegate mode (lead only coordinates)".to_string());
        }
        "create" => {
            let name = parts.get(1).copied().unwrap_or("");
            if name.is_empty() {
                repl.chat.add_message(ChatRole::System, "Usage: /team create <name> [description]".to_string());
                return Ok(());
            }
            let description = parts.get(2..).map(|s| s.join(" ")).unwrap_or_default();
            let config = CoordinatorConfig::default();
            match repl.runtime.block_on(AgentCoordinator::new(config)) {
                Ok(coordinator) => {
                    match repl.runtime.block_on(coordinator.create_team(name.to_string(), description)) {
                        Ok(()) => {
                            repl.team_coordinator = Some(std::sync::Arc::new(coordinator));
                            repl.chat.add_message(ChatRole::System, format!("Team '{name}' created."));
                        }
                        Err(e) => { super::set_error(repl, &format!("creating team: {e}")); }
                    }
                }
                Err(e) => { super::set_error(repl, &format!("initializing coordinator: {e}")); }
            }
        }
        "add" => {
            let team_name = parts.get(1).copied().unwrap_or("");
            let agent_name = parts.get(2).copied().unwrap_or("");
            if team_name.is_empty() || agent_name.is_empty() {
                repl.chat.add_message(ChatRole::System, "Usage: /team add <team> <agent-name>".to_string());
                return Ok(());
            }
            if let Some(ref coordinator) = repl.team_coordinator {
                let config = TeammateConfig::default();
                match repl.runtime.block_on(coordinator.add_teammate(team_name, agent_name.to_string(), config)) {
                    Ok(()) => {
                        let worktree_msg = match create_agent_worktree(repl, agent_name) {
                            Ok(path) => format!(" (worktree: {})", path.display()),
                            Err(reason) => format!(" (worktree skipped: {reason})"),
                        };
                        repl.chat.add_message(ChatRole::System, format!("Agent '{agent_name}' added to team '{team_name}'.{worktree_msg}"));
                    }
                    Err(e) => { super::set_error(repl, &format!("adding agent: {e}")); }
                }
            } else {
                repl.chat.add_message(ChatRole::System, "No team created yet. Use /team create first.".to_string());
            }
        }
        "task" => {
            let team_name = parts.get(1).copied().unwrap_or("");
            let subject = parts.get(2..).map(|s| s.join(" ")).unwrap_or_default();
            if team_name.is_empty() || subject.is_empty() {
                repl.chat.add_message(ChatRole::System, "Usage: /team task <team> <subject>".to_string());
                return Ok(());
            }
            if let Some(ref coordinator) = repl.team_coordinator {
                match repl.runtime.block_on(coordinator.add_task(team_name, subject.clone(), String::new(), TaskPriority::Medium)) {
                    Ok(task_id) => { repl.chat.add_message(ChatRole::System, format!("Task added to '{team_name}': {subject} (id: {task_id})")); }
                    Err(e) => { super::set_error(repl, &format!("adding task: {e}")); }
                }
            } else {
                repl.chat.add_message(ChatRole::System, "No team created yet. Use /team create first.".to_string());
            }
        }
        "assign" => {
            let team_name = parts.get(1).copied().unwrap_or("");
            if team_name.is_empty() {
                repl.chat.add_message(ChatRole::System, "Usage: /team assign <team>".to_string());
                return Ok(());
            }
            if let Some(ref coordinator) = repl.team_coordinator {
                match repl.runtime.block_on(coordinator.assign_task(team_name, uuid::Uuid::nil())) {
                    Ok(agent) => { repl.chat.add_message(ChatRole::System, format!("Task assigned to '{agent}' in team '{team_name}'.")); }
                    Err(e) => { super::set_error(repl, &format!("assigning task: {e}")); }
                }
            } else {
                repl.chat.add_message(ChatRole::System, "No team created yet. Use /team create first.".to_string());
            }
        }
        "status" => {
            let team_name = parts.get(1).copied().unwrap_or("");
            if let Some(ref coordinator) = repl.team_coordinator {
                if team_name.is_empty() {
                    let teams = repl.runtime.block_on(coordinator.list_teams());
                    if teams.is_empty() {
                        repl.chat.add_message(ChatRole::System, "No teams created yet.".to_string());
                    } else {
                        repl.chat.add_message(ChatRole::System, format!("Teams:\n{}", teams.iter().map(|t| format!("  - {t}")).collect::<Vec<_>>().join("\n")));
                    }
                } else {
                    match repl.runtime.block_on(coordinator.team_status(team_name)) {
                        Ok(status) => { repl.chat.add_message(ChatRole::System, status); }
                        Err(e) => { super::set_error(repl, &format!("getting status: {e}")); }
                    }
                }
            } else {
                repl.chat.add_message(ChatRole::System, "No team created yet. Use /team create first.".to_string());
            }
        }
        "list" => {
            if let Some(ref coordinator) = repl.team_coordinator {
                let teams = repl.runtime.block_on(coordinator.list_teams());
                if teams.is_empty() {
                    repl.chat.add_message(ChatRole::System, "No teams created yet.".to_string());
                } else {
                    repl.chat.add_message(ChatRole::System, format!("Teams:\n{}", teams.iter().map(|t| format!("  - {t}")).collect::<Vec<_>>().join("\n")));
                }
            } else {
                repl.chat.add_message(ChatRole::System, "No team created yet. Use /team create first.".to_string());
            }
        }
        "shutdown" => {
            if let Some(ref coordinator) = repl.team_coordinator {
                match repl.runtime.block_on(coordinator.shutdown()) {
                    Ok(()) => { repl.chat.add_message(ChatRole::System, "Team shut down.".to_string()); }
                    Err(e) => { super::set_error(repl, &format!("shutting down: {e}")); }
                }
            } else {
                repl.chat.add_message(ChatRole::System, "No active team.".to_string());
            }
        }
        "disband" => {
            let team_name = parts.get(1).copied().unwrap_or("");
            if team_name.is_empty() {
                repl.chat.add_message(ChatRole::System, "Usage: /team disband <team>".to_string());
                return Ok(());
            }
            if let Some(ref coordinator) = repl.team_coordinator {
                match repl.runtime.block_on(coordinator.disband_team(team_name)) {
                    Ok(()) => { repl.chat.add_message(ChatRole::System, format!("Team '{team_name}' disbanded and cleaned up.")); }
                    Err(e) => { super::set_error(repl, &format!("disbanding team: {e}")); }
                }
            } else {
                repl.chat.add_message(ChatRole::System, "No active team coordinator.".to_string());
            }
        }
        "delegate" => {
            if let Some(ref coordinator) = repl.team_coordinator {
                let current = coordinator.delegate_mode();
                coordinator.set_delegate_mode(!current);
                let state = if !current { "ON" } else { "OFF" };
                repl.chat.add_message(ChatRole::System, format!("Delegate mode: {state}"));
            } else {
                repl.chat.add_message(ChatRole::System, "No active team coordinator.".to_string());
            }
        }
        "run" => {
            use shannon_agents::{MultiAgentSpawner, SpawnAgentConfig, shared_executor};
            if let Some(ref coordinator) = repl.team_coordinator {
                let task_board = coordinator.task_board();
                let ready_tasks = repl.runtime.block_on(task_board.list_ready_tasks());
                if ready_tasks.is_empty() {
                    repl.chat.add_message(ChatRole::System, "No pending tasks to execute.".to_string());
                    return Ok(());
                }
                let agent_configs: Vec<SpawnAgentConfig> = ready_tasks
                    .iter().map(|t| SpawnAgentConfig::new(format!("agent-{}", t.id), t.subject.clone())).collect();
                let mut config = shannon_agents::MultiAgentConfig::new(agent_configs);
                config.default_system_prompt = Some("You are a helpful AI coding assistant. Complete the assigned task concisely and accurately.".to_string());
                // Create executor from the REPL's LLM client if available
                let executor = repl.query_engine.as_ref().map(|engine| {
                    let client = engine.client().clone();
                    shared_executor(client)
                });
                repl.chat.add_message(ChatRole::System, "Starting parallel execution...".to_string());
                let result = repl.runtime.block_on(MultiAgentSpawner::spawn(config, executor));
                let mut report = format!(
                    "Execution complete: {} succeeded, {} failed ({:.1}s)\n",
                    result.success_count, result.failure_count, result.total_duration.as_secs_f64(),
                );
                for ar in &result.agent_results {
                    report.push_str(&format!(
                        "  [{}] {} ({:.1}s){}\n",
                        ar.status, ar.agent_name, ar.duration.as_secs_f64(),
                        ar.error.as_ref().map(|e| format!(" — {e}")).unwrap_or_default(),
                    ));
                    if let Some(ref output) = ar.output {
                        let preview = if output.content.len() > 300 {
                            format!("{}...", &output.content[..300])
                        } else {
                            output.content.clone()
                        };
                        report.push_str(&format!("    {}\n", preview.trim()));
                    }
                }
                repl.chat.add_message(ChatRole::System, report);
            } else {
                repl.chat.add_message(ChatRole::System, "No team created yet. Use /team create first.".to_string());
            }
        }
        _ => {
            repl.chat.add_message(ChatRole::System, format!("Unknown subcommand: {subcommand}. Use /team help."));
        }
    }

    Ok(())
}

pub(crate) fn handle_route(repl: &mut Repl, args: &str) -> Result<()> {
    let parts: Vec<&str> = args.splitn(3, ' ').collect();
    let subcommand = parts.first().copied().unwrap_or("help");

    match subcommand {
        "help" | "" => {
            repl.chat.add_message(ChatRole::System, "\
/route add <pattern> <model>   — Add a routing rule (pattern is case-insensitive substring match)
/route remove <pattern>        — Remove a routing rule
/route list                    — Show all routing rules
/route clear                   — Remove all routing rules
/route test <query>            — Test which model a query would route to

Patterns match against the start of your query. Examples:
  /route add explain claude-haiku-4-5     — 'explain ...' queries use haiku
  /route add refactor claude-opus-4-6     — 'refactor ...' queries use opus
  /route add test claude-sonnet-4-6       — 'test ...' queries use sonnet".to_string());
        }
        "add" => {
            let pattern = parts.get(1).copied().unwrap_or("");
            let model = parts.get(2).copied().unwrap_or("");
            if pattern.is_empty() || model.is_empty() {
                repl.chat.add_message(ChatRole::System, "Usage: /route add <pattern> <model>".to_string());
                return Ok(());
            }
            // Remove existing rule with same pattern if it exists
            repl.model_routes.retain(|(p, _)| p.to_lowercase() != pattern.to_lowercase());
            repl.model_routes.push((pattern.to_lowercase(), model.to_string()));
            repl.chat.add_message(ChatRole::System, format!(
                "Route added: queries starting with '{pattern}' → {model}",
            ));
        }
        "remove" => {
            let pattern = parts.get(1).copied().unwrap_or("");
            if pattern.is_empty() {
                repl.chat.add_message(ChatRole::System, "Usage: /route remove <pattern>".to_string());
                return Ok(());
            }
            let before = repl.model_routes.len();
            repl.model_routes.retain(|(p, _)| p.to_lowercase() != pattern.to_lowercase());
            let removed = before - repl.model_routes.len();
            if removed > 0 {
                repl.chat.add_message(ChatRole::System, format!("Removed {removed} route(s) for pattern '{pattern}'."));
            } else {
                repl.chat.add_message(ChatRole::System, format!("No route found for pattern '{pattern}'."));
            }
        }
        "list" => {
            if repl.model_routes.is_empty() {
                repl.chat.add_message(ChatRole::System, "No routing rules configured. Use /route add <pattern> <model>.".to_string());
            } else {
                let mut out = format!("Routing rules ({}):\n", repl.model_routes.len());
                for (pattern, model) in &repl.model_routes {
                    out.push_str(&format!("  '{pattern}' → {model}\n"));
                }
                repl.chat.add_message(ChatRole::System, out);
            }
        }
        "clear" => {
            let count = repl.model_routes.len();
            repl.model_routes.clear();
            repl.chat.add_message(ChatRole::System, format!("Cleared {count} routing rule(s)."));
        }
        "test" => {
            let query = parts.get(1..).map(|s| s.join(" ")).unwrap_or_default();
            if query.is_empty() {
                repl.chat.add_message(ChatRole::System, "Usage: /route test <query text>".to_string());
                return Ok(());
            }
            let query_lower = query.to_lowercase();
            let matched = repl.model_routes.iter().find(|(pattern, _)| {
                query_lower.starts_with(pattern)
            });
            match matched {
                Some((pattern, model)) => {
                    repl.chat.add_message(ChatRole::System, format!(
                        "Query '{query}' matches pattern '{pattern}' → would use model: {model}",
                    ));
                }
                None => {
                    let current = repl.state.model.as_deref().unwrap_or("default");
                    repl.chat.add_message(ChatRole::System, format!(
                        "Query '{query}' matches no routing rules → would use default model: {current}",
                    ));
                }
            }
        }
        _ => {
            repl.chat.add_message(ChatRole::System, format!("Unknown subcommand: {subcommand}. Use /route help."));
        }
    }

    Ok(())
}

pub(crate) fn handle_credentials(repl: &mut Repl, args: &str) -> Result<()> {
    use shannon_commands::credential_utils::{
        parse_credential_action, CredentialAction,
        format_credentials_list, format_credential_store,
        format_credential_get, format_credential_delete, format_credential_count,
    };

    let parts: Vec<&str> = args.splitn(3, ' ').collect();
    let action_str = parts.first().copied().unwrap_or("");
    let action = parse_credential_action(action_str);

    let output = match action {
        CredentialAction::List => format_credentials_list(),
        CredentialAction::Store => {
            let service = parts.get(1).copied().unwrap_or("");
            let value = parts.get(2).copied().unwrap_or("");
            if service.is_empty() || value.is_empty() {
                "Usage: /credentials store <service> <value>".to_string()
            } else {
                format_credential_store(service, value)
            }
        }
        CredentialAction::Get => {
            let service = parts.get(1).copied().unwrap_or("");
            if service.is_empty() {
                "Usage: /credentials get <service>".to_string()
            } else {
                format_credential_get(service)
            }
        }
        CredentialAction::Delete => {
            let service = parts.get(1).copied().unwrap_or("");
            if service.is_empty() {
                "Usage: /credentials delete <service>".to_string()
            } else {
                format_credential_delete(service)
            }
        }
        CredentialAction::Count => format_credential_count(),
        CredentialAction::Help => {
            "Credential Management:\n\n\
             /credentials list              - Show stored credentials\n\
             /credentials store <svc> <val> - Store a credential\n\
             /credentials get <service>     - Retrieve a credential (masked)\n\
             /credentials delete <service>  - Delete a credential\n\
             /credentials count             - Show stored credential count\n".to_string()
        }
    };

    repl.chat.add_message(ChatRole::System, output);
    Ok(())
}

/// Helper to create a worktree for a team agent.
fn create_agent_worktree(repl: &Repl, agent_name: &str) -> std::result::Result<std::path::PathBuf, String> {
    use shannon_agents::{WorktreeManager, WorktreeConfig};
    let config = WorktreeConfig::default();
    let manager = repl.runtime.block_on(WorktreeManager::new(config))
        .map_err(|e| format!("{e}"))?;
    let session = repl.runtime.block_on(manager.create_agent_session(agent_name, None))
        .map_err(|e| format!("{e}"))?;
    Ok(session.path)
}
