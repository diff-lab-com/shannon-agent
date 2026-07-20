//! Gateway process supervisor (E-1, 方案 C).
//!
//! When the desktop config's `gateway.managed` is true, this spawns the
//! `shannon-gateway` binary as a child process and supervises it. A supervisor
//! task owns the child and `select!`s between an explicit shutdown signal and
//! the child's natural exit. On any exit it updates the shared status and
//! emits a `shannon:gateway-exited` Tauri event so the UI can surface a toast.
//!
//! 方案 C contract: `managed=true` → desktop owns the lifecycle (spawn / kill /
//! restart). `managed=false` → the gateway is external (user / ops runs it);
//! the supervisor is never started and the UI's engine-endpoint fields point at
//! the out-of-process gateway.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::config::GatewayDesktopConfig;

/// Tauri event emitted when the supervised gateway process exits for any
/// reason (crash, clean exit, or explicit `stop()`). Payload: [`ExitedPayload`].
pub const GATEWAY_EXITED_EVENT: &str = "shannon:gateway-exited";

/// Snapshot of the supervisor + child state, surfaced to the UI.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum GatewaySupervisorStatus {
    /// Nothing running.
    Stopped,
    /// No gateway binary could be resolved — the UI shows an
    /// install/configure hint rather than an error.
    NotInstalled,
    /// A child was spawned and is alive (as far as we know).
    Running { pid: u32 },
    /// The child exited on its own; carried detail lets the UI explain why.
    Exited { code: Option<i32>, reason: String },
    /// A user-level OS service (systemd --user / launchd / schtasks) is
    /// already running the gateway. The supervisor does not own a child
    /// process; the UI should disable Start/Stop and surface a "managed
    /// externally" indicator. The supervisor cannot stop this service
    /// — `stop()` is a no-op when the status is `ManagedExternally`.
    ManagedExternally { service_name: String },
}

/// What a `gateway_supervisor_*` command returns — the managed flag + the
/// process status in one round-trip.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayProcessState {
    pub managed: bool,
    pub status: GatewaySupervisorStatus,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExitedPayload {
    reason: String,
    code: Option<i32>,
}

/// Resolve the gateway binary path. Precedence: explicit `binary_path` →
/// Tauri resource dir → `$PATH`. Returns the first existing match, or `None`
/// (caller reports [`GatewaySupervisorStatus::NotInstalled`]). Pure w.r.t. the
/// filesystem (no spawning), so it's unit-testable.
fn resolve_binary(explicit: Option<&str>, resource_dir: Option<&Path>) -> Option<PathBuf> {
    if let Some(p) = explicit {
        let pb = PathBuf::from(p);
        if pb.is_file() {
            return Some(pb);
        }
    }
    if let Some(dir) = resource_dir {
        for name in ["shannon-gateway", "shannon-gateway.exe"] {
            let cand = dir.join(name);
            if cand.is_file() {
                return Some(cand);
            }
        }
    }
    // Last resort: walk `$PATH`. Lets users `cargo install` / `npm i -g` the
    // gateway and have desktop pick it up with zero config.
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        for name in ["shannon-gateway", "shannon-gateway.exe"] {
            let cand = dir.join(name);
            if cand.is_file() {
                return Some(cand);
            }
        }
    }
    None
}

/// Owning handle for a supervised gateway process. Drop is *not* automatic —
/// callers must `stop().await` to kill the child (otherwise the supervisor task
/// keeps it alive for the app's lifetime, which is the intended default).
pub struct GatewaySupervisor {
    status: Arc<std::sync::RwLock<GatewaySupervisorStatus>>,
    cancel: CancellationToken,
    join: Option<JoinHandle<()>>,
}

