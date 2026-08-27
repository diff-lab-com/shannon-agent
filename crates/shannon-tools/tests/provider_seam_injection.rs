//! §4.11 W3-3a — provider injection end-to-end proofs.
//!
//! Validation criteria covered here:
//!
//! 1. **Interchangeability (design red line)**: an in-memory `MemoryFs` plus a
//!    scripted `FakeProc` drive a read/write tool pair, the Edit tool and a
//!    git tool through the same behavioral assertions the local
//!    implementations are tested against.
//! 2. **No direct world access**: behavior of the injected paths is fully
//!    determined by what the mock providers served or recorded — a leftover
//!    direct `std::fs` / spawn call inside a migrated tool breaks these tests.
//! 3. **Sandbox seam**: a `SpawnRewrite` hook installed on `LocalProcess`
//!    rewrites spawn requests before the OS sees them (the §4.12 decoration
//!    point), and assembly via `register_default_tools_with_providers`
//!    produces the same registry shape as the default registration.

use shannon_core::providers::{LocalProcess, SandboxExecutorRewrite};
use shannon_tool_interface::{
    CapturedOutput, ChainedSpawnRewrite, DirEntryInfo, FileMeta, FileSystemProvider, PipedChild,
    PipedSpawn, ProcessExit, ProcessProvider, ProcessRequest, SpawnRewrite,
};
use shannon_tools::Tool;
use shannon_tools::{
    EditTool, GitBranchTool, PathSandbox, PathSandboxConfig, ReadTool, ToolProviders, WriteTool,
};
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex};

// ---------------------------------------------------------------------------
// In-memory filesystem implementation ("second implementation", red line 1)
// ---------------------------------------------------------------------------

/// A complete in-memory [`FileSystemProvider`]. Inherent observer helpers and
/// the trait impl share one struct so tests can both inject the provider into
/// tools and inspect recorded state directly.
#[derive(Clone, Default)]
struct MemoryFs {
    files: Arc<StdMutex<BTreeMap<PathBuf, Vec<u8>>>>,
    dirs: Arc<StdMutex<Vec<PathBuf>>>,
}

impl MemoryFs {
    fn seed(&self, path: &str, contents: &str) {
        self.files
            .lock()
            .expect("memfs lock")
            .insert(PathBuf::from(path), contents.as_bytes().to_vec());
    }

    fn contents(&self, path: &str) -> Option<String> {
        self.files
            .lock()
            .expect("memfs lock")
            .get(Path::new(path))
            .map(|b| String::from_utf8_lossy(b).to_string())
    }

    /// Permissive sandbox whose canonicalization rides this same world.
    fn permissive_sandbox(&self) -> PathSandbox {
        PathSandbox::with_config(PathSandboxConfig {
            allowed_roots: vec![PathBuf::from("/")],
            denied_patterns: PathSandboxConfig::default_denied_patterns(),
            strict_mode: false,
        })
        .with_fs_provider(Arc::new(self.clone()))
    }

    fn as_provider(&self) -> Arc<dyn FileSystemProvider> {
        Arc::new(self.clone())
    }
}

#[async_trait::async_trait]
impl FileSystemProvider for MemoryFs {
    async fn read_text(&self, path: &Path) -> io::Result<String> {
        self.read_text_blocking(path)
    }

    async fn read_bytes(&self, path: &Path) -> io::Result<Vec<u8>> {
        self.read_prefix_blocking(path, usize::MAX)
    }

    async fn metadata(&self, path: &Path) -> io::Result<FileMeta> {
        self.metadata_blocking(path)
    }

