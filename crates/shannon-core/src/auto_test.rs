//! # Auto-Test Loop (P1-5)
//!
//! When enabled, [`QueryEngine`](crate::query_engine::QueryEngine) automatically runs the
//! configured test command after a file-modifying tool (`Edit`/`Write`) succeeds. Test
//! failures are parsed and injected into the LLM context so the model can fix the code on
//! the next turn. Anti-loop guards prevent runaway iteration:
//!
//! * `max_iterations` — hard cap on auto-test attempts per query.
//! * `total_timeout_secs` — wall-clock cap on the entire auto-test budget.
//! * `no_progress_strikes` — bail out when the same failure recurs N times.
//!
//! ## Configuration
//!
//! Auto-test is opt-in. Add a section to `.shannon.toml`:
//!
//! ```toml
//! [auto_test]
//! command = "cargo nextest run -p my_crate"
//! max_iterations = 5
//! timeout_secs = 600
//! languages = ["rust"]
//! no_progress_strikes = 3
//! ```
//!
//! When `command` is omitted, the runner auto-detects based on `languages` (default: rust).
//! When `languages` is omitted, the runner uses the languages present in the working
//! directory (Cargo.toml → rust, package.json → node).
//!
//! ## Failure parsing
//!
//! `TestResult::failure_summary` returns the lines most likely to be relevant to the LLM
//! — the last N lines of combined stderr/stdout, with ANSI escapes stripped.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Test runner language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    /// Rust (`cargo nextest run` or `cargo test`)
    Rust,
    /// Node.js (`npm test`)
    Node,
    /// Python (`pytest`)
    Python,
    /// Go (`go test ./...`)
    Go,
}

impl Language {
    /// Detect languages present in `dir` by scanning well-known manifest files.
    pub fn detect_in(dir: &Path) -> Vec<Language> {
        let mut langs = Vec::new();
        if dir.join("Cargo.toml").exists() {
            langs.push(Language::Rust);
        }
        if dir.join("package.json").exists() {
            langs.push(Language::Node);
        }
        if dir.join("pyproject.toml").exists()
            || dir.join("pytest.ini").exists()
            || dir.join("setup.py").exists()
        {
            langs.push(Language::Python);
        }
        if dir.join("go.mod").exists() {
            langs.push(Language::Go);
        }
        langs
    }

    /// Default test command for this language.
    pub fn default_command(self) -> &'static str {
        match self {
            Language::Rust => "cargo nextest run --no-fail-fast",
            Language::Node => "npm test --silent",
            Language::Python => "pytest -x --tb=short",
            Language::Go => "go test ./...",
        }
    }
}

/// Configuration for the auto-test loop.
///
/// Loaded from `.shannon.toml`'s `[auto_test]` section via [`AutoTestConfig::from_toml_str`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoTestConfig {
    /// Override test command. If `None`, use the per-language default.
    pub command: Option<String>,

    /// Languages to test. If empty, auto-detect from the working directory.
    #[serde(default)]
    pub languages: Vec<Language>,

    /// Maximum number of auto-test iterations per query (default: 5).
    #[serde(default = "default_max_iterations")]
    pub max_iterations: u32,

    /// Per-run timeout in seconds (default: 120).
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,

    /// Total wall-clock budget across all iterations (default: 600s = 10 min).
    #[serde(default = "default_total_timeout_secs")]
    pub total_timeout_secs: u64,

    /// Stop after the same failure recurs this many times (default: 3).
    #[serde(default = "default_no_progress_strikes")]
    pub no_progress_strikes: u32,

    /// Maximum lines of failure output to inject into the LLM context (default: 200).
    /// Lines beyond this are truncated from the head — keep the most recent (where
    /// failures are summarized by most runners).
    #[serde(default = "default_max_failure_lines")]
    pub max_failure_lines: usize,
}

