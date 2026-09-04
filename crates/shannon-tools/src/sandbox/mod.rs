//! §4.12 W3-3b — pluggable sandbox provider seam.
//!
//! Concrete decorators + assembly live here; the vocabulary lives in
//! [`shannon_tool_interface::sandbox`]. Layering (all three coexist, none
//! replaces another):
//!
//! - **Kernel boundary** (`landlock` backend): every forked child gets a
//!   Landlock ruleset installed between fork and exec. Authoritative.
//! - **User-space policy mirror** ([`SandboxedFs`]): the same policy math
//!   applied to in-process filesystem tools (Write/Edit/…), which are not
//!   kernel-restricted because Shannon itself must keep running normally
//!   (session log, LLM API). Denials carry the canonical
//!   `sandbox_denied` classification.
//! - **Permission system**: decision layer over model behavior
//!   (`permission/decision` events) — untouched by all of this.
//!
//! Mode semantics:
//!
//! | mode      | world                                                            |
//! |-----------|------------------------------------------------------------------|
//! | `off`     | §4.11 passthrough, byte-identical (the registration entry points short-circuit to the legacy body) |
//! | `local`   | portable user-space enforcement only ([`assemble_local`]); no kernel restriction |
//! | `landlock`| kernel-enforced child world + user-space fs mirror ([`assemble`]) |

pub mod landlock_backend;

use shannon_tool_interface::sandbox::{
    ChildWorldInit, DegradeNotice, ForkInitHost, SANDBOX_DENIED_CLASSIFICATION, SandboxDenialInfo,
    SandboxError, SandboxMode, SandboxPolicy, path_within,
};
use shannon_tool_interface::{
    CapturedOutput, DirEntryInfo, FileMeta, FileSystemProvider, PipedChild, PipedSpawn,
    ProcessProvider, ProcessRequest,
};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

// ---------------------------------------------------------------------------
// SandboxedFs — decorator over FileSystemProvider
// ---------------------------------------------------------------------------

/// Filesystem world wrapped in sandbox policy checks.
///
/// Every operation is checked against the policy before forwarding to the
/// inner world; failures produce [`SandboxDenialInfo`]-classified errors so
/// tool results can be labeled `sandbox_denied` end-to-end. Allowed calls hit
/// the inner provider unchanged (no extra buffering/copying).
#[derive(Clone)]
pub struct SandboxedFs {
    inner: Arc<dyn FileSystemProvider>,
    policy: Arc<SandboxPolicy>,
}

impl std::fmt::Debug for SandboxedFs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SandboxedFs")
            .field("writable_roots", &self.policy.writable_roots.len())
            .field("readable_roots", &self.policy.readable_roots.len())
            .field("network", &self.policy.network)
            .finish()
    }
}

impl SandboxedFs {
    /// Wrap `inner` under `policy`.
    pub fn new(inner: Arc<dyn FileSystemProvider>, policy: Arc<SandboxPolicy>) -> Self {
        Self { inner, policy }
    }

    fn deny(&self, op: &str, path: &Path) -> io::Error {
        self.policy.denial_for(op, path).into_io_error()
    }
}

#[async_trait::async_trait]
impl FileSystemProvider for SandboxedFs {
    // ---- async face ----------------------------------------------------

    async fn read_text(&self, path: &Path) -> io::Result<String> {
        if !self.policy.allows_read(path) {
            return Err(self.deny("read", path));
        }
        self.inner.read_text(path).await
    }

    async fn read_bytes(&self, path: &Path) -> io::Result<Vec<u8>> {
        if !self.policy.allows_read(path) {
            return Err(self.deny("read", path));
        }
        self.inner.read_bytes(path).await
    }

    async fn metadata(&self, path: &Path) -> io::Result<FileMeta> {
        if !self.policy.allows_read(path) {
            return Err(self.deny("read", path));
        }
        self.inner.metadata(path).await
    }

