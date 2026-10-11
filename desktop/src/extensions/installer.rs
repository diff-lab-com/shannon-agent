//! Installer trait — type-aware install adapters.
//!
//! P1 ships the trait only. Implementations land in P2 (MCP), P3 (Skills),
//! P4 (Agents), P5 (DataSource). The hub UI calls `install(entry)` and the
//! right adapter runs based on `entry.kind`.

use std::error::Error;
use std::fmt;

use async_trait::async_trait;

use super::types::{CatalogEntry, ConfirmationLevel, InstallTarget, InstalledAddon, ProgressSink};

/// Error returned by installer operations.
///
/// `user_facing_message` is shown in the UI as-is. Other variants are
/// translated to a generic error string by the Tauri command layer.
#[derive(Debug)]
pub enum InstallError {
    /// Network fetch failed, registry returned 5xx, etc.
    Network(String),
    /// File system error (clone failed, disk full, permission denied).
    Io(String),
    /// Bundle manifest is malformed, signature mismatch, etc.
    Format(String),
    /// OAuth flow was cancelled or returned an error.
    Auth(String),
    /// User declined the confirmation prompt.
    Cancelled,
    /// Installer doesn't support this entry shape.
    Unsupported(String),
    /// Anything else.
    Other(String),
}

impl fmt::Display for InstallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InstallError::Network(msg) => write!(f, "network error: {msg}"),
            InstallError::Io(msg) => write!(f, "io error: {msg}"),
            InstallError::Format(msg) => write!(f, "format error: {msg}"),
            InstallError::Auth(msg) => write!(f, "auth error: {msg}"),
            InstallError::Cancelled => write!(f, "cancelled by user"),
            InstallError::Unsupported(msg) => write!(f, "unsupported: {msg}"),
            InstallError::Other(msg) => write!(f, "{msg}"),
        }
    }
}

impl Error for InstallError {}

impl From<std::io::Error> for InstallError {
    fn from(err: std::io::Error) -> Self {
        InstallError::Io(err.to_string())
    }
}

impl From<serde_json::Error> for InstallError {
    fn from(err: serde_json::Error) -> Self {
        InstallError::Format(err.to_string())
    }
}

impl From<anyhow::Error> for InstallError {
    fn from(err: anyhow::Error) -> Self {
        InstallError::Other(err.to_string())
    }
}

/// Install-time content gate (Dangerous-install gate design, 2026-10-10,
/// decision D-B). The Tauri command layer hands every repo-based installer a
/// callback that re-scans the exact content about to be persisted and refuses
/// `Dangerous` verdicts unless the caller passed a valid
/// [`super::types::InstallConfirmation`].
///
/// Installers whose content is fully available before any mutation (stdio
/// spec, `.mcpb` manifest, native skill body) are gated directly in the
/// command; installers that must FETCH content (git clone) call this gate at
/// the last moment before promoting the fetch result to its final location —
/// after the fetch is staged and validated, before any persistent
/// registration. On `Err`, the installer must leave no trace (remove the
/// staging area, write nothing to settings/config).
pub type InstallContentGate = Box<dyn Fn(&str) -> Result<(), InstallError> + Send + Sync>;

/// Unique staging directory name for a repo clone in flight (D-B gate).
///
/// The dot prefix keeps an in-flight (or crash-orphaned) staging clone out of
/// the installed-addons listings — plugin slugs can never start with a dot
/// (`safe_plugin_name`), so the namespace is unambiguous. Same-parent
/// staging lets the post-gate promotion be a plain `fs::rename` (atomic, no
/// copy, same filesystem).
pub(crate) fn staging_dir_name(plugin: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    format!(".{plugin}.staging-{}-{nanos}", std::process::id())
}

/// Upstream source pin for repo-installed addons (扩展安全专项·步骤 B,
/// 2026-10-11). Written as a sidecar next to the installed content so the
/// directory-scanning listings can report exactly which commit the content
/// came from — and whether the ref it tracks is mutable. No persistent
/// addon database exists (listings scan `~/.shannon/{skills,agents}/`), so
/// the sidecar IS the metadata store.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct SourcePin {
    /// Repo the content was cloned from (`owner/repo` shorthand or URL).
    pub repo: String,
    /// Ref the install requested (branch/tag name or full commit SHA).
    #[serde(rename = "ref")]
    pub ref_: String,
    /// Commit the clone actually landed on, when resolvable. Absent beats
    /// fabricated — a failed `rev-parse` degrades the pin, never invents one.
    pub commit: Option<String>,
    /// True when the requested ref is mutable — a refresh can silently move
    /// content (supply-chain surface the UI renders as a floating-ref badge).
    pub ref_floating: bool,
}

