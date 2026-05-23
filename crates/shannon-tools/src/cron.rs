//! Cron scheduling tools
//!
//! Provides implementations for:
//! - CronCreate: Schedule a recurring or one-shot prompt
//! - CronDelete: Cancel a scheduled cron job
//! - CronList: List active cron jobs
//!
//! Enables time-based task scheduling with persistence options.
//! Matches Claude Code's ScheduleCronTool behavior:
//! - Standard 5-field cron expressions (M H DoM Mon DoW)
//! - One-shot (auto-delete after fire) vs recurring (7-day auto-expiry)
//! - Day-of-week name support (MON, TUE, ..., SUN)
//! - Range (1-5) and step (*/5) expressions
//! - Next fire time calculation for scheduling
//!
//! # Security Audit (S3)
//!
//! Verified 2026-04-08: No `unreachable!()` macros in production code.
//! All error paths return proper `ToolError` values. Test code may use
//! `panic!()` which is acceptable for test assertions.

use crate::{Tool, ToolError, ToolOutput, ToolResult};
use async_trait::async_trait;
use chrono::{Datelike, Duration, Local, NaiveDateTime, TimeZone, Timelike, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

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

    /// When this job was created (RFC 3339)
    pub created_at: String,

    /// Next scheduled run time (RFC 3339, local time)
    pub next_run: Option<String>,

    /// When this job expires (RFC 3339, UTC). Recurring jobs auto-expire after 7 days.
    pub expires_at: Option<String>,

    /// Whether this job has already fired (used for one-shot detection)
    pub fired: bool,
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

    /// Next scheduled fire time (RFC 3339)
    pub next_run: Option<String>,

    /// When the job expires (RFC 3339)
    pub expires_at: Option<String>,
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

/// Information about a cron job (public view)
#[derive(Debug, Serialize)]
pub struct CronJobInfo {
    /// Job ID
    pub id: String,

    /// Cron expression
    pub cron: String,

    /// Human-readable schedule
    pub human_schedule: String,

    /// Prompt (truncated to 100 chars)
    pub prompt: String,

    /// Whether job is recurring
    pub recurring: Option<bool>,

    /// Whether job persists to disk
    pub durable: Option<bool>,

    /// Next scheduled fire time
    pub next_run: Option<String>,

    /// Expiry time
    pub expires_at: Option<String>,
}

/// Cron job store (shared state)
type CronStore = Arc<RwLock<HashMap<String, CronJob>>>;

/// Number of days before recurring jobs auto-expire
const RECURRING_EXPIRY_DAYS: i64 = 7;

/// A due job returned by [`CronTool::drain_due`].
#[derive(Debug, Clone)]
pub struct DueJob {
    /// Job ID
    pub id: String,
    /// Prompt to execute
    pub prompt: String,
    /// Human-readable next fire time for recurring jobs, None for one-shot
    pub next_run: Option<String>,
    /// True if this job was overdue (missed while offline)
    pub was_overdue: bool,
}

/// Format a datetime as a human-readable relative or absolute time string.
fn format_next_run_human(dt: &chrono::DateTime<chrono::Local>) -> String {
    let now = chrono::Local::now();
    let diff = *dt - now;
    let mins = diff.num_minutes();
    if mins <= 0 {
        "now".to_string()
    } else if mins < 60 {
        format!("in {mins}m")
    } else {
        let hrs = mins / 60;
        let rem_mins = mins % 60;
        if rem_mins == 0 {
            format!("in {hrs}h")
        } else {
            format!("in {hrs}h {rem_mins}m")
        }
    }
}

// ---------------------------------------------------------------------------
// Cron expression parsing & validation
// ---------------------------------------------------------------------------

/// Valid day-of-week name mappings (case-insensitive): name -> numeric (0=Sun..6=Sat)
const DAY_NAMES: &[(&str, u32)] = &[
    ("SUN", 0),
    ("MON", 1),
    ("TUE", 2),
    ("WED", 3),
    ("THU", 4),
    ("FRI", 5),
    ("SAT", 6),
];

/// Valid month name mappings (case-insensitive): name -> numeric (1=Jan..12=Dec)
const MONTH_NAMES: &[(&str, u32)] = &[
    ("JAN", 1),
    ("FEB", 2),
    ("MAR", 3),
    ("APR", 4),
    ("MAY", 5),
    ("JUN", 6),
    ("JUL", 7),
    ("AUG", 8),
    ("SEP", 9),
    ("OCT", 10),
    ("NOV", 11),
    ("DEC", 12),
];

/// Field metadata for the 5 cron fields
struct FieldMeta {
    name: &'static str,
    min: u32,
    max: u32,
}

const FIELD_META: [FieldMeta; 5] = [
    FieldMeta {
        name: "minute",
        min: 0,
        max: 59,
    },
    FieldMeta {
        name: "hour",
        min: 0,
        max: 23,
    },
    FieldMeta {
        name: "day of month",
        min: 1,
        max: 31,
    },
    FieldMeta {
        name: "month",
        min: 1,
        max: 12,
    },
    FieldMeta {
        name: "day of week",
        min: 0,
        max: 6,
    },
];

/// Expand a day-of-week name to its numeric value. Returns None if not a valid name.
fn parse_day_name(s: &str) -> Option<u32> {
    DAY_NAMES
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(s))
        .map(|(_, v)| *v)
}

/// Expand a month name to its numeric value. Returns None if not a valid name.
fn parse_month_name(s: &str) -> Option<u32> {
    MONTH_NAMES
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(s))
        .map(|(_, v)| *v)
}

/// Parse a single token into a numeric value, resolving day/month names as needed.
fn resolve_token(token: &str, field_index: usize) -> Option<u32> {
    // Try direct numeric parse
    if let Ok(n) = token.parse::<u32>() {
        return Some(n);
    }
    // Try name resolution for specific fields
    match field_index {
        4 => parse_day_name(token),   // day of week
        3 => parse_month_name(token), // month
        _ => None,
    }
}

/// Validate a single field value against its range.
fn validate_value(value: u32, meta: &FieldMeta) -> bool {
    value >= meta.min && value <= meta.max
}

