//! macOS backend — `caffeinate -i` child process.
//!
//! Kept byte-for-byte equivalent to the pre-split implementation: acquire
//! spawns `caffeinate -i -t 300` (prevent idle sleep, 5-minute refresh
//! window is irrelevant because we kill the process on release), release
//! kills it. Only ever compiled on `target_os = "macos"`.

use std::sync::Mutex;

/// Stored caffeinate child process.
static CAFFEINATE_CHILD: Mutex<Option<std::process::Child>> = Mutex::new(None);

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
            // Take previous child out of the lock before blocking on kill/wait
            let prev = CAFFEINATE_CHILD.lock().ok().and_then(|mut guard| guard.take());
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

/// Kill the stored caffeinate process (no-op when none is alive).
pub(super) fn release() {
    if let Ok(mut guard) = CAFFEINATE_CHILD.lock() {
        if let Some(ref mut child) = *guard {
            let _ = child.kill();
            let _ = child.wait();
            tracing::debug!("Stopped caffeinate");
        }
        *guard = None;
    }
}
