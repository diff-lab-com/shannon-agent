//! Local execution-world providers (§4.11 W3-3a).
//!
//! [`LocalFs`] and [`LocalProcess`] are the concrete
//! `FileSystemProvider`/`ProcessProvider` implementations backing the default
//! tool assembly. They are thin, zero-copy forwards onto `std::fs`,
//! `tokio::fs`, and process-spawn APIs — every other crate (notably
//! `shannon-tools`) reaches the filesystem and child processes only through
//! the provider traits, so a sandboxed or remote execution world can be
//! swapped in at assembly time (master plan §4.12).
//!
//! ## Sandboxing seam
//!
//! [`SandboxExecutorRewrite`] adapts the existing platform sandbox wrappers
//! (`sandbox.rs`: bubblewrap / Seatbelt / Docker argv rewriting) into the
//! [`SpawnRewrite`](shannon_tool_interface::SpawnRewrite) chain applied by
//! [`LocalProcess`] immediately before OS fork. Landlock-backed policies land
//! as additional rewrites or provider decorators without touching tools.

use async_trait::async_trait;
use shannon_tool_interface::{
    CapturedOutput, ChildWorldInit, DirEntryInfo, FileMeta, FileSystemProvider, ForkInitHost,
    PipedChild, PipedSpawn, ProcessExit, ProcessProvider, ProcessRequest, SpawnRewrite,
};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Project the common metadata surface off a `std::fs::Metadata`.
fn project_meta(meta: &std::fs::Metadata) -> FileMeta {
    FileMeta {
        len: meta.len(),
        is_dir: meta.is_dir(),
        modified: meta.modified().ok(),
    }
}

// ---------------------------------------------------------------------------
// LocalFs
// ---------------------------------------------------------------------------

/// The local-disk filesystem world. Zero-sized; every method is a direct
/// forward onto the matching `tokio::fs` / `std::fs` primitive, so no extra
/// buffering or copying is introduced versus the pre-seam call sites.
#[derive(Debug, Default, Clone, Copy)]
pub struct LocalFs;

impl LocalFs {
    /// `Arc`-boxed shared handle (the typical injection form).
    pub fn shared() -> Arc<dyn FileSystemProvider> {
        Arc::new(Self)
    }
}

#[async_trait]
impl FileSystemProvider for LocalFs {
    // ---- async face ----------------------------------------------------

    async fn read_text(&self, path: &Path) -> io::Result<String> {
        tokio::fs::read_to_string(path).await
    }

    async fn read_bytes(&self, path: &Path) -> io::Result<Vec<u8>> {
        tokio::fs::read(path).await
    }

    async fn metadata(&self, path: &Path) -> io::Result<FileMeta> {
        let meta = tokio::fs::metadata(path).await?;
        Ok(project_meta(&meta))
    }

    async fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        tokio::fs::create_dir_all(path).await
    }

    async fn write_bytes(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
        tokio::fs::write(path, contents).await
    }

    async fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        tokio::fs::rename(from, to).await
    }

    async fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        tokio::fs::canonicalize(path).await
    }

    // ---- blocking face -------------------------------------------------

    fn read_text_blocking(&self, path: &Path) -> io::Result<String> {
        std::fs::read_to_string(path)
    }

    fn write_bytes_blocking(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
        std::fs::write(path, contents)
    }

    fn create_dir_all_blocking(&self, path: &Path) -> io::Result<()> {
        std::fs::create_dir_all(path)
    }

    fn remove_file_blocking(&self, path: &Path) -> io::Result<()> {
        std::fs::remove_file(path)
    }

    fn canonicalize_blocking(&self, path: &Path) -> io::Result<PathBuf> {
        std::fs::canonicalize(path)
    }

    fn metadata_blocking(&self, path: &Path) -> io::Result<FileMeta> {
        Ok(project_meta(&std::fs::metadata(path)?))
    }

    fn read_prefix_blocking(&self, path: &Path, max_bytes: usize) -> io::Result<Vec<u8>> {
        use std::io::Read;
        let mut file = std::fs::File::open(path)?;
        let mut buf = vec![0u8; max_bytes];
        // Single bounded read mirrors `File::open` + fixed-buffer sniffing.
        let n = file.read(&mut buf)?;
        buf.truncate(n);
        Ok(buf)
    }

    fn list_dir_blocking(&self, path: &Path) -> io::Result<Vec<DirEntryInfo>> {
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let meta = entry.metadata()?;
            entries.push(DirEntryInfo {
                path: entry.path(),
                len: meta.len(),
                is_dir: meta.is_dir(),
            });
        }
        Ok(entries)
    }
}

