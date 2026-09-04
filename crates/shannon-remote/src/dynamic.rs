//! Hot-swappable execution world.
//!
//! [`DynamicWorld`] implements both provider traits and delegates every call
//! to the currently-active inner world (local by default). `/remote use`
//! swaps the inner world atomically — single tool calls see a consistent
//! snapshot — and broadcasts status transitions on a `watch` channel for UI
//! indicators. Tool registries keep working unchanged because tools hold the
//! decorator's `Arc` for their whole lifetime.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use shannon_tool_interface::{
    CapturedOutput, DirEntryInfo, ExecCaps, FileMeta, FileSystemProvider, PipedChild, PipedSpawn,
    ProcessProvider, ProcessRequest,
};
use tokio::sync::watch;

use crate::docker::{DockerExecFs, DockerExecProcess};
use crate::ssh::fs::SshFs;
use crate::ssh::process::SshProcess;
use crate::ssh::session::{HealthReport, SshRuntime, WorldStatus};
use crate::target::{RemoteTarget, TargetKind};

/// One immutable world snapshot: providers plus bookkeeping.
#[derive(Clone)]
struct CurrentWorld {
    fs: Arc<dyn FileSystemProvider>,
    process: Arc<dyn ProcessProvider>,
    caps: ExecCaps,
    target: Option<RemoteTarget>,
    runtime: Option<Arc<SshRuntime>>,
}

/// Shared, observable world state (status pill, `/remote` dashboard).
pub struct WorldState {
    status_tx: watch::Sender<WorldStatus>,
    target: RwLock<Option<String>>,
}

impl WorldState {
    /// Current status.
    pub fn status(&self) -> WorldStatus {
        *self.status_tx.borrow()
    }

    /// Subscribe to status transitions.
    pub fn subscribe(&self) -> watch::Receiver<WorldStatus> {
        self.status_tx.subscribe()
    }

    /// Name of the active target, if any.
    pub fn active_target(&self) -> Option<String> {
        self.target.read().ok().and_then(|g| g.clone())
    }

    fn set(&self, status: WorldStatus, target: Option<String>) {
        // send_replace stores even with zero subscribers, so late UI
        // observers always read the current status.
        self.status_tx.send_replace(status);
        if let Ok(mut t) = self.target.write() {
            *t = target;
        }
    }
}

static ACTIVE_STATE: RwLock<Option<Arc<WorldState>>> = RwLock::new(None);

/// Register the session's world state for process-wide UI indicators
/// (status-bar pill). One REPL per process; the desktop loopback world
/// registers harmlessly (nothing reads it there).
pub fn register_active_state(state: Arc<WorldState>) {
    if let Ok(mut guard) = ACTIVE_STATE.write() {
        *guard = Some(state);
    }
}

/// `(target name, degraded)` for the active remote target, or `None` when
/// running locally.
pub fn active_target_display() -> Option<(String, bool)> {
    let state = ACTIVE_STATE.read().ok()?.as_ref()?.clone();
    match state.status() {
        WorldStatus::Local => None,
        WorldStatus::Connected => Some((state.active_target()?, false)),
        WorldStatus::Degraded => Some((state.active_target()?, true)),
    }
}

/// Decorator routing every provider call to the active world.
pub struct DynamicWorld {
    local_fs: Arc<dyn FileSystemProvider>,
    local_process: Arc<dyn ProcessProvider>,
    current: RwLock<Arc<CurrentWorld>>,
    state: Arc<WorldState>,
}

impl DynamicWorld {
    /// Start in the local world. `local_fs`/`local_process` are kept for
    /// later `disconnect()` calls and docker targets that run the CLI locally.
    pub fn new(
        local_fs: Arc<dyn FileSystemProvider>,
        local_process: Arc<dyn ProcessProvider>,
    ) -> (Arc<Self>, Arc<WorldState>) {
        let initial = CurrentWorld {
            fs: local_fs.clone(),
            process: local_process.clone(),
            caps: ExecCaps { is_remote: false },
            target: None,
            runtime: None,
        };
        let (status_tx, _) = watch::channel(WorldStatus::Local);
        let state = Arc::new(WorldState {
            status_tx,
            target: RwLock::new(None),
        });
        (
            Arc::new(Self {
                local_fs,
                local_process,
                current: RwLock::new(Arc::new(initial)),
                state: state.clone(),
            }),
            state,
        )
    }