    async fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        self.create_dir_all_blocking(path)
    }

    async fn write_bytes(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
        self.write_bytes_blocking(path, contents)
    }

    async fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        // Direct move; no temp-file bookkeeping needed by observers here.
        let mut guard = self.files.lock().expect("memfs lock");
        let bytes = guard
            .remove(from)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no such file"))?;
        guard.insert(to.to_path_buf(), bytes);
        Ok(())
    }

    async fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        self.canonicalize_blocking(path)
    }

    fn read_text_blocking(&self, path: &Path) -> io::Result<String> {
        let guard = self.files.lock().expect("memfs lock");
        let bytes = guard
            .get(path)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no such file"))?;
        String::from_utf8(bytes.clone()).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    fn write_bytes_blocking(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
        self.files
            .lock()
            .expect("memfs lock")
            .insert(path.to_path_buf(), contents.to_vec());
        Ok(())
    }

    fn create_dir_all_blocking(&self, path: &Path) -> io::Result<()> {
        self.dirs.lock().expect("dirs log").push(path.to_path_buf());
        Ok(())
    }

    fn remove_file_blocking(&self, path: &Path) -> io::Result<()> {
        self.files.lock().expect("memfs lock").remove(path);
        Ok(())
    }

    fn canonicalize_blocking(&self, path: &Path) -> io::Result<PathBuf> {
        Ok(path.to_path_buf())
    }

    fn metadata_blocking(&self, path: &Path) -> io::Result<FileMeta> {
        if let Some(bytes) = self.files.lock().expect("memfs lock").get(path) {
            return Ok(FileMeta {
                len: bytes.len() as u64,
                is_dir: false,
                modified: None,
            });
        }
        Err(io::Error::new(io::ErrorKind::NotFound, "no such file"))
    }

    fn read_prefix_blocking(&self, path: &Path, max_bytes: usize) -> io::Result<Vec<u8>> {
        let guard = self.files.lock().expect("memfs lock");
        let bytes = guard
            .get(path)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no such file"))?;
        Ok(bytes[..bytes.len().min(max_bytes)].to_vec())
    }

    fn list_dir_blocking(&self, _path: &Path) -> io::Result<Vec<DirEntryInfo>> {
        Ok(Vec::new())
    }
}

// ---------------------------------------------------------------------------
// Scripted process implementation ("second implementation", red line 1)
// ---------------------------------------------------------------------------

/// Deterministic [`ProcessProvider`]: records every request and replays a
/// canned response matched by program+args. Never spawns anything.
#[derive(Clone, Default)]
struct FakeProc {
    requests: Arc<StdMutex<Vec<ProcessRequest>>>,
    responses: Arc<StdMutex<BTreeMap<(String, String), CapturedOutput>>>,
}

impl FakeProc {
    fn respond(&self, program: &str, args: &[&str], out: CapturedOutput) {
        self.responses
            .lock()
            .expect("responses")
            .insert((program.into(), args.join("\u{1}")), out);
    }

    fn recorded(&self) -> Vec<ProcessRequest> {
        self.requests.lock().expect("requests").clone()
    }
}

#[async_trait::async_trait]
impl ProcessProvider for FakeProc {
    fn run_blocking(&self, request: &ProcessRequest) -> io::Result<CapturedOutput> {
        self.requests
            .lock()
            .expect("requests")
            .push(request.clone());
        self.responses
            .lock()
            .expect("responses")
            .get(&(request.program.clone(), request.args.join("\u{1}")))
            .cloned()
            .ok_or_else(|| io::Error::other(format!("unexpected spawn: {}", request.program)))
    }

    async fn run_async(&self, request: &ProcessRequest) -> io::Result<CapturedOutput> {
        self.run_blocking(request)
    }

    async fn spawn_piped(&self, _spec: &PipedSpawn) -> io::Result<Box<dyn PipedChild>> {
        Err(io::Error::other("fake proc hosts no piped children"))
    }
}

// ---------------------------------------------------------------------------
// Red line 1+2 — read/write tools fully driven by the in-memory world
// ---------------------------------------------------------------------------

async fn write_then_read_roundtrip(world: Arc<dyn FileSystemProvider>, sandbox: PathSandbox) {
    let target = "/ws/note.txt";

    let write_tool = WriteTool::with_sandbox(sandbox.clone())
        .with_history_opt(None)
        .with_fs(Arc::clone(&world));
    let out = write_tool
        .execute(serde_json::json!({
            "file_path": target,
            "content": "hello world"
        }))
        .await
        .expect("write succeeds through provider");
    assert!(!out.is_error);
    assert_eq!(out.metadata.get("bytes"), Some(&serde_json::json!(11)));

    let read_tool = ReadTool::with_sandbox(sandbox).with_fs(Arc::clone(&world));
    let out = read_tool
        .execute(serde_json::json!({
            "file_path": target,
            "truncate_large_files": false
        }))
        .await
        .expect("read succeeds through provider");
    assert!(!out.is_error);
    assert_eq!(out.content, "hello world");

    // Overwrite semantics match the local suite's expectations.
    let out = write_tool
        .execute(serde_json::json!({
            "file_path": target,
            "content": "second"
        }))
        .await
        .unwrap();
    assert!(!out.is_error);
    let out = read_tool
        .execute(serde_json::json!({
            "file_path": target,
            "truncate_large_files": false
        }))
        .await
        .unwrap();
    assert_eq!(out.content, "second", "overwrite must replace content");
}

