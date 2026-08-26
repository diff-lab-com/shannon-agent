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
    };
    let options_b = EvalOptions {
        bin_path: None,
        dry_run: true,
        out_dir_override: Some(out_b.path().to_path_buf()),
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
