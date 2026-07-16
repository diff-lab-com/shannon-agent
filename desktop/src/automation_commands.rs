//! Tauri commands for the Automation pages: hook-event catalog and
//! permission-profile management.
//!
//! `list_hook_events` returns metadata for every event the Shannon hook
//! system can fire — used by the `/hooks` page to render a browsable
//! catalog and by the `/routines` create-form to validate trigger names.
//!
//! `list_permission_profiles`, `get_custom_profile`, `save_custom_profile`,
//! and `delete_custom_profile` expose the custom-profile TOML files under
//! `.shannon/profiles/` (and `.claude/profiles/`) for the `/profiles` page.
//! Built-in profiles (Strict / Balanced / Permissive) are always returned
//! alongside the user-defined ones.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::State;

use crate::commands::AppState;

// ─── DTOs ───────────────────────────────────────────────────────────────────

/// Catalog entry for one hook event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookEventInfo {
    /// Canonical event name (matches `HookEventType::to_string()`).
    pub name: String,
    /// Coarse grouping for navigation.
    pub category: String,
    /// One-sentence summary of when the event fires.
    pub description: String,
    /// Fields present in the event payload (best-effort, informational).
    pub payload_fields: Vec<String>,
}

/// Built-in profile summary returned by `list_permission_profiles`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuiltinProfileInfo {
    /// Canonical id ("strict" / "balanced" / "permissive").
    pub id: String,
    pub description: String,
    pub auto_approve_read: bool,
    pub auto_approve_write: bool,
    pub auto_approve_bash: bool,
    pub auto_approve_delete: bool,
    pub auto_approve_network: bool,
    pub deny_destructive: Vec<String>,
}

/// Custom profile row returned by `list_permission_profiles`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomProfileInfo {
    pub name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
    pub auto_approve: Vec<String>,
    pub confirm: Vec<String>,
    pub deny: Vec<String>,
    /// Absolute path to the source TOML file, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
}

/// Response shape for `list_permission_profiles`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfilesList {
    pub builtin: Vec<BuiltinProfileInfo>,
    pub custom: Vec<CustomProfileInfo>,
}

// ─── catalog: hook events ───────────────────────────────────────────────────

