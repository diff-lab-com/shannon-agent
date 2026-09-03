//! External benchmark adapters (master plan §4.13 W2-M3 · M3 milestone).
//!
//! Bridges Shannon's internal eval runner (§4.4) to three external scoring
//! pools while preserving each pool's **native judging protocol**:
//!
//! | Suite | Pool | Native criterion | Pinned workload |
//! |---|---|---|---|
//! | [`BenchSuite::TerminalBench`] | laude-institute/terminal-bench | the task's own container verifier (`run-tests.sh` inside its image), invoked per the official toolchain | `tests/eval/benchmarks/terminalbench_tasks.txt` |
//! | [`BenchSuite::SwebenchVerified50`] | princeton-nlp/SWE-bench_Verified | `FAIL_TO_PASS` ∪ `PASS_TO_PASS` computed by the official harness | `tests/eval/benchmarks/swebench_verified_50.txt` |
//! | [`BenchSuite::Regression`] | self-built (本仓 CHANGELOG 真实缺陷复现) | issue narrative + repo-native verify rules/script (§4.4 schema, full pipeline) | `tests/eval/benchmarks/regression/*.toml` |
//!
//! ## Adapter contracts
//!
//! ### Input format
//! The two *remote* suites are pinned as plaintext ID lists (one native ID per
//! line, `#` comments allowed, blank lines ignored). Pins exist to prevent
//! silent drift of the evaluated workload: the SHA-256 fingerprint over the
//! parsed IDs travels inside every report ([`BenchReport::stable_digest`] and
//! the `diff` anchors), so any pin edit visibly changes provenance. Terminal-
//! Bench IDs are directory-name shaped; SWE-bench IDs follow the
//! `<owner>__<repo>-<number>` dataset convention and are shape-validated at
//! parse time.
//!
//! The regression suite reuses the §4.4 [`EvalTask`] schema byte-for-byte —
//! each task seeds the pre-fix buggy state, states the defect narrative from
//! the CHANGELOG entry it came from, and ships verify rules/script reflecting
//! the shipped fix.
//!
//! ### Conversion rule
//! Remote-suite cases are a *protocol* adaptation layer, not a task-content
//! translation: the adapter enumerates pinned native IDs, probes environment
//! readiness, and (when ready + real mode) delegates each repetition to the
//! configured harness command. Nothing about a foreign task's internals is
//! re-invented locally — 判据一律用基准原生（不自造）.
//!
//! ### Missing-environment behaviour (跳过，非伪绿)
//! Without the suite's corpus/harness mounted, remote cases execute nothing:
//! their dispositions stay [`BenchDisposition::SkippedEnvMissing`] with the
//! concrete remediation in the reason column, aggregate intervals remain
//! `null`, and the citation block refuses citability. Environment probes:
//!
//! - Terminal-Bench per-case: `$SHANNON_TB_TASKS_DIR/<native_id>/` exists
//!   (the official repo layout, one directory per task).
//! - SWE-bench suite-wide: `$SHANNON_SWEBENCH_HOME` points at an existing
//!   checkout/workspace of the official harness and dataset.
//!
//! ### Delegation (real mode, corpus present)
//! Each case repetition formats the configured template and runs it via
//! `sh -c` inside a per-repetition evidence directory:
//!
//! - Template vars: `{native_id}` (and `{task_dir}` for Terminal-Bench,
//!   resolving to that task's mounted directory).
//! - Verdict channel: the delegated command MAY write
//!   `verdict.json` (`{"resolved": bool}`) next to the capture dir
//!   (`SHANNON_BENCH_VERDICT_FILE` advertises the absolute path).
//!   Exit 0 + `resolved:true` ⇒ [`BenchDisposition::Resolved`];
//!   nonzero exit ⇒ `Failed`; wall-clock breach ⇒ `Timeout`;
//!   exit 0 without a verdict ⇒ `Ambiguous` (never counted as resolved).
//!
//! Because the foreign harness drives the engine session itself, L0 metric
//! extraction does not apply to delegated repetitions; metrics columns stay
//! honest (`null`) instead of being fabricated, and the report stamps
//! `metrics_source = "external_verdict"`.
//!
//! ### Variance discipline (n=3 区间与归因)
//! Every suite executes [`N_RUNS_REQUIRED`] (=3) repetitions; per-case status
//! histograms, pass-rate intervals `[min, max]` across repetitions, spread
//! attribution notes (status flips, token/cost spread) and the suite-level
//! citation block (date, n, criteria lineage) travel together so 对外引用永远
//! 附 n 与日期. One mechanical fact per variance dimension — never a guessed
//! cause. Cost-per-resolved shares the §4.7 cost column vocabulary
//! (Σ observed `cost_usd` ÷ Σ resolved events) and stays `null` unless the
//! underlying observations exist.
//!
//! Attribution discipline (模型与 harness 变更不得同 run) is enforced at the
//! artifact level: see the `bench_runner` example — pass `--bin` (engine
//! under test) XOR edit the engine; the report's `app_version` +
//! `workload fingerprints` anchors make mixed-provenance comparisons visible
//! in `bench_runner diff`.

use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::testing::eval_metrics::{FailureRules, MetricSource, TaskMetrics};
use crate::testing::eval_runner::{
    EvalError, EvalOptions, EvalTask, RunStatus, parse_tasks_dir, resolve_failure_rules, run_suite,
};

/// Public re-export so downstream CLI/report consumers share one error type.
pub use crate::testing::eval_runner::EvalError as BenchError;

// ── Suite identity ─────────────────────────────────────────────────────

/// The three external benchmarks wired into Shannon's eval pyramid (§4.13).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BenchSuite {
    /// laude-institute/terminal-bench — terminal-native tasks, per-task
    /// container + bundled verifier.
    TerminalBench,
    /// princeton-nlp/SWE-bench_Verified 50-instance subset — real GitHub
    /// issue repairs judged by fail-to-pass/pass-to-pass.
    SwebenchVerified50,
    /// Self-built regression pool distilled from this repository's
    /// CHANGELOG defect history (10 tasks).
    Regression,
}

impl BenchSuite {
    /// Every suite in canonical report order.
    pub const ALL: [BenchSuite; 3] = [
        BenchSuite::TerminalBench,
        BenchSuite::SwebenchVerified50,
        BenchSuite::Regression,
    ];

    /// Stable wire spelling (report field + output directory segment).
    pub fn slug(self) -> &'static str {
        match self {
            BenchSuite::TerminalBench => "terminal_bench",
            BenchSuite::SwebenchVerified50 => "swebench_verified_50",
            BenchSuite::Regression => "regression",
        }
    }

    /// Human-facing pool name for report headers.
    pub fn title(self) -> &'static str {
        match self {
            BenchSuite::TerminalBench => "Terminal-Bench",
            BenchSuite::SwebenchVerified50 => "SWE-bench Verified 50",
            BenchSuite::Regression => "Shannon Self-built Regression (10)",
        }
    }

    /// The pool's native judging statement, carried verbatim into reports so
    /// 外部引用始终能说出「分数是谁判的」.
    pub fn judge_statement(self) -> &'static str {
        match self {
            BenchSuite::TerminalBench => {
                "task-native verifier (run-tests.sh inside the task's own \
                 container), verdict delegated to the official toolchain"
            }
            BenchSuite::SwebenchVerified50 => {
                "FAIL_TO_PASS + PASS_TO_PASS test batteries, resolution \
                 computed by the official SWE-bench harness"
            }
            BenchSuite::Regression => {
                "issue-derived §4.4 verify rules/scripts asserting the \
                 CHANGELOG-documented post-fix behaviour"
            }
        }
    }

    /// Pinned workload file relative to the benchmarks root
    /// (`tests/eval/benchmarks/`). `None` = directory-of-TOMLs pool.
    pub fn pin_file_name(self) -> Option<&'static str> {
        match self {
            BenchSuite::TerminalBench => Some("terminalbench_tasks.txt"),
            BenchSuite::SwebenchVerified50 => Some("swebench_verified_50.txt"),
            BenchSuite::Regression => None,
        }
    }

    /// Suite-level directory-mount environment variable.
    pub fn home_env_var(self) -> Option<&'static str> {
        match self {
            BenchSuite::TerminalBench => Some("SHANNON_TB_TASKS_DIR"),
            BenchSuite::SwebenchVerified50 => Some("SHANNON_SWEBENCH_HOME"),
            BenchSuite::Regression => None,
        }
    }

    /// Delegation command template variable.
    pub fn harness_cmd_env_var(self) -> Option<&'static str> {
        match self {
            BenchSuite::TerminalBench => Some("SHANNON_TB_HARNESS_CMD"),
            BenchSuite::SwebenchVerified50 => Some("SHANNON_SB_HARNESS_CMD"),
            BenchSuite::Regression => None,
        }
    }
}

/// Repo-root-relative location of the pinned workload files.
pub const BENCHMARK_DIR: (&str, [&str; 3]) = ("benchmark root", ["tests", "eval", "benchmarks"]);

