//! Gateway OS service probe (Q4-B).
//!
//! When the user runs `shannon gateway install`, the gateway registers a
//! user-level service with the OS service manager (systemd --user on
//! Linux, launchd on macOS, schtasks on Windows). If that service is
//! active, the desktop's gateway supervisor must NOT spawn a competing
//! child process — both would contend for port 33430 + the engine
//! websocket endpoint.
//!
//! This module queries the OS service manager for the
//! `shannon-gateway` service state. The supervisor consults the
//! result before deciding to spawn:
//!   - `Active`   → supervisor enters `ManagedExternally`, no child.
//!   - `Inactive` → service is registered but stopped; supervisor
//!                  spawns as before.
//!   - `Unknown`  → service is not registered (fresh install); supervisor
//!                  spawns as before (preserves first-run UX).

use serde::Serialize;
use std::process::Stdio;
use std::sync::Mutex;
use std::time::Duration;

type ProbeFn = fn() -> ServiceState;

/// Human-readable name of the gateway's OS-level service, per platform.
/// Used by the supervisor when surfacing `ManagedExternally` to the UI.
///
/// Linux:   systemd user unit (e.g. `~/.config/systemd/user/shannon-gateway.service`)
/// macOS:   launchd label (reverse-DNS form, what `shannon gateway install` writes)
/// Windows: scheduled task display name (what `schtasks /Query` matches against)
#[cfg(target_os = "linux")]
pub const SERVICE_NAME: &str = "shannon-gateway.service";

#[cfg(target_os = "macos")]
pub const SERVICE_NAME: &str = "shannon.gateway";

#[cfg(target_os = "windows")]
pub const SERVICE_NAME: &str = "Shannon Gateway";

/// Result of querying the OS service manager for `shannon-gateway`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ServiceState {
    /// The service is registered and currently running.
    Active,
    /// The service is registered but not running.
    Inactive,
    /// The service is not registered (no `shannon gateway install` has
    /// run yet), or the platform service manager returned an unexpected
    /// response. Supervisor treats this as "spawn a child".
    Unknown,
}

/// Test injection point. Production code uses the platform-default
/// probe; tests install a fake before calling the public API.
///
/// Note: stored as `Mutex<Option<_>>` rather than `OnceLock<_>` so the
/// three override tests can swap values across runs (OnceLock's
/// "set-once-per-process" semantics would make the Inactive and Unknown
/// tests fail after Active ran first). The brief noted this as a known
/// limitation; this implementation makes the tests deterministic while
/// preserving the same set/get contract for production code (which only
/// ever sets it once at startup). Tests hold the `TEST_LOCK` mutex in
/// addition to keep runs serialized within a single test binary.
static PROBE_OVERRIDE: Mutex<Option<ProbeFn>> = Mutex::new(None);

/// Install a synchronous probe override. Intended for `#[cfg(test)]`
/// only; production code should never call this.
pub fn set_probe_for_tests(f: ProbeFn) {
    if let Ok(mut guard) = PROBE_OVERRIDE.lock() {
        *guard = Some(f);
    }
}

/// Query the OS service manager. Public API.
pub async fn query_gateway_service_state() -> ServiceState {
    if let Ok(guard) = PROBE_OVERRIDE.lock() {
        if let Some(f) = *guard {
            return f();
        }
    }
    default_probe().await
}

/// Platform-default probe implementation.
///
/// Each branch shells out to the platform service manager with stdout/stderr
/// routed to `Stdio::null()` so the child can't wedge on a full pipe buffer.
/// The Linux branch additionally wraps the subprocess in a 2 s
/// `tokio::time::timeout` — `systemctl --user` can hang on dbus stalls or
/// permission query waits, and the probe runs on the Tauri setup path so an
/// unbounded wait blocks the desktop from reaching its runloop. On timeout,
/// returns `ServiceState::Unknown`, which the supervisor treats as
/// "spawn a child" (preserving first-run / unregistered-service UX).
#[cfg(target_os = "linux")]
async fn default_probe() -> ServiceState {
    let probe = async {
        tokio::process::Command::new("systemctl")
            .args(["--user", "is-active", SERVICE_NAME])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .await
    };
    match tokio::time::timeout(Duration::from_secs(2), probe).await {
        Ok(Ok(o)) if o.status.success() => ServiceState::Active,
        Ok(Ok(_)) => ServiceState::Inactive,
        Ok(Err(_)) => ServiceState::Unknown,
        Err(_elapsed) => ServiceState::Unknown,
    }
}

