//! macOS backend — `caffeinate -i` child process.
//!
//! Kept byte-for-byte equivalent to the pre-split implementation: acquire
//! spawns `caffeinate -i -t 300` (prevent idle sleep, 5-minute refresh
//! window is irrelevant because we kill the process on release), release
//! kills it. Only ever compiled on `target_os = "macos"`.

use std::sync::Mutex;

/// Stored caffeinate child process.
static CAFFEINATE_CHILD: Mutex<Option<std::process::Child>> = Mutex::new(None);

/// Poison-tolerant lock on [`CAFFEINATE_CHILD`]: a panicked thread must not
/// strand a running `caffeinate` (Minor #3, fix round 1). The guarded value
/// is a plain `Child` with no cross-call invariant, so recovering it after
/// poison is sound — the poisoned path still kill+wait-s the stored child.
fn child_slot() -> std::sync::MutexGuard<'static, Option<std::process::Child>> {
    CAFFEINATE_CHILD
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Spawn `caffeinate` (previously-held child, if any, is reaped first).
pub(super) fn acquire() {
    use std::process::{Command, Stdio};

    match Command::new("caffeinate")
        .args(["-i", "-t", "300"]) // -i: prevent idle sleep, -t: 5 min timeout
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => {
            tracing::debug!("Started caffeinate (pid: {:?})", child.id());
            // Hold the slot across reap + store so a concurrent
            // force-stop/panic can't interleave; the killed child needs
            // nothing from this mutex to die, so blocking here is safe.
            let mut slot = child_slot();
            if let Some(mut prev) = slot.take() {
                let _ = prev.kill();
                let _ = prev.wait();
            }
            *slot = Some(child);
        }
        Err(e) => {
            tracing::warn!("Failed to start caffeinate: {}", e);
        }
    }
}

/// Kill the stored caffeinate process (no-op when none is alive).
pub(super) fn release() {
    let mut slot = child_slot();
    if let Some(ref mut child) = *slot {
        let _ = child.kill();
        let _ = child.wait();
        tracing::debug!("Stopped caffeinate");
    }
    *slot = None;
}
