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

// F5 (Settings R3 followups) — platform-gated native smoke test. The
// backend historically only proved that it *cross-compiles*; the test below
// exercises the real spawn/kill semantics whenever the suite runs on a
// macOS machine (Cross-platform Check macos leg, nightly core-tests, local
// `cargo test`). It self-skips when the platform environment is unavailable
// so a runner limitation can never turn the CI gate red.
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::{CAFFEINATE_CHILD, acquire, child_slot, release};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, PoisonError};
    use std::time::{Duration, Instant};

    /// Upper bound for the whole smoke test (F5: must finish within 10s).
    const SMOKE_BUDGET: Duration = Duration::from_secs(10);

    /// Timeout protection: if the smoke body ever blocks past
    /// [`SMOKE_BUDGET`] (a wedged kill/wait), fail the process instead of
    /// hanging the suite. nextest runs each test in its own process, so the
    /// hard exit costs only this test; under plain `cargo test` it
    /// sacrifices the remaining results — acceptable, that path is already
    /// broken. Dropping the returned guard (also during unwind) disarms the
    /// watchdog.
    fn arm_watchdog() -> impl Drop {
        struct WatchdogGuard(Arc<AtomicBool>);
        impl Drop for WatchdogGuard {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Relaxed);
            }
        }

        let disarmed = Arc::new(AtomicBool::new(false));
        let flag = disarmed.clone();
        let _ = std::thread::spawn(move || {
            let start = Instant::now();
            while start.elapsed() < SMOKE_BUDGET {
                if flag.load(Ordering::Relaxed) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            eprintln!("prevent_sleep macOS smoke exceeded its {SMOKE_BUDGET:?} budget — aborting");
            std::process::exit(101);
        });
        WatchdogGuard(disarmed)
    }

    /// Test-visible pid of the stored caffeinate child.
    fn stored_child_pid() -> Option<u32> {
        CAFFEINATE_CHILD
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .map(std::process::Child::id)
    }

    /// `kill(pid, 0)` liveness probe: rc 0 (signal deliverable) and EPERM
    /// (exists, not ours to signal) both mean "process exists"; ESRCH means
    /// gone.
    fn process_alive(pid: u32) -> bool {
        // SAFETY: signal 0 sends nothing; `pid` is this test's own child.
        let rc = unsafe { libc::kill(pid as i32, 0) };
        if rc == 0 {
            return true;
        }
        std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }

    /// Does `pgrep -f <pattern>` list exactly `pid`? `None` = pgrep itself
    /// unavailable (then the caller falls back to the kill(0) probe only).
    /// `-f` matches the full command line, so the pattern also proves the
    /// spawned argv really is `caffeinate -i ...`. Exact-token compare: a
    /// substring match would confuse pid 123 with pid 1234.
    fn pgrep_lists_pid(pattern: &str, pid: u32) -> Option<bool> {
        let output = std::process::Command::new("pgrep")
            .arg("-f")
            .arg(pattern)
            .output()
            .ok()?;
        let pid = pid.to_string();
        Some(
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .any(|line| line.trim() == pid),
        )
    }

    /// PATH executable probe (mirrors the Linux module's availability
    /// scan): true only when `caffeinate` exists with an exec bit in some
    /// PATH directory.
    fn caffeinate_on_path() -> bool {
        use std::os::unix::fs::MetadataExt;

        std::env::var("PATH")
            .unwrap_or_default()
            .split(':')
            .filter(|dir| !dir.is_empty())
            .any(|dir| {
                std::path::Path::new(dir)
                    .join("caffeinate")
                    .metadata()
                    .map(|meta| meta.mode() & 0o111 != 0)
                    .unwrap_or(false)
            })
    }

    /// Native round trip: acquire must leave a live `caffeinate -i` child
    /// that is kernel- and pgrep-visible, and release must kill and reap
    /// it. Skips when `caffeinate` is not on PATH (hardened/minimal runner
    /// image) — a skip prints and returns, it never fails the gate.
    #[test]
    fn caffeinate_smoke_acquire_visible_release_gone() {
        // Serialize with the refcount tests in `prevent_sleep::tests`:
        // their start/stop pairs drive this module's acquire/release, so a
        // concurrent mod.rs test could kill our child mid-assertion under
        // plain `cargo test`.
        let _serial = super::super::tests::shared_test_lock();
        let _watchdog = arm_watchdog();

        // Defensive: reap anything a previously panicking test left.
        release();

        // Probe first, then decide run vs skip.
        if !caffeinate_on_path() {
            eprintln!("skip: caffeinate not on PATH — macOS smoke not applicable here");
            return;
        }

        // Acquire → the child exists as a real OS process.
        acquire();
        let pid =
            stored_child_pid().expect("caffeinate on PATH: acquire must spawn and store a child");
        {
            let mut slot = child_slot();
            let child = slot.as_mut().expect("pid came from this slot");
            assert!(
                child.try_wait().expect("try_wait on live child").is_none(),
                "child must still be running right after acquire"
            );
        }
        assert!(
            process_alive(pid),
            "caffeinate pid {pid} must be kernel-visible after acquire"
        );
        if let Some(listed) = pgrep_lists_pid("caffeinate.*-i", pid) {
            assert!(listed, "pgrep -f 'caffeinate.*-i' must list pid {pid}");
        }

        // Release → killed and reaped: slot empty, process gone.
        release();
        assert!(stored_child_pid().is_none(), "release must clear the slot");
        let deadline = Instant::now() + SMOKE_BUDGET;
        loop {
            if !process_alive(pid) {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "caffeinate pid {pid} still alive after release"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        if let Some(listed) = pgrep_lists_pid("caffeinate.*-i", pid) {
            assert!(!listed, "pgrep must no longer list pid {pid} after release");
        }
    }
}