/// All 30 hook events the Shannon engine can fire, with metadata.
///
/// Keep this list in sync with `shannon_engine::hooks::events::HookEventType`.
/// Order is stable (defined by the engine variant order) so the UI can
/// render the catalog deterministically.
fn hook_event_catalog() -> Vec<HookEventInfo> {
    use shannon_engine::hooks::HookEventType as E;
    let describe = |e: &E| -> (String, String, Vec<String>) {
        match e {
            E::PreToolUse => (
                "Tools".into(),
                "Before a tool is executed.".into(),
                vec!["tool_name".into(), "input".into()],
            ),
            E::PostToolUse => (
                "Tools".into(),
                "After a tool completes successfully.".into(),
                vec!["tool_name".into(), "input".into(), "output".into()],
            ),
            E::PostToolUseFailure => (
                "Tools".into(),
                "After a tool call fails with an error.".into(),
                vec!["tool_name".into(), "input".into(), "error".into()],
            ),
            E::PostToolBatch => (
                "Tools".into(),
                "After a batch of tool calls completes.".into(),
                vec!["results".into()],
            ),
            E::SessionStart => (
                "Session".into(),
                "When a conversation session begins.".into(),
                vec!["session_id".into()],
            ),
            E::SessionEnd => (
                "Session".into(),
                "When a conversation session ends.".into(),
                vec!["session_id".into()],
            ),
            E::Stop => (
                "Session".into(),
                "When the model stops generating.".into(),
                vec!["reason".into()],
            ),
            E::StopFailure => (
                "Session".into(),
                "When the model stops due to an error.".into(),
                vec!["error".into()],
            ),
            E::UserPromptSubmit => (
                "Prompt".into(),
                "When the user submits a prompt.".into(),
                vec!["prompt".into()],
            ),
            E::UserPromptExpansion => (
                "Prompt".into(),
                "After a user prompt is expanded (template vars resolved).".into(),
                vec!["expanded".into()],
            ),
            E::Notification => (
                "Prompt".into(),
                "When a notification is emitted.".into(),
                vec!["message".into()],
            ),
            E::Elicitation => (
                "Prompt".into(),
                "When an interactive elicitation is triggered.".into(),
                vec!["question".into()],
            ),
            E::ElicitationResult => (
                "Prompt".into(),
                "When an elicitation result is received.".into(),
                vec!["result".into()],
            ),
            E::PreCompact => (
                "Context".into(),
                "Before context compaction runs.".into(),
                vec!["message_count".into()],
            ),
            E::PostCompact => (
                "Context".into(),
                "After context compaction completes.".into(),
                vec!["compacted_count".into()],
            ),
            E::InstructionsLoaded => (
                "Context".into(),
                "After CLAUDE.md / instructions are loaded.".into(),
                vec!["paths".into()],
            ),
            E::ConfigChange => (
                "Context".into(),
                "When Shannon configuration changes.".into(),
                vec!["key".into(), "value".into()],
            ),
            E::CwdChanged => (
                "Context".into(),
                "When the working directory changes.".into(),
                vec!["old".into(), "new".into()],
            ),
            E::FileChanged => (
                "Context".into(),
                "When a source file is modified on disk.".into(),
                vec!["path".into()],
            ),
            E::SubagentStart => (
                "Agents".into(),
                "When a subagent is spawned.".into(),
                vec!["agent_name".into()],
            ),
            E::SubagentStop => (
                "Agents".into(),
                "When a subagent finishes.".into(),
                vec!["agent_name".into(), "reason".into()],
            ),
            E::TeammateIdle => (
                "Agents".into(),
                "When a teammate goes idle.".into(),
                vec!["agent_name".into()],
            ),
            E::TeamTaskCreated => (
                "Agents".into(),
                "When a team task is created (before committing).".into(),
                vec!["task_id".into(), "team_name".into(), "subject".into()],
            ),
            E::TeamTaskCompleted => (
                "Agents".into(),
                "When a team task is marked completed.".into(),
                vec!["task_id".into(), "team_name".into()],
            ),
            E::TaskCreated => (
                "Agents".into(),
                "When a task is created (Claude Code standard).".into(),
                vec!["task_id".into(), "subject".into()],
            ),
            E::TaskCompleted => (
                "Agents".into(),
                "When a task is completed (Claude Code standard).".into(),
                vec!["task_id".into()],
            ),
            E::WorktreeCreate => (
                "Worktree".into(),
                "When a git worktree is created.".into(),
                vec!["path".into()],
            ),
            E::WorktreeRemove => (
                "Worktree".into(),
                "When a git worktree is removed.".into(),
                vec!["path".into()],
            ),
            E::PermissionRequest => (
                "Permissions".into(),
                "When a tool permission is requested (before user prompt).".into(),
                vec!["tool_name".into()],
            ),
            E::PermissionDenied => (
                "Permissions".into(),
                "When a tool permission is denied.".into(),
                vec!["tool_name".into()],
            ),
        }
    };

    [
        E::PreToolUse,
        E::PostToolUse,
        E::PostToolUseFailure,
        E::PostToolBatch,
        E::SessionStart,
        E::SessionEnd,
        E::Stop,
        E::StopFailure,
        E::UserPromptSubmit,
        E::UserPromptExpansion,
        E::Notification,
        E::Elicitation,
        E::ElicitationResult,
        E::PreCompact,
        E::PostCompact,
        E::InstructionsLoaded,
        E::ConfigChange,
        E::CwdChanged,
        E::FileChanged,
        E::SubagentStart,
        E::SubagentStop,
        E::TeammateIdle,
        E::TeamTaskCreated,
        E::TeamTaskCompleted,
        E::TaskCreated,
        E::TaskCompleted,
        E::WorktreeCreate,
        E::WorktreeRemove,
        E::PermissionRequest,
        E::PermissionDenied,
    ]
    .into_iter()
    .map(|e| {
        let (category, description, payload_fields) = describe(&e);
        HookEventInfo {
            name: e.to_string(),
            category,
            description,
            payload_fields,
        }
    })
    .collect()
}

