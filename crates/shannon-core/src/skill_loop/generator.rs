//! Skill proposal generation from completed tasks
//!
//! This module generates skill proposals by calling an LLM to extract reusable
//! patterns from completed tasks.

use crate::skill_loop::types::{ProposalStatus, SkillMetadataDraft, SkillProposal, TaskEvaluation};
use chrono::Utc;
use shannon_engine::api::LlmClient;
use shannon_engine::api::types::{ContentBlock, Message, MessageContent};
use tracing::{instrument, trace};
use uuid::Uuid;

/// System prompt for the generator LLM
const GENERATOR_SYSTEM_PROMPT: &str = r#"You are a skill generation expert for Shannon AI Assistant.
Your job is to convert a completed task into a reusable skill template.

A Shannon skill has this structure (TOML format):

```toml
name = "Skill Name"
description = "One or two sentences describing when to use this skill"

[triggers]
# Natural language descriptions of when this skill should be suggested
patterns = [
  "when user asks to X",
  "when user needs to Y"
]

[workflow]
# Step-by-step workflow that can be reused with different inputs
steps = """
1. First step description
2. Second step description
3. (variable inputs marked like {variable_name})
"""

[metadata]
# Optional metadata for skill execution
aliases = ["short-name", "another-name"]
argument_hint = "What inputs does this skill expect?"
allowed_tools = ["tool1", "tool2"]  # Leave empty if no restrictions
```

Rules:
1. Extract the **core pattern** from the task, not specific details
2. Replace concrete values with **placeholder variables** like {project_name}, {file_path}
3. Make the workflow **general enough** to reuse but **specific enough** to be useful
4. Include 2-4 trigger patterns covering common variations
5. Keep descriptions concise (1-2 sentences)

Output ONLY valid TOML. No explanations outside the TOML block."#;

/// Build the user prompt for generation
fn build_generator_prompt(input: &TaskEvaluation) -> String {
    let tools_list: Vec<&str> = input.tool_names_used.iter().map(|s| s.as_str()).collect();
    let tools_list = tools_list.join(", ");

    format!(
        r#"Generate a skill template from this completed task:

User Prompt: {user_prompt}
Task Duration: {duration_secs} seconds
Tools Used: {tools_list}
Task Outcome: {outcome:?}

Extract the reusable pattern:
- What was the user trying to achieve? (skill name + description)
- In what situations should this skill be suggested? (trigger patterns)
- What are the reusable steps? (workflow template)
- What variable inputs does it need? (mark as {{variable_name}})

Respond with TOML only."#,
        user_prompt = input.user_prompt,
        duration_secs = input.duration_secs,
        tools_list = tools_list,
        outcome = input.outcome
    )
}

/// Extract TOML block from LLM response (handles ```toml ... ``` fences)
fn extract_toml_block(response: &str) -> Result<String, String> {
    let start = response
        .find("```toml")
        .ok_or("Missing TOML code block marker")?;

    let start_pos = start + 7; // Skip past "```toml"
    let end = response[start_pos..]
        .find("```")
        .ok_or("Unclosed TOML code block")?;

    Ok(response[start_pos..start_pos + end].trim().to_string())
}

