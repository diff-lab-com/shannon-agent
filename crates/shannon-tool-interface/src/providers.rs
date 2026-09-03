//! Execution-world provider seams (§4.11 W3-3a).
//!
//! Tools talk to the outside world exclusively through
//! [`FileSystemProvider`] and [`ProcessProvider`] trait objects instead of
//! calling `std::fs` / `tokio::fs` / process-spawn APIs directly. The local
//! execution world is one implementation (`LocalFs` / `LocalProcess`, hosted
//! in `shannon-core::providers`); sandboxes (§4.12) and future remote
//! execution worlds are alternative implementations of the same traits,
//! injectable at tool-registry assembly time.
//!
//! ## Design notes
//!
//! - **Read/write/edit/list**: `edit` is a *tool-level* composition of
//!   read + write (atomic temp-file rename), not a provider primitive, so it
//!   lives with the tools. There is deliberately **no `watch` method**: no
//!   shipped tool currently watches filesystems, and inventing an unused
//!   capability would violate YAGNI. If a watcher tool appears, add `watch`
//!   here.
//! - **Sync + async faces**: existing tools use both blocking (`std`)
//!   and async (`tokio`) I/O flavors. The provider exposes paired methods so
//!   migration preserves call-site semantics exactly — no forced
//!   `spawn_blocking` bridges, no buffering changes.
//! - **Sandbox seam**: [`ProcessProvider::prepare_spawn`] is the explicit
//!   wrapping point. It is applied by every spawn/run entry point before OS
//!   fork; a §4.12 `SandboxedProcess` decorator rewrites requests there
//!   (argv prefixing today, Landlock-backed worlds later) without any
//!   tool-code changes.

use async_trait::async_trait;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Metadata snapshot mirroring the fields tools actually consume from
/// `std::fs::Metadata` / `tokio::fs::Metadata`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileMeta {
    /// Size in bytes (`Metadata::len`).
    pub len: u64,
    /// Whether the path is a directory (`Metadata::is_dir`).
    pub is_dir: bool,
    /// Last modification time when available (`Metadata::modified`).
    pub modified: Option<SystemTime>,
}

/// Directory entry projection used by list operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntryInfo {
    /// Full path of the entry (`DirEntry::path`).
    pub path: PathBuf,
    /// File length in bytes.
    pub len: u64,
    /// Whether the entry is a directory.
    pub is_dir: bool,
}

// ---------------------------------------------------------------------------
// FileSystemProvider
// ---------------------------------------------------------------------------