/// List every hook event the engine can fire, with descriptive metadata.
#[tauri::command]
pub async fn list_hook_events() -> Result<Vec<HookEventInfo>, String> {
    Ok(hook_event_catalog())
}

// ─── profiles ───────────────────────────────────────────────────────────────

fn builtin_profile_infos() -> Vec<BuiltinProfileInfo> {
    use shannon_engine::permission_profile::PermissionProfile::*;
    [Strict, Balanced, Permissive]
        .into_iter()
        .map(|p| {
            let rules = p.rules();
            let id = match p {
                Strict => "strict",
                Balanced => "balanced",
                Permissive => "permissive",
                Custom(_) => "custom",
            };
            BuiltinProfileInfo {
                id: id.into(),
                description: p.description().into(),
                auto_approve_read: rules.auto_approve_read,
                auto_approve_write: rules.auto_approve_write,
                auto_approve_bash: rules.auto_approve_bash,
                auto_approve_delete: rules.auto_approve_delete,
                auto_approve_network: rules.auto_approve_network,
                deny_destructive: rules.deny_destructive,
            }
        })
        .collect()
}

/// List built-in and custom permission profiles.
///
/// Custom profiles are loaded fresh from disk on every call so newly created
/// files show up immediately in the UI.
#[tauri::command]
pub async fn list_permission_profiles(_state: State<'_, AppState>) -> Result<ProfilesList, String> {
    let registry = shannon_engine::custom_profiles::CustomProfileRegistry::load_from_dirs();
    let custom = registry
        .all()
        .values()
        .map(|def| CustomProfileInfo {
            name: def.name.clone(),
            description: def.description.clone(),
            auto_approve: def.auto_approve.clone(),
            confirm: def.confirm.clone(),
            deny: def.deny.clone(),
            source_path: None,
        })
        .collect();

    Ok(ProfilesList {
        builtin: builtin_profile_infos(),
        custom,
    })
}

/// Resolve the project-local profiles directory (`.shannon/profiles/`),
/// creating it if missing.
fn local_profiles_dir() -> Result<PathBuf, String> {
    let dir = PathBuf::from(".shannon").join("profiles");
    std::fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    Ok(dir)
}

/// Persist a custom profile to `.shannon/profiles/<name>.toml`.
///
/// Overwrites existing files with the same name. The `name` field on the
/// payload is ignored — the filename is the source of truth — so callers
/// can rename safely by delete + save.
#[tauri::command]
pub async fn save_custom_profile(
    _state: State<'_, AppState>,
    name: String,
    description: Option<String>,
    auto_approve: Vec<String>,
    confirm: Vec<String>,
    deny: Vec<String>,
) -> Result<CustomProfileInfo, String> {
    let trimmed = name.trim().to_string();
    if trimmed.is_empty() {
        return Err("profile name must not be empty".into());
    }
    if !trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("profile name may only contain letters, digits, '-' and '_'".into());
    }

    let dir = local_profiles_dir()?;
    let path = dir.join(format!("{trimmed}.toml"));

    let description = description.unwrap_or_default();
    let body = render_profile_toml(&trimmed, &description, &auto_approve, &confirm, &deny);

    std::fs::write(&path, body).map_err(|e| format!("write {}: {e}", path.display()))?;
    tracing::info!(path = %path.display(), "saved custom permission profile");

    Ok(CustomProfileInfo {
        name: trimmed,
        description,
        auto_approve,
        confirm,
        deny,
        source_path: Some(path.to_string_lossy().into_owned()),
    })
}

