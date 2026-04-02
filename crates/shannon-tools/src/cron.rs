//! Cron scheduling tools
//!
//! Provides implementations for:
//! - CronCreate: Schedule a recurring or one-shot prompt
//! - CronDelete: Cancel a scheduled cron job
//! - CronList: List active cron jobs
//!
//! Enables time-based task scheduling with persistence options.

use crate::{Tool, ToolError, ToolResult, ToolOutput};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

/// Cron job configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronJob {
    /// Unique job ID
    pub id: String,

    /// Cron expression (5 fields: M H DoM Mon DoW)
    pub cron: String,

    /// Prompt to enqueue when job fires
    pub prompt: String,

    /// Whether job is recurring (true) or one-shot (false)
    pub recurring: bool,

    /// Whether job persists across sessions
    pub durable: bool,

    /// Agent ID that created this job (for teammates)
    pub agent_id: Option<String>,

    /// When this job was created
    pub created_at: String,

    /// Next scheduled run time
    pub next_run: Option<String>,
}

/// Input for creating a cron job
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CronCreateInput {
    /// Standard 5-field cron expression in local time
    pub cron: String,

    /// The prompt to enqueue at each fire time
    pub prompt: String,

    /// true = recurring, false = one-shot (default: true)
    pub recurring: Option<bool>,

    /// true = persist to disk, false = in-memory only (default: false)
    pub durable: Option<bool>,
}

/// Output from creating a cron job
#[derive(Debug, Serialize)]
pub struct CronCreateOutput {
    /// Job ID
    pub id: String,

    /// Human-readable schedule description
    pub human_schedule: String,

    /// Whether job is recurring
    pub recurring: bool,

    /// Whether job persists to disk
    pub durable: Option<bool>,
}

/// Input for deleting a cron job
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CronDeleteInput {
    /// Job ID returned by CronCreate
    pub id: String,
}

/// Output from deleting a cron job
#[derive(Debug, Serialize)]
pub struct CronDeleteOutput {
    /// Job ID that was deleted
    pub id: String,
}

/// Input for listing cron jobs
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CronListInput {}

/// Output from listing cron jobs
#[derive(Debug, Serialize)]
pub struct CronListOutput {
    /// List of active jobs
    pub jobs: Vec<CronJobInfo>,
}

/// Information about a cron job
#[derive(Debug, Serialize)]
pub struct CronJobInfo {
    /// Job ID
    pub id: String,

    /// Cron expression
    pub cron: String,

    /// Human-readable schedule
    pub human_schedule: String,

    /// Prompt (truncated)
    pub prompt: String,

    /// Whether job is recurring
    pub recurring: Option<bool>,

    /// Whether job persists to disk
    pub durable: Option<bool>,
}

/// Cron job store (shared state)
type CronStore = Arc<RwLock<HashMap<String, CronJob>>>;

fn get_cron_store() -> CronStore {
    Arc::new(RwLock::new(HashMap::new()))
}

/// Convert cron expression to human-readable format
fn cron_to_human(cron: &str) -> String {
    let parts: Vec<&str> = cron.split_whitespace().collect();
    if parts.len() != 5 {
        return format!("Invalid cron: {}", cron);
    }

    let (minute, hour, day_of_month, month, day_of_week) = (
        parts.get(0).unwrap_or(&"*"),
        parts.get(1).unwrap_or(&"*"),
        parts.get(2).unwrap_or(&"*"),
        parts.get(3).unwrap_or(&"*"),
        parts.get(4).unwrap_or(&"*"),
    );

    format!(
        "At {}:{} on {} of {}, {}",
        hour, minute, day_of_month, month, day_of_week
    )
}

/// Validate cron expression
fn validate_cron(cron: &str) -> Result<(), ToolError> {
    let parts: Vec<&str> = cron.split_whitespace().collect();
    if parts.len() != 5 {
        return Err(ToolError::InvalidInput(
            "Invalid cron expression. Expected 5 fields: M H DoM Mon DoW".to_string(),
        ));
    }

    // Validate each field
    let ranges = [
        (0, 59),   // minute
        (0, 23),   // hour
        (1, 31),   // day of month
        (1, 12),   // month
        (0, 6),    // day of week (0 = Sunday)
    ];

    for (i, part) in parts.iter().enumerate() {
        if *part == "*" {
            continue;
        }

        // Handle */n patterns
        if let Some(rest) = part.strip_prefix("*/") {
            if let Ok(n) = rest.parse::<u32>() {
                if n == 0 || n > (ranges[i].1 - ranges[i].0) {
                    return Err(ToolError::InvalidInput(format!(
                        "Invalid {} value in cron expression",
                        ["minute", "hour", "day", "month", "weekday"][i]
                    )));
                }
                continue;
            }
        }

        // Handle comma-separated values
        for value in part.split(',') {
            if let Ok(n) = value.parse::<u32>() {
                if n < ranges[i].0 || n > ranges[i].1 {
                    return Err(ToolError::InvalidInput(format!(
                        "Invalid {} value in cron expression",
                        ["minute", "hour", "day", "month", "weekday"][i]
                    )));
                }
            }
        }
    }

    Ok(())
}

/// Cron management tool
pub struct CronTool {
    description: String,
    store: CronStore,
    max_jobs: usize,
}

impl CronTool {
    pub fn new() -> Self {
        Self {
            description: "Schedule recurring or one-shot prompts for time-based task execution".to_string(),
            store: get_cron_store(),
            max_jobs: 50,
        }
    }

