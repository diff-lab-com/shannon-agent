//! # Self-evolving skill loop
//!
//! **Unstable API** (subject to change between minor versions):
//! - Evaluation criteria may adjust based on user feedback
//! - Prompt templates may evolve for better accuracy
//! - Proposal schema may extend with new fields
//!
//! ## Overview
//!
//! The skill loop system automatically evaluates completed tasks to identify
//! reusable patterns that can be extracted as skills. This reduces manual
//! skill creation effort and helps users build a personalized skill library.
//!
//! ## Usage
//!
//! ```rust,no_run
//! use shannon_engine::api::LlmClient;
//! use shannon_core::skill_loop::{evaluate_task, generate_skill_proposal};
//! use shannon_core::skill_loop::types::{TaskEvaluation, TaskOutcome};
//! use std::collections::HashSet;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! # let client = LlmClient::from_env();
//! let evaluation = TaskEvaluation {
//!     duration_secs: 420,
//!     tool_call_count: 8,
//!     user_prompt: "Review this PR for security issues".to_string(),
//!     outcome: TaskOutcome::Success,
//!     tool_names_used: vec!["read".into(), "grep".into(), "analyze".into()]
//!         .into_iter()
//!         .collect(),
//!     started_at: None,
//!     completed_at: None,
//! };
//!
//! // Step 1: Evaluate
//! let eval_result = evaluate_task(&client, evaluation.clone()).await?;
//! if eval_result.suggest {
//!     // Step 2: Generate proposal
//!     let proposal = generate_skill_proposal(&client, evaluation).await?;
//!     println!("Proposal: {}", proposal.name);
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ## Architecture
//!
//! The skill loop consists of three main components:
//!
//! - **Evaluator**: Analyzes task characteristics to determine if it's worth extracting
//! - **Generator**: Creates skill proposals from approved tasks
//! - **Storage**: Manages proposal files and skill installation
//!
//! ## Workflow
//!
//! 1. Task completes → `TaskEvaluation` is created
//! 2. `evaluate_task()` → `EvaluationResult` (suggest: bool, reason, confidence)
//! 3. If `suggest: true` → `generate_skill_proposal()` → `SkillProposal`
//! 4. User reviews → `approve_proposal()` → writes `~/.shannon/skills/user-proposed/`
//! 5. Skill becomes available for future use

pub mod dedup;
pub mod evaluator;
pub mod generator;
pub mod storage;
pub mod types;

// Re-export main types for convenience
pub use types::{
    EvaluationResult, EvaluationScores, ProposalStatus, SkillMetadataDraft, SkillProposal,
    TaskEvaluation, TaskOutcome,
};

use shannon_engine::api::LlmClient;

/// Evaluate whether a completed task is worth extracting as a skill
///
/// **Unstable**: Evaluation criteria may change based on user feedback.
///
/// # Arguments
/// * `client` - LLM client for evaluation
/// * `input` - Task evaluation data (duration, tools used, prompt, outcome)
///
/// # Returns
/// Evaluation result with suggestion flag, reason, and confidence score.
///
/// # Errors
/// Returns error if the LLM call fails or response parsing fails.
///
/// # Example
/// ```rust,no_run
/// # use shannon_engine::api::LlmClient;
/// # use shannon_core::skill_loop::{evaluate_task, types::TaskEvaluation};
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// # let client = LlmClient::from_env();
/// # let evaluation = TaskEvaluation {
/// # duration_secs: 300, tool_call_count: 5, user_prompt: String::new(),
/// # outcome: shannon_core::skill_loop::types::TaskOutcome::Success,
/// # tool_names_used: std::collections::HashSet::new(),
/// # started_at: None, completed_at: None,
/// # };
/// let result = evaluate_task(&client, evaluation).await?;
/// if result.suggest {
///     println!("Worth extracting: {}", result.reason);
/// }
/// # Ok(())
/// # }
/// ```
pub async fn evaluate_task(
    client: &LlmClient,
    input: TaskEvaluation,
) -> Result<EvaluationResult, Box<dyn std::error::Error>> {
    evaluator::evaluate_internal(client, input).await
}

/// Generate a skill proposal from a completed task
///
/// **Unstable**: Output schema may change.
///
/// # Arguments
/// * `client` - LLM client for proposal generation
/// * `input` - Task evaluation data
///
/// # Returns
/// Complete skill proposal with name, description, triggers, and workflow.
///
/// # Errors
/// Returns error if the LLM call fails or TOML parsing fails.
///
/// # Example
/// ```rust,no_run
/// # use shannon_engine::api::LlmClient;
/// # use shannon_core::skill_loop::{generate_skill_proposal, types::TaskEvaluation};
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// # let client = LlmClient::from_env();
/// # let evaluation = TaskEvaluation {
/// # duration_secs: 300, tool_call_count: 5, user_prompt: String::new(),
/// # outcome: shannon_core::skill_loop::types::TaskOutcome::Success,
/// # tool_names_used: std::collections::HashSet::new(),
/// # started_at: None, completed_at: None,
/// # };
/// let proposal = generate_skill_proposal(&client, evaluation).await?;
/// println!("Generated skill: {}", proposal.name);
/// # Ok(())
/// # }
/// ```
pub async fn generate_skill_proposal(
    client: &LlmClient,
    input: TaskEvaluation,
) -> Result<SkillProposal, Box<dyn std::error::Error>> {
    generator::generate_internal(client, input).await
}

// Re-export storage functions for convenience
pub use storage::{approve_proposal, delete_proposal, load_proposals, save_proposal};

// Re-export dedup functions for convenience
pub use dedup::{find_similar_skill, jaccard_similarity};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_reexports() {
        // Verify public API is re-exported
        let _ = TaskOutcome::Success;
        let _ = ProposalStatus::Pending;
    }
}