/// Render a profile TOML by hand. Avoids pulling the `toml` crate into
/// shannon-desktop's direct deps and matches the format expected by
/// `CustomProfileRegistry::parse_file`.
fn render_profile_toml(
    name: &str,
    description: &str,
    auto_approve: &[String],
    confirm: &[String],
    deny: &[String],
) -> String {
    let mut out = String::new();
    out.push_str("# Custom permission profile — managed by shannon-desktop.\n\n");
    out.push_str(&format!("name = {}\n", toml_basic_string(name)));
    out.push_str(&format!(
        "description = {}\n",
        toml_basic_string(description)
    ));
    out.push_str(&format!(
        "auto_approve = {}\n",
        toml_string_array(auto_approve)
    ));
    out.push_str(&format!("confirm = {}\n", toml_string_array(confirm)));
    out.push_str(&format!("deny = {}\n", toml_string_array(deny)));
    out
}

fn toml_basic_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

fn toml_string_array(items: &[String]) -> String {
    if items.is_empty() {
        return "[]".to_string();
    }
    let mut out = String::from("[");
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(&toml_basic_string(item));
    }
    out.push(']');
    out
}

/// Delete a custom profile file.
///
/// Searches both `.shannon/profiles/` and `.claude/profiles/`. Returns the
/// paths that were removed (usually one; zero if the profile didn't exist
/// in a writable location).
#[tauri::command]
pub async fn delete_custom_profile(
    _state: State<'_, AppState>,
    name: String,
) -> Result<Vec<String>, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("profile name must not be empty".into());
    }

    let candidates = [
        PathBuf::from(".shannon")
            .join("profiles")
            .join(format!("{trimmed}.toml")),
        PathBuf::from(".claude")
            .join("profiles")
            .join(format!("{trimmed}.toml")),
    ];

    let mut removed = Vec::new();
    for path in candidates {
        if path.is_file() {
            std::fs::remove_file(&path).map_err(|e| format!("remove {}: {e}", path.display()))?;
            tracing::info!(path = %path.display(), "deleted custom permission profile");
            removed.push(path.to_string_lossy().into_owned());
        }
    }

    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_covers_all_hook_variants() {
        let catalog = hook_event_catalog();
        // Sanity: the engine exposes 30 variants — if new ones are added
        // the catalog must grow too.
        assert_eq!(catalog.len(), 30, "hook event catalog out of sync");
        // No duplicate names.
        let mut names: Vec<_> = catalog.iter().map(|h| h.name.clone()).collect();
        names.sort();
        let initial = names.len();
        names.dedup();
        assert_eq!(
            names.len(),
            initial,
            "duplicate hook event names in catalog"
        );
    }

    #[test]
    fn catalog_has_required_fields() {
        for info in hook_event_catalog() {
            assert!(!info.name.is_empty(), "missing name");
            assert!(!info.category.is_empty(), "{} missing category", info.name);
            assert!(
                !info.description.is_empty(),
                "{} missing description",
                info.name
            );
            assert!(
                !info.payload_fields.is_empty(),
                "{} missing payload_fields",
                info.name
            );
        }
    }

    #[test]
    fn builtin_profiles_are_three() {
        let builtins = builtin_profile_infos();
        assert_eq!(builtins.len(), 3);
        let ids: Vec<_> = builtins.iter().map(|b| b.id.as_str()).collect();
        assert!(ids.contains(&"strict"));
        assert!(ids.contains(&"balanced"));
        assert!(ids.contains(&"permissive"));
    }

    #[test]
    fn strict_profile_blocks_writes_and_bash() {
        let strict = builtin_profile_infos()
            .into_iter()
            .find(|b| b.id == "strict")
            .unwrap();
        assert!(!strict.auto_approve_write);
        assert!(!strict.auto_approve_bash);
        assert!(strict.deny_destructive.contains(&"Bash".to_string()));
    }
}