/// Sidecar filename inside the installed plugin dir. Dot-prefixed like the
/// other bookkeeping files; never a plugin slug.
pub(crate) const SOURCE_PIN_SIDECAR: &str = ".shannon-source.json";

/// A ref pins content only if it IS a full commit hash — anything else
/// (branch, tag, `HEAD`) is a mutable pointer upstream. Doubles as the
/// validator for `rev-parse` output (SHA-1: 40 hex, SHA-256: 64 hex).
pub(crate) fn is_floating_ref(ref_: &str) -> bool {
    let is_full_hash = ref_.len() == 40 || ref_.len() == 64;
    let all_hex = ref_.bytes().all(|b| b.is_ascii_hexdigit());
    !(is_full_hash && all_hex)
}

/// Resolve the commit the working tree at `dir` is on (`git rev-parse HEAD`
/// — works on the shallow clones the installers create). `None` on any
/// failure: callers record the pin WITHOUT a commit rather than guessing.
pub(crate) fn resolve_head_commit(dir: &std::path::Path) -> Option<String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .arg("rev-parse")
        .arg("HEAD")
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let sha = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if is_floating_ref(&sha) {
        return None;
    }
    Some(sha)
}

/// Persist the pin next to the installed content. Best-effort by design: a
/// failed sidecar write degrades to "no pin reported" and never fails the
/// install it accompanies.
pub(crate) fn write_source_pin(dir: &std::path::Path, pin: &SourcePin) {
    let path = dir.join(SOURCE_PIN_SIDECAR);
    match serde_json::to_vec_pretty(pin) {
        Ok(bytes) => {
            if let Err(e) = std::fs::write(&path, bytes) {
                tracing::warn!(error = %e, path = %path.display(), "source pin sidecar write failed");
            }
        }
        Err(e) => tracing::warn!(error = %e, "source pin serialize failed"),
    }
}

/// Read the pin back. `None` when absent or corrupt — listings report an
/// unpinned addon, which is exactly what the disk shows.
pub(crate) fn read_source_pin(dir: &std::path::Path) -> Option<SourcePin> {
    let bytes = std::fs::read(dir.join(SOURCE_PIN_SIDECAR)).ok()?;
    match serde_json::from_slice(&bytes) {
        Ok(pin) => Some(pin),
        Err(e) => {
            tracing::warn!(error = %e, "source pin sidecar unreadable");
            None
        }
    }
}

/// Shared fixtures for the repo-installer gate tests (skill + agent): local
/// git repos addressed via `file://` — no network, no GitHub.
#[cfg(test)]
pub(crate) mod test_support {
    use std::path::Path;

    pub(crate) fn run_git(dir: &Path, args: &[&str]) {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .expect("spawn git");
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// Build a local git repo containing `files` under `dir` and return a
    /// `file://` URL the installer's `repo_url_override` can clone from.
    pub(crate) fn local_repo_fixture(dir: &Path, files: &[(&str, &str)]) -> String {
        for (name, body) in files {
            let path = dir.join(name);
            std::fs::create_dir_all(path.parent().expect("has parent")).unwrap();
            std::fs::write(path, body).unwrap();
        }
        run_git(dir, &["init", "-q"]);
        run_git(dir, &["symbolic-ref", "HEAD", "refs/heads/main"]);
        run_git(dir, &["add", "."]);
        run_git(
            dir,
            &[
                "-c",
                "user.email=fixture@example.com",
                "-c",
                "user.name=fixture",
                "commit",
                "-q",
                "-m",
                "fixture",
            ],
        );
        format!("file://{}", dir.display())
    }
}

/// Sanitize a catalog-provided plugin name before it is joined onto an
/// install root (B0 P0-6).
///
/// `Path::join` escapes the base for absolute paths and `..` components, and
/// the skill/agent catalogs come from upstream HTTP — so a polluted entry
/// could otherwise write SKILL.md anywhere on disk. Unsafe shapes are
/// rejected loudly; everything else is slugified (lowercase, `[a-z0-9._-]`,
/// other chars collapsed to `-`) in line with the frontend whitelist
/// `^[a-z0-9][a-z0-9._-]*$` and the candidate-approval `slugify`.
pub(crate) fn safe_plugin_name(name: &str) -> Result<String, InstallError> {
    let trimmed = name.trim();
    if trimmed.is_empty()
        || trimmed.starts_with('~')
        || trimmed.starts_with('/')
        || trimmed.starts_with('\\')
        || trimmed.contains('/')
        || trimmed.contains('\\')
        || trimmed.contains("..")
    {
        return Err(InstallError::Format(format!(
            "refusing unsafe plugin name: {name:?}"
        )));
    }
    let mut slug = String::with_capacity(trimmed.len());
    let mut prev_sep = true; // suppress leading separators
    for ch in trimmed.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            prev_sep = false;
        } else if matches!(ch, '.' | '-' | '_') && !prev_sep {
            slug.push(ch);
            prev_sep = true;
        } else if !prev_sep {
            slug.push('-');
            prev_sep = true;
        }
    }
    while slug.ends_with('.') || slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() {
        return Err(InstallError::Format(format!(
            "plugin name has no usable characters: {name:?}"
        )));
    }
    Ok(slug)
}

