//! Static HTML dashboard over eval run reports (§4.15 W2-M4).
//!
//! Reads every `runs/<run-id>/report.json` beneath an eval-runs root
//! ([`collect_reports`]) and renders a **self-contained** HTML page
//! ([`render_dashboard`]): inline CSS only, zero scripts, zero external
//! references — openable offline straight from disk or a `file://` URL.
//!
//! Two views:
//! 1. *By version* — the version×metric comparison matrix. One row per
//!    distinct `app_version`, aggregating its runs (pass rate, tokens,
//!    observed cost, trajectory-quality counters, failure-class tally).
//! 2. *Chronological* — one row per historical run in start order.
//!
//! The runner's report format is consumed read-only; nothing here writes
//! back into `crates/shannon-core/src/testing/eval_runner.rs` structures.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::testing::eval_runner::{EvalError, RunReport, load_report};

/// Directory name the rendered dashboard defaults to under the runs root's
/// parent (`<home>/eval/dashboard.html`).
pub const DASHBOARD_FILE_NAME: &str = "dashboard.html";

// ============================================================================
// Collection
// ============================================================================

/// Scan `<runs_root>/<dir>/report.json` for every subdirectory and load them
/// in deterministic order (`started_at_utc`, then `run_id`). Unreadable
/// entries are skipped with a warning so one malformed run cannot blind the
/// whole board.
pub fn collect_reports(runs_root: &Path) -> Result<Vec<RunReport>, EvalError> {
    let mut dirs: Vec<PathBuf> = match std::fs::read_dir(runs_root) {
        Ok(entries) => entries
            .filter_map(|entry| entry.ok().map(|e| e.path()))
            .filter(|p| p.join("report.json").is_file())
            .collect(),
        Err(e) => {
            return Err(EvalError::Config(format!(
                "cannot read runs root {}: {e}",
                runs_root.display()
            )));
        }
    };
    // Directory order is arbitrary on some filesystems; sort by name so the
    // later timestamp sort is at least stable platform-to-platform.
    dirs.sort();

    let mut reports: Vec<RunReport> = Vec::new();
    for dir in dirs {
        let path = dir.join("report.json");
        match load_report(&path) {
            Ok(report) => reports.push(report),
            Err(e) => {
                tracing::warn!(target: "shannon_core::signals", path = %path.display(), error = %e, "skipping unreadable eval report")
            }
        }
    }
    reports.sort_by(|a, b| (&a.started_at_utc, &a.run_id).cmp(&(&b.started_at_utc, &b.run_id)));
    Ok(reports)
}

// ============================================================================
// Aggregation
// ============================================================================

#[derive(Debug, Clone, Default, PartialEq)]
struct VersionRow {
    app_version: String,
    runs: usize,
    dry_runs: usize,
    tasks_total: usize,
    tasks_passed: usize,
    tokens_in: u64,
    tokens_out: u64,
    token_samples: usize,
    cost_usd: f64,
    cost_known_runs: usize,
    turns: u64,
    tool_calls: u64,
    loops: u64,
    invalid_calls: u64,
    permission_blocks: u64,
    failure_classes: BTreeMap<String, usize>,
    run_ids: Vec<String>,
}

impl VersionRow {
    fn absorb(&mut self, report: &RunReport) {
        self.app_version = report.app_version.clone();
        self.runs += 1;
        if report.dry_run {
            self.dry_runs += 1;
        }
        self.tasks_total += report.tasks_total;
        self.tasks_passed += report.tasks_passed;
        self.run_ids.push(report.run_id.clone());
        for record in &report.records {
            if let Some(metrics) = &record.metrics {
                self.tokens_in += metrics.tokens_in;
                self.tokens_out += metrics.tokens_out;
                self.token_samples += 1;
                if let Some(cost) = metrics.cost_usd {
                    self.cost_usd += cost;
                    self.cost_known_runs += 1;
                }
                self.turns += u64::from(metrics.turns);
                self.tool_calls += u64::from(metrics.tool_calls);
                self.loops += u64::from(metrics.loops);
                self.invalid_calls += u64::from(metrics.invalid_calls);
                self.permission_blocks += u64::from(metrics.permission_blocks);
            }
            if let Some(class) = &record.failure_class {
                *self.failure_classes.entry(class.clone()).or_default() += 1;
            }
        }
    }

    /// Pass rate as a percentage string; "-" with no data (honest unknown).
    fn pass_rate_display(&self) -> String {
        pct(self.tasks_passed, self.tasks_total)
    }
}

