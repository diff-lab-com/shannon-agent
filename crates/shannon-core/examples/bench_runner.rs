//! External benchmark trio command line (§4.13):
//! `cargo run -p shannon-core --example bench_runner -- [flags]`.
//!
//! Modes:
//! - **run** (default) — execute one or all external suites end-to-end,
//!   writing `<out>/<slug>/<run-id>/bench-report.{json,md}`. Dry-run
//!   rehearsal is the default so pipelines validate without API keys or
//!   foreign corpora (§4.4 posture); pass `--real` plus the delegation
//!   environment documented in `eval_benchmarks` for live scoring.
//! - **diff** — compare two persisted bench reports:
//!   `bench_runner diff <a.json> <b.json>`.
//! - **validate-pins** — check a pinned workload against its mounted corpus
//!   (required once before citing scores; loud drift reporting).
//!
//! Exit codes: 0 ran clean (no failed dispositions) · 1 completed with
//! failures/drift · 2 configuration/load error.

use shannon_core::testing::eval_benchmarks::{
    self, BenchDisposition, BenchSuite, BenchmarkOptions, DELEGATION_DEFAULT_TIMEOUT_SECS,
    PinManifest, compare_bench_reports, default_benchmark_dir, load_bench_report,
    load_pin_manifest, run_benchmark,
};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

struct RunArgs {
    suites: Vec<BenchSuite>,
    n_runs: usize,
    real: bool,
    out_override: Option<PathBuf>,
    rules: Option<PathBuf>,
    tb_tasks_dir: Option<PathBuf>,
    sb_home: Option<PathBuf>,
    harness_cmd: Option<String>,
    timeout_secs: u64,
}

const USAGE: &str = "\
Usage:
  bench_runner [flags]
      --suite <SLUG>  terminal_bench | swebench_verified_50 | regression
                      (repeatable; default: all three)
      --n <N>         repetitions per case (default 3; citable minimum 3)
      --real          live execution (default is dry-run rehearsal)
      --out <DIR>     output root override (default $SHANNON_HOME/eval/benchmarks)
      --rules <PATH>  failure-rule table override forwarded to the §4.7 classifier
      --tb-tasks DIR  Terminal-Bench corpus override ($SHANNON_TB_TASKS_DIR)
      --sb-home DIR   SWE-bench harness home override ($SHANNON_SWEBENCH_HOME)
      --cmd TPL       harness command template override ({native_id}, {task_dir};
                      else SHANNON_TB_HARNESS_CMD / SHANNON_SB_HARNESS_CMD)
      --timeout SECS  wall-clock ceiling per delegated repetition

  bench_runner diff <report_a.json> <report_b.json>
  bench_runner validate-pins [--suite SLUG] [--tb-tasks DIR] [--sb-home DIR]
";

