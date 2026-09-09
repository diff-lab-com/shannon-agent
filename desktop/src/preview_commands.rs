//! P1-5 C-1 — dev-server preview: detection, lifecycle, capture.
//!
//! Frozen contract (Tauri commands): `preview_detect({projectDir})`,
//! `preview_start({projectDir}) -> {url}`, `preview_stop()`,
//! `preview_status()`, `preview_capture() -> {imageBase64}` (plus the
//! additive `preview_logs` query for the ring buffer).
//!
//! # Process discipline (mirrors task-2 / inbox_commands)
//!
//! The dev-server child process is owned by the `PreviewManager` that
//! lives on `AppState`:
//!
//! * exactly one preview instance — a second `preview_start` returns the
//!   existing URL (idempotent) instead of spawning a duplicate;
//! * the child is spawned in its own process group (unix) and stopped via
//!   `killpg(SIGKILL)` so the whole `npm → node` tree dies (task-2
//!   terminal-state discipline: a dead child must never linger as a
//!   phantom `running` — an exit watcher clears the slot and logs it);
//! * `kill_on_drop(true)` is the app-exit backstop — when the manager
//!   drops (app quit / main window destroyed) the child is killed even if
//!   the explicit `shutdown_now` hook never ran;
//! * stdout/stderr stream into a ≤500-line ring buffer, queryable via
//!   `preview_logs`.
//!
//! # Security
//!
//! * Only npm/pnpm/yarn `dev`-class scripts are ever executed, and always
//!   as a direct `Command` (no shell), with cwd pinned to the project dir.
//! * Explicit `.shannon/preview.json` configs go through the same
//!   package-manager whitelist and localhost-only URL validation; invalid
//!   entries are ignored (with a warn) and convention detection applies.
//! * Preview URLs must point at `localhost` / `127.0.0.1` — the panel is
//!   never pointed at a remote host.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::net::{IpAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Default candidate ports, in probe order (brief: 3000/5173/8080/4200/1420).
pub const CANDIDATE_PORTS: [u16; 5] = [3000, 5173, 8080, 4200, 1420];

/// Ring-buffer cap (brief: ≤500 lines).
pub const MAX_LOG_LINES: usize = 500;

/// Package managers whose `dev` scripts may be spawned (whitelist).
const ALLOWED_PACKAGE_MANAGERS: [&str; 3] = ["npm", "pnpm", "yarn"];

/// How long `preview_start` waits for the server port to open.
const READY_TIMEOUT: Duration = Duration::from_secs(30);
/// Interval between readiness probes / child-exit polls.
const READINESS_TICK: Duration = Duration::from_millis(250);

// ── DTOs (wire shape: camelCase) ─────────────────────────────────────────

/// A detected dev-server launch recipe (frozen: `{ command, url }`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewDevServer {
    pub command: String,
    pub url: String,
}

/// `preview_detect` response (frozen: `{ devServer: ... | null }`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewDetectResponse {
    pub dev_server: Option<PreviewDevServer>,
}

/// `preview_start` response (frozen: `{ url }`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewStartResponse {
    pub url: String,
}

/// `preview_status` response (frozen).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PreviewStatus {
    pub running: bool,
    pub url: Option<String>,
    pub started_at_ms: Option<i64>,
}

/// One ring-buffer log line (`preview_logs`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewLogLine {
    pub ts_ms: i64,
    /// `stdout` | `stderr` | `system`
    pub stream: String,
    pub text: String,
}

/// `preview_capture` response (frozen: `{ imageBase64 }`, plus dimensions).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapturedImageDto {
    pub image_base64: String,
    pub media_type: String,
    pub width: u32,
    pub height: u32,
    /// Present when the app-window grab failed and the primary MONITOR was
    /// captured instead (`"monitor"`): the image is the whole screen, not an
    /// isolated preview capture. Incremental disclosure — absent on the
    /// normal window path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback: Option<String>,
}

// ── Detection (pure, unit-tested) ────────────────────────────────────────

/// Validate and normalize a preview URL: must parse as http(s) with host
/// `localhost` or a loopback IP — never a remote host.
fn normalize_localhost_url(raw: &str) -> Option<String> {
    let parsed = url::Url::parse(raw).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    let host = parsed.host_str()?.trim_matches(['[', ']']);
    let host_ok = host == "localhost"
        || host
            .parse::<IpAddr>()
            .map(|ip| ip.is_loopback())
            .unwrap_or(false);
    if !host_ok {
        return None;
    }
    Some(parsed.as_str().trim_end_matches('/').to_string())
}

/// TCP-connect probe: is something accepting on this URL's host:port?
/// Only loopback endpoints are connectable here — non-loopback resolved
/// addresses are filtered out so detection never probes remote hosts.
fn probe_url(url: &str, timeout: Duration) -> bool {
    use std::net::ToSocketAddrs;
    let Ok(parsed) = url::Url::parse(url) else {
        return false;
    };
    let Some(host) = parsed.host_str() else {
        return false;
    };
    let Some(port) = parsed.port_or_known_default() else {
        return false;
    };
    let Ok(addrs) = format!("{host}:{port}").to_socket_addrs() else {
        return false;
    };
    addrs
        .into_iter()
        .any(|a| a.ip().is_loopback() && TcpStream::connect_timeout(&a, timeout).is_ok())
}

/// Framework-conventional port for a dev script (brief's probe order seeds
/// the guess: vite → 5173, angular → 4200, tauri → 1420, vue-cli/webpack →
/// 8080; next/cra/unknown → 3000).
fn guess_dev_port(script: &str) -> u16 {
    if script.contains("vite") {
        5173
    } else if script.contains("ng serve") {
        4200
    } else if script.contains("tauri") {
        1420
    } else if script.contains("vue-cli-service") || script.contains("webpack") {
        8080
    } else {
        3000
    }
}

/// Candidate URLs for a project: the framework guess first, then the
/// remaining well-known ports in brief-mandated order.
fn candidate_urls(dev_script: Option<&str>) -> Vec<String> {
    let mut ports = Vec::new();
    if let Some(script) = dev_script {
        ports.push(guess_dev_port(script));
    }
    for p in CANDIDATE_PORTS {
        if !ports.contains(&p) {
            ports.push(p);
        }
    }
    ports
        .into_iter()
        .map(|p| format!("http://127.0.0.1:{p}"))
        .collect()
}

/// Which package manager should drive `run dev`? Lockfile is the signal:
/// pnpm-lock.yaml → pnpm, yarn.lock → yarn, otherwise npm.
fn package_manager_for(project_dir: &Path) -> &'static str {
    if project_dir.join("pnpm-lock.yaml").is_file() {
        "pnpm"
    } else if project_dir.join("yarn.lock").is_file() {
        "yarn"
    } else {
        "npm"
    }
}

/// `dev`-class script-name check: `dev`, `dev:*`, `dev-*`.
fn is_dev_class_script(name: &str) -> bool {
    name == "dev" || name.starts_with("dev:") || name.starts_with("dev-")
}

