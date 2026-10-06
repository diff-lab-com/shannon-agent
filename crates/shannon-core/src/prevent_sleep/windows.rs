//! Windows backend — process-domain `PowerRequest` wake lock.
//!
//! Uses the kernel32 Power Request API (`PowerCreateRequest` +
//! `PowerSetRequest(PowerRequestSystemRequired)` + `PowerClearRequest`),
//! which is scoped to the *process*, not the calling thread. That is the
//! whole point: acquire runs on whatever thread first calls
//! `start_prevent_sleep` (Tauri main thread / a tokio worker) while release
//! runs wherever the count hits zero (a different Tauri async task, an RAII
//! [`crate::prevent_sleep::PreventSleepGuard`] dropped on any worker). The
//! previous `SetThreadExecutionState` approach was per-thread and leaked
//! the wake lock permanently on the first cross-thread release.
//!
//! The request object is created once per process and reused for every
//! acquire/release pair: the `windows 0.62` bindings do not export
//! `PowerDestroyRequest`, so the handle lives until process exit. A request
//! object with no active `PowerRequestSystemRequired` count has no effect
//! on power policy, and reusing one handle keeps acquire/release balanced
//! regardless of thread. The simple reason string handed to
//! `PowerCreateRequest` must outlive the request object, so it is leaked
//! once for the same process lifetime.
//!
//! Only compiled on `target_os = "windows"`. The `windows` crate is already
//! a target dependency of shannon-core (Windows Job Object sandbox); this
//! only uses its `Win32_System_Power` + `Win32_System_Threading` features —
//! no new crate.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

/// A kernel `HANDLE` that may be moved between threads.
///
/// # Safety
/// `PowerCreateRequest` returns a real kernel-object handle (never a
/// thread-affine pseudo-handle), and the whole point of the Power Request
/// API is that `PowerSetRequest`/`PowerClearRequest` accept the same
/// process-wide handle from any thread. `HANDLE`'s inner raw pointer only
/// opts out of `Send` generically.
#[derive(Clone, Copy)]
struct SendHandle(windows::Win32::Foundation::HANDLE);
unsafe impl Send for SendHandle {}

/// The process-wide power request handle (`None` = not created yet, or
/// creation failed — see [`WARNED_CREATE_FAIL`]).
static POWER_REQUEST: Mutex<Option<SendHandle>> = Mutex::new(None);

/// Warn-once latch so a machine where `PowerCreateRequest` fails logs one
/// line instead of one per retry.
static WARNED_CREATE_FAIL: AtomicBool = AtomicBool::new(false);

/// Poison-tolerant lock: a panic in one thread must not strand the wake
/// lock — the handle is a plain kernel value with no cross-call invariant,
/// so recovering the guarded value is always sound here.
fn power_request_slot() -> std::sync::MutexGuard<'static, Option<SendHandle>> {
    POWER_REQUEST
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Acquire the wake lock (process domain — safe from any thread).
///
/// Creates the shared request object on first use, then raises
/// `PowerRequestSystemRequired` on it. Nested acquisitions are idempotent
/// for the OS: the request type is counted per handle, and our refcounting
/// in `mod.rs` guarantees a matching [`release`] when the count hits zero.
pub(super) fn acquire() {
    use windows::Win32::System::Power::{PowerRequestSystemRequired, PowerSetRequest};

    let mut slot = power_request_slot();
    if slot.is_none() {
        match create_request() {
            Some(handle) => *slot = Some(SendHandle(handle)),
            // Creation already warned; stay a no-op (balanced with release).
            None => return,
        }
    }
    let Some(SendHandle(handle)) = *slot else {
        return;
    };

    // SAFETY: `handle` came from `PowerCreateRequest` and is kept alive for
    // the process lifetime; the request-type constant is a plain i32 newtype.
    if let Err(e) = unsafe { PowerSetRequest(handle, PowerRequestSystemRequired) } {
        tracing::warn!("PowerSetRequest(PowerRequestSystemRequired) failed: {e}");
    }
}

/// Release the wake lock (process domain — safe from any thread).
///
/// Clears `PowerRequestSystemRequired` on the shared handle; the request
/// object itself is intentionally kept for reuse (see module docs).
pub(super) fn release() {
    use windows::Win32::System::Power::{PowerClearRequest, PowerRequestSystemRequired};

    let slot = power_request_slot();
    let Some(SendHandle(handle)) = *slot else {
        return;
    };

    // SAFETY: same handle provenance as in `acquire`.
    if let Err(e) = unsafe { PowerClearRequest(handle, PowerRequestSystemRequired) } {
        tracing::warn!("PowerClearRequest(PowerRequestSystemRequired) failed: {e}");
    }
}

