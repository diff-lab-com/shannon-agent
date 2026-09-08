//! P1-5 D — integrated terminal: long-lived PTY sessions + the frozen
//! Tauri command contract.
//!
//! Frozen contract (Tauri commands):
//! `terminal_spawn({projectDir, shell?}) -> {terminalId}`,
//! `terminal_write({terminalId, data})` (stdin bytes as a UTF-8 string),
//! `terminal_resize({terminalId, cols, rows})`,
//! `terminal_kill({terminalId})`,
//! `terminal_list() -> [{terminalId, projectDir, shell, startedAtMs}]`,
//! and the `terminal:output` event (`shannon_types::events::event_names::
//! TERMINAL_OUTPUT`) with payload `{ terminalId, data }` where `data` is
//! the **base64-encoded** raw PTY byte stream (byte-preserving — the
//! frontend decodes before writing to xterm.js).
//!
//! # Process discipline (mirrors `preview_commands.rs`)
//!
//! PTY sessions are owned by the `TerminalManager` on `AppState`:
//!
//! * at most `MAX_TERMINALS` (4) concurrent sessions — a fifth
//!   `terminal_spawn` is rejected with an error;
//! * each child is spawned on its own pty; portable-pty's unix backend
//!   runs `setsid()` + `TIOCSCTTY` in the child, so the shell is a
//!   session/group leader (`pid == pgid`) and `killpg(SIGKILL)` takes down
//!   the whole tree;
//! * the single output-pump thread (≤16 ms tick) coalesces PTY bytes into
//!   at most one `terminal:output` emit per tick per terminal, reaps
//!   naturally exited shells (no phantom entries in `terminal_list`), and
//!   announces the exit in the terminal stream;
//! * `terminal_kill` / `kill_all` (main-window-destroyed hook in
//!   `main.rs`) kill the trees explicitly, and `Drop for TerminalManager`
//!   is the app-exit backstop — the pump thread holds only a `Weak` to the
//!   manager so teardown is never pinned by it;
//! * a per-session reader thread moves pty bytes into a small pending
//!   buffer; it exits on EOF/EIO or when the session is closed.
//!
//! # Security
//!
//! * No new ACL grants: the PTY lives entirely in Rust; the frontend
//!   only talks to the five frozen commands above. No
//!   `tauri-plugin-shell` surface, no telemetry, no network.
//! * The optional `shell` argument is a user choice on the user's own
//!   machine (the terminal is arbitrary-exec by definition — everything
//!   typed into it is user-authored), so unlike the preview dev-server
//!   whitelist it is not restricted to a package-manager list. It is
//!   tokenized with POSIX shell-word rules (quotes respected) and exec'd
//!   directly with cwd pinned to `projectDir`.

use base64::Engine as _;
use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex as StdMutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::Emitter;

/// Maximum concurrent terminal instances (brief: ≤4 per screen).
pub const MAX_TERMINALS: usize = 4;

/// Output coalescing tick: at most one `terminal:output` emit per terminal
/// per tick (brief: ≤16 ms batching).
pub const OUTPUT_TICK: Duration = Duration::from_millis(16);

/// Output backpressure cap per terminal: the pending buffer never holds
/// more than this many unread pty bytes. A flood (`cat bigfile`, `yes`)
/// drops the OLDEST bytes past the cap and the next emit carries an
/// explicit in-stream truncation notice instead of growing without bound.
pub const MAX_PENDING_BYTES: usize = 2 * 1024 * 1024;

/// Maximum bytes per `terminal:output` event: a drained batch is split
/// into consecutive chunks of at most this size so a single emit never
/// ships multi-MB payloads to the webview.
pub const MAX_EMIT_CHUNK: usize = 256 * 1024;

/// Initial pty geometry; corrected by `terminal_resize` once the frontend
/// fit addon measures the panel.
const INITIAL_ROWS: u16 = 24;
const INITIAL_COLS: u16 = 80;

/// Upper bound for resize requests — rejects junk values before they hit
/// the pty ioctl.
const MAX_COLS: u16 = 1024;
const MAX_ROWS: u16 = 1024;

// ── DTOs (wire shape: camelCase, frozen) ─────────────────────────────────

/// `terminal_spawn` response (frozen: `{ terminalId }`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TerminalSpawnResponse {
    pub terminal_id: String,
}

/// One live terminal (`terminal_list` item).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TerminalInfo {
    pub terminal_id: String,
    pub project_dir: String,
    pub shell: String,
    pub started_at_ms: i64,
}

/// `terminal:output` event payload. `data` is base64 of the raw pty bytes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TerminalOutputPayload {
    pub terminal_id: String,
    pub data: String,
}

// ── Shell resolution ─────────────────────────────────────────────────────

/// Default login shell: `$SHELL` on unix (PowerShell on Windows), with a
/// conservative fallback when the env var is unset.
fn resolve_default_shell(shell_env: Option<&str>, is_windows: bool) -> String {
    if is_windows {
        return "powershell.exe".to_string();
    }
    match shell_env {
        Some(s) if !s.trim().is_empty() => s.trim().to_string(),
        _ => "/bin/sh".to_string(),
    }
}

/// Tokenize a shell command line with POSIX word rules (quotes respected)
/// so `/bin/sh -c 'echo a b'` passes `-c` and `echo a b` as two args.
fn tokenize_shell(shell: &str) -> Result<Vec<String>, String> {
    let tokens = shell_words::split(shell).map_err(|e| format!("parsing shell '{shell}': {e}"))?;
    if tokens.is_empty() {
        return Err("shell must not be empty".into());
    }
    Ok(tokens)
}

// ── Output sink (injectable emit boundary) ───────────────────────────────

/// Where coalesced pty bytes go. Production emits the `terminal:output`
/// Tauri event; tests record bytes and count emissions.
pub trait TerminalOutputSink: Send + Sync {
    fn emit_output(&self, terminal_id: &str, data: &[u8]);
}