/// Validate a single cron field. Supports:
/// - `*`  (wildcard)
/// - `N`  (single value)
/// - `N-M` (range)
/// - `*/N` (step from min)
/// - `N-M/S` (range with step)
/// - `N,M,...` (list of values, ranges, or steps)
fn validate_field(field: &str, meta: &FieldMeta, field_index: usize) -> Result<(), ToolError> {
    if field.is_empty() {
        return Err(ToolError::InvalidInput(format!(
            "Empty {} field in cron expression",
            meta.name
        )));
    }

    // Split by comma for list support
    for part in field.split(',') {
        validate_field_part(part.trim(), meta, field_index)?;
    }

    Ok(())
}

/// Validate a single part (no commas) of a cron field.
fn validate_field_part(part: &str, meta: &FieldMeta, field_index: usize) -> Result<(), ToolError> {
    if part == "*" {
        return Ok(());
    }

    // Handle step expressions: value/step or */step or range/step
    if let Some((base, step_str)) = part.split_once('/') {
        let step: u32 = step_str.parse::<u32>().map_err(|_| {
            ToolError::InvalidInput(format!(
                "Invalid step value '{}' in {} field",
                step_str, meta.name
            ))
        })?;
        if step == 0 {
            return Err(ToolError::InvalidInput(format!(
                "Step value must be > 0 in {} field",
                meta.name
            )));
        }

        if base == "*" {
            // */N is always valid if N > 0 (already checked)
            return Ok(());
        }

        // Handle range/step: N-M/S
        if let Some((start_str, end_str)) = base.split_once('-') {
            let start = resolve_token(start_str, field_index).ok_or_else(|| {
                ToolError::InvalidInput(format!(
                    "Invalid start value '{}' in {} field",
                    start_str, meta.name
                ))
            })?;
            let end = resolve_token(end_str, field_index).ok_or_else(|| {
                ToolError::InvalidInput(format!(
                    "Invalid end value '{}' in {} field",
                    end_str, meta.name
                ))
            })?;
            if !validate_value(start, meta) || !validate_value(end, meta) {
                return Err(ToolError::InvalidInput(format!(
                    "Range {}-{} out of bounds for {} ({})",
                    start, end, meta.name, meta.min
                )));
            }
            if start > end {
                return Err(ToolError::InvalidInput(format!(
                    "Range start {} > end {} in {} field",
                    start, end, meta.name
                )));
            }
            return Ok(());
        }

        // Single value/step: N/S
        let value = resolve_token(base, field_index).ok_or_else(|| {
            ToolError::InvalidInput(format!("Invalid value '{}' in {} field", base, meta.name))
        })?;
        if !validate_value(value, meta) {
            return Err(ToolError::InvalidInput(format!(
                "Value {} out of range for {} ({})",
                value, meta.name, meta.min
            )));
        }
        return Ok(());
    }

    // Handle range: N-M
    if let Some((start_str, end_str)) = part.split_once('-') {
        let start = resolve_token(start_str, field_index).ok_or_else(|| {
            ToolError::InvalidInput(format!(
                "Invalid start value '{}' in {} field",
                start_str, meta.name
            ))
        })?;
        let end = resolve_token(end_str, field_index).ok_or_else(|| {
            ToolError::InvalidInput(format!(
                "Invalid end value '{}' in {} field",
                end_str, meta.name
            ))
        })?;
        if !validate_value(start, meta) || !validate_value(end, meta) {
            return Err(ToolError::InvalidInput(format!(
                "Range {}-{} out of bounds for {} ({})",
                start, end, meta.name, meta.min
            )));
        }
        if start > end {
            return Err(ToolError::InvalidInput(format!(
                "Range start {} > end {} in {} field",
                start, end, meta.name
            )));
        }
        return Ok(());
    }

    // Single value (numeric or name)
    let value = resolve_token(part, field_index).ok_or_else(|| {
        ToolError::InvalidInput(format!("Invalid value '{}' in {} field", part, meta.name))
    })?;
    if !validate_value(value, meta) {
        return Err(ToolError::InvalidInput(format!(
            "Value {} out of range for {} ({})",
            value, meta.name, meta.min
        )));
    }

    Ok(())
}

