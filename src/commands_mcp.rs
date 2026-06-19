//! MCP server + Skills + Addons Tauri commands.
//!
//! Extracted from `commands.rs` as part of S2 P1.1 (commands.rs split).
//! Domain spans: MCP server lifecycle, skill discovery, installed-addon
//! aggregation. Backed by `~/.shannon/desktop/mcp-servers.json` for MCP
//! configs and the `shannon_skills` / `shannon_mcp` registries on AppState.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::commands::{chrono_timestamp, AppState, ToolInfo};

/// MCP server info for UI display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerInfo {
    pub name: String,
    pub command: String,
    pub enabled: bool,
    pub connected: bool,
    pub tool_count: usize,
    pub tools: Vec<ToolInfo>,
    pub last_connected: Option<i64>,
}

/// Skill information for the skill browser UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillInfo {
    pub name: String,
    pub description: String,
    pub trigger: String,
    pub source: String,
    pub category: Option<String>,
}

/// Detailed skill information with content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillDetail {
    pub name: String,
    pub description: String,
    pub trigger: String,
    pub content: String,
    pub parameters: Vec<String>,
    pub source: String,
    pub category: Option<String>,
}

/// Add an MCP server configuration and start the process.
#[tauri::command]
pub async fn add_mcp_server(
    state: tauri::State<'_, AppState>,
    name: String,
    command: String,
    args: Vec<String>,
    env: HashMap<String, String>,
) -> Result<McpServerInfo, String> {
    use crate::config;

    if name.is_empty() {
        return Err("Server name cannot be empty".to_string());
    }
    if command.is_empty() {
        return Err("Command cannot be empty".to_string());
    }

    let server_config = config::McpServerConfig {
        name: name.clone(),
        command: command.clone(),
        args: args.clone(),
        env: env.clone(),
        enabled: true,
    };

    let mut servers = config::load_mcp_servers();
    servers.push(server_config.clone());
    config::save_mcp_servers(&servers).map_err(|e| e.to_string())?;

    // Start the server process
    let pool = state.mcp_pool.clone();
    let connected = pool
        .start_server(&name, &command, &args, &env)
        .await
        .is_ok();

    Ok(McpServerInfo {
        name: server_config.name,
        command: server_config.command,
        enabled: server_config.enabled,
        connected,
        tool_count: 0,
        tools: Vec::new(),
        last_connected: if connected {
            Some(chrono_timestamp())
        } else {
            None
        },
    })
}

/// Remove an MCP server configuration and stop its process.
#[tauri::command]
pub async fn remove_mcp_server(
    state: tauri::State<'_, AppState>,
    name: String,
) -> Result<bool, String> {
    use crate::config;

    // Stop the server process first
    let pool = state.mcp_pool.clone();
    let _ = pool.stop_server(&name).await;

    // Load servers, remove matching one, save
    let mut servers = config::load_mcp_servers();
    let original_len = servers.len();
    servers.retain(|s| s.name != name);

    if servers.len() < original_len {
        config::save_mcp_servers(&servers).map_err(|e| e.to_string())?;
        Ok(true)
    } else {
        Err(format!("Server not found: {}", name))
    }
}

/// Restart an MCP server (stop then start).
#[tauri::command]
pub async fn restart_mcp_server(
    state: tauri::State<'_, AppState>,
    name: String,
) -> Result<McpServerInfo, String> {
    use crate::config;

    let servers = config::load_mcp_servers();
    let server = servers
        .iter()
        .find(|s| s.name == name)
        .ok_or_else(|| format!("Server not found: {}", name))?;

    let command = server.command.clone();
    let args = server.args.clone();
    let env = server.env.clone();

    let pool = state.mcp_pool.clone();

    // Stop then start
    let _ = pool.stop_server(&name).await;
    let connected = pool
        .start_server(&name, &command, &args, &env)
        .await
        .is_ok();

    Ok(McpServerInfo {
        name: name.clone(),
        command,
        enabled: true,
        connected,
        tool_count: 0,
        tools: Vec::new(),
        last_connected: if connected {
            Some(chrono_timestamp())
        } else {
            None
        },
    })
}