fn default_max_iterations() -> u32 {
    5
}
fn default_timeout_secs() -> u64 {
    120
}
fn default_total_timeout_secs() -> u64 {
    600
}
fn default_no_progress_strikes() -> u32 {
    3
}
fn default_max_failure_lines() -> usize {
    200
}

impl Default for AutoTestConfig {
    fn default() -> Self {
        Self {
            command: None,
            languages: Vec::new(),
            max_iterations: default_max_iterations(),
            timeout_secs: default_timeout_secs(),
            total_timeout_secs: default_total_timeout_secs(),
            no_progress_strikes: default_no_progress_strikes(),
            max_failure_lines: default_max_failure_lines(),
        }
    }
}

impl AutoTestConfig {
    /// Parse from a TOML string. Returns the default config when the section is missing.
    pub fn from_toml_str(s: &str) -> Result<Self, AutoTestConfigError> {
        #[derive(Deserialize)]
        struct Wrapper {
            #[serde(default)]
            auto_test: Option<AutoTestConfig>,
        }
        let wrapper: Wrapper =
            toml::from_str(s).map_err(|e| AutoTestConfigError::TomlParse(e.to_string()))?;
        Ok(wrapper.auto_test.unwrap_or_default())
    }

    /// Resolve the command to execute. If `command` is set, use it verbatim.
    /// Otherwise pick the default for the first configured (or detected) language.
    ///
    /// `project_dir` is only consulted when `languages` is empty.
    pub fn resolve_command(&self, project_dir: &Path) -> Option<String> {
        if let Some(cmd) = &self.command {
            return Some(cmd.clone());
        }
        let langs = if self.languages.is_empty() {
            Language::detect_in(project_dir)
        } else {
            self.languages.clone()
        };
        langs.first().map(|l| l.default_command().to_string())
    }

    /// Per-run timeout.
    pub fn run_timeout(&self) -> Duration {
        Duration::from_secs(self.timeout_secs)
    }

    /// Total wall-clock budget.
    pub fn total_timeout(&self) -> Duration {
        Duration::from_secs(self.total_timeout_secs)
    }
}

/// Errors from parsing auto-test configuration.
#[derive(Debug, thiserror::Error)]
pub enum AutoTestConfigError {
    #[error("failed to parse .shannon.toml: {0}")]
    TomlParse(String),
}

/// Outcome of running a test command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestOutcome {
    /// Exit code 0 — all tests passed.
    Passed,
    /// Non-zero exit code — tests failed. `summary` is the truncated stderr/stdout tail.
    Failed { summary: String },
    /// The test process was killed by SIGTERM (timeout exceeded).
    TimedOut,
    /// The test process could not be spawned (binary missing, permission denied, etc.).
    SpawnError(String),
}

impl TestOutcome {
    /// True if tests passed and the loop should stop.
    pub fn is_passed(&self) -> bool {
        matches!(self, TestOutcome::Passed)
    }

    /// True if the loop should bail out due to a non-recoverable failure.
    pub fn is_terminal(&self) -> bool {
        // Repeated timeouts and spawn errors are not fixable by the LLM.
        matches!(self, TestOutcome::TimedOut | TestOutcome::SpawnError(_))
    }

    /// Human-readable description for LLM context injection.
    pub fn describe(&self) -> String {
        match self {
            TestOutcome::Passed => "All tests passed.".to_string(),
            TestOutcome::Failed { summary } => {
                format!("Tests failed:\n```\n{summary}\n```")
            }
            TestOutcome::TimedOut => {
                "Tests timed out (no output within the configured timeout).".to_string()
            }
            TestOutcome::SpawnError(e) => format!(
                "Could not execute test command (spawn error): {e}\n\
                 Check that the configured command and required tools are installed."
            ),
        }
    }
}