/// Absolute default root for the pinned manifests (`<repo>/tests/eval/benchmarks`).
pub fn default_benchmark_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crate parent")
        .parent()
        .expect("repo root")
        .join(BENCHMARK_DIR.1[0])
        .join(BENCHMARK_DIR.1[1])
        .join(BENCHMARK_DIR.1[2])
}

// ── Pinned workload manifests ──────────────────────────────────────────

/// A parsed pinned ID list plus its tamper-evident fingerprint.
#[derive(Debug, Clone, PartialEq)]
pub struct PinManifest {
    /// Native IDs in file order (order IS part of the pinned workload).
    pub ids: Vec<String>,
    /// SHA-256 hex over `ids.join("\n")` — identifies the workload version.
    pub fingerprint: String,
}

/// Parse a pin list: `#` comments (full-line or trailing) and blanks
/// stripped; surrounding whitespace trimmed; IDs otherwise byte-exact (case
/// matters to the upstream pools). Structural shape validation happens here
/// so a malformed pin fails loudly at load time instead of mid-run.
pub fn parse_pin_list(suite: BenchSuite, text: &str) -> Result<PinManifest, EvalError> {
    let mut ids = Vec::new();
    for (idx, raw_line) in text.lines().enumerate() {
        let line = raw_line.split('#').next().unwrap_or("").trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let err = |why: &str| {
            EvalError::Config(format!(
                "{}:{}: invalid native id '{line}': {why}",
                suite.pin_file_name().unwrap_or("<inline>"),
                idx + 1
            ))
        };
        match suite {
            BenchSuite::TerminalBench => {
                // Directory-name shaped: lowercase corpus convention.
                let ok = !line.is_empty()
                    && line.chars().all(|c| {
                        c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '-' | '.')
                    })
                    && line
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
                if !ok {
                    return Err(err(
                        "terminal-bench task ids look like directory names (lowercase, -, _, .)",
                    ));
                }
            }
            BenchSuite::SwebenchVerified50 => {
                let bad_shape = |why: &'static str| {
                    EvalError::Config(format!(
                        "{}:{}: invalid SWE-bench instance id '{line}': {why}",
                        suite.pin_file_name().unwrap_or("<inline>"),
                        idx + 1
                    ))
                };
                let Some((owner_repo, number)) = line.rsplit_once('-') else {
                    return Err(bad_shape("must end with a numeric PR/issue suffix"));
                };
                if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(bad_shape("numeric issue/PR suffix"));
                }
                if owner_repo.split("__").count() != 2 || owner_repo.contains(char::is_whitespace) {
                    return Err(bad_shape("expected '<owner>__<repo>-<number>'"));
                }
            }
            BenchSuite::Regression => {
                return Err(err("regression suite has no pin list"));
            }
        }
        ids.push(line.to_string());
    }
    if ids.is_empty() {
        return Err(EvalError::Config(format!(
            "{}: pin list is empty",
            suite.pin_file_name().unwrap_or("<inline>")
        )));
    }
    Ok(PinManifest {
        fingerprint: fingerprint_ids(&ids),
        ids,
    })
}

/// Deterministic workload fingerprint: SHA-256 over newline-joined ids.
pub fn fingerprint_ids(ids: &[String]) -> String {
    use sha2::{Digest, Sha256};
    let joined = ids.join("\n");
    let digest = Sha256::digest(joined.as_bytes());
    format!("sha256:{}", hex::encode(digest))
}

/// Load a suite's pinned manifest from the benchmark root.
pub fn load_pin_manifest(suite: BenchSuite, bench_dir: &Path) -> Result<PinManifest, EvalError> {
    let Some(name) = suite.pin_file_name() else {
        return Err(EvalError::Config(format!(
            "suite '{}' has no pin list",
            suite.slug()
        )));
    };
    let path = bench_dir.join(name);
    let text = std::fs::read_to_string(&path)
        .map_err(|e| EvalError::Config(format!("failed to read {}: {e}", path.display())))?;
    parse_pin_list(suite, &text)
}

// ── Environment readiness (缺环境 → 显式跳过) ───────────────────────────

/// Whether the host currently exposes the materials a suite needs.
#[derive(Debug, Clone, PartialEq)]
pub enum EnvState {
    /// Corpus/harness materialized; delegations may proceed in real mode.
    Ready {
        /// Root of the mounted material (corpus dir or harness home).
        root: PathBuf,
    },
    /// Explicit skip reason surfaced in reports (never silently green).
    Missing {
        /// Concrete remediation hint.
        reason: String,
    },
}

impl EnvState {
    /// One-line status for logs and CLI surfaces.
    pub fn describe(&self) -> String {
        match self {
            EnvState::Ready { root } => format!("ready ({})", root.display()),
            EnvState::Missing { reason } => format!("missing: {reason}"),
        }
    }
}

/// Per-case Terminal-Bench probe: `$SHANNON_TB_TASKS_DIR/<id>/task.yaml`
/// existence decides readiness (official layout: one directory per task
/// holding `task.yaml` + verifier artifacts).
pub fn probe_terminal_bench_case(native_id: &str, tasks_dir: Option<&Path>) -> EnvState {
    let Some(root) = tasks_dir.map(Path::to_path_buf) else {
        return EnvState::Missing {
            reason: format!(
                "terminal-bench corpus not mounted — set {} to the official \
                 tasks/ checkout",
                BenchSuite::TerminalBench.home_env_var().unwrap_or_default(),
            ),
        };
    };
    let task_dir = root.join(native_id);
    if task_dir.join("task.yaml").exists() || task_dir.join("task.yml").exists() {
        EnvState::Ready { root: task_dir }
    } else {
        EnvState::Missing {
            reason: format!(
                "pinned task '{}' absent from {} (needs <dir>/{native_id}/task.yaml)",
                native_id,
                root.display(),
            ),
        }
    }
}

/// Suite-wide SWE-bench probe: `$SHANNON_SWEBENCH_HOME` must point at an
/// existing workspace (harness checkout / dataset cache). Instance-level
/// validity stays with the official harness.
pub fn probe_swebench_env(home: Option<&Path>) -> EnvState {
    let Some(root) = home else {
        return EnvState::Missing {
            reason: format!(
                "SWE-bench harness not mounted — set {} to a workspace \
                 holding the official swebench checkout + Verified dataset",
                BenchSuite::SwebenchVerified50
                    .home_env_var()
                    .unwrap_or_default(),
            ),
        };
    };
    if root.is_dir() {
        EnvState::Ready {
            root: root.to_path_buf(),
        }
    } else {
        EnvState::Missing {
            reason: format!(
                "{} points at nonexistent path {}",
                BenchSuite::SwebenchVerified50
                    .home_env_var()
                    .unwrap_or_default(),
                root.display(),
            ),
        }
    }
}

// ── Cases ──────────────────────────────────────────────────────────────

/// One enumerated workload item before execution accounting.
#[derive(Debug, Clone)]
pub struct BenchCase {
    pub suite: BenchSuite,
    pub native_id: String,
    pub env: EnvState,
    /// Fully-local runner task (regression only).
    pub runner_task: Option<EvalTask>,
}

/// Enumerate every case for one suite against the given benchmark root and
/// environment probes. Regression failures are loud (`EvalError`) because the
/// suite is fully in-repo; remote pins degrade to per-case skips instead.
pub fn load_cases(
    suite: BenchSuite,
    bench_dir: &Path,
    tb_tasks_dir: Option<&Path>,
    sb_home: Option<&Path>,
) -> Result<Vec<BenchCase>, EvalError> {
    match suite {
        BenchSuite::Regression => {
            let dir = bench_dir.join("regression");
            let tasks = parse_tasks_dir(&dir)?;
            if tasks.len() != 10 {
                return Err(EvalError::Config(format!(
                    "regression pool expects exactly 10 pinned defect tasks, found {} in {}",
                    tasks.len(),
                    dir.display()
                )));
            }
            Ok(tasks
                .into_iter()
                .map(|runner_task| BenchCase {
                    suite,
                    native_id: runner_task.id.clone(),
                    env: EnvState::Ready {
                        root: bench_dir.to_path_buf(),
                    },
                    runner_task: Some(runner_task),
                })
                .collect())
        }
        BenchSuite::TerminalBench => {
            let pins = load_pin_manifest(suite, bench_dir)?;
            Ok(pins
                .ids
                .into_iter()
                .map(|native_id| {
                    let env = probe_terminal_bench_case(&native_id, tb_tasks_dir);
                    BenchCase {
                        suite,
                        native_id,
                        env,
                        runner_task: None,
                    }
                })
                .collect())
        }
        BenchSuite::SwebenchVerified50 => {
            let pins = load_pin_manifest(suite, bench_dir)?;
            let env = probe_swebench_env(sb_home);
            Ok(pins
                .ids
                .into_iter()
                .map(|native_id| BenchCase {
                    suite,
                    native_id,
                    env: env.clone(),
                    runner_task: None,
                })
                .collect())
        }
    }
}

