//! Optional structured crash capture for headless/CI runs.
//!
//! Enabled only when `SHANNON_CRASH_DIR` is set; when absent every entry point
//! here is a no-op so normal runs are behaviorally unchanged (default-off, no
//! semver surface). The dogfood loop (docs/plans/autonomous-improvement-loop.md
//! §5.4) sets the env var per task and harvests `<ts>-<pid>-panic.crash.json`
//! files from it as triage artifacts.

use std::backtrace::Backtrace;
use std::collections::VecDeque;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

/// Recent NDJSON events kept for crash context (dogfood plan: last 20).
const RING_CAP: usize = 20;
/// Per-event truncation so one huge tool result cannot blow up the crash file.
const EVENT_MAX_CHARS: usize = 2000;

static INSTALLED: AtomicBool = AtomicBool::new(false);
static RING: OnceLock<Mutex<VecDeque<String>>> = OnceLock::new();
static CRASH_DIR: RwLock<Option<PathBuf>> = RwLock::new(None);

/// Record an NDJSON line for crash context.
///
/// Cheap early-out when capture is not armed; called unconditionally from the
/// NDJSON emitters in `main.rs`.
pub(crate) fn record(line: &str) {
    if !INSTALLED.load(Ordering::Relaxed) {
        return;
    }
    if let Some(ring) = RING.get() {
        if let Ok(mut guard) = ring.lock() {
            let mut event = line.to_string();
            if event.chars().count() > EVENT_MAX_CHARS {
                event = event.chars().take(EVENT_MAX_CHARS).collect();
            }
            guard.push_back(event);
            while guard.len() > RING_CAP {
                guard.pop_front();
            }
        }
    }
}

/// Arm crash capture from `SHANNON_CRASH_DIR`; no-op when the var is unset or
/// empty (default-off contract).
pub(crate) fn install_from_env() {
    let dir = match std::env::var("SHANNON_CRASH_DIR") {
        Ok(v) if !v.trim().is_empty() => PathBuf::from(v),
        _ => return,
    };
    install_at(dir);
}

/// Arm crash capture, writing crash files into `dir` (test/programmatic entry).
fn install_at(dir: PathBuf) {
    if fs::create_dir_all(&dir).is_err() {
        return; // cannot persist crash files; leave capture disarmed
    }
    let _ = RING.set(Mutex::new(VecDeque::with_capacity(RING_CAP)));
    if let Ok(mut guard) = CRASH_DIR.write() {
        *guard = Some(dir);
    }
    if INSTALLED.swap(true, Ordering::SeqCst) {
        return; // hook already chained; only the directory was refreshed
    }
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        write_crash_file(info);
        prev(info); // preserve default stderr reporting + exit semantics
    }));
}

/// Snapshot the recent-event ring (never panics: lock failures yield empty).
fn snapshot() -> Vec<String> {
    RING.get()
        .and_then(|ring| ring.lock().ok())
        .map(|guard| guard.iter().cloned().collect())
        .unwrap_or_default()
}

/// Best-effort crash file write; must never panic itself (it *is* the panic
/// handler), so every fallible step degrades to a no-op.
fn write_crash_file(info: &std::panic::PanicHookInfo<'_>) {
    let Some(dir) = CRASH_DIR.read().ok().and_then(|g| g.clone()) else {
        return;
    };
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let message = if let Some(s) = info.payload().downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = info.payload().downcast_ref::<String>() {
        s.clone()
    } else {
        "non-string panic payload".to_string()
    };
    let location = info
        .location()
        .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()));
    let doc = serde_json::json!({
        "ts_ms": ts,
        "pid": std::process::id(),
        "message": message,
        "location": location,
        "cwd": std::env::current_dir().ok().map(|p| p.display().to_string()),
        "argv": std::env::args().collect::<Vec<_>>(),
        "recent_events": snapshot(),
        "backtrace": format!("{}", Backtrace::force_capture()),
    });
    let path = dir.join(format!("{}-{}-panic.crash.json", ts, std::process::id()));
    if let Ok(mut f) = fs::File::create(path) {
        let _ = serde_json::to_writer_pretty(&mut f, &doc);
        let _ = f.sync_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serializes the tests below: the event ring and the process-global
    /// panic hook they install are shared state, and under plain
    /// `cargo test` (libtest: one process, many threads) concurrent tests
    /// steal each other's hook / interleave into the same ring. nextest
    /// never sees this (one process per test); the lock restores the same
    /// isolation under libtest.
    fn global_state_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "shannon-crash-hook-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock before epoch")
                .as_nanos()
        ));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn test_record_ring_caps_at_20_and_truncates_long_events() {
        let _guard = global_state_lock();
        let dir = temp_dir();
        install_at(dir.clone());
        for i in 0..30 {
            record(&format!(r#"{{"i":{i}}}"#));
        }
        let long: String = "x".repeat(EVENT_MAX_CHARS + 500);
        record(&long);
        let snap = snapshot();
        assert_eq!(snap.len(), RING_CAP, "ring must cap at {RING_CAP}");
        assert!(
            snap.iter().all(|e| e.chars().count() <= EVENT_MAX_CHARS),
            "events must be truncated to {EVENT_MAX_CHARS} chars"
        );
        // 31 records total -> ring holds the last 20: i=11..29 plus the long one.
        assert!(
            snap[0].contains("\"i\":11"),
            "oldest events must be evicted"
        );
        // Restore default hook so later tests in this binary are unaffected.
        let _ = std::panic::take_hook();
    }

    #[test]
    fn test_panic_writes_structured_crash_json() {
        let _guard = global_state_lock();
        let dir = temp_dir();
        // Silence the default hook for the duration of the test, then install.
        let original = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        install_at(dir.clone());
        record(r#"{"type":"tool_result","name":"Edit"}"#);

        let result = std::panic::catch_unwind(|| panic!("boom-crash-hook-test"));
        assert!(result.is_err(), "test panic must be caught");

        let entries: Vec<_> = fs::read_dir(&dir)
            .expect("read crash dir")
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .ends_with("-panic.crash.json")
            })
            .collect();
        assert_eq!(entries.len(), 1, "exactly one crash file expected");
        let content = fs::read_to_string(entries[0].path()).expect("read crash file");
        let doc: serde_json::Value = serde_json::from_str(&content).expect("parse crash json");
        assert_eq!(doc["message"], "boom-crash-hook-test");
        assert!(doc["location"].as_str().is_some(), "location must be set");
        assert!(
            doc["recent_events"]
                .as_array()
                .expect("recent_events array")
                .iter()
                .any(|e| e.to_string().contains("tool_result")),
            "ring must appear in crash context"
        );
        assert!(
            !doc["backtrace"].as_str().unwrap_or_default().is_empty(),
            "backtrace must be captured"
        );
        std::panic::set_hook(original);
    }
}
