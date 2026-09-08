//! Cross-run aggregation with flaky isolation (design §4③④).
//!
//! [`super::eval_runner`] answers "how did ONE run go"; this module answers
//! "what does the suite look like across n runs of the same thing". It is
//! purely a read-side projection over persisted [`RunReport`]s — it never
//! executes anything and it does not touch [`RunReport::stable_digest`]
//! (flaky is an aggregate-layer concept; single-run digests stay
//! byte-compatible with every report written before this module existed).
//!
//! ## Verdicts
//!
//! Every task observed in at least one run lands in exactly one bucket:
//!
//! - **stable_pass** — resolved (passed) in every contributing run;
//! - **flaky** — resolved in some but not all runs (`0 < k < n`) — the
//!   outcome flipped and the task is quarantined: it leaves the pass-rate
//!   main conclusion and is listed separately with its per-run statuses
//!   (诚实呈现噪声而不隐藏: the noise is shown, never averaged in);
//! - **stable_fail** — resolved in no contributing run.
//!
//! A single-run aggregation (n=1) can never flag flaky — one observation
//! carries zero noise information — but the buckets are still emitted,
//! qualified as single observations by the report preamble.
//!
//! ## Attribution guards
//!
//! The ATTRIBUTE-SPLIT discipline of [`super::eval_runner::compare_reports`]
//! carries over: when the runs did not measure the same thing (model,
//! provider, profile digest, failure-rule table, or A/B directive differ),
//! the aggregation is refused — bucket/intervals stay empty, only raw
//! per-run numbers are listed. Non-passing limit classes (timeout,
//! turn/token limit, spawn error) count as "not resolved"; the statuses
//! column keeps the evidence visible so a human can attribute a flip to the
//! harness rather than the model (design §4④: 只怀疑不定罪).

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::eval_metrics::RunAnchor;
use super::eval_runner::{EvalError, RunReport, TaskRunRecord, load_report};

/// Quarantine bucket of one task's cross-run outcome pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AggregateBucket {
    /// Resolved in every contributing run.
    StablePass,
    /// Resolved in some but not all runs — outcome flips across runs.
    Flaky,
    /// Resolved in no contributing run.
    StableFail,
}

impl AggregateBucket {
    /// Lowercase spelling used in reports and JSON.
    pub fn as_str(self) -> &'static str {
        match self {
            AggregateBucket::StablePass => "stable_pass",
            AggregateBucket::Flaky => "flaky",
            AggregateBucket::StableFail => "stable_fail",
        }
    }
}

/// One task's verdict across the aggregated runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskAggregate {
    /// Task id (`read_01`, `edit_04`, …).
    pub id: String,
    /// Suite tier of the task (taken from the first contributing run).
    pub tier: String,
    /// Status spellings per contributing run, oldest run first.
    pub statuses: Vec<String>,
    /// Run ids backing `statuses` (a run that never recorded the task does
    /// not contribute — per-task `n` can be smaller than the suite's run
    /// count when task sets drifted).
    pub contributing_runs: Vec<String>,
    /// Contributing runs in which the task resolved (passed).
    pub resolved_k: usize,
    /// Contributing runs.
    pub n: usize,
    /// `0 < resolved_k < n` — the outcome flipped. Never set when `n <= 1`.
    pub flaky: bool,
    /// Quarantine bucket derived from `(resolved_k, n)`.
    pub bucket: AggregateBucket,
}

/// One refused attribution dimension: the label plus the value observed in
/// each run (run order) — the aggregate-layer rendering of
/// [`super::eval_runner::compare_reports`]'s ATTRIBUTE-SPLIT lines.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AttributeSplit {
    /// Dimension name (`model_id`, `provider`, `profile_digest`,
    /// `failure_rules_fingerprint`, `directive`).
    pub dimension: String,
    /// Per-run values, oldest run first.
    pub values: Vec<String>,
}

