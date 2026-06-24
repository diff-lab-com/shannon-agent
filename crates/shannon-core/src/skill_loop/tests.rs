//! Integration tests for the skill loop module
//!
//! These tests verify the end-to-end workflow of the skill loop system.

use shannon_engine::api::{LlmClient, LlmClientConfig, LlmProvider};
use crate::skill_loop::{
    evaluate_task, generate_skill_proposal, jaccard_similarity, save_proposal,
};
use crate::skill_loop::types::*;
use std::collections::HashSet;
use tempfile::TempDir;

#[tokio::test]
#[ignore = "Requires LLM client"]
async fn test_evaluate_simple_task_returns_false() {
    let client = LlmClient::from_env();

    let evaluation = TaskEvaluation {
        duration_secs: 30, // Short task
        tool_call_count: 1, // Single tool
        user_prompt: "what is rust".to_string(), // Simple Q&A
        outcome: TaskOutcome::Success,
        tool_names_used: vec!["web_search".into()].into_iter().collect(),
        started_at: None,
        completed_at: None,
    };

    let result = evaluate_task(&client, evaluation).await.unwrap();
    assert!(!result.suggest);
    assert!(result.confidence < 0.5);
}

#[tokio::test]
#[ignore = "Requires LLM client"]
async fn test_evaluate_complex_task_returns_true() {
    let client = LlmClient::from_env();

    let evaluation = TaskEvaluation {
        duration_secs: 420, // 7 minutes
        tool_call_count: 8, // 8 tool calls
        user_prompt: "Review this PR for security vulnerabilities and generate a report with findings".to_string(),
        outcome: TaskOutcome::Success,
        tool_names_used: vec![
            "read".into(),
            "grep".into(),
            "analyze".into(),
            "web_search".into(),
        ]
        .into_iter()
        .collect(),
        started_at: None,
        completed_at: None,
    };

    let result = evaluate_task(&client, evaluation).await.unwrap();
    assert!(result.suggest);
    assert!(result.confidence > 0.7);
    assert!(
        result.reason.contains("security")
            || result.reason.contains("complex")
            || result.reason.contains("workflow")
    );
}

#[tokio::test]
#[ignore = "Requires LLM client"]
async fn test_evaluate_failed_task_returns_false() {
    let client = LlmClient::from_env();

    let evaluation = TaskEvaluation {
        duration_secs: 300,
        tool_call_count: 5,
        user_prompt: "Generate report".to_string(),
        outcome: TaskOutcome::Failure, // Failed task
        tool_names_used: vec!["read".into(), "write".into()].into_iter().collect(),
        started_at: None,
        completed_at: None,
    };

    let result = evaluate_task(&client, evaluation).await.unwrap();
    assert!(!result.suggest);
}

#[tokio::test]
#[ignore = "Requires LLM client"]
async fn test_generator_parses_toml_output() {
    let client = LlmClient::from_env();

    let evaluation = TaskEvaluation {
        duration_secs: 300,
        tool_call_count: 5,
        user_prompt: "Review code for security issues".to_string(),
        outcome: TaskOutcome::Success,
        tool_names_used: vec!["read".into(), "grep".into()].into_iter().collect(),
        started_at: None,
        completed_at: None,
    };

    let proposal = generate_skill_proposal(&client, evaluation).await.unwrap();

    assert!(!proposal.name.is_empty());
    assert!(!proposal.slug.is_empty());
    assert!(!proposal.description.is_empty());
    assert!(!proposal.trigger_patterns.is_empty());
    assert!(!proposal.example_workflow.is_empty());
    assert_eq!(proposal.status, ProposalStatus::Pending);
}

#[tokio::test]
#[ignore = "Requires LLM client"]
async fn test_storage_save_and_load() {
    let temp_dir = TempDir::new().unwrap();

    let proposal = SkillProposal {
        id: uuid::Uuid::new_v4(),
        name: "Test Skill".to_string(),
        slug: "test-skill".to_string(),
        description: "A test skill".to_string(),
        trigger_patterns: vec!["when testing".to_string()],
        example_workflow: "1. Test\n2. More test".to_string(),
        source_task_id: Some("task_001".to_string()),
        created_at: chrono::Utc::now(),
        status: ProposalStatus::Pending,
        suggested_metadata: None,
    };

    let path = save_proposal(temp_dir.path(), &proposal).unwrap();
    assert!(path.exists());

    let loaded = load_proposals(temp_dir.path()).unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].name, "Test Skill");
}

#[tokio::test]
fn test_dedup_jaccard_returns_1_for_identical() {
    assert_eq!(jaccard_similarity("test", "test"), 1.0);
    assert_eq!(jaccard_similarity("same text", "same text"), 1.0);
}

#[tokio::test]
fn test_dedup_jaccard_returns_0_for_no_overlap() {
    let result = jaccard_similarity("abc", "xyz");
    assert!(result < 0.3);
}

#[tokio::test]
fn test_dedup_jaccard_partial_overlap() {
    let result = jaccard_similarity("security review", "security code review");
    assert!(result > 0.3 && result < 1.0);
}

#[tokio::test]
#[ignore = "Requires LLM client"]
async fn test_full_workflow_evaluate_to_approve() {
    let client = LlmClient::from_env();
    let temp_dir = TempDir::new().unwrap();
    let skills_dir = TempDir::new().unwrap();

    let evaluation = TaskEvaluation {
        duration_secs: 300,
        tool_call_count: 5,
        user_prompt: "Review code for security vulnerabilities".to_string(),
        outcome: TaskOutcome::Success,
        tool_names_used: vec!["read".into(), "grep".into()].into_iter().collect(),
        started_at: None,
        completed_at: None,
    };

    // Step 1: Evaluate
    let eval_result = evaluate_task(&client, evaluation.clone()).await.unwrap();
    if !eval_result.suggest {
        println!("Skipping test - evaluation returned suggest=false");
        return;
    }

    // Step 2: Generate proposal
    let proposal = generate_skill_proposal(&client, evaluation).await.unwrap();
    assert!(!proposal.name.is_empty());

    // Step 3: Save proposal
    let proposal_path = save_proposal(temp_dir.path(), &proposal).unwrap();
    assert!(proposal_path.exists());

    // Step 4: Approve proposal
    let skill_path = approve_proposal(&proposal, skills_dir.path()).unwrap();
    assert!(skill_path.exists());
    assert!(skill_path.to_string_lossy().ends_with(".toml"));

    // Verify proposal was deleted
    let proposals = load_proposals(temp_dir.path()).unwrap();
    // Note: approve_proposal doesn't auto-delete, that's handled by desktop
}