/// Pluggable filesystem execution world.
///
/// Implementations: [`LocalFs`](https://docs.rs/shannon-core) (local disk,
/// default) plus test doubles. Sandboxed/remote filesystems implement this
/// trait and get injected at assembly time — tool code never learns which
/// world it runs against.
///
/// Method families mirror the exact operations the shipped tools perform;
/// "blocking" variants exist because several tools run synchronous bodies
/// and switching them to async I/O would change scheduling semantics.
#[async_trait]
pub trait FileSystemProvider: Send + Sync + 'static {
    // ---- async face (tokio world) --------------------------------------

    /// Async `read_to_string`.
    async fn read_text(&self, path: &Path) -> io::Result<String>;
    /// Async whole-file byte read (`tokio::fs::read`).
    async fn read_bytes(&self, path: &Path) -> io::Result<Vec<u8>>;
    /// Async stat.
    async fn metadata(&self, path: &Path) -> io::Result<FileMeta>;
    /// Async recursive directory creation.
    async fn create_dir_all(&self, path: &Path) -> io::Result<()>;
    /// Async write of raw bytes (`tokio::fs::write`).
    async fn write_bytes(&self, path: &Path, contents: &[u8]) -> io::Result<()>;
    /// Async atomic same-filesystem rename (temp-file commit step).
    async fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;
    /// Async symlink-resolving canonicalization (TOCTOU-safe path checks).
    async fn canonicalize(&self, path: &Path) -> io::Result<PathBuf>;

    // ---- blocking face (std world) -------------------------------------

    /// Blocking `read_to_string`.
    fn read_text_blocking(&self, path: &Path) -> io::Result<String>;
    /// Blocking write of raw bytes.
    fn write_bytes_blocking(&self, path: &Path, contents: &[u8]) -> io::Result<()>;
    /// Blocking recursive directory creation.
    fn create_dir_all_blocking(&self, path: &Path) -> io::Result<()>;
    /// Blocking single-file removal (temp-file cleanup, snapshot pruning).
    fn remove_file_blocking(&self, path: &Path) -> io::Result<()>;
    /// Blocking symlink-resolving canonicalization.
    fn canonicalize_blocking(&self, path: &Path) -> io::Result<PathBuf>;
    /// Blocking stat.
    fn metadata_blocking(&self, path: &Path) -> io::Result<FileMeta>;
    /// Read up to `max_bytes` bytes from the start of a file (binary sniffing).
    fn read_prefix_blocking(&self, path: &Path, max_bytes: usize) -> io::Result<Vec<u8>>;
    /// List direct children of a directory.
    fn list_dir_blocking(&self, path: &Path) -> io::Result<Vec<DirEntryInfo>>;

    /// Existence probe preserving `Path::exists()` semantics.
    fn exists_blocking(&self, path: &Path) -> bool {
        self.metadata_blocking(path).is_ok()
    }

    /// Depth-first recursive walk used by Grep/Glob (and any future
    /// traversal consumer). Invokes `cb` for the root and every entry;
    /// returning `false` prunes a directory's subtree.
    ///
    /// Implementations whose store has no native walker use
    /// [`crate::walk::provider_walk`] with their three blocking primitives;
    /// `LocalFs` overrides it with an `ignore::WalkBuilder`-backed version
    /// that preserves native local traversal semantics.
    fn walk_blocking(
        &self,
        root: &Path,
        cb: &mut dyn FnMut(&DirEntryInfo) -> bool,
    ) -> io::Result<()>;
}

// ---------------------------------------------------------------------------
// ProcessProvider
// ---------------------------------------------------------------------------

/// One process invocation request.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProcessRequest {
    /// Executable name or absolute path.
    pub program: String,
    /// Argument vector (no shell interpolation is implied anywhere).
    pub args: Vec<String>,
    /// Working directory override; `None` inherits the current one.
    pub cwd: Option<PathBuf>,
    /// Additional environment variables layered on top of inherited env.
    pub env: Vec<(String, String)>,
    /// Bytes written to stdin; `None` leaves stdin closed/null depending on
    /// spawn flavor (captured runs close it immediately).
    pub stdin_data: Option<Vec<u8>>,
}

impl ProcessRequest {
    /// Convenience constructor for `program + args`.
    pub fn new(program: impl Into<String>, args: &[&str]) -> Self {
        Self {
            program: program.into(),
            args: args.iter().map(|a| (*a).to_string()).collect(),
            cwd: None,
            env: Vec::new(),
            stdin_data: None,
        }
    }
}

/// Exit outcome projected from `std::process::ExitStatus`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProcessExit {
    /// Numeric exit code (`ExitStatus::code`); `None` on signal death.
    pub code: Option<i32>,
    /// `ExitStatus::success()`.
    pub success: bool,
}

impl ProcessExit {
    /// Build an exit from a raw code (0 ⇒ success), mirroring Unix semantics.
    pub fn from_code(code: i32) -> Self {
        Self {
            code: Some(code),
            success: code == 0,
        }
    }
}

/// Captured output of a completed run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CapturedOutput {
    /// Raw stdout bytes.
    pub stdout: Vec<u8>,
    /// Raw stderr bytes.
    pub stderr: Vec<u8>,
    /// Exit status.
    pub exit: ProcessExit,
}

