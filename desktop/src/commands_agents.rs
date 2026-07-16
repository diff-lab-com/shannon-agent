//! Agent management Tauri commands.
//!
//! Extracted from `commands.rs` as part of S2 P1.1 (commands.rs split).
//! Covers: listing active agents (from background tasks), inter-agent
//! message history (read / list teams / record), and CRUD for agent
//! definition files (discover / create / delete).

use serde::{Deserialize, Serialize};

use crate::commands::AppState;

/// Agent info for the agents UI surface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInfo {
    pub id: String,
    pub name: String,
    pub model: String,
    pub status: String,
    pub task: Option<String>,
    pub progress: Option<u32>,
    pub tools_used: Option<u32>,
    pub duration: Option<i64>,
}

#[tauri::command]
pub async fn list_agents(state: tauri::State<'_, AppState>) -> Result<Vec<AgentInfo>, String> {
    let tasks = state.background_tasks.lock().await;
    let agents: Vec<AgentInfo> = tasks
        .iter()
        .map(|t| {
            let status = match t.status.as_str() {
                "running" => "running",
                "completed" => "completed",
                "failed" => "failed",
                _ => "pending",
            };
            let duration = t.completed_at.map(|end| end - t.started_at);
            AgentInfo {
                id: t.id.clone(),
                name: "Background Agent".into(),
                model: "default".into(),
                status: status.into(),
                task: Some(t.prompt.clone()),
                progress: None,
                tools_used: None,
                duration,
            }
        })
        .collect();
    Ok(agents)
}

/// Serializable view of a recorded inter-agent message.
///
/// Mirrors `shannon_agents::message_history::MessageRecord` for the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMessageEntry {
    pub message_id: String,
    pub team: String,
    pub from: String,
    pub to: String,
    pub content_preview: String,
    pub content_kind: String,
    pub priority: String,
    pub timestamp: i64,
}

/// List inter-agent messages for a team (most recent first).
///
/// Pass `team=None` to scan all teams (`<adhoc>` plus any team dirs).
#[tauri::command]
pub async fn list_agent_messages(
    state: tauri::State<'_, AppState>,
    team: Option<String>,
    limit: Option<usize>,
) -> Result<Vec<AgentMessageEntry>, String> {
    let store = state.agent_message_history.clone();
    let limit = limit.unwrap_or(100).min(500);
    let mut out: Vec<AgentMessageEntry> = Vec::new();

    let teams: Vec<String> = match team {
        Some(t) => vec![t],
        None => list_message_team_dirs(&store),
    };

    for t in teams {
        match store.list_by_team(&t, limit) {
            Ok(records) => {
                for r in records {
                    out.push(AgentMessageEntry {
                        message_id: r.message_id,
                        team: r.team,
                        from: r.from,
                        to: r.to,
                        content_preview: r.content_preview,
                        content_kind: r.content_kind.as_str().into(),
                        priority: r.priority,
                        timestamp: r.timestamp.timestamp(),
                    });
                }
            }
            Err(e) => tracing::warn!(error = %e, team = %t, "list_agent_messages: skipping team"),
        }
    }

    out.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    out.truncate(limit);
    Ok(out)
}

/// Enumerate teams that have at least one recorded message directory.
#[tauri::command]
pub async fn list_agent_message_teams(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<String>, String> {
    Ok(list_message_team_dirs(&state.agent_message_history))
}

fn list_message_team_dirs(
    store: &shannon_agents::message_history::MessageHistoryStore,
) -> Vec<String> {
    let base = store.base_dir();
    let mut teams = Vec::new();
    if let Ok(entries) = std::fs::read_dir(base) {
        for entry in entries.flatten() {
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                if let Some(name) = entry.file_name().to_str() {
                    teams.push(name.to_string());
                }
            }
        }
    }
    teams.sort();
    teams
}

/// Record an inter-agent message into the append-only history.
///
/// Used by the desktop UI's "Agent Messages" panel for manual / test injection
/// until real team agents are wired in. Real team agents write directly via
/// `AgentCoordinator::record_to_history` (see `shannon-agents`).
#[tauri::command]
pub async fn record_agent_message(
    state: tauri::State<'_, AppState>,
    team: String,
    from: String,
    to: String,
    content: String,
    priority: Option<String>,
) -> Result<String, String> {
    use shannon_agents::message_history::{ContentKind, MessageRecord};

    let priority = priority.unwrap_or_else(|| "normal".into());
    let record = MessageRecord {
        message_id: uuid::Uuid::new_v4().to_string(),
        team,
        from,
        to,
        content_preview: MessageRecord::truncate_preview(&content),
        content_kind: ContentKind::Text,
        priority,
        timestamp: chrono::Utc::now(),
        revision: 0,
    };
    state
        .agent_message_history
        .record(&record)
        .map_err(|e| e.to_string())
}