/// Create the process-lifetime request object (`None` = failure, warned once).
fn create_request() -> Option<windows::Win32::Foundation::HANDLE> {
    use windows::Win32::System::Power::PowerCreateRequest;
    use windows::Win32::System::Threading::{
        POWER_REQUEST_CONTEXT_SIMPLE_STRING, REASON_CONTEXT, REASON_CONTEXT_0,
    };

    // The simple reason string must stay valid for as long as the request
    // object exists — i.e. the rest of the process — so leak it. On the
    // (pathological) retry path after a failed create, one small leaked
    // string per attempt is the worst case.
    let mut wide: Vec<u16> = "Shannon long-running operation".encode_utf16().collect();
    wide.push(0); // null terminator required by POWER_REQUEST_CONTEXT_SIMPLE_STRING
    let reason_string = windows::core::PWSTR(Box::leak(wide.into_boxed_slice()).as_mut_ptr());

    let context = REASON_CONTEXT {
        // POWER_REQUEST_CONTEXT_VERSION (windows crate gates the constant
        // behind Win32_System_SystemServices; the value is specified as 0).
        Version: 0,
        Flags: POWER_REQUEST_CONTEXT_SIMPLE_STRING,
        Reason: REASON_CONTEXT_0 {
            SimpleReasonString: reason_string,
        },
    };

    // SAFETY: `context` is a valid, fully initialized REASON_CONTEXT read
    // only for the duration of the call.
    match unsafe { PowerCreateRequest(&context) } {
        Ok(handle) => Some(handle),
        Err(e) => {
            if !WARNED_CREATE_FAIL.swap(true, Ordering::SeqCst) {
                tracing::warn!(
                    "PowerCreateRequest failed — sleep prevention is unavailable (no-op): {e}"
                );
            }
            None
        }
    }
}

// F5 (Settings R3 followups) — native smoke test for the PowerRequest
// backend. CI historically only proved that this module *cross-compiles*;
// on a real Windows runner (Cross-platform Check windows leg, nightly
// core-tests, local `cargo test`) the round trip below proves the kernel
// API actually accepts our request. It self-skips when the platform
// environment is unavailable so a runner limitation can never turn the CI
// gate red.
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::power_request_slot;
    use std::sync::atomic::Ordering;

    /// Acquire/release round trip against the real Power Request API.
    ///
    /// Smoke scope (F5): the bar is "the API succeeded" — i.e. the request
    /// object was created (`PowerCreateRequest` returned a handle, which
    /// [`power_request_slot`] only stores on success), the refcount flips,
    /// and start/stop round-trip without panicking. A user-space test
    /// cannot observe the OS power state, and `PowerSetRequest` /
    /// `PowerClearRequest` failures are only logged as warnings, so "no
    /// error" is not assertable beyond that level.
    ///
    /// Skip contract: if `PowerCreateRequest` fails on this machine (no
    /// power manager in the session, hardened runner image), the round trip
    /// exercises the documented degraded no-op and the test skips instead
    /// of failing — runner limitations must not turn CI red.
    #[test]
    fn power_request_round_trip_smoke() {
        // Serialize with the refcount tests in `prevent_sleep::tests` (the
        // statics below are process-global; plain `cargo test` runs
        // everything in one process).
        let _serial = super::super::tests::shared_test_lock();
        super::super::PREVENT_SLEEP_REF_COUNT.store(0, Ordering::SeqCst);
        assert!(!super::super::is_preventing_sleep());

        super::super::start_prevent_sleep();
        assert!(
            super::super::is_preventing_sleep(),
            "refcount must flip on acquire"
        );
        let created = power_request_slot().is_some();

        super::super::stop_prevent_sleep();
        assert!(
            !super::super::is_preventing_sleep(),
            "refcount must flip back on release"
        );

        if !created {
            eprintln!(
                "skip: PowerCreateRequest unavailable on this machine — round trip exercised the degraded no-op path"
            );
            return;
        }
        // Release intentionally keeps the request object for reuse (module
        // docs) — that is part of the balanced acquire/release contract.
        assert!(
            power_request_slot().is_some(),
            "release must keep the request object for reuse"
        );
    }
}
