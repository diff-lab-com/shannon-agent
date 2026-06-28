//! Usage statistics: append-only token/cache/cost ledger + aggregation.
//!
//! Every `QueryEvent::Usage` the engine emits during `send_message` is
//! appended as one JSON line to `~/.shannon/usage.jsonl`. The
//! `get_usage_stats` command reads that ledger and aggregates it by
//! model / provider / day for the Usage page.
//!
//! This is local-only, user-inspectable telemetry — there is no billing
//! backend yet. The write is best-effort (a log failure never breaks the
//! query stream). The ledger is size-bounded by best-effort rotation (see
//! [`UsageStore::maybe_rotate`]).

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use shannon_core::scheduled_runs::{ScheduledRun, ScheduledRunsStore};

/// One usage event persisted to `usage.jsonl`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageRecord {
    /// Unix epoch milliseconds.
    pub timestamp_ms: u64,
    pub model: String,
    pub provider: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_tokens: u64,
    pub cache_read_tokens: u64,
    pub cost_usd: f64,
}

/// Append-only usage ledger backed by `~/.shannon/usage.jsonl`.
///
/// Mirrors the `TriageStore` pattern: deterministic default path, an
/// injectable `with_path` for tests, skip-unparseable-lines on read.
/// Concurrent appends rely on `O_APPEND` atomicity for small lines
/// (each record is well under `PIPE_BUF`), the same property
/// `TriageStore` depends on.
pub struct UsageStore {
    path: PathBuf,
}

/// Once the on-disk ledger exceeds this size it is trimmed to
/// [`KEEP_RECORDS`]. Sized so a low-volume desktop rotates rarely.
const ROTATE_AT_BYTES: u64 = 10 * 1024 * 1024;

/// Records retained after a rotation (the most recent ones). Their typical
/// on-disk size sits well below [`ROTATE_AT_BYTES`], giving hysteresis so
/// rotation does not fire on every append.
const KEEP_RECORDS: usize = 30_000;

impl UsageStore {
    /// Default location: `~/.shannon/usage.jsonl`.
    pub fn new() -> Self {
        Self {
            path: default_usage_path(),
        }
    }

    /// Custom path (for testing).
    pub fn with_path(path: PathBuf) -> Self {
        Self { path }
    }

    /// Path accessor.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Append one record as a single JSON line.
    pub fn append(&self, record: &UsageRecord) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("create usage dir: {e}"))?;
        }
        let mut line = serde_json::to_string(record)
            .map_err(|e| format!("serialize usage record: {e}"))?;
        line.push('\n');
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|e| format!("open usage log: {e}"))?;
        file.write_all(line.as_bytes())
            .map_err(|e| format!("write usage log: {e}"))?;

        // Best-effort rotation: a failure here must never block the append
        // that just succeeded.
        let _ = self.maybe_rotate();
        Ok(())
    }

    /// Load every record, skipping unparseable lines.
    pub fn load(&self) -> Vec<UsageRecord> {
        let Ok(content) = fs::read_to_string(&self.path) else {
            return Vec::new();
        };
        content
            .lines()
            .filter_map(|line| serde_json::from_str::<UsageRecord>(line).ok())
            .collect()
    }

    /// Trim the ledger to the most recent [`KEEP_RECORDS`] entries once it
    /// exceeds [`ROTATE_AT_BYTES`]. Idempotent and a no-op below the
    /// threshold. Best-effort by contract: callers (append) ignore the
    /// result so rotation can never break a successful append. The rewrite
    /// is atomic (temp file + rename in the same directory); a single-process
    /// desktop makes the append/rotate race negligible.
    pub fn maybe_rotate(&self) -> Result<(), String> {
        let size = self.path.metadata().map(|m| m.len()).unwrap_or(0);
        if size <= ROTATE_AT_BYTES {
            return Ok(());
        }
        let mut records = self.load();
        if records.len() <= KEEP_RECORDS {
            return Ok(());
        }
        // Keep the most recent KEEP_RECORDS.
        let drop_count = records.len() - KEEP_RECORDS;
        records.drain(..drop_count);

        // Atomic rewrite: temp file in the same directory, then rename.
        let tmp = self.path.with_extension("tmp");
        {
            let mut file = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&tmp)
                .map_err(|e| format!("open usage tmp: {e}"))?;
            for r in &records {
                let mut line = serde_json::to_string(r)
                    .map_err(|e| format!("serialize usage record: {e}"))?;
                line.push('\n');
                file.write_all(line.as_bytes())
                    .map_err(|e| format!("write usage tmp: {e}"))?;
            }
            file.flush().map_err(|e| format!("flush usage tmp: {e}"))?;
        }
        fs::rename(&tmp, &self.path).map_err(|e| format!("rotate usage log: {e}"))?;
        Ok(())
    }
}

