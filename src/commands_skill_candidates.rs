//! Skill-candidate commands — recurring-pattern detection surface for the
//! self-improvement loop (D6 Phase 2).
//!
//! Candidates represent potential skills detected by analyzing recurring
//! tool-call patterns across sessions. They are persisted as JSONL at
//! `~/.shannon/desktop/skill-candidates.jsonl`. The detection cron (C2,
//! `automation_commands.rs`) writes here; this module surfaces them to the
//! UI plus handles approve / reject.
//!
//! Approving a candidate promotes it to an agent-authored skill at
//! `~/.shannon/skills/agent-authored/<slug>.json` and removes the candidate.
//! Rejecting just removes the candidate.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::Emitter;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillCandidate {
    pub id: String,
    pub detected_at: String,
    pub occurrence_count: u32,
    pub example_session_ids: Vec<String>,
    pub proposed_name: String,
    pub proposed_trigger: String,
    pub procedure: Vec<String>,
    pub source_tool_calls: Vec<SourceToolCall>,
    /// True after refine_skill_candidate has produced an LLM-polished
    /// procedure. Persists across detections so the catalog UI can badge
    /// refined entries.
    #[serde(default)]
    pub refined: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceToolCall {
    pub tool: String,
    pub args_summary: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentAuthoredSkill {
    pub id: String,
    pub name: String,
    pub description: String,
    pub trigger: String,
    pub procedure: Vec<String>,
    pub created_at: String,
    pub originating_sessions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentAuthoredSkillEdits {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub trigger: Option<String>,
    #[serde(default)]
    pub procedure: Option<Vec<String>>,
}

fn candidates_file() -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or("Cannot determine home directory")?;
    let dir = home.join(".shannon").join("desktop");
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create {}: {e}", dir.display()))?;
    Ok(dir.join("skill-candidates.jsonl"))
}

fn agent_authored_dir() -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or("Cannot determine home directory")?;
    let dir = home.join(".shannon").join("skills").join("agent-authored");
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create {}: {e}", dir.display()))?;
    Ok(dir)
}

fn load_candidates() -> Result<Vec<SkillCandidate>, String> {
    let path = candidates_file()?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let contents =
        std::fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let mut out = Vec::new();
    for (lineno, line) in contents.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match serde_json::from_str::<SkillCandidate>(trimmed) {
            Ok(c) => out.push(c),
            Err(e) => return Err(format!("candidates:{}: parse error: {e}", lineno + 1)),
        }
    }
    Ok(out)
}

fn save_candidates(candidates: &[SkillCandidate]) -> Result<(), String> {
    let path = candidates_file()?;
    let mut out = String::new();
    for c in candidates {
        let line = serde_json::to_string(c).map_err(|e| format!("serialize: {e}"))?;
        out.push_str(&line);
        out.push('\n');
    }
    std::fs::write(&path, out).map_err(|e| format!("write {}: {e}", path.display()))
}

