//! Suite-level guarantees for `tests/eval/tasks/*.toml` and the eval runner
//! pipeline (§4.4): task inventory integrity, tier distribution, full-suite
//! dry-run execution, and cross-run report stability.

use shannon_core::testing::eval_runner::{
    EvalOptions, EvalTier, RunStatus, compare_reports, parse_tasks_dir, run_suite,
};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn tasks_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crate parent")
        .parent()
        .expect("repo root")
        .join("tests")
        .join("eval")
        .join("tasks")
}

/// ① The shipped suite is exactly 20 well-formed tasks.
#[test]
fn suite_has_twenty_wellformed_tasks() {
    let dir = tasks_dir();
    let tasks = parse_tasks_dir(&dir).expect("suite parses");
    assert_eq!(tasks.len(), 20, "expected 20 tasks in {}", dir.display());

    let ids: Vec<_> = tasks.iter().map(|t| t.id.as_str()).collect();
    let mut sorted = ids.clone();
    sorted.sort();
    assert_eq!(ids, sorted, "task files must sort deterministically");
    let unique = sorted
        .iter()
        .collect::<std::collections::HashSet<_>>()
        .len();
    assert_eq!(unique, 20, "task ids must be unique");

    for task in &tasks {
        assert!(
            task.validate().is_empty(),
            "{} has structural problems: {:?}",
            task.id,
            task.validate()
        );
        // Every assertion vocabulary entry must be honest: no placeholder
        // `max_duration_ms` no-op rules sneaking into the suite.
        for rule in &task.verify.rules {
            let rendered = format!("{rule:?}");
            assert!(
                !rendered.contains("MaxDurationMs"),
                "{} uses a no-op rule",
                task.id
            );
            assert!(
                !rendered.contains("CostBelow"),
                "{} relies on unavailable USD costs",
                task.id
            );
        }
    }
}

/// ② Tier distribution matches the plan: read 3 / edit 5 / search 3 /
/// multi_step 6 / recovery 3.
#[test]
fn tier_distribution_matches_plan() {
    let tasks = parse_tasks_dir(&tasks_dir()).expect("suite parses");
    let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
    for task in &tasks {
        *counts.entry(task.tier.as_str()).or_default() += 1;
    }
    assert_eq!(counts.get("read"), Some(&3));
    assert_eq!(counts.get("edit"), Some(&5));
    assert_eq!(counts.get("search"), Some(&3));
    assert_eq!(counts.get("multi_step"), Some(&6));
    assert_eq!(counts.get("recovery"), Some(&3));

    // File naming mirrors tier labels for easy eyeballing.
    for task in &tasks {
        let prefix = match task.tier {
            EvalTier::Read => "read_",
            EvalTier::Edit => "edit_",
            EvalTier::Search => "search_",
            EvalTier::MultiStep => "multi_",
            EvalTier::Recovery => "rec_",
        };
        assert!(
            task.id.starts_with(prefix),
            "id {} does not match its tier {}",
            task.id,
            task.tier.as_str()
        );
    }
}

/// Every single task passes its own dry-run rehearsal in isolation — the
/// harness regression fence: if this breaks, either a task drifted from its
/// stub script or the runner changed semantics.
#[test]
fn every_task_passes_dry_run_individually() {
    let tasks = parse_tasks_dir(&tasks_dir()).expect("suite parses");
    let tmp = tempfile::TempDir::new().expect("out root");
    let options = EvalOptions {
        bin_path: None,
        dry_run: true,
        out_dir_override: Some(tmp.path().to_path_buf()),
        failure_rules: None,
        instruction_directive: None,
    };

    let mut violations = Vec::new();
    for task in &tasks {
        let (report, _) =
            run_suite(std::slice::from_ref(task), &options).expect("single-task suite");
        let record = &report.records[0];
        if record.status != RunStatus::Passed {
            violations.push(format!("{}: {:?}", record.id, record.violations));
        }
    }
    assert!(violations.is_empty(), "failing rehearsals: {violations:#?}");
}

/// ③ Two consecutive full dry-run suites produce diff-stable reports
/// (statuses, budgets, trajectories); only timestamps/durations/paths move.
#[test]
fn consecutive_runs_stay_digest_stable() {
    let tasks = parse_tasks_dir(&tasks_dir()).expect("suite parses");
    let out_a = tempfile::TempDir::new().expect("run A");
    let out_b = tempfile::TempDir::new().expect("run B");

    let options_a = EvalOptions {
        bin_path: None,
        dry_run: true,
        out_dir_override: Some(out_a.path().to_path_buf()),
        failure_rules: None,
        instruction_directive: None,
    };
    let options_b = EvalOptions {
        bin_path: None,
        dry_run: true,
        out_dir_override: Some(out_b.path().to_path_buf()),
        failure_rules: None,
        instruction_directive: None,
    };

    let (report_a, run_dir_a) = run_suite(&tasks, &options_a).expect("suite A");
    let (report_b, _) = run_suite(&tasks, &options_b).expect("suite B");

    assert_eq!(report_a.tasks_total, 20);
    assert_eq!(
        report_a
            .records
            .iter()
            .filter(|r| r.status == RunStatus::Passed)
            .count(),
        20,
        "full rehearsal must be green"
    );

    // Structural stability across runs...
    assert_eq!(report_a.stable_digest(), report_b.stable_digest());
    assert!(compare_reports(&report_a, &report_b).starts_with("STABLE:"));

    // ...and on-disk reports load back to identical digests.
    let reloaded_a: shannon_core::testing::eval_runner::RunReport = serde_json::from_str(
        &std::fs::read_to_string(run_dir_a.join("report.json")).expect("read report.json"),
    )
    .expect("deserialize report.json");
    assert_eq!(reloaded_a.stable_digest(), report_a.stable_digest());

    // Both runs materialized their per-task evidence directories.
    for task_id in ["read_01", "edit_03", "search_02", "multi_04", "rec_03"] {
        assert!(
            run_dir_a.join(task_id).join("workspace").exists(),
            "missing evidence dir for {task_id}"
        );
        assert!(run_dir_a.join(task_id).join("stream.ndjson").exists());
    }
}

