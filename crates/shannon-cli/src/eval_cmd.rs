//! # `shannon eval` — evaluate the agent against the L1 task suite (§4.4)
//!
//! Thin CLI wrapper over `shannon_core::testing::eval_runner` /
//! `eval_aggregate`, giving journey J7 (evaluate yourself) its user entry
//! point. Logic is ported 1:1 from `shannon-core/examples/eval_runner.rs`,
//! which remains as the low-level rehearsal harness.
//!
//! Modes:
//! - **run** — execute the task suite, emitting `report.json` + `report.md`
//!   under `~/.shannon/eval/runs/<run-id>/` (or `--out DIR`). Dry-run is the
//!   default so the pipeline can be rehearsed without an API key; `--real`
//!   (with `--bin`) drives actual model runs.
//! - **diff** — metric-stability sanity between two persisted reports.
//! - **aggregate** — flaky isolation over repeated runs; scans
//!   `<root>/*/report.json`, refuses on anchor mismatch (ATTRIBUTE-SPLIT).
//!   Flaky presence is data, not a failure — exit stays 0.
//!
//! Exit codes (preserved from the example): 0 all passed / aggregation
//! emitted · 1 run completed with failures (or UNSTABLE diff) · 2
//! configuration/load error or refused aggregation.

use std::path::PathBuf;


use clap::Subcommand;
use shannon_core::testing::eval_aggregate::{
    aggregate_reports, load_reports_from_root, persist_aggregate,
};
use shannon_core::testing::eval_runner::{
    EvalOptions, EvalTier, RunReport, RunStatus, compare_reports, load_report, parse_tasks_dir,
    resolve_bin, run_suite,
};

#[derive(Subcommand, Debug)]
pub enum EvalCommand {
    /// Run the eval task suite (dry-run rehearsal unless --real).
    Run {
        /// Task directory (default: repo tests/eval/tasks).
        #[arg(long, value_name = "DIR")]
        tasks: Option<PathBuf>,
        /// Restrict to one task id (repeatable).
        #[arg(long = "task", value_name = "ID")]
        task_filter: Vec<String>,
        /// Restrict to a tier: read|edit|search|multi_step|recovery (repeatable).
        #[arg(long = "tier", value_name = "NAME")]
        tier_filter: Vec<String>,
        /// Override the output root instead of ~/.shannon.
        #[arg(long, value_name = "DIR")]
        out: Option<PathBuf>,
        /// Engine binary for --real (default: SHANNON_EVAL_BIN, target/debug/shannon, $PATH/shannon).
        #[arg(long, value_name = "PATH")]
        bin: Option<PathBuf>,
        /// Failure-rule table override (§4.7; default: embedded).
        #[arg(long, value_name = "PATH")]
        rules: Option<PathBuf>,
        /// Instruction directive override.
        #[arg(long, value_name = "TEXT")]
        directive: Option<String>,
        /// Launch real engine runs (default is dry-run rehearsal).
        #[arg(long)]
        real: bool,
        /// Print the parsed suite inventory and exit.
        #[arg(long)]
        list: bool,
    },
    /// Compare two persisted reports for metric-stability drift.
    Diff {
        /// Baseline report.json.
        #[arg(value_name = "A_JSON")]
        report_a: PathBuf,
        /// Candidate report.json.
        #[arg(value_name = "B_JSON")]
        report_b: PathBuf,
    },
    /// Cross-run flaky isolation over repeated runs (design §4③④).
    Aggregate {
        /// Run root (scanned for */report.json) or explicit report.json paths.
        #[arg(value_name = "PATH", required = true)]
        paths: Vec<PathBuf>,
        /// Print the aggregate as JSON instead of markdown.
        #[arg(long)]
        json: bool,
    },
}

/// Execute an `shannon eval` subcommand; the returned code becomes the
/// process exit status (see module docs for the contract).
pub fn execute(command: EvalCommand) -> i32 {
    match command {
        EvalCommand::Run {
            tasks,
            task_filter,
            tier_filter,
            out,
            bin,
            rules,
            directive,
            real,
            list,
        } => {
            let tier_filter = match tier_filter
                .iter()
                .map(|name| parse_tier(name))
                .collect::<Result<Vec<_>, _>>()
            {
                Ok(tiers) => tiers,
                Err(message) => {
                    eprintln!("error: {message}");
                    return 2;
                }
            };
            cmd_run(RunArgs {
                tasks_dir: tasks.unwrap_or_else(default_tasks_dir),
                task_filter,
                tier_filter,
                out_override: out,
                bin_path: bin,
                rules,
                directive,
                real,
                list_only: list,
            })
        }
        EvalCommand::Diff { report_a, report_b } => cmd_diff(&report_a, &report_b),
        EvalCommand::Aggregate { paths, json } => cmd_aggregate(&paths, json),
    }
}