/// Cross-run aggregation over same-anchor runs — the n=k companion of a
/// [`RunReport`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AggregateReport {
    /// Run ids in aggregation order (oldest first).
    pub run_ids: Vec<String>,
    /// Number of aggregated runs.
    pub n_runs: usize,
    /// Suite-level anchor (all runs agreed; unknown when refused).
    pub anchors: RunAnchor,
    /// Failure-rule fingerprint shared by every run.
    pub failure_rules_fingerprint: String,
    /// Engine version per run (run order) — surfaced, never blocked.
    pub app_versions: Vec<String>,
    /// Non-empty ⇒ ATTRIBUTE-SPLIT: the runs did not measure the same thing
    /// and all aggregation verdicts below are withheld.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attribute_splits: Vec<AttributeSplit>,
    /// Distinct task ids observed across the runs.
    pub tasks_total: usize,
    /// Per-task verdicts, sorted by id.
    pub tasks: Vec<TaskAggregate>,
    /// Task ids resolved in every contributing run.
    pub stable_pass: Vec<String>,
    /// The flaky quarantine bucket (design §4④).
    pub flaky_tasks: Vec<String>,
    /// Task ids resolved in no contributing run.
    pub stable_fail: Vec<String>,
    /// True when every run recorded the identical task-id set.
    pub task_set_consistent: bool,
    /// Whole-suite resolved fraction per run (run order; `None` for a run
    /// that recorded no tasks). Raw numbers — listed even on refusal.
    pub per_run_pass_rates: Vec<Option<f64>>,
    /// `[min, max]` suite pass rate across runs — the noisy view, context
    /// only. `None` when withheld: refusal, `n < 2`, or differing task sets
    /// (denominators would not be comparable).
    pub pass_rate_interval: Option<[f64; 2]>,
    /// The citable main conclusion: `[min, max]` pass rate over the STABLE
    /// tasks (`stable_pass` + `stable_fail` — every non-flaky task) across
    /// runs. Flaky tasks are excluded from the denominator and listed
    /// separately, so the interval measures capability, not noise.
    pub stable_pass_rate_interval: Option<[f64; 2]>,
}

impl AggregateReport {
    /// True when the ATTRIBUTE-SPLIT guard refused the aggregation.
    pub fn is_refused(&self) -> bool {
        !self.attribute_splits.is_empty()
    }

