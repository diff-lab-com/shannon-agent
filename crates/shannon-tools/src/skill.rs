//! Skill system tools
//!
//! Provides implementations for:
//! - Skill: Invoke user-callable slash-command skills
//!
//! Supports both inline and forked skill execution contexts.

use crate::{Tool, ToolError, ToolResult, ToolOutput};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Skill command type
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SkillContext {
    /// Execute inline in current context
    Inline,
    /// Execute in forked sub-agent
    Fork,
}

/// Skill command definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillCommand {
    /// Command name
    pub name: String,

    /// Description
    pub description: String,

    /// Command type
    #[serde(rename = "type")]
    pub command_type: String,

    /// Execution context
    pub context: Option<SkillContext>,

    /// Allowed tools for this skill
    pub allowed_tools: Option<Vec<String>>,

    /// Model override
    pub model: Option<String>,

    /// Skill content/prompt
    pub content: Option<String>,

    /// Source (bundled, plugin, local)
    pub source: Option<String>,

    /// Effort level
    pub effort: Option<String>,
}

/// Input for invoking a skill
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SkillInvokeInput {
    /// The skill name (e.g., "commit", "review-pr")
    pub skill: String,

    /// Optional arguments for the skill
    pub args: Option<String>,
}

/// Output from skill execution
#[derive(Debug, Serialize)]
pub struct SkillInvokeOutput {
    /// Whether the skill executed successfully
    pub success: bool,

    /// The name of the skill
    pub command_name: String,

    /// Tools allowed by this skill
    pub allowed_tools: Option<Vec<String>>,

    /// Model override if specified
    pub model: Option<String>,

    /// Execution status (inline or forked)
    pub status: Option<String>,

    /// Result from forked skill execution
    pub result: Option<String>,

    /// Agent ID for forked skills
    pub agent_id: Option<String>,
}

/// Skill registry (shared state)
type SkillRegistry = Arc<RwLock<HashMap<String, SkillCommand>>>;

fn get_skill_registry() -> SkillRegistry {
    let registry = Arc::new(RwLock::new(HashMap::new()));

    // Register built-in skills
    {
        let mut reg = registry.write().unwrap_or_else(|e| e.into_inner());
        reg.insert("commit".to_string(), SkillCommand {
            name: "commit".to_string(),
            description: "Create a git commit with staged changes".to_string(),
            command_type: "prompt".to_string(),
            context: Some(SkillContext::Fork),
            allowed_tools: Some(vec!["Read".to_string(), "Bash".to_string()]),
            model: None,
            content: Some("Please create a git commit with the currently staged changes. Follow these guidelines:\n\n1. Review the staged changes\n2. Write a clear commit message following conventional commits format\n3. Create the commit\n\nShow me the diff first, then the commit message you'll use, then execute the commit.".to_string()),
            source: Some("bundled".to_string()),
            effort: None,
        });

        reg.insert("review-pr".to_string(), SkillCommand {
            name: "review-pr".to_string(),
            description: "Review a pull request and provide feedback".to_string(),
            command_type: "prompt".to_string(),
            context: Some(SkillContext::Fork),
            allowed_tools: Some(vec!["Read".to_string(), "Bash".to_string(), "WebFetch".to_string()]),
            model: None,
            content: Some("Please review this pull request:\n\n1. Examine the PR title and description\n2. Review the code changes\n3. Check for issues, bugs, or improvements\n4. Provide constructive feedback\n\nProvide a summary of your review.".to_string()),
            source: Some("bundled".to_string()),
            effort: None,
        });
    }

    registry
}

/// Skill tool
pub struct SkillTool {
    description: String,
    registry: SkillRegistry,
}

impl Default for SkillTool {
    fn default() -> Self {
        Self::new()
    }
}

impl SkillTool {
    pub fn new() -> Self {
        Self {
            description: "Invoke user-callable slash-command skills for specialized workflows".to_string(),
            registry: get_skill_registry(),
        }
    }

