//! MCP server + Skills + Addons Tauri commands.
//!
//! Extracted from `commands.rs` as part of S2 P1.1 (commands.rs split).
//! Domain spans: MCP server lifecycle, skill discovery, installed-addon
//! aggregation. MCP configs live in the unified
//! `~/.shannon/settings.json#mcpServers` store (G1 split-brain fix — the
//! same blob the CLI and the extensions hub read/write), plus the
//! `shannon_skills` / `shannon_mcp` registries on AppState.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::commands::{AppState, ToolInfo, chrono_timestamp};

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
    /// W1-1 (R2-P0-1(B)): remote (HTTP/SSE) endpoint of url-only entries.
    /// `None` on stdio rows. Url-only servers are wired into the desktop
    /// process pool since W2-A (R4/A1) — OAuth entries since W3-B (A2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// W2-A (R4/A1) — the backend's single-source auth verdict, mirrored
    /// from `config::McpServerConfig::has_auth_headers`; since W3-B (A2)
    /// its semantics are "OAuth entry" (url-only row carrying a
    /// credential). Those rows connect through the stored-credential OAuth
    /// path; when a connection fails they render the classified failure
    /// state (with re-authentication for NeedsAuth) instead of a generic
    /// Offline badge. Header-less remote rows render like stdio.
    #[serde(default)]
    pub has_auth_headers: bool,
    /// W1-7 (R2-P1-6): the pool's last start/connection failure for this
    /// server, so a dead server is diagnosable instead of just colored
    /// Offline. `None` = the pool never reported a failure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    /// W3-B (A2): classified failure state for url-only rows —
    /// `needs_auth` | `unreachable` | `server_error` (the wire tokens of
    /// `shannon_mcp::RemoteFailureKind`). The UI renders the three classes
    /// differently (re-authenticate vs retry vs retry+details).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_kind: Option<&'static str>,
    /// F5 (A8): where this row's credential lives — `"keyring"` (migrated
    /// into the OS keyring) or `"plaintext_file"` (keyring unavailable;
    /// owner-only 0600 file). `None` on rows without a credential. Drives
    /// the MCP page's credential-storage status line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_storage: Option<&'static str>,
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

/// Build the UI row for a server plus its live pool status. Shared by the
/// add/restart/toggle commands, which all return the fresh row.
/// `last_connected` is the uptime-derived timestamp from the shared
/// [`pool_last_connected`] helper — W3-B unified the two conventions
/// (the add/restart path used "now", the list path derived from uptime).
pub(crate) fn mcp_server_info(
    server: &crate::config::McpServerConfig,
    connected: bool,
    last_connected: Option<i64>,
    last_error: Option<String>,
) -> McpServerInfo {
    // W3-B (A2): classify remote failures so the UI can render the three
    // failure classes distinctly. Stdio rows keep the plain Offline badge.
    let failure_kind = if !connected && server.url.is_some() {
        last_error
            .as_deref()
            .map(shannon_mcp::classify_remote_failure)
            .map(|k| k.as_str())
    } else {
        None
    };
    // F5 (A8): the credential-storage verdict for rows carrying a
    // credential — keyring when the store holds the token (migrated), the
    // degraded plaintext-file token otherwise.
    let store = crate::secret_store::global();
    let keyring_hit = matches!(
        store
            .as_deref()
            .map(|s| s.get(&crate::secret_store::mcp_oauth_key(&server.name))),
        Some(Ok(Some(_)))
    );
    let credential_storage = if keyring_hit || server.has_auth_headers {
        Some(crate::secret_store::storage_mode(keyring_hit))
    } else {
        None
    };
    McpServerInfo {
        name: server.name.clone(),
        command: server.command.clone(),
        enabled: server.enabled,
        connected,
        tool_count: 0,
        tools: Vec::new(),
        last_connected,
        url: server.url.clone(),
        has_auth_headers: server.has_auth_headers,
        last_error,
        failure_kind,
        credential_storage,
    }
}