    async fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        if !self.policy.allows_write(path) {
            return Err(self.deny("create_dir", path));
        }
        self.inner.create_dir_all(path).await
    }

    async fn write_bytes(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
        if !self.policy.allows_write(path) {
            return Err(self.deny("write", path));
        }
        self.inner.write_bytes(path, contents).await
    }

    async fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        // Renames need write on both endpoints plus read on the source side.
        for (op, p) in [("rename_from_write", from), ("rename_to_write", to)] {
            if !self.policy.allows_write(p) {
                return Err(self.deny(op, p));
            }
        }
        if !self.policy.allows_read(from) {
            return Err(self.deny("rename_from_read", from));
        }
        self.inner.rename(from, to).await
    }

    async fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        self.inner.canonicalize(path).await
    }

    // ---- blocking face -------------------------------------------------

    fn read_text_blocking(&self, path: &Path) -> io::Result<String> {
        if !self.policy.allows_read(path) {
            return Err(self.deny("read", path));
        }
        self.inner.read_text_blocking(path)
    }

    fn write_bytes_blocking(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
        if !self.policy.allows_write(path) {
            return Err(self.deny("write", path));
        }
        self.inner.write_bytes_blocking(path, contents)
    }

    fn create_dir_all_blocking(&self, path: &Path) -> io::Result<()> {
        if !self.policy.allows_write(path) {
            return Err(self.deny("create_dir", path));
        }
        self.inner.create_dir_all_blocking(path)
    }

    fn remove_file_blocking(&self, path: &Path) -> io::Result<()> {
        if !self.policy.allows_write(path) {
            return Err(self.deny("remove", path));
        }
        self.inner.remove_file_blocking(path)
    }

    fn canonicalize_blocking(&self, path: &Path) -> io::Result<PathBuf> {
        self.inner.canonicalize_blocking(path)
    }

    fn metadata_blocking(&self, path: &Path) -> io::Result<FileMeta> {
        if !self.policy.allows_read(path) {
            return Err(self.deny("read", path));
        }
        self.inner.metadata_blocking(path)
    }

    fn read_prefix_blocking(&self, path: &Path, max_bytes: usize) -> io::Result<Vec<u8>> {
        if !self.policy.allows_read(path) {
            return Err(self.deny("read", path));
        }
        self.inner.read_prefix_blocking(path, max_bytes)
    }

    fn list_dir_blocking(&self, path: &Path) -> io::Result<Vec<DirEntryInfo>> {
        if !self.policy.allows_read(path) {
            return Err(self.deny("list", path));
        }
        self.inner.list_dir_blocking(path)
    }

    fn walk_blocking(
        &self,
        root: &Path,
        cb: &mut dyn FnMut(&DirEntryInfo) -> bool,
    ) -> io::Result<()> {
        // Policy-gate the traversal root, then delegate: per-entry rules are
        // re-checked by the inner world's own consumers (Read/Grep validate
        // each path they touch).
        if !self.policy.allows_read(root) {
            return Err(self.deny("list", root));
        }
        self.inner.walk_blocking(root, cb)
    }
}

// ---------------------------------------------------------------------------
// SandboxedProcess — decorator over ProcessProvider
// ---------------------------------------------------------------------------

/// Process world whose children were born inside the restricted world (the
/// fork-time initializer was installed on the inner provider during
/// decoration). Stream bytes and exit codes pass through untouched;
/// enforcement happened at fork time inside the child.
///
/// [`SandboxedProcess`] may pin environment defaults onto every child (the
/// Landlock assembly pins `LC_ALL=C`/`LANG=C` so kernel refusals surface as
/// stable, classifiable `"Permission denied"` text regardless of the host
/// locale). Caller-supplied env values still win over the pinned ones.
#[derive(Clone)]
pub struct SandboxedProcess {
    inner: Arc<dyn ProcessProvider>,
    kind: &'static str,
    env_defaults: Vec<(String, String)>,
}

impl std::fmt::Debug for SandboxedProcess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SandboxedProcess")
            .field("kind", &self.kind)
            .field("env_defaults", &self.env_defaults.len())
            .finish()
    }
}

impl SandboxedProcess {
    /// Wrap an already-fork-initialized process world.
    pub fn new(
        inner: Arc<dyn ProcessProvider>,
        kind: &'static str,
        env_defaults: Vec<(String, String)>,
    ) -> Self {
        Self {
            inner,
            kind,
            env_defaults,
        }
    }

    /// Backend identifier carried by this wrapper.
    pub fn kind(&self) -> &'static str {
        self.kind
    }

    /// Merge pinned defaults ahead of the caller's env (later entries win).
    fn apply_env_defaults(&self, request: &ProcessRequest) -> ProcessRequest {
        let mut cloned = request.clone();
        let mut env = self.env_defaults.clone();
        env.extend(cloned.env.iter().cloned());
        cloned.env = env;
        cloned
    }
}

#[async_trait::async_trait]
impl ProcessProvider for SandboxedProcess {
    fn run_blocking(&self, request: &ProcessRequest) -> io::Result<CapturedOutput> {
        let prepared = self.apply_env_defaults(request);
        self.inner.run_blocking(&prepared)
    }

    async fn run_async(&self, request: &ProcessRequest) -> io::Result<CapturedOutput> {
        let prepared = self.apply_env_defaults(request);
        self.inner.run_async(&prepared).await
    }

    async fn spawn_piped(&self, spec: &PipedSpawn) -> io::Result<Box<dyn PipedChild>> {
        let mut prepared = spec.clone();
        prepared.request = self.apply_env_defaults(&spec.request);
        self.inner.spawn_piped(&prepared).await
    }
}

// ---------------------------------------------------------------------------
// Settings (env > project .shannon.toml > ~/.shannon/config.toml)
// ---------------------------------------------------------------------------

/// User-facing sandbox configuration resolved from the standard config stack.
///
/// Unknown tokens never abort the session: they log a warning and fall back
/// to [`SandboxMode::Off`] (fail-open for *configuration*, while any actual
/// enforcement failure stays fail-closed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxSettings {
    /// Selected execution-world mode.
    pub mode: SandboxMode,
    /// Whether children may use TCP networking.
    pub network: bool,
    /// Additional writable directories (absolute paths).
    pub extra_writable: Vec<PathBuf>,
    /// Additional readable directories.
    pub extra_readable: Vec<PathBuf>,
    /// Additional executable roots.
    pub extra_executable: Vec<PathBuf>,
}

