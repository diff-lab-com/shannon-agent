//! Path sandboxing for file operations
//!
//! Provides security boundaries to prevent unauthorized file access through:
//! - Path traversal detection (e.g., `../../etc/passwd`)
//! - Symlink resolution (checks resolved path, not the literal path)
//! - Denied path patterns (system directories)
//! - Home directory boundary enforcement
//! - Strict mode (allow only explicitly configured roots)
//!
//! # TOCTOU Protection
//!
//! The sandbox uses canonicalization to resolve symlinks before checking paths.
//! This protects against time-of-check/time-of-use (TOCTOU) attacks where
//! an attacker might replace a safe path with a symlink after validation.
//! The canonicalization happens immediately before the access check, making
//! it difficult for an attacker to race the condition.

use std::path::{Path, PathBuf};

/// Mount alias the command sandbox backends use for the project dir
/// (bwrap/Docker bind `<project_dir>` here — see shannon-core sandbox.rs).
const SANDBOX_BIND_ALIAS: &str = "/workspace";

/// The command sandbox's scratch root. bwrap/Docker give the sandboxed shell
/// a writable `/tmp`, so the file tools must accept the same literal path —
/// it is already sandbox-visible spelling, unlike the project dir which the
/// backends relocate to [`SANDBOX_BIND_ALIAS`].
const SANDBOX_TMP_ROOT: &str = "/tmp";

/// Configuration for the path sandbox
#[derive(Debug, Clone)]
pub struct SandboxConfig {
    /// Directories that are explicitly allowed as root paths.
    /// Subdirectories of these roots are also allowed.
    pub allowed_roots: Vec<PathBuf>,

    /// Path prefixes that are always denied, even if inside an allowed root.
    pub denied_patterns: Vec<String>,

    /// When true, deny all paths that are not under an allowed root.
    /// When false, only explicitly denied paths are blocked.
    pub strict_mode: bool,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            allowed_roots: vec![std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))],
            denied_patterns: Self::default_denied_patterns(),
            strict_mode: true,
        }
    }
}

impl SandboxConfig {
    /// Returns the default list of denied system path prefixes.
    pub fn default_denied_patterns() -> Vec<String> {
        vec![
            "/etc/".to_string(),
            "/boot/".to_string(),
            "/usr/bin/".to_string(),
            "/usr/sbin/".to_string(),
            "/bin/".to_string(),
            "/sbin/".to_string(),
            "/dev/".to_string(),
            "/proc/".to_string(),
            "/sys/".to_string(),
            "/run/".to_string(),
            "/var/log/".to_string(),
            "/var/run/".to_string(),
        ]
    }

    /// The writable-root set the command sandbox grants (B3 alignment).
    ///
    /// shannon-core's `SandboxProfile` gives sandboxed commands the project
    /// dir plus `/tmp`; the file tools must accept the same set or the
    /// model hits "Write refuses /tmp while Bash writes it" splits
    /// (docs/eval-findings-2026-09-glm.md B3). System paths stay excluded —
    /// denied patterns and strict-mode checks are unchanged.
    pub fn command_aligned_roots(project_dir: &Path) -> Vec<PathBuf> {
        let mut roots = vec![project_dir.to_path_buf()];
        let temp = std::env::temp_dir();
        if !roots.contains(&temp) {
            roots.push(temp);
        }
        roots
    }
}

/// Shared roots/home overrides for swappable execution worlds (§remote).
///
/// Clones of a [`PathSandbox`] hold `Arc` handles to the same override cell,
/// so `crate::shannon_remote`-style assemblies can retarget every registered
/// tool's sandbox when the execution world changes (`/remote use`) without
/// rebuilding the registry. An unset (default) override is a passthrough:
/// the sandbox keeps using its configured roots and local home.
#[derive(Debug, Default)]
pub struct WorldSandboxHandle {
    inner: std::sync::RwLock<WorldRoots>,
}

/// Roots + home boundary currently in effect for the active world.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorldRoots {
    /// Allowed roots for the active world (remote workspace dir).
    pub allowed_roots: Vec<PathBuf>,
    /// Home directory of the active world (remote `$HOME`).
    pub home_dir: Option<PathBuf>,
}

impl WorldSandboxHandle {
    /// New passthrough handle (no override installed).
    pub fn new() -> Self {
        Self::default()
    }

    /// Install (or clear with `WorldRoots::default()`) an override. Takes
    /// effect immediately for every clone sharing this handle.
    pub fn set(&self, roots: WorldRoots) {
        if let Ok(mut guard) = self.inner.write() {
            *guard = roots;
        }
    }

    /// The installed override, or `None` when passthrough (empty).
    pub fn current(&self) -> Option<WorldRoots> {
        self.inner
            .read()
            .ok()
            .map(|r| r.clone())
            .filter(|r| !r.allowed_roots.is_empty() || r.home_dir.is_some())
    }
}

/// A sandbox that validates file paths against security rules.
///
/// Every file operation should pass through `PathSandbox::validate` before
/// accessing the filesystem. The sandbox resolves symlinks and canonicalizes
/// paths, then checks the resolved path against allowed roots and denied
/// patterns.
#[derive(Clone)]
pub struct PathSandbox {
    config: SandboxConfig,
    /// Cached home directory of the current user for boundary checking.
    home_dir: Option<PathBuf>,
    /// Filesystem world used for TOCTOU canonicalization (§4.11). Defaults to
    /// the local world; assemblies that replace the execution environment
    /// inject the matching provider so resolution follows the same world the
    /// tools will act in.
    fs: std::sync::Arc<dyn shannon_tool_interface::FileSystemProvider>,
    /// Shared override for swappable worlds (remote targets). `None` or a
    /// passthrough handle leaves config/home in charge.
    world: Option<std::sync::Arc<WorldSandboxHandle>>,
    /// When true, tool OUTPUT paths are re-rendered into the command
    /// sandbox's view (`SANDBOX_BIND_ALIAS` reverse mapping) — see
    /// [`PathSandbox::alias_display_path`]. Off by default: a sandbox with
    /// the flag unset echoes canonical host paths unchanged.
    bind_alias_output: bool,
}

impl std::fmt::Debug for PathSandbox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PathSandbox")
            .field("config", &self.config)
            .field("home_dir", &self.home_dir)
            .field("world", &self.world.as_ref().map(|_| "<shared>"))
            .field("bind_alias_output", &self.bind_alias_output)
            .finish()
    }
}

/// Errors returned by sandbox validation
#[derive(Debug, thiserror::Error)]
pub enum SandboxError {
    #[error("Path traversal detected: {0}")]
    PathTraversal(String),

    #[error("Access denied - path is in a restricted area: {0}")]
    Denied(String),

    #[error("Path outside allowed roots: {0}")]
    OutsideAllowedRoots(String),

    #[error("Symlink resolves outside allowed roots: {symlink} -> {target}")]
    SymlinkEscape { symlink: String, target: String },

    #[error("Potential TOCTOU attack detected: symlink target changed between check and use")]
    ToctouDetected(String),

    #[error("Failed to resolve path: {0}")]
    ResolutionFailed(String),

    #[error("Path is empty or invalid")]
    InvalidPath,
}

impl PathSandbox {
    /// Create a new sandbox with default configuration.
    ///
    /// Default configuration:
    /// - Allowed root: current working directory
    /// - Strict mode: true (only paths under allowed roots are permitted)
    /// - Denied patterns: system directories (`/etc/`, `/boot/`, `/dev/`, etc.)
    pub fn new() -> Self {
        Self::with_config(SandboxConfig::default())
    }

    /// Create a sandbox with custom configuration.
    pub fn with_config(config: SandboxConfig) -> Self {
        let home_dir = dirs_home_dir();
        Self {
            config,
            home_dir,
            fs: crate::defaults::fs(),
            world: None,
            bind_alias_output: false,
        }
    }

    /// Install a shared world-roots override (remote targets). Every clone
    /// sharing the handle retargets together; see [`WorldSandboxHandle`].
    pub fn with_world_sandbox(mut self, world: std::sync::Arc<WorldSandboxHandle>) -> Self {
        self.world = Some(world);
        self
    }