    /// Human-readable companion of `aggregate.json`.
    pub fn render_markdown(&self) -> String {
        let mut md = String::new();
        if self.is_refused() {
            md.push_str("# Shannon Eval Aggregate — REFUSED (ATTRIBUTE-SPLIT)\n\n");
            md.push_str(&format!("- Runs: {}\n", self.run_ids.join(" → ")));
            md.push_str(
                "ATTRIBUTE-SPLIT: attribution dimensions differ across runs — \
                 aggregation verdict withheld, raw numbers only\n",
            );
            for split in &self.attribute_splits {
                md.push_str(&format!(
                    "  {dimension}: {values}\n",
                    dimension = split.dimension,
                    values = split.values.join(" -> ")
                ));
            }
            md.push_str(&format!(
                "\nPer-run pass rates (numbers still listed): {}\n",
                render_rates(&self.per_run_pass_rates)
            ));
            md.push_str(
                "\n(no cross-run aggregation: align anchors and the rule table, \
                 then re-aggregate)\n",
            );
            return md;
        }

        md.push_str(&format!(
            "# Shannon Eval Aggregate — {} run{}\n\n",
            self.n_runs,
            if self.n_runs == 1 { "" } else { "s" }
        ));
        md.push_str(&format!("- Runs: {}\n", self.run_ids.join(" → ")));
        md.push_str(&format!(
            "- Anchor: model={model} · provider={provider} · profile={profile}\n",
            model = self.anchors.model_id.as_deref().unwrap_or("-"),
            provider = self.anchors.provider.as_deref().unwrap_or("-"),
            profile = self.anchors.profile_digest.as_deref().unwrap_or("-"),
        ));
        md.push_str(&format!(
            "- Rules fingerprint: {}\n",
            self.failure_rules_fingerprint
        ));
        md.push_str(&format!(
            "- Engine versions: {}\n",
            if self.app_versions.is_empty() {
                "-".to_string()
            } else {
                self.app_versions.join(", ")
            }
        ));
        md.push_str(&format!(
            "- Task set consistent: {}\n",
            if self.task_set_consistent {
                "yes"
            } else {
                "no"
            }
        ));

        md.push_str("\n## Suite summary\n\n");
        md.push_str("| bucket | tasks |\n|---|---|\n");
        md.push_str(&format!(
            "| stable_pass | {} |\n| flaky | {} |\n| stable_fail | {} |\n",
            self.stable_pass.len(),
            self.flaky_tasks.len(),
            self.stable_fail.len()
        ));
        if self.n_runs < 2 {
            md.push_str(
                "\n- n=1: single-run aggregation — buckets are single observations; \
                 no flaky information exists yet (repeat the run to measure noise).\n",
            );
        } else if !self.flaky_tasks.is_empty() {
            md.push_str(
                "\n- Pass-rate conclusions cite the stable scope only; flaky tasks \
                 are quarantined below (suspected, not convicted).\n",
            );
        }

        md.push_str(&format!(
            "\n- pass_rate interval (all tasks, noisy view): {}\n\
             - stable pass-rate interval (main conclusion — cite this): {}\n",
            interval_note(
                self.pass_rate_interval,
                self.n_runs,
                self.task_set_consistent
            ),
            interval_note(
                self.stable_pass_rate_interval,
                self.n_runs,
                self.task_set_consistent
            ),
        ));

        md.push_str("\n## Per-task verdicts (resolved k/n)\n\n");
        md.push_str("| id | tier | k/n | bucket | statuses (run order) |\n");
        md.push_str("|---|---|---|---|---|\n");
        for task in &self.tasks {
            md.push_str(&format!(
                "| {} | {} | {}/{} | {} | {} |\n",
                task.id,
                task.tier,
                task.resolved_k,
                task.n,
                task.bucket.as_str(),
                task.statuses.join(", ")
            ));
        }

        if !self.flaky_tasks.is_empty() {
            md.push_str("\n## Flaky tasks (quarantined from the capability score)\n\n");
            for task in &self.tasks {
                if task.bucket != AggregateBucket::Flaky {
                    continue;
                }
                md.push_str(&format!(
                    "- {}: {}/{} — {}\n",
                    task.id,
                    task.resolved_k,
                    task.n,
                    task.statuses.join(", ")
                ));
            }
        }
        md
    }
}