impl GatewaySupervisor {
    /// Attempt to start the gateway under supervision.
    ///
    /// - Binary can't be resolved → `NotInstalled` (no task spawned, no error).
    /// - `spawn()` fails → `Exited { reason: "spawn failed: …" }`.
    /// - Otherwise → `Running { pid }` and a supervisor task is watching it.
    pub fn start<R: Runtime>(app: &AppHandle<R>, config: &GatewayDesktopConfig) -> Self {
        let status = Arc::new(std::sync::RwLock::new(GatewaySupervisorStatus::Stopped));
        let resource_dir = app.path().resource_dir().ok();
        let bin = match resolve_binary(config.binary_path.as_deref(), resource_dir.as_deref()) {
            Some(b) => b,
            None => {
                *status.write().expect("status lock poisoned") =
                    GatewaySupervisorStatus::NotInstalled;
                return Self {
                    status,
                    cancel: CancellationToken::new(),
                    join: None,
                };
            }
        };

        let mut cmd = tokio::process::Command::new(&bin);
        cmd.args(&config.extra_args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());

        let child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                *status.write().expect("status lock poisoned") = GatewaySupervisorStatus::Exited {
                    code: None,
                    reason: format!("spawn failed: {e}"),
                };
                return Self {
                    status,
                    cancel: CancellationToken::new(),
                    join: None,
                };
            }
        };
        let pid = child.id().unwrap_or(0);
        *status.write().expect("status lock poisoned") = GatewaySupervisorStatus::Running { pid };

        let cancel = CancellationToken::new();
        let join = tokio::spawn(supervise::<R>(
            app.clone(),
            child,
            cancel.clone(),
            status.clone(),
        ));
        Self {
            status,
            cancel,
            join: Some(join),
        }
    }

    /// Construct a supervisor that represents a gateway process owned by
    /// an external OS service. The supervisor holds no child pid and
    /// `stop()` is a no-op (the external service manager owns the
    /// lifecycle — stopping it requires `shannon gateway stop` or the
    /// platform equivalent).
    pub fn managed_externally(service_name: impl Into<String>) -> Self {
        let status = Arc::new(std::sync::RwLock::new(
            GatewaySupervisorStatus::ManagedExternally {
                service_name: service_name.into(),
            },
        ));
        Self {
            status,
            cancel: CancellationToken::new(),
            join: None,
        }
    }

    /// Signal the supervisor to kill + reap the child. Idempotent.
    pub async fn stop(&mut self) {
        // External OS service owns the gateway; the supervisor cannot
        // stop it. Treat stop() as a no-op (the UI button is also
        // disabled in ManagedExternally state, so this is a defensive
        // guard).
        if matches!(
            self.status(),
            GatewaySupervisorStatus::ManagedExternally { .. }
        ) {
            return;
        }
        self.cancel.cancel();
        if let Some(h) = self.join.take() {
            // Bound the wait so a misbehaving kill can't hang the UI action.
            let _ = tokio::time::timeout(std::time::Duration::from_secs(3), h).await;
        }
    }

    pub fn status(&self) -> GatewaySupervisorStatus {
        self.status.read().expect("status lock poisoned").clone()
    }
}