fn pct(numerator: usize, denominator: usize) -> String {
    if denominator == 0 {
        return "-".to_string();
    }
    format!("{:.1}%", numerator as f64 / denominator as f64 * 100.0)
}

fn money(value: f64) -> String {
    format!("${value:.4}")
}

fn aggregate_by_version(reports: &[RunReport]) -> Vec<VersionRow> {
    let mut by_version: BTreeMap<String, VersionRow> = BTreeMap::new();
    for report in reports {
        let key = if report.app_version.is_empty() {
            "(unknown)".to_string()
        } else {
            report.app_version.clone()
        };
        by_version.entry(key).or_default().absorb(report);
    }
    by_version.into_values().collect()
}

// ============================================================================
// Rendering
// ============================================================================

const STYLE: &str = "\
body{font-family:ui-monospace,Menlo,Consolas,monospace;margin:1.5rem;background:#111;color:#ddd}
h1{font-size:1.3rem} h2{font-size:1.05rem;margin-top:2rem}
table{border-collapse:collapse;width:auto;margin-top:.75rem}
th,td{border:1px solid #444;padding:.35rem .6rem;text-align:right}
th:first-child,td:first-child{text-align:left}
th{background:#1d1d1d;position:sticky;top:0}
tr:hover td{background:#181818}
td.classes{text-align:left;font-size:.85rem;color:#aaa}
footer{margin-top:2rem;color:#666;font-size:.8rem}";

/// Minimal HTML escaping for interpolated report fields.
fn esc(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Render the complete static dashboard. Deterministic for identical inputs
/// (sorted maps everywhere) — safe to diff between generations.
pub fn render_dashboard(runs_root_display: &str, reports: &[RunReport]) -> String {
    let mut html = String::with_capacity(16 << 10);
    html.push_str("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n");
    html.push_str("<title>Shannon Eval Trends</title>\n");
    html.push_str(&format!("<style>{STYLE}</style>\n</head>\n<body>\n"));
    html.push_str("<h1>Shannon Eval Trends — version comparison</h1>\n");

    if reports.is_empty() {
        html.push_str(
            "<p>No run reports found yet. Generate some with \
             <code>cargo run -p shannon-core --example eval_runner -- --tasks tests/eval/tasks</code>.\
             </p>\n</body>\n</html>\n",
        );
        return html;
    }

    // ── View 1: version × metric matrix ────────────────────────────────
    html.push_str("<h2>Aggregated by engine version</h2>\n<table id=\"by-version\">\n<thead><tr>");
    for header in [
        "version",
        "runs",
        "pass rate",
        "passed/total",
        "tokens/task (in/out)",
        "cost (observed)",
        "turns",
        "tool calls",
        "loops",
        "invalid calls",
        "perm blocks",
        "failure classes",
    ] {
        html.push_str(&format!("<th>{header}</th>"));
    }
    html.push_str("</tr></thead>\n<tbody>\n");
    for row in aggregate_by_version(reports) {
        let tokens_per_task = if row.token_samples > 0 && row.tasks_total > 0 {
            format!(
                "{}/{}",
                row.tokens_in / row.tasks_total.max(1) as u64,
                row.tokens_out / row.tasks_total.max(1) as u64
            )
        } else {
            "-".to_string()
        };
        let cost = if row.cost_known_runs > 0 {
            money(row.cost_usd)
        } else {
            "-".to_string()
        };
        let classes = if row.failure_classes.is_empty() {
            "-".to_string()
        } else {
            row.failure_classes
                .iter()
                .map(|(class, count)| format!("{}={}", esc(class), count))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let mode_note = if row.dry_runs > 0 {
            format!(" ({} dry-run)", row.dry_runs)
        } else {
            String::new()
        };
        html.push_str(&format!(
            "<tr><td>{}</td><td>{}{}</td><td>{}</td><td>{}/{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td class=\"classes\">{}</td></tr>\n",
            esc(&row.app_version),   // 1 version
            row.runs,                // 2 runs
            mode_note,               //    mode note rides the same cell
            row.pass_rate_display(), // 3 pass rate
            row.tasks_passed,        // 4 passed
            row.tasks_total,         //   / total
            tokens_per_task,         // 5 tokens/task
            cost,                    // 6 cost
            row.turns,               // 7 turns
            row.tool_calls,          // 8 tool calls
            row.loops,               // 9 loops
            row.invalid_calls,       // 10 invalid calls
            row.permission_blocks,   // 11 perm blocks
            classes,                 // 12 failure classes
        ));
    }
    html.push_str("</tbody>\n</table>\n");

    // ── View 2: chronological run sequence ─────────────────────────────
    html.push_str("<h2>Historical runs (oldest first)</h2>\n<table id=\"by-time\">\n<thead><tr>");
    for header in [
        "started (UTC)",
        "run id",
        "version",
        "mode",
        "passed/total",
        "limited",
        "timed out",
        "spawn errors",
        "cost",
        "failure classes",
    ] {
        html.push_str(&format!("<th>{header}</th>"));
    }
    html.push_str("</tr></thead>\n<tbody>\n");
    for report in reports {
        let tally: Vec<String> = report
            .failure_class_tally()
            .into_iter()
            .map(|(class, count)| format!("{class}={count}"))
            .collect();
        let cost: f64 = report
            .records
            .iter()
            .filter_map(|r| r.metrics.as_ref().and_then(|m| m.cost_usd))
            .sum();
        html.push_str(&format!(
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}/{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td class=\"classes\">{}</td></tr>\n",
            esc(&report.started_at_utc),
            esc(&report.run_id),
            esc(if report.app_version.is_empty() { "(unknown)" } else { &report.app_version }),
            if report.dry_run { "dry-run" } else { "real" },
            report.tasks_passed,
            report.tasks_total,
            report.tasks_limited,
            report.tasks_timed_out,
            report.tasks_spawn_errors,
            if cost > 0.0 { money(cost) } else { "-".to_string() },
            if tally.is_empty() { "-".to_string() } else { tally.join(", ") },
        ));
    }
    html.push_str("</tbody>\n</table>\n");

    html.push_str(&format!(
        "<footer>generated {} · source: {} · static offline page (no scripts, no network)</footer>\n",
        chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        esc(runs_root_display),
    ));
    html.push_str("</body>\n</html>\n");
    html
}

/// Convenience used by the example: collect + render + write to `out_path`.
/// Returns the number of runs rendered and bytes written.
pub fn generate(runs_root: &Path, out_path: &Path) -> Result<(usize, usize), EvalError> {
    let reports = collect_reports(runs_root)?;
    let count = reports.len();
    let html = render_dashboard(&runs_root.display().to_string(), &reports);
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| EvalError::Config(format!("cannot create {}: {e}", parent.display())))?;
    }
    std::fs::write(out_path, &html)
        .map_err(|e| EvalError::Config(format!("cannot write {}: {e}", out_path.display())))?;
    Ok((count, html.len()))
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::testing::eval_metrics::{MetricSource, RunAnchor, TaskMetrics};
    use crate::testing::eval_runner::{RecordedRuleOutcome, RunStatus, TaskRunRecord};

    /// Build a minimal persisted-looking report. Timestamps encode the
    /// ordering; ids keep ties resolvable.
    fn fixture_report(
        run_id: &str,
        version: &str,
        passed: usize,
        failed_class: Option<&str>,
    ) -> RunReport {
        let total = passed + usize::from(failed_class.is_some());
        let records = (0..total)
            .map(|i| TaskRunRecord {
                id: format!("task_{i}"),
                tier: "edit".to_string(),
                horizon: "short".to_string(),
                status: if failed_class.is_some() && i == 0 {
                    RunStatus::Failed
                } else {
                    RunStatus::Passed
                },
                passed: !(failed_class.is_some() && i == 0),
                duration_ms: 900,
                exit_code: Some(0),
                turns: 3,
                tokens_in: 400,
                tokens_out: 200,
                total_tokens: 600,
                session_id: None,
                violations: vec![],
                soft_flags: vec![],
                rule_outcomes: vec![RecordedRuleOutcome {
                    rule: "verify_exit_zero".to_string(),
                    passed: failed_class.is_none() || i != 0,
                    details: vec![],
                    soft_flags: vec![],
                }],
                trajectory_tools: vec!["Read".to_string()],
                metrics: Some(TaskMetrics {
                    tokens_in: 400,
                    tokens_out: 200,
                    cache_creation_tokens: 10,
                    cache_read_tokens: 5,
                    cost_usd: Some(0.0025),
                    turns: 3,
                    tool_calls: 4,
                    wall_clock_ms: Some(900),
                    loops: 0,
                    loop_max_streak: 0,
                    invalid_calls: if i == 0 { 1 } else { 0 },
                    permission_blocks: 0,
                    source: MetricSource::EventsLog,
                }),
                failure_class: if failed_class.is_some() && i == 0 {
                    failed_class.map(str::to_string)
                } else {
                    None
                },
                failure_evidence: vec![],
                over_expected: None,
                anchor: RunAnchor::default(),
                workspace: PathBuf::from("/tmp"),
                task_file: PathBuf::from("/tmp/task.toml"),
            })
            .collect();
        RunReport {
            directive: None,
            run_id: run_id.to_string(),
            started_at_utc: format!(
                "2026-08-2{}T00:00:00+00:00",
                if run_id == "run_b" { 7 } else { 6 }
            ),
            dry_run: false,
            shannon_bin: "shannon".to_string(),
            anchors: RunAnchor::default(),
            tasks_total: total,
            tasks_passed: passed,
            tasks_failed: total - passed,
            tasks_limited: 0,
            tasks_timed_out: 0,
            tasks_spawn_errors: 0,
            metrics_source: "events_jsonl".to_string(),
            app_version: version.to_string(),
            failure_rules_fingerprint: "f".repeat(16),
            records,
        }
    }

    #[test]
    fn collect_reports_sorts_chronologically_and_skips_garbage() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for (name, report) in [
            ("20260827-b", fixture_report("run_b", "0.11.0", 2, None)),
            (
                "20260826-a",
                fixture_report("run_a", "0.10.0", 1, Some("assertion_mismatch")),
            ),
        ] {
            let d = root.join(name);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(
                d.join("report.json"),
                serde_json::to_string(&report).unwrap(),
            )
            .unwrap();
        }
        // Garbage sibling must be skipped silently.
        let bad = root.join("20260825-garbage");
        std::fs::create_dir_all(bad.join("report.json").parent().unwrap()).unwrap();
        std::fs::write(bad.join("report.json"), "{not json").unwrap();

        let collected = collect_reports(root).unwrap();
        assert_eq!(collected.len(), 2, "garbage skipped");
        assert_eq!(collected[0].run_id, "run_a", "oldest first");
        assert_eq!(collected[1].app_version, "0.11.0");
    }

    #[test]
    fn render_is_static_offline_html_with_both_views() {
        let reports = vec![
            fixture_report("run_a", "0.10.0", 1, Some("assertion_mismatch")),
            fixture_report("run_b", "0.11.0", 2, None),
        ];
        let html = render_dashboard("/tmp/runs", &reports);

        // Golden structural anchors — both views present in order.
        let v_pos = html
            .find("<table id=\"by-version\"")
            .expect("version matrix table");
        let t_pos = html
            .find("<table id=\"by-time\"")
            .expect("chronology table");
        assert!(v_pos < t_pos);

        // Version rows carry aggregates (pass rate, counters, class tally).
        assert!(html.contains("100.0%"));
        assert!(html.contains("assertion_mismatch=1"));
        assert!(html.contains("0.11.0"));

        // Chronological rows include both run ids in start order.
        let ra = html.find("run_a").unwrap();
        let rb = html.find("run_b").unwrap();
        assert!(ra < rb);

        // Offline guarantees: no scripts, no external references of any kind.
        assert!(!html.contains("<script"));
        assert!(!html.contains("http://"));
        assert!(!html.contains("https://"));
        assert!(!html.contains("src="));
        assert!(html.ends_with("</html>\n"));
    }

    #[test]
    fn empty_runs_render_a_helpful_placeholder_page() {
        let html = render_dashboard("/tmp/none", &[]);
        assert!(html.contains("No run reports found"));
        assert!(!html.contains("<script"));
        assert!(html.ends_with("</html>\n"));
    }

    #[test]
    fn generation_writes_a_file_and_round_trips_count() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("runs");
        let d = root.join("20260826-a");
        std::fs::create_dir_all(&d).unwrap();
        let report = fixture_report("run_a", "0.10.0", 3, None);
        std::fs::write(
            d.join("report.json"),
            serde_json::to_string(&report).unwrap(),
        )
        .unwrap();

        let out = dir.path().join("eval").join(DASHBOARD_FILE_NAME);
        let (count, bytes) = generate(&root, &out).unwrap();
        assert_eq!(count, 1);
        assert!(bytes > 500);
        let text = std::fs::read_to_string(&out).unwrap();
        assert!(text.contains("0.10.0"));
    }
}