// ── Delegation (real-mode remote judging) ──────────────────────────────

/// Format a harness command template. Supported variables: `{native_id}`,
/// `{task_dir}` (empty for suites without a task-dir concept). Templates may
/// not span lines — they become one `sh -c` payload.
pub fn format_harness_command(template: &str, native_id: &str, task_dir: &str) -> String {
    template
        .replace("{native_id}", native_id)
        .replace("{task_dir}", task_dir)
}

/// Refuse templated commands with newlines (single `sh -c` payload hygiene).
fn validate_template(template: &str) -> Result<(), EvalError> {
    if template.trim().is_empty() || template.lines().count() > 1 {
        return Err(EvalError::Config(
            "harness command template must be a single non-empty line".into(),
        ));
    }
    Ok(())
}

/// What the delegated verdict file is allowed to carry.
#[derive(Debug, Clone, Default)]
pub struct HarnessVerdict {
    pub resolved: Option<bool>,
    pub cost_usd: Option<f64>,
    pub tokens_in: Option<u64>,
    pub tokens_out: Option<u64>,
    pub notes: Option<String>,
}

/// Lenient verdict-file reader: anything malformed degrades to absent fields
/// (the reporter then surfaces Ambiguous rather than inventing an outcome).
pub fn parse_verdict(text: &str) -> HarnessVerdict {
    let Ok(value) = serde_json::from_str::<Value>(text.trim()) else {
        return HarnessVerdict::default();
    };
    HarnessVerdict {
        resolved: value.get("resolved").and_then(Value::as_bool),
        cost_usd: value.get("cost_usd").and_then(Value::as_f64),
        tokens_in: value.get("tokens_in").and_then(Value::as_u64),
        tokens_out: value.get("tokens_out").and_then(Value::as_u64),
        notes: value
            .get("notes")
            .and_then(Value::as_str)
            .map(str::to_owned),
    }
}

/// Outcome of one delegated invocation.
struct DelegatedOutcome {
    exit_code: Option<i32>,
    timed_out: bool,
    launch_error: Option<String>,
}

/// Default wall-clock ceiling per delegated repetition (remote pools hold
/// multi-step builds/tests; 1h matches SWE-bench harness norms).
pub const DELEGATION_DEFAULT_TIMEOUT_SECS: u64 = 3600;

/// Run one harness delegation under a wall-clock deadline; persisted stdout
/// tail doubles as forensic evidence beside the verdict file.
fn delegate_harness(
    command: &str,
    cwd: &Path,
    verdict_path: &Path,
    timeout_secs: u64,
) -> DelegatedOutcome {
    let spawned = Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .env("SHANNON_BENCH_VERDICT_FILE", verdict_path)
        .spawn();

    let mut child = match spawned {
        Ok(child) => child,
        Err(e) => {
            return DelegatedOutcome {
                exit_code: None,
                timed_out: false,
                launch_error: Some(format!("failed to spawn harness: {e}")),
            };
        }
    };

    let stdout = child.stdout.take().expect("child stdout piped");
    let collector = thread::spawn(move || {
        use std::io::{BufRead, BufReader};
        let mut collected = String::new();
        for line in BufReader::new(stdout).lines() {
            match line {
                Ok(text) => collected.push_str(&text),
                Err(_) => break,
            }
            collected.push('\n');
        }
        collected
    });

    let deadline = Duration::from_secs(timeout_secs);
    let began = Instant::now();
    let result = loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                break DelegatedOutcome {
                    exit_code: status.code(),
                    timed_out: false,
                    launch_error: None,
                };
            }
            Ok(None) if began.elapsed() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                break DelegatedOutcome {
                    exit_code: None,
                    timed_out: true,
                    launch_error: None,
                };
            }
            Ok(None) => thread::sleep(Duration::from_millis(50)),
            Err(e) => {
                break DelegatedOutcome {
                    exit_code: None,
                    timed_out: false,
                    launch_error: Some(format!("wait on harness failed: {e}")),
                };
            }
        }
    };

    // Persist whatever output made it through before any kill. On unix the
    // child dies alone here (no process-group games like §4.4 because foreign
    // harnesses manage their own containers); a hung grandchild would keep the
    // pipe open, so join defensively and fall back to an empty tail.
    let tail = if result.timed_out {
        String::new()
    } else {
        collector.join().unwrap_or_default()
    };
    if !tail.is_empty() {
        let mut clipped = tail;
        if clipped.chars().count() > 2000 {
            clipped = clipped.chars().take(2000).collect();
        }
        let _ = std::fs::write(cwd.join("harness-stdout.tail"), &clipped);
    }
    result
}

// ── Dispositions & records ─────────────────────────────────────────────

/// Outcome classes understood at the benchmark layer. `Passed` from the
/// internal runner maps onto [`BenchDisposition::Resolved`] because for the
/// regression pool "passed" *means* the issue-specific criterion held.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BenchDisposition {
    /// Native criterion judged satisfied.
    Resolved,
    /// Native criterion judged unsatisfied (or process failed).
    Failed,
    /// Wall-clock budget exhausted mid-delegation.
    Timeout,
    /// Corpus/harness unavailable — skippd, annotated, never scored green.
    SkippedEnvMissing,
    /// Ready environment but no judge configured; refusing to self-judge.
    SkippedJudgeUnconfigured,
    /// Dry-run rehearsal of a remote suite: enumeration validated, nothing ran.
    NotExecutedDryRun,
    /// Process exited cleanly without emitting a usable verdict.
    Ambiguous,
    /// Harness could not even be launched.
    SpawnError,
}

impl BenchDisposition {
    pub fn as_str(&self) -> &'static str {
        match self {
            BenchDisposition::Resolved => "resolved",
            BenchDisposition::Failed => "failed",
            BenchDisposition::Timeout => "timeout",
            BenchDisposition::SkippedEnvMissing => "skipped_env_missing",
            BenchDisposition::SkippedJudgeUnconfigured => "skipped_judge_unconfigured",
            BenchDisposition::NotExecutedDryRun => "not_executed_dry_run",
            BenchDisposition::Ambiguous => "ambiguous",
            BenchDisposition::SpawnError => "spawn_error",
        }
    }

    /// Counts toward the resolved-rate numerators of citable scores.
    pub fn is_scored(&self) -> bool {
        matches!(self, BenchDisposition::Resolved | BenchDisposition::Failed)
    }
}

/// One repetition outcome for one case.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BenchRepRecord {
    pub rep_index: usize,
    pub disposition: BenchDisposition,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    /// Failure classification from §4.7 (regression reps reuse the rule
    /// table; delegated reps carry none — the harness owns judgment).
    #[serde(default)]
    pub failure_class: Option<String>,
    /// §4.7 metrics blob for pipeline-executed reps; `null` when honestly
    /// unobservable (skips, delegated externals, ambiguous spawns).
    #[serde(default)]
    pub metrics: Option<TaskMetrics>,
    /// Optional passthrough from a delegated verdict file (foreign scope).
    #[serde(default)]
    pub external_metrics: Option<Value>,
    #[serde(default)]
    pub note: String,
}

/// Aggregated per-case view across the n repetitions.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BenchCaseAggregate {
    pub native_id: String,
    /// Terse criteria lineage for the row (pool judge statement).
    pub criteria: String,
    pub reps: Vec<BenchRepRecord>,
    pub resolved_reps: usize,
    pub failed_reps: usize,
    pub skipped_reps: usize,
    /// Mechanical variance-attribution facts (no invented causes).
    pub variance_notes: Vec<String>,
    pub cost_usd_sum: Option<f64>,
    pub tokens_in_sum: u64,
    pub tokens_out_sum: u64,
}

// ── Aggregations ───────────────────────────────────────────────────────

/// Build the per-case aggregate from raw repetition records, folding §4.7
/// cost/token sums and emitting mechanical variance facts.
pub fn aggregate_case(
    native_id: &str,
    criteria: &str,
    reps: Vec<BenchRepRecord>,
) -> BenchCaseAggregate {
    let mut costs: Vec<Option<f64>> = Vec::new();
    let mut tokens: Vec<(u64, u64)> = Vec::new();
    let mut status_sequence: Vec<&str> = Vec::new();

    for rep in &reps {
        costs.push(match &rep.external_metrics {
            // Delegated foreign-scope passthrough wins when present…
            Some(ext) => ext.get("cost_usd").and_then(Value::as_f64),
            // …otherwise the pipeline-extracted metric feeds the column.
            None => rep.metrics.as_ref().and_then(|m| m.cost_usd),
        });
        tokens.push(match (&rep.external_metrics, &rep.metrics) {
            (Some(ext), _) => (
                ext.get("tokens_in").and_then(Value::as_u64).unwrap_or(0),
                ext.get("tokens_out").and_then(Value::as_u64).unwrap_or(0),
            ),
            (None, Some(m)) => (m.tokens_in, m.tokens_out),
            (None, None) => (0, 0),
        });
        status_sequence.push(rep.disposition.as_str());
    }

    let resolved_reps = reps
        .iter()
        .filter(|r| r.disposition == BenchDisposition::Resolved)
        .count();
    let failed_reps = reps
        .iter()
        .filter(|r| {
            matches!(
                r.disposition,
                BenchDisposition::Failed | BenchDisposition::Timeout | BenchDisposition::Ambiguous
            )
        })
        .count();
    let skipped_reps = reps.iter().filter(|r| !r.disposition.is_scored()).count();

    let variance_notes = attribute_variance(&status_sequence, &tokens);
    let (tokens_in_sum, tokens_out_sum) = tokens
        .iter()
        .fold((0u64, 0u64), |(i, o), (tin, tout)| (i + tin, o + tout));

    let cost_usd_sum: Option<f64> = if costs.iter().any(Option::is_none) {
        None // honest unknown: one missing observation voids the sum
    } else {
        Some(costs.iter().flatten().sum())
    };

    BenchCaseAggregate {
        native_id: native_id.to_string(),
        criteria: criteria.to_string(),
        reps,
        resolved_reps,
        failed_reps,
        skipped_reps,
        variance_notes,
        cost_usd_sum,
        tokens_in_sum,
        tokens_out_sum,
    }
}

