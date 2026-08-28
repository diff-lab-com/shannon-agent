//! Eval runner command line (§4.4): `cargo run -p shannon-core --example eval_runner`.
//!
//! Modes:
//! - **run** (default) — execute the L1 task suite and emit `report.json` +
//!   `report.md` under `$SHANNON_HOME/eval/runs/<run-id>/` (or `--out DIR`).
//!   Dry-run is the default so the pipeline can be rehearsed without an API
//!   key; pass `--real` (and `--bin`) to drive actual model runs.
//! - **diff** — compare two persisted reports for metric-stability sanity:
//!   `eval_runner diff runs/<a>/report.json runs/<b>/report.json`.
//! - **aggregate** — flaky isolation over repeated runs (design §4③④):
//!   `eval_runner aggregate ~/.shannon/eval/v1-official [--json]`. Scans
//!   `<root>/*/report.json` (or takes explicit report paths), refuses on
//!   anchor mismatch (ATTRIBUTE-SPLIT), and reports per-task `resolved_k/n`,
//!   `stable_pass`/`flaky`/`stable_fail` buckets and suite pass-rate
//!   intervals (the stable scope is the citable conclusion). Writes
//!   `aggregate.json`/`aggregate.md` into the first directory argument.
//!   Flaky presence is data, not a failure — exit stays 0.
//!
//! Exit codes: 0 all tasks passed (run) / aggregation emitted (aggregate,
//! diff STABLE) · 1 run completed with failures · 2 configuration/load error
//! or refused aggregation.
//!

use shannon_core::testing::eval_aggregate::{
    aggregate_reports, load_reports_from_root, persist_aggregate,
};
use shannon_core::testing::eval_runner::{
    EvalOptions, EvalTier, RunReport, RunStatus, compare_reports, load_report, parse_tasks_dir,
    resolve_bin, run_suite,
};
use std::path::PathBuf;
use std::process::ExitCode;

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

const USAGE: &str = "\
Usage:
  eval_runner [run flags]
      --tasks <DIR>   Task directory (default: repo tests/eval/tasks)
      --task <ID>     Restrict to one task id (repeatable)
      --tier <NAME>   Restrict to a tier: read|edit|search|multi_step|recovery (repeatable)
      --out <DIR>     Override the output root instead of $SHANNON_HOME/~/.shannon
      --bin <PATH>    Engine binary for --real (default: SHANNON_EVAL_BIN,
                      target/debug/shannon, then $PATH/shannon)
      --rules <PATH>  Failure-rule table override (§4.7; default: embedded)
      --real          Launch real engine runs (default is dry-run rehearsal)
      --list          Print the parsed suite inventory and exit

  eval_runner diff <report_a.json> <report_b.json>

  eval_runner aggregate <run-root|report.json> [more paths...] [--json]
";

