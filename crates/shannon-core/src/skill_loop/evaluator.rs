//! Task evaluation logic for skill extraction
//!
//! This module evaluates whether a completed task is worth extracting as a reusable skill.
//! It uses an LLM to analyze task characteristics (duration, complexity, clarity, success).

use crate::skill_loop::types::{EvaluationResult, EvaluationScores, TaskEvaluation};
use serde_json::Value;
use shannon_engine::api::LlmClient;
use shannon_engine::api::types::{ContentBlock, Message, MessageContent};
use tracing::{instrument, trace};

/// System prompt for the evaluation LLM
const EVALUATOR_SYSTEM_PROMPT: &str = r#"You are a task evaluation expert for Shannon AI Assistant.
Your job is to determine whether a completed task is worth extracting as a reusable skill.

A "skill" in Shannon is a reusable prompt template that captures:
- A specific type of task (e.g., "code review for security vulnerabilities")
- Trigger conditions (when to suggest this skill to users)
- A workflow template (steps that can be reused with different inputs)

Evaluation criteria (weighted equally):
1. **Duration**: Long-running tasks (>5 minutes) indicate complex workflows worth automating
2. **Complexity**: Tasks using 3+ different tools suggest multi-step workflows
3. **Goal clarity**: Tasks with clear verbs (generate/analyze/review) and specific deliverables are easier to templatize
4. **Success status**: Only successful or partially successful tasks should become skills

Output a JSON object with this schema:
{
  "suggest": boolean,  // true if worth extracting
  "reason": string,    // human-readable explanation (1-2 sentences)
  "confidence": float, // 0.0-1.0 score
  "scores": {          // detailed breakdown (optional)
    "duration_score": float,      // 0-100
    "complexity_score": float,     // 0-100
    "clarity_score": float,        // 0-100
    "success_score": float         // 0-100
  }
}

Rules:
- Return suggest=false for: simple Q&A, single-round conversations, failed tasks
- Return suggest=true for: multi-step workflows, repeated patterns, clear deliverables
- Always explain the reason in plain language (no jargon)"#;

/// Build the user prompt for evaluation
fn build_evaluation_prompt(input: &TaskEvaluation) -> String {
    let tools_list: Vec<&str> = input.tool_names_used.iter().map(|s| s.as_str()).collect();
    let tools_list = tools_list.join(", ");
    let duration_label = if input.duration_secs > 300 {
        "complex"
    } else {
        "simple"
    };

    format!(
        r#"Evaluate this completed task for skill extraction potential:

Task Duration: {duration_secs} seconds ({duration_label})
Tools Used: {num_tools} different tools ({tools_list})
Task Outcome: {outcome:?}
User Prompt: {user_prompt}

Consider:
1. Did this task take long enough to suggest complexity? (>5 min = strong signal)
2. Did it use multiple tools suggesting a multi-step workflow?
3. Does the prompt have clear intent (specific verbs + deliverables)?
4. Did the task succeed (or partially succeed)?

Respond with JSON only."#,
        duration_secs = input.duration_secs,
        duration_label = duration_label,
        num_tools = input.tool_names_used.len(),
        tools_list = tools_list,
        outcome = input.outcome,
        user_prompt = input.user_prompt
    )
}

/// Evaluate whether a completed task is worth extracting as a skill
///
/// This function calls an LLM to analyze the task characteristics and determine
/// if it represents a reusable pattern worth capturing.
///
/// # Errors
/// Returns `ShannonError::ApiError` if the LLM call fails
/// Returns `ShannonError::ParseError` if the LLM output is not valid JSON
#[instrument(skip_all)]
pub(crate) async fn evaluate_internal(
    client: &LlmClient,
    input: TaskEvaluation,
) -> Result<EvaluationResult, Box<dyn std::error::Error>> {
    trace!("Evaluating task for skill extraction");

    let system = EVALUATOR_SYSTEM_PROMPT.to_string();
    let user_prompt = build_evaluation_prompt(&input);

    let message = Message {
        role: "user".to_string(),
        content: MessageContent::Text(user_prompt),
    };

    let response_blocks = client
        .send_message(vec![message], None, Some(system))
        .await?;

    // Extract text from response blocks
    let response_text: String = response_blocks
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(text.clone()),
            _ => None,
        })
        .collect();

    trace!("LLM evaluation response: {}", response_text);

    // Parse JSON response
    let parsed: Value = serde_json::from_str(&response_text)
        .map_err(|e| format!("Invalid JSON from evaluation LLM: {e}"))?;

    let suggest = parsed["suggest"]
        .as_bool()
        .ok_or("Missing 'suggest' field in LLM response")?;

    let reason = parsed["reason"]
        .as_str()
        .unwrap_or("Unknown reason")
        .to_string();

    let confidence = parsed["confidence"].as_f64().unwrap_or(0.0) as f32;

    let scores = parsed
        .get("scores")
        .and_then(|s| serde_json::from_value::<EvaluationScores>(s.clone()).ok());

    Ok(EvaluationResult {
        suggest,
        reason,
        confidence,
        scores,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skill_loop::types::TaskOutcome;

    #[test]
    fn test_build_evaluation_prompt() {
        let evaluation = TaskEvaluation {
            duration_secs: 420,
            tool_call_count: 8,
            user_prompt: "Review this PR for security issues".to_string(),
            outcome: TaskOutcome::Success,
            tool_names_used: vec!["read".into(), "grep".into()].into_iter().collect(),
            started_at: None,
            completed_at: None,
        };

        let prompt = build_evaluation_prompt(&evaluation);

        assert!(prompt.contains("420 seconds"));
        assert!(prompt.contains("2 different tools"));
        assert!(prompt.contains("Review this PR"));
        assert!(prompt.contains("Success"));
    }

    #[test]
    fn test_evaluation_result_structure() {
        let result = EvaluationResult {
            suggest: true,
            reason: "Test reason".to_string(),
            confidence: 0.75,
            scores: None,
        };

        assert!(result.suggest);
        assert_eq!(result.reason, "Test reason");
        assert!((result.confidence - 0.75).abs() < 0.01);
        assert!(result.scores.is_none());
    }
}