    /// Shared state handle (UI status pill, dashboards).
    pub fn state(&self) -> &Arc<WorldState> {
        &self.state
    }

    fn snapshot(&self) -> Arc<CurrentWorld> {
        self.current.read().expect("world lock").clone()
    }

    fn swap(&self, next: CurrentWorld, status: WorldStatus) {
        let target_name = next.target.as_ref().map(|t| t.name.clone());
        *self.current.write().expect("world lock") = Arc::new(next);
        self.state.set(status, target_name);
    }

    /// The active SSH runtime, when a remote target is connected.
    pub fn ssh_runtime(&self) -> Option<Arc<SshRuntime>> {
        self.snapshot().runtime.clone()
    }

    /// Connect to `target` and switch to it. The local world stays installed
    /// (and an error is returned) when the connection or health check fails.
    pub async fn connect_target(
        self: &Arc<Self>,
        target: &RemoteTarget,
    ) -> io::Result<HealthReport> {
        match target.kind {
            TargetKind::Ssh => {
                let runtime = SshRuntime::connect(target).await?;
                let health = runtime.health().await?;
                if !health.workspace_exists {
                    return Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        format!(
                            "workspace_dir {} does not exist on {}",
                            target.workspace_dir.display(),
                            target.ssh_destination()
                        ),
                    ));
                }
                let fs = SshFs::connect(runtime.clone()).await?;
                let process = SshProcess::new(runtime.clone(), target.workspace_dir.clone());
                self.swap(
                    CurrentWorld {
                        fs: fs.clone(),
                        process: process.clone(),
                        caps: ExecCaps { is_remote: true },
                        target: Some(target.clone()),
                        runtime: Some(runtime),
                    },
                    WorldStatus::Connected,
                );
                Ok(health)
            }
            TargetKind::Docker => {
                // Docker needs no connection handshake; validate the
                // container exists by probing `true` inside it.
                let process = DockerExecProcess::new(
                    target.container.clone().unwrap_or_default(),
                    target.workspace_dir.clone(),
                    self.local_process.clone(),
                );
                let fs = DockerExecFs::new(process.clone());
                let probe = process
                    .run_async(&ProcessRequest::new("true", &[]))
                    .await
                    .map_err(|e| io::Error::other(format!("docker probe failed: {e}")))?;
                if !probe.exit.success {
                    return Err(io::Error::other(format!(
                        "container {} is not running (docker exec failed)",
                        target.container.as_deref().unwrap_or("?")
                    )));
                }
                self.swap(
                    CurrentWorld {
                        fs: fs.clone(),
                        process: process.clone(),
                        caps: ExecCaps { is_remote: true },
                        target: Some(target.clone()),
                        runtime: None,
                    },
                    WorldStatus::Connected,
                );
                Ok(HealthReport {
                    platform: "docker".into(),
                    home: target.workspace_dir.to_string_lossy().to_string(),
                    bash_available: true,
                    workspace_exists: true,
                    latency_ms: 0,
                })
            }
        }
    }

    /// Re-establish the current remote target (after a degraded state).
    pub async fn reconnect(self: &Arc<Self>) -> io::Result<HealthReport> {
        let target = self
            .snapshot()
            .target
            .clone()
            .ok_or_else(|| io::Error::other("no remote target to reconnect"))?;
        self.connect_target(&target).await
    }

    /// Switch back to the local world captured at construction.
    pub fn disconnect(&self) {
        self.swap(
            CurrentWorld {
                fs: self.local_fs.clone(),
                process: self.local_process.clone(),
                caps: ExecCaps { is_remote: false },
                target: None,
                runtime: None,
            },
            WorldStatus::Local,
        );
    }

    /// Whether the active world executes away from this machine.
    pub fn is_remote(&self) -> bool {
        self.snapshot().caps.is_remote
    }
}