// ---------------------------------------------------------------------------
// SpawnRewrite adapters
// ---------------------------------------------------------------------------

/// Rewriter backed by the legacy [`crate::sandbox::SandboxExecutor`].
///
/// Bridges the two shapes: it materializes a `std::process::Command` from the
/// request exactly like the pre-seam bash-sandbox path did, lets
/// `SandboxExecutor::wrap_command` replace program/args (bwrap, Seatbelt,
/// Docker), then reads the wrapped invocation back into a request. Behavior
/// is byte-equivalent with the pre-provider bash sandboxing flow.
pub struct SandboxExecutorRewrite {
    executor: Arc<crate::sandbox::SandboxExecutor>,
}

impl SandboxExecutorRewrite {
    /// Wrap an executor previously configured for the project directory.
    pub fn new(executor: Arc<crate::sandbox::SandboxExecutor>) -> Self {
        Self { executor }
    }
}

impl SpawnRewrite for SandboxExecutorRewrite {
    fn rewrite(&self, request: ProcessRequest) -> Result<ProcessRequest, String> {
        let ProcessRequest {
            program,
            args,
            cwd,
            env,
            ..
        } = request;

        let mut cmd = std::process::Command::new(&program);
        cmd.args(&args);
        for (k, v) in &env {
            cmd.env(k, v);
        }
        if let Some(dir) = &cwd {
            cmd.current_dir(dir);
        }

        self.executor
            .wrap_command(&mut cmd)
            .map_err(|e| e.to_string())?;

        let program = cmd.get_program().to_string_lossy().to_string();
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();
        // Re-read the effective additions so wrapper-inserted env survives.
        let env = cmd
            .get_envs()
            .filter_map(|(k, v)| {
                v.map(|v| {
                    (
                        k.to_string_lossy().to_string(),
                        v.to_string_lossy().to_string(),
                    )
                })
            })
            .collect();

        Ok(ProcessRequest {
            program,
            args,
            cwd,
            env,
            stdin_data: None,
        })
    }
}

// ---------------------------------------------------------------------------
// LocalProcess
// ---------------------------------------------------------------------------

/// The local host execution world (direct OS child processes).
///
/// A [`SpawnRewrite`] chain may be installed via [`LocalProcess::with_rewrite`]
/// / [`LocalProcess::set_rewrite`]; every spawn entry point runs the chain
/// first — this is the sandbox wrapping point consumed by §4.12. Separately,
/// a fork-time child initializer (`pre_exec`) may be installed; sandbox
/// backends use it to drop each child into its execution-world boundary
/// before exec. When unset, spawn paths are byte-identical to the §4.11
/// passthrough.
#[derive(Clone, Default)]
pub struct LocalProcess {
    rewrite: Option<Arc<dyn SpawnRewrite>>,
    #[cfg(unix)]
    fork_init: Option<Arc<ForkInitFn>>,
}

#[cfg(unix)]
type ForkInitFn = dyn Fn() -> io::Result<()> + Send + Sync;

impl std::fmt::Debug for LocalProcess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut dbg = f.debug_struct("LocalProcess");
        dbg.field("rewrite", &self.rewrite.is_some());
        #[cfg(unix)]
        dbg.field("fork_init", &self.fork_init.is_some());
        dbg.finish()
    }
}

