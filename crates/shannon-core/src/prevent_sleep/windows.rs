//! Windows backend — `SetThreadExecutionState` wake lock.
//!
//! `ES_CONTINUOUS | ES_SYSTEM_REQUIRED` tells the OS this thread keeps the
//! system awake (idle timers must not suspend the machine); resetting to
//! bare `ES_CONTINUOUS` restores the default. No child process, no external
//! binary — the call is always available on Windows, which is why the
//! desktop reports `keepAwakeSupported: true` at compile time.
//!
//! Only compiled on `target_os = "windows"`. The `windows` crate is already
//! a target dependency of shannon-core (Windows Job Object sandbox); this
//! only enables its `Win32_System_Power` feature — no new crate.

/// Acquire the wake lock for the calling thread.
pub(super) fn acquire() {
    use windows::Win32::System::Power::{
        SetThreadExecutionState, ES_CONTINUOUS, ES_SYSTEM_REQUIRED,
    };

    // SAFETY: plain FFI call with flag arguments; no pointers involved.
    let previous = unsafe { SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED) };
    if previous.0 == 0 {
        tracing::warn!("SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED) failed");
    }
}

/// Release the wake lock (restore the calling thread's default state).
pub(super) fn release() {
    use windows::Win32::System::Power::{SetThreadExecutionState, ES_CONTINUOUS};

    // SAFETY: plain FFI call with a flag argument; no pointers involved.
    let previous = unsafe { SetThreadExecutionState(ES_CONTINUOUS) };
    if previous.0 == 0 {
        tracing::warn!("SetThreadExecutionState(ES_CONTINUOUS) failed");
    }
}