async fn supervise<R: Runtime>(
    app: AppHandle<R>,
    mut child: tokio::process::Child,
    cancel: CancellationToken,
    status: Arc<std::sync::RwLock<GatewaySupervisorStatus>>,
) {
    tokio::select! {
        _ = cancel.cancelled() => {
            let _ = child.kill().await;
            let code = child.wait().await.ok().and_then(|s| s.code());
            *status.write().expect("status lock poisoned") = GatewaySupervisorStatus::Stopped;
            let _ = app.emit(
                GATEWAY_EXITED_EVENT,
                ExitedPayload { reason: "stopped".into(), code },
            );
        }
        res = child.wait() => {
            let (code, reason) = match res {
                Ok(s) => (s.code(), if s.success() { "exited".to_string() } else { format!("exit code {:?}", s.code()) }),
                Err(e) => (None, format!("wait error: {e}")),
            };
            *status.write().expect("status lock poisoned") =
                GatewaySupervisorStatus::Exited { code, reason: reason.clone() };
            let _ = app.emit(
                GATEWAY_EXITED_EVENT,
                ExitedPayload { reason, code },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_path_wins_when_it_exists() {
        let dir = tempfile::tempdir().expect("tempdir");
        let explicit = dir.path().join("custom-gw");
        std::fs::write(&explicit, b"#!/bin/sh\n").expect("write");
        let got = resolve_binary(Some(explicit.to_str().unwrap()), None);
        assert_eq!(got, Some(explicit));
    }

    #[test]
    fn falls_back_to_resource_dir() {
        let res = tempfile::tempdir().expect("tempdir");
        let bin = res.path().join("shannon-gateway");
        std::fs::write(&bin, b"#!/bin/sh\n").expect("write");
        let got = resolve_binary(None, Some(res.path()));
        assert_eq!(got, Some(bin));
    }

    #[test]
    fn missing_explicit_falls_through_to_resource_dir() {
        let res = tempfile::tempdir().expect("tempdir");
        let bin = res.path().join("shannon-gateway");
        std::fs::write(&bin, b"#!/bin/sh\n").expect("write");
        let got = resolve_binary(Some("/does/not/exist"), Some(res.path()));
        assert_eq!(got, Some(bin));
    }

    #[test]
    fn returns_none_when_nothing_resolves() {
        // No explicit path, a resource dir without the binary, and rely on the
        // test runner's $PATH not containing a shannon-gateway.
        let res = tempfile::tempdir().expect("tempdir");
        let got = resolve_binary(None, Some(res.path()));
        // Only assert None if $PATH genuinely lacks it; skip otherwise.
        if std::env::var_os("PATH").is_some() && which("shannon-gateway").is_some() {
            return; // someone has it installed; can't assert None.
        }
        assert_eq!(got, None);
    }

    fn which(name: &str) -> Option<PathBuf> {
        let path = std::env::var_os("PATH")?;
        for dir in std::env::split_paths(&path) {
            let cand = dir.join(name);
            if cand.is_file() {
                return Some(cand);
            }
        }
        None
    }

    #[test]
    fn status_serializes_camel_case() {
        let s = GatewaySupervisorStatus::Running { pid: 4242 };
        let j = serde_json::to_string(&s).expect("serialize");
        assert!(j.contains("\"pid\":4242"));
        let s2 = GatewaySupervisorStatus::Exited {
            code: Some(1),
            reason: "boom".into(),
        };
        let j2 = serde_json::to_string(&s2).expect("serialize");
        assert!(j2.contains("\"code\":1"));
        assert!(j2.contains("\"reason\":\"boom\""));
        let s3 = GatewaySupervisorStatus::ManagedExternally {
            service_name: "shannon-gateway.service".into(),
        };
        let j3 = serde_json::to_string(&s3).expect("serialize");
        // Variant tag camelCases ("managedExternally"); inner field name
        // remains snake_case under serde's default enum-level rename_all
        // (matches the existing Running { pid } / Exited { code, reason }
        // patterns in this enum).
        assert!(j3.contains("\"managedExternally\""));
        assert!(j3.contains("\"service_name\":\"shannon-gateway.service\""));
        assert!(!j3.contains("\"managed_externally\""));
    }
}

/// End-to-end supervisor smoke against the **real** `shannon-gateway` binary.
///
/// Spawns the binary built at `../shannon-gateway/dist/shannon-gateway` under a
/// `tauri::test::mock_app()` handle, asserts the supervisor reaches `Running`
/// with a live pid, then `stop()`s and asserts the child is reaped (`Stopped`).
/// Skips gracefully when the sibling binary isn't built (CI without the
/// shannon-gateway checkout) so this never breaks the gate there.
#[cfg(test)]
mod e2e_tests {
    use super::*;
    use tauri::test::mock_app;

    /// Resolve the sibling gateway binary relative to this crate's manifest dir.
    fn gateway_binary() -> Option<PathBuf> {
        let candidate = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../shannon-gateway/dist/shannon-gateway");
        if candidate.is_file() {
            Some(candidate)
        } else {
            None
        }
    }

    #[tokio::test]
    async fn start_supervises_and_stop_reaps_real_binary() {
        let bin = match gateway_binary() {
            Some(b) => b,
            None => {
                eprintln!(
                    "skipping E2E: ../shannon-gateway/dist/shannon-gateway not built \
                     (run `pnpm build:binary` in shannon-gateway)"
                );
                return;
            }
        };

        // Loopback config with zero adapters — the gateway boots to its "up"
        // line and idles, which is exactly what we want the supervisor to keep
        // alive. The engine URL points at nothing; the gateway logs connection
        // retries but does not exit.
        let cfg_dir = tempfile::tempdir().expect("tempdir");
        let cfg_path = cfg_dir.path().join("gw.json");
        std::fs::write(
            &cfg_path,
            r#"{"engine":{"wsUrl":"ws://127.0.0.1:9999/ws","httpBaseUrl":"http://127.0.0.1:9999"},"adapters":[]}"#,
        )
        .expect("write config");

        let app = mock_app();
        let handle = app.handle().clone();
        let config = GatewayDesktopConfig {
            managed: true,
            binary_path: Some(bin.to_string_lossy().into_owned()),
            extra_args: vec!["--config".into(), cfg_path.to_string_lossy().into_owned()],
        };

        let mut sup = GatewaySupervisor::start(&handle, &config);
        let pid = match sup.status() {
            GatewaySupervisorStatus::Running { pid } => pid,
            other => panic!("expected Running after start, got {other:?}"),
        };
        assert!(pid > 0, "supervisor reported a non-positive pid");

        // The child must actually exist in the process table.
        assert!(
            proc_is_alive(pid),
            "pid {pid} is not a live process after start"
        );

        sup.stop().await;

        // stop() cancels + reaps; status flips to Stopped and the pid is gone.
        assert_eq!(sup.status(), GatewaySupervisorStatus::Stopped);
        // Give the OS a beat to reap, then confirm the process is gone.
        for _ in 0..20 {
            if !proc_is_alive(pid) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(
            !proc_is_alive(pid),
            "pid {pid} still alive after stop() — supervisor did not kill the child"
        );
    }

    /// `kill(pid, 0)` returns true iff the process exists (and we may signal it).
    fn proc_is_alive(pid: u32) -> bool {
        // Safety: kill with signal 0 is a standard POSIX existence check; it
        // performs no action other than error-checking.
        unsafe { libc::kill(pid as i32, 0) == 0 }
    }
}
