//! Linux backend — `systemd-inhibit --what=idle` child process.
//!
//! `systemd-inhibit` (part of every systemd installation) holds an
//! idle-suspend inhibitor lock for as long as the wrapped command runs; we
//! wrap `sleep infinity` so the lock lives exactly as long as our child.
//! Acquire spawns it, release kills the whole process group (the inhibitor
//! spawns `sleep` as its own child — killing only the inhibitor would
//! orphan the `sleep infinity` and, worse, not matter since the lock dies
//! with the inhibitor; killing the group reaps both deterministically).
//!
//! Degradation contract: when `systemd-inhibit` is not on PATH (non-systemd
//! distros, minimal containers), acquire degrades to a no-op and warns ONCE
//! — every later start/stop pair stays balanced and silent.
//!
//! Orphan hardening (fix round 1): the child is spawned with
//! `PR_SET_PDEATHSIG = SIGKILL`, so a hard crash of this process (SIGKILL,
//! panic=abort) cannot leave the inhibitor running and holding the
//! idle-inhibit lock forever. The `pre_exec` closure also re-checks the
//! parent to close the classic race where the parent dies between `fork`
//! and `prctl` (the child would already have been re-parented to PID 1 and
//! never receive the death signal — in that window it exits immediately).
//!
//! Only compiled on `target_os = "linux"`.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

/// The stored inhibitor child (None = not holding / degraded no-op).
static INHIBIT_CHILD: Mutex<Option<std::process::Child>> = Mutex::new(None);

/// Poison-tolerant lock on [`INHIBIT_CHILD`]: a panicked thread must not
/// strand the inhibitor. The guarded value is a plain `Child` with no
/// cross-call invariant, so recovering it after poison is sound — and it
/// lets the poisoned path still kill + reap the stored child instead of
/// leaking it.
fn child_slot() -> std::sync::MutexGuard<'static, Option<std::process::Child>> {
    INHIBIT_CHILD
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Warn-once latch for the missing-binary degradation.
static WARNED_MISSING: AtomicBool = AtomicBool::new(false);

/// Test-only injection point: replace the spawned program + args (unit
/// tests point this at `/bin/sleep 30` so spawn/kill semantics are
/// exercised without requiring systemd on the build machine). Never
/// consulted in release builds.
#[cfg(test)]
static TEST_PROGRAM_OVERRIDE: Mutex<Option<(std::ffi::OsString, Vec<std::ffi::OsString>)>> =
    Mutex::new(None);

/// The command to spawn: `systemd-inhibit --what=idle sleep infinity`,
/// unless a test override is installed.
fn inhibit_command() -> (std::ffi::OsString, Vec<std::ffi::OsString>) {
    #[cfg(test)]
    if let Ok(guard) = TEST_PROGRAM_OVERRIDE.lock() {
        if let Some((program, args)) = guard.as_ref() {
            return (program.clone(), args.clone());
        }
    }
    (
        std::ffi::OsString::from("systemd-inhibit"),
        vec![
            std::ffi::OsString::from("--what=idle"),
            std::ffi::OsString::from("sleep"),
            std::ffi::OsString::from("infinity"),
        ],
    )
}

/// Spawn the inhibitor (previously-held child, if any, is reaped first).
pub(super) fn acquire() {
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};

    let (program, args) = inhibit_command();
    let mut command = Command::new(&program);
    command
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        // Own process group so `release` can kill the inhibitor AND its
        // `sleep infinity` child in one signal.
        .process_group(0);
    // Die with the parent even on a hard crash (SIGKILL / panic=abort);
    // see `set_parent_death_signal` for the spawn-race re-check.
    // SAFETY: the hook only calls async-signal-safe syscalls (`prctl`,
    // `getppid`) and runs between fork and exec, as `pre_exec` requires.
    unsafe {
        command.pre_exec(|| set_parent_death_signal());
    }

    match command.spawn() {
        Ok(child) => {
            tracing::debug!("Started systemd-inhibit (pid: {:?})", child.id());
            // Hold the slot across reap + store so a concurrent
            // force-stop/panic can't interleave; the killed child needs
            // nothing from this mutex to die, so blocking here is safe.
            let mut slot = child_slot();
            if let Some(mut prev) = slot.take() {
                kill_child_group(&mut prev);
            }
            *slot = Some(child);
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Degraded platform (no systemd / not on PATH): stay silent
            // after the first warning so per-run begin/end pairs don't
            // spam the log.
            if !WARNED_MISSING.swap(true, Ordering::SeqCst) {
                tracing::warn!(
                    "systemd-inhibit not found — sleep prevention is unavailable on this machine (no-op)"
                );
            }
        }
        Err(e) => {
            tracing::warn!("Failed to start systemd-inhibit: {}", e);
        }
    }
}