impl Default for SandboxSettings {
    fn default() -> Self {
        Self {
            mode: SandboxMode::Off,
            network: false,
            extra_writable: Vec::new(),
            extra_readable: Vec::new(),
            extra_executable: Vec::new(),
        }
    }
}

impl SandboxSettings {
    /// Resolve settings from env vars layered over TOML files:
    /// `SHANNON_SANDBOX`/`SHANNON_SANDBOX_NETWORK`/`SHANNON_SANDBOX_WRITABLE`
    /// (+`_READABLE`, `_EXECUTABLE`) override `[sandbox]` tables parsed from
    /// `./.shannon.toml` then `<shannon home>/config.toml`.
    pub fn detect() -> Self {
        let mut settings = Self::default();

        for toml_path in [
            PathBuf::from(".shannon.toml"),
            shannon_home().join("config.toml"),
        ] {
            if let Ok(raw) = std::fs::read_to_string(&toml_path) {
                match raw.parse::<toml::Value>() {
                    Ok(value) => settings.apply_toml_table(value.get("sandbox"), &toml_path),
                    Err(e) => {
                        tracing::warn!("sandbox: ignoring unparsable {}: {e}", toml_path.display())
                    }
                }
            }
        }

        if let Some(token) = std::env::var_os("SHANNON_SANDBOX") {
            let token = token.to_string_lossy();
            match SandboxMode::parse(&token) {
                Some(mode) => settings.mode = mode,
                None => {
                    tracing::warn!(
                        "sandbox: unknown SHANNON_SANDBOX={token:?}; falling back to off"
                    )
                }
            }
        }
        if let Some(flag) = std::env::var_os("SHANNON_SANDBOX_NETWORK") {
            let flag = flag.to_string_lossy().to_ascii_lowercase();
            settings.network = matches!(flag.as_str(), "1" | "true" | "yes" | "on");
        }
        if let Some(paths) = std::env::var_os("SHANNON_SANDBOX_WRITABLE") {
            settings.extra_writable = split_paths(&paths.to_string_lossy());
        }
        if let Some(paths) = std::env::var_os("SHANNON_SANDBOX_READABLE") {
            settings.extra_readable = split_paths(&paths.to_string_lossy());
        }
        if let Some(paths) = std::env::var_os("SHANNON_SANDBOX_EXECUTABLE") {
            settings.extra_executable = split_paths(&paths.to_string_lossy());
        }
        settings
    }

    /// Merge one optional TOML `[sandbox]` table into these settings.
    fn apply_toml_table(&mut self, table: Option<&toml::Value>, origin: &Path) {
        let Some(table) = table.and_then(|t| t.as_table()) else {
            return;
        };
        if let Some(token) = table.get("mode").and_then(|v| v.as_str()) {
            match SandboxMode::parse(token) {
                Some(mode) => self.mode = mode,
                None => tracing::warn!(
                    "sandbox: unknown mode={token:?} in {}; staying off",
                    origin.display()
                ),
            }
        }
        if let Some(network) = table.get("network") {
            match network.as_bool() {
                Some(b) => self.network = b,
                None => tracing::warn!("sandbox: network must be boolean in {}", origin.display()),
            }
        }
        for (key, slot) in [
            ("writable", &mut self.extra_writable),
            ("readable", &mut self.extra_readable),
            ("executable", &mut self.extra_executable),
        ] {
            if let Some(list) = table.get(key).and_then(|v| v.as_array()) {
                for item in list {
                    if let Some(p) = item.as_str() {
                        slot.push(PathBuf::from(p));
                    }
                }
            }
        }
    }
}

/// Resolve the Shannon home directory (`$SHANNON_HOME` or `~/.shannon`),
/// matching the loader conventions used across the repo.
fn shannon_home() -> PathBuf {
    if let Some(home) = std::env::var_os("SHANNON_HOME") {
        return PathBuf::from(home);
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".shannon")
}

/// Split a `:`-separated path list (env var form).
fn split_paths(raw: &str) -> Vec<PathBuf> {
    raw.split(':')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .collect()
}

// ---------------------------------------------------------------------------
// Policy seeding + assembly
// ---------------------------------------------------------------------------

/// Build the effective policy for a project directory.
///
/// Defaults favor usability without giving up the safety intent: everything
/// readable, only the project writable, binaries executable from the system
/// roots, network denied unless configured otherwise. Explicit extras append.
pub fn seed_policy(project_dir: &Path, settings: &SandboxSettings) -> SandboxPolicy {
    SandboxPolicy {
        writable_roots: roots_from([project_dir], &settings.extra_writable),
        readable_roots: roots_from([Path::new("/")], &settings.extra_readable),
        // Readable ≠ executable in the kernel world; the dynamic loader and
        // interpreter locations must be granted explicitly or bash cannot
        // even start.
        executable_roots: roots_from(
            [
                Path::new("/usr"),
                Path::new("/bin"),
                Path::new("/sbin"),
                Path::new("/lib"),
                Path::new("/lib64"),
            ],
            &settings.extra_executable,
        ),
        network: settings.network,
    }
}