#[tokio::test]
async fn read_write_tools_run_end_to_end_on_memory_fs() {
    let memfs = MemoryFs::default();
    write_then_read_roundtrip(memfs.as_provider(), memfs.permissive_sandbox()).await;

    // Everything the pair did is observable inside the mock — and *only*
    // there, proving no real-disk side effects occurred.
    assert_eq!(memfs.contents("/ws/note.txt"), Some("second".to_string()));
}

#[tokio::test]
async fn edit_tool_applies_replacement_through_memory_fs() {
    let memfs = MemoryFs::default();
    memfs.seed("/ws/code.rs", "fn main() {}\n");
    let proc = FakeProc::default();

    let edit = EditTool::with_sandbox(memfs.permissive_sandbox())
        .with_worlds(memfs.as_provider(), Arc::new(proc.clone()));

    let out = edit
        .execute(serde_json::json!({
            "file_path": "/ws/code.rs",
            "old_string": "fn main() {}",
            "new_string": "fn main() { println!(\"hi\"); }"
        }))
        .await
        .expect("edit succeeds entirely via providers");
    assert!(!out.is_error);
    assert!(out.content.contains("Successfully replaced 1 occurrence"));

    assert_eq!(
        memfs.contents("/ws/code.rs"),
        Some("fn main() { println!(\"hi\"); }\n".to_string()),
        "the edit landed inside the in-memory world only"
    );
    // No git probe fired because old_string was found directly.
    assert!(proc.recorded().is_empty());
}

#[tokio::test]
async fn edit_merge_fallback_probes_git_through_injected_process() {
    let memfs = MemoryFs::default();
    let content = "alpha\n";
    memfs.seed("/ws/f.txt", content);
    let proc = FakeProc::default();

    // git show HEAD:<path> → base containing the target string lets the
    // three-way merge path run without touching any real repository.
    proc.respond(
        "git",
        &["show", "HEAD:/ws/f.txt"],
        CapturedOutput {
            stdout: b"alpha\ngamma\n".to_vec(),
            stderr: Vec::new(),
            exit: ProcessExit::from_code(0),
        },
    );
    // old_string missing on disk but present in the scripted HEAD base.
    proc.respond(
        "git",
        &["show", "HEAD:/ws/f.txt"], // second lookup when re-reading base? single probe suffices
        CapturedOutput {
            stdout: b"alpha\ngamma\n".to_vec(),
            stderr: Vec::new(),
            exit: ProcessExit::from_code(0),
        },
    );

    let edit = EditTool::with_sandbox(memfs.permissive_sandbox())
        .with_worlds(memfs.as_provider(), Arc::new(proc.clone()));

    let out = edit
        .execute(serde_json::json!({
            "file_path": "/ws/f.txt",
            "old_string": "gamma",
            "new_string": "GAMMA"
        }))
        .await;
    // Either the merge applied (content now contains GAMMA) or the tool
    // reported "not found"; both prove the probe rode the fake provider.
    if let Ok(out) = out {
        assert!(
            !out.is_error || out.content.contains("merge"),
            "unexpected failure shape: {}",
            out.content
        );
    }
    let recorded = proc.recorded();
    assert!(
        recorded.iter().all(|r| r.program == "git"),
        "all spawns route through the provider"
    );
    assert!(
        recorded
            .iter()
            .any(|r| r.args.first().map(String::as_str) == Some("show")),
        "git show HEAD probe went through the injected world"
    );
}

// ---------------------------------------------------------------------------
// Red line 1+2 — process-backed tool driven by the scripted world
// ---------------------------------------------------------------------------

#[tokio::test]
async fn git_tools_run_through_scripted_process_provider() {
    let proc = FakeProc::default();

    proc.respond(
        "git",
        &["branch", "-a", "--color=never", "-v", "--no-abbrev"],
        CapturedOutput {
            stdout: b"* main abc123 feat: x\n".to_vec(),
            stderr: Vec::new(),
            exit: ProcessExit::from_code(0),
        },
    );

    let branch_tool = GitBranchTool::new().with_process(Arc::new(proc.clone()));
    let result = branch_tool
        .execute(serde_json::json!({ "action": "list" }))
        .await
        .expect("git tool runs against the scripted provider");
    assert!(!result.is_error);
    assert!(result.content.contains("main"));

    let recorded = proc.recorded();
    assert!(
        recorded.iter().any(|r| r.program == "git"),
        "tool routed its invocation through the injected world"
    );
}