/// Kill the stored inhibitor and its process group (no-op when none).
pub(super) fn release() {
    let mut slot = child_slot();
    if let Some(mut child) = slot.take() {
        kill_child_group(&mut child);
        tracing::debug!("Stopped systemd-inhibit");
    }
}

/// `pre_exec` hook: ask the kernel to SIGKILL this (child) process the
/// moment its parent dies, and close the fork/prctl race.
///
/// Safety contract (for `CommandExt::pre_exec`): runs in the forked child
/// before exec, must be async-signal-safe — `prctl(2)` and `getppid(2)`
/// are. Errors abort the spawn (`spawn` reports them to the caller).
unsafe fn set_parent_death_signal() -> std::io::Result<()> {
    // PR_SET_PDEATHSIG survives execve for non-setuid binaries;
    // systemd-inhibit is a normal root-owned executable, so the signal
    // survives into the final image.
    if unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    // Race window: the parent may have died between fork and prctl. The
    // kernel delivers PDEATHSIG only on the death of the *original*
    // parent, so if we were already re-parented to init (PID 1) the signal
    // will never fire — exit now instead of living on as an orphan lock
    // holder. (In PID namespaces init is that namespace's PID 1, which is
    // exactly the re-parent target, so the check stays correct there.)
    if unsafe { libc::getppid() } == 1 {
        return Err(std::io::Error::other(
            "parent died before PR_SET_PDEATHSIG was set",
        ));
    }
    Ok(())
}

/// SIGKILL the child's whole process group, then reap it. Group-kill first
/// (the group leader is the inhibitor; `sleep infinity` dies with it) so no
/// orphaned `sleep` outlives the wake lock.
fn kill_child_group(child: &mut std::process::Child) {
    let pid = child.id();
    // Negative pid targets the process group spawned via process_group(0).
    // ESRCH (already gone) is fine — the goal state is "not running".
    unsafe {
        libc::kill(-(pid as i32), libc::SIGKILL);
    }
    let _ = child.wait();
}

/// Whether `systemd-inhibit` resolves on the current `PATH` — the runtime
/// capability probe the desktop's `get_power_capabilities` serves to the UI.
///
/// Implemented as a direct PATH scan rather than spawning `which`: it works
/// on images where `which` itself is missing, costs no process spawn, and
/// the scanning half is unit-testable without touching the process PATH.
pub fn systemd_inhibit_available() -> bool {
    systemd_inhibit_in_path(&std::env::var("PATH").unwrap_or_default())
}