fn main() -> ExitCode {
    let raw: Vec<String> = std::env::args().skip(1).collect();

    if raw.first().map(String::as_str) == Some("diff") {
        return cmd_diff(&raw[1..]);
    }

    if raw.first().map(String::as_str) == Some("aggregate") {
        return cmd_aggregate(&raw[1..]);
    }

    match parse_run_args(&raw) {
        Ok(args) => cmd_run(args),
        Err(message) => {
            eprintln!("{message}\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn parse_run_args(raw: &[String]) -> Result<RunArgs, String> {
    let mut args = RunArgs {
        tasks_dir: default_tasks_dir(),
        task_filter: Vec::new(),
        tier_filter: Vec::new(),
        out_override: None,
        bin_path: None,
        rules: None,
        directive: None,
        real: false,
        list_only: false,
    };
    let mut i = 0usize;
    while i < raw.len() {
        let flag = raw[i].as_str();
        let value_at = |k: usize| -> Result<String, String> {
            raw.get(k)
                .cloned()
                .ok_or_else(|| format!("missing value after {flag}"))
        };
        match flag {
            "--tasks" => {
                args.tasks_dir = PathBuf::from(value_at(i + 1)?);
                i += 1;
            }
            "--task" => {
                args.task_filter.push(value_at(i + 1)?);
                i += 1;
            }
            "--tier" => {
                let name = value_at(i + 1)?;
                let tier = match name.as_str() {
                    "read" => EvalTier::Read,
                    "edit" => EvalTier::Edit,
                    "search" => EvalTier::Search,
                    "multi_step" => EvalTier::MultiStep,
                    "recovery" => EvalTier::Recovery,
                    other => return Err(format!("unknown tier '{other}'")),
                };
                args.tier_filter.push(tier);
                i += 1;
            }
            "--out" => {
                args.out_override = Some(PathBuf::from(value_at(i + 1)?));
                i += 1;
            }
            "--bin" => {
                args.bin_path = Some(PathBuf::from(value_at(i + 1)?));
                i += 1;
            }
            "--rules" => {
                args.rules = Some(PathBuf::from(value_at(i + 1)?));
                i += 1;
            }
            "--directive" => {
                args.directive = Some(value_at(i + 1)?.to_string());
                i += 1;
            }
            "--real" => args.real = true,
            "--list" => args.list_only = true,
            other => return Err(format!("unknown flag '{other}'")),
        }
        i += 1;
    }
    Ok(args)
}

fn default_tasks_dir() -> PathBuf {
    // Examples compile with CARGO_MANIFEST_DIR pointing at the crate; the
    // shared suite lives in the repo-root tests/ tree.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crate parent")
        .parent()
        .expect("repo root")
        .join("tests")
        .join("eval")
        .join("tasks")
}

fn cmd_run(args: RunArgs) -> ExitCode {
    let all_tasks = match parse_tasks_dir(&args.tasks_dir) {
        Ok(tasks) => tasks,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };

    let selected: Vec<_> = all_tasks
        .iter()
        .filter(|t| args.task_filter.is_empty() || args.task_filter.contains(&t.id))
        .filter(|t| args.tier_filter.is_empty() || args.tier_filter.contains(&t.tier))
        .filter(|t| !args.list_only || t.validate().is_empty())
        .cloned()
        .collect::<Vec<_>>();

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
        return ExitCode::SUCCESS;
    }

    if selected.is_empty() {
        eprintln!("error: no tasks matched the given filters");
        return ExitCode::from(2);
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
                return ExitCode::from(2);
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
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(2)
        }
    }
}

fn cmd_diff(rest: &[String]) -> ExitCode {
    if rest.len() != 2 {
        eprintln!("diff mode needs exactly two report paths\n\n{USAGE}");
        return ExitCode::from(2);
    }
    let (Ok(a), Ok(b)) = (
        load_report(&PathBuf::from(&rest[0])),
        load_report(&PathBuf::from(&rest[1])),
    ) else {
        eprintln!("error: could not load one of the reports");
        return ExitCode::from(2);
    };
    println!("{a}", a = short_summary(&a));
    println!("{b}", b = short_summary(&b));
    print!("{}", compare_reports(&a, &b));
    if compare_reports(&a, &b).starts_with("STABLE") {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn short_summary(report: &shannon_core::testing::eval_runner::RunReport) -> String {
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

/// `aggregate <run-root|report.json>... [--json]` — cross-run flaky
/// isolation (design §4③④). Directory arguments are scanned for
/// `<child>/report.json`; the first directory also receives the persisted
/// `aggregate.json`/`aggregate.md` pair. Anchor mismatches refuse the
/// aggregation (ATTRIBUTE-SPLIT) and exit 2; flaky findings are data and do
/// not change the exit code.
fn cmd_aggregate(rest: &[String]) -> ExitCode {
    let mut json_mode = false;
    let mut paths: Vec<PathBuf> = Vec::new();
    for arg in rest {
        if arg == "--json" {
            json_mode = true;
        } else {
            paths.push(PathBuf::from(arg));
        }
    }
    if paths.is_empty() {
        eprintln!("aggregate needs a run root or report.json path(s)\n\n{USAGE}");
        return ExitCode::from(2);
    }

    let mut reports: Vec<RunReport> = Vec::new();
    let mut root_dir: Option<PathBuf> = None;
    for path in &paths {
        if path.is_file() {
            match load_report(path) {
                Ok(report) => reports.push(report),
                Err(e) => {
                    eprintln!("error: {e}");
                    return ExitCode::from(2);
                }
            }
        } else if path.is_dir() {
            match load_reports_from_root(path) {
                Ok(loaded) if loaded.is_empty() => {
                    eprintln!("error: no report.json found under {}", path.display());
                    return ExitCode::from(2);
                }
                Ok(mut loaded) => {
                    if root_dir.is_none() {
                        root_dir = Some(path.clone());
                    }
                    reports.append(&mut loaded);
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    return ExitCode::from(2);
                }
            }
        } else {
            eprintln!("error: {} does not exist", path.display());
            return ExitCode::from(2);
        }
    }

    // Chronological order (aggregate_reports canonicalizes too; sorting here
    // keeps the CLI's own prelude aligned with the verdict).
    reports.sort_by(|a, b| a.run_id.cmp(&b.run_id));
    for report in &reports {
        println!("{report}", report = short_summary(report));
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
                return ExitCode::from(2);
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
        return ExitCode::from(2);
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
    ExitCode::SUCCESS
}
