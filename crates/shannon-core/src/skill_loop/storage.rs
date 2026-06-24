//! Storage layer for skill proposals
//!
//! This module handles reading and writing proposal files to disk, with atomic
//! writes and proper file permissions.

use crate::skill_loop::types::SkillProposal;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{instrument, trace};

/// Persisted proposal wrapper with versioning
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct PersistedProposal {
    /// Proposal data
    pub proposal: SkillProposal,
    /// Serialization version (for future migrations)
    pub version: u8,
}

impl PersistedProposal {
    fn new(proposal: SkillProposal) -> Self {
        Self {
            proposal,
            version: 1,
        }
    }
}

/// Save a proposal to the proposals directory
///
/// Creates an atomic write using temp file + rename pattern.
///
/// # Errors
/// Returns IO errors if directory creation or file write fails
#[instrument(skip(proposal))]
pub fn save_proposal(
    dir: &Path,
    proposal: &SkillProposal,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    fs::create_dir_all(dir)?;

    let proposal_path = dir.join(format!("{id}.json", id = proposal.id));
    let persisted = PersistedProposal::new(proposal.clone());
    let content = serde_json::to_string_pretty(&persisted)?;

    // Atomic write: temp file + rename
    let temp_path = proposal_path.with_extension("tmp");
    fs::write(&temp_path, content)?;
    fs::rename(&temp_path, &proposal_path)?;

    trace!("Saved proposal to {:?}", proposal_path);
    Ok(proposal_path)
}

/// Load all proposals from the proposals directory
///
/// # Errors
/// Returns IO errors if directory cannot be read, or JSON errors for invalid files
#[instrument(skip(dir))]
pub fn load_proposals(dir: &Path) -> Result<Vec<SkillProposal>, Box<dyn std::error::Error>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut proposals = Vec::new();

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        // Skip non-JSON files
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }

        let content = fs::read_to_string(&path)?;
        let persisted: PersistedProposal = serde_json::from_str(&content)?;
        proposals.push(persisted.proposal);
    }

    // Sort by creation time, newest first
    proposals.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    trace!("Loaded {} proposals from {:?}", proposals.len(), dir);
    Ok(proposals)
}

/// Delete a proposal by ID
///
/// # Errors
/// Returns IO errors if file cannot be deleted
#[instrument(skip(dir))]
pub fn delete_proposal(dir: &Path, id: uuid::Uuid) -> Result<(), Box<dyn std::error::Error>> {
    let proposal_path = dir.join(format!("{id}.json"));

    if proposal_path.exists() {
        fs::remove_file(&proposal_path)?;
        trace!("Deleted proposal {:?}", proposal_path);
    }

    Ok(())
}

/// Approve a proposal and write it as a skill file
///
/// Writes the proposal to `~/.shannon/skills/user-proposed/{slug}.toml`
/// and deletes the proposal draft.
///
/// # Errors
/// Returns IO errors if skill file cannot be written or proposal deleted
#[instrument(skip(proposal, skills_dir))]
pub fn approve_proposal(
    proposal: &SkillProposal,
    skills_dir: &Path,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let user_proposed_dir = skills_dir.join("user-proposed");
    fs::create_dir_all(&user_proposed_dir)?;

    let skill_path = user_proposed_dir.join(format!("{slug}.toml", slug = proposal.slug));
    let toml_content = generate_skill_toml(proposal)?;

    // Atomic write
    let temp_path = skill_path.with_extension("tmp");
    fs::write(&temp_path, toml_content)?;
    fs::rename(&temp_path, &skill_path)?;

    trace!("Approved and wrote skill to {:?}", skill_path);
    Ok(skill_path)
}