/// Validate a candidate command line: first token must be a whitelisted
/// package manager and the line must target a `dev`-class script. No shell
/// is involved — the string is split and exec'd directly — but shell
/// metacharacters are rejected anyway so the whitelist stays auditable.
fn validate_dev_command(command: &str) -> Result<(), String> {
    let tokens: Vec<&str> = command.split_whitespace().collect();
    let Some(program) = tokens.first() else {
        return Err("empty command".into());
    };
    let program_name = program.rsplit(['/', '\\']).next().unwrap_or(program);
    if !ALLOWED_PACKAGE_MANAGERS.contains(&program_name) {
        return Err(format!(
            "command '{program_name}' is not an allowed package manager (expected one of {ALLOWED_PACKAGE_MANAGERS:?})"
        ));
    }
    let has_dev_token = tokens.iter().enumerate().skip(1).any(|(i, token)| {
        (token == &"run" && tokens.get(i + 1).is_some_and(|s| is_dev_class_script(s)))
            || is_dev_class_script(token)
    });
    if !has_dev_token {
        return Err("command must target a dev-class script (dev, dev:*, dev-*)".into());
    }
    if command.contains([';', '|', '&', '>', '<', '`', '$', '\n']) {
        return Err("command contains shell metacharacters".into());
    }
    Ok(())
}

/// Convention launch command for a project (`<pm> run dev`).
fn convention_command(project_dir: &Path) -> String {
    format!("{} run dev", package_manager_for(project_dir))
}

/// Read `scripts.dev` from a project's package.json.
fn read_dev_script(project_dir: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(project_dir.join("package.json")).ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&raw).ok()?;
    parsed
        .get("scripts")?
        .get("dev")?
        .as_str()
        .map(str::to_string)
}

/// Explicit project override: `.shannon/preview.json` —
/// `{ "command": "...", "url": "..." }`. `Err(reason)` when the file is
/// absent or invalid (callers fall back to convention detection).
fn load_explicit_config(project_dir: &Path) -> Result<PreviewDevServer, String> {
    const NO_FILE: &str = "no .shannon/preview.json";
    let path = project_dir.join(".shannon").join("preview.json");
    if !path.is_file() {
        return Err(NO_FILE.into());
    }
    let raw =
        std::fs::read_to_string(&path).map_err(|e| format!("reading {}: {e}", path.display()))?;
    let parsed: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| format!("parsing {}: {e}", path.display()))?;
    let command = parsed
        .get("command")
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("{}: missing string field 'command'", path.display()))?
        .to_string();
    let url = parsed
        .get("url")
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("{}: missing string field 'url'", path.display()))?
        .to_string();
    validate_dev_command(&command).map_err(|e| format!("{}: {e}", path.display()))?;
    let normalized = normalize_localhost_url(&url).ok_or_else(|| {
        format!(
            "{}: url must be localhost/127.0.0.1 http(s)",
            path.display()
        )
    })?;
    Ok(PreviewDevServer {
        command,
        url: normalized,
    })
}

/// Detection matrix: explicit `.shannon/preview.json` wins; else convention
/// (`package.json` scripts.dev + lockfile package manager), with the URL
/// resolved as first-listening candidate or the framework guess.
fn detect_preview_config(project_dir: &Path) -> Option<PreviewDevServer> {
    detect_with_probe(project_dir, &|url| {
        probe_url(url, Duration::from_millis(200))
    })
}

/// Probe-injected variant (hermetic tests pass a no-op probe).
fn detect_with_probe(
    project_dir: &Path,
    probe: &impl Fn(&str) -> bool,
) -> Option<PreviewDevServer> {
    const NO_FILE: &str = "no .shannon/preview.json";
    match load_explicit_config(project_dir) {
        Ok(explicit) => return Some(explicit),
        Err(reason) if reason != NO_FILE => {
            tracing::warn!(reason = %reason, "preview: ignoring invalid .shannon/preview.json");
        }
        Err(_) => {}
    }

    let dev_script = read_dev_script(project_dir)?.trim().to_string();
    if dev_script.is_empty() {
        return None;
    }
    let candidates = candidate_urls(Some(&dev_script));
    // If something already serves on a candidate port, report that URL;
    // otherwise hand back the framework-conventional guess.
    let url = candidates
        .iter()
        .find(|url| probe(url))
        .cloned()
        .unwrap_or_else(|| candidates[0].clone());
    Some(PreviewDevServer {
        command: convention_command(project_dir),
        url,
    })
}

// ── Capture source (injectable; tests use a mock) ────────────────────────

/// Pixel producer for `preview_capture`. Production impl snapshots the
/// Shannon app window (which contains the live-preview iframe); tests
/// inject a fixture source.
pub trait PreviewCaptureSource: Send + Sync {
    fn capture(&self) -> Result<CapturedImageDto, String>;
}

/// Production capture: xcap window grab → PNG → base64, with a tagged
/// whole-screen fallback.
///
/// Path rationale (brief allows a self-chosen implementation): Tauri 2.11
/// exposes no webview screenshot API, and a cross-origin iframe
/// (`tauri://localhost` parent vs `http://127.0.0.1:port` dev server)
/// cannot be canvas-captured from the frontend. The same `xcap` capture
/// path the computer-use tool uses is applied, in this order:
///
/// 1. **The Shannon app window** (main or session windows) — matched by
///    *application name* only. A window-title substring would also match
///    an unrelated browser tab that happens to have "shannon" on the page;
///    `app_name` cannot. On this path the image is the panel the user
///    already sees inside Shannon.
/// 2. **Fallback: the primary monitor.** That image IS the whole screen —
///    the payload is therefore explicitly tagged `fallback: "monitor"`,
///    a warning is logged, and the `preview_screenshot` engine tool tells
///    the model it is looking at the entire screen, not the panel.
///
/// Other applications' windows are never captured on either path.
pub struct AppWindowCapture;

#[cfg(feature = "preview-capture")]
impl PreviewCaptureSource for AppWindowCapture {
    fn capture(&self) -> Result<CapturedImageDto, String> {
        use base64::Engine as _;

        fn encode(image: image::RgbaImage) -> Result<CapturedImageDto, String> {
            let (width, height) = (image.width(), image.height());
            let mut png = Vec::new();
            image
                .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
                .map_err(|e| format!("PNG encoding failed: {e}"))?;
            Ok(CapturedImageDto {
                image_base64: base64::engine::general_purpose::STANDARD.encode(png),
                media_type: "image/png".into(),
                width,
                height,
                fallback: None,
            })
        }

        let window = xcap::Window::all()
            .map_err(|e| format!("window enumeration failed: {e}"))?
            .into_iter()
            .find(|w| {
                w.app_name()
                    .map(|name| name.to_lowercase().contains("shannon"))
                    .unwrap_or(false)
            });
        if let Some(image) = window.and_then(|w| w.capture_image().ok()) {
            return encode(image);
        }

        let image = xcap::Monitor::all()
            .ok()
            .and_then(|monitors| monitors.into_iter().next())
            .and_then(|monitor| monitor.capture_image().ok())
            .ok_or_else(|| "no capturable Shannon window or monitor found".to_string())?;
        tracing::warn!(
            "preview capture: Shannon app window not found — falling back to the entire monitor"
        );
        let mut dto = encode(image)?;
        dto.fallback = Some("monitor".into());
        Ok(dto)
    }
}