    /// Create a new cron job
    async fn create_cron(&self, input: CronCreateInput) -> Result<CronCreateOutput, ToolError> {
        // Validate cron expression
        validate_cron(&input.cron)?;

        // Check max jobs limit
        let job_count = {
            let store = self.store.read().map_err(|e| {
                ToolError::ExecutionFailed(format!("Failed to acquire store lock: {}", e))
            })?;
            store.len()
        };

        if job_count >= self.max_jobs {
            return Err(ToolError::ExecutionFailed(format!(
                "Too many scheduled jobs (max {})",
                self.max_jobs
            )));
        }

        let id = Uuid::new_v4().to_string();
        let recurring = input.recurring.unwrap_or(true);
        let durable = input.durable.unwrap_or(false);

        let job = CronJob {
            id: id.clone(),
            cron: input.cron.clone(),
            prompt: input.prompt.clone(),
            recurring,
            durable,
            agent_id: None,
            created_at: chrono::Utc::now().to_rfc3339(),
            next_run: None, // Would calculate next run time
        };

        {
            let mut store = self.store.write().map_err(|e| {
                ToolError::ExecutionFailed(format!("Failed to acquire store lock: {}", e))
            })?;
            store.insert(id.clone(), job);
        }

        let human_schedule = cron_to_human(&input.cron);

        Ok(CronCreateOutput {
            id,
            human_schedule,
            recurring,
            durable: Some(durable),
        })
    }

    /// Delete a cron job
    async fn delete_cron(&self, input: CronDeleteInput) -> Result<CronDeleteOutput, ToolError> {
        let exists = {
            let store = self.store.read().map_err(|e| {
                ToolError::ExecutionFailed(format!("Failed to acquire store lock: {}", e))
            })?;
            store.contains_key(&input.id)
        };

        if !exists {
            return Err(ToolError::InvalidInput(format!(
                "No scheduled job with id '{}'",
                input.id
            )));
        }

        {
            let mut store = self.store.write().map_err(|e| {
                ToolError::ExecutionFailed(format!("Failed to acquire store lock: {}", e))
            })?;
            store.remove(&input.id);
        }

        Ok(CronDeleteOutput { id: input.id })
    }

    /// List all cron jobs
    async fn list_cron(&self, _input: CronListInput) -> Result<CronListOutput, ToolError> {
        let store = self.store.read().map_err(|e| {
            ToolError::ExecutionFailed(format!("Failed to acquire store lock: {}", e))
        })?;

        let jobs: Vec<CronJobInfo> = store
            .values()
            .map(|job| CronJobInfo {
                id: job.id.clone(),
                cron: job.cron.clone(),
                human_schedule: cron_to_human(&job.cron),
                prompt: job.prompt.clone(),
                recurring: Some(job.recurring),
                durable: Some(job.durable),
            })
            .collect();

        Ok(CronListOutput { jobs })
    }
}

#[async_trait]
impl Tool for CronTool {
    async fn execute(&self, input: serde_json::Value) -> ToolResult<ToolOutput> {
        // Parse operation type from input
        let operation = input
            .get("operation")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput("Missing operation field".to_string()))?;

        match operation {
            "Create" => {
                let create_input: CronCreateInput = serde_json::from_value(input)
                    .map_err(|e| ToolError::InvalidInput(format!("Invalid create cron input: {}", e)))?;
                let output = self.create_cron(create_input).await?;
                Ok(ToolOutput {
                    content: format!("Created cron job with ID: {}", output.id),
                    is_error: false,
                    metadata: {
                        let mut map = HashMap::new();
                        map.insert("id".to_string(), json!(output.id));
                        map.insert("human_schedule".to_string(), json!(output.human_schedule));
                        map.insert("recurring".to_string(), json!(output.recurring));
                        if let Some(durable) = output.durable {
                            map.insert("durable".to_string(), json!(durable));
                        }
                        map
                    },
                })
            }
            "Delete" => {
                let delete_input: CronDeleteInput = serde_json::from_value(input)
                    .map_err(|e| ToolError::InvalidInput(format!("Invalid delete cron input: {}", e)))?;
                let output = self.delete_cron(delete_input).await?;
                Ok(ToolOutput {
                    content: format!("Deleted cron job: {}", output.id),
                    is_error: false,
                    metadata: {
                        let mut map = HashMap::new();
                        map.insert("id".to_string(), json!(output.id));
                        map
                    },
                })
            }
            "List" => {
                let list_input: CronListInput = serde_json::from_value(input)
                    .map_err(|e| ToolError::InvalidInput(format!("Invalid list cron input: {}", e)))?;
                let output = self.list_cron(list_input).await?;
                Ok(ToolOutput {
                    content: format!("Found {} cron jobs", output.jobs.len()),
                    is_error: false,
                    metadata: {
                        let mut map = HashMap::new();
                        map.insert("jobs".to_string(), json!(output.jobs));
                        map
                    },
                })
            }
            _ => Err(ToolError::InvalidInput(format!(
                "Unknown operation: {}",
                operation
            ))),
        }
    }

    fn name(&self) -> &str {
        "Cron"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "operation": {
                    "type": "string",
                    "description": "Operation type",
                    "enum": ["Create", "Delete", "List"]
                },
                "cron": {
                    "type": "string",
                    "description": "Cron expression (5 fields: M H DoM Mon DoW)"
                },
                "prompt": {
                    "type": "string",
                    "description": "Prompt to enqueue"
                },
                "recurring": {
                    "type": "boolean",
                    "description": "Recurring job"
                },
                "durable": {
                    "type": "boolean",
                    "description": "Persist to disk"
                },
                "id": {
                    "type": "string",
                    "description": "Job ID"
                }
            },
            "required": ["operation"]
        })
    }
}