fn main() -> ExitCode {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    match raw.first().map(String::as_str) {
        Some("diff") => return cmd_diff(&raw[1..]),
        Some("validate-pins") => return cmd_validate_pins(&raw[1..]),
        _ => {}
    }
    match parse_run_args(&raw) {
        Ok(args) => cmd_run(args),
        Err(message) => {
            eprintln!("{message}\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn parse_suite(slug: &str) -> Result<BenchSuite, String> {
    BenchSuite::ALL
        .iter()
        .copied()
        .find(|s| s.slug() == slug)
        .ok_or_else(|| format!("unknown suite '{slug}'"))
}

fn parse_run_args(raw: &[String]) -> Result<RunArgs, String> {
    let mut args = RunArgs {
        suites: Vec::new(),
        n_runs: eval_benchmarks::N_RUNS_REQUIRED,
        real: false,
        out_override: None,
        rules: None,
        tb_tasks_dir: None,
        sb_home: None,
        harness_cmd: None,
        timeout_secs: DELEGATION_DEFAULT_TIMEOUT_SECS,
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
            "--suite" => {
                args.suites.push(parse_suite(&value_at(i + 1)?)?);
                i += 1;
            }
            "--n" => {
                args.n_runs = value_at(i + 1)?.parse().map_err(|e| format!("--n: {e}"))?;
                if args.n_runs == 0 {
                    return Err("--n must be >= 1".into());
                }
                i += 1;
            }
            "--real" => args.real = true,
            "--out" => {
                args.out_override = Some(PathBuf::from(value_at(i + 1)?));
                i += 1;
            }
            "--rules" => {
                args.rules = Some(PathBuf::from(value_at(i + 1)?));
                i += 1;
            }
            "--tb-tasks" => {
                args.tb_tasks_dir = Some(PathBuf::from(value_at(i + 1)?));
                i += 1;
            }
            "--sb-home" => {
                args.sb_home = Some(PathBuf::from(value_at(i + 1)?));
                i += 1;
            }
            "--cmd" => {
                args.harness_cmd = Some(value_at(i + 1)?);
                i += 1;
            }
            "--timeout" => {
                args.timeout_secs = value_at(i + 1)?
                    .parse()
                    .map_err(|e| format!("--timeout: {e}"))?;
                i += 1;
            }
            other => return Err(format!("unknown flag '{other}'")),
        }
        i += 1;
    }
    if args.suites.is_empty() {
        args.suites = BenchSuite::ALL.to_vec();
    }
    Ok(args)
}

fn cmd_run(args: RunArgs) -> ExitCode {
    let bench_dir = default_benchmark_dir();
    println!(
        "[bench] {} mode · suites: {} · n={} · pins root: {}",
        if args.real { "REAL" } else { "DRY-RUN" },
        args.suites
            .iter()
            .map(|s| s.slug())
            .collect::<Vec<_>>()
            .join(","),
        args.n_runs,
        bench_dir.display(),
    );

    let mut any_failure = false;
    let mut summaries: Vec<(String, PathBuf)> = Vec::new();

    for suite in &args.suites {
        let options = BenchmarkOptions {
            suite: *suite,
            n_runs: args.n_runs,
            dry_run: !args.real,
            out_root_override: args.out_override.clone(),
            failure_rules: args.rules.clone(),
            tb_tasks_dir: args.tb_tasks_dir.clone(),
            sb_home: args.sb_home.clone(),
            harness_cmd: args.harness_cmd.clone(),
            delegation_timeout_secs: args.timeout_secs,
        };
        match run_benchmark(&options, &bench_dir) {
            Ok((report, run_dir)) => {
                println!("[bench/{}] report: {}", suite.slug(), run_dir.display());
                println!(
                    "[bench/{}] cases {} · resolved events {} · interval {:?} · cost/resolved {}",
                    suite.slug(),
                    report.cases_total,
                    report.resolved_events,
                    report.resolved_rate_interval,
                    report
                        .cost_per_resolved_usd
                        .map(|c| format!("{c:.4}"))
                        .unwrap_or_else(|| "null".into()),
                );
                for record in &report.records {
                    println!(
                        "  {:<30} reps:{:>2} resolved:{:<3} failed:{:<3} skipped:{:<3}",
                        record.native_id,
                        record.reps.len(),
                        record.resolved_reps,
                        record.failed_reps,
                        record.skipped_reps,
                    );
                }
                for blocker in &report.citation.blockers {
                    println!("[bench/{}] uncitable: {}", suite.slug(), blocker);
                }
                if report.records.iter().any(|r| {
                    r.reps.iter().any(|rep| {
                        matches!(
                            rep.disposition,
                            BenchDisposition::Failed | BenchDisposition::Ambiguous
                        )
                    })
                }) {
                    any_failure = true;
                }
                summaries.push((suite.slug().to_string(), run_dir));
            }
            Err(e) => {
                eprintln!("[bench/{}] error: {}", suite.slug(), e);
                return ExitCode::from(2);
            }
        }
    }

    // Stitched cross-suite overview pointing at each per-suite artifact.
    if let Some(out) = args.out_override.as_ref() {
        let summary_path = out.join("bench-summary.md");
        let _ = std::fs::write(
            &summary_path,
            render_summary(&summaries, args.n_runs, !args.real),
        );
        println!("[bench] stitched summary: {}", summary_path.display());
    }

    if any_failure && args.real {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn cmd_diff(rest: &[String]) -> ExitCode {
    if rest.len() != 2 {
        eprintln!("diff needs exactly two report paths\n\n{USAGE}");
        return ExitCode::from(2);
    }
    let a = load_bench_report(Path::new(&rest[0]));
    let b = load_bench_report(Path::new(&rest[1]));
    let (Ok(a), Ok(b)) = (a, b) else {
        eprintln!("error: could not load one of the reports");
        return ExitCode::from(2);
    };
    println!("A: {} ({}, n={})", a.run_id, a.suite, a.n_runs);
    println!("B: {} ({}, n={})", b.run_id, b.suite, b.n_runs);
    let verdict = compare_bench_reports(&a, &b);
    println!("{verdict}");
    if verdict.starts_with("STABLE") {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn cmd_validate_pins(rest: &[String]) -> ExitCode {
    let mut suite = BenchSuite::TerminalBench;
    let mut tb_dir: Option<PathBuf> = std::env::var("SHANNON_TB_TASKS_DIR")
        .ok()
        .map(PathBuf::from);
    let mut sb_home: Option<PathBuf> = std::env::var("SHANNON_SWEBENCH_HOME")
        .ok()
        .map(PathBuf::from);
    let mut i = 0usize;
    while i < rest.len() {
        match rest[i].as_str() {
            "--suite" => {
                let slug = rest.get(i + 1).cloned().unwrap_or_default();
                suite = match parse_suite(&slug) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("{e}\n\n{USAGE}");
                        return ExitCode::from(2);
                    }
                };
                i += 1;
            }
            "--tb-tasks" => {
                tb_dir = rest.get(i + 1).cloned().map(PathBuf::from);
                i += 1;
            }
            "--sb-home" => {
                sb_home = rest.get(i + 1).cloned().map(PathBuf::from);
                i += 1;
            }
            other => {
                eprintln!("unknown flag '{other}'\n\n{USAGE}");
                return ExitCode::from(2);
            }
        }
        i += 1;
    }

    let bench_dir = default_benchmark_dir();
    let manifest: PinManifest = match load_pin_manifest(suite, &bench_dir) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };
    if suite.pin_file_name().is_none() {
        eprintln!("error: '{}' has no pin manifest", suite.slug());
        return ExitCode::from(2);
    }
    println!(
        "[pins] {} fingerprint {}",
        suite.pin_file_name().unwrap_or("<none>"),
        manifest.fingerprint
    );

    // Corpus contact: exact set comparison surfaces both drift directions.
    match suite {
        BenchSuite::TerminalBench => {
            let Some(dir) = tb_dir else {
                println!(
                    "[pins] corpus NOT mounted — set {}; validation stays pending",
                    suite.home_env_var().unwrap_or("-")
                );
                return ExitCode::SUCCESS;
            };
            let listed = std::fs::read_dir(&dir)
                .map(|entries| {
                    entries
                        .filter_map(Result::ok)
                        .filter(|e| e.path().is_dir())
                        .filter_map(|e| e.file_name().to_str().map(str::to_owned))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let pinned: std::collections::BTreeSet<&str> =
                manifest.ids.iter().map(String::as_str).collect();
            let present: std::collections::BTreeSet<&str> =
                listed.iter().map(String::as_str).collect();
            let absent: Vec<_> = pinned.difference(&present).collect();
            for extra in present.difference(&pinned) {
                println!("[note ] corpus-only (not pinned): {extra}");
            }
            for miss in &absent {
                println!("[drift] pinned but not in corpus: {miss}");
            }
            if absent.is_empty() {
                println!("[pins] VALIDATED — every pin resolves against the corpus");
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        BenchSuite::SwebenchVerified50 => {
            match sb_home {
                Some(home) => println!(
                    "[pins] corpus contact at {} — instance-level confirmation \
                     happens inside the official harness on first real run; \
                     treat scores as uncitable until then.",
                    home.display()
                ),
                None => println!(
                    "[pins] corpus NOT mounted — set {}; validation stays pending",
                    suite.home_env_var().unwrap_or("-")
                ),
            }
            ExitCode::SUCCESS
        }
        BenchSuite::Regression => unreachable!("handled above"),
    }
}

fn render_summary(suites: &[(String, PathBuf)], n: usize, dry: bool) -> String {
    let mut md = String::from("# External Benchmark Summary (§4.13)\n\n");
    md.push_str(&format!(
        "- Mode: {}\n- Runs per case: n={n}\n- External reference rule: attach \
         n + date; never mix model and harness changes inside one run\n\n",
        if dry {
            "DRY-RUN rehearsal (mock口径)"
        } else {
            "REAL"
        },
    ));
    md.push_str("| suite | report directory |\n|---|---|\n");
    for (slug, dir) in suites {
        md.push_str(&format!("| {slug} | {} |\n", dir.display()));
    }
    md.push_str(
        "\nEach linked report carries its own score block (resolved-rate \
         interval, cost-per-resolved), variance-attribution notes and the \
         citation gates.\n",
    );
    md
}