#[cfg(not(feature = "preview-capture"))]
impl PreviewCaptureSource for AppWindowCapture {
    fn capture(&self) -> Result<CapturedImageDto, String> {
        Err("shannon-desktop was built without the `preview-capture` feature".into())
    }
}

// ── PreviewManager (lifecycle owner on AppState) ─────────────────────────

struct RunningPreview {
    child: tokio::process::Child,
    /// Process-group id (== child pid, unix) for tree kill.
    #[cfg_attr(not(unix), allow(dead_code))]
    pgid: Option<u32>,
    url: String,
    command: String,
    started_at_ms: i64,
}

/// One ring-buffer entry. `seq` is a monotonically increasing cursor so
/// consumers can scan "everything newer than X" rotation-safely — an index
/// into the sliding window would silently skip lines once the ring wraps
/// at [`MAX_LOG_LINES`].
#[derive(Clone)]
struct LogEntry {
    seq: u64,
    line: PreviewLogLine,
}

struct PreviewInner {
    running: Option<RunningPreview>,
    logs: VecDeque<LogEntry>,
    next_seq: u64,
}

/// State shared between the manager and its background pipe-drain /
/// exit-watcher tasks. Tasks hold *this* handle only — never the manager —
/// so dropping the last `Arc<PreviewManager>` (app teardown) drops the
/// child and `kill_on_drop` fires even without the explicit hook.
struct PreviewShared {
    inner: StdMutex<PreviewInner>,
    /// Live exit-watcher task count (spawn → 1, every exit path → −1);
    /// makes watcher leaks observable (stop() must retire the watcher).
    live_watchers: std::sync::atomic::AtomicUsize,
}