/// Per-kind install adapter. Implementations:
/// - `McpRegistryInstaller` (P2) — dispatches to one of the four MCP installers
/// - `OAuthRemoteMcpInstaller` (P2) — vendor-hosted remote MCP
/// - `McpbInstaller` (P2) — `.mcpb` ZIP bundle
/// - `StdioMcpInstaller` (P2) — Tier 3 escape hatch
/// - `MarketplacePluginInstaller` (P3) — `.claude-plugin/marketplace.json`
/// - `NativeRustInstaller` (P5) — Obsidian vault + Email IMAP
#[async_trait]
pub trait AddonInstaller: Send + Sync {
    /// What this installer handles.
    fn kind(&self) -> super::types::AddonKind;

    /// Whether this installer can handle a specific entry.
    ///
    /// Used when multiple installers exist for the same kind (e.g. four MCP
    /// installers all return `AddonKind::Mcp`). The hub dispatcher picks the
    /// first one whose `supports()` returns true.
    fn supports(&self, entry: &CatalogEntry) -> bool;

    /// Install the entry into the target location.
    async fn install(
        &self,
        entry: &CatalogEntry,
        target: &InstallTarget,
        progress: &ProgressSink,
    ) -> Result<InstalledAddon, InstallError>;

    /// Remove a previously-installed addon.
    async fn uninstall(&self, addon_id: &str) -> Result<(), InstallError>;

    /// Refresh an installed addon from upstream.
    async fn update(&self, addon_id: &str) -> Result<InstalledAddon, InstallError>;

    /// How scary should the confirm prompt be?
    fn requires_confirmation(&self, entry: &CatalogEntry) -> ConfirmationLevel;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extensions::types::{AddonKind, CatalogSource, TrustLevel};

    #[test]
    fn safe_plugin_name_rejects_unsafe_shapes() {
        for name in [
            "../pwned",
            "/etc/passwd",
            "C:\\Windows",
            "~/pwned",
            "a/b",
            "a\\b",
            "..",
            ".",
            "",
            "   ",
        ] {
            assert!(safe_plugin_name(name).is_err(), "must reject {name:?}");
        }
    }

    #[test]
    fn floating_ref_classification() {
        // Full SHA-1 / SHA-256 pin; everything else is a mutable pointer.
        assert!(!is_floating_ref(&"a".repeat(40)));
        assert!(!is_floating_ref(&"b".repeat(64)));
        for mutable in [
            "main".to_string(),
            "v1.2.3".into(),
            "HEAD".into(),
            "heads/main".into(),
            "g".repeat(40),
            "x".repeat(39),
            "x".repeat(41),
        ] {
            assert!(is_floating_ref(&mutable), "{mutable:?} must be floating");
        }
    }

    #[test]
    fn head_commit_resolves_and_degrades_honestly() {
        let tmp = tempfile::tempdir().expect("tmp");
        let url = test_support::local_repo_fixture(
            &tmp.path().join("repo"),
            &[("SKILL.md", "# fixture")],
        );
        let clone = tmp.path().join("clone");
        test_support::run_git(
            tmp.path(),
            &["clone", "-q", "--depth", "1", &url, clone.to_str().unwrap()],
        );
        let sha = resolve_head_commit(&clone).expect("shallow clone resolves HEAD");
        assert_eq!(sha.len(), 40);
        // Not a repo → None, never a fabricated value.
        let empty = tmp.path().join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        assert_eq!(resolve_head_commit(&empty), None);
    }

    #[test]
    fn source_pin_roundtrip_and_corruption() {
        let tmp = tempfile::tempdir().expect("tmp");
        let dir = tmp.path().join("plugin");
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(read_source_pin(&dir), None, "absent sidecar reads None");
        let pin = SourcePin {
            repo: "o/r".into(),
            ref_: "main".into(),
            commit: Some("a".repeat(40)),
            ref_floating: true,
        };
        write_source_pin(&dir, &pin);
        assert_eq!(read_source_pin(&dir), Some(pin));
        // Corrupt sidecar → None (honest degradation, not a guessed pin).
        std::fs::write(dir.join(SOURCE_PIN_SIDECAR), b"{not json").unwrap();
        assert_eq!(read_source_pin(&dir), None);
    }