/// Get MCP server configuration details.
#[tauri::command]
pub async fn get_mcp_server_config(name: String) -> Result<crate::config::McpServerConfig, String> {
    use crate::config;

    let servers = config::load_mcp_servers();
    servers
        .into_iter()
        .find(|s| s.name == name)
        .ok_or_else(|| format!("Server not found: {}", name))
}

/// List all configured MCP servers with their status.
#[tauri::command]
pub async fn list_mcp_servers(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<McpServerInfo>, String> {
    use crate::config;
    use shannon_mcp::ServerState;

    let servers = config::load_mcp_servers();
    let pool = state.mcp_pool.clone();

    let pool_states = pool.list_servers().await;
    let state_map: std::collections::HashMap<String, ServerState> =
        pool_states.into_iter().collect();

    let mut server_infos = Vec::new();
    for s in servers {
        let connected = state_map
            .get(&s.name)
            .map(|st| matches!(st, ServerState::Healthy))
            .unwrap_or(false);

        let (tool_count, tools) = if connected {
            match pool.refresh_tools_for_server(&s.name).await {
                adapters if !adapters.is_empty() => {
                    use shannon_core::Tool as ToolTrait;
                    let tools: Vec<ToolInfo> = adapters
                        .iter()
                        .map(|a| ToolInfo {
                            name: a.name().to_string(),
                            description: a.description().to_string(),
                            enabled: true,
                        })
                        .collect();
                    (tools.len(), tools)
                }
                _ => (0, Vec::new()),
            }
        } else {
            (0, Vec::new())
        };

        server_infos.push(McpServerInfo {
            name: s.name,
            command: s.command,
            enabled: s.enabled,
            connected,
            tool_count,
            tools,
            last_connected: None,
        });
    }

    Ok(server_infos)
}

/// and returns a flat list for the Installed tab.
#[tauri::command]
pub async fn list_installed_addons() -> Result<Vec<crate::extensions::InstalledAddonSummary>, String>
{
    Ok(crate::extensions::aggregate_installed())
}

/// List all available skills from shannon-skills registry.
#[tauri::command]
pub async fn list_skills(state: tauri::State<'_, AppState>) -> Result<Vec<SkillInfo>, String> {
    let registry = state.skill_registry.clone();

    // Load skills from standard directories
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;

    // Load from .shannon/skills/ and .claude/commands/
    let shannon_skills_dir = cwd.join(".shannon/skills");
    let claude_commands_dir = cwd.join(".claude/commands");

    if shannon_skills_dir.exists() {
        use shannon_skills::SkillSource;
        let _ = registry.load_from_directory(&shannon_skills_dir, &SkillSource::Project);
    }

    if claude_commands_dir.exists() {
        use shannon_skills::SkillSource;
        let _ =
            registry.load_from_directory(&claude_commands_dir, &SkillSource::CommandsDeprecated);
    }

    // Get all available skills
    let skills = registry.list();

    // Convert to SkillInfo
    let mut skill_infos: Vec<SkillInfo> = skills
        .into_iter()
        .filter(|skill| skill.user_invocable && !skill.is_hidden)
        .map(|skill| {
            let trigger = if skill.aliases.is_empty() {
                format!("/{}", skill.name)
            } else {
                format!("/{}", skill.aliases.first().unwrap_or(&skill.name))
            };

            SkillInfo {
                name: skill.name.clone(),
                description: skill.description,
                trigger,
                source: format!("{:?}", skill.source),
                category: None,
            }
        })
        .collect();

    // Sort by name
    skill_infos.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(skill_infos)
}

/// Get detailed information about a specific skill.
#[tauri::command]
pub async fn get_skill_detail(
    state: tauri::State<'_, AppState>,
    name: String,
) -> Result<SkillDetail, String> {
    let registry = state.skill_registry.clone();

    let full = registry.get_full_skill(&name).map_err(|e| e.to_string())?;
    let skill = &full.skill;

    let trigger = if skill.aliases.is_empty() {
        format!("/{}", skill.name)
    } else {
        format!("/{}", skill.aliases.first().unwrap_or(&skill.name))
    };

    Ok(SkillDetail {
        name: skill.name.clone(),
        description: skill.description.clone(),
        trigger,
        content: full.content().to_string(),
        parameters: skill
            .argument_hint
            .as_ref()
            .map(|h| vec![h.clone()])
            .unwrap_or_default(),
        source: skill.id.to_string(),
        category: None,
    })
}