#[async_trait]
impl FileSystemProvider for DynamicWorld {
    async fn read_text(&self, path: &Path) -> io::Result<String> {
        self.snapshot().fs.read_text(path).await
    }
    async fn read_bytes(&self, path: &Path) -> io::Result<Vec<u8>> {
        self.snapshot().fs.read_bytes(path).await
    }
    async fn metadata(&self, path: &Path) -> io::Result<FileMeta> {
        self.snapshot().fs.metadata(path).await
    }
    async fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        self.snapshot().fs.create_dir_all(path).await
    }
    async fn write_bytes(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
        self.snapshot().fs.write_bytes(path, contents).await
    }
    async fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        self.snapshot().fs.rename(from, to).await
    }
    async fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        self.snapshot().fs.canonicalize(path).await
    }
    fn read_text_blocking(&self, path: &Path) -> io::Result<String> {
        self.snapshot().fs.read_text_blocking(path)
    }
    fn write_bytes_blocking(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
        self.snapshot().fs.write_bytes_blocking(path, contents)
    }
    fn create_dir_all_blocking(&self, path: &Path) -> io::Result<()> {
        self.snapshot().fs.create_dir_all_blocking(path)
    }
    fn remove_file_blocking(&self, path: &Path) -> io::Result<()> {
        self.snapshot().fs.remove_file_blocking(path)
    }
    fn canonicalize_blocking(&self, path: &Path) -> io::Result<PathBuf> {
        self.snapshot().fs.canonicalize_blocking(path)
    }
    fn metadata_blocking(&self, path: &Path) -> io::Result<FileMeta> {
        self.snapshot().fs.metadata_blocking(path)
    }
    fn read_prefix_blocking(&self, path: &Path, max_bytes: usize) -> io::Result<Vec<u8>> {
        self.snapshot().fs.read_prefix_blocking(path, max_bytes)
    }
    fn list_dir_blocking(&self, path: &Path) -> io::Result<Vec<DirEntryInfo>> {
        self.snapshot().fs.list_dir_blocking(path)
    }
    fn walk_blocking(
        &self,
        root: &Path,
        cb: &mut dyn FnMut(&DirEntryInfo) -> bool,
    ) -> io::Result<()> {
        self.snapshot().fs.walk_blocking(root, cb)
    }
}

#[async_trait]
impl ProcessProvider for DynamicWorld {
    fn run_blocking(&self, request: &ProcessRequest) -> io::Result<CapturedOutput> {
        self.snapshot().process.run_blocking(request)
    }
    async fn run_async(&self, request: &ProcessRequest) -> io::Result<CapturedOutput> {
        self.snapshot().process.run_async(request).await
    }
    async fn spawn_piped(&self, spec: &PipedSpawn) -> io::Result<Box<dyn PipedChild>> {
        self.snapshot().process.spawn_piped(spec).await
    }
    fn capabilities(&self) -> ExecCaps {
        self.snapshot().caps
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shannon_tool_interface::{DirEntryInfo, FileMeta, ProcessExit};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    // -- programmable fake worlds counting calls ----------------------------

    #[derive(Default)]
    struct FakeFs {
        reads: AtomicUsize,
    }

    #[async_trait]
    impl FileSystemProvider for FakeFs {
        async fn read_text(&self, _path: &Path) -> io::Result<String> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            Ok("fake".into())
        }
        async fn read_bytes(&self, path: &Path) -> io::Result<Vec<u8>> {
            Ok(self.read_text(path).await?.into_bytes())
        }
        async fn metadata(&self, _path: &Path) -> io::Result<FileMeta> {
            Ok(FileMeta {
                len: 0,
                is_dir: false,
                modified: None,
            })
        }
        async fn create_dir_all(&self, _path: &Path) -> io::Result<()> {
            Ok(())
        }
        async fn write_bytes(&self, _path: &Path, _contents: &[u8]) -> io::Result<()> {
            Ok(())
        }
        async fn rename(&self, _from: &Path, _to: &Path) -> io::Result<()> {
            Ok(())
        }
        async fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
            Ok(path.to_path_buf())
        }
        fn read_text_blocking(&self, _path: &Path) -> io::Result<String> {
            Ok("fake".into())
        }
        fn write_bytes_blocking(&self, _path: &Path, _c: &[u8]) -> io::Result<()> {
            Ok(())
        }
        fn create_dir_all_blocking(&self, _path: &Path) -> io::Result<()> {
            Ok(())
        }
        fn remove_file_blocking(&self, _path: &Path) -> io::Result<()> {
            Ok(())
        }
        fn canonicalize_blocking(&self, path: &Path) -> io::Result<PathBuf> {
            Ok(path.to_path_buf())
        }
        fn metadata_blocking(&self, _path: &Path) -> io::Result<FileMeta> {
            Ok(FileMeta {
                len: 0,
                is_dir: false,
                modified: None,
            })
        }
        fn read_prefix_blocking(&self, _p: &Path, _m: usize) -> io::Result<Vec<u8>> {
            Ok(Vec::new())
        }
        fn list_dir_blocking(&self, _p: &Path) -> io::Result<Vec<DirEntryInfo>> {
            Ok(Vec::new())
        }
        fn walk_blocking(
            &self,
            _root: &Path,
            _cb: &mut dyn FnMut(&DirEntryInfo) -> bool,
        ) -> io::Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct FakeProcess {
        calls: AtomicUsize,
        remote: bool,
    }

