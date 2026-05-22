//! ScheduleWakeup tool for /loop dynamic pacing.
//!
//! Provides a simpler scheduling interface than CronTool:
//! - `delaySeconds`: how long to wait (clamped to 60–3600s)
//! - `prompt`: what to enqueue when the timer fires
//!
//! Designed for the /loop dynamic mode where the LLM self-paces iterations.
//! Calling without a prompt (or omitting the call entirely) ends the loop.

use crate::{Tool, ToolError, ToolResult, ToolOutput};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

/// A pending wakeup request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WakeupRequest {
    /// Unique ID
    pub id: String,
    /// Seconds to wait before firing
    pub delay_seconds: u64,
    /// Prompt to enqueue when the timer fires.
    /// The sentinel `<<autonomous-loop-dynamic>>` signals an autonomous loop tick.
    pub prompt: String,
    /// When this request was created
    pub created_at: String,
    /// When this request should fire
    pub fire_at: String,
    /// Whether this is an autonomous loop tick
    pub is_autonomous: bool,
}

/// Input for scheduling a wakeup.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ScheduleWakeupInput {
    /// Seconds from now to wait. Clamped to [60, 3600].
    pub delay_seconds: u64,
    /// Prompt to enqueue on wake-up. Pass `<<autonomous-loop-dynamic>>` for
    /// autonomous /loop ticks; omitting or empty ends the loop.
    pub prompt: String,
}

/// Input for listing pending wakeups.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ListWakeupsInput {}

/// Input for cancelling a wakeup.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CancelWakeupInput {
    /// ID returned by ScheduleWakeup
    pub id: String,
}

/// Wakeup store (shared state).
type WakeupStore = Arc<RwLock<HashMap<String, WakeupRequest>>>;

/// Sentinel for autonomous loop ticks.
pub const AUTONOMOUS_LOOP_SENTINEL: &str = "<<autonomous-loop-dynamic>>";

/// Min / max delay bounds.
const MIN_DELAY_SECS: u64 = 60;
const MAX_DELAY_SECS: u64 = 3600;

/// Approximate prompt cache TTL (5 minutes, matching Anthropic).
const _CACHE_TTL_SECS: u64 = 300;

/// Return a human-readable cache hint for the given delay.
fn cache_hint(delay_secs: u64) -> &'static str {
    if delay_secs <= 270 {
        "cache warm (within TTL window)"
    } else if delay_secs <= 300 {
        "cache boundary — consider 270s to stay cached"
    } else if delay_secs <= 600 {
        "cache miss — consider 270s to stay cached or 600s+ to amortize"
    } else {
        "cache miss — pay cold read cost"
    }
}

// ---------------------------------------------------------------------------
// ScheduleWakeup tool
// ---------------------------------------------------------------------------

/// Tool for scheduling delayed wake-ups in /loop dynamic mode.
pub struct ScheduleWakeupTool {
    store: WakeupStore,
}