impl LocalProcess {
    /// Provider with no rewrites (plain local execution).
    pub fn new() -> Self {
        Self {
            rewrite: None,
            #[cfg(unix)]
            fork_init: None,
        }
    }

    /// Provider applying `rewrite` before every OS spawn (§4.12 seam).
    pub fn with_rewrite(rewrite: Arc<dyn SpawnRewrite>) -> Self {
        Self {
            rewrite: Some(rewrite),
            #[cfg(unix)]
            fork_init: None,
        }
    }

    /// Replace the installed rewrite chain at runtime.
    pub fn set_rewrite(&mut self, rewrite: Arc<dyn SpawnRewrite>) {
        self.rewrite = Some(rewrite);
    }

    /// Apply the installed rewrite chain (identity when none).
    fn prepare(&self, request: ProcessRequest) -> Result<ProcessRequest, String> {
        match &self.rewrite {
            Some(rewrite) => rewrite.rewrite(request),
            None => Ok(request),
        }
    }

    /// Build a blocking `std::process::Command` from a prepared request.
    fn build_blocking(request: &ProcessRequest) -> std::process::Command {
        let mut cmd = std::process::Command::new(&request.program);
        cmd.args(&request.args);
        for (k, v) in &request.env {
            cmd.env(k, v);
        }
        if let Some(dir) = &request.cwd {
            cmd.current_dir(dir);
        }
        cmd
    }

    /// Install the fork-time initializer on a blocking command. Unset hook ⇒
    /// no-op (byte-identical spawn path).
    #[cfg(unix)]
    fn install_fork_init_std(&self, cmd: &mut std::process::Command) {
        use std::os::unix::process::CommandExt;
        if let Some(init) = &self.fork_init {
            let init = init.clone();
            // Executes inside the freshly forked child; an `Err` aborts the
            // spawn so a child never starts without its boundary installed.
            unsafe {
                cmd.pre_exec(move || init());
            }
        }
    }

    /// [`Self::install_fork_init_std`] twin for tokio commands.
    #[cfg(unix)]
    fn install_fork_init_tokio(&self, cmd: &mut tokio::process::Command) {
        if let Some(init) = &self.fork_init {
            let init = init.clone();
            unsafe {
                cmd.pre_exec(move || init());
            }
        }
    }
}

#[async_trait]
impl ProcessProvider for LocalProcess {
    fn run_blocking(&self, request: &ProcessRequest) -> io::Result<CapturedOutput> {
        let prepared = self
            .prepare(clone_request(request))
            .map_err(io::Error::other)?;

        if let Some(stdin_bytes) = prepared.stdin_data.clone() {
            use std::io::Write;
            use std::process::Stdio;
            let mut cmd = Self::build_blocking(&prepared);
            #[cfg(unix)]
            self.install_fork_init_std(&mut cmd);
            cmd.stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            let mut child = cmd.spawn()?;
            // Feed stdin fully before waiting to avoid pipe-capacity deadlock.
            if let Some(mut stdin) = child.stdin.take() {
                stdin.write_all(&stdin_bytes)?;
            }
            let output = child.wait_with_output()?;
            return Ok(CapturedOutput {
                stdout: output.stdout,
                stderr: output.stderr,
                exit: ProcessExit {
                    code: output.status.code(),
                    success: output.status.success(),
                },
            });
        }

        let output = {
            #[allow(unused_mut)]
            let mut cmd = Self::build_blocking(&prepared);
            #[cfg(unix)]
            self.install_fork_init_std(&mut cmd);
            cmd.output()?
        };
        Ok(CapturedOutput {
            stdout: output.stdout,
            stderr: output.stderr,
            exit: ProcessExit {
                code: output.status.code(),
                success: output.status.success(),
            },
        })
    }

    async fn run_async(&self, request: &ProcessRequest) -> io::Result<CapturedOutput> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::process::Command;