/// Generate TOML content for a skill proposal
fn generate_skill_toml(proposal: &SkillProposal) -> Result<String, Box<dyn std::error::Error>> {
    let mut output = String::new();

    // Header
    output.push_str(&format!("name = \"{name}\"\n", name = proposal.name));
    output.push_str(&format!(
        "description = \"{desc}\"\n\n",
        desc = proposal.description
    ));

    // Triggers
    output.push_str("[triggers]\npatterns = [\n");
    for pattern in &proposal.trigger_patterns {
        output.push_str(&format!("  \"{pattern}\",\n"));
    }
    output.push_str("]\n\n");

    // Workflow
    output.push_str("[workflow]\nsteps = \"\"\"\n");
    output.push_str(&proposal.example_workflow);
    output.push_str("\"\"\"\n");

    // Optional metadata
    if let Some(ref meta) = proposal.suggested_metadata {
        output.push_str("\n[metadata]\n");

        if !meta.aliases.is_empty() {
            output.push_str("aliases = [\n");
            for alias in &meta.aliases {
                output.push_str(&format!("  \"{alias}\",\n"));
            }
            output.push_str("]\n");
        }

        if let Some(ref hint) = meta.argument_hint {
            output.push_str(&format!("argument_hint = \"{hint}\"\n"));
        }

        if !meta.allowed_tools.is_empty() {
            output.push_str("allowed_tools = [\n");
            for tool in &meta.allowed_tools {
                output.push_str(&format!("  \"{tool}\",\n"));
            }
            output.push_str("]\n");
        }
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use tempfile::TempDir;

    fn create_test_proposal() -> SkillProposal {
        SkillProposal {
            id: uuid::Uuid::new_v4(),
            name: "Test Skill".to_string(),
            slug: "test-skill".to_string(),
            description: "A test skill".to_string(),
            trigger_patterns: vec!["when user asks to test".to_string()],
            example_workflow: "1. Test this\n2. Test that".to_string(),
            source_task_id: Some("task_001".to_string()),
            created_at: Utc::now(),
            status: crate::skill_loop::types::ProposalStatus::Pending,
            suggested_metadata: Some(crate::skill_loop::types::SkillMetadataDraft {
                aliases: vec!["test".to_string()],
                argument_hint: Some("Test input".to_string()),
                allowed_tools: vec!["read".to_string()],
                model: None,
                user_invocable: true,
            }),
        }
    }

    #[test]
    fn test_save_and_load_proposal() {
        let temp_dir = TempDir::new().unwrap();
        let proposal = create_test_proposal();

        let path = save_proposal(temp_dir.path(), &proposal).unwrap();
        assert!(path.exists());

        let loaded = load_proposals(temp_dir.path()).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "Test Skill");
        assert_eq!(loaded[0].slug, "test-skill");
    }

    #[test]
    fn test_delete_proposal() {
        let temp_dir = TempDir::new().unwrap();
        let proposal = create_test_proposal();

        save_proposal(temp_dir.path(), &proposal).unwrap();
        assert_eq!(load_proposals(temp_dir.path()).unwrap().len(), 1);

        delete_proposal(temp_dir.path(), proposal.id).unwrap();
        assert_eq!(load_proposals(temp_dir.path()).unwrap().len(), 0);
    }

    #[test]
    fn test_approve_proposal() {
        let _temp_dir = TempDir::new().unwrap();
        let skills_dir = TempDir::new().unwrap();
        let proposal = create_test_proposal();

        let skill_path = approve_proposal(&proposal, skills_dir.path()).unwrap();

        assert!(skill_path.ends_with("test-skill.toml"));
        assert!(skill_path.exists());

        let content = fs::read_to_string(&skill_path).unwrap();
        assert!(content.contains("name = \"Test Skill\""));
        assert!(content.contains("description = \"A test skill\""));
        assert!(content.contains("[triggers]"));
        assert!(content.contains("[workflow]"));
    }

    #[test]
    fn test_generate_skill_toml() {
        let proposal = create_test_proposal();
        let toml = generate_skill_toml(&proposal).unwrap();

        assert!(toml.contains("name = \"Test Skill\""));
        assert!(toml.contains("description = \"A test skill\""));
        assert!(toml.contains("[triggers]"));
        assert!(toml.contains("[workflow]"));
        assert!(toml.contains("1. Test this"));
        assert!(toml.contains("[metadata]"));
    }

    #[test]
    fn test_load_proposals_empty_dir() {
        let temp_dir = TempDir::new().unwrap();
        let proposals = load_proposals(temp_dir.path()).unwrap();
        assert_eq!(proposals.len(), 0);
    }
}
