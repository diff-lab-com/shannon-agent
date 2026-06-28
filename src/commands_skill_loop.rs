//! Skill loop commands — task evaluation → candidate generation → user review → install.
//!
//! Semi-automated: evaluation runs automatically, but generation and installation
//! require explicit user approval.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tauri::Emitter;

use crate::commands::AppState;
use shannon_core::skill_loop::{self};
use shannon_engine::api::client::LlmClient;

// Re-export types for frontend (via Tauri auto-serialization)
pub use shannon_core::skill_loop::{
    EvaluationResult, ProposalStatus, SkillProposal, TaskEvaluation, TaskOutcome,
};

/// Skill proposal count event payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillProposalCountPayload {
    pub pending_count: usize,
}

/// Evaluate a task to determine if it's worth extracting as a skill.
#[tauri::command]
#[tracing::instrument(skip_all)]
#[allow(clippy::too_many_arguments)]
pub async fn skill_loop_evaluate(
    state: tauri::State<'_, AppState>,
    duration_secs: u64,
    tool_call_count: usize,
    user_prompt: String,
    outcome: String,
    tool_names_used: Vec<String>,
    started_at: Option<i64>,
    completed_at: Option<i64>,
) -> Result<EvaluationResult, String> {
    // Parse outcome string to enum
    let task_outcome = match outcome.as_str() {
        "Success" => TaskOutcome::Success,
        "Partial" => TaskOutcome::Partial,
        "Failure" => TaskOutcome::Failure,
        _ => return Err(format!("Invalid outcome: {outcome}")),
    };

    let evaluation = TaskEvaluation {
        duration_secs,
        tool_call_count,
        user_prompt,
        outcome: task_outcome,
        tool_names_used: tool_names_used.into_iter().collect::<HashSet<_>>(),
        started_at,
        completed_at,
    };

    // Get LLM client from state
    let client_config = state.client_config.read().await.clone();
    let client = LlmClient::new(client_config);

    // Call shannon-core evaluation
    skill_loop::evaluate_task(&client, evaluation)
        .await
        .map_err(|e| format!("Evaluation failed: {e}"))
}

/// Generate a skill proposal draft (called after user approves generation).
#[tauri::command]
#[tracing::instrument(skip_all)]
#[allow(clippy::too_many_arguments)]
pub async fn skill_loop_generate(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
    duration_secs: u64,
    tool_call_count: usize,
    user_prompt: String,
    outcome: String,
    tool_names_used: Vec<String>,
    started_at: Option<i64>,
    completed_at: Option<i64>,
) -> Result<SkillProposal, String> {
    // Parse outcome
    let task_outcome = match outcome.as_str() {
        "Success" => TaskOutcome::Success,
        "Partial" => TaskOutcome::Partial,
        "Failure" => TaskOutcome::Failure,
        _ => return Err(format!("Invalid outcome: {outcome}")),
    };

    let evaluation = TaskEvaluation {
        duration_secs,
        tool_call_count,
        user_prompt,
        outcome: task_outcome,
        tool_names_used: tool_names_used.into_iter().collect::<HashSet<_>>(),
        started_at,
        completed_at,
    };

    // Get LLM client
    let client_config = state.client_config.read().await.clone();
    let client = LlmClient::new(client_config);

    // Generate proposal
    let proposal = skill_loop::generate_skill_proposal(&client, evaluation)
        .await
        .map_err(|e| format!("Generation failed: {e}"))?;

    // Save proposal to disk
    let proposals_dir = proposals_directory()?;
    let _proposal_path = skill_loop::save_proposal(&proposals_dir, &proposal)
        .map_err(|e| format!("Failed to save proposal: {e}"))?;

    // Emit proposal count event
    let pending_count = count_pending_proposals(proposals_dir.as_path())?;
    let payload = SkillProposalCountPayload { pending_count };
    let _ = app_handle.emit(
        crate::events::event_names::SKILL_PROPOSAL_AVAILABLE,
        payload,
    );

    Ok(proposal)
}