        let prepared = self
            .prepare(clone_request(request))
            .map_err(io::Error::other)?;

        let mut cmd = Command::new(&prepared.program);
        cmd.args(&prepared.args);
        for (k, v) in &prepared.env {
            cmd.env(k, v);
        }
        if let Some(dir) = &prepared.cwd {
            cmd.current_dir(dir);
        }

        // Mirror `Command::output()` stdin semantics: inherit nothing —
        // attempts by the child to read stdin see an immediately-closed
        // stream unless stdin data was supplied.
        let stdin_bytes = prepared.stdin_data.clone();
        if stdin_bytes.is_some() {
            cmd.stdin(std::process::Stdio::piped());
        } else {
            cmd.stdin(std::process::Stdio::null());
        }
        cmd.stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        // Cancellation of the returned future drops the child; mirror the
        // historical bash-tool `kill_on_drop(true)` so timeouts actually
        // terminate the underlying process.
        cmd.kill_on_drop(true);
        #[cfg(unix)]
        self.install_fork_init_tokio(&mut cmd);

        let mut child = cmd.spawn()?;
        if let Some(bytes) = stdin_bytes {
            if let Some(mut stdin) = child.stdin.take() {
                // Feed then close so the child sees EOF promptly.
                stdin.write_all(&bytes).await?;
                stdin.flush().await?;
            }
        }