/// Derive a real last-connected timestamp from the pool's uptime clock
/// (process start = now − uptime). W1-7 introduced this for the list
/// command; W3-B unified the add/restart/toggle rows onto the same
/// convention (they previously stamped "now", which was a lie for a server
/// connected seconds ago during a past session).
pub(crate) async fn pool_last_connected(
    pool: &shannon_mcp::McpProcessPool,
    name: &str,
    connected: bool,
) -> Option<i64> {
    if !connected {
        return None;
    }
    let now = chrono_timestamp();
    pool.server_status(name)
        .await
        .and_then(|st| st.uptime)
        .map(|up| now - up.as_millis() as i64)
}

/// Start one server in the pool according to its config row: stdio rows
/// spawn a process, url-only rows connect as remote HTTP/SSE — pure
/// remote (W2-A R4/A1) or OAuth with stored credentials (W3-B A2, which
/// also persists handshake-time token refreshes). Returns
/// `(connected, last_error)`.
async fn start_for_config(
    pool: &shannon_mcp::McpProcessPool,
    server: &crate::config::McpServerConfig,
) -> (bool, Option<String>) {
    if server.command.is_empty() {
        let Some(url) = server.url.clone() else {
            return (
                false,
                Some(format!(
                    "Server '{}' has neither command nor url",
                    server.name
                )),
            );
        };
        if server.has_auth_headers {
            let outcome = crate::mcp::start_oauth_remote_row(pool, server).await;
            return (outcome.connected, outcome.error);
        }
        return match pool
            .start_remote_server(&server.name, &url, HashMap::new(), None)
            .await
        {
            Ok(()) => (true, None),
            Err(e) => (false, Some(e)),
        };
    }
    match pool
        .start_server(&server.name, &server.command, &server.args, &server.env)
        .await
    {
        Ok(()) => (true, None),
        Err(e) => (false, Some(e)),
    }
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
        url: None,
        has_auth_headers: false,
        oauth: None,
    };

    // G1: single source of truth is `~/.shannon/settings.json#mcpServers`
    // (shared with the CLI and the extensions hub). save is an upsert of
    // this one row — other entries in the unified store are untouched.
    config::save_mcp_servers(std::slice::from_ref(&server_config)).map_err(|e| e.to_string())?;

    // Start the server process. W1-7: a failed start keeps its error so the
    // UI can say *why* the server is down, not just that it is.
    let pool = state.mcp_pool.clone();
    let (connected, last_error) = start_for_config(&pool, &server_config).await;
    let last_connected = pool_last_connected(&pool, &name, connected).await;

    Ok(mcp_server_info(
        &server_config,
        connected,
        last_connected,
        last_error,
    ))
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

    // G1: delete from the unified settings.json store (insert-only saves
    // never drop rows, so removal is an explicit delete).
    if config::remove_mcp_server_entry(&name).map_err(|e| e.to_string())? {
        Ok(true)
    } else {
        Err(format!("Server not found: {name}"))
    }
}

/// Restart an MCP server (stop then start).
///
/// W2-A (R4/A1) restarted pure remote (header-less url-only) rows via the
/// pool's remote transport. W3-B (A2) extends that to OAuth rows: they
/// reconnect from the stored credential (a handshake-time refresh is
/// persisted by [`crate::mcp::start_oauth_remote_row`]), so "Retry" in the
/// UI is a real reconnect for every remote row.
#[tauri::command]
pub async fn restart_mcp_server(
    state: tauri::State<'_, AppState>,
    name: String,
) -> Result<McpServerInfo, String> {
    use crate::config;

    let servers = config::load_mcp_servers()?;
    let server = servers
        .iter()
        .find(|s| s.name == name)
        .cloned()
        .ok_or_else(|| format!("Server not found: {name}"))?;

    if server.command.is_empty() && server.url.is_none() {
        return Err(format!("Server '{name}' has no command to restart"));
    }

    let pool = state.mcp_pool.clone();

    // Stop then start. W1-7: keep the start error for the UI.
    let _ = pool.stop_server(&name).await;
    let (connected, last_error) = start_for_config(&pool, &server).await;
    let last_connected = pool_last_connected(&pool, &name, connected).await;

    Ok(mcp_server_info(
        &server,
        connected,
        last_connected,
        last_error,
    ))
}