/// Validate a full cron expression.
pub fn validate_cron(cron: &str) -> Result<(), ToolError> {
    let parts: Vec<&str> = cron.split_whitespace().collect();
    if parts.len() != 5 {
        return Err(ToolError::InvalidInput(
            "Invalid cron expression. Expected 5 fields (minute hour day_of_month month day_of_week)".to_string(),
        ));
    }

    for (i, part) in parts.iter().enumerate() {
        validate_field(part, &FIELD_META[i], i)?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Next fire time calculation
// ---------------------------------------------------------------------------

/// Represents a parsed cron field's matching values.
#[derive(Debug, Clone)]
struct CronFieldValues {
    /// Sorted set of values that match this field
    values: Vec<u32>,
}

impl CronFieldValues {
    fn matches(&self, value: u32) -> bool {
        self.values.contains(&value)
    }
}

/// Build the set of matching values for a single cron field.
fn build_field_values(field: &str, meta: &FieldMeta, field_index: usize) -> CronFieldValues {
    let mut values = Vec::new();

    for part in field.split(',') {
        collect_part_values(part.trim(), meta, field_index, &mut values);
    }

    values.sort_unstable();
    values.dedup();
    CronFieldValues { values }
}

/// Collect matching values from a single field part.
fn collect_part_values(part: &str, meta: &FieldMeta, field_index: usize, out: &mut Vec<u32>) {
    if part == "*" {
        for v in meta.min..=meta.max {
            out.push(v);
        }
        return;
    }

    // Step expressions
    if let Some((base, step_str)) = part.split_once('/') {
        let step: u32 = step_str.parse().unwrap_or(1).max(1);
        if base == "*" {
            let mut v = meta.min;
            while v <= meta.max {
                out.push(v);
                v += step;
            }
        } else if let Some((start_str, end_str)) = base.split_once('-') {
            let start = resolve_token(start_str, field_index).unwrap_or(meta.min);
            let end = resolve_token(end_str, field_index).unwrap_or(meta.max);
            let mut v = start;
            while v <= end {
                out.push(v);
                v += step;
            }
        } else {
            let start = resolve_token(base, field_index).unwrap_or(meta.min);
            let mut v = start;
            while v <= meta.max {
                out.push(v);
                v += step;
            }
        }
        return;
    }

    // Range
    if let Some((start_str, end_str)) = part.split_once('-') {
        let start = resolve_token(start_str, field_index).unwrap_or(meta.min);
        let end = resolve_token(end_str, field_index).unwrap_or(meta.max);
        for v in start..=end {
            out.push(v);
        }
        return;
    }

    // Single value
    if let Some(v) = resolve_token(part, field_index) {
        out.push(v);
    }
}

/// Calculate the next fire time for a cron expression starting from `after`.
/// Returns None if no fire time can be found within a reasonable window (366 days).
pub fn calculate_next_fire(cron: &str, after: NaiveDateTime) -> Option<NaiveDateTime> {
    if validate_cron(cron).is_err() {
        return None;
    }

    let parts: Vec<&str> = cron.split_whitespace().collect();
    let minutes = build_field_values(parts[0], &FIELD_META[0], 0);
    let hours = build_field_values(parts[1], &FIELD_META[1], 1);
    let doms = build_field_values(parts[2], &FIELD_META[2], 2);
    let months = build_field_values(parts[3], &FIELD_META[3], 3);
    let dows = build_field_values(parts[4], &FIELD_META[4], 4);

    // Track whether dom/dow were originally wildcarded.
    // Standard cron behavior: if one is *, only the other is checked.
    // If both are restricted, either match is valid (OR logic).
    let dom_is_wildcard = parts[2] == "*";
    let dow_is_wildcard = parts[4] == "*";

    // Advance by one minute to start searching from the next minute
    let mut candidate = after + Duration::minutes(1);
    // Zero out seconds and nanoseconds
    candidate = candidate
        .with_second(0)
        .unwrap_or(candidate)
        .with_nanosecond(0)
        .unwrap_or(candidate);

    let max_search = after + Duration::days(366);

    while candidate <= max_search {
        let month = candidate.month();
        if !months.matches(month) {
            // Skip to the first day of the next matching month
            candidate = skip_to_next_month(&candidate, &months);
            continue;
        }

        let dow = candidate.weekday().num_days_from_sunday();
        let dom = candidate.day();
        let hour = candidate.hour();
        let minute = candidate.minute();

        // Day matching logic (standard cron):
        // - If both dom and dow are restricted: match if EITHER matches (OR)
        // - If only dom is restricted (dow is *): match dom only
        // - If only dow is restricted (dom is *): match dow only
        // - If both are *: always matches
        let day_match = if dom_is_wildcard && dow_is_wildcard {
            true
        } else if dom_is_wildcard {
            dows.matches(dow)
        } else if dow_is_wildcard {
            doms.matches(dom)
        } else {
            // Both restricted: either match is valid
            doms.matches(dom) || dows.matches(dow)
        };

        if !day_match {
            candidate += Duration::days(1);
            candidate = candidate
                .with_hour(0)
                .unwrap_or(candidate)
                .with_minute(0)
                .unwrap_or(candidate);
            continue;
        }

        if !hours.matches(hour) {
            candidate += Duration::hours(1);
            candidate = candidate.with_minute(0).unwrap_or(candidate);
            continue;
        }

        if !minutes.matches(minute) {
            candidate += Duration::minutes(1);
            continue;
        }

        // All fields match
        return Some(candidate);
    }

    None
}

/// Skip to the first day of the next matching month.
fn skip_to_next_month(current: &NaiveDateTime, months: &CronFieldValues) -> NaiveDateTime {
    let mut month = current.month();
    let mut year = current.year();

    loop {
        month += 1;
        if month > 12 {
            month = 1;
            year += 1;
        }
        if months.matches(month) {
            break;
        }
        // Safety: don't loop forever
        if year > current.year() + 5 {
            break;
        }
    }

    NaiveDateTime::new(
        chrono::NaiveDate::from_ymd_opt(year, month, 1).unwrap_or_else(|| {
            chrono::NaiveDate::from_ymd_opt(year, 1, 1).unwrap_or(chrono::NaiveDate::MIN)
        }),
        chrono::NaiveTime::from_hms_opt(0, 0, 0).unwrap_or_default(),
    )
}

// ---------------------------------------------------------------------------
// Human-readable conversion
// ---------------------------------------------------------------------------

/// Convert a cron expression to a human-readable description.
fn cron_to_human(cron: &str) -> String {
    let parts: Vec<&str> = cron.split_whitespace().collect();
    if parts.len() != 5 {
        return format!("Invalid cron: {cron}");
    }

    let minute = humanize_field(parts[0], "minute", None);
    let hour = humanize_field(parts[1], "hour", None);
    let dom = humanize_field(parts[2], "day", None);
    let month = humanize_field(parts[3], "month", Some(MONTH_NAMES));
    let dow = humanize_field(parts[4], "weekday", Some(DAY_NAMES));

    if parts[2] == "*" && parts[4] != "*" {
        format!("At {hour}:{minute} on {dow}")
    } else if parts[4] == "*" && parts[2] != "*" {
        format!("At {hour}:{minute} on the {dom} of {month}")
    } else if parts[2] == "*" && parts[4] == "*" {
        format!("At {hour}:{minute} every day")
    } else {
        format!("At {hour}:{minute} on {dom} of {month}, {dow}")
    }
}

/// Humanize a single cron field value.
fn humanize_field(field: &str, name: &str, names: Option<&[(&str, u32)]>) -> String {
    if field == "*" {
        return format!("every {name}");
    }

    if let Some(names) = names {
        // Try to resolve to name
        if let Ok(n) = field.parse::<u32>() {
            if let Some((label, _)) = names.iter().find(|(_, v)| *v == n) {
                return label.to_string();
            }
        }
    }

    // Handle step
    if let Some((base, step)) = field.split_once('/') {
        let base_str = if base == "*" {
            format!("every {name}")
        } else {
            base.to_string()
        };
        return format!("{base_str} (every {step})");
    }

    // Handle range
    if field.contains('-') {
        return field.to_string();
    }

    field.to_string()
}

// ---------------------------------------------------------------------------
// Cron tool implementation
// ---------------------------------------------------------------------------

/// Cron management tool
#[derive(Debug, Clone)]
pub struct CronTool {
    description: String,
    store: CronStore,
    max_jobs: usize,
    /// Optional path for persisting durable jobs. If None, no persistence occurs.
    persistence_path: Option<PathBuf>,
}

/// Default file name for durable cron jobs.
const DURABLE_CRON_FILE: &str = "durable_cron_jobs.json";

impl CronTool {
    pub fn new() -> Self {
        Self {
            description: "Schedule recurring or one-shot prompts for time-based task execution"
                .to_string(),
            store: Arc::new(RwLock::new(HashMap::new())),
            max_jobs: 50,
            persistence_path: None,
        }
    }

    /// Create a CronTool with persistence enabled.
    /// Durable jobs are saved to/loaded from `~/.shannon/{DURABLE_CRON_FILE}`.
    pub fn with_persistence() -> Self {
        let path = dirs::home_dir().map(|h| h.join(".shannon").join(DURABLE_CRON_FILE));
        let mut tool = Self {
            description: "Schedule recurring or one-shot prompts for time-based task execution"
                .to_string(),
            store: Arc::new(RwLock::new(HashMap::new())),
            max_jobs: 50,
            persistence_path: path,
        };
        tool.load_durable_jobs();
        tool
    }

    /// Create a new CronTool with a shared store (for testing).
    pub fn with_store(store: CronStore) -> Self {
        Self {
            description: "Schedule recurring or one-shot prompts for time-based task execution"
                .to_string(),
            store,
            max_jobs: 50,
            persistence_path: None,
        }
    }

    /// Get a reference to the underlying store (for testing).
    pub fn store(&self) -> &CronStore {
        &self.store
    }

    /// Load durable jobs from disk into the in-memory store.
    /// Called during construction when persistence is enabled.
    fn load_durable_jobs(&mut self) {
        let path = match &self.persistence_path {
            Some(p) => p.clone(),
            None => return,
        };

        if !path.exists() {
            return;
        }

        match std::fs::read_to_string(&path) {
            Ok(content) => {
                match serde_json::from_str::<Vec<CronJob>>(&content) {
                    Ok(jobs) => {
                        if let Ok(mut store) = self.store.write() {
                            let now = Utc::now();
                            for job in jobs {
                                // Skip expired jobs during load
                                if let Some(ref expires_at) = job.expires_at {
                                    if let Ok(expiry) =
                                        chrono::DateTime::parse_from_rfc3339(expires_at)
                                    {
                                        if expiry <= now {
                                            continue;
                                        }
                                    }
                                }
                                // Recalculate next_run for recurring jobs
                                let mut job = job;
                                let now_local = Local::now();
                                if job.recurring {
                                    if let Some(next) =
                                        calculate_next_fire(&job.cron, now_local.naive_local())
                                    {
                                        let local = Local
                                            .from_local_datetime(&next)
                                            .single()
                                            .unwrap_or(now_local);
                                        job.next_run = Some(local.to_rfc3339());
                                    }
                                }
                                store.insert(job.id.clone(), job);
                            }
                        }
                        tracing::debug!("Loaded durable cron jobs from {}", path.display());
                    }
                    Err(e) => {
                        tracing::warn!("Failed to parse durable cron jobs: {e}");
                    }
                }
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to read durable cron jobs from {}: {e}",
                    path.display()
                );
            }
        }
    }

    /// Persist durable jobs to disk.
    /// Called after create/delete operations when persistence is enabled.
    fn save_durable_jobs(&self) {
        let path = match &self.persistence_path {
            Some(p) => p.clone(),
            None => return,
        };

        let store = match self.store.read() {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("Failed to acquire store lock for saving: {e}");
                return;
            }
        };

        let durable_jobs: Vec<&CronJob> = store.values().filter(|j| j.durable).collect();

        if durable_jobs.is_empty() {
            // Remove the file if no durable jobs remain
            if let Err(e) = std::fs::remove_file(&path) {
                tracing::debug!("Failed to remove durable cron file {}: {e}", path.display());
            }
            return;
        }

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        match serde_json::to_string_pretty(&durable_jobs) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&path, json) {
                    tracing::warn!(
                        "Failed to write durable cron jobs to {}: {e}",
                        path.display()
                    );
                }
            }
            Err(e) => {
                tracing::warn!("Failed to serialize durable cron jobs: {e}");
            }
        }
    }

    /// Create a new cron job
    async fn create_cron(&self, input: CronCreateInput) -> Result<CronCreateOutput, ToolError> {
        // Validate cron expression
        validate_cron(&input.cron)?;

        // Prune expired jobs first
        self.prune_expired_jobs();

        // Check max jobs limit
        let job_count = {
            let store = self.store.read().map_err(|e| {
                ToolError::ExecutionFailed(format!("Failed to acquire store lock: {e}"))
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

        let now = Local::now();
        let created_at = now.to_rfc3339();

        // Calculate next fire time
        let next_run = calculate_next_fire(&input.cron, now.naive_local()).map(|dt| {
            let local = Local.from_local_datetime(&dt).single().unwrap_or(now);
            local.to_rfc3339()
        });

        // Recurring jobs auto-expire after 7 days
        let expires_at = if recurring {
            Some((Utc::now() + Duration::days(RECURRING_EXPIRY_DAYS)).to_rfc3339())
        } else {
            // One-shot jobs: expire after 1 day as a safety net
            Some((Utc::now() + Duration::days(1)).to_rfc3339())
        };

        let job = CronJob {
            id: id.clone(),
            cron: input.cron.clone(),
            prompt: input.prompt.clone(),
            recurring,
            durable,
            agent_id: None,
            created_at,
            next_run: next_run.clone(),
            expires_at: expires_at.clone(),
            fired: false,
        };

        {
            let mut store = self.store.write().map_err(|e| {
                ToolError::ExecutionFailed(format!("Failed to acquire store lock: {e}"))
            })?;
            store.insert(id.clone(), job);
        }

        // Persist durable jobs to disk if enabled
        self.save_durable_jobs();

        let human_schedule = cron_to_human(&input.cron);

        Ok(CronCreateOutput {
            id,
            human_schedule,
            recurring,
            durable: Some(durable),
            next_run,
            expires_at,
        })
    }

    /// Delete a cron job
    async fn delete_cron(&self, input: CronDeleteInput) -> Result<CronDeleteOutput, ToolError> {
        let exists = {
            let store = self.store.read().map_err(|e| {
                ToolError::ExecutionFailed(format!("Failed to acquire store lock: {e}"))
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
                ToolError::ExecutionFailed(format!("Failed to acquire store lock: {e}"))
            })?;
            store.remove(&input.id);
        }

        // Persist durable jobs to disk if enabled
        self.save_durable_jobs();

        Ok(CronDeleteOutput { id: input.id })
    }

    /// List all cron jobs (excluding expired ones)
    async fn list_cron(&self, _input: CronListInput) -> Result<CronListOutput, ToolError> {
        // Prune expired jobs first
        self.prune_expired_jobs();

        let store = self.store.read().map_err(|e| {
            ToolError::ExecutionFailed(format!("Failed to acquire store lock: {e}"))
        })?;

        let jobs: Vec<CronJobInfo> = store
            .values()
            .map(|job| {
                let mut prompt = job.prompt.clone();
                if prompt.len() > 100 {
                    prompt.truncate(100);
                    prompt.push_str("...");
                }
                CronJobInfo {
                    id: job.id.clone(),
                    cron: job.cron.clone(),
                    human_schedule: cron_to_human(&job.cron),
                    prompt,
                    recurring: Some(job.recurring),
                    durable: Some(job.durable),
                    next_run: job.next_run.clone(),
                    expires_at: job.expires_at.clone(),
                }
            })
            .collect();

        Ok(CronListOutput { jobs })
    }

    /// Remove expired jobs from the store.
    fn prune_expired_jobs(&self) {
        let now = Utc::now();
        if let Ok(mut store) = self.store.write() {
            store.retain(|_, job| {
                if let Some(ref expires_at) = job.expires_at {
                    if let Ok(expiry) = chrono::DateTime::parse_from_rfc3339(expires_at) {
                        return expiry > now;
                    }
                }
                true // Keep if no expiry set
            });
        }
    }

    /// Simulate firing a job. Marks one-shot jobs as fired (they should be auto-deleted).
    /// Returns true if the job should be removed (one-shot that has fired).
    pub fn fire_job(&self, job_id: &str) -> Result<bool, ToolError> {
        let should_delete = {
            let mut store = self.store.write().map_err(|e| {
                ToolError::ExecutionFailed(format!("Failed to acquire store lock: {e}"))
            })?;

            let job = store.get_mut(job_id).ok_or_else(|| {
                ToolError::InvalidInput(format!("No scheduled job with id '{job_id}'"))
            })?;

            if !job.recurring {
                // One-shot: mark as fired, should be deleted
                job.fired = true;
                true
            } else {
                // Recurring: update next_run with jitter
                let now = Local::now();
                if let Some(next) = calculate_next_fire(&job.cron, now.naive_local()) {
                    let jitter_secs = shannon_core::scheduled_routines::apply_jitter(300, 900);
                    let local = Local.from_local_datetime(&next).single().unwrap_or(now);
                    let jittered = local + chrono::Duration::seconds(jitter_secs as i64);
                    job.next_run = Some(jittered.to_rfc3339());
                }
                false
            }
        };

        if should_delete {
            let mut store = self.store.write().map_err(|e| {
                ToolError::ExecutionFailed(format!("Failed to acquire store lock: {e}"))
            })?;
            store.remove(job_id);
        }

        Ok(should_delete)
    }

    /// Check for due jobs, fire them, and return their prompts.
    /// Recurring jobs get their `next_run` recalculated with jitter.
    /// One-shot jobs are removed from the store.
    pub fn drain_due(&self) -> Vec<DueJob> {
        let now = Local::now();
        let now_ts = now.timestamp();
        let mut due = Vec::new();

        if let Ok(mut store) = self.store.write() {
            let due_ids: Vec<String> = store
                .iter()
                .filter(|(_, job)| {
                    if job.fired {
                        return false;
                    }
                    if let Some(ref next_run) = job.next_run {
                        if let Ok(next) = chrono::DateTime::parse_from_rfc3339(next_run) {
                            return next.timestamp() <= now_ts;
                        }
                    }
                    false
                })
                .map(|(id, _)| id.clone())
                .collect();

            for id in &due_ids {
                if let Some(job) = store.get_mut(id) {
                    let was_overdue = job.next_run.as_ref().is_some_and(|nr| {
                        chrono::DateTime::parse_from_rfc3339(nr)
                            .is_ok_and(|t| t.timestamp() < now_ts - 300)
                    });
                    due.push(DueJob {
                        id: id.clone(),
                        prompt: job.prompt.clone(),
                        next_run: None,
                        was_overdue,
                    });
                    if !job.recurring {
                        job.fired = true;
                    } else if let Some(next) = calculate_next_fire(&job.cron, now.naive_local()) {
                        let jitter_secs = shannon_core::scheduled_routines::apply_jitter(300, 900);
                        let local = Local.from_local_datetime(&next).single().unwrap_or(now);
                        let jittered = local + chrono::Duration::seconds(jitter_secs as i64);
                        let next_str = jittered.to_rfc3339();
                        if let Some(last) = due.last_mut() {
                            last.next_run = Some(format_next_run_human(&jittered));
                        }
                        job.next_run = Some(next_str);
                    }
                }
            }

            store.retain(|_, job| !job.fired);
        }

        self.save_durable_jobs();
        due
    }
}

impl Default for CronTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for CronTool {
    async fn execute(&self, input: serde_json::Value) -> ToolResult<ToolOutput> {
        let operation = input
            .get("operation")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput("Missing operation field".to_string()))?;

        match operation {
            "Create" => {
                let create_input: CronCreateInput = serde_json::from_value(input).map_err(|e| {
                    ToolError::InvalidInput(format!("Invalid create cron input: {e}"))
                })?;
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
                        if let Some(ref next_run) = output.next_run {
                            map.insert("next_run".to_string(), json!(next_run));
                        }
                        if let Some(ref expires_at) = output.expires_at {
                            map.insert("expires_at".to_string(), json!(expires_at));
                        }
                        map
                    },
                })
            }
            "Delete" => {
                let delete_input: CronDeleteInput = serde_json::from_value(input).map_err(|e| {
                    ToolError::InvalidInput(format!("Invalid delete cron input: {e}"))
                })?;
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
                let list_input: CronListInput = serde_json::from_value(input).map_err(|e| {
                    ToolError::InvalidInput(format!("Invalid list cron input: {e}"))
                })?;
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
                "Unknown operation: {operation}"
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
                    "description": "Recurring job (default: true)"
                },
                "durable": {
                    "type": "boolean",
                    "description": "Persist to disk (default: false)"
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Validation tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_valid_every_minute() {
        assert!(validate_cron("* * * * *").is_ok());
    }

    #[test]
    fn test_valid_specific_time() {
        assert!(validate_cron("0 9 * * *").is_ok());
    }

    #[test]
    fn test_valid_every_five_minutes() {
        assert!(validate_cron("*/5 * * * *").is_ok());
    }

    #[test]
    fn test_valid_hourly() {
        assert!(validate_cron("0 * * * *").is_ok());
    }

    #[test]
    fn test_valid_weekdays() {
        assert!(validate_cron("0 9 * * 1-5").is_ok());
    }

    #[test]
    fn test_valid_range_expression() {
        assert!(validate_cron("0 9-17 * * *").is_ok());
    }

    #[test]
    fn test_valid_step_with_range() {
        assert!(validate_cron("0-30/10 * * * *").is_ok());
    }

    #[test]
    fn test_valid_list_values() {
        assert!(validate_cron("0,15,30,45 * * * *").is_ok());
    }

    #[test]
    fn test_valid_day_names() {
        assert!(validate_cron("0 9 * * MON").is_ok());
    }

    #[test]
    fn test_valid_day_names_case_insensitive() {
        assert!(validate_cron("0 9 * * mon").is_ok());
        assert!(validate_cron("0 9 * * Mon").is_ok());
    }

    #[test]
    fn test_valid_multiple_day_names() {
        assert!(validate_cron("0 9 * * MON,FRI").is_ok());
    }

    #[test]
    fn test_valid_month_names() {
        assert!(validate_cron("0 9 1 JAN *").is_ok());
        assert!(validate_cron("0 9 1 Feb *").is_ok());
    }

    #[test]
    fn test_valid_month_range_names() {
        assert!(validate_cron("0 9 * JAN-MAR *").is_ok());
    }

    #[test]
    fn test_valid_complex_expression() {
        assert!(validate_cron("30 14 28 2 *").is_ok());
    }

    #[test]
    fn test_valid_sunday() {
        assert!(validate_cron("0 9 * * 0").is_ok());
        assert!(validate_cron("0 9 * * SUN").is_ok());
    }

    #[test]
    fn test_valid_day_range_with_step() {
        assert!(validate_cron("0 9 * * 1-5/2").is_ok());
    }

    // -----------------------------------------------------------------------
    // Invalid expression tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_invalid_too_few_fields() {
        let result = validate_cron("* * *");
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("5 fields"));
    }

    #[test]
    fn test_invalid_too_many_fields() {
        let result = validate_cron("* * * * * *");
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("5 fields"));
    }

    #[test]
    fn test_invalid_minute_out_of_range() {
        let result = validate_cron("60 * * * *");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("minute"));
    }

    #[test]
    fn test_invalid_hour_out_of_range() {
        let result = validate_cron("0 24 * * *");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("hour"));
    }

    #[test]
    fn test_invalid_day_of_month_out_of_range() {
        let result = validate_cron("0 0 32 * *");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("day of month"));
    }

    #[test]
    fn test_invalid_month_out_of_range() {
        let result = validate_cron("0 0 1 13 *");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("month"));
    }

    #[test]
    fn test_invalid_day_of_week_out_of_range() {
        let result = validate_cron("0 0 * * 7");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("day of week"));
    }

    #[test]
    fn test_invalid_step_zero() {
        let result = validate_cron("*/0 * * * *");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("> 0"));
    }

    #[test]
    fn test_invalid_range_inverted() {
        let result = validate_cron("0 17-9 * * *");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("start"));
    }

    #[test]
    fn test_invalid_non_numeric() {
        let result = validate_cron("abc * * * *");
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_empty_field_via_deserialize() {
        // An empty cron string is not a valid 5-field expression
        let result = validate_cron("");
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("5 fields"));
    }

    #[test]
    fn test_invalid_day_name() {
        let result = validate_cron("0 9 * * FOO");
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_month_name() {
        let result = validate_cron("0 9 1 FOO *");
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_negative() {
        let result = validate_cron("-1 * * * *");
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_step_non_numeric() {
        let result = validate_cron("*/abc * * * *");
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // Next fire time calculation tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_next_fire_every_minute() {
        let now = Local::now().naive_local();
        let next = calculate_next_fire("* * * * *", now).unwrap();
        // Should be approximately 1 minute from now
        let diff = next.signed_duration_since(now);
        assert!(diff.num_minutes() >= 0);
        assert!(diff.num_minutes() <= 2);
    }

    #[test]
    fn test_next_fire_specific_time() {
        // "0 9 * * *" - fires at 9:00 AM
        let base = NaiveDateTime::new(
            chrono::NaiveDate::from_ymd_opt(2026, 4, 3).unwrap(),
            chrono::NaiveTime::from_hms_opt(8, 30, 0).unwrap(),
        );
        let next = calculate_next_fire("0 9 * * *", base).unwrap();
        assert_eq!(next.hour(), 9);
        assert_eq!(next.minute(), 0);
        assert_eq!(
            next.date(),
            chrono::NaiveDate::from_ymd_opt(2026, 4, 3).unwrap()
        );
    }

    #[test]
    fn test_next_fire_specific_time_already_passed() {
        // "0 9 * * *" - fires at 9:00 AM, but it's already 10:00
        let base = NaiveDateTime::new(
            chrono::NaiveDate::from_ymd_opt(2026, 4, 3).unwrap(),
            chrono::NaiveTime::from_hms_opt(10, 0, 0).unwrap(),
        );
        let next = calculate_next_fire("0 9 * * *", base).unwrap();
        assert_eq!(next.hour(), 9);
        assert_eq!(
            next.date(),
            chrono::NaiveDate::from_ymd_opt(2026, 4, 4).unwrap()
        );
    }

    #[test]
    fn test_next_fire_five_minute_interval() {
        // "*/5 * * * *" - fires every 5 minutes
        let base = NaiveDateTime::new(
            chrono::NaiveDate::from_ymd_opt(2026, 4, 3).unwrap(),
            chrono::NaiveTime::from_hms_opt(9, 7, 0).unwrap(),
        );
        let next = calculate_next_fire("*/5 * * * *", base).unwrap();
        assert_eq!(next.minute(), 10);
        assert_eq!(next.hour(), 9);
    }

    #[test]
    fn test_next_fire_specific_weekday() {
        // "0 9 * * 1" - fires at 9:00 AM on Mondays
        // 2026-04-03 is a Friday, so next Monday is 2026-04-06
        let base = NaiveDateTime::new(
            chrono::NaiveDate::from_ymd_opt(2026, 4, 3).unwrap(),
            chrono::NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
        );
        let next = calculate_next_fire("0 9 * * 1", base).unwrap();
        assert_eq!(next.hour(), 9);
        assert_eq!(next.minute(), 0);
        // Should be a Monday
        assert_eq!(next.weekday().num_days_from_monday(), 0);
    }

    #[test]
    fn test_next_fire_day_name() {
        // "0 9 * * MON" - same as "0 9 * * 1"
        let base = NaiveDateTime::new(
            chrono::NaiveDate::from_ymd_opt(2026, 4, 3).unwrap(),
            chrono::NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
        );
        let next_numeric = calculate_next_fire("0 9 * * 1", base).unwrap();
        let next_name = calculate_next_fire("0 9 * * MON", base).unwrap();
        assert_eq!(next_numeric, next_name);
    }

    #[test]
    fn test_next_fire_hour_range() {
        // "0 9-17 * * *" - fires at minute 0 of hours 9-17
        let base = NaiveDateTime::new(
            chrono::NaiveDate::from_ymd_opt(2026, 4, 3).unwrap(),
            chrono::NaiveTime::from_hms_opt(8, 30, 0).unwrap(),
        );
        let next = calculate_next_fire("0 9-17 * * *", base).unwrap();
        assert_eq!(next.hour(), 9);
        assert_eq!(next.minute(), 0);
    }

    #[test]
    fn test_next_fire_invalid_expression() {
        let base = Local::now().naive_local();
        assert!(calculate_next_fire("invalid * * * *", base).is_none());
    }

    // -----------------------------------------------------------------------
    // CronTool integration tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_create_and_list_cron_job() {
        let tool = CronTool::new();

        let result = tool
            .execute(json!({
                "operation": "Create",
                "cron": "0 9 * * *",
                "prompt": "Check the deploy"
            }))
            .await
            .unwrap();

        assert!(!result.is_error);
        assert!(result.content.contains("Created cron job"));

        let id = result.metadata.get("id").unwrap().as_str().unwrap();
        let human = result
            .metadata
            .get("human_schedule")
            .unwrap()
            .as_str()
            .unwrap();
        assert!(human.contains("9"));

        // List should show the job
        let list_result = tool
            .execute(json!({
                "operation": "List"
            }))
            .await
            .unwrap();

        assert!(!list_result.is_error);
        let jobs = list_result
            .metadata
            .get("jobs")
            .unwrap()
            .as_array()
            .unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0]["id"].as_str().unwrap(), id);
    }

    #[tokio::test]
    async fn test_create_recurring_default() {
        let tool = CronTool::new();

        let result = tool
            .execute(json!({
                "operation": "Create",
                "cron": "*/5 * * * *",
                "prompt": "Ping server"
            }))
            .await
            .unwrap();

        assert!(result.metadata.get("recurring").unwrap().as_bool().unwrap());
        // Should have expires_at (7-day expiry)
        assert!(result.metadata.get("expires_at").is_some());
        // Should have next_run
        assert!(result.metadata.get("next_run").is_some());
    }

    #[tokio::test]
    async fn test_create_one_shot() {
        let tool = CronTool::new();

        let result = tool
            .execute(json!({
                "operation": "Create",
                "cron": "30 14 28 2 *",
                "prompt": "Check deploy",
                "recurring": false
            }))
            .await
            .unwrap();

        assert!(!result.metadata.get("recurring").unwrap().as_bool().unwrap());
        // Should have expires_at (1-day safety net)
        assert!(result.metadata.get("expires_at").is_some());
    }

    #[tokio::test]
    async fn test_delete_cron_job() {
        let tool = CronTool::new();

        let create_result = tool
            .execute(json!({
                "operation": "Create",
                "cron": "0 9 * * *",
                "prompt": "Test"
            }))
            .await
            .unwrap();

        let id = create_result
            .metadata
            .get("id")
            .unwrap()
            .as_str()
            .unwrap()
            .to_string();

        let delete_result = tool
            .execute(json!({
                "operation": "Delete",
                "id": id
            }))
            .await
            .unwrap();

        assert!(!delete_result.is_error);
        assert!(delete_result.content.contains(&id));

        // List should be empty
        let list_result = tool
            .execute(json!({
                "operation": "List"
            }))
            .await
            .unwrap();

        let jobs = list_result
            .metadata
            .get("jobs")
            .unwrap()
            .as_array()
            .unwrap();
        assert_eq!(jobs.len(), 0);
    }

    #[tokio::test]
    async fn test_delete_nonexistent_job() {
        let tool = CronTool::new();

        let result = tool
            .execute(json!({
                "operation": "Delete",
                "id": "nonexistent-id"
            }))
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_create_invalid_cron() {
        let tool = CronTool::new();

        let result = tool
            .execute(json!({
                "operation": "Create",
                "cron": "invalid * * * *",
                "prompt": "Test"
            }))
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_create_missing_fields() {
        let tool = CronTool::new();

        let result = tool
            .execute(json!({
                "operation": "Create",
                "cron": "0 9 * * *"
                // missing prompt
            }))
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_unknown_operation() {
        let tool = CronTool::new();

        let result = tool
            .execute(json!({
                "operation": "Update"
            }))
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_missing_operation() {
        let tool = CronTool::new();

        let result = tool
            .execute(json!({
                "cron": "0 9 * * *"
            }))
            .await;

        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // One-shot fire simulation tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_fire_one_shot_deletes_job() {
        let store = Arc::new(RwLock::new(HashMap::new()));
        let tool = CronTool::with_store(store.clone());

        let result = tool
            .execute(json!({
                "operation": "Create",
                "cron": "30 14 28 2 *",
                "prompt": "One-time check",
                "recurring": false
            }))
            .await
            .unwrap();

        let id = result
            .metadata
            .get("id")
            .unwrap()
            .as_str()
            .unwrap()
            .to_string();

        // Verify job exists
        {
            let s = store.read().unwrap();
            assert!(s.contains_key(&id));
        }

        // Fire the job
        let deleted = tool.fire_job(&id).unwrap();
        assert!(deleted);

        // Job should be gone
        {
            let s = store.read().unwrap();
            assert!(!s.contains_key(&id));
        }
    }

    #[tokio::test]
    async fn test_fire_recurring_keeps_job() {
        let store = Arc::new(RwLock::new(HashMap::new()));
        let tool = CronTool::with_store(store.clone());

        let result = tool
            .execute(json!({
                "operation": "Create",
                "cron": "*/5 * * * *",
                "prompt": "Recurring ping"
            }))
            .await
            .unwrap();

        let id = result
            .metadata
            .get("id")
            .unwrap()
            .as_str()
            .unwrap()
            .to_string();

        // Verify job exists
        {
            let s = store.read().unwrap();
            assert!(s.contains_key(&id));
            assert!(s.get(&id).unwrap().next_run.is_some());
        }

        // Fire the job - recurring jobs should NOT be deleted
        let deleted = tool.fire_job(&id).unwrap();
        assert!(!deleted);

        // Job should still exist with a next_run
        {
            let s = store.read().unwrap();
            let job = s.get(&id).unwrap();
            assert!(job.next_run.is_some());
        }
    }

    // -----------------------------------------------------------------------
    // Human-readable tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_human_every_minute() {
        assert!(cron_to_human("* * * * *").contains("every minute"));
    }

    #[test]
    fn test_human_specific_time() {
        let human = cron_to_human("0 9 * * *");
        assert!(human.contains("9"));
    }

    #[test]
    fn test_human_weekday_only() {
        let human = cron_to_human("0 9 * * MON");
        assert!(human.contains("MON"));
    }

    // -----------------------------------------------------------------------
    // Day-of-week and month name resolution tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_day_name_resolution() {
        assert_eq!(parse_day_name("MON"), Some(1));
        assert_eq!(parse_day_name("mon"), Some(1));
        assert_eq!(parse_day_name("Mon"), Some(1));
        assert_eq!(parse_day_name("TUE"), Some(2));
        assert_eq!(parse_day_name("WED"), Some(3));
        assert_eq!(parse_day_name("THU"), Some(4));
        assert_eq!(parse_day_name("FRI"), Some(5));
        assert_eq!(parse_day_name("SAT"), Some(6));
        assert_eq!(parse_day_name("SUN"), Some(0));
        assert_eq!(parse_day_name("FOO"), None);
    }

    #[test]
    fn test_month_name_resolution() {
        assert_eq!(parse_month_name("JAN"), Some(1));
        assert_eq!(parse_month_name("jan"), Some(1));
        assert_eq!(parse_month_name("Jan"), Some(1));
        assert_eq!(parse_month_name("FEB"), Some(2));
        assert_eq!(parse_month_name("DEC"), Some(12));
        assert_eq!(parse_month_name("FOO"), None);
    }

    // -----------------------------------------------------------------------
    // Max jobs limit test
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_max_jobs_limit() {
        let tool = CronTool::new();
        // Try to create 51 jobs (max is 50)
        let mut results = Vec::new();
        for i in 0..51 {
            let result = tool
                .execute(json!({
                    "operation": "Create",
                    "cron": format!("{} * * * *", i % 60),
                    "prompt": format!("Job {}", i)
                }))
                .await;

            results.push(result);
        }

        // First 50 should succeed, 51st should fail
        assert!(results[49].is_ok());
        assert!(results[50].is_err());
        let err_msg = match &results[50] {
            Err(e) => e.to_string(),
            other => panic!("expected Err at results[50], got: {other:?}"),
        };
        assert!(err_msg.contains("max 50"));
    }

    // -----------------------------------------------------------------------
    // Field value building tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_build_field_values_wildcard() {
        let vals = build_field_values("*", &FIELD_META[0], 0);
        assert_eq!(vals.values.len(), 60);
        assert!(vals.matches(0));
        assert!(vals.matches(59));
        assert!(!vals.matches(60));
    }

    #[test]
    fn test_build_field_values_step() {
        let vals = build_field_values("*/5", &FIELD_META[0], 0);
        assert!(vals.matches(0));
        assert!(vals.matches(5));
        assert!(vals.matches(55));
        assert!(!vals.matches(3));
    }

    #[test]
    fn test_build_field_values_range() {
        let vals = build_field_values("9-17", &FIELD_META[1], 1);
        assert!(vals.matches(9));
        assert!(vals.matches(12));
        assert!(vals.matches(17));
        assert!(!vals.matches(8));
        assert!(!vals.matches(18));
    }

    #[test]
    fn test_build_field_values_list() {
        let vals = build_field_values("0,15,30,45", &FIELD_META[0], 0);
        assert!(vals.matches(0));
        assert!(vals.matches(15));
        assert!(vals.matches(30));
        assert!(vals.matches(45));
        assert!(!vals.matches(10));
    }

    #[test]
    fn test_build_field_values_day_names() {
        let vals = build_field_values("MON,FRI", &FIELD_META[4], 4);
        assert!(vals.matches(1)); // MON
        assert!(vals.matches(5)); // FRI
        assert!(!vals.matches(0)); // SUN
        assert!(!vals.matches(3)); // WED
    }

    #[test]
    fn test_build_field_values_range_with_step() {
        let vals = build_field_values("0-30/10", &FIELD_META[0], 0);
        assert!(vals.matches(0));
        assert!(vals.matches(10));
        assert!(vals.matches(20));
        assert!(vals.matches(30));
        assert!(!vals.matches(5));
        assert!(!vals.matches(15));
    }
}