/// Render a per-run rate list for raw-number display (`0.667, 0.667, 0.333`).
fn render_rates(rates: &[Option<f64>]) -> String {
    rates
        .iter()
        .map(|rate| match rate {
            Some(value) => format!("{value:.3}"),
            None => "n/a".to_string(),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Interval line: the value when computable, otherwise the specific reason
/// it is withheld (honesty: a missing interval always says why).
fn interval_note(interval: Option<[f64; 2]>, n_runs: usize, task_set_consistent: bool) -> String {
    match interval {
        Some([lo, hi]) => format!("[{lo:.3}, {hi:.3}] · n={n_runs}"),
        None if n_runs < 2 => "withheld (n<2 — no cross-run spread to bound)".to_string(),
        None if !task_set_consistent => "withheld (task sets differ across runs)".to_string(),
        None => "withheld (no comparable observations)".to_string(),
    }
}

/// `[min, max]` over a non-empty rate list (partial-order-safe folds; no
/// `unwrap` on `partial_cmp`).
fn min_max(rates: &[f64]) -> [f64; 2] {
    [
        rates.iter().copied().fold(f64::INFINITY, f64::min),
        rates.iter().copied().fold(f64::NEG_INFINITY, f64::max),
    ]
}

/// Values of one anchor dimension across the ordered runs, `(unknown)`
/// rendered for missing dimensions — matching
/// [`super::eval_runner::compare_reports`]'s honesty convention.
fn anchor_values(ordered: &[&RunReport], pick: fn(&RunReport) -> &Option<String>) -> Vec<String> {
    ordered
        .iter()
        .map(|report| pick(report).clone().unwrap_or_else(|| "(unknown)".into()))
        .collect()
}

/// Aggregate same-anchor runs into one flaky-aware verdict.
///
/// Run order is canonicalized by `run_id` (its compact UTC-stamp prefix makes
/// lexicographic order chronological). When any attribution dimension
/// disagrees, the report comes back refused: buckets/intervals empty, raw
/// per-run pass rates still listed.
pub fn aggregate_reports(reports: &[RunReport]) -> AggregateReport {
    let mut ordered: Vec<&RunReport> = reports.iter().collect();
    ordered.sort_by(|a, b| a.run_id.cmp(&b.run_id));

    let run_ids: Vec<String> = ordered.iter().map(|r| r.run_id.clone()).collect();
    let app_versions: Vec<String> = ordered.iter().map(|r| r.app_version.clone()).collect();

    let per_run_pass_rates: Vec<Option<f64>> = ordered
        .iter()
        .map(|report| {
            let total = report.records.len();
            (total > 0).then(|| {
                let resolved = report.records.iter().filter(|r| r.passed).count();
                resolved as f64 / total as f64
            })
        })
        .collect();

    // ── ATTRIBUTE-SPLIT guard: dimensions that block cross-run verdicts ──
    let mut attribute_splits: Vec<AttributeSplit> = Vec::new();
    let mut push_split = |dimension: &str, values: Vec<String>| {
        if values.iter().any(|value| value != &values[0]) {
            attribute_splits.push(AttributeSplit {
                dimension: dimension.to_string(),
                values,
            });
        }
    };
    for (dimension, pick) in [
        (
            "model_id",
            (|r: &RunReport| &r.anchors.model_id) as fn(&RunReport) -> &Option<String>,
        ),
        (
            "provider",
            (|r: &RunReport| &r.anchors.provider) as fn(&RunReport) -> &Option<String>,
        ),
        (
            "profile_digest",
            (|r: &RunReport| &r.anchors.profile_digest) as fn(&RunReport) -> &Option<String>,
        ),
        (
            "directive",
            (|r: &RunReport| &r.directive) as fn(&RunReport) -> &Option<String>,
        ),
    ] {
        push_split(dimension, anchor_values(&ordered, pick));
    }
    if ordered.len() >= 2 {
        let fingerprints: Vec<String> = ordered
            .iter()
            .map(|r| r.failure_rules_fingerprint.clone())
            .collect();
        if fingerprints.iter().any(|f| f != &fingerprints[0]) {
            attribute_splits.push(AttributeSplit {
                dimension: "failure_rules_fingerprint".to_string(),
                values: fingerprints,
            });
        }
    }

    // Distinct task ids and per-run id sets (drift detection).
    let id_sets: Vec<BTreeSet<&str>> = ordered
        .iter()
        .map(|r| r.records.iter().map(|rec| rec.id.as_str()).collect())
        .collect();
    let task_set_consistent = id_sets.iter().all(|set| *set == id_sets[0]);
    let tasks_total = id_sets
        .iter()
        .flatten()
        .copied()
        .collect::<BTreeSet<_>>()
        .len();

    let refused = !attribute_splits.is_empty();
    if refused {
        return AggregateReport {
            run_ids,
            n_runs: ordered.len(),
            anchors: RunAnchor::default(),
            failure_rules_fingerprint: ordered
                .first()
                .map_or_else(String::new, |r| r.failure_rules_fingerprint.clone()),
            app_versions,
            attribute_splits,
            tasks_total,
            tasks: Vec::new(),
            stable_pass: Vec::new(),
            flaky_tasks: Vec::new(),
            stable_fail: Vec::new(),
            task_set_consistent,
            per_run_pass_rates,
            pass_rate_interval: None,
            stable_pass_rate_interval: None,
        };
    }

    // ── Per-task bucketing ────────────────────────────────────────────────
    let mut by_task: BTreeMap<&str, Vec<(&RunReport, &TaskRunRecord)>> = BTreeMap::new();
    for report in &ordered {
        for record in &report.records {
            by_task
                .entry(record.id.as_str())
                .or_default()
                .push((report, record));
        }
    }

    let mut tasks = Vec::with_capacity(by_task.len());
    let mut stable_pass = Vec::new();
    let mut flaky_tasks = Vec::new();
    let mut stable_fail = Vec::new();
    for (id, entries) in by_task {
        let n = entries.len();
        let resolved_k = entries.iter().filter(|(_, r)| r.passed).count();
        let flaky = n > 1 && resolved_k > 0 && resolved_k < n;
        let bucket = if flaky {
            AggregateBucket::Flaky
        } else if resolved_k == n {
            AggregateBucket::StablePass
        } else {
            AggregateBucket::StableFail
        };
        match bucket {
            AggregateBucket::StablePass => stable_pass.push(id.to_string()),
            AggregateBucket::Flaky => flaky_tasks.push(id.to_string()),
            AggregateBucket::StableFail => stable_fail.push(id.to_string()),
        }
        tasks.push(TaskAggregate {
            id: id.to_string(),
            tier: entries[0].1.tier.clone(),
            statuses: entries
                .iter()
                .map(|(_, r)| r.status.as_str().to_string())
                .collect(),
            contributing_runs: entries
                .iter()
                .map(|(report, _)| report.run_id.clone())
                .collect(),
            resolved_k,
            n,
            flaky,
            bucket,
        });
    }

    // ── Suite intervals ───────────────────────────────────────────────────
    let intervals_allowed = ordered.len() >= 2 && task_set_consistent;
    let pass_rate_interval = if intervals_allowed && per_run_pass_rates.iter().all(Option::is_some)
    {
        let rates: Vec<f64> = per_run_pass_rates.iter().flatten().copied().collect();
        Some(min_max(&rates))
    } else {
        None
    };

    // The stable scope is every NON-flaky task — stable_fail drags the rate
    // down honestly; only flaky tasks leave the capability denominator.
    let stable_set: BTreeSet<&str> = tasks
        .iter()
        .filter(|t| t.bucket != AggregateBucket::Flaky)
        .map(|t| t.id.as_str())
        .collect();
    let stable_pass_rate_interval = if intervals_allowed && !stable_set.is_empty() {
        let rates: Option<Vec<f64>> = ordered
            .iter()
            .map(|report| {
                let mut resolved = 0usize;
                let mut total = 0usize;
                for record in &report.records {
                    if stable_set.contains(record.id.as_str()) {
                        total += 1;
                        resolved += usize::from(record.passed);
                    }
                }
                (total > 0).then(|| resolved as f64 / total as f64)
            })
            .collect();
        rates.map(|values| min_max(&values))
    } else {
        None
    };

    AggregateReport {
        run_ids,
        n_runs: ordered.len(),
        anchors: ordered
            .first()
            .map_or_else(RunAnchor::default, |r| r.anchors.clone()),
        failure_rules_fingerprint: ordered
            .first()
            .map_or_else(String::new, |r| r.failure_rules_fingerprint.clone()),
        app_versions,
        attribute_splits: Vec::new(),
        tasks_total,
        tasks,
        stable_pass,
        flaky_tasks,
        stable_fail,
        task_set_consistent,
        per_run_pass_rates,
        pass_rate_interval,
        stable_pass_rate_interval,
    }
}

/// Load every `<child>/report.json` under a run root, sorted by child
/// directory name (the run-id UTC-stamp prefix keeps that chronological).
pub fn load_reports_from_root(root: &Path) -> Result<Vec<RunReport>, EvalError> {
    let entries = std::fs::read_dir(root).map_err(|e| {
        EvalError::Config(format!("failed to read run root {}: {e}", root.display()))
    })?;
    let mut report_paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .map(|dir| dir.join("report.json"))
        .filter(|path| path.is_file())
        .collect();
    report_paths.sort();
    report_paths.iter().map(|path| load_report(path)).collect()
}

/// Write the dual aggregate artifacts (`aggregate.json` + `aggregate.md`)
/// into a run root, mirroring [`super::eval_runner::run_suite`]'s discipline.
pub fn persist_aggregate(
    root: &Path,
    report: &AggregateReport,
) -> Result<(PathBuf, PathBuf), EvalError> {
    let json_path = root.join("aggregate.json");
    let md_path = root.join("aggregate.md");
    std::fs::write(&json_path, serde_json::to_vec_pretty(report)?)?;
    std::fs::write(&md_path, report.render_markdown())?;
    Ok((json_path, md_path))
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::testing::eval_runner::RunStatus;

    const RUN_A: &str = "20260827175908-aaaaaaa1";
    const RUN_B: &str = "20260827180600-bbbbbbb2";
    const RUN_C: &str = "20260827181226-ccccccc3";

    fn record(id: &str, tier: &str, status: RunStatus) -> TaskRunRecord {
        TaskRunRecord {
            id: id.to_string(),
            tier: tier.to_string(),
            horizon: "short".to_string(),
            passed: status == RunStatus::Passed,
            status,
            duration_ms: 0,
            exit_code: None,
            turns: 0,
            tokens_in: 0,
            tokens_out: 0,
            total_tokens: 0,
            session_id: None,
            violations: Vec::new(),
            soft_flags: Vec::new(),
            rule_outcomes: Vec::new(),
            trajectory_tools: Vec::new(),
            metrics: None,
            failure_class: None,
            failure_evidence: Vec::new(),
            over_expected: None,
            anchor: RunAnchor::default(),
            workspace: PathBuf::new(),
            task_file: PathBuf::new(),
        }
    }

    /// Three-task synthetic suite: `t_fail` always fails, `t_pass` always
    /// passes, `t_flaky` passes unless `flip` turns it into a plain failure.
    fn suite(flip: bool) -> Vec<TaskRunRecord> {
        vec![
            record("t_fail", "edit", RunStatus::Failed),
            record(
                "t_flaky",
                "edit",
                if flip {
                    RunStatus::Failed
                } else {
                    RunStatus::Passed
                },
            ),
            record("t_pass", "read", RunStatus::Passed),
        ]
    }

    fn report(run_id: &str, model: &str, flip: bool) -> RunReport {
        let records = suite(flip);
        RunReport {
            run_id: run_id.to_string(),
            started_at_utc: "2026-08-27T00:00:00Z".to_string(),
            dry_run: false,
            shannon_bin: String::new(),
            directive: None,
            anchors: RunAnchor {
                model_id: Some(model.to_string()),
                provider: Some("mock".to_string()),
                profile_digest: Some("digest1".to_string()),
            },
            tasks_total: records.len(),
            tasks_passed: records.iter().filter(|r| r.passed).count(),
            tasks_failed: records
                .iter()
                .filter(|r| r.status == RunStatus::Failed)
                .count(),
            tasks_limited: 0,
            tasks_timed_out: 0,
            tasks_spawn_errors: 0,
            metrics_source: "none".to_string(),
            app_version: "0.11.0".to_string(),
            failure_rules_fingerprint: "fp1".to_string(),
            records,
        }
    }

    #[test]
    fn aggregate_flags_flaky_bucket_and_intervals() {
        let aggregate = aggregate_reports(&[
            report(RUN_A, "glm", false),
            report(RUN_B, "glm", false),
            report(RUN_C, "glm", true),
        ]);

        assert_eq!(aggregate.n_runs, 3);
        assert!(aggregate.task_set_consistent);
        assert_eq!(aggregate.tasks_total, 3);
        assert_eq!(aggregate.stable_pass, vec!["t_pass"]);
        assert_eq!(aggregate.flaky_tasks, vec!["t_flaky"]);
        assert_eq!(aggregate.stable_fail, vec!["t_fail"]);

        let flaky = aggregate
            .tasks
            .iter()
            .find(|t| t.id == "t_flaky")
            .expect("flaky task present");
        assert_eq!(flaky.resolved_k, 2);
        assert_eq!(flaky.n, 3);
        assert!(flaky.flaky);
        assert_eq!(flaky.bucket, AggregateBucket::Flaky);
        assert_eq!(flaky.statuses, vec!["passed", "passed", "failed"]);
        assert_eq!(flaky.contributing_runs, vec![RUN_A, RUN_B, RUN_C]);

        let stable = aggregate
            .tasks
            .iter()
            .find(|t| t.id == "t_pass")
            .expect("stable task present");
        assert!(!stable.flaky);
        assert_eq!(stable.bucket, AggregateBucket::StablePass);

        // Per-run rates: 2/3, 2/3, 1/3 → interval [1/3, 2/3].
        assert_eq!(aggregate.pass_rate_interval, Some([1.0 / 3.0, 2.0 / 3.0]));
        // Stable scope (t_pass + t_fail): 1/2 every run → tight [0.5, 0.5];
        // the flaky flip leaves the main conclusion untouched.
        assert_eq!(aggregate.stable_pass_rate_interval, Some([0.5, 0.5]));

        let md = aggregate.render_markdown();
        assert!(md.contains("| flaky | 1 |"), "{md}");
        assert!(
            md.contains("stable pass-rate interval (main conclusion — cite this): [0.500, 0.500]"),
            "{md}"
        );
        assert!(md.contains("## Flaky tasks"), "{md}");
        assert!(
            md.contains("- t_flaky: 2/3 — passed, passed, failed"),
            "{md}"
        );

        // JSON roundtrip keeps every field.
        let encoded = serde_json::to_string(&aggregate).expect("serialize");
        let decoded: AggregateReport = serde_json::from_str(&encoded).expect("deserialize");
        assert_eq!(decoded, aggregate);
    }

    #[test]
    fn attribute_split_refuses_verdict_but_lists_numbers() {
        let aggregate = aggregate_reports(&[
            report(RUN_A, "glm", false),
            report(RUN_B, "other-model", false),
        ]);

        assert!(aggregate.is_refused());
        assert_eq!(aggregate.attribute_splits.len(), 1);
        assert_eq!(aggregate.attribute_splits[0].dimension, "model_id");
        assert_eq!(
            aggregate.attribute_splits[0].values,
            vec!["glm", "other-model"]
        );
        // Verdict fields withheld.
        assert!(aggregate.tasks.is_empty());
        assert!(aggregate.flaky_tasks.is_empty());
        assert!(aggregate.pass_rate_interval.is_none());
        assert!(aggregate.stable_pass_rate_interval.is_none());
        // 数字照列: raw per-run numbers survive the refusal.
        assert_eq!(aggregate.per_run_pass_rates.len(), 2);
        assert!(aggregate.per_run_pass_rates.iter().all(Option::is_some));
        let md = aggregate.render_markdown();
        assert!(md.starts_with("# Shannon Eval Aggregate — REFUSED"), "{md}");
        assert!(md.contains("model_id: glm -> other-model"), "{md}");
        assert!(md.contains("Per-run pass rates"), "{md}");

        // Rule-table drift splits too (W1 §4② vocabulary).
        let mut drifted = report(RUN_B, "glm", false);
        drifted.failure_rules_fingerprint = "fp2".to_string();
        let aggregate = aggregate_reports(&[report(RUN_A, "glm", false), drifted]);
        assert!(aggregate.is_refused());
        assert_eq!(
            aggregate.attribute_splits[0].dimension,
            "failure_rules_fingerprint"
        );
    }

    #[test]
    fn single_run_makes_no_flaky_claim() {
        let aggregate = aggregate_reports(&[report(RUN_A, "glm", false)]);

        assert_eq!(aggregate.n_runs, 1);
        assert!(aggregate.flaky_tasks.is_empty());
        assert!(aggregate.tasks.iter().all(|t| !t.flaky));
        // Buckets still emit, but intervals are honestly withheld (n<2).
        // The single-observation pass of t_flaky lands in stable_pass —
        // qualified as a single observation by the preamble, never "flaky".
        assert_eq!(aggregate.stable_pass, vec!["t_flaky", "t_pass"]);
        assert_eq!(aggregate.stable_fail, vec!["t_fail"]);
        assert!(aggregate.pass_rate_interval.is_none());
        assert!(aggregate.stable_pass_rate_interval.is_none());

        let md = aggregate.render_markdown();
        assert!(
            md.contains("n=1: single-run aggregation"),
            "honesty note required: {md}"
        );
    }

    #[test]
    fn task_set_drift_withholds_intervals_but_buckets_survive() {
        let mut third = report(RUN_C, "glm", true);
        // Run C never recorded t_fail AND flips t_flaky via a limit class —
        // a timeout is as much "not resolved" as a plain failure.
        third.records.retain(|r| r.id != "t_fail");
        third.tasks_total = third.records.len();
        for rec in &mut third.records {
            if rec.id == "t_flaky" {
                rec.status = RunStatus::Timeout;
                rec.passed = false;
            }
        }
        let aggregate = aggregate_reports(&[
            report(RUN_A, "glm", false),
            report(RUN_B, "glm", false),
            third,
        ]);

        assert!(!aggregate.task_set_consistent);
        assert!(aggregate.pass_rate_interval.is_none());
        assert!(aggregate.stable_pass_rate_interval.is_none());

        let flaky = aggregate
            .tasks
            .iter()
            .find(|t| t.id == "t_flaky")
            .expect("task present");
        assert!(flaky.flaky, "timeout flip still isolates as flaky");
        assert_eq!(flaky.statuses, vec!["passed", "passed", "timeout"]);
        assert_eq!(flaky.n, 3);

        // The absent-everywhere run does not contribute to t_fail's n.
        let stable_fail = aggregate
            .tasks
            .iter()
            .find(|t| t.id == "t_fail")
            .expect("task present");
        assert_eq!(stable_fail.n, 2, "only runs that recorded the task count");
        assert_eq!(stable_fail.statuses, vec!["failed", "failed"]);
        assert_eq!(stable_fail.bucket, AggregateBucket::StableFail);

        let md = aggregate.render_markdown();
        assert!(md.contains("Task set consistent: no"), "{md}");
        assert!(
            md.contains("withheld (task sets differ across runs)"),
            "{md}"
        );
    }

    #[test]
    fn disk_roundtrip_scans_sorted_and_persists_dual_artifacts() {
        let root = tempfile::TempDir::new().expect("root");
        for (run_id, flip) in [(RUN_B, false), (RUN_A, false), (RUN_C, true)] {
            let dir = root.path().join(run_id);
            std::fs::create_dir_all(&dir).expect("run dir");
            std::fs::write(
                dir.join("report.json"),
                serde_json::to_vec_pretty(&report(run_id, "glm", flip)).expect("serialize"),
            )
            .expect("write report");
        }
        // Noise entry: a directory without report.json must be skipped.
        std::fs::create_dir_all(root.path().join("not-a-run")).expect("noise dir");

        let mut reports = load_reports_from_root(root.path()).expect("load");
        reports.sort_by(|a, b| a.run_id.cmp(&b.run_id));
        assert_eq!(
            reports
                .iter()
                .map(|r| r.run_id.as_str())
                .collect::<Vec<_>>(),
            vec![RUN_A, RUN_B, RUN_C],
            "scan must be chronological"
        );

        let aggregate = aggregate_reports(&reports);
        let (json_path, md_path) = persist_aggregate(root.path(), &aggregate).expect("persist");
        assert!(json_path.is_file() && md_path.is_file());

        let decoded: AggregateReport =
            serde_json::from_str(&std::fs::read_to_string(&json_path).expect("read"))
                .expect("deserialize");
        assert_eq!(decoded, aggregate);
        assert_eq!(decoded.flaky_tasks, vec!["t_flaky"]);
        assert_eq!(decoded.stable_pass_rate_interval, Some([0.5, 0.5]));
    }

    #[test]
    fn directive_drift_blocks_aggregation() {
        let mut second = report(RUN_B, "glm", false);
        second.directive = Some("use rustfmt".to_string());
        let aggregate = aggregate_reports(&[report(RUN_A, "glm", false), second]);
        assert!(aggregate.is_refused());
        assert_eq!(aggregate.attribute_splits[0].dimension, "directive");
    }
}