/// Specification for spawning a persistent child with piped streams
/// (JSON-RPC LSP sessions, line-streamed bash output).
#[derive(Debug, Clone, Default)]
pub struct PipedSpawn {
    /// The invocation itself.
    pub request: ProcessRequest,
    /// Pipe stdin (`Stdio::piped`) vs inherit/closed.
    pub pipe_stdin: bool,
    /// Pipe stdout.
    pub pipe_stdout: bool,
    /// Pipe stderr.
    pub pipe_stderr: bool,
    /// Kill the child if the handle is dropped (`kill_on_drop(true)`).
    pub kill_on_drop: bool,
}

/// Handle to a live child process with individually pipped streams.
///
/// Mirrors `tokio::process::Child` semantics for exactly the operations
/// tools exercise: taking each stream once, kill, and wait.
#[async_trait]
pub trait PipedChild: Send {
    /// Take ownership of the piped stdin writer (once).
    fn take_stdin(&mut self) -> Option<Box<dyn tokio::io::AsyncWrite + Send + Unpin>>;
    /// Take ownership of the piped stdout reader (once).
    fn take_stdout(&mut self) -> Option<Box<dyn tokio::io::AsyncRead + Send + Unpin>>;
    /// Take ownership of the piped stderr reader (once).
    fn take_stderr(&mut self) -> Option<Box<dyn tokio::io::AsyncRead + Send + Unpin>>;
    /// Terminate the child (`Child::kill`).
    async fn kill(&mut self);
    /// Reap the child and return its exit status (`Child::wait`).
    async fn wait(&mut self) -> io::Result<ProcessExit>;
}

/// Request rewriting hook invoked immediately before OS spawn.
///
/// This is the sandbox argv-wrapping point called out by master plan §4.11④:
/// `LocalProcess` installs a rewriter built over `SandboxExecutor`
/// (bwrap/Seatbelt/Docker argv prefixes); a §4.12 `SandboxedProcess`
/// decorator installs its policy rewrite (or simply wraps the whole
/// provider). Hooks compose via [`ChainedSpawnRewrite`].
pub trait SpawnRewrite: Send + Sync {
    /// Rewrite (or reject) a request right before the OS sees it.
    fn rewrite(&self, request: ProcessRequest) -> Result<ProcessRequest, String>;
}

/// Apply multiple rewrites in order; first error aborts the spawn.
pub struct ChainedSpawnRewrite {
    rewrites: Vec<std::sync::Arc<dyn SpawnRewrite>>,
}

impl ChainedSpawnRewrite {
    /// Chain rewrites left-to-right.
    pub fn new(rewrites: Vec<std::sync::Arc<dyn SpawnRewrite>>) -> Self {
        Self { rewrites }
    }
}

impl SpawnRewrite for ChainedSpawnRewrite {
    fn rewrite(&self, mut request: ProcessRequest) -> Result<ProcessRequest, String> {
        for r in &self.rewrites {
            request = r.rewrite(request)?;
        }
        Ok(request)
    }
}

/// Capability flags describing how a process world executes requests.
///
/// Worlds route through the same trait, but some call sites need to know
/// whether execution leaves the local machine — the bash tool gates its
/// local-PTY and argv-sandbox branches on [`ExecCaps::is_remote`] so remote
/// sessions never fall back to local-only paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ExecCaps {
    /// Commands run on a different host/container than the one Shannon
    /// itself runs on.
    pub is_remote: bool,
}

