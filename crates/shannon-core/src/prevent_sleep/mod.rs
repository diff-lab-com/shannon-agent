//! Prevent Sleep
//!
//! Platform-aware sleep prevention during long-running operations.
//! Reference-counted: nested begin/end pairs stay balanced and the platform
//! backend is only acquired on the 0→1 transition and released on 1→0.
//!
//! Platform backends (Settings R3 T3, ruling R3 — no new crates):
//! - macOS: `caffeinate -i -t 300` child process (`macos` module)
//! - Linux: `systemd-inhibit --what=idle sleep infinity` child process,
//!   degrading to a warn-once no-op when the binary is missing (`linux`)
//! - Windows: process-domain `PowerRequest` (`PowerCreateRequest` +
//!   `PowerSetRequest(PowerRequestSystemRequired)`) via the `windows` crate
//!   already in the dependency graph (`windows`) — process-scoped so
//!   acquire and release may run on different threads
//! - anything else: inert stubs (`other`)
//!
//! # Platform verification matrix (Settings R3 followups F5)
//!
//! How far each backend is actually exercised — CI proves more than
//! compilation, but none of it replaces an on-device check:
//!
//! | Backend | Verified where | Scope |
//! |---------|----------------|-------|
//! | macOS `caffeinate` | Cross-platform Check (macos-latest leg) and the nightly core-tests run the `macos` module's smoke natively: acquire, child kernel-visible (kill(0) + pgrep on the full command line), release, gone | CI native smoke only — **no manual on-device verification has been performed** |
//! | Windows `PowerRequest` | Cross-platform Check (windows-latest leg) and the nightly core-tests run the `windows` module's round trip natively | CI native smoke at "the Power Request API accepts our request" level — the OS power state itself is not assertable from user space; **no manual on-device verification has been performed** |
//! | Linux `systemd-inhibit` | Unit tests on every PR (Test job): injected-binary spawn/kill semantics, pdeathsig hook, group kill | Real child-process semantics; binary presence is probed at runtime (`systemd_inhibit_available`) and absence degrades to a warn-once no-op |
//!
//! The platform smoke tests self-skip when the platform environment is
//! unavailable (no `caffeinate` on PATH, `PowerCreateRequest` fails) — a
//! runner limitation must never turn the CI gate red.

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as platform;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as platform;
#[cfg(target_os = "linux")]
pub use linux::systemd_inhibit_available;

/// Inert fallback for the runtime capability probe on non-Linux targets:
/// no `systemd-inhibit` backend exists there, so the answer is constant
/// `false`. Keeping the function name on every target (same philosophy as
/// [`other`] below) lets callers like the desktop's capability probe use
/// one `cfg!`-dispatched call site instead of scattering `#[cfg]` blocks.
#[cfg(not(target_os = "linux"))]
pub fn systemd_inhibit_available() -> bool {
    false
}

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
use windows as platform;

/// Inert fallback so other targets (e.g. freebsd) keep compiling with the
/// same public API.
#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
mod other {
    pub(super) fn acquire() {}
    pub(super) fn release() {}
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
use other as platform;

use std::sync::atomic::{AtomicUsize, Ordering};

/// Reference count for nested sleep prevention
static PREVENT_SLEEP_REF_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Whether prevent sleep is currently active
pub fn is_preventing_sleep() -> bool {
    PREVENT_SLEEP_REF_COUNT.load(Ordering::SeqCst) > 0
}

/// Start preventing sleep (reference counted)
pub fn start_prevent_sleep() {
    let prev = PREVENT_SLEEP_REF_COUNT.fetch_add(1, Ordering::SeqCst);
    if prev == 0 {
        platform::acquire();
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
        platform::release();
        tracing::debug!("Sleep prevention stopped");
    }
}

/// Force stop sleep prevention regardless of reference count
pub fn force_stop_prevent_sleep() {
    PREVENT_SLEEP_REF_COUNT.store(0, Ordering::SeqCst);
    platform::release();
    tracing::debug!("Sleep prevention force stopped");
}

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

    /// Poison-tolerant: one panicking test must not cascade PoisonErrors
    /// into every sibling (the guard has no cross-call invariant to break).
    fn lock() -> std::sync::MutexGuard<'static, ()> {
        TEST_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// THE serialization point for every test that touches the module's
    /// global statics (`PREVENT_SLEEP_REF_COUNT`, the platform backends'
    /// stored child/override state) — the `macos`/`windows` smoke modules
    /// AND the `linux` backend tests. Two per-module locks do NOT compose:
    /// the linux spawn tests reset the refcount while these refcount
    /// assertions run, and the interleaving is exactly the flake that
    /// motivated this fix (same disease as the declared_models registry).
    #[cfg(test)]
    pub(super) fn shared_test_lock() -> std::sync::MutexGuard<'static, ()> {
        lock()
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

    /// Settings R3 T3 — the RAII guard and the raw begin/end pair must be
    /// interchangeable: a dropped guard releases exactly the count its
    /// constructor acquired (the shared lock keeps the platform backend
    /// balanced too).
    #[test]
    fn test_guard_drop_releases_once() {
        let _guard = lock();
        reset_state();

        start_prevent_sleep();
        {
            let _raii = PreventSleepGuard::new();
            assert!(is_preventing_sleep());
        }
        assert!(
            is_preventing_sleep(),
            "outer refcount must survive the guard drop"
        );
        stop_prevent_sleep();
        assert!(!is_preventing_sleep());
    }
}