/// Enable or disable one MCP server (W2-A inline toggle) and reconcile the
/// pool: disabling stops the server, enabling starts it (OAuth rows start
/// from their stored credential since W3-B/A2).
#[tauri::command]
pub async fn set_mcp_server_enabled(
    state: tauri::State<'_, AppState>,
    name: String,
    enabled: bool,
) -> Result<McpServerInfo, String> {
    use crate::config;

    // Persist first — the store is the single source of truth; the pool is
    // reconciled after. The in-place JSON edit keeps url-only rows
    // (`type`/`url`/`headers`/`shannonOAuth`) intact.
    if !config::set_mcp_server_enabled(&name, enabled)? {
        return Err(format!("Server not found: {name}"));
    }

    let servers = config::load_mcp_servers()?;
    let server = servers
        .iter()
        .find(|s| s.name == name)
        .cloned()
        .ok_or_else(|| format!("Server not found: {name}"))?;

    let pool = state.mcp_pool.clone();
    let (connected, last_error) = if enabled {
        start_for_config(&pool, &server).await
    } else {
        // Not running is fine — a never-started (seed-failed) server has a
        // pool handle but stop is still the right reconcile step.
        let _ = pool.stop_server(&name).await;
        (false, None)
    };
    let last_connected = pool_last_connected(&pool, &name, connected).await;

    Ok(mcp_server_info(
        &server,
        connected,
        last_connected,
        last_error,
    ))
}

/// Get MCP server configuration details.
#[tauri::command]
pub async fn get_mcp_server_config(name: String) -> Result<crate::config::McpServerConfig, String> {
    use crate::config;

    let servers = config::load_mcp_servers()?;
    servers
        .into_iter()
        .find(|s| s.name == name)
        .ok_or_else(|| format!("Server not found: {name}"))
}