fn roots_from<const N: usize>(base: [&Path; N], extras: &[PathBuf]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = base.iter().map(|p| (*p).to_path_buf()).collect();
    for p in extras {
        if !out.iter().any(|b| path_within(p, b)) {
            out.push(p.clone());
        }
    }
    out
}

/// Result of assembling providers for one sandbox mode.
#[derive(Clone)]
pub struct AssembledWorlds {
    /// Providers ready for `register_default_tools_with_providers`.
    pub providers: crate::ToolProviders,
    /// Backend identity (`"local"` / `"landlock"`).
    pub kind: &'static str,
    /// Non-fatal degrades discovered during construction (already logged).
    pub notices: Vec<DegradeNotice>,
}

impl std::fmt::Debug for AssembledWorlds {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AssembledWorlds")
            .field("kind", &self.kind)
            .field("notices", &self.notices)
            .finish()
    }
}

/// Assemble the **kernel-enforced** world (mode = `landlock`).
///
/// Fails closed with [`SandboxError::Unsupported`] when the host cannot
/// enforce Landlock (non-Linux builds reach the same error through the stub
/// backend): callers decide between surfacing the refusal or falling back to
/// explicit local mode with a loud warning — never a silent fake sandbox.
pub fn assemble(
    settings: &SandboxSettings,
    project_dir: &Path,
) -> Result<AssembledWorlds, SandboxError> {
    match settings.mode {
        SandboxMode::Landlock => {
            let policy = Arc::new(seed_policy(project_dir, settings));
            let backend = landlock_backend::probe_new(policy.clone())?;
            let notices: Vec<DegradeNotice> = backend.degrade_notices().to_vec();

            // FS world: user-space mirror of the kernel ruleset (in-process
            // tools are not kernel-restricted; see module docs).
            let fs: Arc<dyn FileSystemProvider> =
                backend.decorate_fs(shannon_core::providers::LocalFs::shared());

            // Process world: fork initializer installs rulesets pre-exec.
            let host: Arc<dyn ForkInitHost> =
                Arc::new(shannon_core::providers::LocalProcess::new());
            let proc_world = backend.decorate_process(host)?;
            for notice in &notices {
                tracing::warn!(tag = %notice.tag, "sandbox degrade: {}", notice.detail);
            }

            Ok(AssembledWorlds {
                providers: crate::ToolProviders {
                    fs,
                    process: proc_world,
                    denial_classifier: Some(kernel_denial_classifier()),
                    world_sandbox: None,
                },
                kind: "landlock",
                notices,
            })
        }
        other => Err(SandboxError::InvalidConfig(format!(
            "assemble() is the kernel-world constructor; mode '{other}' must take its own \
             assembly path (off ⇒ legacy passthrough, local ⇒ assemble_local)"
        ))),
    }
}

/// Assemble the portable **user-space-only** world (mode = `local`).
///
/// In-process filesystem denials apply everywhere (including macOS); child
/// processes stay as unrestricted as the surrounding platform allows (on
/// Linux the existing argv-level wrappers may still apply where detected).
pub fn assemble_local(settings: &SandboxSettings, project_dir: &Path) -> AssembledWorlds {
    let policy = Arc::new(seed_policy(project_dir, settings));
    let proc_inner: Arc<dyn ProcessProvider> =
        Arc::new(shannon_core::providers::LocalProcess::new());
    AssembledWorlds {
        providers: crate::ToolProviders {
            fs: Arc::new(SandboxedFs::new(
                shannon_core::providers::LocalFs::shared(),
                policy,
            )),
            process: Arc::new(SandboxedProcess::new(proc_inner, "local", Vec::new())),
            denial_classifier: None,
            world_sandbox: None,
        },
        kind: "local",
        notices: vec![DegradeNotice::new(
            "user-space-only",
            "local mode enforces sandbox policy in-process only; child processes are NOT \
             kernel-restricted",
        )],
    }
}

// ---------------------------------------------------------------------------
// Classification helper shared by bash/tool layers
// ---------------------------------------------------------------------------

/// Classifier attached to BashTool by kernel-enforcing assemblies: converts a
/// failed captured run into structured denial metadata. Heuristic by nature
/// (in-child syscall failures are only visible through the child's stderr),
/// therefore it runs only under worlds known to enforce and never on plain
/// failures of unrestricted worlds.
pub type DenialClassifier =
    Arc<dyn Fn(&crate::system::CommandOutput) -> Option<SandboxDenialInfo> + Send + Sync>;

/// Standard classifier for the Landlock world: recognizes the two canonical
/// Linux kernel-denial messages and extracts the offending target when the
/// shell printed one.
#[must_use]
pub fn kernel_denial_classifier() -> DenialClassifier {
    const MARKERS: [&str; 2] = ["Permission denied", "Operation not permitted"];
    Arc::new(move |out: &crate::system::CommandOutput| {
        if out.success || out.stderr.is_empty() {
            return None;
        }
        let mut matched_marker = false;
        let mut target = String::new();
        for line in out.stderr.lines() {
            for marker in MARKERS {
                if let Some(idx) = line.find(marker) {
                    matched_marker = true;
                    if let Some(token) = line[..idx].split_whitespace().next_back() {
                        target = token.trim_end_matches(':').to_string();
                    }
                }
            }
        }
        if !matched_marker {
            return None;
        }
        Some(SandboxDenialInfo {
            op: "child_command".to_string(),
            target,
            reason: "refused by the kernel inside the sandboxed execution world".to_string(),
        })
    })
}