impl ScheduleWakeupTool {
    pub fn new() -> Self {
        Self {
            store: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create with a shared store (for testing / integration).
    pub fn with_store(store: WakeupStore) -> Self {
        Self { store }
    }

    /// Get a reference to the underlying store.
    pub fn store(&self) -> &WakeupStore {
        &self.store
    }

    /// Clamp delay to allowed range.
    fn clamp_delay(secs: u64) -> u64 {
        secs.clamp(MIN_DELAY_SECS, MAX_DELAY_SECS)
    }

    /// Schedule a new wakeup.
    async fn schedule(&self, input: ScheduleWakeupInput) -> Result<WakeupRequest, ToolError> {
        let delay = Self::clamp_delay(input.delay_seconds);
        let jitter = shannon_core::scheduled_routines::apply_jitter(delay, 900);
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let fire_at = now + chrono::Duration::seconds((delay + jitter) as i64);

        let is_autonomous = input.prompt == AUTONOMOUS_LOOP_SENTINEL;

        let req = WakeupRequest {
            id: id.clone(),
            delay_seconds: delay + jitter,
            prompt: input.prompt,
            created_at: now.to_rfc3339(),
            fire_at: fire_at.to_rfc3339(),
            is_autonomous,
        };

        {
            let mut store = self.store.write().map_err(|e| {
                ToolError::ExecutionFailed(format!("Failed to acquire store lock: {e}"))
            })?;
            store.insert(id, req.clone());
        }

        Ok(req)
    }

    /// List all pending wakeups.
    async fn list(&self) -> Result<Vec<WakeupRequest>, ToolError> {
        let store = self.store.read().map_err(|e| {
            ToolError::ExecutionFailed(format!("Failed to acquire store lock: {e}"))
        })?;
        Ok(store.values().cloned().collect())
    }

    /// Cancel a wakeup by ID.
    async fn cancel(&self, id: &str) -> Result<(), ToolError> {
        let mut store = self.store.write().map_err(|e| {
            ToolError::ExecutionFailed(format!("Failed to acquire store lock: {e}"))
        })?;
        store
            .remove(id)
            .ok_or_else(|| ToolError::NotFound(format!("Wakeup {id} not found")))?;
        Ok(())
    }

    /// Check if any wakeup is due and return it, removing one-shot entries.
    pub fn poll_due(&self) -> Vec<WakeupRequest> {
        let now = Utc::now();
        let mut due = Vec::new();
        if let Ok(mut store) = self.store.write() {
            let due_ids: Vec<String> = store
                .iter()
                .filter(|(_, req)| {
                    if let Ok(fire_at) = DateTime::parse_from_rfc3339(&req.fire_at) {
                        fire_at <= now
                    } else {
                        false
                    }
                })
                .map(|(id, _)| id.clone())
                .collect();

            for id in due_ids {
                if let Some(req) = store.remove(&id) {
                    due.push(req);
                }
            }
        }
        due
    }
}

impl Default for ScheduleWakeupTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for ScheduleWakeupTool {
    async fn execute(&self, input: serde_json::Value) -> ToolResult<ToolOutput> {
        let operation = input
            .get("operation")
            .and_then(|v| v.as_str())
            .unwrap_or("Schedule");

        match operation {
            "Schedule" => {
                let sched_input: ScheduleWakeupInput = serde_json::from_value(input)
                    .map_err(|e| {
                        ToolError::InvalidInput(format!("Invalid ScheduleWakeup input: {e}"))
                    })?;

                let req = self.schedule(sched_input).await?;
                let hint = cache_hint(req.delay_seconds);

                Ok(ToolOutput {
                    content: format!(
                        "Scheduled wakeup in {}s (fires at {}). Cache: {}",
                        req.delay_seconds, req.fire_at, hint
                    ),
                    is_error: false,
                    metadata: {
                        let mut map = HashMap::new();
                        map.insert("id".to_string(), json!(req.id));
                        map.insert("delay_seconds".to_string(), json!(req.delay_seconds));
                        map.insert("fire_at".to_string(), json!(req.fire_at));
                        map.insert("is_autonomous".to_string(), json!(req.is_autonomous));
                        map.insert("cache_ttl_hint".to_string(), json!(hint));
                        map
                    },
                })
            }
            "List" => {
                let wakeups = self.list().await?;
                Ok(ToolOutput {
                    content: format!("{} pending wakeup(s)", wakeups.len()),
                    is_error: false,
                    metadata: {
                        let mut map = HashMap::new();
                        map.insert("wakeups".to_string(), json!(wakeups));
                        map
                    },
                })
            }
            "Cancel" => {
                let cancel_input: CancelWakeupInput = serde_json::from_value(input)
                    .map_err(|e| {
                        ToolError::InvalidInput(format!("Invalid CancelWakeup input: {e}"))
                    })?;
                self.cancel(&cancel_input.id).await?;
                Ok(ToolOutput {
                    content: format!("Cancelled wakeup {}", cancel_input.id),
                    is_error: false,
                    metadata: HashMap::new(),
                })
            }
            _ => Err(ToolError::InvalidInput(format!(
                "Unknown operation: {operation}"
            ))),
        }
    }

    fn name(&self) -> &str {
        "ScheduleWakeup"
    }

    fn description(&self) -> &str {
        "Schedule a prompt to be enqueued after a delay (for /loop dynamic pacing). \
         Call with delaySeconds and prompt to schedule; omit or call without arguments to end the loop. \
         Cache optimization: delays <= 270s keep the prompt cache warm; 270-300s risks cache expiry; \
         > 300s pays cold read cost. Prefer 270s for active work, 600s+ for idle waits."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "operation": {
                    "type": "string",
                    "description": "Operation type",
                    "enum": ["Schedule", "List", "Cancel"]
                },
                "delay_seconds": {
                    "type": "integer",
                    "description": "Seconds from now to wait. Clamped to [60, 3600].",
                    "minimum": 60,
                    "maximum": 3600
                },
                "prompt": {
                    "type": "string",
                    "description": "Prompt to enqueue on wake-up. Use '<<autonomous-loop-dynamic>>' for autonomous loop ticks."
                },
                "id": {
                    "type": "string",
                    "description": "Wakeup ID (for Cancel)"
                }
            },
            "required": ["delay_seconds", "prompt"]
        })
    }

    fn category(&self) -> &str {
        "scheduling"
    }

    fn is_read_only(&self) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clamp_delay_minimum() {
        assert_eq!(ScheduleWakeupTool::clamp_delay(0), 60);
        assert_eq!(ScheduleWakeupTool::clamp_delay(30), 60);
    }

    #[test]
    fn test_clamp_delay_maximum() {
        assert_eq!(ScheduleWakeupTool::clamp_delay(5000), 3600);
    }

    #[test]
    fn test_clamp_delay_in_range() {
        assert_eq!(ScheduleWakeupTool::clamp_delay(120), 120);
        assert_eq!(ScheduleWakeupTool::clamp_delay(270), 270);
    }

    #[tokio::test]
    async fn test_schedule_wakeup() {
        let tool = ScheduleWakeupTool::new();

        let result = tool
            .execute(json!({
                "operation": "Schedule",
                "delay_seconds": 120,
                "prompt": "Continue the task"
            }))
            .await
            .unwrap();

        assert!(!result.is_error);
        assert!(result.content.contains("Cache:"));
        let delay = result.metadata.get("delay_seconds").unwrap().as_u64().unwrap();
        assert!((120..=132).contains(&delay), "delay {delay} should be ~120 + jitter");
        assert_eq!(
            result.metadata.get("is_autonomous").unwrap(),
            &json!(false)
        );
        assert!(result.metadata.get("id").is_some());
        assert!(result.metadata.get("fire_at").is_some());
        assert!(result.metadata.get("cache_ttl_hint").is_some());
    }

    #[tokio::test]
    async fn test_schedule_clamps_delay() {
        let tool = ScheduleWakeupTool::new();

        // Below minimum → clamped to 60 + jitter
        let result = tool
            .execute(json!({
                "operation": "Schedule",
                "delay_seconds": 10,
                "prompt": "test"
            }))
            .await
            .unwrap();
        let delay = result.metadata.get("delay_seconds").unwrap().as_u64().unwrap();
        assert!((60..=66).contains(&delay), "delay {delay} should be ~60 + jitter");

        // Above maximum → clamped to 3600 + jitter
        let result = tool
            .execute(json!({
                "operation": "Schedule",
                "delay_seconds": 9999,
                "prompt": "test"
            }))
            .await
            .unwrap();
        let delay = result.metadata.get("delay_seconds").unwrap().as_u64().unwrap();
        assert!((3600..=3600 + 360).contains(&delay), "delay {delay} should be ~3600 + jitter");
    }

    #[tokio::test]
    async fn test_autonomous_loop_sentinel() {
        let tool = ScheduleWakeupTool::new();

        let result = tool
            .execute(json!({
                "operation": "Schedule",
                "delay_seconds": 270,
                "prompt": "<<autonomous-loop-dynamic>>"
            }))
            .await
            .unwrap();

        assert_eq!(
            result.metadata.get("is_autonomous").unwrap(),
            &json!(true)
        );
    }

    #[tokio::test]
    async fn test_list_wakeups() {
        let tool = ScheduleWakeupTool::new();

        tool.execute(json!({
            "operation": "Schedule",
            "delay_seconds": 120,
            "prompt": "Task A"
        }))
        .await
        .unwrap();

        tool.execute(json!({
            "operation": "Schedule",
            "delay_seconds": 300,
            "prompt": "Task B"
        }))
        .await
        .unwrap();

        let list = tool
            .execute(json!({"operation": "List"}))
            .await
            .unwrap();

        assert!(!list.is_error);
        let wakeups = list.metadata.get("wakeups").unwrap().as_array().unwrap();
        assert_eq!(wakeups.len(), 2);
    }

    #[tokio::test]
    async fn test_cancel_wakeup() {
        let tool = ScheduleWakeupTool::new();

        let created = tool
            .execute(json!({
                "operation": "Schedule",
                "delay_seconds": 120,
                "prompt": "Cancel me"
            }))
            .await
            .unwrap();

        let id = created.metadata.get("id").unwrap().as_str().unwrap();

        let cancelled = tool
            .execute(json!({
                "operation": "Cancel",
                "id": id
            }))
            .await
            .unwrap();

        assert!(!cancelled.is_error);

        // List should be empty now
        let list = tool
            .execute(json!({"operation": "List"}))
            .await
            .unwrap();
        let wakeups = list.metadata.get("wakeups").unwrap().as_array().unwrap();
        assert_eq!(wakeups.len(), 0);
    }

    #[tokio::test]
    async fn test_cancel_nonexistent() {
        let tool = ScheduleWakeupTool::new();

        let result = tool
            .execute(json!({
                "operation": "Cancel",
                "id": "does-not-exist"
            }))
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_unknown_operation() {
        let tool = ScheduleWakeupTool::new();

        let result = tool
            .execute(json!({"operation": "Bogus"}))
            .await;

        assert!(result.is_err());
    }

    #[test]
    fn test_poll_due_nothing_due() {
        let tool = ScheduleWakeupTool::new();
        // Nothing scheduled, nothing due
        let due = tool.poll_due();
        assert!(due.is_empty());
    }

    #[test]
    fn test_cache_hint_warm() {
        assert!(cache_hint(120).contains("cache warm"));
        assert!(cache_hint(270).contains("cache warm"));
    }

    #[test]
    fn test_cache_hint_boundary() {
        assert!(cache_hint(280).contains("cache boundary"));
        assert!(cache_hint(300).contains("cache boundary"));
    }

    #[test]
    fn test_cache_hint_miss() {
        assert!(cache_hint(310).contains("cache miss"));
        assert!(cache_hint(600).contains("cache miss"));
        assert!(cache_hint(3600).contains("cache miss"));
    }

    #[tokio::test]
    async fn test_schedule_includes_cache_hint() {
        let tool = ScheduleWakeupTool::new();
        let result = tool
            .execute(json!({
                "operation": "Schedule",
                "delay_seconds": 120,
                "prompt": "test"
            }))
            .await
            .unwrap();
        assert!(result.content.contains("Cache:"));
        assert!(result.metadata.get("cache_ttl_hint").is_some());
    }

    #[tokio::test]
    async fn test_poll_due_fires() {
        let store = Arc::new(RwLock::new(HashMap::new()));
        let tool = ScheduleWakeupTool::with_store(store.clone());

        // Manually insert an already-due wakeup
        let now = Utc::now();
        let past = now - chrono::Duration::seconds(10);
        let req = WakeupRequest {
            id: "test-due".to_string(),
            delay_seconds: 60,
            prompt: "Should fire now".to_string(),
            created_at: past.to_rfc3339(),
            fire_at: past.to_rfc3339(),
            is_autonomous: false,
        };
        store.write().unwrap().insert("test-due".to_string(), req);

        let due = tool.poll_due();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].id, "test-due");

        // Should be removed from store
        assert!(store.read().unwrap().is_empty());
    }
}