impl PreviewShared {
    fn new() -> Self {
        Self {
            inner: StdMutex::new(PreviewInner {
                running: None,
                logs: VecDeque::new(),
                next_seq: 0,
            }),
            live_watchers: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, PreviewInner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Append one line to the ring buffer (≤ [`MAX_LOG_LINES`]).
    fn push_log(&self, stream: &str, text: &str) {
        let mut inner = self.lock();
        if inner.logs.len() >= MAX_LOG_LINES {
            inner.logs.pop_front();
        }
        let seq = inner.next_seq;
        inner.next_seq += 1;
        inner.logs.push_back(LogEntry {
            seq,
            line: PreviewLogLine {
                ts_ms: now_ms(),
                stream: stream.to_string(),
                text: text.to_string(),
            },
        });
    }

    /// Most recent `limit` lines (default all, capped at the ring size).
    fn logs(&self, limit: Option<usize>) -> Vec<PreviewLogLine> {
        let inner = self.lock();
        let skip = inner
            .logs
            .len()
            .saturating_sub(limit.unwrap_or(MAX_LOG_LINES).min(MAX_LOG_LINES));
        inner
            .logs
            .iter()
            .skip(skip)
            .map(|entry| entry.line.clone())
            .collect()
    }

    /// Entries with `seq >= first_seq`, oldest first. The cursor passed by
    /// callers is "next unseen seq" (`last seen + 1`), starting at 0.
    fn lines_since(&self, first_seq: u64) -> Vec<LogEntry> {
        self.lock()
            .logs
            .iter()
            .filter(|entry| entry.seq >= first_seq)
            .cloned()
            .collect()
    }

    /// Current lifecycle status (frozen `preview_status` shape).
    fn status(&self) -> PreviewStatus {
        match &self.lock().running {
            Some(running) => PreviewStatus {
                running: true,
                url: Some(running.url.clone()),
                started_at_ms: Some(running.started_at_ms),
            },
            None => PreviewStatus {
                running: false,
                url: None,
                started_at_ms: None,
            },
        }
    }

    fn running_url(&self) -> Option<String> {
        self.lock().running.as_ref().map(|r| r.url.clone())
    }

    fn set_running_url(&self, url: &str) {
        let mut inner = self.lock();
        if let Some(running) = inner.running.as_mut() {
            running.url = url.to_string();
        }
    }

    fn insert_running(&self, running: RunningPreview) {
        self.lock().running = Some(running);
    }

    fn take_running(&self) -> Option<RunningPreview> {
        self.lock().running.take()
    }

    /// Poll the child; when it has exited, clear the slot and return the
    /// terminal state (task-2 discipline: no phantom `running`).
    fn poll_child_exit(&self) -> ChildPoll {
        let mut inner = self.lock();
        let Some(running) = inner.running.as_mut() else {
            // Slot already taken (user stop / previous poll) — the watcher
            // must retire instead of spinning on an empty slot forever.
            return ChildPoll::NoChild;
        };
        let Some(exited) = running.child.try_wait().ok().flatten() else {
            return ChildPoll::Running;
        };
        inner.running = None;
        ChildPoll::Exited {
            code: exited.code(),
            success: exited.success(),
        }
    }
}

/// Decrements the shared watcher-liveness counter when the watcher task
/// ends, on every exit path. No-op when the manager (and with it the
/// counter) is already gone.
struct WatcherGuard {
    shared: std::sync::Weak<PreviewShared>,
}

impl Drop for WatcherGuard {
    fn drop(&mut self) {
        if let Some(shared) = self.shared.upgrade() {
            shared
                .live_watchers
                .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        }
    }
}

/// Result of one child-exit poll.
enum ChildPoll {
    Running,
    Exited { code: Option<i32>, success: bool },
    NoChild,
}

/// Single-preview-instance owner. Held on `AppState` as `Arc`.
///
/// Uses `std::sync::Mutex`es (every critical section is await-free) so the
/// sync `shannon_tools::preview::PreviewAccess` bridge can query it.
pub struct PreviewManager {
    shared: Arc<PreviewShared>,
    capture_source: StdMutex<Arc<dyn PreviewCaptureSource>>,
    /// Test seam: readiness/exit-poll tick.
    tick: Duration,
    /// Test seam: readiness wait cap.
    ready_timeout: Duration,
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

impl Default for PreviewManager {
    fn default() -> Self {
        Self::new()
    }
}

impl PreviewManager {
    pub fn new() -> Self {
        Self {
            shared: Arc::new(PreviewShared::new()),
            capture_source: StdMutex::new(Arc::new(AppWindowCapture)),
            tick: READINESS_TICK,
            ready_timeout: READY_TIMEOUT,
        }
    }

    /// Construct with short timings (lifecycle tests).
    #[cfg(test)]
    fn with_test_timings(tick: Duration, ready_timeout: Duration) -> Self {
        let mut manager = Self::new();
        manager.tick = tick;
        manager.ready_timeout = ready_timeout;
        manager
    }

    pub fn set_capture_source(&self, source: Arc<dyn PreviewCaptureSource>) {
        *self
            .capture_source
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = source;
    }

    fn capture_source(&self) -> Arc<dyn PreviewCaptureSource> {
        self.capture_source
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    /// Most recent log lines (frozen-adjacent `preview_logs`).
    pub fn logs(&self, limit: Option<usize>) -> Vec<PreviewLogLine> {
        self.shared.logs(limit)
    }

    /// Current lifecycle status (frozen `preview_status` shape).
    pub fn status(&self) -> PreviewStatus {
        self.shared.status()
    }

    /// Detection (frozen `preview_detect` shape).
    pub fn detect(&self, project_dir: &Path) -> Option<PreviewDevServer> {
        detect_preview_config(project_dir)
    }

    /// Spawn the dev server. Idempotent: when an instance is already
    /// running its URL is returned unchanged (brief: choose idempotent).
    pub async fn start(self: Arc<Self>, project_dir: &Path) -> Result<String, String> {
        if let Some(url) = self.shared.running_url() {
            return Ok(url);
        }
        let dir = project_dir
            .canonicalize()
            .map_err(|e| format!("project dir {}: {e}", project_dir.display()))?;
        let Some(config) = detect_preview_config(&dir) else {
            return Err(format!(
                "no dev server detected in {} (no .shannon/preview.json, no package.json scripts.dev)",
                dir.display()
            ));
        };
        validate_dev_command(&config.command)?;
        self.spawn_config(&dir, &config).await
    }

    /// Core spawn path used by [`Self::start`] (which has already validated
    /// the config).
    async fn spawn_config(
        self: Arc<Self>,
        dir: &Path,
        config: &PreviewDevServer,
    ) -> Result<String, String> {
        if let Some(existing) = self.shared.running_url() {
            return Ok(existing);
        }

        let tokens: Vec<String> = config
            .command
            .split_whitespace()
            .map(str::to_string)
            .collect();
        let (program, args) = tokens.split_first().ok_or("empty command")?;

        self.shared.push_log(
            "system",
            &format!("starting `{}` in {}", config.command, dir.display()),
        );

        let mut cmd = tokio::process::Command::new(program);
        cmd.args(args)
            .current_dir(dir)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            // App-exit backstop: dropping the manager (app quit) kills the
            // child even if the explicit shutdown hook never ran.
            .kill_on_drop(true);
        // Own process group so `killpg` takes down the whole npm→node tree.
        #[cfg(unix)]
        cmd.process_group(0);

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("spawning `{}` failed: {e}", config.command))?;
        let pgid = child.id();
        if let Some(stdout) = child.stdout.take() {
            self.spawn_log_drain(stdout, "stdout");
        }
        if let Some(stderr) = child.stderr.take() {
            self.spawn_log_drain(stderr, "stderr");
        }

        self.shared.insert_running(RunningPreview {
            child,
            pgid,
            url: config.url.clone(),
            command: config.command.clone(),
            started_at_ms: now_ms(),
        });
        self.spawn_exit_watcher();

        // Wait (outside the lock) until the port answers, adopting any
        // localhost URL the server prints if it picked a non-default port.
        Ok(self.wait_ready(&config.url).await)
    }

    /// Test seam: spawn an arbitrary (validated-elsewhere) command through
    /// the exact production spawn/readiness/watcher pipeline.
    #[cfg(test)]
    async fn start_raw(
        self: Arc<Self>,
        dir: &Path,
        program: &str,
        args: &[&str],
        url: &str,
    ) -> Result<String, String> {
        let config = PreviewDevServer {
            command: format!("{program} {}", args.join(" ")),
            url: url.to_string(),
        };
        self.spawn_config(dir, &config).await
    }

    /// Background task: pipe lines from the child into the ring buffer.
    ///
    /// Holds the shared state only across one bounded wait per iteration
    /// (a `Weak` handle in between) so dropping the last
    /// `Arc<PreviewManager>` (app teardown) promptly drops the child —
    /// that is what arms the `kill_on_drop` backstop.
    fn spawn_log_drain(
        &self,
        pipe: impl tokio::io::AsyncRead + Unpin + Send + 'static,
        stream: &'static str,
    ) {
        let shared = Arc::downgrade(&self.shared);
        tokio::spawn(async move {
            use tokio::io::{AsyncBufReadExt, BufReader};
            let reader = BufReader::new(pipe);
            let mut lines = reader.lines();
            loop {
                // No strong handle is held across the read await: if the
                // manager is dropped (app teardown) the child drops and is
                // killed immediately, and this task sees EOF. Liveness is
                // observed via the Weak's strong count.
                if shared.strong_count() == 0 {
                    break;
                }
                match tokio::time::timeout(Duration::from_millis(100), lines.next_line()).await {
                    Ok(Ok(Some(line))) => {
                        let Some(shared) = shared.upgrade() else {
                            break;
                        };
                        shared.push_log(stream, &line);
                    }
                    // A quiet pipe is normal (vite/next go silent after
                    // their banner): keep reading — stopping here would
                    // freeze the ring buffer and eventually block the child
                    // on a full stdout pipe. Only EOF or a read error ends
                    // the drain.
                    Err(_idle) => continue,
                    _ => break,
                }
            }
        });
    }

    /// Background task: poll the child; when it exits (crash or natural),
    /// clear the running slot and log the terminal state (task-2
    /// discipline: a dead child must never leave a phantom `running`).
    fn spawn_exit_watcher(&self) {
        let shared = Arc::downgrade(&self.shared);
        let tick = self.tick;
        if let Some(strong) = shared.upgrade() {
            strong
                .live_watchers
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        let guard = WatcherGuard {
            shared: shared.clone(),
        };
        tokio::spawn(async move {
            let _guard = guard;
            loop {
                tokio::time::sleep(tick).await;
                let Some(shared) = shared.upgrade() else {
                    break;
                };
                match shared.poll_child_exit() {
                    ChildPoll::Running => {}
                    // Slot taken by stop() (or a prior poll): retire —
                    // looping here would leak a ~4Hz polling task per
                    // start/stop cycle.
                    ChildPoll::NoChild => break,
                    ChildPoll::Exited { code, success } => {
                        shared.push_log(
                            "system",
                            &format!(
                                "dev server exited (code: {}, success: {success})",
                                code.map(|c| c.to_string())
                                    .unwrap_or_else(|| "signal".into())
                            ),
                        );
                        break;
                    }
                }
                drop(shared);
            }
        });
    }

    /// Probe the configured URL until it answers (or the wait times out —
    /// the URL is returned either way; slow servers just load later).
    /// Along the way, adopt any localhost URL the server prints to its
    /// logs (vite/next auto-port-increment case).
    async fn wait_ready(&self, configured: &str) -> String {
        let deadline = Instant::now() + self.ready_timeout;
        let mut url = configured.to_string();
        // Rotation-safe scan cursor: `skip(index)` over the sliding window
        // would miss newly wrapped lines once the ring is at its 500-line
        // cap, so scan strictly newer-than-last-seen instead.
        let mut next_seq = 0u64;
        loop {
            if probe_url(&url, Duration::from_millis(200)) {
                self.shared
                    .push_log("system", &format!("dev server ready at {url}"));
                return url;
            }
            // Adopt a printed URL when the configured one is still closed.
            for entry in self.shared.lines_since(next_seq) {
                next_seq = entry.seq + 1;
                if let Some(printed) = extract_localhost_url(&entry.line.text) {
                    if printed != url && probe_url(&printed, Duration::from_millis(200)) {
                        self.shared
                            .push_log("system", &format!("adopting dev server url {printed}"));
                        self.shared.set_running_url(&printed);
                        return printed;
                    }
                    if printed != url {
                        url = printed;
                    }
                }
            }
            if Instant::now() >= deadline {
                self.shared.push_log(
                    "system",
                    &format!("dev server not reachable yet at {url} — continuing"),
                );
                return url;
            }
            tokio::time::sleep(self.tick).await;
        }
    }

    /// Stop the preview (frozen `preview_stop`). Kills the whole process
    /// group (unix) so npm wrapper + node children all die. Returns whether
    /// something was running.
    pub fn stop(&self) -> bool {
        let Some(mut running) = self.shared.take_running() else {
            return false;
        };
        kill_process_tree(&mut running);
        self.shared.push_log(
            "system",
            &format!("preview stopped (`{}`)", running.command),
        );
        true
    }

    /// App-exit hook (same as [`Self::stop`]; kept separate for intent).
    pub fn shutdown_now(&self) {
        self.stop();
    }

    /// Capture pixels of the current preview (frozen `preview_capture`).
    pub fn capture(&self) -> Result<CapturedImageDto, String> {
        if self.shared.running_url().is_none() {
            return Err("no preview running — start it from the Live tab first".into());
        }
        let dto = self.capture_source().capture()?;
        if dto.fallback.is_some() {
            self.shared.push_log(
                "system",
                "capture fell back to the entire monitor — payload tagged fallback=monitor",
            );
        }
        Ok(dto)
    }

    /// Live exit-watcher task count (test seam for leak regressions).
    #[cfg(test)]
    fn live_watchers(&self) -> usize {
        self.shared
            .live_watchers
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    #[cfg(test)]
    fn running_pid(&self) -> Option<u32> {
        self.shared.lock().running.as_ref().and_then(|r| r.pgid)
    }

    /// Test seam: seed the ring buffer (drain-task equivalent).
    #[cfg(test)]
    fn push_log(&self, stream: &str, text: &str) {
        self.shared.push_log(stream, text);
    }
}

/// Send SIGKILL to the process group, falling back to direct child kill.
fn kill_process_tree(running: &mut RunningPreview) {
    #[cfg(unix)]
    if let Some(pgid) = running.pgid {
        // Negative pid → the whole group (npm wrapper + node children).
        unsafe { libc::kill(-(pgid as i32), libc::SIGKILL) };
        return;
    }
    let _ = running.child.start_kill();
}

/// Pull the first `http://localhost:PORT/...` (or 127.0.0.1) URL out of a
/// dev-server log line, if any.
fn extract_localhost_url(text: &str) -> Option<String> {
    const NEEDLE: &str = "http://";
    let start = text.find(NEEDLE)?;
    let rest = &text[start..];
    let end = rest
        .find(|c: char| c.is_whitespace() || c == '"' || c == '\'')
        .unwrap_or(rest.len());
    normalize_localhost_url(&rest[..end])
}

// ── Tauri commands (frozen contract) ─────────────────────────────────────

/// Resolve the effective project dir: explicit arg or the current session
/// working directory (desktop config mirror — session switches keep it in
/// sync via `set_session_working_dir`).
async fn resolve_project_dir(
    state: &tauri::State<'_, crate::commands::AppState>,
    project_dir: Option<String>,
) -> Result<PathBuf, String> {
    match project_dir {
        Some(dir) if !dir.trim().is_empty() => Ok(PathBuf::from(dir)),
        _ => Ok(crate::commands_agents::resolve_working_dir(state).await),
    }
}

/// `preview_detect({projectDir}) -> { devServer: {command,url} | null }`
#[tauri::command]
pub async fn preview_detect(
    state: tauri::State<'_, crate::commands::AppState>,
    project_dir: Option<String>,
) -> Result<PreviewDetectResponse, String> {
    let dir = resolve_project_dir(&state, project_dir).await?;
    Ok(PreviewDetectResponse {
        dev_server: state.preview.detect(&dir),
    })
}

/// `preview_start({projectDir}) -> { url }` — idempotent.
#[tauri::command]
pub async fn preview_start(
    state: tauri::State<'_, crate::commands::AppState>,
    project_dir: Option<String>,
) -> Result<PreviewStartResponse, String> {
    let dir = resolve_project_dir(&state, project_dir).await?;
    let url = state.preview.clone().start(&dir).await?;
    Ok(PreviewStartResponse { url })
}

/// `preview_stop()`
#[tauri::command]
pub async fn preview_stop(
    state: tauri::State<'_, crate::commands::AppState>,
) -> Result<(), String> {
    state.preview.stop();
    Ok(())
}

/// `preview_status() -> { running, url, startedAtMs }`
#[tauri::command]
pub async fn preview_status(
    state: tauri::State<'_, crate::commands::AppState>,
) -> Result<PreviewStatus, String> {
    Ok(state.preview.status())
}

/// `preview_capture() -> { imageBase64, mediaType, width, height }`
#[tauri::command]
pub async fn preview_capture(
    state: tauri::State<'_, crate::commands::AppState>,
) -> Result<CapturedImageDto, String> {
    // xcap grabs are blocking (~100ms); keep them off the async executors.
    let manager = state.preview.clone();
    tokio::task::spawn_blocking(move || manager.capture())
        .await
        .map_err(|e| format!("capture task failed: {e}"))?
}

/// `preview_logs(limit?)` — additive: ring-buffer tail for the Live tab.
#[tauri::command]
pub async fn preview_logs(
    state: tauri::State<'_, crate::commands::AppState>,
    limit: Option<usize>,
) -> Result<Vec<PreviewLogLine>, String> {
    Ok(state.preview.logs(limit))
}

/// Sync shutdown for the main-window-destroyed hook in `main.rs`.
pub fn shutdown_on_exit(state: &crate::commands::AppState) {
    state.preview.shutdown_now();
}

/// Sync `PreviewAccess` bridge binding the engine tool (`preview_screenshot`)
/// to the live manager. Registered only on the desktop surface — the CLI's
/// `register_default_tools` never sees this tool.
pub struct ManagerPreviewAccess {
    manager: Arc<PreviewManager>,
}

impl ManagerPreviewAccess {
    pub fn new(manager: Arc<PreviewManager>) -> Self {
        Self { manager }
    }
}

impl shannon_tools::preview::PreviewAccess for ManagerPreviewAccess {
    fn status(&self) -> shannon_tools::preview::PreviewStatusInfo {
        let status = self.manager.status();
        shannon_tools::preview::PreviewStatusInfo {
            running: status.running,
            url: status.url,
            started_at_ms: status.started_at_ms,
        }
    }

    fn capture(&self) -> Result<shannon_tools::preview::PreviewScreenshot, String> {
        let shot = self.manager.capture()?;
        Ok(shannon_tools::preview::PreviewScreenshot {
            image_base64: shot.image_base64,
            media_type: shot.media_type,
            width: shot.width,
            height: shot.height,
            fallback: shot.fallback,
        })
    }
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine as _;
    use std::net::TcpListener;
    use std::sync::Arc;

    /// 1x1 transparent PNG fixture for capture format/size assertions.
    const PNG_1X1_BASE64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";

    fn write_project(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        for (path, contents) in files {
            let full = dir.path().join(path);
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent).expect("mkdirs");
            }
            std::fs::write(&full, contents).expect("write file");
        }
        dir
    }

    fn package_json(dev: &str) -> String {
        format!(r#"{{ "name": "demo", "scripts": {{ "dev": "{dev}", "build": "vite build" }} }}"#)
    }

    // ── URL whitelist ────────────────────────────────────────────────────

    #[test]
    fn localhost_urls_are_accepted_and_normalized() {
        assert_eq!(
            normalize_localhost_url("http://localhost:5173/"),
            Some("http://localhost:5173".into())
        );
        assert_eq!(
            normalize_localhost_url("http://127.0.0.1:3000"),
            Some("http://127.0.0.1:3000".into())
        );
        assert!(normalize_localhost_url("http://[::1]:8080").is_some());
    }

    #[test]
    fn non_localhost_urls_are_rejected() {
        assert!(normalize_localhost_url("http://192.168.1.5:3000").is_none());
        assert!(normalize_localhost_url("http://example.com").is_none());
        assert!(normalize_localhost_url("http://0.0.0.0:3000").is_none());
        assert!(normalize_localhost_url("ftp://localhost:21").is_none());
        assert!(normalize_localhost_url("not a url").is_none());
    }

    #[test]
    fn probe_finds_a_bound_listener_and_skips_closed_ports() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        assert!(probe_url(
            &format!("http://127.0.0.1:{port}"),
            Duration::from_millis(300)
        ));
        // Port 9 (discard) is closed almost everywhere.
        assert!(!probe_url("http://127.0.0.1:9", Duration::from_millis(300)));
    }

    // ── Command whitelist ────────────────────────────────────────────────

    #[test]
    fn dev_class_package_manager_commands_pass_validation() {
        for ok in [
            "npm run dev",
            "pnpm dev",
            "yarn run dev:mock",
            "npm run dev-ssr",
        ] {
            assert!(validate_dev_command(ok).is_ok(), "{ok} should validate");
        }
    }

    #[test]
    fn non_whitelisted_commands_are_rejected() {
        for bad in [
            "bash -c 'rm -rf /'",
            "/usr/bin/env node server.js",
            "npm run storybook",
            "npm install",
            "npm run dev && echo pwned",
            "pnpm dev; curl evil.example",
            "",
        ] {
            assert!(
                validate_dev_command(bad).is_err(),
                "{bad} should be rejected"
            );
        }
    }

    // ── Detection matrix ─────────────────────────────────────────────────

    /// Detection with port probing disabled (hermetic: no network).
    fn detect_hermetic(dir: &std::path::Path) -> Option<PreviewDevServer> {
        detect_with_probe(dir, &|_| false)
    }

    #[test]
    fn detects_vite_with_npm_and_5173_guess() {
        let dir = write_project(&[
            ("package.json", &package_json("vite")),
            ("package-lock.json", "{}"),
        ]);
        let cfg = detect_hermetic(dir.path()).expect("detected");
        assert_eq!(cfg.command, "npm run dev");
        assert_eq!(cfg.url, "http://127.0.0.1:5173");
    }

    #[test]
    fn detects_next_on_port_3000() {
        let dir = write_project(&[("package.json", &package_json("next dev"))]);
        let cfg = detect_hermetic(dir.path()).expect("detected");
        assert_eq!(cfg.url, "http://127.0.0.1:3000");
    }

    #[test]
    fn detects_angular_on_port_4200() {
        let dir = write_project(&[("package.json", &package_json("ng serve --port 0"))]);
        let cfg = detect_hermetic(dir.path()).expect("detected");
        assert_eq!(cfg.url, "http://127.0.0.1:4200");
    }

    #[test]
    fn detects_tauri_on_port_1420_and_vue_cli_on_8080() {
        let tauri = write_project(&[("package.json", &package_json("tauri dev"))]);
        assert_eq!(
            detect_hermetic(tauri.path()).expect("detected").url,
            "http://127.0.0.1:1420"
        );
        let vue = write_project(&[("package.json", &package_json("vue-cli-service serve"))]);
        assert_eq!(
            detect_hermetic(vue.path()).expect("detected").url,
            "http://127.0.0.1:8080"
        );
    }

    #[test]
    fn lockfiles_select_the_package_manager() {
        let pnpm = write_project(&[
            ("package.json", &package_json("vite")),
            ("pnpm-lock.yaml", ""),
        ]);
        assert_eq!(
            detect_hermetic(pnpm.path()).expect("detected").command,
            "pnpm run dev"
        );
        let yarn = write_project(&[("package.json", &package_json("vite")), ("yarn.lock", "")]);
        assert_eq!(
            detect_hermetic(yarn.path()).expect("detected").command,
            "yarn run dev"
        );
    }

    #[test]
    fn unknown_scripts_fall_back_to_port_3000_first() {
        let dir = write_project(&[("package.json", &package_json("node ./scripts/serve.mjs"))]);
        let cfg = detect_hermetic(dir.path()).expect("detected");
        assert_eq!(cfg.url, "http://127.0.0.1:3000");
    }

    #[test]
    fn no_dev_script_or_no_package_json_yields_none() {
        let no_dev = write_project(&[(
            "package.json",
            r#"{ "name": "demo", "scripts": { "build": "vite build" } }"#,
        )]);
        assert!(detect_hermetic(no_dev.path()).is_none());
        assert!(detect_hermetic(tempfile::tempdir().expect("t").path()).is_none());
    }

    #[test]
    fn explicit_preview_json_wins_over_convention() {
        let dir = write_project(&[
            ("package.json", &package_json("vite")),
            (
                ".shannon/preview.json",
                r#"{ "command": "pnpm run dev:https", "url": "http://localhost:3001" }"#,
            ),
        ]);
        let cfg = detect_hermetic(dir.path()).expect("detected");
        assert_eq!(cfg.command, "pnpm run dev:https");
        assert_eq!(cfg.url, "http://localhost:3001");
    }

    #[test]
    fn invalid_explicit_config_is_ignored_and_falls_back() {
        // Non-whitelisted command in the explicit config → convention applies.
        let rogue = write_project(&[
            ("package.json", &package_json("vite")),
            (
                ".shannon/preview.json",
                r#"{ "command": "bash serve.sh", "url": "http://localhost:3001" }"#,
            ),
        ]);
        let cfg = detect_hermetic(rogue.path()).expect("convention fallback");
        assert_eq!(cfg.command, "npm run dev");

        // Remote URL in the explicit config → convention applies.
        let remote = write_project(&[
            ("package.json", &package_json("vite")),
            (
                ".shannon/preview.json",
                r#"{ "command": "pnpm run dev", "url": "http://192.168.1.5:3000" }"#,
            ),
        ]);
        assert_eq!(
            detect_hermetic(remote.path())
                .expect("convention fallback")
                .command,
            "npm run dev"
        );

        // Malformed JSON → convention applies.
        let malformed = write_project(&[
            ("package.json", &package_json("vite")),
            (".shannon/preview.json", "{ not json"),
        ]);
        assert!(detect_hermetic(malformed.path()).is_some());
    }

    #[test]
    fn detection_prefers_an_already_listening_port_over_the_guess() {
        // The vite guess (5173) is closed but 3000 is "live": the candidate
        // probe scan must report the listening port, not the raw guess.
        let dir = write_project(&[("package.json", &package_json("vite"))]);
        let cfg = detect_with_probe(dir.path(), &|url| !url.ends_with(":5173")).expect("detected");
        assert_eq!(cfg.url, "http://127.0.0.1:3000");
    }

    // ── Lifecycle (unix: process-group semantics) ────────────────────────

    #[cfg(unix)]
    fn test_manager() -> Arc<PreviewManager> {
        Arc::new(PreviewManager::with_test_timings(
            Duration::from_millis(20),
            Duration::from_millis(400),
        ))
    }

    /// True when `pid` is gone or a reaped-pending zombie.
    #[cfg(unix)]
    fn process_gone(pid: u32) -> bool {
        match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
            // No /proc entry → definitively gone (non-Linux unix).
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

    /// Variant that yields to the tokio runtime while waiting, so spawned
    /// drain/watcher tasks can make progress on the current-thread test
    /// runtime (the blocking variant starves them).
    #[cfg(unix)]
    async fn wait_until_yielding(deadline: Duration, mut cond: impl FnMut() -> bool) -> bool {
        let start = Instant::now();
        loop {
            if cond() {
                return true;
            }
            if start.elapsed() >= deadline {
                return cond();
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn start_is_idempotent_stop_kills_and_status_tracks() {
        let manager = test_manager();
        let dir = tempfile::tempdir().expect("tempdir");
        let url = "http://127.0.0.1:41230";

        let started = Arc::clone(&manager)
            .start_raw(dir.path(), "sleep", &["30"], url)
            .await;
        assert_eq!(started.expect("start"), url);
        assert_eq!(manager.status().url.as_deref(), Some(url));
        assert!(manager.status().running);
        assert!(manager.status().started_at_ms.is_some());

        // Idempotent second start: same URL, no second child.
        let again = Arc::clone(&manager)
            .start_raw(dir.path(), "sleep", &["30"], url)
            .await;
        assert_eq!(again.expect("start again"), url);

        let pid = manager.running_pid().expect("child pid");
        assert!(manager.stop(), "stop reports something was running");
        assert!(!manager.stop(), "second stop is a no-op");
        assert!(!manager.status().running);
        assert!(wait_until(Duration::from_secs(5), || process_gone(pid)));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn dropping_the_manager_kills_the_child_app_exit_backstop() {
        let manager = test_manager();
        let dir = tempfile::tempdir().expect("tempdir");
        Arc::clone(&manager)
            .start_raw(dir.path(), "sleep", &["30"], "http://127.0.0.1:41231")
            .await
            .expect("start");
        let pid = manager.running_pid().expect("child pid");
        drop(manager);
        assert!(wait_until(Duration::from_secs(5), || process_gone(pid)));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn exit_watcher_clears_the_running_slot_when_child_dies() {
        let manager = test_manager();
        let dir = tempfile::tempdir().expect("tempdir");
        Arc::clone(&manager)
            .start_raw(dir.path(), "true", &[], "http://127.0.0.1:41232")
            .await
            .expect("start");
        assert!(
            wait_until(Duration::from_secs(5), || !manager.status().running),
            "watcher must clear the slot after the child exits"
        );
        assert!(manager.logs(None).iter().any(|l| l.text.contains("exited")));
        assert!(
            wait_until_yielding(Duration::from_secs(5), || manager.live_watchers() == 0).await,
            "watcher must retire after the natural-exit path too"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn wait_ready_adopts_a_printed_localhost_url() {
        let manager = test_manager();
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        // Dev server "printed" its actual URL to stdout before we probed.
        manager.push_log(
            "stdout",
            &format!("Local: http://127.0.0.1:{port}/ ready in 12ms"),
        );
        let adopted = manager.wait_ready("http://127.0.0.1:9").await;
        assert_eq!(adopted, format!("http://127.0.0.1:{port}"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn wait_ready_times_out_gracefully_on_a_closed_port() {
        let manager = test_manager();
        let started = Instant::now();
        let url = manager.wait_ready("http://127.0.0.1:9").await;
        assert_eq!(url, "http://127.0.0.1:9");
        assert!(started.elapsed() < Duration::from_secs(3), "must not hang");
        assert!(
            manager
                .logs(None)
                .iter()
                .any(|l| l.text.contains("not reachable"))
        );
    }

    // ── Ring buffer ──────────────────────────────────────────────────────

    #[test]
    fn ring_buffer_caps_at_500_lines() {
        let manager = PreviewManager::new();
        for i in 0..600 {
            manager.push_log("stdout", &format!("line-{i}"));
        }
        let logs = manager.logs(None);
        assert_eq!(logs.len(), MAX_LOG_LINES);
        assert_eq!(logs[0].text, "line-100", "oldest lines were dropped");
        assert_eq!(logs.last().expect("non-empty").text, "line-599");
        // Limit query returns only the tail.
        assert_eq!(manager.logs(Some(10)).len(), 10);
    }

    // ── Capture ──────────────────────────────────────────────────────────

    struct MockCapture {
        fallback: Option<String>,
    }
    impl PreviewCaptureSource for MockCapture {
        fn capture(&self) -> Result<CapturedImageDto, String> {
            Ok(CapturedImageDto {
                image_base64: PNG_1X1_BASE64.into(),
                media_type: "image/png".into(),
                width: 1,
                height: 1,
                fallback: self.fallback.clone(),
            })
        }
    }

    #[test]
    fn capture_requires_a_running_preview() {
        let manager = PreviewManager::new();
        assert!(manager.capture().is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn capture_returns_base64_png_with_dimensions_from_the_source() {
        let manager = test_manager();
        manager.set_capture_source(Arc::new(MockCapture { fallback: None }));
        let dir = tempfile::tempdir().expect("tempdir");
        Arc::clone(&manager)
            .start_raw(dir.path(), "sleep", &["5"], "http://127.0.0.1:41233")
            .await
            .expect("start");

        let shot = manager.capture().expect("captured");
        assert_eq!(shot.media_type, "image/png");
        assert_eq!((shot.width, shot.height), (1, 1));
        assert!(shot.fallback.is_none());
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&shot.image_base64)
            .expect("valid base64");
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "payload is a PNG");
    }

    // ── Engine tool bridge ───────────────────────────────────────────────

    #[test]
    fn preview_access_bridge_maps_status_and_capture() {
        let manager = Arc::new(PreviewManager::new());
        manager.set_capture_source(Arc::new(MockCapture { fallback: None }));
        let bridge = ManagerPreviewAccess::new(Arc::clone(&manager));
        let idle = shannon_tools::preview::PreviewAccess::status(&bridge);
        assert!(!idle.running);

        manager.push_log("system", "boot");
        let logs = manager.logs(None);
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].stream, "system");
    }

    #[tokio::test]
    async fn detect_reports_none_for_dirs_without_a_project() {
        let manager = PreviewManager::new();
        let empty = tempfile::tempdir().expect("tempdir");
        assert!(manager.detect(empty.path()).is_none());
    }

    // ── Fix round 1 regressions ──────────────────────────────────────────

    /// A dev server that goes quiet after its banner must NOT kill the
    /// drain task: the 100ms idle timeout must keep polling, or the pipe
    /// stops being read (ring buffer freezes; the child eventually blocks
    /// on a full stdout pipe).
    #[cfg(unix)]
    #[tokio::test]
    async fn slow_dev_server_output_still_reaches_the_ring_buffer() {
        let manager = test_manager();
        let dir = tempfile::tempdir().expect("tempdir");
        Arc::clone(&manager)
            .start_raw(
                dir.path(),
                "sh",
                &["-c", "echo drain-marker-1; sleep 0.4; echo drain-marker-2; sleep 0.4; echo drain-marker-3"],
                "http://127.0.0.1:41235",
            )
            .await
            .expect("start");
        assert!(
            wait_until_yielding(Duration::from_secs(5), || {
                let logs = manager.logs(None);
                let texts: Vec<&str> = logs.iter().map(|l| l.text.as_str()).collect();
                texts.iter().any(|t| t.contains("drain-marker-1"))
                    && texts.iter().any(|t| t.contains("drain-marker-2"))
                    && texts.iter().any(|t| t.contains("drain-marker-3"))
            })
            .await,
            "all post-idle lines must reach the ring buffer, got: {:?}",
            manager
                .logs(None)
                .iter()
                .map(|l| &l.text)
                .collect::<Vec<_>>()
        );
        manager.stop();
    }

    /// The scan cursor must be sequence-based: a sliding-window index
    /// would skip everything once the ring wraps at 500 lines.
    #[test]
    fn lines_since_tracks_buffer_rotation() {
        let manager = PreviewManager::new();
        for i in 0..600 {
            manager.push_log("stdout", &format!("line-{i}"));
        }
        // Cursor after the first 500 pushes (next unseen = seq 500); the
        // buffer now only holds seq 100..=599 — 100 lines must still be
        // scannable, where a sliding-window index would return none.
        let entries = manager.shared.lines_since(500);
        assert_eq!(entries.len(), 100);
        assert_eq!(entries[0].line.text, "line-500");
        assert_eq!(entries.last().expect("non-empty").line.text, "line-599");
        // A fresh cursor still sees the whole window.
        assert_eq!(manager.shared.lines_since(0).len(), MAX_LOG_LINES);
    }

    /// stop() takes the running slot; the exit watcher must retire instead
    /// of spinning on it — start/stop cycles must not leak watcher tasks.
    #[cfg(unix)]
    #[tokio::test]
    async fn stop_reaps_the_exit_watcher_task() {
        let manager = test_manager();
        let dir = tempfile::tempdir().expect("tempdir");
        let url = "http://127.0.0.1:41236";

        Arc::clone(&manager)
            .start_raw(dir.path(), "sleep", &["30"], url)
            .await
            .expect("start");
        assert_eq!(manager.live_watchers(), 1);
        manager.stop();
        assert!(
            wait_until_yielding(Duration::from_secs(5), || manager.live_watchers() == 0).await,
            "watcher must retire after stop()"
        );

        // Repeated cycles stay at one concurrent watcher, zero afterwards.
        Arc::clone(&manager)
            .start_raw(dir.path(), "sleep", &["30"], url)
            .await
            .expect("restart");
        assert_eq!(manager.live_watchers(), 1);
        // Idempotent start must not spawn a second watcher.
        Arc::clone(&manager)
            .start_raw(dir.path(), "sleep", &["30"], url)
            .await
            .expect("idempotent start");
        assert_eq!(manager.live_watchers(), 1);
        manager.stop();
        assert!(wait_until_yielding(Duration::from_secs(5), || manager.live_watchers() == 0).await);
    }

    /// Monitor-fallback captures must be tagged and logged.
    #[cfg(unix)]
    #[tokio::test]
    async fn monitor_fallback_capture_is_tagged_and_logged() {
        let manager = test_manager();
        manager.set_capture_source(Arc::new(MockCapture {
            fallback: Some("monitor".into()),
        }));
        let dir = tempfile::tempdir().expect("tempdir");
        Arc::clone(&manager)
            .start_raw(dir.path(), "sleep", &["5"], "http://127.0.0.1:41237")
            .await
            .expect("start");
        let shot = manager.capture().expect("captured");
        assert_eq!(shot.fallback.as_deref(), Some("monitor"));
        assert!(
            manager
                .logs(None)
                .iter()
                .any(|l| l.text.contains("fallback=monitor")),
            "fallback must be logged"
        );
        manager.stop();
    }

    /// stdout of the child must flow into the ring buffer (drain task).
    #[cfg(unix)]
    #[tokio::test]
    async fn child_stdout_is_drained_into_the_ring_buffer() {
        let manager = test_manager();
        let dir = tempfile::tempdir().expect("tempdir");
        // `echo` writes one line to stdout, then stays alive briefly.
        Arc::clone(&manager)
            .start_raw(
                dir.path(),
                "sh",
                &["-c", "echo preview-marker-12345; sleep 3"],
                "http://127.0.0.1:41234",
            )
            .await
            .expect("start");
        assert!(
            wait_until(Duration::from_secs(5), || manager
                .logs(None)
                .iter()
                .any(|l| l.text.contains("preview-marker-12345"))),
            "stdout drain must feed the ring buffer"
        );
        manager.stop();
    }
}