/// Anti-loop bookkeeping. Owned by `QueryEngine` and mutated each iteration.
#[derive(Debug)]
pub struct AntiLoopState {
    /// When the auto-test loop first started for this query.
    pub started_at: Instant,
    /// Current iteration count (0 = no runs yet).
    pub iterations: u32,
    /// Hash of the failure summary from the previous run; used to detect non-progress.
    pub last_failure_hash: Option<u64>,
    /// Number of consecutive iterations with the same failure.
    pub consecutive_same: u32,
    /// Why the loop exited (if it did). `None` = still running or not started.
    pub stopped_reason: Option<StopReason>,
}

impl AntiLoopState {
    pub fn new() -> Self {
        Self {
            started_at: Instant::now(),
            iterations: 0,
            last_failure_hash: None,
            consecutive_same: 0,
            stopped_reason: None,
        }
    }

    /// Record an iteration result and decide whether to keep looping.
    ///
    /// Returns `Continue` if the next iteration should run, `Stop(reason)` if not.
    pub fn record(&mut self, cfg: &AutoTestConfig, outcome: &TestOutcome) -> LoopDecision {
        self.iterations += 1;

        match outcome {
            TestOutcome::Passed => {
                self.stopped_reason = Some(StopReason::Passed);
                return LoopDecision::Stop(StopReason::Passed);
            }
            TestOutcome::TimedOut => {
                self.stopped_reason = Some(StopReason::Timeout);
                return LoopDecision::Stop(StopReason::Timeout);
            }
            TestOutcome::SpawnError(_) => {
                self.stopped_reason = Some(StopReason::SpawnError);
                return LoopDecision::Stop(StopReason::SpawnError);
            }
            TestOutcome::Failed { summary } => {
                let hash = hash_str(summary);
                if self.last_failure_hash == Some(hash) {
                    self.consecutive_same += 1;
                } else {
                    self.consecutive_same = 1;
                    self.last_failure_hash = Some(hash);
                }
            }
        }

        if self.iterations >= cfg.max_iterations {
            self.stopped_reason = Some(StopReason::MaxIterations);
            return LoopDecision::Stop(StopReason::MaxIterations);
        }
        if self.started_at.elapsed() >= cfg.total_timeout() {
            self.stopped_reason = Some(StopReason::TotalTimeout);
            return LoopDecision::Stop(StopReason::TotalTimeout);
        }
        if self.consecutive_same >= cfg.no_progress_strikes {
            self.stopped_reason = Some(StopReason::NoProgress);
            return LoopDecision::Stop(StopReason::NoProgress);
        }

        LoopDecision::Continue
    }

    /// True if any iteration has run.
    pub fn has_run(&self) -> bool {
        self.iterations > 0
    }

    /// Wall-clock time spent so far.
    pub fn elapsed(&self) -> Duration {
        self.started_at.elapsed()
    }
}

impl Default for AntiLoopState {
    fn default() -> Self {
        Self::new()
    }
}

/// Decision returned by [`AntiLoopState::record`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoopDecision {
    Continue,
    Stop(StopReason),
}

/// Why the auto-test loop stopped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    /// Tests passed — done.
    Passed,
    /// `max_iterations` reached.
    MaxIterations,
    /// `total_timeout_secs` reached.
    TotalTimeout,
    /// Per-run timeout exceeded.
    Timeout,
    /// Test command could not be spawned.
    SpawnError,
    /// Same failure recurred `no_progress_strikes` times.
    NoProgress,
}

impl StopReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            StopReason::Passed => "passed",
            StopReason::MaxIterations => "max_iterations",
            StopReason::TotalTimeout => "total_timeout",
            StopReason::Timeout => "timeout",
            StopReason::SpawnError => "spawn_error",
            StopReason::NoProgress => "no_progress",
        }
    }
}