/// Convert a name to a URL-safe slug
fn slugify(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

/// Generate a task ID from timestamp
fn generate_task_id() -> String {
    format!("task_{}", Utc::now().format("%Y%m%d_%H%M%S"))
}

/// Generate a skill proposal from a completed task
///
/// This function calls an LLM to analyze the task and generate a structured
/// skill proposal that can be reviewed and approved by the user.
///
/// # Errors
/// Returns `ShannonError::ApiError` if the LLM call fails
/// Returns `ShannonError::ParseError` if the LLM output is not valid TOML
#[instrument(skip_all)]
pub(crate) async fn generate_internal(
    client: &LlmClient,
    input: TaskEvaluation,
) -> Result<SkillProposal, Box<dyn std::error::Error>> {
    trace!("Generating skill proposal from task");

    let system = GENERATOR_SYSTEM_PROMPT.to_string();
    let user_prompt = build_generator_prompt(&input);

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

    trace!(
        "LLM generation response length: {} chars",
        response_text.len()
    );

    // Extract and parse TOML
    let toml_content = extract_toml_block(&response_text)?;
    let parsed: toml::Value =
        toml::from_str(&toml_content).map_err(|e| format!("Failed to parse TOML: {e}"))?;

    // Extract fields from TOML
    let name = parsed["name"]
        .as_str()
        .ok_or("Missing 'name' field in TOML")?
        .to_string();

    let slug = slugify(&name);

    let description = parsed["description"]
        .as_str()
        .ok_or("Missing 'description' field in TOML")?
        .to_string();

    let trigger_patterns = parsed["triggers"]["patterns"]
        .as_array()
        .ok_or("Missing 'triggers.patterns' field in TOML")?
        .iter()
        .filter_map(|v| v.as_str())
        .map(|s| s.to_string())
        .collect();

    let example_workflow = parsed["workflow"]["steps"]
        .as_str()
        .ok_or("Missing 'workflow.steps' field in TOML")?
        .to_string();

    // Extract optional metadata
    let suggested_metadata = parsed.get("metadata").and_then(|meta| {
        Some(SkillMetadataDraft {
            aliases: meta["aliases"]
                .as_array()?
                .iter()
                .filter_map(|v| v.as_str())
                .map(|s| s.to_string())
                .collect(),
            argument_hint: meta["argument_hint"].as_str().map(|s| s.to_string()),
            allowed_tools: meta["allowed_tools"]
                .as_array()?
                .iter()
                .filter_map(|v| v.as_str())
                .map(|s| s.to_string())
                .collect(),
            model: meta["model"].as_str().map(|s| s.to_string()),
            user_invocable: meta["user_invocable"].as_bool().unwrap_or(true),
        })
    });

    Ok(SkillProposal {
        id: Uuid::new_v4(),
        name,
        slug,
        description,
        trigger_patterns,
        example_workflow,
        source_task_id: Some(generate_task_id()),
        created_at: Utc::now(),
        status: ProposalStatus::Pending,
        suggested_metadata,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skill_loop::types::TaskOutcome;

    #[test]
    fn test_slugify() {
        assert_eq!(slugify("Security Code Review"), "security-code-review");
        assert_eq!(slugify("Test & Validate"), "test-validate");
        assert_eq!(slugify("  Multiple   Spaces  "), "multiple-spaces");
    }

    #[test]
    fn test_extract_toml_block() {
        let response = r#"Here is the skill:

```toml
name = "Test Skill"
description = "A test"
```

Hope this helps!"#;

        let toml = extract_toml_block(response).unwrap();
        assert!(toml.contains("name = \"Test Skill\""));
        assert!(toml.contains("description = \"A test\""));
    }

    #[test]
    fn test_extract_toml_block_no_fence() {
        let response = r#"name = "Test"
description = "A description""#;

        let result = extract_toml_block(response);
        assert!(result.is_err());
    }

    #[test]
    fn test_generate_task_id() {
        let id = generate_task_id();
        assert!(id.starts_with("task_"));
        // Check format: task_YYYYMMDD_HHMMSS
        assert!(id.len() == "task_20250623_123456".len());
    }

    #[test]
    fn test_build_generator_prompt() {
        let evaluation = crate::skill_loop::types::TaskEvaluation {
            duration_secs: 300,
            tool_call_count: 5,
            user_prompt: "Review code for security".to_string(),
            outcome: TaskOutcome::Success,
            tool_names_used: vec!["read".into(), "grep".into()].into_iter().collect(),
            started_at: None,
            completed_at: None,
        };

        let prompt = build_generator_prompt(&evaluation);

        assert!(prompt.contains("Review code for security"));
        assert!(prompt.contains("300 seconds"));
        assert!(prompt.contains("Generate a skill template"));
    }
}
