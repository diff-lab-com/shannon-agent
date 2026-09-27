//! Export-diagnostics command (batch-3 follow-up): bundle local logs, crash
//! reports, and a fresh `shannon doctor --json --deep` report into a single
//! zip the user can attach to a bug report.
//!
//! Privacy contract: **nothing is collected beyond the log directory and
//! the doctor report.** Sessions (conversation content), provider config,
//! and credentials are never included; log lines and the doctor output are
//! already passed through the session-log redaction policy. The bundle is
//! written wherever the user picked in the save dialog and only leaves the
//! machine by the user's own hand.

use serde::Serialize;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Include log files up to this many bytes, then skip the rest (reported as
/// truncated in the manifest). WARN-level daily logs are small; the cap only
/// bites on pathological runaway logging.
const MAX_LOG_BYTES: u64 = 20 * 1024 * 1024;

/// Budget for the `shannon doctor --json --deep` subprocess. The deep sweep
/// is capped at 2000 session files by doctor itself; 4 minutes covers a
/// cold start on a full history without hanging the export forever.
const DOCTOR_TIMEOUT: Duration = Duration::from_secs(240);

/// Summary returned to the UI after a successful export.
#[derive(Debug, Clone, Serialize)]
pub struct ExportSummary {
    /// Absolute path of the written zip.
    pub path: String,
    /// Number of log/crash files included.
    pub log_files: usize,
    /// Uncompressed bytes of the included log files.
    pub log_bytes: u64,
    /// Whether a bundled `shannon doctor --json --deep` run succeeded and
    /// was embedded (false → manifest explains why not).
    pub doctor_ok: bool,
    /// True when the log-size cap skipped some files.
    pub truncated: bool,
}

/// Collect `<home>/logs` (daily-rotated desktop log + crash reports) plus a
/// doctor report and manifest into a deflated zip at `dest`.
///
/// Pure fn over explicit paths — unit-testable without Tauri or `$HOME`.
pub fn build_bundle(
    logs_dir: &Path,
    doctor_json: Option<&str>,
    doctor_version: Option<&str>,
    dest: &Path,
) -> Result<ExportSummary, String> {
    // ── Collect log files (name-sorted, size-capped) ──
    let mut log_files: Vec<(PathBuf, String)> = Vec::new(); // (path, archive name)
    let mut log_bytes = 0u64;
    let mut truncated = false;
    if let Ok(entries) = std::fs::read_dir(logs_dir) {
        let mut files: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_file())
            .collect();
        files.sort();
        for path in files {
            let Some(name) = path
                .file_name()
                .and_then(|n| n.to_str().map(str::to_string))
            else {
                continue;
            };
            let len = path.metadata().map(|m| m.len()).unwrap_or(0);
            if log_bytes + len > MAX_LOG_BYTES {
                truncated = true;
                continue;
            }
            log_bytes += len;
            log_files.push((path, format!("logs/{name}")));
        }
    }

    // ── Manifest (plain text, human-first) ──
    let ts = chrono::Utc::now().to_rfc3339();
    let manifest = format!(
        "Shannon desktop diagnostics bundle\n\
         generated: {ts}\n\
         desktop version: {}\n\
         platform: {} ({})\n\
         bundled shannon (doctor): {}\n\
         log files included: {} ({log_bytes} bytes)\n\
         truncated at {MAX_LOG_BYTES}-byte cap: {truncated}\n\
         \n\
         Privacy: this bundle contains application logs, crash reports and a\n\
         `shannon doctor --json --deep` report only. Session transcripts,\n\
         provider configuration and credentials are NOT included. Log lines\n\
         have passed the built-in redaction policy at write time — review\n\
         before attaching anywhere public.\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        doctor_version.unwrap_or(
            "not found — include `shannon doctor --json --deep` output manually if asked"
        ),
        log_files.len(),
    );

    // ── Write the zip ──
    let file = std::fs::File::create(dest)
        .map_err(|e| format!("cannot create {}: {e}", dest.display()))?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    // `start_file` yields ZipError while `write_all` yields io::Error — the
    // steps can't share one `and_then` chain, so map each explicitly.
    let zw = |r: Result<(), zip::result::ZipError>| r.map_err(|e| format!("zip write failed: {e}"));
    zw(zip.add_directory("logs/", options))?;
    zw(zip.start_file("manifest.txt", options))?;
    zip.write_all(manifest.as_bytes())
        .map_err(|e| format!("zip write failed: {e}"))?;
    if let Some(doctor) = doctor_json {
        zw(zip.start_file("doctor.json", options))?;
        zip.write_all(doctor.as_bytes())
            .map_err(|e| format!("zip write failed: {e}"))?;
    }
    for (path, archive_name) in &log_files {
        let bytes =
            std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        zw(zip.start_file(archive_name.as_str(), options))?;
        zip.write_all(&bytes)
            .map_err(|e| format!("zip write failed: {e}"))?;
    }
    zw(zip.finish().map(|_| ()))?;

    Ok(ExportSummary {
        path: dest.display().to_string(),
        log_files: log_files.len(),
        log_bytes,
        doctor_ok: doctor_json.is_some(),
        truncated,
    })
}