    /// Roots currently in effect: the world override when installed and
    /// populated, otherwise the configured ones.
    fn effective_roots(&self) -> Vec<PathBuf> {
        if let Some(handle) = &self.world {
            if let Some(roots) = handle.current() {
                return roots.allowed_roots;
            }
        }
        self.config.allowed_roots.clone()
    }

    /// Home boundary currently in effect: the world override's home when
    /// installed and populated, otherwise the local home.
    fn effective_home(&self) -> Option<PathBuf> {
        if let Some(handle) = &self.world {
            if let Some(roots) = handle.current() {
                if roots.home_dir.is_some() {
                    return roots.home_dir;
                }
            }
        }
        self.home_dir.clone()
    }

    /// Inject the filesystem world used for canonicalization (§4.11).
    pub fn with_fs_provider(
        mut self,
        fs: std::sync::Arc<dyn shannon_tool_interface::FileSystemProvider>,
    ) -> Self {
        self.fs = fs;
        self
    }

    /// Echo tool-output paths in the command sandbox's view (A3,
    /// docs/eval-findings-2026-09-glm.md).
    ///
    /// When the command sandbox backends (bwrap/Docker in shannon-core's
    /// sandbox.rs) bind the project dir at `/workspace`, the file tools run
    /// on the host and canonicalize there — so every echoed path used to be
    /// a host absolute path the model could not use in the sandboxed shell
    /// (`cd /workspace/src` worked, `cd /home/.../workspace/src` did not).
    /// With this flag set, output echo points run host paths back through
    /// [`PathSandbox::alias_display_path`] so the model sees one consistent,
    /// sandbox-visible path space. Internal reads/writes are unaffected:
    /// the host-canonical path keeps flowing through the actual I/O.
    pub fn with_bind_alias_output(mut self, on: bool) -> Self {
        self.bind_alias_output = on;
        self
    }

    /// Whether output-echo aliasing is enabled.
    pub fn bind_alias_output(&self) -> bool {
        self.bind_alias_output
    }

    /// Roots whose children are rendered under the bind alias in output.
    ///
    /// The temp root is excluded on purpose: the command sandbox exposes
    /// `/tmp` at the same literal path (tmpfs mount), so a host `/tmp/...`
    /// path is already sandbox-visible spelling and must not be rewritten
    /// to `/workspace/...`.
    fn alias_candidate_roots(&self) -> Vec<PathBuf> {
        if !self.bind_alias_output {
            return Vec::new();
        }
        self.effective_roots()
            .into_iter()
            .filter(|root| root.to_string_lossy() != SANDBOX_TMP_ROOT)
            .map(|root| self.fs.canonicalize_blocking(&root).unwrap_or(root))
            .collect()
    }

    /// Render a canonical host path the way the command sandbox sees it —
    /// the reverse of [`PathSandbox::remap_bind_alias`] (A3).
    ///
    /// A path under the project root becomes `/workspace/<rest>`; anything
    /// else (the temp root, paths outside every root) is returned unchanged.
    /// When output aliasing is off ([`PathSandbox::with_bind_alias_output`])
    /// this is the identity, so plain non-sandboxed assemblies keep echoing
    /// host paths exactly as before.
    pub fn alias_display_path(&self, path: &Path) -> String {
        for root in self.alias_candidate_roots() {
            if let Ok(rest) = path.strip_prefix(&root) {
                return match rest.as_os_str().is_empty() {
                    true => SANDBOX_BIND_ALIAS.to_string(),
                    false => format!("{SANDBOX_BIND_ALIAS}/{}", rest.display()),
                };
            }
        }
        path.to_string_lossy().to_string()
    }

    /// Apply [`PathSandbox::alias_display_path`] to every workspace path
    /// occurring in a blob of tool output text (A3).
    ///
    /// Used for tool-formatted echoes (Edit's success message, Glob/Grep
    /// path listings) where rewriting the root prefix is the point. Pure
    /// file *content* returned by Read is not pushed through this — only
    /// tool-generated path echoes are.
    pub fn alias_display_text(&self, text: &str) -> String {
        let mut out = text.to_string();
        for root in self.alias_candidate_roots() {
            out = replace_path_prefix(&out, &root.to_string_lossy(), SANDBOX_BIND_ALIAS);
        }
        out
    }

    /// Rewrite an entire tool output in place: the content blob plus every
    /// string buried in the metadata JSON (A3 echo points).
    pub fn remap_tool_output(&self, output: &mut crate::ToolOutput) {
        if !self.bind_alias_output {
            return;
        }
        output.content = self.alias_display_text(&output.content);
        for value in output.metadata.values_mut() {
            remap_json_strings(value, self);
        }
    }

