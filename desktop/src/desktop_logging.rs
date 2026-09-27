//! Local file logging + crash capture for the desktop shell (audit batch 3, B).
//!
//! `shannon desktop` detaches from its parent terminal, so everything the
//! app writes to stderr is lost in normal use — a crash or a misbehaving
//! build used to arrive at support with nothing attached. This module adds:
//!
//! - a **redacting line-writer** wrapping the tracing non-blocking file
//!   appender: every completed line passes through the session-log
//!   redaction policy before it touches disk, so a secret-shaped token in
//!   a log argument doesn't land in a persistent file;
//! - **retention**: daily-rotated `shannon-desktop.log.*` files older than
//!   the keep window are deleted on startup; crash reports get a longer
//!   window;
//! - a **panic hook** writing a redacted `crash-<ts>-<pid>.log` (message +
//!   forced backtrace) before delegating to the previous hook.
//!
//! Everything stays on disk under `~/.shannon/logs/` — nothing is uploaded,
//! consistent with the project's no-telemetry stance. Exposing it to support
//! is an explicit user action (copying the file), which is the Phase-1 story;
//! an in-app "export diagnostics" button is a possible follow-up.

use shannon_core::session_log::redaction::redact_string;
use std::io::Write as _;
use std::path::Path;

/// Delete rotated `shannon-desktop.log.<date>` files older than `keep_days`
/// and `crash-*.log` reports older than `max(keep_days * 4, 30)` days
/// (crash reports are rare and valuable; give them a longer window).
/// Best-effort: any failure is ignored — cleanup must never block or break
/// startup.
pub fn cleanup_retention(log_dir: &Path, keep_days: u32) {
    let log_cutoff = chrono::Utc::now().date_naive() - chrono::Duration::days(keep_days as i64);
    let crash_cutoff =
        chrono::Utc::now().date_naive() - chrono::Duration::days((keep_days as i64 * 4).max(30));
    let Ok(entries) = std::fs::read_dir(log_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let removable = if let Some(date) = name.strip_prefix("shannon-desktop.log.") {
            chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
                .map(|d| d < log_cutoff)
                .unwrap_or(false)
        } else if let Some(rest) = name.strip_prefix("crash-") {
            // crash-<unix-secs>-<pid>.log — compare timestamps, not dates.
            rest.split('-')
                .next()
                .and_then(|ts| ts.parse::<i64>().ok())
                .map(|ts_secs| {
                    chrono::DateTime::from_timestamp(ts_secs, 0)
                        .map(|dt| dt.date_naive() < crash_cutoff)
                        .unwrap_or(false)
                })
                .unwrap_or(false)
        } else {
            false
        };
        if removable {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Install a global panic hook that writes a redacted crash report to
/// `<log_dir>/crash-<unix-secs>-<pid>.log` (version, platform, panic
/// message, forced backtrace), then delegates to the previously-installed
/// hook so stderr behavior (and any outer handler) is preserved.
pub fn install_panic_hook(log_dir: &Path) {
    let log_dir = log_dir.to_path_buf();
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let file = log_dir.join(format!("crash-{ts}-{}.log", std::process::id()));
        // Panic payloads can interpolate user data — pass the message
        // through the same redaction policy as the log sink.
        let message = redact_string(&info.to_string());
        let backtrace = std::backtrace::Backtrace::force_capture();
        let report = format!(
            "shannon-desktop crash report\n\
             version:  {}\n\
             platform: {} ({})\n\
             ts:       {} (unix secs)\n\
             \n{}\n\
             \nbacktrace:\n{backtrace}\n",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH,
            ts,
            message,
        );
        let _ = std::fs::write(&file, report);
        previous(info);
    }));
}

/// [`tracing_subscriber::fmt::MakeWriter`] that masks every completed line
/// through the session-log redaction policy before forwarding it to the
/// underlying non-blocking file writer.
///
/// tracing emits one event per `write_all`, but nothing guarantees line
/// boundaries, so bytes are buffered until a `\n` arrives (or flush).
#[derive(Clone)]
pub struct RedactingMakeWriter {
    inner: tracing_appender::non_blocking::NonBlocking,
}

impl RedactingMakeWriter {
    pub fn new(inner: tracing_appender::non_blocking::NonBlocking) -> Self {
        Self { inner }
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for RedactingMakeWriter {
    type Writer = RedactingWriter;

    fn make_writer(&'a self) -> Self::Writer {
        RedactingWriter {
            inner: self.inner.clone(),
            buf: Vec::with_capacity(256),
        }
    }
}

pub struct RedactingWriter {
    inner: tracing_appender::non_blocking::NonBlocking,
    buf: Vec<u8>,
}

impl Write for RedactingWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.buf.extend_from_slice(buf);
        while let Some(pos) = self.buf.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.buf.drain(..=pos).collect();
            let redacted = redact_string(&String::from_utf8_lossy(&line));
            self.inner.write_all(redacted.as_bytes())?;
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        if !self.buf.is_empty() {
            let line = std::mem::take(&mut self.buf);
            let redacted = redact_string(&String::from_utf8_lossy(&line));
            self.inner.write_all(redacted.as_bytes())?;
        }
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redaction_policy_masks_secret_shaped_lines() {
        // The built-in policy is fail-closed: token-shaped strings are
        // masked regardless of configuration. The file sink relies on this
        // for every line it writes.
        let clean = redact_string("engine started on 127.0.0.1:33420");
        assert_eq!(clean, "engine started on 127.0.0.1:33420");
        let dirty = redact_string("provider error for sk-abc123DEF456ghi789JKL");
        assert!(
            !dirty.contains("sk-abc123DEF456ghi789JKL"),
            "secret leaked: {dirty}"
        );
    }

    #[test]
    fn retention_keeps_recent_and_removes_old() {
        let dir = tempfile::tempdir().unwrap();
        let old_log = dir.path().join("shannon-desktop.log.2020-01-01");
        let new_log = dir.path().join("shannon-desktop.log.2099-01-01");
        let old_crash = dir.path().join("crash-1577836800-123.log"); // 2020-01-01
        let new_crash = dir.path().join("crash-4070908800-123.log"); // year 2099
        let unrelated = dir.path().join("other.txt");
        for f in [&old_log, &new_log, &old_crash, &new_crash, &unrelated] {
            std::fs::write(f, b"x").unwrap();
        }
        cleanup_retention(dir.path(), 7);
        assert!(!old_log.exists(), "old rotation should be pruned");
        assert!(new_log.exists(), "recent rotation must survive");
        assert!(!old_crash.exists(), "old crash report should be pruned");
        assert!(new_crash.exists(), "recent crash report must survive");
        assert!(unrelated.exists(), "unrelated files must be untouched");
    }
}