/// Production sink: base64-encode the coalesced bytes and emit
/// `terminal:output` with `{ terminalId, data }` (frozen payload shape).
pub struct TauriTerminalSink {
    app: tauri::AppHandle,
}

impl TauriTerminalSink {
    pub fn new(app: tauri::AppHandle) -> Self {
        Self { app }
    }
}

impl TerminalOutputSink for TauriTerminalSink {
    fn emit_output(&self, terminal_id: &str, data: &[u8]) {
        let payload = TerminalOutputPayload {
            terminal_id: terminal_id.to_string(),
            data: base64::engine::general_purpose::STANDARD.encode(data),
        };
        if let Err(e) = self
            .app
            .emit(crate::events::event_names::TERMINAL_OUTPUT, payload)
        {
            tracing::debug!(terminal_id, error = %e, "terminal:output emit failed");
        }
    }
}

// ── Session internals ────────────────────────────────────────────────────

/// Pending pty bytes for one session, shared between the reader thread
/// (producer) and the pump (consumer). `closed` makes the reader exit
/// promptly once the session is reaped or killed even if the pty fd stays
/// duplicated in the cloned reader.
struct PendingBuffer {
    bytes: StdMutex<Vec<u8>>,
    closed: AtomicBool,
    /// Backpressure cap (bytes) and how many bytes were dropped oldest-
    /// first when it was exceeded; the next drain prepends an explicit
    /// truncation notice so the user sees the gap instead of silent loss.
    cap: usize,
    dropped: StdMutex<u64>,
}

/// In-stream notice prepended after bytes were dropped (pure ASCII so the
/// frontend's byte-wise marker search and the terminal both render it).
fn truncation_notice(dropped: u64) -> Vec<u8> {
    format!("\r\n\u{1b}[2m[shannon: output truncated — {dropped} bytes dropped]\u{1b}[0m\r\n")
        .into_bytes()
}

impl PendingBuffer {
    fn new() -> Self {
        Self::with_cap(MAX_PENDING_BYTES)
    }

    fn with_cap(cap: usize) -> Self {
        Self {
            bytes: StdMutex::new(Vec::new()),
            closed: AtomicBool::new(false),
            cap,
            dropped: StdMutex::new(0),
        }
    }

    fn push(&self, chunk: &[u8]) {
        if self.closed.load(Ordering::SeqCst) {
            return;
        }
        let Ok(mut buf) = self.bytes.lock() else {
            return;
        };
        if chunk.len() >= self.cap {
            // A single chunk at/over the cap: everything buffered is stale
            // by definition — keep only the chunk's tail.
            let mut dropped = self.dropped.lock().unwrap_or_else(|p| p.into_inner());
            *dropped += (buf.len() + chunk.len() - self.cap) as u64;
            buf.clear();
            let tail_start = chunk.len() - self.cap;
            buf.extend_from_slice(&chunk[tail_start..]);
            return;
        }
        let overflow = (buf.len() + chunk.len()).saturating_sub(self.cap);
        if overflow > 0 {
            let mut dropped = self.dropped.lock().unwrap_or_else(|p| p.into_inner());
            *dropped += overflow as u64;
            buf.drain(..overflow);
        }
        buf.extend_from_slice(chunk);
    }

    /// Take everything buffered so far (coalesced by the caller's tick).
    /// Bytes dropped by backpressure surface as a leading truncation
    /// notice exactly once.
    fn drain(&self) -> Vec<u8> {
        let mut buf = match self.bytes.lock() {
            Ok(mut buf) => std::mem::take(&mut *buf),
            Err(_) => Vec::new(),
        };
        let dropped = std::mem::take(&mut *self.dropped.lock().unwrap_or_else(|p| p.into_inner()));
        if dropped > 0 {
            let mut out = truncation_notice(dropped);
            out.append(&mut buf);
            out
        } else {
            buf
        }
    }

    fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
    }

    /// Buffered size + dropped-so-far (test seam).
    #[cfg(test)]
    fn state(&self) -> (usize, u64) {
        (
            self.bytes.lock().map(|b| b.len()).unwrap_or(0),
            *self.dropped.lock().unwrap_or_else(|p| p.into_inner()),
        )
    }
}

struct TerminalSession {
    info: TerminalInfo,
    pending: Arc<PendingBuffer>,
    writer: StdMutex<Box<dyn std::io::Write + Send>>,
    /// `dyn MasterPty` is Send but not Sync (portable-pty declares no Sync
    /// impl), so the master lives behind a mutex — sessions sit in
    /// `Arc`-land on `AppState`, which demands `Send + Sync`. Only
    /// `resize` touches the master after setup.
    master: StdMutex<Box<dyn MasterPty + Send>>,
    /// `None` once the child has been reaped or killed.
    child: StdMutex<Option<Box<dyn portable_pty::Child + Send + Sync>>>,
}

impl TerminalSession {
    /// Poll the child; `Some((success, exit_code))` when it has exited.
    fn poll_exit(&self) -> Option<(bool, u32)> {
        let mut guard = self.child.lock().unwrap_or_else(|p| p.into_inner());
        let child = guard.as_mut()?;
        child
            .try_wait()
            .ok()
            .flatten()
            .map(|status| (status.success(), status.exit_code()))
    }

    /// Take the child handle out (reap/kill paths drop it afterwards).
    fn take_child(&self) -> Option<Box<dyn portable_pty::Child + Send + Sync>> {
        self.child.lock().unwrap_or_else(|p| p.into_inner()).take()
    }

    fn write_stdin(&self, data: &str) -> Result<(), String> {
        let mut writer = self.writer.lock().unwrap_or_else(|p| p.into_inner());
        writer
            .write_all(data.as_bytes())
            .and_then(|_| writer.flush())
            .map_err(|e| format!("writing to terminal {}: {e}", self.info.terminal_id))
    }
}