    #[test]
    fn safe_plugin_name_slugs_to_frontend_whitelist() {
        // The slug must satisfy the UI-side ^[a-z0-9][a-z0-9._-]*$ whitelist.
        assert_eq!(safe_plugin_name("My Skill v2!").unwrap(), "my-skill-v2");
        assert_eq!(safe_plugin_name("code.review").unwrap(), "code.review");
        assert_eq!(safe_plugin_name("  spaced  out  ").unwrap(), "spaced-out");
        assert_eq!(safe_plugin_name(".hidden").unwrap(), "hidden");
        let re = regex::Regex::new(r"^[a-z0-9][a-z0-9._-]*$").unwrap();
        for slug in ["my-skill-v2", "code.review", "spaced-out", "hidden"] {
            assert!(re.is_match(slug), "{slug} must match the whitelist");
        }
    }

    #[test]
    fn install_error_display_is_human_readable() {
        let cases = [
            (
                InstallError::Network("timeout".into()),
                "network error: timeout",
            ),
            (
                InstallError::Io("permission denied".into()),
                "io error: permission denied",
            ),
            (
                InstallError::Format("bad zip".into()),
                "format error: bad zip",
            ),
            (InstallError::Auth("revoked".into()), "auth error: revoked"),
            (InstallError::Cancelled, "cancelled by user"),
            (
                InstallError::Unsupported("no transport".into()),
                "unsupported: no transport",
            ),
            (InstallError::Other("boom".into()), "boom"),
        ];
        for (err, expected) in cases {
            assert_eq!(err.to_string(), expected);
        }
    }

    #[test]
    fn install_error_from_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "missing");
        let install_err: InstallError = io_err.into();
        assert!(matches!(install_err, InstallError::Io(_)));
    }

    #[test]
    fn install_error_from_serde_json() {
        let json_err = serde_json::from_str::<serde_json::Value>("bad").unwrap_err();
        let install_err: InstallError = json_err.into();
        assert!(matches!(install_err, InstallError::Format(_)));
    }

    #[test]
    fn install_error_from_anyhow() {
        let any_err = anyhow::anyhow!("multi\nline\nfailure");
        let install_err: InstallError = any_err.into();
        assert!(matches!(install_err, InstallError::Other(_)));
    }

    /// A no-op installer that the tests can use to verify trait shape.
    struct DummyInstaller;

    #[async_trait]
    impl AddonInstaller for DummyInstaller {
        fn kind(&self) -> AddonKind {
            AddonKind::Skill
        }
        fn supports(&self, _entry: &CatalogEntry) -> bool {
            true
        }
        async fn install(
            &self,
            entry: &CatalogEntry,
            _target: &InstallTarget,
            _progress: &ProgressSink,
        ) -> Result<InstalledAddon, InstallError> {
            Ok(InstalledAddon {
                id: entry.id.clone(),
                kind: entry.kind,
                name: entry.name.clone(),
                install_path: Some("/tmp/dummy".to_string()),
                installed_at: Some(chrono::Utc::now()),
                version: entry.version.clone(),
                enabled: true,
                source_commit: None,
                ref_floating: false,
            })
        }
        async fn uninstall(&self, _addon_id: &str) -> Result<(), InstallError> {
            Ok(())
        }
        async fn update(&self, addon_id: &str) -> Result<InstalledAddon, InstallError> {
            Err(InstallError::Unsupported(format!(
                "no update for {addon_id}"
            )))
        }
        fn requires_confirmation(&self, _entry: &CatalogEntry) -> ConfirmationLevel {
            ConfirmationLevel::Review
        }
    }

    #[tokio::test]
    async fn dummy_installer_satisfies_trait() {
        let installer = DummyInstaller;
        let entry = CatalogEntry {
            id: "test:1".to_string(),
            kind: AddonKind::Skill,
            name: "Test Skill".to_string(),
            description: "test".to_string(),
            author: None,
            version: Some("0.1".to_string()),
            homepage_url: None,
            license: None,
            stars: None,
            last_updated: None,
            source: CatalogSource::Native,
            trust: TrustLevel::Unknown,
            metadata: Default::default(),
            tags: vec![],
        };
        assert!(installer.supports(&entry));
        let installed = installer
            .install(
                &entry,
                &InstallTarget::ShannonSkillsDir {
                    plugin: "test".into(),
                },
                &ProgressSink::null(),
            )
            .await
            .expect("install");
        assert_eq!(installed.id, "test:1");
        assert!(installed.enabled);
        installer.uninstall("test:1").await.expect("uninstall");
        assert!(installer.update("test:1").await.is_err());
    }
}