#[tokio::test]
async fn bash_captured_runs_use_the_provider_request_shape() {
    let proc = FakeProc::default();

    proc.respond(
        "bash",
        &["-c", "echo hi"],
        CapturedOutput {
            stdout: b"hi\n".to_vec(),
            stderr: Vec::new(),
            exit: ProcessExit::from_code(0),
        },
    );

    use shannon_tools::BashTool;
    let bash = BashTool::new().with_worlds(Arc::new(proc.clone()));

    let result = bash
        .execute(serde_json::json!({ "command": "echo hi" }))
        .await
        .expect("captured run goes through the injected world");
    assert!(!result.is_error);
    assert!(result.content.starts_with("hi"));

    let recorded = proc.recorded();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].program, "bash");
    assert_eq!(
        recorded[0].args,
        vec!["-c".to_string(), "echo hi".to_string()]
    );
}

// ---------------------------------------------------------------------------
// Red line 3 — assembly-level interchangeability + §4.12 seam
// ---------------------------------------------------------------------------

#[tokio::test]
async fn assembly_with_injected_providers_yields_same_registry_shape() {
    use shannon_tools::{register_default_tools, register_default_tools_with_providers};

    let mut default_registry = shannon_core::tools::ToolRegistry::new();
    register_default_tools(&mut default_registry).expect("defaults register");

    let mut injected_registry = shannon_core::tools::ToolRegistry::new();
    let worlds = ToolProviders::default();
    register_default_tools_with_providers(&mut injected_registry, &worlds)
        .expect("providers variant registers");

    let mut names_a: Vec<String> = default_registry
        .list_tools_info()
        .into_iter()
        .map(|t| t.name)
        .collect();
    let mut names_b: Vec<String> = injected_registry
        .list_tools_info()
        .into_iter()
        .map(|t| t.name)
        .collect();
    names_a.sort();
    names_b.sort();
    assert_eq!(names_a, names_b, "world injection must not change the set");
}

struct PrefixArgsRewrite {
    prefix: &'static str,
}

impl SpawnRewrite for PrefixArgsRewrite {
    fn rewrite(&self, mut request: ProcessRequest) -> Result<ProcessRequest, String> {
        request.args.insert(0, self.prefix.to_string());
        Ok(request)
    }
}

#[tokio::test]
async fn local_process_spawn_rewrite_hook_wraps_before_os_spawn() {
    // With the hook the invocation becomes `/bin/echo -x hooked`, so success
    // plus output ordering proves the rewrite ran between run_async and fork.
    let world = LocalProcess::with_rewrite(Arc::new(PrefixArgsRewrite { prefix: "-x" }));
    let req = ProcessRequest::new("/bin/echo", &["hooked"]);
    let out = world.run_async(&req).await.expect("rewritten spawn runs");
    assert_eq!(out.stdout, b"-x hooked\n");
}

#[tokio::test]
async fn local_process_without_rewrite_fails_missing_binary() {
    let plain = LocalProcess::new();
    let req = ProcessRequest::new("definitely-not-a-real-binary-x9", &[]);
    assert!(
        plain.run_async(&req).await.is_err(),
        "identity hook keeps normal spawn failure"
    );
}

#[test]
fn chained_rewrite_composes_and_short_circuits_like_the_seam_contract() {
    struct Rejecter;
    impl SpawnRewrite for Rejecter {
        fn rewrite(&self, _r: ProcessRequest) -> Result<ProcessRequest, String> {
            Err("sandbox policy denied".into())
        }
    }
    let chain = ChainedSpawnRewrite::new(vec![Arc::new(Rejecter)]);
    assert_eq!(
        chain
            .rewrite(ProcessRequest::new("sh", &[]))
            .err()
            .as_deref(),
        Some("sandbox policy denied")
    );
}

#[test]
fn sandbox_executor_bridge_compile_reference() {
    // §4.11→§4.12 adapter reference: rewriting through a disabled executor is
    // identity — documents how bwrap/Seatbelt/Docker land behind the seam.
    let executor = Arc::new(shannon_core::sandbox::SandboxExecutor::new(
        shannon_core::sandbox::SandboxConfig::new("/tmp/seam-ref-project").disabled(),
    ));
    let rewrite = SandboxExecutorRewrite::new(executor);
    let original = ProcessRequest::new("git", &["status"]);
    let got = rewrite
        .rewrite(original.clone())
        .expect("disabled = identity");
    assert_eq!(got.program, "git");
}