/// List all pending proposals.
#[tauri::command]
pub async fn skill_loop_list_proposals() -> Result<Vec<SkillProposal>, String> {
    let proposals_dir = proposals_directory()?;

    if !proposals_dir.exists() {
        return Ok(Vec::new());
    }

    let proposals = skill_loop::load_proposals(&proposals_dir)
        .map_err(|e| format!("Failed to load proposals: {e}"))?;

    // Filter only pending proposals
    let pending: Vec<_> = proposals
        .into_iter()
        .filter(|p| p.status == ProposalStatus::Pending)
        .collect();

    Ok(pending)
}

/// Approve a proposal and write to ~/.shannon/skills/user-proposed/.
#[tauri::command]
#[tracing::instrument(skip_all)]
pub async fn skill_loop_approve(
    proposal_id: String,
    app_handle: tauri::AppHandle,
) -> Result<PathBuf, String> {
    let proposals_dir = proposals_directory()?;

    // Parse UUID
    let uuid =
        uuid::Uuid::parse_str(&proposal_id).map_err(|e| format!("Invalid proposal ID: {e}"))?;

    // Load proposal
    let proposals = skill_loop::load_proposals(&proposals_dir)
        .map_err(|e| format!("Failed to load proposals: {e}"))?;
    let proposal = proposals
        .into_iter()
        .find(|p| p.id == uuid)
        .ok_or_else(|| format!("Proposal not found: {proposal_id}"))?;

    // Check for duplicates using shannon-core dedup
    let skills_dir = skills_directory()?;
    let user_proposed_dir = skills_dir.join("user-proposed");

    // Load existing skills for deduplication
    let existing_skills: Vec<String> = std::fs::read_dir(&user_proposed_dir)
        .ok()
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("toml"))
                .filter_map(|e| std::fs::read_to_string(e.path()).ok())
                .collect()
        })
        .unwrap_or_default();

    if let Some(similarity) = skill_loop::find_similar_skill(&proposal, &existing_skills) {
        if similarity > 0.8 {
            return Err(format!(
                "Similar skill already exists (similarity: {similarity:.2})"
            ));
        }
    }

    // Approve and write to user-proposed directory
    let skill_path = skill_loop::approve_proposal(&proposal, &skills_dir)
        .map_err(|e| format!("Failed to approve proposal: {e}"))?;

    // Delete original proposal
    skill_loop::delete_proposal(&proposals_dir, uuid)
        .map_err(|e| format!("Failed to delete proposal: {e}"))?;

    // Emit updated count event
    let pending_count = count_pending_proposals(proposals_dir.as_path())?;
    let payload = SkillProposalCountPayload { pending_count };
    let _ = app_handle.emit(
        crate::events::event_names::SKILL_PROPOSAL_AVAILABLE,
        payload,
    );

    Ok(skill_path)
}

/// Reject a proposal and delete the draft.
#[tauri::command]
pub async fn skill_loop_reject(
    proposal_id: String,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    let proposals_dir = proposals_directory()?;

    // Parse UUID
    let uuid =
        uuid::Uuid::parse_str(&proposal_id).map_err(|e| format!("Invalid proposal ID: {e}"))?;

    // Delete proposal
    skill_loop::delete_proposal(&proposals_dir, uuid)
        .map_err(|e| format!("Failed to delete proposal: {e}"))?;

    // Emit updated count event
    let pending_count = count_pending_proposals(proposals_dir.as_path())?;
    let payload = SkillProposalCountPayload { pending_count };
    let _ = app_handle.emit(
        crate::events::event_names::SKILL_PROPOSAL_AVAILABLE,
        payload,
    );

    Ok(())
}

// ===== Helper Functions =====

/// Get proposals directory: ~/.shannon/skill-loop/proposals/
fn proposals_directory() -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or("Cannot determine home directory")?;
    Ok(home.join(".shannon").join("skill-loop").join("proposals"))
}

/// Get skills directory: ~/.shannon/skills/
fn skills_directory() -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or("Cannot determine home directory")?;
    Ok(home.join(".shannon").join("skills"))
}