    /// Comma-separated, sandbox-visible list of the roots currently in
    /// effect (B3): rejection messages must tell the model where writes
    /// WOULD be accepted, e.g. `allowed: /workspace, /tmp`.
    fn allowed_roots_summary(&self) -> String {
        self.effective_roots()
            .iter()
            .map(|root| self.alias_display_path(root))
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Remap a `/workspace/<rest>` path onto the allowed roots, in order,
    /// without any existence check.
    ///
    /// The command sandbox backends (bwrap and Docker in shannon-core's
    /// sandbox.rs) bind-mount the project dir at `/workspace`, so a model
    /// that has run a sandboxed command legitimately addresses files as
    /// `/workspace/<rest>` while the file tools canonicalize on the host.
    /// Without this remap the model's world-view splits: Bash succeeds on
    /// `/workspace/src/x.rs` and Read on the same path fails with
    /// "No such file" (dogfood l1, 2026-08-22).
    ///
    /// Only called after direct canonicalization failed, so a real host
    /// `/workspace` always wins. The mapped candidate is never returned
    /// as-is: callers re-canonicalize it and run the full check pipeline,
    /// so traversal (`/workspace/../etc`) still dies in
    /// `check_allowed_roots` / denied-pattern checks.
    fn remap_bind_alias(&self, path: &Path) -> Option<Vec<PathBuf>> {
        // Component-based, so "/workspacefoo" does not match.
        let rest = path.strip_prefix(SANDBOX_BIND_ALIAS).ok()?;
        let roots = self.effective_roots();
        if roots.is_empty() {
            return None;
        }
        Some(roots.iter().map(|root| root.join(rest)).collect())
    }

    /// Async companion of `remap_bind_alias` that also canonicalizes the
    /// candidate — the value read paths substitute for the failed direct
    /// canonicalization. First existing candidate wins.
    async fn resolve_bind_alias(&self, path: &Path) -> Option<PathBuf> {
        let candidates = self.remap_bind_alias(path)?;
        for candidate in candidates {
            if let Ok(c) = self.fs.canonicalize(&candidate).await {
                return Some(c);
            }
        }
        None
    }

    /// Validate a path against the sandbox rules.
    ///
    /// Returns the canonicalized (resolved) path if access is allowed.
    /// Returns `SandboxError` if the path violates any security rule.
    ///
    /// # TOCTOU Protection
    ///
    /// This method uses immediate canonicalization to protect against
    /// time-of-check/time-of-use (TOCTOU) attacks. By canonicalizing
    /// the path right before checking it, we minimize the window where
    /// an attacker could replace a path component with a symlink.
    ///
    /// # Checks performed in order:
    /// 1. Path is not empty
    /// 2. Path does not contain raw `..` traversal that escapes the filesystem
    /// 3. Path can be canonicalized (symlinks resolved) - **TOCTOU protection point**
    /// 4. Canonicalized path does not match any denied pattern
    /// 5. In strict mode, canonicalized path is under an allowed root
    /// 6. Path does not cross into another user's home directory
    pub async fn validate(&self, path: &Path) -> Result<PathBuf, SandboxError> {
        if path.as_os_str().is_empty() {
            return Err(SandboxError::InvalidPath);
        }

        let path_str = path.to_string_lossy().to_string();

        // Check for obviously malicious raw traversal patterns
        // (canonicalize will catch these too, but we provide a clearer error)
        self.check_raw_traversal(&path_str)?;

        // Canonicalize: resolve symlinks, `.` and `..` components
        // This is the primary TOCTOU protection - we resolve the actual
        // target immediately before checking it against allowed roots.
        // On failure, retry through the sandbox bind alias (`/workspace`),
        // which the command sandbox uses for the project dir — see
        // `remap_bind_alias`. Every check below still runs on the result.
        let canonical = match self.fs.canonicalize(path).await {
            Ok(c) => c,
            Err(e) => self.resolve_bind_alias(path).await.ok_or_else(|| {
                SandboxError::ResolutionFailed(format!("Cannot resolve path '{path_str}': {e}"))
            })?,
        };

        let canonical_str = canonical.to_string_lossy().to_string();

        // Check denied patterns against the resolved path
        self.check_denied_patterns(&canonical_str)?;

        // In strict mode, verify the resolved path is under an allowed root
        if self.config.strict_mode {
            self.check_allowed_roots(&canonical)?;
        }

        // Check home directory boundaries
        self.check_home_boundary(&canonical)?;

        Ok(canonical)
    }

    /// Synchronous version of `validate` for use in non-async contexts.
    ///
    /// Has the same TOCTOU protection properties as `validate` - uses
    /// immediate canonicalization to resolve symlinks before checking.
    pub fn validate_sync(&self, path: &Path) -> Result<PathBuf, SandboxError> {
        if path.as_os_str().is_empty() {
            return Err(SandboxError::InvalidPath);
        }

        let path_str = path.to_string_lossy().to_string();

        self.check_raw_traversal(&path_str)?;

        let canonical = match self.fs.canonicalize_blocking(path) {
            Ok(c) => c,
            Err(e) => {
                // Bind-alias fallback — see `remap_bind_alias`. Checks below
                // still run on the resolved candidate.
                let mut resolved = None;
                if let Some(candidates) = self.remap_bind_alias(path) {
                    resolved = candidates
                        .into_iter()
                        .find_map(|c| self.fs.canonicalize_blocking(&c).ok());
                }
                resolved.ok_or_else(|| {
                    SandboxError::ResolutionFailed(format!("Cannot resolve path '{path_str}': {e}"))
                })?
            }
        };

        let canonical_str = canonical.to_string_lossy().to_string();

        self.check_denied_patterns(&canonical_str)?;

        if self.config.strict_mode {
            self.check_allowed_roots(&canonical)?;
        }

        self.check_home_boundary(&canonical)?;

        Ok(canonical)
    }

    /// Validate a path for writing, handling non-existent target files.
    ///
    /// Unlike `validate()`, this method handles the case where the target file
    /// does not exist yet. It canonicalizes the parent directory and appends
    /// the filename, allowing the Write tool to create new files.
    pub async fn validate_for_write(&self, path: &Path) -> Result<PathBuf, SandboxError> {
        if path.as_os_str().is_empty() {
            return Err(SandboxError::InvalidPath);
        }

        let path_str = path.to_string_lossy().to_string();
        self.check_raw_traversal(&path_str)?;

        // Try canonicalizing the full path first (works for existing files)
        if let Ok(canonical) = self.fs.canonicalize(path).await {
            self.check_denied_patterns(&canonical.to_string_lossy())?;
            if self.config.strict_mode {
                self.check_allowed_roots(&canonical)?;
            }
            self.check_home_boundary(&canonical)?;
            return Ok(canonical);
        }

        // File doesn't exist — canonicalize the nearest EXISTING ancestor and
        // re-append the missing components. Write creates missing parent dirs
        // (see `write::execute`'s `create_dir_all`), so a not-yet-existing
        // parent is legitimate. Components below an existing ancestor cannot
        // be symlinks, so resolving only the ancestor keeps the same TOCTOU
        // posture as canonicalizing the full path; every check below still
        // runs against the complete reconstructed path.
        //
        // Bind alias first: a `/workspace/<rest>` write with missing parents
        // must be remapped to the host project root BEFORE the ancestor
        // walk — otherwise the walk escapes to the filesystem root `/`,
        // reconstructs the literal /workspace path, and the allowed-roots
        // check correctly rejects it. Remap needs no existence check here;
        // the walk below resolves whichever ancestors do exist.
        let remapped: PathBuf;
        let path: &Path = if let Some(mut candidates) = self.remap_bind_alias(path) {
            // First root wins for creation semantics; read paths
            // (`resolve_bind_alias`) prefer the first existing candidate.
            remapped = candidates.remove(0);
            &remapped
        } else {
            path
        };
        let path_str = path.to_string_lossy().to_string();

        let parent = path.parent().ok_or_else(|| {
            SandboxError::ResolutionFailed(format!("Cannot resolve path '{path_str}': no parent"))
        })?;

        let file_name = path.file_name().ok_or_else(|| {
            SandboxError::ResolutionFailed(format!("Cannot resolve path '{path_str}': no filename"))
        })?;

        // Walk up until an ancestor canonicalizes; collect the missing tail.
        let mut missing: Vec<std::ffi::OsString> = Vec::new();
        let mut cur = parent;
        let canonical_parent = loop {
            match self.fs.canonicalize(cur).await {
                Ok(c) => break c,
                Err(_) => {
                    let Some(name) = cur.file_name() else {
                        return Err(SandboxError::ResolutionFailed(format!(
                            "Cannot resolve parent directory '{}': no existing ancestor",
                            parent.display()
                        )));
                    };
                    missing.push(name.to_os_string());
                    match cur.parent() {
                        Some(p) => cur = p,
                        None => {
                            return Err(SandboxError::ResolutionFailed(format!(
                                "Cannot resolve parent directory '{}': no existing ancestor",
                                parent.display()
                            )));
                        }
                    }
                }
            }
        };

        let mut canonical = canonical_parent;
        for comp in missing.iter().rev() {
            canonical.push(comp);
        }
        canonical.push(file_name);
        let canonical_str = canonical.to_string_lossy().to_string();

        self.check_denied_patterns(&canonical_str)?;
        if self.config.strict_mode {
            self.check_allowed_roots(&canonical)?;
        }
        self.check_home_boundary(&canonical)?;

        Ok(canonical)
    }

    /// Check for raw `..` traversal components before canonicalization.
    ///
    /// This provides a more descriptive error message. Even if this check
    /// passes, canonicalization may still detect a traversal that resolves
    /// outside allowed roots.
    fn check_raw_traversal(&self, path_str: &str) -> Result<(), SandboxError> {
        // Count `..` components to detect potential traversal
        let components: Vec<&str> = path_str.split('/').collect();
        let mut depth = 0i32;
        for comp in &components {
            if *comp == ".." {
                depth -= 1;
                if depth < 0 {
                    return Err(SandboxError::PathTraversal(format!(
                        "Path '{path_str}' contains '..' that escapes the root directory"
                    )));
                }
            } else if *comp != "." && !comp.is_empty() {
                depth += 1;
            }
        }
        Ok(())
    }

    /// Check if the canonicalized path matches any denied pattern.
    fn check_denied_patterns(&self, canonical_str: &str) -> Result<(), SandboxError> {
        for pattern in &self.config.denied_patterns {
            // Match as prefix. Both "/etc/passwd" and "/etc/" itself should match "/etc/"
            if canonical_str.starts_with(pattern) || canonical_str == pattern.trim_end_matches('/')
            {
                // B3: even a denied-pattern hit should tell the model where
                // writes ARE accepted, so a rejected Write is recoverable in
                // one turn instead of three guesses.
                return Err(SandboxError::Denied(format!(
                    "Path '{canonical_str}' is in a restricted area (matches '{pattern}'); \
                     allowed roots: {}",
                    self.allowed_roots_summary()
                )));
            }
        }
        Ok(())
    }

    /// In strict mode, verify the canonicalized path is under an allowed root.
    fn check_allowed_roots(&self, canonical: &Path) -> Result<(), SandboxError> {
        let canonical_str = canonical.to_string_lossy().to_string();

        for root in &self.effective_roots() {
            // Canonicalize the root as well so comparison is consistent
            let resolved_root = match self.fs.canonicalize_blocking(root) {
                Ok(r) => r,
                Err(_) => {
                    // If root doesn't exist yet (e.g., a project dir not yet created),
                    // try to canonicalize it first for comparison, then fall back to prefix matching
                    // Canonicalize the root path to resolve any symlinks before comparison
                    let canonical_root = match self.fs.canonicalize_blocking(root) {
                        Ok(r) => r,
                        Err(_) => {
                            // Root doesn't exist and can't be canonicalized,
                            // use as-is with trailing separator for prefix matching
                            let root_str = root.to_string_lossy().to_string();
                            let root_with_sep = if root_str.ends_with('/') {
                                root_str
                            } else {
                                format!("{root_str}/")
                            };
                            if canonical_str.starts_with(&root_with_sep) {
                                return Ok(());
                            }
                            continue;
                        }
                    };
                    // Compare canonicalized paths to prevent symlink escape
                    if canonical == canonical_root.as_os_str() {
                        return Ok(());
                    }
                    continue;
                }
            };

            let resolved_root_str = resolved_root.to_string_lossy().to_string();
            // Check if canonical is the root itself or a child of it
            if canonical == resolved_root
                || canonical_str.starts_with(&format!("{resolved_root_str}/"))
                // Handle Windows paths with backslash
                || canonical_str.starts_with(&format!("{resolved_root_str}\\"))
            {
                return Ok(());
            }
        }

        Err(SandboxError::OutsideAllowedRoots(format!(
            "Path '{}' is not within any allowed root; allowed: {}. \
             Address files relative to the current working directory or under \
             an allowed root.",
            self.alias_display_path(canonical),
            self.allowed_roots_summary()
        )))
    }

    /// Check that the path doesn't cross into another user's home directory.
    fn check_home_boundary(&self, canonical: &Path) -> Result<(), SandboxError> {
        if let Some(ref my_home) = self.effective_home() {
            let my_home_str = my_home.to_string_lossy().to_string();

            // Get the canonical form of /home or determine if this path is
            // under a home directory that isn't ours
            let canonical_str = canonical.to_string_lossy().to_string();

            // Only check if the path is under /home/ or a typical home root
            let home_roots = ["/home/", "C:\\Users\\"];
            let is_under_home_root = home_roots.iter().any(|hr| canonical_str.starts_with(hr));

            if is_under_home_root {
                // Check if it's under our home directory
                let my_home_with_sep = if my_home_str.ends_with('/') {
                    my_home_str.clone()
                } else {
                    format!("{my_home_str}/")
                };

                if !canonical_str.starts_with(&my_home_with_sep) {
                    return Err(SandboxError::Denied(format!(
                        "Path '{canonical_str}' is in another user's home directory"
                    )));
                }
            }
        }
        Ok(())
    }

    /// Add an additional allowed root directory.
    pub fn add_allowed_root(&mut self, root: PathBuf) {
        if !self.config.allowed_roots.contains(&root) {
            self.config.allowed_roots.push(root);
        }
    }

    /// Add an additional denied pattern (path prefix).
    pub fn add_denied_pattern(&mut self, pattern: String) {
        if !self.config.denied_patterns.contains(&pattern) {
            self.config.denied_patterns.push(pattern);
        }
    }

    /// Get a reference to the sandbox configuration.
    pub fn config(&self) -> &SandboxConfig {
        &self.config
    }
}

impl Default for PathSandbox {
    fn default() -> Self {
        Self::new()
    }
}

/// Replace `from` with `to` in `text`, but only at path boundaries.
///
/// An occurrence counts as a path prefix when the preceding character is not
/// itself part of a longer path and the following character opens a path
/// continuation (`/`), ends the token, or is punctuation — so
/// `/tmp/eval/workspace/src` rewrites but `/tmp/eval/workspace-backup/x`
/// and `/tmp/eval/workspacefoo` stay untouched.
fn replace_path_prefix(text: &str, from: &str, to: &str) -> String {
    if from.len() < 2 || from == to {
        // Root "/" would rewrite every absolute path; skip degenerate cases.
        return text.to_string();
    }
    let is_path_char =
        |c: char| c.is_alphanumeric() || matches!(c, '_' | '.' | '-');
    let mut result = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(pos) = rest.find(from) {
        let (before, after) = rest.split_at(pos);
        let tail = &after[from.len()..];
        // Embedded in a longer path (`.../workspace-backup`, `workspacefoo`)
        // the match is not the root — keep it. A `/` tail is a CHILD path
        // and is exactly the case being rewritten.
        let preceded_by_path = before.ends_with(|c: char| is_path_char(c) || c == '/');
        let tail_blocks =
            matches!(tail.chars().next(), Some(c) if is_path_char(c));
        if preceded_by_path || tail_blocks {
            result.push_str(before);
            result.push_str(from);
        } else {
            result.push_str(before);
            result.push_str(to);
        }
        rest = tail;
    }
    result.push_str(rest);
    result
}

/// Recursively rewrite every JSON string through the alias display mapping.
fn remap_json_strings(
    value: &mut serde_json::Value,
    sandbox: &PathSandbox,
) {
    match value {
        serde_json::Value::String(s) => {
            let remapped = sandbox.alias_display_text(s);
            if remapped != *s {
                *s = remapped;
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                remap_json_strings(item, sandbox);
            }
        }
        serde_json::Value::Object(map) => {
            for (_key, item) in map.iter_mut() {
                remap_json_strings(item, sandbox);
            }
        }
        _ => {}
    }
}

/// Attempt to determine the current user's home directory.
/// Returns `None` if it cannot be determined.
fn dirs_home_dir() -> Option<PathBuf> {
    // Try standard environment variables first
    if let Ok(home) = std::env::var("HOME") {
        let path = PathBuf::from(&home);
        if path.is_dir() {
            return Some(path);
        }
    }

    // Fallback: check /etc/passwd for the current user
    if let Ok(uid) = std::env::var("USER") {
        // We can't easily parse /etc/passwd in a portable way without deps,
        // so just construct /home/<user> as a best-effort guess
        let guess = PathBuf::from("/home").join(&uid);
        if guess.is_dir() {
            return Some(guess);
        }
    }

    None
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::fs;

    /// Helper to create a temporary directory structure for sandbox tests.
    struct TestDir {
        root: TempDirHolder,
    }

    impl TestDir {
        fn new() -> Self {
            // Use a unique suffix to avoid collisions between parallel tests
            let unique = format!(
                "sandbox_test_{}_{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            );
            let dir = std::env::temp_dir().join(unique);
            fs::create_dir_all(&dir).expect("Failed to create test dir");
            Self {
                root: TempDirHolder(dir),
            }
        }

        fn path(&self) -> &Path {
            self.root.path()
        }

        fn file(&self, relative: &str) -> PathBuf {
            self.root.path().join(relative)
        }

        fn create_file(&self, relative: &str, content: &str) -> PathBuf {
            let path = self.file(relative);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("Failed to create parent dirs");
            }
            fs::write(&path, content).expect("Failed to write test file");
            path
        }

        fn create_symlink(&self, link: &str, target: &Path) -> PathBuf {
            let link_path = self.file(link);
            if let Some(parent) = link_path.parent() {
                fs::create_dir_all(parent).expect("Failed to create parent dirs");
            }
            #[cfg(unix)]
            std::os::unix::fs::symlink(target, &link_path).expect("Failed to create symlink");
            #[cfg(windows)]
            std::os::windows::fs::symlink_file(target, &link_path)
                .expect("Failed to create symlink");
            link_path
        }
    }

    // Minimal tempdir stand-in that cleans up on drop
    struct TempDirHolder(PathBuf);
    impl TempDirHolder {
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TempDirHolder {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    // --- Path traversal tests ---

    #[tokio::test]
    async fn test_path_traversal_detected() {
        let td = TestDir::new();
        td.create_file("secret.txt", "sensitive data");

        let sandbox = PathSandbox::with_config(SandboxConfig {
            allowed_roots: vec![td.path().to_path_buf()],
            denied_patterns: vec![],
            strict_mode: true,
        });

        // Attempt to traverse above the allowed root
        let malicious = td.file("../../etc/passwd");
        let result = sandbox.validate(&malicious).await;
        assert!(result.is_err(), "Expected error for path traversal, got OK");
        let err = result.unwrap_err().to_string();
        let err_lower = err.to_lowercase();
        assert!(
            err_lower.contains("traversal")
                || err_lower.contains("outside allowed roots")
                || err_lower.contains("cannot resolve"),
            "Expected traversal or outside-roots error, got: {err}"
        );
    }

    #[tokio::test]
    async fn test_path_with_dotdot_components() {
        let td = TestDir::new();
        td.create_file("subdir/file.txt", "hello");

        let sandbox = PathSandbox::with_config(SandboxConfig {
            allowed_roots: vec![td.path().to_path_buf()],
            denied_patterns: vec![],
            strict_mode: true,
        });

        // `subdir/../subdir/file.txt` should resolve fine (it stays inside root)
        let path = td.file("subdir/../subdir/file.txt");
        let result = sandbox.validate(&path).await;
        // This should succeed because after resolution it stays within the root
        assert!(result.is_ok(), "Expected OK, got: {result:?}");
    }

    // --- Denied path tests ---

    #[tokio::test]
    async fn test_denied_etc_path() {
        let sandbox = PathSandbox::with_config(SandboxConfig {
            allowed_roots: vec![PathBuf::from("/")],
            denied_patterns: vec!["/etc/".to_string()],
            strict_mode: true,
        });

        // /etc/passwd should be denied
        let result = sandbox.validate(Path::new("/etc/passwd")).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("restricted"));
    }

    #[tokio::test]
    async fn test_denied_boot_path() {
        let sandbox = PathSandbox::with_config(SandboxConfig {
            allowed_roots: vec![PathBuf::from("/")],
            denied_patterns: vec!["/boot/".to_string()],
            strict_mode: true,
        });

        let result = sandbox.validate(Path::new("/boot/vmlinuz")).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_denied_dev_path() {
        let sandbox = PathSandbox::with_config(SandboxConfig {
            allowed_roots: vec![PathBuf::from("/")],
            denied_patterns: vec!["/dev/".to_string()],
            strict_mode: true,
        });

        let result = sandbox.validate(Path::new("/dev/null")).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_denied_usr_bin_path() {
        let sandbox = PathSandbox::with_config(SandboxConfig {
            allowed_roots: vec![PathBuf::from("/")],
            denied_patterns: vec!["/usr/bin/".to_string()],
            strict_mode: true,
        });

        let result = sandbox.validate(Path::new("/usr/bin/ls")).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_denied_proc_path() {
        let sandbox = PathSandbox::with_config(SandboxConfig {
            allowed_roots: vec![PathBuf::from("/")],
            denied_patterns: vec!["/proc/".to_string()],
            strict_mode: true,
        });

        let result = sandbox.validate(Path::new("/proc/self/mem")).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_denied_sys_path() {
        let sandbox = PathSandbox::with_config(SandboxConfig {
            allowed_roots: vec![PathBuf::from("/")],
            denied_patterns: vec!["/sys/".to_string()],
            strict_mode: true,
        });

        let result = sandbox.validate(Path::new("/sys/kernel/notes")).await;
        assert!(result.is_err());
    }

    // --- Allowed root tests ---

    #[tokio::test]
    async fn test_allowed_root_access() {
        let td = TestDir::new();
        td.create_file("project/src/main.rs", "fn main() {}");

        let sandbox = PathSandbox::with_config(SandboxConfig {
            allowed_roots: vec![td.path().to_path_buf()],
            denied_patterns: vec![],
            strict_mode: true,
        });

        let result = sandbox.validate(&td.file("project/src/main.rs")).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_outside_allowed_roots_denied() {
        let td = TestDir::new();
        td.create_file("file.txt", "data");

        // Create a separate allowed root
        let allowed = std::env::temp_dir().join(format!("sandbox_allowed_{}", std::process::id()));
        fs::create_dir_all(&allowed).expect("Failed to create allowed dir");

        let sandbox = PathSandbox::with_config(SandboxConfig {
            allowed_roots: vec![allowed.clone()],
            denied_patterns: vec![],
            strict_mode: true,
        });

        // Try to access a file in a different directory
        let result = sandbox.validate(&td.file("file.txt")).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("not within any allowed root")
        );

        let _ = fs::remove_dir_all(&allowed);
    }

    #[tokio::test]
    async fn test_multiple_allowed_roots() {
        let td1 = TestDir::new();
        let td1 = &td1;
        let td1_file = td1.create_file("file.txt", "data in td1");

        let td2_dir = std::env::temp_dir().join(format!("sandbox_td2_{}", std::process::id()));
        fs::create_dir_all(&td2_dir).expect("Failed to create td2");
        let td2_file = td2_dir.join("file.txt");
        fs::write(&td2_file, "data in td2").expect("Failed to write td2 file");

        let sandbox = PathSandbox::with_config(SandboxConfig {
            allowed_roots: vec![td1.path().to_path_buf(), td2_dir.clone()],
            denied_patterns: vec![],
            strict_mode: true,
        });

        // Both roots should be accessible
        assert!(sandbox.validate(&td1_file).await.is_ok());
        assert!(sandbox.validate(&td2_file).await.is_ok());

        let _ = fs::remove_dir_all(&td2_dir);
    }

    // --- Sandbox bind alias (/workspace) tests ---
    //
    // bwrap/Docker bind the project dir at /workspace (shannon-core
    // sandbox.rs), so after a sandboxed Bash command the model addresses
    // files as /workspace/<rest>. The file tools run on the host and must
    // remap that prefix onto the allowed root (dogfood l1, 2026-08-22).

    fn alias_sandbox(root: &Path) -> PathSandbox {
        PathSandbox::with_config(SandboxConfig {
            allowed_roots: vec![root.to_path_buf()],
            denied_patterns: SandboxConfig::default_denied_patterns(),
            strict_mode: true,
        })
    }

    #[tokio::test]
    async fn test_bind_alias_read_maps_to_project_root() {
        let td = TestDir::new();
        let file = td.create_file("src/lib.rs", "pub fn f() {}");
        let expected = fs::canonicalize(&file).expect("canonicalize fixture");

        let sandbox = alias_sandbox(td.path());
        let result = sandbox.validate(Path::new("/workspace/src/lib.rs")).await;
        assert_eq!(
            result.expect("alias path should resolve"),
            expected,
            "alias path must canonicalize to the same host file"
        );
    }

    #[tokio::test]
    async fn test_bind_alias_sync_read_maps_to_project_root() {
        let td = TestDir::new();
        let file = td.create_file("src/main.rs", "fn main() {}");
        let expected = fs::canonicalize(&file).expect("canonicalize fixture");

        let sandbox = alias_sandbox(td.path());
        let result = sandbox.validate_sync(Path::new("/workspace/src/main.rs"));
        assert_eq!(result.expect("alias path should resolve"), expected);
    }

    #[tokio::test]
    async fn test_bind_alias_write_with_missing_parents_maps() {
        let td = TestDir::new();
        let root = fs::canonicalize(td.path()).expect("canonicalize root");
        let expected = root.join("newdir/nested/file.rs");

        let sandbox = alias_sandbox(td.path());
        let result = sandbox
            .validate_for_write(Path::new("/workspace/newdir/nested/file.rs"))
            .await;
        assert_eq!(result.expect("alias write should resolve"), expected);
    }

    #[tokio::test]
    async fn test_bind_alias_unknown_path_still_fails() {
        let td = TestDir::new();
        let sandbox = alias_sandbox(td.path());
        let result = sandbox
            .validate(Path::new("/workspace/no-such-file.rs"))
            .await;
        assert!(result.is_err(), "missing target under root must fail");
    }

    #[tokio::test]
    async fn test_bind_alias_traversal_escape_denied() {
        let td = TestDir::new();
        // Existing sibling directory OUTSIDE the allowed root, reached via
        // `/workspace/../<sibling>`.
        let sibling =
            std::env::temp_dir().join(format!("sandbox_alias_sib_{}", std::process::id()));
        fs::create_dir_all(&sibling).expect("create sibling dir");

        let sandbox = alias_sandbox(td.path());
        let alias_escape = Path::new("/workspace")
            .join("..")
            .join(sibling.file_name().expect("sibling name"));
        let result = sandbox.validate(&alias_escape).await;

        let _ = fs::remove_dir_all(&sibling);
        assert!(
            result.is_err(),
            "alias remap must not bypass allowed-roots: got {result:?}"
        );
    }

    #[tokio::test]
    async fn test_bind_alias_root_itself_maps() {
        let td = TestDir::new();
        let expected = fs::canonicalize(td.path()).expect("canonicalize root");

        let sandbox = alias_sandbox(td.path());
        let result = sandbox.validate(Path::new("/workspace")).await;
        assert_eq!(
            result.expect("/workspace maps to the project root"),
            expected
        );
    }

    // --- Output alias (bind-alias reverse map, A3) tests ---
    //
    // With `with_bind_alias_output(true)` the sandbox renders canonical host
    // paths the way the command sandbox sees them, so a path echoed by a
    // file tool works verbatim in a sandboxed Bash command.

    /// Sandbox wired like the registration assembly: project root + the temp
    /// root (B3 alignment), output aliasing enabled.
    fn alias_output_sandbox(root: &Path) -> PathSandbox {
        PathSandbox::with_config(SandboxConfig {
            allowed_roots: SandboxConfig::command_aligned_roots(root),
            denied_patterns: SandboxConfig::default_denied_patterns(),
            strict_mode: true,
        })
        .with_bind_alias_output(true)
    }

    #[test]
    fn command_aligned_roots_include_project_and_temp() {
        let td = TestDir::new();
        let roots = SandboxConfig::command_aligned_roots(td.path());
        assert_eq!(roots.len(), 2, "project dir + temp root");
        assert!(roots.contains(&td.path().to_path_buf()));
        assert!(roots.contains(&std::env::temp_dir()));
    }

    #[test]
    fn alias_display_path_maps_project_children() {
        let td = TestDir::new();
        let sandbox = alias_output_sandbox(td.path());
        let root = fs::canonicalize(td.path()).expect("canonicalize root");
        let file = fs::canonicalize(td.create_file("src/lib.rs", "x")).expect("fixture");

        assert_eq!(sandbox.alias_display_path(&file), "/workspace/src/lib.rs");
        assert_eq!(sandbox.alias_display_path(&root), "/workspace");
    }

    #[test]
    fn alias_display_path_leaves_temp_and_outside_paths() {
        let td = TestDir::new();
        let sandbox = alias_output_sandbox(td.path());

        // The temp root keeps its literal (sandbox-visible) spelling.
        let tmp_file = std::env::temp_dir().join("alias_display_probe.txt");
        assert_eq!(
            sandbox.alias_display_path(&tmp_file),
            tmp_file.to_string_lossy()
        );
        // Paths outside every root are echoed unchanged.
        assert_eq!(sandbox.alias_display_path(Path::new("/etc/hosts")), "/etc/hosts");
    }

    #[test]
    fn alias_display_path_identity_when_disabled() {
        let td = TestDir::new();
        let sandbox = alias_sandbox(td.path()); // flag off
        assert!(!sandbox.bind_alias_output());
        let file = fs::canonicalize(td.create_file("src/lib.rs", "x")).expect("fixture");
        assert_eq!(
            sandbox.alias_display_path(&file),
            file.to_string_lossy(),
            "no alias mount -> host path echoed unchanged"
        );
    }

    #[test]
    fn alias_display_text_rewrites_only_at_path_boundaries() {
        let td = TestDir::new();
        let sandbox = alias_output_sandbox(td.path());
        let root = fs::canonicalize(td.path()).expect("canonicalize root");
        let root_str = root.to_string_lossy().to_string();

        // Child paths rewrite; multiple occurrences handled.
        let text = format!("head {root_str}/src/a.rs mid {root_str}/src/b.rs tail");
        assert_eq!(
            sandbox.alias_display_text(&text),
            "head /workspace/src/a.rs mid /workspace/src/b.rs tail"
        );

        // Sibling names sharing the prefix stay untouched.
        for sibling in [format!("{root_str}-backup/x.rs"), format!("{root_str}foo")] {
            assert_eq!(
                sandbox.alias_display_text(&sibling),
                sibling,
                "boundary safety"
            );
        }

        // The bare root inside prose rewrites too.
        assert_eq!(
            sandbox.alias_display_text(&format!("edited {root_str}")),
            "edited /workspace"
        );
    }

    #[tokio::test]
    async fn outside_roots_error_lists_sandbox_view_roots() {
        let td = TestDir::new();
        let sandbox = alias_output_sandbox(td.path());

        // /usr/lib exists on every Unix CI image, is not denied, and is not
        // under either root -> OutsideAllowedRoots.
        let result = sandbox.validate(Path::new("/usr/lib")).await;
        let err = result.err().expect("/usr/lib must be rejected").to_string();
        assert!(err.contains("not within any allowed root"), "got: {err}");
        assert!(
            err.contains("allowed: /workspace") && err.contains("/tmp"),
            "error must list sandbox-visible roots, got: {err}"
        );
        assert!(
            !err.contains(&fs::canonicalize(td.path()).unwrap().to_string_lossy().to_string()),
            "host workspace path must not leak into the error, got: {err}"
        );
    }

    #[tokio::test]
    async fn denied_pattern_error_lists_allowed_roots() {
        let td = TestDir::new();
        let sandbox = alias_output_sandbox(td.path());

        let result = sandbox.validate(Path::new("/etc/hosts")).await;
        let err = result.err().expect("/etc/hosts must be rejected").to_string();
        assert!(err.contains("restricted"), "got: {err}");
        assert!(
            err.contains("allowed roots: /workspace") && err.contains("/tmp"),
            "denied error must list the allowed roots, got: {err}"
        );
    }

    #[tokio::test]
    async fn validate_for_write_allows_temp_root() {
        // B3: the command sandbox can write /tmp; the file tools must too.
        let td = TestDir::new();
        let sandbox = alias_output_sandbox(td.path());

        let target = std::env::temp_dir().join(format!(
            "sandbox_b3_write_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let result = sandbox.validate_for_write(&target).await;
        assert!(
            result.is_ok(),
            "write under the temp root must be allowed: {result:?}"
        );
    }

    // --- Symlink tests ---

    #[tokio::test]
    async fn test_symlink_inside_allowed_root() {
        let td = TestDir::new();
        let target = td.create_file("real.txt", "real content");
        let link = td.create_symlink("link.txt", &target);

        let sandbox = PathSandbox::with_config(SandboxConfig {
            allowed_roots: vec![td.path().to_path_buf()],
            denied_patterns: vec![],
            strict_mode: true,
        });

        // Symlink that stays within allowed root should be fine
        let result = sandbox.validate(&link).await;
        assert!(
            result.is_ok(),
            "Symlink inside root should be allowed, got: {result:?}"
        );
    }

    #[tokio::test]
    async fn test_symlink_outside_allowed_root_blocked() {
        let td = TestDir::new();

        // Create a target outside the allowed root
        let outside_dir =
            std::env::temp_dir().join(format!("sandbox_outside_{}", std::process::id()));
        fs::create_dir_all(&outside_dir).expect("Failed to create outside dir");
        let outside_file = outside_dir.join("secret.txt");
        fs::write(&outside_file, "secret data").expect("Failed to write outside file");

        // Create a symlink inside the sandbox pointing outside
        let link = td.create_symlink("escape.txt", &outside_file);

        let sandbox = PathSandbox::with_config(SandboxConfig {
            allowed_roots: vec![td.path().to_path_buf()],
            denied_patterns: vec![],
            strict_mode: true,
        });

        let result = sandbox.validate(&link).await;
        assert!(result.is_err(), "Symlink escaping root should be blocked");

        let _ = fs::remove_dir_all(&outside_dir);
    }

    #[tokio::test]
    async fn test_symlink_to_system_file_blocked() {
        let td = TestDir::new();

        // Try to create a symlink to /etc/passwd (a common attack vector)
        // Note: This test doesn't create the actual symlink (would need privileges)
        // but verifies that even if such a symlink existed, it would be blocked
        #[cfg(unix)]
        {
            let etc_passwd = PathBuf::from("/etc/passwd");
            if etc_passwd.exists() {
                let link = td.file("etc_passwd_link");
                #[allow(unused_variables)]
                let symlink_result = std::os::unix::fs::symlink(&etc_passwd, &link);

                // Only test if we could create the symlink
                if symlink_result.is_ok() {
                    let sandbox = PathSandbox::with_config(SandboxConfig {
                        allowed_roots: vec![td.path().to_path_buf()],
                        denied_patterns: vec![],
                        strict_mode: true,
                    });

                    let result = sandbox.validate(&link).await;
                    assert!(result.is_err(), "Symlink to /etc/passwd should be blocked");

                    let _ = fs::remove_file(&link);
                }
            }
        }
    }

    #[tokio::test]
    async fn test_symlink_chain_does_not_escape() {
        let td = TestDir::new();

        // Create a chain: link1 -> link2 -> outside_file
        // This should still be blocked
        let outside_dir =
            std::env::temp_dir().join(format!("sandbox_chain_{}", std::process::id()));
        fs::create_dir_all(&outside_dir).expect("Failed to create outside dir");
        let outside_file = outside_dir.join("secret.txt");
        fs::write(&outside_file, "secret data").expect("Failed to write outside file");

        // Create first symlink (outside)
        let _link2 = td.create_symlink("link2", &outside_file);

        #[cfg(unix)]
        {
            // Create second symlink pointing to the first (also inside sandbox)
            let link1 = td.file("link1");
            let link2_path = td.file("link2");
            std::os::unix::fs::symlink(&link2_path, &link1).expect("Failed to create link1");

            let sandbox = PathSandbox::with_config(SandboxConfig {
                allowed_roots: vec![td.path().to_path_buf()],
                denied_patterns: vec![],
                strict_mode: true,
            });

            // Accessing link1 should fail (it resolves to outside the root)
            let result = sandbox.validate(&link1).await;
            assert!(
                result.is_err(),
                "Symlink chain escaping root should be blocked"
            );

            let _ = fs::remove_file(&link1);
        }

        let _ = fs::remove_dir_all(&outside_dir);
    }

    // --- Strict mode vs non-strict mode ---

    #[tokio::test]
    async fn test_non_strict_mode_allows_non_root_paths() {
        let td = TestDir::new();

        let sandbox = PathSandbox::with_config(SandboxConfig {
            allowed_roots: vec![td.path().to_path_buf()],
            denied_patterns: vec![], // No denied patterns
            strict_mode: false,      // Non-strict: only denied patterns apply
        });

        // In non-strict mode, paths outside roots are allowed (unless denied)
        // We use /tmp since it's not in the denied list
        let tmp_file = std::env::temp_dir().join("sandbox_non_strict_test.txt");
        fs::write(&tmp_file, "test").ok(); // May already exist, ignore error

        let result = sandbox.validate(&tmp_file).await;
        assert!(
            result.is_ok(),
            "Non-strict mode should allow non-root paths: {result:?}"
        );

        let _ = fs::remove_file(&tmp_file);
    }

    #[tokio::test]
    async fn test_non_strict_mode_still_denies_patterns() {
        let sandbox = PathSandbox::with_config(SandboxConfig {
            allowed_roots: vec![],
            denied_patterns: vec!["/etc/".to_string()],
            strict_mode: false,
        });

        let result = sandbox.validate(Path::new("/etc/passwd")).await;
        assert!(
            result.is_err(),
            "Denied patterns should apply even in non-strict mode"
        );
    }

    // --- Empty / invalid path tests ---

    #[tokio::test]
    async fn test_empty_path_rejected() {
        let sandbox = PathSandbox::new();
        let result = sandbox.validate(Path::new("")).await;
        assert!(result.is_err());
    }

    // --- Default configuration tests ---

    #[tokio::test]
    async fn test_default_config_denies_system_paths() {
        let sandbox = PathSandbox::new();

        let system_paths = [
            "/etc/passwd",
            "/etc/shadow",
            "/boot/vmlinuz",
            "/usr/bin/ls",
            "/dev/null",
            "/proc/self/status",
            "/sys/kernel/notes",
        ];

        for path_str in &system_paths {
            let result = sandbox.validate(Path::new(path_str)).await;
            assert!(
                result.is_err(),
                "Default config should deny '{path_str}', got: {result:?}"
            );
        }
    }

    #[tokio::test]
    async fn test_default_config_allows_cwd() {
        let sandbox = PathSandbox::new();

        // The current working directory itself should be accessible
        let cwd = std::env::current_dir().expect("Failed to get cwd");
        let result = sandbox.validate(&cwd).await;
        assert!(
            result.is_ok(),
            "Default config should allow CWD: {result:?}"
        );
    }

    // --- Home directory boundary tests ---

    #[tokio::test]
    async fn test_home_boundary_other_user_denied() {
        let sandbox = PathSandbox::with_config(SandboxConfig {
            allowed_roots: vec![PathBuf::from("/")],
            denied_patterns: vec![],
            strict_mode: false,
        });

        // /home/root should be denied if the current user isn't root
        // (This test may not work if running as root, but that's fine)
        if std::env::var("USER").map(|u| u != "root").unwrap_or(true) {
            let result = sandbox.validate(Path::new("/home/root/.bashrc")).await;
            // Path might not exist, but if it does exist, it should be denied
            // If it doesn't exist, we get ResolutionFailed which is also acceptable
            if let Err(e) = result {
                let err_str = e.to_string();
                assert!(
                    err_str.contains("another user") || err_str.contains("Cannot resolve"),
                    "Expected home boundary or resolution error, got: {err_str}"
                );
            }
        }
    }

    // --- Sync validation tests ---

    #[test]
    fn test_sync_validation_denies_system_paths() {
        let sandbox = PathSandbox::new();
        let result = sandbox.validate_sync(Path::new("/etc/passwd"));
        assert!(result.is_err());
    }

    #[test]
    fn test_sync_validation_allows_cwd() {
        let sandbox = PathSandbox::new();
        let cwd = std::env::current_dir().expect("Failed to get cwd");
        let result = sandbox.validate_sync(&cwd);
        assert!(result.is_ok(), "Sync validate should allow CWD: {result:?}");
    }

    // --- Builder-style API tests ---

    #[test]
    fn test_add_allowed_root() {
        let mut sandbox = PathSandbox::new();
        let extra = PathBuf::from("/tmp/sandbox_test_extra");
        sandbox.add_allowed_root(extra.clone());
        assert!(sandbox.config().allowed_roots.contains(&extra));
    }

    #[test]
    fn test_add_denied_pattern() {
        let mut sandbox = PathSandbox::new();
        sandbox.add_denied_pattern("/custom/denied/".to_string());
        assert!(
            sandbox
                .config()
                .denied_patterns
                .iter()
                .any(|p| p == "/custom/denied/")
        );
    }

    #[test]
    fn test_default_denied_patterns() {
        let patterns = SandboxConfig::default_denied_patterns();
        assert!(patterns.contains(&"/etc/".to_string()));
        assert!(patterns.contains(&"/dev/".to_string()));
        assert!(patterns.contains(&"/boot/".to_string()));
        assert!(patterns.contains(&"/usr/bin/".to_string()));
    }

    // --- Error boundary tests ---

    #[tokio::test]
    async fn test_empty_path_returns_invalid_path_error() {
        let sandbox = PathSandbox::new();
        let result = sandbox.validate(Path::new("")).await;
        assert!(result.is_err(), "Empty path should be rejected");
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("empty") || err.contains("Invalid"),
            "Error should mention empty/invalid path, got: {err}"
        );
    }

    #[test]
    fn test_sync_empty_path_returns_invalid_path_error() {
        let sandbox = PathSandbox::new();
        let result = sandbox.validate_sync(Path::new(""));
        assert!(result.is_err(), "Empty path should be rejected (sync)");
    }

    #[tokio::test]
    async fn test_path_traversal_escape_root_returns_error() {
        let td = TestDir::new();
        let sandbox = PathSandbox::with_config(SandboxConfig {
            allowed_roots: vec![td.path().to_path_buf()],
            denied_patterns: vec![],
            strict_mode: true,
        });
        // Attempt traversal that escapes the allowed root
        let malicious = td.file("../../../etc/passwd");
        let result = sandbox.validate(&malicious).await;
        assert!(result.is_err(), "Path traversal should be rejected");
        let err = result.unwrap_err().to_string();
        let err_lower = err.to_lowercase();
        assert!(
            err_lower.contains("traversal")
                || err_lower.contains("outside allowed")
                || err_lower.contains("cannot resolve"),
            "Expected traversal or escape error, got: {err}"
        );
    }

    #[tokio::test]
    async fn test_path_outside_allowed_root_strict_mode() {
        let td = TestDir::new();
        let sandbox = PathSandbox::with_config(SandboxConfig {
            allowed_roots: vec![td.path().to_path_buf()],
            denied_patterns: vec![],
            strict_mode: true,
        });
        // Use /etc/passwd which exists but is outside the allowed root
        let result = sandbox.validate(Path::new("/etc/passwd")).await;
        assert!(
            result.is_err(),
            "Path outside allowed root should be rejected in strict mode"
        );
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("not within any allowed root") || err.contains("restricted"),
            "Expected outside-roots error, got: {err}"
        );
    }

    #[tokio::test]
    async fn test_denied_pattern_overrides_allowed_root() {
        let td = TestDir::new();
        td.create_file("secret.key", "private-key-data");

        let sandbox = PathSandbox::with_config(SandboxConfig {
            allowed_roots: vec![td.path().to_path_buf()],
            denied_patterns: vec!["/etc/".to_string()],
            strict_mode: true,
        });
        // /etc/passwd is in the denied list, even if we somehow got past roots
        let result = sandbox.validate(Path::new("/etc/passwd")).await;
        assert!(result.is_err(), "Denied pattern should block access");
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("restricted"),
            "Error should mention restricted area, got: {err}"
        );
    }

    #[test]
    fn test_sync_path_outside_allowed_root_rejected() {
        let sandbox = PathSandbox::with_config(SandboxConfig {
            allowed_roots: vec![PathBuf::from("/tmp")],
            denied_patterns: vec![],
            strict_mode: true,
        });
        let result = sandbox.validate_sync(Path::new("/etc/passwd"));
        assert!(
            result.is_err(),
            "Sync validate should reject path outside allowed root"
        );
    }

    // --- validate_for_write tests ---

    #[tokio::test]
    async fn test_validate_for_write_existing_file() {
        let td = TestDir::new();
        td.create_file("existing.txt", "content");

        let sandbox = PathSandbox::with_config(SandboxConfig {
            allowed_roots: vec![td.path().to_path_buf()],
            denied_patterns: vec![],
            strict_mode: true,
        });

        let result = sandbox.validate_for_write(&td.file("existing.txt")).await;
        assert!(result.is_ok(), "Should allow writing to existing file");
    }

    #[tokio::test]
    async fn test_validate_for_write_new_file_in_allowed_root() {
        let td = TestDir::new();
        // Only create the directory, not the file
        let sandbox = PathSandbox::with_config(SandboxConfig {
            allowed_roots: vec![td.path().to_path_buf()],
            denied_patterns: vec![],
            strict_mode: true,
        });

        let new_file = td.file("brand_new_file.txt");
        assert!(!new_file.exists(), "File should not exist yet");

        let result = sandbox.validate_for_write(&new_file).await;
        assert!(
            result.is_ok(),
            "Should allow creating new file in allowed root: {result:?}"
        );
        let canonical = result.unwrap();
        assert!(
            canonical.ends_with("brand_new_file.txt"),
            "Canonical path should preserve filename: {canonical:?}"
        );
    }

    #[tokio::test]
    async fn test_validate_for_write_new_file_in_subdirectory() {
        let td = TestDir::new();
        td.create_file("src/.gitkeep", ""); // create src/ dir

        let sandbox = PathSandbox::with_config(SandboxConfig {
            allowed_roots: vec![td.path().to_path_buf()],
            denied_patterns: vec![],
            strict_mode: true,
        });

        let new_file = td.file("src/lib.rs");
        assert!(!new_file.exists(), "File should not exist yet");

        let result = sandbox.validate_for_write(&new_file).await;
        assert!(
            result.is_ok(),
            "Should allow creating new file in existing subdir: {result:?}"
        );
    }

    #[tokio::test]
    async fn test_validate_for_write_new_file_in_missing_subdirectory() {
        // Dogfood m4 regression: `Write ws/docs/API.md` where neither `ws/`
        // nor `ws/docs/` exists yet. The nearest existing ancestor is the
        // task root itself; the canonical path must still land inside it.
        let td = TestDir::new();

        let sandbox = PathSandbox::with_config(SandboxConfig {
            allowed_roots: vec![td.path().to_path_buf()],
            denied_patterns: vec![],
            strict_mode: true,
        });

        let new_file = td.file("ws/docs/API.md");
        assert!(
            !new_file.parent().unwrap().exists(),
            "Parent dirs should not exist yet"
        );

        let result = sandbox.validate_for_write(&new_file).await;
        assert!(
            result.is_ok(),
            "Should allow creating new file in not-yet-existing subdir: {result:?}"
        );
        let canonical = result.unwrap();
        assert!(
            canonical.starts_with(td.path()),
            "Canonical path must stay inside the allowed root: {canonical:?}"
        );
        assert!(canonical.ends_with("ws/docs/API.md"));
    }

    #[tokio::test]
    async fn test_validate_for_write_rejects_outside_allowed_root() {
        let td = TestDir::new();

        let sandbox = PathSandbox::with_config(SandboxConfig {
            allowed_roots: vec![td.path().to_path_buf()],
            denied_patterns: vec![],
            strict_mode: true,
        });

        let outside = PathBuf::from("/tmp/outside_sandbox_test.txt");
        let result = sandbox.validate_for_write(&outside).await;
        assert!(result.is_err(), "Should reject file outside allowed root");
    }

    #[tokio::test]
    async fn test_validate_for_write_rejects_denied_pattern() {
        let _td = TestDir::new();

        let sandbox = PathSandbox::with_config(SandboxConfig {
            allowed_roots: vec![PathBuf::from("/")],
            denied_patterns: vec!["/etc/".to_string()],
            strict_mode: true,
        });

        let result = sandbox
            .validate_for_write(Path::new("/etc/new_file.txt"))
            .await;
        assert!(result.is_err(), "Should reject file in denied pattern area");
    }

    #[tokio::test]
    async fn world_override_retargets_roots_and_home() {
        use std::sync::Arc as StdArc;

        let local = tempfile::tempdir().unwrap();
        let remote = tempfile::tempdir().unwrap();
        let local_file = local.path().join("x.txt");
        let remote_file = remote.path().join("y.txt");
        std::fs::write(&local_file, b"l").unwrap();
        std::fs::write(&remote_file, b"r").unwrap();

        let sandbox = PathSandbox::with_config(SandboxConfig {
            allowed_roots: vec![local.path().to_path_buf()],
            denied_patterns: SandboxConfig::default_denied_patterns(),
            strict_mode: true,
        });
        let handle = StdArc::new(WorldSandboxHandle::new());
        let sandbox = sandbox.with_world_sandbox(handle.clone());

        // Passthrough: configured roots still govern while override empty.
        assert!(
            sandbox.validate_sync(&local_file).is_ok(),
            "configured root should validate while override is empty"
        );
        assert!(sandbox.validate_sync(&remote_file).is_err());

        // Swap to the remote world: remote paths validate, local paths die
        // with a proper outside-roots denial (both files exist).
        handle.set(WorldRoots {
            allowed_roots: vec![remote.path().to_path_buf()],
            home_dir: Some(PathBuf::from("/home/remote-user")),
        });
        assert!(
            sandbox.validate_sync(&remote_file).is_ok(),
            "world override must retarget the allowed roots"
        );
        assert!(
            matches!(
                sandbox.validate_sync(&local_file),
                Err(SandboxError::OutsideAllowedRoots(_))
            ),
            "local root must be rejected once the world swapped"
        );

        // Clearing the override restores the configured roots.
        handle.set(WorldRoots::default());
        assert!(sandbox.validate_sync(&local_file).is_ok());
        assert!(sandbox.validate_sync(&remote_file).is_err());
    }
}