/// Pluggable process execution world.
///
/// Consumers never build `std::process::Command` / `tokio::process::Command`
/// themselves; they describe intent as [`ProcessRequest`] and let the
/// provider decide how the OS gets involved.
#[async_trait]
pub trait ProcessProvider: Send + Sync + 'static {
    /// Blocking captured run (git helpers and other sync call sites).
    fn run_blocking(&self, request: &ProcessRequest) -> io::Result<CapturedOutput>;

    /// Async captured run (github/lsp-diagnostics/bash-captured paths).
    async fn run_async(&self, request: &ProcessRequest) -> io::Result<CapturedOutput>;

    /// Spawn a long-lived child with piped streams.
    async fn spawn_piped(&self, spec: &PipedSpawn) -> io::Result<Box<dyn PipedChild>>;

    /// Sandbox/world-decoration seam applied by providers before spawn.
    ///
    /// The default implementation is identity; concrete providers apply their
    /// installed [`SpawnRewrite`] chain inside their own spawn paths, and
    /// decorators may additionally override this to make wrapping explicit.
    fn prepare_spawn(&self, request: ProcessRequest) -> Result<ProcessRequest, String> {
        Ok(request)
    }

    /// What this world can do (default: plain local execution).
    fn capabilities(&self) -> ExecCaps {
        ExecCaps { is_remote: false }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct AppendRewrite {
        arg: &'static str,
    }

    impl SpawnRewrite for AppendRewrite {
        fn rewrite(&self, mut request: ProcessRequest) -> Result<ProcessRequest, String> {
            request.args.push(self.arg.to_string());
            if request.program == "boom" {
                return Err("rejected".to_string());
            }
            Ok(request)
        }
    }

    #[test]
    fn process_request_new_builds_args() {
        let req = ProcessRequest::new("git", &["status", "--porcelain"]);
        assert_eq!(req.program, "git");
        assert_eq!(req.args, vec!["status", "--porcelain"]);
        assert_eq!(req.cwd, None);
        assert_eq!(req.env.len(), 0);
        assert_eq!(req.stdin_data, None);
    }

    #[test]
    fn chained_rewrite_composes_in_order() {
        let chain = ChainedSpawnRewrite::new(vec![
            std::sync::Arc::new(AppendRewrite { arg: "a" }),
            std::sync::Arc::new(AppendRewrite { arg: "b" }),
        ]);
        let req = chain.rewrite(ProcessRequest::new("x", &[])).expect("ok");
        assert_eq!(req.args, vec!["a", "b"]);
    }

    #[test]
    fn chained_rewrite_first_error_short_circuits() {
        let chain = ChainedSpawnRewrite::new(vec![
            std::sync::Arc::new(AppendRewrite { arg: "never" }),
            std::sync::Arc::new(AppendRewrite { arg: "never2" }),
        ]);
        let err = chain
            .rewrite(ProcessRequest::new("boom", &[]))
            .expect_err("should reject");
        assert_eq!(err, "rejected");
    }

    /// Minimal provider proving the trait is object-safe and that the
    /// default `prepare_spawn` is identity.
    struct NoopProvider;

    #[async_trait]
    impl ProcessProvider for NoopProvider {
        fn run_blocking(&self, _request: &ProcessRequest) -> io::Result<CapturedOutput> {
            Ok(CapturedOutput {
                exit: ProcessExit::from_code(0),
                ..CapturedOutput::default()
            })
        }

        async fn run_async(&self, _request: &ProcessRequest) -> io::Result<CapturedOutput> {
            self.run_blocking(_request)
        }

        async fn spawn_piped(&self, _spec: &PipedSpawn) -> io::Result<Box<dyn PipedChild>> {
            Err(io::Error::new(io::ErrorKind::Unsupported, "noop"))
        }
    }

    #[tokio::test]
    async fn noop_provider_runs_and_prepare_spawn_is_identity() {
        use std::sync::Arc;
        let provider: Arc<dyn ProcessProvider> = Arc::new(NoopProvider);
        let out = provider
            .run_async(&ProcessRequest::new("true", &[]))
            .await
            .expect("run should succeed");
        assert!(out.exit.success);

        let original = ProcessRequest::new("git", &["log"]);
        let prepared = provider
            .prepare_spawn(original.clone())
            .expect("identity by default");
        assert_eq!(prepared, original);
    }

    #[test]
    fn captured_output_default_is_failed_exit() {
        let out = CapturedOutput::default();
        assert_eq!(out.stdout.len(), 0);
        assert!(!out.exit.success);
        assert_eq!(out.exit.code, None);
    }
}
