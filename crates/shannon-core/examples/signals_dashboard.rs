//! Signals dashboard command line (§4.15): `cargo run -p shannon-core --example signals_dashboard`.
//!
//! Renders the static version-trend board over eval run reports:
//! - scans `<runs-root>/<run-id>/report.json` (default:
//!   `$SHANNON_HOME/eval/runs`, `~/.shannon/eval/runs`)
//! - writes self-contained HTML (inline CSS, no scripts, no external
//!   references — offline openable) to
//!   `<eval-home>/eval/dashboard.html` (or `--out`).
//!
//! Exit codes: 0 dashboard written · 2 configuration/read error.

use std::path::PathBuf;

use shannon_core::testing::dashboard::{DASHBOARD_FILE_NAME, generate};
use std::process::ExitCode;

const USAGE: &str = "\
Usage:
  signals_dashboard [--runs <DIR>] [--out <FILE>]

  --runs <DIR>    Eval runs root holding <run-id>/report.json
                  (default: $SHANNON_HOME/eval/runs, else ~/.shannon/eval/runs)
  --out <FILE>    Output HTML path (default: <runs-root>/../dashboard.html)
";

fn main() -> ExitCode {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let mut runs_root: Option<PathBuf> = None;
    let mut out_path: Option<PathBuf> = None;

    let mut i = 0usize;
    while i < raw.len() {
        let value_at = |k: usize| -> Result<String, String> {
            raw.get(k)
                .cloned()
                .ok_or_else(|| "missing flag value".to_string())
        };
        match raw[i].as_str() {
            "--runs" => match value_at(i + 1) {
                Ok(v) => {
                    runs_root = Some(PathBuf::from(v));
                    i += 1;
                }
                Err(e) => return usage_error(&e),
            },
            "--out" => match value_at(i + 1) {
                Ok(v) => {
                    out_path = Some(PathBuf::from(v));
                    i += 1;
                }
                Err(e) => return usage_error(&e),
            },
            "--help" | "-h" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            other => return usage_error(&format!("unknown flag '{other}'")),
        }
        i += 1;
    }

    let runs_root = runs_root.unwrap_or_else(default_runs_root);
    let out_path = out_path.unwrap_or_else(|| default_out_path(runs_root.clone()));

    println!("[signals-dashboard] scanning {}", runs_root.display());
    match generate(&runs_root, &out_path) {
        Ok((count, bytes)) => {
            if count == 0 {
                println!(
                    "[signals-dashboard] no run reports found; wrote empty placeholder to {}",
                    out_path.display()
                );
            } else {
                println!(
                    "[signals-dashboard] rendered {count} run(s), {bytes} bytes → {}",
                    out_path.display()
                );
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(2)
        }
    }
}

fn usage_error(message: &str) -> ExitCode {
    eprintln!("{message}\n\n{USAGE}");
    ExitCode::from(2)
}

/// `$SHANNON_HOME/eval/runs` falling back to `~/.shannon/eval/runs`
/// (mirrors `resolve_eval_home`).
fn default_runs_root() -> PathBuf {
    resolve_eval_home().join("eval").join("runs")
}

fn default_out_path(runs_root: PathBuf) -> PathBuf {
    runs_root
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or(runs_root)
        .join(DASHBOARD_FILE_NAME)
}

fn resolve_eval_home() -> PathBuf {
    if let Ok(home) = std::env::var("SHANNON_HOME") {
        return PathBuf::from(home);
    }
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}