/// Serializable view of an agent definition loaded from disk.
///
/// Mirrors `shannon_skills::agent_loader::AgentDefinition` minus the
/// file-system-only fields. Used by the desktop UI's "My Agents" panel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDefinitionInfo {
    pub name: String,
    pub description: String,
    pub tools: Vec<String>,
    pub model: String,
    pub prompt: String,
    pub source_path: String,
}

/// Resolve the working directory used for agent file discovery / creation.
///
/// Prefers the persisted `working_dir`, falls back to the process cwd.
pub(crate) async fn resolve_working_dir(state: &AppState) -> std::path::PathBuf {
    let cfg = state.desktop_config.read().await;
    cfg.working_dir
        .clone()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
        })
}

/// List agent definitions (`.claude/agents/*.md` and `.shannon/agents/*.md`)
/// discovered from the working directory upward.
#[tauri::command]
pub async fn list_agent_definitions(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<AgentDefinitionInfo>, String> {
    let cwd = resolve_working_dir(&state).await;
    let dirs = shannon_skills::agent_loader::discover_agent_directories(&cwd);
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for dir in dirs {
        let agents = shannon_skills::agent_loader::load_agents_from_directory(&dir)
            .map_err(|e| e.to_string())?;
        for a in agents {
            if seen.insert(a.name.clone()) {
                out.push(AgentDefinitionInfo {
                    name: a.name,
                    description: a.description,
                    tools: a.tools,
                    model: format!("{:?}", a.model).to_ascii_lowercase(),
                    prompt: a.prompt,
                    source_path: a.source_path.to_string_lossy().into_owned(),
                });
            }
        }
    }
    Ok(out)
}

/// Create a new agent definition by writing `.claude/agents/<name>.md`.
///
/// The file uses Claude Code-compatible YAML frontmatter so the same
/// definition works in `claude code`, Codex, and Shannon. Returns the
/// absolute path of the created file.
#[tauri::command]
pub async fn create_agent_definition(
    state: tauri::State<'_, AppState>,
    name: String,
    model: Option<String>,
    system_prompt: Option<String>,
    tools: Vec<String>,
) -> Result<String, String> {
    let sanitized = name
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect::<String>();
    if sanitized.is_empty() {
        return Err("Agent name is required".into());
    }

    let cwd = resolve_working_dir(&state).await;
    let agents_dir = cwd.join(".claude").join("agents");
    std::fs::create_dir_all(&agents_dir).map_err(|e| e.to_string())?;
    let file_path = agents_dir.join(format!("{sanitized}.md"));
    if file_path.exists() {
        return Err(format!("Agent '{sanitized}' already exists"));
    }

    let model_line = model
        .as_deref()
        .filter(|m| !m.trim().is_empty())
        .unwrap_or("sonnet");
    let tools_line = if tools.is_empty() {
        "Read, Glob, Grep, Bash".to_string()
    } else {
        tools
            .iter()
            .map(|t| {
                let t = t.trim();
                let first = t.chars().next().map(|c| c.to_ascii_uppercase());
                let rest: String = t.chars().skip(1).collect();
                first.map(|f| format!("{f}{rest}")).unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    let description = format!("Agent created via Shannon Desktop: {sanitized}");
    let prompt_body = system_prompt
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or("You are a helpful agent. Complete the task thoroughly.");

    let body = format!(
        "---\n\
         name: {sanitized}\n\
         description: {description}\n\
         tools: {tools_line}\n\
         model: {model_line}\n\
         ---\n\n\
         {prompt_body}\n"
    );
    std::fs::write(&file_path, body).map_err(|e| e.to_string())?;
    Ok(file_path.to_string_lossy().into_owned())
}

/// Delete an agent definition file. Only deletes files inside the
/// discovered agent directories to prevent arbitrary file deletion.
#[tauri::command]
pub async fn delete_agent_definition(
    state: tauri::State<'_, AppState>,
    name: String,
) -> Result<bool, String> {
    let sanitized = name
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect::<String>();
    let cwd = resolve_working_dir(&state).await;
    let dirs = shannon_skills::agent_loader::discover_agent_directories(&cwd);
    for dir in dirs {
        let candidate = dir.join(format!("{sanitized}.md"));
        if candidate.exists() {
            // Ensure the resolved path is inside `dir` (no traversal).
            let canonical_dir = dir.canonicalize().map_err(|e| e.to_string())?;
            let canonical_candidate = candidate.canonicalize().map_err(|e| e.to_string())?;
            if !canonical_candidate.starts_with(&canonical_dir) {
                return Err("Refusing to delete file outside agent directory".into());
            }
            std::fs::remove_file(&canonical_candidate).map_err(|e| e.to_string())?;
            return Ok(true);
        }
    }
    Ok(false)
}