fn parse_tier(name: &str) -> Result<EvalTier, String> {
    match name {
        "read" => Ok(EvalTier::Read),
        "edit" => Ok(EvalTier::Edit),
        "search" => Ok(EvalTier::Search),
        "multi_step" => Ok(EvalTier::MultiStep),
        "recovery" => Ok(EvalTier::Recovery),
        other => Err(format!("unknown tier '{other}'")),
    }
}

struct RunArgs {
    tasks_dir: PathBuf,
    task_filter: Vec<String>,
    tier_filter: Vec<EvalTier>,
    out_override: Option<PathBuf>,
    bin_path: Option<PathBuf>,
    rules: Option<PathBuf>,
    directive: Option<String>,
    real: bool,
    list_only: bool,
}

fn default_tasks_dir() -> PathBuf {
    // The shared suite lives in the repo-root tests/ tree.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crate parent")
        .parent()
        .expect("repo root")
        .join("tests")
        .join("eval")
        .join("tasks")
}

fn cmd_run(args: RunArgs) -> i32 {
    let all_tasks = match parse_tasks_dir(&args.tasks_dir) {
        Ok(tasks) => tasks,
        Err(e) => {
            eprintln!("error: {e}");
            return 2;
        }
    };

    let selected: Vec<_> = all_tasks
        .iter()
        .filter(|t| args.task_filter.is_empty() || args.task_filter.contains(&t.id))
        .filter(|t| args.tier_filter.is_empty() || args.tier_filter.contains(&t.tier))
        .filter(|t| !args.list_only || t.validate().is_empty())
        .cloned()
        .collect();

    if args.list_only {
        println!("tasks dir: {}", args.tasks_dir.display());
        for task in &selected {
            let problems = task.validate();
            let verdict = if problems.is_empty() { "ok" } else { "INVALID" };
            let limits = task.resolved_limits();
            println!(
                "{:>10} {:<8} {:<5} limits(turns={},tokens={},timeout={}s) [{}] {}",
                task.id,
                task.tier.as_str(),
                task.effective_horizon().as_str(),
                limits.max_turns,
                limits.max_tokens,
                limits.timeout_secs,
                verdict,
                task.description,
            );
            for problem in problems {
                println!("    ! {problem}");
            }
        }
        println!("{} task(s)", selected.len());
        return 0;
    }

    if selected.is_empty() {
        eprintln!("error: no tasks matched the given filters");
        return 2;
    }

    let mode_note = if args.real { "REAL" } else { "DRY-RUN" };
    println!("[eval] {} mode · {} task(s)", mode_note, selected.len());
    if args.real && std::env::var("SHANNON_API_KEY").is_err() {
        eprintln!("warning: no SHANNON_API_KEY set — real runs will fail fast at engine startup");
    }

    let options = EvalOptions {
        bin_path: args.bin_path.clone(),
        dry_run: !args.real,
        out_dir_override: args.out_override.clone(),
        failure_rules: args.rules.clone(),
        instruction_directive: args.directive.clone(),
    };
    if args.real && options.bin_path.is_none() {
        match resolve_bin(None) {
            Ok(resolved) => println!("[eval] engine binary: {}", resolved.display()),
            Err(e) => {
                eprintln!("error resolving engine binary: {e}");
                return 2;
            }
        }
    }

    match run_suite(&selected, &options) {
        Ok((report, run_dir)) => {
            println!("[eval] run directory: {}", run_dir.display());
            println!(
                "[eval] report.json / report.md written · passed {}/{}",
                report.tasks_passed, report.tasks_total
            );
            for record in &report.records {
                let class = record.failure_class.as_deref().unwrap_or("-");
                let over = match record.over_expected {
                    Some(over) => {
                        let mut parts = Vec::new();
                        if let Some(multiple) = over.turns_multiple {
                            parts.push(format!("turn×{multiple:.1}"));
                        }
                        if let Some(multiple) = over.tokens_multiple {
                            parts.push(format!("tok×{multiple:.1}"));
                        }
                        format!(" OVER[{}]", parts.join(" "))
                    }
                    None => String::new(),
                };
                println!(
                    "  {:>10} {:<8} {:<5} {:<11} turns={:<3} tokens={:<6} violations={:<3} class={class}{over}",
                    record.id,
                    record.tier,
                    record.horizon,
                    record.status.as_str(),
                    record.turns,
                    record.total_tokens,
                    record.violations.len(),
                );
            }
            if report.tasks_total > 0
                && report.records.iter().all(|r| r.status == RunStatus::Passed)
            {
                0
            } else {
                1
            }
        }
        Err(e) => {
            eprintln!("error: {e}");
            2
        }
    }
}