/// Kill the whole process group of a session leader (portable-pty's unix
/// backend runs `setsid()` in the child ⇒ `pid == pgid`), falling back to
/// the direct child kill.
fn kill_process_tree(child: &mut Box<dyn portable_pty::Child + Send + Sync>) {
    if let Some(pid) = child.process_id() {
        #[cfg(unix)]
        unsafe {
            // Negative pid → the whole process group.
            if libc::kill(-(pid as i32), libc::SIGKILL) == 0 {
                return;
            }
        }
        #[cfg(not(unix))]
        let _ = pid;
    }
    let _ = child.kill();
}

// ── TerminalManager (lifecycle owner on AppState) ────────────────────────

struct TerminalInner {
    sessions: StdMutex<HashMap<String, Arc<TerminalSession>>>,
    sink: StdMutex<Option<Arc<dyn TerminalOutputSink>>>,
    /// Live pump-task count (0 or 1) — makes retirement observable.
    live_pumps: AtomicUsize,
}

/// Owns every PTY session (process-tree discipline, mirrors
/// [`crate::preview_commands::PreviewManager`]). Held on `AppState` as an
/// `Arc`; dropping it kills all children (app-exit backstop).
pub struct TerminalManager {
    inner: Arc<TerminalInner>,
    /// Test seam: coalescing tick.
    tick: Duration,
    pump_started: AtomicBool,
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// `terminal_list` ordering: oldest first, id as the tie-break so same-ms
/// spawns have a stable order.
fn sort_infos(infos: &mut [TerminalInfo]) {
    infos.sort_by(|a, b| {
        a.started_at_ms
            .cmp(&b.started_at_ms)
            .then(a.terminal_id.cmp(&b.terminal_id))
    });
}

impl Default for TerminalManager {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for TerminalManager {
    fn drop(&mut self) {
        self.kill_all();
    }
}

impl TerminalManager {
    pub fn new() -> Self {
        Self::with_tick(OUTPUT_TICK)
    }

    /// Construct with an explicit coalescing tick (short in tests).
    fn with_tick(tick: Duration) -> Self {
        Self {
            inner: Arc::new(TerminalInner {
                sessions: StdMutex::new(HashMap::new()),
                sink: StdMutex::new(None),
                live_pumps: AtomicUsize::new(0),
            }),
            tick,
            pump_started: AtomicBool::new(false),
        }
    }

    /// Install the production (or test) emit sink. Called from `main.rs`
    /// setup once the `AppHandle` exists.
    pub fn set_sink(&self, sink: Arc<dyn TerminalOutputSink>) {
        *self.inner.sink.lock().unwrap_or_else(|p| p.into_inner()) = Some(sink);
    }

    /// `terminal_spawn({projectDir, shell?})`. The cwd is canonicalized;
    /// the default shell is `$SHELL` (PowerShell on Windows).
    pub fn spawn(&self, project_dir: &Path, shell: Option<String>) -> Result<TerminalInfo, String> {
        let dir = project_dir
            .canonicalize()
            .map_err(|e| format!("project dir {}: {e}", project_dir.display()))?;
        let shell_line = shell.unwrap_or_else(|| {
            resolve_default_shell(std::env::var("SHELL").ok().as_deref(), cfg!(windows))
        });
        let tokens = tokenize_shell(&shell_line)?;
        let (program, args) = tokens.split_first().expect("tokenize_shell rejects empty");

        let mut sessions = self
            .inner
            .sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if sessions.len() >= MAX_TERMINALS {
            return Err(format!(
                "terminal limit reached ({MAX_TERMINALS}) — close a terminal before opening another"
            ));
        }

        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: INITIAL_ROWS,
                cols: INITIAL_COLS,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| format!("opening pty failed: {e}"))?;

        let mut cmd = CommandBuilder::new(program);
        cmd.args(args);
        cmd.cwd(&dir);
        // Programs inspect TERM to decide on colors/escape sequences; a bare
        // shell without it degrades to plain ASCII.
        cmd.env("TERM", "xterm-256color");

        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| format!("spawning '{shell_line}' failed: {e}"))?;
        // Drop our handle on the slave so EOF propagates when the child's
        // side closes (we never use it again after spawn).
        drop(pair.slave);

        let terminal_id = uuid::Uuid::new_v4().to_string();
        let info = TerminalInfo {
            terminal_id: terminal_id.clone(),
            project_dir: dir.display().to_string(),
            shell: shell_line.clone(),
            started_at_ms: now_ms(),
        };

        let writer = pair
            .master
            .take_writer()
            .map_err(|e| format!("taking pty writer failed: {e}"))?;
        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| format!("taking pty reader failed: {e}"))?;

        let pending = Arc::new(PendingBuffer::new());
        spawn_reader(reader, pending.clone());

        let session = Arc::new(TerminalSession {
            pending: pending.clone(),
            writer: StdMutex::new(writer),
            master: StdMutex::new(pair.master),
            child: StdMutex::new(Some(child)),
            info: info.clone(),
        });
        sessions.insert(terminal_id, session.clone());
        drop(sessions);

