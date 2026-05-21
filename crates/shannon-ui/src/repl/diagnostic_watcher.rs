//! DiagnosticWatcher — background diagnostic refresh triggered by source changes.
//!
//! When source files change, the SourceWatcher marks the DiagnosticStore as stale.
//! This watcher detects the stale flag and spawns a background `cargo check` (or
//! equivalent) to refresh diagnostics. Results are sent back via a channel and
//! applied to the DiagnosticStore on the next main-loop tick.

use shannon_tools::CliDiagnosticResult;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Handle for receiving diagnostic results from background tasks.
pub(crate) type DiagnosticReceiver = tokio::sync::mpsc::UnboundedReceiver<CliDiagnosticResult>;

/// Spawns a background diagnostic run for the given project directory.
///
/// Returns a receiver that will receive the result once the check completes.
/// Only one check runs at a time — if a check is already pending, the request
/// is silently dropped (debounce).
pub(crate) fn spawn_diagnostic_run(
    project_dir: PathBuf,
    pending: Arc<Mutex<bool>>,
) -> Option<DiagnosticReceiver> {
    let mut guard = pending.blocking_lock();
    if *guard {
        // Already running — debounce.
        return None;
    }
    *guard = true;
    drop(guard);

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

    tokio::spawn(async move {
        let result = shannon_tools::run_cli_diagnostics(&project_dir).await;
        let _ = tx.send(result);
        let mut guard = pending.lock().await;
        *guard = false;
    });

    Some(rx)
}

/// Async variant that can be called from async context.
pub(crate) async fn spawn_diagnostic_run_async(
    project_dir: PathBuf,
    pending: Arc<Mutex<bool>>,
) -> Option<DiagnosticReceiver> {
    let mut guard = pending.lock().await;
    if *guard {
        return None;
    }
    *guard = true;
    drop(guard);

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

    tokio::spawn(async move {
        let result = shannon_tools::run_cli_diagnostics(&project_dir).await;
        let _ = tx.send(result);
        let mut guard = pending.lock().await;
        *guard = false;
    });

    Some(rx)
}

/// Try to receive a completed diagnostic result without blocking.
///
/// Returns `Some(result)` if a result is ready, `None` otherwise.
pub(crate) fn try_receive(rx: &mut DiagnosticReceiver) -> Option<CliDiagnosticResult> {
    rx.try_recv().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn test_spawn_diagnostic_run_debounce() {
        let pending = Arc::new(Mutex::new(false));
        let dir = std::env::current_dir().unwrap();

        // First spawn should succeed.
        let result = spawn_diagnostic_run_async(dir.clone(), pending.clone()).await;
        assert!(result.is_some());

        // Second spawn while first is running should be debounced.
        let result2 = spawn_diagnostic_run_async(dir.clone(), pending.clone()).await;
        assert!(result2.is_none());
    }

    #[tokio::test]
    async fn test_spawn_diagnostic_run_empty_dir() {
        let pending = Arc::new(Mutex::new(false));
        let temp = tempfile::tempdir().unwrap();

        let mut rx = spawn_diagnostic_run_async(temp.path().to_path_buf(), pending)
            .await
            .unwrap();

        // Should complete without error (no project type detected).
        let result = tokio::time::timeout(Duration::from_secs(10), rx.recv())
            .await
            .expect("timeout")
            .expect("channel closed");

        assert!(result.success);
        assert!(result.diagnostics.is_empty());
        assert!(result.error.is_none());
    }

    #[tokio::test]
    async fn test_try_receive_empty() {
        let (_, mut rx) = tokio::sync::mpsc::unbounded_channel::<CliDiagnosticResult>();
        assert!(try_receive(&mut rx).is_none());
    }

    #[tokio::test]
    async fn test_try_receive_ready() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<CliDiagnosticResult>();
        tx.send(CliDiagnosticResult {
            diagnostics: Vec::new(),
            success: true,
            error: None,
        })
        .unwrap();
        let result = try_receive(&mut rx);
        assert!(result.is_some());
        assert!(result.unwrap().success);
    }
}