fn slugify(input: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if ch == '-' || ch.is_whitespace() || ch == '_' {
            if !prev_dash && !out.is_empty() {
                out.push('-');
                prev_dash = true;
            }
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

/// Take an optional String, trim it, and return Some(String) only when it
/// contains at least one non-whitespace character.
fn non_empty(s: &Option<String>) -> Option<String> {
    s.as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

#[tauri::command]
pub async fn list_skill_candidates() -> Result<Vec<SkillCandidate>, String> {
    load_candidates()
}

#[tauri::command]
pub async fn approve_skill_candidate(
    app: tauri::AppHandle,
    id: String,
    edits: Option<AgentAuthoredSkillEdits>,
) -> Result<AgentAuthoredSkill, String> {
    let mut candidates = load_candidates()?;
    let idx = candidates
        .iter()
        .position(|c| c.id == id)
        .ok_or_else(|| format!("Candidate {id} not found"))?;
    let candidate = candidates.remove(idx);
    save_candidates(&candidates)?;

    let name = edits
        .as_ref()
        .and_then(|e| non_empty(&e.name))
        .unwrap_or(candidate.proposed_name);
    let trigger = edits
        .as_ref()
        .and_then(|e| non_empty(&e.trigger))
        .unwrap_or(candidate.proposed_trigger);
    let procedure = edits
        .as_ref()
        .and_then(|e| e.procedure.clone())
        .filter(|p| !p.is_empty())
        .unwrap_or(candidate.procedure);
    let description = edits
        .as_ref()
        .and_then(|e| non_empty(&e.description))
        .unwrap_or_else(|| {
            format!(
                "Auto-detected skill from {} recurring session(s)",
                candidate.occurrence_count
            )
        });

    let slug = slugify(&name);
    if slug.is_empty() {
        return Err("Skill name must contain alphanumeric characters".into());
    }
    let skill = AgentAuthoredSkill {
        id: format!("agent-{slug}"),
        name,
        description,
        trigger,
        procedure,
        created_at: chrono::Utc::now().to_rfc3339(),
        originating_sessions: candidate.example_session_ids.clone(),
    };

    let dir = agent_authored_dir()?;
    let path = dir.join(format!("{slug}.json"));
    let json = serde_json::to_string_pretty(&skill).map_err(|e| format!("serialize skill: {e}"))?;
    std::fs::write(&path, json).map_err(|e| format!("write {}: {e}", path.display()))?;

    let _ = app.emit(
        "skill-catalog-changed",
        serde_json::json!({ "slug": slug, "action": "approved" }),
    );

    Ok(skill)
}

#[tauri::command]
pub async fn reject_skill_candidate(app: tauri::AppHandle, id: String) -> Result<(), String> {
    let mut candidates = load_candidates()?;
    let before = candidates.len();
    candidates.retain(|c| c.id != id);
    if candidates.len() == before {
        return Err(format!("Candidate {id} not found"));
    }
    save_candidates(&candidates)?;
    let _ = app.emit(
        "skill-catalog-changed",
        serde_json::json!({ "action": "rejected" }),
    );
    Ok(())
}

/// Ask the configured LLM to rewrite a candidate's procedure into a
/// clean, step-by-step skill procedure. Stores the result back into
/// the candidate and marks `refined=true`. Returns the refined text
/// so the UI can show it without a re-fetch.
///
/// Falls back to the original procedure (joined with newlines) when
/// the LLM call fails — the user still sees something usable and can
/// edit it during approve.
#[tauri::command]
pub async fn refine_skill_candidate(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::commands::AppState>,
    id: String,
) -> Result<String, String> {
    let mut candidates = load_candidates()?;
    let idx = candidates
        .iter()
        .position(|c| c.id == id)
        .ok_or_else(|| format!("Candidate {id} not found"))?;
    let original = candidates[idx].procedure.join("\n");
    let trigger = candidates[idx].proposed_trigger.clone();
    let name = candidates[idx].proposed_name.clone();

    let system = "You refine agent-authored skill procedures. Output ONLY a numbered list of concrete steps, no preamble, no markdown headings. Keep each step under 120 characters. Preserve every tool the original procedure referenced.";
    let user = format!(
        "Skill name: {name}\nTrigger: {trigger}\nOriginal procedure:\n{original}\n\nRewrite as a clean numbered list:"
    );

    let client_config = state.client_config.read().await.clone();
    let client = shannon_engine::api::client::LlmClient::new(client_config);
    let messages = vec![shannon_engine::api::types::Message {
        role: "user".into(),
        content: shannon_engine::api::types::MessageContent::Text(user),
    }];

    let refined = match client
        .send_message(messages, None, Some(system.into()))
        .await
    {
        Ok(blocks) => {
            let mut out = String::new();
            for block in blocks {
                if let shannon_engine::api::types::ContentBlock::Text { text } = block {
                    if !out.is_empty() {
                        out.push('\n');
                    }
                    out.push_str(&text);
                }
            }
            if out.trim().is_empty() { original } else { out }
        }
        Err(_) => original,
    };

    let new_procedure: Vec<String> = refined
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    candidates[idx].procedure = new_procedure;
    candidates[idx].refined = true;
    save_candidates(&candidates)?;

    let _ = app.emit(
        "skill-catalog-changed",
        serde_json::json!({ "slug": id, "action": "refined" }),
    );
    Ok(refined)
}

#[tauri::command]
pub async fn list_agent_authored_skills() -> Result<Vec<AgentAuthoredSkill>, String> {
    let dir = agent_authored_dir()?;
    let mut out = Vec::new();
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("read {}: {e}", dir.display())),
    };
    for entry in entries {
        let entry = entry.map_err(|e| format!("readdir: {e}"))?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let contents = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        match serde_json::from_str::<AgentAuthoredSkill>(&contents) {
            Ok(skill) => out.push(skill),
            Err(_) => continue,
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// Append a new candidate (used by the C2 cron when it detects a pattern).
/// Public so the automation module can call it without going through Tauri.
pub fn append_candidate(candidate: SkillCandidate) -> Result<(), String> {
    let mut current = load_candidates()?;
    current.push(candidate);
    save_candidates(&current)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    /// Use a throwaway HOME so tests don't touch the user's real ~/.shannon.
    /// We can't easily override `dirs::home_dir()` for the production code,
    /// so instead we verify the slug / serde layer that doesn't touch disk.
    #[test]
    fn slugify_handles_spaces_punctuation_and_case() {
        assert_eq!(slugify("My Cool Skill"), "my-cool-skill");
        assert_eq!(slugify("UPPER-case_Test!"), "upper-case-test");
        assert_eq!(
            slugify("    leading and trailing   "),
            "leading-and-trailing"
        );
        assert_eq!(slugify("---only-dashes---"), "only-dashes");
        assert_eq!(slugify(" ironic 😎 mixed "), "ironic-mixed");
    }

    #[test]
    fn slugify_returns_empty_for_no_alphanumeric() {
        assert_eq!(slugify("😱🔥"), "");
        assert_eq!(slugify("    "), "");
    }

    #[test]
    fn candidate_round_trip_json() {
        let candidate = SkillCandidate {
            id: "test-1".into(),
            detected_at: "2026-06-26T19:00:00Z".into(),
            occurrence_count: 3,
            example_session_ids: vec!["s1".into(), "s2".into()],
            proposed_name: "Daily report".into(),
            proposed_trigger: "user says 'daily report'".into(),
            procedure: vec!["run query".into(), "send to slack".into()],
            source_tool_calls: vec![],
            refined: false,
        };
        let json = serde_json::to_string(&candidate).expect("serialize");
        let parsed: SkillCandidate = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.id, "test-1");
        assert_eq!(parsed.occurrence_count, 3);
        assert_eq!(parsed.example_session_ids.len(), 2);
        assert!(!parsed.refined);
    }

    #[test]
    fn candidate_json_without_refined_field_defaults_false() {
        let json = r#"{
            "id":"legacy",
            "detected_at":"2026-06-26T19:00:00Z",
            "occurrence_count":1,
            "example_session_ids":[],
            "proposed_name":"X",
            "proposed_trigger":"Y",
            "procedure":["step"],
            "source_tool_calls":[]
        }"#;
        let parsed: SkillCandidate = serde_json::from_str(json).expect("deserialize legacy");
        assert!(!parsed.refined);
    }

    #[test]
    fn agent_authored_skill_serialization_includes_required_fields() {
        let skill = AgentAuthoredSkill {
            id: "agent-daily-report".into(),
            name: "Daily report".into(),
            description: "Runs query and posts to Slack".into(),
            trigger: "user says 'daily report'".into(),
            procedure: vec!["step 1".into()],
            created_at: "2026-06-26T19:00:00Z".into(),
            originating_sessions: vec!["s1".into()],
        };
        let json = serde_json::to_string(&skill).expect("serialize");
        for field in [
            "agent-daily-report",
            "Daily report",
            "Runs query",
            "step 1",
            "originating_sessions",
        ] {
            assert!(json.contains(field), "expected {field} in JSON: {json}");
        }
    }

    /// Verify candidates_file path resolves under $HOME/.shannon/desktop.
    /// Skip if HOME isn't set (CI edge cases).
    #[test]
    fn candidates_file_path_under_shannon() {
        if env::var_os("HOME").is_none() && env::var_os("USERPROFILE").is_none() {
            return;
        }
        let path = candidates_file().expect("path");
        let components: Vec<_> = path
            .components()
            .map(|c| c.as_os_str().to_string_lossy().to_string())
            .collect();
        assert!(
            components.iter().any(|c| c == ".shannon"),
            "expected .shannon in path: {components:?}"
        );
        assert!(
            path.ends_with("skill-candidates.jsonl"),
            "expected skill-candidates.jsonl suffix: {}",
            path.display()
        );
    }
}