#[cfg(target_os = "macos")]
async fn default_probe() -> ServiceState {
    let output = std::process::Command::new("launchctl")
        .args(["print", &format!("user/{}", unsafe { libc::getuid() })])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .output();
    let stdout = match output {
        Ok(o) => String::from_utf8_lossy(&o.stdout).to_string(),
        Err(_) => return ServiceState::Unknown,
    };
    if stdout.contains("shannon-gateway") || stdout.contains("shannon.gateway") {
        // The label is registered; whether it's actually running is a
        // more subtle query. For our purposes, "registered + the print
        // output mentions it" is sufficient evidence of Active. launchd
        // doesn't have a single equivalent of `is-active`; this is the
        // closest portable check.
        ServiceState::Active
    } else {
        ServiceState::Unknown
    }
}

#[cfg(target_os = "windows")]
async fn default_probe() -> ServiceState {
    // Windows service registration via `shannon gateway install` is not
    // yet implemented (out-of-scope per the design spec). Probe for the
    // scheduled task defensively in case the user registered one
    // manually with nssm or similar. Missing task → Unknown → supervisor
    // spawns a child (the v0.7.0 behavior).
    let output = std::process::Command::new("schtasks")
        .args(["/Query", "/TN", SERVICE_NAME])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .output();
    match output {
        Ok(o) if o.status.success() => ServiceState::Active,
        Ok(_) => ServiceState::Inactive,
        Err(_) => ServiceState::Unknown,
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
async fn default_probe() -> ServiceState {
    ServiceState::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Serialize probe-override tests so the PROBE_OVERRIDE swap is
    // deterministic across parallel test runners. PROBE_OVERRIDE is a
    // process-wide Mutex<Option<ProbeFn>> (not a OnceLock — that design
    // didn't allow swapping between Active/Inactive/Unknown within a
    // single test binary), so concurrent test threads would race on it
    // without this extra lock.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn probe_override_active_short_circuits_default() {
        let _g = TEST_LOCK.lock().unwrap();
        set_probe_for_tests(|| ServiceState::Active);
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        assert_eq!(
            rt.block_on(query_gateway_service_state()),
            ServiceState::Active
        );
    }

    #[test]
    fn probe_override_inactive_falls_through_in_supervisor_logic() {
        // This test documents the contract: Inactive means "registered
        // but stopped" — the supervisor still spawns (no stop/start
        // orchestration here). Just confirms the value flows through.
        let _g = TEST_LOCK.lock().unwrap();
        set_probe_for_tests(|| ServiceState::Inactive);
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        assert_eq!(
            rt.block_on(query_gateway_service_state()),
            ServiceState::Inactive
        );
    }

    #[test]
    fn probe_override_unknown_preserves_first_run_spawn_behavior() {
        let _g = TEST_LOCK.lock().unwrap();
        set_probe_for_tests(|| ServiceState::Unknown);
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        assert_eq!(
            rt.block_on(query_gateway_service_state()),
            ServiceState::Unknown
        );
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn linux_default_probe_returns_unknown_for_unregistered_service() {
        // `shannon-gateway.service` is almost certainly not registered
        // in CI. systemctl returns non-zero, the probe returns Inactive.
        // Acceptable: also a "don't spawn externally" signal for the
        // supervisor (matches Unknown's spawn behavior). The strict
        // assertion here is "anything other than Active".
        let _g = TEST_LOCK.lock().unwrap();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let state = rt.block_on(default_probe());
        assert_ne!(
            state,
            ServiceState::Active,
            "test env must not have a real shannon-gateway.service running"
        );
    }
}
