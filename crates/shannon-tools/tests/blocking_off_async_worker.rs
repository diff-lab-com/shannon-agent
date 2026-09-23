//! Review §P2-14: `*_blocking` provider calls are synchronous IO — over an
//! SSH/Docker world each one is even a network round-trip plus a helper
//! thread. They must run on tokio's blocking pool, never inline inside the
//! calling tool's async task (which would stall the tokio worker for the
//! whole walk/read).
//!
//! Detection: the tests drive a `#[tokio::test]` **current-thread** runtime
//! where the tool's async `execute` and a sibling ticker task share one OS
//! thread. The probe provider's blocking methods sleep for a while. If the
//! blocking section ran inline in the async task, the whole thread is
//! blocked and the ticker freezes (≈0 ticks during the call); running on the
//! blocking pool — the §P2-14 fix — keeps the runtime thread free and the
//! ticker advances.

#![allow(clippy::unwrap_used)]

use shannon_tool_interface::{DirEntryInfo, FileMeta, FileSystemProvider, Tool};
use shannon_tools::{GrepTool, ReadTool};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// How long each probe blocking call pretends to take.
const BLOCKING_CALL_MS: u64 = 250;
/// Ticker period; the async worker must stay responsive at this granularity.
const TICK_MS: u64 = 10;
/// Minimum ticks expected DURING one blocking call when the worker is free.
/// 250ms / 10ms = ~25; a generous 5x margin keeps this deterministic.
const MIN_TICKS_DURING_BLOCK: usize = 5;

#[derive(Default)]
struct ProbeFs {
    blocking_calls: AtomicUsize,
}

impl ProbeFs {
    fn block_a_while(&self) {
        self.blocking_calls.fetch_add(1, Ordering::SeqCst);
        std::thread::sleep(std::time::Duration::from_millis(BLOCKING_CALL_MS));
    }
}

#[async_trait::async_trait]
impl FileSystemProvider for ProbeFs {
    async fn read_text(&self, _path: &Path) -> io::Result<String> {
        Ok("needle\nplain\nneedle again\n".to_string())
    }
    async fn read_bytes(&self, _p: &Path) -> io::Result<Vec<u8>> {
        Ok(Vec::new())
    }
    async fn metadata(&self, _p: &Path) -> io::Result<FileMeta> {
        Ok(FileMeta {
            len: 64,
            is_dir: false,
            modified: None,
        })
    }
    async fn create_dir_all(&self, _p: &Path) -> io::Result<()> {
        Ok(())
    }
    async fn write_bytes(&self, _p: &Path, _c: &[u8]) -> io::Result<()> {
        Ok(())
    }
    async fn rename(&self, _f: &Path, _t: &Path) -> io::Result<()> {
        Ok(())
    }
    async fn canonicalize(&self, p: &Path) -> io::Result<PathBuf> {
        Ok(p.to_path_buf())
    }

    fn read_text_blocking(&self, _p: &Path) -> io::Result<String> {
        self.block_a_while();
        Ok("needle\nplain\nneedle again\n".to_string())
    }
    fn write_bytes_blocking(&self, _p: &Path, _c: &[u8]) -> io::Result<()> {
        Ok(())
    }
    fn create_dir_all_blocking(&self, _p: &Path) -> io::Result<()> {
        Ok(())
    }
    fn rename_blocking(&self, _from: &Path, _to: &Path) -> io::Result<()> {
        Ok(())
    }
    fn remove_file_blocking(&self, _p: &Path) -> io::Result<()> {
        Ok(())
    }
    fn canonicalize_blocking(&self, p: &Path) -> io::Result<PathBuf> {
        Ok(p.to_path_buf())
    }
    fn metadata_blocking(&self, _p: &Path) -> io::Result<FileMeta> {
        self.block_a_while();
        Ok(FileMeta {
            len: 64,
            is_dir: false,
            modified: None,
        })
    }
    fn read_prefix_blocking(&self, _p: &Path, _m: usize) -> io::Result<Vec<u8>> {
        self.block_a_while();
        Ok(b"needle".to_vec())
    }
    fn list_dir_blocking(&self, _p: &Path) -> io::Result<Vec<DirEntryInfo>> {
        Ok(Vec::new())
    }
    fn exists_blocking(&self, _p: &Path) -> bool {
        self.block_a_while();
        true
    }
    fn walk_blocking(
        &self,
        root: &Path,
        cb: &mut dyn FnMut(&DirEntryInfo) -> bool,
    ) -> io::Result<()> {
        self.block_a_while();
        cb(&DirEntryInfo {
            path: root.to_path_buf(),
            len: 0,
            is_dir: true,
        });
        cb(&DirEntryInfo {
            path: root.join("a.rs"),
            len: 64,
            is_dir: false,
        });
        cb(&DirEntryInfo {
            path: root.join("b.rs"),
            len: 64,
            is_dir: false,
        });
        Ok(())
    }
}