// ── §4.7 W2-M2: metrics, failure classification, version comparison ────

/// ① 20 题全跑指标字段完整: every dry-run rehearsal row carries a complete
/// metrics blob (zero-missing field contract), stamped `derived_stream`, with
/// provenance + per-task metrics.json persisted beside each workspace.
#[test]
fn full_suite_metrics_are_complete_for_every_task() {
    let tasks = parse_tasks_dir(&tasks_dir()).expect("suite parses");
    let out_root = tempfile::TempDir::new().expect("out root");
    let options = EvalOptions {
        bin_path: None,
        dry_run: true,
        out_dir_override: Some(out_root.path().to_path_buf()),
        failure_rules: None,
        instruction_directive: None,
    };

    let (report, run_dir) = run_suite(&tasks, &options).expect("suite runs");

    // Report-level provenance & version fields.
    assert_eq!(report.metrics_source, "derived_stream");
    assert_eq!(report.app_version, env!("CARGO_PKG_VERSION"));
    assert!(
        !report.failure_rules_fingerprint.is_empty(),
        "rule fingerprint anchors the report to a rule-table version"
    );

    assert_eq!(report.records.len(), 20);
    for record in &report.records {
        let metrics = record.metrics.as_ref().unwrap_or_else(|| {
            panic!(
                "{} must carry §4.7 metrics (violations: {:?})",
                record.id, record.violations
            )
        });
        assert_eq!(
            metrics.source,
            shannon_core::testing::eval_metrics::MetricSource::DerivedStream
        );
        assert_eq!(
            shannon_core::testing::eval_metrics::missing_fields(metrics),
            Vec::<&'static str>::new(),
            "{} has missing metric fields",
            record.id
        );
        assert!(metrics.tool_calls >= 1);
        assert_eq!(metrics.cost_usd, None, "dry run honestly reports no cost");
        assert!(
            record.failure_class.is_none(),
            "green rehearsals classify nothing"
        );
    }

    // Per-task evidence bundle gained metrics.json marking completeness.
    for task_id in ["read_01", "edit_03", "search_02", "multi_04", "rec_01"] {
        let raw =
            std::fs::read_to_string(run_dir.join(task_id).join("metrics.json")).expect("metrics");
        assert!(
            raw.contains("\"metrics_complete\": true"),
            "{task_id}: {raw}"
        );
    }

    // Markdown carries the cost matrix + classification blocks.
    let md = report.render_markdown();
    assert!(md.contains("## Cost & trajectory matrix"));
    assert!(md.contains("## Failure classification"));
    assert!(md.contains("derived_stream"));

    // Round-trip keeps the new fields.
    let reloaded: shannon_core::testing::eval_runner::RunReport = serde_json::from_str(
        &std::fs::read_to_string(run_dir.join("report.json")).expect("report.json"),
    )
    .expect("deserialize");
    assert_eq!(reloaded.metrics_source, report.metrics_source);
    assert_eq!(reloaded.records[0].failure_class, None);
}

/// ③ 版本对比字段可与上一 run diff 出报告: a degraded second run is compared
/// against the baseline; the diff enumerates meta-version rows and per-task
/// metric/class deltas.
#[test]
fn version_comparison_diff_between_runs_reports_deltas() {
    let tasks = parse_tasks_dir(&tasks_dir()).expect("suite parses");
    let out = tempfile::TempDir::new().expect("out root");
    let options = |dir: &std::path::Path| EvalOptions {
        bin_path: None,
        dry_run: true,
        out_dir_override: Some(dir.to_path_buf()),
        failure_rules: None,
        instruction_directive: None,
    };

    // Baseline: all green.
    let (baseline, _) = run_suite(&tasks, &options(out.path())).expect("baseline");

    // Degraded twin: force one task past its token budget.
    let mut drifted = tasks.clone();
    if let Some(task) = drifted.iter_mut().find(|t| t.id == "read_01") {
        task.limits.max_tokens = Some(1);
    }
    let (degraded, _) = run_suite(&drifted, &options(out.path())).expect("degraded");

    let diff = compare_reports(&baseline, &degraded);
    assert!(diff.starts_with("UNSTABLE"), "{diff}");
    // Meta/version anchor rows are always enumerated on drift.
    assert!(diff.contains("[meta]"), "{diff}");
    assert!(diff.contains("app_version ="), "{diff}");
    assert!(diff.contains("failure_rules_fingerprint ="), "{diff}");
    assert!(diff.contains("metrics_source ="), "{diff}");
    // Per-task deltas expose the failure-class transition (metrics rows are
    // printed too, but this drift only moves status/violations/class).
    assert!(diff.contains("[read_01] read"), "{diff}");
    assert!(
        diff.contains("status: \"passed\" -> \"token_limit\""),
        "{diff}"
    );
    assert!(
        diff.contains("failure_class: null -> \"timeout_or_limit\""),
        "{diff}"
    );

    let degraded_record = degraded
        .records
        .iter()
        .find(|r| r.id == "read_01")
        .expect("row");
    assert_eq!(
        degraded_record.failure_class.as_deref(),
        Some("timeout_or_limit"),
        "token ceiling routes through the ⑤ limit class"
    );
    assert!(
        degraded_record
            .failure_evidence
            .iter()
            .any(|line| line.contains("status equals token_limit"))
    );
}