        self.ensure_pump();
        Ok(info)
    }

    /// `terminal_write({terminalId, data})` — data is a UTF-8 string
    /// (xterm.js `onData` output incl. control/escape bytes).
    pub fn write(&self, terminal_id: &str, data: &str) -> Result<(), String> {
        let session = self.session(terminal_id)?;
        session.write_stdin(data)
    }

    /// `terminal_resize({terminalId, cols, rows})`.
    pub fn resize(&self, terminal_id: &str, cols: u16, rows: u16) -> Result<(), String> {
        if cols == 0 || rows == 0 || cols > MAX_COLS || rows > MAX_ROWS {
            return Err(format!("invalid terminal size {cols}x{rows}"));
        }
        let session = self.session(terminal_id)?;
        session
            .master
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| format!("resizing terminal {terminal_id}: {e}"))
    }

    /// `terminal_kill({terminalId})` — kill the process tree and retire the
    /// session. Repeat kills of an unknown id are an error (the frontend
    /// treats it as already-closed).
    pub fn kill(&self, terminal_id: &str) -> Result<TerminalInfo, String> {
        let session = {
            let mut sessions = self
                .inner
                .sessions
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            sessions.remove(terminal_id)
        }
        .ok_or_else(|| format!("no such terminal: {terminal_id}"))?;
        session.pending.close();
        if let Some(mut child) = session.take_child() {
            kill_process_tree(&mut child);
        }
        Ok(session.info.clone())
    }

    /// `terminal_list()` — live sessions, oldest first.
    pub fn list(&self) -> Vec<TerminalInfo> {
        let sessions = self
            .inner
            .sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let mut infos: Vec<TerminalInfo> = sessions.values().map(|s| s.info.clone()).collect();
        sort_infos(&mut infos);
        infos
    }

    /// Kill every session (app-exit hook / `Drop` backstop).
    pub fn kill_all(&self) {
        let removed: Vec<Arc<TerminalSession>> = {
            let mut sessions = self
                .inner
                .sessions
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            sessions.drain().map(|(_, session)| session).collect()
        };
        for session in removed {
            session.pending.close();
            if let Some(mut child) = session.take_child() {
                kill_process_tree(&mut child);
            }
        }
    }

    fn session(&self, terminal_id: &str) -> Result<Arc<TerminalSession>, String> {
        self.inner
            .sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(terminal_id)
            .cloned()
            .ok_or_else(|| format!("no such terminal: {terminal_id}"))
    }

    /// Spawn the single output pump (once per manager). The pump holds only
    /// a `Weak` to the inner state so manager teardown is never pinned.
    fn ensure_pump(&self) {
        if self.pump_started.swap(true, Ordering::SeqCst) {
            return;
        }
        let weak = Arc::downgrade(&self.inner);
        let tick = self.tick;
        self.inner.live_pumps.fetch_add(1, Ordering::SeqCst);
        std::thread::Builder::new()
            .name("terminal-output-pump".into())
            .spawn(move || {
                loop {
                    if weak.strong_count() == 0 {
                        break;
                    }
                    std::thread::sleep(tick);
                    let Some(inner) = weak.upgrade() else {
                        break;
                    };
                    pump_once(&inner);
                }
                // Mirror PreviewManager's WatcherGuard: retirement must be
                // observable on every exit path.
                if let Some(inner) = weak.upgrade() {
                    inner.live_pumps.fetch_sub(1, Ordering::SeqCst);
                }
            })
            .expect("spawning terminal output pump");
    }

    /// Live pump count (test seam for retirement regressions).
    #[cfg(test)]
    fn live_pumps(&self) -> usize {
        self.inner.live_pumps.load(Ordering::SeqCst)
    }

    /// One synchronous coalescing pass (test seam — the real pump loops
    /// this on its tick).
    #[cfg(test)]
    fn pump_once_for_test(&self) {
        pump_once(&self.inner);
    }

    /// Seed a session's pending buffer (test seam — reader equivalent).
    #[cfg(test)]
    fn test_push_pending(&self, terminal_id: &str, bytes: &[u8]) {
        if let Ok(s) = self.session(terminal_id) {
            s.pending.push(bytes);
        }
    }
}

/// One pump pass over every live session: coalesced emit + exit reaping.
///
/// Exit discipline (task-2 style: no phantom running): when the child has
/// exited we flush the remaining output plus a final notice line, then
/// remove the session so `terminal_list` stays truthful. The frontend tab
/// shows the notice until the user closes it (kill on a reaped id is a
/// clean "no such terminal" error).
fn pump_once(inner: &TerminalInner) {
    let sink = match inner.sink.lock().unwrap_or_else(|p| p.into_inner()).clone() {
        Some(sink) => sink,
        None => {
            // No sink attached (yet): still reap exits so the list stays
            // truthful, but drop the bytes.
            reap_exits_without_sink(inner);
            return;
        }
    };
    let snapshot: Vec<Arc<TerminalSession>> = {
        let sessions = inner.sessions.lock().unwrap_or_else(|p| p.into_inner());
        sessions.values().cloned().collect()
    };
    for session in snapshot {
        let drained = session.pending.drain();
        if let Some((success, code)) = session.poll_exit() {
            let mut final_bytes = drained;
            final_bytes.extend_from_slice(
                format!(
                    "\r\n\u{1b}[2m[shannon: process exited — {}]\u{1b}[0m\r\n",
                    if success {
                        "done".to_string()
                    } else {
                        format!("exit code {code}")
                    }
                )
                .as_bytes(),
            );
            emit_chunked(&sink, &session.info.terminal_id, &final_bytes);
            // Retire: close the reader, drop the child handle, remove the
            // session. The take_child here is just prompt cleanup — the
            // process already exited, so no kill signal is needed.
            session.pending.close();
            let _ = session.take_child();
            inner
                .sessions
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .remove(&session.info.terminal_id);
        } else if !drained.is_empty() {
            emit_chunked(&sink, &session.info.terminal_id, &drained);
        }
    }
}

/// Emit a drained batch as consecutive events of at most
/// [`MAX_EMIT_CHUNK`] bytes — one tick's flood must never become a
/// multi-MB webview payload.
fn emit_chunked(sink: &Arc<dyn TerminalOutputSink>, terminal_id: &str, bytes: &[u8]) {
    for chunk in bytes.chunks(MAX_EMIT_CHUNK) {
        sink.emit_output(terminal_id, chunk);
    }
}