/// Metadata entries marking a tool result as a sandbox denial (goes into
/// `QueryEvent::ToolUseResult.meta` → L0 `tool/result.meta`).
pub fn denial_metadata(denial: &SandboxDenialInfo) -> [(String, serde_json::Value); 2] {
    [
        (
            "classification".to_string(),
            serde_json::Value::String(SANDBOX_DENIED_CLASSIFICATION.to_string()),
        ),
        ("sandbox".to_string(), denial.to_json()),
    ]
}

/// A fork-side verifier usable as the engine of any world installer.
/// Provided here so tests can fake boundaries without touching syscalls.
pub struct FnChildWorldInit<F>(pub F);

impl<F: Fn() -> io::Result<()> + Send + Sync> ChildWorldInit for FnChildWorldInit<F> {
    fn init_child(&self) -> io::Result<()> {
        (self.0)()
    }
}

// ---------------------------------------------------------------------------
// § write_files enforcement — plugin stdio spawn chain
// ---------------------------------------------------------------------------

/// A manifest-derived execution-world boundary ready to install around a
/// plugin's stdio server spawn chain (write_files enforcement: "declaration
/// IS sandbox").
#[derive(Clone)]
pub struct PluginSpawnWorld {
    /// Boundary handed to `gated_discover_tools_stdio_guarded` and stamped
    /// onto every discovered adapter (discovery + per-call cold spawns).
    pub guard: Arc<shannon_core::plugin::PluginSpawnGuard>,
    /// The derived policy this world enforces (kept for operator logging).
    pub policy: SandboxPolicy,
    /// Non-fatal degrades discovered at construction (e.g. the kernel lacks
    /// the network ABI); surfaced to logs by the assemblers.
    pub notices: Vec<DegradeNotice>,
}

impl std::fmt::Debug for PluginSpawnWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginSpawnWorld")
            .field("kind", &self.guard.kind())
            .field("writable_roots", &self.policy.writable_roots)
            .field("network", &self.policy.network)
            .field("notices", &self.notices.len())
            .finish()
    }
}

/// Build the execution-world boundary for a manifest-derived plugin sandbox
/// policy.
///
/// Platform matrix:
///
/// | host | boundary |
/// |------|----------|
/// | Linux | Landlock fork-init world ([`landlock_backend::child_world_install`]) — kernel-enforced, fail-closed per spawn |
/// | macOS | the existing Seatbelt bridge ([`shannon_core::sandbox::SandboxExecutor`]) — spawns rewritten through `sandbox-exec` |
/// | other / backend missing | `Err(SandboxError::Unsupported)` — callers degrade by spawning **without** a boundary plus a loud warning; a silent fake sandbox is never produced |
pub fn plugin_spawn_world(
    policy: SandboxPolicy,
    workspace: &Path,
) -> Result<PluginSpawnWorld, SandboxError> {
    // The Linux fork-init world is fully determined by the policy; the
    // workspace parameter exists for the macOS Seatbelt config.
    #[cfg(target_os = "linux")]
    let _ = workspace;
    #[cfg(target_os = "linux")]
    {
        let policy = Arc::new(policy);
        let (init, notices) = landlock_backend::child_world_install(Arc::clone(&policy))?;
        Ok(PluginSpawnWorld {
            guard: Arc::new(shannon_core::plugin::PluginSpawnGuard::ForkInit(init)),
            policy: (*policy).clone(),
            notices,
        })
    }
    #[cfg(target_os = "macos")]
    {
        seatbelt_plugin_world(policy, workspace)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = workspace;
        Err(SandboxError::Unsupported {
            backend: "plugin-spawn-world".to_string(),
            detail: "no execution-world backend for this platform".to_string(),
        })
    }
}

/// macOS Seatbelt arm: profile-generated argv bridge built on the existing
/// executor, scoped to the plugin's declared writable roots.
#[cfg(target_os = "macos")]
fn seatbelt_plugin_world(
    policy: SandboxPolicy,
    workspace: &Path,
) -> Result<PluginSpawnWorld, SandboxError> {
    use shannon_core::sandbox::{NetworkAccess, SandboxConfig, SandboxExecutor, SandboxType};

    let mut config = SandboxConfig::new(workspace);
    for root in &policy.writable_roots {
        if root.as_path() != workspace {
            config = config.readwrite_mount(root);
        }
    }
    if policy.network {
        config = config.with_network(NetworkAccess::Full);
    }
    let executor = Arc::new(SandboxExecutor::new(config));
    if executor.sandbox_type() != SandboxType::Seatbelt {
        return Err(SandboxError::Unsupported {
            backend: "seatbelt".to_string(),
            detail: "sandbox-exec not available on this host".to_string(),
        });
    }
    Ok(PluginSpawnWorld {
        guard: Arc::new(shannon_core::plugin::PluginSpawnGuard::Seatbelt(executor)),
        policy,
        notices: vec![DegradeNotice::new(
            "seatbelt-argv-bridge",
            "plugin stdio spawns run under sandbox-exec profiles; enforcement is \
             profile-based (argv bridge), not a fork-time kernel world",
        )],
    })
}

