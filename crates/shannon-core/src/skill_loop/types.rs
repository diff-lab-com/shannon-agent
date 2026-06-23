//! Data types for the self-evolving skill loop
//!
//! This module defines the core data structures used throughout the skill loop system:
//! - Task evaluation input from completed tasks
//! - Evaluation results with confidence scores
//! - Skill proposals awaiting user approval
//! - Proposal statuses and metadata

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use uuid::Uuid;

/// Task execution metadata for skill extraction evaluation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskEvaluation {
    /// Task duration in seconds
    pub duration_secs: u64,

    /// Total number of tool calls (including repeats)
    pub tool_call_count: usize,

    /// Original user prompt (complete input)
    pub user_prompt: String,

    /// Task execution outcome
    pub outcome: TaskOutcome,

    /// Set of tool names used (deduplicated)
    pub tool_names_used: HashSet<String>,

    /// Task start time (optional, for context)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<i64>,

    /// Task end time (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<i64>,
}

/// Task execution result status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TaskOutcome {
    /// Task completed successfully
    Success,
    /// Task partially succeeded (e.g., 2 of 3 files processed)
    Partial,
    /// Task failed (should not extract skill)
    Failure,
}

/// Task evaluation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationResult {
    /// Whether to suggest generating a skill proposal
    pub suggest: bool,

    /// Human-readable explanation (for UI display)
    pub reason: String,

    /// Confidence score (0.0-1.0), for UI strength indication
    pub confidence: f32,

    /// Dimension scores (for debugging)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scores: Option<EvaluationScores>,
}

/// Evaluation scores per dimension
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationScores {
    /// Duration score (0-100, >5 minutes = high score)
    pub duration_score: f32,

    /// Complexity score (0-100, many tools = high score)
    pub complexity_score: f32,

    /// Goal clarity score (0-100, clear prompt with verbs + deliverables = high score)
    pub clarity_score: f32,

    /// Success status score (0-100, Success=100, Partial=50, Failure=0)
    pub success_score: f32,
}

/// Skill proposal awaiting user approval
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillProposal {
    /// Unique identifier (UUID v4)
    pub id: Uuid,

    /// Skill display name (user-editable)
    pub name: String,

    /// URL-safe identifier (for filename, generated from name)
    pub slug: String,

    /// Skill description (1-2 sentences)
    pub description: String,

    /// Trigger pattern list (natural language descriptions of trigger scenarios)
    pub trigger_patterns: Vec<String>,

    /// Example workflow (concrete operation steps, markdown format)
    pub example_workflow: String,

    /// Source task ID (optional, for traceability)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_task_id: Option<String>,

    /// Creation timestamp
    pub created_at: DateTime<Utc>,

    /// Approval status
    pub status: ProposalStatus,

    /// Suggested skill metadata (for writing TOML)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested_metadata: Option<SkillMetadataDraft>,
}

/// Approval status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ProposalStatus {
    /// Pending review (default)
    Pending,
    /// Approved (written to ~/.shannon/skills/)
    Approved,
    /// Rejected (not persisted)
    Rejected,
}

/// Suggested skill metadata (referencing shannon-skills definition format)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillMetadataDraft {
    /// Skill aliases (for invocation)
    pub aliases: Vec<String>,

    /// Parameter hint (e.g., "project name" or "file path")
    pub argument_hint: Option<String>,

    /// Allowed tools list (empty = no restrictions)
    pub allowed_tools: Vec<String>,

    /// Model override (optional)
    pub model: Option<String>,

    /// Whether user can invoke (default true)
    pub user_invocable: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_outcome_partial_eq() {
        assert_eq!(TaskOutcome::Success, TaskOutcome::Success);
        assert_ne!(TaskOutcome::Success, TaskOutcome::Failure);
    }

    #[test]
    fn test_proposal_status_PartialEq() {
        assert_eq!(ProposalStatus::Pending, ProposalStatus::Pending);
        assert_ne!(ProposalStatus::Pending, ProposalStatus::Approved);
    }

    #[test]
    fn test_evaluation_result_serialization() {
        let result = EvaluationResult {
            suggest: true,
            reason: "Complex multi-step task".to_string(),
            confidence: 0.85,
            scores: Some(EvaluationScores {
                duration_score: 90.0,
                complexity_score: 80.0,
                clarity_score: 75.0,
                success_score: 100.0,
            }),
        };

        let json = serde_json::to_string(&result).unwrap();
        let parsed: EvaluationResult = serde_json::from_str(&json).unwrap();

        assert!(parsed.suggest);
        assert_eq!(parsed.reason, "Complex multi-step task");
        assert!((parsed.confidence - 0.85).abs() < 0.01);
    }
}