/// Run the bundled (or on-PATH) `shannon doctor --json --deep` and return
/// its redacted JSON plus the CLI version reported inside it. Best-effort:
/// any failure returns `(None, None)` and the manifest tells the user how
/// to produce the report by hand.
async fn run_bundled_doctor() -> (Option<String>, Option<String>) {
    let Some(bin) = crate::commands_surface::resolve_cli() else {
        return (None, None);
    };
    let output = tokio::process::Command::new(&bin)
        .args(["doctor", "--json", "--deep"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .output();
    let Ok(Ok(out)) = tokio::time::timeout(DOCTOR_TIMEOUT, output).await else {
        tracing::warn!("diagnostics export: bundled doctor did not finish in time");
        return (None, None);
    };
    if !out.status.success() {
        tracing::warn!(status = %out.status, "diagnostics export: bundled doctor failed");
        return (None, None);
    }
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    // Engine URLs from the environment can embed credentials — the doctor
    // report goes through the same redaction policy as the log sink.
    let redacted = shannon_core::session_log::redaction::redact_string(&stdout);
    let version = serde_json::from_str::<serde_json::Value>(&redacted)
        .ok()
        .and_then(|v| v["version"].as_str().map(str::to_string));
    (Some(redacted), version)
}

/// Bundle diagnostics into `dest` (an absolute path the user picked in the
/// frontend save dialog). See the module docs for the privacy contract.
#[tauri::command]
pub async fn export_diagnostics(dest: String) -> Result<ExportSummary, String> {
    let logs_dir = shannon_core::data_meta::home().join("logs");
    let (doctor_json, doctor_version) = run_bundled_doctor().await;
    build_bundle(
        &logs_dir,
        doctor_json.as_deref(),
        doctor_version.as_deref(),
        Path::new(&dest),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, bytes: &[u8]) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, bytes).unwrap();
    }

    /// Read all (name, bytes) entries back out of a written zip.
    fn zip_entries(path: &Path) -> Vec<(String, Vec<u8>)> {
        let file = std::fs::File::open(path).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        let mut out = Vec::new();
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).unwrap();
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(&mut entry, &mut bytes).unwrap();
            out.push((entry.name().to_string(), bytes));
        }
        out
    }

    #[test]
    fn bundle_contains_manifest_doctor_and_logs() {
        let dir = tempfile::tempdir().unwrap();
        let logs = dir.path().join("logs");
        write(&logs.join("shannon-desktop.log.2026-09-27"), b"warn line\n");
        write(&logs.join("crash-1790000000-42.log"), b"panic: x\n");
        let dest = dir.path().join("bundle.zip");

        let summary = build_bundle(
            &logs,
            Some(r#"{"version":"0.11.0","deep":{}}"#),
            Some("0.11.0"),
            &dest,
        )
        .unwrap();

        assert_eq!(summary.log_files, 2);
        assert!(!summary.truncated);
        assert!(summary.doctor_ok);
        let entries = zip_entries(&dest);
        let names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"manifest.txt"));
        assert!(names.contains(&"doctor.json"));
        assert!(names.contains(&"logs/shannon-desktop.log.2026-09-27"));
        assert!(names.contains(&"logs/crash-1790000000-42.log"));
        let manifest = entries.iter().find(|(n, _)| n == "manifest.txt").unwrap();
        let manifest = String::from_utf8_lossy(&manifest.1);
        assert!(manifest.contains("desktop version:"));
        assert!(manifest.contains("Privacy:"));
        // Doctor version surfaced into the manifest.
        assert!(manifest.contains("bundled shannon (doctor): 0.11.0"));
    }

    #[test]
    fn size_cap_skips_large_files_and_reports_truncation() {
        let dir = tempfile::tempdir().unwrap();
        let logs = dir.path().join("logs");
        std::fs::create_dir_all(&logs).unwrap();
        write(&logs.join("shannon-desktop.log.small"), b"tiny\n");
        // A file at exactly the cap cannot fit next to the tiny one (5 +
        // MAX > MAX) — it must be skipped; later small files still fit.
        write(
            &logs.join("shannon-desktop.log.huge"),
            &vec![b'a'; (MAX_LOG_BYTES) as usize],
        );
        write(&logs.join("crash-1.log"), b"included\n");
        let dest = dir.path().join("bundle.zip");

        let summary = build_bundle(&logs, None, None, &dest).unwrap();

        assert_eq!(summary.log_files, 2);
        assert!(summary.truncated);
        assert!(!summary.doctor_ok);
        let names: Vec<String> = zip_entries(&dest).into_iter().map(|(n, _)| n).collect();
        assert!(names.contains(&"logs/shannon-desktop.log.small".to_string()));
        assert!(names.contains(&"logs/crash-1.log".to_string()));
        assert!(!names.iter().any(|n| n.contains("huge")));
        // No doctor report → manifest says so.
        let entries = zip_entries(&dest);
        let manifest = entries.iter().find(|(n, _)| n == "manifest.txt").unwrap();
        assert!(String::from_utf8_lossy(&manifest.1).contains("not found"));
    }

    #[test]
    fn missing_logs_dir_yields_manifest_only_bundle() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("bundle.zip");
        let summary = build_bundle(&dir.path().join("no-such-logs"), None, None, &dest).unwrap();
        assert_eq!(summary.log_files, 0);
        assert!(dest.exists());
        // The `logs/` directory entry is always created; no file entries.
        let names: Vec<String> = zip_entries(&dest)
            .into_iter()
            .map(|(n, _)| n)
            .filter(|n| n != "logs/")
            .collect();
        assert_eq!(names, vec!["manifest.txt".to_string()]);
    }
}
