//! P1-5 D — integrated terminal: long-lived PTY sessions + the frozen
//! Tauri command contract.
//!
//! Frozen contract (Tauri commands):
//! `terminal_spawn({projectDir, shell?}) -> {terminalId}`,
//! `terminal_write({terminalId, data})` (stdin bytes as a UTF-8 string),
//! `terminal_resize({terminalId, cols, rows})`,
//! `terminal_kill({terminalId})`,
//! `terminal_list() -> [{terminalId, projectDir, projectDirRaw?, shell,
//! startedAtMs, spawnedByWindow?}]`,
//! the `terminal:output` event (`shannon_types::events::event_names::
//! TERMINAL_OUTPUT`) with payload `{ terminalId, data, seq }` where `data`
//! is the **base64-encoded** raw PTY byte stream (byte-preserving — the
//! frontend decodes before writing to xterm.js) and `seq` is a per-session
//! monotonic chunk number (review fix — see [`TerminalOutputPayload`]),
//! and — additive, P3-6 —
//! the `terminal:exit` event (`TERMINAL_EXIT`, `{ terminalId }`) emitted
//! when a session is reaped after a natural exit.
//!
//! Additive settings surface (P3-1): `terminal_get_settings()` /
//! `terminal_set_settings(TerminalSettingsDto) -> TerminalSettingsDto`
//! (camelCase `{ shell, fontSize, scrollback, drawerHeight,
//! screenReaderMode }`), persisted under the `[terminal]` table of
//! `~/.shannon/config.toml`. `shell` participates in the spawn-shell
//! precedence: explicit `shell` arg > configured `[terminal].shell` >
//! `$SHELL` > `/bin/sh` (PowerShell on Windows).
//!
//! Additive replay surface (US6): `terminal_history({terminalId}) ->
//! { data, endSeq }` returns the base64 of the session's newest
//! [`TERMINAL_HISTORY_CAP`] raw output bytes (in-memory ring only — it
//! dies with the session; unknown id → empty string, not an error) plus
//! `endSeq`, the highest output-chunk seq fully contained in the returned
//! snapshot — the stitch key that lets the frontend drop the events it
//! already replayed (review fix, see [`TerminalHistoryResponse`]).
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
//!   naturally exited shells (no phantom entries in `terminal_list`),
//!   announces the exit in the terminal stream, and emits the
//!   `terminal:exit` event as the machine-readable exit signal;
//! * `terminal_kill` / `kill_all` (main-window-destroyed hook in
//!   `main.rs`) kill the trees explicitly — as does `kill_for_window`
//!   (P3-2: every `session-*` window's destroyed hook reaps the sessions
//!   its terminal panel spawned, via the additive `spawnedByWindow`
//!   attribution on `TerminalInfo`) — and `Drop for TerminalManager`
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
use std::collections::{HashMap, VecDeque};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex as StdMutex,
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
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

/// Per-session replay-history cap (US6 / plan §5 Task 3.2): the pump
/// retains the newest this-many raw bytes of each session's output so a
/// re-opened panel can replay the scrollback. Purely in-memory — the ring
/// dies with its session (kill/reap drops it), nothing is persisted.
pub const TERMINAL_HISTORY_CAP: usize = 1024 * 1024;

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
    /// Review fix (per-project tab filter): the project dir EXACTLY as the
    /// spawner requested it, captured before [`TerminalManager::spawn`]
    /// canonicalizes it into `project_dir`. The frontend's per-project
    /// filter matches the raw prop against this field first — a
    /// canonical-vs-raw mismatch (symlinked path segments on Unix,
    /// `\\?\C:\…` verbatim prefixes on Windows) previously made a freshly
    /// spawned tab vanish into the empty state. Always `Some` from `spawn`
    /// (like-for-like with the raw prop); `#[serde(default)]` keeps
    /// pre-existing wire payloads deserializable.
    #[serde(default)]
    pub project_dir_raw: Option<String>,
    pub shell: String,
    pub started_at_ms: i64,
    /// P3-2 window attribution: label of the webview window whose terminal
    /// panel spawned this session (`main` or `session-<uuid>`), used by
    /// `kill_for_window` when a session window is destroyed. Additive
    /// field — `#[serde(default)]` keeps pre-existing wire payloads
    /// (which never carried it) deserializable.
    #[serde(default)]
    pub spawned_by_window: Option<String>,
}

/// `terminal:exit` event payload (P3-6, frozen: `{ terminalId }`).
/// Emitted exactly once when the pump reaps a naturally exited session;
/// the in-stream exit notice stays for humans.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TerminalExitPayload {
    pub terminal_id: String,
}

/// `terminal:output` event payload. `data` is base64 of the raw pty bytes.
/// `seq` (review fix, replay-ordering races) is a per-session monotonic
/// counter — every emit CHUNK gets its own value, assigned in stream
/// order. The frontend stitches `terminal_history`'s snapshot (which
/// covers everything up to its `endSeq`) with the queued events whose
/// `seq > endSeq`, closing both the loss window and the duplication
/// window around (re)connect. `#[serde(default)]` so payloads emitted
/// before the field existed (frontend fixtures, mock events) keep parsing.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TerminalOutputPayload {
    pub terminal_id: String,
    pub data: String,
    #[serde(default)]
    pub seq: u64,
}

/// `terminal_history` response — base64 of the session's retained replay
/// bytes, oldest-retained first (plain stream order), plus `endSeq`: the
/// highest output-chunk seq fully contained in `data` (see
/// [`TerminalOutputPayload`]). Empty string when the id is unknown or the
/// session already ended: the frontend calls it speculatively on
/// reconnect, so a missing ring must not be an error.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TerminalHistoryResponse {
    pub data: String,
    #[serde(default)]
    pub end_seq: u64,
}

// ── Shell resolution ─────────────────────────────────────────────────────

/// Login shell for a new session (P3-1 precedence, pure):
///
/// 1. explicit `shell` argument — handled by the caller ([`TerminalManager::
///    spawn`] receives it pre-separated as `shell`);
/// 2. `configured` — the persisted `[terminal].shell`
///    ([`TerminalSettings`]), trimmed; blank = unset;
/// 3. `$SHELL` env (unix only — Windows ignores it, as before);
/// 4. platform fallback: `/bin/sh` (PowerShell on Windows).
fn resolve_default_shell(
    configured: Option<&str>,
    shell_env: Option<&str>,
    is_windows: bool,
) -> String {
    if let Some(configured) = configured.map(str::trim).filter(|s| !s.is_empty()) {
        return configured.to_string();
    }
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

// ── Terminal settings (P3-1, `[terminal]` in ~/.shannon/config.toml) ─────

/// Lowest / highest accepted `font_size` (clamped, not rejected — a junk
/// value must never wedge the settings command).
const MIN_FONT_SIZE: u32 = 8;
const MAX_FONT_SIZE: u32 = 32;
/// Highest accepted `scrollback` lines (0 = frontend disables scrollback).
const MAX_SCROLLBACK: u32 = 100_000;
/// Accepted `drawer_height` px range (clamped).
const MIN_DRAWER_HEIGHT: u32 = 120;
const MAX_DRAWER_HEIGHT: u32 = 1200;

/// Persisted terminal preferences (P3-1). Lives under the `[terminal]`
/// table of `~/.shannon/config.toml` — the *global* Shannon config the
/// engine's `ConfigBuilder::load_global_toml` and `/config set` also
/// read/write (same file as `[notifications.webhook]`) — NOT the desktop's
/// own `~/.shannon/desktop/config.json`.
///
/// Wire shape: exposed to the frontend through [`TerminalSettingsDto`]
/// (camelCase); this struct's snake_case keys are the on-disk TOML keys.
///
/// Missing keys (or a missing `[terminal]` table, or no config file at
/// all) fall back to the defaults below via the container-level
/// `#[serde(default)]` — hand-edited configs degrade, never error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TerminalSettings {
    /// Shell override applied to newly spawned sessions (P3-1 precedence:
    /// explicit `shell` arg > this > `$SHELL` > platform default). Blank /
    /// missing = unset. Skipped when unset because TOML has no null.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shell: Option<String>,
    /// Font size (px) for xterm.js. Default 12.
    pub font_size: u32,
    /// Scrollback lines kept by xterm.js. Default 5000.
    pub scrollback: u32,
    /// Terminal drawer height (px). Default 320.
    pub drawer_height: u32,
    /// xterm.js screen-reader mode (Task 2.2 accessibility). Default off.
    pub screen_reader_mode: bool,
}

