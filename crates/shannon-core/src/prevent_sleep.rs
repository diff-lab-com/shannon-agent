//! Prevent Sleep
//!
//! Platform-aware sleep prevention during long-running operations.
//! On macOS, uses `caffeinate` to prevent idle sleep.
//! On other platforms, provides a no-op implementation.

#[cfg(target_os = "macos")]
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Reference count for nested sleep prevention
static PREVENT_SLEEP_REF_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Stored caffeinate child process (macOS only)
#[cfg(target_os = "macos")]
static CAFFEINATE_CHILD: Mutex<Option<std::process::Child>> = Mutex::new(None);

/// Whether prevent sleep is currently active
pub fn is_preventing_sleep() -> bool {
    PREVENT_SLEEP_REF_COUNT.load(Ordering::SeqCst) > 0
}

/// Start preventing sleep (reference counted)
pub fn start_prevent_sleep() {
    let prev = PREVENT_SLEEP_REF_COUNT.fetch_add(1, Ordering::SeqCst);
    if prev == 0 {
        #[cfg(target_os = "macos")]
        {
            spawn_caffeinate();
        }
        tracing::debug!("Sleep prevention started (ref count: {})", prev + 1);
    }
}

/// Stop preventing sleep (reference counted)
pub fn stop_prevent_sleep() {
    let prev = PREVENT_SLEEP_REF_COUNT.fetch_sub(1, Ordering::SeqCst);
    if prev == 0 {
        // Underflow — restore the count and return
        PREVENT_SLEEP_REF_COUNT.fetch_add(1, Ordering::SeqCst);
        tracing::warn!("stop_prevent_sleep called without matching start_prevent_sleep");
        return;
    }
    if prev == 1 {
        #[cfg(target_os = "macos")]
        {
            kill_caffeinate();
        }
        tracing::debug!("Sleep prevention stopped");
    }
}

/// Force stop sleep prevention regardless of reference count
pub fn force_stop_prevent_sleep() {
    PREVENT_SLEEP_REF_COUNT.store(0, Ordering::SeqCst);
    #[cfg(target_os = "macos")]
    {
        kill_caffeinate();
    }
    tracing::debug!("Sleep prevention force stopped");
}

/// Platform: spawn caffeinate process (macOS)
#[cfg(target_os = "macos")]
fn spawn_caffeinate() {
    use std::process::{Command, Stdio};

    match Command::new("caffeinate")
        .args(["-i", "-t", "300"]) // -i: prevent idle sleep, -t: 5 min timeout
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => {
            tracing::debug!("Started caffeinate (pid: {:?})", child.id());
            // Take previous child out of the lock before blocking on kill/wait
            let prev = CAFFEINATE_CHILD
                .lock()
                .ok()
                .and_then(|mut guard| guard.take());
            if let Some(mut prev) = prev {
                let _ = prev.kill();
                let _ = prev.wait();
            }
            if let Ok(mut guard) = CAFFEINATE_CHILD.lock() {
                *guard = Some(child);
            }
        }
        Err(e) => {
            tracing::warn!("Failed to start caffeinate: {}", e);
        }
    }
}

/// Platform: kill caffeinate process (macOS)
#[cfg(target_os = "macos")]
fn kill_caffeinate() {
    if let Ok(mut guard) = CAFFEINATE_CHILD.lock() {
        if let Some(ref mut child) = *guard {
            let _ = child.kill();
            let _ = child.wait();
            tracing::debug!("Stopped caffeinate");
        }
        *guard = None;
    }
}

/// Platform: no-op for non-macOS
/// Platform: no-op stub for non-macOS (real impl gated by `#[cfg(target_os = "macos")]`)
#[cfg(not(target_os = "macos"))]
#[allow(dead_code)] // KEEP: cross-platform stub
fn spawn_caffeinate() {}

/// Platform: no-op stub for non-macOS
#[cfg(not(target_os = "macos"))]
#[allow(dead_code)] // KEEP: cross-platform stub
fn kill_caffeinate() {}

/// RAII guard that prevents sleep while alive.
///
/// Call [`PreventSleepGuard::new()`] at the start of a long-running operation.
/// Sleep prevention stops automatically when the guard is dropped.
pub struct PreventSleepGuard;

impl PreventSleepGuard {
    /// Create a new guard that prevents sleep until dropped.
    pub fn new() -> Self {
        start_prevent_sleep();
        PreventSleepGuard
    }
}

impl Drop for PreventSleepGuard {
    fn drop(&mut self) {
        stop_prevent_sleep();
    }
}

impl Default for PreventSleepGuard {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::sync::Mutex;
    use std::sync::OnceLock;

    // Mutex to serialize tests that share the global atomic state
    static TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    fn lock() -> std::sync::MutexGuard<'static, ()> {
        TEST_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
    }
    use super::*;

    // Reset state before each test to avoid interference
    fn reset_state() {
        PREVENT_SLEEP_REF_COUNT.store(0, Ordering::SeqCst);
    }

    #[test]
    fn test_initial_state() {
        let _guard = lock();
        reset_state();
        assert!(!is_preventing_sleep());
    }

    #[test]
    fn test_reference_counting() {
        let _guard = lock();
        reset_state();
        assert!(!is_preventing_sleep());
        start_prevent_sleep();
        assert!(is_preventing_sleep());
        stop_prevent_sleep();
        assert!(!is_preventing_sleep());
    }

    #[test]
    fn test_nested() {
        let _guard = lock();
        reset_state();
        start_prevent_sleep();
        start_prevent_sleep();
        assert!(is_preventing_sleep());
        stop_prevent_sleep();
        assert!(is_preventing_sleep()); // Still active
        stop_prevent_sleep();
        assert!(!is_preventing_sleep());
    }

    #[test]
    fn test_force_stop() {
        let _guard = lock();
        reset_state();
        start_prevent_sleep();
        start_prevent_sleep();
        force_stop_prevent_sleep();
        assert!(!is_preventing_sleep());
    }
}