fn cmd_diff(report_a: &PathBuf, report_b: &PathBuf) -> i32 {
    let (Ok(a), Ok(b)) = (load_report(report_a), load_report(report_b)) else {
        eprintln!("error: could not load one of the reports");
        return 2;
    };
    println!("{}", short_summary(&a));
    println!("{}", short_summary(&b));
    print!("{}", compare_reports(&a, &b));
    if compare_reports(&a, &b).starts_with("STABLE") {
        0
    } else {
        1
    }
}

fn short_summary(report: &RunReport) -> String {
    format!(
        "{}: {}/{} passed (limited {}, timed out {}, spawn errors {})",
        report.run_id,
        report.tasks_passed,
        report.tasks_total,
        report.tasks_limited,
        report.tasks_timed_out,
        report.tasks_spawn_errors,
    )
}

/// Cross-run flaky isolation. Directory arguments are scanned for
/// `<child>/report.json`; the first directory argument also receives the
/// persisted `aggregate.json`/`aggregate.md` pair. Anchor mismatches refuse
/// the aggregation (ATTRIBUTE-SPLIT) and exit 2.
fn cmd_aggregate(paths: &[PathBuf], json_mode: bool) -> i32 {
    if paths.is_empty() {
        eprintln!("error: aggregate needs a run root or report.json path(s)");
        return 2;
    }

    let mut reports: Vec<RunReport> = Vec::new();
    let mut root_dir: Option<PathBuf> = None;
    for path in paths {
        if path.is_file() {
            match load_report(path) {
                Ok(report) => reports.push(report),
                Err(e) => {
                    eprintln!("error: {e}");
                    return 2;
                }
            }
        } else if path.is_dir() {
            match load_reports_from_root(path) {
                Ok(loaded) if loaded.is_empty() => {
                    eprintln!("error: no report.json found under {}", path.display());
                    return 2;
                }
                Ok(mut loaded) => {
                    if root_dir.is_none() {
                        root_dir = Some(path.clone());
                    }
                    reports.append(&mut loaded);
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    return 2;
                }
            }
        } else {
            eprintln!("error: {} does not exist", path.display());
            return 2;
        }
    }

    // Chronological order (aggregate_reports canonicalizes too; sorting here
    // keeps the CLI's own prelude aligned with the verdict).
    reports.sort_by(|a, b| a.run_id.cmp(&b.run_id));
    for report in &reports {
        println!("{}", short_summary(report));
    }

    let aggregate = aggregate_reports(&reports);
    if let Some(root) = &root_dir {
        match persist_aggregate(root, &aggregate) {
            Ok((json_path, md_path)) => println!(
                "[eval] aggregate written: {} · {}",
                json_path.display(),
                md_path.display()
            ),
            Err(e) => {
                eprintln!("error persisting aggregate: {e}");
                return 2;
            }
        }
    }

    if json_mode {
        println!(
            "{}",
            serde_json::to_string_pretty(&aggregate).expect("aggregate serialization")
        );
    } else {
        println!("{}", aggregate.render_markdown());
    }

    if aggregate.is_refused() {
        eprintln!("error: ATTRIBUTE-SPLIT — runs are not comparable (see above)");
        return 2;
    }
    if aggregate.n_runs >= 2 {
        println!(
            "[eval] aggregate: stable_pass {} · flaky {} · stable_fail {} (n={})",
            aggregate.stable_pass.len(),
            aggregate.flaky_tasks.len(),
            aggregate.stable_fail.len(),
            aggregate.n_runs,
        );
    }
    0
}