impl Default for TerminalSettings {
    fn default() -> Self {
        Self {
            shell: None,
            font_size: 12,
            scrollback: 5000,
            drawer_height: 320,
            screen_reader_mode: false,
        }
    }
}

impl TerminalSettings {
    /// Normalize for use / persistence: blank shell → unset, every numeric
    /// knob clamped into its accepted range. Applied on load AND on set so
    /// a hand-edited config.toml can never smuggle junk into spawn or the
    /// frontend (clamping, per the brief — junk is corrected, not fatal).
    pub fn sanitized(mut self) -> Self {
        self.shell = self
            .shell
            .take()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        self.font_size = self.font_size.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE);
        self.scrollback = self.scrollback.clamp(0, MAX_SCROLLBACK);
        self.drawer_height = self
            .drawer_height
            .clamp(MIN_DRAWER_HEIGHT, MAX_DRAWER_HEIGHT);
        self
    }
}

/// Wire DTO for `terminal_get_settings` / `terminal_set_settings`
/// (camelCase, frozen):
/// `{ shell: string|null, fontSize, scrollback, drawerHeight, screenReaderMode }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalSettingsDto {
    pub shell: Option<String>,
    pub font_size: u32,
    pub scrollback: u32,
    pub drawer_height: u32,
    pub screen_reader_mode: bool,
}

impl From<TerminalSettings> for TerminalSettingsDto {
    fn from(s: TerminalSettings) -> Self {
        Self {
            shell: s.shell,
            font_size: s.font_size,
            scrollback: s.scrollback,
            drawer_height: s.drawer_height,
            screen_reader_mode: s.screen_reader_mode,
        }
    }
}

impl From<TerminalSettingsDto> for TerminalSettings {
    fn from(dto: TerminalSettingsDto) -> Self {
        Self {
            shell: dto.shell,
            font_size: dto.font_size,
            scrollback: dto.scrollback,
            drawer_height: dto.drawer_height,
            screen_reader_mode: dto.screen_reader_mode,
        }
    }
}

/// Resolve the global config file: `~/.shannon/config.toml`. Same path the
/// engine's `ConfigBuilder::load_global_toml` and `config_persist` use
/// (`dirs::home_dir()`, deliberately NOT `$SHANNON_HOME`-rewritten — this
/// must land in the file the rest of the ecosystem reads).
fn terminal_settings_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".shannon")
        .join("config.toml")
}

/// Load `[terminal]` from the global config; defaults when the file, the
/// table, or any key is missing. Output is always sanitized.
pub fn load_terminal_settings() -> TerminalSettings {
    load_terminal_settings_from(&terminal_settings_path())
}

/// Pure, path-injected core of [`load_terminal_settings`] (test seam —
/// settings tests use tempdirs instead of mutating `HOME` process-wide).
pub(crate) fn load_terminal_settings_from(path: &Path) -> TerminalSettings {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| toml::from_str::<toml::Value>(&text).ok())
        .and_then(|root| root.get("terminal").cloned())
        .and_then(|value| TerminalSettings::deserialize(value).ok())
        .map(TerminalSettings::sanitized)
        .unwrap_or_default()
}

/// Persist `[terminal]` into the global config (read-modify-write:
/// preserves every other table/key; comments and formatting are lost —
/// same accepted trade-off as the `[notifications.webhook]` write path).
pub fn save_terminal_settings(settings: &TerminalSettings) -> Result<(), String> {
    save_terminal_settings_to(&terminal_settings_path(), settings)
}

/// Pure, path-injected core of [`save_terminal_settings`].
pub(crate) fn save_terminal_settings_to(
    path: &Path,
    settings: &TerminalSettings,
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
    }
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    let mut root: toml::Value = toml::from_str(&existing)
        .unwrap_or(toml::Value::Table(toml::value::Table::new()));
    let table = root.as_table_mut().ok_or_else(|| {
        "config root is not a table — refusing to overwrite user config".to_string()
    })?;
    // Sanitize before disk so a clamped value, not the raw junk, lands in
    // the file (what you set is what `terminal_get_settings` reads back).
    let term = toml::Value::try_from(settings.clone().sanitized())
        .map_err(|e| format!("serialize [terminal]: {e}"))?;
    table.insert("terminal".into(), term);
    let serialized = toml::to_string_pretty(&root).map_err(|e| format!("serialize: {e}"))?;
    std::fs::write(path, serialized).map_err(|e| format!("write {}: {e}", path.display()))?;
    crate::file_permissions::restrict_to_owner(path);
    Ok(())
}

// ── Output sink (injectable emit boundary) ───────────────────────────────

/// Where coalesced pty bytes go. Production emits the `terminal:output`
/// Tauri event; tests record bytes and count emissions. `seq` is the
/// chunk's per-session monotonic number (review fix — see
/// [`TerminalOutputPayload`]). `emit_exit` has a no-op default so simple
/// test sinks stay one-method.
pub trait TerminalOutputSink: Send + Sync {
    fn emit_output(&self, terminal_id: &str, data: &[u8], seq: u64);

    /// P3-6: the session exited naturally and was reaped. Emitted once,
    /// after the final output flush (which still carries the human-readable
    /// exit notice). Explicit kills do not emit this — the caller asked for
    /// the death and already updates its own state.
    fn emit_exit(&self, _terminal_id: &str) {}
}

/// Production sink: base64-encode the coalesced bytes and emit
/// `terminal:output` with `{ terminalId, data }` (frozen payload shape);
/// exits go out as `terminal:exit` with `{ terminalId }`.
pub struct TauriTerminalSink {
    app: tauri::AppHandle,
}

impl TauriTerminalSink {
    pub fn new(app: tauri::AppHandle) -> Self {
        Self { app }
    }
}

impl TerminalOutputSink for TauriTerminalSink {
    fn emit_output(&self, terminal_id: &str, data: &[u8], seq: u64) {
        let payload = TerminalOutputPayload {
            terminal_id: terminal_id.to_string(),
            data: base64::engine::general_purpose::STANDARD.encode(data),
            seq,
        };
        if let Err(e) = self
            .app
            .emit(crate::events::event_names::TERMINAL_OUTPUT, payload)
        {
            tracing::debug!(terminal_id, error = %e, "terminal:output emit failed");
        }
    }