/// Count pending proposals in a directory.
fn count_pending_proposals(proposals_dir: &Path) -> Result<usize, String> {
    if !proposals_dir.exists() {
        return Ok(0);
    }

    let proposals = skill_loop::load_proposals(proposals_dir)
        .map_err(|e| format!("Failed to load proposals for counting: {e}"))?;

    Ok(proposals
        .iter()
        .filter(|p| p.status == ProposalStatus::Pending)
        .count())
}

/// Save a generated proposal to disk and return the resulting pending count.
/// Used by the auto-evaluation hook (`commands.rs::send_message`) so it emits
/// the *real* proposal count instead of a hardcoded placeholder — after an
/// actual proposal has been generated and persisted.
pub(crate) fn save_proposal_and_count(proposal: &SkillProposal) -> Result<usize, String> {
    let proposals_dir = proposals_directory()?;
    skill_loop::save_proposal(&proposals_dir, proposal)
        .map_err(|e| format!("Failed to save proposal: {e}"))?;
    count_pending_proposals(proposals_dir.as_path())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_skill_proposal_count_payload_serialization() {
        let payload = SkillProposalCountPayload { pending_count: 3 };
        let json = serde_json::to_string(&payload).unwrap();
        let back: SkillProposalCountPayload = serde_json::from_str(&json).unwrap();
        assert_eq!(back.pending_count, 3);
    }

    #[test]
    fn test_proposals_directory_under_home() {
        let home = dirs::home_dir().expect("home directory");
        let proposals_dir = proposals_directory().expect("proposals directory");
        assert!(proposals_dir.starts_with(&home));
        assert!(proposals_dir.ends_with(".shannon/skill-loop/proposals"));
    }

    #[test]
    fn test_skills_directory_under_home() {
        let home = dirs::home_dir().expect("home directory");
        let skills_dir = skills_directory().expect("skills directory");
        assert!(skills_dir.starts_with(&home));
        assert!(skills_dir.ends_with(".shannon/skills"));
    }

    #[test]
    fn test_task_outcome_parsing() {
        assert_eq!(
            match "Success" {
                "Success" => TaskOutcome::Success,
                _ => unreachable!(),
            },
            TaskOutcome::Success
        );
    }

    // Serialize filesystem-touching tests behind a single mutex so the
    // process-global HOME override can't race across parallel tests.
    static FS_HOME_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// RAII guard that points HOME at a temp dir for the duration of a test,
    /// restoring the previous value on drop.
    struct HomeGuard {
        prev: Option<String>,
    }
    impl HomeGuard {
        fn new(tmp: &std::path::Path) -> Self {
            let prev = std::env::var("HOME").ok();
            // SAFETY: every caller holds FS_HOME_LOCK, serializing HOME mutation.
            unsafe { std::env::set_var("HOME", tmp) };
            HomeGuard { prev }
        }
    }
    impl Drop for HomeGuard {
        fn drop(&mut self) {
            // SAFETY: every caller holds FS_HOME_LOCK.
            unsafe {
                match &self.prev {
                    Some(h) => std::env::set_var("HOME", h),
                    None => std::env::remove_var("HOME"),
                }
            }
        }
    }

    fn pending_proposal() -> SkillProposal {
        SkillProposal {
            id: uuid::Uuid::new_v4(),
            name: "Test Skill".into(),
            slug: "test-skill".into(),
            description: "desc".into(),
            trigger_patterns: vec!["when X".into()],
            example_workflow: "1. do Y".into(),
            source_task_id: None,
            created_at: chrono::Utc::now(),
            status: ProposalStatus::Pending,
            suggested_metadata: None,
        }
    }

    #[test]
    fn save_proposal_and_count_persists_and_counts() {
        let _guard = FS_HOME_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().expect("tempdir");
        let _home = HomeGuard::new(tmp.path());

        // first saved proposal => 1 pending
        assert_eq!(save_proposal_and_count(&pending_proposal()).unwrap(), 1);
        // a second pending proposal => 2
        assert_eq!(save_proposal_and_count(&pending_proposal()).unwrap(), 2);
    }
}