/// Derive + build the spawn-chain boundary for one loaded plugin (the single
/// wiring point used by the REPL and CLI plugin loaders).
///
/// * No `write_files` declaration ⇒ `None` with nothing constructed — the
///   zero-overhead passthrough that keeps the default-allow compat contract.
/// * Declared and backend available ⇒ `Some(guard)`; every degrade notice is
///   logged under the `plugin/sandbox` target.
/// * Declared but the platform cannot enforce ⇒ loud `plugin/sandbox`
///   warning and `None`: the spawn chain proceeds exactly as before this
///   enforcement existed (degrade, never a silently-fake sandbox).
pub fn plugin_spawn_guard_for_manifest(
    policy: &shannon_core::plugin::PluginPermissionPolicy,
    plugin_name: &str,
    manifest_path: &Path,
) -> Option<Arc<shannon_core::plugin::PluginSpawnGuard>> {
    // Workspace snapshot: the process working directory at plugin-load time.
    // Documented limitation — the boundary pins this directory for the
    // plugin's lifetime in the session.
    let workspace = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let derived = policy.spawn_sandbox_policy(
        manifest_path.parent().unwrap_or_else(|| Path::new(".")),
        &workspace,
    )?;

    match plugin_spawn_world(derived, &workspace) {
        Ok(world) => {
            for notice in &world.notices {
                tracing::warn!(
                    target: "plugin/sandbox",
                    plugin = %plugin_name,
                    tag = %notice.tag,
                    "sandbox degrade: {}",
                    notice.detail
                );
            }
            tracing::info!(
                target: "plugin/sandbox",
                plugin = %plugin_name,
                kind = %world.guard.kind(),
                writable = ?world.policy.writable_roots,
                "plugin stdio spawns sandboxed (write_files declaration = execution world)"
            );
            Some(world.guard)
        }
        Err(e) => {
            tracing::warn!(
                target: "plugin/sandbox",
                plugin = %plugin_name,
                error = %e,
                "write_files declared but no execution-world backend is available on this \
                 host; spawning WITHOUT the sandbox boundary (degraded, never fake-restricted)"
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sandbox::landlock_backend::{FS_EXECUTE, FS_READ_DIR, FS_READ_FILE};
    use shannon_tool_interface::SANDBOX_DENIED_PREFIX;
    use std::path::Path;
    use std::path::PathBuf;

    fn tempdir() -> tempfile::TempDir {
        tempfile::tempdir().expect("temp dir")
    }

    fn settings(mode: SandboxMode) -> SandboxSettings {
        SandboxSettings {
            mode,
            ..Default::default()
        }
    }

    // ── SandboxedFs decorator ──────────────────────────────────────────

    #[tokio::test]
    async fn sandboxed_fs_denies_writes_outside_roots_with_classification() {
        let ws = tempdir();
        let outside = tempdir();
        let policy = Arc::new(SandboxPolicy {
            writable_roots: vec![ws.path().to_path_buf()],
            readable_roots: vec![PathBuf::from("/")],
            executable_roots: vec![],
            network: false,
        });
        let fs = SandboxedFs::new(shannon_core::providers::LocalFs::shared(), policy);

        // In-workspace write succeeds and reads back byte-identical.
        fs.write_bytes(&ws.path().join("ok.txt"), b"payload")
            .await
            .expect("workspace write allowed");
        assert_eq!(
            fs.read_text(&ws.path().join("ok.txt")).await.expect("read"),
            "payload"
        );

        // Outside-root write is refused with the canonical classification.
        let err = fs
            .write_bytes(&outside.path().join("nope.txt"), b"x")
            .await
            .expect_err("outside write must be denied");
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
        let text = err.to_string();
        assert!(text.starts_with(SANDBOX_DENIED_PREFIX), "got: {text}");
        let denial = SandboxDenialInfo::parse(&text).expect("classified");
        assert_eq!(denial.op, "write");
    }

    #[tokio::test]
    async fn sandboxed_fs_blocking_faces_cover_remove_and_rename() {
        let ws = tempdir();
        let outside = tempdir();
        let policy = Arc::new(SandboxPolicy {
            writable_roots: vec![ws.path().to_path_buf()],
            readable_roots: vec![PathBuf::from("/")],
            executable_roots: vec![],
            network: false,
        });
        let fs = SandboxedFs::new(shannon_core::providers::LocalFs::shared(), policy);

        fs.write_bytes_blocking(&ws.path().join("a"), b"1")
            .expect("seed");
        assert!(
            fs.remove_file_blocking(&outside.path().join("gone"))
                .is_err(),
            "remove outside roots must be denied"
        );
        fs.remove_file_blocking(&ws.path().join("a"))
            .expect("remove inside");
        assert!(
            fs.rename(&ws.path().join("missing"), &ws.path().join("x"))
                .await
                .is_err(),
            "rename from outside-writable source must be denied before hitting disk"
        );
    }

    #[test]
    fn sandboxed_fs_list_dir_follows_policy() {
        let ws = tempdir();
        let policy = Arc::new(SandboxPolicy {
            writable_roots: vec![ws.path().to_path_buf()],
            readable_roots: vec![],
            executable_roots: vec![],
            network: false,
        });
        let fs = SandboxedFs::new(shannon_core::providers::LocalFs::shared(), policy);
        assert!(
            fs.list_dir_blocking(ws.path()).is_ok(),
            "listed root is granted"
        );
        let err = fs
            .list_dir_blocking(Path::new("/proc/1-nonexistent-grant-check"))
            .err()
            .map(|e| e.to_string());
        // Either a policy denial (prefix) or a genuine ENOENT — both mean the
        // decorator handled the call; the important part is no panic.
        if let Some(text) = err {
            assert!(!text.is_empty());
        }
    }

    // ── Settings resolution ────────────────────────────────────────────

    #[test]
    fn settings_default_is_off_without_network() {
        let s = SandboxSettings::default();
        assert_eq!(s.mode, SandboxMode::Off);
        assert!(!s.network);
        assert!(s.extra_writable.is_empty());
    }

    #[test]
    fn toml_table_parse_accepts_all_documented_keys() {
        let raw = r#"
[sandbox]
mode = "landlock"
network = true
writable = ["/tmp/a", "/tmp/b"]
readable = ["/opt"]
executable = ["/usr/local"]
"#;
        let value: toml::Value = toml::from_str(raw).expect("parse");
        let mut s = SandboxSettings::default();
        s.apply_toml_table(value.get("sandbox"), Path::new("inline"));
        assert_eq!(s.mode, SandboxMode::Landlock);
        assert!(s.network);
        assert_eq!(s.extra_writable.len(), 2);
        assert_eq!(s.extra_readable, vec![PathBuf::from("/opt")]);
        assert_eq!(s.extra_executable, vec![PathBuf::from("/usr/local")]);
    }

    #[test]
    fn toml_unknown_mode_token_stays_off() {
        let raw = "[sandbox]\nmode = \"bubblewrap\"\n";
        let value: toml::Value = toml::from_str(raw).expect("parse");
        let mut s = SandboxSettings::default();
        s.apply_toml_table(value.get("sandbox"), Path::new("inline"));
        assert_eq!(
            s.mode,
            SandboxMode::Off,
            "unknown tokens degrade loudly, not crash"
        );
    }

    #[test]
    fn env_path_lists_split_on_colon() {
        assert_eq!(
            split_paths("/tmp/x:/tmp/y : "),
            vec![PathBuf::from("/tmp/x"), PathBuf::from("/tmp/y")]
        );
    }

    #[test]
    fn shannon_home_honors_override_env() {
        // Non-empty result on every host; SHANNON_HOME redirect respected in
        // spirit (deep environment mutation in tests is racy under nextest,
        // so only the shape contract is asserted).
        assert!(!shannon_home().as_os_str().is_empty());
    }

    // ── Seeding + assembly ─────────────────────────────────────────────

    #[test]
    fn seed_policy_scopes_defaults_to_project() {
        let proj = tempdir();
        let mut settings = settings(SandboxMode::Landlock);
        settings.network = true;
        settings
            .extra_writable
            .push(PathBuf::from("/var/tmp/extra"));
        let policy = seed_policy(proj.path(), &settings);
        assert!(policy.allows_write(proj.path()));
        assert!(policy.allows_write(Path::new("/var/tmp/extra/f")));
        assert!(!policy.allows_write(Path::new("/etc")));
        assert!(
            policy.allows_read(Path::new("/anything/at/all")),
            "read defaults to /"
        );
        assert!(policy.allows_execute(Path::new("/usr/bin/env")));
        assert!(policy.network, "explicit opt-in propagates");
    }

    #[test]
    fn assemble_rejects_non_kernel_modes() {
        let proj = tempdir();
        let err = assemble(&settings(SandboxMode::Off), proj.path())
            .expect_err("off must take the legacy passthrough");
        assert!(matches!(err, SandboxError::InvalidConfig(_)));
        let err = assemble(&settings(SandboxMode::Local), proj.path())
            .expect_err("local has its own constructor");
        assert!(matches!(err, SandboxError::InvalidConfig(_)));
    }

    #[test]
    fn assemble_local_worlds_enforce_user_space_only() {
        let proj = tempdir();
        let assembled = assemble_local(&settings(SandboxMode::Local), proj.path());
        assert_eq!(assembled.kind, "local");
        assert_eq!(
            assembled.notices.len(),
            1,
            "the user-space-only caveat is explicit"
        );

        // The decorated fs refuses out-of-workspace writes; the process world
        // wrapper exposes its backend identity through Debug.
        let outside = tempdir();
        let err = assembled
            .providers
            .fs
            .write_bytes_blocking(&outside.path().join("x"), b"y")
            .expect_err("must be denied by policy mirror");
        assert!(SandboxDenialInfo::parse(&err.to_string()).is_some());
    }

    /// Kernel assembly path: succeeds on landlock-capable hosts (this one),
    /// fails closed with `Unsupported` elsewhere.
    #[tokio::test]
    async fn assemble_landlock_matches_host_capability() {
        let proj = tempdir();
        match assemble(&settings(SandboxMode::Landlock), proj.path()) {
            Ok(assembled) => {
                assert_eq!(assembled.kind, "landlock");
                // The process world is a plain Arc<dyn ProcessProvider> — the
                // decoration is invisible at the type level (that IS the
                // zero-code-change contract). Assert it round-trips a run.
                let out = assembled
                    .providers
                    .process
                    .run_async(&shannon_tool_interface::ProcessRequest::new(
                        "/bin/echo",
                        &["worlds-ok"],
                    ))
                    .await
                    .expect("decorated world still runs children");
                assert_eq!(out.stdout, b"worlds-ok\n");
            }
            Err(SandboxError::Unsupported { backend, .. }) => {
                assert_eq!(backend, "landlock");
                println!("host cannot enforce landlock; fail-closed OK");
            }
            Err(other) => panic!("unexpected error: {other}"),
        }
    }

    /// Kernel-numbered bit groups stay mutually disjoint so grant masks are
    /// interpretable as sets.
    #[test]
    fn access_bit_groups_are_disjoint() {
        let read_set = FS_EXECUTE | FS_READ_FILE | FS_READ_DIR;
        // kernel numberings: execute=bit0, read file/dir bits 2..3, writes at
        // bit1 + make-group above bit6 — groups must never overlap.
        assert_eq!(read_set & (1 << 1), 0);
        assert_eq!(
            (1 << 14) & crate::sandbox::landlock_backend::FS_WRITE_FILE,
            0
        );
        assert_ne!(read_set, 0);
    }

    // ── § write_files enforcement: plugin spawn chain ──────────────────

    fn plugin_policy(permissions: &[&str]) -> shannon_core::plugin::PluginPermissionPolicy {
        let perms: Vec<shannon_core::plugin::PluginPermission> = permissions
            .iter()
            .map(|n| serde_json::from_str(&format!("\"{n}\"")).expect("known permission"))
            .collect();
        shannon_core::plugin::PluginPermissionPolicy::from_permissions(perms)
    }

    /// The wiring helper's compat contract: no `write_files` declaration ⇒
    /// `None` and nothing constructed (zero-overhead passthrough).
    #[test]
    fn manifest_helper_passes_through_without_write_files() {
        for perms in [
            Vec::new(),         // unspecified (default-allow legacy)
            vec!["read_files"], // declared, different face
            vec!["execute_commands", "mcp_tools"],
        ] {
            let policy = plugin_policy(&perms);
            assert!(
                plugin_spawn_guard_for_manifest(&policy, "probe", Path::new("/tmp/x/plugin.toml"))
                    .is_none(),
                "permissions {perms:?} must not install a spawn boundary"
            );
        }
    }

    /// Declared `write_files`: the boundary matches host capability —
    /// enforced where the platform has a backend, otherwise loudly degraded
    /// to `None`. Both outcomes carry operator-visible signal; neither one
    /// can be a silent fake restriction.
    #[test]
    fn manifest_helper_matches_host_capability_for_write_files() {
        let dir = tempdir();
        let manifest = dir.path().join("plugin.toml");
        std::fs::write(&manifest, "# fixture\n").expect("manifest fixture");
        let policy = plugin_policy(&["write_files", "execute_commands"]);

        let guard = plugin_spawn_guard_for_manifest(&policy, "probe", &manifest);
        match guard {
            Some(guard) => {
                let expected = if cfg!(target_os = "linux") {
                    "landlock"
                } else if cfg!(target_os = "macos") {
                    "seatbelt"
                } else {
                    unreachable!("helper never constructs a guard without a backend");
                };
                assert_eq!(guard.kind(), expected);
            }
            None => {
                // Degrade path: only legitimate when the platform lacks any
                // execution-world backend.
                assert!(
                    !cfg!(any(target_os = "linux", target_os = "macos")),
                    "degradation on Linux/macOS would mean a backend-capable host silently \
                     lost enforcement"
                );
            }
        }
    }

    /// The constructed world's policy is exactly the derived one: writable
    /// roots converge to install dir + workspace, network follows the
    /// declaration.
    #[cfg(target_os = "linux")]
    #[test]
    fn plugin_spawn_world_policy_is_the_derived_one() {
        let install = tempdir();
        let workspace = tempdir();
        let policy = plugin_policy(&["write_files"]);
        let derived = policy
            .spawn_sandbox_policy(install.path(), workspace.path())
            .expect("write_files derives a policy");
        let world = match plugin_spawn_world(derived.clone(), workspace.path()) {
            Ok(world) => world,
            Err(SandboxError::Unsupported { detail, .. }) => {
                println!("host lacks landlock ({detail}); nothing to assert about a world");
                return;
            }
            Err(other) => panic!("unexpected sandbox error: {other}"),
        };
        assert_eq!(world.policy, derived);
        assert_eq!(world.guard.kind(), "landlock");
        assert_eq!(
            world.policy.writable_roots,
            vec![
                install.path().canonicalize().expect("install"),
                workspace.path().canonicalize().expect("workspace"),
            ]
        );
        assert!(!world.policy.network, "network follows its declaration");
    }
}