/// Exit reaping when no sink is attached (e.g. the window closed and the
/// app is tearing down): same removal discipline, bytes discarded.
fn reap_exits_without_sink(inner: &TerminalInner) {
    let snapshot: Vec<Arc<TerminalSession>> = {
        let sessions = inner.sessions.lock().unwrap_or_else(|p| p.into_inner());
        sessions.values().cloned().collect()
    };
    for session in snapshot {
        if session.poll_exit().is_some() {
            session.pending.close();
            let _ = session.take_child();
            inner
                .sessions
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .remove(&session.info.terminal_id);
        }
    }
}

/// Reader thread: pty bytes → pending buffer. Exits on EOF/EIO, any read
/// error, or when the session is closed. Holds only the pending buffer —
/// never the manager or the session — so teardown is never pinned.
fn spawn_reader(reader: Box<dyn Read + Send>, pending: Arc<PendingBuffer>) {
    std::thread::Builder::new()
        .name("terminal-pty-reader".into())
        .spawn(move || {
            let mut reader = reader;
            let mut buf = [0u8; 8192];
            loop {
                if pending.closed.load(Ordering::SeqCst) {
                    break;
                }
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => pending.push(&buf[..n]),
                    Err(_) => break,
                }
            }
            pending.close();
        })
        .expect("spawning terminal pty reader");
}

// ── Tauri commands (frozen contract) ─────────────────────────────────────

/// `terminal_spawn({projectDir, shell?}) -> { terminalId }`. `projectDir`
/// may be omitted: the backend then resolves the current session working
/// directory (same fallback as `preview_*`).
#[tauri::command]
pub async fn terminal_spawn(
    state: tauri::State<'_, crate::commands::AppState>,
    project_dir: Option<String>,
    shell: Option<String>,
) -> Result<TerminalSpawnResponse, String> {
    let dir = match project_dir {
        Some(dir) if !dir.trim().is_empty() => PathBuf::from(dir),
        _ => crate::commands_agents::resolve_working_dir(&state).await,
    };
    let info = state.terminals.spawn(&dir, shell)?;
    Ok(TerminalSpawnResponse {
        terminal_id: info.terminal_id,
    })
}

/// `terminal_write({terminalId, data})` — stdin (keystrokes, paste).
#[tauri::command]
pub async fn terminal_write(
    state: tauri::State<'_, crate::commands::AppState>,
    terminal_id: String,
    data: String,
) -> Result<(), String> {
    state.terminals.write(&terminal_id, &data)
}

/// `terminal_resize({terminalId, cols, rows})`.
#[tauri::command]
pub async fn terminal_resize(
    state: tauri::State<'_, crate::commands::AppState>,
    terminal_id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    state.terminals.resize(&terminal_id, cols, rows)
}

/// `terminal_kill({terminalId})`.
#[tauri::command]
pub async fn terminal_kill(
    state: tauri::State<'_, crate::commands::AppState>,
    terminal_id: String,
) -> Result<TerminalInfo, String> {
    state.terminals.kill(&terminal_id)
}

/// `terminal_list() -> [{ terminalId, projectDir, shell, startedAtMs }]`.
#[tauri::command]
pub async fn terminal_list(
    state: tauri::State<'_, crate::commands::AppState>,
) -> Result<Vec<TerminalInfo>, String> {
    Ok(state.terminals.list())
}

/// Install the production `terminal:output` sink on the manager. Called
/// from `main.rs` setup once the `AppHandle` exists (the manager itself is
/// created in `AppState::new` before Tauri is up). `pub(crate)`-field
/// access from a lib function keeps the bin's surface tiny — same pattern
/// as [`shutdown_on_exit`].
pub fn attach_sink(state: &crate::commands::AppState, app: tauri::AppHandle) {
    state
        .terminals
        .set_sink(std::sync::Arc::new(TauriTerminalSink::new(app)));
}