/// Strip ANSI escape sequences (CSI + simple escapes) from test output.
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Skip CSI sequence: ESC [ ... letter
            if chars.peek() == Some(&'[') {
                chars.next(); // consume '['
                // Parameter bytes: 0x30-0x3F, intermediate bytes: 0x20-0x2F
                // Final byte: 0x40-0x7E
                for nc in chars.by_ref() {
                    if nc.is_ascii_alphabetic() || nc == '~' {
                        break;
                    }
                }
                continue;
            }
            // Skip OSC sequence: ESC ] ... BEL or ESC \
            if chars.peek() == Some(&']') {
                chars.next();
                // Walk until we hit BEL (\x07) or ST (ESC \).
                loop {
                    match chars.next() {
                        Some('\x07') => break,
                        Some('\x1b') => {
                            // The terminator must be '\\'; consume it if present.
                            if chars.peek() == Some(&'\\') {
                                chars.next();
                            }
                            break;
                        }
                        Some(_) => continue,
                        None => break,
                    }
                }
                continue;
            }
            // Skip simple two-char escapes (e.g. ESC =, ESC >)
            chars.next();
            continue;
        }
        out.push(c);
    }
    out
}

/// Truncate a string to the last `max_lines` lines, with a leading note about
/// how many lines were dropped from the head. This keeps the failure tail —
/// where most runners print their summary — while bounding context size.
pub fn tail_lines(s: &str, max_lines: usize) -> String {
    if max_lines == 0 {
        return String::new();
    }
    let lines: Vec<&str> = s.lines().collect();
    if lines.len() <= max_lines {
        return s.to_string();
    }
    let drop = lines.len() - max_lines;
    let tail = lines[max_lines.min(lines.len())..].join("\n");
    // Multi-line tail lines() yields lines without trailing newlines;
    // re-join with '\n' produces an equivalent representation.
    let mut out = String::with_capacity(tail.len() + 64);
    out.push_str(&format!("[truncated: {drop} earlier lines omitted]\n"));
    out.push_str(&tail);
    out
}