    fn emit_exit(&self, terminal_id: &str) {
        let payload = TerminalExitPayload {
            terminal_id: terminal_id.to_string(),
        };
        if let Err(e) = self
            .app
            .emit(crate::events::event_names::TERMINAL_EXIT, payload)
        {
            tracing::debug!(terminal_id, error = %e, "terminal:exit emit failed");
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
    /// Replay ring (US6): newest [`TERMINAL_HISTORY_CAP`] raw bytes of the
    /// session's output, appended by the pump from each drained batch.
    /// Owned by the session, so kill/reap (which removes the session from
    /// the map) retires the ring with it — no persistence. Carries the
    /// `end_seq` watermark (review fix) under the same lock.
    history: StdMutex<HistoryRing>,
    /// Review fix (replay ordering): per-session monotonic emit-chunk
    /// counter — the next seq to hand out. Incremented once per CHUNK (a
    /// drain split by [`MAX_EMIT_CHUNK`] consumes one value per chunk), so
    /// the seq order of the event stream matches the byte order exactly.
    next_emit_seq: AtomicU64,
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

/// Replay ring (US6) plus its snapshot watermark: `end_seq` is the
/// highest emit-chunk seq whose bytes are FULLY contained in `bytes` (the
/// `terminal_history` stitch key — review fix, see
/// [`TerminalOutputPayload`]). Ring and watermark always mutate together
/// under the one lock: a reader must never observe new ring bytes without
/// their seq (that would duplicate them) or an advanced seq without the
/// bytes (that would lose them).
struct HistoryRing {
    bytes: VecDeque<u8>,
    end_seq: u64,
}

impl HistoryRing {
    fn new() -> Self {
        Self {
            bytes: VecDeque::with_capacity(4096),
            end_seq: 0,
        }
    }
}

/// Append one drained batch to a session's replay ring, keeping only the
/// newest [`TERMINAL_HISTORY_CAP`] bytes (same oldest-dropped discipline
/// as [`PendingBuffer::push`], minus the truncation notice — replay is a
/// best-effort scrollback, not a guaranteed log), and publish `last_seq`
/// as the snapshot watermark in the same critical section. A no-op for an
/// empty batch (the watermark only ever advances over bytes actually
/// present in the ring).
fn append_history(ring: &StdMutex<HistoryRing>, bytes: &[u8], last_seq: u64) {
    if bytes.is_empty() {
        return;
    }
    let mut ring = ring.lock().unwrap_or_else(|p| p.into_inner());
    if bytes.len() >= TERMINAL_HISTORY_CAP {
        // A single batch at/over the cap: everything retained so far is
        // stale by definition — keep only the batch's tail.
        ring.bytes.clear();
        ring.bytes
            .extend(bytes[bytes.len() - TERMINAL_HISTORY_CAP..].iter().copied());
        ring.end_seq = last_seq;
        return;
    }
    let overflow = (ring.bytes.len() + bytes.len()).saturating_sub(TERMINAL_HISTORY_CAP);
    if overflow > 0 {
        ring.bytes.drain(..overflow);
    }
    ring.bytes.extend(bytes.iter().copied());
    ring.end_seq = last_seq;
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

    /// `terminal_spawn({projectDir, shell?})`. The cwd is canonicalized
    /// (the requested form is preserved verbatim on
    /// `TerminalInfo::project_dir_raw` — review fix); the shell is picked
    /// by the P3-1 precedence: explicit `shell`
    /// argument > `configured_shell` (persisted `[terminal].shell`) >
    /// `$SHELL` (PowerShell on Windows).
    ///
    /// `spawned_by_window` is the calling webview window's label (P3-2) —
    /// `None` only in tests / non-window callers. It rides on
    /// `TerminalInfo` so `kill_for_window` can reap every session a
    /// destroyed window owns.
    pub fn spawn(
        &self,
        project_dir: &Path,
        shell: Option<String>,
        configured_shell: Option<String>,
        spawned_by_window: Option<String>,
    ) -> Result<TerminalInfo, String> {
        // Capture the REQUESTED form before canonicalization: the
        // frontend's per-project filter compares its raw prop against this
        // string, so canonicalization (symlinks, Windows verbatim
        // prefixes) must not be the only surviving representation.
        let project_dir_raw = project_dir.display().to_string();
        let dir = project_dir
            .canonicalize()
            .map_err(|e| format!("project dir {}: {e}", project_dir.display()))?;
        let shell_line = shell.unwrap_or_else(|| {
            resolve_default_shell(
                configured_shell.as_deref(),
                std::env::var("SHELL").ok().as_deref(),
                cfg!(windows),
            )
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
            project_dir_raw: Some(project_dir_raw),
            shell: shell_line.clone(),
            started_at_ms: now_ms(),
            spawned_by_window,
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
            history: StdMutex::new(HistoryRing::new()),
            next_emit_seq: AtomicU64::new(0),
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

    /// `terminal_history` core (US6): the session's retained replay bytes
    /// in stream order plus `end_seq` — the highest emit-chunk seq fully
    /// contained in those bytes (read atomically with the ring so the
    /// frontend's snapshot ⊕ events stitch is exact). Unknown /
    /// already-ended id → empty (the frontend calls speculatively on
    /// reconnect; absence is normal, not an error). The ring dies with the
    /// session — kill/reap removed it from the map, so this naturally
    /// returns empty afterwards.
    pub fn history(&self, terminal_id: &str) -> (Vec<u8>, u64) {
        let Ok(session) = self.session(terminal_id) else {
            return (Vec::new(), 0);
        };
        let ring = session.history.lock().unwrap_or_else(|p| p.into_inner());
        (ring.bytes.iter().copied().collect(), ring.end_seq)
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

    /// P3-2 — kill every session spawned by the named window (same
    /// process-tree discipline as [`Self::kill`]). Called from the
    /// `session-*` window-destroyed hook in `main.rs` so a closed session
    /// window cannot leak its shells; returns how many sessions were
    /// reaped (0 is normal — a window whose panel never opened a terminal).
    pub fn kill_for_window(&self, label: &str) -> usize {
        let removed: Vec<Arc<TerminalSession>> = {
            let mut sessions = self
                .inner
                .sessions
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            let victims: Vec<String> = sessions
                .values()
                .filter(|s| s.info.spawned_by_window.as_deref() == Some(label))
                .map(|s| s.info.terminal_id.clone())
                .collect();
            victims
                .into_iter()
                .filter_map(|id| sessions.remove(&id))
                .collect()
        };
        for session in &removed {
            session.pending.close();
            if let Some(mut child) = session.take_child() {
                kill_process_tree(&mut child);
            }
        }
        removed.len()
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
        // Review fix (replay ordering): the drained batch is split into
        // chunks up front and each chunk gets its own monotonic seq, in
        // stream order. The ring append below (which covers exactly the
        // drained bytes) publishes the LAST drained chunk's seq as the
        // snapshot watermark in the same critical section, and the emits
        // happen strictly after — so a `terminal_history` snapshot is
        // always a clean prefix of the (seq, bytes) event stream, whichever
        // way a concurrent reader interleaves.
        let chunk_count = drained.chunks(MAX_EMIT_CHUNK).count() as u64;
        let first_seq = session
            .next_emit_seq
            .fetch_add(chunk_count, Ordering::SeqCst);
        // Replay ring (US6): every drained byte lands in the ring before
        // the emit so a reconnecting panel can replay what was shown.
        // (The in-stream exit notice below is NOT part of the drain, so
        // it gets its own seq past the watermark instead.)
        if chunk_count > 0 {
            append_history(
                &session.history,
                &drained,
                first_seq + chunk_count - 1,
            );
        }
        if let Some((success, code)) = session.poll_exit() {
            emit_chunked(&sink, &session.info.terminal_id, &drained, first_seq);
            // The human-readable exit notice rides as its own chunk with
            // the next seq — past `end_seq` by construction, so a
            // reconnecting panel never drops it.
            let notice = format!(
                "\r\n\u{1b}[2m[shannon: process exited — {}]\u{1b}[0m\r\n",
                if success {
                    "done".to_string()
                } else {
                    format!("exit code {code}")
                }
            );
            let notice_seq = session.next_emit_seq.fetch_add(1, Ordering::SeqCst);
            sink.emit_output(&session.info.terminal_id, notice.as_bytes(), notice_seq);
            // P3-6 — the machine-readable exit signal, after the final
            // bytes so the frontend never sees `terminal:exit` before the
            // last output / in-stream notice for this session.
            sink.emit_exit(&session.info.terminal_id);
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
        } else if chunk_count > 0 {
            emit_chunked(&sink, &session.info.terminal_id, &drained, first_seq);
        }
    }
}

/// Emit a drained batch as consecutive events of at most
/// [`MAX_EMIT_CHUNK`] bytes, numbered `first_seq` onward — one tick's
/// flood must never become a multi-MB webview payload, and every chunk
/// carries its own place in the stream order (review fix).
fn emit_chunked(
    sink: &Arc<dyn TerminalOutputSink>,
    terminal_id: &str,
    bytes: &[u8],
    first_seq: u64,
) {
    for (i, chunk) in bytes.chunks(MAX_EMIT_CHUNK).enumerate() {
        sink.emit_output(terminal_id, chunk, first_seq + i as u64);
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
///
/// The `window` parameter is injected by Tauri (never sent by the
/// frontend) — the invoking webview window's label is recorded on the
/// session (`TerminalInfo.spawnedByWindow`) so the window's destroyed hook
/// can reap it (P3-2). The wire contract is unchanged.
#[tauri::command]
pub async fn terminal_spawn(
    state: tauri::State<'_, crate::commands::AppState>,
    window: tauri::WebviewWindow,
    project_dir: Option<String>,
    shell: Option<String>,
) -> Result<TerminalSpawnResponse, String> {
    let dir = match project_dir {
        Some(dir) if !dir.trim().is_empty() => PathBuf::from(dir),
        _ => crate::commands_agents::resolve_working_dir(&state).await,
    };
    // P3-1 shell precedence: the explicit `shell` argument (if any) wins;
    // otherwise the persisted `[terminal].shell` (if set); `$SHELL` /
    // platform default are resolved inside `spawn`. One small config read
    // per spawn — spawns are user-initiated and rare.
    let configured_shell = load_terminal_settings().shell;
    let info = state.terminals.spawn(
        &dir,
        shell,
        configured_shell,
        Some(window.label().to_string()),
    )?;
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

/// `terminal_list() -> [{ terminalId, projectDir, projectDirRaw, shell,
/// startedAtMs }]`.
#[tauri::command]
pub async fn terminal_list(
    state: tauri::State<'_, crate::commands::AppState>,
) -> Result<Vec<TerminalInfo>, String> {
    Ok(state.terminals.list())
}

/// `terminal_get_settings() -> TerminalSettingsDto` (P3-1, new command).
/// Reads `[terminal]` from `~/.shannon/config.toml`; missing anything →
/// defaults. Always returns the sanitized (clamped) effective values.
#[tauri::command]
pub async fn terminal_get_settings() -> Result<TerminalSettingsDto, String> {
    Ok(TerminalSettingsDto::from(load_terminal_settings()))
}

/// `terminal_set_settings(settings) -> TerminalSettingsDto` (P3-1, new
/// command). Sanitizes (blank shell → null, numerics clamped), persists
/// under `[terminal]`, and returns the effective values — the frontend
/// renders what was actually stored, not what it sent.
#[tauri::command]
pub async fn terminal_set_settings(
    settings: TerminalSettingsDto,
) -> Result<TerminalSettingsDto, String> {
    let effective = TerminalSettings::from(settings).sanitized();
    save_terminal_settings(&effective)?;
    Ok(TerminalSettingsDto::from(effective))
}

/// `terminal_history({terminalId}) -> { data, endSeq }` (US6) — base64 of
/// the session's retained replay bytes (newest 1 MiB, see
/// [`TERMINAL_HISTORY_CAP`]) plus `endSeq`, the highest output-chunk seq
/// fully included in the snapshot (review fix — the frontend drops the
/// queued events it already replayed, see [`TerminalOutputPayload`]).
/// Unknown or already-ended id → `{ data: "", endSeq: 0 }`,
/// deliberately not an error: the frontend calls speculatively on
/// reconnect. No persistence — the ring dies with its session.
#[tauri::command]
pub async fn terminal_history(
    state: tauri::State<'_, crate::commands::AppState>,
    terminal_id: String,
) -> Result<TerminalHistoryResponse, String> {
    let (bytes, end_seq) = state.terminals.history(&terminal_id);
    Ok(TerminalHistoryResponse {
        data: base64::engine::general_purpose::STANDARD.encode(bytes),
        end_seq,
    })
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

/// P3-2 — reap every session spawned by the named window, for the
/// `session-*` window-destroyed hook in `main.rs`. Pub lib function (not
/// field access) keeps the bin's surface tiny — same pattern as
/// [`shutdown_on_exit`]. Returns the number of sessions killed.
pub fn kill_window_sessions(state: &crate::commands::AppState, label: &str) -> usize {
    state.terminals.kill_for_window(label)
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    /// Short coalescing tick so pump-driven tests stay fast.
    const TEST_TICK: Duration = Duration::from_millis(10);
    const WAIT: Duration = Duration::from_secs(5);

    /// Records every emission (id, seq, bytes) in order; the test asserts
    /// on delivery, coalescing, and seq monotonicity. `emit_exit` records
    /// the terminal id so the P3-6 contract is observable.
    type Emissions = Arc<StdMutex<Vec<(String, u64, Vec<u8>)>>>;

    #[derive(Default, Clone)]
    struct RecordingSink {
        emissions: Emissions,
        exits: Arc<StdMutex<Vec<String>>>,
    }

    impl RecordingSink {
        fn joined(&self, terminal_id: &str) -> Vec<u8> {
            self.emissions
                .lock()
                .unwrap()
                .iter()
                .filter(|(id, _, _)| id == terminal_id)
                .flat_map(|(_, _, bytes)| bytes.iter().copied())
                .collect()
        }

        /// The seqs recorded for one terminal, in emission order.
        fn seqs(&self, terminal_id: &str) -> Vec<u64> {
            self.emissions
                .lock()
                .unwrap()
                .iter()
                .filter(|(id, _, _)| id == terminal_id)
                .map(|(_, seq, _)| *seq)
                .collect()
        }

        fn count_for(&self, terminal_id: &str) -> usize {
            self.emissions
                .lock()
                .unwrap()
                .iter()
                .filter(|(id, _, _)| id == terminal_id)
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

        /// True once `emit_exit` fired for this terminal (P3-6).
        fn exited(&self, terminal_id: &str) -> bool {
            self.exits
                .lock()
                .unwrap()
                .iter()
                .any(|id| id == terminal_id)
        }
    }

    impl TerminalOutputSink for RecordingSink {
        fn emit_output(&self, terminal_id: &str, data: &[u8], seq: u64) {
            self.emissions
                .lock()
                .unwrap()
                .push((terminal_id.to_string(), seq, data.to_vec()));
        }

        fn emit_exit(&self, terminal_id: &str) {
            self.exits.lock().unwrap().push(terminal_id.to_string());
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
            project_dir_raw: Some("/home/u/proj".into()),
            shell: "/bin/bash".into(),
            started_at_ms: 1_700,
            spawned_by_window: Some("session-abc".into()),
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"terminalId\":\"t-1\""), "{json}");
        assert!(json.contains("\"projectDir\":\"/home/u/proj\""), "{json}");
        assert!(json.contains("\"shell\":\"/bin/bash\""), "{json}");
        assert!(json.contains("\"startedAtMs\":1700"), "{json}");
        // P3-2 additive attribution field (camelCase, nullable).
        assert!(
            json.contains("\"spawnedByWindow\":\"session-abc\""),
            "{json}"
        );
        // Review-fix additive raw-dir field (camelCase, nullable).
        assert!(
            json.contains("\"projectDirRaw\":\"/home/u/proj\""),
            "{json}"
        );
        let back: TerminalInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(back, info);
    }

    #[test]
    fn list_item_without_spawned_by_window_still_parses() {
        // P3-2: wire payloads emitted before the additive attribution field
        // existed (and the frontend's own fixtures) must keep deserializing.
        let legacy: TerminalInfo = serde_json::from_str(
            r#"{"terminalId":"t-1","projectDir":"/p","shell":"/bin/sh","startedAtMs":7}"#,
        )
        .expect("legacy TerminalInfo must deserialize");
        assert_eq!(legacy.terminal_id, "t-1");
        assert_eq!(legacy.spawned_by_window, None);
        assert_eq!(legacy.project_dir_raw, None);

        // The raw-dir field round-trips as string|null (canonical == raw
        // collapses to a repeated value, never a surrogate).
        let with_raw: TerminalInfo = serde_json::from_str(
            r#"{"terminalId":"t-1","projectDir":"/p","projectDirRaw":null,"shell":"/bin/sh","startedAtMs":7}"#,
        )
        .expect("TerminalInfo with null projectDirRaw must deserialize");
        assert_eq!(with_raw.project_dir_raw, None);
    }

    #[test]
    fn exit_payload_is_frozen_camel_case() {
        // Task 3's frontend parses exactly this shape (TerminalExitPayload
        // in desktop/ui/src/types) — `{"terminalId": …}`, nothing else.
        let json = serde_json::to_string(&TerminalExitPayload {
            terminal_id: "t-9".into(),
        })
        .unwrap();
        assert_eq!(json, r#"{"terminalId":"t-9"}"#);
        let back: TerminalExitPayload = serde_json::from_str(&json).unwrap();
        assert_eq!(back.terminal_id, "t-9");
    }

    #[test]
    fn output_payload_is_base64_and_byte_preserving() {
        // Non-UTF8 bytes (0xFF 0xFE) must survive the wire: base64, not
        // lossy UTF-8 — that is why the contract freezes base64.
        let raw: Vec<u8> = vec![0xFF, 0xFE, 0x00, 0x1B, b'[', b'A', 0x80];
        let payload = TerminalOutputPayload {
            terminal_id: "t-9".into(),
            data: base64::engine::general_purpose::STANDARD.encode(&raw),
            seq: 12,
        };
        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains("\"terminalId\":\"t-9\""), "{json}");
        assert!(json.contains("\"data\":\""), "{json}");
        // Review fix: the additive per-chunk seq rides along (camelCase).
        assert!(json.contains("\"seq\":12"), "{json}");
        let back: TerminalOutputPayload = serde_json::from_str(&json).unwrap();
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(&back.data)
            .unwrap();
        assert_eq!(decoded, raw);
        assert_eq!(back.seq, 12);

        // Payloads emitted before the field existed keep parsing (seq
        // defaults to 0, the pre-history watermark).
        let legacy: TerminalOutputPayload = serde_json::from_str(
            r#"{"terminalId":"t-9","data":"aGk="}"#,
        )
        .expect("legacy TerminalOutputPayload must deserialize");
        assert_eq!(legacy.seq, 0);
    }

    // ── Shell resolution (pure) ──────────────────────────────────────────

    #[test]
    fn default_shell_prefers_env_with_fallbacks() {
        // No configured shell → the legacy $SHELL → /bin/sh chain (and
        // PowerShell on Windows regardless of $SHELL).
        assert_eq!(
            resolve_default_shell(None, Some("/usr/bin/zsh"), false),
            "/usr/bin/zsh"
        );
        assert_eq!(resolve_default_shell(None, None, false), "/bin/sh");
        assert_eq!(resolve_default_shell(None, Some("  "), false), "/bin/sh");
        // Windows always gets PowerShell regardless of $SHELL.
        assert_eq!(
            resolve_default_shell(None, Some("/usr/bin/zsh"), true),
            "powershell.exe"
        );
    }

    #[test]
    fn default_shell_configured_beats_env_and_platform() {
        // P3-1 precedence step 2: the persisted `[terminal].shell` wins
        // over $SHELL and over the Windows PowerShell default.
        assert_eq!(
            resolve_default_shell(Some("/usr/bin/fish"), Some("/usr/bin/zsh"), false),
            "/usr/bin/fish"
        );
        assert_eq!(
            resolve_default_shell(Some("pwsh.exe"), Some("C:/shell"), true),
            "pwsh.exe"
        );
        // A blank/whitespace configured shell counts as unset.
        assert_eq!(
            resolve_default_shell(Some("   "), Some("/usr/bin/zsh"), false),
            "/usr/bin/zsh"
        );
        assert_eq!(resolve_default_shell(Some("   "), None, false), "/bin/sh");
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

    // ── Terminal settings ([terminal] persistence, P3-1) ─────────────────

    #[test]
    fn terminal_settings_defaults_match_the_brief() {
        let s = TerminalSettings::default();
        assert_eq!(s.shell, None, "shell None → env fallback");
        assert_eq!(s.font_size, 12);
        assert_eq!(s.scrollback, 5000);
        assert_eq!(s.drawer_height, 320);
        assert!(!s.screen_reader_mode);
        // Missing keys / missing table fall back to the same defaults.
        assert_eq!(load_terminal_settings_from(Path::new("/nonexistent/config.toml")), s);
    }

    #[test]
    fn terminal_settings_dto_is_frozen_camel_case() {
        let dto = TerminalSettingsDto {
            shell: None,
            font_size: 12,
            scrollback: 5000,
            drawer_height: 320,
            screen_reader_mode: false,
        };
        let json = serde_json::to_string(&dto).unwrap();
        assert_eq!(
            json,
            r#"{"shell":null,"fontSize":12,"scrollback":5000,"drawerHeight":320,"screenReaderMode":false}"#
        );
        let back: TerminalSettingsDto = serde_json::from_str(&json).unwrap();
        assert_eq!(back, dto);
        // shell round-trips as string|null.
        let with_shell = TerminalSettingsDto {
            shell: Some("/usr/bin/fish".into()),
            ..dto
        };
        let json = serde_json::to_string(&with_shell).unwrap();
        assert!(
            json.contains(r#""shell":"/usr/bin/fish""#),
            "shell stays on the wire when set: {json}"
        );
    }

    #[test]
    fn terminal_settings_sanitize_clamps_junk() {
        let clamped = TerminalSettings {
            shell: Some("   ".into()),
            font_size: 9_999,
            scrollback: 424_242,
            drawer_height: 5,
            screen_reader_mode: true,
        }
        .sanitized();
        assert_eq!(clamped.shell, None, "blank shell = unset");
        assert_eq!(clamped.font_size, MAX_FONT_SIZE);
        assert_eq!(clamped.scrollback, MAX_SCROLLBACK);
        assert_eq!(clamped.drawer_height, MIN_DRAWER_HEIGHT);
        // …and the low side.
        let low = TerminalSettings {
            shell: None,
            font_size: 1,
            scrollback: 0,
            drawer_height: 1,
            screen_reader_mode: false,
        }
        .sanitized();
        assert_eq!(low.font_size, MIN_FONT_SIZE);
        assert_eq!(low.scrollback, 0, "scrollback 0 stays legal");
        assert_eq!(low.drawer_height, MIN_DRAWER_HEIGHT);
        // Whitespace around a real shell value is trimmed, not dropped.
        let padded = TerminalSettings {
            shell: Some("  /usr/bin/fish ".into()),
            ..TerminalSettings::default()
        }
        .sanitized();
        assert_eq!(padded.shell.as_deref(), Some("/usr/bin/fish"));
    }

    #[test]
    fn terminal_settings_round_trip_through_the_toml_store() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        // Missing file → defaults (no error).
        assert_eq!(load_terminal_settings_from(&path), TerminalSettings::default());
        let settings = TerminalSettings {
            shell: Some("/usr/bin/fish".into()),
            font_size: 14,
            scrollback: 10_000,
            drawer_height: 400,
            screen_reader_mode: true,
        };
        save_terminal_settings_to(&path, &settings).expect("save");
        // Read-back equals what was written (sanitized is a no-op here).
        assert_eq!(load_terminal_settings_from(&path), settings);
        // The file landed under the `[terminal]` table with snake_case keys.
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("[terminal]"), "{text}");
        assert!(text.contains("font_size = 14"), "{text}");
        assert!(text.contains("screen_reader_mode = true"), "{text}");
        assert!(
            text.contains("shell = \"/usr/bin/fish\""),
            "set shell lands on disk: {text}"
        );

        // Overwrite: new values replace, nothing duplicates, and an unset
        // shell stays off disk entirely (TOML has no null — the key is
        // skipped so the default fallback applies on load).
        save_terminal_settings_to(&path, &TerminalSettings::default()).expect("save 2");
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("shell"), "unset shell must be skipped: {text}");
        let reloaded = load_terminal_settings_from(&path);
        assert_eq!(reloaded, TerminalSettings::default());
    }

    #[test]
    fn terminal_settings_store_preserves_unrelated_tables() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "model = \"glm-4\"\n\n[notifications]\nwebhook_url = \"https://x\"\n\n[other]\nkey = 1\n",
        )
        .unwrap();
        save_terminal_settings_to(&path, &TerminalSettings::default()).expect("save");
        let text = std::fs::read_to_string(&path).unwrap();
        // Flat keys and sibling tables survive the read-modify-write.
        assert!(text.contains("model = \"glm-4\""), "{text}");
        assert!(text.contains("[notifications]"), "{text}");
        assert!(text.contains("webhook_url = \"https://x\""), "{text}");
        assert!(text.contains("[other]"), "{text}");
        assert!(text.contains("key = 1"), "{text}");
        assert!(text.contains("[terminal]"), "{text}");
    }

    #[test]
    fn terminal_settings_store_sanitizes_hand_edited_junk_on_load() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "[terminal]\nshell = \"   \"\nfont_size = 9999\nscrollback = 424242\ndrawer_height = 5\nunknown_key = true\n",
        )
        .unwrap();
        let loaded = load_terminal_settings_from(&path);
        assert_eq!(loaded.shell, None);
        assert_eq!(loaded.font_size, MAX_FONT_SIZE);
        assert_eq!(loaded.scrollback, MAX_SCROLLBACK);
        assert_eq!(loaded.drawer_height, MIN_DRAWER_HEIGHT);
        assert!(!loaded.screen_reader_mode, "missing key → default");
    }

    #[cfg(unix)]
    #[test]
    fn spawn_shell_precedence_explicit_beats_configured() {
        let manager = TerminalManager::with_tick(TEST_TICK);
        let dir = tempfile::tempdir().expect("tempdir");
        // Explicit argument wins over the configured shell.
        let explicit = manager
            .spawn(dir.path(), Some("/bin/sh".into()), Some("/bin/false".into()), None)
            .expect("spawn explicit");
        assert_eq!(explicit.shell, "/bin/sh");
        // No argument → configured shell is used verbatim (recorded in
        // TerminalInfo, so the test never depends on $SHELL).
        let configured = manager
            .spawn(dir.path(), None, Some("/bin/sh".into()), None)
            .expect("spawn configured");
        assert_eq!(configured.shell, "/bin/sh");
        manager.kill_all();
    }

    // ── Replay history ring (US6) ────────────────────────────────────────

    #[test]
    fn history_response_is_frozen_camel_case() {
        // Review fix: the additive `endSeq` watermark rides next to `data`
        // (the frontend drops queued events with seq ≤ endSeq).
        let resp = TerminalHistoryResponse {
            data: "aGk=".into(),
            end_seq: 7,
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert_eq!(json, r#"{"data":"aGk=","endSeq":7}"#);
        let back: TerminalHistoryResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.data, "aGk=");
        assert_eq!(back.end_seq, 7);
        // Legacy shape (no endSeq) still parses — defaults to 0, i.e.
        // "the snapshot covers nothing" → the frontend flushes everything.
        let legacy: TerminalHistoryResponse =
            serde_json::from_str(r#"{"data":"aGk="}"#).expect("legacy response parses");
        assert_eq!(legacy.end_seq, 0);
    }

    #[test]
    fn append_history_keeps_the_newest_cap_bytes() {
        let ring: StdMutex<HistoryRing> = StdMutex::new(HistoryRing::new());
        append_history(&ring, &[b'a'; 64], 0);
        append_history(&ring, &[b'b'; 64], 1);
        let held: Vec<u8> = ring.lock().unwrap().bytes.iter().copied().collect();
        assert_eq!(held.len(), 128, "under the cap everything is retained");
        assert_eq!(ring.lock().unwrap().end_seq, 1, "watermark = last batch");

        // A cap-sized batch evicts everything older in one step (the
        // "batch at/over the cap" branch) and keeps only its own tail.
        append_history(&ring, &[b'c'; TERMINAL_HISTORY_CAP], 2);
        let held: Vec<u8> = ring.lock().unwrap().bytes.iter().copied().collect();
        assert_eq!(held.len(), TERMINAL_HISTORY_CAP);
        assert!(
            held.iter().all(|&b| b == b'c'),
            "a cap-sized batch evicts everything older"
        );

        // A batch larger than the cap itself keeps only its own tail.
        let ring: StdMutex<HistoryRing> = StdMutex::new(HistoryRing::new());
        append_history(&ring, &[b'x'; TERMINAL_HISTORY_CAP + 5], 9);
        let held: Vec<u8> = ring.lock().unwrap().bytes.iter().copied().collect();
        assert_eq!(held.len(), TERMINAL_HISTORY_CAP);
        assert!(held.iter().all(|&b| b == b'x'));
        assert_eq!(ring.lock().unwrap().end_seq, 9);
        // …and appending afterwards still preserves stream order.
        append_history(&ring, b"tail", 10);
        let held: Vec<u8> = ring.lock().unwrap().bytes.iter().copied().collect();
        assert!(held.ends_with(b"tail"));
        assert_eq!(ring.lock().unwrap().end_seq, 10);

        // An empty batch is a full no-op: the watermark only ever covers
        // bytes actually present in the ring.
        append_history(&ring, b"", 99);
        assert_eq!(ring.lock().unwrap().end_seq, 10);
    }

    #[test]
    fn history_of_unknown_id_is_empty_not_an_error() {
        let sink = RecordingSink::default();
        let manager = test_manager(&sink);
        assert!(manager.history("no-such-terminal").0.is_empty());
        assert_eq!(manager.history("no-such-terminal").1, 0);
    }

    #[cfg(unix)]
    #[test]
    fn output_accumulates_into_the_history_ring() {
        let sink = RecordingSink::default();
        let manager = test_manager(&sink);
        let dir = tempfile::tempdir().expect("tempdir");
        let info = manager
            .spawn(dir.path(), Some("/bin/sh".into()), None, None)
            .expect("spawn");
        manager.test_push_pending(&info.terminal_id, b"echo replay-marker-99\n");
        manager.pump_once_for_test();
        let (bytes, end_seq) = manager.history(&info.terminal_id);
        assert!(
            String::from_utf8_lossy(&bytes).contains("replay-marker-99"),
            "drained output must land in the ring, got: {}",
            String::from_utf8_lossy(&bytes)
        );
        // The snapshot watermark covers exactly what the pump appended.
        assert_eq!(end_seq, 0, "one drain = one chunk = seq 0");
        // …and the ring dies with the session.
        manager.kill(&info.terminal_id).expect("kill");
        assert!(
            manager.history(&info.terminal_id).0.is_empty(),
            "killed session must replay nothing"
        );
    }

    #[cfg(unix)]
    #[test]
    fn history_ring_enforces_the_one_mib_cap_through_the_pump() {
        let sink = RecordingSink::default();
        let manager = test_manager(&sink);
        let dir = tempfile::tempdir().expect("tempdir");
        let info = manager
            .spawn(dir.path(), Some("/bin/sh -c 'sleep 30'".into()), None, None)
            .expect("spawn");
        // 1.5 MiB in one batch: under the 2 MiB pending cap it drains
        // whole, and the ring must retain exactly the newest 1 MiB.
        let flood = vec![b'f'; TERMINAL_HISTORY_CAP + TERMINAL_HISTORY_CAP / 2];
        manager.test_push_pending(&info.terminal_id, &flood);
        manager.pump_once_for_test();
        let (bytes, end_seq) = manager.history(&info.terminal_id);
        assert_eq!(bytes.len(), TERMINAL_HISTORY_CAP, "cap enforced");
        assert!(bytes.iter().all(|&b| b == b'f'), "newest tail kept");
        // The watermark names the LAST chunk of the drain (the eviction
        // only dropped older bytes, never the watermark's own chunk).
        let expected_chunks = (flood.len() as u64).div_ceil(MAX_EMIT_CHUNK as u64);
        assert_eq!(end_seq, expected_chunks - 1, "endSeq = last chunk seq");
        manager.kill(&info.terminal_id).expect("kill");
    }

    #[cfg(unix)]
    #[test]
    fn spawn_keeps_the_requested_dir_verbatim_in_project_dir_raw() {
        let sink = RecordingSink::default();
        let manager = test_manager(&sink);
        let dir = tempfile::tempdir().expect("tempdir");
        let real = dir.path().join("real");
        std::fs::create_dir(&real).expect("mkdir real");
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&real, &link).expect("symlink");
        let requested = link.display().to_string();

        let info = manager
            .spawn(&link, Some("/bin/sh -c 'sleep 30'".into()), None, None)
            .expect("spawn");
        // The stored dir is the canonicalized form (unchanged behavior)…
        assert_eq!(
            info.project_dir,
            real.canonicalize().expect("canonicalize").display().to_string()
        );
        // …while the additive raw field preserves the exact requested
        // (symlinked) form the frontend will compare its prop against.
        assert_eq!(info.project_dir_raw.as_deref(), Some(requested.as_str()));
        assert_ne!(
            info.project_dir,
            info.project_dir_raw.as_deref().expect("raw is Some"),
            "the canonical-vs-raw mismatch this field exists for"
        );
        manager.kill(&info.terminal_id).expect("kill");
    }

    #[cfg(unix)]
    #[test]
    fn emit_seqs_are_monotonic_and_contiguous_across_chunked_emits() {
        let sink = RecordingSink::default();
        let manager = test_manager(&sink);
        let dir = tempfile::tempdir().expect("tempdir");
        // `-c 'sleep 30'` prints nothing: every byte below is test-pushed,
        // so the chunk/seq accounting is exact.
        let info = manager
            .spawn(dir.path(), Some("/bin/sh -c 'sleep 30'".into()), None, None)
            .expect("spawn");
        let id = info.terminal_id.clone();
        // Two drains of 2 chunks + a remainder each → 2+2 chunks.
        let flood = vec![b'f'; MAX_EMIT_CHUNK + MAX_EMIT_CHUNK / 2];
        manager.test_push_pending(&id, &flood);
        manager.pump_once_for_test();
        manager.test_push_pending(&id, &flood);
        manager.pump_once_for_test();
        assert!(
            wait_until(WAIT, || sink.count_for(&id) >= 4),
            "all four chunks emitted"
        );
        let seqs = sink.seqs(&id);
        assert_eq!(
            seqs,
            (0..4u64).collect::<Vec<u64>>(),
            "one seq per chunk, strictly increasing, no gaps"
        );
        // Each chunk respects MAX_EMIT_CHUNK and reassembles into the
        // exact pushed stream, in order.
        for (_, _, bytes) in sink.emissions.lock().unwrap().iter() {
            assert!(bytes.len() <= MAX_EMIT_CHUNK, "chunk oversized");
        }
        let mut expected = flood.clone();
        expected.extend_from_slice(&flood);
        assert_eq!(sink.joined(&id), expected, "chunks reassemble losslessly");
        // The final watermark covers the whole stream.
        assert_eq!(manager.history(&id).1, 3);
        manager.kill(&id).expect("kill");
    }

    #[cfg(unix)]
    #[test]
    fn snapshot_plus_post_snapshot_events_reconstructs_losslessly() {
        // The review-fix invariant, end to end: whatever a reconnecting
        // panel snapshots mid-stream, stitching history ⊕ {emissions with
        // seq > endSeq} reproduces the byte stream with no loss and no
        // duplication.
        let sink = RecordingSink::default();
        let manager = test_manager(&sink);
        let dir = tempfile::tempdir().expect("tempdir");
        let info = manager
            .spawn(dir.path(), Some("/bin/sh -c 'sleep 30'".into()), None, None)
            .expect("spawn");
        let id = info.terminal_id.clone();

        // Stream A: a 3-chunk drain, pumped, then snapshotted mid-stream.
        let a = vec![b'A'; MAX_EMIT_CHUNK * 2 + 1024];
        manager.test_push_pending(&id, &a);
        manager.pump_once_for_test();
        assert!(wait_until(WAIT, || sink.count_for(&id) >= 3));
        let (snapshot, end_seq) = manager.history(&id);
        assert_eq!(snapshot, a, "ring holds the full drain (under the cap)");
        assert_eq!(end_seq, 2, "watermark = last of the 3 chunks");

        // Stream B: emitted strictly AFTER the snapshot.
        let b = vec![b'B'; MAX_EMIT_CHUNK + 512];
        manager.test_push_pending(&id, &b);
        manager.pump_once_for_test();
        assert!(wait_until(WAIT, || sink.count_for(&id) >= 5));

        // The frontend stitch: history ⊕ {events with seq > endSeq}.
        let mut stitched = snapshot;
        for (tid, seq, bytes) in sink.emissions.lock().unwrap().iter() {
            if *tid == id && *seq > end_seq {
                stitched.extend_from_slice(bytes);
            }
        }
        let mut full = a;
        full.extend_from_slice(&b);
        assert_eq!(stitched, full, "no byte loss, no duplication");

        // A SECOND snapshot taken now carries the advanced watermark, and
        // stitching against it (nothing new emitted) stays lossless.
        let (snapshot2, end_seq2) = manager.history(&id);
        assert_eq!(end_seq2, 4);
        let mut stitched2 = snapshot2;
        for (tid, seq, bytes) in sink.emissions.lock().unwrap().iter() {
            if *tid == id && *seq > end_seq2 {
                stitched2.extend_from_slice(bytes);
            }
        }
        assert_eq!(stitched2, full, "late snapshot is lossless too");
        manager.kill(&id).expect("kill");
    }

    // ── Lifecycle (unix: real pty + /bin/sh) ─────────────────────────────

    #[cfg(unix)]
    #[test]
    fn spawn_write_and_output_flows_to_sink() {
        let sink = RecordingSink::default();
        let manager = test_manager(&sink);
        let dir = tempfile::tempdir().expect("tempdir");
        let info = manager
            .spawn(dir.path(), Some("/bin/sh".into()), None, None)
            .expect("spawn");
        assert_eq!(
            info.project_dir,
            dir.path().canonicalize().unwrap().display().to_string()
        );
        // Review fix: the requested form rides along verbatim (here the
        // tempdir path is already canonical, so raw == canonical).
        assert_eq!(info.project_dir_raw.as_deref(), Some(dir.path().display().to_string().as_str()));
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
            .spawn(dir.path(), Some("/bin/sh".into()), None, None)
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
            .spawn(dir.path(), Some("/bin/sh".into()), None, None)
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
            .spawn(dir.path(), Some("/bin/sh -c 'sleep 30'".into()), None, None)
            .expect("spawn");
        let pid = pid_of(&manager, &info.terminal_id).expect("pid");
        let killed = manager.kill(&info.terminal_id).expect("kill");
        assert_eq!(killed.terminal_id, info.terminal_id);
        assert!(wait_until(WAIT, || process_gone(pid)), "process must die");
        assert!(manager.list().is_empty(), "no phantom entries after kill");
        // Explicit kills are caller-requested deaths: no `terminal:exit`
        // event (only natural exits emit it — the killer already knows).
        assert!(!sink.exited(&info.terminal_id));
        // Unknown id on every mutating command → explicit error.
        assert!(manager.kill(&info.terminal_id).is_err());
        assert!(manager.write(&info.terminal_id, "x").is_err());
        assert!(manager.resize(&info.terminal_id, 80, 24).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn kill_for_window_reaps_only_that_windows_sessions() {
        let sink = RecordingSink::default();
        let manager = test_manager(&sink);
        let dir = tempfile::tempdir().expect("tempdir");
        // Two sessions attributed to different (session-)window labels;
        // each runs a `sleep 30` child so the group kill is observable.
        let a = manager
            .spawn(
                dir.path(),
                Some("/bin/sh -c 'sleep 30'".into()),
                None,
                Some("session-aaaa".into()),
            )
            .expect("spawn a");
        let b = manager
            .spawn(
                dir.path(),
                Some("/bin/sh -c 'sleep 30'".into()),
                None,
                Some("session-bbbb".into()),
            )
            .expect("spawn b");
        assert_eq!(a.spawned_by_window.as_deref(), Some("session-aaaa"));
        assert_eq!(b.spawned_by_window.as_deref(), Some("session-bbbb"));
        let pid_a = pid_of(&manager, &a.terminal_id).expect("pid a");

        // Reap window A only.
        let killed = manager.kill_for_window("session-aaaa");
        assert_eq!(killed, 1, "exactly session A dies");
        assert!(
            wait_until(WAIT, || process_gone(pid_a)),
            "targeted session's process tree must die"
        );
        assert!(
            !manager.list().iter().any(|t| t.terminal_id == a.terminal_id),
            "targeted session leaves the list"
        );
        assert!(
            manager.list().iter().any(|t| t.terminal_id == b.terminal_id),
            "the other window's session must survive"
        );
        // Reaping an unknown/already-clean label is a no-op, not an error.
        assert_eq!(manager.kill_for_window("session-aaaa"), 0);
        assert_eq!(manager.kill_for_window("main"), 0);
        // The survivor is still writable; clean up.
        manager
            .write(&b.terminal_id, "exit\n")
            .expect("survivor alive");
        manager.kill_all();
        assert!(manager.list().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn natural_exit_is_reaped_and_announced() {
        let sink = RecordingSink::default();
        let manager = test_manager(&sink);
        let dir = tempfile::tempdir().expect("tempdir");
        let info = manager
            .spawn(dir.path(), Some("/bin/sh -c 'echo bye-now'".into()), None, None)
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
        // P3-6 — and the machine-readable `terminal:exit` event must fire
        // exactly once for the natural exit (the in-stream notice is for
        // humans only; the frontend keys off this event).
        assert!(sink.exited(&info.terminal_id), "terminal:exit must fire");
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
                .spawn(dir.path(), Some("/bin/sh -c 'sleep 30'".into()), None, None)
                .expect("spawn");
            ids.push(info.terminal_id);
        }
        assert_eq!(manager.list().len(), MAX_TERMINALS);
        let err = manager
            .spawn(dir.path(), Some("/bin/sh -c 'sleep 30'".into()), None, None)
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
                .spawn(dir.path(), Some("/bin/sh -c 'sleep 30'".into()), None, None)
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
            .spawn(dir.path(), Some("/bin/sh -c 'sleep 30'".into()), None, None)
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
            .spawn(dir.path(), Some("/bin/sh".into()), None, None)
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
            .spawn(&PathBuf::from("/nonexistent/dir/for/terminal"), None, None, None)
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
            .spawn(dir.path(), Some("/bin/sh -c 'sleep 30'".into()), None, None)
            .expect("spawn");
        // 3 MiB in a single push: the 2 MiB cap drops the oldest 1 MiB and
        // the pump must ship the retained 2 MiB as ≤256 KiB events.
        let flood = vec![b'f'; 3 * 1024 * 1024];
        manager.test_push_pending(&info.terminal_id, &flood);
        manager.pump_once_for_test();

        let emissions = sink.emissions.lock().unwrap();
        assert!(!emissions.is_empty());
        let total: usize = emissions.iter().map(|(_, _, bytes)| bytes.len()).sum();
        for (id, _, bytes) in emissions.iter() {
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
        let head = String::from_utf8_lossy(&emissions[0].2);
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
            project_dir_raw: Some("/tmp".into()),
            shell: "/bin/sh".into(),
            started_at_ms: ms,
            spawned_by_window: None,
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