/// Mechanical variance attribution: one fact per observable dimension.
/// Status flip detection compares successive scored repetitions; budget
/// spread flags >20% relative movement between the smallest/largest token
/// totals. Deliberately cause-blind — classification lives in the readers.
pub fn attribute_variance(status_sequence: &[&str], token_totals: &[(u64, u64)]) -> Vec<String> {
    let mut notes = Vec::new();
    let mut flips: BTreeMap<(String, String), usize> = BTreeMap::new();
    for pair in status_sequence.windows(2) {
        if pair[0] != pair[1] {
            *flips
                .entry((pair[0].to_string(), pair[1].to_string()))
                .or_default() += 1;
        }
    }
    for ((from, to), count) in &flips {
        notes.push(format!(
            "status_flip: {count}× {from}->{to} between consecutive reps"
        ));
    }
    if let (Some(min_i), Some(max_i)) = (
        token_totals
            .iter()
            .enumerate()
            .min_by_key(|(_, t)| t.0 + t.1),
        token_totals
            .iter()
            .enumerate()
            .max_by_key(|(_, t)| t.0 + t.1),
    ) {
        if min_i.0 != max_i.0 {
            let lo = min_i.1.0 + min_i.1.1;
            let hi = max_i.1.0 + max_i.1.1;
            if hi > 0 {
                let pct = ((hi.saturating_sub(lo)) as f64) / (hi as f64) * 100.0;
                if pct > 20.0 {
                    notes.push(format!(
                        "token_spread: rep{}={} vs rep{}={} total tokens ({pct:.1}% band)",
                        min_i.0 + 1,
                        lo,
                        max_i.0 + 1,
                        hi
                    ));
                }
            }
        }
    }
    if notes.is_empty() {
        notes.push("variance: none detected across recorded dimensions".into());
    }
    notes
}

// ── Report ─────────────────────────────────────────────────────────────

/// Options controlling one benchmark execution (one suite = one report).
#[derive(Debug, Clone)]
pub struct BenchmarkOptions {
    /// Suite to run.
    pub suite: BenchSuite,
    /// Repetition count; external references REQUIRE [`N_RUNS_REQUIRED`].
    pub n_runs: usize,
    /// Pipeline rehearsal (no models, consistent with the §4.4 posture).
    pub dry_run: bool,
    /// Override output root; default `$SHANNON_HOME/eval/benchmarks/<slug>/`.
    pub out_root_override: Option<PathBuf>,
    /// Failure-rule table override forwarded to the §4.7 classifier.
    pub failure_rules: Option<PathBuf>,
    /// Terminal-Bench corpus mount override (else `$SHANNON_TB_TASKS_DIR`).
    pub tb_tasks_dir: Option<PathBuf>,
    /// SWE-bench harness home override (else `$SHANNON_SWEBENCH_HOME`).
    pub sb_home: Option<PathBuf>,
    /// Delegation template override (else the suite's `*_HARNESS_CMD` var).
    pub harness_cmd: Option<String>,
    /// Wall-clock ceiling per delegated repetition.
    pub delegation_timeout_secs: u64,
}

impl Default for BenchmarkOptions {
    fn default() -> Self {
        Self {
            suite: BenchSuite::Regression,
            n_runs: N_RUNS_REQUIRED,
            dry_run: true,
            out_root_override: None,
            failure_rules: None,
            tb_tasks_dir: None,
            sb_home: None,
            harness_cmd: None,
            delegation_timeout_secs: DELEGATION_DEFAULT_TIMEOUT_SECS,
        }
    }
}

/// External-reference discipline: citability demands n≥3 (DP1 allows the
/// spend — shorter series stay uncitable, not forbidden).
pub const N_RUNS_REQUIRED: usize = 3;

/// Citation governance block embedded in every report.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CitationBlock {
    /// True iff every blocker cleared; external notes may cite ONLY then.
    pub citable: bool,
    /// Why this artifact may not (yet) be quoted externally.
    pub blockers: Vec<String>,
    /// RFC3339 UTC date attached to any (future) external quotation.
    pub started_at_utc: String,
    /// Judge lineage sentence from [`BenchSuite::judge_statement`].
    pub criteria: String,
    /// Workload fingerprint(s) binding score↔dataset slice.
    pub workload_fingerprint: String,
}

/// One suite's report: dual JSON/MD on disk, digest-comparable across versions.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BenchReport {
    pub run_id: String,
    pub suite: String,
    pub started_at_utc: String,
    pub dry_run: bool,
    pub n_runs: usize,
    pub cases_total: usize,
    /// Fully-scored repetitions summed across cases (denominator ingredient).
    pub resolved_events: usize,
    /// Pass-rate interval across fully-executed repetitions (min/max),
    /// `null` when repetitions did not uniformly execute (dry / partial).
    pub resolved_rate_interval: Option<[f64; 2]>,
    /// Σ observed cost ÷ Σ resolved events (§4.7-compatible column),
    /// `null` while any observation is missing or denominator is zero.
    pub cost_per_resolved_usd: Option<f64>,
    /// Provenance stamp mirroring the §4.7 vocabulary (`derived_stream`,
    /// `events_jsonl`, `external_verdict`, or `none`).
    pub metrics_source: String,
    pub app_version: String,
    pub failure_rules_fingerprint: String,
    pub pin_manifest_file: Option<String>,
    pub pin_manifest_fingerprint: Option<String>,
    pub pin_validation: String,
    pub citation: CitationBlock,
    pub records: Vec<BenchCaseAggregate>,
}

fn utc_now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// UTC compact stamp + entropy — mirrors the §4.4 run-id discipline.
fn fresh_run_id(prefix: &str) -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_secs();
    let hex = uuid::Uuid::new_v4().as_simple().to_string();
    format!(
        "{prefix}-{}-{}",
        chrono::DateTime::from_timestamp(secs as i64, 0)
            .expect("valid epoch seconds")
            .format("%Y%m%d%H%M%S"),
        &hex[..8]
    )
}