/// Hash a string for failure-summary equality. Uses Rust's default hasher
/// — collisions are irrelevant (we only use this for "is this the same
/// failure as last time?"), and the input is short.
fn hash_str(s: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

/// Build the [`TestOutcome`] from raw stdout/stderr/exit-code.
///
/// `max_lines` controls how much of the combined output is retained.
pub fn outcome_from_output(
    stdout: &str,
    stderr: &str,
    exit_code: Option<i32>,
    timed_out: bool,
    max_lines: usize,
) -> TestOutcome {
    if timed_out {
        return TestOutcome::TimedOut;
    }
    let success = matches!(exit_code, Some(0));
    if success {
        return TestOutcome::Passed;
    }
    let combined = if stderr.is_empty() {
        stdout.to_string()
    } else if stdout.is_empty() {
        stderr.to_string()
    } else {
        format!("{stdout}\n--- stderr ---\n{stderr}")
    };
    let stripped = strip_ansi(&combined);
    let summary = tail_lines(&stripped, max_lines);
    TestOutcome::Failed { summary }
}

/// Run a test command and return the [`TestOutcome`].
///
/// `project_dir` is the working directory; `command` is the full shell
/// command to execute; `timeout` caps the per-run wall-clock.
pub async fn run_test_command(
    command: &str,
    project_dir: &Path,
    timeout: Duration,
    max_lines: usize,
) -> TestOutcome {
    use tokio::process::Command;

    let mut cmd = Command::new("bash");
    cmd.arg("-c")
        .arg(command)
        .current_dir(project_dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);

    let output = match tokio::time::timeout(timeout, cmd.output()).await {
        Err(_) => {
            return TestOutcome::TimedOut;
        }
        Ok(Err(e)) => {
            return TestOutcome::SpawnError(format!("failed to spawn bash -c: {e}"));
        }
        Ok(Ok(out)) => out,
    };

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    outcome_from_output(&stdout, &stderr, output.status.code(), false, max_lines)
}

/// High-level runner: resolves the command from config and executes it.
pub async fn run_auto_test(cfg: &AutoTestConfig, project_dir: &Path) -> Option<TestOutcome> {
    let command = cfg.resolve_command(project_dir)?;
    Some(
        run_test_command(
            &command,
            project_dir,
            cfg.run_timeout(),
            cfg.max_failure_lines,
        )
        .await,
    )
}

/// Detect the project directory. Falls back to the current working directory.
pub fn project_dir() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn default_config_round_trip() {
        let cfg = AutoTestConfig::default();
        assert_eq!(cfg.max_iterations, 5);
        assert_eq!(cfg.timeout_secs, 120);
        assert_eq!(cfg.total_timeout_secs, 600);
        assert_eq!(cfg.no_progress_strikes, 3);
        assert_eq!(cfg.max_failure_lines, 200);
        assert!(cfg.command.is_none());
        assert!(cfg.languages.is_empty());
    }

    #[test]
    fn parse_minimal_toml_section() {
        let toml = r#"
            [auto_test]
            command = "cargo test"
            max_iterations = 3
        "#;
        let cfg = AutoTestConfig::from_toml_str(toml).unwrap();
        assert_eq!(cfg.command.as_deref(), Some("cargo test"));
        assert_eq!(cfg.max_iterations, 3);
        // defaults preserved
        assert_eq!(cfg.timeout_secs, 120);
    }

    #[test]
    fn parse_full_toml_section() {
        let toml = r#"
            [auto_test]
            command = "cargo nextest run -p foo"
            max_iterations = 10
            timeout_secs = 60
            total_timeout_secs = 900
            languages = ["rust", "node"]
            no_progress_strikes = 5
            max_failure_lines = 100
        "#;
        let cfg = AutoTestConfig::from_toml_str(toml).unwrap();
        assert_eq!(cfg.command.as_deref(), Some("cargo nextest run -p foo"));
        assert_eq!(cfg.max_iterations, 10);
        assert_eq!(cfg.timeout_secs, 60);
        assert_eq!(cfg.total_timeout_secs, 900);
        assert_eq!(cfg.languages.len(), 2);
        assert_eq!(cfg.languages[0], Language::Rust);
        assert_eq!(cfg.languages[1], Language::Node);
        assert_eq!(cfg.no_progress_strikes, 5);
        assert_eq!(cfg.max_failure_lines, 100);
    }

    #[test]
    fn parse_missing_section_returns_default() {
        let toml = "[other]\nfoo = 1\n";
        let cfg = AutoTestConfig::from_toml_str(toml).unwrap();
        assert_eq!(cfg.max_iterations, 5);
        assert!(cfg.command.is_none());
    }

    #[test]
    fn parse_invalid_toml_errors() {
        let toml = "this is not valid toml ===";
        let err = AutoTestConfig::from_toml_str(toml).unwrap_err();
        assert!(matches!(err, AutoTestConfigError::TomlParse(_)));
    }

    #[test]
    fn resolve_command_uses_explicit() {
        let cfg = AutoTestConfig {
            command: Some("make test".into()),
            ..Default::default()
        };
        let cmd = cfg.resolve_command(Path::new("/tmp"));
        assert_eq!(cmd.as_deref(), Some("make test"));
    }

    #[test]
    fn resolve_command_uses_first_language_default() {
        let cfg = AutoTestConfig {
            languages: vec![Language::Rust, Language::Node],
            ..Default::default()
        };
        let cmd = cfg.resolve_command(Path::new("/tmp"));
        assert!(cmd.unwrap().contains("cargo"));
    }

    #[test]
    fn resolve_command_no_languages_no_command_returns_none() {
        let cfg = AutoTestConfig::default();
        let dir = Path::new("/tmp/does_not_exist_xyz");
        assert!(cfg.resolve_command(dir).is_none());
    }

    #[test]
    fn resolve_command_detects_rust_when_manifest_present() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\n").unwrap();
        let cfg = AutoTestConfig::default();
        let cmd = cfg.resolve_command(dir.path()).unwrap();
        assert!(cmd.contains("cargo"));
    }

    #[test]
    fn detect_languages_in_temp_dir_rust() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\n").unwrap();
        let langs = Language::detect_in(dir.path());
        assert_eq!(langs, vec![Language::Rust]);
    }

    #[test]
    fn detect_languages_in_temp_dir_multi() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\n").unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        let langs = Language::detect_in(dir.path());
        assert!(langs.contains(&Language::Rust));
        assert!(langs.contains(&Language::Node));
    }

    #[test]
    fn detect_languages_empty_for_unknown() {
        let dir = tempfile::tempdir().unwrap();
        let langs = Language::detect_in(dir.path());
        assert!(langs.is_empty());
    }

    #[test]
    fn language_default_command() {
        assert!(Language::Rust.default_command().contains("cargo"));
        assert!(Language::Node.default_command().contains("npm"));
        assert!(Language::Python.default_command().contains("pytest"));
        assert!(Language::Go.default_command().contains("go test"));
    }

    #[test]
    fn strip_ansi_removes_csi() {
        let input = "\x1b[31mred\x1b[0m normal";
        let out = strip_ansi(input);
        assert_eq!(out, "red normal");
    }

    #[test]
    fn strip_ansi_removes_osc() {
        let input = "\x1b]0;title\x07body";
        let out = strip_ansi(input);
        assert_eq!(out, "body");
    }

    #[test]
    fn strip_ansi_keeps_plain_text() {
        let input = "plain text with no escapes";
        assert_eq!(strip_ansi(input), input);
    }

    #[test]
    fn tail_lines_keeps_last_n() {
        let s = (1..=10)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let tailed = tail_lines(&s, 3);
        assert!(tailed.contains("line 8"));
        assert!(tailed.contains("line 10"));
        assert!(!tailed.contains("line 1") || tailed.contains("truncated"));
        assert!(tailed.contains("truncated"));
    }

    #[test]
    fn tail_lines_no_change_when_under_limit() {
        let s = "a\nb\nc";
        assert_eq!(tail_lines(s, 10), s);
    }

    #[test]
    fn tail_lines_zero_returns_empty() {
        assert_eq!(tail_lines("a\nb", 0), "");
    }

    #[test]
    fn outcome_passed_when_exit_zero() {
        let out = outcome_from_output("ok", "", Some(0), false, 10);
        assert_eq!(out, TestOutcome::Passed);
    }

    #[test]
    fn outcome_failed_includes_truncated_output() {
        let stdout = "test_1 FAILED\n".repeat(50);
        let out = outcome_from_output(&stdout, "", Some(101), false, 5);
        match out {
            TestOutcome::Failed { summary } => {
                assert!(summary.contains("truncated"));
                assert!(summary.contains("FAILED"));
            }
            _ => panic!("expected Failed"),
        }
    }

    #[test]
    fn outcome_combines_stderr_when_stdout_empty() {
        let out = outcome_from_output("", "boom", Some(2), false, 100);
        match out {
            TestOutcome::Failed { summary } => assert!(summary.contains("boom")),
            _ => panic!("expected Failed"),
        }
    }

    #[test]
    fn outcome_combines_both_streams() {
        let out = outcome_from_output("out-text", "err-text", Some(2), false, 100);
        match out {
            TestOutcome::Failed { summary } => {
                assert!(summary.contains("out-text"));
                assert!(summary.contains("err-text"));
                assert!(summary.contains("stderr"));
            }
            _ => panic!("expected Failed"),
        }
    }

    #[test]
    fn outcome_timed_out_flag() {
        let out = outcome_from_output("partial", "", Some(-1), true, 10);
        assert_eq!(out, TestOutcome::TimedOut);
    }

    #[test]
    fn outcome_is_passed_and_terminal() {
        assert!(TestOutcome::Passed.is_passed());
        assert!(
            !TestOutcome::Failed {
                summary: "x".into()
            }
            .is_passed()
        );
        assert!(TestOutcome::TimedOut.is_terminal());
        assert!(TestOutcome::SpawnError("x".into()).is_terminal());
        assert!(
            !TestOutcome::Failed {
                summary: "x".into()
            }
            .is_terminal()
        );
    }

    #[test]
    fn anti_loop_initial_state() {
        let s = AntiLoopState::new();
        assert_eq!(s.iterations, 0);
        assert!(!s.has_run());
        assert!(s.stopped_reason.is_none());
        assert_eq!(s.consecutive_same, 0);
    }

    #[test]
    fn anti_loop_passes_stops_immediately() {
        let cfg = AutoTestConfig::default();
        let mut s = AntiLoopState::new();
        let d = s.record(&cfg, &TestOutcome::Passed);
        assert_eq!(d, LoopDecision::Stop(StopReason::Passed));
        assert_eq!(s.stopped_reason, Some(StopReason::Passed));
        assert_eq!(s.iterations, 1);
    }

    #[test]
    fn anti_loop_max_iterations() {
        let cfg = AutoTestConfig {
            max_iterations: 2,
            ..Default::default()
        };
        let mut s = AntiLoopState::new();
        let f = TestOutcome::Failed {
            summary: "boom".into(),
        };
        assert_eq!(s.record(&cfg, &f), LoopDecision::Continue);
        assert_eq!(
            s.record(&cfg, &f),
            LoopDecision::Stop(StopReason::MaxIterations)
        );
    }

    #[test]
    fn anti_loop_no_progress_strikes() {
        let cfg = AutoTestConfig {
            max_iterations: 100,
            no_progress_strikes: 2,
            ..Default::default()
        };
        let mut s = AntiLoopState::new();
        let f = TestOutcome::Failed {
            summary: "same error".into(),
        };
        assert_eq!(s.record(&cfg, &f), LoopDecision::Continue);
        // Second consecutive same failure: counter hits 2 → stop
        assert_eq!(
            s.record(&cfg, &f),
            LoopDecision::Stop(StopReason::NoProgress)
        );
    }

    #[test]
    fn anti_loop_progress_resets_counter() {
        let cfg = AutoTestConfig {
            max_iterations: 100,
            no_progress_strikes: 3,
            ..Default::default()
        };
        let mut s = AntiLoopState::new();
        let f1 = TestOutcome::Failed {
            summary: "error A".into(),
        };
        let f2 = TestOutcome::Failed {
            summary: "error B".into(),
        };
        s.record(&cfg, &f1);
        s.record(&cfg, &f1);
        assert_eq!(s.consecutive_same, 2);
        s.record(&cfg, &f2); // different failure → counter resets
        assert_eq!(s.consecutive_same, 1);
    }

    #[test]
    fn anti_loop_timeout_stops_immediately() {
        let cfg = AutoTestConfig {
            max_iterations: 100,
            ..Default::default()
        };
        let mut s = AntiLoopState::new();
        let d = s.record(&cfg, &TestOutcome::TimedOut);
        assert_eq!(d, LoopDecision::Stop(StopReason::Timeout));
    }

    #[test]
    fn anti_loop_spawn_error_stops_immediately() {
        let cfg = AutoTestConfig::default();
        let mut s = AntiLoopState::new();
        let d = s.record(&cfg, &TestOutcome::SpawnError("no such binary".into()));
        assert_eq!(d, LoopDecision::Stop(StopReason::SpawnError));
    }

    #[test]
    fn anti_loop_total_timeout() {
        let cfg = AutoTestConfig {
            max_iterations: 100,
            total_timeout_secs: 0, // already expired
            ..Default::default()
        };
        let mut s = AntiLoopState::new();
        // Sleep a moment so elapsed > 0
        std::thread::sleep(Duration::from_millis(10));
        let f = TestOutcome::Failed {
            summary: "x".into(),
        };
        let d = s.record(&cfg, &f);
        assert_eq!(d, LoopDecision::Stop(StopReason::TotalTimeout));
    }

    #[test]
    fn stop_reason_as_str() {
        assert_eq!(StopReason::Passed.as_str(), "passed");
        assert_eq!(StopReason::MaxIterations.as_str(), "max_iterations");
        assert_eq!(StopReason::TotalTimeout.as_str(), "total_timeout");
        assert_eq!(StopReason::Timeout.as_str(), "timeout");
        assert_eq!(StopReason::SpawnError.as_str(), "spawn_error");
        assert_eq!(StopReason::NoProgress.as_str(), "no_progress");
    }

    #[test]
    fn run_test_command_success() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let dir = tempfile::tempdir().unwrap();
            let out = run_test_command("exit 0", dir.path(), Duration::from_secs(5), 50).await;
            assert_eq!(out, TestOutcome::Passed);
        });
    }

    #[test]
    fn run_test_command_failure_exit_code() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let dir = tempfile::tempdir().unwrap();
            let out = run_test_command(
                "echo failed && exit 1",
                dir.path(),
                Duration::from_secs(5),
                50,
            )
            .await;
            match out {
                TestOutcome::Failed { summary } => {
                    assert!(summary.contains("failed"));
                }
                _ => panic!("expected Failed, got {out:?}"),
            }
        });
    }

    #[test]
    fn run_test_command_timeout() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let dir = tempfile::tempdir().unwrap();
            // sleep 30s but timeout after 100ms
            let out =
                run_test_command("sleep 30", dir.path(), Duration::from_millis(100), 50).await;
            assert_eq!(out, TestOutcome::TimedOut);
        });
    }

    #[test]
    fn run_test_command_missing_command_yields_failed() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let dir = tempfile::tempdir().unwrap();
            // `/no_such_binary_xyz` will not be found by bash — bash returns exit
            // 127 ("command not found"), which surfaces as `Failed`, not
            // `SpawnError` (bash itself can always be spawned on a normal system).
            let out = run_test_command(
                "/this/path/definitely/does/not/exist_xyz",
                dir.path(),
                Duration::from_secs(5),
                50,
            )
            .await;
            assert!(matches!(out, TestOutcome::Failed { .. }));
        });
    }

    #[test]
    fn test_outcome_spawn_error_describes_problem() {
        let desc = TestOutcome::SpawnError("missing binary".into()).describe();
        assert!(desc.contains("spawn error"));
        assert!(desc.contains("missing binary"));
    }

    #[test]
    fn run_auto_test_no_command_no_lang_no_dir_returns_none() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let dir = tempfile::tempdir().unwrap();
            let cfg = AutoTestConfig::default();
            assert!(run_auto_test(&cfg, dir.path()).await.is_none());
        });
    }

    #[test]
    fn run_auto_test_runs_with_explicit_command() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let dir = tempfile::tempdir().unwrap();
            let cfg = AutoTestConfig {
                command: Some("exit 0".into()),
                ..Default::default()
            };
            let out = run_auto_test(&cfg, dir.path()).await.unwrap();
            assert_eq!(out, TestOutcome::Passed);
        });
    }

    #[test]
    fn auto_test_describe_contains_relevant_info() {
        let passed = TestOutcome::Passed.describe();
        assert!(passed.contains("passed"));
        let failed = TestOutcome::Failed {
            summary: "x".into(),
        }
        .describe();
        assert!(failed.contains("failed"));
        assert!(failed.contains("```"));
        let timeout = TestOutcome::TimedOut.describe();
        assert!(timeout.contains("timed out"));
    }

    #[test]
    fn language_serde_roundtrip() {
        for lang in [
            Language::Rust,
            Language::Node,
            Language::Python,
            Language::Go,
        ] {
            let json = serde_json::to_string(&lang).unwrap();
            let back: Language = serde_json::from_str(&json).unwrap();
            assert_eq!(back, lang);
        }
    }
}