/// List all configured MCP servers with their status.
#[tauri::command]
pub async fn list_mcp_servers(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<McpServerInfo>, String> {
    use crate::config;
    use shannon_mcp::ServerState;

    // W2-A: a corrupt settings.json is an error — the UI renders its error
    // state instead of a fake "nothing installed".
    let servers = config::load_mcp_servers()?;
    let pool = state.mcp_pool.clone();

    let pool_states = pool.list_servers().await;
    let state_map: std::collections::HashMap<String, ServerState> =
        pool_states.into_iter().collect();

    let mut server_infos = Vec::new();
    for s in servers {
        let pool_state = state_map.get(&s.name);
        let connected = pool_state
            .map(|st| matches!(st, ServerState::Healthy))
            .unwrap_or(false);

        // W1-7 (R2-P1-6): the pool's Unhealthy state carries the failure
        // reason (spawn error, failed health check) — surface it instead of
        // leaving a dead server indistinguishable from a paused one.
        let last_error = match pool_state {
            Some(ServerState::Unhealthy(err)) => Some(err.clone()),
            _ => None,
        };

        // W1-7: derive a real last-connected timestamp from the pool's
        // uptime clock (process start = now − uptime). W3-B unified the
        // add/restart rows onto this same helper (they previously stamped
        // "now", a different convention for the same field).
        let last_connected = pool_last_connected(&pool, &s.name, connected).await;

        // W3-B (A2): classify the failure of url-only rows so the UI can
        // render NeedsAuth / Unreachable / ServerError distinctly. Stdio
        // rows keep the plain Offline badge (no classification).
        let failure_kind = if !connected && s.url.is_some() {
            last_error
                .as_deref()
                .map(shannon_mcp::classify_remote_failure)
                .map(|k| k.as_str())
        } else {
            None
        };

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

        // F5 (A8): same credential-storage verdict as mcp_server_info —
        // keyring when the store holds this server's token, the degraded
        // plaintext-file token otherwise, nothing on credential-less rows.
        let store = crate::secret_store::global();
        let keyring_hit = matches!(
            store
                .as_deref()
                .map(|st| st.get(&crate::secret_store::mcp_oauth_key(&s.name))),
            Some(Ok(Some(_)))
        );
        let credential_storage = if keyring_hit || s.has_auth_headers {
            Some(crate::secret_store::storage_mode(keyring_hit))
        } else {
            None
        };

        server_infos.push(McpServerInfo {
            name: s.name,
            command: s.command,
            enabled: s.enabled,
            connected,
            tool_count,
            tools,
            last_connected,
            url: s.url,
            has_auth_headers: s.has_auth_headers,
            last_error,
            failure_kind,
            credential_storage,
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

/// Default skill discovery roots for the list command: the user-global
/// home skill directory (where the extensions hub installs,
/// `~/.shannon/skills`) plus the cwd project directories.
fn skill_roots() -> Result<Vec<(std::path::PathBuf, shannon_skills::SkillSource)>, String> {
    use shannon_skills::SkillSource;
    use std::path::PathBuf;

    let mut roots: Vec<(PathBuf, SkillSource)> = Vec::new();
    if let Some(home) = dirs::home_dir() {
        roots.push((home.join(".shannon").join("skills"), SkillSource::User));
    }
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    roots.push((cwd.join(".shannon").join("skills"), SkillSource::Project));
    roots.push((cwd.join(".claude").join("skills"), SkillSource::Project));
    roots.push((
        cwd.join(".claude").join("commands"),
        SkillSource::CommandsDeprecated,
    ));
    Ok(roots)
}

/// List all available skills from shannon-skills registry.
///
/// G1 P0-2.2: merges the user-global home skill directory (where the
/// extensions hub installs, `~/.shannon/skills`) with the cwd project
/// directories. Previously only the cwd was read, so hub-installed skills
/// never showed up in the slash completion.
///
/// W1-2 (R2-P0-2): opening this menu also hot-registers the matching
/// `skill_<id>` chat tools, so a skill installed while the app is running
/// is model-callable on the very next turn — see the private `list_skills_inner`.
#[tauri::command]
pub async fn list_skills(state: tauri::State<'_, AppState>) -> Result<Vec<SkillInfo>, String> {
    list_skills_inner(&state.skill_registry, &state.tools, &skill_roots()?)
}

/// Core of [`list_skills`], split from the `#[tauri::command]` wrapper so
/// the install→usable chain is unit-testable without a Tauri app handle.
///
/// Hydrates `registry` from `roots`, then hot-registers every not-yet-known
/// user-invocable skill as a `skill_<id>` chat tool
/// (`skill_tools::register_missing_skill_tools`). The system prompt
/// (`skills_for_chat_prompt`) advertises `/name` ↔ `skill_<name>`, so
/// without this pass a skill installed while the app runs would be
/// advertised to the model on the next turn while its tool only existed
/// after a restart. Idempotent: skills whose tool is already registered are
/// skipped. Hydration and registration failures are logged and skipped —
/// they never fail the listing.
pub(crate) fn list_skills_inner(
    registry: &shannon_skills::SkillRegistry,
    tools: &shannon_core::tools::ToolRegistry,
    roots: &[(std::path::PathBuf, shannon_skills::SkillSource)],
) -> Result<Vec<SkillInfo>, String> {
    for (dir, source) in roots {
        if !dir.exists() {
            continue;
        }
        let _ = registry.load_from_directory(dir, source);
    }

    // W1-2 (R2-P0-2) — the slash menu just made newly installed skills
    // visible, and the next turn's system prompt will advertise their
    // `/name` ↔ `skill_<name>` mapping; register the matching chat tools
    // now so the model is never pointed at a tool that doesn't exist.
    let hot_registered = crate::skill_tools::register_missing_skill_tools(tools, registry);
    if hot_registered > 0 {
        tracing::info!(
            count = hot_registered,
            "list_skills hot-registered new skill chat tools"
        );
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

#[cfg(test)]
mod tests {
    use super::*;

    /// W1-2 core acceptance (R2-P0-2): a skill installed while the app is
    /// running must become chat-callable the moment the slash menu lists it
    /// — no restart. The chain proven here mirrors production exactly:
    /// hub install lands a SKILL.md under a skill root (temp dir), the
    /// `list_skills` path hydrates it into the SkillRegistry, the hot pass
    /// registers its `skill_<id>` chat tool, and the system-prompt block
    /// advertises the `/name` ↔ `skill_<name>` mapping. The registries are
    /// fresh (never `AppState::new`), so nothing touches the real HOME.
    #[tokio::test]
    async fn installed_skill_becomes_chat_callable_without_restart() {
        let tmp = tempfile::tempdir().unwrap();
        let skills_dir = tmp.path().join("skills");
        let skill_dir = skills_dir.join("freshly-installed");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: freshly-installed\ndescription: Installed while the app was running\n\
             ---\n\n# Freshly installed\n\nEcho body: ${0}\n",
        )
        .unwrap();

        let skill_registry = shannon_skills::SkillRegistry::new();
        let tools = shannon_core::tools::ToolRegistry::new();

        // Pre-install state: no tool, no prompt advertisement.
        assert!(tools.get("skill_freshly-installed").is_none());
        assert!(
            !crate::skill_tools::skills_for_chat_prompt(&skill_registry)
                .contains("freshly-installed")
        );

        // The `list_skills` command path (hydration + hot registration).
        let roots = vec![(skills_dir, shannon_skills::SkillSource::User)];
        let infos = list_skills_inner(&skill_registry, &tools, &roots).unwrap();

        // 列表态: the slash menu lists the new skill.
        let info = infos
            .iter()
            .find(|i| i.name == "freshly-installed")
            .unwrap_or_else(|| panic!("skill missing from list: {infos:?}"));
        assert_eq!(info.trigger, "/freshly-installed");

        // 聊天内可见: the system prompt advertises it with the tool mapping.
        let prompt = crate::skill_tools::skills_for_chat_prompt(&skill_registry);
        assert!(prompt.contains("/freshly-installed"), "{prompt}");
        assert!(prompt.contains("skill_<name>"), "{prompt}");

        // 可调用: the tool exists and executes through the shared registry.
        let tool = tools
            .get("skill_freshly-installed")
            .unwrap_or_else(|| panic!("hot registration did not land the chat tool"));
        let output = tool
            .execute(serde_json::json!({ "args": "hello" }))
            .await
            .unwrap();
        assert!(!output.is_error);
        assert!(
            output.content.contains("Echo body: hello"),
            "{}",
            output.content
        );

        // Idempotent: reopening the slash menu registers nothing new.
        assert_eq!(
            crate::skill_tools::register_missing_skill_tools(&tools, &skill_registry),
            0
        );
        let again = list_skills_inner(&skill_registry, &tools, &roots).unwrap();
        assert_eq!(again.len(), infos.len(), "listing must not duplicate");
    }

    /// W1-2 requirement: a hydration failure (unreadable/broken skill file)
    /// is logged and skipped — the listing itself still succeeds.
    #[tokio::test]
    async fn broken_skill_file_does_not_fail_listing() {
        let tmp = tempfile::tempdir().unwrap();
        let skills_dir = tmp.path().join("skills");
        let skill_dir = skills_dir.join("broken");
        std::fs::create_dir_all(&skill_dir).unwrap();
        // Unclosed frontmatter delimiter → the loader rejects the file and
        // logs (F36); the listing itself must still succeed.
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: broken\nno closing marker",
        )
        .unwrap();

        let skill_registry = shannon_skills::SkillRegistry::new();
        let tools = shannon_core::tools::ToolRegistry::new();
        let roots = vec![(skills_dir, shannon_skills::SkillSource::User)];

        let infos = list_skills_inner(&skill_registry, &tools, &roots).unwrap();
        assert!(infos.is_empty());
        assert!(tools.get("skill_broken").is_none());
    }
}