        // Drain both pipes concurrently, mirroring `Command::output()` but
        // with the stdin feed folded in.
        let mut stdout_pipe = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("stdout not captured"))?;
        let mut stderr_pipe = child
            .stderr
            .take()
            .ok_or_else(|| io::Error::other("stderr not captured"))?;
        let (mut stdout_buf, mut stderr_buf) = (Vec::new(), Vec::new());
        let (r1, r2) = tokio::join!(
            stdout_pipe.read_to_end(&mut stdout_buf),
            stderr_pipe.read_to_end(&mut stderr_buf)
        );
        r1?;
        r2?;

        let status = child.wait().await?;
        Ok(CapturedOutput {
            stdout: stdout_buf,
            stderr: stderr_buf,
            exit: ProcessExit {
                code: status.code(),
                success: status.success(),
            },
        })
    }

    async fn spawn_piped(&self, spec: &PipedSpawn) -> io::Result<Box<dyn PipedChild>> {
        use std::process::Stdio;
        use tokio::process::Command;

        let prepared = self
            .prepare(clone_request(&spec.request))
            .map_err(io::Error::other)?;

        let mut cmd = Command::new(&prepared.program);
        cmd.args(&prepared.args);
        for (k, v) in &prepared.env {
            cmd.env(k, v);
        }
        if let Some(dir) = &prepared.cwd {
            cmd.current_dir(dir);
        }

        cmd.stdin(if spec.pipe_stdin {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(if spec.pipe_stdout {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stderr(if spec.pipe_stderr {
            Stdio::piped()
        } else {
            Stdio::null()
        });
        if spec.kill_on_drop {
            cmd.kill_on_drop(true);
        }
        #[cfg(unix)]
        self.install_fork_init_tokio(&mut cmd);

        let child = cmd.spawn()?;
        Ok(Box::new(LocalPipedChild { child }))
    }
}

impl ForkInitHost for LocalProcess {
    /// Produce a provider whose every future child runs `init` between fork
    /// and exec (§4.12 sandbox enforcement hook point).
    fn boxed_with_fork_init(
        self: Arc<Self>,
        init: Arc<dyn ChildWorldInit>,
    ) -> Result<Arc<dyn ProcessProvider>, String> {
        #[cfg(not(unix))]
        {
            let _ = init;
            Err("fork-time sandbox enforcement requires a unix host".to_string())
        }
        #[cfg(unix)]
        {
            let fork_init: Arc<dyn Fn() -> io::Result<()> + Send + Sync> =
                Arc::new(move || init.init_child());
            Ok(Arc::new(Self {
                rewrite: self.rewrite.clone(),
                fork_init: Some(fork_init),
            }))
        }
    }
}

/// Clone helper kept local so `ProcessRequest` stays cheap at call sites.
fn clone_request(request: &ProcessRequest) -> ProcessRequest {
    request.clone()
}

/// Host OS process id, surfaced through the provider layer so consumers never
/// touch process-spawn APIs directly (e.g. the LSP `initialize` handshake).
pub fn host_process_id() -> u32 {
    std::process::id()
}

/// tokio-backed [`PipedChild`]; `kill_on_drop` state lives inside the child
/// itself, so dropping this handle preserves the spawner's cancellation intent.
struct LocalPipedChild {
    child: tokio::process::Child,
}

#[async_trait]
impl PipedChild for LocalPipedChild {
    fn take_stdin(&mut self) -> Option<Box<dyn tokio::io::AsyncWrite + Send + Unpin>> {
        self.child.stdin.take().map(|s| Box::new(s) as _)
    }

    fn take_stdout(&mut self) -> Option<Box<dyn tokio::io::AsyncRead + Send + Unpin>> {
        self.child.stdout.take().map(|s| Box::new(s) as _)
    }

    fn take_stderr(&mut self) -> Option<Box<dyn tokio::io::AsyncRead + Send + Unpin>> {
        self.child.stderr.take().map(|s| Box::new(s) as _)
    }

    async fn kill(&mut self) {
        // `start_kill` only signals; `wait` reaps so no zombie remains and
        // behavior matches tokio's `Child::kill` contract.
        let _ = self.child.kill().await;
    }

    async fn wait(&mut self) -> io::Result<ProcessExit> {
        let status = self.child.wait().await?;
        Ok(ProcessExit {
            code: status.code(),
            success: status.success(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn tempdir() -> TempDir {
        tempfile::tempdir().expect("create temp dir")
    }

    // ── LocalFs ─────────────────────────────────────────────────────────

    #[tokio::test]
    async fn local_fs_roundtrip_and_rename() {
        let dir = tempdir();
        let target = dir.path().join("a.txt");
        let fs = LocalFs;
        fs.create_dir_all(&dir.path().join("nested/deep"))
            .await
            .expect("mkdir");
        fs.write_bytes(&target, b"hello").await.expect("write");
        assert_eq!(fs.read_text(&target).await.expect("read"), "hello");
        let meta = fs.metadata(&target).await.expect("meta");
        assert_eq!(meta.len, 5);
        assert!(!meta.is_dir);

        fs.rename(&target, &dir.path().join("b.txt"))
            .await
            .expect("rename");
        assert!(fs.read_text(&dir.path().join("b.txt")).await.is_ok());
    }

    #[test]
    fn local_fs_blocking_face() {
        let dir = tempdir();
        let fs = LocalFs;
        let p = dir.path().join("x.bin");
        fs.write_bytes_blocking(&p, &[0u8, 1, 0]).expect("write");
        let prefix = fs.read_prefix_blocking(&p, 2).expect("prefix");
        assert_eq!(prefix, vec![0u8, 1]);

        fs.create_dir_all_blocking(&dir.path().join("d/e"))
            .expect("dirs");
        fs.write_bytes_blocking(&dir.path().join("d/f.txt"), b"data")
            .expect("leaf");
        let entries = fs.list_dir_blocking(&dir.path().join("d")).expect("list");
        assert_eq!(entries.len(), 2);
        let sub = entries
            .iter()
            .find(|e| e.path.ends_with("e"))
            .expect("entry");
        assert!(sub.is_dir);
        let leaf = entries
            .iter()
            .find(|e| e.path.ends_with("f.txt"))
            .expect("entry");
        assert_eq!(leaf.len, 4);
        assert!(!leaf.is_dir);

        assert!(fs.exists_blocking(&dir.path().join("d/f.txt")));
        assert!(!fs.exists_blocking(&dir.path().join("missing")));
        let canon = fs
            .canonicalize_blocking(&dir.path().join("d"))
            .expect("canon");
        assert!(canon.ends_with("d"));

        fs.remove_file_blocking(&dir.path().join("d/f.txt"))
            .expect("remove");
        assert!(!fs.exists_blocking(&dir.path().join("d/f.txt")));
    }

    // ── LocalProcess captured runs ─────────────────────────────────────

    #[test]
    fn local_process_run_blocking_captures_streams() {
        let out = LocalProcess::new()
            .run_blocking(&ProcessRequest::new("printf", &["hi %s", "there"]))
            .expect("run");
        assert_eq!(out.stdout, b"hi there".to_vec());
        assert!(out.exit.success);
    }

    #[test]
    fn local_process_run_blocking_exit_code_and_cwd_env() {
        let dir = tempdir();
        let proc_world = LocalProcess::new();
        let req = ProcessRequest {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), "pwd; printf \"$MARK\"; exit 7".into()],
            cwd: Some(dir.path().to_path_buf()),
            env: vec![("MARK".into(), "env-ok".into())],
            ..Default::default()
        };
        let out = proc_world.run_blocking(&req).expect("run");
        assert_eq!(out.exit.code, Some(7));
        assert!(!out.exit.success);
        assert!(
            out.stdout
                .starts_with(dir.path().to_str().expect("utf8").as_bytes())
        );
        assert!(
            String::from_utf8(out.stdout)
                .expect("utf8")
                .contains("env-ok")
        );
    }

    #[tokio::test]
    async fn local_process_run_async_matches_blocking_shape() {
        let out = LocalProcess::new()
            .run_async(&ProcessRequest::new(
                "/bin/sh",
                &["-c", "echo async-out; echo err 1>&2"],
            ))
            .await
            .expect("run");
        assert_eq!(out.stdout, b"async-out\n");
        assert_eq!(out.stderr, b"err\n");
        assert!(out.exit.success);
    }

    #[tokio::test]
    async fn local_process_run_async_feeds_stdin() {
        let req = ProcessRequest {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), "tr a-z A-Z".into()],
            stdin_data: Some(b"payload".to_vec()),
            ..Default::default()
        };
        let out = LocalProcess::new().run_async(&req).await.expect("run");
        assert_eq!(out.stdout, b"PAYLOAD");
    }

    // ── Piped spawn ────────────────────────────────────────────────────

    #[tokio::test]
    async fn local_process_spawn_piped_roundtrip_through_stream_handles() {
        let spec = PipedSpawn {
            request: ProcessRequest::new(
                "/bin/sh",
                &["-c", "while read -r line; do echo \"got:$line\"; done"],
            ),
            pipe_stdin: true,
            pipe_stdout: true,
            pipe_stderr: false,
            kill_on_drop: true,
        };
        let world = LocalProcess::new();
        let mut child = world.spawn_piped(&spec).await.expect("spawn");

        // Drop the writer explicitly: tokio's `shutdown` only flushes; the
        // pipe EOF the child waits for comes from closing the descriptor.
        {
            let mut stdin = child.take_stdin().expect("stdin piped");
            use tokio::io::AsyncWriteExt;
            stdin.write_all(b"ping\n").await.expect("feed");
        }

        let stdout = child.take_stdout().expect("stdout piped");
        use tokio::io::AsyncBufReadExt;
        let mut lines = tokio::io::BufReader::new(stdout).lines();
        let line = lines.next_line().await.expect("line").expect("eof?");
        assert_eq!(line, "got:ping");

        let exit = child.wait().await.expect("reap");
        assert!(exit.success);
    }

    // ── §4.12 wrapping seam ────────────────────────────────────────────

    struct SubstituteProgramRewrite {
        program: &'static str,
        args: Vec<&'static str>,
    }

    impl SpawnRewrite for SubstituteProgramRewrite {
        fn rewrite(&self, mut request: ProcessRequest) -> Result<ProcessRequest, String> {
            request.program = self.program.into();
            request.args = self.args.iter().map(|a| (*a).into()).collect();
            Ok(request)
        }
    }

    #[tokio::test]
    async fn spawn_rewrite_hook_intercepts_before_os_spawn() {
        // The original request names a binary that does not exist; the
        // installed §4.12-style hook swaps it to /bin/echo. Success proves
        // the hook ran between `run_*` and the actual OS spawn.
        let world = LocalProcess::with_rewrite(Arc::new(SubstituteProgramRewrite {
            program: "/bin/echo",
            args: vec!["rewritten"],
        }));
        let req = ProcessRequest::new("definitely-not-a-real-binary-x9", &[]);
        let out = world.run_async(&req).await.expect("hook should have fired");
        assert_eq!(out.stdout, b"rewritten\n");

        let blocked = LocalProcess::new();
        assert!(
            blocked.run_blocking(&req).is_err(),
            "without the rewrite the missing binary must fail"
        );
    }

    #[test]
    fn sandbox_executor_rewrite_disabled_config_is_identity() {
        use crate::sandbox::SandboxConfig;
        let executor = Arc::new(crate::sandbox::SandboxExecutor::new(
            SandboxConfig::new("/tmp/some-project").disabled(),
        ));
        let rewrite = SandboxExecutorRewrite::new(executor);
        let original = ProcessRequest::new("git", &["status"]);
        let got = rewrite.rewrite(original.clone()).expect("ok");
        assert_eq!(got.program, "git");
        assert_eq!(got.args, vec!["status"]);
        assert_eq!(got.cwd, None);
    }

    // ── §4.12 fork-time world initializer (pre_exec seam) ──────────────

    struct TouchInit {
        path: std::path::PathBuf,
        /// When set, initialization fails with this raw OS error instead of
        /// installing (mirrors how kernel-backed initializers report).
        fail_with_raw: Option<i32>,
    }

    impl ChildWorldInit for TouchInit {
        fn init_child(&self) -> io::Result<()> {
            if let Some(code) = self.fail_with_raw {
                return Err(io::Error::from_raw_os_error(code));
            }
            std::fs::write(&self.path, b"installed")?;
            Ok(())
        }
    }

    #[cfg(unix)]
    fn host_with_init(
        path: std::path::PathBuf,
        fail_with_raw: Option<i32>,
    ) -> Arc<dyn ProcessProvider> {
        Arc::new(LocalProcess::new())
            .boxed_with_fork_init(Arc::new(TouchInit {
                path,
                fail_with_raw,
            }))
            .expect("local host accepts fork init")
    }

    /// The installed initializer runs inside the child before exec.
    #[cfg(unix)]
    #[test]
    fn fork_init_runs_before_child_exec() {
        let dir = tempdir();
        let marker = dir.path().join("boundary-marker");
        let host = host_with_init(marker.clone(), None);
        host.run_blocking(&ProcessRequest::new("/bin/true", &[]))
            .expect("child with boundary installed");
        assert!(
            marker.exists(),
            "initializer must have run inside the child pre-exec"
        );
    }

    /// A failing initializer aborts the spawn — a child never starts without
    /// its execution-world boundary (fail-closed contract). Raw OS errors
    /// propagate verbatim (production kernels report EPERM here).
    #[cfg(unix)]
    #[test]
    fn fork_init_failure_aborts_spawn_fail_closed() {
        let dir = tempdir();
        let marker = dir.path().join("never");
        let host = host_with_init(marker.clone(), Some(13)); // EACCES
        let err = match host.run_blocking(&ProcessRequest::new("/bin/true", &[])) {
            Ok(_) => panic!("failing initializer must abort the spawn"),
            Err(e) => e,
        };
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
        assert!(!marker.exists(), "child must never have started");
    }
}