/// Spawn the sibling ticker on the same single-threaded runtime and return
/// (abort handle, shared tick counter).
async fn spawn_ticker() -> (tokio::task::JoinHandle<()>, Arc<AtomicUsize>) {
    let ticks = Arc::new(AtomicUsize::new(0));
    let t = ticks.clone();
    let handle = tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(TICK_MS)).await;
            t.fetch_add(1, Ordering::SeqCst);
        }
    });
    (handle, ticks)
}

#[tokio::test]
async fn grep_blocking_io_runs_off_the_async_worker() {
    let probe = Arc::new(ProbeFs::default());
    // Wire the SAME world into the sandbox's TOCTOU canonicalization so the
    // nonexistent local path resolves through the provider (as the remote
    // assembly does).
    let sandbox = shannon_tools::file::sandbox::PathSandbox::with_config(
        shannon_tools::file::sandbox::SandboxConfig {
            allowed_roots: vec![PathBuf::from("/remote/proj")],
            denied_patterns: shannon_tools::file::sandbox::SandboxConfig::default_denied_patterns(),
            strict_mode: true,
        },
    )
    .with_fs_provider(probe.clone());
    let tool = GrepTool::with_sandbox(sandbox).with_fs(probe.clone());

    let (ticker, ticks) = spawn_ticker().await;
    let before = ticks.load(Ordering::SeqCst);

    let output = tool
        .execute(serde_json::json!({ "pattern": "needle", "path": "/remote/proj" }))
        .await
        .expect("grep must succeed through the probe world");
    assert!(!output.is_error, "grep output: {}", output.content);

    let after = ticks.load(Ordering::SeqCst);
    ticker.abort();

    assert!(
        probe.blocking_calls.load(Ordering::SeqCst) > 0,
        "test must exercise the blocking provider surface"
    );
    let ticks_during = after.saturating_sub(before);
    assert!(
        ticks_during >= MIN_TICKS_DURING_BLOCK,
        "the async worker froze for the duration of the blocking calls \
         (only {ticks_during} ticks over {}ms of blocking IO) — §P2-14 regression",
        BLOCKING_CALL_MS
    );
}

#[tokio::test]
async fn read_binary_sniff_runs_off_the_async_worker() {
    let probe = Arc::new(ProbeFs::default());
    // Allow the temp dir explicitly and create the real file so the default
    // sandbox resolution succeeds.
    let file = std::env::temp_dir().join("shannon-p2-14-probe.txt");
    std::fs::write(&file, b"placeholder").unwrap();
    let sandbox = shannon_tools::file::sandbox::PathSandbox::with_config(
        shannon_tools::file::sandbox::SandboxConfig {
            allowed_roots: vec![std::env::temp_dir()],
            denied_patterns: shannon_tools::file::sandbox::SandboxConfig::default_denied_patterns(),
            strict_mode: false,
        },
    );
    let tool = ReadTool::with_sandbox(sandbox).with_fs(probe.clone());

    let (ticker, ticks) = spawn_ticker().await;
    let before = ticks.load(Ordering::SeqCst);

    let output = tool
        .execute(serde_json::json!({ "file_path": file.display().to_string() }))
        .await
        .expect("read must succeed through the probe world");
    assert!(!output.is_error, "read output: {}", output.content);

    let after = ticks.load(Ordering::SeqCst);
    ticker.abort();

    assert!(
        probe.blocking_calls.load(Ordering::SeqCst) > 0,
        "test must exercise the blocking provider surface"
    );
    let ticks_during = after.saturating_sub(before);
    assert!(
        ticks_during >= MIN_TICKS_DURING_BLOCK,
        "the async worker froze for the duration of the binary-sniff read \
         (only {ticks_during} ticks over {}ms of blocking IO) — §P2-14 regression",
        BLOCKING_CALL_MS
    );
}