/// Version-stability digest: drops timestamps/durations/paths, keeps
/// everything that makes cross-version comparison meaningful.
impl BenchReport {
    pub fn stable_digest(&self) -> String {
        let body = json!({
            "suite": self.suite,
            "dry_run": self.dry_run,
            "n_runs": self.n_runs,
            "cases_total": self.cases_total,
            "resolved_events": self.resolved_events,
            "resolved_rate_interval": self.resolved_rate_interval,
            "cost_per_resolved_usd": self.cost_per_resolved_usd,
            "metrics_source": self.metrics_source,
            "app_version": self.app_version,
            "failure_rules_fingerprint": self.failure_rules_fingerprint,
            "pin_manifest_fingerprint": self.pin_manifest_fingerprint,
            "citation_blockers": self.citation.blockers,
            "records": self.records.iter().map(|rec| json!({
                "native_id": rec.native_id,
                "criteria": rec.criteria,
                "resolved_reps": rec.resolved_reps,
                "failed_reps": rec.failed_reps,
                "skipped_reps": rec.skipped_reps,
                "variance_notes": rec.variance_notes,
                "cost_usd_sum": rec.cost_usd_sum,
                "tokens_in_sum": rec.tokens_in_sum,
                "tokens_out_sum": rec.tokens_out_sum,
                "dispositions": rec.reps.iter().map(|r| r.disposition.as_str()).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        });
        serde_json::to_string_pretty(&body).expect("bench digest serialization")
    }

    /// Human-readable companion of `bench-report.json`.
    pub fn render_markdown(&self) -> String {
        let mode = if self.dry_run { "DRY-RUN" } else { "REAL" };
        let mut md = String::new();
        md.push_str(&format!(
            "# Shannon Benchmark Report — {} (`{}`, {mode})\n\n\
             - Started: {}\n\
             - Runs per case: n={}\n\
             - Criteria: {}\n\
             - Metrics source: {}\n\
             - Engine: {}\n\n",
            self.suite_title_lookup(),
            self.run_id,
            self.started_at_utc,
            self.n_runs,
            self.citation.criteria,
            self.metrics_source,
            if self.app_version.is_empty() {
                "-"
            } else {
                &self.app_version
            },
        ));

        md.push_str("## Score\n\n");
        match self.resolved_rate_interval {
            Some([lo, hi]) => md.push_str(&format!(
                "- resolved rate [{lo:.3}, {hi:.3}] over {} cases · n={} \
                 (对外引用请附 n 与日期)\n",
                self.cases_total, self.n_runs
            )),
            None => md.push_str(
                "- resolved rate: pending (repetitions did not uniformly execute; \
                 n placeholder retained)\n",
            ),
        }
        match self.cost_per_resolved_usd {
            Some(cpr) => md.push_str(&format!("- cost-per-resolved: ${cpr:.4}\n")),
            None => md.push_str(
                "- cost-per-resolved: null (no complete cost observations — \
                 honestly unknown)\n",
            ),
        }

        md.push_str("\n## Citability\n\n");
        if self.citation.citable {
            md.push_str("- CITABLE: all external-reference gates cleared.\n");
        } else {
            for blocker in &self.citation.blockers {
                md.push_str(&format!("- NOT CITABLE: {blocker}\n"));
            }
        }

        md.push_str("\n## Cases\n\n");
        md.push_str("| case | disposition histogram (rep1..n) | resolved | failed | skipped | tokens in/out | cost_usd | variance notes |\n");
        md.push_str("|---|---|---|---|---|---|---|---|\n");
        for rec in &self.records {
            let hist = rec
                .reps
                .iter()
                .map(|r| r.disposition.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            let cost = rec
                .cost_usd_sum
                .map_or_else(|| "null".into(), |c| format!("{c:.4}"));
            md.push_str(&format!(
                "| {} | [{}] | {} | {} | {} | {}/{} | {} | {} |\n",
                rec.native_id,
                hist,
                rec.resolved_reps,
                rec.failed_reps,
                rec.skipped_reps,
                rec.tokens_in_sum,
                rec.tokens_out_sum,
                cost,
                rec.variance_notes.join("; "),
            ));
        }

        md.push_str("\n## Provenance\n\n");
        if let (Some(file), Some(fp)) = (&self.pin_manifest_file, &self.pin_manifest_fingerprint) {
            md.push_str(&format!("- pinned workload: `{file}` @ `{fp}`\n"));
        }
        md.push_str(&format!("- pin validation: {}\n", self.pin_validation));
        md.push_str(&format!(
            "- failure-rule table: `{}`\n",
            if self.failure_rules_fingerprint.is_empty() {
                "-"
            } else {
                &self.failure_rules_fingerprint
            }
        ));
        md
    }

    fn suite_title_lookup(&self) -> &str {
        match BenchSuite::ALL.iter().find(|s| s.slug() == self.suite) {
            Some(suite) => suite.title(),
            None => self.suite.as_str(),
        }
    }
}

/// Volatility-free projection of one case — shared by the suite digest and
/// the cross-version comparer so neither can drift apart silently.
fn case_stable_view(record: &BenchCaseAggregate) -> Value {
    json!({
        "native_id": record.native_id,
        "criteria": record.criteria,
        "resolved_reps": record.resolved_reps,
        "failed_reps": record.failed_reps,
        "skipped_reps": record.skipped_reps,
        "variance_notes": record.variance_notes,
        "cost_usd_sum": record.cost_usd_sum,
        "tokens_in_sum": record.tokens_in_sum,
        "tokens_out_sum": record.tokens_out_sum,
        "dispositions": record.reps.iter().map(|r| r.disposition.as_str()).collect::<Vec<_>>(),
    })
}

/// Diff verdict across two suite reports — version anchors first, then
/// per-case drift. Structure-stable when digests agree.
pub fn compare_bench_reports(a: &BenchReport, b: &BenchReport) -> String {
    if a.stable_digest() == b.stable_digest() {
        return format!(
            "STABLE: structural digest identical ({} cases, suite {})",
            a.cases_total, a.suite
        );
    }
    let mut out = String::from("UNSTABLE: digests differ\n");

    out.push_str("\n[meta]\n");
    for (label, left, right) in [
        (
            "app_version",
            a.app_version.as_str(),
            b.app_version.as_str(),
        ),
        (
            "failure_rules_fingerprint",
            a.failure_rules_fingerprint.as_str(),
            b.failure_rules_fingerprint.as_str(),
        ),
        (
            "metrics_source",
            a.metrics_source.as_str(),
            b.metrics_source.as_str(),
        ),
        ("n_runs", &a.n_runs.to_string(), &b.n_runs.to_string()),
        (
            "pin_manifest_fingerprint",
            a.pin_manifest_fingerprint.as_deref().unwrap_or("-"),
            b.pin_manifest_fingerprint.as_deref().unwrap_or("-"),
        ),
        (
            "resolved_rate_interval",
            &a.resolved_rate_interval.map_or_else(
                || "none".into(),
                |iv| format!("[{:.3}, {:.3}]", iv[0], iv[1]),
            ),
            &b.resolved_rate_interval.map_or_else(
                || "none".into(),
                |iv| format!("[{:.3}, {:.3}]", iv[0], iv[1]),
            ),
        ),
    ] {
        let mark = if left == right { "=" } else { "->" };
        out.push_str(&format!("  {label} {mark} {left} -> {right}\n"));
    }

    for (ra, rb) in a.records.iter().zip(b.records.iter()) {
        let va = case_stable_view(ra);
        let vb = case_stable_view(rb);
        if va == vb {
            continue;
        }
        out.push_str(&format!("\n[{}] drift\n", ra.native_id));
        for key in [
            "resolved_reps",
            "failed_reps",
            "skipped_reps",
            "tokens_in_sum",
            "tokens_out_sum",
            "cost_usd_sum",
        ] {
            if va.get(key) != vb.get(key) {
                out.push_str(&format!(
                    "  {key}: {} -> {}\n",
                    va.get(key).map_or("?".into(), ToString::to_string),
                    vb.get(key).map_or("?".into(), ToString::to_string),
                ));
            }
        }
        let da: Vec<&str> = ra.reps.iter().map(|r| r.disposition.as_str()).collect();
        let db: Vec<&str> = rb.reps.iter().map(|r| r.disposition.as_str()).collect();
        if da != db {
            out.push_str(&format!("  dispositions: {da:?} -> {db:?}\n"));
        }
    }
    out
}

// ── Execution orchestration ────────────────────────────────────────────

/// Output-root resolution: override → `$SHANNON_HOME`/`~/.shannon` +
/// `eval/benchmarks/<slug>` (§4.4-sibling convention).
pub fn bench_output_root(options: &BenchmarkOptions) -> PathBuf {
    match &options.out_root_override {
        Some(dir) => dir.clone(),
        None => crate::testing::eval_runner::resolve_eval_home()
            .unwrap_or_else(|_| std::env::temp_dir().join("shannon-bench"))
            .join("eval")
            .join("benchmarks")
            .join(options.suite.slug()),
    }
}

fn env_or_option(option: &Option<PathBuf>, var: &str) -> Option<PathBuf> {
    option
        .clone()
        .or_else(|| std::env::var(var).ok().map(PathBuf::from))
}

fn env_str_option(option: &Option<String>, var: &str) -> Option<String> {
    option.clone().or_else(|| std::env::var(var).ok())
}

/// Execute one benchmark suite end-to-end and persist the dual report.
///
/// Regression reps flow through the genuine §4.4 runner (per-rep suites land
/// beside the bench artifacts); remote suites honor the skip/delegate
/// contracts above. Failing loud on `EvalError` keeps configuration bugs out
/// of "reports"; per-case problems become visible dispositions instead.
pub fn run_benchmark(
    options: &BenchmarkOptions,
    bench_dir: &Path,
) -> Result<(BenchReport, PathBuf), EvalError> {
    let rules = resolve_failure_rules(options.failure_rules.as_deref())?;
    let tb_tasks = env_or_option(&options.tb_tasks_dir, "SHANNON_TB_TASKS_DIR");
    let sb_home = env_or_option(&options.sb_home, "SHANNON_SWEBENCH_HOME");
    let harness_cmd = env_str_option(
        &options.harness_cmd,
        options.suite.harness_cmd_env_var().unwrap_or(""),
    );

    let cases = load_cases(
        options.suite,
        bench_dir,
        tb_tasks.as_deref(),
        sb_home.as_deref(),
    )?;

    let out_root = bench_output_root(options);
    let run_id = fresh_run_id("bench");
    let run_dir = out_root.join(&run_id);
    std::fs::create_dir_all(&run_dir)?;
    let started_at = utc_now_rfc3339();

    let pin = match options.suite.pin_file_name() {
        Some(name) => Some((
            name.to_string(),
            load_pin_manifest(options.suite, bench_dir)?,
        )),
        None => None,
    };

    let mut records = Vec::with_capacity(cases.len());

    for case in &cases {
        let mut reps: Vec<BenchRepRecord> = Vec::with_capacity(options.n_runs);

        for rep_index in 1..=options.n_runs {
            let rep = match options.suite {
                BenchSuite::Regression => run_regression_rep(case, options, &run_dir, rep_index),
                BenchSuite::TerminalBench | BenchSuite::SwebenchVerified50 => {
                    run_remote_rep(case, options, harness_cmd.as_deref(), &run_dir, rep_index)
                }
            };
            reps.push(rep);
        }

        let criteria = options.suite.judge_statement().to_string();
        records.push(aggregate_case(&case.native_id, &criteria, reps));
    }

    // Suite rollups (mechanical; nothing invented for absent dimensions).
    let fully_executed = records
        .iter()
        .all(|r| r.skipped_reps == 0 && r.reps.len() == options.n_runs);
    let mut resolved_events = 0usize;
    let mut cost_observations: Vec<Option<f64>> = Vec::new();
    for record in &records {
        for rep in &record.reps {
            if rep.disposition == BenchDisposition::Resolved {
                resolved_events += 1;
            }
        }
        cost_observations.push(record.cost_usd_sum);
    }
    let rate_interval = if fully_executed && options.n_runs > 0 {
        let per_rep: Vec<usize> = (0..options.n_runs)
            .map(|i| {
                records
                    .iter()
                    .filter(|r| r.reps[i].disposition == BenchDisposition::Resolved)
                    .count()
            })
            .collect();
        let total = records.len() as f64;
        let lo = per_rep.iter().copied().min().unwrap_or(0) as f64 / total;
        let hi = per_rep.iter().copied().max().unwrap_or(0) as f64 / total;
        Some([lo, hi])
    } else {
        None
    };
    let cost_per_resolved = if cost_observations.iter().any(Option::is_none) || resolved_events == 0
    {
        None
    } else {
        let total_cost: f64 = cost_observations.iter().flatten().sum();
        Some(total_cost / resolved_events as f64)
    };

    let metrics_source = derive_metrics_source(options.suite, &records);

    let mut blockers = Vec::new();
    if options.dry_run {
        blockers
            .push("mock口径: dry-run rehearsal (§4.4 posture) — no model/engine executed".into());
    }
    if options.suite != BenchSuite::Regression && pin.is_none() {
        blockers.push("missing pinned workload manifest".into());
    }
    if options.n_runs < N_RUNS_REQUIRED {
        blockers.push(format!(
            "n_runs={} below the externally-citable minimum of {N_RUNS_REQUIRED}",
            options.n_runs
        ));
    }
    if metrics_source == "none" && !options.dry_run {
        blockers.push("no metric source observed (nothing executed)".into());
    }
    if options.suite != BenchSuite::Regression {
        blockers.push(
            "pin-vs-corpus validation pending: workload IDs require mechanical \
             confirmation on first contact with the official corpus before citing"
                .into(),
        );
    }

    let report = BenchReport {
        run_id,
        suite: options.suite.slug().to_string(),
        started_at_utc: started_at.clone(),
        dry_run: options.dry_run,
        n_runs: options.n_runs,
        cases_total: records.len(),
        resolved_events,
        resolved_rate_interval: rate_interval,
        cost_per_resolved_usd: cost_per_resolved,
        metrics_source: metrics_source.clone(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        failure_rules_fingerprint: rules.fingerprint().to_string(),
        pin_manifest_file: pin.as_ref().map(|(name, _)| name.clone()),
        pin_manifest_fingerprint: pin
            .as_ref()
            .map(|(_, manifest)| manifest.fingerprint.clone()),
        pin_validation: if options.suite == BenchSuite::Regression {
            "not_applicable".to_string()
        } else {
            "pending_corpus_contact".to_string()
        },
        citation: CitationBlock {
            citable: blockers.is_empty(),
            blockers,
            started_at_utc: started_at,
            criteria: options.suite.judge_statement().to_string(),
            workload_fingerprint: pin
                .as_ref()
                .map(|(_, m)| m.fingerprint.clone())
                .unwrap_or_else(|| format!("toml-dir@{}", fingerprint_ids(&[]))),
        },
        records,
    };

    std::fs::write(
        run_dir.join("bench-report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    std::fs::write(run_dir.join("bench-report.md"), report.render_markdown())?;

    Ok((report, run_dir))
}

/// Load a persisted `bench-report.json` for the diff workflow.
pub fn load_bench_report(path: &Path) -> Result<BenchReport, EvalError> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| EvalError::Config(format!("cannot read {}: {e}", path.display())))?;
    serde_json::from_str(&raw).map_err(|e| EvalError::Config(format!("{}: {e}", path.display())))
}

/// Map aggregate observations onto the §4.7 provenance vocabulary.
fn derive_metrics_source(suite: BenchSuite, records: &[BenchCaseAggregate]) -> String {
    let executed_reps = || records.iter().flat_map(|r| r.reps.iter());
    if suite == BenchSuite::Regression {
        if executed_reps().any(|rep| {
            rep.metrics
                .as_ref()
                .is_some_and(|m| m.source == MetricSource::EventsLog)
        }) {
            "events_jsonl".to_string()
        } else if executed_reps().any(|rep| rep.metrics.is_some()) {
            "derived_stream".to_string()
        } else {
            "none".to_string()
        }
    } else if executed_reps().any(|rep| rep.external_metrics.is_some()) {
        "external_verdict".to_string()
    } else {
        "none".to_string()
    }
}

/// One regression repetition through the authentic §4.4 pipeline.
fn run_regression_rep(
    case: &BenchCase,
    options: &BenchmarkOptions,
    run_dir: &Path,
    rep_index: usize,
) -> BenchRepRecord {
    let Some(task) = &case.runner_task else {
        return BenchRepRecord {
            rep_index,
            disposition: BenchDisposition::SpawnError,
            exit_code: None,
            duration_ms: 0,
            failure_class: None,
            metrics: None,
            external_metrics: None,
            note: "regression case lost its runner task".into(),
        };
    };
    let rep_options = EvalOptions {
        bin_path: None,
        dry_run: options.dry_run,
        out_dir_override: Some(run_dir.join(format!("{}_rep{}", case.native_id, rep_index))),
        failure_rules: options.failure_rules.clone(),
        instruction_directive: None,
    };
    let began = Instant::now();
    match run_suite(std::slice::from_ref(task), &rep_options) {
        Ok((inner, _suite_dir)) => {
            let record = &inner.records[0];
            let disposition = match record.status {
                RunStatus::Passed => BenchDisposition::Resolved,
                RunStatus::Failed => BenchDisposition::Failed,
                RunStatus::Timeout => BenchDisposition::Timeout,
                RunStatus::SpawnError => BenchDisposition::SpawnError,
                RunStatus::TurnLimit | RunStatus::TokenLimit => BenchDisposition::Failed,
            };
            BenchRepRecord {
                rep_index,
                disposition,
                exit_code: record.exit_code,
                duration_ms: began.elapsed().as_millis() as u64,
                failure_class: record.failure_class.clone(),
                metrics: record.metrics.clone(),
                external_metrics: None,
                note: record.violations.first().cloned().unwrap_or_default(),
            }
        }
        Err(e) => BenchRepRecord {
            rep_index,
            disposition: BenchDisposition::SpawnError,
            exit_code: None,
            duration_ms: began.elapsed().as_millis() as u64,
            failure_class: None,
            metrics: None,
            external_metrics: None,
            note: format!("pipeline error: {e}"),
        },
    }
}

/// One remote-suite repetition honoring skip/delegate contracts.
#[allow(clippy::too_many_arguments)]
fn run_remote_rep(
    case: &BenchCase,
    options: &BenchmarkOptions,
    harness_cmd: Option<&str>,
    run_dir: &Path,
    rep_index: usize,
) -> BenchRepRecord {
    // Rehearsal mode never touches foreign corpora.
    if options.dry_run {
        return skipped_record(
            rep_index,
            BenchDisposition::NotExecutedDryRun,
            "enumeration-only rehearsal; corpus untouched",
        );
    }

    if let EnvState::Missing { reason } = &case.env {
        return skipped_record(rep_index, BenchDisposition::SkippedEnvMissing, reason);
    }

    let Some(template) = harness_cmd else {
        return skipped_record(
            rep_index,
            BenchDisposition::SkippedJudgeUnconfigured,
            &format!(
                "environment ready but no judge delegated — set {} (template \
                 with {{native_id}}); Shannon refuses to self-judge foreign pools",
                options.suite.harness_cmd_env_var().unwrap_or("<cmd-var>"),
            ),
        );
    };
    if let Err(e) = validate_template(template) {
        return BenchRepRecord {
            rep_index,
            disposition: BenchDisposition::SpawnError,
            exit_code: None,
            duration_ms: 0,
            failure_class: None,
            metrics: None,
            external_metrics: None,
            note: e.to_string(),
        };
    }

    let workspace = run_dir.join(format!("{}_rep{}", case.native_id, rep_index));
    if std::fs::create_dir_all(&workspace).is_err() {
        return BenchRepRecord {
            rep_index,
            disposition: BenchDisposition::SpawnError,
            exit_code: None,
            duration_ms: 0,
            failure_class: None,
            metrics: None,
            external_metrics: None,
            note: "cannot create repetition workspace".into(),
        };
    }
    let task_dir_for_template = match &case.env {
        EnvState::Ready { root } => root.display().to_string(),
        EnvState::Missing { .. } => String::new(),
    };
    let command = format_harness_command(template, &case.native_id, &task_dir_for_template);
    let verdict_path = workspace.join("verdict.json");
    let began = Instant::now();
    let outcome = delegate_harness(
        &command,
        &workspace,
        &verdict_path,
        options.delegation_timeout_secs,
    );

    let verdict = std::fs::read_to_string(&verdict_path)
        .map(|text| parse_verdict(&text))
        .unwrap_or_default();

    let mut record = if let Some(error) = outcome.launch_error {
        skipped_record(rep_index, BenchDisposition::SpawnError, &error)
    } else if outcome.timed_out {
        skipped_record(
            rep_index,
            BenchDisposition::Timeout,
            &format!(
                "delegation exceeded {}s wall-clock budget",
                options.delegation_timeout_secs
            ),
        )
    } else if outcome.exit_code == Some(0) {
        match verdict.resolved {
            Some(true) => BenchRepRecord {
                disposition: BenchDisposition::Resolved,
                note: verdict.notes.clone().unwrap_or_default(),
                ..dummy_record(rep_index)
            },
            Some(false) => BenchRepRecord {
                disposition: BenchDisposition::Failed,
                note: verdict.notes.clone().unwrap_or_default(),
                ..dummy_record(rep_index)
            },
            None => skipped_record(
                rep_index,
                BenchDisposition::Ambiguous,
                "exit 0 without usable verdict.json — not counted",
            ),
        }
    } else {
        BenchRepRecord {
            disposition: BenchDisposition::Failed,
            note: format!(
                "harness exited {:?}; {}",
                outcome.exit_code,
                verdict.notes.clone().unwrap_or_default()
            ),
            ..dummy_record(rep_index)
        }
    };

    if verdict.cost_usd.is_some() || verdict.tokens_in.is_some() || verdict.tokens_out.is_some() {
        record.external_metrics = Some(json!({
            "cost_usd": verdict.cost_usd,
            "tokens_in": verdict.tokens_in,
            "tokens_out": verdict.tokens_out,
        }));
    }
    record.exit_code = outcome.exit_code;
    record.duration_ms = began.elapsed().as_millis() as u64;
    record
}

/// Placeholder-free skip builder — every field explicitly honest.
fn skipped_record(rep_index: usize, disposition: BenchDisposition, reason: &str) -> BenchRepRecord {
    BenchRepRecord {
        rep_index,
        disposition,
        exit_code: None,
        duration_ms: 0,
        failure_class: None,
        metrics: None,
        external_metrics: None,
        note: reason.to_string(),
    }
}

fn dummy_record(rep_index: usize) -> BenchRepRecord {
    skipped_record(rep_index, BenchDisposition::Ambiguous, "")
}

// Convenience alias so downstream callers share the §4.7 rule resolution.
pub fn bench_failure_rules(explicit: Option<&Path>) -> Result<FailureRules, EvalError> {
    resolve_failure_rules(explicit)
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Pin parsing & fingerprints ─────────────────────────────────────

    const SB_LIST: &str = "# pinned 50-subset sample\n\
         astropy__astropy-12907\n\
         \n\
         django__django-11099 # canonical demo instance\n";

    #[test]
    fn parse_pin_list_keeps_order_strips_comments_and_fingerprints() {
        let manifest = parse_pin_list(BenchSuite::SwebenchVerified50, SB_LIST).expect("parses");
        assert_eq!(
            manifest.ids,
            vec![
                "astropy__astropy-12907".to_string(),
                "django__django-11099".to_string(),
            ]
        );
        assert_eq!(manifest.fingerprint, fingerprint_ids(&manifest.ids));
        // Same content ⇒ same fingerprint; different content ⇒ different.
        let again = parse_pin_list(BenchSuite::SwebenchVerified50, SB_LIST).expect("again");
        assert_eq!(again.fingerprint, manifest.fingerprint);
        let drifted = parse_pin_list(
            BenchSuite::SwebenchVerified50,
            &SB_LIST.replace("11099", "11100"),
        )
        .expect("drift parses");
        assert_ne!(drifted.fingerprint, manifest.fingerprint);
    }

    #[test]
    fn parse_pin_list_enforces_native_shapes_and_nonemptiness() {
        let bad_sb = "ownerrepo-nope\n";
        assert!(parse_pin_list(BenchSuite::SwebenchVerified50, bad_sb).is_err());
        let bad_tb = "Upper_Case\n";
        assert!(parse_pin_list(BenchSuite::TerminalBench, bad_tb).is_err());
        let empty_tb = "# only comments\n";
        assert!(parse_pin_list(BenchSuite::TerminalBench, empty_tb).is_err());
        let good_tb = "hello-world\npolyglot_c.py\n";
        assert!(parse_pin_list(BenchSuite::TerminalBench, good_tb).is_ok());
        // Regression pool declares no pin list at all.
        assert!(parse_pin_list(BenchSuite::Regression, "anything").is_err());
    }

    // ── Environment probing ───────────────────────────────────────────

    #[test]
    fn terminal_bench_probe_reads_mounted_task_directories() {
        let dir = tempfile::TempDir::new().expect("tmp");
        std::fs::create_dir_all(dir.path().join("hello-world")).expect("mkdir");
        std::fs::write(dir.path().join("hello-world/task.yaml"), "instruction: x")
            .expect("seed yaml");

        let ready = probe_terminal_bench_case("hello-world", Some(dir.path()));
        assert!(
            matches!(ready, EnvState::Ready { .. }),
            "mounted task dir must read ready: {ready:?}"
        );
        let missing = probe_terminal_bench_case("absent-task", Some(dir.path()));
        match &missing {
            EnvState::Missing { reason } => assert!(reason.contains("absent-task"), "{reason}"),
            other => panic!("expected explicit missing, got {other:?}"),
        }
        let unmounted = probe_terminal_bench_case("hello-world", None);
        assert!(matches!(unmounted, EnvState::Missing { .. }));
    }

    #[test]
    fn swebench_probe_requires_existing_home() {
        let missing_unset = probe_swebench_env(None);
        assert!(
            matches!(&missing_unset, EnvState::Missing { reason } if reason.contains("SHANNON_SWEBENCH_HOME"))
        );
        let bogus = tempfile::TempDir::new().expect("tmp");
        let bogus_path = bogus.path().join("gone");
        let missing_bad = probe_swebench_env(Some(&bogus_path));
        assert!(matches!(missing_bad, EnvState::Missing { .. }));
        let home = tempfile::TempDir::new().expect("home");
        assert!(matches!(
            probe_swebench_env(Some(home.path())),
            EnvState::Ready { .. }
        ));
    }

    // ── Delegation plumbing ───────────────────────────────────────────

    #[test]
    fn harness_command_templates_substitute_placeholders() {
        let rendered = format_harness_command(
            "tb run --task-id {native_id} --task-path {task_dir}",
            "hello-world",
            "/corp/tasks/hello-world",
        );
        assert!(rendered.contains("--task-id hello-world"));
        assert!(rendered.contains("/corp/tasks/hello-world"));
        assert!(validate_template(rendered.as_str()).is_ok());
        assert!(validate_template("two\nlines").is_err());
        assert!(validate_template("   ").is_err());
    }

    #[test]
    fn verdict_files_parse_leniently() {
        let full = parse_verdict(
            r#"{"resolved": true, "cost_usd": 0.12, "tokens_in": 11, "tokens_out": 7, "notes": "f2p green"}"#,
        );
        assert_eq!(full.resolved, Some(true));
        assert_eq!(full.cost_usd, Some(0.12));
        assert_eq!(full.tokens_out, Some(7));

        let garbage = parse_verdict("not json at all");
        assert_eq!(garbage.resolved, None);
        let empty = parse_verdict("");
        assert_eq!(empty.tokens_in, None);
    }

    // ── Aggregation math ──────────────────────────────────────────────

    fn rep(idx: usize, disp: BenchDisposition, cost: Option<f64>) -> BenchRepRecord {
        BenchRepRecord {
            rep_index: idx,
            disposition: disp,
            exit_code: None,
            duration_ms: 1,
            failure_class: None,
            metrics: cost.map(|c| TaskMetrics {
                cost_usd: Some(c),
                ..Default::default()
            }),
            external_metrics: None,
            note: String::new(),
        }
    }

    #[test]
    fn aggregate_counts_dispositions_and_flags_variance_flips() {
        let agg = aggregate_case(
            "reg_07",
            "criteria",
            vec![
                rep(1, BenchDisposition::Resolved, Some(0.10)),
                rep(2, BenchDisposition::Failed, Some(0.30)),
                rep(3, BenchDisposition::Resolved, Some(0.10)),
            ],
        );
        assert_eq!(agg.resolved_reps, 2);
        assert_eq!(agg.failed_reps, 1);
        assert_eq!(agg.skipped_reps, 0);
        assert_eq!(agg.cost_usd_sum, Some(0.5));
        assert!(
            agg.variance_notes
                .iter()
                .any(|n| n.contains("resolved->failed")),
            "flip must be attributed mechanically: {:?}",
            agg.variance_notes
        );
    }

    #[test]
    fn aggregate_voids_cost_sum_on_any_missing_observation() {
        let agg = aggregate_case(
            "tb::partial",
            "criteria",
            vec![
                rep(1, BenchDisposition::Resolved, Some(0.20)),
                rep(2, BenchDisposition::Resolved, None),
            ],
        );
        assert_eq!(
            agg.cost_usd_sum, None,
            "unknown observation poisons the sum"
        );
    }

    #[test]
    fn token_spread_note_appears_only_above_band() {
        let tight = attribute_variance(&["resolved", "resolved"], &[(100, 100), (104, 104)]);
        assert!(
            !tight.iter().any(|n| n.contains("token_spread")),
            "{tight:?}"
        );
        let wide = attribute_variance(&["resolved", "failed"], &[(1_000, 500), (300, 200)]);
        assert!(wide.iter().any(|n| n.contains("token_spread")), "{wide:?}");
        let quiet = attribute_variance(&["resolved", "resolved"], &[(10, 10), (12, 12)]);
        assert!(quiet.iter().any(|n| n.contains("none detected")));
    }

    // ── End-to-end dry-run (regression, real pipeline) ─────────────────

    const MINI_REGRESSION_TOML: &str = r#"
id = "reg_x1"
tier = "edit"
description = "mini seeded defect (pipeline rehearsal stand-in)"
prompt = "Make the helper say READY."

[[setup.files]]
path = "src/helper.txt"
content = """state = PENDING"""

[[verify.rules]]
rule = "file_content"
path = "src/helper.txt"
contains = "state = READY"

[dry_run]
final_text = "updated"

[[dry_run.steps]]
tool = "Edit"
input = { file_path = "src/helper.txt", old_string = "state = PENDING", new_string = "state = READY" }
"#;

    #[test]
    fn regression_benchmark_runs_full_pipeline_dry_and_gates_citability() {
        let bench_dir = tempfile::TempDir::new().expect("bench root");
        let reg_dir = bench_dir.path().join("regression");
        std::fs::create_dir_all(&reg_dir).expect("mkdir regression");
        std::fs::write(reg_dir.join("reg_x1.toml"), MINI_REGRESSION_TOML).expect("seed task");
        // Exactly-ten guard must refuse stray counts loudly.
        let strict = BenchmarkOptions {
            suite: BenchSuite::Regression,
            n_runs: 2,
            dry_run: true,
            ..BenchmarkOptions::default()
        };
        assert!(
            run_benchmark(&strict, bench_dir.path()).is_err(),
            "regression pool pins exactly ten tasks; one-off dirs must be refused"
        );

        // Pad with 9 more copies so the count guard clears, then verify n=2 run.
        for i in 2..=10 {
            let body = MINI_REGRESSION_TOML.replace("reg_x1", &format!("reg_x{i}"));
            std::fs::write(reg_dir.join(format!("reg_x{i}.toml")), body).expect("pad");
        }

        let options = BenchmarkOptions {
            suite: BenchSuite::Regression,
            n_runs: 2,
            dry_run: true,
            out_root_override: Some(bench_dir.path().join("out")),
            ..BenchmarkOptions::default()
        };
        let (report, run_dir) = run_benchmark(&options, bench_dir.path()).expect("dry run");
        assert_eq!(report.suite, "regression");
        assert_eq!(report.n_runs, 2);
        assert_eq!(report.cases_total, 10);
        assert!(report.dry_run);
        for record in &report.records {
            assert_eq!(
                record.resolved_reps, 2,
                "self-consistent script must resolve"
            );
            assert!(matches!(
                record.reps[0].disposition,
                BenchDisposition::Resolved
            ));
        }
        assert_eq!(report.resolved_events, 20);
        assert_eq!(
            report.resolved_rate_interval,
            Some([1.0, 1.0]),
            "uniformly executed reps yield the interval"
        );
        assert!(report.metrics_source.starts_with("derived_stream"));
        assert!(
            !report.citation.citable,
            "mock-pattern dry artifacts must refuse citability"
        );
        assert!(
            report
                .citation
                .blockers
                .iter()
                .any(|b| b.contains("dry-run rehearsal")),
            "{:?}",
            report.citation.blockers
        );

        // Artifacts on disk.
        assert!(run_dir.join("bench-report.json").exists());
        assert!(run_dir.join("bench-report.md").exists());
        let md = std::fs::read_to_string(run_dir.join("bench-report.md")).expect("md");
        assert!(md.contains("DRY-RUN"), "{md}");
        assert!(md.contains("NOT CITABLE"));

        // Digest stability across repeated (volatile-nudged) executions.
        let (second, _) = run_benchmark(&options, bench_dir.path()).expect("second");
        assert_eq!(second.stable_digest(), report.stable_digest());
        assert!(
            compare_bench_reports(&report, &second).starts_with("STABLE"),
            "identical structure must compare STABLE"
        );
    }

    #[test]
    fn remote_benchmark_dry_enumerates_pins_without_execution() {
        let bench_dir = tempfile::TempDir::new().expect("root");
        std::fs::create_dir_all(bench_dir.path().join("regression")).expect("unused reg dir");
        std::fs::write(
            bench_dir.path().join("terminalbench_tasks.txt"),
            "hello-world\nchess-best-move\n",
        )
        .expect("pins");

        let options = BenchmarkOptions {
            suite: BenchSuite::TerminalBench,
            n_runs: 3,
            dry_run: true,
            out_root_override: Some(bench_dir.path().join("out")),
            ..BenchmarkOptions::default()
        };
        let (report, _) = run_benchmark(&options, bench_dir.path()).expect("enumeration run");
        assert_eq!(report.cases_total, 2);
        assert_eq!(
            report.pin_manifest_fingerprint.as_deref(),
            Some(
                load_pin_manifest(BenchSuite::TerminalBench, bench_dir.path())
                    .expect("reload")
                    .fingerprint
                    .as_str()
            ),
            "report must carry the workload fingerprint"
        );
        assert_eq!(report.pin_validation, "pending_corpus_contact");
        for record in &report.records {
            assert_eq!(record.skipped_reps, 3);
            assert!(
                record
                    .reps
                    .iter()
                    .all(|r| r.disposition == BenchDisposition::NotExecutedDryRun)
            );
        }
        assert_eq!(report.resolved_rate_interval, None, "n stays a placeholder");
        assert_eq!(report.cost_per_resolved_usd, None);
    }

    // ── Version diff mechanics ────────────────────────────────────────

    #[test]
    fn compare_surfaces_meta_drift_between_versions() {
        let bench_dir = tempfile::TempDir::new().expect("root");
        let reg_dir = bench_dir.path().join("regression");
        std::fs::create_dir_all(&reg_dir).expect("mkdir");
        std::fs::write(reg_dir.join("reg_x1.toml"), MINI_REGRESSION_TOML).expect("seed");
        for i in 2..=10 {
            let body = MINI_REGRESSION_TOML.replace("reg_x1", &format!("reg_x{i}"));
            std::fs::write(reg_dir.join(format!("reg_x{i}.toml")), body).expect("pad");
        }
        let options = BenchmarkOptions {
            suite: BenchSuite::Regression,
            n_runs: 1,
            dry_run: true,
            out_root_override: Some(bench_dir.path().join("out-a")),
            ..BenchmarkOptions::default()
        };
        let (base, _) = run_benchmark(&options, bench_dir.path()).expect("a");

        // Drift the workload: rename one case and compare becomes UNSTABLE.
        let drifted = reg_dir.join("reg_x3.toml");
        let body = std::fs::read_to_string(&drifted)
            .expect("read")
            .replace("helper.txt", "helper_v2.txt");
        std::fs::write(&drifted, body).expect("rewrite");

        let options_b = BenchmarkOptions {
            suite: BenchSuite::Regression,
            n_runs: 1,
            dry_run: true,
            out_root_override: Some(bench_dir.path().join("out-b")),
            ..BenchmarkOptions::default()
        };
        let (next, _) = run_benchmark(&options_b, bench_dir.path()).expect("b");
        let verdict = compare_bench_reports(&base, &next);
        assert!(verdict.starts_with("UNSTABLE"), "{verdict}");
        assert!(verdict.contains("[reg_x3] drift"), "{verdict}");
    }
}