/// Sync shutdown for the main-window-destroyed hook in `main.rs` (mirrors
/// `preview_commands::shutdown_on_exit`): kill every PTY process tree.
pub fn shutdown_on_exit(state: &crate::commands::AppState) {
    state.terminals.kill_all();
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    /// Short coalescing tick so pump-driven tests stay fast.
    const TEST_TICK: Duration = Duration::from_millis(10);
    const WAIT: Duration = Duration::from_secs(5);

    /// Records every emission (id, bytes) in order; the test asserts on
    /// both delivery and coalescing.
    type Emissions = Arc<StdMutex<Vec<(String, Vec<u8>)>>>;

    #[derive(Default, Clone)]
    struct RecordingSink {
        emissions: Emissions,
    }

    impl RecordingSink {
        fn joined(&self, terminal_id: &str) -> Vec<u8> {
            self.emissions
                .lock()
                .unwrap()
                .iter()
                .filter(|(id, _)| id == terminal_id)
                .flat_map(|(_, bytes)| bytes.iter().copied())
                .collect()
        }

        fn count_for(&self, terminal_id: &str) -> usize {
            self.emissions
                .lock()
                .unwrap()
                .iter()
                .filter(|(id, _)| id == terminal_id)
                .count()
        }

        fn contains(&self, terminal_id: &str, needle: &str) -> bool {
            let hay = self.joined(terminal_id);
            let needle = needle.as_bytes();
            hay.windows(needle.len()).any(|w| w == needle)
        }

        fn wait_for(&self, terminal_id: &str, needle: &str, deadline: Duration) -> bool {
            let start = Instant::now();
            while start.elapsed() < deadline {
                if self.contains(terminal_id, needle) {
                    return true;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            self.contains(terminal_id, needle)
        }
    }

    impl TerminalOutputSink for RecordingSink {
        fn emit_output(&self, terminal_id: &str, data: &[u8]) {
            self.emissions
                .lock()
                .unwrap()
                .push((terminal_id.to_string(), data.to_vec()));
        }
    }

    fn test_manager(sink: &RecordingSink) -> TerminalManager {
        let manager = TerminalManager::with_tick(TEST_TICK);
        manager.set_sink(Arc::new(sink.clone()));
        manager
    }

    /// True when `pid` is gone or a reaped-pending zombie (mirrors the
    /// preview_commands helper).
    #[cfg(unix)]
    fn process_gone(pid: u32) -> bool {
        match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
            Err(_) => unsafe { libc::kill(pid as i32, 0) != 0 },
            Ok(stat) => stat
                .split_once(')')
                .and_then(|(_, rest)| rest.trim_start().chars().next())
                .is_some_and(|state| state == 'Z'),
        }
    }

    #[cfg(unix)]
    fn wait_until(deadline: Duration, mut cond: impl FnMut() -> bool) -> bool {
        let start = Instant::now();
        while start.elapsed() < deadline {
            if cond() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        cond()
    }

    fn pid_of(manager: &TerminalManager, terminal_id: &str) -> Option<u32> {
        manager.session(terminal_id).ok().and_then(|s| {
            s.child
                .lock()
                .unwrap()
                .as_ref()
                .and_then(|c| c.process_id())
        })
    }

    // ── DTO shapes (frozen contract) ─────────────────────────────────────

    #[test]
    fn spawn_response_is_frozen_camel_case() {
        let json = serde_json::to_string(&TerminalSpawnResponse {
            terminal_id: "t-1".into(),
        })
        .unwrap();
        assert_eq!(json, r#"{"terminalId":"t-1"}"#);
    }

    #[test]
    fn list_item_is_frozen_camel_case() {
        let info = TerminalInfo {
            terminal_id: "t-1".into(),
            project_dir: "/home/u/proj".into(),
            shell: "/bin/bash".into(),
            started_at_ms: 1_700,
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"terminalId\":\"t-1\""), "{json}");
        assert!(json.contains("\"projectDir\":\"/home/u/proj\""), "{json}");
        assert!(json.contains("\"shell\":\"/bin/bash\""), "{json}");
        assert!(json.contains("\"startedAtMs\":1700"), "{json}");
        let back: TerminalInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(back, info);
    }

    #[test]
    fn output_payload_is_base64_and_byte_preserving() {
        // Non-UTF8 bytes (0xFF 0xFE) must survive the wire: base64, not
        // lossy UTF-8 — that is why the contract freezes base64.
        let raw: Vec<u8> = vec![0xFF, 0xFE, 0x00, 0x1B, b'[', b'A', 0x80];
        let payload = TerminalOutputPayload {
            terminal_id: "t-9".into(),
            data: base64::engine::general_purpose::STANDARD.encode(&raw),
        };
        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains("\"terminalId\":\"t-9\""), "{json}");
        assert!(json.contains("\"data\":\""), "{json}");
        let back: TerminalOutputPayload = serde_json::from_str(&json).unwrap();
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(&back.data)
            .unwrap();
        assert_eq!(decoded, raw);
    }

    // ── Shell resolution (pure) ──────────────────────────────────────────

    #[test]
    fn default_shell_prefers_env_with_fallbacks() {
        assert_eq!(
            resolve_default_shell(Some("/usr/bin/zsh"), false),
            "/usr/bin/zsh"
        );
        assert_eq!(resolve_default_shell(None, false), "/bin/sh");
        assert_eq!(resolve_default_shell(Some("  "), false), "/bin/sh");
        // Windows always gets PowerShell regardless of $SHELL.
        assert_eq!(
            resolve_default_shell(Some("/usr/bin/zsh"), true),
            "powershell.exe"
        );
    }

    #[test]
    fn shell_string_supports_arguments_and_quotes() {
        let tokens = tokenize_shell("/bin/sh -c 'echo a b'").unwrap();
        assert_eq!(tokens, vec!["/bin/sh", "-c", "echo a b"]);
        assert!(tokenize_shell("").is_err());
        assert!(tokenize_shell("   ").is_err());
        // Unbalanced quote → explicit error, never a silent mis-split.
        assert!(tokenize_shell("/bin/sh -c 'oops").is_err());
    }

    // ── Lifecycle (unix: real pty + /bin/sh) ─────────────────────────────

    #[cfg(unix)]
    #[test]
    fn spawn_write_and_output_flows_to_sink() {
        let sink = RecordingSink::default();
        let manager = test_manager(&sink);
        let dir = tempfile::tempdir().expect("tempdir");
        let info = manager
            .spawn(dir.path(), Some("/bin/sh".into()))
            .expect("spawn");
        assert_eq!(
            info.project_dir,
            dir.path().canonicalize().unwrap().display().to_string()
        );
        assert!(
            manager
                .list()
                .iter()
                .any(|t| t.terminal_id == info.terminal_id)
        );

        manager
            .write(&info.terminal_id, "echo pty-marker-$((20+3))\n")
            .expect("write");
        assert!(
            sink.wait_for(&info.terminal_id, "pty-marker-23", WAIT),
            "output must reach the sink through reader → pending → pump, got: {:?}",
            String::from_utf8_lossy(&sink.joined(&info.terminal_id))
        );
        manager.kill(&info.terminal_id).expect("kill");
    }

    #[cfg(unix)]
    #[test]
    fn output_is_coalesced_into_one_emit_per_tick() {
        let sink = RecordingSink::default();
        let manager = test_manager(&sink);
        let dir = tempfile::tempdir().expect("tempdir");
        let info = manager
            .spawn(dir.path(), Some("/bin/sh".into()))
            .expect("spawn");
        // Direct pending seeding (reader equivalent) + a synchronous pump
        // pass: five bursts within one tick must leave as ONE emission.
        for i in 0..5 {
            manager.test_push_pending(&info.terminal_id, format!("burst-{i}\r\n").as_bytes());
        }
        manager.pump_once_for_test();
        assert_eq!(sink.count_for(&info.terminal_id), 1, "one emit per tick");
        let joined = sink.joined(&info.terminal_id);
        for i in 0..5 {
            let needle = format!("burst-{i}\r\n").into_bytes();
            assert!(
                joined.windows(needle.len()).any(|w| w == needle.as_slice()),
                "burst-{i} must be inside the single emission"
            );
        }
        // A second pass with nothing buffered emits nothing.
        manager.pump_once_for_test();
        assert_eq!(sink.count_for(&info.terminal_id), 1);
        manager.kill(&info.terminal_id).expect("kill");
    }

    #[cfg(unix)]
    #[test]
    fn resize_reaches_the_shell() {
        let sink = RecordingSink::default();
        let manager = test_manager(&sink);
        let dir = tempfile::tempdir().expect("tempdir");
        let info = manager
            .spawn(dir.path(), Some("/bin/sh".into()))
            .expect("spawn");
        manager.resize(&info.terminal_id, 100, 30).expect("resize");
        // `stty size` reports "<rows> <cols>" from the pty winsize.
        manager
            .write(&info.terminal_id, "stty size\n")
            .expect("write");
        assert!(
            sink.wait_for(&info.terminal_id, "30 100", WAIT),
            "resize must update the pty winsize, got: {:?}",
            String::from_utf8_lossy(&sink.joined(&info.terminal_id))
        );
        // Junk sizes are rejected before the ioctl.
        assert!(manager.resize(&info.terminal_id, 0, 30).is_err());
        assert!(manager.resize(&info.terminal_id, u16::MAX, 1).is_err());
        manager.kill(&info.terminal_id).expect("kill");
    }

    #[cfg(unix)]
    #[test]
    fn kill_stops_the_tree_and_clears_the_slot() {
        let sink = RecordingSink::default();
        let manager = test_manager(&sink);
        // `sleep 30` runs as a child of the sh session leader — killing the
        // group must take both down.
        let dir = tempfile::tempdir().expect("tempdir");
        let info = manager
            .spawn(dir.path(), Some("/bin/sh -c 'sleep 30'".into()))
            .expect("spawn");
        let pid = pid_of(&manager, &info.terminal_id).expect("pid");
        let killed = manager.kill(&info.terminal_id).expect("kill");
        assert_eq!(killed.terminal_id, info.terminal_id);
        assert!(wait_until(WAIT, || process_gone(pid)), "process must die");
        assert!(manager.list().is_empty(), "no phantom entries after kill");
        // Unknown id on every mutating command → explicit error.
        assert!(manager.kill(&info.terminal_id).is_err());
        assert!(manager.write(&info.terminal_id, "x").is_err());
        assert!(manager.resize(&info.terminal_id, 80, 24).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn natural_exit_is_reaped_and_announced() {
        let sink = RecordingSink::default();
        let manager = test_manager(&sink);
        let dir = tempfile::tempdir().expect("tempdir");
        let info = manager
            .spawn(dir.path(), Some("/bin/sh -c 'echo bye-now'".into()))
            .expect("spawn");
        assert!(sink.wait_for(&info.terminal_id, "bye-now", WAIT));
        // The pump must reap the exited session: list empties and the
        // stream carries the exit notice.
        assert!(
            wait_until(WAIT, || manager.list().is_empty()),
            "exited session must be reaped from the list"
        );
        assert!(
            sink.contains(&info.terminal_id, "process exited"),
            "exit must be announced in the stream"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_fifth_terminal_is_rejected() {
        let sink = RecordingSink::default();
        let manager = test_manager(&sink);
        let dir = tempfile::tempdir().expect("tempdir");
        let mut ids = Vec::new();
        for _ in 0..MAX_TERMINALS {
            let info = manager
                .spawn(dir.path(), Some("/bin/sh -c 'sleep 30'".into()))
                .expect("spawn");
            ids.push(info.terminal_id);
        }
        assert_eq!(manager.list().len(), MAX_TERMINALS);
        let err = manager
            .spawn(dir.path(), Some("/bin/sh -c 'sleep 30'".into()))
            .unwrap_err();
        assert!(err.contains("terminal limit reached (4)"), "{err}");
        manager.kill_all();
        assert!(manager.list().is_empty());
        for id in &ids {
            assert!(
                ids.iter().filter(|other| *other == id).count() == 1,
                "ids are unique"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn kill_all_shuts_every_session_down() {
        let sink = RecordingSink::default();
        let manager = test_manager(&sink);
        let dir = tempfile::tempdir().expect("tempdir");
        let mut pids = Vec::new();
        for _ in 0..2 {
            let info = manager
                .spawn(dir.path(), Some("/bin/sh -c 'sleep 30'".into()))
                .expect("spawn");
            pids.push(pid_of(&manager, &info.terminal_id).expect("pid"));
        }
        manager.kill_all();
        assert!(manager.list().is_empty());
        for pid in pids {
            assert!(wait_until(WAIT, || process_gone(pid)));
        }
    }

    #[cfg(unix)]
    #[test]
    fn dropping_the_manager_kills_children_and_retires_the_pump() {
        let sink = RecordingSink::default();
        let manager = test_manager(&sink);
        let dir = tempfile::tempdir().expect("tempdir");
        let info = manager
            .spawn(dir.path(), Some("/bin/sh -c 'sleep 30'".into()))
            .expect("spawn");
        let pid = pid_of(&manager, &info.terminal_id).expect("pid");
        assert_eq!(manager.live_pumps(), 1, "exactly one pump thread");
        drop(manager);
        assert!(
            wait_until(WAIT, || process_gone(pid)),
            "drop is a kill backstop"
        );
        let manager = test_manager(&sink);
        assert!(
            wait_until(WAIT, || manager.live_pumps() == 0),
            "the dropped manager's pump must retire"
        );
    }

    #[cfg(unix)]
    #[test]
    fn bursts_are_delivered_intact_through_the_real_pipeline() {
        let sink = RecordingSink::default();
        let manager = test_manager(&sink);
        let dir = tempfile::tempdir().expect("tempdir");
        let info = manager
            .spawn(dir.path(), Some("/bin/sh".into()))
            .expect("spawn");
        // Five rapid writes inside one tick window — however the pump
        // coalesces them, every byte must arrive exactly once in order.
        for i in 0..5 {
            manager
                .write(&info.terminal_id, &format!("echo burst-{i}\n"))
                .expect("write");
        }
        assert!(sink.wait_for(&info.terminal_id, "burst-4", WAIT));
        let text = String::from_utf8_lossy(&sink.joined(&info.terminal_id)).to_string();
        // A pty echoes typed input (line discipline), so each marker shows
        // up at least twice (echo + command output) — the guarantee under
        // test is delivery, not de-duplication.
        for i in 0..5 {
            assert!(
                text.contains(&format!("burst-{i}")),
                "burst-{i} lost, got: {text}"
            );
        }
        // …and the shell must have consumed the echo'd prompt marker too.
        assert!(
            text.contains('$'),
            "interactive shell prompt visible: {text}"
        );
        manager.kill(&info.terminal_id).expect("kill");
    }

    #[test]
    fn spawn_rejects_a_missing_project_dir() {
        let sink = RecordingSink::default();
        let manager = test_manager(&sink);
        let err = manager
            .spawn(&PathBuf::from("/nonexistent/dir/for/terminal"), None)
            .unwrap_err();
        assert!(err.contains("project dir"), "{err}");
    }

    // ── Backpressure (fix round 1) ───────────────────────────────────────

    #[test]
    fn pending_buffer_caps_and_marks_the_overflow() {
        let buffer = PendingBuffer::with_cap(64);
        buffer.push(&[b'a'; 50]);
        buffer.push(&[b'b'; 50]);
        // Cap held, oldest 36 bytes dropped.
        let (len, dropped) = buffer.state();
        assert_eq!(len, 64);
        assert_eq!(dropped, 36);

        let drained = buffer.drain();
        let text = String::from_utf8_lossy(&drained);
        assert!(text.starts_with('\r'), "marker starts on a fresh line");
        assert!(text.contains("output truncated"), "{text}");
        assert!(text.contains("36 bytes dropped"), "{text}");
        // Retained bytes are the TAIL of the stream (newest wins).
        assert!(text.ends_with(&"b".repeat(50)), "newest bytes kept: {text}");

        // The notice is emitted exactly once; the next drain is clean.
        let (len, dropped) = buffer.state();
        assert_eq!((len, dropped), (0, 0));
        assert!(buffer.drain().is_empty());
    }

    #[test]
    fn single_oversized_chunk_keeps_its_tail() {
        let buffer = PendingBuffer::with_cap(64);
        buffer.push(&[b'x'; 10]);
        buffer.push(&[b'y'; 100]);
        let (len, dropped) = buffer.state();
        assert_eq!(len, 64);
        // 10 buffered + 100 - 64 kept = 46 dropped.
        assert_eq!(dropped, 46);
        let drained = buffer.drain();
        let text = String::from_utf8_lossy(&drained);
        assert!(text.contains("46 bytes dropped"), "{text}");
        assert!(text.ends_with(&"y".repeat(64)));
    }

    #[cfg(unix)]
    #[test]
    fn flood_is_capped_and_emits_are_chunked() {
        let sink = RecordingSink::default();
        let manager = test_manager(&sink);
        let dir = tempfile::tempdir().expect("tempdir");
        let info = manager
            .spawn(dir.path(), Some("/bin/sh -c 'sleep 30'".into()))
            .expect("spawn");
        // 3 MiB in a single push: the 2 MiB cap drops the oldest 1 MiB and
        // the pump must ship the retained 2 MiB as ≤256 KiB events.
        let flood = vec![b'f'; 3 * 1024 * 1024];
        manager.test_push_pending(&info.terminal_id, &flood);
        manager.pump_once_for_test();

        let emissions = sink.emissions.lock().unwrap();
        assert!(!emissions.is_empty());
        let total: usize = emissions.iter().map(|(_, bytes)| bytes.len()).sum();
        for (id, bytes) in emissions.iter() {
            assert_eq!(id, &info.terminal_id);
            assert!(
                bytes.len() <= MAX_EMIT_CHUNK,
                "every event must be ≤ MAX_EMIT_CHUNK, got {}",
                bytes.len()
            );
        }
        // Retained cap (2 MiB) + the in-stream truncation notice + whatever
        // idle prompt bytes the shell may have printed — never the full 3 MiB.
        assert!(total < 3 * 1024 * 1024, "flood must be capped, got {total}");
        assert!(total >= MAX_PENDING_BYTES);
        let head = String::from_utf8_lossy(&emissions[0].1);
        assert!(
            head.contains("output truncated") && head.contains("bytes dropped"),
            "truncation notice must lead the emit stream: {head}"
        );
        manager.kill(&info.terminal_id).expect("kill");
    }

    #[test]
    fn list_ordering_is_oldest_first_with_id_tiebreak() {
        let make = |id: &str, ms: i64| TerminalInfo {
            terminal_id: id.into(),
            project_dir: "/tmp".into(),
            shell: "/bin/sh".into(),
            started_at_ms: ms,
        };
        let mut infos = vec![
            make("b", 200),
            make("a", 200),
            make("d", 100),
            make("c", 300),
        ];
        sort_infos(&mut infos);
        let order: Vec<&str> = infos.iter().map(|i| i.terminal_id.as_str()).collect();
        assert_eq!(order, vec!["d", "a", "b", "c"]);
    }
}