/// Pure helper behind [`systemd_inhibit_available`]: scan a `:`-separated
/// PATH string for a directory containing an *executable*
/// `systemd-inhibit` entry (any of the owner/group/other exec bits set —
/// matching `execvp`'s own notion of "runnable").
fn systemd_inhibit_in_path(path_var: &str) -> bool {
    use std::os::unix::fs::MetadataExt;

    path_var
        .split(':')
        .filter(|dir| !dir.is_empty())
        .any(|dir| {
            let candidate = std::path::Path::new(dir).join("systemd-inhibit");
            match candidate.metadata() {
                // Regular file (or at least a file-like entry) with some
                // executable bit set; a bare `is_file()` would report
                // 0644 data files as usable and then fail to spawn.
                Ok(meta) => meta.mode() & 0o111 != 0,
                Err(_) => false,
            }
        })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::sync::{Mutex as StdMutex, OnceLock};

    /// Serializes every test that touches the global refcount / child
    /// statics (nextest also isolates per-process; this keeps plain
    /// `cargo test` correct too).
    static TEST_LOCK: OnceLock<StdMutex<()>> = OnceLock::new();

    fn lock() -> std::sync::MutexGuard<'static, ()> {
        TEST_LOCK.get_or_init(|| StdMutex::new(())).lock().unwrap()
    }

    fn set_override(program: &str, args: &[&str]) {
        *TEST_PROGRAM_OVERRIDE.lock().unwrap() = Some((
            program.into(),
            args.iter().map(std::ffi::OsString::from).collect(),
        ));
    }

    fn clear_override() {
        *TEST_PROGRAM_OVERRIDE.lock().unwrap() = None;
    }

    fn reset_state() {
        super::super::PREVENT_SLEEP_REF_COUNT.store(0, Ordering::SeqCst);
        // Reap any child a failed assertion left behind.
        release();
    }

    /// Test-visible pid of the stored inhibitor child.
    fn stored_child_pid() -> Option<u32> {
        INHIBIT_CHILD
            .lock()
            .unwrap()
            .as_ref()
            .map(std::process::Child::id)
    }

    fn process_alive(pid: u32) -> bool {
        std::path::Path::new("/proc").join(pid.to_string()).exists()
    }

    #[test]
    fn default_command_is_systemd_inhibit() {
        let _guard = lock();
        clear_override();
        let (program, args) = inhibit_command();
        assert_eq!(program, std::ffi::OsString::from("systemd-inhibit"));
        assert_eq!(
            args,
            vec![
                std::ffi::OsString::from("--what=idle"),
                std::ffi::OsString::from("sleep"),
                std::ffi::OsString::from("infinity"),
            ]
        );
    }

    #[test]
    fn acquire_stores_child_and_release_kills_it() {
        let _guard = lock();
        set_override("/bin/sleep", &["30"]);
        reset_state();

        acquire();
        let pid = stored_child_pid().expect("acquire must store the spawned child");
        assert!(process_alive(pid), "child must be alive right after spawn");

        release();
        assert!(stored_child_pid().is_none(), "release must drop the child");
        // Reaped + group-killed: /proc entry disappears within a moment.
        let mut gone = !process_alive(pid);
        for _ in 0..50 {
            if gone {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
            gone = !process_alive(pid);
        }
        assert!(gone, "released child (pid {pid}) must be killed and reaped");

        clear_override();
        reset_state();
    }

    #[test]
    fn missing_binary_degrades_to_noop_without_panicking() {
        let _guard = lock();
        set_override("/nonexistent-shannon-test-inhibit", &[]);
        reset_state();

        // Full public-API round trip on a machine without the injected
        // binary: refcounting proceeds, spawn degrades to a no-op.
        super::super::start_prevent_sleep();
        assert!(super::super::is_preventing_sleep());
        assert!(
            stored_child_pid().is_none(),
            "missing binary must not store a child"
        );
        super::super::stop_prevent_sleep();
        assert!(!super::super::is_preventing_sleep());

        clear_override();
        reset_state();
    }

    #[test]
    fn path_scan_finds_systemd_inhibit_only_in_existing_dirs() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("systemd-inhibit");
        std::fs::write(&fake, b"#!/bin/sh\n").unwrap();
        // The scan must require an executable bit (execvp semantics), so
        // make the fixture actually executable.
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();

        let joined = format!(
            "{}:{}",
            dir.path().display(),
            dir.path().join("nope").display()
        );
        assert!(systemd_inhibit_in_path(&joined));

        // Missing file → not available.
        assert!(!systemd_inhibit_in_path(
            &dir.path().join("nope").display().to_string()
        ));
        // Empty entries and an empty PATH string are tolerated.
        assert!(!systemd_inhibit_in_path(""));
        assert!(!systemd_inhibit_in_path("::"));
    }

    /// Fix round 1 — a non-executable `systemd-inhibit` on PATH must not
    /// count as available (the spawn would just fail at runtime).
    #[test]
    fn path_scan_rejects_non_executable_binary() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("systemd-inhibit");
        std::fs::write(&fake, b"#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o644)).unwrap();

        assert!(!systemd_inhibit_in_path(&dir.path().display().to_string()));
    }

    /// Fix round 1 — spawning goes through the `pre_exec` pdeathsig hook:
    /// if `prctl(PR_SET_PDEATHSIG)` were to fail, `spawn()` would return
    /// Err and no child would be stored. This proves the hook succeeds on
    /// the current kernel. (The kill-on-parent-death behaviour itself can
    /// only be observed by killing this process, which a test cannot do.)
    #[test]
    fn acquire_spawns_child_through_pdeathsig_hook() {
        let _guard = lock();
        set_override("/bin/sleep", &["30"]);
        reset_state();

        acquire();
        let pid = stored_child_pid().expect("pre_exec hook must not fail the spawn");
        assert!(process_alive(pid), "child must be alive right after spawn");

        clear_override();
        reset_state();
    }
}