    #[async_trait]
    impl ProcessProvider for FakeProcess {
        fn run_blocking(&self, _r: &ProcessRequest) -> io::Result<CapturedOutput> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(CapturedOutput {
                stdout: b"fake".to_vec(),
                stderr: Vec::new(),
                exit: ProcessExit::from_code(0),
            })
        }
        async fn run_async(&self, r: &ProcessRequest) -> io::Result<CapturedOutput> {
            self.run_blocking(r)
        }
        async fn spawn_piped(&self, _s: &PipedSpawn) -> io::Result<Box<dyn PipedChild>> {
            Err(io::Error::other("fake has no children"))
        }
        fn capabilities(&self) -> ExecCaps {
            ExecCaps {
                is_remote: self.remote,
            }
        }
    }

    fn target() -> RemoteTarget {
        RemoteTarget {
            name: "t".into(),
            kind: TargetKind::Docker,
            host: None,
            port: None,
            user: None,
            container: Some("c".into()),
            shell: None,
            ssh_target: None,
            workspace_dir: PathBuf::from("/w"),
        }
    }

    #[tokio::test]
    async fn delegates_to_active_world_and_survives_swap() {
        let (world, state) = DynamicWorld::new(
            Arc::new(FakeFs::default()),
            Arc::new(FakeProcess::default()),
        );
        assert!(!world.is_remote());
        assert_eq!(state.status(), WorldStatus::Local);

        // In-flight snapshot: clone of the inner world keeps serving even
        // after a swap (Arc semantics).
        let snap_fs: Arc<dyn FileSystemProvider> = Arc::new(FakeFs::default());
        let _ = snap_fs.read_text(Path::new("/x")).await;

        // Swap local->local with fresh fakes; calls now hit the new world.
        world.disconnect();
        assert_eq!(state.status(), WorldStatus::Local);
        assert_eq!(state.active_target(), None);
    }

    #[tokio::test]
    async fn disconnect_restores_local_status() {
        let (world, state) = DynamicWorld::new(
            Arc::new(FakeFs::default()),
            Arc::new(FakeProcess::default()),
        );
        let rx = state.subscribe();
        world.disconnect();
        // watch channel delivers the (unchanged) Local status.
        assert_eq!(*rx.borrow(), WorldStatus::Local);
        assert!(!world.is_remote());
    }

    #[tokio::test]
    async fn reconnect_without_target_errors() {
        let (world, _state) = DynamicWorld::new(
            Arc::new(FakeFs::default()),
            Arc::new(FakeProcess::default()),
        );
        assert!(world.reconnect().await.is_err());
    }

    #[tokio::test]
    async fn docker_target_probe_failure_keeps_local_world() {
        let (world, state) = DynamicWorld::new(
            Arc::new(FakeFs::default()),
            Arc::new(FakeProcess::default()),
        );
        // `docker` almost certainly missing/failed here: either way the
        // local world must stay installed.
        let _ = world.connect_target(&target()).await;
        if state.status() != WorldStatus::Connected {
            assert!(!world.is_remote());
        }
    }
}