    /// Find a skill by name
    fn find_skill(&self, name: &str) -> Option<SkillCommand> {
        let registry = self.registry.read().unwrap_or_else(|e| e.into_inner());
        // Normalize name (remove leading slash)
        let normalized_name = name.strip_prefix('/').unwrap_or(name);
        registry.get(normalized_name).cloned()
    }

    /// Execute skill inline
    fn execute_inline(&self, command: &SkillCommand, _args: Option<&str>) -> SkillInvokeOutput {
        SkillInvokeOutput {
            success: true,
            command_name: command.name.clone(),
            allowed_tools: command.allowed_tools.clone(),
            model: command.model.clone(),
            status: Some("inline".to_string()),
            result: None,
            agent_id: None,
        }
    }

    /// Execute skill in forked sub-agent
    fn execute_forked(&self, command: &SkillCommand, args: Option<&str>) -> SkillInvokeOutput {
        use uuid::Uuid;

        let agent_id = Uuid::new_v4().to_string();

        // In a real implementation, this would spawn a sub-agent
        // For now, return a mock result
        let result = format!(
            "Executed skill '{}' with args: '{}'",
            command.name,
            args.unwrap_or("")
        );

        SkillInvokeOutput {
            success: true,
            command_name: command.name.clone(),
            allowed_tools: None,
            model: command.model.clone(),
            status: Some("forked".to_string()),
            result: Some(result),
            agent_id: Some(agent_id),
        }
    }

    /// Execute skill invocation
    async fn execute_invoke(&self, input: SkillInvokeInput) -> Result<SkillInvokeOutput, ToolError> {
        let normalized_name = input.skill.trim().strip_prefix('/').unwrap_or(&input.skill);

        let command = self.find_skill(normalized_name).ok_or_else(|| {
            ToolError::InvalidInput(format!("Unknown skill: {normalized_name}"))
        })?;

        // Check if command is a prompt-based skill
        if command.command_type != "prompt" {
            return Ok(SkillInvokeOutput {
                success: false,
                command_name: input.skill.clone(),
                allowed_tools: None,
                model: None,
                status: None,
                result: Some(format!("Skill {normalized_name} is not a prompt-based skill")),
                agent_id: None,
            });
        }

        // Execute based on context
        let context = command.context.as_ref().unwrap_or(&SkillContext::Inline);

        match context {
            SkillContext::Inline => Ok(self.execute_inline(&command, input.args.as_deref())),
            SkillContext::Fork => Ok(self.execute_forked(&command, input.args.as_deref())),
        }
    }
}

#[async_trait]
impl Tool for SkillTool {
    async fn execute(&self, input: serde_json::Value) -> ToolResult<ToolOutput> {
        let invoke_input: SkillInvokeInput = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("Invalid skill invoke input: {e}")))?;
        let output = self.execute_invoke(invoke_input).await?;

        let content = if output.success {
            format!("Skill '{}' executed successfully", output.command_name)
        } else {
            output.result.clone().unwrap_or_else(|| format!("Skill '{}' execution failed", output.command_name))
        };

        Ok(ToolOutput {
            content,
            is_error: !output.success,
            metadata: {
                let mut map = HashMap::new();
                map.insert("command_name".to_string(), json!(output.command_name));
                map.insert("success".to_string(), json!(output.success));
                if let Some(allowed_tools) = output.allowed_tools {
                    map.insert("allowed_tools".to_string(), json!(allowed_tools));
                }
                if let Some(model) = output.model {
                    map.insert("model".to_string(), json!(model));
                }
                if let Some(status) = output.status {
                    map.insert("status".to_string(), json!(status));
                }
                if let Some(result) = output.result {
                    map.insert("result".to_string(), json!(result));
                }
                if let Some(agent_id) = output.agent_id {
                    map.insert("agent_id".to_string(), json!(agent_id));
                }
                map
            },
        })
    }

    fn name(&self) -> &str {
        "Skill"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "skill": {
                    "type": "string",
                    "description": "Skill name (e.g., 'commit', 'review-pr')"
                },
                "args": {
                    "type": "string",
                    "description": "Optional arguments for the skill"
                }
            },
            "required": ["skill"]
        })
    }
    fn is_read_only(&self) -> bool {        true    }
}