impl Default for UsageStore {
    fn default() -> Self {
        Self::new()
    }
}

fn default_usage_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".shannon")
        .join("usage.jsonl")
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Build a [`UsageRecord`] from an engine Usage event, attributing the
/// current model/provider and timestamping "now". Kept here so the
/// `send_message` handler stays a thin call.
pub fn record_event(
    model: &str,
    provider: &str,
    input_tokens: u64,
    output_tokens: u64,
    cache_creation_tokens: u64,
    cache_read_tokens: u64,
    cost_usd: f64,
) -> UsageRecord {
    UsageRecord {
        timestamp_ms: now_ms(),
        model: model.to_string(),
        provider: provider.to_string(),
        input_tokens,
        output_tokens,
        cache_creation_tokens,
        cache_read_tokens,
        cost_usd,
    }
}

// --- Aggregation DTOs + command -------------------------------------------

/// Aggregated totals for one bucket (a model, a provider, or a day).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BucketTotals {
    pub label: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_tokens: u64,
    pub cache_read_tokens: u64,
    pub cost_usd: f64,
    pub requests: u64,
}

impl BucketTotals {
    fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            input_tokens: 0,
            output_tokens: 0,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
            cost_usd: 0.0,
            requests: 0,
        }
    }

    fn add(&mut self, r: &UsageRecord) {
        self.input_tokens += r.input_tokens;
        self.output_tokens += r.output_tokens;
        self.cache_creation_tokens += r.cache_creation_tokens;
        self.cache_read_tokens += r.cache_read_tokens;
        self.cost_usd += r.cost_usd;
        self.requests += 1;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageStats {
    pub days: u32,
    pub totals: BucketTotals,
    /// Sorted by total cost, descending.
    pub by_model: Vec<BucketTotals>,
    /// Sorted by total cost, descending.
    pub by_provider: Vec<BucketTotals>,
    /// Sorted chronologically (oldest first).
    pub by_day: Vec<BucketTotals>,
}

fn day_label(ms: u64) -> String {
    DateTime::<Utc>::from_timestamp_millis(ms as i64)
        .map(|dt| dt.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| ms.to_string())
}

fn aggregate_by(
    records: &[UsageRecord],
    key: impl Fn(&UsageRecord) -> String,
) -> Vec<BucketTotals> {
    let mut order: Vec<String> = Vec::new();
    let mut map: std::collections::HashMap<String, BucketTotals> =
        std::collections::HashMap::new();
    for r in records {
        let k = key(r);
        if !map.contains_key(&k) {
            order.push(k.clone());
        }
        map.entry(k).or_insert_with(|| BucketTotals::new("")).add(r);
    }
    order
        .into_iter()
        .map(|k| {
            let mut bucket = map.remove(&k).expect("bucket present");
            bucket.label = k;
            bucket
        })
        .collect()
}

/// Pure aggregation core: filter `records` to the last `days` (relative
/// to `now_ms_value`) and bucket by model / provider / day. Extracted
/// from the command so it is unit-testable without files, async, or
/// environment manipulation.
fn compute_stats(days: u32, records: &[UsageRecord], now_ms_value: u64) -> UsageStats {
    let days = days.clamp(1, 365);
    let cutoff_ms = now_ms_value.saturating_sub(u64::from(days) * 86_400_000);
    let filtered: Vec<&UsageRecord> = records
        .iter()
        .filter(|r| r.timestamp_ms >= cutoff_ms)
        .collect();

    let mut totals = BucketTotals::new("total");
    for r in &filtered {
        totals.add(r);
    }

    let owned: Vec<UsageRecord> = filtered.into_iter().cloned().collect();
    let mut by_model = aggregate_by(&owned, |r| r.model.clone());
    by_model.sort_by(|a, b| b.cost_usd.total_cmp(&a.cost_usd));

    let mut by_provider = aggregate_by(&owned, |r| r.provider.clone());
    by_provider.sort_by(|a, b| b.cost_usd.total_cmp(&a.cost_usd));

    let mut by_day = aggregate_by(&owned, |r| day_label(r.timestamp_ms));
    by_day.sort_by(|a, b| a.label.cmp(&b.label));

    UsageStats {
        days,
        totals,
        by_model,
        by_provider,
        by_day,
    }
}

/// Label used for spend that cannot be attributed to a specific model or
/// provider — today, only scheduled-routine runs (which execute engine-side
/// and carry no model/provider). Surfaced as its own bucket so the Usage
/// breakdowns stay consistent with the headline totals.
const SCHEDULED_LABEL: &str = "Scheduled tasks";

/// Map a scheduled-routine run onto a usage record. Scheduled runs track a
/// single lump `token_usage` (no input/output/cache split) and no
/// model/provider, so the lump is counted under `input_tokens` and both
/// attribution fields fall back to [`SCHEDULED_LABEL`]. Runs that tracked
/// neither cost nor tokens (e.g. never reached the accounting point) are
/// dropped.
fn scheduled_run_to_record(run: &ScheduledRun) -> Option<UsageRecord> {
    if run.cost_usd.is_none() && run.token_usage.is_none() {
        return None;
    }
    Some(UsageRecord {
        timestamp_ms: run.started_at.timestamp_millis().max(0) as u64,
        model: SCHEDULED_LABEL.to_string(),
        provider: SCHEDULED_LABEL.to_string(),
        input_tokens: run.token_usage.unwrap_or(0),
        output_tokens: 0,
        cache_creation_tokens: 0,
        cache_read_tokens: 0,
        cost_usd: run.cost_usd.unwrap_or(0.0),
    })
}

/// Read the usage ledger and aggregate by model / provider / day.
///
/// `days` is clamped to `[1, 365]`; records older than the window are
/// excluded. Scheduled-routine spend (engine-side — the desktop never sees
/// its Usage events) is merged in from the scheduled-runs store under
/// [`SCHEDULED_LABEL`]. Stateless beyond the on-disk stores (paths are
/// fixed), so it mirrors the billing commands rather than taking `AppState`.
#[tauri::command]
pub async fn get_usage_stats(days: u32) -> Result<UsageStats, String> {
    let now = now_ms();
    let mut records = UsageStore::new().load();

    // Fold scheduled-routine spend into the same aggregation. compute_stats
    // re-filters by the day window, so a slightly wider fetch here is
    // harmless. The usage and scheduled-runs stores are independent: if the
    // scheduled-runs store is unreadable we log it and proceed with chat
    // usage rather than failing the whole page.
    let days_clamped = days.clamp(1, 365);
    let now_dt =
        DateTime::<Utc>::from_timestamp_millis(now as i64).unwrap_or_else(Utc::now);
    let start = now_dt - Duration::days(days_clamped as i64);
    match ScheduledRunsStore::new().list_by_time_range(start, now_dt) {
        Ok(runs) => {
            for run in &runs {
                if let Some(r) = scheduled_run_to_record(run) {
                    records.push(r);
                }
            }
        }
        Err(e) => tracing::warn!(
            error = %e,
            "scheduled-runs store unreadable; usage page will omit scheduled spend"
        ),
    }

    Ok(compute_stats(days, &records, now))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(ts_ms: u64, model: &str, provider: &str, cost: f64) -> UsageRecord {
        UsageRecord {
            timestamp_ms: ts_ms,
            model: model.into(),
            provider: provider.into(),
            input_tokens: 100,
            output_tokens: 50,
            cache_creation_tokens: 10,
            cache_read_tokens: 5,
            cost_usd: cost,
        }
    }

    #[test]
    fn append_then_load_round_trips() {
        let tmp = tempfile::tempdir().unwrap();
        let store = UsageStore::with_path(tmp.path().join("usage.jsonl"));
        store
            .append(&rec(1, "claude-sonnet-4-6", "anthropic", 0.01))
            .unwrap();
        store.append(&rec(2, "gpt-4o", "openai", 0.02)).unwrap();

        let loaded = store.load();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].model, "claude-sonnet-4-6");
        assert_eq!(loaded[1].provider, "openai");
    }

    #[test]
    fn load_missing_file_is_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let store = UsageStore::with_path(tmp.path().join("nope.jsonl"));
        assert!(store.load().is_empty());
    }

    #[test]
    fn load_skips_unparseable_lines() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("usage.jsonl");
        fs::write(&path, "not json\n").unwrap();
        let store = UsageStore::with_path(path);
        store.append(&rec(9, "claude", "anthropic", 0.5)).unwrap();
        let loaded = store.load();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].timestamp_ms, 9);
    }

    #[test]
    fn aggregate_groups_and_sums() {
        let records = vec![
            rec(1, "a", "anthropic", 0.01),
            rec(2, "a", "anthropic", 0.03),
            rec(3, "b", "openai", 0.10),
        ];
        let by_model = aggregate_by(&records, |r| r.model.clone());
        let a = by_model.iter().find(|b| b.label == "a").unwrap();
        assert_eq!(a.requests, 2);
        assert_eq!(a.cost_usd, 0.04);
        assert_eq!(a.input_tokens, 200);
    }

    #[test]
    fn compute_stats_filters_by_day_window_and_sorts() {
        let now = 1_700_000_000_000u64;
        let day_ms = 86_400_000u64;
        let records = vec![
            rec(now, "a", "anthropic", 0.01),
            rec(now - 5 * day_ms, "b", "openai", 0.02),
            rec(now - 40 * day_ms, "c", "mistral", 1.0),
        ];

        let stats = compute_stats(30, &records, now);

        // 40-day-old record falls outside the 30-day window.
        assert_eq!(stats.days, 30);
        assert_eq!(stats.totals.requests, 2);
        assert!(!stats.by_model.iter().any(|b| b.label == "c"));
        // Sorted by cost desc: openai (0.02) before anthropic (0.01).
        assert_eq!(stats.by_model[0].label, "b");
        assert_eq!(stats.by_model[1].label, "a");
        assert_eq!(stats.by_provider[0].label, "openai");
    }

    #[test]
    fn compute_stats_clamps_extremes() {
        let now = 1_700_000_000_000u64;
        let records = vec![rec(now, "a", "anthropic", 0.01)];
        // days=0 clamps to 1 (keeps today's record); days=99_999 clamps to 365.
        assert_eq!(compute_stats(0, &records, now).days, 1);
        assert_eq!(compute_stats(99_999, &records, now).days, 365);
        assert_eq!(compute_stats(99_999, &records, now).totals.requests, 1);
    }

    #[test]
    fn compute_stats_day_buckets_are_chronological() {
        let now = 1_700_000_000_000u64;
        let day_ms = 86_400_000u64;
        let records = vec![
            rec(now, "a", "anthropic", 0.01),
            rec(now - 2 * day_ms, "a", "anthropic", 0.01),
            rec(now - day_ms, "a", "anthropic", 0.01),
        ];
        let stats = compute_stats(30, &records, now);
        assert_eq!(stats.by_day.len(), 3);
        // Oldest first.
        assert!(stats.by_day[0].label < stats.by_day[1].label);
        assert!(stats.by_day[1].label < stats.by_day[2].label);
    }

    #[test]
    fn day_label_is_utc_date() {
        // 2024-01-02T03:04:05Z = 1704169445000 ms.
        assert_eq!(day_label(1_704_169_445_000), "2024-01-02");
    }

    #[test]
    fn maybe_rotate_is_noop_below_threshold() {
        let tmp = tempfile::tempdir().unwrap();
        let store = UsageStore::with_path(tmp.path().join("usage.jsonl"));
        store
            .append(&rec(1, "claude", "anthropic", 0.01))
            .unwrap();
        store.maybe_rotate().unwrap();
        assert_eq!(store.load().len(), 1);
    }

    #[test]
    fn maybe_rotate_trims_large_ledger_to_keep_records() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("usage.jsonl");
        let store = UsageStore::with_path(path.clone());
        // Build a ledger well over ROTATE_AT_BYTES via raw lines, bypassing
        // append's own auto-rotate so maybe_rotate is exercised in isolation.
        let mut buf = String::new();
        for i in 0..80_000u64 {
            let r = rec(i, "claude", "anthropic", 0.01);
            buf.push_str(&serde_json::to_string(&r).unwrap());
            buf.push('\n');
        }
        fs::write(&path, &buf).unwrap();
        assert!(
            path.metadata().unwrap().len() > ROTATE_AT_BYTES,
            "precondition: ledger must exceed the rotation threshold"
        );

        store.maybe_rotate().unwrap();

        let loaded = store.load();
        assert_eq!(loaded.len(), KEEP_RECORDS);
        // The most recent KEEP_RECORDS are retained.
        assert_eq!(
            loaded.first().unwrap().timestamp_ms,
            80_000 - KEEP_RECORDS as u64
        );
        assert_eq!(loaded.last().unwrap().timestamp_ms, 79_999);
    }

    #[test]
    fn scheduled_run_maps_to_usage_record() {
        let mut run = ScheduledRun::start("t1", "Daily digest");
        run.cost_usd = Some(0.42);
        run.token_usage = Some(12_345);
        let r = scheduled_run_to_record(&run).expect("tracked run maps to a record");
        assert_eq!(r.model, SCHEDULED_LABEL);
        assert_eq!(r.provider, SCHEDULED_LABEL);
        assert_eq!(r.input_tokens, 12_345);
        assert_eq!(r.output_tokens, 0);
        assert_eq!(r.cache_creation_tokens, 0);
        assert_eq!(r.cache_read_tokens, 0);
        assert_eq!(r.cost_usd, 0.42);
        assert!(r.timestamp_ms > 0);
    }

    #[test]
    fn scheduled_run_without_tracking_is_dropped() {
        // start() yields a Running run with no cost/tokens recorded yet.
        let run = ScheduledRun::start("t2", "No-op");
        assert!(scheduled_run_to_record(&run).is_none());
    }
}
